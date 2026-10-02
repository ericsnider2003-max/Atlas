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
    // Read straight from the registry (2 Oct 2026): this runs every 15 s,
    // and starting reg.exe each time was a new process four times a minute
    // and the largest share of the idle loop's time on Eric's laptop.
    // reg.exe stays as the way back if the direct read fails.
    #[cfg(windows)]
    if let Some(users) = mic_users_direct() {
        return call_from(&users);
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

/// Windows' "using your microphone" list, read with the registry calls
/// rather than reg.exe. Store apps are direct children of the key; desktop
/// programs sit one level down, under `NonPackaged`.
#[cfg(windows)]
fn mic_users_direct() -> Option<Vec<MicUser>> {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ};
    const MIC: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: a key this module opened, closed once.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
    fn open(parent: HKEY, sub: &str) -> Option<Key> {
        let mut k = HKEY::default();
        // SAFETY: `k` outlives the call; the name is a live HSTRING.
        let r = unsafe { RegOpenKeyExW(parent, &HSTRING::from(sub), 0, KEY_READ, &mut k) };
        r.is_ok().then_some(Key(k))
    }
    fn children(k: &Key) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0..4096u32 {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            // SAFETY: the buffer and its length are passed together.
            let r = unsafe { RegEnumKeyExW(k.0, i, PWSTR(buf.as_mut_ptr()), &mut len, None, PWSTR::null(), None, None) };
            if r.is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        out
    }
    fn qword(k: &Key, name: &str) -> Option<u64> {
        let mut v = [0u8; 8];
        let mut len = 8u32;
        // SAFETY: an 8-byte buffer and its length.
        let r = unsafe { RegQueryValueExW(k.0, &HSTRING::from(name), None, None, Some(v.as_mut_ptr()), Some(&mut len)) };
        (r.is_ok() && len == 8).then(|| u64::from_le_bytes(v))
    }
    fn entry(k: &Key, who: &str, out: &mut Vec<MicUser>) {
        if let (Some(start), Some(stop)) = (qword(k, "LastUsedTimeStart"), qword(k, "LastUsedTimeStop")) {
            out.push(MicUser { who: who.to_string(), now: start != 0 && stop == 0 });
        }
    }
    let root = open(HKEY_CURRENT_USER, MIC)?;
    let mut out = Vec::new();
    for name in children(&root) {
        let Some(k) = open(root.0, &name) else { continue };
        if name == "NonPackaged" {
            for inner in children(&k) {
                if let Some(ik) = open(k.0, &inner) {
                    entry(&ik, &inner, &mut out);
                }
            }
        } else {
            entry(&k, &name, &mut out);
        }
    }
    Some(out)
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
