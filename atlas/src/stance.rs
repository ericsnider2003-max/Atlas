//! Writing that says something.
//!
//! Catching faults makes a draft less bad. It doesn't make it good, and the
//! difference matters: you can remove every hedge and every filler word from a
//! paragraph and still be left with something that takes no position, supports
//! nothing, and could have been written about anything.
//!
//! So this asks a different question. Not "what's wrong with it" but "does it
//! make a claim, is the claim held up, and is it built in an order that makes
//! sense for where it's going".

use serde::{Deserialize, Serialize};

/// What a piece of writing is for. Structure follows from this, and a form
/// borrowed from the wrong one is most of what makes writing feel off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An argument. Needs a claim and support.
    Case,
    /// Telling someone what happened. Needs the facts and their order.
    Account,
    /// Asking for something. Needs the ask, early.
    Request,
    /// Teaching something. Needs a shape you can follow.
    Explanation,
    /// A note to yourself. Needs nothing but to be clear.
    Note,
}

impl Kind {
    /// The shape this kind of writing wants.
    pub fn wants(&self) -> &'static [&'static str] {
        match self {
            Kind::Case => &["a claim", "reasons", "the strongest objection", "what follows"],
            Kind::Account => &["what happened", "when", "what it means"],
            Kind::Request => &["the ask", "why you", "what happens next"],
            Kind::Explanation => &["the point", "how it works", "an example"],
            Kind::Note => &["the thing"],
        }
    }

    /// Should this take a position at all?
    ///
    /// An account that argues is editorialising; a case that doesn't is
    /// wasting your time.
    pub fn needs_a_position(&self) -> bool {
        matches!(self, Kind::Case | Kind::Request)
    }

    /// How formal, roughly. Not a rule — a default.
    pub fn formality(&self) -> &'static str {
        match self {
            Kind::Case => "direct, not stiff",
            Kind::Account => "plain",
            Kind::Request => "brief and warm",
            Kind::Explanation => "conversational",
            Kind::Note => "however you like",
        }
    }
}

/// What's missing from a draft that would make it good rather than merely
/// inoffensive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Missing {
    /// It never says what it thinks.
    NoClaim,
    /// It asserts and never supports.
    NothingBehindIt,
    /// It supports with nothing checkable — no number, source, or example.
    NoEvidence,
    /// It never acknowledges the obvious objection, which reads as either
    /// naive or evasive.
    NoCounterCase,
    /// It ends without saying what follows.
    NoConclusion,
    /// The parts are in an order that fights the reader.
    OutOfOrder,
    /// It buries the point somewhere in the middle.
    BuriedLead,
    /// Every sentence is a claim with nothing between them.
    Relentless,
}

impl Missing {
    pub fn what(&self) -> &'static str {
        match self {
            Missing::NoClaim => "never says what you actually think",
            Missing::NothingBehindIt => "asserts things without supporting any of them",
            Missing::NoEvidence => "has nothing checkable in it — no numbers, sources or examples",
            Missing::NoCounterCase => "never takes on the obvious objection",
            Missing::NoConclusion => "stops without saying what follows",
            Missing::OutOfOrder => "puts things in an order that fights the reader",
            Missing::BuriedLead => "buries the point in the middle",
            Missing::Relentless => "is claim after claim with no room to breathe",
        }
    }

    /// What to do about it, specifically.
    pub fn fix(&self) -> &'static str {
        match self {
            Missing::NoClaim => "Say the thing you actually believe, in one sentence, early.",
            Missing::NothingBehindIt => "Take the two strongest assertions and give a reason for each.",
            Missing::NoEvidence => "Put one real number, name or example in. One is enough.",
            Missing::NoCounterCase => "Name the best argument against, then answer it in a sentence.",
            Missing::NoConclusion => "End with what should happen, not with a summary.",
            Missing::OutOfOrder => "Move the claim to the front and the support behind it.",
            Missing::BuriedLead => "The best sentence is in the middle. Move it to the top.",
            Missing::Relentless => "Break the run of claims with an example or a concession.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gap {
    pub missing: Missing,
    pub evidence: String,
}

/// A reference the writing leans on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Support {
    /// What it's supporting.
    pub claim: String,
    /// A number, a source, an example, or an observation.
    pub kind: SupportKind,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    /// A figure you can check.
    Number,
    /// Something Atlas read, with where.
    Source,
    /// A concrete case.
    Example,
    /// Something Atlas measured itself, which is the strongest kind here.
    Measured,
}

impl SupportKind {
    /// How much this actually holds up a claim.
    pub fn weight(&self) -> f32 {
        match self {
            // Something Atlas measured on your machine beats a citation,
            // because it's about your situation rather than in general.
            SupportKind::Measured => 1.0,
            SupportKind::Number => 0.8,
            SupportKind::Source => 0.7,
            SupportKind::Example => 0.5,
        }
    }
}

/// Words that mark a sentence as taking a position rather than describing.
const CLAIM_MARKERS: &[&str] = &[
    "should", "shouldn't", "must", "the answer is", "the problem is", "wrong",
    "right", "better", "worse", "worth", "not worth", "matters", "doesn't matter",
    "the point is", "i'd", "i would", "the case for", "beats", "instead of",
];

/// Words that mark a sentence as support rather than assertion.
const SUPPORT_MARKERS: &[&str] = &[
    "because", "since", "which means", "for example", "in practice", "measured",
    "we found", "the data", "according to", "when we", "it turned out", "that's why",
];

/// Words that mark taking on the other side.
const COUNTER_MARKERS: &[&str] = &[
    "the obvious objection", "you could argue", "the case against", "admittedly",
    "the counter", "it's true that", "against that", "the risk is", "what this misses",
    "the honest problem", "where this falls down",
];

fn sentences(text: &str) -> Vec<&str> {
    text.split_inclusive(['.', '!', '?'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

fn marks(s: &str, markers: &[&str]) -> bool {
    let l = s.to_lowercase();
    markers.iter().any(|m| l.contains(m))
}

/// Read a draft for what it's missing rather than what's wrong with it.
pub fn assess(text: &str, kind: Kind) -> Vec<Gap> {
    let mut gaps = Vec::new();
    let ss = sentences(text);
    if ss.is_empty() {
        return gaps;
    }

    let claims: Vec<&&str> = ss.iter().filter(|s| marks(s, CLAIM_MARKERS)).collect();
    let supports: Vec<&&str> = ss.iter().filter(|s| marks(s, SUPPORT_MARKERS)).collect();
    let counters: Vec<&&str> = ss.iter().filter(|s| marks(s, COUNTER_MARKERS)).collect();

    if kind.needs_a_position() && claims.is_empty() {
        gaps.push(Gap {
            missing: Missing::NoClaim,
            evidence: "nothing in it commits to a view".into(),
        });
    }

    if !claims.is_empty() && supports.is_empty() {
        gaps.push(Gap {
            missing: Missing::NothingBehindIt,
            evidence: format!("{} assertion{}, no reasons", claims.len(),
                if claims.len() == 1 { "" } else { "s" }),
        });
    }

    // Anything checkable at all?
    let has_number = text.chars().any(|c| c.is_ascii_digit());
    let has_example = marks(text, &["for example", "for instance", "such as", "in one case"]);
    let has_source = text.contains("http") || marks(text, &["according to", "the docs", "the spec"]);
    if !has_number && !has_example && !has_source && ss.len() > 3 {
        gaps.push(Gap {
            missing: Missing::NoEvidence,
            evidence: "no numbers, no sources, no examples".into(),
        });
    }

    if kind == Kind::Case && counters.is_empty() && ss.len() > 4 {
        gaps.push(Gap {
            missing: Missing::NoCounterCase,
            evidence: "the obvious objection is never raised".into(),
        });
    }

    // The strongest sentence should be near the front, not buried.
    if claims.len() >= 1 && ss.len() > 3 {
        let first_claim = ss.iter().position(|s| marks(s, CLAIM_MARKERS)).unwrap_or(0);
        if first_claim as f32 > ss.len() as f32 * 0.55 {
            gaps.push(Gap {
                missing: Missing::BuriedLead,
                evidence: format!("the point arrives in sentence {}", first_claim + 1),
            });
        }
    }

    // Ending.
    if let Some(last) = ss.last() {
        let ends_well = marks(last, &["so ", "which means", "next", "i'd", "let's", "worth", "should"]);
        if kind.needs_a_position() && !ends_well && ss.len() > 3 {
            gaps.push(Gap {
                missing: Missing::NoConclusion,
                evidence: format!("ends on \"{}\"", short(last, 46)),
            });
        }
    }

    // Claim, claim, claim with nothing between them is exhausting to read
    // however true each one is.
    if ss.len() >= 5 {
        let run = longest_run(&ss, CLAIM_MARKERS);
        if run >= 4 {
            gaps.push(Gap {
                missing: Missing::Relentless,
                evidence: format!("{run} assertions in a row"),
            });
        }
    }

    gaps
}

fn longest_run(ss: &[&str], markers: &[&str]) -> usize {
    let mut best = 0;
    let mut run = 0;
    for s in ss {
        if marks(s, markers) {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    best
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// What to do about it, in order of what would improve it most.
///
/// Ordered deliberately: a piece with no claim cannot be fixed by adding
/// evidence, because there is nothing for the evidence to support.
pub fn brief(gaps: &[Gap], kind: Kind) -> Option<String> {
    if gaps.is_empty() {
        return None;
    }
    let mut ordered = gaps.to_vec();
    ordered.sort_by_key(|g| match g.missing {
        Missing::NoClaim => 0,
        Missing::BuriedLead => 1,
        Missing::NothingBehindIt => 2,
        Missing::NoEvidence => 3,
        Missing::NoCounterCase => 4,
        Missing::NoConclusion => 5,
        Missing::OutOfOrder => 6,
        Missing::Relentless => 7,
    });

    let mut s = format!("Rewrite it as {}. It wants: {}.\n\n", 
        kind.formality(), kind.wants().join(", then "));
    for g in &ordered {
        s.push_str(&format!("- It {} ({}). {}\n", g.missing.what(), g.evidence, g.missing.fix()));
    }
    s.push_str("\nKeep the voice. Don't pad it to cover the gaps.");
    Some(s)
}

/// What Atlas says about a draft, spoken. The single most useful thing first.
pub fn spoken(gaps: &[Gap]) -> String {
    match gaps.first() {
        None => "That holds up. It says something and backs it.".into(),
        Some(g) => {
            let more = gaps.len() - 1;
            let mut s = format!("It {}. {}", g.missing.what(), g.missing.fix());
            if more > 0 {
                s.push_str(&format!(" {more} other thing{}.", if more == 1 { "" } else { "s" }));
            }
            s
        }
    }
}

/// Work out what kind of thing you're writing, so the standard fits.
pub fn kind_of(request: &str) -> Kind {
    let t = request.to_lowercase();
    if ["argue", "make the case", "convince", "why we should", "pitch", "position"]
        .iter()
        .any(|w| t.contains(w))
    {
        return Kind::Case;
    }
    if ["ask", "request", "can you", "would you", "invite", "follow up"].iter().any(|w| t.contains(w)) {
        return Kind::Request;
    }
    if ["explain", "how does", "teach", "walk through", "guide"].iter().any(|w| t.contains(w)) {
        return Kind::Explanation;
    }
    if ["note", "jot", "remind", "for myself"].iter().any(|w| t.contains(w)) {
        return Kind::Note;
    }
    Kind::Account
}
