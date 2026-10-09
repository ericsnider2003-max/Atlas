//! Owned inference transport. Native provider code runs only in this child.
use super::{In, Session, Where};
use serde::{Deserialize, Serialize};
use std::{io::{Read, Write}, net::{TcpListener, TcpStream, Shutdown}, path::{Path, PathBuf}, process::{Child, Command, Stdio}, sync::{Mutex, OnceLock}, time::{Duration, Instant}};

const FRAME: usize = 8 * 1024 * 1024;
pub(super) const ELEMENTS: usize = 1_048_576;
const OPEN_TIME: Duration = Duration::from_secs(90);
const RUN_TIME: Duration = Duration::from_secs(2);
const HELPERS: usize = 3;
static SIGNATURES: OnceLock<Mutex<std::collections::BTreeMap<String, String>>> = OnceLock::new();
static ACTIVE: OnceLock<Mutex<std::collections::BTreeSet<PathBuf>>> = OnceLock::new();
static LIVE: Mutex<usize> = Mutex::new(0);
static FAILURES: OnceLock<Mutex<std::collections::BTreeMap<String, String>>> = OnceLock::new();
struct Permit;
impl Permit {
    fn take() -> Result<Self, String> {
        let mut live = LIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if *live >= HELPERS { return Err("the three isolated inference slots are occupied; using the processor".into()); }
        *live += 1; Ok(Self)
    }
}
impl Drop for Permit { fn drop(&mut self) { let mut live=LIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner); *live=live.saturating_sub(1); } }

struct Attempt { path: PathBuf, retain: bool }
impl Attempt {
    fn begin(root: &Path, signature: &str, token: &[u8]) -> Result<Self, String> {
        let parent=root.join("data/cache/npu");std::fs::create_dir_all(&parent).map_err(|e|e.to_string())?;
        let nonce:String=token.iter().map(|byte|format!("{byte:02x}")).collect();
        let path=parent.join(format!("isolated-attempt-{nonce}.json"));
        let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e|e.to_string())?;
        file.write_all(signature.as_bytes()).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
        ACTIVE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(path.clone());
        Ok(Self {path,retain:false})
    }
}
impl Drop for Attempt {fn drop(&mut self){
    ACTIVE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&self.path);
    if !self.retain {crate::heard!(std::fs::remove_file(&self.path));}
}}
fn unfinished(root:&Path,key:&str)->Option<String> {
    let parent=root.join("data/cache/npu");
    let entries=match std::fs::read_dir(parent){Ok(entries)=>entries,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return None,Err(_)=>return Some("isolated NPU attempt records are unavailable".into())};
    let active=ACTIVE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for (count,entry) in entries.enumerate() {
        if count>=256{return Some("isolated NPU recovery inspection exceeds its entry budget".into());}
        let entry=match entry{Ok(entry)=>entry,Err(_)=>return Some("isolated NPU attempt records cannot be inspected".into())};
        let name=entry.file_name();let name=name.to_string_lossy();
        if !name.starts_with("isolated-attempt-") || !name.ends_with(".json") || active.contains(&entry.path()){continue;}
        let mut file=match std::fs::File::open(entry.path()){Ok(file)=>file,Err(_)=>return Some("isolated NPU attempt evidence cannot be read".into())};
        let mut bytes=Vec::new();if Read::by_ref(&mut file).take(1025).read_to_end(&mut bytes).is_err() || bytes.len()>1024{return Some("isolated NPU attempt evidence is invalid".into());}
        if bytes==key.as_bytes(){return Some("an isolated NPU attempt remains unconfirmed; another process may still own it. Using the processor".into());}
    }None
}

#[derive(Serialize, Deserialize)]
enum Request { Open { root: PathBuf, model: PathBuf, shapes: Vec<(String, Vec<i64>)>, quiet: bool, signature: String }, Run { sequence: u64, inputs: Vec<In> } }
#[derive(Serialize, Deserialize)]
enum Response { Ready, Rejected(String), Output { sequence: u64, values: Vec<Vec<f32>> } }

fn dimensions(shape:&[i64])->Result<usize,String> {
    if shape.is_empty() || shape.len()>8 { return Err("inference rank is outside its budget".into()); }
    shape.iter().try_fold(1usize, |product, dimension| {
        let value=usize::try_from(*dimension).ok().filter(|value|*value>0).ok_or("inference dimensions must be positive")?;
        product.checked_mul(value).filter(|value|*value<=ELEMENTS).ok_or_else(||"inference shape exceeds its element budget".into())
    })
}
fn names_shapes(shapes:&[(String,Vec<i64>)])->Result<(),String> {
    if shapes.is_empty() || shapes.len()>16 { return Err("inference input count is outside its budget".into()); }
    let mut names=std::collections::BTreeSet::new();
    for (name,shape) in shapes { if name.is_empty() || name.len()>256 || name.contains('\0') || !names.insert(name) {return Err("inference names are invalid".into());} dimensions(shape)?; }
    Ok(())
}
pub(super) fn validate_inputs(inputs:&[In])->Result<(),String> {
    let shapes:Vec<_>=inputs.iter().map(|input|match input {In::F32(n,s,_)|In::I64(n,s,_) =>(n.clone(),s.clone())}).collect();
    names_shapes(&shapes)?;
    let mut bytes=0usize;
    for input in inputs {let (shape,count,width)=match input {
        In::F32(_,shape,values)=> {if values.iter().any(|value|!value.is_finite()){return Err("inference values must be finite".into());} (shape,values.len(),4)},
        In::I64(_,shape,values)=>(shape,values.len(),8),
    }; if dimensions(shape)?!=count{return Err("inference values do not match their shape".into());}
        bytes=bytes.checked_add(count.checked_mul(width).ok_or("inference byte count overflow")?).ok_or("inference byte count overflow")?;
        if bytes>8*1024*1024{return Err("inference input exceeds its byte budget".into());}
    } Ok(())
}
fn output_valid(values:&[Vec<f32>])->bool { !values.is_empty() && values.len()<=16 && values.iter().map(Vec::len).sum::<usize>()<=ELEMENTS && values.iter().flatten().all(|v|v.is_finite()) }
fn transfer(stream:&mut TcpStream, bytes:&mut [u8], deadline:Instant, writing:bool)->Result<(),String> {
    let mut position=0;
    while position<bytes.len() {
        let left=deadline.checked_duration_since(Instant::now()).filter(|d|!d.is_zero()).ok_or("NPU helper timed out")?;
        if writing {stream.set_write_timeout(Some(left))} else {stream.set_read_timeout(Some(left))}.map_err(|e|e.to_string())?;
        let result=if writing {stream.write(&bytes[position..])}else{stream.read(&mut bytes[position..])};
        match result {Ok(0)=>return Err("NPU helper closed its transport unexpectedly".into()),Ok(count)=>position+=count,
            Err(e) if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)=>return Err("NPU helper timed out".into()),Err(e)=>return Err(format!("NPU helper transport failed: {e}")),}
    } Ok(())
}
fn send<T:Serialize>(stream:&mut TcpStream,value:&T,deadline:Instant)->Result<(),String> {
    let mut bytes=serde_json::to_vec(value).map_err(|e|e.to_string())?;
    if bytes.len()>FRAME{return Err("NPU helper frame exceeds its byte budget".into());}
    transfer(stream,&mut (bytes.len() as u32).to_le_bytes(),deadline,true)?;
    transfer(stream,&mut bytes,deadline,true)
}
fn receive<T:serde::de::DeserializeOwned>(stream:&mut TcpStream,deadline:Instant)->Result<T,String> {
    let mut header=[0;4];transfer(stream,&mut header,deadline,false)?;
    let size=u32::from_le_bytes(header) as usize;
    if size==0 || size>FRAME{return Err("NPU helper sent an invalid frame length".into());}
    let mut bytes=vec![0;size];transfer(stream,&mut bytes,deadline,false)?;
    let value=serde_json::from_slice(&bytes).map_err(|_|String::from("NPU helper sent a malformed frame"))?;
    if Instant::now()>deadline{return Err("NPU helper timed out while decoding its reply".into());}Ok(value)
}

// Content signatures distinguish a newly installed engine from a failed one.
fn signature(root:&Path)->Result<String,String> {
    use sha2::{Digest,Sha256};
    let paths=[root.join(super::PLUGIN),super::runtime_path(root).ok_or("processor runtime is missing")?];
    let mut stamp=format!("{}|{}",root.display(),super::engine_version());
    for path in &paths {let metadata=std::fs::metadata(path).map_err(|e|e.to_string())?;stamp.push_str(&format!("|{}|{:?}",metadata.len(),metadata.modified().ok()));}
    let cache=SIGNATURES.get_or_init(Default::default);
    if let Some(key)=cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&stamp){return Ok(key.clone());}
    let mut digest=Sha256::new();digest.update(super::engine_version().as_bytes());
    for path in paths {
        let mut file=std::fs::File::open(path).map_err(|e|e.to_string())?;
        if file.metadata().map_err(|e|e.to_string())?.len()>512*1024*1024{return Err("engine library exceeds its signature budget".into());}
        let mut buffer=[0;64*1024];let mut total=0usize;
        loop {let n=file.read(&mut buffer).map_err(|e|e.to_string())?;if n==0{break;}total+=n;if total>512*1024*1024{return Err("engine library grew beyond its signature budget".into());}digest.update(&buffer[..n]);}
    } let key=format!("isolated|{:x}",digest.finalize());
    let mut cached=cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);if cached.len()>=16{cached.clear();}cached.insert(stamp,key.clone());Ok(key)
}
fn journal(root:&Path)->PathBuf {root.join("data/cache/npu/isolated-failures.json")}
fn failure_map(root:&Path)->Result<std::collections::BTreeMap<String,String>,String> {
    let file=match std::fs::File::open(journal(root)){Ok(file)=>file,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(Default::default()),Err(_)=>return Err("isolated NPU recovery record is unavailable; using the processor".into())};
    let mut bytes=Vec::new();file.take(64*1024+1).read_to_end(&mut bytes).map_err(|_|"isolated NPU recovery record cannot be read")?;
    if bytes.len()>64*1024{return Err("isolated NPU recovery record exceeds its budget".into());}
    serde_json::from_slice(&bytes).map_err(|_|"isolated NPU recovery record is unreadable; using the processor".into())
}
pub(super) fn blocked(root:&Path)->Option<String> {
    let key=match signature(root){Ok(key)=>key,Err(e)=>return Some(e)};
    if let Some(reason)=FAILURES.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key){return Some(reason.clone());}
    match failure_map(root){Ok(map)=>map.get(&key).cloned().or_else(||unfinished(root,&key)),Err(e)=>Some(e)}
}
pub(super) fn allowed(root:&Path)->bool {blocked(root).is_none()}
fn quarantine(root:&Path,key:&str,why:&str) {
    FAILURES.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key.into(),why.into());
    let saved=(||->crate::error::Result<()> {let mut map=failure_map(root).map_err(crate::error::AtlasError::Platform)?;map.insert(key.into(),why.into());
        if map.len()>128{return Err(crate::error::AtlasError::Platform("isolated NPU failure history is full".into()));}
        let path=journal(root);std::fs::create_dir_all(path.parent().ok_or_else(||crate::error::AtlasError::Platform("missing NPU cache parent".into()))?)?;
        Ok(crate::store::write_json(&path,&map)?)})();
    crate::kept!(saved);
}

fn model_path(root:&Path,model:&Path)->Result<(),String> {
    if !root.is_absolute() || !model.is_absolute() || root.as_os_str().len()>4096 || model.as_os_str().len()>4096 {return Err("model paths must be bounded absolute local paths".into());}
    let root=std::fs::canonicalize(root).map_err(|_|"model root cannot be resolved")?;
    let model=std::fs::canonicalize(model).map_err(|_|"model file cannot be resolved")?;
    let metadata=std::fs::metadata(&model).map_err(|_|"model cannot be inspected")?;
    if !model.starts_with(root) || !metadata.is_file() || metadata.len()>512*1024*1024 || model.extension().is_none_or(|extension|extension!="onnx") {return Err("helper model is outside its bounded model root".into());}
    Ok(())
}

pub(super) struct Client { child:Child, scope:crate::childjob::Scope, stream:TcpStream, root:PathBuf, signature:String, sequence:u64, failed:bool, _permit:Option<Permit>, attempt:Attempt }
impl Client {
    pub(super) fn open(root:&Path,model:&Path,shapes:&[(String,Vec<i64>)],quiet:bool)->Result<Self,String> {
        names_shapes(shapes)?;model_path(root,model)?;
        let exe=std::env::current_exe().map_err(|e|e.to_string())?;
        let mut command=Command::new(exe);command.arg("--npu-worker");
        Self::open_command(command,root,Request::Open {root:root.into(),model:model.into(),shapes:shapes.to_vec(),quiet,signature:signature(root)?},OPEN_TIME)
    }
    fn open_command(mut command:Command,root:&Path,request:Request,budget:Duration)->Result<Self,String> {
        let permit=Permit::take()?;let key=signature(root)?;
        let listener=TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).map_err(|e|e.to_string())?;
        listener.set_nonblocking(true).map_err(|e|e.to_string())?;
        let port=listener.local_addr().map_err(|e|e.to_string())?.port();
        let token=crate::vault::random_bytes(32);if token.len()!=32{return Err("NPU helper entropy is unavailable".into());}
        let mut attempt=Attempt::begin(root,&key,&token)?;
        command.env("ATLAS_NPU_WORKER_PORT",port.to_string()).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        let (mut child,scope)=crate::childjob::Scope::spawn(&mut command).map_err(|e|e.to_string())?;
        let startup=(||->Result<TcpStream,String> {
            child.stdin.take().ok_or("helper input is unavailable")?.write_all(&token).map_err(|e|e.to_string())?;
            let deadline=Instant::now()+budget;
            loop {
                if let Some(status)=child.try_wait().map_err(|e|e.to_string())?{return Err(format!("NPU helper exited unexpectedly ({status})"));}
                if Instant::now()>=deadline{return Err("NPU helper timed out while opening".into());}
                match listener.accept(){Ok((mut stream,peer))=>{
                    if !peer.ip().is_loopback(){continue;}
                    // Windows accepts inherit the listener's nonblocking mode.
                    // Each blocking operation below has an absolute budget.
                    stream.set_nonblocking(false).map_err(|e|e.to_string())?;
                    let mut proof=[0u8;32];transfer(&mut stream,&mut proof,deadline.min(Instant::now()+Duration::from_secs(1)),false).map_err(|why|format!("NPU authentication: {why}"))?;
                    if proof.as_slice()!=token{return Err("NPU helper authentication failed".into());}
                    stream.set_nodelay(true).map_err(|e|e.to_string())?;
                    send(&mut stream,&request,deadline).map_err(|why|format!("NPU opening request: {why}"))?;
                    return match receive::<Response>(&mut stream,deadline).map_err(|why|format!("NPU opening response: {why}"))?{Response::Ready=>Ok(stream),Response::Rejected(why) if why.len()<=2048=>Err(format!("model rejected: {why}")),_=>Err("NPU helper sent an unexpected opening response".into())};
                },Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(5)),Err(e)=>return Err(e.to_string())}
            }
        })();
        match startup {Ok(stream)=>Ok(Self{child,scope,stream,root:root.into(),signature:key,sequence:0,failed:false,_permit:Some(permit),attempt}),Err(why)=>{
            drop(scope);crate::heard!(child.kill());crate::heard!(child.wait());
            if !why.starts_with("model rejected:"){attempt.retain=true;quarantine(root,&key,&why);}Err(why)
        }}
    }
    pub(super) fn run(&mut self,inputs:Vec<In>)->Result<Vec<Vec<f32>>,String> {
        validate_inputs(&inputs)?;if self.failed{return Err("the isolated NPU session has stopped".into());}
        self.sequence=self.sequence.checked_add(1).ok_or("NPU sequence exhausted")?;
        let deadline=Instant::now()+RUN_TIME;
        let result=(||{send(&mut self.stream,&Request::Run {sequence:self.sequence,inputs},deadline)?;
            match receive::<Response>(&mut self.stream,deadline)? {
                Response::Output{sequence,values} if sequence==self.sequence && output_valid(&values)=>Ok(values),
                Response::Rejected(why) if why.len()<=2048=>Err(format!("NPU inference rejected: {why}")),_=>Err("NPU helper returned invalid or mismatched outputs".into())
            }})();
        if let Err(why)=&result {self.failed=true;self.attempt.retain=true;quarantine(&self.root,&self.signature,why);self.scope.stop();crate::heard!(self.child.kill());crate::heard!(self.child.wait());self._permit.take();crate::heard!(self.stream.shutdown(Shutdown::Both));}
        result
    }
}
impl Drop for Client {fn drop(&mut self){self.scope.stop();if !self.failed {crate::heard!(self.child.kill());crate::heard!(self.child.wait());}crate::heard!(self.stream.shutdown(Shutdown::Both));}}

fn connection()->Result<TcpStream,String> {
    let port=std::env::var("ATLAS_NPU_WORKER_PORT").map_err(|_|"missing helper port")?.parse::<u16>().map_err(|_|"invalid helper port")?;
    let mut token=[0;32];std::io::stdin().read_exact(&mut token).map_err(|_|"missing helper authentication")?;
    let mut stream=TcpStream::connect_timeout(&std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST,port)),Duration::from_secs(2)).map_err(|e|e.to_string())?;
    #[cfg(test)]
    if std::env::var("ATLAS_NPU_SYNTHETIC_FAULT").ok().as_deref()==Some("auth-delay") {
        // Force the parent to accept before any authentication bytes exist.
        std::thread::sleep(Duration::from_millis(100));
    }
    transfer(&mut stream,&mut token,Instant::now()+Duration::from_secs(2),true)?;Ok(stream)
}
fn idle_request(stream:&mut TcpStream)->Result<Request,String> {
    // Idle cached sessions own no queued work. Parent lifetime scope and Drop
    // kill this child; each actual frame remains strictly bounded after header.
    stream.set_read_timeout(None).map_err(|e|e.to_string())?;
    let mut header=[0;4];stream.read_exact(&mut header).map_err(|_|"parent closed the inference transport")?;
    let size=u32::from_le_bytes(header) as usize;if size==0 || size>FRAME {return Err("invalid inference frame size".into());}
    let mut bytes=vec![0;size];transfer(stream,&mut bytes,Instant::now()+Duration::from_secs(2),false)?;
    serde_json::from_slice(&bytes).map_err(|_|"invalid inference frame".into())
}

pub(super) fn serve()->i32 {
    let result=(||->Result<(),String>{
        let mut stream=connection()?;
        let Request::Open{root,model,shapes,quiet,signature:expected_signature}=receive(&mut stream,Instant::now()+Duration::from_secs(5))? else{return Err("first inference frame must open a model".into());};
        names_shapes(&shapes)?;
        model_path(&root,&model)?;
        if signature(&root)? != expected_signature {
            send(&mut stream,&Response::Rejected("engine changed before the isolated trial; use a fresh trial".into()),Instant::now()+Duration::from_secs(2))?;return Ok(());
        }
        super::WORKER_ROOT.set(root.clone()).map_err(|_|"helper model root was already set")?;
        let session=match Session::open_with(&root,&model,&shapes,Where::Npu,quiet){Ok(session) if session.on()==Where::Npu=>session,other=>{
            let mut reason=match other {Err(why)=>why,Ok(_)=>"the provider has no usable NPU".into()};
            while reason.len()>2048 {reason.pop();}send(&mut stream,&Response::Rejected(reason),Instant::now()+Duration::from_secs(2))?;return Ok(());
        }};
        send(&mut stream,&Response::Ready,Instant::now()+Duration::from_secs(2))?;
        let mut last=0;
        loop {let request=idle_request(&mut stream)?;
            let Request::Run{sequence,inputs}=request else{return Err("model is already opened".into());};
            if sequence!=last+1{return Err("inference sequence changed".into());}last=sequence;validate_inputs(&inputs)?;
            let response=match session.run(inputs){Ok(values) if output_valid(&values)=>Response::Output{sequence,values},Ok(_)=>return Err("native output budget exceeded".into()),Err(mut why)=>{while why.len()>2048 {why.pop();}Response::Rejected(why)}};
            send(&mut stream,&response,Instant::now()+Duration::from_secs(2))?;
        }
    })();if result.is_ok(){0}else{2}
}


#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->PathBuf {
        static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
        let root=std::env::temp_dir().join(format!("atlas-isolated-npu-{}-{}",std::process::id(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(root.join(super::super::DIR)).unwrap();
        let (runtime,_)=crate::kokoro::runtime_files().unwrap();
        std::fs::create_dir_all(root.join(crate::kokoro::RUNTIME_DIR)).unwrap();
        std::fs::write(root.join(crate::kokoro::RUNTIME_DIR).join(runtime),root.to_string_lossy().as_bytes()).unwrap();
        std::fs::write(root.join(super::super::PLUGIN),b"synthetic library signature only; never loaded").unwrap();root
    }
    fn request(root:&Path)->Request {Request::Open{root:root.into(),model:root.join("synthetic.onnx"),shapes:vec![("x".into(),vec![1])],quiet:true,signature:signature(root).unwrap()}}
    fn client(root:&Path,mode:&str,budget:Duration)->Result<Client,String> {
        let mut command=Command::new(std::env::current_exe().unwrap());
        command.args(["npu::worker::tests::synthetic_child","--exact","--ignored","--nocapture","--test-threads=1"]).env("ATLAS_NPU_SYNTHETIC_FAULT",mode);
        Client::open_command(command,root,request(root),budget)
    }
    #[test]
    #[ignore = "owned synthetic subprocess only; invoked by isolation tests"]
    fn synthetic_child() {
        let mode=std::env::var("ATLAS_NPU_SYNTHETIC_FAULT").unwrap();
        let mut stream=connection().unwrap();let _:Request=receive(&mut stream,Instant::now()+Duration::from_secs(5)).unwrap();
        match mode.as_str(){"load-exit"=>std::process::exit(37),"open-timeout"=>{std::thread::sleep(Duration::from_secs(10));return;},_=>{}}
        send(&mut stream,&Response::Ready,Instant::now()+Duration::from_secs(2)).unwrap();
        let Request::Run{sequence,..}=idle_request(&mut stream).unwrap() else{panic!("expected run")};
        match mode.as_str(){
            "run-exit"=>std::process::exit(38),
            "run-timeout"=>std::thread::sleep(Duration::from_secs(10)),
            "flood"=>stream.write_all(&((FRAME as u32)+1).to_le_bytes()).unwrap(),
            "malformed"=>{stream.write_all(&1u32.to_le_bytes()).unwrap();stream.write_all(b"!").unwrap();},
            "wrong-sequence"=>send(&mut stream,&Response::Output{sequence:sequence+1,values:vec![vec![1.0]]},Instant::now()+Duration::from_secs(2)).unwrap(),
            _=>send(&mut stream,&Response::Output{sequence,values:vec![vec![1.0]]},Instant::now()+Duration::from_secs(2)).unwrap(),
        }
    }
    #[test] fn a_first_load_exit_and_timeout_are_contained_and_have_distinct_evidence() {
        for mode in ["load-exit","open-timeout"] {
            let root=fixture();let error=match client(&root,mode,if mode=="open-timeout"{Duration::from_millis(500)}else{Duration::from_secs(5)}){Ok(_)=>panic!("fault opened"),Err(e)=>e};
            assert!(if mode=="open-timeout"{error.contains("timed out")}else{error.contains("closed")||error.contains("exited")},"{error}");
            assert!(blocked(&root).is_some());
            let next=fixture();assert!(allowed(&next),"a different engine signature stays eligible");
        }
    }
    #[test] fn inference_exit_timeout_flood_and_invalid_frames_stop_and_reap_only_the_helper() {
        for mode in ["run-exit","run-timeout","flood","malformed","wrong-sequence"] {
            let root=fixture();let mut client=client(&root,mode,Duration::from_secs(5)).unwrap();let pid=client.child.id();
            let error=client.run(vec![In::F32("x".into(),vec![1],vec![1.0])]).unwrap_err();
            assert!(!error.is_empty());assert!(client.failed);assert!(client.child.try_wait().unwrap().is_some(),"owned child {pid} still alive");
            assert!(blocked(&root).is_some());assert!(client.run(vec![In::F32("x".into(),vec![1],vec![1.0])]).is_err());
        }
    }
    #[test] fn active_attempts_are_not_previous_crashes_and_dropped_helpers_release_slots() {
        let root=fixture();let client=client(&root,"auth-delay",Duration::from_secs(5)).unwrap();
        assert!(allowed(&root),"a live current-owned marker is not a previous crash");
        let marker=client.attempt.path.clone();drop(client);assert!(!marker.exists());assert!(allowed(&root));
        let marker=Attempt::begin(&root,&signature(&root).unwrap(),&[4;32]).unwrap();
        let path=marker.path.clone();let mut marker=marker;marker.retain=true;drop(marker);
        assert!(path.exists());assert!(blocked(&root).unwrap().contains("unconfirmed"));
    }
    #[test] fn dimensions_values_and_helper_count_are_bounded_before_native_work() {
        assert!(dimensions(&[i64::MAX,i64::MAX]).is_err());
        assert!(validate_inputs(&[In::F32("x".into(),vec![2],vec![1.0])]).is_err());
        assert!(validate_inputs(&[In::F32("x".into(),vec![1],vec![f32::NAN])]).is_err());
        let a=Permit::take().unwrap();let b=Permit::take().unwrap();let c=Permit::take().unwrap();assert!(Permit::take().is_err());drop((a,b,c));assert!(Permit::take().is_ok());
    }
    #[cfg(all(windows, feature = "onnx"))]
    #[test]
    #[ignore = "run alone with explicit CPU runtime fixture; never loads the NPU plugin"]
    fn actual_processor_inference_survives_an_isolated_inference_exit() {
        let root=PathBuf::from(std::env::var_os("ATLAS_ORT_TEST_ROOT").expect("explicit runtime fixture"));
        let synthetic=fixture();let remote=client(&synthetic,"run-exit",Duration::from_secs(5)).unwrap();
        let model=root.join(crate::meaningnative::MODEL);let names=Session::input_names(&root,&model).unwrap();let shapes=names.iter().map(|n|(n.clone(),vec![1,16])).collect::<Vec<_>>();
        let session=Session{s:None,remote:Some(Mutex::new(remote)),fallback:Some((root.clone(),model,shapes,false)),cpu_fallback:Mutex::new(None),on:std::sync::atomic::AtomicU8::new(1)};
        let inputs=||names.iter().map(|name|In::I64(name.clone(),vec![1,16],if name.contains("attention"){vec![1;16]}else{vec![0;16]})).collect();
        let output=session.run(inputs()).expect("real CPU fallback after helper exits");assert_eq!(session.on(),Where::Cpu);assert!(!output.is_empty());assert!(output.iter().flatten().all(|value|value.is_finite()));
        assert_eq!(output,session.run(inputs()).unwrap());
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        assert!(unsafe{GetModuleHandleW(windows::core::w!("onnxruntime_providers_openvino_plugin.dll"))}.is_err());
    }
}
