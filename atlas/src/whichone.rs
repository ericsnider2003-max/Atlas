//! Which of several things you meant — and whether it was clear enough to act on.
//!
//! # The gap this fills
//!
//! Atlas arrives at what you meant in two places. `intent.rs` matches a phrase
//! from your own list, and `understood.rs` grades how a reading was arrived at
//! so an inferred one that changes the world gets checked with you first.
//!
//! Between those two there was nothing. Once an `Intent` carried an argument,
//! the daemon decided what to do with it by asking whether your words
//! *contained* a substring, in `match` arms whose order was never written down
//! anywhere and was doing the deciding:
//!
//! ```ignore
//! Intent::Mail(what) if what.contains("tax") || what.contains("statement") => { .. }
//! Intent::Mail(what) if what.contains("money") || what.contains("spend")   => { .. }
//! ```
//!
//! "How much did I spend on my trading statement last month" is a question
//! about money. It contains `statement` and `trading`, so it reached the tax
//! arm, because the tax arm is written first. Nothing was wrong, nothing
//! failed, and the answer was about the wrong thing — the failure this tree
//! keeps finding in other shapes.
//!
//! # What this does instead
//!
//! The competing readings are named, each says what belongs to it **and what
//! belongs to one of the others**, and they are weighed together rather than
//! tried in sequence. The winner comes back with how far it won by, and a win
//! too narrow to trust is reported as [`Clarity::Close`] rather than taken.
//!
//! Saying *"did you mean the money side or the tax side?"* costs you a
//! sentence. Answering the wrong question costs you the answer you wanted and
//! the time spent believing it.
//!
//! # Why the contrast half matters
//!
//! A list of words that mean an option, with no list of words that mean a
//! different one, cannot tell "trading statement" from "tax statement". Every
//! reading here is defined by both, and
//! [`tests/asking_which_one_you_meant.rs`] fails the build for a set where two
//! readings share a word without either disclaiming it — which is the only
//! way this degrades back into the thing it replaced.
//!
//! # No model, on purpose
//!
//! This is a word-overlap judgment over a handful of named options, and a
//! model would make it slower, unavailable offline, and untestable, in that
//! order. `certainty.rs` and `understood.rs` already handle the cases where a
//! model has been consulted; this one runs everywhere Atlas runs, including
//! the machine with nothing downloaded yet.

use serde::Deserialize;

/// One of the things you might have meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// What the caller matches on. Not shown to anyone.
    pub id: &'static str,
    /// How Atlas would say it back to you: "the money side".
    pub plain: &'static str,
    /// Words and phrases that mean this one.
    ///
    /// A phrase counts for more than a word, because "strong enough" appearing
    /// is better evidence than "strong" appearing.
    pub means: &'static [&'static str],
    /// Words that sit near this reading and belong to a different one.
    ///
    /// The half that makes this better than a list of `contains`. "Statement"
    /// means the tax reading, *unless* the sentence is about spending — so the
    /// money reading claims "spend" and the tax reading disclaims it.
    pub not: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clarity {
    /// One reading won by enough. Act on it.
    Clear,
    /// Two readings were close. Ask which, rather than taking the first.
    Close,
    /// Nothing matched at all. Not this set of readings.
    Nothing,
}

/// What the weighing came to.
#[derive(Debug, Clone, PartialEq)]
pub struct Weighed {
    pub clarity: Clarity,
    /// The winner, when there was one. `None` only for [`Clarity::Nothing`].
    pub best: Option<&'static str>,
    /// The one behind it, when something was.
    pub behind: Option<&'static str>,
    /// How far ahead the winner was. Zero when nothing matched.
    pub margin: f32,
}

impl Weighed {
    /// The reading to act on, or `None` if Atlas should ask instead.
    ///
    /// Deliberately collapses `Close` and `Nothing` together: from the
    /// caller's side both mean "do not take a branch on this", and the two are
    /// told apart by [`Weighed::clarity`] when the caller wants to phrase
    /// something different for each.
    pub fn settled(&self) -> Option<&'static str> {
        match self.clarity {
            Clarity::Clear => self.best,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WhichOneConfig {
    /// How far ahead the winner has to be before Atlas acts on it.
    ///
    /// In the same units as the scoring: one plain word of evidence is 1.0, a
    /// two-word phrase 2.0. So the shipped 1.0 means "one clear word more than
    /// the runner-up", and a sentence carrying one word for each of two
    /// readings is a question rather than a guess.
    pub act_above_margin: f32,
    /// Below this, the sentence was not about this set of readings at all.
    ///
    /// Measured against the *gross* evidence for the best-supported reading --
    /// its own words, before any other reading has argued it down. Measuring
    /// the net instead makes two readings that disclaim each other cancel to
    /// zero, so a sentence in perfect contention between them reads as being
    /// about neither. It is about both, which is why it is a question.
    pub matched_at_all: f32,
}

impl Default for WhichOneConfig {
    fn default() -> Self {
        // Asking is cheap and being wrong is not, so the shipped numbers are
        // set where a genuinely ambiguous sentence gets a question. They are
        // settable because what counts as ambiguous depends on how you talk.
        WhichOneConfig { act_above_margin: 1.0, matched_at_all: 1.0 }
    }
}

/// Lowercased, punctuation dropped, single-spaced, with a space at each end.
///
/// The bracketing spaces are what make a whole-word search a substring search:
/// `" spend "` cannot match inside "spending", and `"spend"` can. Matching
/// inside a longer word is how "grade" found "upgrade" and "colour" found
/// "colourless", both of which this module exists to stop.
fn flattened(s: &str) -> String {
    let inner: String = s
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    format!(" {inner} ")
}

/// Evidence for one term appearing: one per word in it, or nothing.
///
/// Named `evidence_for` rather than `weight_of`: another module once had a
/// `weight_of`, the deadness scans read bare names, and calling this one made
/// that one look reached. Fourth time in two days -- `install::total_mb`,
/// `Platform::honest`, `install::plan_for`, and now this.
fn evidence_for(haystack: &str, term: &str) -> f32 {
    let needle = flattened(term);
    let needle = needle.trim();
    if needle.is_empty() {
        return 0.0;
    }
    if haystack.contains(&format!(" {needle} ")) {
        needle.split_whitespace().count() as f32
    } else {
        0.0
    }
}

/// Weigh the sentence against every reading at once.
///
/// Every reading is scored against the same sentence independently — none of
/// them sees another's score, and the order they are given in has no effect on
/// the result. That is the whole difference from the `match` arms this
/// replaces, where position was policy.
///
/// A tie is not broken. Two readings on the same score come back
/// [`Clarity::Close`] with a margin of zero, which is the honest answer and
/// the one that produces a question.
pub fn weigh(said: &str, readings: &[Reading], cfg: &WhichOneConfig) -> Weighed {
    let hay = flattened(said);

    // Two numbers per reading, and keeping them apart is the whole of the
    // subtlety here.
    //
    // `net` is what the reading wins on: its own words, less the words it
    // disclaims. `gross` is only its own words, and it answers a different
    // question -- *was the sentence about this set at all*.
    //
    // The first version scored on `net` alone, and the case it got wrong is
    // the exact case this module was built for. "Tax on what I spend" gives
    // the tax reading one word for and one against, and the money reading the
    // same, so both net to zero, and zero is below `matched_at_all` -- a
    // sentence in perfect contention between two readings came back
    // `Nothing`, meaning "not about any of this". It is about both of them,
    // which is why it is a question. Caught by the test that asked for
    // `Close` and got `Nothing`.
    let mut scored: Vec<(&'static str, f32)> = Vec::with_capacity(readings.len());
    let mut loudest = 0.0f32;
    for r in readings {
        let for_it: f32 = r.means.iter().map(|m| evidence_for(&hay, m)).sum();
        let against: f32 = r.not.iter().map(|n| evidence_for(&hay, n)).sum();
        loudest = loudest.max(for_it);
        // Net is floored at zero. A reading the sentence argues against is
        // simply not this one; letting it go negative would make it *lose* to
        // a reading with no evidence either way, and "less wrong" is not a
        // reason to pick something.
        scored.push((r.id, (for_it - against).max(0.0)));
    }
    // Stable, so readings on the same score keep the order they were given
    // in. That order decides nothing -- both come back and the caller asks --
    // but a set that reshuffled its own ties would make this function's
    // answer depend on the sort, which is the property the whole module is
    // about not having.
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Was the sentence about this set at all? Asked of the gross evidence,
    // before any reading has argued another one down.
    if loudest < cfg.matched_at_all {
        return Weighed { clarity: Clarity::Nothing, best: None, behind: None, margin: 0.0 };
    }

    let (best, top) = match scored.first() {
        Some(&(id, s)) => (id, s),
        // An empty set of readings. Nothing to be about.
        None => {
            return Weighed { clarity: Clarity::Nothing, best: None, behind: None, margin: 0.0 }
        }
    };
    let (behind, second) = match scored.get(1) {
        Some(&(id, s)) => (Some(id), s),
        None => (None, 0.0),
    };

    let margin = top - second;
    let clarity = if margin >= cfg.act_above_margin { Clarity::Clear } else { Clarity::Close };
    Weighed { clarity, best: Some(best), behind, margin }
}

/// The question to ask when two readings were too close to call.
///
/// Names the two that were actually close rather than listing every reading in
/// the set: a question offering six options is one you have to read twice, and
/// the other four were not in contention.
pub fn which_did_you_mean(w: &Weighed, readings: &[Reading]) -> Option<String> {
    if w.clarity != Clarity::Close {
        return None;
    }
    let plain = |id: &str| readings.iter().find(|r| r.id == id).map(|r| r.plain);
    match (w.best.and_then(plain), w.behind.and_then(plain)) {
        (Some(a), Some(b)) => Some(format!("Did you mean {a} or {b}?")),
        // One reading, and it did not clear the margin against nothing. That
        // is a set with a single entry and a threshold above its score, which
        // is a configuration to fix rather than a question to ask.
        _ => None,
    }
}

// ===================== the sets the daemon weighs =====================
//
// Written here rather than beside each `match` arm on purpose. The whole
// defect was that the readings competed without anything saying so; keeping
// them in one list is what makes "two of these claim the same word" a thing
// you can see, and `tests/asking_which_one_you_meant.rs` checks it.

/// What "ask about my mail" might be asking about.
///
/// The three that were ordered `match` arms, and the ordering was the bug:
/// "how much did I spend on my trading statement" reached the tax arm because
/// the tax arm is written first.
pub const ABOUT_MAIL: &[Reading] = &[
    Reading {
        id: "rules",
        plain: "the tax and trading rules",
        means: &["tax", "hmrc", "irs", "trading rule", "wash sale", "allowance"],
        // "Statement" belongs to both and is the word the two fought over, so
        // neither claims it alone: the rest of the sentence decides.
        not: &["how much", "spend", "spent", "budget", "category"],
    },
    Reading {
        id: "statements",
        plain: "the money side — what you actually spent",
        means: &["how much", "spend", "spent", "spending", "statement", "month", "budget", "category"],
        not: &["tax", "hmrc", "irs", "trading rule", "wash sale"],
    },
    Reading {
        id: "messages",
        plain: "your chats rather than your email",
        means: &["message", "messages", "chat", "telegram", "whatsapp", "signal", "discord", "slack"],
        not: &["tax", "spend", "statement", "inbox", "email"],
    },
];

/// What "look at this post" might be asking you to look at.
pub const ABOUT_A_POST: &[Reading] = &[
    Reading {
        id: "stance",
        plain: "whether it actually says anything",
        means: &["say anything", "says anything", "position", "strong enough", "argument", "point", "wishy washy"],
        not: &["colour", "color", "grade", "grading", "footage", "exposure"],
    },
    Reading {
        id: "grading",
        plain: "the colour of the footage",
        means: &["colour", "color", "grade", "grading", "footage", "exposure", "skin tone", "white balance"],
        // "Strong" is a colour word too ("too strong"), so the stance reading
        // claims the whole phrase "strong enough" and this one disclaims it.
        not: &["say anything", "says anything", "position", "strong enough", "argument"],
    },
];

/// Which machine "can you do this on X" is asking about.
///
/// Not a fix for an ordering bug -- this one's `if/else` ladder was correct --
/// but the same shape, and keeping it here means the ladder cannot drift away
/// from the guard that checks the words do not overlap.
pub const WHICH_MACHINE: &[Reading] = &[
    Reading {
        id: "mac",
        plain: "a Mac",
        means: &["mac", "macos", "macbook", "osx"],
        not: &["iphone", "ipad", "android", "linux", "windows"],
    },
    Reading {
        id: "linux",
        plain: "Linux",
        means: &["linux", "ubuntu", "debian", "fedora"],
        not: &["mac", "iphone", "android", "windows"],
    },
    Reading {
        id: "android",
        plain: "an Android phone",
        means: &["android", "pixel", "samsung"],
        not: &["iphone", "ipad", "mac", "linux", "windows"],
    },
    Reading {
        id: "ios",
        plain: "an iPhone or iPad",
        means: &["iphone", "ipad", "ios"],
        not: &["android", "mac", "linux", "windows"],
    },
    Reading {
        id: "windows",
        plain: "Windows",
        means: &["windows", "pc"],
        not: &["mac", "linux", "iphone", "android"],
    },
];
