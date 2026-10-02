//! Talking to Atlas while you're on a call, and showing Atlas off on one
//! (Eric, 1 Oct 2026, round-two item 37).
//!
//! **Muted in the call while you talk to Atlas.** Say "Atlas…" (or hold the
//! talk key) on a Discord, Zoom, Teams, Slack or browser call, and the call
//! app's microphone is muted by Windows itself -- the per-app volume mixer,
//! the same switch as muting that app in Sound settings -- for as long as
//! you're talking with Atlas, then put back. Nothing is pressed in the call
//! app, nothing is focused, and only what Atlas muted is unmuted: if you'd
//! muted yourself already, you stay muted.
//!
//! What it can't hide: with the wake word, the name itself ("Atlas") is
//! heard before Atlas knows it was said; everything after it isn't. The talk
//! key hides all of it.
//!
//! **Showing Atlas off.** "Show Atlas off" (or "demo mode") turns the muting
//! off, so the call hears you and Atlas, and says how to share with sound in
//! the call app in use -- the call hears Atlas through the screen share's own
//! sound (Zoom's "Share sound", Teams' "Include sound", Discord's window
//! share). No driver is needed. "Stop showing off" ends it; it also ends by
//! itself after two hours.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// The programs that are calls, by a word in their executable's name.
pub const CALL_PROGRAMS: &[&str] = &[
    "discord", "zoom", "teams", "slack", "webex", "skype", "whatsapp", "signal", "telegram", "chrome", "msedge",
    "firefox", "brave", "opera", "vivaldi",
];

/// Atlas's own listening and helpers: never muted.
const NEVER: &[&str] = &["atlas", "ffmpeg", "whisper", "piper", "sherpa", "kokoro"];

/// Is this executable a call app whose microphone may be muted?
pub fn is_call_program(exe: &str) -> bool {
    let e = exe.to_lowercase();
    let name = e.rsplit(['\\', '/']).next().unwrap_or(&e);
    !NEVER.iter().any(|n| name.contains(n)) && CALL_PROGRAMS.iter().any(|c| name.contains(c))
}

/// A call app's microphone Atlas muted, to put back.
#[derive(Debug, Clone, PartialEq)]
pub struct Muted {
    pub pid: u32,
    pub exe: String,
}

/// On unless you've said otherwise (`set_on`).
static ON: AtomicBool = AtomicBool::new(true);
/// Showing Atlas off: until this time (seconds), nothing is muted.
static SHOWING_UNTIL: AtomicU64 = AtomicU64::new(0);
/// What Atlas muted and hasn't put back yet.
static MUTED: Mutex<Vec<Muted>> = Mutex::new(Vec::new());

/// How long showing off lasts when nobody ends it.
pub const SHOW_SECS: u64 = 2 * 60 * 60;

pub fn set_on(on: bool) {
    ON.store(on, Ordering::SeqCst);
}

fn is_on() -> bool {
    ON.load(Ordering::SeqCst)
}

pub fn showing(now: u64) -> bool {
    SHOWING_UNTIL.load(Ordering::SeqCst) > now
}

pub fn show(now: u64) {
    SHOWING_UNTIL.store(now + SHOW_SECS, Ordering::SeqCst);
    release();
}

pub fn stop_showing() {
    SHOWING_UNTIL.store(0, Ordering::SeqCst);
}

/// You've started talking to Atlas: mute the call apps holding the
/// microphone, if that's on and Atlas isn't being shown off. Cheap when
/// there's no call: one look at Windows' microphone sessions. Called from the
/// microphone's thread, the moment the name or the talk key is noticed.
pub fn addressed() {
    if !is_on() || showing(crate::store::now()) {
        return;
    }
    let Ok(mut held) = MUTED.lock() else { return };
    if !held.is_empty() {
        return;
    }
    *held = platform::mute_calls();
}

/// You're done talking to Atlas: put back what was muted. What you'd muted
/// yourself was never touched.
pub fn release() {
    let taken: Vec<Muted> = match MUTED.lock() {
        Ok(mut held) => std::mem::take(&mut *held),
        Err(_) => return,
    };
    if !taken.is_empty() {
        platform::unmute(&taken);
    }
}

/// What's muted right now, for the log.
pub fn muted_now() -> Vec<Muted> {
    MUTED.lock().map(|m| m.clone()).unwrap_or_default()
}

/// What was asked about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ask {
    ShowOff,
    StopShowing,
    MuteOn,
    MuteOff,
}

fn plain(s: &str) -> String {
    let s = s.to_lowercase().replace('\u{2019}', "'");
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn asks(said: &str) -> Option<Ask> {
    let t = plain(said);
    let any = |ps: &[&str]| ps.iter().any(|p| t.contains(p));
    if any(&["stop showing off", "stop showing you off", "demo mode off", "end demo mode", "show mode off", "done showing you off", "done showing off"]) {
        return Some(Ask::StopShowing);
    }
    if any(&["show atlas off", "show you off", "showing you off", "demo mode", "show mode on", "i want to show you off", "let them hear you"]) {
        return Some(Ask::ShowOff);
    }
    if any(&["don't mute my call", "dont mute my call", "stop muting my call", "stop muting me on call", "never mute my call", "don't mute me on call"]) {
        return Some(Ask::MuteOff);
    }
    if any(&["mute my call when i talk to you", "mute me on calls when i talk to you", "mute my calls when i talk to you", "start muting my call"]) {
        return Some(Ask::MuteOn);
    }
    None
}

/// How to share with sound in the call app in use.
pub fn share_with_sound(app: Option<&str>) -> &'static str {
    match app.map(str::to_lowercase).as_deref() {
        Some(a) if a.contains("zoom") => "In Zoom, press Share Screen and tick \"Share sound\" before you share.",
        Some(a) if a.contains("teams") => "In Teams, press Share and switch on \"Include sound\".",
        Some(a) if a.contains("discord") => "In Discord, share your screen or Atlas's window -- Discord sends its sound with it.",
        Some(a) if a.contains("slack") => "In Slack, share your screen; Slack sends only the picture, so the call hears me through your microphone.",
        Some(a) if a.contains("chrome") || a.contains("edge") || a.contains("meet") => {
            "In Google Meet, present a tab or your entire screen and switch on \"Also share system audio\"."
        }
        _ => "Share your screen with its sound on -- Zoom calls it \"Share sound\", Teams \"Include sound\", and Discord sends it with a window share.",
    }
}

#[cfg(windows)]
mod platform {
    use super::{is_call_program, Muted};
    use windows::core::Interface;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Media::Audio::{
        eCapture, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

    fn exe_of(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 520];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// Every capture session on every active microphone, with its process.
    fn each_session(mut f: impl FnMut(u32, &str, bool, &ISimpleAudioVolume)) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
            let Ok(all) = en.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) else { return };
            let n = all.GetCount().unwrap_or(0);
            for i in 0..n {
                let Ok(dev) = all.Item(i) else { continue };
                let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
                let Ok(list) = mgr.GetSessionEnumerator() else { continue };
                let count = list.GetCount().unwrap_or(0);
                for j in 0..count {
                    let Ok(ctl) = list.GetSession(j) else { continue };
                    let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
                    let Ok(pid) = ctl2.GetProcessId() else { continue };
                    if pid == 0 {
                        continue;
                    }
                    let active = ctl.GetState().map(|s| s == AudioSessionStateActive).unwrap_or(false);
                    let Ok(vol) = ctl.cast::<ISimpleAudioVolume>() else { continue };
                    let Some(exe) = exe_of(pid) else { continue };
                    f(pid, &exe, active, &vol);
                }
            }
        }
    }

    pub(super) fn mute_calls() -> Vec<Muted> {
        let mut out: Vec<Muted> = Vec::new();
        each_session(|pid, exe, active, vol| unsafe {
            if !active || !is_call_program(exe) {
                return;
            }
            // Already muted (by you): left alone, and not ours to unmute.
            if vol.GetMute().map(|m| m.as_bool()).unwrap_or(false) {
                return;
            }
            if vol.SetMute(true, std::ptr::null()).is_ok() && !out.iter().any(|m| m.pid == pid) {
                out.push(Muted { pid, exe: exe.to_string() });
            }
        });
        out
    }

    pub(super) fn unmute(what: &[Muted]) {
        each_session(|pid, _exe, _active, vol| unsafe {
            if what.iter().any(|m| m.pid == pid) {
                let _ = vol.SetMute(false, std::ptr::null());
            }
        });
    }
}

#[cfg(not(windows))]
mod platform {
    use super::Muted;
    pub(super) fn mute_calls() -> Vec<Muted> {
        Vec::new()
    }
    pub(super) fn unmute(_what: &[Muted]) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_apps_are_muted_and_atlas_never_is() {
        assert!(is_call_program(r"C:\Users\erics\AppData\Local\Discord\app-1.0.9\Discord.exe"));
        assert!(is_call_program(r"C:\Program Files\Zoom\bin\Zoom.exe"));
        assert!(is_call_program("ms-teams.exe"));
        assert!(is_call_program(r"C:\Program Files\Google\Chrome\Application\chrome.exe"));
        assert!(!is_call_program(r"C:\Users\erics\AppData\Local\Atlas\atlas.exe"));
        assert!(!is_call_program(r"C:\Users\erics\AppData\Local\Atlas\tools\ffmpeg\ffmpeg.exe"));
        assert!(!is_call_program("obs64.exe"));
    }

    #[test]
    fn erics_ways_of_asking() {
        assert_eq!(asks("I want to show you off on this call"), Some(Ask::ShowOff));
        assert_eq!(asks("Atlas, demo mode"), Some(Ask::ShowOff));
        assert_eq!(asks("ok stop showing off"), Some(Ask::StopShowing));
        assert_eq!(asks("don't mute my calls"), Some(Ask::MuteOff));
        assert_eq!(asks("what's the weather"), None);
    }

    #[test]
    fn showing_off_lasts_two_hours_and_turns_muting_off() {
        stop_showing();
        assert!(!showing(1_000));
        show(1_000);
        assert!(showing(1_000 + SHOW_SECS - 1));
        assert!(!showing(1_000 + SHOW_SECS));
        stop_showing();
        assert!(share_with_sound(Some("Zoom")).contains("Share sound"));
        assert!(share_with_sound(Some("Teams")).contains("Include sound"));
    }
}
