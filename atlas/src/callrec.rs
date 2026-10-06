//! Recording a call: your microphone, and — once they've said yes — what
//! the others say.
//!
//! Your side comes from the microphone. The other side is what the laptop
//! plays, captured the way Windows lets any program capture its own sound
//! output ("loopback", through WASAPI). No virtual cable, no Stereo Mix, no
//! second program. The two are kept as separate files, which is what lets
//! the notes say who said what without guessing from voices.
//!
//! Both are written as 16 kHz mono, the form the speech-to-text model reads,
//! straight to disk as they arrive: an hour is about 115 MB per side, and
//! nothing is held in memory beyond a second or so.

use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Whose voices a recording holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Your microphone: you.
    Yours,
    /// What the laptop plays: everyone else on the call.
    Theirs,
}

/// Turns whatever rate and channel count a device gives into 16 kHz mono.
///
/// Averaging each output sample's share of the input, rather than picking
/// one input sample in every three, so speech keeps its shape and the
/// speech model hears words rather than aliasing.
#[derive(Debug, Clone)]
pub struct To16k {
    step: f64,
    channels: usize,
    at: f64,
    sum: f64,
    count: u32,
    last: f64,
}

pub const RATE: u32 = 16_000;

impl To16k {
    pub fn new(rate_in: u32, channels: u16) -> To16k {
        To16k { step: RATE as f64 / rate_in.max(1) as f64, channels: channels.max(1) as usize, at: 0.0, sum: 0.0, count: 0, last: 0.0 }
    }

    /// Feed interleaved samples (−1..1); get 16 kHz mono ones back.
    pub fn feed(&mut self, interleaved: &[f32]) -> Vec<i16> {
        let mut out = Vec::with_capacity((interleaved.len() as f64 * self.step) as usize + 1);
        for frame in interleaved.chunks(self.channels) {
            let mono = frame.iter().sum::<f32>() / frame.len() as f32;
            self.sum += mono as f64;
            self.count += 1;
            self.at += self.step;
            // `while`, not `if`: below 16 kHz (an 8 kHz headset, say) one
            // input sample makes two or more output ones. With `if` those
            // came out at the input's rate, labelled 16 kHz, and played back
            // at double speed.
            while self.at >= 1.0 {
                self.at -= 1.0;
                let v = if self.count > 0 { (self.sum / self.count as f64).clamp(-1.0, 1.0) } else { self.last };
                self.last = v;
                out.push((v * i16::MAX as f64) as i16);
                self.sum = 0.0;
                self.count = 0;
            }
        }
        out
    }
}

/// A WAV file written as it goes, with its sizes filled in when it's closed.
pub struct WavOut {
    file: std::fs::File,
    samples: u64,
}

impl WavOut {
    pub fn create(path: &Path) -> std::io::Result<WavOut> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut file = std::fs::File::create(path)?;
        file.write_all(&wav_header(0))?;
        Ok(WavOut { file, samples: 0 })
    }

    /// How many samples are in the file so far.
    pub fn samples(&self) -> u64 {
        self.samples
    }

    pub fn write(&mut self, samples: &[i16]) -> std::io::Result<()> {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.file.write_all(&bytes)?;
        self.samples += samples.len() as u64;
        Ok(())
    }

    /// Put the real sizes in the header. A file closed without this is
    /// still readable by most tools, but not reliably by all.
    pub fn close(mut self) -> std::io::Result<u64> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&wav_header(self.samples * 2))?;
        self.file.flush()?;
        Ok(self.samples)
    }
}

/// The 44-byte header of a 16 kHz mono 16-bit WAV holding `data_bytes`.
pub fn wav_header(data_bytes: u64) -> [u8; 44] {
    let data = data_bytes.min(u32::MAX as u64 - 36) as u32;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&RATE.to_le_bytes());
    h[28..32].copy_from_slice(&(RATE * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

/// How much silence to write so a file keeps pace with the clock: none
/// until it's half a second behind, then all of the gap.
pub fn silence_due(elapsed: std::time::Duration, written: u64) -> usize {
    let expected = (elapsed.as_secs_f64() * RATE as f64) as u64;
    if expected > written + RATE as u64 / 2 {
        (expected - written) as usize
    } else {
        0
    }
}

/// The microphone your side of a call is recorded from: the one Atlas
/// listens to you through (`voice::microphone_now`), by name.
///
/// Until 5 Oct 2026 your side was Windows' default microphone, whatever
/// Atlas itself was set to hear you through. On a laptop with a webcam
/// microphone picked for Atlas and the built-in array left as Windows'
/// default, the call recorded the microphone you weren't talking into
/// (`atlas call check`: "Your microphone: ... but it was silent").
static WANT_MIC: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

/// Record your side from the microphone called `name` (as the device names
/// itself); empty is Windows' default.
pub fn use_microphone(name: &str) {
    if let Ok(mut m) = WANT_MIC.write() {
        *m = Some(name.trim().to_string()).filter(|n| !n.is_empty());
    }
}

/// Which of `names` is the microphone asked for: the same name, or, failing
/// that, one name inside the other ("Microphone (HD Pro Webcam C920)" and
/// "HD Pro Webcam C920"). `None`: none of them, so the default is used.
pub fn microphone_named(want: &str, names: &[String]) -> Option<usize> {
    let w = want.trim().to_lowercase();
    if w.is_empty() {
        return None;
    }
    names
        .iter()
        .position(|n| n.trim().to_lowercase() == w)
        .or_else(|| names.iter().position(|n| {
            let n = n.trim().to_lowercase();
            !n.is_empty() && (n.contains(&w) || w.contains(&n))
        }))
}

/// One side of a call being recorded. Stops and closes its file when
/// `finish` is called or when it's dropped.
pub struct Recording {
    pub side: Side,
    pub path: PathBuf,
    /// Held: nothing is taken from the device, and silence is written in
    /// its place so the file stays on the clock. Pausing Atlas mid-call
    /// holds the recording rather than ending the call's notes.
    held: std::sync::Arc<std::sync::atomic::AtomicBool>,
    stop: Option<std::sync::mpsc::Sender<()>>,
    done: Option<std::sync::mpsc::Receiver<Result<u64, String>>>,
}

impl Recording {
    /// Hold (or carry on) recording. Held, nothing is captured.
    pub fn hold(&self, on: bool) {
        self.held.store(on, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_held(&self) -> bool {
        self.held.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Stop, close the file, and say how many seconds it holds.
    pub fn finish(mut self) -> Result<f64, String> {
        self.end()
    }

    fn end(&mut self) -> Result<f64, String> {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
        match self.done.take() {
            Some(d) => d
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|_| "the recording didn't stop when asked".to_string())?
                .map(|n| n as f64 / RATE as f64),
            None => Ok(0.0),
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        crate::heard!(self.end());
    }
}

/// Start recording one side into `path`; `held` starts it held (Atlas is
/// paused), so not a moment is captured before the hold takes effect.
pub fn start(side: Side, path: &Path, held: bool) -> Result<Recording, String> {
    let held = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(held));
    let held_in = held.clone();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<u64, String>>();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let out = path.to_path_buf();
    std::thread::Builder::new()
        .name(format!("call-{side:?}"))
        .spawn(move || {
            let r = capture(side, &out, stop_rx, &ready_tx, &held_in);
            let _ = ready_tx.send(r.as_ref().map(|_| ()).map_err(|e| e.clone()));
            let _ = done_tx.send(r);
        })
        .map_err(|e| format!("couldn't start recording: {e}"))?;
    match ready_rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(())) => Ok(Recording { side, path: path.to_path_buf(), held, stop: Some(stop_tx), done: Some(done_rx) }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("the sound device didn't answer".into()),
    }
}

/// A recording of silence, kept to the clock like a real one: what a side
/// holds when there's no sound device to hand it, and what the call-notes
/// steps are checked against on a machine without one.
pub fn silent(side: Side, path: &Path, held: bool) -> Result<Recording, String> {
    let held = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(held));
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<u64, String>>();
    let mut wav = WavOut::create(path).map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    std::thread::Builder::new()
        .name(format!("call-{side:?}-silent"))
        .spawn(move || {
            let began = std::time::Instant::now();
            let r = loop {
                let due = silence_due(began.elapsed(), wav.samples());
                if due > 0 {
                    if let Err(e) = wav.write(&vec![0i16; due]) {
                        break Err(e.to_string());
                    }
                }
                match stop_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    _ => break wav.close().map_err(|e| e.to_string()),
                }
            };
            let _ = done_tx.send(r);
        })
        .map_err(|e| format!("couldn't start recording: {e}"))?;
    Ok(Recording { side, path: path.to_path_buf(), held, stop: Some(stop_tx), done: Some(done_rx) })
}

#[cfg(windows)]
fn capture(
    side: Side,
    path: &Path,
    stop: std::sync::mpsc::Receiver<()>,
    ready: &std::sync::mpsc::Sender<Result<(), String>>,
    held: &std::sync::atomic::AtomicBool,
) -> Result<u64, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let (device, config) = match side {
        Side::Yours => {
            let want = WANT_MIC.read().ok().and_then(|m| m.clone()).unwrap_or_default();
            let mut inputs: Vec<cpal::Device> = host.input_devices().map(|d| d.collect()).unwrap_or_default();
            let names: Vec<String> = inputs.iter().map(|d| d.name().unwrap_or_default()).collect();
            let d = match microphone_named(&want, &names) {
                Some(i) => inputs.swap_remove(i),
                None => {
                    if !want.is_empty() {
                        crate::errln!("recording: no microphone called \"{want}\" (have: {}); using Windows' default", names.join(", "));
                    }
                    host.default_input_device().ok_or("there's no microphone")?
                }
            };
            let c = d.default_input_config().map_err(|e| format!("the microphone won't say how it records: {e}"))?;
            (d, c)
        }
        // Loopback: an input stream on the output device is what WASAPI
        // calls capturing what the machine plays.
        Side::Theirs => {
            let d = host.default_output_device().ok_or("there's no speaker or headset to listen to")?;
            let c = d.default_output_config().map_err(|e| format!("the speakers won't say how they play: {e}"))?;
            (d, c)
        }
    };
    let (tx, rx) = std::sync::mpsc::channel::<Vec<f32>>();
    // A headset unplugged mid-call ends the stream with an error and then
    // nothing. Noticed, so the file is closed properly rather than padded
    // with silence until the call ends.
    let lost = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let lost_in = lost.clone();
    let err = move |e: cpal::StreamError| {
        crate::errln!("recording: {e}");
        if matches!(e, cpal::StreamError::DeviceNotAvailable) {
            lost_in.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    };
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let tx = tx.clone();
            device.build_input_stream(&config.config(), move |d: &[f32], _: &_| { let _ = tx.send(d.to_vec()); }, err, None)
        }
        cpal::SampleFormat::I16 => {
            let tx = tx.clone();
            device.build_input_stream(
                &config.config(),
                move |d: &[i16], _: &_| { let _ = tx.send(d.iter().map(|s| *s as f32 / i16::MAX as f32).collect()); },
                err,
                None,
            )
        }
        other => return Err(format!("the sound device records in a form I don't read ({other:?})")),
    }
    .map_err(|e| format!("couldn't open the sound device: {e}"))?;
    // Only the stream's callback holds a sender now, so when the stream goes
    // the channel says so.
    drop(tx);
    stream.play().map_err(|e| format!("couldn't start the sound device: {e}"))?;
    let mut wav = WavOut::create(path).map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    let mut down = To16k::new(config.sample_rate().0, config.channels());
    let _ = ready.send(Ok(()));
    let began = std::time::Instant::now();
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            // Held: what arrives is dropped unheard, and the gap is filled
            // with silence below like any other.
            Ok(_) if held.load(std::sync::atomic::Ordering::SeqCst) => {}
            Ok(chunk) => {
                wav.write(&down.feed(&chunk)).map_err(|e| e.to_string())?;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
        // Windows sends nothing at all from loopback while nothing plays,
        // so a quiet stretch would vanish from their side and every later
        // word would line up against the wrong moment of yours. Silence is
        // written in for any gap, keeping both files on the same clock.
        let due = silence_due(began.elapsed(), wav.samples());
        if due > 0 {
            wav.write(&vec![0i16; due]).map_err(|e| e.to_string())?;
        }
        // Asked to stop, or the `Recording` that would ask is gone.
        if !matches!(stop.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)) {
            break;
        }
        if lost.load(std::sync::atomic::Ordering::Relaxed) {
            crate::errln!("recording: the sound device went away; closing the file");
            break;
        }
    }
    drop(stream);
    while let Ok(chunk) = rx.try_recv() {
        if held.load(std::sync::atomic::Ordering::SeqCst) {
            continue;
        }
        wav.write(&down.feed(&chunk)).map_err(|e| e.to_string())?;
    }
    wav.close().map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn capture(
    _side: Side,
    _path: &Path,
    _stop: std::sync::mpsc::Receiver<()>,
    _ready: &std::sync::mpsc::Sender<Result<(), String>>,
    _held: &std::sync::atomic::AtomicBool,
) -> Result<u64, String> {
    Err("recording a call works on Windows only".into())
}

#[cfg(test)]
mod picking_the_microphone {
    use super::microphone_named;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_microphone_atlas_listens_through_is_the_one_recorded() {
        let have = names(&["Microphone Array (Realtek(R) Audio)", "Microphone (HD Pro Webcam C920)"]);
        assert_eq!(microphone_named("Microphone (HD Pro Webcam C920)", &have), Some(1));
        assert_eq!(microphone_named("microphone array (realtek(r) audio)", &have), Some(0));
        // ffmpeg's name and Windows' can differ by the "Microphone (...)" wrapper.
        assert_eq!(microphone_named("HD Pro Webcam C920", &have), Some(1));
    }

    #[test]
    fn none_named_or_none_found_is_windows_default() {
        let have = names(&["Microphone Array (Realtek(R) Audio)"]);
        assert_eq!(microphone_named("", &have), None);
        assert_eq!(microphone_named("AirPods Pro", &have), None);
    }
}
