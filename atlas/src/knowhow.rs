//! Knowing how to do things without asking anyone.
//!
//! A model is not knowledge. It is a way of producing plausible text about
//! knowledge, and when it isn't there — or is a 1.5B running on a laptop —
//! Atlas needs something else to fall back on that isn't guessing.
//!
//! So Atlas ships with procedures. Not facts about the world, which go stale
//! and are what the internet is for, but **how to get things done**: the steps,
//! what has to be true first, what usually goes wrong, and what to do when it
//! does. That's the part that doesn't change, and it's why Atlas offline is
//! narrower than Atlas online rather than useless.
//!
//! Everything here is written once and improved by use: when a procedure fails
//! in a new way, the failure is added, so the same surprise happens once.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Procedure {
    pub id: String,
    /// What it achieves, in the words you'd use asking for it.
    pub goal: String,
    /// Other ways you might ask.
    pub also_called: Vec<String>,
    /// What has to be true before starting.
    pub needs: Vec<String>,
    pub steps: Vec<Step>,
    /// What usually goes wrong, and what to do.
    pub snags: Vec<Snag>,
    /// Does this work with no internet?
    pub offline: bool,
    /// How confident Atlas is that this still works.
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub do_this: String,
    /// How you know it worked.
    pub check: Option<String>,
    /// What to do when it didn't (`None`: nothing more than moving on).
    #[serde(default)]
    pub if_it_fails: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snag {
    /// What you'd see.
    pub looks_like: String,
    /// What's actually wrong.
    pub cause: String,
    pub fix: String,
}

/// The procedures Atlas ships with.
///
/// Deliberately about *its own world* — the machine, the files, the tools it
/// runs. General world knowledge belongs on the internet; this is the part
/// that has to work when the internet doesn't.
/// The words in a request that say what it's about: whole words, with the
/// ones every request has taken out.
pub fn content_words(t: &str) -> Vec<String> {
    const ORDINARY: &[&str] = &[
        "a", "an", "the", "and", "or", "of", "to", "in", "on", "for", "with", "at", "by", "from", "into", "about",
        "i", "me", "my", "you", "your", "it", "its", "it's", "is", "are", "was", "be", "do", "does", "did", "can",
        "could", "would", "should", "will", "how", "what", "what's", "whats", "who", "why", "when", "where", "which",
        "this", "that", "these", "those", "there", "here", "some", "something", "anything", "thing", "things",
        "make", "get", "got", "go", "want", "need", "help", "please", "just", "up", "out", "not", "isn't", "don't",
        "can't", "won't", "let", "tell", "show", "walk", "through", "steps", "step", "way", "best", "good", "new",
        "when", "then", "than", "so", "if", "but", "all", "any", "more", "have", "has", "had", "am", "we", "our",
    ];
    t.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|w| w.trim_matches('\'').to_string())
        .filter(|w| w.len() > 1 && !ORDINARY.contains(&w.as_str()))
        .collect()
}

pub fn shipped() -> Vec<Procedure> {
    vec![
        Procedure {
            id: "free-up-memory".into(),
            goal: "make room when memory is tight".into(),
            also_called: vec!["it's running slowly".into(), "out of memory".into(), "free up memory".into(), "freeing up memory".into()],
            needs: vec![],
            steps: vec![
                Step { do_this: "find what's holding memory and hasn't been used today".into(),
                    check: Some("something over 200MB and untouched".into()), if_it_fails: Some("nothing stands out: say memory is tight from many small things, and name the biggest three".into()) },
                Step { do_this: "close it, having said which".into(), check: None, if_it_fails: Some("it won't close: leave it and say so — never force-close something you might be using".into()) },
                Step { do_this: "if a model is loaded and idle, unload it".into(),
                    check: Some("memory pressure below 85%".into()), if_it_fails: Some("still above 85%: say what's using it rather than closing more".into()) },
            ],
            snags: vec![Snag {
                looks_like: "memory frees and immediately refills".into(),
                cause: "something is restarting itself".into(),
                fix: "check whether it starts with Windows rather than closing it again".into(),
            }],
            offline: true,
            confidence: 0.9,
        },
        Procedure {
            id: "cant-hear-you".into(),
            goal: "fix Atlas not hearing you".into(),
            also_called: vec!["microphone not working".into(), "you're not listening".into()],
            needs: vec![],
            steps: vec![
                Step { do_this: "check a microphone is found at all".into(),
                    check: Some("at least one device listed".into()), if_it_fails: Some("none found: say so and switch to typing".into()) },
                Step { do_this: "measure the level on each while you speak".into(),
                    check: Some("something above -40dB".into()), if_it_fails: Some("all silent: go to the last step".into()) },
                Step { do_this: "switch to the loudest one and say which".into(), check: None, if_it_fails: Some("it's still quiet: ask you to move closer or check the mute switch".into()) },
                Step { do_this: "if all are silent, fall back to typing and say so".into(), check: None, if_it_fails: Some("typing isn't open either: say how to open it".into()) },
            ],
            snags: vec![
                Snag {
                    looks_like: "a device is listed but always silent".into(),
                    cause: "another app has it exclusively, or it's muted in Windows".into(),
                    fix: "open the sound settings page — I don't change those myself".into(),
                },
                Snag {
                    looks_like: "it worked yesterday and not today".into(),
                    cause: "a headset connected or disconnected and the device names shifted".into(),
                    fix: "re-measure rather than trusting the saved name".into(),
                },
            ],
            offline: true,
            confidence: 0.85,
        },
        Procedure {
            id: "find-a-file".into(),
            goal: "find something you wrote but can't place".into(),
            also_called: vec!["where did I put".into(), "find my notes on".into()],
            needs: vec![],
            steps: vec![
                Step { do_this: "search the index by the words you remember".into(),
                    check: Some("anything above the floor".into()), if_it_fails: Some("nothing above the floor: go to the next step".into()) },
                Step { do_this: "if nothing, search by meaning if a model is there".into(), check: None, if_it_fails: Some("no model for meaning: say so and go to the next step".into()) },
                Step { do_this: "if still nothing, list what changed around when you think you wrote it".into(),
                    check: None, if_it_fails: Some("nothing changed then: ask what it was about in other words".into()) },
                Step { do_this: "quote the line so you can tell without opening it".into(), check: None, if_it_fails: Some("the file can't be read now: give its name and where it is".into()) },
            ],
            snags: vec![Snag {
                looks_like: "nothing found but you're sure it exists".into(),
                cause: "it's outside the indexed folders, or the index is stale".into(),
                fix: "say when it was last indexed and offer to re-run it".into(),
            }],
            offline: true,
            confidence: 0.9,
        },
        Procedure {
            id: "app-wont-start".into(),
            goal: "open something that isn't opening".into(),
            also_called: vec!["it won't launch".into(), "nothing happens".into()],
            needs: vec![],
            steps: vec![
                Step { do_this: "check the path in config still exists".into(), check: None, if_it_fails: Some("it's gone: ask where it lives now".into()) },
                Step { do_this: "if it's a Store app, use the AppID rather than a path".into(), check: None, if_it_fails: Some("no AppID found: say it's a Store app and the name to search in Start".into()) },
                Step { do_this: "check it isn't already running behind something".into(),
                    check: Some("a window with a matching title".into()), if_it_fails: Some("it's running but hidden: bring it forward instead of starting another".into()) },
                Step { do_this: "say which of those it was rather than trying again".into(), check: None, if_it_fails: Some("none of them: say it's something else, and don't try again blindly".into()) },
            ],
            snags: vec![Snag {
                looks_like: "it launches and closes immediately".into(),
                cause: "an updater stub, common with Discord and Chrome".into(),
                fix: "wait two seconds and look for the real window before calling it failed".into(),
            }],
            offline: true,
            confidence: 0.85,
        },
        Procedure {
            id: "disk-filling".into(),
            goal: "make room on the disk".into(),
            also_called: vec!["running out of space".into()],
            needs: vec![],
            steps: vec![
                Step { do_this: "find the largest disposable folders".into(), check: None, if_it_fails: Some("nothing disposable is big: say what is big, and don't touch it".into()) },
                Step { do_this: "check for a second drive with room".into(), check: None, if_it_fails: Some("no second drive: say how much would need clearing instead".into()) },
                Step { do_this: "offer to move models and captures there — large, static, re-downloadable".into(),
                    check: None, if_it_fails: Some("you say no: leave them and say what else could go".into()) },
                Step { do_this: "never move notes, state or backups off the fast drive".into(), check: None, if_it_fails: None },
            ],
            snags: vec![Snag {
                looks_like: "space frees and disappears again".into(),
                cause: "a log or a cache is growing".into(),
                fix: "find what grew rather than clearing the same folder twice".into(),
            }],
            offline: true,
            confidence: 0.8,
        },
        Procedure {
            id: "understand-a-document".into(),
            goal: "make sense of a document you're looking at".into(),
            also_called: vec!["what does this say".into(), "summarise this".into()],
            needs: vec![],
            steps: vec![
                Step { do_this: "work out which document is meant".into(),
                    check: Some("one clear candidate, or ask".into()), if_it_fails: Some("more than one fits: ask which".into()) },
                Step { do_this: "read its text — from the file if possible, the window if not".into(), check: None, if_it_fails: Some("neither can be read: say why".into()) },
                Step { do_this: "if it's a scan, read the text off the image".into(), check: None, if_it_fails: Some("the text reader isn't installed: say it needs the text reader, which setup can fetch".into()) },
                Step { do_this: "without a model, give structure: headings, numbers, dates, names".into(),
                    check: None, if_it_fails: None },
            ],
            snags: vec![Snag {
                looks_like: "the text comes out as gibberish".into(),
                cause: "it's a scanned image, or the encoding is wrong".into(),
                fix: "say which rather than reading nonsense aloud".into(),
            }],
            offline: true,
            confidence: 0.75,
        },
        Procedure {
            id: "research-something".into(),
            goal: "find out about something Atlas doesn't know".into(),
            also_called: vec!["look into".into(), "what is".into()],
            needs: vec!["the internet".into()],
            steps: vec![
                Step { do_this: "check what's already indexed first — it may be there".into(), check: None, if_it_fails: None },
                Step { do_this: "search, preferring the original source over anything summarising it".into(),
                    check: None, if_it_fails: Some("no connection: say so and fall back to what's indexed".into()) },
                Step { do_this: "read two sources, not one".into(),
                    check: Some("they agree, or the disagreement is named".into()), if_it_fails: Some("only one source exists: say it's one source".into()) },
                Step { do_this: "say where it came from and when".into(), check: None, if_it_fails: None },
            ],
            snags: vec![Snag {
                looks_like: "everything found says the same thing in the same words".into(),
                cause: "they're all copying one source".into(),
                fix: "find the original and cite that instead of the copies".into(),
            }],
            offline: false,
            confidence: 0.85,
        },
    ]
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Knowhow {
    pub procedures: Vec<Procedure>,
}

/// Where snags learned by use are kept.
pub const LEARNED: &str = "knowhow_learned";

impl Knowhow {
    pub fn shipped() -> Knowhow {
        Knowhow { procedures: shipped() }
    }

    /// The shipped procedures plus every snag learned since. "Improved by
    /// use" used to be untrue: `learn_snag` had no caller and nothing it
    /// learned was kept.
    pub fn load(store: &crate::store::Store) -> Knowhow {
        let mut k = Knowhow::shipped();
        let learned: Vec<(String, Snag)> = store.load(LEARNED);
        for (id, snag) in learned {
            k.learn_snag(&id, &snag.looks_like, &snag.cause, &snag.fix);
        }
        k
    }

    /// Learn a snag and keep it.
    pub fn learn_and_keep(store: &crate::store::Store, procedure_id: &str, snag: Snag) -> bool {
        let mut learned: Vec<(String, Snag)> = store.load(LEARNED);
        if learned.iter().any(|(id, s)| id == procedure_id && s.looks_like == snag.looks_like) {
            return false;
        }
        learned.push((procedure_id.to_string(), snag));
        store.save(LEARNED, &learned).is_ok()
    }

    /// Find something that fits what was asked.
    ///
    /// Whole words only, and the ordinary ones don't count. It used to take
    /// any 4-letter-plus word appearing anywhere in a goal, as a substring:
    /// "how do I make pancakes" shared "make" with "make sense of a document
    /// you're looking at", so a friend asking about pancakes was read Atlas's
    /// own procedure for reading documents, steps and all — and "what is
    /// photosynthesis" matched research's "what is" (26 Sep 2026). Now a
    /// procedure fits when one of its own phrases is in the request (a phrase
    /// with a real word in it, not "what is"), or when at least two of the
    /// request's real words are in its goal — one if the request only has one.
    pub fn for_request(&self, request: &str, online: bool) -> Option<&Procedure> {
        let t = request.to_lowercase();
        let asked = content_words(&t);
        self.procedures
            .iter()
            .filter(|p| online || p.offline)
            .filter_map(|p| {
                let phrase = p.also_called.iter().any(|a| {
                    let a = a.to_lowercase();
                    !content_words(&a).is_empty() && t.contains(&a)
                });
                let goal: Vec<String> =
                    content_words(&format!("{} {}", p.goal, p.also_called.join(" ")).to_lowercase());
                let hits = asked.iter().filter(|w| goal.contains(w)).count();
                let enough = !asked.is_empty() && hits >= asked.len().min(2);
                (phrase || enough).then_some((p, hits + phrase as usize * 5))
            })
            .max_by_key(|(_, score)| *score)
            .map(|(p, _)| p)
    }

    /// What to do about a symptom, without knowing the cause.
    ///
    /// Scored rather than first-match. One word in common is nothing —
    /// "immediately" appears in half of these — so the best overlap wins and a
    /// single shared word isn't enough to claim a match.
    pub fn for_symptom(&self, seen: &str) -> Option<(&Procedure, &Snag)> {
        let t = seen.to_lowercase();
        let mut best: Option<(&Procedure, &Snag, usize)> = None;

        for p in &self.procedures {
            for s in &p.snags {
                let l = s.looks_like.to_lowercase();
                let words: Vec<&str> = l.split_whitespace().filter(|w| w.len() > 4).collect();
                if words.is_empty() {
                    continue;
                }
                let hits = words.iter().filter(|w| t.contains(**w)).count();
                if hits < 2 && words.len() > 2 {
                    continue;
                }
                if hits == 0 {
                    continue;
                }
                if best.map(|(_, _, b)| hits > b).unwrap_or(true) {
                    best = Some((p, s, hits));
                }
            }
        }
        best.map(|(p, s, _)| (p, s))
    }

    /// Add what was learned, so the same surprise happens once.
    pub fn learn_snag(&mut self, procedure_id: &str, looks_like: &str, cause: &str, fix: &str) -> bool {
        match self.procedures.iter_mut().find(|p| p.id == procedure_id) {
            Some(p) => {
                if p.snags.iter().any(|s| s.looks_like == looks_like) {
                    return false;
                }
                p.snags.push(Snag {
                    looks_like: looks_like.into(),
                    cause: cause.into(),
                    fix: fix.into(),
                });
                true
            }
            None => false,
        }
    }

    /// What Atlas can still do with no internet at all.
    pub fn offline_coverage(&self) -> (usize, usize) {
        (self.procedures.iter().filter(|p| p.offline).count(), self.procedures.len())
    }
}

/// What Atlas says when it's following a known procedure rather than guessing.
///
/// Saying so matters: "I know how to do this" and "I'm working it out" deserve
/// different amounts of your attention.
pub fn announce(p: &Procedure, online: bool) -> String {
    let mut s = format!("{} — I know this one, {} steps.", p.goal, p.steps.len());
    if !p.offline && !online {
        s = format!("{} needs the internet, and there isn't any. Here's what I'd do offline instead.", p.goal);
    }
    s
}

/// Turned into something to actually follow: each step with how you know it
/// worked and what to do if it didn't — the runbook shape from wshobson's
/// `incident-response` (prerequisite, expected result, "if this fails").
pub fn as_plan(p: &Procedure) -> Vec<String> {
    p.steps
        .iter()
        .map(|s| {
            let mut line = match &s.check {
                Some(c) => format!("{} (until {c})", s.do_this),
                None => s.do_this.clone(),
            };
            if let Some(f) = &s.if_it_fails {
                line.push_str(&format!(" — if not: {f}"));
            }
            line
        })
        .collect()
}

/// The quick version, before the steps: what has to be true, and the steps
/// in a few words each.
pub fn checklist(p: &Procedure) -> String {
    let mut s = String::new();
    if !p.needs.is_empty() {
        s.push_str(&format!("Needs: {}. ", p.needs.join(", ")));
    }
    let short: Vec<String> = p
        .steps
        .iter()
        .map(|st| st.do_this.split([',', '—', '(']).next().unwrap_or(&st.do_this).trim().to_string())
        .collect();
    s.push_str(&format!("In short: {}.", short.join(" → ")));
    s
}

/// A blameless look back after a procedure didn't work, filed as a note.
/// From `incident-response`: what happened, what was expected, why — asked
/// down to a cause that can be changed, never to who — and what changes.
/// Atlas can fill in the first two and whatever it already knows of the why;
/// the rest is written as questions, because a guessed cause is worse than
/// an open one.
pub fn look_back(p: &Procedure, what_happened: &str, known_cause: Option<&str>) -> String {
    let mut s = format!("# Look back: {}\n\n", p.goal);
    s.push_str(&format!("**What happened:** {what_happened}\n\n"));
    s.push_str(&format!("**What should have happened:** {}.\n\n", as_plan(p).join("; ")));
    s.push_str("**Why** (no blame — each answer asks why again, until it reaches something that can be changed):\n\n");
    match known_cause {
        Some(c) => s.push_str(&format!("1. {c}\n2. Why did that happen? (not known yet)\n")),
        None => s.push_str("1. Why did it happen? (not known yet)\n"),
    }
    s.push_str("\n**What changes:** the symptom is now a known snag for this procedure, so the same surprise is recognised next time.\n");
    s
}
