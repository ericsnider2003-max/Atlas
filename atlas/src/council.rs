//! A room of people who do not agree with each other.
//!
//! Asking one model a question four times gets you the same answer four times
//! in different words. Asking four *seats* — each with its own brief, its own
//! disposition, and no sight of the others — gets you the thing a board
//! meeting is actually for: the disagreement.
//!
//! Three rules make this work, and dropping any one of them collapses it back
//! into an expensive way to ask one question:
//!
//! 1. **Seats answer blind.** No seat sees another's answer before giving its
//!    own. Show them and they converge, which is the entire failure this is
//!    built to avoid. `Round::Blind` enforces it and `Round::Open` — the
//!    follow-up round where they *do* see each other — can only happen after.
//! 2. **The verdict names the split.** A verdict that averages four opinions
//!    into a moderate one has destroyed the only thing the council produced.
//!    Where seats disagree, the disagreement is the output.
//!  3. **Unanimity is suspicious, not reassuring.** Four seats agreeing on the
//!    first blind round usually means the brief leaked the answer. It gets
//!    said out loud rather than reported as high confidence.
//!
//! `otherside` argues one case against a decision you have already made. This
//! is the plural version, for decisions you have not made yet.

use serde::{Deserialize, Serialize};

/// What a seat is inclined to worry about. Not a personality — a standing
/// bias, deliberately assigned, so the room covers different ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Disposition {
    /// Wants it shipped. Argues from cost of delay.
    Bias,
    /// Wants it proved. Argues from what could go wrong.
    Sceptic,
    /// Argues from what it costs and what it returns.
    Operator,
    /// Argues from who it is for and whether they asked.
    Customer,
    /// Argues from what it commits you to afterwards.
    Steward,
}

impl Disposition {

    pub fn plain(&self) -> &'static str {
        match self {
            Disposition::Bias => "wants it moving",
            Disposition::Sceptic => "wants it proved",
            Disposition::Operator => "counts the cost",
            Disposition::Customer => "speaks for whoever it is for",
            Disposition::Steward => "thinks about afterwards",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seat {
    pub id: String,
    /// What this seat is, in your words.
    pub brief: String,
    pub disposition: Disposition,
    /// Seats you trust more on some subjects than others. Nothing here scales
    /// a seat past the others — it only breaks ties, because a weighting that
    /// can outvote the room is one seat wearing five hats.
    #[serde(default)]
    pub tiebreak: bool,
}

impl Seat {
    pub fn new(id: &str, brief: &str, disposition: Disposition) -> Seat {
        Seat { id: id.into(), brief: brief.into(), disposition, tiebreak: false }
    }

    fn breaks_ties(mut self) -> Seat {
        self.tiebreak = true;
        self
    }

    /// The prompt this seat answers with. Deliberately does not contain the
    /// question's framing twice — a brief that restates the question as a
    /// good idea produces a seat that agrees it is a good idea.
    pub fn prompt(&self, question: &str) -> String {
        format!(
            "You are {}. You {}.\n\nQuestion: {}\n\nGive your own view. \
             Say what would change your mind. Do not hedge — a seat that will \
             not commit is an empty chair.",
            self.brief,
            self.disposition.plain(),
            question
        )
    }

    /// The prompt for `Round::Open` — every other seat's blind opinion, never
    /// this seat's own, laid out plainly and put back to it. Only meaningful
    /// after a `Round::Blind` pass exists to show, which is rule 1's whole
    /// point: nothing here lets a seat see another's answer before it has
    /// given its own.
    fn open_prompt(&self, question: &str, blind: &[Opinion]) -> String {
        let others: String = blind
            .iter()
            .filter(|o| o.seat != self.id)
            .map(|o| format!("- {} ({}): {}", o.seat, o.lean.plain(), o.because))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "You are {}. You {}.\n\nQuestion: {}\n\nHere is what the rest of the room said, \
             blind, before anyone had seen anyone else's answer:\n{}\n\nDoes that change your \
             view, or hold it? Say what you think now and why. Do not hedge — a seat that will \
             not commit is an empty chair.",
            self.brief,
            self.disposition.plain(),
            question,
            others
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lean {
    For,
    Against,
    Depends,
}

impl Lean {
    pub fn plain(&self) -> &'static str {
        match self {
            Lean::For => "for",
            Lean::Against => "against",
            Lean::Depends => "it depends",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opinion {
    pub seat: String,
    pub lean: Lean,
    /// One line. The reason, not the conclusion.
    pub because: String,
    /// What would move this seat. A seat with no answer here is posturing.
    pub would_change_my_mind: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Round {
    /// Nobody has seen anybody. The only round whose answers are independent.
    Blind,
    /// Seats have read the blind round and may revise.
    Open,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    /// What the room came to, if it came to anything.
    pub call: Option<Lean>,
    /// The split, always. Present even when the room agreed.
    pub split: String,
    /// The strongest thing said against the call. Never empty when there was
    /// a dissent — burying the losing argument is how a council becomes a
    /// rubber stamp.
    pub strongest_dissent: Option<String>,
    /// Set when everyone agreed on the blind round, which is a warning.
    pub suspiciously_unanimous: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Council {
    pub seats: Vec<Seat>,
    /// A seat that says no also says what would make it yes, and the
    /// proposal, changed that way, is put to the room again
    /// (`conditions`, `amended`). Eric, 25 Sep 2026: "If one of them says no
    /// it also needs to suggest what would make it ok then it gets retested."
    pub retests: bool,
}

/// A council needs enough seats to disagree and few enough to read.
pub const MIN_SEATS: usize = 3;
pub const MAX_SEATS: usize = 7;

impl Council {
    pub fn new(seats: Vec<Seat>) -> Council {
        Council { seats, retests: false }
    }

    /// This room's no's come with what would make it yes, and get retested.
    fn with_retests(mut self) -> Council {
        self.retests = true;
        self
    }

    /// A reasonable room when you have not defined one. Deliberately covers
    /// five different grounds rather than five different voices.
    pub fn default_room() -> Council {
        Council::new(vec![
            Seat::new("mover", "someone who ships things", Disposition::Bias),
            Seat::new("sceptic", "someone who has been burned before", Disposition::Sceptic),
            Seat::new("operator", "someone who pays for it", Disposition::Operator),
            Seat::new("user", "the person it is supposed to help", Disposition::Customer),
            Seat::new("steward", "whoever maintains it in a year", Disposition::Steward)
                .breaks_ties(),
        ])
    }

    /// Every prompt for a blind round. Returned together so a caller cannot
    /// accidentally feed one seat's answer into the next seat's context.
    pub fn blind_prompts(&self, question: &str) -> Vec<(String, String)> {
        self.seats
            .iter()
            .map(|s| {
                let mut p = s.prompt(question);
                if self.retests {
                    p.push_str(IF_AGAINST);
                }
                (s.id.clone(), p)
            })
            .collect()
    }

    /// Every prompt for the follow-up round, once the blind round exists.
    /// Same shape as `blind_prompts`, so a caller drives both rounds the same
    /// way — ask every seat, in one batch, none of them fed another's answer
    /// before their own is in.
    pub fn open_prompts(&self, question: &str, blind: &[Opinion]) -> Vec<(String, String)> {
        self.seats.iter().map(|s| (s.id.clone(), s.open_prompt(question, blind))).collect()
    }

    pub fn is_quorate(&self) -> bool {
        (MIN_SEATS..=MAX_SEATS).contains(&self.seats.len())
    }

    /// Do the dispositions actually differ? Five seats that all want it proved
    /// is one seat, five times, and worth saying so before spending the turns.
    pub fn covers_different_ground(&self) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        for s in &self.seats {
            seen.insert(s.disposition);
        }
        seen.len() >= 3
    }

    pub fn tally(&self, opinions: &[Opinion], round: Round) -> Verdict {
        let count = |l: Lean| opinions.iter().filter(|o| o.lean == l).count();
        let (f, a, d) = (count(Lean::For), count(Lean::Against), count(Lean::Depends));

        let split = format!("{f} for, {a} against, {d} it depends");

        let call = if f > a && f > d {
            Some(Lean::For)
        } else if a > f && a > d {
            Some(Lean::Against)
        } else if d > f && d > a {
            Some(Lean::Depends)
        } else {
            // A tie. The tiebreak seat decides, and if there isn't one the
            // room genuinely has no call — which is an answer, not a failure.
            self.seats
                .iter()
                .find(|s| s.tiebreak)
                .and_then(|s| opinions.iter().find(|o| o.seat == s.id))
                .map(|o| o.lean)
        };

        // The best argument on the losing side, whatever the call was.
        let strongest_dissent = call.and_then(|c| {
            opinions
                .iter()
                .filter(|o| o.lean != c)
                .max_by_key(|o| o.because.len())
                .map(|o| format!("{}: {}", o.seat, o.because))
        });

        let unanimous = !opinions.is_empty()
            && opinions.iter().all(|o| o.lean == opinions[0].lean);

        Verdict {
            call,
            split,
            strongest_dissent,
            suspiciously_unanimous: unanimous && round == Round::Blind,
        }
    }

    /// What Atlas says back. One paragraph, the split first, because the count
    /// is the least interesting part and burying the disagreement under it is
    /// how this becomes theatre.
    pub fn spoken(&self, v: &Verdict) -> String {
        let mut out = String::new();
        if v.suspiciously_unanimous {
            out.push_str(
                "Everyone agreed on the first pass, which usually means the question told them \
                 the answer. Worth rewording before you trust it. ",
            );
        }
        match (&v.call, &v.strongest_dissent) {
            (Some(_), Some(d)) => {
                out.push_str(&format!("{}. The room did not settle it — {}.", v.split, d));
            }
            (Some(_), None) => out.push_str(&format!("{}, with nobody arguing the other way.", v.split)),
            (None, _) => out.push_str(&format!(
                "The council couldn't come to an agreement — {}. We're asking for guidance on the matter.",
                v.split
            )),
        }
        out
    }
}

// ---------------------------------------------------------------------------
// The half that was missing: a room that has seen the machine, and a way to
// turn what a seat says back into an `Opinion`.
//
// Everything above decides what to do with a list of opinions. **Nothing ever
// built one.** `tally`, `Verdict`, `strongest_dissent`, `suspiciously_unanimous`
// and `spoken` all computed over a `&[Opinion]` that only a test had ever
// filled, and `nudge::convene` produced five prompts that nothing sent and
// nothing read back.
// ---------------------------------------------------------------------------

use crate::fit::{Machine, Plan};

/// The subject the council is actually for here: what this machine should be
/// asked to do, and what it should be upgraded to.
///
/// The reason to scope it rather than convene about anything: five seats
/// arguing in the abstract is five opinions, and the thing that makes a
/// council worth its cost is that every seat is looking at the same real
/// numbers. Hardware is the subject where Atlas *has* real numbers — it
/// measures the machine every tick — so it is the subject where the room can
/// disagree about something rather than about a word.
pub fn is_hardware_question(q: &str) -> bool {
    let q = q.to_lowercase();
    const ABOUT: &[&str] = &[
        "ram", "memory", "gpu", "graphics card", "vram", "cpu", "cores", "disk", "ssd",
        "nvme", "upgrade", "hardware", "machine", "spec", "npu", "model size", "bigger model",
        "smaller model", "14b", "7b", "quantis", "quantiz", "rack", "server", "mini pc",
        "thermal", "fans", "power draw", "watt",
    ];
    ABOUT.iter().any(|w| q.contains(w))
}

/// A room briefed on the machine Atlas is actually on.
///
/// Every seat gets the same measured numbers. That is the whole difference
/// between this and `default_room`: the dispositions still differ, but they
/// are now disagreeing about 15.7GB and no discrete graphics rather than
/// about "should I upgrade".
///
/// `Machine` comes from `fit::measure`, which reads what `health` already
/// takes every tick — so the numbers are this machine, now, not a guess and
/// not a spec sheet.
pub fn hardware_room(m: &Machine, p: &Plan) -> Council {
    let facts = format!(
        "{}. {}MB free of {}MB, {}MB usable by Atlas right now. \
         {}. Current plan: {} tier{}.",
        crate::fit::describe(m),
        m.free_ram_mb,
        m.total_ram_mb,
        m.budget_now_mb(),
        if m.usable_vram_mb() == 0 {
            "No dedicated video memory — integrated graphics share system memory, so VRAM is not extra".to_string()
        } else {
            format!("{}MB of usable video memory", m.usable_vram_mb())
        },
        p.tier.name(),
        match p.model {
            Some(model) => format!(", running {model}"),
            None => ", no language model fits".into(),
        },
    );

    Council::new(vec![
        Seat::new(
            "mover",
            &format!("someone who wants Atlas doing more today. You know: {facts}"),
            Disposition::Bias,
        ),
        Seat::new(
            "sceptic",
            &format!(
                "someone who has watched a machine that benchmarked fine fall over under \
                 real load. You know: {facts}"
            ),
            Disposition::Sceptic,
        ),
        Seat::new(
            "operator",
            &format!(
                "someone who pays for the hardware and the power it draws. You know: {facts}"
            ),
            Disposition::Operator,
        ),
        Seat::new(
            "user",
            &format!(
                "the person who has to live with this machine every day and does not care \
                 what is in it, only what it lets them do. You know: {facts}"
            ),
            Disposition::Customer,
        ),
        Seat::new(
            "steward",
            &format!(
                "whoever keeps this running in a year: drivers, heat, spare parts, and the \
                 second machine that will have to match it. You know: {facts}"
            ),
            Disposition::Steward,
        )
        .breaks_ties(),
    ])
}

/// Asked of every seat in a room that retests: a no has to come with the
/// change that would turn it into a yes, on a line of its own so it can be
/// read back without a model.
pub const IF_AGAINST: &str = "\n\nIf you are against it, finish with one line that starts \
    \"It would be OK if\" and names the one change that would win you over. Make it a \
    change someone could actually make, not \"if it were a different idea\".";

/// How many times a changed proposal goes back to the rooms. Each pass is a
/// seat's worth of model calls per seat in two rooms; past two, the
/// remaining no is a decision for you, not another lap.
pub const MAX_RETESTS: usize = 2;

/// The seats that said no (or "it depends") and what they said would make
/// it OK. A no with nothing attached is left out here and named by the
/// caller: it can't be acted on.
pub fn conditions(opinions: &[Opinion]) -> Vec<(String, String)> {
    opinions
        .iter()
        .filter(|o| matches!(o.lean, Lean::Against | Lean::Depends))
        .filter_map(|o| o.would_change_my_mind.as_ref().map(|c| (o.seat.clone(), c.trim().trim_end_matches('.').to_string())))
        .filter(|(_, c)| !c.is_empty())
        .collect()
}

/// The seats that said no and gave nothing that would change it.
pub fn flat_noes(opinions: &[Opinion]) -> Vec<&str> {
    opinions
        .iter()
        .filter(|o| o.lean == Lean::Against && o.would_change_my_mind.is_none())
        .map(|o| o.seat.as_str())
        .collect()
}

/// The question again, with the proposal changed the way the seats asked.
/// Changes accumulate: a second pass keeps the first pass's changes.
pub fn amended(question: &str, changes: &[(String, String)]) -> String {
    if changes.is_empty() {
        return question.to_string();
    }
    let list: String = changes.iter().map(|(seat, c)| format!("\n- {c} (asked for by {seat})")).collect();
    format!(
        "{question}\n\nThe proposal has since been changed so that:{list}\n\nJudge it as changed, not as it was."
    )
}

/// The two rooms a changed proposal goes back to: fixing what worried one
/// can open a hole the other would see, so it is put to both.
pub fn retest_rooms() -> Vec<(&'static str, Council)> {
    vec![("Should it be built", build_room()), ("Is it safe", security_room())]
}

/// Is this "should I build it?" — a product, a tool, a feature, before any
/// of it is made?
pub fn is_build_question(q: &str) -> bool {
    let q = q.to_lowercase();
    const BUILD: &[&str] = &[
        "should i build", "should we build", "worth building", "should i make", "should we make",
        "should i launch", "should i ship", "worth making", "worth shipping", "should i start building",
        "is there a market", "would anyone pay", "would anyone use", "worth selling", "should i sell",
    ];
    BUILD.iter().any(|w| q.contains(w))
}

/// A room for "should I build this?", one seat per way a thing that works
/// still fails: nobody wants it, nobody can tell what it is, nobody pays,
/// nobody comes back, nobody trusts it enough, nobody hears about it.
///
/// The risks are the pre-build checklist from the MIT-licensed
/// `before-you-build` skill in wshobson/agents (THIRD_PARTY_NOTICES.md),
/// put into seats so each is argued by someone who cares about it, blind,
/// rather than ticked off a list by one voice. Product risk only: whether
/// it can be built is a different question and a different room.
pub fn build_room() -> Council {
    Council::new(vec![
        Seat::new(
            "demand",
            "someone who needs evidence that a specific person urgently wants this — a name, a complaint, money already spent on a worse fix — and says so when there is none",
            Disposition::Sceptic,
        ),
        Seat::new(
            "first user",
            "the person it is for, trying it once: can they tell what it is and why it matters in one sentence, and would they come back next week",
            Disposition::Customer,
        ),
        Seat::new(
            "money",
            "someone who asks who pays, how much, and whether that covers the hours it takes to build and keep running",
            Disposition::Operator,
        ),
        Seat::new(
            "reach",
            "someone who wants it out in front of people this month and asks how anyone would hear about it, again and again, not just once",
            Disposition::Bias,
        ),
        Seat::new(
            "trust",
            "whoever maintains it and answers for it in a year: what data it touches, what people must trust it with, and what breaks for them if it goes away",
            Disposition::Steward,
        )
        .breaks_ties(),
    ])
    .with_retests()
}

/// Is this "is it safe?" — about an attack, a leak, or who can reach what?
pub fn is_security_question(q: &str) -> bool {
    let q = q.to_lowercase();
    const SAFE: &[&str] = &[
        "is it safe", "is this safe", "secure", "security", "attack", "hack", "leak",
        "could someone", "can someone", "exploit", "breach", "phish", "who can reach",
        "who can see", "expose", "exposed",
    ];
    SAFE.iter().any(|w| q.contains(w))
}

/// A room for "is this safe?", one seat per way a thing gets abused: someone
/// pretends to be you, changes what they shouldn't, sees what they
/// shouldn't, stops it working, or gets more reach than they were given.
///
/// The seats follow STRIDE, the threat-modelling checklist used in the
/// MIT-licensed `security-scanning` plugin of wshobson/agents
/// (THIRD_PARTY_NOTICES.md). Asked of each seat blind, so "it's fine" from
/// one doesn't talk the others out of the hole they found.
pub fn security_room() -> Council {
    Council::new(vec![
        Seat::new(
            "impostor",
            "someone who tries to pass as you or as a device you trust — a stolen token, a copied cookie, a look-alike sign-in page — and asks what stops them",
            Disposition::Sceptic,
        ),
        Seat::new(
            "tamperer",
            "someone who wants to change what shouldn't change — a setting, a file, a message in transit, a record after the fact — and asks whether anyone would notice",
            Disposition::Steward,
        ),
        Seat::new(
            "eavesdropper",
            "someone who only wants to see: what leaves the machine, what sits unencrypted, what a log or a backup quietly keeps",
            Disposition::Customer,
        ),
        Seat::new(
            "wrecker",
            "someone who just wants it to stop working, and asks what one bad input or a flood of them costs you",
            Disposition::Operator,
        ),
        Seat::new(
            "climber",
            "someone who has a little access and wants more — a guest who becomes the owner, a helper that gets the keys — and wants to move fast before it is noticed",
            Disposition::Bias,
        )
        .breaks_ties(),
    ])
    .with_retests()
}

/// The room for a question: one built for its kind, or `None` for the
/// general one. The hardware room is chosen separately, because it needs
/// the measured machine.
pub fn room_for(q: &str) -> Option<(&'static str, Council)> {
    if is_build_question(q) {
        Some(("build", build_room()))
    } else if is_security_question(q) {
        Some(("security", security_room()))
    } else {
        None
    }
}

/// What a seat said, read back into an `Opinion`.
///
/// Deliberately lenient about shape and strict about substance. A local model
/// will not reliably return JSON, and demanding it would mean throwing away
/// four good answers because one of them opened with "Sure!". So the lean is
/// found wherever it appears and the rest is prose.
///
/// The one thing it will not do is invent a lean. `Seat::prompt` tells every
/// seat that "a seat that will not commit is an empty chair", and a reply that
/// commits to nothing is recorded as `Depends` — which `tally` already counts
/// separately and which can leave the room with no call at all. That is a real
/// finding and much better than a coin flip dressed as a verdict.
pub fn parse_opinion(seat: &str, reply: &str) -> Opinion {
    let lower = reply.to_lowercase();

    // Looked for as whole words: "for" is a substring of "before", "form" and
    // "therefore", and an opinion decided by the word "therefore" is noise.
    // Both positions found by the same whole-word rule that decides presence.
    // The first version used `lower.find(" for ")` here, which cannot match a
    // reply that *opens* with "For" -- so a seat saying "For -- ship it. The
    // case against is thermals" was recorded as Against, the exact opposite of
    // what it said. Two different rules for the same word is how that happens.
    let lean = match (word_at(&lower, "against"), word_at(&lower, "for")) {
        // Both present: whichever is stated first is the seat's own position,
        // because the second is nearly always "the argument for X is...".
        (Some(a), Some(f)) => {
            if f < a {
                Lean::For
            } else {
                Lean::Against
            }
        }
        (Some(_), None) => Lean::Against,
        (None, Some(_)) => Lean::For,
        (None, None) => Lean::Depends,
    };
    // An explicit "it depends" outranks a stray "for" anywhere in the prose.
    let lean = if lower.contains("it depends") || lower.contains("depends on") {
        Lean::Depends
    } else {
        lean
    };

    // "It would be OK if …" is the answer a retesting room asks for, and
    // the most specific; "change my mind" is the general room's.
    let would = after_phrase(reply, "would be ok if")
        .or_else(|| after_phrase(reply, "would be okay if"))
        .or_else(|| after_phrase(reply, "change my mind"));
    Opinion { seat: seat.into(), lean, because: first_sentence(reply), would_change_my_mind: would }
}

/// Where `needle` first appears as a whole word, if it does.
///
/// One function rather than a `has_word` and a separate position search: the
/// two got out of step immediately, and the seat whose answer was inverted by
/// it said exactly what it meant.
fn word_at(hay: &str, needle: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let at = from + i;
        let before_ok = at == 0 || !hay.as_bytes()[at - 1].is_ascii_alphanumeric();
        let end = at + needle.len();
        let after_ok = end >= hay.len() || !hay.as_bytes()[end].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return Some(at);
        }
        from = at + needle.len();
    }
    None
}

/// The reason, not the whole essay. `Opinion::because` says "one line".
fn first_sentence(text: &str) -> String {
    let t = text.trim();
    let cut = t.find(". ").map(|i| i + 1).unwrap_or(t.len());
    let s: String = t[..cut].trim().chars().take(240).collect();
    if s.is_empty() {
        "gave no reason".into()
    } else {
        s
    }
}

/// Whatever the seat said after "change my mind", where it said anything.
fn after_phrase(text: &str, phrase: &str) -> Option<String> {
    let at = text.to_lowercase().find(phrase)?;
    let rest = text[at + phrase.len()..].trim_start_matches([':', ' ', '-', '\n']).trim();
    let line = rest.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.chars().take(200).collect())
    }
}

/// Seats that would not commit. A room of these has not disagreed, it has
/// declined to answer, and saying so is more use than a verdict built on it.
pub fn empty_chairs(opinions: &[Opinion]) -> Vec<&str> {
    opinions
        .iter()
        .filter(|o| o.lean == Lean::Depends && o.would_change_my_mind.is_none())
        .map(|o| o.seat.as_str())
        .collect()
}
