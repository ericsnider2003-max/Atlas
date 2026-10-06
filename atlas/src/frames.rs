//! Pictures out of the camera, continuously.
//!
//! ## Why not just take a photo each time
//!
//! Atlas already has `capture_webcam`: run ffmpeg, get one PNG. That is right
//! for "look at the room once every twenty seconds" and wrong for tracking a
//! hand, because it pays the whole cost of opening the camera, negotiating a
//! format and starting a process — a few hundred milliseconds — for every
//! single frame. Fifteen times a second, that is not a slow tracker, it is a
//! machine doing nothing but starting and stopping ffmpeg.
//!
//! So one process is started and left running, and frames are read off its
//! output as they arrive. The camera opens once.
//!
//! ## Raw, not encoded
//!
//! The pipe carries uncompressed pixels. Encoding to PNG and decoding again
//! is work done twice for no gain — the model wants numbers, and a PNG is
//! numbers that have been squeezed and unsqueezed on the way. Raw at a small
//! capture size is both faster and simpler than compressed at a large one.
//!
//! ## Closing it properly
//!
//! A camera left open is a light left on. `Drop` kills the process and waits
//! for it, because a webcam that stays lit after Eric said stop is the most
//! visible way this could misbehave.

use crate::error::{AtlasError, Result};
use std::io::Read;
use std::process::{Child, Stdio};

/// How the camera is opened.
#[derive(Debug, Clone)]
pub struct Feed {
    /// The ffmpeg input arguments, from config, so this works on whatever
    /// camera stack the machine has.
    pub open_with: Vec<String>,
    /// Capture size. Small on purpose — the model resizes down to about two
    /// hundred pixels anyway, so capturing at 1080p means moving several
    /// megabytes a frame in order to throw almost all of it away.
    pub width: usize,
    pub height: usize,
    /// Frames a second to hand over, when fewer than the camera's own are
    /// wanted. Dropped by ffmpeg *before* the picture is scaled and turned
    /// into pixels, so the frames nobody reads cost almost nothing: measured
    /// on Eric's C920 (2 Oct 2026), kept open at 2 a second is about 0.7%
    /// of a core, against about 9% for opening the camera afresh every four
    /// seconds.
    pub per_second: Option<u32>,
}

impl Default for Feed {
    fn default() -> Self {
        Feed {
            open_with: Vec::new(),
            width: 640,
            height: 480,
            per_second: None,
        }
    }
}

/// A camera that is open and handing over frames.
pub struct Rolling {
    child: Child,
    frame: Vec<u8>,
    width: usize,
    height: usize,
    /// Frames that arrived torn or short, so a camera that is failing can be
    /// told from one that is simply not seeing a hand.
    pub dropped: u32,
}

impl Rolling {
    /// Open the camera and start reading.
    pub fn start(feed: &Feed) -> Result<Rolling> {
        if feed.open_with.is_empty() {
            return Err(AtlasError::Config(
                "there's no camera set up — the capture line in tools.yaml is empty".into(),
            ));
        }
        let mut args: Vec<String> = feed.open_with.clone();
        let filter = match feed.per_second {
            Some(n) if n > 0 => format!("fps={n},scale={}:{}", feed.width, feed.height),
            _ => format!("scale={}:{}", feed.width, feed.height),
        };
        args.extend(
            [
                "-vf",
                &filter,
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "-",
            ]
            .map(str::to_string),
        );

        let child = crate::tools::command("ffmpeg")
            .args(&args)
            .stdout(Stdio::piped())
            // Kept rather than inherited: ffmpeg is chatty, and its running
            // commentary appearing in the middle of Atlas talking is the sort
            // of thing that makes a product feel unfinished.
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| AtlasError::Platform(format!("couldn't open the camera: {e}")))?;

        Ok(Rolling {
            child,
            frame: vec![0u8; feed.width * feed.height * 3],
            width: feed.width,
            height: feed.height,
            dropped: 0,
        })
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// The next frame, or `None` if the camera stopped.
    ///
    /// Blocks until a whole frame has arrived. A partial frame is never handed
    /// back — half a picture would be read as a hand in the wrong place rather
    /// than as an error, which is worse than no picture at all.
    #[allow(clippy::should_implement_trait, reason = "a blocking read of the next whole frame, borrowing the buffer; not an Iterator")]
    pub fn next(&mut self) -> Option<&[u8]> {
        let out = self.child.stdout.as_mut()?;
        match out.read_exact(&mut self.frame) {
            Ok(()) => Some(&self.frame),
            Err(_) => {
                self.dropped += 1;
                None
            }
        }
    }

    /// Has the camera process died?
    ///
    /// Asked so a dead camera is reported once as a dead camera, rather than
    /// as an endless run of frames with no hand in them — which looks
    /// identical to sitting still.
    pub fn ended(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
    }
}

impl Drop for Rolling {
    fn drop(&mut self) {
        // A camera left open is a light left on.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A camera kept open between looks, with the newest frame always to hand.
///
/// For the looks the daemon takes on its own -- presence, a hand answering a
/// question, "watch me" -- which come every one to five seconds. Each used
/// to open the camera afresh: about 360 ms of ffmpeg's CPU and almost two
/// seconds of wall time per look on Eric's laptop (2 Oct 2026), with the
/// daemon's loop waiting the whole time. Kept open at a couple of frames a
/// second instead, a thread reads every frame as it arrives (so nothing goes
/// stale in the pipe) and keeps only the latest.
///
/// Closed when dropped: the camera process is killed, its stream ends and
/// the reading thread with it, so the light goes off with the value.
pub struct Latest {
    shared: std::sync::Arc<std::sync::Mutex<Option<(Vec<u8>, std::time::Instant)>>>,
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    width: usize,
    height: usize,
    opened: std::time::Instant,
    /// The camera process. Dropped with this value, which kills it.
    _camera: Rolling,
}

impl Latest {
    /// Open the camera and start keeping its newest frame.
    pub fn start(feed: &Feed) -> Result<Latest> {
        let mut camera = Rolling::start(feed)?;
        let (width, height) = camera.size();
        // The reading end goes to the thread; the process stays here, so
        // letting go of this value kills it even if the thread is stuck
        // waiting on a camera that has hung.
        let mut out = camera
            .child
            .stdout
            .take()
            .ok_or_else(|| AtlasError::Platform("the camera gave no picture stream".into()))?;
        let shared = std::sync::Arc::new(std::sync::Mutex::new(None));
        let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (s2, alive2) = (shared.clone(), alive.clone());
        let len = width * height * 3;
        std::thread::Builder::new()
            .name("atlas-camera".into())
            .spawn(move || {
                let mut buf = vec![0u8; len];
                while out.read_exact(&mut buf).is_ok() {
                    if let Ok(mut g) = s2.lock().or_else(crate::crash::unpoison) {
                        *g = Some((buf.clone(), std::time::Instant::now()));
                    }
                }
                alive2.store(false, std::sync::atomic::Ordering::Relaxed);
            })
            .map_err(|e| AtlasError::Platform(format!("couldn't start reading the camera: {e}")))?;
        Ok(Latest { shared, alive, width, height, opened: std::time::Instant::now(), _camera: camera })
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Still reading frames?
    pub fn alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The newest frame that arrived after `settle` from opening (a camera
    /// that has just opened is still finding its exposure) and is no older
    /// than `fresh`, waiting up to `wait` for one.
    pub fn frame(&self, settle: std::time::Duration, fresh: std::time::Duration, wait: std::time::Duration) -> Option<Vec<u8>> {
        let until = std::time::Instant::now() + wait;
        loop {
            if let Ok(g) = self.shared.lock().or_else(crate::crash::unpoison) {
                if let Some((f, at)) = g.as_ref() {
                    if at.duration_since(self.opened) >= settle && at.elapsed() <= fresh {
                        return Some(f.clone());
                    }
                }
            }
            if !self.alive() || std::time::Instant::now() >= until {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }
}

/// Turn the configured one-shot capture command into a continuous one.
///
/// `capture_webcam` in tools.yaml already knows how to open the camera on this
/// machine — which device, which backend, which format. Re-deriving that here
/// would mean two places to get it right and one of them silently rotting. So
/// the input half is reused and the output half replaced.
pub fn from_capture_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for a in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        match a.as_str() {
            // Everything about writing a single PNG belongs to the one-shot
            // version and must not survive into a continuous feed.
            "-frames:v" => skip_next = true,
            "-y" => {}
            other if other.ends_with(".png") || other.contains("{out_png}") => {}
            other => out.push(other.to_string()),
        }
    }
    out
}
