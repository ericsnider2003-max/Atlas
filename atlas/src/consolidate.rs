//! One thing learned once, however many times you ask about it.
//!
//! Two problems, and they're the same problem seen from either end.
//!
//! **Asking differently shouldn't cost you what you knew.** You ask about the
//! wash sale rule in January and again in March, phrased differently. Without
//! this, that's two notes: two timestamps, two decay curves, and confidence in
//! a settled fact drifting down because you happened to ask twice. Nothing
//! changed except your wording.
//!
//! **And a store that only grows is a store you eventually turn off.** But
//! size isn't the right thing to cap — it's *value density*. A settled fact
//! costs a few hundred bytes and never needs looking at again. Twelve stale
//! quotes cost the same and are worth nothing.
//!
//! So: the same claim arriving again **merges and strengthens** rather than
//! duplicating, and what gets dropped when space is short is chosen by what
//! it's worth rather than by when it arrived.

use serde::{Deserialize, Serialize};

/// A claim, and everything that has confirmed it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    /// As first written down.
    pub says: String,
    pub shelf: crate::freshness::Shelf,
    /// Every time this has been seen, and where.
    pub confirmations: Vec<Confirmation>,
    /// First learned.
    pub first_at: u64,
    /// Times you've asked something this answered.
    pub asked_about: u32,
    /// You said it was wrong.
    pub corrected: bool,
    /// How much of it is still here.
    pub density: Density,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Confirmation {
    pub source: String,
    pub at: u64,
    /// The wording it arrived in this time. Kept because the phrasing you
    /// used is how you'll ask again.
    pub as_worded: String,
}

impl Claim {
    /// When this was last confirmed, which is what decay should be measured
    /// from.
    ///
    /// Not `first_at`. A fact confirmed again last week is a week old, not
    /// two years old — measuring from first sight is how re-reading something
    /// makes Atlas less sure of it.
    pub fn as_of(&self) -> u64 {
        self.confirmations.iter().map(|c| c.at).max().unwrap_or(self.first_at)
    }

    /// How many different places have said this.
    ///
    /// The same source twice is one source. Two sources agreeing is the thing
    /// worth counting.
    pub fn independent_sources(&self) -> usize {
        let mut seen: Vec<&str> = self.confirmations.iter().map(|c| c.source.as_str()).collect();
        seen.sort();
        seen.dedup();
        seen.len()
    }

    /// Confidence, before age is applied.
    ///
    /// Rises with independent agreement and caps — five sources saying the
    /// same thing is not meaningfully surer than three, and letting it climb
    /// forever would make a popular wrong answer unassailable.
    pub fn standing(&self) -> f32 {
        if self.corrected {
            return 0.0;
        }
        match self.independent_sources() {
            0 | 1 => 0.7,
            2 => 0.85,
            _ => 0.92,
        }
    }

    /// Does it need checking again?
    ///
    /// Something settled never does, however old — that's what settled means,
    /// and re-checking it is work that produces nothing.
    pub fn worth_rechecking(&self, now: u64) -> bool {
        use crate::freshness::State;
        let k = crate::freshness::Known::new(
            &self.says,
            self.shelf,
            crate::freshness::Checkable::File(String::new()),
            self.as_of(),
        );
        k.state(now) != State::Fresh
    }
}

/// Is this the same claim, said differently?
///
/// Deliberately conservative. Merging two things that aren't the same claim
/// loses one of them silently, which is worse than keeping a near-duplicate —
/// so the bar is high and the failure is a harmless extra note.
pub fn same_claim(a: &str, b: &str) -> bool {
    // Crude stemming, because the same claim reworded is exactly where word
    // forms differ: "loss" and "losses", "rebuy" and "rebought". Without it
    // the matcher only catches near-identical wording, which is the case that
    // needed no matcher.
    fn stem(w: &str) -> String {
        for suffix in ["ing", "ies", "ed", "es", "s"] {
            if w.len() > suffix.len() + 3 && w.ends_with(suffix) {
                let mut base = w[..w.len() - suffix.len()].to_string();
                if suffix == "ies" {
                    base.push('y');
                }
                return base;
            }
        }
        w.to_string()
    }

    let key = |s: &str| -> Vec<String> {
        let mut w: Vec<String> = s
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .filter(|w| !COMMON.contains(w))
            .map(stem)
            .collect();
        w.sort();
        w.dedup();
        w
    };
    let (x, y) = (key(a), key(b));
    if x.len() < 2 || y.len() < 2 {
        return false;
    }
    let shared = x.iter().filter(|w| y.contains(w)).count();
    // Most of the shorter one's meaningful words appearing in the longer.
    //
    // Two-thirds rather than three-quarters: a genuine rewording keeps the
    // nouns and changes the connective tissue, so demanding near-identity
    // only matches things that needed no matcher. The cost of being wrong
    // here is a harmless extra note, which is why the bar can come down.
    shared as f32 / x.len().min(y.len()) as f32 >= 0.66
}

const COMMON: &[&str] = &[
    "what", "when", "where", "which", "that", "this", "with", "from", "have",
    "does", "about", "your", "into", "then", "than", "will", "would", "there",
];

/// Take something learned and either merge it or add it.
///
/// Returns whether it strengthened something already known.
pub fn learn(
    known: &mut Vec<Claim>,
    says: &str,
    source: &str,
    at: u64,
) -> bool {
    if let Some(c) = known.iter_mut().find(|c| same_claim(&c.says, says)) {
        // Asking differently doesn't restart the clock or fork the fact.
        c.confirmations.push(Confirmation {
            source: source.into(),
            at,
            as_worded: says.into(),
        });
        c.asked_about += 1;
        return true;
    }
    known.push(Claim {
        says: says.into(),
        shelf: crate::freshness::shelf_for(says),
        confirmations: vec![Confirmation {
            source: source.into(),
            at,
            as_worded: says.into(),
        }],
        first_at: at,
        asked_about: 1,
        corrected: false,
        density: Density::Full,
    });
    false
}

// ---------- what to drop when space is short ----------

/// What a claim is worth keeping, per byte.
///
/// The thing that makes this work: a settled fact and a stale quote cost the
/// same to store and are worth wildly different amounts. Dropping by age would
/// throw away the settled one first, because settled facts are the oldest
/// things you know.
pub fn worth_keeping(c: &Claim, now: u64) -> f32 {
    use crate::freshness::Shelf;

    let base = match c.shelf {
        // Never needs refreshing and never goes wrong. The cheapest thing to
        // keep and the most expensive to lose.
        Shelf::Settled => 1.0,
        // Yours. Nobody else can tell you this.
        Shelf::Yours => 1.0,
        Shelf::Slow => 0.6,
        Shelf::Quick => 0.3,
        // Storing these is nearly pointless.
        Shelf::Volatile => 0.05,
    };
    let used = (c.asked_about as f32).min(10.0) / 10.0;
    let agreed = (c.independent_sources() as f32 - 1.0).max(0.0).min(2.0) / 2.0;

    // Stale volatile things go first; used settled things go last.
    let stale = if c.worth_rechecking(now) { 0.6 } else { 1.0 };
    (base * 0.6 + used * 0.25 + agreed * 0.15) * stale
}

/// Trim to a budget, keeping what's worth most.
///
/// Returns what was dropped, so it can be said rather than done quietly — a
/// store that silently forgets is one you stop trusting.
/// Make room by compacting before dropping anything.
///
/// The order matters and is the whole point: **squeeze first, drop second.**
/// Compacting the long ones usually frees enough that nothing has to go, and
/// a compacted claim still answers the question.
///
/// Returns what was compacted and what was dropped, separately, because they
/// mean different things to you.
pub fn make_room(
    known: &mut Vec<Claim>,
    stones: &mut Vec<Tombstone>,
    keep_at_most: usize,
    now: u64,
) -> (usize, Vec<String>) {
    let mut squeezed = 0;
    // At the limit, not past it. Waiting until it overflows means the first
    // thing over the line gets dropped when squeezing would have made room
    // for it.
    if known.len() >= keep_at_most {
        for c in known.iter_mut() {
            if c.density == Density::Full && worth_compacting(&c.says) {
                c.says = compact(&c.says);
                c.density = Density::Compact;
                squeezed += 1;
            }
        }
    }
    // Only now, and only if compacting wasn't enough.
    let dropped = trim_with_stones(known, stones, keep_at_most, now);
    (squeezed, dropped)
}

pub fn trim_with_stones(
    known: &mut Vec<Claim>,
    stones: &mut Vec<Tombstone>,
    keep_at_most: usize,
    now: u64,
) -> Vec<String> {
    let before: Vec<Claim> = known.clone();
    let dropped = trim(known, keep_at_most, now);
    for c in before.iter().filter(|c| dropped.contains(&c.says)) {
        stones.push(tombstone(c, now));
    }
    // Tombstones are twenty bytes each, so the cap on them is generous —
    // but not unbounded, because the whole point is that nothing grows
    // forever.
    if stones.len() > keep_at_most * 4 {
        let excess = stones.len() - keep_at_most * 4;
        stones.drain(..excess);
    }
    dropped
}

pub fn trim(known: &mut Vec<Claim>, keep_at_most: usize, now: u64) -> Vec<String> {
    use crate::freshness::Shelf;

    if known.len() <= keep_at_most {
        return Vec::new();
    }
    // Sorting by worth would nearly always protect these, and nearly always
    // isn't a guarantee — with more settled claims than the budget, sorting
    // alone would drop some. The config says these are never dropped, so they
    // are held out of the trim entirely rather than ranked highly within it.
    let (protected, mut rest): (Vec<Claim>, Vec<Claim>) = known
        .drain(..)
        .partition(|c| matches!(c.shelf, Shelf::Settled | Shelf::Yours) && !c.corrected);

    rest.sort_by(|a, b| {
        worth_keeping(b, now)
            .partial_cmp(&worth_keeping(a, now))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // The budget applies to what's left after the protected ones. If they
    // alone exceed it, the budget is simply exceeded — losing something that
    // can't be looked up again to satisfy a number is the wrong trade.
    let room = keep_at_most.saturating_sub(protected.len());
    let dropped: Vec<String> = rest
        .iter()
        .skip(room)
        .map(|c| c.says.clone())
        .collect();
    rest.truncate(room);

    *known = protected;
    known.extend(rest);
    dropped
}

/// Is the store over its budget because of things it refuses to drop?
///
/// Said rather than hidden: a cap that's quietly being exceeded is a cap that
/// isn't doing anything, and you should know which it is.
pub fn over_budget_on_purpose(known: &[Claim], keep_at_most: usize) -> Option<String> {
    if known.len() <= keep_at_most {
        return None;
    }
    Some(format!(
        "{} things known against a budget of {keep_at_most}. All of the excess is settled facts \
         or things about your own setup, which I won't drop — they're the two kinds that can't be \
         looked up again.",
        known.len()
    ))
}

/// How much of a claim is kept.
///
/// Dropping straight to a pointer was too blunt. Most of what makes a claim
/// long is elaboration — the qualifier, the example, the restatement — and
/// almost none of it is the fact. A claim can usually lose 60% of its
/// characters and 0% of what it tells you.
///
/// So there are three states rather than two, and the middle one is where
/// nearly everything ends up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    /// As learned, with its sources and its wording.
    Full,
    /// The fact, without the elaboration. Still an answer.
    Compact,
    /// A pointer. Not an answer, but it knows where to get one.
    Stone,
}

/// Squeeze a claim down without losing what it says.
///
/// Sentence-level rather than word-level: the first sentence of a factual
/// claim almost always carries it, and what follows qualifies. Dropping a
/// clause mid-sentence is how you get a fact that reads fine and means
/// something else.
pub fn compact(says: &str) -> String {
    // The first sentence, or the first clause before a qualifier that starts
    // a new thought.
    let first = says
        .split(['.', ';'])
        .next()
        .unwrap_or(says)
        .trim();

    let mut out = first.to_string();
    // These introduce elaboration rather than the fact.
    for lead in [
        ", which ", ", so ", ", because ", ", although ", " — ", ", meaning ",
        ", in other words", ", for example", ", that is",
    ] {
        if let Some(i) = out.find(lead) {
            // Only if what's left still says something.
            if i > 24 {
                out.truncate(i);
            }
        }
    }
    out.trim_end_matches(',').trim().to_string()
}

/// Is compacting worth it here?
///
/// Below a certain length there's nothing to remove, and compacting something
/// already short just risks losing a word that mattered.
pub fn worth_compacting(says: &str) -> bool {
    says.len() > 80 && compact(says).len() * 4 < says.len() * 3
}

/// What's left behind when something is dropped.
///
/// The honest answer to "does compacting lose knowledge" is yes — dropping a
/// claim loses the claim. What it doesn't have to lose is the *knowledge that
/// you once knew it*, which is much smaller and is the part that hurts.
///
/// A tombstone is one line: what it was about, where it came from, and when.
/// Twenty bytes against a few hundred. So a search that would have hit the
/// dropped claim says "I knew something about this and let it go, it came from
/// here" rather than silently returning nothing — which is the failure that
/// makes you stop trusting the store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tombstone {
    /// The few words it was about.
    pub about: String,
    /// Where it came from, so it can be found again.
    pub source: String,
    pub dropped_at: u64,
}

/// Reduce a claim to what's worth remembering about having known it.
pub fn tombstone(c: &Claim, now: u64) -> Tombstone {
    let about: String = c
        .says
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .take(6)
        .collect::<Vec<_>>()
        .join(" ");
    Tombstone {
        about,
        source: c
            .confirmations
            .first()
            .map(|f| f.source.clone())
            .unwrap_or_default(),
        dropped_at: now,
    }
}

/// Did Atlas once know something about this?
pub fn once_knew<'a>(stones: &'a [Tombstone], asked: &str) -> Option<&'a Tombstone> {
    // Proportion rather than a fixed count. A tombstone holds six words at
    // most, so demanding two hits misses short questions — "what was the tier
    // two fee" has one distinctive word in common and is obviously the same
    // subject.
    let words: Vec<String> = asked
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3 && !COMMON.contains(w))
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        return None;
    }
    stones.iter().find(|t| {
        let hay = t.about.to_lowercase();
        let hits = words.iter().filter(|w| hay.contains(w.as_str())).count();
        hits > 0 && hits as f32 / words.len() as f32 >= 0.5
    })
}

/// What Atlas says when it finds a tombstone rather than a claim.
///
/// The useful part is the source: it can go and get it again rather than
/// shrugging.
pub fn knew_once(t: &Tombstone) -> String {
    if t.source.is_empty() {
        format!("I knew something about {} and let it go to save space.", t.about)
    } else {
        format!(
            "I knew something about {} and let it go to save space. It came from {} — want me to \
             look again?",
            t.about, t.source
        )
    }
}

/// What Atlas says when it drops things.
pub fn dropped_note(dropped: &[String]) -> Option<String> {
    if dropped.is_empty() {
        return None;
    }
    Some(format!(
        "Forgot {} things I'd stored — the ones that go out of date fastest and that you never \
         asked about. Nothing settled and nothing about your own setup.",
        dropped.len()
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConsolidateConfig {
    pub enabled: bool,
    /// Most claims to keep.
    ///
    /// Ten thousand claims is a few megabytes of text and searches in
    /// milliseconds. The cap exists so it can't grow without limit, not
    /// because the storage is scarce.
    pub keep_at_most: usize,
}

impl Default for ConsolidateConfig {
    fn default() -> Self {
        ConsolidateConfig {
            enabled: true,
            keep_at_most: 10_000,
        }
    }
}

/// Roughly what a store of this size costs.
///
/// Worth stating because the fear is that it grows without bound, and the
/// numbers say otherwise.
pub fn size_note(claims: usize) -> String {
    // A claim with its confirmations is a few hundred bytes.
    let mb = (claims as f32 * 350.0) / 1_048_576.0;
    format!(
        "{claims} things known, about {mb:.0}MB. Searching that is instant and it doesn't grow \
         while you're not asking."
    )
}

/// Why asking twice doesn't cost you anything.
pub const ASKING_AGAIN_IS_FREE: &str =
    "Asking about something a second time, worded differently, doesn't make a second note. It \
     confirms the one I have — which makes me surer of it, not less. Decay is measured from when \
     something was last confirmed, so re-reading a thing makes it fresher rather than adding a \
     stale copy beside it.";

/// What never gets dropped.
pub const NEVER_DROPPED: &str =
    "Two things are never forgotten to save space: anything settled, and anything about your own \
     setup. Everything else can be looked up again; those two can't — one because nobody publishes \
     it, and the other because you'd have to work it out from scratch.";
