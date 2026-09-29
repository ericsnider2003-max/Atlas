//! Mac and Linux, as far as they go without platform-specific work.
//!
//! Written because "not supported on your platform" for *everything* is a
//! lack of effort dressed as a limit. Launching an app, reading files, running
//! a command, sleeping — none of that needs Win32, and refusing to do it off
//! Windows was sloppiness rather than a boundary.
//!
//! What genuinely needs writing per platform is window management, screen
//! reading and input synthesis. Those still refuse here, and the refusal now
//! names the specific thing rather than the whole platform.

use crate::error::{AtlasError, Result};
use crate::config::AppSpec;
use crate::platform::{ActiveWindow, Monitor, Platform, WindowId};

/// Which flavour of Unix this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavour {
    Mac,
    Linux,
}

pub struct Posix {
    pub flavour: Flavour,
}

impl Posix {
    pub fn here() -> Posix {
        Posix {
            flavour: if cfg!(target_os = "macos") { Flavour::Mac } else { Flavour::Linux },
        }
    }

    /// The command that opens things on this flavour.
    fn opener(&self) -> &'static str {
        match self.flavour {
            Flavour::Mac => "open",
            // Present on every desktop Linux worth the name.
            Flavour::Linux => "xdg-open",
        }
    }

    /// What would have to be written for this to work here.
    ///
    /// Says the specific piece rather than "not supported", because those are
    /// different messages: one tells you what's missing and one tells you to
    /// give up.
    fn not_yet(&self, what: &str) -> AtlasError {
        AtlasError::Platform(match self.flavour {
            Flavour::Mac => format!(
                "{what} needs the Mac layer — Accessibility APIs and a permission you grant \
                 once in System Settings. It isn't written yet, and nothing about the Mac \
                 prevents it"
            ),
            Flavour::Linux => format!(
                "{what} needs the Linux layer, and which one depends on whether you're running \
                 X11 or Wayland. It isn't written yet"
            ),
        })
    }
}

impl Platform for Posix {
    /// Best effort, and honest when it can't.
    ///
    /// A single screen at a sensible size is a better answer than an error for
    /// everything downstream that only wants to know where to put a window.
    fn monitors(&self) -> Result<Vec<Monitor>> {
        Ok(vec![Monitor {
            id: 1,
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            primary: true,
        }])
    }

    /// This genuinely works. Launching an app is a command, not an API.
    fn launch(&self, spec: &AppSpec) -> Result<()> {
        // A Store app id means nothing here, so it launches whatever the
        // name resolves to rather than failing on a Windows concept.
        let target = spec.launch.clone();
        let mut cmd = std::process::Command::new(self.opener());
        cmd.arg(&target);
        for a in &spec.args {
            cmd.arg(a);
        }
        cmd.spawn()
            // See `unwaited`: dropping the `Child` here left one zombie per
            // app the person opened.
            .map(crate::unwaited::dont_wait)
            .map_err(|e| AtlasError::Platform(format!("couldn't launch {target}: {e}")))
    }

    fn find_window(&self, _spec: &AppSpec) -> Result<Option<WindowId>> {
        // None rather than an error: "not up yet" is a legitimate answer and
        // the caller already handles it by retrying.
        Ok(None)
    }

    fn place(&self, _win: WindowId, _rect: crate::platform::PixelRect) -> Result<()> {
        Err(self.not_yet("moving windows"))
    }

    fn focus(&self, _win: WindowId) -> Result<()> {
        Err(self.not_yet("focusing a window"))
    }

    fn close(&self, spec: &AppSpec) -> Result<()> {
        // Closing by name works through the process table, which is portable.
        let name = spec
            .process_names
            .first()
            .cloned()
            .unwrap_or_else(|| spec.launch.clone());
        std::process::Command::new("pkill")
            .arg("-f")
            .arg(&name)
            .status()
            .map(|_| ())
            .map_err(|e| AtlasError::Platform(format!("couldn't close {name}: {e}")))
    }

    fn active_window(&self) -> Result<Option<ActiveWindow>> {
        Ok(None)
    }

    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    fn open_path(&self, path: &str) -> Result<()> {
        // A file that's there, or a web address -- nothing else is handed
        // to the opener.
        let web = path.starts_with("https://") || path.starts_with("http://");
        if !web && !std::path::Path::new(path).exists() {
            return Err(crate::error::AtlasError::Platform(format!("{path} isn't there any more")));
        }
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let child = std::process::Command::new(opener)
            .arg(path)
            .spawn()
            .map_err(|e| crate::error::AtlasError::Platform(format!("couldn't open {path}: {e}")))?;
        crate::unwaited::dont_wait(child);
        Ok(())
    }

    fn read_clipboard(&self) -> Result<Option<String>> {
        // The OS's own clipboard utility, not a Rust crate: pbpaste on a Mac,
        // and on Linux whichever of Wayland's or X11's tool is installed. If
        // none is present we return None, not an empty string — "I can't reach
        // the clipboard here" is a different thing from "it's empty", and the
        // caller says so.
        for (prog, args) in clipboard_readers(self.flavour) {
            match std::process::Command::new(prog).args(args).output() {
                Ok(out) if out.status.success() => {
                    return Ok(Some(String::from_utf8_lossy(&out.stdout).to_string()));
                }
                // Tool present but errored, or not installed — try the next.
                _ => continue,
            }
        }
        Ok(None)
    }

    fn write_clipboard(&self, text: &str) -> Result<()> {
        use std::io::Write;
        for (prog, args) in clipboard_writers(self.flavour) {
            let mut child = match std::process::Command::new(prog)
                .args(args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(_) => continue, // not installed; try the next
            };
            if let Some(mut stdin) = child.stdin.take() {
                if stdin.write_all(text.as_bytes()).is_err() {
                    let _ = child.wait();
                    continue;
                }
            }
            match child.wait() {
                Ok(status) if status.success() => return Ok(()),
                _ => continue,
            }
        }
        Err(AtlasError::Platform(
            "no clipboard tool is installed here — on Linux that's wl-clipboard or xclip/xsel".into(),
        ))
    }
}

/// The commands that read the clipboard, in the order to try them. Each is a
/// utility that ships with the desktop, not a dependency Atlas pulls in.
fn clipboard_readers(flavour: Flavour) -> Vec<(&'static str, Vec<&'static str>)> {
    match flavour {
        Flavour::Mac => vec![("pbpaste", vec![])],
        Flavour::Linux => vec![
            ("wl-paste", vec!["--no-newline"]),
            ("xclip", vec!["-selection", "clipboard", "-o"]),
            ("xsel", vec!["--clipboard", "--output"]),
        ],
    }
}

/// The commands that write the clipboard, taking the text on stdin. Same rule:
/// the desktop's own tools, tried in turn.
fn clipboard_writers(flavour: Flavour) -> Vec<(&'static str, Vec<&'static str>)> {
    match flavour {
        Flavour::Mac => vec![("pbcopy", vec![])],
        Flavour::Linux => vec![
            ("wl-copy", vec![]),
            ("xclip", vec!["-selection", "clipboard"]),
            ("xsel", vec!["--clipboard", "--input"]),
        ],
    }
}

/// What works here today, so the capability list doesn't have to guess.
pub fn works_here(flavour: Flavour) -> Vec<(&'static str, bool)> {
    let _ = flavour;
    vec![
        ("launching apps", true),
        ("closing apps", true),
        ("reading and writing files", true),
        ("running commands", true),
        ("everything that only thinks", true),
        ("moving windows", false),
        ("reading the screen", false),
        ("typing and clicking for you", false),
    ]
}
