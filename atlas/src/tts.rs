//! Atlas's voice, and changing it by asking.
//!
//! Every voice here is free. Piper is MIT-licensed and its voices are free
//! downloads — no account, no key, nothing to sign up for. "Model" in this
//! project has never meant "paid"; it means a file on your disk.
//!
//! You should be able to say "a bit slower", "try a deeper one", "use a
//! British voice" and have it change, then and there, without going to look
//! for a setting. That is what this is.

use serde::{Deserialize, Serialize};

/// How good a voice sounds against how big it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// Tiny and noticeably synthetic. For a slow machine.
    Low,
    /// The sensible default. Good on almost anything.
    Medium,
    /// Noticeably better, several times the size and slower.
    High,
}

impl Quality {
    pub fn megabytes(&self) -> u32 {
        match self {
            Quality::Low => 20,
            Quality::Medium => 63,
            Quality::High => 114,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    /// The file name, which is what piper is given.
    pub id: String,
    /// What you'd call it.
    pub name: String,
    /// "American", "British", "Irish".
    pub accent: String,
    /// Roughly how it reads: "warm", "flat", "brisk", "low".
    pub character: String,
    pub quality: Quality,
}

/// A shortlist to audition, for the brief: softer and warm, mid-twenties,
/// generally calm but not flat, able to be dry when it disagrees.
///
/// Kokoro voice ids, not catalogue entries — it ships 54 fixed presets and you
/// pick one rather than describing what you want.
///
/// **Kokoro's own letter grades are interface labels, not a blinded listening
/// benchmark**, and its publishers say so. They narrow 54 options; they do not
/// rank quality. This is a starting point to listen to, not an answer.
pub const SHORTLIST: &[(&str, &str)] = &[
    ("af_heart", "warmest of the American voices, and the highest graded. Start here."),
    ("af_bella", "similar warmth, a little more body. The obvious second."),
    ("af_jessica", "more personality — carries a dry line better than most."),
    ("am_liam", "casual and young. The closest male voice to the brief."),
    ("am_puck", "lighter and more playful; the one with mischief in it."),
    ("bm_fable", "British, warm, storytelling. Calmer than the rest."),
];

/// Said plainly, because it is the gap between the brief and the engine.
///
/// Kokoro is decoder-only with no expressive control: each preset has one
/// delivery, and it does not get more energetic because the moment calls for
/// it. Punctuation shifts prosody a little, and that is all. So "generally
/// calm but can get riled up" is not something Kokoro does — that needs an
/// engine with an expressiveness control, which is what `variation` feeds and
/// why `Engine::can_clone` marks Chatterbox as the one that has it.
pub const KOKORO_HAS_NO_RANGE: &str =
    "Kokoro has one delivery per voice. It can sound warm, or dry, or calm — but it won't get      more energetic when the moment calls for it, because the model has no control for that.      If vocal range matters more than speed, Chatterbox is the engine that has it.";

/// Free voices worth having. All of them download without an account.
///
/// Deliberately short: a list of ninety is not a choice, it's a chore.
pub fn catalogue() -> Vec<Voice> {
    vec![
        Voice {
            id: "en_US-ryan-medium".into(),
            name: "Ryan".into(),
            accent: "American".into(),
            character: "level, unhurried".into(),
            quality: Quality::Medium,
        },
        Voice {
            id: "en_US-amy-medium".into(),
            name: "Amy".into(),
            accent: "American".into(),
            character: "warm, slightly brighter".into(),
            quality: Quality::Medium,
        },
        Voice {
            id: "en_GB-alan-medium".into(),
            name: "Alan".into(),
            accent: "British".into(),
            character: "dry, low".into(),
            quality: Quality::Medium,
        },
        Voice {
            id: "en_GB-northern_english_male-medium".into(),
            name: "Northern".into(),
            accent: "British".into(),
            character: "flatter, more casual".into(),
            quality: Quality::Medium,
        },
        Voice {
            id: "en_GB-jenny_dioco-medium".into(),
            name: "Jenny".into(),
            accent: "British".into(),
            character: "crisp, precise".into(),
            quality: Quality::Medium,
        },
        Voice {
            id: "en_US-lessac-high".into(),
            name: "Lessac".into(),
            accent: "American".into(),
            character: "the most natural of these, and the largest".into(),
            quality: Quality::High,
        },
        Voice {
            id: "en_US-danny-low".into(),
            name: "Danny".into(),
            accent: "American".into(),
            character: "small and quick, obviously synthetic".into(),
            quality: Quality::Low,
        },
    ]
}

pub fn find(id_or_name: &str) -> Option<Voice> {
    let q = id_or_name.trim().to_lowercase();
    catalogue().into_iter().find(|v| {
        v.id.to_lowercase() == q || v.name.to_lowercase() == q || v.id.to_lowercase().contains(&q)
    })
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct VoiceSettings {
    pub voice: String,
    /// The pace: below 1 is faster, above is slower. piper calls this its
    /// length scale; `Engine::speed_value` turns it into what each engine
    /// wants.
    pub speed: f32,
    /// How much the delivery varies. Low is flat and robotic; high wanders.
    pub variation: f32,
    /// Pause between sentences, in seconds.
    pub sentence_gap: f32,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            // A piper voice id, matching the shipped config and
            // `Engine::default()`, which is also piper. Kept in step with
            // `config/tools.yaml` deliberately: the Settings page shows what
            // you have changed from the defaults, and a default that
            // disagrees with the shipped config makes every setting read as
            // modified before you have touched anything.
            //
            // This said `af_bella` while the engine default said piper --
            // two declarations of one fact, disagreeing, and the voice half
            // named a preset for an engine nothing installed.
            voice: "en_US-amy-medium".into(),
            speed: 1.0,
            variation: 0.667,
            sentence_gap: 0.2,
        }
    }
}

impl VoiceSettings {
    /// Piper's arguments. Nothing here is a paid feature or an account.
    pub fn args(&self, models_dir: &str, out_wav: &str) -> Vec<String> {
        vec![
            "--model".into(),
            format!("{models_dir}/{}.onnx", self.voice),
            "--output_file".into(),
            out_wav.into(),
            "--length_scale".into(),
            format!("{:.2}", self.speed),
            "--noise_scale".into(),
            format!("{:.3}", self.variation),
            "--sentence_silence".into(),
            format!("{:.2}", self.sentence_gap),
        ]
    }

    pub fn describe(&self) -> String {
        let v = find(&self.voice);
        let name = v.as_ref().map(|v| v.name.clone()).unwrap_or_else(|| self.voice.clone());
        let accent = v.map(|v| v.accent).unwrap_or_default();
        let pace = if self.speed < 0.92 {
            "quick"
        } else if self.speed > 1.12 {
            "slow"
        } else {
            "normal pace"
        };
        format!("{name}, {accent}, {pace}.")
    }
}

/// A change you asked for out loud.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Adjust the current voice.
    Adjust { what: String },
    /// Move to a different voice.
    Switch { to: Voice, why: String },
    /// Play each of a few so you can pick.
    Audition(Vec<Voice>),
    /// Not understood.
    Unclear(String),
}

/// Work out what "a bit slower" or "try a deeper one" means.
///
/// Adjustments are relative and small, because you will say it again if it
/// isn't enough — and a change that overshoots is more annoying than one that
/// undershoots.
pub fn interpret(said: &str, now: &VoiceSettings) -> Change {
    let t = said.to_lowercase();

    // Pace.
    if t.contains("slower") || t.contains("slow down") || t.contains("too fast") {
        return Change::Adjust { what: "slower".into() };
    }
    if t.contains("faster") || t.contains("speed up") || t.contains("too slow") {
        return Change::Adjust { what: "faster".into() };
    }
    // Delivery.
    if t.contains("robotic") || t.contains("flat") || t.contains("monotone") || t.contains("boring") {
        return Change::Adjust { what: "more variation".into() };
    }
    if t.contains("dramatic") || t.contains("too much") || t.contains("calm down") || t.contains("steadier") {
        return Change::Adjust { what: "less variation".into() };
    }

    // A named voice.
    for v in catalogue() {
        if t.contains(&v.name.to_lowercase()) {
            return Change::Switch { to: v.clone(), why: format!("{} it is", v.name) };
        }
    }

    // A described voice.
    let want_british = t.contains("british") || t.contains("english accent") || t.contains("uk");
    let want_american = t.contains("american") || t.contains("us accent");
    let want_deeper = t.contains("deeper") || t.contains("lower") || t.contains("darker");
    let want_warmer = t.contains("warmer") || t.contains("friendlier") || t.contains("softer");
    let want_better = t.contains("better") || t.contains("more natural") || t.contains("nicer");

    if want_better {
        if let Some(v) = catalogue().into_iter().find(|v| v.quality == Quality::High) {
            return Change::Switch {
                why: format!("{} is the most natural, but it's {}MB and a bit slower", v.name, v.quality.megabytes()),
                to: v,
            };
        }
    }
    let mut pool: Vec<Voice> = catalogue()
        .into_iter()
        .filter(|v| v.id != now.voice)
        .filter(|v| !want_british || v.accent == "British")
        .filter(|v| !want_american || v.accent == "American")
        .collect();

    if want_deeper {
        pool.retain(|v| v.character.contains("low") || v.character.contains("dry"));
    }
    if want_warmer {
        pool.retain(|v| v.character.contains("warm"));
    }

    if want_british || want_american || want_deeper || want_warmer {
        return match pool.first() {
            Some(v) => Change::Switch { to: v.clone(), why: v.character.clone() },
            None => Change::Unclear("I haven't got one like that".into()),
        };
    }

    // "A different voice", with nothing else to go on.
    if t.contains("different voice") || t.contains("another voice") || t.contains("change your voice")
        || t.contains("new voice")
    {
        let choices: Vec<Voice> =
            catalogue().into_iter().filter(|v| v.id != now.voice).take(3).collect();
        return Change::Audition(choices);
    }

    Change::Unclear("I didn't catch what to change".into())
}

/// Apply an adjustment. Bounded, so nothing can be driven into being unusable.
pub fn adjust(now: &VoiceSettings, what: &str) -> (VoiceSettings, String) {
    let mut next = now.clone();
    let said = match what {
        "slower" => {
            next.speed = (now.speed + 0.12).min(1.6);
            format!("Slower. {}", pace_word(next.speed))
        }
        "faster" => {
            next.speed = (now.speed - 0.12).max(0.6);
            format!("Faster. {}", pace_word(next.speed))
        }
        "more variation" => {
            next.variation = (now.variation + 0.12).min(1.0);
            "A bit more life in it.".into()
        }
        "less variation" => {
            next.variation = (now.variation - 0.12).max(0.2);
            "Steadier.".into()
        }
        _ => return (next, "I don't know how to change that.".into()),
    };
    (next, said)
}

fn pace_word(speed: f32) -> String {
    if speed >= 1.55 {
        "That's as slow as I go.".into()
    } else if speed <= 0.65 {
        "That's as fast as I go.".into()
    } else {
        "Like this.".into()
    }
}

/// What Atlas says while auditioning, in each candidate voice.
///
/// Deliberately a real sentence rather than "testing one two three" — you're
/// choosing how it will sound saying the things it actually says.
pub fn audition_line(v: &Voice) -> String {
    format!("This is {}. Your nine o'clock post is over length by twelve characters.", v.name)
}

// ---------------------------------------------------------------------------
// Which engine speaks.
//
// `Voice.id` is a piper file name and `speed` is piper's length scale, so
// every voice in the catalogue above is piper-shaped. That was fine while
// piper was the only option; it is now the thing stopping a better voice going
// in.
//
// piper is a 2023 VITS model. It is small, free, offline and unbeatable on a
// Raspberry Pi, and it is the weakest part of how Atlas sounds. What replaced
// it is also free, also offline, and considerably better.
// ---------------------------------------------------------------------------

/// A speech engine Atlas can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    /// What ships today. Kept as the floor: it runs on anything, needs no GPU,
    /// and is what makes Atlas speak on a machine that can run nothing else.
    Piper,
    /// 82M parameters, Apache 2.0, 54 voices, realtime on CPU. The low-risk
    /// upgrade and the right default once proven.
    Kokoro,
    /// Zero-shot cloning from a short reference clip. MIT.
    Chatterbox,
}

impl Engine {
    /// Its name, for a sentence that already has a description in it.
    ///
    /// `plain()` below describes what the engine *is* ("much better, still
    /// runs on the processor alone"), which does not fit where a name is
    /// wanted. `atlas doctor` reached for `{:?}` instead and printed the
    /// Rust variant — the same variant-name-as-English fault the hub guard
    /// exists to catch, in a file that was simply not on the guard's list.
    pub fn name(&self) -> &'static str {
        match self {
            Engine::Piper => "piper",
            Engine::Kokoro => "Kokoro",
            Engine::Chatterbox => "Chatterbox",
        }
    }

    /// Can it be told to sound like a specific person from a sample?
    pub fn can_clone(&self) -> bool {
        matches!(self, Engine::Chatterbox)
    }

    /// The voice-file extension this engine expects. `Voice.id` means a
    /// different thing per engine, and pretending otherwise is what hardcoded
    /// piper in the first place.
    fn voice_extension(&self) -> &'static str {
        match self {
            Engine::Piper => "onnx",
            Engine::Kokoro => "pt",
            Engine::Chatterbox => "wav",
        }
    }

    /// Does this engine want the speed setting turned upside down?
    ///
    /// The setting (`VoiceSettings::speed`) is a *pace*: larger is slower.
    /// "A bit slower" adds to it (`adjust`), Settings says "below 1 is
    /// faster", and `describe` calls under 0.92 quick. piper's length scale
    /// means the same (larger is slower), so piper gets it as it is. The
    /// others take a speed multiplier, where larger is faster, so they get
    /// its reciprocal.
    ///
    /// Until 28 Sep 2026 this was the other way round: piper got the
    /// reciprocal, so "a bit slower" (1.00 to 1.12) reached piper as a
    /// length scale of 0.89 and it spoke *faster*, and Kokoro would have
    /// done the same. Two tests pinned the inversion, each with a comment
    /// saying the opposite of the other about what the number meant.
    pub fn speed_is_inverted(&self) -> bool {
        !matches!(self, Engine::Piper)
    }

    /// Translate your pace setting into what this engine wants.
    pub fn speed_value(&self, pace: f32) -> f32 {
        if self.speed_is_inverted() {
            1.0 / pace.max(0.1)
        } else {
            pace
        }
    }

    pub fn plain(&self) -> &'static str {
        match self {
            Engine::Piper => "small and reliable, sounds like 2023",
            Engine::Kokoro => "much better, still runs on the processor alone",
            Engine::Chatterbox => "best, and can copy a voice from a short clip",
        }
    }
}

impl Default for Engine {
    /// piper until another engine is proven on this machine. Defaulting to the
    /// better one and failing is worse than defaulting to the plainer one and
    /// working.
    fn default() -> Self {
        Engine::Piper
    }
}

/// Everything Atlas needs to drive an engine it did not ship with.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    pub engine: Engine,
    /// Where the executable lives, relative to the Atlas folder.
    pub exe: String,
    /// Where its voices live.
    pub voices_dir: String,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            engine: Engine::Piper,
            exe: "tools/piper/piper.exe".into(),
            voices_dir: "models".into(),
        }
    }
}

impl EngineConfig {
    /// The file this voice lives in for the configured engine.
    pub fn voice_file(&self, v: &Voice) -> String {
        self.voice_file_for(&v.id)
    }

    /// The same, from a bare voice id.
    ///
    /// Needed because the id in your settings is not always a voice in the
    /// catalogue — you can point at one you downloaded yourself — and
    /// requiring a catalogue entry would mean the setting silently falling
    /// back to a different voice, which is the fault this is fixing.
    pub fn voice_file_for(&self, id: &str) -> String {
        crate::roots::under_install(&self.voices_dir)
            .join(format!("{id}.{}", self.engine.voice_extension()))
            .to_string_lossy()
            .into_owned()
    }

    /// A config whose engine and executable disagree is a misconfiguration
    /// that only shows up as silence, which is the hardest fault to chase.
    pub fn is_consistent(&self) -> bool {
        let exe = self.exe.to_lowercase();
        match self.engine {
            Engine::Piper => exe.contains("piper"),
            Engine::Kokoro => exe.contains("kokoro"),
            Engine::Chatterbox => exe.contains("chatterbox") || exe.contains("voicebox"),
        }
    }
}
