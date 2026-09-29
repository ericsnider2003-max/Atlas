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
}

impl Default for Feed {
    fn default() -> Self {
        Feed {
            open_with: Vec::new(),
            width: 640,
            height: 480,
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
        args.extend(
            [
                "-vf",
                &format!("scale={}:{}", feed.width, feed.height),
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
