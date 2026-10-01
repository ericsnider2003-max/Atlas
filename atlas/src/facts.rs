//! What kind of thing a remembered fact is, and what it connects to.
//!
//! `freshness` knows how long a fact stays true. `consolidate` knows when two
//! facts are the same fact. Neither knows *what kind* of fact it is, so a
//! standing instruction about how Atlas should behave is stored beside a note
//! about a project deadline and recalled the same way.
//!
//! That matters in three places:
//!
//! - **Decay.** A preference does not go stale on a clock. A project note
//!   does. Typing lets `freshness` pick the right shelf instead of guessing
//!   from wording.
//! - **Precedence.** When a recalled fact contradicts what Atlas is about to
//!   do, which wins depends on the kind. Something you told Atlas to do beats
//!   something Atlas noticed.
//! - **Reach.** One fact leads to another. Without links, recall returns a
//!   sentence; with them it returns the sentence and what it depends on.
//!
//! Links are written by name and may point at a fact that does not exist yet.
//! A dangling link is not an error, it marks something worth writing down —
//! and refusing to store one would mean facts could only ever be added in
//! dependency order, which is not how anything is learned.

use serde::{Deserialize, Serialize};

/// Roughly how much fact text the book holds in memory before `trim` starts
/// fading Atlas's own stale guesses. Eight megabytes of plain text is tens of
/// thousands of facts — far more than a person states in years — so on any
/// real device this ceiling is never felt, and it exists only so the book can
/// never grow to a size that slows the machine down. Stated knowledge is never
/// evicted to meet it (see `Book::trim`).
pub const MEMORY_BUDGET_BYTES: usize = 8 * 1024 * 1024;

/// What kind of thing this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Who you are. Role, how you work, what you know.
    You,
    /// What you have told Atlas to do or stop doing. A correction, or an
    /// approach you confirmed.
    Instruction,
    /// Ongoing work: a goal, a constraint, a decision already taken.
    Project,
    /// A pointer somewhere else — a path, an address, a machine.
    Reference,
    /// Something Atlas worked out rather than being told.
    ///
    /// Kept separate on purpose. An observation that has quietly become
    /// indistinguishable from something you said is how a system ends up
    /// confidently acting on its own guess.
    Noticed,
}

impl Kind {
    /// How long this kind stays true by default.
    ///
    /// Typing decides the shelf, so `freshness` no longer has to infer it from
    /// the wording of the claim.
    pub fn shelf(&self) -> crate::freshness::Shelf {
        use crate::freshness::Shelf;
        match self {
            // Yours until you change it. A preference from two years ago is
            // still your preference.
            Kind::You | Kind::Instruction => Shelf::Yours,
            Kind::Project => Shelf::Slow,
            Kind::Reference => Shelf::Slow,
            // The weakest kind gets the shortest life. If Atlas worked it out
            // and has not seen it hold up since, it should fade.
            Kind::Noticed => Shelf::Quick,
        }
    }

    /// Which kind wins when two facts disagree.
    ///
    /// Explicit rather than derived from declaration order, for the same
    /// reason `policy::Decision` and `selfgrant::Reach` are: reordering the
    /// enum for readability must not silently change whose word counts.
    pub fn weight(&self) -> u8 {
        match self {
            Kind::Instruction => 4,
            Kind::You => 3,
            Kind::Project => 2,
            Kind::Reference => 1,
            Kind::Noticed => 0,
        }
    }

    /// Did you say this, or did Atlas decide it?
    pub fn came_from_you(&self) -> bool {
        !matches!(self, Kind::Noticed)
    }

    pub fn plain(&self) -> &'static str {
        match self {
            Kind::You => "about you",
            Kind::Instruction => "something you told me",
            Kind::Project => "about work in progress",
            Kind::Reference => "a pointer to something",
            Kind::Noticed => "something I worked out",
        }
    }
}

/// One remembered fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    /// Short, stable, lowercase-with-dashes. What links point at.
    pub name: String,
    /// One line, so recall can decide relevance without reading the body.
    pub summary: String,
    pub body: String,
    pub kind: Kind,
    /// Names of related facts. May point at something not written yet.
    #[serde(default)]
    pub links: Vec<String>,
    /// When this was last known to be true.
    pub as_of: u64,
    /// Topic tags — what this fact is *about*, cutting across its kind. Two
    /// facts that share a tag are related even when their wording doesn't
    /// overlap, which is what lets recall bring in the neighbours of what you
    /// asked, not just the exact match. Auto-derived from the content, plus any
    /// explicit `#hashtags` you wrote. Lowercased.
    #[serde(default)]
    pub tags: Vec<String>,
    /// How many times it's been restated or confirmed beyond the first. A fact
    /// that keeps coming up is more durable and more valuable: it decays slower
    /// (spaced retention, in `known`), ranks higher in recall, and is never
    /// dropped by `trim`. Zero for a fact heard once, and for every fact from
    /// before this field.
    #[serde(default)]
    pub confirmed: u32,
    /// The thing this fact is *about*, when it has the shape of a statement
    /// about one thing — "my car is a Honda" is about the `car`. Two facts that
    /// fill the same `(subject, attribute)` slot are the same fact with a
    /// possibly-changed value, even when their value words don't overlap, which
    /// is what lets a correction supersede the old value instead of sitting
    /// beside it and contradicting it. `None` for a fact that isn't a plain
    /// statement about one thing (a document sentence, a reference), which falls
    /// back to word-overlap merging. Normalised, lowercase.
    #[serde(default)]
    pub subject: Option<String>,
    /// Which property of the subject this fact gives — "type" for "is a Honda",
    /// "colour" for "the colour of my car is red", "is" for a plain identity.
    /// Part of the slot key alongside `subject`.
    #[serde(default)]
    pub attribute: Option<String>,
    /// The current value in the slot — "honda", "red", "hunter2". What a precise
    /// question about the subject is answered with.
    #[serde(default)]
    pub value: Option<String>,
    /// What it said before a correction replaced it, oldest first: (when it
    /// was true until, what it said). A correction keeps its history
    /// (research report item 20) -- "it used to be X" is still knowable, and
    /// a wrong correction can be undone. At most `HISTORY_KEPT`.
    #[serde(default)]
    pub history: Vec<(u64, String)>,
}

/// A password, a code, a key: never carried into a prompt unasked, whether
/// or not its value looks like one (`redact` judges values; "the wifi
/// password is hunter2pass" names what it is).
pub fn reads_as_secret(text: &str) -> bool {
    let low = text.to_lowercase();
    !crate::redact::secrets_in(text).is_empty()
        || ["password", "passcode", "passphrase", "pin is", "pin number", "api key", "secret", "token", "access code", "recovery key", "seed phrase", "account number", "card number"]
            .iter()
            .any(|w| low.contains(w))
}

/// Earlier wordings kept per fact.
pub const HISTORY_KEPT: usize = 5;

/// Turn a title into a link name.
///
/// Same slug rule the research notes use when naming a file, so a note and a
/// fact about the same thing land on the same name rather than on two that
/// differ only by punctuation.
pub fn slug(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let mut out = String::new();
    let mut last_dash = true;
    for c in s.chars() {
        if c == '-' {
            if !last_dash {
                out.push('-');
                last_dash = true;
            }
        } else {
            out.push(c);
            last_dash = false;
        }
    }
    out.trim_matches('-').chars().take(60).collect()
}

/// Pull `[[name]]` references out of a body.
///
/// Written inline so a fact can be typed as one piece of prose rather than
/// prose plus a separate list that drifts from it.
fn links_in(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else { break };
        let name = slug(&after[..close]);
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
        rest = &after[close + 2..];
    }
    out
}

/// Normalise a subject or entity name: lowercase, trimmed of trailing
/// punctuation, one leading article removed, whitespace collapsed. This is the
/// form a fact's `subject` is stored in and the form aliases key on, so a
/// mention resolves however it was phrased ("the Windows VPS" → "windows vps").
fn norm_subject(s: &str) -> String {
    let s = s.trim().to_lowercase();
    let mut cur = s.trim_end_matches(['.', '!', '?', ',']).trim();
    for lead in ["the ", "my ", "our ", "your ", "a ", "an "] {
        if let Some(r) = cur.strip_prefix(lead) {
            cur = r.trim_start();
            break;
        }
    }
    cur.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An alias declaration — "the windows vps is also known as the homelab
/// server", "the backup box aka the vps" — as `(alias, canonical)`, or `None`.
///
/// The name the sentence leads with is treated as the canonical entity and the
/// one after the marker as another name for it, so a person naming a thing and
/// then its nickname reads the way they'd expect. This is what feeds
/// `Book::note_alias`, collapsing facts about one thing that were filed under
/// the several names it goes by. Deterministic and offline.
pub fn alias_decl(text: &str) -> Option<(String, String)> {
    let low = text.trim().trim_end_matches(['.', '!', '?']).to_lowercase();
    const MARKERS: &[&str] = &[
        " is also known as ",
        " also known as ",
        " is also called ",
        " also called ",
        " aka ",
        " a.k.a. ",
        " is the same as ",
        " is the same thing as ",
        " goes by ",
    ];
    for m in MARKERS {
        if let Some(pos) = low.find(m) {
            let canonical = norm_subject(&low[..pos]);
            let alias = norm_subject(&low[pos + m.len()..]);
            if !canonical.is_empty() && !alias.is_empty() && canonical != alias {
                return Some((alias, canonical));
            }
        }
    }
    None
}

/// Split `text` at the first standalone occurrence of `word`, returning the
/// trimmed text before and after it. Standalone means surrounded by spaces (or
/// the ends), so the `is` inside `this` or `prism` is not a split point.
fn split_once_word(text: &str, word: &str) -> Option<(String, String)> {
    let needle = format!(" {word} ");
    let hay = format!(" {} ", text.trim());
    let pos = hay.find(&needle)?;
    let before = hay[..pos].trim().to_string();
    let after = hay[pos + needle.len()..].trim().to_string();
    Some((before, after))
}

/// A stated fact broken into `(subject, attribute, value)`, when it has the
/// shape of one.
///
/// This is the structured core of learning: it lets "my car is a Honda" and a
/// later "my car is a Toyota" be recognised as the *same slot* with a changed
/// value — so the correction supersedes the old one instead of contradicting
/// it — and lets "what kind of car?" be answered with the single fact that fills
/// that slot rather than everything that mentions a car. Deterministic and
/// offline: it reads a few plain sentence shapes and returns `None` for anything
/// that isn't one, which simply falls back to word-overlap merging. Recognised
/// shapes: "the `<attr>` of `<subj>` is `<val>`", "`<subj>`'s `<attr>` is
/// `<val>`", and "[my|the|our] `<subj>` is [a|an] `<val>`" (the article marks a
/// type, so "is a Honda" and "is fast" are different slots and don't clobber
/// each other).
pub fn triple(text: &str) -> Option<(String, String, String)> {
    let t = text.trim().trim_end_matches(['.', '!', '?', ',']).trim();
    let low = t.to_lowercase();
    let norm =
        |s: &str| s.split_whitespace().map(|w| w.to_ascii_lowercase()).collect::<Vec<_>>().join(" ");
    let strip_lead = |s: &str| -> String {
        let s = s.trim();
        for lead in ["my ", "the ", "our ", "your ", "a ", "an "] {
            if let Some(r) = s.strip_prefix(lead) {
                return r.trim().to_string();
            }
        }
        s.to_string()
    };

    let (before, after) = split_once_word(&low, "is")?;
    if before.is_empty() || after.is_empty() {
        return None;
    }
    // The value, minus a leading article or hedge. An article ("a"/"an") marks a
    // type, which is a different slot from a bare adjective: "my car is a Honda"
    // (type) must not clobber "my car is fast" (a plain quality).
    let (article_attr, value) = if let Some(r) =
        after.strip_prefix("a ").or_else(|| after.strip_prefix("an "))
    {
        (Some("type"), r.trim())
    } else if let Some(r) = after.strip_prefix("now ").or_else(|| after.strip_prefix("currently ")) {
        (None, r.trim())
    } else {
        (None, after.as_str())
    };
    if value.is_empty() {
        return None;
    }
    let value = norm(value);

    // "the <attr> of <subj> is <val>"
    if let Some(rest) = before.strip_prefix("the ") {
        if let Some((attr, subj)) = split_once_word(rest, "of") {
            let subject = norm(&strip_lead(&subj));
            let attribute = norm(&attr);
            if !subject.is_empty() && !attribute.is_empty() {
                return Some((subject, attribute, value));
            }
        }
    }
    // "<subj>'s <attr> is <val>"
    if let Some(apos) = before.find("'s ") {
        let subject = norm(&strip_lead(&before[..apos]));
        let attribute = norm(before[apos + 3..].trim());
        if !subject.is_empty() && !attribute.is_empty() {
            return Some((subject, attribute, value));
        }
    }
    // Plain "[my|the] <subj> is [a] <val>" — subject is the whole lead phrase,
    // attribute is the article marker ("type") or a plain identity ("is").
    let subject = norm(&strip_lead(&before));
    if subject.is_empty() {
        return None;
    }
    Some((subject, article_attr.unwrap_or("is").to_string(), value))
}

impl Fact {
    pub fn new(name: &str, summary: &str, body: &str, kind: Kind, as_of: u64) -> Fact {
        let (subject, attribute, value) = match triple(summary) {
            Some((s, a, v)) => (Some(s), Some(a), Some(v)),
            None => (None, None, None),
        };
        Fact {
            name: slug(name),
            summary: summary.to_string(),
            links: links_in(body),
            tags: tags_from(&format!("{summary} {body}")),
            body: body.to_string(),
            kind,
            as_of,
            confirmed: 0,
            subject,
            attribute,
            value,
            history: Vec::new(),
        }
    }

    /// Whether this fact can ever be dropped or compacted by `trim`.
    ///
    /// Anything you stated — who you are, what you told Atlas to do, a pointer
    /// you gave it — is kept whole forever, and so is anything confirmed more
    /// than once. Only Atlas's own unconfirmed guesses (`Noticed`, confirmed
    /// zero) are ever evictable, and even then the summary survives.
    fn evictable(&self) -> bool {
        self.kind == Kind::Noticed && self.confirmed == 0
    }

    /// Drop the long body but keep the one-line summary, so a stale fact leaves
    /// a trace ("you mentioned X once") rather than vanishing. The first,
    /// gentlest step of eviction.
    fn compact(&mut self) {
        self.body.clear();
        self.links.clear();
    }

    /// Build a fact from something the person stated in plain words — a capture,
    /// or a "remember that …".
    ///
    /// Never `Noticed`: the person said it, so it carries their word, not
    /// Atlas's guess. The kind is read from the phrasing (an instruction, a
    /// pointer, or a fact about you), the name is a short slug of the key words
    /// so a later "what do you know about the wifi" can find it, and the whole
    /// statement is kept as both the one-line summary and the body.
    pub fn stated(text: &str, now: u64) -> Fact {
        let t = text.trim();
        let low = t.to_lowercase();
        let is = |words: &[&str]| words.iter().any(|w| low.contains(w));
        let kind = if is(&[
            "always", "never", "stop ", "don't", "dont ", "prefer", "call me", "from now on",
            "make sure", "remind me to", "please don", "no longer",
        ]) {
            Kind::Instruction
        } else if is(&[
            "password", "passcode", "api key", "key is", "token", "access code", "account number",
            "serial number", "license key", "the path", "located at", "lives at", " live at ", "stored at", "saved in", "ip is", "http",
            "www.", "my address",
        ]) {
            Kind::Reference
        } else {
            Kind::You
        };
        let key: String = terms(t).into_iter().take(4).collect::<Vec<_>>().join(" ");
        let name = if key.is_empty() { format!("note-{now}") } else { key };
        Fact::new(&name, t, t, kind, now)
    }

    /// This fact as something `freshness` can age.
    pub fn known(&self) -> crate::freshness::Known {
        crate::freshness::Known {
            claim: self.summary.clone(),
            shelf: self.kind.shelf(),
            source: if self.kind.came_from_you() {
                crate::freshness::Checkable::YouSaid
            } else {
                crate::freshness::Checkable::ModelAlone
            },
            as_of: self.as_of,
            // Confirmations slow decay: a fact restated or re-checked stays
            // fresh longer, the way a memory strengthens with use.
            confirmed_times: self.confirmed,
        }
    }

    /// How to say this fact as an answer, carrying its provenance so a guess is
    /// never delivered as though you had stated it.
    ///
    /// A fact Atlas worked out for itself (`Noticed`) and has never had
    /// confirmed is hedged — "I think …, that's my own read" — because
    /// answering a guess in the same flat voice as something you told it is
    /// exactly how an assistant ends up confidently wrong. A guess that has held
    /// up (confirmed at least once) is still marked as a guess but with less
    /// hedging. Anything you stated, a pointer, or a piece it looked up is
    /// spoken plainly, with only the freshness caveat `known().spoken` already
    /// attaches when it has aged. This is provenance in the answer itself.
    pub fn answer(&self, now: u64) -> String {
        let base = self.known().spoken(now);
        match (self.kind, self.confirmed) {
            (Kind::Noticed, 0) => {
                format!("I think {} — that's my own read, not something you told me.", lower_first(&base))
            }
            (Kind::Noticed, _) => {
                format!("{base} (I worked that out rather than being told, but it's held up.)")
            }
            _ => base,
        }
    }
}

/// Lowercase the first character, so a claim reads naturally after "I think ".
fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Everything remembered, in one place.
///
/// The `index` is the reason recall stays fast as the book grows: a word maps
/// straight to the facts that use it, so "what do you know about the server"
/// touches only the handful of facts that mention a server, never the whole
/// book. It is rebuilt from `facts` and never stored — persistence is the
/// facts alone, and `load` rebuilds the index once on the way in. This is the
/// difference between memory that scales and memory that slows the machine
/// down a little more with everything it learns.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Book {
    pub facts: Vec<Fact>,
    /// word -> positions in `facts`. Not serialized; rebuilt on load and on
    /// every write. Positions are stable because `put` replaces in place or
    /// appends, and nothing removes.
    #[serde(skip)]
    index: std::collections::BTreeMap<String, Vec<usize>>,
    /// tag -> positions in `facts`, for finding a fact's neighbours by topic in
    /// one lookup rather than a scan. Rebuilt alongside `index`.
    #[serde(skip)]
    by_tag: std::collections::BTreeMap<String, Vec<usize>>,
    /// Which storage shards have changed since the last save. The book is
    /// written as `SHARDS` small files split by fact name, so a single new fact
    /// rewrites one shard — a sixteenth of the book — not the whole thing. This
    /// is what keeps a write cheap however large the book grows. Not persisted.
    #[serde(skip)]
    dirty: [bool; SHARDS],
    /// Alternate name → canonical name, so "the windows vps" and "the
    /// homelab server" resolve to one entity and facts about either collapse
    /// together instead of fragmenting across names. The lightweight core of an
    /// ontology: entities that can be called more than one thing. Stored in its
    /// own file (`fact-aliases`), rebuilt into the book on load.
    #[serde(skip)]
    aliases: std::collections::BTreeMap<String, String>,
    #[serde(skip)]
    aliases_dirty: bool,
}

/// How many files the book is split across on disk. A new fact rewrites only
/// the shard its name falls in, so per-write cost is ~1/16th of the book rather
/// than all of it.
const SHARDS: usize = 16;

/// Which shard a fact's name belongs to. A stable FNV-1a hash so a fact always
/// lands in the same shard across runs.
fn shard_of(name: &str) -> usize {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    (h % SHARDS as u64) as usize
}

// Two books are equal when they hold the same facts; the index is derived, so
// comparing it would be comparing the same thing twice.
impl PartialEq for Book {
    fn eq(&self, other: &Self) -> bool {
        self.facts == other.facts
    }
}

/// Split text into the words worth indexing: lowercased, three letters or more,
/// and not one of the handful that carry no meaning on their own.
fn terms(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "for", "that", "this", "with", "you", "your", "are", "was", "has", "have",
        "not", "but", "all", "any", "how", "what", "when", "where", "who", "why", "about", "from",
        "into", "out", "off", "its", "their", "they", "them", "then", "than", "some", "one",
    ];
    text.split(|c: char| !c.is_alphanumeric())
        .filter_map(|w| {
            let w = w.to_ascii_lowercase();
            (w.len() >= 3 && !STOP.contains(&w.as_str())).then_some(w)
        })
        .collect()
}

/// Whether two facts say the same thing, by word overlap of their summaries.
///
/// Deliberately simple and offline — the same idea as `consolidate::same_claim`
/// — so restating a fact a slightly different way merges into the one you
/// already have rather than adding a near-duplicate. Two thirds of the shorter
/// summary's meaningful words shared is enough to call it the same thing.
/// Topic tags for a fact: the explicit `#hashtags` a person wrote, plus a few
/// distinctive content words, so facts about the same thing can find each other
/// even when the wording differs. Lowercased, deduped, capped small.
fn tags_from(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Explicit hashtags win — a person tagging a fact on purpose.
    for w in text.split_whitespace() {
        if let Some(rest) = w.strip_prefix('#') {
            let t: String = rest.chars().take_while(|c| c.is_alphanumeric()).collect::<String>().to_ascii_lowercase();
            if t.len() >= 2 && !out.contains(&t) {
                out.push(t);
            }
        }
    }
    // Then a few of the most distinctive words (longest first are usually the
    // topical ones — "password", "homelab" — over "runs", "note").
    let mut words: Vec<String> = terms(text);
    words.sort_by(|a, b| b.len().cmp(&a.len()));
    for w in words {
        if out.len() >= 6 {
            break;
        }
        if !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

fn same_fact(a: &Fact, b: &Fact) -> bool {
    let sa: std::collections::BTreeSet<String> = terms(&a.summary).into_iter().collect();
    let sb: std::collections::BTreeSet<String> = terms(&b.summary).into_iter().collect();
    if sa.is_empty() || sb.is_empty() {
        return false;
    }
    let shared = sa.intersection(&sb).count();
    let smaller = sa.len().min(sb.len());
    shared * 3 >= smaller * 2
}

/// Break a body of text into learnable facts — paragraphs, and long paragraphs
/// into sentences — dropping anything too short to be a fact.
///
/// This is how a document, a page of notes, or a reference file becomes many
/// discrete, indexed, tagged facts in one pass, which is how the knowledge base
/// grows vast without a hundred separate captures.
pub fn into_facts(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split("\n\n") {
        let para = para.split_whitespace().collect::<Vec<_>>().join(" ");
        if para.is_empty() {
            continue;
        }
        if para.len() <= 300 {
            if terms(&para).len() >= 3 {
                out.push(para);
            }
        } else {
            for sent in split_sentences(&para) {
                if terms(&sent).len() >= 3 {
                    out.push(sent);
                }
            }
        }
    }
    out
}

/// Split a paragraph into sentences, on a `.`, `!` or `?` followed by space.
/// Offline and imprecise on purpose — it only has to break a wall of text into
/// facts small enough to recall, not parse grammar.
fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let ends = matches!(c, '.' | '!' | '?')
            && chars.get(i + 1).map(|n| n.is_whitespace()).unwrap_or(true);
        if ends {
            let s = cur.trim().to_string();
            if !s.is_empty() {
                out.push(s);
            }
            cur.clear();
        }
    }
    let s = cur.trim().to_string();
    if !s.is_empty() {
        out.push(s);
    }
    out
}

/// A fact built from external reference material — a document, a page, a
/// research finding. Kind is `Reference`: knowledge Atlas holds, not something
/// you stated about yourself and not a guess it made, so it is kept (never an
/// evictable guess) and decays slowly the way looked-up knowledge should.
pub fn reference_fact(text: &str, now: u64) -> Fact {
    let key: String = terms(text).into_iter().take(4).collect::<Vec<_>>().join(" ");
    let name = if key.is_empty() { format!("ref-{now}") } else { key };
    let summary: String = text.chars().take(140).collect();
    Fact::new(&name, &summary, text, Kind::Reference, now)
}

/// Whether a fact is a confident answer to a question — enough of the
/// question's meaningful words appear in it that it is plausibly the answer,
/// not a stray one-word match. This is the gate that lets a plain question be
/// answered from the book when the book really knows, and fall through to
/// searching notes when it doesn't, so Atlas never answers an unrelated
/// question with whatever fact happened to share a word.
pub fn answers(fact: &Fact, question: &str) -> bool {
    let q: std::collections::BTreeSet<String> = terms(question).into_iter().collect();
    if q.is_empty() {
        return false;
    }
    let hay: std::collections::BTreeSet<String> = terms(&fact.name)
        .into_iter()
        .chain(terms(&fact.summary))
        .chain(terms(&fact.body))
        .collect();
    let shared = q.iter().filter(|w| hay.contains(*w)).count();
    // At least 60% of the question's meaningful words are in the fact.
    shared * 5 >= q.len() * 3
}

impl Book {
    /// Load the book from its shards and build its index, so recall is ready
    /// immediately. Falls back to the pre-sharding single `facts` file the
    /// first time, then writes shards from then on.
    pub fn load(store: &crate::store::Store) -> Book {
        let mut facts: Vec<Fact> = Vec::new();
        let mut any_shard = false;
        for s in 0..SHARDS {
            let shard: Vec<Fact> = store.load(&format!("facts-{s}"));
            if !shard.is_empty() {
                any_shard = true;
            }
            facts.extend(shard);
        }
        let mut b = Book::default();
        if any_shard {
            b.facts = facts;
            // Loaded cleanly from shards; nothing to rewrite until a change.
        } else {
            // Migrate the old single file, if there is one, and mark every
            // shard to be written once so the next save lays them down.
            let legacy: Book = store.load("facts");
            b.facts = legacy.facts;
            if !b.facts.is_empty() {
                b.dirty = [true; SHARDS];
            }
        }
        b.aliases = store.load("fact-aliases");
        b.reindex();
        b
    }

    /// Write only the shards that changed since the last save. A single new
    /// fact touches one shard, so this writes a sixteenth of the book, not all
    /// of it — the incremental persistence that keeps a save from getting
    /// slower as the book grows.
    pub fn save(&mut self, store: &crate::store::Store) -> crate::error::Result<()> {
        let mut result = Ok(());
        for s in 0..SHARDS {
            if !self.dirty[s] {
                continue;
            }
            let shard: Vec<&Fact> = self.facts.iter().filter(|f| shard_of(&f.name) == s).collect();
            if let Err(e) = store.save(&format!("facts-{s}"), &shard) {
                result = Err(e);
            } else {
                self.dirty[s] = false;
            }
        }
        if self.aliases_dirty {
            if let Err(e) = store.save("fact-aliases", &self.aliases) {
                result = Err(e);
            } else {
                self.aliases_dirty = false;
            }
        }
        result
    }

    /// Resolve a subject through the alias map to its canonical name, following
    /// a short chain ("backup box" → "windows vps" → "homelab server") and
    /// stopping at the entity everything points to. Normalised. A subject with
    /// no alias returns itself.
    fn canonical(&self, subject: &str) -> String {
        let mut cur = norm_subject(subject);
        for _ in 0..6 {
            match self.aliases.get(&cur) {
                Some(next) if next != &cur => cur = next.clone(),
                _ => break,
            }
        }
        cur
    }

    /// Record that `alias` is another name for `canonical`, and pull any facts
    /// already filed under the alias onto the canonical entity — merging ones
    /// that now share a slot — so knowledge about one thing stops fragmenting
    /// across the names it goes by. Returns true if the alias was new.
    pub fn note_alias(&mut self, alias: &str, canonical: &str, now: u64) -> bool {
        let a = norm_subject(alias);
        let c = self.canonical(canonical);
        if a.is_empty() || c.is_empty() || a == c {
            return false;
        }
        let is_new = self.aliases.insert(a.clone(), c.clone()).as_deref() != Some(c.as_str());
        self.aliases_dirty = true;
        // Re-point existing facts filed under the alias onto the canonical.
        let mut moved: Vec<Fact> = Vec::new();
        let mut kept: Vec<Fact> = Vec::new();
        for mut f in self.facts.drain(..) {
            if f.subject.as_deref() == Some(a.as_str()) {
                f.subject = Some(c.clone());
                moved.push(f);
            } else {
                kept.push(f);
            }
        }
        self.facts = kept;
        self.dirty = [true; SHARDS];
        self.reindex();
        // Re-learn the moved facts so same-slot duplicates merge, not pile up.
        for f in moved {
            self.learn(f, now);
        }
        is_new
    }

    /// Rebuild the word index from the facts. Cheap relative to how often facts
    /// are read, and called after every write so recall is never stale.
    fn reindex(&mut self) {
        self.index.clear();
        self.by_tag.clear();
        for (i, f) in self.facts.iter().enumerate() {
            let mut seen = std::collections::BTreeSet::new();
            // The canonical subject is indexed alongside the fact's own words,
            // so a fact filed under an entity is findable by that entity's name
            // even when the fact's text used one of its other names.
            let subject_terms = f.subject.as_deref().map(terms).unwrap_or_default();
            for t in terms(&f.name)
                .into_iter()
                .chain(terms(&f.summary))
                .chain(terms(&f.body))
                .chain(subject_terms)
            {
                if seen.insert(t.clone()) {
                    self.index.entry(t).or_default().push(i);
                }
            }
            for tag in &f.tags {
                self.by_tag.entry(tag.clone()).or_default().push(i);
            }
        }
    }

    /// The neighbours of a fact — what else it's related to, by shared topic
    /// tag or by an explicit `[[link]]`, best first and excluding itself.
    ///
    /// This is what turns recall from lookup into association: ask about the
    /// server and the fact about its password and the one about who runs it
    /// come along, because they share a tag or point at each other. The tag and
    /// link indexes make it a couple of small lookups, not a scan, so it stays
    /// cheap however large the book grows.
    pub fn related(&self, fact: &Fact, now: u64, limit: usize) -> Vec<&Fact> {
        use crate::freshness::State;
        let mut score: std::collections::BTreeMap<usize, i64> = std::collections::BTreeMap::new();
        let self_name = &fact.name;
        // Neighbours by shared tag.
        for tag in &fact.tags {
            if let Some(positions) = self.by_tag.get(tag) {
                for &i in positions {
                    if &self.facts[i].name != self_name {
                        *score.entry(i).or_insert(0) += 2;
                    }
                }
            }
        }
        // Neighbours by an explicit link, either direction.
        for link in &fact.links {
            if let Some(i) = self.facts.iter().position(|x| &x.name == link) {
                if &self.facts[i].name != self_name {
                    *score.entry(i).or_insert(0) += 3;
                }
            }
        }
        for (i, f) in self.facts.iter().enumerate() {
            if f.links.iter().any(|l| l == self_name) && &f.name != self_name {
                *score.entry(i).or_insert(0) += 3;
            }
        }
        let mut hits: Vec<(i64, usize)> = score
            .into_iter()
            .map(|(i, s)| {
                let f = &self.facts[i];
                let mut total = s + f.kind.weight() as i64 + f.confirmed.min(5) as i64;
                if f.known().state(now) == State::Stale {
                    total -= 4;
                }
                (total, i)
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(self.facts[b.1].as_of.cmp(&self.facts[a.1].as_of)));
        hits.into_iter().take(limit).map(|(_, i)| &self.facts[i]).collect()
    }

    pub fn get(&self, name: &str) -> Option<&Fact> {
        let n = slug(name);
        self.facts.iter().find(|f| f.name == n)
    }

    /// Add, or replace one of the same name.
    ///
    /// Replacing rather than appending: two facts under one name means recall
    /// picks whichever it reaches first, and which one that is depends on
    /// insertion order. Same reasoning as `consolidate`.
    pub fn put(&mut self, f: Fact) {
        let s = shard_of(&f.name);
        match self.facts.iter().position(|x| x.name == f.name) {
            Some(i) => self.facts[i] = f,
            None => self.facts.push(f),
        }
        self.dirty[s] = true;
        self.reindex();
    }

    /// Learn a fact, merging it into an existing one when it restates something
    /// already known rather than adding a duplicate.
    ///
    /// Restating strengthens, the way a memory does with use: the confirmation
    /// count rises (so the fact decays slower and ranks higher), the timestamp
    /// refreshes, and the richer body and stronger-sourced kind win. This is
    /// what keeps the book from growing every time you say the same thing a
    /// different way. Returns true if it merged into an existing fact, false if
    /// it added a new one.
    pub fn learn(&mut self, mut f: Fact, now: u64) -> bool {
        // Resolve the subject to its canonical entity first, so a fact stated
        // about an alias ("the windows vps") lands on the same slot as facts
        // about its canonical name ("the homelab server") instead of a
        // separate one — knowledge about one thing stays in one place.
        if let Some(subj) = &f.subject {
            let c = self.canonical(subj);
            if &c != subj {
                f.subject = Some(c);
            }
        }
        // Structured supersession first: a fact that fills the same
        // `(subject, attribute)` slot is the same fact with a possibly-new
        // value, even when the value words share nothing ("my car is a Honda"
        // then "…a Toyota"). This is stronger and more correct than word
        // overlap — it is what makes a correction replace the old value instead
        // of contradicting it — so it is tried before `same_fact`.
        let slot_hit = f.subject.as_ref().and_then(|_| {
            self.facts
                .iter()
                .position(|x| x.subject.is_some() && x.subject == f.subject && x.attribute == f.attribute)
        });
        let hit = slot_hit.or_else(|| self.facts.iter().position(|x| x.name == f.name || same_fact(x, &f)));
        match hit {
            Some(i) => {
                // Which statement wins (Eric, H13c): what you told it beats
                // what it noticed, and newer beats older. A tie in both goes
                // to the richer wording.
                let new_wins = {
                    let old = &self.facts[i];
                    let settled = self.settles(old, &f);
                    std::ptr::eq(settled, &f)
                        || (old.kind.came_from_you() == f.kind.came_from_you()
                            && f.as_of == old.as_of
                            && f.body.len() > old.body.len())
                };
                let existing = &mut self.facts[i];
                existing.confirmed = existing.confirmed.saturating_add(1);
                // The value: a newer statement supersedes an older one, so
                // correcting something ("my car is a Honda" → "…a Toyota")
                // replaces it and the old version stops surfacing — vast *and*
                // accurate, not vast and contradicting itself. A tie goes to the
                // richer wording. Tags only ever grow, so association doesn't
                // shrink when a fact is updated.
                if new_wins {
                    // A different statement replaces it; the old one is kept.
                    if existing.summary.trim() != f.summary.trim() && !existing.summary.trim().is_empty() {
                        let was = (existing.as_of, existing.summary.clone());
                        existing.history.push(was);
                        let extra = existing.history.len().saturating_sub(HISTORY_KEPT);
                        existing.history.drain(..extra);
                    }
                    existing.summary = f.summary;
                    existing.body = f.body;
                    existing.links = f.links;
                    // The slot's value moves with the newer statement; the
                    // subject/attribute stay (they are the slot's identity), and
                    // are filled in if the older fact predated structured facts.
                    if f.value.is_some() {
                        existing.value = f.value;
                    }
                    if existing.subject.is_none() {
                        existing.subject = f.subject;
                        existing.attribute = f.attribute;
                    }
                }
                for t in f.tags {
                    if !existing.tags.contains(&t) {
                        existing.tags.push(t);
                    }
                }
                existing.as_of = now.max(existing.as_of);
                if f.kind.weight() > existing.kind.weight() {
                    existing.kind = f.kind;
                }
                let s = shard_of(&existing.name);
                self.dirty[s] = true;
                self.reindex();
                true
            }
            None => {
                let s = shard_of(&f.name);
                self.facts.push(f);
                self.dirty[s] = true;
                self.reindex();
                false
            }
        }
    }

    /// Keep the book's memory footprint bounded without losing what matters.
    ///
    /// `budget_bytes` is roughly how much fact text the book will hold. Under
    /// it, nothing is touched. Over it, eviction proceeds in the gentlest order
    /// that never loses stated knowledge:
    ///
    /// 1. **Compact** the evictable facts — Atlas's own unconfirmed guesses that
    ///    have gone stale — dropping the long body but keeping the one-line
    ///    summary, so a faded guess leaves a trace rather than vanishing.
    /// 2. If still over, **drop** the oldest of those stale guesses entirely.
    ///
    /// A fact you stated, or one confirmed more than once, is never compacted or
    /// dropped. If nothing is left that may be evicted, the book stays over
    /// budget rather than lose knowledge you gave it — a little more memory is
    /// always better than forgetting something you said. Returns how many facts
    /// were dropped whole.
    pub fn trim(&mut self, budget_bytes: usize, now: u64) -> usize {
        use crate::freshness::State;
        let footprint =
            |facts: &[Fact]| facts.iter().map(|f| f.summary.len() + f.body.len()).sum::<usize>();
        if footprint(&self.facts) <= budget_bytes {
            return 0;
        }
        // Eviction rewrites across the book; it's rare (only near the ceiling),
        // so mark every shard to be written rather than track each one.
        self.dirty = [true; SHARDS];

        // 1. Compact stale guesses, oldest first.
        let mut order: Vec<usize> = (0..self.facts.len())
            .filter(|&i| {
                let f = &self.facts[i];
                f.evictable() && !f.body.is_empty() && f.known().state(now) == State::Stale
            })
            .collect();
        order.sort_by_key(|&i| self.facts[i].as_of);
        for i in order {
            if footprint(&self.facts) <= budget_bytes {
                break;
            }
            self.facts[i].compact();
        }

        // 2. Still over: drop the oldest stale guesses whole, one at a time,
        //    stopping the moment nothing evictable is left.
        let mut dropped = 0;
        while footprint(&self.facts) > budget_bytes {
            let victim = self
                .facts
                .iter()
                .enumerate()
                .filter(|(_, f)| f.evictable() && f.known().state(now) == State::Stale)
                .min_by_key(|(_, f)| f.as_of)
                .map(|(i, _)| i);
            match victim {
                Some(i) => {
                    self.facts.remove(i);
                    dropped += 1;
                }
                None => break,
            }
        }
        self.reindex();
        dropped
    }

    /// What the book knows about a query, best first.
    ///
    /// The index turns the query's words into a small candidate set, then each
    /// candidate is scored: a matching word in the name or the one-line summary
    /// counts for more than one buried in the body, a fact you stated outranks
    /// one Atlas merely noticed (via `Kind::weight`), and a fact that has gone
    /// stale on its shelf is pushed down but not dropped. It reads the index,
    /// not the whole book, so it stays fast however much is remembered.
    pub fn recall(&self, query: &str, now: u64) -> Vec<&Fact> {
        self.recall_in_context(query, &[], now)
    }

    /// `recall`, with the live conversation allowed to break ties.
    ///
    /// The same rule `recall::search_in_context` keeps: context adds a small,
    /// capped bonus to facts the QUERY already found, and puts nothing new on
    /// the list. Two facts share the question's word, the one about what you
    /// were just discussing wins; a fact the question never touched stays
    /// exactly where it was — unmentioned.
    pub fn recall_in_context(&self, query: &str, context: &[String], now: u64) -> Vec<&Fact> {
        use crate::freshness::State;
        let mut q = terms(query);
        if q.is_empty() {
            return Vec::new();
        }
        // Expand the query through aliases so asking by one name for an entity
        // finds facts filed under any of its names. If the query names an alias
        // or a canonical entity in full, the other name's words are added.
        if !self.aliases.is_empty() {
            let qset: std::collections::BTreeSet<&String> = q.iter().collect();
            let mut extra: Vec<String> = Vec::new();
            let mut add_if_named = |from: &str, to: &str| {
                let ft = terms(from);
                if !ft.is_empty() && ft.iter().all(|w| qset.contains(w)) {
                    extra.extend(terms(to));
                }
            };
            for (alias, canon) in &self.aliases {
                add_if_named(alias, canon);
                add_if_named(canon, alias);
            }
            for t in extra {
                if !q.contains(&t) {
                    q.push(t);
                }
            }
        }
        let mut score: std::collections::BTreeMap<usize, i64> = std::collections::BTreeMap::new();
        for term in &q {
            if let Some(positions) = self.index.get(term) {
                for &i in positions {
                    let f = &self.facts[i];
                    let mut s = 2; // a body match
                    if terms(&f.name).contains(term) {
                        s += 5;
                    }
                    if terms(&f.summary).contains(term) {
                        s += 3;
                    }
                    *score.entry(i).or_insert(0) += s;
                }
            }
        }
        // The conversation's nudge: +1 per context word a fact carries, at
        // most +3 — beside query scores of 2–10 per matching term, enough to
        // settle a tie and nothing more. Written into a separate map keyed by
        // facts already in `score`, so context can only re-rank what the
        // question found, never add to it.
        let mut context_bonus: std::collections::BTreeMap<usize, i64> =
            std::collections::BTreeMap::new();
        if !context.is_empty() {
            for term in context {
                if let Some(positions) = self.index.get(term) {
                    for &i in positions {
                        if score.contains_key(&i) {
                            let b = context_bonus.entry(i).or_insert(0);
                            *b = (*b + 1).min(3);
                        }
                    }
                }
            }
        }
        let mut hits: Vec<(i64, usize)> = score
            .into_iter()
            .map(|(i, s)| {
                let f = &self.facts[i];
                // Stated facts outrank noticed ones; a fact confirmed more often
                // is worth more; stale facts sink but stay.
                let mut total = s + f.kind.weight() as i64 + f.confirmed.min(5) as i64;
                if f.known().state(now) == State::Stale {
                    total -= 4;
                }
                total += context_bonus.get(&i).copied().unwrap_or(0);
                (total, i)
            })
            .collect();
        // Highest score first; ties broken by most recent.
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(self.facts[b.1].as_of.cmp(&self.facts[a.1].as_of)));
        hits.into_iter().map(|(_, i)| &self.facts[i]).collect()
    }

    /// The single fact that fills the slot a question asks about, if any.
    ///
    /// "what kind of car do I have" → the fact whose subject is `car`; "what's
    /// the wifi password" → the fact whose subject is `wifi password`. This is
    /// *precise* recall: it returns the one fact that answers, carrying the
    /// current value, not everything that happens to share a word — which is how
    /// Atlas gives a clean, current answer instead of a pile of near-matches.
    /// The question must name the whole subject, so it never fires on a stray
    /// overlap. Best-confirmed, strongest-kind fact wins a tie.
    pub fn slot_answer(&self, question: &str, _now: u64) -> Option<&Fact> {
        let qterms: std::collections::BTreeSet<String> = terms(question).into_iter().collect();
        if qterms.is_empty() {
            return None;
        }
        // canonical entity → the other names it goes by, so a question that
        // asks by an alias still finds the fact filed under the canonical name.
        let mut names_for: std::collections::BTreeMap<&str, Vec<&str>> =
            std::collections::BTreeMap::new();
        for (alias, canon) in &self.aliases {
            names_for.entry(canon.as_str()).or_default().push(alias.as_str());
        }
        let mut best: Option<(i64, usize)> = None;
        for (i, f) in self.facts.iter().enumerate() {
            let Some(subj) = &f.subject else { continue };
            // The question answers this fact if it names the subject or any of
            // the subject's aliases in full.
            let mut names: Vec<&str> = vec![subj.as_str()];
            if let Some(al) = names_for.get(subj.as_str()) {
                names.extend(al.iter().copied());
            }
            let matched = names.iter().any(|name| {
                let nt = terms(name);
                !nt.is_empty() && nt.iter().all(|w| qterms.contains(w))
            });
            if !matched {
                continue;
            }
            let score = f.confirmed as i64 + f.kind.weight() as i64 + terms(subj).len() as i64;
            if best.map(|(s, _)| score > s).unwrap_or(true) {
                best = Some((score, i));
            }
        }
        best.map(|(_, i)| &self.facts[i])
    }

    /// Facts that point at nothing that exists yet.
    ///
    /// Reported rather than refused. Writing a link before the thing it points
    /// at is how notes actually get made, and it marks what is worth writing
    /// next.
    pub fn dangling(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for f in &self.facts {
            for l in &f.links {
                if self.get(l).is_none() {
                    out.push((f.name.clone(), l.clone()));
                }
            }
        }
        out
    }

    /// A fact and everything it reaches, up to `depth` hops.
    ///
    /// Breadth first, so the fact you asked for and its immediate neighbours
    /// come before anything further out. Depth is capped by the caller because
    /// following every link is how a recall turns into the whole book.
    pub fn reachable(&self, from: &str, depth: usize) -> Vec<&Fact> {
        let mut seen: Vec<String> = Vec::new();
        let mut out: Vec<&Fact> = Vec::new();
        let mut frontier = vec![slug(from)];
        for _ in 0..=depth {
            let mut next = Vec::new();
            for name in frontier.drain(..) {
                if seen.contains(&name) {
                    continue;
                }
                seen.push(name.clone());
                if let Some(f) = self.get(&name) {
                    out.push(f);
                    next.extend(f.links.iter().cloned());
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        out
    }

    /// Which fact wins when two say different things.
    ///
    /// Eric's rule (25 Sep 2026, H13c): what you told it beats what it
    /// noticed, and newer beats older. Told-or-noticed first, because a
    /// preference you stated last year still beats something Atlas noticed
    /// this morning — recency alone would let an observation quietly override
    /// an instruction. Between two things you said, the newer one is you
    /// changing your mind, whatever kind of fact each was filed as.
    pub fn settles<'a>(&self, a: &'a Fact, b: &'a Fact) -> &'a Fact {
        match (a.kind.came_from_you(), b.kind.came_from_you()) {
            (true, false) => a,
            (false, true) => b,
            _ => {
                if a.as_of >= b.as_of {
                    a
                } else {
                    b
                }
            }
        }
    }

    /// Facts of a kind, newest first.
    /// Who you are, at a glance: what's always in front of the model
    /// (research report item 20, "a who-Eric-is core memory"). Your standing
    /// instructions first, then facts about you; within each, the ones
    /// you've confirmed more, then the newer. Never a pointer (a path, an
    /// address, a password -- those are looked up when asked, not carried in
    /// every prompt) and never anything that reads as a secret. The order is
    /// fixed for the same facts, so the model server can reuse it turn after
    /// turn.
    pub fn core(&self, most: usize) -> Vec<&Fact> {
        let mut c: Vec<&Fact> = self
            .facts
            .iter()
            .filter(|f| matches!(f.kind, Kind::Instruction | Kind::You))
            .filter(|f| !f.summary.trim().is_empty() && !reads_as_secret(&f.summary))
            .collect();
        c.sort_by(|a, b| {
            b.kind
                .weight()
                .cmp(&a.kind.weight())
                .then(b.confirmed.min(5).cmp(&a.confirmed.min(5)))
                .then(b.as_of.cmp(&a.as_of))
                .then(a.name.cmp(&b.name))
        });
        c.truncate(most);
        c
    }

    /// Everything else you told it that bears on what was just said, best
    /// first: how well it matches (relevance), what kind it is (importance),
    /// and how recently it was true (recency) -- the three-part ranking
    /// generative-agent memory uses. Leaves out the core (already in front of
    /// the model) and anything that reads as a secret.
    pub fn bearing_on(&self, said: &str, now: u64, skip: &[&str], most: usize) -> Vec<&Fact> {
        let words = terms(said);
        if words.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(f64, &Fact)> = self
            .facts
            .iter()
            .filter(|f| f.kind.came_from_you() && !skip.contains(&f.name.as_str()))
            .filter(|f| !reads_as_secret(&f.summary))
            .filter_map(|f| {
                let hay = format!("{} {}", f.summary.to_lowercase(), f.tags.join(" "));
                let hits = words.iter().filter(|w| hay.contains(w.as_str())).count();
                if hits == 0 {
                    return None;
                }
                let relevance = hits as f64 / words.len() as f64;
                let importance = 1.0 + f.kind.weight() as f64 / 4.0 + (f.confirmed.min(5) as f64) / 10.0;
                let days = now.saturating_sub(f.as_of) as f64 / 86_400.0;
                let recency = match f.kind.shelf() {
                    crate::freshness::Shelf::Yours => 1.0,
                    _ => 0.5f64.powf(days / 30.0).max(0.2),
                };
                Some((relevance * importance * recency, f))
            })
            .filter(|(s, _)| *s >= 0.3)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.name.cmp(&b.1.name)));
        scored.into_iter().take(most).map(|(_, f)| f).collect()
    }

    pub fn of_kind(&self, kind: Kind) -> Vec<&Fact> {
        let mut v: Vec<&Fact> = self.facts.iter().filter(|f| f.kind == kind).collect();
        v.sort_by(|a, b| b.as_of.cmp(&a.as_of));
        v
    }

    /// What Atlas worked out rather than being told, oldest first.
    ///
    /// The review list. An observation nobody has confirmed in months is a
    /// guess that has been sitting in memory long enough to look like a fact.
    pub fn guesses_worth_checking(&self, now: u64) -> Vec<&Fact> {
        use crate::freshness::State;
        let mut v: Vec<&Fact> = self
            .facts
            .iter()
            .filter(|f| f.kind == Kind::Noticed)
            .filter(|f| f.known().state(now) != State::Fresh)
            .collect();
        v.sort_by_key(|f| f.as_of);
        v
    }
}
