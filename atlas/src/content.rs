//! Running your content.
//!
//! Not writing it for you — that produces the flat, interchangeable stuff
//! everyone can spot. What a manager actually does is know **why** something
//! worked, notice when you're about to repeat a mistake, and handle the
//! tedious half so you can make more.
//!
//! The knowledge here is about short-form video specifically, because that's
//! where the rules are unusually firm: attention is decided in the first
//! second, retention is the only metric that compounds, and almost every
//! failure is one of about six things.

use serde::{Deserialize, Serialize};

/// How a piece opens. This decides most of its fate before anyone has heard a
/// sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hook {
    /// Names who it's for. "If you trade futures..."
    Called,
    /// States a claim that sounds wrong. "Stop using stop losses."
    Contradiction,
    /// Opens a loop. "This cost me $4,000 to learn."
    Unfinished,
    /// Shows the result first. "Here's the finished thing."
    Outcome,
    /// Asks something they'd answer wrong.
    Question,
    /// No hook. Starts by explaining.
    None,
}

impl Hook {
    /// Roughly how well it holds, from how these behave generally.
    pub fn holds(&self) -> f32 {
        match self {
            Hook::Contradiction => 0.85,
            Hook::Unfinished => 0.8,
            Hook::Called => 0.75,
            Hook::Outcome => 0.7,
            Hook::Question => 0.55,
            Hook::None => 0.25,
        }
    }
    pub fn plain(&self) -> &'static str {
        match self {
            Hook::Called => "names who it's for",
            Hook::Contradiction => "says something that sounds wrong",
            Hook::Unfinished => "opens a loop it doesn't close yet",
            Hook::Outcome => "shows the result first",
            Hook::Question => "asks a question",
            Hook::None => "starts by explaining",
        }
    }
}

/// Read the opening.
pub fn hook_of(first_line: &str) -> Hook {
    let t = first_line.to_lowercase();
    if ["if you", "for anyone", "for people who", "you're a", "youre a", "when you"]
        .iter()
        .any(|p| t.starts_with(p))
    {
        return Hook::Called;
    }
    if ["stop ", "never ", "don't ", "dont ", "everyone is wrong", "you're doing", "youre doing",
        "nobody tells you", "the truth about"]
        .iter()
        .any(|p| t.starts_with(p) || t.contains(p))
    {
        return Hook::Contradiction;
    }
    if ["this cost me", "i lost", "it took me", "three years", "here's what happened",
        "heres what happened", "i almost"]
        .iter()
        .any(|p| t.contains(p))
    {
        return Hook::Unfinished;
    }
    if ["here's the", "heres the", "this is what", "look at"].iter().any(|p| t.starts_with(p)) {
        return Hook::Outcome;
    }
    if t.trim().ends_with('?') {
        return Hook::Question;
    }
    Hook::None
}

/// What's wrong with a piece, in the order it matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    /// The first second is setup rather than substance.
    SlowStart,
    /// It explains before it earns attention.
    ContextFirst,
    /// Nothing happens in the middle.
    SaggyMiddle,
    /// It ends without a reason to watch again or act.
    NoLanding,
    /// It's about a category rather than a case.
    TooGeneral,
    /// Too long for what it says.
    Padded,
    /// The point arrives after most people have gone.
    LateValue,
}

impl Fault {
    pub fn what(&self) -> &'static str {
        match self {
            Fault::SlowStart => "the first second is setup, not substance",
            Fault::ContextFirst => "it explains before it's earned the attention",
            Fault::SaggyMiddle => "nothing happens in the middle",
            Fault::NoLanding => "it stops rather than landing",
            Fault::TooGeneral => "it's about a category, not a case",
            Fault::Padded => "it's longer than what it says",
            Fault::LateValue => "the point arrives after most people have left",
        }
    }
    pub fn fix(&self) -> &'static str {
        match self {
            Fault::SlowStart => "Cut everything before the first real sentence. Usually 2–4 seconds.",
            Fault::ContextFirst => "Move the claim to the front and the background behind it.",
            Fault::SaggyMiddle => "Put the second-strongest thing at the midpoint, not the end.",
            Fault::NoLanding => "End on the thing that makes them watch it again, or one instruction.",
            Fault::TooGeneral => "Replace the category with one specific instance, with numbers.",
            Fault::Padded => "Cut to the length of the idea. A good 22 seconds beats a padded 60.",
            Fault::LateValue => "Whatever is at 40 seconds should be at 8.",
        }
    }
}

/// A piece of content, described.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub first_line: String,
    /// The whole script or transcript.
    pub script: String,
    pub seconds: f32,
    /// Where the substance actually starts.
    pub value_at_secs: f32,
    /// Something specific — a number, a name, a case.
    pub has_specifics: bool,
    /// The end gives a reason to act or rewatch.
    pub lands: bool,
}

pub fn faults(p: &Piece) -> Vec<Fault> {
    let mut out = Vec::new();
    let hook = hook_of(&p.first_line);

    if hook == Hook::None {
        out.push(Fault::ContextFirst);
    }
    // The first second decides most of it. Anything before the substance is
    // a cost paid at the most expensive moment there is.
    if p.value_at_secs > 3.0 {
        out.push(Fault::SlowStart);
    }
    if p.value_at_secs > p.seconds * 0.25 {
        out.push(Fault::LateValue);
    }
    if !p.has_specifics {
        out.push(Fault::TooGeneral);
    }
    if !p.lands {
        out.push(Fault::NoLanding);
    }

    // Words per second — under about 2.2 and it's usually padding rather than
    // pacing. Only worth checking on longer pieces: a short one is either
    // dense or it's already too short to matter.
    let words = p.script.split_whitespace().count() as f32;
    if p.seconds > 30.0 && words / p.seconds.max(1.0) < 2.2 {
        out.push(Fault::Padded);
    }
    if p.seconds > 45.0 {
        out.push(Fault::SaggyMiddle);
    }
    out
}

/// How a piece did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Performance {
    pub id: String,
    pub views: u64,
    /// Fraction who watched to the end.
    pub completion: f32,
    /// Fraction still there at three seconds. The number that decides
    /// distribution.
    pub held_at_three: f32,
    pub saves: u64,
    pub shares: u64,
    pub hook: Hook,
    pub topic: String,
    pub seconds: f32,
}

impl Performance {
    /// What actually predicts reach, rather than what feels good.
    ///
    /// Views are an outcome, not a signal — a piece with 40k views and 8%
    /// completion taught you nothing you can repeat.
    pub fn worth_repeating(&self) -> bool {
        self.held_at_three > 0.6 && self.completion > 0.3
    }
}

/// What worked, across everything you've posted.
///
/// The point is patterns you can act on, not a leaderboard.
#[derive(Debug, Clone, PartialEq)]
pub struct Learned {
    /// Hook to how well it held, and how many times you used it.
    pub by_hook: Vec<(Hook, f32, usize)>,
    pub best_length: Option<(f32, f32)>,
    /// Topics that consistently held attention.
    pub topics_that_work: Vec<String>,
    /// Enough data to trust any of it?
    pub confident: bool,
}

/// What the record says, and whether there is enough of it to say anything.
///
/// `min_posts_for_patterns` was hardcoded here as `>= 8` and printed as a
/// bare `8` in `atlas content learn`, so the shipped `min_posts_for_patterns:
/// 8` agreed with the code by coincidence and raising it changed nothing. The
/// number is the difference between a pattern and a coincidence, which makes
/// it exactly the kind a person should be able to move.
pub fn learn(history: &[Performance], cfg: &ContentConfig) -> Learned {
    let mut by_hook: std::collections::BTreeMap<String, (f32, usize)> = Default::default();
    for p in history {
        let e = by_hook.entry(format!("{:?}", p.hook)).or_insert((0.0, 0));
        e.0 += p.held_at_three;
        e.1 += 1;
    }
    let mut hooks: Vec<(Hook, f32, usize)> = Vec::new();
    for h in [Hook::Called, Hook::Contradiction, Hook::Unfinished, Hook::Outcome, Hook::Question, Hook::None] {
        if let Some((total, n)) = by_hook.get(&format!("{h:?}")) {
            hooks.push((h, total / *n as f32, *n));
        }
    }
    hooks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Length: compare the good ones against the rest rather than averaging
    // everything.
    let good: Vec<&Performance> = history.iter().filter(|p| p.worth_repeating()).collect();
    let best_length = if good.len() >= 3 {
        let lens: Vec<f32> = good.iter().map(|p| p.seconds).collect();
        let min = lens.iter().cloned().fold(f32::MAX, f32::min);
        let max = lens.iter().cloned().fold(0.0, f32::max);
        Some((min, max))
    } else {
        None
    };

    let mut topics: Vec<String> = good.iter().map(|p| p.topic.clone()).collect();
    topics.sort();
    topics.dedup();

    Learned {
        by_hook: hooks,
        best_length,
        topics_that_work: topics,
        // Under about eight posts, any pattern you see is noise -- and how
        // many "about eight" is, is yours.
        confident: history.len() >= cfg.min_posts_for_patterns,
    }
}

/// What Atlas says about a piece before you post it.
pub fn before_posting(p: &Piece) -> String {
    let f = faults(p);
    let hook = hook_of(&p.first_line);
    if f.is_empty() {
        return format!("Opens well — {}. Nothing I'd change.", hook.plain());
    }
    let first = &f[0];
    let mut s = format!("{}. {}", first.what(), first.fix());
    if f.len() > 1 {
        s.push_str(&format!(" {} other thing{}.", f.len() - 1, if f.len() == 2 { "" } else { "s" }));
    }
    s
}

/// What Atlas says about how things are going.
pub fn how_its_going(l: &Learned) -> String {
    if !l.confident {
        return "Not enough posted yet to see a pattern — anything I said would be noise.".into();
    }
    let mut s = String::new();
    if let Some((hook, held, n)) = l.by_hook.first() {
        s.push_str(&format!(
            "Your best opening is the one that {} — {:.0}% still there at three seconds, across {n}.",
            hook.plain(),
            held * 100.0
        ));
    }
    if let Some((min, max)) = l.best_length {
        s.push_str(&format!(" The ones that work run {min:.0} to {max:.0} seconds."));
    }
    if !l.topics_that_work.is_empty() {
        s.push_str(&format!(" {} holds attention.", l.topics_that_work[0]));
    }
    s
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ContentConfig {
    pub enabled: bool,
    /// Review a piece before it goes out.
    pub review_before_posting: bool,
    /// Posts before Atlas will claim a pattern.
    pub min_posts_for_patterns: usize,
}

impl Default for ContentConfig {
    fn default() -> Self {
        ContentConfig {
            enabled: false,
            review_before_posting: true,
            min_posts_for_patterns: 8,
        }
    }
}

/// The edits Atlas can actually make, with ffmpeg, offline.
///
/// Nothing here needs a model or a service — cutting, captioning and resizing
/// are the tedious half, and they're the half a manager should take.
pub fn edits_it_can_do() -> Vec<(&'static str, &'static str)> {
    vec![
        ("trim the dead opening", "cut everything before the first real sentence"),
        ("cut silences", "remove gaps over 400ms, which usually takes 15% off"),
        ("burn in captions", "from the transcript, since most watch on mute"),
        ("crop to vertical", "9:16 from a landscape original"),
        ("normalise the audio", "so it isn't quieter than everything else in the feed"),
        ("pull a thumbnail", "the frame where you're mid-word tends to look worst"),
        ("cut a shorter version", "the same piece at 22 seconds, to compare"),
    ]
}
