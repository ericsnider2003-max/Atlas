//! "Get to know me": one guided conversation that fills Atlas with your life
//! (1 Oct 2026, the "why Atlas feels stale" report, phase 1).
//!
//! Atlas's own records on the laptop showed every store about you empty --
//! no saved facts, no projects, no traits -- and an assistant with nothing
//! to work from has nothing new to say, so it talks about itself. This asks
//! seven short questions, one at a time, and keeps each answer as something
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
    Money,
    Connect,
}

pub const QUESTIONS: &[(Slot, &str)] = &[
    (Slot::Name, "First, what should I call you?"),
    (Slot::Work, "What are you working on right now -- your businesses, projects, anything you want done this month?"),
    (Slot::Day, "What does your day look like -- when do you usually work, film, and switch off?"),
    (Slot::Push, "What should I push you on? Habits, posting, deadlines -- whatever you want me to keep you honest about."),
    (Slot::Folders, "Which folders hold your work -- Desktop, Documents, Dropbox, somewhere else?"),
    (Slot::Money, "What kinds of extra-money work should I look out for -- a few words each, like T-shirt design or video editing?"),
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

/// Your words turned round to be said back to you: "I need you to push me
/// on my habits" becomes "you need me to push you on your habits". On
/// 1 Oct 2026 the read-back said "I'll push you on I need you to push me on
/// my habits" -- your sentence, pasted in unturned.
pub fn said_back(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut after_you = false;
    for w in text.split_whitespace() {
        let core: String = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_string();
        let low = core.to_lowercase();
        let swap = match low.as_str() {
            "i" => Some("you"),
            "i'm" => Some("you're"),
            "i've" => Some("you've"),
            "i'll" => Some("you'll"),
            "i'd" => Some("you'd"),
            "me" => Some("you"),
            "my" => Some("your"),
            "mine" => Some("yours"),
            "myself" => Some("yourself"),
            "you" => Some("me"),
            "your" => Some("my"),
            "yours" => Some("mine"),
            "yourself" => Some("myself"),
            "you're" => Some("I'm"),
            "you'll" => Some("I'll"),
            "am" if after_you => Some("are"),
            "was" if after_you => Some("were"),
            _ => None,
        };
        after_you = matches!(low.as_str(), "i");
        match swap {
            Some(r) if !core.is_empty() => {
                let r = if core.chars().next().is_some_and(|c| c.is_uppercase()) && low != "i" && out.is_empty() {
                    let mut c = r.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                } else {
                    r.to_string()
                };
                out.push(w.replacen(core.as_str(), &r, 1));
            }
            _ => out.push(w.to_string()),
        }
    }
    out.join(" ")
}

/// At most `n` words, with an ellipsis when cut -- a read-back is a check,
/// not a recital.
fn clipped(text: &str, n: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= n {
        return text.trim().trim_end_matches(['.', '!']).to_string();
    }
    format!("{}...", words[..n].join(" ").trim_end_matches([',', '.', ';']))
}

/// Is this a short list of names ("the bakery, my channel and Atlas"),
/// rather than sentences about them?
fn a_short_list(answer: &str) -> bool {
    let sentences = answer.matches(['.', '!', '?']).count();
    let ps = pieces(answer);
    sentences <= 1 && !ps.is_empty() && ps.iter().all(|p| p.split_whitespace().count() <= 5)
}

/// What you'd say before the thing to push you on: "I need you to push me
/// on my habits" is about "my habits".
pub fn without_push_lead(answer: &str) -> String {
    let a = answer.trim();
    let low = a.to_ascii_lowercase();
    const LEADS: &[&str] = &[
        "i need you to push me on ", "i want you to push me on ", "i'd like you to push me on ", "push me on ",
        "keep me honest about ", "keep me honest on ", "you can push me on ", "i need pushing on ",
    ];
    LEADS.iter().find_map(|l| low.strip_prefix(l).map(|_| a[l.len()..].to_string())).unwrap_or_else(|| a.to_string())
}

/// What an answer becomes: the facts to keep, and the lines to read back.
pub fn facts_from(slot: Slot, answer: &str, now: u64) -> Vec<(Fact, String)> {
    let a = answer.trim().trim_end_matches(['.', '!']).trim();
    if a.is_empty() {
        return Vec::new();
    }
    match slot {
        Slot::Name => {
            let low = a.to_ascii_lowercase();
            let name = ["call me ", "it's ", "its ", "i'm ", "im ", "my name is ", "just "]
                .iter()
                .find_map(|p| low.strip_prefix(p).map(|r| a[a.len() - r.len()..].to_string()))
                .unwrap_or_else(|| a.to_string());
            let name = name.trim().to_string();
            vec![(Fact::new("what to call them", &format!("Call them {name}"), &format!("They asked to be called {name}."), Kind::Instruction, now), format!("I'll call you {name}."))]
        }
        // A short list is several projects; sentences about your work are
        // kept whole, in your words, as one (1 Oct 2026: a paragraph was cut
        // at every comma and "and", each piece read back as a project).
        Slot::Work if a_short_list(a) => pieces(a)
            .into_iter()
            .map(|p| {
                let line = format!("Working on: {p}");
                (Fact::new(&format!("project {p}"), &line, &line, Kind::Project, now), format!("You're working on {}.", said_back(&p)))
            })
            .collect(),
        Slot::Work => {
            let line = format!("What they're working on: {a}");
            vec![(Fact::new("what they're working on", &line, &line, Kind::Project, now), format!("Your work: {}.", clipped(&said_back(a), 30)))]
        }
        Slot::Day => {
            let line = format!("Their day: {a}");
            vec![(Fact::new("their day", &line, &line, Kind::You, now), format!("Your day: {}.", clipped(&said_back(a), 25)))]
        }
        Slot::Push => {
            let what = without_push_lead(a);
            let line = format!("Push them on: {what}");
            vec![(Fact::new("push them on", &line, &line, Kind::Instruction, now), format!("I'll push you on {}.", clipped(&said_back(&what), 25)))]
        }
        Slot::Folders => {
            let line = format!("Their work is in: {a}");
            vec![(Fact::new("where their work is", &line, &line, Kind::Reference, now), format!("Where your work is: {}.", clipped(&said_back(a), 25)))]
        }
        // What to hunt for (why-stale idea 8): the short pieces, as the
        // opportunity hunt's own two lists, so "look for" and "my skills
        // are" start from what you said here.
        Slot::Money => {
            let items: Vec<String> = pieces(&without_money_lead(a))
                .into_iter()
                .filter(|p| p.split_whitespace().count() <= 6)
                .map(|p| p.to_lowercase())
                .collect();
            if items.is_empty() {
                return Vec::new();
            }
            let line = format!("I'll look out for {}.", items.join(", "));
            vec![
                (crate::hunt::Interests::fact(crate::hunt::FACT_WANT, &items, now), line),
                (crate::hunt::Interests::fact(crate::hunt::FACT_SKILLS, &items, now), String::new()),
            ]
        }
        Slot::Connect => Vec::new(),
    }
}

/// "I'm good at T-shirt design and video editing" is about the last part.
fn without_money_lead(answer: &str) -> String {
    let a = answer.trim();
    let low = a.to_ascii_lowercase();
    const LEADS: &[&str] = &["i'm good at ", "im good at ", "i am good at ", "look for ", "look out for ", "anything in ", "things like ", "stuff like "];
    LEADS.iter().find_map(|l| low.strip_prefix(l).map(|_| a[l.len()..].to_string())).unwrap_or_else(|| a.to_string())
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
                "Seven quick questions so I know what you're working on -- skip any you like, or tell me to stop. {}",
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
                if !line.is_empty() {
                    self.kept.push(line);
                }
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
    fn a_paragraph_about_your_work_is_one_thing_not_twelve() {
        let a = "I have two projects. The bakery which is opening in November. Atlas (you). I want to make the bakery profitable";
        let f = facts_from(Slot::Work, a, 0);
        assert_eq!(f.len(), 1);
        assert!(f[0].1.starts_with("Your work: you have two projects."), "{}", f[0].1);
        assert!(f[0].1.contains("Atlas (me)"), "{}", f[0].1);
    }

    #[test]
    fn a_name_is_taken_from_how_it_was_said() {
        let f = facts_from(Slot::Name, "Call me Eric.", 0);
        assert_eq!(f[0].1, "I'll call you Eric.");
    }
}
