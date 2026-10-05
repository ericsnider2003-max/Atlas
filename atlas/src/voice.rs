//! The voice loop: record -> transcribe -> (caller acts) -> synthesize -> play.
//!
//! Each stage is an ExternalTool from config/tools.yaml, so this module has no
//! idea whether it is driving Whisper or Vosk, Piper or Windows SAPI.

use crate::error::{AtlasError, Result};
use crate::tools::{ExternalTool, Vars};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub enabled: bool,
    /// Scratch directory for wav files. Cleared per turn.
    #[serde(default = "d_work")]
    pub work_dir: String,
    #[serde(default = "d_seconds")]
    pub record_seconds: u32,
    /// Substituted into every tool's command and args: device names, model
    /// paths, anything machine-specific. Keeps paths out of the code.
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    pub record: ExternalTool,
    pub stt: ExternalTool,
    /// Which speech-to-text hears you: `auto` (Parakeet when `atlas get
    /// hearing` has fetched it, whisper otherwise), `parakeet`, or `whisper`.
    pub stt_engine: String,
    pub tts: ExternalTool,
    pub play: ExternalTool,
    pub capture_screen: Option<ExternalTool>,
    pub capture_webcam: Option<ExternalTool>,
    #[serde(default)]
    pub wake: Option<WakeConfig>,
    pub llm: Option<crate::brain::LlmConfig>,
    /// An optional stronger model, used only for the handful of hard reasoning
    /// tasks worth escalating — drafting a self-fix, writing code from a
    /// description. Left unset (the default), Atlas is fully local. Set it and
    /// the local model still handles everything else; only the hard drafts, and
    /// a failed local call, reach for it. This is the "online secondary" of the
    /// project's offline-first rule, made explicit rather than a second `llm`
    /// silently swapped in.
    #[serde(default)]
    pub llm_secondary: Option<crate::brain::LlmConfig>,
    #[serde(default)]
    pub proactive: crate::proactive::ProactiveConfig,
    /// Standing watches you write in words — "machine.disk_used_pct above 90
    /// for 10m" — read by `automation`. A firing is said, never acted on.
    #[serde(default)]
    pub automations: Vec<crate::automation::RuleSpec>,
    /// Your time zone: an IANA name ("America/Los_Angeles"), a Windows name
    /// ("Pacific Standard Time"), or "automatic"/empty for the computer's own
    /// (the TZ variable) and UTC failing that. It decides what "at 7" means,
    /// the wall clock a repeating event keeps, the hour a greeting reads, and
    /// how an invite's times are shown. Read by `tz`.
    #[serde(default)]
    pub time_zone: String,
    /// How to say words the speech engine gets wrong: `nginx: engine x`. Yours win over the built-in list in `pronounce`, which already
    /// covers the business name, currency pairs, acronyms and symbols.
    #[serde(default)]
    pub pronounce: std::collections::BTreeMap<String, String>,
    /// How much Atlas runs on this machine (`fit`): pin a tier, and whether
    /// to re-plan when the machine changes.
    #[serde(default)]
    pub fit: crate::fit::FitConfig,
    /// Whether Atlas asks what you want when it cannot tell (`wanted`).
    #[serde(default)]
    pub wanted: crate::wanted::WantedConfig,
    #[serde(default)]
    pub perf: crate::perf::PerfConfig,
    #[serde(default)]
    pub lanes: crate::lanes::LaneConfig,
    /// How background work is admitted: the memory margin and battery floor.
    #[serde(default)]
    pub crew: crate::crew::CrewConfig,
    #[serde(default)]
    pub research: crate::research::ResearchConfig,
    /// The weather (`weather`): your town and units.
    #[serde(default)]
    pub weather: crate::weather::WeatherConfig,
    /// Online delegation to Cloudflare sub-agents, when the machine is
    /// online. Ships disabled and empty; offline crews never depend on it.
    pub cloudflare: crate::online::CloudflareConfig,
    /// Building code from a description, checked against the real toolchain.
    /// Local and safe (a throwaway sandbox is the judge), so it ships on.
    pub build: crate::build_it::BuildConfig,
    /// The morning run. Was not on this struct at all until now, which meant
    /// `BriefConfig` could only ever be `Default::default()` -- `enabled:
    /// false` -- at its single call site. The brief was not merely starved of
    /// inputs; there was nowhere to turn it on.
    #[serde(default)]
    pub brief: crate::brief::BriefConfig,
    #[serde(default)]
    pub push_to_talk: PttConfig,
    /// Cutting in by voice while Atlas is speaking (`micthread`). Off by
    /// default: without a headset the microphone hears Atlas too.
    pub barge_in: crate::micthread::BargeInConfig,
    #[serde(default)]
    pub lifecycle: crate::lifecycle::LifecycleConfig,
    #[serde(default)]
    pub retention: crate::retention::RetentionConfig,
    #[serde(default)]
    pub connectivity: crate::connectivity::ConnectivityConfig,
    #[serde(default)]
    pub backlog: crate::backlog::BacklogConfig,
    #[serde(default)]
    pub uia: UiaConfig,
    #[serde(default)]
    pub models: crate::models::ModelsConfig,
    #[serde(default)]
    pub browser: crate::browser::BrowserConfig,
    #[serde(default)]
    pub video: VideoConfig,
    #[serde(default)]
    pub voice_id: crate::voiceid::VoiceIdConfig,
    pub speaker: crate::speaker::SpeakerConfig,
    #[serde(default)]
    pub presence: crate::presence::PresenceConfig,
    /// Reading a face and a pair of hands off the camera. Off by default: a
    /// camera that starts watching because you updated is not something
    /// anyone should have to discover.
    #[serde(default)]
    pub gaze: crate::gaze::GazeConfig,
    /// Atlas's own marks drawn on the desktop — the ring around what your
    /// hand is over, and the mark while it's speaking.
    #[serde(default)]
    pub overlay: crate::overlay::OverlayConfig,
    /// Asking about a picture — a chart, the screen, a photo — with a local
    /// model that can talk about what it sees (`picture_talk`).
    #[serde(default)]
    pub picture_talk: crate::picture_talk::PictureTalkConfig,
    /// Making pictures on this machine (`imagemake`).
    #[serde(default)]
    pub picture_making: crate::imagemake::PictureMakingConfig,
    /// Smoothing and prediction for the pointer.
    #[serde(default)]
    pub smoothing: crate::handtrack::SmoothConfig,
    /// How much of one core hand tracking may use.
    #[serde(default)]
    pub pace: crate::handtrack::PaceConfig,
    /// How heavy hand tracking may be on this machine: automatic, light or
    /// full, the NPU, and when to stop with no hand in view (2 Oct 2026).
    #[serde(default)]
    pub hands: crate::handweight::HandsConfig,
    /// Reading a market: how structure is found, where levels go, and what
    /// counts as having earned the right to be believed.
    ///
    /// Grouped rather than scattered because these three are read together
    /// every time and a mismatch between them — a stop measured against one
    /// window and a regime bucketed against another — is the kind of thing
    /// that is never noticed and quietly wrong.
    #[serde(default)]
    pub trading: TradingConfig,
    /// Recognising what is in a picture — faces, things, whatever it has been
    /// shown. Off by default, like everything that uses the camera.
    #[serde(default)]
    pub vision: crate::vision::VisionConfig,
    pub words: crate::words::WordsConfig,
    pub together: crate::together::TogetherConfig,
    /// An outside program that turns a frame into a face and a hand.
    ///
    /// Now a fallback rather than the only way. Atlas reads its own frames
    /// through `vision`; this stays for anyone who would rather point it at
    /// something else. Absent no longer means blind.
    #[serde(default)]
    pub gaze_detector: Option<ExternalTool>,

    #[serde(default)]
    pub persona: crate::persona::Persona,
    #[serde(default)]
    pub thread: crate::thread::ThreadConfig,
    #[serde(default)]
    pub health: crate::health::HealthConfig,
    #[serde(default)]
    pub watch: crate::watch::Watcher,
    #[serde(default)]
    pub server: crate::server::ServerConfig,
    #[serde(default)]
    pub backup: crate::safety::BackupConfig,
    #[serde(default)]
    pub trash: crate::safety::TrashConfig,
    #[serde(default)]
    pub audio: crate::audio::AudioConfig,
    #[serde(default)]
    pub kin: crate::kin::KinConfig,
    #[serde(default)]
    pub hearing: crate::hearing::HearingConfig,
    #[serde(default)]
    pub quick_input: crate::quickinput::QuickInputConfig,
    #[serde(default)]
    pub identity: crate::identity::IdentityConfig,
    #[serde(default)]
    pub ocr: crate::ocr::OcrConfig,
    /// How Atlas watches a video, as opposed to listening to one.
    #[serde(default)]
    pub viewing: crate::viewing::ViewConfig,
    /// A transcriber that writes timestamps.
    ///
    /// Optional and separate from `stt`: the everyday one writes plain text,
    /// which is right for a spoken command and useless for lining a transcript
    /// up against what was on screen. Absent means Atlas still watches, and
    /// says plainly that it couldn't line the two up.
    #[serde(default)]
    pub stt_timed: Option<ExternalTool>,
    #[serde(default)]
    pub hub: HubConfig,
    #[serde(default)]
    pub finance: crate::finance::FinanceConfig,
    #[serde(default)]
    pub call_notes: crate::consent::ConsentConfig,
    /// Working the window in front for you ("draft a reply to this",
    /// "carry on until I'm back").
    #[serde(default)]
    pub delegate: crate::delegate::DelegateConfig,
    /// Keeping the words of graded model calls (`trace::TraceConfig`).
    #[serde(default)]
    pub trace: crate::trace::TraceConfig,
    #[serde(default)]
    pub answering: crate::answering::AnsweringConfig,
    #[serde(default)]
    pub clipboard: crate::clipboard::ClipboardConfig,
    #[serde(default)]
    pub system: crate::system::SystemConfig,
    #[serde(default)]
    pub certainty: crate::certainty::CertaintyConfig,
    pub understood: crate::understood::UnderstoodConfig,
    /// Choosing between competing readings of what you asked for.
    pub whichone: crate::whichone::WhichOneConfig,
    /// Finding your other Atlas on the same network.
    pub nearby: crate::nearby::NearbyConfig,
    /// Reading a Telegram bot's messages. Online, and off by default.
    pub telegram: crate::telegram::TelegramConfig,
    /// How sure a graded judgment has to be before Atlas acts on it.
    pub judgment: crate::judgment::JudgmentConfig,
    pub elsewhere: crate::elsewhere::ElsewhereConfig,
    #[serde(default)]
    pub tune: crate::tune::TuneConfig,
    #[serde(default)]
    pub budget: crate::budget::BudgetConfig,
    #[serde(default)]
    pub handoff: crate::handoff::HandoffConfig,
    #[serde(default)]
    pub overnight: crate::overnight::OvernightConfig,
    #[serde(default)]
    pub strategy: crate::strategy::StrategyConfig,
    #[serde(default)]
    pub consult: crate::consult::ConsultConfig,
    #[serde(default)]
    pub panels: crate::panel::PanelConfig,
    #[serde(default)]
    pub language: crate::language::LanguageConfig,
    #[serde(default)]
    pub voice_settings: crate::tts::VoiceSettings,
    /// When Atlas speaks, how loud, quiet hours, and when it may pop up
    /// (`sound`; the Sound & voice page).
    #[serde(default)]
    pub sound: crate::sound::SoundConfig,
    pub tts_engine: crate::tts::EngineConfig,
    #[serde(default)]
    pub endpoint: crate::endpoint::EndpointConfig,
    /// The conversation after the wake word: hands-free until you stop
    /// talking for a while (`utterance`).
    #[serde(default)]
    pub conversation: crate::utterance::ConversationConfig,
    #[serde(default)]
    pub dictate: crate::dictate::DictateConfig,
    #[serde(default)]
    pub watching: crate::watching::WatchConfig,
    #[serde(default)]
    pub recall: crate::recall::RecallConfig,
    /// Turning text into a meaning vector, for `recall`'s search by meaning.
    #[serde(default)]
    pub meaning: crate::meaning::MeaningConfig,
    pub notify: crate::notify::NotifyConfig,
    pub phone: crate::phone::PhoneConfig,
    #[serde(default)]
    pub self_work: crate::selfwork::SelfWorkConfig,
    #[serde(default)]
    pub route: crate::route::RouteConfig,
    #[serde(default)]
    pub draft: crate::draft::DraftConfig,
    #[serde(default)]
    pub person: crate::person::PersonConfig,
    #[serde(default)]
    pub prose: crate::prose::ProseConfig,
    #[serde(default)]
    pub unsub: crate::unsub::UnsubConfig,
    #[serde(default)]
    pub interrupt: crate::interrupt::InterruptConfig,
    #[serde(default)]
    pub mail: crate::mail::MailConfig,
    /// Other programs' tools (`mcp`): none configured, none started.
    #[serde(default)]
    pub mcp: crate::mcp::McpConfig,
    #[serde(default)]
    pub returning: crate::returning::ReturnConfig,
    /// Where the time went: what's kept, and how away is told apart.
    #[serde(default)]
    pub worklog: crate::worklog::WorkLogConfig,
    /// Round 11's tools: clipboard history, waiting-for, the trading
    /// check-in, feeds, cards, translation, key chords, meeting prep.
    #[serde(default)]
    pub workday: crate::workday::WorkdayConfig,
    /// Looking for gigs, grants and niches once a day (`hunt`). Off until
    /// you turn it on.
    #[serde(default)]
    pub hunt: crate::hunt::HuntConfig,
    #[serde(default)]
    pub routine: crate::routine::RoutineConfig,
    /// Long jobs left to run on their own (Eric, E3).
    #[serde(default)]
    pub long_jobs: crate::goal::LongJobConfig,
    #[serde(default)]
    pub capture: crate::capture::CaptureConfig,
    #[serde(default)]
    pub content: crate::content::ContentConfig,
    /// Loudness and look for `atlas video grade`. The `grade:` block shipped
    /// for weeks with no field to land in, so serde dropped it whole.
    #[serde(default)]
    pub grade: crate::grade::GradeConfig,
    /// How much posted before `reach` will name a direction. Same shape as
    /// `grade` above: a shipped section with nowhere to parse into.
    #[serde(default)]
    pub reach: crate::reach::ReachConfig,
    #[serde(default)]
    pub editors: crate::editors::EditorConfig,
    #[serde(default)]
    pub accounts: crate::accounts::AccountsConfig,
    #[serde(default)]
    pub going_away: crate::goingaway::AwayConfig,
    #[serde(default)]
    pub voiceover: crate::voiceover::VoiceoverConfig,
    #[serde(default)]
    pub vault: crate::vault::VaultConfig,
    #[serde(default)]
    pub walkthrough: crate::walkthrough::WalkConfig,
    #[serde(default)]
    pub codes: crate::codes::CodesConfig,
    #[serde(default)]
    pub signin: crate::signin::SignInConfig,
    #[serde(default)]
    pub confirmed: crate::confirmed::ConfirmConfig,
    #[serde(default)]
    pub opsec: crate::opsec::OpsecConfig,
    #[serde(default)]
    pub recovery: crate::recovery::RecoveryConfig,
    #[serde(default)]
    pub after_me: crate::afterme::AfterMeConfig,
    #[serde(default)]
    pub companion: crate::companion::CompanionConfig,
    #[serde(default)]
    pub sync: crate::sync::SyncConfig,
    #[serde(default)]
    pub working_set: crate::workingset::WorkingSetConfig,
    #[serde(default)]
    pub remote: crate::remote::RemoteConfig,
    #[serde(default)]
    pub files: crate::files::FilesConfig,
    #[serde(default)]
    pub cloud: crate::cloudsync::CloudConfig,
    #[serde(default)]
    pub enrol: crate::enrol::EnrolConfig,
    #[serde(default)]
    pub messaging: crate::messaging::MessagingConfig,
    #[serde(default)]
    pub daily: crate::daily::DailyConfig,
    #[serde(default)]
    pub awake: crate::awake::AwakeConfig,
    #[serde(default)]
    pub editcraft: crate::editcraft::EditCraftConfig,
    #[serde(default)]
    pub grading: crate::grading::GradingConfig,
    #[serde(default)]
    pub pipeline: crate::pipeline::PipelineConfig,
    #[serde(default)]
    pub self_audit: crate::selfaudit::SelfAuditConfig,
    #[serde(default)]
    pub self_grant: crate::selfgrant::SelfGrantConfig,
    #[serde(default)]
    pub reference: crate::reference::ReferenceConfig,
    #[serde(default)]
    pub consolidate: crate::consolidate::ConsolidateConfig,
    #[serde(default)]
    pub money: crate::money::MoneyConfig,
    #[serde(default)]
    pub booking: crate::booking::BookingConfig,
    #[serde(default)]
    pub calendar: crate::calendar::CalendarConfig,
    #[serde(default)]
    pub layout_prefs: crate::layout_prefs::LayoutConfig,
    #[serde(default)]
    pub ios: crate::ios::IosConfig,
    #[serde(default)]
    pub android: crate::android::AndroidConfig,
    #[serde(default)]
    pub mesh: crate::mesh::MeshConfig,
    #[serde(default)]
    pub household: crate::household::HouseholdConfig,
    /// Where the pieces land, and whether the optional ones are wanted.
    ///
    /// `InstallConfig` existed with no field to hold it and no block in
    /// `tools.yaml`, so `tools_dir` and `models_dir` could not be set -- and
    /// `atlas install` reported every piece missing on exactly the machine
    /// they existed for. Added 19 Sep 2026.
    pub install: crate::install::InstallConfig,
    /// The dashboard: which view it opens on, how it groups, and how long
    /// finished things stay visible.
    ///
    /// `WorkspaceConfig` existed with no field to hold it and no block in
    /// `tools.yaml`, so none of its three settings could be set. Added
    /// 19 Sep 2026.
    pub workspace: crate::workspace_view::WorkspaceConfig,
    /// What Atlas says about this machine before you run into it.
    ///
    /// `PortableConfig` existed with no field anywhere to hold it, so there
    /// was no `portable:` block a person could write and nothing that would
    /// have read one. Added 19 Sep 2026, once the catalogue could answer the
    /// question it asks.
    pub portable: crate::portable::PortableConfig,
    /// The house style the design review holds a page to, and that generated
    /// web pages are gated against. The defaults are deliberately neutral —
    /// spacing on a 4px grid, colours from tokens, the accessibility floors —
    /// not anyone's personal taste, so the same page reads as considered for
    /// whoever it goes to. Change the base unit or turn a rule off here.
    #[serde(default)]
    pub taste: crate::taste::Rules,
    /// Atlas on the Windows desktop with no window open: its icon by the
    /// clock (`notifyicon`). Added 28 Sep 2026.
    #[serde(default)]
    pub desktop: crate::notifyicon::DesktopConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HubConfig {
    /// Serve the settings pages. Loopback only, behind the same token.
    pub enabled: bool,
}

impl Default for HubConfig {
    fn default() -> Self {
        HubConfig { enabled: true }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct VideoConfig {
    pub work_dir: String,
    pub ffmpeg: ExternalTool,
    pub ffprobe: ExternalTool,
}

impl Default for VideoConfig {
    fn default() -> Self {
        VideoConfig {
            work_dir: String::new(),
            ffmpeg: ExternalTool {
                command: "ffmpeg".into(), args: vec![], stdin_text: false, result_file: None,
                timeout_secs: 120,
            },
            ffprobe: ExternalTool {
                command: "ffprobe".into(), args: vec![], stdin_text: false, result_file: None,
                timeout_secs: 120,
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UiaConfig {
    pub min_nodes: usize,
    pub min_named_ratio: f32,
}

impl Default for UiaConfig {
    fn default() -> Self {
        UiaConfig { min_nodes: 4, min_named_ratio: 0.4 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PttConfig {
    /// Hold a key to talk, alongside the wake word (Eric, H1). Off here
    /// turns only the key off.
    pub enabled: bool,
    /// Any key by name: tab, capslock, rightctrl, f13 … (`hotkeys::key_code`).
    pub key: String,
    /// Hold this long before Atlas claims the key. Below ~250ms, ordinary
    /// typing starts triggering it.
    pub hold_ms: u64,
}

impl Default for PttConfig {
    fn default() -> Self {
        PttConfig { enabled: true, key: "tab".into(), hold_ms: 350 }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WakeConfig {
    #[serde(default)]
    pub enabled: bool,
    /// What you say to get its attention. Matched loosely against the
    /// transcript, because speech-to-text will render it differently every
    /// time: "Atlas", "atlas,", "Atless".
    pub phrase: String,
    #[serde(default = "d_clip")]
    pub clip_seconds: u32,
    /// A dedicated wake-word binary (openWakeWord, Porcupine). It should block
    /// until the word is heard, then exit 0. If absent, Atlas falls back to
    /// polling the speech-to-text engine on short clips — which works with no
    /// extra install but keeps a CPU core busy.
    pub detector: Option<ExternalTool>,
    /// Listen for the name by its sound before writing anything out (the
    /// spotter in `kws`, when it's downloaded and the phrase is "Atlas"):
    /// speech without the name is never transcribed. Off by default -- it
    /// missed 2 of 63 requests Parakeet heard, on synthetic voices (1 Oct
    /// 2026); worth turning on once measured on your own voice.
    #[serde(default)]
    pub listen_first: bool,
}
fn d_clip() -> u32 {
    3
}
fn d_work() -> String {
    // Anchored to the install rather than to whatever folder the process
    // was started in. A relative value written by hand in tools.yaml is
    // still honoured — `Daemon::tools_cfg` resolves it against the store.
    crate::roots::tmp_dir().to_string_lossy().into_owned()
}
fn d_seconds() -> u32 {
    10
}

pub struct Voice<'a> {
    cfg: &'a ToolsConfig,
    /// How the last `listen()` divided: milliseconds recording, then
    /// milliseconds transcribing, packed hi/lo into one word so it can be
    /// read without a lock.
    ///
    /// Kept here rather than returned because `Ears::listen` returns the
    /// transcript and changing that signature would touch every test double
    /// in the suite. `u64::MAX` means nothing has been measured yet, which is
    /// a different fact from two zeroes.
    ///
    /// Shared (28 Sep 2026) with the microphone's own thread (`micthread`),
    /// which now does the listening after the wake word: the turn is timed
    /// by the same numbers whichever thread recorded it.
    last_listen: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// The same, for the last `speak()`: synthesis, then playback.
    last_speak: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Kokoro's next sentence, made while this one plays (`kokoro::Ahead`).
    ahead: crate::kokoro::Ahead,
}

/// Nothing measured yet. Distinct from a measured zero.
const UNMEASURED: u64 = u64::MAX;

fn pack(a: u128, b: u128) -> u64 {
    // Saturating rather than wrapping: a stage that took longer than 49 days
    // should read as "very slow", not as a small number.
    let a = a.min(u32::MAX as u128) as u64;
    let b = b.min(u32::MAX as u128) as u64;
    (a << 32) | b
}

fn unpack(v: u64) -> Option<(u32, u32)> {
    if v == UNMEASURED {
        return None;
    }
    Some(((v >> 32) as u32, (v & 0xFFFF_FFFF) as u32))
}

/// What Atlas asks ffmpeg to record at, everywhere.
///
/// Named rather than repeated: the configured record command says 16000, the
/// streaming one has to agree, and whisper's models are trained at this rate.
/// Three places that must match is two places too many to leave as literals.
pub const RECORD_RATE_HZ: u32 = 16_000;

impl ToolsConfig {
    /// Anchor every install-relative path in the config to this install.
    ///
    /// `Daemon::tools_cfg` already did this for `work_dir`, but both voice
    /// doors build their `Voice` straight from the loaded config instead —
    /// so the recorder wrote its frames and clips to `data/tmp` **relative
    /// to wherever you started Atlas**, while everything else in the same
    /// run used the install's own scratch. Two installs, or two tests,
    /// sharing a working directory overwrote each other's recordings
    /// mid-turn.
    ///
    /// Called once, at load, so there is no second view of the
    /// configuration for anything downstream to disagree with.
    pub fn anchored(mut self) -> ToolsConfig {
        if self.work_dir.trim().is_empty() {
            self.work_dir = crate::roots::tmp_dir().to_string_lossy().into_owned();
        } else {
            self.work_dir =
                crate::roots::under_install(&self.work_dir).to_string_lossy().into_owned();
        }
        self.models.dir =
            crate::roots::under_install(&self.models.dir).to_string_lossy().into_owned();
        // The tools and model files named in `vars` too (29 Sep 2026):
        // "tools/whisper/whisper-cli.exe", "models/ggml-base.en.bin" and the
        // rest were relative to whatever folder Atlas was started from. The
        // sign-in Run entry has no working folder, so from there Atlas found
        // no speech tools, dropped to typing only and stopped speaking.
        for v in self.vars.values_mut() {
            let rel = v.replace('\\', "/");
            if rel.starts_with("tools/") || rel.starts_with("models/") {
                *v = crate::roots::under_install(&*v).to_string_lossy().into_owned();
            }
        }
        self
    }
}

impl<'a> Voice<'a> {
    pub fn new(cfg: &'a ToolsConfig) -> Self {
        if cfg.enabled && cfg.tts_engine.engine == crate::tts::Engine::Kokoro {
            // Loaded on a thread of its own, then the lines Atlas says most
            // are made ahead (Phase 0.8), so "Yes?" never waits on synthesis.
            let (voice, pace, volume) = (cfg.voice_settings.voice.clone(), cfg.voice_settings.speed, cfg.sound.volume);
            if crate::kokoro::check(&crate::roots::install_root()).is_ok() {
                std::thread::spawn(move || {
                    if let Ok(synth) = kokoro_synth_for(&voice, pace, volume) {
                        crate::kokoro::prepare_stock(synth, crate::kokoro::made_for(&voice, pace, volume));
                    }
                });
            }
        }
        Voice {
            cfg,
            last_listen: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(UNMEASURED)),
            last_speak: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(UNMEASURED)),
            ahead: crate::kokoro::Ahead::default(),
        }
    }

    /// The microphone's work for its own thread (`micthread`): an owned copy
    /// of the settings, and this voice's timings shared.
    ///
    /// The thread's own switch (`MicThread::set_wake`) is what says whether
    /// the wake word is listened for, so its copy is marked on (30 Sep 2026:
    /// the copy kept `wake.enabled` from the start, so switching the wake
    /// word on while Atlas ran armed a loop that never heard anything).
    pub fn mic_work(&self) -> VoiceWork {
        let mut cfg = self.cfg.clone();
        if let Some(w) = cfg.wake.as_mut() {
            w.enabled = true;
        }
        VoiceWork { cfg, last_listen: self.last_listen.clone(), watch: None, said_with_wake: None, warm: None }
    }

    /// Speech-to-text for the clip at `{in_wav}`, only when someone is
    /// speaking in it (`audio::check_speech`, by way of `leveller::prepare`):
    /// silence is "" without asking whisper, speech is levelled to where
    /// whisper hears it best, and what whisper writes for
    /// a quiet room ("you", "Thanks for watching") is dropped
    /// (`not_really_said`). 29 Sep 2026.
    fn transcribe_heard(&self, vars: &Vars) -> Result<String> {
        let in_wav = vars.get("in_wav").cloned().unwrap_or_default();
        if let Ok(bytes) = std::fs::read(&in_wav) {
            if let Ok((samples, rate)) = crate::diarize::read_wav(&bytes) {
                // Judged against the room and levelled by your usual voice on
                // this microphone (`leveller`, 30 Sep 2026: "I have to yell").
                let mic = microphone_now(self.cfg).0;
                match crate::leveller::prepare(&samples, rate, &mic) {
                    None => return Ok(String::new()),
                    Some(ready) => {
                        if ready != samples {
                            crate::kept!(std::fs::write(&in_wav, crate::audio::wav_bytes(&ready, rate)));
                        }
                    }
                }
            }
        }
        // Parakeet first, when it's here (`parakeet`: a quarter of whisper
        // base.en's mistakes up close, a third across the room); whisper for
        // anything it can't do, so hearing never depends on it.
        let english = vars.get("lang_val").is_none_or(|l| l.trim().is_empty() || l.trim().eq_ignore_ascii_case("en"))
            && vars.get("task_opt").is_none_or(|t| t.trim().is_empty());
        if english && !self.cfg.stt_engine.trim().eq_ignore_ascii_case("whisper") {
            if let Some(Ok(text)) = crate::parakeet::transcribe_file(&crate::roots::install_root(), std::path::Path::new(&in_wav)) {
                let text = clean_transcript(&text);
                return Ok(if not_really_said(&text) { String::new() } else { text });
            }
        }
        let raw = self.cfg.stt.run(vars, None)?;
        let text = clean_transcript(&raw);
        Ok(if not_really_said(&text) { String::new() } else { text })
    }

    /// Words for audio already recorded: written where speech-to-text reads
    /// and transcribed. Used for what you said over Atlas while it spoke.
    fn transcribe_samples(&self, samples: &[i16]) -> Result<String> {
        let vars = self.vars()?;
        let in_wav = vars.get("in_wav").cloned().unwrap_or_default();
        std::fs::write(&in_wav, crate::audio::wav_bytes(samples, RECORD_RATE_HZ))?;
        let raw = self.transcribe_heard(&vars)?;
        Ok(clean_transcript(&raw))
    }

    fn vars(&self) -> Result<Vars> {
        let dir = PathBuf::from(&self.cfg.work_dir);
        std::fs::create_dir_all(&dir)?;
        let mut v: Vars = self.cfg.vars.clone();
        // The microphone picked again since start, if it was (`set_microphone`).
        if let Some((name, device)) = microphone_override() {
            v.insert("mic_name".into(), name);
            v.insert("mic_device".into(), device);
        }
        let stem = dir.join("turn");
        v.insert("work_dir".into(), dir.display().to_string());
        v.insert("in_wav".into(), format!("{}.wav", stem.display()));
        v.insert("out_wav".into(), format!("{}_out.wav", stem.display()));
        v.insert("stem".into(), stem.display().to_string());
        v.insert("transcript".into(), format!("{}.txt", stem.display()));
        v.insert("seconds".into(), self.cfg.record_seconds.to_string());

        // The voice settings, as variables the speech command can actually
        // use.
        //
        // Until now they reached nothing. `tts.args` was
        // `["-m", "{tts_model}", "-f", "{out_wav}"]` — no speed, no variation,
        // no sentence gap — so the three sliders in Settings stored numbers
        // and changed nothing you could hear. Worse, `{tts_model}` named a
        // different voice than `voice_settings.voice`, so the voice you chose
        // lost to a var you never edited.
        //
        // Exposing them as vars rather than building the command line in Rust
        // keeps this file's own rule intact: nothing is compiled in, and an
        // engine is swapped by editing the config.
        let vs = &self.cfg.voice_settings;
        let eng = &self.cfg.tts_engine;
        v.insert("voice_file".into(), eng.voice_file_for(&vs.voice));
        v.insert("voice_id".into(), vs.voice.clone());
        // Engine-aware. piper takes a *length scale*, where larger is slower;
        // every other engine takes a speed multiplier, where larger is faster.
        // Passing one to the other silently inverts every speed you have ever
        // chosen — `tts.rs` warned about exactly this and nothing applied it.
        v.insert("speed".into(), format!("{:.2}", eng.engine.speed_value(vs.speed)));
        v.insert("variation".into(), format!("{:.3}", vs.variation));
        v.insert("sentence_gap".into(), format!("{:.2}", vs.sentence_gap));
        // The language flags whisper's command carries. Without these every
        // spoken turn passed the literal `{task_opt}` to whisper-cli.
        // The sharper listening model when setup has fetched it (29 Sep 2026).
        let chosen = crate::language::speech_model_for(
            v.get("stt_model").map(String::as_str).unwrap_or(""),
            &crate::roots::install_root(),
        );
        v.insert("stt_model".into(), chosen);
        let model = crate::language::model_facts(v.get("stt_model").map(String::as_str).unwrap_or(""));
        crate::language::insert_whisper_vars(&self.cfg.language, &model, &mut v);
        // Your own words as the speech model's hints (H11): names, projects,
        // jargon it would otherwise guess at -- after the few every install
        // is primed with ("Atlas" first), so there's a prompt from day one.
        let vocab: crate::improve::Vocabulary = crate::roots::store().load("vocabulary");
        let (opt, val) = crate::improve::hint_args(&vocab, crate::language::SPEECH_PRIMER);
        v.insert("hint_opt".into(), opt);
        v.insert("hint_val".into(), val);
        Ok(v)
    }

    /// Record from the mic and return what was said.
    /// Listen until you stop talking, rather than for a fixed eight seconds.
    ///
    /// `endpoint.rs` has decided when a person has finished speaking since the
    /// day it was written, and nothing ever fed it: the configured record
    /// command carries `-t {seconds}`, so every turn recorded for the same
    /// length whether you said "yes" or a paragraph. That is the eight-second
    /// problem that module's own doc opens by describing, and this is the
    /// half that was missing.
    ///
    /// The shape: ffmpeg streams raw PCM to stdout with no `-t`, Atlas reads
    /// it a window at a time, measures each window and hands the number to
    /// `Endpointer`. When it says you are finished, the child is killed and
    /// the audio collected so far becomes the wav whisper reads. Nothing
    /// waits for a stopwatch, and a one-word answer costs one word of
    /// recording and one word of transcription.
    ///
    /// Falls back to the fixed-length path — returning `None` — whenever
    /// endpointing is off or unavailable, rather than failing: a slower turn
    /// is a much better outcome than no turn.
    pub fn listen_until_you_stop(
        &self,
        cfg: &crate::endpoint::EndpointConfig,
        device: &str,
    ) -> Result<Option<String>> {
        if !cfg.enabled || device.trim().is_empty() {
            return Ok(None);
        }
        let hard_stop_secs = ((cfg.hard_stop_ms / 1000) as u32).max(1);
        let mut stream = self.open_pcm(device, hard_stop_secs + 5)?;
        let started = std::time::Instant::now();
        // Cut by `utterance`: speech told from the room by the detector that
        // learns the room (`vad`), a pause long enough for what you were
        // saying, and a third of a second kept from before the first word.
        // (29 Sep 2026: this loop fed the endpointer itself and threw away
        // everything before the detector was sure, so the first syllable of
        // an answer said promptly was lost.)
        let got = crate::utterance::next_utterance(&mut stream, cfg, cfg.no_speech_after_ms, &|| false);
        let Some(kept) = got else {
            if let Some(why) = crate::micthread::MicStream::why_stopped(&mut stream) {
                return Err(AtlasError::Platform(format!("the microphone \"{device}\" couldn't be opened: {why}")));
            }
            return Ok(None);
        };
        drop(stream);
        let recorded = started.elapsed().as_millis();
        let t1 = std::time::Instant::now();
        // Through `transcribe_heard` (silence never reaches whisper), by way
        // of `transcribe_samples` (merge of 30 Sep 2026).
        let text = self.transcribe_samples(&kept)?;
        self.last_listen.store(
            pack(recorded, t1.elapsed().as_millis()),
            std::sync::atomic::Ordering::Relaxed,
        );
        Ok((!text.is_empty()).then_some(text))
    }

    /// The microphone as a stream of 16 kHz samples, for up to `secs`: the
    /// recorder's complaint kept for when it gives nothing
    /// (`MicStream::why_stopped`).
    fn open_pcm(&self, device: &str, secs: u32) -> Result<PcmStream> {
        PcmStream::open(&self.cfg.record.command, device, secs)
    }

    /// Record for as long as the push-to-talk key is held (H1): the same
    /// streaming recorder as `listen_until_you_stop`, stopped by your finger
    /// rather than by a silence. Falls back to an ordinary listen when there
    /// is no mic device named for streaming.
    fn listen_while_held(&self, held: &dyn Fn() -> bool) -> Result<Option<String>> {
        use std::io::Read;
        let device = microphone_now(self.cfg).1;
        if device.trim().is_empty() {
            return self.listen().map(Some);
        }
        let vars = self.vars()?;
        let rate = RECORD_RATE_HZ;
        let want = crate::audio::window_samples(rate, 50);
        // A minute is longer than anyone holds a key to speak.
        // The recorder's own complaint is kept (it was thrown away): a
        // microphone that can't be opened ends the recording at once with
        // nothing read, which looked exactly like silence (29 Sep 2026).
        let mut child = crate::tools::command(&self.cfg.record.command)
            .args(crate::audio::stream_args(&device, rate, 60))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AtlasError::Platform(format!("could not start '{}' to listen: {e}", self.cfg.record.command)))?;
        let mut out = child
            .stdout
            .take()
            .ok_or_else(|| AtlasError::Platform("the recorder gave nothing to read".into()))?;
        let complaint = child.stderr.take().map(|mut e| {
            std::thread::spawn(move || {
                let mut s = String::new();
                crate::heard!(e.read_to_string(&mut s));
                s
            })
        });
        let started = std::time::Instant::now();
        let mut bytes: Vec<u8> = Vec::new();
        let mut buf = vec![0u8; want * 2];
        while held() {
            match out.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => bytes.extend_from_slice(&buf[..n]),
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let said = complaint.and_then(|h| h.join().ok()).unwrap_or_default();
        if bytes.is_empty() {
            if let Some(why) = recorder_could_not_open(&said) {
                return Err(AtlasError::Platform(format!("the microphone \"{device}\" couldn't be opened: {why}")));
            }
        }
        let kept = crate::audio::samples_from_le(&bytes[..bytes.len() / 2 * 2]);
        // Under a third of a second is a tap, not speech.
        if kept.len() < (rate as usize) / 3 {
            return Ok(None);
        }
        let in_wav = vars.get("in_wav").cloned().unwrap_or_default();
        std::fs::write(&in_wav, crate::audio::wav_bytes(&kept, rate))?;
        let recorded = started.elapsed().as_millis();
        let t1 = std::time::Instant::now();
        let raw = self.transcribe_heard(&vars)?;
        self.last_listen.store(pack(recorded, t1.elapsed().as_millis()), std::sync::atomic::Ordering::Relaxed);
        let text = clean_transcript(&raw);
        Ok((!text.is_empty()).then_some(text))
    }

    pub fn listen(&self) -> Result<String> {
        // Stop when you stop, if that is available.
        //
        // Tried here rather than at each call site so that everything which
        // listens -- the daemon loop, the console loop, the wake-word path --
        // gets it without a signature change and without a second code path
        // to keep honest. A `None` means endpointing is off or heard nothing;
        // an `Err` means the streaming recorder could not start. Both fall
        // through to the fixed-length path below, because a slower turn is a
        // far better outcome than a failed one.
        let device = microphone_now(self.cfg).1;
        let streamed = self.cfg.endpoint.enabled && !device.trim().is_empty();
        match self.listen_until_you_stop(&self.cfg.endpoint, &device) {
            Ok(Some(text)) => return Ok(text),
            // It listened, and you said nothing: that is the answer, not a
            // reason to record for another eight seconds (29 Sep 2026).
            Ok(None) if streamed => return Err(AtlasError::Platform(HEARD_NOTHING.into())),
            Ok(None) => {}
            Err(_) => {}
        }

        let vars = self.vars()?;
        let t0 = std::time::Instant::now();
        self.cfg.record.run(&vars, None)?;
        let recorded = t0.elapsed().as_millis();
        let t1 = std::time::Instant::now();
        let raw = self.transcribe_heard(&vars)?;
        self.last_listen.store(
            pack(recorded, t1.elapsed().as_millis()),
            std::sync::atomic::Ordering::Relaxed,
        );
        let text = clean_transcript(&raw);
        if text.is_empty() {
            return Err(AtlasError::Platform(HEARD_NOTHING.into()));
        }
        Ok(text)
    }

    /// Listen for a follow-up without a wake word. Silence returns None
    /// rather than an error — silence is the normal case here.
    fn listen_for(&self, secs: u32) -> Result<Option<String>> {
        self.listen_for_until(secs, &|| false)
    }

    /// `listen_for`, stopped part-way when `stop` says so (paused, Atlas
    /// closing): stopped reads as silence.
    fn listen_for_until(&self, secs: u32, stop: &dyn Fn() -> bool) -> Result<Option<String>> {
        // Waiting for you to start, rather than recording `secs` and
        // transcribing all of it (29 Sep 2026). The open floor after a reply
        // used to be a fixed six-second recording: start speaking at five
        // seconds and you were cut off at six; say nothing and six seconds of
        // silence went through the speech engine. Now nothing is transcribed
        // unless you spoke, and what you say is taken until you stop -- which
        // is what lets the conversation stay open for half a minute between
        // turns (`conversation.quiet_secs`) at no cost.
        let device = microphone_now(self.cfg).1;
        if self.cfg.endpoint.enabled && !device.trim().is_empty() {
            if let Ok(mut stream) = self.open_pcm(&device, secs + 30) {
                let t0 = std::time::Instant::now();
                let got = crate::utterance::next_utterance(&mut stream, &self.cfg.endpoint, u64::from(secs) * 1000, stop);
                let Some(kept) = got else {
                    if let Some(why) = crate::micthread::MicStream::why_stopped(&mut stream) {
                        return Err(AtlasError::Platform(format!("the microphone \"{device}\" couldn't be opened: {why}")));
                    }
                    return Ok(None);
                };
                drop(stream);
                let recorded = t0.elapsed().as_millis();
                let t1 = std::time::Instant::now();
                let text = self.transcribe_samples(&kept)?;
                self.last_listen.store(pack(recorded, t1.elapsed().as_millis()), std::sync::atomic::Ordering::Relaxed);
                return Ok((!text.is_empty()).then_some(text));
            }
        }
        let mut vars = self.vars()?;
        vars.insert("seconds".into(), secs.to_string());
        let t0 = std::time::Instant::now();
        if self.cfg.record.run_stoppable(&vars, None, stop)?.is_none() {
            return Ok(None);
        }
        let recorded = t0.elapsed().as_millis();
        let t1 = std::time::Instant::now();
        let raw = self.transcribe_heard(&vars)?;
        self.last_listen.store(
            pack(recorded, t1.elapsed().as_millis()),
            std::sync::atomic::Ordering::Relaxed,
        );
        let text = clean_transcript(&raw);
        Ok((!text.is_empty()).then_some(text))
    }

    /// A voice embedding for the wav the last turn was recorded into.
    ///
    /// Runs over `{in_wav}` — the same file speech-to-text just read — so no
    /// second recording is made and nothing extra is asked of you. `None`
    /// when there is no encoder configured or it failed: a verdict Atlas
    /// cannot compute must read as "no opinion", never as "not you".
    pub fn voiceprint(&self) -> Option<Vec<f32>> {
        let vars = self.vars().ok()?;
        crate::speaker::embed(&self.cfg.speaker, &vars).ok()
    }

    /// Speak in a named voice, whatever the settings currently say.
    ///
    /// For auditioning. Everything else about how Atlas sounds — speed, gap,
    /// engine — stays as configured, so what you hear is what you would get
    /// rather than a demo tuned to flatter.
    pub fn say_as(&self, voice_id: &str, text: &str) -> Result<()> {
        let mut vars = self.vars()?;
        vars.insert("voice_id".into(), voice_id.to_string());
        vars.insert("voice_file".into(), self.cfg.tts_engine.voice_file_for(voice_id));
        self.cfg.tts.run(&vars, Some(text))?;
        self.cfg.play.run(&vars, None)?;
        Ok(())
    }

    pub fn speak(&self, text: &str) -> Result<()> {
        // Kokoro, spoken inside Atlas, when it's the engine chosen and it's
        // here; otherwise the configured command (piper) below, as always.
        if self.cfg.tts_engine.engine == crate::tts::Engine::Kokoro {
            match self.speak_kokoro(text) {
                Kokoro::Done(done) => return done,
                Kokoro::Unavailable => {}
                // Kokoro stopped partway: piper says only what's left (29
                // Sep 2026: it was handed the whole line, so what you had
                // already heard was said again).
                Kokoro::Rest(rest) => return self.speak_with_piper(&rest),
            }
        }
        self.speak_with_piper(text)
    }

    /// Play `{out_wav}`: inside Atlas, through the speaker `audio::choose`
    /// picked (`playout`, 29 Sep 2026), when the player is the shipped
    /// `ffplay`; the configured player when that can't, or when you named
    /// another. `Ok(None)` when it was stopped (you cut in).
    fn play_out(&self, vars: &Vars) -> Result<Option<String>> {
        let stop = &crate::micthread::playback_cut;
        if cfg!(windows) && crate::playout::plays_inside(&self.cfg.play.command) {
            if let Some(bytes) = vars.get("out_wav").and_then(|p| std::fs::read(p).ok()) {
                let speaker = crate::playout::speaker_now(&self.cfg.audio);
                let played = crate::playout::parse_wav(&bytes).and_then(|wav| crate::playout::play(&wav, speaker.as_deref(), stop));
                match played {
                    Ok(()) => return Ok(if stop() { None } else { Some(String::new()) }),
                    Err(why) => {
                        if let Some(note) = crate::playout::note_once(&why) {
                            crate::outln!("{note}");
                        }
                    }
                }
            }
        }
        self.cfg.play.run_stoppable(vars, None, stop)
    }

    /// The configured speech command (piper), for the whole of `text`.
    fn speak_with_piper(&self, text: &str) -> Result<()> {
        let vars = self.speech_command_vars()?;
        let t0 = std::time::Instant::now();
        // Said the way it should sound: "EURUSD" as "euro dollar", "VPS" as
        // "V P S", "→" as "to" (`pronounce`). The words on screen are unchanged.
        let spoken = crate::pronounce::for_speech(text, &self.cfg.pronounce);
        self.cfg.tts.run(&vars, Some(&spoken))?;
        // Speaking volume (Sound & voice), applied to the audio itself.
        if self.cfg.sound.volume < 100 {
            if let Some(p) = vars.get("out_wav") {
                if let Ok(mut w) = std::fs::read(p) {
                    if crate::sound::scale_wav(&mut w, self.cfg.sound.volume) {
                        crate::kept!(std::fs::write(p, &w));
                    }
                }
            }
        }
        let synth = t0.elapsed().as_millis();
        // The mark moves with the voice: the reply's loudness, frame by
        // frame, and the moment playback starts (`speaking`). Best effort —
        // a line that doesn't move is no reason not to speak.
        let data = crate::roots::data_dir();
        if let Some(wav) = vars.get("out_wav").and_then(|p| std::fs::read(p).ok()) {
            crate::heard!(crate::speaking::begin(&data, text, &wav, crate::speaking::now_ms() + crate::speaking::PLAYBACK_LAG_MS));
            crate::overlaywin::start_if_gone(&data);
        }
        let t1 = std::time::Instant::now();
        // Ended mid-way when you speak over it (`micthread`, cutting in by
        // voice); a reply already cut isn't started at all.
        let played = if crate::micthread::playback_cut() {
            Ok(None)
        } else {
            self.play_out(&vars)
        };
        crate::speaking::end(&data);
        played?;
        // Recorded separately because they are different facts: synthesis is
        // Atlas being slow, playback is the audio's real length. Folding them
        // together would make a long answer look like a slow assistant.
        self.last_speak.store(
            pack(synth, t1.elapsed().as_millis()),
            std::sync::atomic::Ordering::Relaxed,
        );
        Ok(())
    }

    /// The variables for the configured speech command.
    ///
    /// With Kokoro chosen and not available, that command is piper's (the
    /// shipped `tts`, which choosing Kokoro in Settings doesn't change), so
    /// piper is given a piper voice and piper's own speed -- not the Kokoro
    /// voice name, a `.pt` file and Kokoro's multiplier, which piper would
    /// refuse and Atlas would fall silent.
    fn speech_command_vars(&self) -> Result<Vars> {
        let mut vars = self.vars()?;
        let eng = &self.cfg.tts_engine;
        if eng.engine == crate::tts::Engine::Kokoro && self.cfg.tts.command.to_lowercase().contains("piper") {
            let piper = crate::tts::EngineConfig { engine: crate::tts::Engine::Piper, ..eng.clone() };
            let vs = &self.cfg.voice_settings;
            let voice = if crate::kokoro::speaker_id(&vs.voice).is_some() {
                crate::tts::VoiceSettings::default().voice
            } else {
                vs.voice.clone()
            };
            vars.insert("voice_file".into(), piper.voice_file_for(&voice));
            vars.insert("voice_id".into(), voice);
            vars.insert("speed".into(), format!("{:.2}", crate::tts::Engine::Piper.speed_value(vs.speed)));
        }
        Ok(vars)
    }

    /// How one sentence becomes Kokoro audio: the voice, your pace and your
    /// volume. `Err` (with why, in words) when Kokoro can't be had.
    fn kokoro_synth(&self) -> std::result::Result<crate::kokoro::Synth, String> {
        kokoro_synth_for(&self.cfg.voice_settings.voice, self.cfg.voice_settings.speed, self.cfg.sound.volume)
    }

    /// What the stock lines are made with, for these settings.
    fn stock_made_for(&self) -> String {
        crate::kokoro::made_for(&self.cfg.voice_settings.voice, self.cfg.voice_settings.speed, self.cfg.sound.volume)
    }
}

/// Kokoro saying text in `voice` at `pace` and `volume` -- loaded the first
/// time (`kokoro::engine`), so this can wait seconds; call it off the loop.
fn kokoro_synth_for(voice: &str, pace: f32, volume: u8) -> std::result::Result<crate::kokoro::Synth, String> {
        let root = crate::roots::install_root();
        let engine = crate::kokoro::engine(&root)?;
        let (_, sid) = crate::kokoro::voice_or_default(voice);
        let speed = crate::tts::Engine::Kokoro.speed_value(pace);
        Ok(std::sync::Arc::new(move |text: &str| {
            let k = engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut wav = k.synth_wav(text, sid, speed)?;
            // Speaking volume (Sound & voice), applied to the audio itself.
            if volume < 100 {
                crate::sound::scale_wav(&mut wav, volume);
            }
            Ok(wav)
        }))
}

impl Voice<'_> {
    /// A whole reply, before its first sentence is spoken, so Kokoro can
    /// make each next sentence while the one before it plays. The sentences
    /// are the ones `speakthread::Saying` will hand to `speak`, as `speak` will
    /// see them (`spoken_form`, then `pronounce`). Nothing for piper.
    pub fn prepare(&self, reply: &str) {
        if self.cfg.tts_engine.engine != crate::tts::Engine::Kokoro {
            return;
        }
        let Ok(synth) = self.kokoro_synth() else { return };
        let sentences: Vec<String> = crate::speech::split(reply)
            .iter()
            .map(|c| crate::pronounce::for_speech(&crate::spoken_form::for_speech(c), &self.cfg.pronounce))
            .collect();
        self.ahead.prepare(sentences, synth);
    }

    /// More of a reply already `prepare`d, queued after it (the model was
    /// still writing when the start was handed over).
    pub fn prepare_more(&self, more: &str) {
        if self.cfg.tts_engine.engine != crate::tts::Engine::Kokoro {
            return;
        }
        let Ok(synth) = self.kokoro_synth() else { return };
        let sentences: Vec<String> = crate::speech::split(more)
            .iter()
            .map(|c| crate::pronounce::for_speech(&crate::spoken_form::for_speech(c), &self.cfg.pronounce))
            .collect();
        self.ahead.extend(sentences, synth);
    }

    /// This voice for the thread that plays a reply (`speakthread`): an
    /// owned copy of the settings, and this voice's timings and Kokoro queue
    /// shared -- so what `prepare` queues here is what the thread plays.
    pub fn speaker(&self) -> VoiceSpeaker {
        VoiceSpeaker { cfg: self.cfg.clone(), last_speak: self.last_speak.clone(), ahead: self.ahead.clone() }
    }

    /// Speak in Kokoro. `None` when it isn't available -- said once, in
    /// words, and then piper speaks instead; never an error, and never
    /// tried again on every sentence (`kokoro::engine` keeps the failure).
    fn speak_kokoro(&self, text: &str) -> Kokoro {
        let synth = match self.kokoro_synth() {
            Ok(s) => s,
            Err(why) => {
                if let Some(note) = crate::kokoro::note_once(&why) {
                    crate::outln!("{note}");
                }
                return Kokoro::Unavailable;
            }
        };
        let spoken = crate::pronounce::for_speech(text, &self.cfg.pronounce);
        let sentences = crate::speech::split(&spoken);
        // A reply `prepare` was given is already queued. Anything else of
        // more than one sentence (a line from `Daemon::say`) is queued here,
        // so its second sentence is made while its first plays. A single
        // sentence that isn't queued is made directly, leaving the queue --
        // the rest of a reply -- alone.
        if sentences.len() > 1 && !sentences.first().is_some_and(|s| self.ahead.has(s)) {
            self.ahead.prepare(sentences.clone(), synth.clone());
        }
        let vars = match self.vars() {
            Ok(v) => v,
            Err(e) => return Kokoro::Done(Err(e)),
        };
        let out_wav = vars.get("out_wav").cloned().unwrap_or_default();
        let data = crate::roots::data_dir();
        let (mut synth_ms, mut play_ms) = (0u128, 0u128);
        for (i, s) in sentences.iter().enumerate() {
            let t0 = std::time::Instant::now();
            let made_for = self.stock_made_for();
            let wav = match self.ahead.take(s).or_else(|| crate::kokoro::stock(s, &made_for).map(Ok)).unwrap_or_else(|| synth(s)) {
                Ok(w) => w,
                Err(why) => {
                    // Fails mid-reply: say why once, and piper says the rest
                    // -- only the rest.
                    crate::kokoro::forget();
                    if let Some(note) = crate::kokoro::note_once(&why) {
                        crate::outln!("{note}");
                    }
                    return kokoro_stopped_at(&sentences, i);
                }
            };
            synth_ms += t0.elapsed().as_millis();
            if let Err(e) = std::fs::write(&out_wav, &wav) {
                return Kokoro::Done(Err(e.into()));
            }
            // The mark moves with the voice, sentence by sentence.
            crate::heard!(crate::speaking::begin(&data, s, &wav, crate::speaking::now_ms() + crate::speaking::PLAYBACK_LAG_MS));
            let t1 = std::time::Instant::now();
            // Ended mid-way when you speak over it or hold the talk key, as
            // piper's is (28 Sep 2026: Kokoro's player couldn't be stopped,
            // so cutting in waited for the end of the sentence).
            if crate::micthread::playback_cut() {
                crate::speaking::end(&data);
                break;
            }
            let played = self.play_out(&vars);
            play_ms += t1.elapsed().as_millis();
            crate::speaking::end(&data);
            if let Err(e) = played {
                return Kokoro::Done(Err(e));
            }
        }
        self.last_speak.store(pack(synth_ms, play_ms), std::sync::atomic::Ordering::Relaxed);
        Kokoro::Done(Ok(()))
    }

    /// Photo from the webcam. Returns the file path.
    pub fn capture_webcam(&self) -> Result<String> {
        self.snap(self.cfg.capture_webcam.as_ref(), "webcam")
    }

    /// Screenshot for "view my display". Returns the file path.
    pub fn capture_screen(&self) -> Result<String> {
        self.snap(self.cfg.capture_screen.as_ref(), "screen")
    }

    fn snap(&self, tool: Option<&ExternalTool>, kind: &str) -> Result<String> {
        let tool = tool
            .ok_or_else(|| AtlasError::Config(format!("no {kind} capture tool configured")))?;
        // The working folder has to exist before anything writes into it.
        // `listen` created it and this didn't, so a capture on a fresh install
        // failed with whatever the capture tool says when a directory is
        // missing — which on Windows is nothing useful at all.
        std::fs::create_dir_all(&self.cfg.work_dir)
            .map_err(|e| AtlasError::Config(format!("can't make {}: {e}", self.cfg.work_dir)))?;
        let mut vars = self.vars()?;
        // Joined as a path (2 Oct 2026): "C:\\...\\work/screen_1.png" reached
        // tools as a mixed path, and Windows' own tools (cmd's copy among
        // them) read the "/screen..." half as a switch.
        let path = std::path::Path::new(&self.cfg.work_dir)
            .join(format!(
                "{kind}_{}.png",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            ))
            .to_string_lossy()
            .to_string();
        vars.insert("out_png".into(), path.clone());
        tool.run(&vars, None)?;
        Ok(path)
    }

    /// Block until the wake phrase is heard.
    ///
    /// Two strategies. A dedicated detector binary is cheap and accurate and
    /// is what you want long term. The fallback — record a short clip,
    /// transcribe it, look for the phrase — needs nothing extra installed but
    /// runs speech-to-text continuously, so expect real CPU use and battery
    /// cost. Start on the fallback, move to a detector once the loop is
    /// proven.
    pub fn wait_for_wake(&self) -> Result<()> {
        let wake = self
            .cfg
            .wake
            .as_ref()
            .ok_or_else(|| AtlasError::Config("no wake config".into()))?;

        if let Some(det) = &wake.detector {
            det.run(&self.vars()?, None)?;
            return Ok(());
        }

        // Failures are counted, paced, and eventually given up on.
        //
        // This loop used to `continue` on both errors with no sleep, no
        // backoff and no way out. Both failures are FAST -- `record.run`
        // returns in milliseconds when ffmpeg is not on PATH (ENOENT) or when
        // `mic_device` names a device that does not exist (non-zero exit). So
        // on a machine with a misnamed microphone, `atlas --wake` pinned a
        // core at 100%, spawning processes as fast as the OS would allow,
        // forever, with nothing printed and nothing returned.
        //
        // The give-up matters as much as the sleep: `input::Tiers` exists to
        // demote Atlas to push-to-talk when voice stops working, and it could
        // never fire here, because a function that never returns never
        // reports a failure. An infinite retry is not resilience -- it is a
        // hang that looks like patience.
        //
        // A *successful* listen that simply did not contain the wake word is
        // not a failure and does not count toward the limit: that is the
        // normal case, once per clip, all day.
        const GIVE_UP_AFTER: u32 = 10;
        let want = loose(&wake.phrase);
        let mut failures: u32 = 0;
        loop {
            if failures >= GIVE_UP_AFTER {
                return Err(AtlasError::Platform(format!(
                    "couldn't record or transcribe {GIVE_UP_AFTER} times in a row while \
                     listening for the wake word -- check the microphone and that the \
                     recording tool is installed"
                )));
            }

            let mut vars = self.vars()?;
            vars.insert("seconds".into(), wake.clip_seconds.to_string());

            let heard = self
                .cfg
                .record
                .run(&vars, None)
                .and_then(|_| self.transcribe_heard(&vars));

            match heard {
                Ok(raw) => {
                    failures = 0;
                    if loose(&clean_transcript(&raw)).contains(&want) {
                        return Ok(());
                    }
                }
                Err(_) => {
                    failures += 1;
                    // Backing off rather than a fixed pause: a genuinely
                    // transient failure (a device busy for a moment) recovers
                    // in the first second or two, and a permanent one should
                    // not be asked ten times in ten milliseconds.
                    let pause = 200u64 * u64::from(failures);
                    std::thread::sleep(std::time::Duration::from_millis(pause.min(2_000)));
                }
            }
        }
    }

    /// A single bounded attempt: record a short clip, transcribe, look for the
    /// phrase. Returns quickly either way so the daemon stays responsive.
    pub fn wake_once(&self) -> Result<bool> {
        self.wake_once_until(&|| false)
    }

    /// `wake_once`, stopped part-way when `stop` says so (28 Sep 2026: it
    /// runs on the microphone's own thread now, and Pause, Atlas closing or
    /// a turn starting must end the clip at once rather than three seconds
    /// later). Stopped reads as "not heard".
    fn wake_once_until(&self, stop: &dyn Fn() -> bool) -> Result<bool> {
        Ok(self.wake_heard_until(stop)?.is_some())
    }

    /// The wake word, listened for by what's said rather than by clips
    /// (30 Sep 2026).
    ///
    /// The clip way records three seconds, looks for the name in them, and
    /// records the next three: "Atl-" at the end of one clip and "-as" at
    /// the start of the next is never heard, and whatever is said while a
    /// clip is being transcribed is lost. Here the microphone streams the
    /// whole time; each thing said is taken from where the talking starts
    /// to where it stops (`endpoint`), heard (Parakeet, about a tenth of its
    /// length), and checked for the name.
    ///
    /// `Ok(Some(Some(words)))`: the name, and what followed it (maybe
    /// nothing). `Ok(Some(None))`: listened until `stop` or the stream's end
    /// without the name. `Ok(None)`: can't listen this way here (no
    /// streaming microphone, endpointing off) -- use clips.
    fn wake_by_speech(&self, phrase: &str, stop: &dyn Fn() -> bool) -> Result<Option<Option<String>>> {
        use std::io::Read;
        let cfg = &self.cfg.endpoint;
        let device = microphone_now(self.cfg).1;
        if !cfg.enabled || device.trim().is_empty() {
            return Ok(None);
        }
        let vars = self.vars()?;
        let rate = RECORD_RATE_HZ;
        let want = crate::audio::window_samples(rate, 250);
        // A minute a stream, then the loop starts another: a recorder that
        // dies is replaced, and settings changed meanwhile are picked up.
        let mut child = crate::tools::command(&self.cfg.record.command)
            .args(crate::audio::stream_args(&device, rate, 60))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| AtlasError::Platform(format!("could not start '{}' to listen: {e}", self.cfg.record.command)))?;
        let mut out = child.stdout.take().ok_or_else(|| AtlasError::Platform("the recorder gave nothing to read".into()))?;
        // Time by the sound itself, not the clock: what's heard is measured in
        // what was recorded, however late it's read.
        let mut heard_ms: u64 = 0;
        let mut vad = crate::vad::Vad::tuned(rate, cfg.vad_params());
        let mut ep = crate::endpoint::Endpointer::start(0);
        let mut kept: Vec<i16> = Vec::new();
        let mut pending: Vec<u8> = Vec::new();
        let mut buf = vec![0u8; want * 2];
        let preroll = want * 2;
        let mut heard: Option<String> = None;
        'stream: while !stop() {
            let n = match out.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            pending.extend_from_slice(&buf[..n]);
            while pending.len() >= want * 2 {
                let chunk: Vec<u8> = pending.drain(..want * 2).collect();
                let samples = crate::audio::samples_from_le(&chunk);
                let db = crate::audio::level_db(&samples);
                kept.extend_from_slice(&samples);
                heard_ms += 250;
                let now_ms = heard_ms;
                let level = crate::vad::level_for_endpoint(db, vad.window(&samples), cfg.silence_below_db);
                ep.feed(level, "", now_ms, cfg);
                if !ep.finished() {
                    continue;
                }
                if ep.heard_anything {
                    let said = self.heard_in(&kept, &vars).unwrap_or_default();
                    if let Some(rest) = words_after_name(&said, phrase) {
                        heard = Some(rest);
                        break 'stream;
                    }
                    kept.clear();
                } else {
                    // Nothing said: keep only the last moment, in case the
                    // name starts right at the edge.
                    let from = kept.len().saturating_sub(preroll);
                    kept.drain(..from);
                }
                ep = crate::endpoint::Endpointer::start(now_ms);
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        // The stream ended mid-sentence: what was said so far still counts.
        if heard.is_none() && ep.heard_anything && !stop() {
            let said = self.heard_in(&kept, &vars).unwrap_or_default();
            heard = words_after_name(&said, phrase);
        }
        Ok(Some(heard))
    }

    /// Words in samples already recorded, through the usual hearing.
    fn heard_in(&self, samples: &[i16], vars: &Vars) -> Result<String> {
        let in_wav = vars.get("in_wav").cloned().unwrap_or_default();
        std::fs::write(&in_wav, crate::audio::wav_bytes(samples, RECORD_RATE_HZ))?;
        Ok(clean_transcript(&self.transcribe_heard(vars)?))
    }

    /// `wake_once_until`, keeping what was said after the name in the same
    /// clip: `None` not heard, `Some("")` the name alone, `Some(words)` the
    /// name and then those words (29 Sep 2026: they were thrown away).
    fn wake_heard_until(&self, stop: &dyn Fn() -> bool) -> Result<Option<String>> {
        let wake = self
            .cfg
            .wake
            .as_ref()
            .ok_or_else(|| AtlasError::Config("no wake config".into()))?;
        // Switched off means not listening at all (27 Sep 2026): this used
        // to record a clip and run speech-to-text on it regardless of
        // `wake.enabled`, every pass of the loop.
        if !wake.enabled {
            return Ok(None);
        }
        let name_alone = |heard: bool| heard.then(String::new);
        if let Some(det) = &wake.detector {
            return Ok(name_alone(det.run_stoppable(&self.vars()?, None, stop)?.is_some()));
        }
        // Listening the whole time, by what you say rather than by clips,
        // when Parakeet is here to hear each thing said quickly.
        // (Not with a phrase you taught it: that one is matched by sound.)
        if crate::parakeet::installed(&crate::roots::install_root()).is_some()
            && !self.cfg.stt_engine.trim().eq_ignore_ascii_case("whisper")
            && crate::wakeword::load(&crate::roots::store()).is_none()
        {
            if let Some(heard) = self.wake_by_speech(&wake.phrase, stop)? {
                return Ok(heard);
            }
        }
        let mut vars = self.vars()?;
        vars.insert("seconds".into(), wake.clip_seconds.to_string());
        if self.cfg.record.run_stoppable(&vars, None, stop)?.is_none() {
            return Ok(None);
        }
        // Your own phrase, taught with `atlas wake-word` (`wakeword`): matched
        // against the takes directly, with no speech-to-text in the loop.
        if let Some(model) = crate::wakeword::load(&crate::roots::store()) {
            let path = vars.get("in_wav").cloned().unwrap_or_default();
            let bytes = std::fs::read(&path)?;
            let (samples, rate) = crate::diarize::read_wav(&bytes).map_err(AtlasError::Platform)?;
            return Ok(name_alone(crate::wakeword::heard(&samples, rate, &model)));
        }
        let raw = self.transcribe_heard(&vars)?;
        Ok(words_after_name(&clean_transcript(&raw), &wake.phrase))
    }
}

// `after_wake_phrase` (the other chat's, 29 Sep 2026) and `words_after_name`
// (this one's) were the same matcher written twice; merged 30 Sep 2026 into
// `words_after_name`, used by both the clip-at-a-time wake word and the
// stream (`VoiceWork::name_in`). The one difference: a phrase found only
// inside a longer word ("Atlassian") is no longer the name -- it woke Atlas
// on the clip path and not on the stream.

/// A listen that worked and heard no words. Not a broken microphone: the
/// daemon answers it and doesn't count it towards dropping to push-to-talk
/// (29 Sep 2026: three hesitations after the wake word switched the wake word
/// off as "not working").
pub const HEARD_NOTHING: &str = "heard nothing";

/// Lowercase, strip everything that isn't a letter or digit. "Hey, Atlas!"
/// and "hey atlas" and "HEY ATLAS." all collapse to the same string.
pub fn loose(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// The words after the wake phrase in what was heard: "Atlas, what time is
/// it?" -> "what time is it?". `None` when the phrase isn't there; empty when
/// nothing came after it. The phrase is matched word by word, loosely ("Hey,
/// Atlas!" is "hey atlas"), so "Atlassian" isn't the name. (29 Sep 2026: the
/// words were thrown away once the name was found, and the request said in
/// the same breath was lost.)
pub fn words_after_name(heard: &str, phrase: &str) -> Option<String> {
    let want = loose(phrase);
    if want.is_empty() {
        return None;
    }
    let words: Vec<&str> = heard.split_whitespace().collect();
    for i in 0..words.len() {
        let mut acc = String::new();
        for j in i..words.len() {
            acc.push_str(&loose(words[j]));
            if acc == want {
                let rest = words[j + 1..].join(" ");
                let rest = rest.trim_start_matches(|c: char| !c.is_alphanumeric()).trim();
                return Some(rest.to_string());
            }
            if !want.starts_with(&acc) {
                break;
            }
        }
    }
    None
}

/// What whisper writes for a quiet room, not for anything said: a whole
/// transcript that is only one of these is dropped. "Thank you" alone is not
/// here -- people say it -- but it only reaches whisper at all when the clip
/// held speech (`audio::check_speech`).
pub const WHISPER_SILENCE: &[&str] = &[
    "you",
    "thanks for watching",
    "thank you for watching",
    "thanks for watching bye",
    "please subscribe",
    "subscribe",
    "like and subscribe",
    "bye bye",
    "the end",
    "so",
    "uh",
    "um",
    "hmm",
    "mm",
    "oh",
    "music",
    "applause",
    "silence",
    "blank audio",
];

/// Is `text` only something whisper writes for silence?
pub fn not_really_said(text: &str) -> bool {
    let plain: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() || c == '\'' { c } else { ' ' })
        .collect();
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    plain.is_empty() || WHISPER_SILENCE.contains(&plain.as_str())
}

/// Whisper emits bracketed timestamps and blank-audio markers. Strip them,
/// or "[BLANK_AUDIO]" gets parsed as a command.
pub fn clean_transcript(raw: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in raw.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl crate::daemon::Ears for Voice<'_> {
    fn wait_for_wake(&self) -> Result<()> {
        Voice::wait_for_wake(self)
    }
    fn listen(&self) -> Result<String> {
        Voice::listen(self)
    }
    fn listen_while(&self, held: &dyn Fn() -> bool) -> Result<Option<String>> {
        Voice::listen_while_held(self, held)
    }
    fn listen_briefly(&self, secs: u32) -> Result<Option<String>> {
        Voice::listen_for(self, secs)
    }
    fn wake_once(&self) -> Result<bool> {
        Voice::wake_once(self)
    }
    fn mic_work(&self) -> Option<Box<dyn crate::micthread::MicWork>> {
        Some(Box::new(Voice::mic_work(self)))
    }
    fn last_listen_split_ms(&self) -> Option<(u32, u32)> {
        // Taken, not read (Phase 0.2): about 60 of 107 turns on 30 Sep
        // carried the listening and hearing figures of an earlier turn,
        // because a typed or follow-up turn left the last ones in place.
        unpack(self.last_listen.swap(UNMEASURED, std::sync::atomic::Ordering::Relaxed))
    }
    fn voiceprint(&self) -> Option<Vec<f32>> {
        Voice::voiceprint(self)
    }
}

/// The microphone's work on its own thread (`micthread`): the same recorder,
/// wake word and speech-to-text as `Voice`, with settings it owns.
pub struct VoiceWork {
    cfg: ToolsConfig,
    last_listen: std::sync::Arc<std::sync::atomic::AtomicU64>,
    watch: Option<crate::speaking::Watch>,
    /// Words heard after the name in the wake word's clip (`wake_heard_until`).
    said_with_wake: Option<String>,
    /// The recorder for the open floor, started while the reply played.
    warm: Option<WarmMic>,
}

impl VoiceWork {
    fn voice(&self) -> Voice<'_> {
        Voice {
            cfg: &self.cfg,
            last_listen: self.last_listen.clone(),
            last_speak: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(UNMEASURED)),
            // The microphone's thread never speaks, so nothing is made ahead.
            ahead: crate::kokoro::Ahead::default(),
        }
    }
}

impl crate::micthread::MicWork for VoiceWork {
    fn listen_while(&mut self, held: &dyn Fn() -> bool) -> Result<Option<String>> {
        self.voice().listen_while_held(held)
    }
    fn wake_once(&mut self, stop: &dyn Fn() -> bool) -> Result<bool> {
        let heard = self.voice().wake_heard_until(stop)?;
        self.said_with_wake = heard.clone().filter(|w| !w.trim().is_empty());
        Ok(heard.is_some())
    }
    fn take_said_with_wake(&mut self) -> Option<String> {
        self.said_with_wake.take()
    }
    fn listen(&mut self) -> Result<String> {
        self.voice().listen()
    }
    fn open_stream(&mut self) -> Option<Box<dyn crate::micthread::MicStream>> {
        let device = microphone_now(&self.cfg).1;
        if device.trim().is_empty() {
            return None;
        }
        // Ten minutes is longer than any reply; listening for the wake word
        // opens it again when it runs out.
        PcmStream::open(&self.cfg.record.command, &device, 600).ok().map(|p| Box::new(p) as Box<dyn crate::micthread::MicStream>)
    }
    /// The wake word from what you said, when it's heard by speech-to-text
    /// (with or without your taught phrase) rather than by a program of its
    /// own, and the microphone can be streamed.
    fn hears_name_in_audio(&self) -> bool {
        let Some(wake) = self.cfg.wake.as_ref() else { return false };
        wake.enabled && wake.detector.is_none() && self.cfg.endpoint.enabled && !microphone_now(&self.cfg).1.trim().is_empty()
    }
    fn name_by_sound(&mut self, samples: &[i16]) -> Option<bool> {
        let wake = self.cfg.wake.as_ref()?;
        if !wake.listen_first || !crate::kws::knows(&wake.phrase) {
            return None;
        }
        crate::kws::heard_name(&crate::roots::install_root(), samples)
    }
    fn name_in(&mut self, samples: &[i16]) -> Result<crate::micthread::NameCheck> {
        use crate::micthread::NameCheck;
        let phrase = self.cfg.wake.as_ref().map(|w| w.phrase.clone()).unwrap_or_else(|| "atlas".into());
        // Your own phrase, taught with `atlas wake-word`, is matched on the
        // sound first: speech-to-text only runs when it's there.
        let taught = crate::wakeword::load(&crate::roots::store());
        if let Some(model) = &taught {
            if !crate::wakeword::heard(samples, RECORD_RATE_HZ, model) {
                return Ok(NameCheck::NotNamed(String::new()));
            }
        }
        let words = self.voice().transcribe_samples(samples)?;
        Ok(match words_after_name(&words, &phrase) {
            Some(rest) => NameCheck::Named(rest),
            // Misheard: Parakeet wrote "Alice, what time is it?" -- but the
            // spotter heard the name by its sound (`kws`, measured 1 Oct).
            None if taught.is_none()
                && crate::kws::knows(&phrase)
                && crate::kws::soundalike_at_start(&words).is_some()
                && crate::kws::heard_name(&crate::roots::install_root(), samples) == Some(true) =>
            {
                NameCheck::Named(crate::kws::soundalike_at_start(&words).unwrap_or_default())
            }
            // Heard by its sound and not written as the phrase: what was said
            // after the phrase's own number of words.
            None if taught.is_some() => {
                let n = phrase.split_whitespace().count().max(1);
                NameCheck::Named(words.split_whitespace().skip(n).collect::<Vec<_>>().join(" "))
            }
            None => NameCheck::NotNamed(words),
        })
    }
    fn endpointing(&self) -> crate::endpoint::EndpointConfig {
        self.cfg.endpoint.clone()
    }
    fn clip_secs(&self) -> u32 {
        self.cfg.wake.as_ref().map(|w| w.clip_seconds).unwrap_or(3)
    }
    fn transcribe(&mut self, samples: &[i16]) -> Result<String> {
        self.voice().transcribe_samples(samples)
    }
    fn follow_up(&mut self, secs: u32, stop: &dyn Fn() -> bool) -> Result<Option<String>> {
        // The recorder started while the reply played, when there is one:
        // opening a fresh one here took long enough that words said straight
        // after Atlas finished were lost (research report, Stage 1 item 9).
        if let Some(mut warm) = self.warm.take().filter(|w| w.alive()) {
            warm.claim();
            let t0 = std::time::Instant::now();
            let got = crate::utterance::next_utterance(&mut warm, &self.cfg.endpoint, u64::from(secs) * 1000, stop);
            drop(warm);
            let Some(kept) = got else { return Ok(None) };
            let recorded = t0.elapsed().as_millis();
            let t1 = std::time::Instant::now();
            let voice = self.voice();
            let text = voice.transcribe_samples(&kept)?;
            voice.last_listen.store(pack(recorded, t1.elapsed().as_millis()), std::sync::atomic::Ordering::Relaxed);
            return Ok((!text.is_empty()).then_some(text));
        }
        self.voice().listen_for_until(secs, stop)
    }
    fn warm_up(&mut self) {
        if self.warm.as_ref().is_some_and(|w| w.alive()) || !self.cfg.endpoint.enabled {
            return;
        }
        let device = microphone_now(&self.cfg).1;
        if device.trim().is_empty() {
            return;
        }
        self.warm = PcmStream::open(&self.cfg.record.command, &device, 600).ok().map(WarmMic::start);
    }
    fn playing_level(&mut self) -> Option<f32> {
        self.watch.get_or_insert_with(|| crate::speaking::Watch::new(crate::roots::data_dir())).level()
    }
    fn models_dir(&self) -> PathBuf {
        PathBuf::from(&self.cfg.models.dir)
    }
}

/// A recorder started ahead of the open floor (`MicWork::warm_up`). Its
/// sound is read and thrown away -- that's Atlas's own reply, mostly --
/// until it's claimed, and from then on handed over as it comes. Unclaimed
/// for a minute, it stops on its own: the microphone isn't held open for a
/// follow-up that never came.
struct WarmMic {
    rx: std::sync::mpsc::Receiver<Vec<i16>>,
    claimed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pending: std::collections::VecDeque<i16>,
}

const WARM_UNCLAIMED_SECS: u64 = 60;

impl WarmMic {
    fn start(mut stream: PcmStream) -> WarmMic {
        use crate::micthread::MicStream;
        use std::sync::atomic::Ordering;
        let (tx, rx) = std::sync::mpsc::channel();
        let claimed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (c, st) = (claimed.clone(), stop.clone());
        let _ = std::thread::Builder::new().name("atlas-warm-mic".into()).spawn(move || {
            let started = std::time::Instant::now();
            // 20 ms at a time.
            while !st.load(Ordering::SeqCst) {
                let Some(chunk) = stream.read((RECORD_RATE_HZ / 50) as usize) else { break };
                if c.load(Ordering::SeqCst) {
                    if tx.send(chunk).is_err() {
                        break;
                    }
                } else if started.elapsed().as_secs() >= WARM_UNCLAIMED_SECS {
                    break;
                }
            }
            st.store(true, Ordering::SeqCst);
        });
        WarmMic { rx, claimed, stop, pending: std::collections::VecDeque::new() }
    }
    fn alive(&self) -> bool {
        !self.stop.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn claim(&mut self) {
        self.claimed.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl crate::micthread::MicStream for WarmMic {
    fn read(&mut self, n: usize) -> Option<Vec<i16>> {
        while self.pending.len() < n {
            match self.rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(chunk) => self.pending.extend(chunk),
                Err(_) => return None,
            }
        }
        Some(self.pending.drain(..n).collect())
    }
    fn why_stopped(&mut self) -> Option<String> {
        None
    }
}

impl Drop for WarmMic {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The recorder streaming raw 16-bit samples. Dropping it ends the recorder.
/// What the recorder writes about itself is kept, so a microphone that
/// couldn't be opened says why rather than looking like silence.
struct PcmStream {
    child: std::process::Child,
    out: std::process::ChildStdout,
    said: Option<std::thread::JoinHandle<String>>,
}

impl PcmStream {
    fn open(command: &str, device: &str, secs: u32) -> Result<PcmStream> {
        use std::io::Read;
        let mut child = crate::tools::command(command)
            .args(crate::audio::stream_args(device, RECORD_RATE_HZ, secs))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AtlasError::Platform(format!("could not start '{command}' to listen: {e}")))?;
        let out = child.stdout.take().ok_or_else(|| AtlasError::Platform("the recorder gave nothing to read".into()))?;
        let said = child.stderr.take().map(|mut e| {
            std::thread::spawn(move || {
                let mut s = String::new();
                crate::heard!(e.read_to_string(&mut s));
                s
            })
        });
        Ok(PcmStream { child, out, said })
    }
}

impl crate::micthread::MicStream for PcmStream {
    fn read(&mut self, n: usize) -> Option<Vec<i16>> {
        use std::io::Read;
        let mut buf = vec![0u8; n * 2];
        self.out.read_exact(&mut buf).ok()?;
        Some(crate::audio::samples_from_le(&buf))
    }
    fn why_stopped(&mut self) -> Option<String> {
        // The recorder has ended (that's why `read` gave nothing), so its
        // complaint is all written.
        let _ = self.child.kill();
        let _ = self.child.wait();
        let said = self.said.take().and_then(|h| h.join().ok()).unwrap_or_default();
        recorder_could_not_open(&said)
    }
}

impl Drop for PcmStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The voice on the thread that plays a reply (`speakthread`).
pub struct VoiceSpeaker {
    cfg: ToolsConfig,
    last_speak: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ahead: crate::kokoro::Ahead,
}

impl crate::speakthread::SpeakWork for VoiceSpeaker {
    fn speak(&mut self, text: &str) -> Result<()> {
        Voice {
            cfg: &self.cfg,
            last_listen: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(UNMEASURED)),
            last_speak: self.last_speak.clone(),
            ahead: self.ahead.clone(),
        }
        .speak(text)
    }
}

impl crate::daemon::Mouth for Voice<'_> {
    fn speak(&self, text: &str) -> Result<()> {
        Voice::speak(self, text)
    }
    fn prepare(&self, reply: &str) {
        Voice::prepare(self, reply)
    }
    fn prepare_more(&self, more: &str) {
        Voice::prepare_more(self, more)
    }
    fn speak_work(&self) -> Option<Box<dyn crate::speakthread::SpeakWork>> {
        Some(Box::new(self.speaker()))
    }
    fn last_speak_split_ms(&self) -> Option<(u32, u32)> {
        unpack(self.last_speak.swap(UNMEASURED, std::sync::atomic::Ordering::Relaxed))
    }
}


/// Everything about reading a market that is still config.
///
/// ## `market` used to be here and deliberately is not any more
///
/// `market` owns its own constants now, and `market::params` checks each one
/// against the module that uses it, hashes the set to a fingerprint, and
/// counts how many configurations have been tried. That last number is the
/// multiple-comparisons count — a system that has tried forty configurations
/// and reports the best has found the largest of forty noise draws, and there
/// is no way to recover that fact afterwards unless it was counted at the
/// time.
///
/// A config file breaks all three. The value in the file and the value the
/// module was reasoned about stop being the same thing; the fingerprint stops
/// describing what actually ran; and every edit is an untracked attempt. So
/// reading a market is tuned by changing a constant in the module that owns
/// it, where the registry can see it — which is slower on purpose.
///
/// What stays here is what is genuinely yours to set: how much you risk and
/// what reward you will take.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct TradingConfig {
    pub levels: crate::levels::Rules,
}

/// The recorder's reason for giving no sound at all, when it has one: the
/// first line of what ffmpeg wrote when it could not open the device.
/// `None` for an empty complaint -- a key let go before any sound arrived is
/// not a broken microphone.
pub fn recorder_could_not_open(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| {
            // "[dshow @ 000001...] Could not find ..." -> "Could not find ..."
            match (l.starts_with('['), l.find("] ")) {
                (true, Some(i)) => l[i + 2..].to_string(),
                _ => l.to_string(),
            }
        })
}

/// The microphone picked again while Atlas runs (29 Sep 2026): AirPods
/// connecting after sign-in, a dock, or the picked one unplugged. Every
/// recorder reads through `microphone_now`, so a new pick takes effect at the
/// next recording without rebuilding anything. `None`: the pick made at start
/// (in `vars`) stands.
static MIC_NOW: std::sync::RwLock<Option<(String, String)>> = std::sync::RwLock::new(None);

/// Record from this microphone from now on: `name` as the device calls
/// itself, `device` as ffmpeg opens it.
pub fn set_microphone(name: &str, device: &str) {
    if let Ok(mut m) = MIC_NOW.write() {
        *m = Some((name.to_string(), device.to_string()));
    }
}

fn microphone_override() -> Option<(String, String)> {
    MIC_NOW.read().ok().and_then(|m| m.clone())
}

/// The microphone recorded from now: (its name, what ffmpeg opens).
pub fn microphone_now(cfg: &ToolsConfig) -> (String, String) {
    if let Some(m) = microphone_override() {
        return m;
    }
    let device = cfg.vars.get("mic_device").cloned().unwrap_or_default();
    let name = cfg.vars.get("mic_name").cloned().unwrap_or_else(|| device.clone());
    (name, device)
}

/// How a line went in Kokoro.
#[derive(Debug)]
pub enum Kokoro {
    /// Said (or failed to play) in Kokoro.
    Done(Result<()>),
    /// Not available: piper says the whole line.
    Unavailable,
    /// Stopped partway: piper says this, the part not yet said.
    Rest(String),
}

/// Kokoro failed on sentence `at` of `sentences`: nothing said yet is
/// `Unavailable` (piper says it all), otherwise the rest from `at`.
pub fn kokoro_stopped_at(sentences: &[String], at: usize) -> Kokoro {
    if at == 0 {
        return Kokoro::Unavailable;
    }
    Kokoro::Rest(sentences[at.min(sentences.len())..].join(" "))
}
