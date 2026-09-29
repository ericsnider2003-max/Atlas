//! Recording a call, and telling people you are.
//!
//! You asked how consent would actually work. It's the part that decides
//! whether this feature is usable or a liability, so it's the part that gets
//! built first rather than bolted on.
//!
//! The legal shape, roughly: some places require only one party to consent —
//! you, since you're in the call. Others require **everyone**. Getting it
//! wrong isn't a technicality; in two-party jurisdictions it's a criminal
//! matter. Atlas has no way to know where the other people are sitting.
//!
//! So the design refuses to guess:
//!
//! 1. **Your side is always safe.** Recording your own microphone captures
//!    only you. That alone gives you your own notes, and needs nobody's
//!    permission.
//! 2. **Everything else is announced.** To record the whole call, Atlas says
//!    so at the start — out loud or in the chat — before anything is captured.
//! 3. **Silence is not consent.** If the announcement can't be delivered,
//!    nothing is recorded. Ever.
//! 4. **Anyone can stop it.** One objection ends recording and discards what
//!    was captured.

use serde::{Deserialize, Serialize};

/// How much of a call is being captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Nothing.
    Off,
    /// Only your microphone. Captures you and nobody else.
    YouOnly,
    /// Everyone. Requires an announcement first.
    Everyone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// One participant's consent is enough — yours.
    OneParty,
    /// Everyone must be told. The safe default.
    AllParties,
}

/// What it asks the call, when it asks rather than tells.
pub const QUESTION: &str =
    "Is it alright if my assistant takes notes on this call? I won't record anyone until you've said yes.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    /// Waiting for the announcement to be delivered.
    Announcing,
    /// The question has been put to the call; waiting for their yes. Only
    /// your side is captured meanwhile.
    Asking,
    Recording,
    /// Someone objected. Nothing is kept.
    Stopped,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConsentConfig {
    pub enabled: bool,
    /// Which rule to assume. Defaults to the stricter one, because Atlas
    /// cannot know where the other people are.
    pub assume: Rule,
    /// The most it may ever do without being asked each time.
    pub default_scope: Scope,
    /// Ask about recording every call, rather than remembering a yes.
    pub ask_every_call: bool,
    /// What it says **to the other people on the call** before recording
    /// them. Separate from telling you, which happens either way.
    pub announcement: String,
    /// Tell the call in the chat window rather than out loud. Less
    /// disruptive, and it leaves a written record that everyone was told.
    pub announce_in_chat: bool,
    /// Delete recordings after this many days. Transcripts and notes stay.
    pub keep_audio_days: u64,
    /// Ask the others and wait for a yes, rather than telling them and
    /// recording unless someone objects.
    ///
    /// Eric, 24 Sep 2026: "voices ask then record". An announcement with an
    /// objection window is consent by silence — someone who didn't hear it,
    /// or didn't like to object, is recorded. Asking and waiting for a yes
    /// is the stricter reading, and it's his.
    pub ask_the_others: bool,
    /// What it asks the call when `ask_the_others` is on.
    pub question: String,
}

impl Default for ConsentConfig {
    fn default() -> Self {
        ConsentConfig {
            enabled: false,
            assume: Rule::AllParties,
            default_scope: Scope::YouOnly,
            ask_every_call: true,
            announcement: ANNOUNCEMENTS[0].1.to_string(),
            announce_in_chat: true,
            keep_audio_days: 7,
            ask_the_others: true,
            question: QUESTION.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Do nothing.
    Nothing,
    /// Ask you first.
    AskYou(String),
    /// Say this to the call, then wait for it to land.
    Announce(String),
    /// Begin capturing at this scope.
    Start(Scope),
    /// Stop, and throw away what was captured.
    StopAndDiscard(String),
    /// Stop, keep what there is.
    StopAndKeep,
}

#[derive(Debug, Clone)]
pub struct Recorder {
    pub cfg: ConsentConfig,
    pub state: State,
    pub scope: Scope,
    announced: bool,
    /// Whether the announcement was actually delivered, not merely attempted.
    announcement_landed: bool,
    /// Whether the others said yes, when they were asked.
    agreed: bool,
    pub objections: Vec<String>,
    pub started_at: Option<u64>,
}

impl Default for Recorder {
    fn default() -> Self {
        Recorder::new(ConsentConfig::default())
    }
}

impl Recorder {
    pub fn new(cfg: ConsentConfig) -> Recorder {
        Recorder {
            scope: Scope::Off,
            cfg,
            state: State::Idle,
            announced: false,
            announcement_landed: false,
            agreed: false,
            objections: Vec::new(),
            started_at: None,
        }
    }

    /// A call has started. What should Atlas do?
    pub fn call_started(&mut self, wanted: Scope) -> Step {
        if !self.cfg.enabled {
            return Step::Nothing;
        }
        self.state = State::Idle;
        self.announced = false;
        self.announcement_landed = false;
        self.agreed = false;
        self.objections.clear();

        match wanted {
            Scope::Off => Step::Nothing,
            // Your own microphone captures only you. Nobody else's consent is
            // involved, so there is nothing to announce.
            Scope::YouOnly => {
                self.scope = Scope::YouOnly;
                self.state = State::Recording;
                Step::Start(Scope::YouOnly)
            }
            Scope::Everyone => {
                if self.cfg.ask_every_call {
                    return Step::AskYou(
                        "Record the whole call? I'll tell everyone first.".into(),
                    );
                }
                self.begin_announcement()
            }
        }
    }

    /// You said yes to recording everyone.
    ///
    /// `cfg.enabled` is checked here too, and not only in `call_started`.
    /// The question and the answer are separated by a person thinking about
    /// it, and the switch can be turned off in between — an answer that
    /// starts a recording the feature is switched off for is a recording
    /// nobody could have expected. It is also the one entry point a spoken
    /// "yes, record it" could reach without `call_started` having run at all.
    pub fn you_approved(&mut self) -> Step {
        if !self.cfg.enabled {
            return Step::Nothing;
        }
        self.begin_announcement()
    }

    pub fn you_declined(&mut self) -> Step {
        if !self.cfg.enabled {
            return Step::Nothing;
        }
        self.scope = Scope::YouOnly;
        self.state = State::Recording;
        Step::Start(Scope::YouOnly)
    }

    fn begin_announcement(&mut self) -> Step {
        self.state = State::Announcing;
        self.announced = true;
        if self.cfg.ask_the_others {
            Step::Announce(self.cfg.question.clone())
        } else {
            Step::Announce(self.cfg.announcement.clone())
        }
    }

    /// The others said yes. Only this starts recording them when Atlas
    /// asked rather than told — a question that nobody answered is not a yes.
    pub fn they_agreed(&mut self) -> Step {
        if !self.cfg.enabled || self.state != State::Asking {
            return Step::Nothing;
        }
        if !self.objections.is_empty() {
            self.state = State::Stopped;
            self.scope = Scope::YouOnly;
            return Step::AskYou(format!(
                "{} said no, so I'm only noting your side.",
                self.objections.join(" and ")
            ));
        }
        self.agreed = true;
        self.scope = Scope::Everyone;
        self.state = State::Recording;
        Step::Start(Scope::Everyone)
    }

    /// Nobody answered the question. Silence is not a yes.
    pub fn no_answer(&mut self) -> Step {
        if self.state != State::Asking {
            return Step::Nothing;
        }
        self.state = State::Recording;
        self.scope = Scope::YouOnly;
        Step::AskYou("Nobody said yes, so I'm only noting your side.".into())
    }

    /// The announcement was actually delivered — spoken aloud, or posted in
    /// the chat and confirmed sent.
    pub fn announcement_delivered(&mut self) -> Step {
        // Objections FIRST, before the state check, and the order is what
        // makes this worth having.
        //
        // The two race by their nature: the announcement is what people are
        // responding to, so an objection arriving in the same moment it lands
        // is the normal case rather than the strange one. Starting here would
        // record someone who had already said no, and
        // `recorded_others_without_announcing()` would report nothing wrong —
        // the announcement did land.
        //
        // Checked ahead of `state != Announcing` because `someone_objected`
        // moves the state out of `Announcing` itself, so behind that check
        // this would be code that can never run. The invariant is about the
        // objection, not about the state: nothing starts capturing everyone
        // while one stands, whatever the machine thinks it is doing.
        if !self.objections.is_empty() {
            self.state = State::Stopped;
            self.scope = Scope::YouOnly;
            return Step::AskYou(format!(
                "{} asked me not to record, so I'm only capturing your side.",
                self.objections.join(" and ")
            ));
        }
        if self.state != State::Announcing {
            return Step::Nothing;
        }
        self.announcement_landed = true;
        if self.cfg.ask_the_others {
            // Asked, not told: keep noting your side and wait for the yes.
            self.state = State::Asking;
            self.scope = Scope::YouOnly;
            return Step::Start(Scope::YouOnly);
        }
        self.scope = Scope::Everyone;
        self.state = State::Recording;
        Step::Start(Scope::Everyone)
    }

    /// The announcement could not be delivered — muted, no chat, call
    /// dropped. **Silence is not consent**, so nothing is recorded.
    pub fn announcement_failed(&mut self, why: &str) -> Step {
        self.state = State::Idle;
        self.scope = Scope::YouOnly;
        Step::AskYou(format!(
            "I couldn't tell everyone I'm recording ({why}), so I'm only capturing your side."
        ))
    }

    /// Someone objected. One is enough.
    ///
    /// ## The window this used to have
    ///
    /// It was `if self.state != State::Recording { return Step::Nothing }` —
    /// the objection was pushed onto the list and then thrown away unless
    /// recording had already started.
    ///
    /// `State::Announcing` is the window in which the announcement is being
    /// delivered, which is **the moment people actually object**: they have
    /// just been told. An objection there did nothing, and
    /// `announcement_delivered` then set `announcement_landed = true`,
    /// `scope = Everyone`, `state = Recording`, and started.
    ///
    /// So somebody said "please don't record me" and Atlas recorded the call.
    /// `recorded_others_without_announcing()` — described as "the invariant
    /// the whole module exists to hold" — returned false, because the
    /// announcement had landed. The invariant was intact and the promise was
    /// not; announcing is what the module measures, and consent is what it is
    /// for.
    pub fn someone_objected(&mut self, who: &str) -> Step {
        self.objections.push(who.to_string());
        match self.state {
            State::Recording => {
                self.state = State::Stopped;
                self.scope = Scope::Off;
                Step::StopAndDiscard(format!(
                    "{who} asked me not to record, so I've stopped and deleted it."
                ))
            }
            // Objected to during the announcement. Nothing has been captured
            // yet, so there is nothing to discard — but the recording must
            // not start, and `announcement_delivered` checks the same list in
            // case the two arrive together.
            State::Announcing | State::Asking => {
                self.state = State::Stopped;
                self.scope = Scope::YouOnly;
                Step::AskYou(format!(
                    "{who} asked me not to record, so I won't — I'm only capturing your side."
                ))
            }
            // Not recording and not about to. Still remembered: the objection
            // is a fact about this call, and `call_started` is what clears
            // the list for the next one.
            _ => Step::Nothing,
        }
    }

    pub fn call_ended(&mut self) -> Step {
        match self.state {
            State::Recording => {
                self.state = State::Idle;
                Step::StopAndKeep
            }
            State::Announcing => {
                self.state = State::Idle;
                Step::StopAndDiscard("the call ended before I could tell anyone".into())
            }
            // Your side was being noted while waiting for a yes; that's yours.
            State::Asking => {
                self.state = State::Idle;
                Step::StopAndKeep
            }
            _ => Step::Nothing,
        }
    }

    /// Is Atlas capturing anyone but you right now?
    pub fn capturing_others(&self) -> bool {
        self.state == State::Recording && self.scope == Scope::Everyone
    }

    /// The indicator shown while recording. Visible, always.
    pub fn indicator(&self) -> Option<String> {
        match (self.state, self.scope) {
            (State::Recording, Scope::Everyone) => Some("recording the call".into()),
            (State::Recording, Scope::YouOnly) => Some("noting your side".into()),
            _ => None,
        }
    }

    /// Could this recording have happened without everyone knowing?
    ///
    /// The invariant the whole module exists to hold. Anything but false here
    /// is a bug.
    pub fn recorded_others_without_announcing(&self) -> bool {
        self.capturing_others() && (!self.announcement_landed || (self.cfg.ask_the_others && !self.agreed))
    }
}

/// The exact words, so you can see them before agreeing to them.
///
/// Wording matters more than it looks. It has to be short enough that nobody
/// resents it interrupting, clear enough that "notes" isn't mistaken for a
/// full recording being published somewhere, and it has to make declining
/// easy — an announcement people feel awkward objecting to isn't consent.
pub const ANNOUNCEMENTS: &[(&str, &str)] = &[
    (
        "plain",
        "Heads up — I've got an assistant taking notes on this call. Say if you'd rather I didn't.",
    ),
    (
        "brief",
        "Note-taking assistant is on for this call. Shout if that's a problem.",
    ),
    (
        "formal",
        "For transparency: this call is being transcribed by an assistant so I can write up notes afterwards. Please let me know if you'd prefer I turn it off.",
    ),
    (
        "casual",
        "FYI I'm having an assistant take notes so I don't have to type while we talk. Happy to switch it off.",
    ),
];

/// What Atlas would put in the chat, end to end.
///
/// Four messages, and only the first is guaranteed to appear — the rest
/// depend on what happens.
pub fn script(cfg: &ConsentConfig) -> Vec<(&'static str, String)> {
    vec![
        ("on joining", the_announcement(cfg)),
        (
            "if someone objects",
            "Understood — recording off, and I've deleted what there was.".to_string(),
        ),
        (
            "only if someone asks what it does",
            "It transcribes locally on my machine and writes up the decisions and actions. Nothing is uploaded and nothing is shared.".to_string(),
        ),
    ]
}

/// One line of `script`, by when it's used.
pub fn script_line(cfg: &ConsentConfig, when: &str) -> Option<String> {
    script(cfg).into_iter().find(|(w, _)| *w == when).map(|(_, s)| s)
}

/// The announcement to use: `call_notes.announcement` may be one of the
/// names in `ANNOUNCEMENTS` ("plain", "brief", "formal", "casual") or your
/// own sentence.
pub fn the_announcement(cfg: &ConsentConfig) -> String {
    announcement_named(cfg.announcement.trim()).map(str::to_string).unwrap_or_else(|| cfg.announcement.clone())
}

pub fn announcement_named(name: &str) -> Option<&'static str> {
    ANNOUNCEMENTS.iter().find(|(n, _)| *n == name).map(|(_, text)| *text)
}

/// Who gets told what, in plain terms.
///
/// There are two different announcements and it's worth being exact about
/// which is which:
///
/// * **To you** — Atlas confirms it's about to record. Always happens.
/// * **To the call** — Atlas tells the other people. Only when it's about to
///   capture *them*, and never skippable, because that one isn't courtesy.
pub fn who_gets_told(scope: Scope) -> &'static str {
    match scope {
        Scope::Off => "nobody, because nothing is being recorded",
        Scope::YouOnly => "only you — your microphone captures you and nobody else",
        Scope::Everyone => "you, and then everyone on the call before anything is captured",
    }
}

/// A plain answer to "what will you do on calls?"
pub fn explain(cfg: &ConsentConfig) -> String {
    if !cfg.enabled {
        return "I don't record calls.".into();
    }
    let base = match cfg.default_scope {
        Scope::Off => "I don't record unless you ask.",
        Scope::YouOnly => "I note your side only, which captures nobody else.",
        Scope::Everyone if cfg.ask_the_others => "I record the whole call once everyone has said yes.",
        Scope::Everyone => "I record the whole call, and I tell everyone first.",
    };
    let others = if cfg.ask_the_others {
        "To record anyone else I ask them first and wait for a yes; no answer means your side only"
    } else {
        "To record everyone I always announce it"
    };
    // `assume`: which consent rule to act on. Atlas can't know where the
    // others are, so it says which it's working to.
    let rule = match cfg.assume {
        Rule::AllParties => "I work to the stricter rule, that everyone on a call has to agree.",
        Rule::OneParty => "I work to the rule that your consent is enough for your own calls, but I still ask before recording anyone else.",
    };
    format!(
        "{base} {others}, and if anyone objects I stop and delete it. \
         Audio is deleted after {} days; notes are kept. {rule}",
        cfg.keep_audio_days
    )
}
