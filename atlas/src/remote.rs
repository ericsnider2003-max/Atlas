//! Asking the laptop to do something while you're not at it.
//!
//! You're out with your phone, the laptop is at home and on. Anything that
//! needs the bigger model, your files, or the browser should happen there —
//! and you should hear about it when it's done, not have to remember to check.
//!
//! The rule that keeps this sane: **the phone asks, it doesn't command.** The
//! laptop decides whether it can, does the work, and reports back. If the
//! laptop is off, the request waits rather than failing, because a request
//! that quietly disappears is worse than one that's slow.

use serde::{Deserialize, Serialize};

/// Something asked of another device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    /// In your words.
    pub what: String,
    /// Which device it needs.
    pub needs: Needs,
    pub asked_at: u64,
    pub state: State,
    /// How you want to hear.
    pub tell_me: How,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {
    /// Files, windows, the browser, the big model.
    TheLaptop,
    /// Could run anywhere.
    Anything,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// The laptop hasn't seen it.
    Waiting,
    Running,
    Done,
    Failed,
    /// You cancelled it.
    Dropped,
}

/// How you find out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    /// Spoken in your ear, if something's in it.
    InYourEar,
    /// A notification.
    Quietly,
    /// Nothing — you'll ask.
    DontTellMe,
}

impl How {
    /// Should this interrupt you?
    ///
    /// Goes through the same gate as everything else: something finishing
    /// isn't automatically worth your attention.
    pub fn interrupts(&self, took_secs: u64, failed: bool) -> bool {
        match self {
            How::DontTellMe => false,
            // A failure is worth hearing whatever it cost.
            _ if failed => true,
            // Something that finished in twenty seconds isn't news — you were
            // still holding the phone.
            _ => took_secs > 45,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    pub enabled: bool,
    /// Give up on a waiting request after this long.
    pub wait_days: u32,
    /// Default way of hearing back.
    pub tell_me: String,
    /// Ask before running something that changes things.
    pub confirm_side_effects: bool,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        RemoteConfig {
            enabled: false,
            // Long enough to survive a trip.
            wait_days: 30,
            tell_me: "in_your_ear".into(),
            confirm_side_effects: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Queue {
    pub requests: Vec<Request>,
    next_id: u64,
}

impl Queue {
    pub fn ask(&mut self, what: &str, needs: Needs, tell_me: How, now: u64) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.requests.push(Request {
            id,
            what: what.into(),
            needs,
            asked_at: now,
            state: State::Waiting,
            tell_me,
        });
        id
    }

    /// What the laptop should pick up when it next hears from the phone.
    pub fn for_the_laptop(&self) -> Vec<&Request> {
        self.requests.iter().filter(|r| r.state == State::Waiting).collect()
    }

    pub fn set(&mut self, id: u64, state: State) -> bool {
        match self.requests.iter_mut().find(|r| r.id == id) {
            Some(r) => {
                r.state = state;
                true
            }
            None => false,
        }
    }

    /// Things that have been waiting too long.
    ///
    /// Said once, so you know it never happened rather than assuming it did.
    pub fn given_up(&self, now: u64, cfg: &RemoteConfig) -> Vec<&Request> {
        self.requests
            .iter()
            .filter(|r| {
                r.state == State::Waiting
                    && now.saturating_sub(r.asked_at) > cfg.wait_days as u64 * 86_400
            })
            .collect()
    }

    pub fn waiting_count(&self) -> usize {
        self.requests.iter().filter(|r| r.state == State::Waiting).count()
    }
}

/// What Atlas on the phone says when you ask for something it can't do there.
pub fn handing_off(what: &str, laptop_reachable: bool) -> String {
    if laptop_reachable {
        "That needs the laptop. Sending it over — I'll tell you when it's done.".to_string()
    } else {
        "That needs the laptop and it's not reachable. I've queued it, and it'll go the \
             moment we're back in touch.".to_string()
    }
    .replace("That needs", &format!("{what} needs"))
}

/// What you hear when it's finished.
pub fn finished(r: &Request, took_secs: u64, result: &str) -> Option<String> {
    if !r.tell_me.interrupts(took_secs, r.state == State::Failed) {
        return None;
    }
    Some(match r.state {
        State::Failed => format!("{} didn't work — {result}", r.what),
        _ => {
            let mins = took_secs / 60;
            let took = if mins >= 1 {
                format!("{mins} minute{}", if mins == 1 { "" } else { "s" })
            } else {
                format!("{took_secs} seconds")
            };
            format!("{} is done, took {took}. {result}", r.what)
        }
    })
}

/// Something you asked for that never ran.
pub fn never_ran(r: &Request, days: u64) -> String {
    format!(
        "\"{}\" has been waiting {days} days and the laptop hasn't been on. Still want it?",
        r.what
    )
}

/// Whether it's worth waking the laptop for.
///
/// If you have wake-on-LAN set up, some things are worth it and most aren't.
pub fn worth_waking_for(needs: Needs, urgent: bool) -> bool {
    urgent && needs == Needs::TheLaptop
}

// ============ the gate before the other machine runs it ============
//
// `confirm_side_effects` ships **on** — "ask before running something that
// changes things" — and nothing read it, so `atlas remote start 3` ran
// whatever the text said with no gate at all. A safety switch that is on by
// default and connected to nothing is the worst of the three states: the file
// says you are protected, and you are not.
//
// The rule is `policy.rs`'s, borrowed rather than reinvented: **unclassified
// is not safe.** A request goes through without a word only when what it asks
// for is recognisably a reading. Everything else is a change until you say
// otherwise, because the cost of being wrong is asymmetric — an unnecessary
// "yes" costs a keystroke, and a missed one ran something on a machine you
// were not sitting at.

/// What a queued request looks like, as far as text can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Looks {
    /// It asks to be told something. Nothing moves.
    LikeAReading,
    /// It might change something, or it might not, and nothing here can tell.
    Unknown,
}

/// The words that open a request for information rather than for work.
///
/// Deliberately short and deliberately about the *opening* word. "Check the
/// render finished" is a reading; "render the draft and check it finished"
/// is not, and a scan for the word anywhere would have called both readings.
const READINGS: &[&str] = &[
    "read", "check", "look", "show", "list", "find", "count", "what", "which",
    "when", "where", "how", "is", "are", "does", "did", "tell me",
];

/// Does this look like it only wants to be told something?
///
/// Named `reading_or_change` rather than `looks_like`: `cloudsync` already
/// has a free function by that name, and a second one would have made the
/// first look like it had a caller it does not have.
pub fn reading_or_change(what: &str) -> Looks {
    let w = what.trim().to_lowercase();
    let first_word_is_a_reading = READINGS.iter().any(|r| {
        w.starts_with(r)
            && w[r.len()..].chars().next().map(|c| !c.is_alphanumeric()).unwrap_or(true)
    });
    if !first_word_is_a_reading {
        return Looks::Unknown;
    }
    // A reading that goes on to ask for work is not a reading. This is the
    // half that stops the opening word from being a way past the gate.
    const THEN_DOES: &[&str] =
        &[" and then ", " then ", " and delete", " and send", " and move", " and run", " and post"];
    if THEN_DOES.iter().any(|t| w.contains(t)) {
        return Looks::Unknown;
    }
    Looks::LikeAReading
}

/// Does running this need a yes from you first?
pub fn needs_your_yes(what: &str, cfg: &RemoteConfig) -> bool {
    cfg.confirm_side_effects && reading_or_change(what) == Looks::Unknown
}

/// What to say instead of running it.
pub fn asking_before_it_runs(id: u64, what: &str) -> String {
    format!(
        "\"{what}\" isn't obviously just a read, and you've asked me to check before running \
         anything that changes things. `atlas remote start {id} --yes` to go ahead, or turn off \
         `confirm_side_effects` if you'd rather I never asked."
    )
}

// ============ the phone and the laptop, over sync (item 24) ============
//
// The phone asks with a sync event in your sealed bundles; the laptop
// answers the same way. Only your own devices take either: a bundle that
// isn't sealed with your household key is never read for these.

/// A request from a phone: `ask:<device>:<n>`.
pub const ASK_PREFIX: &str = "ask:";
/// Your yes to a held one: `askyes:<device>:<n>`.
pub const YES_PREFIX: &str = "askyes:";
/// The laptop's answer: `answer:<device>:<n>`.
pub const ANSWER_PREFIX: &str = "answer:";

/// The request in "ask the laptop to find the contract", "on my laptop,
/// check the render finished", "have the laptop look up …". `None` when the
/// words don't hand anything to the laptop.
pub fn handed_over(said: &str) -> Option<String> {
    let s = said.trim().trim_end_matches(['.', '!', '?']).trim();
    let l = s.to_ascii_lowercase();
    const LEADS: &[&str] = &[
        "ask the laptop to ", "ask my laptop to ", "ask the computer to ", "ask my computer to ",
        "have the laptop ", "have my laptop ", "get the laptop to ", "get my laptop to ",
        "tell the laptop to ", "tell my laptop to ", "on the laptop, ", "on my laptop, ",
        "on the laptop ", "on my laptop ", "on my computer, ", "on my computer ",
    ];
    let lead = LEADS.iter().find(|p| l.starts_with(*p))?;
    let rest = s[lead.len()..].trim().trim_start_matches(',').trim();
    // "on my laptop is there…" asks about the laptop, it doesn't hand it work.
    (rest.split_whitespace().count() >= 2).then(|| rest.to_string())
}

/// "Go ahead on the laptop", "yes, run it on the laptop".
pub fn go_ahead_on_the_laptop(said: &str) -> bool {
    let l = said.trim().trim_end_matches(['.', '!']).to_lowercase();
    matches!(
        l.trim_start_matches("yes, ").trim_start_matches("yes ").trim(),
        "go ahead on the laptop" | "go ahead on my laptop" | "run it on the laptop" | "run it on my laptop" | "do it on the laptop" | "do it on my laptop"
    )
}

pub fn ask_to_carry(device: &str, n: u64, what: &str, at: u64) -> (String, String, String) {
    (format!("{ASK_PREFIX}{device}:{n}"), "ask".into(), serde_json::json!({ "what": what, "at": at }).to_string())
}

/// `(device, n, what, at)` from an ask event.
pub fn read_ask(id: &str, to: &str) -> Option<(String, u64, String, u64)> {
    let (device, n) = id.strip_prefix(ASK_PREFIX)?.rsplit_once(':')?;
    let v: serde_json::Value = serde_json::from_str(to).ok()?;
    let what = v.get("what")?.as_str()?.trim();
    if device.is_empty() || what.is_empty() || what.len() > 2000 {
        return None;
    }
    Some((device.into(), n.parse().ok()?, what.into(), v.get("at").and_then(|a| a.as_u64()).unwrap_or(0)))
}

pub fn yes_to_carry(device: &str, n: u64) -> (String, String, String) {
    (format!("{YES_PREFIX}{device}:{n}"), "askyes".into(), "yes".into())
}

pub fn read_yes(id: &str) -> Option<(String, u64)> {
    let (device, n) = id.strip_prefix(YES_PREFIX)?.rsplit_once(':')?;
    Some((device.into(), n.parse().ok()?))
}

pub fn answer_to_carry(device: &str, n: u64, what: &str, text: &str, done: bool) -> (String, String, String) {
    (
        format!("{ANSWER_PREFIX}{device}:{n}"),
        "answer".into(),
        serde_json::json!({ "what": what, "text": text, "done": done }).to_string(),
    )
}

/// `(device, n, what, text, done)` from an answer event.
pub fn read_answer(id: &str, to: &str) -> Option<(String, u64, String, String, bool)> {
    let (device, n) = id.strip_prefix(ANSWER_PREFIX)?.rsplit_once(':')?;
    let v: serde_json::Value = serde_json::from_str(to).ok()?;
    Some((
        device.into(),
        n.parse().ok()?,
        v.get("what")?.as_str()?.into(),
        v.get("text")?.as_str()?.into(),
        v.get("done").and_then(|d| d.as_bool()).unwrap_or(true),
    ))
}

/// Said on the phone straight away.
pub fn sent_to_the_laptop(what: &str) -> String {
    format!(
        "Sent \"{what}\" to your laptop -- I'll tell you when it answers. If it's asleep, it runs when it wakes."
    )
}

/// The laptop's answer for something it won't run without you.
pub fn held_for_your_yes(what: &str) -> String {
    format!(
        "\"{what}\" might change something on the laptop, so it's waiting for your yes. Say \"go ahead on the laptop\" \
         and it runs."
    )
}
