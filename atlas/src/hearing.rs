//! Which ear Atlas listens with, decided automatically.
//!
//! The situation this exists for: a laptop closed on a stand behind the
//! monitors, so its own microphone array is muffled and half-blocked. A webcam
//! sitting on the monitor with a clear line to your face. AirPods that hear
//! you anywhere but cost audio quality while they listen. And a phone that can
//! hear you in another room entirely.
//!
//! No single one of those is right. So Atlas keeps all of them and picks, and
//! the rules are:
//!
//! 1. **Never make you choose.** It measures which microphone actually hears
//!    you rather than trusting a name.
//! 2. **Only pay the Bluetooth cost when it buys something.** At the desk with
//!    a webcam that hears you, the AirPods stay on full-quality playback.
//! 3. **Follow you.** Away from the desk it switches to the headset; out of
//!    the room, to the phone; back at the desk, back again.
//! 4. **Don't flap.** Switching ears mid-sentence is worse than a slightly
//!    worse microphone, so a change has to be clearly better and has to last.

use crate::audio::{Device, Kind};
use serde::{Deserialize, Serialize};

/// Quieter than this is digital silence: a muted, covered-off or dead
/// microphone, not a quiet room (which reads about -50 to -70 dB).
pub const SILENCE_DB: f32 = -80.0;

/// Where Atlas is listening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ear {
    /// A microphone at the desk — webcam, desk mic, laptop array.
    Desk(String),
    /// A headset you're wearing.
    Headset(String),
    /// Your phone, over the local API.
    Phone,
    /// Nothing can hear you.
    Deaf,
}

impl Ear {
    pub fn name(&self) -> String {
        match self {
            Ear::Desk(n) | Ear::Headset(n) => n.clone(),
            Ear::Phone => "your phone".into(),
            Ear::Deaf => "nothing".into(),
        }
    }
    /// Does listening on this degrade what you're hearing?
    fn costs_quality(&self) -> bool {
        matches!(self, Ear::Headset(_))
    }
}

/// What Atlas knows about a microphone from actually trying it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub name: String,
    pub bluetooth: bool,
    /// Mean level in dBFS from a short sample. Closer to zero is louder.
    /// None means never measured.
    pub measured_db: Option<f32>,
    /// Turns that produced usable speech through this device.
    pub good_turns: u32,
    /// Turns that produced silence or nonsense.
    pub bad_turns: u32,
}

impl Candidate {
    pub fn from(d: &Device) -> Candidate {
        Candidate {
            name: d.name.clone(),
            bluetooth: d.bluetooth,
            measured_db: None,
            good_turns: 0,
            bad_turns: 0,
        }
    }

    /// Did it hear anything at all? Below this is a muffled or blocked mic —
    /// exactly the closed-laptop case.
    fn hears_you(&self, floor_db: f32) -> bool {
        self.measured_db.map(|db| db > floor_db).unwrap_or(false)
    }

    /// How much to trust it, from measurement and from history.
    pub fn score(&self, floor_db: f32) -> f32 {
        let level = match self.measured_db {
            // Map -60..0 dBFS onto 0..1.
            Some(db) => ((db - floor_db) / -floor_db).clamp(0.0, 1.0),
            None => 0.4, // untried: worth a go, but not preferred over proven
        };
        let attempts = self.good_turns + self.bad_turns;
        let reliability = if attempts == 0 {
            0.5
        } else {
            self.good_turns as f32 / attempts as f32
        };
        level * 0.6 + reliability * 0.4
    }
}

/// What's true right now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Where {
    /// The camera says you're at the desk.
    pub at_desk: bool,
    /// Presence is unavailable, so don't infer anything from its absence.
    pub presence_unknown: bool,
    /// A headset is paired and connected.
    pub headset_connected: bool,
    /// The phone has spoken to Atlas recently.
    pub phone_active: bool,
    /// You're playing something you'd notice degrading.
    pub audio_playing: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HearingConfig {
    /// Below this level a microphone counts as not hearing you.
    pub floor_db: f32,
    /// A new ear must beat the current one by this much to be worth switching.
    pub switch_margin: f32,
    /// And the situation must have held this long.
    pub settle_secs: u64,
    /// Re-measure the microphones this often.
    pub recalibrate_secs: u64,
}

impl Default for HearingConfig {
    fn default() -> Self {
        HearingConfig {
            floor_db: -45.0,
            switch_margin: 0.15,
            settle_secs: 20,
            recalibrate_secs: 6 * 3600,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub ear: Ear,
    /// One sentence, so a surprising switch is explainable.
    pub why: String,
    /// True when this is costing you audio quality.
    pub costs_quality: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hearing {
    pub candidates: Vec<Candidate>,
    pub current: Option<Ear>,
    switched_at: u64,
    wanted: Option<Ear>,
    wanted_since: u64,
    pub last_calibration: u64,
    /// The microphone you asked for by name ("use my webcam mic", 29 Sep
    /// 2026): kept to while it is plugged in, whatever the scores say.
    #[serde(default)]
    pub chosen: Option<String>,
}

impl Hearing {
    /// Remembered across runs, so measuring which microphone hears you is
    /// something that happens rarely rather than at every start.
    pub fn load_from(store: &crate::store::Store) -> Hearing {
        store.load("hearing")
    }

    pub fn save_to(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save("hearing", self)
    }

    /// Keep to this microphone from now on (`pick_microphone`).
    pub fn choose(&mut self, name: &str) {
        self.chosen = Some(name.to_string());
        self.current = Some(Ear::Desk(name.to_string()));
    }

    pub fn observe_devices(&mut self, devices: &[Device]) {
        let inputs: Vec<&Device> = devices.iter().filter(|d| d.kind == Kind::Input).collect();
        // Keep what we've learned about devices that are still here.
        self.candidates.retain(|c| inputs.iter().any(|d| d.name == c.name));
        for d in inputs {
            if !self.candidates.iter().any(|c| c.name == d.name) {
                self.candidates.push(Candidate::from(d));
            }
        }
    }

    pub fn record_level(&mut self, name: &str, db: f32, t: u64) {
        if let Some(c) = self.candidates.iter_mut().find(|c| c.name == name) {
            c.measured_db = Some(db);
        }
        self.last_calibration = t;
    }

    /// A turn came through this ear and produced usable speech — or didn't.
    pub fn record_turn(&mut self, ear: &Ear, understood: bool) {
        let name = match ear {
            Ear::Desk(n) | Ear::Headset(n) => n.clone(),
            _ => return,
        };
        if let Some(c) = self.candidates.iter_mut().find(|c| c.name == name) {
            if understood {
                c.good_turns += 1;
            } else {
                c.bad_turns += 1;
            }
        }
    }

    pub fn needs_calibration(&self, cfg: &HearingConfig, t: u64) -> bool {
        self.candidates.iter().any(|c| c.measured_db.is_none())
            || t.saturating_sub(self.last_calibration) >= cfg.recalibrate_secs
    }

    /// The best desk microphone — anything not Bluetooth.
    fn best_desk(&self, cfg: &HearingConfig) -> Option<&Candidate> {
        self.candidates
            .iter()
            .filter(|c| !c.bluetooth && c.hears_you(cfg.floor_db))
            .max_by(|a, b| {
                a.score(cfg.floor_db).partial_cmp(&b.score(cfg.floor_db)).unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// The loudest desk microphone that gives any real sound at all, when
    /// none clears the floor. Below `SILENCE_DB` is a muted or dead device.
    fn faint_desk(&self) -> Option<&Candidate> {
        self.candidates
            .iter()
            .filter(|c| !c.bluetooth && c.measured_db.is_some_and(|db| db > SILENCE_DB))
            .max_by(|a, b| a.measured_db.partial_cmp(&b.measured_db).unwrap_or(std::cmp::Ordering::Equal))
    }

    fn headset(&self) -> Option<&Candidate> {
        self.candidates.iter().find(|c| c.bluetooth)
    }

    /// What Atlas would ideally listen with, before hysteresis.
    fn ideal(&self, w: &Where, cfg: &HearingConfig) -> Choice {
        // The phone is talking to Atlas, so the phone is the microphone.
        if w.phone_active {
            return Choice {
                ear: Ear::Phone,
                why: "you're talking to me from your phone".into(),
                costs_quality: false,
            };
        }

        let desk = self.best_desk(cfg);
        let headset = self.headset();
        // Treat "presence unknown" as at the desk, so a covered webcam doesn't
        // silently move Atlas to the headset.
        let probably_here = w.at_desk || w.presence_unknown;

        match (probably_here, desk, headset, w.headset_connected) {
            // At the desk with something that actually hears you: use it, and
            // leave the headset on full-quality playback.
            (true, Some(d), _, _) => Choice {
                ear: Ear::Desk(d.name.clone()),
                why: if w.headset_connected {
                    format!("{} hears you well, so your headphones keep full sound", short(&d.name))
                } else {
                    format!("{} hears you best", short(&d.name))
                },
                costs_quality: false,
            },
            // Away, wearing the headset. Now the quality cost buys something.
            (false, _, Some(h), true) => Choice {
                ear: Ear::Headset(h.name.clone()),
                why: "you're away from the desk, so I'm listening through your headphones".into(),
                costs_quality: true,
            },
            // At the desk but nothing here can hear you — the closed-laptop
            // case. The headset is the only option left.
            (true, None, Some(h), true) => Choice {
                ear: Ear::Headset(h.name.clone()),
                why: "no desk microphone is picking you up, so I'm using your headset".into(),
                costs_quality: true,
            },
            // Away with no headset. Say so rather than pretending.
            (false, _, _, false) => Choice {
                ear: Ear::Deaf,
                why: "you're away and I've no way to hear you — talk to me from your phone".into(),
                costs_quality: false,
            },
            // Nothing clears the floor, but something gives real sound: that
            // one, said honestly, rather than nothing (29 Sep 2026). The
            // start-up test hears a second of the room, not your voice, and on
            // Eric's laptop a quiet room on the working Intel microphone read
            // -51.6 dB against a -45 floor: every microphone was ruled out
            // and Atlas listened to nothing at all. Only digital silence (a
            // muted or dead device, about -90) means it truly can't hear.
            (true, None, None, _) if self.faint_desk().is_some() => {
                let d = self.faint_desk().expect("just checked");
                Choice {
                    ear: Ear::Desk(d.name.clone()),
                    why: format!("{} is the only microphone picking anything up, and only faintly", short(&d.name)),
                    costs_quality: false,
                }
            }
            (_, None, None, _) => Choice {
                ear: Ear::Deaf,
                why: "no microphone can hear you".into(),
                costs_quality: false,
            },
            (_, None, Some(h), _) => Choice {
                ear: Ear::Headset(h.name.clone()),
                why: "the only microphone that hears you".into(),
                costs_quality: true,
            },
            // Away, no headset paired, but a desk mic exists. It cannot hear
            // you from another room, so say so rather than pretending.
            (false, Some(_), None, _) => Choice {
                ear: Ear::Deaf,
                why: "you are away from the desk and only the desk mic is available".into(),
                costs_quality: false,
            },
        }
    }

    /// Decide, with hysteresis.
    ///
    /// A change has to be clearly better *and* has to have held for a while.
    /// Switching ears mid-sentence is worse than a slightly worse microphone,
    /// and presence detection flickers.
    pub fn decide(&mut self, w: &Where, cfg: &HearingConfig, t: u64) -> Choice {
        let ideal = self.ideal(w, cfg);

        let Some(current) = self.current.clone() else {
            self.current = Some(ideal.ear.clone());
            self.switched_at = t;
            self.wanted = None;
            return ideal;
        };

        if ideal.ear == current {
            self.wanted = None;
            return self.describe(&current, w, cfg);
        }

        // Losing the current ear entirely is not a preference change.
        let gone = match &current {
            Ear::Desk(n) | Ear::Headset(n) => !self.candidates.iter().any(|c| c.name == *n),
            _ => false,
        };
        if gone || current == Ear::Deaf {
            self.current = Some(ideal.ear.clone());
            self.switched_at = t;
            self.wanted = None;
            return ideal;
        }

        // Two desk microphones that hear you about equally well should not
        // trade the ear back and forth: a different desk mic has to beat the
        // one in use by `switch_margin` in score before it even counts as a
        // change Atlas wants. Situational moves (to the phone or the headset
        // because you left) are not score comparisons, so the margin does not
        // gate them.
        if let (Ear::Desk(cur), Ear::Desk(new)) = (&current, &ideal.ear) {
            let score_of = |name: &str| {
                self.candidates
                    .iter()
                    .find(|c| c.name == *name)
                    .map(|c| c.score(cfg.floor_db))
                    .unwrap_or(0.0)
            };
            if score_of(new) - score_of(cur) < cfg.switch_margin {
                self.wanted = None;
                return self.describe(&current, w, cfg);
            }
        }

        // Otherwise the new situation has to persist before Atlas moves.
        match &self.wanted {
            Some(w2) if *w2 == ideal.ear => {
                if t.saturating_sub(self.wanted_since) >= cfg.settle_secs {
                    self.current = Some(ideal.ear.clone());
                    self.switched_at = t;
                    self.wanted = None;
                    return ideal;
                }
            }
            _ => {
                self.wanted = Some(ideal.ear.clone());
                self.wanted_since = t;
            }
        }
        self.describe(&current, w, cfg)
    }

    fn describe(&self, ear: &Ear, w: &Where, cfg: &HearingConfig) -> Choice {
        let mut c = self.ideal(w, cfg);
        if c.ear != *ear {
            c = Choice {
                costs_quality: ear.costs_quality(),
                why: format!("still on {}", short(&ear.name())),
                ear: ear.clone(),
            };
        }
        c
    }

    /// Devices that were measured and heard nothing — worth telling you about
    /// once, because a muffled mic is usually a physical problem you can fix.
    pub fn deaf_devices(&self, cfg: &HearingConfig) -> Vec<&Candidate> {
        self.candidates
            .iter()
            .filter(|c| c.measured_db.is_some() && !c.hears_you(cfg.floor_db))
            .collect()
    }
}

/// Is this microphone part of something worn -- AirPods, a headset, a
/// Bluetooth hands-free link -- rather than on the desk?
pub fn is_headset(name: &str) -> bool {
    let n = name.to_lowercase();
    ["headset", "headphone", "hands-free", "handsfree", "airpods", "buds", "earbuds"].iter().any(|k| n.contains(k))
}

/// "Microphone (HD Pro Webcam C920)" -> "the webcam".
pub fn short(name: &str) -> String {
    let n = name.to_lowercase();
    if n.contains("webcam") || n.contains("camera") {
        return "the webcam".into();
    }
    if n.contains("airpods") {
        return "your AirPods".into();
    }
    if n.contains("headset") || n.contains("hands-free") {
        return "your headset".into();
    }
    if n.contains("array") || n.contains("realtek") || n.contains("internal") {
        return "the laptop mic".into();
    }
    name.to_string()
}

/// Parse `volumedetect` output from ffmpeg. This is how Atlas finds out
/// whether a microphone can actually hear you, rather than guessing from its
/// name.
pub fn mean_volume(ffmpeg_stderr: &str) -> Option<f32> {
    ffmpeg_stderr
        .lines()
        .find(|l| l.contains("mean_volume:"))
        .and_then(|l| l.split("mean_volume:").nth(1))
        .and_then(|v| v.trim().split_whitespace().next())
        .and_then(|v| v.parse::<f32>().ok())
}

/// The ffmpeg arguments for a short listening test on one device.
pub fn calibration_args(device: &str, seconds: u32) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-f".into(),
        "dshow".into(),
        "-i".into(),
        format!("audio={device}"),
        "-t".into(),
        seconds.to_string(),
        "-af".into(),
        "volumedetect".into(),
        "-f".into(),
        "null".into(),
        "-".into(),
    ]
}

/// A microphone to record from, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    /// The device's own name.
    pub name: String,
    /// What ffmpeg opens (Windows' id where there is one).
    pub device: String,
    pub why: String,
    /// Listening on it costs sound quality (a Bluetooth headset's call mode).
    pub costs_quality: bool,
}

/// Which microphone to record from, of `devices`, the way `atlas --daemon`
/// picks at start: by which one hears you (`Hearing::decide`), then by name
/// (`audio::choose`), then any real input. `None` only when there is no
/// input at all. Used again while Atlas runs (29 Sep 2026), so a headset
/// that connects later, or a dock, is picked up without a restart.
pub fn pick_microphone(
    devices: &[Device],
    hearing: &mut Hearing,
    tc: &crate::voice::ToolsConfig,
    w: &Where,
    laptop_active: bool,
    now: u64,
) -> Option<Picked> {
    hearing.observe_devices(devices);
    // The one you asked for, while it's here (29 Sep 2026): the re-pick
    // every few minutes used to take it back to whichever scored best.
    if let Some(name) = hearing.chosen.clone() {
        if devices.iter().any(|d| d.kind == crate::audio::Kind::Input && d.name == name) {
            return Some(Picked {
                device: crate::audio::ffmpeg_name_for(devices, &name),
                name,
                why: "you asked for this one".into(),
                costs_quality: false,
            });
        }
    }
    let choice = hearing.decide(w, &tc.hearing, now);
    // "Nothing" and "your phone" are not devices to record from.
    let picked = match &choice.ear {
        Ear::Desk(n) | Ear::Headset(n) => n.clone(),
        _ => String::new(),
    };
    if !picked.is_empty() {
        return Some(Picked {
            device: crate::audio::ffmpeg_name_for(devices, &picked),
            name: picked,
            why: choice.why.clone(),
            costs_quality: choice.costs_quality,
        });
    }
    let sel = crate::audio::choose(devices, &tc.audio, laptop_active);
    if let Some(mic) = &sel.input {
        return Some(Picked {
            device: crate::audio::ffmpeg_name_for(devices, mic),
            name: mic.clone(),
            why: sel.why.clone(),
            costs_quality: false,
        });
    }
    devices.iter().find(|d| d.kind == crate::audio::Kind::Input).map(|d| Picked {
        name: d.name.clone(),
        device: d.ffmpeg_name(),
        why: format!("no microphone stood out ({}), so the first one", sel.why),
        costs_quality: false,
    })
}

/// Which microphone a spoken kind ("webcam", "headset", "laptop") means.
#[derive(Debug, Clone, PartialEq)]
pub enum MicFit<'a> {
    One(&'a Device),
    Several(Vec<&'a Device>),
    None,
}

/// The input devices whose names fit `kind`: the word itself in the name,
/// or what that kind of microphone is usually called ("webcam" fits a
/// "Camera", a "C920", a "BRIO", or the name of the camera Atlas uses,
/// `camera`), or -- for "laptop" -- the built-in one.
pub fn mic_by_kind<'a>(devices: &'a [Device], kind: &str, camera: &str) -> MicFit<'a> {
    let k = kind.trim().to_lowercase();
    let k = k.trim_end_matches(" microphone").trim_end_matches(" mic").trim();
    let camera = camera.trim().to_lowercase();
    let also: &[&str] = match k {
        "webcam" | "camera" | "cam" => &["webcam", "camera", "cam", "c920", "c922", "c930", "brio", "kiyo", "streamcam", "facecam", "lifecam"],
        "headset" | "headphones" => &["headset", "headphone", "hands-free", "handsfree"],
        "laptop" | "builtin" | "built in" => &["array", "internal", "built-in", "realtek", "intel"],
        "airpods" => &["airpods"],
        "bluetooth" => &["bluetooth", "hands-free", "airpods"],
        "usb" => &["usb"],
        _ => &[],
    };
    let fits = |d: &&Device| {
        if d.kind != Kind::Input {
            return false;
        }
        let n = d.name.to_lowercase();
        n.contains(k)
            || also.iter().any(|a| n.contains(a))
            || (matches!(k, "webcam" | "camera" | "cam") && !camera.is_empty() && camera.split_whitespace().filter(|w| w.len() > 3 && *w != "camera").any(|w| n.contains(w)))
            || (matches!(k, "laptop" | "builtin" | "built in") && d.builtin)
    };
    let found: Vec<&Device> = devices.iter().filter(fits).collect();
    match found.len() {
        0 => MicFit::None,
        1 => MicFit::One(found[0]),
        _ => MicFit::Several(found),
    }
}
