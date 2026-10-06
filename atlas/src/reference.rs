//! Knowing things offline, without a bigger model.
//!
//! The instinct for "Atlas should know more" is a larger model, and it's the
//! wrong instinct. A model that fits on your laptop is about 4GB and it does
//! not reliably recall a futures contract's tick size or the wash sale rule's
//! exact window — it recalls something *shaped* like them, confidently, which
//! is worse than not knowing.
//!
//! A text file recalls it exactly, costs 50MB, and can cite where it came
//! from.
//!
//! So the split is: **the model reasons, the shelf remembers.** Anything with
//! a number, a date, a threshold or a legal definition lives on the shelf.
//! Anything requiring judgement goes to the model, with the relevant page from
//! the shelf attached.

use serde::{Deserialize, Serialize};

/// A body of reference material held locally.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shelf {
    pub name: String,
    /// What it covers, in your words.
    pub covers: String,
    pub mb: u32,
    /// Where it came from, so a wrong answer can be traced.
    pub from: String,
    /// When it was fetched. Reference material goes stale silently, which is
    /// the failure mode that matters here.
    pub as_of: String,
    /// How long before it's worth replacing.
    pub stale_after_days: u32,
    pub kind: Sort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// Rules with numbers in them. The main case.
    Rules,
    /// Specifications — contract sizes, tick values, session times.
    Specs,
    /// General reference.
    Encyclopaedia,
    /// How to do something.
    Procedures,
    /// Your own notes and past work.
    Yours,
}

impl Sort {
    /// Does being out of date make it wrong, or just old?
    pub fn goes_wrong_when_stale(&self) -> bool {
        matches!(self, Sort::Rules | Sort::Specs)
    }
}

/// What's worth putting on the shelf, and roughly what it costs.
///
/// Ordered by how badly a model gets it wrong from memory, which is not the
/// same as how important it is.
pub fn worth_having() -> Vec<Shelf> {
    vec![
        Shelf {
            name: "futures contract specs".into(),
            covers: "tick size, tick value, contract months, session times, margin per contract".into(),
            mb: 4,
            from: "the exchanges' own specification pages".into(),
            as_of: String::new(),
            stale_after_days: 180,
            kind: Sort::Specs,
        },
        Shelf {
            name: "options mechanics".into(),
            covers: "greeks, assignment, exercise style, settlement, pin risk, corporate actions".into(),
            mb: 8,
            from: "the OCC's own material".into(),
            as_of: String::new(),
            stale_after_days: 365,
            kind: Sort::Rules,
        },
        Shelf {
            name: "trader tax".into(),
            covers: "wash sales, Section 1256, trader tax status, mark-to-market election and its deadline".into(),
            mb: 12,
            from: "the IRS publications themselves".into(),
            as_of: String::new(),
            // The deadline moves and the rules change. Stale here is
            // expensive rather than merely embarrassing.
            stale_after_days: 300,
            kind: Sort::Rules,
        },
        Shelf {
            name: "forex sessions and conventions".into(),
            covers: "session overlaps, rollover times, pip values, settlement, carry".into(),
            mb: 3,
            from: "public reference material".into(),
            as_of: String::new(),
            stale_after_days: 365,
            kind: Sort::Specs,
        },
        Shelf {
            name: "crypto mechanics".into(),
            covers: "funding rates, liquidation maths, settlement, exchange differences".into(),
            mb: 6,
            from: "the exchanges' documentation".into(),
            as_of: String::new(),
            // Faster-moving than anything else here.
            stale_after_days: 120,
            kind: Sort::Specs,
        },
        Shelf {
            name: "editing and colour".into(),
            covers: "codecs, colour spaces, transforms, export settings, common grades".into(),
            mb: 5,
            from: "the software's own documentation".into(),
            as_of: String::new(),
            stale_after_days: 540,
            kind: Sort::Specs,
        },
        Shelf {
            name: "the platforms".into(),
            covers: "posting limits, aspect ratios, what each one rewards, rules on branded content".into(),
            mb: 4,
            from: "each platform's published guidelines".into(),
            as_of: String::new(),
            // These change constantly and quietly.
            stale_after_days: 90,
            kind: Sort::Rules,
        },
        Shelf {
            name: "how to do things".into(),
            covers: "procedures for the tools you use — steps, snags, what goes wrong".into(),
            mb: 6,
            from: "written as you go, from what actually worked".into(),
            as_of: String::new(),
            stale_after_days: 365,
            kind: Sort::Procedures,
        },
        Shelf {
            name: "your own work".into(),
            covers: "what you've written, what you've posted, what did well and what didn't".into(),
            mb: 20,
            from: "you".into(),
            as_of: String::new(),
            // Never stale. It's a record of what happened.
            stale_after_days: 36_500,
            kind: Sort::Yours,
        },
        Shelf {
            name: "an encyclopaedia".into(),
            covers: "everything else, for the questions that aren't about your work".into(),
            mb: 4_000,
            from: "an offline Wikipedia snapshot".into(),
            as_of: String::new(),
            stale_after_days: 730,
            kind: Sort::Encyclopaedia,
        },
    ]
}

/// The shelves you asked for.
///
/// `reference.shelves` — "which shelves to keep" — was read by nothing until
/// 18 Sep 2026: `Intent::Ask` took `for_trading()` whatever the list said, so
/// naming one shelf and naming none produced the same answer.
///
/// Empty still means the trading set. `[]` is what the shipped file carries,
/// and reading it as "no shelves at all" would take the shelf away from every
/// install that never edited the line — a silent removal dressed up as
/// honouring a setting. Disabled means none, which is what `enabled: false`
/// already meant.
///
/// Names are matched case-insensitively against `Shelf::name`; a name that
/// matches nothing simply is not in the result, and `nothing_found` then says
/// so in the same words it uses for a shelf that was never fetched.
pub fn chosen(cfg: &ReferenceConfig) -> Vec<Shelf> {
    if !cfg.enabled {
        return Vec::new();
    }
    if cfg.shelves.is_empty() {
        return for_trading();
    }
    worth_having()
        .into_iter()
        .filter(|s| cfg.shelves.iter().any(|n| n.trim().eq_ignore_ascii_case(&s.name)))
        .collect()
}

/// Everything needed for the trading side.
pub fn for_trading() -> Vec<Shelf> {
    worth_having()
        .into_iter()
        .filter(|s| s.kind != Sort::Encyclopaedia)
        .collect()
}

/// What the whole trading shelf costs.
///
/// Worth knowing before deciding: the specific knowledge is small. The
/// encyclopaedia is what costs gigabytes, and it's the least useful part.
pub fn trading_mb() -> u32 {
    for_trading().iter().map(|s| s.mb).sum()
}

impl Shelf {
    /// Is this actually on the shelf, or only worth having?
    ///
    /// `worth_having` is a catalogue of what would be worth holding, and
    /// every entry in it ships with `as_of` empty, because nothing in this
    /// tree fetches reference material yet. Until 19 Sep 2026 nothing made
    /// that distinction, and `nothing_found` answered "my futures contract
    /// specs covers tick size, tick value, ..." about a shelf that does not
    /// exist — which is the one thing a grounded answer must never do.
    pub fn held(&self) -> bool {
        !self.as_of.trim().is_empty()
    }

    /// How old what's on the shelf is, in days.
    ///
    /// `as_of` is an ISO date. `None` when it is empty or unreadable, which
    /// is not the same as zero days old.
    pub fn days_old(&self, now: u64) -> Option<u32> {
        let t = self.as_of.trim();
        let mut parts = t.split('-');
        let y: i32 = parts.next()?.parse().ok()?;
        let m: u32 = parts.next()?.parse().ok()?;
        let d: u32 = parts.next()?.parse().ok()?;
        if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        let fetched = crate::market::time::days_from_civil(y, m, d);
        let today = (now / 86_400) as i64;
        u32::try_from(today.saturating_sub(fetched)).ok()
    }
}

/// Has this gone stale?
pub fn is_stale(s: &Shelf, days_since: u32) -> bool {
    days_since > s.stale_after_days
}

/// What Atlas says about stale reference material.
///
/// Never silently. A wrong number given confidently from a two-year-old file
/// is worse than not having the file.
pub fn stale_warning(s: &Shelf, days_since: u32) -> Option<String> {
    if !is_stale(s, days_since) {
        return None;
    }
    Some(if s.kind.goes_wrong_when_stale() {
        format!(
            "my {} is {} days old, and that kind goes wrong rather than just old. Worth checking \
             anything I tell you from it",
            s.name, days_since
        )
    } else {
        format!("my {} is a bit old, but it won't have changed much", s.name)
    })
}

/// An answer that came off the shelf.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub text: String,
    /// Which shelf, so it can be checked.
    pub from: String,
    pub as_of: String,
    /// How well it matched.
    pub confidence: f32,
}

/// What Atlas says with an answer from the shelf.
///
/// Always with the source. The whole reason for having this rather than
/// trusting the model is that the answer is traceable — saying it without the
/// source throws that away.
pub fn quoted(f: &Found) -> String {
    format!("{} — from my {}{}.", f.text, f.from,
        if f.as_of.is_empty() { String::new() } else { format!(", as of {}", f.as_of) })
}

/// Nothing on the shelf covers it.
///
/// The important case. A model asked a specific question it doesn't know
/// answers anyway, so the shelf coming up empty has to be said rather than
/// quietly falling through.
/// `say_how_old` is `reference.warn_when_stale`. It ships on and nothing read
/// it, which mattered more than it looks: the sentence this returns is what
/// Atlas says instead of inventing a number, and it was claiming to hold
/// shelves that have never been fetched.
pub fn nothing_found(about: &str, have: &[Shelf], say_how_old: bool, now: u64) -> String {
    let nearest = have
        .iter()
        .find(|s| about.to_lowercase().split_whitespace().any(|w| s.covers.contains(w)));
    match nearest {
        Some(s) if s.held() => {
            let mut said = format!(
                "I haven't got that specifically. My {} covers {} — I could reason from that, \
                 but I'd be guessing at the number.",
                s.name, s.covers
            );
            if say_how_old {
                if let Some(warning) = s.days_old(now).and_then(|d| stale_warning(s, d)) {
                    said.push_str(&format!(" And {warning}."));
                }
            }
            said
        }
        // The shelf is on the list of what would be worth having and has
        // never been fetched. Saying "my X covers Y" about it is a claim to
        // hold something, and a grounded answer that is not grounded is worse
        // than an ungrounded one, because you stop checking it.
        Some(s) => format!(
            "Nothing on my shelf covers {about} — a {} would, and I haven't got one. Nothing \
             here fetches reference material yet, so the shelf is a list of what would be \
             worth holding rather than what I hold. Anything I said with a number in it would \
             be invented.",
            s.name
        ),
        None => format!(
            "Nothing on my shelf covers {about}. I can think about it, but anything I said with \
             a number in it would be invented."
        ),
    }
}

/// When to use the shelf and when to use the model.
///
/// The rule that makes this work: anything with a number, a date, a threshold
/// or a definition comes off the shelf. Judgement goes to the model.
pub fn needs_the_shelf(question: &str) -> bool {
    let q = question.to_lowercase();
    [
        "how much", "how many", "what is the", "what's the", "whats the",
        "tick", "margin", "deadline", "limit", "threshold", "rate", "size",
        "when is", "what time", "expiry", "expires", "settlement", "rule",
        "section", "allowed", "required",
    ]
    .iter()
    .any(|w| q.contains(w))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ReferenceConfig {
    pub enabled: bool,
    /// Which shelves to keep.
    pub shelves: Vec<String>,
    /// Warn when reference material is older than its own limit.
    pub warn_when_stale: bool,
}

impl Default for ReferenceConfig {
    fn default() -> Self {
        ReferenceConfig {
            enabled: false,
            shelves: Vec::new(),
            warn_when_stale: true,
        }
    }
}

// ---------- what Atlas keeps from what it finds ----------

/// Something Atlas learned and decided to keep.
///
/// The question this answers: does a search get thrown away when the
/// conversation ends? It shouldn't — you paid for it in time, and the second
/// time you need it you'd search again for the same thing.
///
/// But keeping everything is how a knowledge store becomes a landfill, so
/// what's kept is deliberately narrow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    /// What it says.
    pub fact: String,
    /// Where it came from — a URL, a document, you.
    pub source: String,
    pub at: u64,
    /// Why it was worth keeping.
    pub because: Worth,
    /// Times it's been used since.
    pub used: u32,
    /// You said it was wrong.
    pub corrected: bool,
}

/// The only reasons to keep something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Worth {
    /// It has a number, a date or a threshold in it. The kind a model
    /// misremembers.
    HasAFigure,
    /// You said so.
    YouSaidKeep,
    /// It's about your setup rather than about the world — your broker's fee
    /// tiers, your camera's settings.
    AboutYou,
    /// It took real work to find.
    WasHardToFind,
    /// It contradicts something Atlas thought was true.
    Corrects,
}

/// Should this be kept?
///
/// The bar is deliberately high. Most of what a search returns is context you
/// needed once — keeping it makes everything after it harder to find.
pub fn worth_keeping(text: &str, took_searches: u32, you_said_keep: bool) -> Option<Worth> {
    if you_said_keep {
        return Some(Worth::YouSaidKeep);
    }
    let has_figure = text.chars().any(|c| c.is_ascii_digit())
        && ["%", "$", "£", "bps", "basis point", "per ", "minimum", "maximum", "limit",
            "deadline", "rate", "fee"]
            .iter()
            .any(|w| text.to_lowercase().contains(w));
    if has_figure {
        return Some(Worth::HasAFigure);
    }
    // Four searches for one answer means the fifth time would be four more.
    if took_searches >= 4 {
        return Some(Worth::WasHardToFind);
    }
    None
}

/// The shelf life of something kept, using the same rule as everything else.
///
/// My first version had a fixed 180-day cliff for anything with a figure in
/// it, which is wrong in both directions: a contract's tick size doesn't
/// change in six months, and a funding rate is stale in an hour. `freshness`
/// works out what kind of claim it is and decays confidence rather than
/// expiring it.
pub fn shelf_life(k: &Kept) -> crate::freshness::Shelf {
    match k.because {
        // Yours by definition — it's about your setup, and it stays true
        // until you change it rather than on a clock.
        Worth::AboutYou => crate::freshness::Shelf::Yours,
        Worth::YouSaidKeep => crate::freshness::Shelf::Yours,
        // Everything else is read from what it says.
        _ => crate::freshness::shelf_for(&k.fact),
    }
}

/// Something kept that's gone off.
///
/// A fee tier from two years ago given as current is the failure this whole
/// idea has to avoid — it's exactly the confident wrongness the shelf exists
/// to prevent.
pub fn gone_off(k: &Kept, now: u64) -> Option<String> {
    use crate::freshness::{Checkable, Known, State};

    let known = Known::new(
        &k.fact,
        shelf_life(k),
        Checkable::File(k.source.clone()),
        k.at,
    );
    match known.state(now) {
        State::Fresh => None,
        State::Ageing => Some(format!(
            "I've got \"{}\" from {}, and it's the sort of thing that changes. Worth checking \
             before you rely on it.",
            k.fact, k.source
        )),
        State::Stale => Some(format!(
            "\"{}\" is old enough that I wouldn't rely on it. Want me to look again?",
            k.fact
        )),
    }
}

/// Something you corrected is never quietly kept alongside the correction.
pub fn correct_it(kept: &mut Vec<Kept>, fact: &str, replacement: &str, now: u64) {
    for k in kept.iter_mut() {
        if k.fact == fact {
            k.corrected = true;
        }
    }
    kept.retain(|k| !k.corrected);
    kept.push(Kept {
        fact: replacement.into(),
        source: "you".into(),
        at: now,
        // What you told it outranks what it found.
        because: Worth::YouSaidKeep,
        used: 0,
        corrected: false,
    });
}

/// Things kept and never used again.
///
/// Offered back rather than deleted — the point of a low bar for looking is
/// that some of it turns out not to matter.
pub fn never_used(kept: &[Kept], now: u64, older_than_days: u64) -> Vec<&Kept> {
    kept.iter()
        .filter(|k| k.used == 0 && now.saturating_sub(k.at) > older_than_days * 86_400)
        .collect()
}

/// Why not just a bigger model.
pub const WHY_NOT_A_BIGGER_MODEL: &str =
    "A model that fits on your laptop doesn't reliably recall a contract's tick size or the exact \
     wash sale window — it recalls something shaped like them, confidently, which is worse than \
     not knowing. A text file recalls it exactly, costs a few megabytes, and can tell you where \
     it came from. The model reasons; the shelf remembers.";

/// The size that surprises people.
pub const WHAT_IT_COSTS: &str =
    "All the trading reference together is about 33MB. The encyclopaedia is 4GB and it's the \
     least useful part — the specific knowledge, the part a model gets wrong, is the small part.";
