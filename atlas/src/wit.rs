//! How much of a smart-ass Atlas may be.
//!
//! Eric, 29 Sep 2026: "Can we give Atlas the ability to be a smart ass". Yes,
//! with three levels -- `off`, `dry` and `full` -- and a fence around it that
//! no level gets past.
//!
//! **What it changes.** Wording only: the line the model is given about
//! humour, and a short tail on a handful of canned replies (a greeting, a
//! thanks, an app opened). Never a fact, never a number, never whether a
//! thing worked, and never the order: the answer comes first, whole, and the
//! remark is at most a clause after it. `dress` is the only place a canned
//! reply gains a tail, and it can only *append* to what it was given, so the
//! answer is the same at every level by construction -- the tests check it.
//!
//! **Where it never goes** (`holds`), whatever the level:
//! - anything that went wrong, or when you're frustrated (`Register::Rough`,
//!   which already reads a failed last turn and fed-up wording);
//! - a reply that reads as a failure ("I couldn't", "failed", "not found");
//! - security, money, health, or bad news -- in what you said or in the
//!   answer;
//! - in the middle of a security, vault or confirmation step -- a sign-in,
//!   a two-factor code, the vault, pairing, a "shall I?" waiting on your
//!   yes (`Moment::in_a_flow`, which the daemon sets from what it is waiting
//!   on and from `fenced_intent`);
//! - anything written for somebody else: a draft, a message, a reply sent on
//!   your behalf. Those paths never read the persona at all (`draft`,
//!   `outreach`, `messaging` and `outbox` build their own words), and
//!   `Moment::for_someone_else` is the fence for anything that might.
//!
//! **Default: `dry`.** The shipped `persona.wit` was the number 0.35, which
//! `Persona::prompt_for` read as "an occasional dry aside is fine" in
//! conversation. `dry` produces exactly that prompt, word for word, and
//! leaves every canned reply as it was -- so a fresh install and an upgraded
//! one behave as they did before this setting existed until someone turns it
//! up. (The brief suggested `off`; `off` would have *removed* the aside Atlas
//! already had, which is a change nobody asked for.) An old config holding a
//! number still reads: 0 is `off`, up to 0.5 is `dry`, above is `full`.

use crate::register::Register;
use serde::{Deserialize, Serialize};

/// The three levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Wit {
    /// Straight answers. No jokes, no sarcasm.
    Off,
    /// An occasional dry aside in conversation. What Atlas always did.
    #[default]
    Dry,
    /// A smart-ass: a sharp remark after the answer, in conversation and
    /// on the small canned replies, still inside every fence.
    Full,
}

/// The words the setting takes, in order. The hub's choice list.
pub const LEVELS: &[&str] = &["off", "dry", "full"];

impl Wit {
    pub fn word(self) -> &'static str {
        match self {
            Wit::Off => "off",
            Wit::Dry => "dry",
            Wit::Full => "full",
        }
    }

    /// What it means, for the settings page and for saying it back.
    pub fn plain(self) -> &'static str {
        match self {
            Wit::Off => "straight answers, no jokes",
            Wit::Dry => "the odd dry aside, in conversation only",
            Wit::Full => "a smart-ass, after the answer and never about anything serious",
        }
    }

    /// Read what a person or a config file wrote.
    pub fn parse(raw: &str) -> Option<Wit> {
        let t: String = raw.trim().to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_ascii_digit() || *c == '.').collect();
        match t.as_str() {
            "off" | "none" | "no" | "false" | "serious" | "straight" => Some(Wit::Off),
            "dry" | "some" | "default" | "on" | "true" => Some(Wit::Dry),
            "full" | "smartass" | "smartarse" | "sarcastic" | "max" | "high" => Some(Wit::Full),
            n => n.parse::<f64>().ok().map(Wit::from_number),
        }
    }

    /// The old 0-to-1 dial, read as a level.
    fn from_number(n: f64) -> Wit {
        if n <= 0.0 {
            Wit::Off
        } else if n <= 0.5 {
            Wit::Dry
        } else {
            Wit::Full
        }
    }

    pub fn up(self) -> Wit {
        match self {
            Wit::Off => Wit::Dry,
            _ => Wit::Full,
        }
    }

    pub fn down(self) -> Wit {
        match self {
            Wit::Full => Wit::Dry,
            _ => Wit::Off,
        }
    }
}

impl Serialize for Wit {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.word())
    }
}

impl<'de> Deserialize<'de> for Wit {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Wit, D::Error> {
        // A word ("dry", "smart-ass") or the old number (0.35).
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            N(f64),
            S(String),
        }
        match Either::deserialize(d)? {
            Either::N(n) => Ok(Wit::from_number(n)),
            Either::S(s) => Wit::parse(&s).ok_or_else(|| {
                serde::de::Error::custom(format!("persona.wit must be one of {}, not {s:?}", LEVELS.join(", ")))
            }),
        }
    }
}

// ---------------------------------------------------------------- the fence

/// What is known about the moment a remark would land in.
#[derive(Debug, Clone, Copy)]
pub struct Moment<'a> {
    pub register: Register,
    /// What you said this turn. Empty when it isn't known.
    pub said: &'a str,
    /// The answer the remark would follow. Empty when it isn't written yet.
    pub reply: &'a str,
    /// A draft, a message, anything with somebody else as its reader.
    pub for_someone_else: bool,
    /// A security, vault or confirmation step is under way.
    pub in_a_flow: bool,
}

impl<'a> Moment<'a> {
    pub fn new(register: Register, said: &'a str, reply: &'a str) -> Moment<'a> {
        Moment { register, said, reply, for_someone_else: false, in_a_flow: false }
    }

    /// The same moment, during a security, vault or confirmation step.
    pub fn during_a_flow(self, yes: bool) -> Moment<'a> {
        Moment { in_a_flow: self.in_a_flow || yes, ..self }
    }
}

/// Commands that are themselves a security, vault, money or confirmation
/// step: their replies stay plain at every level, whatever the words.
/// ("Signing in to the bank now. Another triumph." is not a joke anyone
/// wants.)
pub fn fenced_intent(i: &crate::intent::Intent) -> bool {
    use crate::intent::Intent as I;
    matches!(
        i,
        I::CreateAccount(_)
            | I::SignIn(_)
            | I::TypeCode(_)
            | I::TwoFactor(_)
            | I::SetKey(_)
            | I::Unlock(_)
            | I::HandOver(_)
            | I::TakeItBack
            | I::Pair(_)
            | I::AcceptPairing(_)
            | I::ForgetPeer(_)
            | I::MoneyAdvice(_)
            | I::Receipt(_)
            | I::TradeDay(_)
            | I::MarketDay(_)
            | I::Undo
            | I::BackUp
            | I::Diagnose(_)
            | I::Feedback(_)
    )
}

/// Why a remark is held back. `None` from `holds` means it may be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    /// The setting is off.
    Off,
    /// You're frustrated or something just failed.
    Rough,
    /// The answer itself reads as something not working.
    AnError,
    /// Security, money, health or bad news.
    Serious,
    /// Written for someone else.
    NotYours,
    /// A security, vault or confirmation step is under way.
    MidFlow,
    /// `dry` stays in conversation; a task gets a plain confirmation.
    NotAConversation,
}

/// Security, money, health and bad news. Whole words or whole phrases, so
/// "pain" does not fire on "painting".
const SERIOUS: &[&str] = &[
    // security
    "password", "passphrase", "passcode", "vault", "hacked", "breach", "breached", "scam", "scammed",
    "phishing", "2fa", "two factor", "locked out", "virus", "malware", "ransomware", "security",
    "stolen", "fraud", "leaked", "recovery key",
    // money
    "money", "pay", "paid", "payment", "payments", "invoice", "bank", "debt", "loan", "mortgage", "rent",
    "bill", "bills", "tax", "taxes", "salary", "wage", "budget", "refund", "credit card", "overdraft",
    "broke", "savings", "invest", "investment", "crypto", "owe", "owed", "price", "cost", "costs",
    "spend", "spent", "expensive",
    // health
    "health", "doctor", "hospital", "sick", "ill", "illness", "pain", "symptom", "symptoms",
    "medication", "medicine", "diagnosis", "diagnosed", "therapy", "surgery", "injury", "injured",
    "hurt", "ambulance", "emergency", "pregnant", "cancer", "clinic", "prescription",
    // bad news
    "died", "dead", "death", "passed away", "funeral", "divorce", "fired", "laid off", "layoff",
    "lost my", "accident", "bad news", "grief", "grieving", "miscarriage", "breakup", "broke up",
    "rejected", "eviction", "evicted",
];

/// A reply that says something didn't work.
const FAILURE: &[&str] = &[
    "couldn't", "could not", "can't", "cannot", "failed", "failing", "error", "didn't work",
    "did not work", "not found", "unable", "refused", "won't", "went wrong", "isn't working",
    "is not working", "not working", "broken", "sorry", "no such", "timed out", "denied",
    "blocked", "offline", "missing",
];

fn words_of(s: &str) -> String {
    let t: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' || c == '$' || c == '£' || c == '€' { c } else { ' ' })
        .collect();
    format!(" {} ", t.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn mentions(text: &str, list: &[&str]) -> bool {
    let w = words_of(text);
    list.iter().any(|p| w.contains(&format!(" {p} ")))
}

/// Is this about security, money, health or bad news?
pub fn is_serious(text: &str) -> bool {
    text.contains(['$', '£', '€']) || mentions(text, SERIOUS)
}

/// Does this read as something not working?
fn reads_as_failure(text: &str) -> bool {
    mentions(text, FAILURE)
}

/// Whether a remark may be made here, and if not, why not.
///
/// Order matters only for the reason given: every fence is checked, and any
/// one of them holds the remark back.
pub fn holds_back(level: Wit, m: &Moment) -> Option<Held> {
    if level == Wit::Off {
        return Some(Held::Off);
    }
    if m.for_someone_else {
        return Some(Held::NotYours);
    }
    if m.in_a_flow {
        return Some(Held::MidFlow);
    }
    if m.register == Register::Rough {
        return Some(Held::Rough);
    }
    if reads_as_failure(m.reply) {
        return Some(Held::AnError);
    }
    if is_serious(m.said) || is_serious(m.reply) {
        return Some(Held::Serious);
    }
    if level == Wit::Dry && !m.register.humour_welcome() {
        return Some(Held::NotAConversation);
    }
    None
}

// ---------------------------------------------------------------- the prompt

/// The line the model is given about humour, or nothing.
///
/// `dry` is the two sentences `Persona` has always used, word for word, so
/// the default changes nothing the model is told. `said` is what was said
/// this turn when it's known; a serious subject takes the line away and puts
/// a plain "no jokes" in its place, because a model left to its own
/// judgement on a question about a bill will sometimes be charming about it.
pub fn prompt_line(level: Wit, m: &Moment, per_turn: bool) -> String {
    match holds_back(level, m) {
        None => match level {
            Wit::Off => String::new(),
            Wit::Dry if per_turn => " A dry aside is fine when it's actually funny.".into(),
            Wit::Dry => "\n\nAn occasional dry aside is fine. Rarely, and never instead of the answer.".into(),
            Wit::Full => {
                let rule = "You're allowed to be a bit of a smart-ass: one sharp, playful remark, at most a \
                            clause, after the answer and never instead of it or before it. Tease the situation, \
                            or the user lightly -- never anyone else, and nothing written for someone else is ever sarcastic. The facts, numbers and whether something worked stay exactly the same. \
                            Never about security, money, health or bad news.";
                if per_turn { format!(" {rule}") } else { format!("\n\n{rule}") }
            }
        },
        // Serious subjects, and a full setting during a task: say so, rather
        // than leave it to the model.
        Some(Held::Serious) | Some(Held::MidFlow) => {
            if per_turn { " No jokes: this is a serious subject.".into() } else { "\n\nNo jokes: this is a serious subject.".into() }
        }
        Some(Held::Off) => {
            if per_turn { " No jokes or sarcasm.".into() } else { String::new() }
        }
        // Rough already says "no jokes" in its own words; a task under `dry`
        // gets nothing, exactly as before.
        Some(_) => String::new(),
    }
}

// ---------------------------------------------------------------- canned replies

/// Which canned reply this is, so the right kind of tail is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Canned {
    Greeting,
    Thanks,
    Goodbye,
    /// An action the phrase parser settled: "Opening Chrome now."
    Done,
}

/// The witty tails. Each is one short clause, said after the answer; none
/// mentions a fact, a number or the user's failings.
const TAILS: &[(Canned, &[&str])] = &[
    (Canned::Greeting, &["Try to look busy.", "I've been awake for ages, obviously.", "Let's make it look easy."]),
    (Canned::Thanks, &["I'll put it on your tab.", "Try to contain your gratitude.", "Noted, framed, hung on the wall."]),
    (Canned::Goodbye, &["Not like I've anywhere to be.", "I'll hold the fort.", "Don't do anything I'd have to log."]),
    (Canned::Done, &["Try to act surprised.", "Another triumph.", "Hardly broke a sweat.", "You're welcome, in advance."]),
];

/// A tail for this kind of reply, or `None` this time.
///
/// Only at `full`, and only every other seed: a joke on every single reply
/// stops being a joke by lunch.
fn tail(level: Wit, kind: Canned, seed: u64) -> Option<&'static str> {
    if level != Wit::Full || seed % 2 == 1 {
        return None;
    }
    let list = TAILS.iter().find(|(k, _)| *k == kind).map(|(_, l)| *l)?;
    Some(list[((seed / 2) as usize) % list.len()])
}

/// A canned reply, with a tail when every fence allows one.
///
/// Only ever appends: the answer is `plain`, whole and first, at every
/// level.
pub fn dress(level: Wit, plain: &str, kind: Canned, m: &Moment, seed: u64) -> String {
    let plain = plain.trim();
    if plain.is_empty() {
        return String::new();
    }
    let m = Moment { reply: plain, ..*m };
    if holds_back(level, &m).is_some() {
        return plain.to_string();
    }
    match tail(level, kind, seed) {
        Some(t) => {
            let sep = if plain.ends_with(['.', '!', '?']) { " " } else { ". " };
            format!("{plain}{sep}{t}")
        }
        None => plain.to_string(),
    }
}

// ---------------------------------------------------------------- by voice

/// A change to the level asked for out loud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    Up,
    Down,
    To(Wit),
}

/// "Be more of a smart-ass", "tone it down", "no more jokes".
///
/// Whole sentences only -- the words, with the wake word, "please" and
/// "can you" set aside, must be one of these -- so nothing longer is
/// swallowed: "tone it down in the email to Maya" is not this.
pub fn level_asked(said: &str) -> Option<Asked> {
    let mut t: String = said
        .to_lowercase()
        .replace(['-', '’'], " ")
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '\'')
        .collect();
    t = t.split_whitespace().filter(|w| *w != "atlas").collect::<Vec<_>>().join(" ");
    for lead in ["please ", "can you ", "could you ", "would you ", "just ", "ok ", "okay "] {
        if let Some(r) = t.strip_prefix(lead) {
            t = r.to_string();
        }
    }
    for end in [" please", " a bit", " a little", " a notch", " for me", " now"] {
        if let Some(r) = t.strip_suffix(end) {
            t = r.to_string();
        }
    }
    let t = t.replace("smartass", "smart ass").replace("smart arse", "smart ass");
    const UP: &[&str] = &[
        "be more of a smart ass", "more of a smart ass", "be more sarcastic", "more sarcasm", "be funnier",
        "more wit", "turn the wit up", "turn up the wit", "more jokes", "be cheekier", "sass me more",
    ];
    const FULL: &[&str] = &[
        "be a smart ass", "smart ass mode", "full smart ass", "go full smart ass", "be as sarcastic as you like",
        "you can be a smart ass", "turn the wit all the way up",
    ];
    const DOWN: &[&str] = &[
        "tone it down", "less sarcasm", "be less sarcastic", "less of a smart ass", "dial it back",
        "dial it down", "less wit", "turn the wit down", "turn down the wit", "fewer jokes", "less jokes",
    ];
    const OFF: &[&str] = &[
        "no more jokes", "no jokes", "stop joking", "stop being a smart ass", "be serious", "no sarcasm",
        "turn off the wit", "turn the wit off", "wit off", "just be straight with me", "straight answers",
    ];
    const DRY: &[&str] = &["just be dry", "be dry", "dry wit", "dry wit only", "wit to dry"];
    if FULL.contains(&t.as_str()) {
        return Some(Asked::To(Wit::Full));
    }
    if UP.contains(&t.as_str()) {
        return Some(Asked::Up);
    }
    if OFF.contains(&t.as_str()) {
        return Some(Asked::To(Wit::Off));
    }
    if DRY.contains(&t.as_str()) {
        return Some(Asked::To(Wit::Dry));
    }
    if DOWN.contains(&t.as_str()) {
        return Some(Asked::Down);
    }
    None
}

impl Asked {
    pub fn from(self, now: Wit) -> Wit {
        match self {
            Asked::Up => now.up(),
            Asked::Down => now.down(),
            Asked::To(w) => w,
        }
    }
}

/// What to say once the level has changed (or was already there). Said in
/// the new level's own voice, and still only wording.
pub fn confirm(before: Wit, after: Wit) -> String {
    if before == after {
        return match after {
            Wit::Full => "That's already as smart-ass as I go. Anything more and it stops being useful.".into(),
            Wit::Off => "Already off. Straight answers only.".into(),
            Wit::Dry => "Already dry: the odd aside, in conversation only.".into(),
        };
    }
    match after {
        Wit::Full => "Fine. Smart-ass it is. Answers first, and never about anything that matters.".into(),
        Wit::Dry => "Toned down: the odd dry aside, in conversation only.".into(),
        Wit::Off => "No more jokes. Straight answers.".into(),
    }
}
