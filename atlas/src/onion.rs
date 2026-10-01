//! How one person's Atlas reaches another's: through Tor, with nothing in the
//! middle that anybody runs or pays for.
//!
//! Eric's rules (25 Sep): Tailscale may join *your own* devices; it never
//! joins two people's. No server in the middle, nothing to pay for, and no
//! friend's Atlas holding anyone's messages. So each desktop Atlas is a Tor
//! **onion service**: it has a permanent address made from its own key, and it
//! reaches -- and is reached -- only by connecting *out* into the Tor network,
//! which joins the two ends inside itself. That works from behind any router
//! and any provider, needs no door opened anywhere, and nobody along the way
//! learns who is talking to whom. The same way Briar and Ricochet work.
//!
//! What Atlas does here:
//! * makes the onion address from this Atlas's key (`Identity::derive`, so the
//!   onion key is its own secret, not the signing key reused), and writes it
//!   where `tor` expects it -- nothing to set up;
//! * starts `tor` itself, shipped beside Atlas, and knows when it's ready;
//! * connects to a friend's onion address through it (SOCKS5, written here).
//!
//! What travels through it is still sealed by Atlas (`wire`), so the door
//! checks who sent what exactly as before.
//!
//! Also here: the few facts about *your own* networks the door needs -- which
//! addresses count as this machine, home, or your own private network.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The port an onion address answers on; Tor forwards it to the door.
pub const ONION_PORT: u16 = 80;
/// Opening a connection through Tor can take a while the first time.
pub const CONNECT_SECS: u64 = 60;

// ---- SHA3-256, for the address checksum -------------------------------------

const RC: [u64; 24] = [
    0x0000000000000001, 0x0000000000008082, 0x800000000000808a, 0x8000000080008000, 0x000000000000808b,
    0x0000000080000001, 0x8000000080008081, 0x8000000000008009, 0x000000000000008a, 0x0000000000000088,
    0x0000000080008009, 0x000000008000000a, 0x000000008000808b, 0x800000000000008b, 0x8000000000008089,
    0x8000000000008003, 0x8000000000008002, 0x8000000000000080, 0x000000000000800a, 0x800000008000000a,
    0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
];
const ROT: [u32; 25] = [0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56, 14];

fn keccak_f(a: &mut [u64; 25]) {
    for rc in RC {
        let mut c = [0u64; 5];
        for x in 0..5 {
            c[x] = a[x] ^ a[x + 5] ^ a[x + 10] ^ a[x + 15] ^ a[x + 20];
        }
        for x in 0..5 {
            let d = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                a[x + 5 * y] ^= d;
            }
        }
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                b[y + 5 * ((2 * x + 3 * y) % 5)] = a[x + 5 * y].rotate_left(ROT[x + 5 * y]);
            }
        }
        for x in 0..5 {
            for y in 0..5 {
                a[x + 5 * y] = b[x + 5 * y] ^ (!b[(x + 1) % 5 + 5 * y] & b[(x + 2) % 5 + 5 * y]);
            }
        }
        a[0] ^= rc;
    }
}

/// SHA3-256 (FIPS 202).
pub fn sha3_256(data: &[u8]) -> [u8; 32] {
    const RATE: usize = 136;
    let mut st = [0u64; 25];
    let mut padded = data.to_vec();
    padded.push(0x06);
    while padded.len() % RATE != 0 {
        padded.push(0);
    }
    *padded.last_mut().unwrap_or(&mut 0) |= 0x80;
    for block in padded.chunks(RATE) {
        for (i, lane) in block.chunks(8).enumerate() {
            let mut w = [0u8; 8];
            w.copy_from_slice(lane);
            st[i] ^= u64::from_le_bytes(w);
        }
        keccak_f(&mut st);
    }
    let mut out = [0u8; 32];
    for i in 0..4 {
        out[i * 8..i * 8 + 8].copy_from_slice(&st[i].to_le_bytes());
    }
    out
}

fn base32(data: &[u8]) -> String {
    const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in data {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            out.push(ALPHA[((buf >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(ALPHA[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

// ---- The address and its keys ------------------------------------------------

/// A v3 onion address for this public key (rend-spec-v3 §6):
/// base32(key || checksum[..2] || 3) + ".onion".
fn address_of(public: &[u8; 32]) -> String {
    let mut c = b".onion checksum".to_vec();
    c.extend_from_slice(public);
    c.push(3);
    let sum = sha3_256(&c);
    let mut raw = public.to_vec();
    raw.extend_from_slice(&sum[..2]);
    raw.push(3);
    format!("{}.onion", base32(&raw))
}

/// Does this read as a v3 onion address?
pub fn is_onion(s: &str) -> bool {
    s.len() == 62 && s.ends_with(".onion") && s[..56].bytes().all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
}

/// The onion key for this Atlas: (the expanded secret Tor keeps, the public key).
fn onion_keys(seed: &[u8; 32]) -> ([u8; 64], [u8; 32]) {
    use sha2::{Digest, Sha512};
    let mut h: [u8; 64] = Sha512::digest(seed).into();
    h[0] &= 248;
    h[31] &= 63;
    h[31] |= 64;
    let public = ed25519_dalek::SigningKey::from_bytes(seed).verifying_key().to_bytes();
    (h, public)
}

/// This Atlas's onion address, from its key.
pub fn my_address(me: &crate::peerkey::Identity) -> String {
    address_of(&onion_keys(&me.derive("onion")).1)
}

/// Write the onion service's keys where `tor` reads them. Returns the address.
fn write_service(dir: &Path, me: &crate::peerkey::Identity) -> Result<String, String> {
    let (secret, public) = onion_keys(&me.derive("onion"));
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let mut s = b"== ed25519v1-secret: type0 ==\0\0\0".to_vec();
    s.extend_from_slice(&secret);
    let mut p = b"== ed25519v1-public: type0 ==\0\0\0".to_vec();
    p.extend_from_slice(&public);
    let address = address_of(&public);
    // `hostname` is left for Tor to write from the key: its answer, not ours.
    let _ = std::fs::remove_file(dir.join("hostname"));
    std::fs::write(dir.join("hs_ed25519_secret_key"), s)
        .and_then(|_| std::fs::write(dir.join("hs_ed25519_public_key"), p))
        .map_err(|e| format!("couldn't write the onion keys: {e}"))?;
    Ok(address)
}

// ---- Running tor -------------------------------------------------------------

/// Where `tor` is: the one set in config, else beside Atlas (`tor/tor.exe`,
/// as the installer puts it), else on the system path.
pub fn find_tor(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = configured.map(str::trim).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p)).filter(|p| p.is_file());
    }
    let exe = if cfg!(windows) { "tor.exe" } else { "tor" };
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        for c in [dir.join("tor").join(exe), dir.join(exe)] {
            if c.is_file() {
                return Some(c);
            }
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(exe)).find(|p| p.is_file())
}

/// The configuration Atlas runs `tor` with. `extra`: lines appended as they
/// are (a private test network, or bridges).
///
/// `__OwningControllerProcess` names this Atlas: Tor watches it and exits by
/// itself when it's gone. Until 28 Sep 2026 a Tor outlived an Atlas that
/// crashed (or left through `process::exit`, which runs no clean-up), kept
/// the lock on its data folder, and every restart's Tor failed on that lock,
/// every five minutes, until the computer was restarted.
pub fn torrc(data: &Path, service: &Path, socks: u16, door: u16, extra: &[String]) -> String {
    let mut t = format!(
        "DataDirectory {}\nSocksPort 127.0.0.1:{socks}\nHiddenServiceDir {}\nHiddenServiceVersion 3\n\
         HiddenServicePort {ONION_PORT} 127.0.0.1:{door}\nLog notice file {}\nAvoidDiskWrites 1\n\
         __OwningControllerProcess {}\n",
        data.join("data").display(),
        service.display(),
        data.join("tor.log").display(),
        std::process::id()
    );
    for l in extra {
        t.push_str(l.trim());
        t.push('\n');
    }
    t
}

/// How far `tor` has got, from its log: the last "Bootstrapped N%".
fn bootstrapped(log: &str) -> u8 {
    log.lines()
        .filter_map(|l| l.split("Bootstrapped ").nth(1))
        .filter_map(|r| r.split('%').next()?.trim().parse().ok())
        .last()
        .unwrap_or(0)
}

fn free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok().map(|a| a.port())
}

/// `tor`, started by Atlas and stopped with it.
pub struct Tor {
    child: std::process::Child,
    pub socks: u16,
    pub address: String,
    dir: PathBuf,
    /// The last percent seen, and when it was first seen (seconds, Atlas's
    /// clock): how long Tor has sat without getting further.
    seen: (u8, u64),
    /// Which kind of bridge it's going through, if any (`BRIDGE_KINDS`).
    pub bridges: Option<String>,
}

impl Tor {
    /// Start `tor` for this Atlas: its onion service forwarding to `door`
    /// (the door's sealed-only port), and a SOCKS port for reaching others.
    pub fn start(binary: &Path, dir: &Path, me: &crate::peerkey::Identity, door: u16, extra: &[String]) -> Result<Tor, String> {
        let service = dir.join("onion");
        let address = write_service(&service, me)?;
        let socks = free_port().ok_or("couldn't find a free port for Tor")?;
        std::fs::create_dir_all(dir.join("data")).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(dir.join("tor.log"));
        let rc = dir.join("torrc");
        std::fs::write(&rc, torrc(dir, &service, socks, door, extra)).map_err(|e| format!("couldn't write Tor's settings: {e}"))?;
        // Absolute (without Windows' \\?\ form, which not every program reads),
        // since it's started from its own folder (below).
        let binary = std::path::absolute(binary).unwrap_or_else(|_| binary.to_path_buf());
        let rc = std::path::absolute(&rc).unwrap_or(rc);
        let mut cmd = crate::tools::command(&binary);
        cmd.arg("-f").arg(&rc).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        // In Tor's own folder, so the bridge programs' paths are short and
        // have no spaces in them (`bridge_lines`).
        if let Some(home) = binary.parent().filter(|p| !p.as_os_str().is_empty()) {
            cmd.current_dir(home);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // no console window
        }
        // A Tor left by an Atlas that didn't get to stop it holds the lock on
        // this data folder, and a new one can't start until it's gone.
        if let Some(pid) = stop_orphan(dir, &binary) {
            crate::errln!("atlas: stopped a Tor (process {pid}) left running by an earlier Atlas");
        }
        let child = cmd.spawn().map_err(|e| format!("couldn't start Tor ({}): {e}", binary.display()))?;
        let _ = std::fs::write(pid_file(dir), format!("{}\n{}\n", child.id(), binary.display()));
        // On Windows, also tied to this Atlas by a job object: when Atlas's
        // last handle closes -- however it ended -- Windows ends Tor.
        #[cfg(windows)]
        tie_to_this_process(&child);
        Ok(Tor { child, socks, address, dir: dir.to_path_buf(), seen: (0, 0), bridges: None })
    }

    /// Start it going through one kind of bridge (`bridge_lines`), for a
    /// network that blocks Tor. `extra` lines still come after.
    pub fn start_bridged(
        binary: &Path,
        dir: &Path,
        me: &crate::peerkey::Identity,
        door: u16,
        kind: &str,
        extra: &[String],
    ) -> Result<Tor, String> {
        let mut lines = bridge_lines(binary, kind).ok_or_else(|| format!("this copy of Tor has no {kind} bridges"))?;
        lines.extend(extra.iter().cloned());
        let mut t = Tor::start(binary, dir, me, door, &lines)?;
        t.bridges = Some(kind.to_string());
        Ok(t)
    }

    /// Has it got stuck connecting, as of `now`? Keeps its own note of when
    /// the percent last moved.
    pub fn stalled(&mut self, now: u64) -> bool {
        let log = std::fs::read_to_string(self.dir.join("tor.log")).unwrap_or_default();
        let p = bootstrapped(&log);
        // The clock starts the first time it's asked, on the caller's clock.
        if p != self.seen.0 || self.seen.1 == 0 {
            self.seen = (p, now);
        }
        is_stalled(&log, p, now.saturating_sub(self.seen.1))
    }

    /// Percent connected, as `tor` last said (100 = ready).
    pub fn progress(&self) -> u8 {
        bootstrapped(&std::fs::read_to_string(self.dir.join("tor.log")).unwrap_or_default())
    }

    /// Has it stopped by itself?
    pub fn stopped(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Tor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(pid_file(&self.dir));
    }
}

/// Where the Tor Atlas started last is written down: its process number and
/// the program it is.
pub fn pid_file(dir: &Path) -> PathBuf {
    dir.join("tor.pid")
}

/// The program a running process is, if it's running and the system says.
pub(crate) fn process_program(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{pid}/exe")).ok()
    }
    #[cfg(windows)]
    {
        use windows::core::PWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: the handle is closed before return; the buffer outlives the call.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
            let _ = CloseHandle(h);
            r.ok()?;
            Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let out = crate::tools::command("ps").args(["-p", &pid.to_string(), "-o", "comm="]).output().ok()?;
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!name.is_empty()).then(|| PathBuf::from(name))
    }
}

pub(crate) fn kill_process(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
        // SAFETY: the handle is closed before return.
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) else { return false };
            let ok = TerminateProcess(h, 1).is_ok();
            let _ = CloseHandle(h);
            ok
        }
    }
    #[cfg(not(windows))]
    {
        crate::tools::command("kill").args(["-9", &pid.to_string()]).status().is_ok_and(|s| s.success())
    }
}

/// If the Tor written down in `dir` (`pid_file`) is still running -- the
/// same program as `binary`, so never some other process that has since
/// been given that number -- stop it and wait for it to go. Returns its
/// process number if one was stopped.
pub fn stop_orphan(dir: &Path, binary: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(pid_file(dir)).ok()?;
    let mut lines = text.lines();
    let pid: u32 = lines.next()?.trim().parse().ok()?;
    let _ = std::fs::remove_file(pid_file(dir));
    if pid == std::process::id() {
        return None;
    }
    let running = process_program(pid)?;
    let same_name = running.file_name().map(|n| n.to_ascii_lowercase()) == binary.file_name().map(|n| n.to_ascii_lowercase());
    let written = lines.next().map(PathBuf::from);
    let same_program = same_name
        && (written.as_deref().is_none_or(|w| same_file(w, &running)) || same_file(binary, &running));
    if !same_program {
        return None;
    }
    if !kill_process(pid) {
        return None;
    }
    // Gone, so its lock on the data folder is too.
    for _ in 0..50 {
        if process_program(pid).is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Some(pid)
}

fn same_file(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    norm(a) == norm(b)
}

/// Put `child` in a job object that ends it when this process ends.
#[cfg(windows)]
fn tie_to_this_process(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    // One job for the life of Atlas: never closed by Atlas, so it closes
    // when Atlas ends, and everything in it ends with it.
    static JOB: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    let job = JOB.get_or_init(|| {
        // SAFETY: plain Win32 calls; `info` outlives the call that reads it.
        unsafe {
            let job = CreateJobObjectW(None, None).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .ok()?;
            Some(job.0 as usize)
        }
    });
    if let Some(job) = job {
        // SAFETY: both handles are valid for the call; the child's is owned
        // by `child`, which outlives it.
        unsafe {
            let _ = AssignProcessToJobObject(HANDLE(*job as *mut std::ffi::c_void), HANDLE(child.as_raw_handle()));
        }
    }
}

/// Open a connection to `onion` through the Tor running at `socks` (SOCKS5,
/// RFC 1928, no authentication; Tor resolves the name itself).
pub fn connect(socks: u16, onion: &str, timeout: Duration) -> std::io::Result<TcpStream> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::Other, m.to_string());
    if !is_onion(onion) {
        return Err(bad("that isn't an onion address"));
    }
    let mut s = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], socks)), Duration::from_secs(5))?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    s.write_all(&[5, 1, 0])?;
    let mut hello = [0u8; 2];
    s.read_exact(&mut hello)?;
    if hello != [5, 0] {
        return Err(bad("Tor refused the connection request"));
    }
    let mut req = vec![5, 1, 0, 3, onion.len() as u8];
    req.extend_from_slice(onion.as_bytes());
    req.extend_from_slice(&ONION_PORT.to_be_bytes());
    s.write_all(&req)?;
    let mut head = [0u8; 4];
    s.read_exact(&mut head)?;
    if head[1] != 0 {
        return Err(bad(match head[1] {
            4 => "their Atlas isn't online right now",
            6 => "Tor took too long to reach them",
            _ => "Tor couldn't reach them",
        }));
    }
    // The bound address that follows: skip it.
    let skip = match head[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut l = [0u8; 1];
            s.read_exact(&mut l)?;
            l[0] as usize
        }
        _ => return Err(bad("an answer Tor doesn't give")),
    };
    let mut rest = vec![0u8; skip + 2];
    s.read_exact(&mut rest)?;
    Ok(s)
}

// ---- Networks that block Tor (gap AM, 8.6) ------------------------------------

/// The ways round a network that blocks Tor, in the order worth trying them.
/// Each is a kind of bridge the Tor Project ships with Tor itself: obfs4
/// (looks like nothing in particular), then Snowflake (looks like a video
/// call), then meek (looks like visiting a big cloud website -- slow, but
/// hardest to block).
pub const BRIDGE_KINDS: [&str; 3] = ["obfs4", "snowflake", "meek"];

/// How long Tor may sit without getting further before the network is taken
/// to be blocking it. Tor normally connects in well under a minute; three
/// warnings from Tor itself ("Problem bootstrapping") count as stuck sooner.
pub const STALL_SECS: u64 = 120;
const STALL_WARNINGS: usize = 3;

/// The torrc lines that make Tor connect through one kind of bridge, read from
/// the `pt_config.json` the Tor Project ships in `pluggable_transports/`
/// beside `tor` -- so the bridge addresses are theirs, current as of the
/// bundle, and never typed into Atlas. `None` if the bundle has no such kind
/// (or no transports at all, as a bare system `tor` doesn't).
pub fn bridge_lines(tor_binary: &Path, kind: &str) -> Option<Vec<String>> {
    let pt_dir = tor_binary.parent()?.join("pluggable_transports");
    let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(pt_dir.join("pt_config.json")).ok()?).ok()?;
    let bridges: Vec<String> = config
        .get("bridges")?
        .get(kind)?
        .as_array()?
        .iter()
        .filter_map(|b| b.as_str().map(str::trim).filter(|b| !b.is_empty() && !b.contains('\n')).map(String::from))
        .collect();
    if bridges.is_empty() {
        return None;
    }
    // The transport lines: whichever plugin line names this transport
    // (lyrebird carries obfs4, meek_lite and webtunnel; snowflake has its own
    // line). The bridge line's first word is the transport's name, which for
    // "meek" is "meek_lite".
    let transport = bridges[0].split_whitespace().next()?.to_string();
    // Relative to Tor's own folder, which is where Atlas starts it
    // (`Tor::start`): Tor splits this line on spaces, and a Windows user
    // folder with a space in it ("C:\Users\Sam Lee\...") would cut the
    // program's path in two.
    let path = format!("pluggable_transports{}", std::path::MAIN_SEPARATOR);
    let plugin = config
        .get("pluggableTransports")?
        .as_object()?
        .values()
        .filter_map(|v| v.as_str())
        .find(|line| {
            line.strip_prefix("ClientTransportPlugin ")
                .and_then(|r| r.split_whitespace().next())
                .is_some_and(|names| names.split(',').any(|n| n == transport))
        })?
        .replace("${pt_path}", &path);
    // The plugin program itself has to be there: a line naming a missing
    // program makes Tor refuse to start at all.
    let program = plugin.split(" exec ").nth(1)?.split_whitespace().next()?;
    if !tor_binary.parent()?.join(program).is_file() {
        return None;
    }
    let mut lines = vec!["UseBridges 1".to_string(), plugin];
    lines.extend(bridges.into_iter().map(|b| format!("Bridge {b}")));
    Some(lines)
}

/// The next way to try after `current` stalled (`None`: going direct), or
/// `None` when every kind has been tried.
pub fn next_bridge_kind(current: Option<&str>) -> Option<&'static str> {
    match current {
        None => BRIDGE_KINDS.first().copied(),
        Some(c) => BRIDGE_KINDS.iter().position(|k| *k == c).and_then(|i| BRIDGE_KINDS.get(i + 1)).copied(),
    }
}

/// How many times Tor has said it's having trouble getting started.
fn bootstrap_warnings(log: &str) -> usize {
    log.lines().filter(|l| l.contains("Problem bootstrapping")).count()
}

/// Has Tor got stuck connecting? `progress` is the last percent it reached,
/// `still_for` how long it's sat there. At 100% it's never stuck.
pub fn is_stalled(log: &str, progress: u8, still_for: u64) -> bool {
    progress < 100 && (still_for >= STALL_SECS || bootstrap_warnings(log) >= STALL_WARNINGS)
}

// ---- Your own networks ---------------------------------------------------------

/// 100.64.0.0/10: shared addresses -- and your own private network's.
fn is_shared(v: Ipv4Addr) -> bool {
    let o = v.octets();
    o[0] == 100 && (o[1] & 0xc0) == 64
}

/// Is a request from this address one of your own -- this machine, your home
/// network, or your own private network? Anything else must come sealed.
/// Note: what Tor delivers arrives from this machine, which is why Tor is
/// pointed at the door's sealed-only port, never the ordinary one.
pub fn is_local_origin(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(v) => v.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v)),
        other => other,
    };
    match ip {
        IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local() || is_shared(v),
        IpAddr::V6(v) => {
            let s = v.segments()[0];
            v.is_loopback() || (s & 0xfe00) == 0xfc00 || (s & 0xffc0) == 0xfe80
        }
    }
}

/// "host:port" read as an address.
pub fn read_addr(s: &str) -> Option<SocketAddr> {
    s.trim().parse().ok()
}

/// This machine's address on the home network -- for a friend on the same
/// wifi, who then needn't go through Tor. Sends nothing to find it.
pub fn lan_v4() -> Option<Ipv4Addr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(v) if v.is_private() => Some(v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha3_matches_the_standards_test_vectors() {
        assert_eq!(hex(&sha3_256(b"")), "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a");
        assert_eq!(hex(&sha3_256(b"abc")), "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532");
        // Longer than one block (136 bytes).
        let long = vec![b'a'; 200];
        assert_eq!(sha3_256(&long).len(), 32);
        assert_ne!(sha3_256(&long), sha3_256(&long[..199]));
    }

    #[test]
    fn an_address_reads_as_one_and_is_the_same_every_time() {
        let me = crate::peerkey::Identity::from_seed_for_test([7; 32]);
        let a = my_address(&me);
        assert!(is_onion(&a), "{a}");
        assert_eq!(a, my_address(&crate::peerkey::Identity::from_seed_for_test([7; 32])));
        assert_ne!(a, my_address(&crate::peerkey::Identity::from_seed_for_test([8; 32])));
        assert!(!is_onion("example.com"));
    }

    #[test]
    fn tor_is_told_where_the_door_is_and_nothing_else_is_open() {
        let t = torrc(Path::new("/x"), Path::new("/x/onion"), 9051, 40000, &["UseBridges 1".into()]);
        assert!(t.contains("HiddenServicePort 80 127.0.0.1:40000"));
        assert!(t.contains("SocksPort 127.0.0.1:9051"), "Tor's SOCKS port must be this machine only");
        assert!(t.ends_with("UseBridges 1\n"));
        // Tor goes when this Atlas goes (28 Sep 2026).
        assert!(t.contains(&format!("__OwningControllerProcess {}\n", std::process::id())), "{t}");
        assert_eq!(bootstrapped("x Bootstrapped 45% (x)\ny Bootstrapped 100% (done): Done\n"), 100);
        assert_eq!(bootstrapped(""), 0);
    }

    #[test]
    fn socks_asks_tor_for_the_onion_by_name() {
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let onion = my_address(&crate::peerkey::Identity::from_seed_for_test([9; 32]));
        let want = onion.clone();
        let t = std::thread::spawn(move || {
            let (mut c, _) = server.accept().unwrap();
            let mut g = [0u8; 3];
            c.read_exact(&mut g).unwrap();
            assert_eq!(g, [5, 1, 0]);
            c.write_all(&[5, 0]).unwrap();
            let mut h = [0u8; 5];
            c.read_exact(&mut h).unwrap();
            assert_eq!(&h[..4], &[5, 1, 0, 3]);
            let mut name = vec![0u8; h[4] as usize + 2];
            c.read_exact(&mut name).unwrap();
            assert_eq!(&name[..name.len() - 2], want.as_bytes());
            assert_eq!(&name[name.len() - 2..], &80u16.to_be_bytes());
            c.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
            c.write_all(b"hello").unwrap();
        });
        let mut s = connect(port, &onion, Duration::from_secs(5)).unwrap();
        let mut got = [0u8; 5];
        s.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"hello");
        t.join().unwrap();
        assert!(connect(port, "not-an-onion", Duration::from_secs(1)).is_err());
    }

    #[test]
    fn your_own_networks_are_told_from_the_internet() {
        assert!(is_local_origin("::ffff:192.168.1.9".parse().unwrap()));
        assert!(is_local_origin("100.101.102.103".parse().unwrap()));
        assert!(is_local_origin("127.0.0.1".parse().unwrap()));
        assert!(!is_local_origin("8.8.8.8".parse().unwrap()));
        assert!(!is_local_origin("2606:4700::1111".parse().unwrap()));
    }

    fn bundle(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atlas-pt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pt = dir.join("pluggable_transports");
        std::fs::create_dir_all(&pt).unwrap();
        std::fs::write(pt.join("lyrebird"), b"").unwrap();
        // The shape of the Tor Project's own pt_config.json (expert bundle 15.0.23).
        std::fs::write(
            pt.join("pt_config.json"),
            r#"{"recommendedDefault":"obfs4",
                "pluggableTransports":{
                  "lyrebird":"ClientTransportPlugin meek_lite,obfs2,obfs3,obfs4,scramblesuit,webtunnel exec ${pt_path}lyrebird",
                  "snowflake":"ClientTransportPlugin snowflake exec ${pt_path}lyrebird",
                  "conjure":"ClientTransportPlugin conjure exec ${pt_path}conjure-client -registerURL https://example"},
                "bridges":{
                  "obfs4":["obfs4 192.0.2.1:443 AAAA cert=x iat-mode=0","obfs4 192.0.2.2:443 BBBB cert=y iat-mode=0"],
                  "snowflake":["snowflake 192.0.2.3:80 CCCC url=https://example/"],
                  "meek":["meek_lite 192.0.2.18:80 DDDD url=https://example/ front=www.example.com"],
                  "conjure":["conjure 143.110.214.222:80"]}}"#,
        )
        .unwrap();
        dir.join("tor")
    }

    #[test]
    fn bridges_come_from_the_bundle_tor_ships_with() {
        let tor = bundle("lines");
        let pt = format!("pluggable_transports{}", std::path::MAIN_SEPARATOR);
        let l = bridge_lines(&tor, "obfs4").unwrap();
        assert_eq!(l[0], "UseBridges 1");
        assert_eq!(l[1], format!("ClientTransportPlugin meek_lite,obfs2,obfs3,obfs4,scramblesuit,webtunnel exec {pt}lyrebird"));
        assert_eq!(&l[2..], ["Bridge obfs4 192.0.2.1:443 AAAA cert=x iat-mode=0", "Bridge obfs4 192.0.2.2:443 BBBB cert=y iat-mode=0"]);
        // Snowflake has its own plugin line; meek's bridges are "meek_lite".
        assert!(bridge_lines(&tor, "snowflake").unwrap()[1].starts_with("ClientTransportPlugin snowflake exec"));
        assert!(bridge_lines(&tor, "meek").unwrap()[1].contains("meek_lite"));
        // A kind the bundle lacks, or whose program is missing, gives nothing
        // rather than a torrc Tor would refuse.
        assert!(bridge_lines(&tor, "webtunnel").is_none());
        assert!(bridge_lines(&tor, "conjure").is_none(), "conjure-client isn't there, so no line for it");
        assert!(bridge_lines(Path::new("/nowhere/tor"), "obfs4").is_none());
    }

    #[test]
    fn a_stall_is_time_without_progress_or_tor_saying_so() {
        assert!(!is_stalled("", 5, 30));
        assert!(is_stalled("", 5, STALL_SECS));
        assert!(!is_stalled("", 100, 10_000), "connected is never stuck");
        let warned = "[warn] Problem bootstrapping. Stuck at 5% (conn): Connection refused\n".repeat(3);
        assert!(is_stalled(&warned, 5, 1));
        assert_eq!(next_bridge_kind(None), Some("obfs4"));
        assert_eq!(next_bridge_kind(Some("obfs4")), Some("snowflake"));
        assert_eq!(next_bridge_kind(Some("snowflake")), Some("meek"));
        assert_eq!(next_bridge_kind(Some("meek")), None);
    }

    /// The real check that the keys and address are what Tor itself makes:
    /// start `tor` on them and read back the address it serves. Needs `tor`
    /// installed; run with `cargo test --lib onion -- --ignored`.
    #[test]
    #[ignore = "needs the tor program on this machine"]
    fn tor_itself_serves_the_address_atlas_worked_out() {
        let tor = find_tor(None).expect("tor on the path");
        let dir = std::env::temp_dir().join(format!("atlas-onion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let me = crate::peerkey::Identity::from_seed_for_test([11; 32]);
        let expected = my_address(&me);
        let mut t = Tor::start(&tor, &dir, &me, 1, &["DisableNetwork 1".into()]).unwrap();
        std::thread::sleep(Duration::from_secs(3));
        assert!(!t.stopped(), "tor refused the keys: {}", std::fs::read_to_string(dir.join("tor.log")).unwrap_or_default());
        let host = std::fs::read_to_string(dir.join("onion").join("hostname")).unwrap();
        assert_eq!(host.trim(), expected);
        drop(t);
    }
}
