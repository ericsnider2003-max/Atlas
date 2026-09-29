//! Choosing which microphone and which speakers.
//!
//! This looks trivial and isn't, because of one hardware fact:
//!
//! > **A Bluetooth headset cannot play high quality audio and record at the
//! > same time.** The moment anything opens its microphone, the connection
//! > drops from A2DP to a headset profile — mono, roughly 8–16kHz, and the
//! > music you were listening to becomes muddy.
//!
//! So an assistant that naively grabs "the AirPods mic" quietly wrecks
//! everything else you're listening to, for as long as it's listening. With a
//! wake word running, that's all day.
//!
//! Atlas's default is therefore to **listen on the laptop's own microphone and
//! speak through your headphones.** You get private replies without the codec
//! collapse. It only uses the headset mic when there's nothing else, or when
//! you tell it to — walking around, say, where the laptop mic can't hear you.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// Exactly as the system names it. ffmpeg needs this verbatim.
    pub name: String,
    pub kind: Kind,
    pub bluetooth: bool,
    /// Built into the machine, rather than plugged in or paired.
    pub builtin: bool,
}

impl Device {
    pub fn new(name: &str, kind: Kind) -> Device {
        let n = name.to_lowercase();
        Device {
            bluetooth: looks_bluetooth(&n),
            builtin: looks_builtin(&n),
            name: name.to_string(),
            kind,
        }
    }
}

fn looks_bluetooth(n: &str) -> bool {
    const HINTS: &[&str] = &[
        "airpods", "bluetooth", "hands-free", "handsfree", "headset",
        "buds", "beats", "wh-", "wf-", "jabra", "bose", "sony wh", "soundcore",
    ];
    HINTS.iter().any(|h| n.contains(h))
}

fn looks_builtin(n: &str) -> bool {
    const HINTS: &[&str] = &[
        // Windows names.
        "microphone array", "internal", "built-in", "realtek", "intel smart sound", "laptop",
        // Linux and macOS name the same hardware differently, and the list
        // was written when this only ever saw Windows dshow output. ALSA
        // reports the onboard codec as "HDA Intel PCH"; macOS says
        // "Built-in Microphone" (already covered) but also "MacBook Pro
        // Microphone".
        "hda intel", "hda-intel", "pch", "onboard", "macbook",
    ];
    HINTS.iter().any(|h| n.contains(h))
}

/// Parse `ffmpeg -list_devices true -f dshow -i dummy` output.
///
/// The names must come out byte-exact, quotes stripped and nothing else
/// touched — ffmpeg matches them literally, so one wrong character means
/// silence rather than an error.
pub fn parse_devices(text: &str) -> Vec<Device> {
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        // Alternative names are internal ids, not what you pass back in.
        if l.contains("Alternative name") {
            continue;
        }
        let Some(start) = l.find('"') else { continue };
        let Some(end) = l.rfind('"') else { continue };
        if end <= start + 1 {
            continue;
        }
        let name = &l[start + 1..end];
        let lower = l.to_lowercase();
        let kind = if lower.contains("(audio)") {
            Kind::Input
        } else if lower.contains("(video)") {
            continue; // a camera, not a microphone
        } else {
            continue;
        };
        out.push(Device::new(name, kind));
    }
    out
}

/// Parse `ffmpeg -sources alsa` / `ffmpeg -sinks alsa` output (Linux).
///
/// A different shape from dshow entirely — no quotes, and the machine-usable
/// name comes *first* on the line with a human label in brackets after it:
///
/// ```text
/// Auto-detected sources for alsa:
///   hw:CARD=PCH,DEV=0 [HDA Intel PCH] (Some description)
///   default [Default Audio Device]
/// ```
///
/// The bare name is what ffmpeg takes back, so that is what is kept; the
/// bracketed label is what a person recognises, so it is what the name is
/// classified on. `null` is ffmpeg's own discard device and is dropped —
/// offering it as a microphone would give you a working setup that records
/// silence, which is the worst failure this file has.
pub fn parse_alsa(text: &str, kind: Kind) -> Vec<Device> {
    let mut out = Vec::new();
    for line in text.lines() {
        // Header lines are flush left; devices are indented.
        if !line.starts_with(' ') && !line.starts_with('\t') {
            continue;
        }
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let name = l.split_whitespace().next().unwrap_or("");
        if name.is_empty() || name == "null" {
            continue;
        }
        // Classify on the human label when there is one -- "hw:CARD=PCH" says
        // nothing about whether it is built in, and "[HDA Intel PCH]" does.
        let label = match (l.find('['), l.rfind(']')) {
            (Some(a), Some(b)) if b > a + 1 => &l[a + 1..b],
            _ => name,
        };
        let mut d = Device::new(label, kind);
        d.name = name.to_string();
        out.push(d);
    }
    out
}

/// Parse `ffmpeg -f avfoundation -list_devices true -i ""` output (macOS).
///
/// ```text
/// [AVFoundation indev @ 0x7f8] AVFoundation video devices:
/// [AVFoundation indev @ 0x7f8] [0] FaceTime HD Camera
/// [AVFoundation indev @ 0x7f8] AVFoundation audio devices:
/// [AVFoundation indev @ 0x7f8] [0] Built-in Microphone
/// ```
///
/// Cameras and microphones are in one listing separated only by a heading,
/// so the heading has to be tracked -- taking every `[n] name` line would
/// offer you the FaceTime camera as a microphone.
pub fn parse_avfoundation(text: &str) -> Vec<Device> {
    let mut out = Vec::new();
    let mut in_audio = false;
    for line in text.lines() {
        let l = line.trim();
        // The *first* `]` closes ffmpeg's own "[AVFoundation indev @ 0x7f9]"
        // prefix. Using the last one instead eats the device's index too and
        // leaves a line that no longer looks like a device at all -- which is
        // the bug this comment exists because of.
        let after = if l.starts_with('[') {
            l.find(']').map(|i| l[i + 1..].trim()).unwrap_or(l)
        } else {
            l
        };
        if after.contains("video devices") {
            in_audio = false;
            continue;
        }
        if after.contains("audio devices") {
            in_audio = true;
            continue;
        }
        if !in_audio {
            continue;
        }
        // What is left is "[0] Built-in Microphone" -- strip the index.
        let Some(close) = after.find(']') else { continue };
        if !after.starts_with('[') {
            continue;
        }
        let name = after[close + 1..].trim();
        if name.is_empty() {
            continue;
        }
        out.push(Device::new(name, Kind::Input));
    }
    out
}

/// The ffmpeg invocation that lists devices on this platform.
///
/// Three different answers, which is why nothing called `parse_devices` for
/// so long: it only ever understood the Windows one, and Atlas is
/// cross-platform.
pub fn listing_command(inputs: bool) -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        // dshow lists inputs and outputs together and exits non-zero doing
        // it; the caller reads stderr regardless.
        ("ffmpeg", ["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"]
            .iter()
            .map(|s| s.to_string())
            .collect())
    } else if cfg!(target_os = "macos") {
        ("ffmpeg", ["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""]
            .iter()
            .map(|s| s.to_string())
            .collect())
    } else {
        let what = if inputs { "-sources" } else { "-sinks" };
        ("ffmpeg", ["-hide_banner", what, "alsa"].iter().map(|s| s.to_string()).collect())
    }
}

/// Parse whichever listing this platform produces.
pub fn parse_listing(text: &str, inputs: bool) -> Vec<Device> {
    let kind = if inputs { Kind::Input } else { Kind::Output };
    if cfg!(windows) {
        // dshow's own listing distinguishes audio from video but not input
        // from output, so an output request gets nothing rather than a
        // confident wrong list.
        if inputs { parse_devices(text) } else { Vec::new() }
    } else if cfg!(target_os = "macos") {
        if inputs { parse_avfoundation(text) } else { Vec::new() }
    } else {
        parse_alsa(text, kind)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Microphones in order of preference. First match wins.
    pub preferred_input: Vec<String>,
    /// Speakers in order of preference.
    pub preferred_output: Vec<String>,
    /// Keep off the Bluetooth microphone while a wired or built-in one exists.
    ///
    /// Setting this false means "use the headset mic" — the only reason to
    /// turn the guard off is that you're away from the desk and the laptop
    /// can't hear you, so it switches preference rather than merely allowing.
    pub avoid_bluetooth_mic: bool,
    /// Speak through headphones when they're connected.
    pub prefer_headphones_for_output: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig {
            preferred_input: Vec::new(),
            preferred_output: Vec::new(),
            avoid_bluetooth_mic: true,
            prefer_headphones_for_output: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub input: Option<String>,
    pub output: Option<String>,
    /// Why these, in a sentence — so a surprising choice is explainable.
    pub why: String,
    /// True when Atlas had to take the headset mic, which will degrade
    /// whatever you're listening to.
    pub degrades_audio: bool,
}

/// Pick the best microphone and speaker from what is actually plugged in.
///
/// `laptop_screen_active` is the one piece of physical-world context that
/// changes everything: whether the machine's own screen is part of the
/// current monitor layout right now. A closed lid does not always stop
/// Windows from still listing the internal microphone array — the OS can
/// keep enumerating a device that is sealed inside a shut clamshell — so
/// "the device still shows up" is not evidence it is usable. Atlas already
/// tracks which monitor is the laptop's own screen for window placement; that
/// same fact is repurposed here rather than inventing a new, less reliable
/// way to ask Windows whether the lid is down.
///
/// When the screen is not active, every built-in device is treated as
/// unreachable and dropped before any other reasoning runs — not
/// deprioritised, excluded, because a muffled recording sealed inside a
/// laptop is not a fallback worth offering.
pub fn choose(devices: &[Device], cfg: &AudioConfig, laptop_screen_active: bool) -> Selection {
    let reachable: Vec<&Device> = if laptop_screen_active {
        devices.iter().collect()
    } else {
        devices.iter().filter(|d| !d.builtin).collect()
    };
    let inputs: Vec<&Device> = reachable.iter().filter(|d| d.kind == Kind::Input).copied().collect();
    let outputs: Vec<&Device> = reachable.iter().filter(|d| d.kind == Kind::Output).copied().collect();

    // An explicit preference always wins — you know your setup better.
    if let Some(d) = first_preferred(&inputs, &cfg.preferred_input) {
        let out = first_preferred(&outputs, &cfg.preferred_output).map(|o| o.name.clone());
        return Selection {
            degrades_audio: d.bluetooth,
            input: Some(d.name.clone()),
            output: out,
            why: "you named these".into(),
        };
    }

    let wired_mic = inputs.iter().find(|d| !d.bluetooth);
    let bt_mic = inputs.iter().find(|d| d.bluetooth);
    let bt_out = outputs.iter().find(|d| d.bluetooth);

    // Whether the built-in array was actually dropped from consideration --
    // not merely whether the screen is inactive, since a lid-down laptop with
    // no built-in mic in its device list at all has nothing to explain.
    let lid_excluded = !laptop_screen_active && devices.iter().any(|d| d.builtin && d.kind == Kind::Input);

    // The lid-down reasoning has to be decided before the ordinary
    // avoid-Bluetooth logic runs, not layered on top of it -- the first
    // version of this checked lid_excluded only on some match arms, so a
    // wired mic found *because* the lid excluded the built-in one still
    // reported "listening on the laptop mic", which was not what happened.
    let (input, degrades, mut why) = if lid_excluded {
        match (wired_mic, bt_mic) {
            (Some(w), _) => (
                Some(w.name.clone()),
                false,
                "the laptop's own mic isn't reachable with the lid down, so I'm using this instead"
                    .to_string(),
            ),
            (None, Some(b)) => (
                Some(b.name.clone()),
                true,
                "the laptop's own mic isn't reachable with the lid down, and this is the only \
                 other one here — audio quality will drop while I listen"
                    .to_string(),
            ),
            (None, None) => (None, false, "no microphone found".to_string()),
        }
    } else {
        match (cfg.avoid_bluetooth_mic, wired_mic, bt_mic) {
            // The case this module exists for.
            (true, Some(w), Some(_)) => (
                Some(w.name.clone()),
                false,
                "listening on the laptop mic so your headphones keep full sound quality".to_string(),
            ),
            (_, Some(w), None) => (Some(w.name.clone()), false, "the only microphone here".to_string()),
            (_, _, Some(b)) => (
                Some(b.name.clone()),
                true,
                "using the headset mic — it will drop your audio quality while I listen".to_string(),
            ),
            (_, None, None) => (None, false, "no microphone found".to_string()),
        }
    };

    let output = if cfg.prefer_headphones_for_output {
        bt_out.or_else(|| outputs.first()).map(|d| d.name.clone())
    } else {
        outputs.iter().find(|d| !d.bluetooth).or_else(|| outputs.first()).map(|d| d.name.clone())
    };

    if output.is_some() && bt_out.is_some() && cfg.prefer_headphones_for_output {
        why.push_str(", speaking through your headphones");
    }

    Selection { input, output, why, degrades_audio: degrades }
}

fn first_preferred<'a>(devices: &[&'a Device], preferred: &[String]) -> Option<&'a Device> {
    for want in preferred {
        let w = want.to_lowercase();
        if let Some(d) = devices.iter().find(|d| d.name.to_lowercase().contains(&w)) {
            return Some(d);
        }
    }
    None
}

/// Did the available audio change? Headphones connecting mid-session is the
/// common case, and re-selecting silently is better than making you restart.
pub fn changed(before: &[Device], after: &[Device]) -> bool {
    let names = |v: &[Device]| {
        let mut n: Vec<String> = v.iter().map(|d| d.name.clone()).collect();
        n.sort();
        n
    };
    names(before) != names(after)
}

/// What Atlas says when the audio setup changes under it.
pub fn announce(before: &Selection, after: &Selection) -> Option<String> {
    if before == after {
        return None;
    }
    match (&after.input, after.degrades_audio) {
        (None, _) => Some("I've lost the microphone.".into()),
        (Some(_), true) => {
            Some("Switched to the headset mic — your audio quality will drop while I listen.".into())
        }
        (Some(_), false) if before.input.is_none() => Some("Microphone's back.".into()),
        _ => Some(format!("Audio changed: {}.", after.why)),
    }
}

/// Ask Windows what audio devices exist, right now.
///
/// This is the one function in the module that touches a process. `dummy` is
/// not a real input — dshow's device-listing mode only works by attempting to
/// open a bogus source and reporting every real one on its way to failing, so
/// a non-zero exit here is normal and expected, not an error to surface.
pub fn probe_devices(ffmpeg_cmd: &str) -> crate::error::Result<Vec<Device>> {
    probe(ffmpeg_cmd, true)
}

/// Ask the system what it has, inputs or outputs.
///
/// This used to be Windows-only, and silently. It ran
/// `ffmpeg -list_devices true -f dshow` on every platform; on Linux and
/// macOS that fails, `voice_loop` prints "couldn't list audio devices" and
/// falls back to whatever device name is sitting in `tools.yaml` -- which is
/// a Windows device name, so it also doesn't work. The result was that mic
/// auto-detection, the whole point of which is not trusting that guess, had
/// never once run anywhere but Windows.
///
/// Confusingly, the outstanding-task list recorded the opposite problem:
/// "microphone enumeration is unimplemented, `audio::parse_devices` is
/// correct and no OS command calls it". A command did call it. What was
/// missing was every platform except one.
///
/// Both streams are read and the exit status ignored on purpose: each of
/// these commands writes its listing to stderr, and dshow and avfoundation
/// exit non-zero while doing it.
pub fn probe(ffmpeg_cmd: &str, inputs: bool) -> crate::error::Result<Vec<Device>> {
    use std::process::Stdio;
    let (_, args) = listing_command(inputs);
    let out = crate::tools::command(ffmpeg_cmd)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            crate::error::AtlasError::Platform(format!(
                "could not run '{ffmpeg_cmd}' to list audio devices: {e}"
            ))
        })?;
    let mut text = String::from_utf8_lossy(&out.stderr).to_string();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&out.stdout));
    Ok(parse_listing(&text, inputs))
}

// ---------------------------------------------------------------------------
// Listening to the stream rather than to a stopwatch.
//
// `endpoint.rs` decides when you have stopped talking, and it decides from
// the loudness of short windows of audio. Nothing produced those windows:
// the record command writes a fixed-length wav and Atlas waits for it, which
// is the eight-second problem that module opens by describing.
//
// What follows is the missing half, and it is deliberately small. Raw 16-bit
// mono PCM in, a number in dBFS out, and a wav header when the clip needs to
// go to whisper. No dependency, because a decoder is not needed for audio
// this program specified the format of itself.
// ---------------------------------------------------------------------------

/// One window of audio, as loud as it was, in dBFS.
///
/// dBFS rather than a raw amplitude because that is what a person can reason
/// about and what `EndpointConfig::silence_below_db` is written in: 0 is as
/// loud as the format can represent, quiet speech is around -30, a silent
/// room somewhere below -50.
///
/// RMS rather than peak. A single click in an otherwise silent window is not
/// speech, and peak would call it speech; RMS is the average energy, which is
/// what "is someone talking" actually depends on.
pub fn level_db(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        // Not silence — the absence of a measurement. Returning 0 here (the
        // loudest value there is) would read as speech and hold a recording
        // open forever on a dead microphone.
        return SILENT_DB;
    }
    // f64 for the sum: 16-bit samples squared reach ~1.07e9 each, and a
    // second of audio at 16kHz would overflow an f32's precision long before
    // it overflowed its range, quietly biasing the answer low.
    let sum: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    let rms = (sum / samples.len() as f64).sqrt();
    if rms <= 0.0 {
        return SILENT_DB;
    }
    // i16::MAX is full scale.
    let db = 20.0 * (rms / i16::MAX as f64).log10();
    (db as f32).max(SILENT_DB)
}

/// The floor. Digital silence is negative infinity dB, which is not a number
/// any threshold comparison handles well, so it is clamped to something far
/// below any real room.
pub const SILENT_DB: f32 = -96.0;

/// Split raw little-endian 16-bit mono PCM into samples.
///
/// A trailing odd byte is dropped rather than being half-read into a sample:
/// a stream read can land mid-sample, and half a sample is noise at an
/// arbitrary amplitude, which is exactly the thing that would be mistaken
/// for speech.
pub fn samples_from_le(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

/// How many samples make up one window of this many milliseconds.
pub fn window_samples(rate_hz: u32, window_ms: u64) -> usize {
    ((rate_hz as u64 * window_ms) / 1000) as usize
}

/// Wrap raw PCM in the wav header whisper expects.
///
/// Written by hand, and it is forty-four bytes. Everything here is known
/// exactly — mono, 16-bit, the rate Atlas asked ffmpeg for — so a general
/// encoder would be a dependency to keep current forever in exchange for
/// nothing.
pub fn wav_bytes(samples: &[i16], rate_hz: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM header size
    out.extend_from_slice(&1u16.to_le_bytes()); // uncompressed
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate_hz.to_le_bytes());
    out.extend_from_slice(&(rate_hz * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// The ffmpeg invocation that streams raw PCM to stdout rather than writing a
/// fixed-length file.
///
/// The `-t` in the configured record command is the eight-second stopwatch.
/// This has no `-t` at all: it runs until Atlas stops it, which is what lets
/// `endpoint` be the thing that decides.
///
/// `hard_stop_secs` is not that stopwatch coming back. It is the backstop for
/// a microphone that has stuck open, an order of magnitude longer than a
/// sentence, and `endpoint` has its own `hard_stop_ms` for the same reason —
/// belt and braces on the one failure that costs disk rather than patience.
pub fn stream_args(device: &str, rate_hz: u32, hard_stop_secs: u32) -> Vec<String> {
    let mut a: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into()];
    if cfg!(windows) {
        a.push("-f".into());
        a.push("dshow".into());
        a.push("-i".into());
        a.push(format!("audio={device}"));
    } else if cfg!(target_os = "macos") {
        a.push("-f".into());
        a.push("avfoundation".into());
        a.push("-i".into());
        a.push(format!(":{device}"));
    } else {
        a.push("-f".into());
        a.push("alsa".into());
        a.push("-i".into());
        a.push(device.to_string());
    }
    a.extend([
        "-ar".into(),
        rate_hz.to_string(),
        "-ac".into(),
        "1".into(),
        "-t".into(),
        hard_stop_secs.to_string(),
        "-f".into(),
        "s16le".into(),
        "-".into(),
    ]);
    a
}
