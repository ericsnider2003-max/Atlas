//! Working a decision instead of answering it.
//!
//! `council` gets you disagreement. `otherside` argues against something you
//! have already chosen. `certainty` says how sure it is. Each is one move.
//! Nothing runs the whole thing, so a decision gets whichever move you happen
//! to ask for.
//!
//! The order below is not arbitrary and it is the part that does the work.
//! Framing comes before options, because most bad decisions are right answers
//! to the wrong question. Options come before evidence, because the moment
//! anything is recommended you stop comparing the rest. Stakes come last,
//! because how much thought a decision deserves depends on what it costs to be
//! wrong, and you cannot know that until you know what you are choosing
//! between.
//!
//! **It recommends only when it can show its working.** An unexplained pick is
//! a coin flip in a confident voice, and that is what the no-recommendation
//! rule was really guarding against. The fix is not silence, it is a
//! recommendation that arrives with what it rests on attached, so you can
//! disagree with the reasoning rather than only with the answer.
//!
//! Three conditions before it will lean. Every option has to have been costed,
//! or the cheap-looking one wins by not having been examined. Something has to
//! have been argued against it, because a lean nobody attacked is a preference
//! wearing evidence. And where the choice turns on what you want rather than
//! on what is true, it asks instead of assuming — an assistant guessing your
//! preference and then reasoning from the guess is confidently wrong in a way
//! you cannot see.
//!
//! One exception, and it is the useful one: when a decision is cheap and
//! reversible, working it is a waste of your evening. It says so and stops.

use serde::{Deserialize, Serialize};

/// The moves, in the order they are worth making.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Move {
    /// What question is actually being asked.
    Framing,
    /// What the boring answer is, and what would have to be true to beat it.
    Default,
    /// The options, and what each one costs.
    Options,
    /// What each option rests on, and what would change it.
    Evidence,
    /// The strongest case against the way it is leaning.
    Reversal,
    /// If this is wrong: what breaks, when you find out, whether you can undo.
    Stakes,
    /// How sure, and what is pulling that down.
    Confidence,
    /// The questions you have not answered yet.
    Ownership,
}

impl Move {
    /// The question this move asks, in the words you would use out loud.
    pub fn asks(&self) -> &'static str {
        match self {
            Move::Framing => "What's the question you should be asking that you haven't?",
            Move::Default => "What's the boring default here, and what would have to be true to beat it?",
            Move::Options => "What are the options, and what does each one cost you?",
            Move::Evidence => "What is each of these resting on, and what would change it?",
            Move::Reversal => "What's the strongest case against the way this is leaning?",
            Move::Stakes => "If this is wrong: what breaks, when do you find out, can you undo it?",
            Move::Confidence => "How sure are you, and what's pulling that down?",
            Move::Ownership => "What do you need answered before you can decide?",
        }
    }

    /// Every move, in order.
    pub fn all() -> [Move; 8] {
        [
            Move::Framing,
            Move::Default,
            Move::Options,
            Move::Evidence,
            Move::Reversal,
            Move::Stakes,
            Move::Confidence,
            Move::Ownership,
        ]
    }
}

/// How much it costs to be wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weight {
    /// Cheap and reversible. Working it costs more than getting it wrong.
    JustPick,
    /// Costly or slow to undo. Worth the full pass.
    WorthWorking,
    /// Cannot be undone. Nothing here decides it for you.
    OneWay,
}

/// One option and what taking it costs.
///
/// `costs` is not optional and that is deliberate. An option with no stated
/// cost is one whose cost nobody looked for, and it will win by default
/// against options that were honest about theirs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Option_ {
    pub what: String,
    pub costs: String,
    /// What this option assumes. Empty is a finding, not a blank.
    #[serde(default)]
    pub rests_on: Vec<String>,
    /// Why this one stopped being a candidate.
    ///
    /// Set aside rather than deleted. An option that did not hold up is
    /// evidence about the others, and quietly dropping it means the same idea
    /// comes back in a month looking new.
    #[serde(default)]
    pub set_aside: Option<String>,
}

/// A recommendation, with what it rests on.
///
/// Two shapes, in the order Atlas speaks. `spoken()` is the recommendation on
/// its own, short enough to say. `written()` is the same recommendation with
/// the working underneath.
///
/// Voice first, then written, the same as everything else. Reading three
/// paragraphs of reasoning aloud to reach an answer at the end is the wrong
/// order for a spoken assistant — you asked what to do, and the reasoning is
/// there when you want it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lean {
    pub option: String,
    /// Why. Never empty — an unexplained lean is the thing being avoided.
    pub because: Vec<String>,
    /// What would change it.
    pub would_change_it: String,
}

impl Lean {
    /// The recommendation, plainly, with nothing else attached.
    ///
    /// One sentence. Not hedged, not prefaced, and not followed by the
    /// reasoning — a recommendation that arrives buried in its own working is
    /// one you have to dig for.
    pub fn spoken(&self) -> String {
        format!("{}.", self.option)
    }

    /// The recommendation with what it rests on, for when you ask.
    pub fn written(&self) -> String {
        let mut out = vec![self.spoken()];
        out.push(String::new());
        for b in &self.because {
            out.push(format!("- {b}"));
        }
        out.push(format!("What would change it: {}", self.would_change_it));
        out.join("\n")
    }

    /// Is there anything to give if asked?
    ///
    /// An unexplained lean is the thing this whole module exists to avoid, so
    /// this must never be false for a lean that was actually produced.
    pub fn can_explain(&self) -> bool {
        !self.because.is_empty()
    }
}

/// Why it will not lean yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CannotLean {
    /// A move has not been made.
    StillWorking(Move),
    /// Fewer than two options are still standing.
    NothingLeftToCompare,
    /// It turns on what you want, not on what is true.
    NeedsSomethingFromYou(String),
}

impl CannotLean {
    pub fn plain(&self) -> String {
        match self {
            CannotLean::StillWorking(m) => m.asks().to_string(),
            CannotLean::NothingLeftToCompare => {
                "Only one option is still standing, so there's nothing to weigh.".into()
            }
            CannotLean::NeedsSomethingFromYou(q) => q.clone(),
        }
    }
}

/// A decision being worked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub about: String,
    pub weight: Weight,
    /// The question as re-framed, if it changed.
    pub real_question: Option<String>,
    /// The boring answer, and what would beat it.
    pub boring: Option<(String, String)>,
    pub options: Vec<Option_>,
    /// The case against wherever it is leaning.
    pub against: Vec<String>,
    /// What breaks, when you find out, whether it can be undone.
    pub if_wrong: Option<String>,
    /// 1 to 10, and what is holding it down.
    pub confidence: Option<(u8, String)>,
    /// What you still have to answer yourself.
    pub open_questions: Vec<String>,
    /// Something only you can settle: a preference, a constraint, a threshold.
    ///
    /// Asked rather than assumed. Reasoning from a guessed preference produces
    /// an answer that is confidently wrong in a way you cannot see, because
    /// the guess never appears in the working.
    #[serde(default)]
    pub needs_from_you: Vec<String>,
}

impl Decision {
    pub fn new(about: &str, weight: Weight) -> Decision {
        Decision {
            about: about.to_string(),
            weight,
            real_question: None,
            boring: None,
            options: Vec::new(),
            against: Vec::new(),
            if_wrong: None,
            confidence: None,
            open_questions: Vec::new(),
            needs_from_you: Vec::new(),
        }
    }

    /// Options that are still candidates.
    pub fn standing(&self) -> Vec<&Option_> {
        self.options.iter().filter(|o| o.set_aside.is_none()).collect()
    }

    /// Set one aside, with the reason.
    pub fn set_aside(&mut self, what: &str, why: &str) {
        if let Some(o) = self.options.iter_mut().find(|o| o.what == what) {
            o.set_aside = Some(why.to_string());
        }
    }

    /// Recommend, if it can show its working.
    ///
    /// The conditions are the argument. Without costed options the
    /// cheap-looking one wins by not having been examined; without a case
    /// against, a lean is a preference wearing evidence; and where the answer
    /// turns on what you want, guessing is worse than asking.
    pub fn lean(&self) -> Result<Lean, CannotLean> {
        if let Some(q) = self.needs_from_you.first() {
            return Err(CannotLean::NeedsSomethingFromYou(q.clone()));
        }
        for m in [Move::Framing, Move::Options, Move::Evidence, Move::Reversal, Move::Stakes] {
            if self.gaps().contains(&m) {
                return Err(CannotLean::StillWorking(m));
            }
        }
        let standing = self.standing();
        if standing.len() < 2 {
            return Err(CannotLean::NothingLeftToCompare);
        }
        // Fewest unsupported assumptions, then cheapest to be wrong about.
        let best = standing
            .iter()
            .min_by_key(|o| (o.rests_on.len(), o.costs.len()))
            .expect("at least two standing");
        let mut because = vec![format!("it rests on the fewest unstated things: {}", best.rests_on.join("; "))];
        because.push(format!("what it costs you: {}", best.costs));
        if let Some(w) = &self.if_wrong {
            because.push(format!("if it's wrong: {w}"));
        }
        for o in self.options.iter().filter(|o| o.set_aside.is_some()) {
            because.push(format!("{} was set aside: {}", o.what, o.set_aside.clone().unwrap_or_default()));
        }
        Ok(Lean {
            option: best.what.clone(),
            because,
            would_change_it: self
                .against
                .first()
                .cloned()
                .unwrap_or_else(|| "nothing stated".into()),
        })
    }

    /// Is this worth working at all?
    ///
    /// Cheap and reversible needs no thought. Saying so is more use than a
    /// careful analysis of whether to buy the £4 thing.
    pub fn worth_working(&self) -> bool {
        self.weight != Weight::JustPick
    }

    /// The next move that has not been made.
    ///
    /// Returns `None` when the pass is complete. Order is fixed: skipping
    /// framing and going to options is how you get a well-compared answer to
    /// the wrong question.
    pub fn next_move(&self) -> Option<Move> {
        if !self.worth_working() {
            return None;
        }
        if self.real_question.is_none() {
            return Some(Move::Framing);
        }
        if self.boring.is_none() {
            return Some(Move::Default);
        }
        if self.options.len() < 2 {
            return Some(Move::Options);
        }
        if self.options.iter().any(|o| o.rests_on.is_empty()) {
            return Some(Move::Evidence);
        }
        if self.against.is_empty() {
            return Some(Move::Reversal);
        }
        if self.if_wrong.is_none() {
            return Some(Move::Stakes);
        }
        if self.confidence.is_none() {
            return Some(Move::Confidence);
        }
        if self.open_questions.is_empty() {
            return Some(Move::Ownership);
        }
        None
    }

    pub fn complete(&self) -> bool {
        self.next_move().is_none()
    }

    /// Everything laid out, with no winner named.
    ///
    /// The refusal to recommend is enforced here rather than left to whoever
    /// writes the summary. `never_names_a_winner` holds it in place.
    pub fn laid_out(&self) -> String {
        if !self.worth_working() {
            return format!(
                "{} is cheap and you can undo it. Pick one and move on — \
                 working this costs more than getting it wrong.",
                self.about
            );
        }
        let mut out = Vec::new();
        if let Some(q) = &self.real_question {
            if q != &self.about {
                out.push(format!("The question underneath: {q}"));
            }
        }
        if let Some((boring, beats)) = &self.boring {
            out.push(format!("The boring answer is {boring}. To beat it: {beats}"));
        }
        for o in &self.options {
            let rests = if o.rests_on.is_empty() {
                "nothing stated".to_string()
            } else {
                o.rests_on.join("; ")
            };
            out.push(format!("{} — costs you {}. Rests on: {}", o.what, o.costs, rests));
        }
        if !self.against.is_empty() {
            out.push(format!("Against: {}", self.against.join("; ")));
        }
        if let Some(w) = &self.if_wrong {
            out.push(format!("If it's wrong: {w}"));
        }
        if let Some((n, why)) = &self.confidence {
            out.push(format!("Confidence {n} out of 10, held down by {why}"));
        }
        if !self.open_questions.is_empty() {
            out.push(format!(
                "Before you decide: {}",
                self.open_questions.join("; ")
            ));
        }
        if self.weight == Weight::OneWay {
            out.push("This one can't be undone, so it's yours to make.".into());
        }
        out.join("\n")
    }

    /// What is missing, so a half-worked decision is not mistaken for a whole
    /// one.
    pub fn gaps(&self) -> Vec<Move> {
        if !self.worth_working() {
            return Vec::new();
        }
        let mut d = self.clone();
        let mut out = Vec::new();
        // Walk the moves, pretending each is filled, to find them all rather
        // than only the first.
        for m in Move::all() {
            if d.next_move() == Some(m) {
                out.push(m);
                match m {
                    Move::Framing => d.real_question = Some(String::new()),
                    Move::Default => d.boring = Some((String::new(), String::new())),
                    Move::Options => {
                        while d.options.len() < 2 {
                            d.options.push(Option_ {
                                what: String::new(),
                                costs: String::new(),
                                rests_on: vec![String::new()],
                                set_aside: None,
                            });
                        }
                    }
                    Move::Evidence => {
                        for o in d.options.iter_mut() {
                            if o.rests_on.is_empty() {
                                o.rests_on.push(String::new());
                            }
                        }
                    }
                    Move::Reversal => d.against.push(String::new()),
                    Move::Stakes => d.if_wrong = Some(String::new()),
                    Move::Confidence => d.confidence = Some((0, String::new())),
                    Move::Ownership => d.open_questions.push(String::new()),
                }
            }
        }
        out
    }
}

// ------------------------------------------------------------------ worked over turns (H9)
//
// Eric, 25 Sep 2026: "Atlas gives reasoning or different paths to the same
// outcome. Where that doesn't make sense, opinions are allowed: Atlas is
// supposed to be like a friend that does work for me and helps." So a lean is
// said when the working supports one, the paths are said when it doesn't, and
// the reasoning is always there to ask for.

impl Decision {
    /// Take your answer to a move and put it where that move keeps it.
    pub fn take_answer(&mut self, m: Move, answer: &str) {
        let a = answer.trim();
        let nothing = matches!(a.to_lowercase().as_str(), "no" | "none" | "nothing" | "same" | "no idea" | "dunno" | "not sure");
        match m {
            Move::Framing => self.real_question = Some(if nothing || a.is_empty() { self.about.clone() } else { a.to_string() }),
            Move::Default => self.boring = Some((if nothing { "doing nothing".into() } else { a.to_string() }, String::new())),
            Move::Options => {
                for part in split_options(a) {
                    if !self.options.iter().any(|o| o.what.eq_ignore_ascii_case(&part)) {
                        self.options.push(Option_ { what: part, costs: String::new(), rests_on: Vec::new(), set_aside: None });
                    }
                }
            }
            Move::Evidence => {
                if let Some(o) = self.options.iter_mut().find(|o| o.rests_on.is_empty()) {
                    o.rests_on = if nothing { vec!["nothing you could name".into()] } else {
                        a.split(';').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
                    };
                    if o.costs.is_empty() {
                        o.costs = "not said".into();
                    }
                }
            }
            Move::Reversal => self.against.push(if nothing { "nothing anyone could think of".into() } else { a.to_string() }),
            Move::Stakes => self.if_wrong = Some(a.to_string()),
            Move::Confidence => {
                let n: u8 = a.split(|c: char| !c.is_ascii_digit()).find_map(|w| w.parse().ok()).unwrap_or(5).clamp(1, 10);
                self.confidence = Some((n, a.to_string()));
            }
            Move::Ownership => self.open_questions.push(if nothing { "nothing left".into() } else { a.to_string() }),
        }
    }

    /// The next question, worded for this decision rather than generically.
    pub fn next_question(&self) -> Option<String> {
        let m = self.next_move()?;
        Some(match m {
            Move::Evidence => match self.options.iter().find(|o| o.rests_on.is_empty()) {
                Some(o) => format!("What is \"{}\" resting on — what has to be true for it to work?", o.what),
                None => m.asks().to_string(),
            },
            Move::Options if !self.options.is_empty() => {
                format!("Besides \"{}\", what else could you do?", self.options[0].what)
            }
            _ => m.asks().to_string(),
        })
    }

    /// The different paths to the same place, said.
    pub fn paths(&self) -> Option<String> {
        let standing = self.standing();
        if standing.len() < 2 {
            return None;
        }
        let each: Vec<String> = standing
            .iter()
            .map(|o| if o.costs.is_empty() || o.costs == "not said" { o.what.clone() } else { format!("{} (costs you {})", o.what, o.costs) })
            .collect();
        Some(format!("There are {} ways to get there: {}.", each.len(), each.join("; ")))
    }
}

/// "the contract or the retainer, or neither" → three options.
fn split_options(a: &str) -> Vec<String> {
    a.replace(", or ", ",").replace(" or ", ",").replace(" versus ", ",").replace(" vs ", ",")
        .split(',')
        .map(|x| x.trim().trim_start_matches("either ").trim_end_matches('.').to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// How much being wrong costs, guessed from the words, so a cheap choice
/// isn't worked for an evening and a one-way one is said to be one.
pub fn how_much_it_matters(about: &str) -> Weight {
    let l = about.to_lowercase();
    const ONE_WAY: &[&str] = &["quit", "resign", "sell the house", "sell my house", "divorce", "tattoo", "sign the", "delete my", "move country", "move abroad", "have surgery"];
    const CHEAP: &[&str] = &["which shirt", "what to eat", "for dinner", "for lunch", "which colour", "which color", "what to watch", "which phone case", "pizza or", "tea or coffee"];
    if ONE_WAY.iter().any(|w| l.contains(w)) {
        Weight::OneWay
    } else if CHEAP.iter().any(|w| l.contains(w)) {
        Weight::JustPick
    } else {
        Weight::WorthWorking
    }
}

/// How the local model is asked to work a decision in one go, in lines this
/// module reads back (`from_draft`).
pub const DRAFTER_PROMPT: &str = "You help someone work a decision, like a thoughtful friend. \
Reply ONLY in these lines, nothing else:\n\
QUESTION: the real question underneath\n\
DEFAULT: the boring default | what would have to be true to beat it\n\
OPTION: an option | what it costs | what it rests on; another thing it rests on\n\
(two to four OPTION lines)\n\
AGAINST: the strongest case against the option you'd lean to\n\
IF WRONG: what breaks, when they'd find out, whether it can be undone\n\
CONFIDENCE: a number 1-10 | what holds it down\n\
OPEN: something they still need to answer\n\
NEEDS: only if it turns on a preference only they can state, the question to ask them";

/// A decision from the model's lines. Lines it can't read are left out rather
/// than guessed at.
pub fn from_draft(about: &str, weight: Weight, text: &str) -> Decision {
    let mut d = Decision::new(about, weight);
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else { continue };
        let parts: Vec<String> = rest.split('|').map(|p| p.trim().to_string()).collect();
        let first = parts.first().cloned().unwrap_or_default();
        if first.is_empty() {
            continue;
        }
        match key.trim().to_uppercase().as_str() {
            "QUESTION" => d.real_question = Some(first),
            "DEFAULT" => d.boring = Some((first, parts.get(1).cloned().unwrap_or_default())),
            "OPTION" => d.options.push(Option_ {
                what: first,
                costs: parts.get(1).cloned().unwrap_or_else(|| "not said".into()),
                rests_on: parts.get(2).map(|r| r.split(';').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default(),
                set_aside: None,
            }),
            "AGAINST" => d.against.push(first),
            "IF WRONG" => d.if_wrong = Some(first),
            "CONFIDENCE" => {
                let n: u8 = first.split(|c: char| !c.is_ascii_digit()).find_map(|w| w.parse().ok()).unwrap_or(5).clamp(1, 10);
                d.confidence = Some((n, parts.get(1).cloned().unwrap_or_default()));
            }
            "OPEN" => d.open_questions.push(first),
            "NEEDS" => d.needs_from_you.push(first),
            _ => {}
        }
    }
    d
}

/// What Atlas says about a decision as it stands: a lean with its reason when
/// the working supports one, the paths and the next question when it
/// doesn't, and the thing only you can answer when it turns on that.
pub fn said(d: &Decision) -> String {
    if !d.worth_working() {
        return d.laid_out();
    }
    match d.lean() {
        Ok(l) => {
            let why = l.because.get(1).or(l.because.first()).cloned().unwrap_or_default();
            let mut s = format!("I'd lean toward {} — {why}.", l.option.trim_end_matches('.'));
            if d.weight == Weight::OneWay {
                s.push_str(" It can't be undone, so it's your call.");
            }
            s.push_str(" Want the whole working?");
            s
        }
        Err(CannotLean::NeedsSomethingFromYou(q)) => q,
        Err(CannotLean::NothingLeftToCompare) => match d.standing().first() {
            Some(o) => format!("Only \"{}\" is still standing, so that's the one.", o.what),
            None => "Everything's been set aside — what else could you do?".into(),
        },
        Err(CannotLean::StillWorking(_)) => {
            let q = d.next_question().unwrap_or_default();
            match d.paths() {
                Some(p) => format!("{p} {q}"),
                None => q,
            }
        }
    }
}

/// The openings of a question you are working through rather than one that
/// has a fact for an answer.
///
/// A decision usually arrives mid-sentence -- "should I take the contract or
/// keep the retainer", "help me decide whether to hire now" -- and the reply
/// that helps is not a pick but the first move of working it. These are the
/// shapes that mark such a question so the assistant can reach for
/// `Decision` instead of guessing an answer.
const DECIDING: &[&str] = &[
    "should i ", "should we ", "help me decide", "cant decide", "can't decide",
    "which should ", "is it worth ", "worth it", "better to ", "or should ",
    "torn between", "do i go with", "which one should",
];

/// Is this the kind of question that wants working rather than answering?
///
/// Deliberately narrow: it fires on an explicit deciding phrase, not on any
/// sentence with a choice in it, because the cost of a false positive is
/// answering an ordinary question with "what's the question underneath".
pub fn wants_working(text: &str) -> bool {
    let lower = text.to_lowercase();
    DECIDING.iter().any(|p| lower.contains(p))
}
