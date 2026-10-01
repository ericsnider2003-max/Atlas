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
    /// Windows' own id for the device (dshow's "Alternative name"), when the
    /// listing gave one: plain ASCII, so it survives what the friendly name
    /// doesn't. `ffmpeg_name` prefers it.
    pub id: Option<String>,
}

impl Device {
    pub fn new(name: &str, kind: Kind) -> Device {
        let n = name.to_lowercase();
        Device {
            bluetooth: looks_bluetooth(&n),
            builtin: looks_builtin(&n),
            name: name.to_string(),
            kind,
            id: None,
        }
    }

    /// What to hand ffmpeg to open this device.
    ///
    /// The id when there is one (29 Sep 2026). On Eric's laptop the listing
    /// came through as "Microphone Array (Intelr Smart Sound ...)": the "\u{ae}"
    /// in "Intel\u{ae}" had been turned into an "r" on its way out of ffmpeg,
    /// and ffmpeg couldn't open the device by the name it had just printed.
    /// The id has no such letters to lose.
    pub fn ffmpeg_name(&self) -> String {
        self.id.clone().unwrap_or_else(|| self.name.clone())
    }
}

/// What to hand ffmpeg for the device called `name` in `devices`: its id
/// when the listing gave one, otherwise the name as given.
pub fn ffmpeg_name_for(devices: &[Device], name: &str) -> String {
    devices
        .iter()
        .find(|d| d.name == name)
        .map(Device::ffmpeg_name)
        .unwrap_or_else(|| name.to_string())
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
    let mut out: Vec<Device> = Vec::new();
    // Whether the last named line was a microphone, so its "Alternative
    // name" is kept with it and a camera's isn't.
    let mut last_was_audio = false;
    for line in text.lines() {
        let l = line.trim();
        // An alternative name is Windows' id for the device just listed:
        // never a device of its own, but the surest way to open it
        // (`Device::ffmpeg_name`).
        if l.contains("Alternative name") {
            if last_was_audio {
                if let (Some(a), Some(b)) = (l.find('"'), l.rfind('"')) {
                    if b > a + 1 {
                        if let Some(d) = out.last_mut() {
                            d.id = Some(l[a + 1..b].to_string());
                        }
                    }
                }
            }
            last_was_audio = false;
            continue;
        }
        last_was_audio = false;
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
        last_was_audio = true;
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

    // A speaker you named wins here too (29 Sep 2026: it was honoured only
    // when you had also named a microphone). Headphones, when preferred,
    // include wired ones, not only Bluetooth.
    let named_out = first_preferred(&outputs, &cfg.preferred_output);
    let ears_out = outputs.iter().find(|d| d.bluetooth || looks_like_headphones(&d.name.to_lowercase()));
    let output = if let Some(d) = named_out {
        Some(d.name.clone())
    } else if cfg.prefer_headphones_for_output {
        ears_out.or_else(|| outputs.first()).map(|d| d.name.clone())
    } else {
        outputs.iter().find(|d| !d.bluetooth).or_else(|| outputs.first()).map(|d| d.name.clone())
    };

    if named_out.is_none() && output.is_some() && ears_out.is_some() && cfg.prefer_headphones_for_output {
        why.push_str(", speaking through your headphones");
    }
    let _ = bt_out;

    Selection { input, output, why, degrades_audio: degrades }
}

/// A speaker you wear: headphones, a headset, earbuds.
fn looks_like_headphones(lower: &str) -> bool {
    ["headphone", "headset", "earphone", "earbud", "buds", "airpods"].iter().any(|k| lower.contains(k))
}

/// The microphone you named in `preferred_input`, if this machine has it.
pub fn preferred_input_of<'a>(devices: &'a [Device], cfg: &AudioConfig) -> Option<&'a Device> {
    let inputs: Vec<&Device> = devices.iter().filter(|d| d.kind == Kind::Input).collect();
    first_preferred(&inputs, &cfg.preferred_input)
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

// ---------------------------------------------------------------------------
// Cameras, from the same listing (29 Sep 2026: `webcam_device` was the
// shipped guess "Integrated Camera", never checked against the machine).
// ---------------------------------------------------------------------------

/// The cameras in a dshow listing, by the name ffmpeg opens them with.
pub fn parse_cameras(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.contains("Alternative name") && l.to_lowercase().contains("(video)"))
        .filter_map(|l| {
            let (a, b) = (l.find('"')?, l.rfind('"')?);
            (b > a + 1).then(|| l[a + 1..b].to_string())
        })
        .collect()
}

/// The camera to use: the configured one when this machine has it, else the
/// built-in one, else any real one (not a virtual camera). `None` when the
/// machine has none.
pub fn pick_camera(cameras: &[String], configured: &str) -> Option<String> {
    if let Some(c) = cameras.iter().find(|c| c.as_str() == configured) {
        return Some(c.clone());
    }
    let low = |c: &String| c.to_lowercase();
    let virtual_cam = |c: &String| ["virtual", "obs", "snap camera", "droidcam", "nvidia broadcast"].iter().any(|v| low(c).contains(v));
    let real: Vec<&String> = cameras.iter().filter(|c| !virtual_cam(c)).collect();
    real.iter()
        .find(|c| ["integrated", "built-in", "front", "facetime", "user facing"].iter().any(|k| low(c).contains(k)))
        .or_else(|| real.first())
        .map(|c| (*c).clone())
}

/// The camera to look through, knowing more about the desk: the lid (a
/// laptop's own camera under a shut lid sees its keyboard) and the
/// microphone Atlas hears you with (the webcam whose microphone hears you is
/// the one pointed at you).
///
/// 30 Sep 2026: Eric's laptop is shut behind two monitors with a C920 on
/// top, and `webcam_device` is the shipped "Integrated Camera". `pick_camera`
/// keeps a configured camera the machine has, so a look would have opened
/// the camera inside the shut lid -- a black frame. Order: the camera that
/// matches the microphone in use; any real camera but a built-in one when the
/// lid is shut; then `pick_camera`.
pub fn pick_camera_for(cameras: &[String], configured: &str, microphone: &str, lid_open: bool) -> Option<String> {
    let low = |c: &str| c.to_lowercase();
    let builtin = |c: &str| ["integrated", "built-in", "internal", "front", "facetime", "user facing"].iter().any(|k| low(c).contains(k));
    let virtual_cam = |c: &str| ["virtual", "obs", "snap camera", "droidcam", "nvidia broadcast"].iter().any(|v| low(c).contains(v));
    let usable: Vec<&String> = cameras.iter().filter(|c| !virtual_cam(c) && (lid_open || !builtin(c))).collect();
    // "Microphone (HD Pro Webcam C920)" -> the words inside the brackets
    // that name the device, matched against each camera's name.
    let mic = low(microphone);
    let inside = mic.split_once('(').map(|(_, r)| r.trim_end_matches(')').to_string()).unwrap_or_default();
    let tokens: Vec<&str> = inside.split_whitespace().filter(|w| w.len() >= 4 && !["microphone", "array", "audio", "webcam"].contains(w)).collect();
    if !tokens.is_empty() {
        if let Some(c) = usable.iter().find(|c| tokens.iter().all(|t| low(c).contains(t))) {
            return Some((*c).clone());
        }
    }
    if !lid_open {
        if let Some(c) = usable.iter().find(|c| c.as_str() == configured).or_else(|| usable.first()) {
            return Some((*c).clone());
        }
    }
    pick_camera(cameras, configured)
}

/// The cameras this machine has (Windows' listing; empty elsewhere, where
/// the camera is named differently and `webcam_device` stands).
pub fn probe_cameras(ffmpeg_cmd: &str) -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let (_, args) = listing_command(true);
    match crate::tools::command(ffmpeg_cmd)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(o) => parse_cameras(&String::from_utf8_lossy(&o.stderr)),
        Err(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Is there anyone speaking in this clip? (29 Sep 2026)
//
// Whisper, handed a clip of a quiet room, writes something anyway -- most
// often "you", sometimes "Thank you." or "Thanks for watching". On Eric's
// laptop that turned every silent follow-up listen into a turn: Atlas
// answered "you", listened again, heard the room, got "you" again, and went
// round for as long as it ran, with the model and the transcriber working
// the whole time. And the wake-word listener ran whisper on every two-second
// clip of an empty room. A clip with no speech in it is now silence, and
// whisper is never asked about it; a clip with quiet speech in it is turned
// up first.
// ---------------------------------------------------------------------------

/// What a clip holds, for speech-to-text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpeechCheck {
    /// Nobody speaking: don't transcribe it.
    Silence,
    /// Speech, loud enough as it is.
    Speech,
    /// Speech, but quiet: multiply by this before transcribing.
    Quiet(f32),
}

/// How much speech a clip needs before it is transcribed.
pub const MIN_SPEECH_MS: u32 = 250;
/// Speech stands this far above the clip's own quiet parts.
pub const SPEECH_OVER_ROOM_DB: f32 = 9.0;
/// Below this nothing is speech, however quiet the room: just above the
/// dither of a dead or muted device (about -90 dBFS), so zeros and stray
/// clicks are never transcribed.
///
/// Was -62 (29 Sep 2026), and that line is why Eric had to shout (30 Sep
/// 2026): his webcam microphone read -90 dB in a quiet room, so a normal
/// voice on it at -68 dB, 22 dB clear of the room, was thrown away as
/// silence (`tests/a_normal_voice_is_heard.rs`). Speech is told from the room by how far it stands above
/// it (`SPEECH_OVER_ROOM_DB`), which is the test that holds on any
/// microphone at any input level; this only guards the dead device.
pub const QUIETEST_SPEECH_DB: f32 = -75.0;
/// Speech whose loudest part is under this is turned up.
pub const TURN_UP_BELOW_DB: f32 = -28.0;

/// Look at `samples` (at `rate`) the way speech is looked for: 30 ms frames,
/// the room's level taken from the quietest fifth of them, speech as frames
/// well above it. A clip too short to judge is given the benefit of the doubt.
pub fn check_speech(samples: &[i16], rate: u32) -> SpeechCheck {
    let frame = (rate as usize * 30 / 1000).max(1);
    let levels: Vec<f32> = samples.chunks(frame).filter(|c| c.len() == frame).map(level_db).collect();
    if levels.len() < 4 {
        return SpeechCheck::Speech;
    }
    let mut sorted = levels.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let room = sorted[sorted.len() / 5];
    let loudest = *sorted.last().unwrap_or(&SILENT_DB);
    let bar = (room + SPEECH_OVER_ROOM_DB).max(QUIETEST_SPEECH_DB);
    let speech_frames = levels.iter().filter(|l| **l >= bar).count() as u32;
    if speech_frames * 30 < MIN_SPEECH_MS {
        return SpeechCheck::Silence;
    }
    if loudest < TURN_UP_BELOW_DB {
        // Up to about -12 dBFS at its loudest, never more than 100x
        // (`leveller::MAX_GAIN_DB`), and never lifting the room past
        // `leveller::NOISE_CEILING_DB`. Was capped at 30x, which left a voice
        // at -65 dB still at -35 when whisper heard it (30 Sep 2026).
        let up_db = (-12.0 - loudest).min(crate::leveller::NOISE_CEILING_DB - room).min(crate::leveller::MAX_GAIN_DB);
        let gain = 10f32.powf(up_db / 20.0).max(1.0);
        return SpeechCheck::Quiet(gain);
    }
    SpeechCheck::Speech
}

/// `samples` turned up by `gain`, clipped rather than wrapped.
pub fn turned_up(samples: &[i16], gain: f32) -> Vec<i16> {
    samples.iter().map(|s| ((*s as f32) * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16).collect()
}
