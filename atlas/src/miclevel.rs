//! A microphone's input level in Windows: read, and raised when it is set
//! so low that you'd have to shout.
//!
//! **What this touches, and why it's safe to.** Windows keeps one input
//! volume per recording device: the slider under Sound settings -> Input ->
//! Volume (the Recording tab's Levels in the old control panel). It is
//! `IAudioEndpointVolume`'s master level, a number from 0 to 1, and reading
//! and setting it through that interface is exactly what the slider does --
//! per user, per device, kept across restarts, and put back by setting the
//! old number again or moving the slider. Nothing else is touched: not the
//! mute switch, not a driver's microphone boost, not any other device.
//!
//! When Atlas raises it (`raise_if_low`) it does so once per microphone, only
//! upward, to `RAISE_TO`, only when the level is under `LOW_SCALAR` and the
//! voice it hears is under `leveller::LOW_INPUT_DB`, and it says so, with the
//! level it was at, so it can be put back.

use serde::{Deserialize, Serialize};

/// A level under this, with a voice that reaches Atlas quietly, is raised.
pub const LOW_SCALAR: f32 = 0.5;
/// Where a low level is raised to: loud enough for a voice across a desk,
/// short of the top, where a close-up voice starts to clip.
pub const RAISE_TO: f32 = 0.8;

/// A capture device's level as Windows has it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InputLevel {
    /// 0..1: the Sound settings slider.
    pub scalar: f32,
    pub muted: bool,
}

/// Where a low level should go, if anywhere: never down, never when muted
/// (unmuting is yours to decide), only when it's under `LOW_SCALAR`.
pub fn raise_to(level: InputLevel) -> Option<f32> {
    (!level.muted && level.scalar < LOW_SCALAR).then_some(RAISE_TO)
}

/// Is `wanted` the device Windows calls `friendly`? The names ffmpeg lists
/// are Windows' friendly names, sometimes cut short or with the "®" lost to
/// a code page (29 Sep 2026), so letters and digits are compared.
pub fn same_device(wanted: &str, friendly: &str) -> bool {
    let plain = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    let (a, b) = (plain(wanted), plain(friendly));
    !a.is_empty() && (a == b || (a.len() >= 12 && (b.starts_with(&a) || a.starts_with(&b))))
}

/// What Atlas changed, kept so it can be said and put back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Changes {
    /// (microphone, level it was at, level Atlas set, when).
    pub raised: Vec<(String, f32, f32, u64)>,
    /// (microphone, when Atlas last said it was too quiet).
    pub told: Vec<(String, u64)>,
}

impl Changes {
    pub const RECORD: &'static str = "mic_level_changes";

    fn raised_before(&self, mic: &str) -> bool {
        self.raised.iter().any(|r| r.0 == mic)
    }

    /// Said within the last day? Once a day is enough.
    fn told_recently(&self, mic: &str, now: u64) -> bool {
        self.told.iter().any(|(m, at)| m == mic && now.saturating_sub(*at) < 86_400)
    }

    fn note_told(&mut self, mic: &str, now: u64) {
        self.told.retain(|(m, _)| m != mic);
        self.told.push((mic.to_string(), now));
    }
}

/// The level Windows has `name` at. `None` off Windows, or when the device
/// can't be found or read.
#[cfg(windows)]
fn read(name: &str) -> Option<InputLevel> {
    win::with_endpoint(name, |v| unsafe {
        let scalar = v.GetMasterVolumeLevelScalar().ok()?;
        let muted = v.GetMute().map(|b| b.as_bool()).unwrap_or(false);
        Some(InputLevel { scalar, muted })
    })
    .flatten()
}

#[cfg(not(windows))]
fn read(_name: &str) -> Option<InputLevel> {
    None
}

/// Set `name`'s input level to `scalar` (0..1); the level it was at.
#[cfg(windows)]
fn set(name: &str, scalar: f32) -> Result<f32, String> {
    win::with_endpoint(name, |v| unsafe {
        let was = v.GetMasterVolumeLevelScalar().map_err(|e| e.to_string())?;
        v.SetMasterVolumeLevelScalar(scalar.clamp(0.0, 1.0), std::ptr::null()).map_err(|e| e.to_string())?;
        Ok(was)
    })
    .unwrap_or_else(|| Err(format!("Windows has no recording device called \"{name}\"")))
}

#[cfg(not(windows))]
fn set(_name: &str, _scalar: f32) -> Result<f32, String> {
    Err("the input level can only be set on Windows".into())
}

/// When the voice heard on `mic` is too quiet (`leveller::quiet_input`), and
/// not said in the last day: the sentence to say, having raised Windows'
/// input level once if it was low. Records what it did in `changes`.
pub fn after_a_turn(mic: &str, lev: &crate::leveller::Leveller, changes: &mut Changes, now: u64) -> Option<String> {
    let speech = lev.quiet_input(mic)?;
    if changes.told_recently(mic, now) {
        return None;
    }
    let level = crate::miclevel::read(mic);
    let mut raised_to = None;
    if let Some(to) = level.and_then(crate::miclevel::raise_to) {
        if !changes.raised_before(mic) {
            if let Ok(was) = crate::miclevel::set(mic, to) {
                changes.raised.push((mic.to_string(), was, to, now));
                raised_to = Some(to);
            }
        }
    }
    changes.note_told(mic, now);
    Some(crate::leveller::quiet_input_line(&crate::hearing::short(mic), speech, level, raised_to))
}

#[cfg(windows)]
mod win {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eCapture, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

    /// Run `f` on the endpoint volume of the active capture device called
    /// `name`. `None` when there's no such device or COM refuses.
    pub(super) fn with_endpoint<T>(name: &str, f: impl FnOnce(&IAudioEndpointVolume) -> T) -> Option<T> {
        unsafe {
            // Already initialised on this thread (in either model) is fine.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let all = en.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE).ok()?;
            let n = all.GetCount().ok()?;
            for i in 0..n {
                let Ok(dev) = all.Item(i) else { continue };
                let Ok(props) = dev.OpenPropertyStore(STGM_READ) else { continue };
                let Ok(v) = props.GetValue(&PKEY_Device_FriendlyName) else { continue };
                if !crate::miclevel::same_device(name, &v.to_string()) {
                    continue;
                }
                let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
                return Some(f(&vol));
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_low_unmuted_level_is_raised_and_only_up() {
        assert_eq!(raise_to(InputLevel { scalar: 0.04, muted: false }), Some(RAISE_TO));
        assert_eq!(raise_to(InputLevel { scalar: 0.04, muted: true }), None);
        assert_eq!(raise_to(InputLevel { scalar: 0.7, muted: false }), None);
    }

    #[test]
    fn device_names_survive_a_lost_trademark_sign() {
        assert!(same_device("Microphone Array (Intel Smart Sound Technology for Digital Microphones)", "Microphone Array (Intel® Smart Sound Technology for Digital Microphones)"));
        assert!(same_device("Microphone (HD Pro Webcam C920)", "Microphone (HD Pro Webcam C920)"));
        assert!(!same_device("Microphone (HD Pro Webcam C920)", "Microphone Array (Intel® Smart Sound Technology for Digital Microphones)"));
    }
}
