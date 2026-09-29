//! Saying how it's going, and saying how it went.
//!
//! Atlas has one voice. A long job either narrates into silence or says
//! nothing until it finishes, and both are bad in the same way: you cannot
//! tell a system that is working from one that has stopped.
//!
//! Two channels fix it, and the split has one rule that matters. **Progress is
//! disposable; the result is not.** You may miss every update — asleep, out of
//! the room, the speaker off — so the final word has to stand on its own for
//! someone who heard none of it. A result that says "as I mentioned" is a
//! result that fails for the person who most needed it.
//!
//! That rule is enforced here rather than trusted, because it is the one that
//! decays first: it is always tempting to lean on something already said.
//!
//! Ordering is `faithful`'s job — anything that went wrong leads. This decides
//! what may be said where.

use serde::{Deserialize, Serialize};

/// Where a line is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    /// While the work is happening. Missable by design.
    Progress,
    /// What you are left with. Must stand alone.
    Result,
}

/// A way a final word leans on something you may not have heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leak {
    /// Refers back to an update rather than repeating what it said.
    RefersToAnUpdate(String),
    /// Says a number or name that only appeared in progress.
    OnlyMentionedInProgress(String),
    /// Reads as a continuation rather than a beginning.
    StartsMidThought(String),
}

impl Leak {
    pub fn plain(&self) -> String {
        match self {
            Leak::RefersToAnUpdate(p) => {
                format!("\"{p}\" points at an update you may not have heard")
            }
            Leak::OnlyMentionedInProgress(w) => {
                format!("\"{w}\" was only ever said while the work was running")
            }
            Leak::StartsMidThought(p) => {
                format!("opening with \"{p}\" reads as the middle of something")
            }
        }
    }
}

/// Phrases that hand the listener back to an earlier update.
const POINTS_BACK: &[&str] = &[
    "as i mentioned", "as i said", "as noted", "as above", "like i said",
    "mentioned earlier", "said earlier", "noted earlier", "from before",
    "the one i mentioned", "that i found", "as discussed", "per the update",
    "which i said", "i told you", "referred to above", "see above",
];

/// Openers that only make sense following something.
const MID_THOUGHT: &[&str] = &[
    "so ", "then ", "and ", "but ", "also ", "anyway ", "however ",
    "that done", "after that", "next ", "finally ", "meanwhile ", "otherwise ",
];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

/// One run: what was said along the way, and what is being said at the end.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Run {
    pub updates: Vec<String>,
}

impl Run {
    /// Say something while working. Nothing is checked — progress is allowed
    /// to be partial, chatty, and wrong-in-hindsight. That is what it is for.
    pub fn update(&mut self, line: &str) {
        self.updates.push(line.to_string());
    }

    /// Everything named in progress that a listener would not otherwise have.
    ///
    /// Numbers and capitalised names only. Ordinary words recur everywhere and
    /// flagging them would make the check noise, which is how a check gets
    /// switched off.
    fn only_said_while_working(&self) -> Vec<String> {
        let mut out = Vec::new();
        for u in &self.updates {
            // Sentence position matters. The first word of a sentence is
            // capitalised whatever it is, so a capital there carries no
            // information — treating "Having" and "Found" as names is how this
            // check becomes noise.
            let mut at_sentence_start = true;
            for w in u.split(|c: char| c.is_whitespace()) {
                let ends_sentence = w.ends_with('.') || w.ends_with('!') || w.ends_with('?');
                let bare = w.trim_matches(|c: char| !c.is_alphanumeric());
                let first = at_sentence_start;
                at_sentence_start = ends_sentence;
                if bare.len() < 2 {
                    continue;
                }
                let has_digit = bare.chars().any(|c| c.is_ascii_digit());
                let looks_like_a_name = !first
                    && bare.chars().next().is_some_and(|c| c.is_uppercase())
                    && bare.chars().skip(1).any(|c| c.is_lowercase());
                if (has_digit || looks_like_a_name) && !out.contains(&bare.to_string()) {
                    out.push(bare.to_string());
                }
            }
        }
        out
    }

    /// Does this final word stand on its own?
    ///
    /// Empty means yes. The listener heard nothing and still knows where they
    /// are.
    pub fn check_result(&self, result: &str) -> Vec<Leak> {
        let mut out = Vec::new();
        let lower = result.to_lowercase();

        for p in POINTS_BACK {
            if lower.contains(p) {
                out.push(Leak::RefersToAnUpdate((*p).to_string()));
            }
        }

        let trimmed = lower.trim_start();
        for p in MID_THOUGHT {
            if trimmed.starts_with(p) {
                out.push(Leak::StartsMidThought(p.trim().to_string()));
                break;
            }
        }

        let carried = words(result);
        for token in self.only_said_while_working() {
            let t = token.to_lowercase();
            // A thing worth knowing that the result never repeats.
            if !carried.contains(&t) && !out.iter().any(|l| matches!(l, Leak::OnlyMentionedInProgress(x) if *x == token)) {
                out.push(Leak::OnlyMentionedInProgress(token));
            }
        }
        out
    }

    /// What to say at the end, given the steps and a draft.
    ///
    /// Runs the draft through `faithful` first, so anything that went wrong
    /// leads, then reports what the result still leans on. Two separate
    /// questions: whether it is true, and whether it can be heard on its own.
    pub fn close(
        &self,
        steps: &[crate::faithful::Step],
        draft: &str,
    ) -> (String, Vec<Leak>) {
        let ordered = crate::faithful::lead_with_the_problem(steps, draft);
        let leaks = self.check_result(&ordered);
        (ordered, leaks)
    }
}
