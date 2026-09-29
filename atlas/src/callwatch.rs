//! Noticing that you're on a call.
//!
//! Call notes (Eric, 24 Sep 2026: "call: yes notes") need to know when a call
//! starts and ends, and there's no call app Atlas could ask. Windows already
//! knows: the privacy page that lists "apps using your microphone" is kept
//! in the registry, one entry per program, and a program holding the
//! microphone right now has a start time and a stop time of zero. That's the
//! same fact the little microphone icon in the taskbar shows.
//!
//! A call is a call app, or a browser (Google Meet, Teams on the web),
//! holding the microphone. Atlas's own listening is left out, or it would
//! notice itself.

use std::path::PathBuf;

/// One program's entry on Windows' "using your microphone" list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicUser {
    /// The program, as Windows names it: a path with `#` for `\`, or a
    /// store app's package name.
    pub who: String,
    /// Holding the microphone right now.
    pub now: bool,
}

/// Read `reg query ...\ConsentStore\microphone /s`.
pub fn mic_users_from(reg_output: &str) -> Vec<MicUser> {
    let mut out: Vec<MicUser> = Vec::new();
    let mut current: Option<(String, Option<u64>, Option<u64>)> = None;
    let finish = |c: Option<(String, Option<u64>, Option<u64>)>, out: &mut Vec<MicUser>| {
        if let Some((who, Some(start), Some(stop))) = c {
            out.push(MicUser { who, now: start != 0 && stop == 0 });
        }
    };
    for line in reg_output.lines() {
        let t = line.trim();
        if t.starts_with("HKEY_") {
            finish(current.take(), &mut out);
            let who = t.rsplit('\\').next().unwrap_or("").to_string();
            current = Some((who, None, None));
            continue;
        }
        let Some((_, start, stop)) = current.as_mut() else { continue };
        let mut parts = t.split_whitespace();
        let (Some(name), Some(_kind), Some(value)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let n = value
            .strip_prefix("0x")
            .and_then(|h| u64::from_str_radix(h, 16).ok())
            .or_else(|| value.parse().ok());
        match name {
            "LastUsedTimeStart" => *start = n,
            "LastUsedTimeStop" => *stop = n,
            _ => {}
        }
    }
    finish(current, &mut out);
    out
}

/// The programs that mean "a call" when they hold the microphone, by a word
/// in their name, and what to call each one.
const CALL_APPS: &[(&str, &str)] = &[
    ("teams", "Teams"),
    ("zoom", "Zoom"),
    ("discord", "Discord"),
    ("slack", "Slack"),
    ("webex", "Webex"),
    ("skype", "Skype"),
    ("whatsapp", "WhatsApp"),
    ("signal", "Signal"),
    ("telegram", "Telegram"),
    ("facetime", "FaceTime"),
    ("chrome.exe", "a call in Chrome"),
    ("msedge.exe", "a call in Edge"),
    ("firefox.exe", "a call in Firefox"),
    ("brave.exe", "a call in Brave"),
];

/// What holds the microphone for Atlas's own listening, which is not a call.
const ATLAS_OWN: &[&str] = &["ffmpeg", "whisper", "atlas.exe", "piper"];

/// Which call is going on, if any: the first call app holding the microphone.
pub fn call_from(users: &[MicUser]) -> Option<&'static str> {
    users.iter().filter(|u| u.now).find_map(|u| {
        let who = u.who.to_lowercase();
        if ATLAS_OWN.iter().any(|a| who.contains(a)) {
            return None;
        }
        CALL_APPS.iter().find(|(word, _)| who.contains(word)).map(|(_, name)| *name)
    })
}

/// Ask Windows, right now. `None` off Windows or when Windows won't say.
pub fn call_now() -> Option<&'static str> {
    if !cfg!(windows) {
        return None;
    }
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let reg = root.join("System32").join("reg.exe");
    let out = crate::firstlaunch::run_quietly(
        &reg,
        &["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone", "/s"],
    )
    .ok()?;
    call_from(&mic_users_from(&String::from_utf8_lossy(&out.stdout)))
}

/// A call starting or ending, from two looks in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Started(&'static str),
    Ended(&'static str),
    Same,
}

/// Remembers what the last look saw.
#[derive(Debug, Default)]
pub struct Watch {
    on: Option<&'static str>,
}

impl Watch {
    pub fn saw(&mut self, now: Option<&'static str>) -> Change {
        let was = self.on;
        self.on = now;
        match (was, now) {
            (None, Some(app)) => Change::Started(app),
            (Some(app), None) => Change::Ended(app),
            _ => Change::Same,
        }
    }
}
