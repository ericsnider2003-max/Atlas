//! Arguing the other side.
//!
//! You've decided something. Asking Atlas whether it's a good idea gets you
//! agreement, because you've framed it as a good idea — that's how questions
//! work, and it's why "what do you think?" is nearly useless once you've
//! already made your mind up.
//!
//! This is the deliberate version: **make the strongest case against**, on
//! request. Not to change your mind, and not as a general habit — asked for,
//! and then done properly.
//!
//! The failure mode to avoid is a system that lists generic risks. "There may
//! be unforeseen costs" is true of everything and helps with nothing. What's
//! useful is the argument someone who disagreed with you would actually make.

use serde::{Deserialize, Serialize};

/// The kinds of case that exist against most decisions.
///
/// Not all apply, and saying so is part of the job — a second opinion that
/// finds six objections to everything is one you learn to ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Angle {
    /// The thing it's meant to fix isn't the actual problem.
    WrongProblem,
    /// It'll work, and cost more than it saves.
    NotWorthIt,
    /// It commits you to something you can't get out of.
    HardToUndo,
    /// It works now and not later.
    DoesntScale,
    /// It depends on something you don't control.
    RestsOnSomethingElse,
    /// Doing nothing is genuinely fine.
    NothingIsFine,
    /// You've decided this before and it didn't hold.
    YouTriedThis,
    /// The reason you want it isn't the reason you're giving.
    RealReason,
}

impl Angle {
    pub fn question(&self) -> &'static str {
        match self {
            Angle::WrongProblem => "What if the thing this fixes isn't what's actually wrong?",
            Angle::NotWorthIt => "What does this cost, against what it saves?",
            Angle::HardToUndo => "How do you get out of this if it's wrong?",
            Angle::DoesntScale => "Does this still work when there's ten times as much of it?",
            Angle::RestsOnSomethingElse => "What does this depend on that you don't control?",
            Angle::NothingIsFine => "What actually happens if you do nothing?",
            Angle::YouTriedThis => "Haven't you decided this before?",
            Angle::RealReason => "Is the reason you're giving the reason you want it?",
        }
    }

    /// Only worth raising when there's something to point at.
    pub fn needs_evidence(&self) -> bool {
        matches!(self, Angle::YouTriedThis | Angle::RealReason | Angle::NotWorthIt)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Objection {
    pub angle: Angle,
    /// The argument, specific to this decision.
    pub case: String,
    /// What would settle it.
    pub what_would_answer_it: String,
    /// How much weight it carries, 0 to 1.
    pub weight: f32,
}

/// What was decided.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub what: String,
    /// Why you said you're doing it.
    pub because: String,
    /// Can it be undone?
    pub reversible: bool,
    /// Roughly what it costs — money, time, or attention.
    pub costs: Option<String>,
    /// Things it depends on.
    pub depends_on: Vec<String>,
    /// You've decided something like this before, and how it went.
    pub previously: Option<String>,
}

/// Build the case against.
///
/// Only the angles that actually apply. A second opinion that finds six
/// objections to everything is one you learn to ignore.
pub fn against(d: &Decision) -> Vec<Objection> {
    let mut out = Vec::new();

    if !d.reversible {
        out.push(Objection {
            angle: Angle::HardToUndo,
            case: format!(
                "{} can't be undone, and you're deciding it on {}.",
                d.what, d.because
            ),
            what_would_answer_it: "a version of this you could back out of".into(),
            weight: 0.9,
        });
    }

    if let Some(prev) = &d.previously {
        out.push(Objection {
            angle: Angle::YouTriedThis,
            case: format!("You decided something like this before — {prev}."),
            what_would_answer_it: "what's different this time".into(),
            weight: 0.85,
        });
    }

    if let Some(cost) = &d.costs {
        out.push(Objection {
            angle: Angle::NotWorthIt,
            case: format!("It costs {cost}. What it saves has to beat that, and you haven't said it does."),
            what_would_answer_it: "the number on the other side".into(),
            weight: 0.7,
        });
    }

    for dep in d.depends_on.iter().take(2) {
        out.push(Objection {
            angle: Angle::RestsOnSomethingElse,
            case: format!("This only works while {dep} holds, and that isn't yours to control."),
            what_would_answer_it: format!("what you'd do if {dep} changed"),
            weight: 0.65,
        });
    }

    // The one that's nearly always worth asking and nearly never asked.
    out.push(Objection {
        angle: Angle::NothingIsFine,
        case: format!(
            "The case for doing nothing: {}. If that's tolerable, this is optional.",
            what_if_nothing(&d.what)
        ),
        what_would_answer_it: "what actually breaks if you leave it".into(),
        weight: 0.6,
    });

    // Reasons that sound like justifications after the fact.
    if reads_as_rationalisation(&d.because) {
        out.push(Objection {
            angle: Angle::RealReason,
            case: format!(
                "\"{}\" sounds like a reason found afterwards. That doesn't make it wrong, but \
                 it's worth checking what you actually want here.",
                d.because
            ),
            what_would_answer_it: "the reason you'd give a friend".into(),
            weight: 0.5,
        });
    }

    out.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn what_if_nothing(what: &str) -> String {
    format!("you carry on as you are, and \"{what}\" stays available next month")
}

/// Phrases that tend to appear when the reason came after the decision.
const RATIONALISATION: &[&str] = &[
    "everyone", "obviously", "it just makes sense", "why wouldn't", "no brainer",
    "no-brainer", "at some point", "eventually", "might as well", "can't hurt",
    "cant hurt", "future proof", "future-proof",
];

fn reads_as_rationalisation(because: &str) -> bool {
    let b = because.to_lowercase();
    RATIONALISATION.iter().any(|r| b.contains(r))
}

/// What Atlas says. Framed as the case against, not as its own view.
///
/// That framing matters: this was asked for, and presenting an argument you
/// requested as a personal opinion would be dishonest about what it is.
pub fn spoken(d: &Decision, objections: &[Objection]) -> String {
    if objections.is_empty() {
        return format!("I can't make much of a case against {}.", d.what);
    }
    let first = &objections[0];
    let more = objections.len() - 1;
    let mut s = format!("The strongest case against: {}", first.case);
    if !s.ends_with('.') {
        s.push('.');
    }
    s.push_str(&format!(" What would answer it: {}.", first.what_would_answer_it));
    if more > 0 {
        s.push_str(&format!(" {more} weaker one{}.", if more == 1 { "" } else { "s" }));
    }
    s
}

/// The written version.
pub fn written(d: &Decision, objections: &[Objection]) -> String {
    let mut s = format!("The case against {}\n\n", d.what);
    if objections.is_empty() {
        s.push_str("I can't find one worth making.\n");
        return s;
    }
    for o in objections {
        s.push_str(&format!("{}\n  {}\n  Answered by: {}\n\n",
            o.angle.question(), o.case, o.what_would_answer_it));
    }
    s.push_str("None of that means don't. It's the argument someone who disagreed would make.\n");
    s
}

/// Is this being asked for?
///
/// Never volunteered. Arguing against a decision nobody asked you about is
/// how a system becomes exhausting.
pub fn is_asked_for(said: &str) -> bool {
    let t = said.to_lowercase();
    [
        "argue the other side", "second opinion", "talk me out of", "poke holes",
        "what's wrong with", "whats wrong with", "devil's advocate", "devils advocate",
        "why shouldn't i", "why shouldnt i", "make the case against", "push back on",
    ]
    .iter()
    .any(|p| t.contains(p))
}

// ---------------------------------------------------------------------------
// Hearing the decision in what was said (23 Sep 2026)
// ---------------------------------------------------------------------------

/// Triggers that only mean "argue against" when a decision follows them.
/// "What's wrong with the printer" is a fault report, not a request to be
/// talked out of anything.
const WEAK_TRIGGERS: &[&str] = &[
    "what's wrong with", "whats wrong with", "push back on", "second opinion",
];

const STRONG_TRIGGERS: &[&str] = &[
    "argue the other side", "talk me out of", "poke holes", "devil's advocate",
    "devils advocate", "why shouldn't i", "why shouldnt i", "make the case against",
];

/// Words that make the rest of a sentence read as a decision.
const DECISION_CUES: &[&str] = &[
    "because", "going to", "gonna", "decided", "decide", "i'll", "i will", "plan",
    "switch", "buying", "buy ", "quit", "moving", "signing", "i want to", "i'm ",
];

/// The decision in "argue the other side: X because Y".
///
/// `earlier` is what was said just before — "argue the other side" on its own
/// is about the last thing on the table. Only what was actually said is read:
/// irreversibility, cost and dependencies come from the speaker's own words
/// ("for good", "costs £900", "as long as the job holds"), never guessed, and
/// `previously` is left empty because nothing here knows the history — which
/// is exactly what [`not_raised`] says out loud.
///
/// `None` when this isn't a request to argue, or when a weak trigger isn't
/// followed by anything decision-shaped.
pub fn decision_from(said: &str, earlier: Option<&str>) -> Option<Decision> {
    if !is_asked_for(said) {
        return None;
    }
    let lower = said.to_lowercase();
    let (at, len, weak) = STRONG_TRIGGERS
        .iter()
        .map(|t| (*t, false))
        .chain(WEAK_TRIGGERS.iter().map(|t| (*t, true)))
        .find_map(|(t, weak)| lower.find(t).map(|i| (i, t.len(), weak)))?;
    // Byte offsets from the lowercase copy; `said` has the same byte layout
    // for ASCII triggers only when its own case-folding is 1:1, so cut the
    // lowercase copy and recover the original span by char count.
    let before_chars = lower[..at + len].chars().count();
    let rest: String = said.chars().skip(before_chars).collect();
    let mut rest = rest.trim_matches(|c: char| c.is_whitespace() || ":,-—?.".contains(c)).to_string();
    for lead in ["on ", "about ", "for ", "against ", "me on ", "with "] {
        if rest.to_lowercase().starts_with(lead) {
            rest = rest[lead.len()..].trim().to_string();
        }
    }
    let rest = if rest.is_empty() {
        earlier?.trim().to_string()
    } else {
        rest
    };
    if rest.is_empty() {
        return None;
    }
    let rl = rest.to_lowercase();
    if weak && !DECISION_CUES.iter().any(|c| rl.contains(c)) {
        return None;
    }

    let (what, because) = match split_ci(&rest, " because ") {
        Some((w, b)) => (w.trim().to_string(), b.trim().to_string()),
        None => (rest.clone(), "a reason you haven't said".to_string()),
    };
    let what = what.trim_end_matches(|c: char| ".?!".contains(c)).to_string();
    let because = because.trim_end_matches(|c: char| ".?!".contains(c)).to_string();

    let reversible = ![
        "can't undo", "cannot undo", "can't be undone", "no going back", "for good",
        "permanent", "irreversible", "non-refundable", "nonrefundable",
    ]
    .iter()
    .any(|p| rl.contains(p));

    Some(Decision {
        what,
        because,
        reversible,
        costs: phrase_after(&rest, &["it costs ", "costs ", "it'll cost ", "will cost ", "cost me "]),
        depends_on: phrase_after(&rest, &["as long as ", "assuming ", "depends on ", "provided "])
            .into_iter()
            .collect(),
        previously: None,
    })
}

/// The words after the first cue found, up to the end of that clause.
fn phrase_after(text: &str, cues: &[&str]) -> Option<String> {
    let lower = text.to_lowercase();
    let (i, cue) = cues.iter().find_map(|c| lower.find(c).map(|i| (i, *c)))?;
    let start = lower[..i + cue.len()].chars().count();
    let tail: String = text.chars().skip(start).collect();
    let end = tail
        .find(|c: char| ",;.!?".contains(c))
        .unwrap_or(tail.len());
    let mut s = tail[..end].trim().to_string();
    for stop in [" and ", " but ", " because "] {
        if let Some((head, _)) = split_ci(&s, stop) {
            s = head;
        }
    }
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Split at the first case-insensitive occurrence of an ASCII `pat`, by
/// characters rather than bytes so a lowercase copy's offsets never cut the
/// original mid-character.
fn split_ci(text: &str, pat: &str) -> Option<(String, String)> {
    let lower = text.to_lowercase();
    let i = lower.find(pat)?;
    let head_chars = lower[..i].chars().count();
    let pat_chars = pat.chars().count();
    let head: String = text.chars().take(head_chars).collect();
    let tail: String = text.chars().skip(head_chars + pat_chars).collect();
    Some((head, tail))
}

/// Angles that need evidence and weren't raised — because there was nothing
/// to point at.
///
/// Worth saying rather than silently skipping: "I didn't argue you've tried
/// this before" is different from "you haven't", and a case against that
/// hides its gaps looks more complete than it is.
pub fn not_raised(objections: &[Objection]) -> Vec<Angle> {
    [Angle::YouTriedThis, Angle::RealReason, Angle::NotWorthIt]
        .into_iter()
        .filter(|a| a.needs_evidence())
        .filter(|a| !objections.iter().any(|o| o.angle == *a))
        .collect()
}

/// The full spoken answer to "argue the other side": the case, then the
/// angles left out for want of evidence.
pub fn argued(d: &Decision) -> String {
    let objections = against(d);
    let mut s = spoken(d, &objections);
    let gaps: Vec<&str> = not_raised(&objections)
        .iter()
        .map(|a| match a {
            Angle::YouTriedThis => "that you've tried this before",
            Angle::RealReason => "that the reason isn't the real one",
            Angle::NotWorthIt => "that it costs more than it saves",
            _ => "",
        })
        .filter(|p| !p.is_empty())
        .collect();
    if !gaps.is_empty() {
        s.push_str(&format!(
            " I didn't argue {} — I'd need something to point at, and I don't have it.",
            join_or(&gaps)
        ));
    }
    s
}

fn join_or(parts: &[&str]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.to_string(),
        [a, b] => format!("{a} or {b}"),
        _ => format!("{}, or {}", parts[..parts.len() - 1].join(", "), parts[parts.len() - 1]),
    }
}
