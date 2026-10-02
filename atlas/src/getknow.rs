//! "Get to know me": one guided conversation that fills Atlas with your life
//! (1 Oct 2026, the "why Atlas feels stale" report, phase 1).
//!
//! Atlas's own records on the laptop showed every store about you empty --
//! no saved facts, no projects, no traits -- and an assistant with nothing
//! to work from has nothing new to say, so it talks about itself. This asks
//! six short questions, one at a time, and keeps each answer as something
//! you said (`facts::Kind::You`, `Project`, `Instruction`, `Reference` --
//! never `Noticed`): who to call you, what you're working on, your day,
//! what to push you on, which folders matter, and what to connect. At the
//! end it reads back what it kept, so you can correct it.
//!
//! "Skip" moves on; "stop" (or "that's enough", "later") ends it, keeping
//! what was said so far. Nothing is guessed: an answer is kept in your own
//! words.

use crate::facts::{Fact, Kind};

/// One question, and what its answer becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Name,
    Work,
    Day,
    Push,
    Folders,
    Connect,
}

pub const QUESTIONS: &[(Slot, &str)] = &[
    (Slot::Name, "First, what should I call you?"),
    (Slot::Work, "What are you working on right now -- your businesses, projects, anything you want done this month?"),
    (Slot::Day, "What does your day look like -- when do you usually work, film, and switch off?"),
    (Slot::Push, "What should I push you on? Habits, posting, deadlines -- whatever you want me to keep you honest about."),
    (Slot::Folders, "Which folders hold your work -- Desktop, Documents, Dropbox, somewhere else?"),
    (Slot::Connect, "Last one: want to connect your email and calendar so I can keep track of them? Yes or no."),
];

/// A conversation in progress.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Interview {
    /// The question asked last (an index into `QUESTIONS`).
    pub at: usize,
    /// What was kept, one line each, for the read-back.
    pub kept: Vec<String>,
}

/// "Get to know me", "let's get to know each other", "interview me".
pub fn asked_to_start(said: &str) -> bool {
    let t = said.trim().trim_end_matches(['.', '!', '?']).to_lowercase();
    [
        "get to know me", "let's get to know each other", "lets get to know each other", "get to know each other",
        "interview me", "learn about me", "ask me about myself", "find out about me", "set yourself up for me",
    ]
    .iter()
    .any(|p| t == *p || t.ends_with(p) || t.starts_with(p))
}

fn skipped(said: &str) -> bool {
    let t = said.trim().trim_end_matches(['.', '!']).to_lowercase();
    matches!(t.as_str(), "skip" | "skip it" | "skip that" | "pass" | "next" | "next one" | "don't know" | "dont know" | "not sure" | "no idea")
}

fn stopped(said: &str) -> bool {
    let t = said.trim().trim_end_matches(['.', '!']).to_lowercase();
    matches!(
        t.as_str(),
        "stop" | "that's enough" | "thats enough" | "later" | "let's stop" | "lets stop" | "finish" | "done" | "never mind" | "nevermind" | "enough"
    )
}

/// The pieces of a list said in one breath: "the bakery, my
/// YouTube channel and Atlas" gives three.
fn pieces(answer: &str) -> Vec<String> {
    answer
        .split([',', ';', '\n'])
        .flat_map(|p| p.split(" and "))
        .map(|p| p.trim().trim_start_matches("and ").trim_end_matches(['.', '!']).trim().to_string())
        .filter(|p| p.split_whitespace().count() >= 1 && p.len() > 1)
        .collect()
}

/// What an answer becomes: the facts to keep, and the lines to read back.
pub fn facts_from(slot: Slot, answer: &str, now: u64) -> Vec<(Fact, String)> {
    let a = answer.trim().trim_end_matches(['.', '!']).trim();
    if a.is_empty() {
        return Vec::new();
    }
    match slot {
        Slot::Name => {
            let low = a.to_lowercase();
            let name = ["call me ", "it's ", "its ", "i'm ", "im ", "my name is ", "just "]
                .iter()
                .find_map(|p| low.strip_prefix(p).map(|r| a[a.len() - r.len()..].to_string()))
                .unwrap_or_else(|| a.to_string());
            let name = name.trim().to_string();
            vec![(Fact::new("what to call them", &format!("Call them {name}"), &format!("They asked to be called {name}."), Kind::Instruction, now), format!("I'll call you {name}."))]
        }
        Slot::Work => pieces(a)
            .into_iter()
            .map(|p| {
                let line = format!("Working on: {p}");
                (Fact::new(&format!("project {p}"), &line, &line, Kind::Project, now), format!("You're working on {p}."))
            })
            .collect(),
        Slot::Day => {
            let line = format!("Their day: {a}");
            vec![(Fact::new("their day", &line, &line, Kind::You, now), format!("Your day: {a}."))]
        }
        Slot::Push => {
            let line = format!("Push them on: {a}");
            vec![(Fact::new("push them on", &line, &line, Kind::Instruction, now), format!("I'll push you on {a}."))]
        }
        Slot::Folders => {
            let line = format!("Their work is in: {a}");
            vec![(Fact::new("where their work is", &line, &line, Kind::Reference, now), format!("Your work is in {a}."))]
        }
        Slot::Connect => Vec::new(),
    }
}

/// Does this answer say yes?
fn said_yes(answer: &str) -> bool {
    let t = answer.trim().to_lowercase();
    ["yes", "yeah", "yep", "sure", "ok", "okay", "please", "do it", "go ahead"].iter().any(|w| t == *w || t.starts_with(&format!("{w} ")) || t.starts_with(&format!("{w},")))
}

/// What happens to one answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Keep these, then say this (the next question, or the read-back).
    Ask { keep: Vec<Fact>, say: String },
    /// Finished: keep these and say the read-back.
    Done { keep: Vec<Fact>, say: String },
}

impl Interview {
    /// The opening: what this is, and the first question.
    pub fn begin() -> (Interview, String) {
        (
            Interview::default(),
            format!(
                "Six quick questions so I know what you're working on -- skip any you like, or tell me to stop. {}",
                QUESTIONS[0].1
            ),
        )
    }

    /// Take the answer to the question asked last.
    pub fn answer(&mut self, said: &str, now: u64) -> Next {
        if stopped(said) {
            return Next::Done { keep: Vec::new(), say: self.read_back(true) };
        }
        let (slot, _) = QUESTIONS[self.at];
        let mut keep = Vec::new();
        let mut extra = String::new();
        if !skipped(said) {
            for (f, line) in facts_from(slot, said, now) {
                keep.push(f);
                self.kept.push(line);
            }
            if slot == Slot::Connect {
                extra = if said_yes(said) {
                    " For email: an Outlook address I can connect when you ask me to connect Outlook; any other account goes in on the hub's Connections page. Your calendar comes across from the phone app once it's paired.".into()
                } else {
                    " No problem -- the hub's Connections page has it whenever you want.".into()
                };
            }
        }
        self.at += 1;
        if self.at >= QUESTIONS.len() {
            return Next::Done { keep, say: format!("{}{extra}", self.read_back(false)) };
        }
        Next::Ask { keep, say: QUESTIONS[self.at].1.to_string() }
    }

    /// What was kept, said back so it can be corrected.
    fn read_back(&self, early: bool) -> String {
        if self.kept.is_empty() {
            return if early { "Stopped -- nothing kept. Ask me to get to know you whenever you like.".into() } else { "Done -- you skipped them all, so nothing's kept.".into() };
        }
        format!(
            "{}Here's what I kept: {} If any of that's wrong, just tell me what's right.",
            if early { "Stopped there. " } else { "That's everything. " },
            self.kept.join(" ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_said_in_one_breath_is_several_projects() {
        assert_eq!(pieces("the bakery, my YouTube channel and Atlas"), vec!["the bakery", "my YouTube channel", "Atlas"]);
    }

    #[test]
    fn a_name_is_taken_from_how_it_was_said() {
        let f = facts_from(Slot::Name, "Call me Eric.", 0);
        assert_eq!(f[0].1, "I'll call you Eric.");
    }
}
