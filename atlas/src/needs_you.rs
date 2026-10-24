//! What needs you, now, and why (Plan A, 24 Oct 2026: the chief-of-staff
//! attention loop, as a proof slice).
//!
//! Atlas already has the pieces of a chief of staff -- the one work record
//! (`hub::Open`), the calendar, the interrupt gate's budget and mutes, the
//! approval rules in `policy`. What it didn't have: one short list, in the
//! order that matters, where every item says *why now*, and that stays quiet
//! about what you've already put off. "What needs me?" answers from here;
//! "not now" puts those off for a few hours, and they don't come back early.
//!
//! This module only reads and ranks. It never sends, pays, signs, deletes or
//! changes anything but its own snooze list and (24 Oct 2026) the memory of
//! what a timed check-in has already said -- `tests/what_needs_you.rs`
//! holds that.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The most said at once: a chief of staff hands you three things, not a list.
pub const CAP: usize = 3;
/// How far ahead an event counts as "soon".
pub const SOON_SECS: u64 = 60 * 60;
/// How long "not now" puts something off.
pub const SNOOZE_SECS: u64 = 4 * 60 * 60;
/// The saved snoozes, and what was last said (what "not now" refers to).
pub const SNOOZED_KEY: &str = "needs_you_snoozed";
pub const LAST_KEY: &str = "needs_you_last";
/// What a check-in has already said, with when (24 Oct 2026).
pub const SURFACED_KEY: &str = "needs_you_surfaced";
/// The least time between two spoken check-ins.
pub const CHECK_IN_EVERY_SECS: u64 = 30 * 60;
/// How long something said stays remembered: a restart, or a week, doesn't
/// make the same item new again, but a file never grows without end.
pub const SURFACED_KEEP_SECS: u64 = 7 * 24 * 60 * 60;
/// Most entries kept, newest first.
pub const SURFACED_MAX: usize = 200;

/// Why something is on the list, which is also its place in the order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Why {
    /// On your calendar within the hour: time decides, so it goes first.
    StartsSoon,
    /// Waiting on your answer or your yes.
    YourMove,
    /// Something stopped, failed or was cut off, and needs a look.
    Stopped,
}

/// One thing that needs you.
#[derive(Debug, Clone, PartialEq)]
pub struct Need {
    /// Stable across calls, so a snooze sticks.
    pub key: String,
    pub what: String,
    /// In plain words, why it's on the list now.
    pub why_now: String,
    pub why: Why,
    /// For ordering within the same `Why`: sooner or older first.
    pub at: u64,
}

/// Things you've put off, until when.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snoozed {
    pub until: BTreeMap<String, u64>,
}

impl Snoozed {
    pub fn holds(&self, key: &str, now: u64) -> bool {
        self.until.get(key).is_some_and(|t| *t > now)
    }
    pub fn put_off(&mut self, keys: &[String], now: u64) {
        for k in keys {
            self.until.insert(k.clone(), now + SNOOZE_SECS);
        }
        self.until.retain(|_, t| *t > now);
    }
}

/// The interruption filter's memory (24 Oct 2026): what a spoken check-in has
/// already surfaced, and when the last check-in was. Kept in the store, so a
/// restart doesn't say the same things again.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Surfaced {
    pub at: BTreeMap<String, u64>,
    pub last_check_in: u64,
}

impl Surfaced {
    /// Is a check-in allowed yet, by the clock alone?
    pub fn due(&self, now: u64) -> bool {
        now.saturating_sub(self.last_check_in) >= CHECK_IN_EVERY_SECS
    }

    /// What needs you and hasn't been said: best first, calendar items left
    /// to the calendar's own reminders, never more than `CAP`.
    pub fn fresh(&self, all: &[Need]) -> Vec<Need> {
        all.iter()
            .filter(|n| n.why != Why::StartsSoon && !self.at.contains_key(&n.key))
            .take(CAP)
            .cloned()
            .collect()
    }

    /// These were just said.
    pub fn mark(&mut self, said: &[Need], now: u64) {
        for n in said {
            self.at.insert(n.key.clone(), now);
        }
        self.last_check_in = now;
        self.at
            .retain(|_, t| now.saturating_sub(*t) < SURFACED_KEEP_SECS);
        if self.at.len() > SURFACED_MAX {
            let mut by_age: Vec<(String, u64)> = self.at.clone().into_iter().collect();
            by_age.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
            by_age.truncate(SURFACED_MAX);
            self.at = by_age.into_iter().collect();
        }
    }
}

/// The calendar's next hour, as (id, title, start).
pub type Soon = (u64, String, u64);

/// Everything that needs you now, best first, from the one work record and
/// the next hour of the calendar, minus what you've put off.
pub fn rank(open: &crate::hub::Open, soon: &[Soon], snoozed: &Snoozed, now: u64) -> Vec<Need> {
    let mut all: Vec<Need> = Vec::new();
    for (id, title, start) in soon {
        if *start < now {
            continue;
        }
        let mins = (start - now).div_ceil(60);
        all.push(Need {
            key: format!("cal:{id}:{start}"),
            what: title.clone(),
            why_now: format!(
                "It starts at {}, in {}.",
                crate::localclock::hhmm_here(*start),
                if mins <= 1 {
                    "a minute".to_string()
                } else {
                    format!("{mins} minutes")
                }
            ),
            why: Why::StartsSoon,
            at: *start,
        });
    }
    for (what, why, _href) in &open.waiting {
        let why_now = if why.trim().is_empty() {
            "It's waiting for your answer.".to_string()
        } else {
            why.clone()
        };
        all.push(Need {
            key: format!("ask:{what}"),
            what: what.clone(),
            why_now,
            why: Why::YourMove,
            at: 0,
        });
    }
    for s in &open.blocked {
        all.push(Need {
            key: format!("stop:{}", s.what),
            what: s.what.clone(),
            why_now: format!("It stopped: {}", s.stopped.trim_end_matches('.')).to_string() + ".",
            why: Why::Stopped,
            at: 0,
        });
    }
    for e in open.failed.iter().chain(open.interrupted.iter()) {
        all.push(Need {
            key: format!("end:{}:{}", e.what, e.at),
            what: e.what.clone(),
            why_now: if e.detail.trim().is_empty() {
                "It didn't finish.".to_string()
            } else {
                e.detail.clone()
            },
            why: Why::Stopped,
            at: e.at,
        });
    }
    all.retain(|n| !snoozed.holds(&n.key, now));
    all.sort_by(|a, b| a.why.cmp(&b.why).then(a.at.cmp(&b.at)));
    all
}

/// What "what needs me?" says: at most `CAP`, numbered, each with why, and
/// how many more there are. Plain and short; never a wall of text.
pub fn said(all: &[Need]) -> String {
    if all.is_empty() {
        return "Nothing needs you right now.".to_string();
    }
    let shown = &all[..all.len().min(CAP)];
    let mut s = match shown.len() {
        1 => "One thing needs you.".to_string(),
        n => format!("{} things need you.", words(n)),
    };
    for (i, n) in shown.iter().enumerate() {
        s.push_str(&format!(
            " {}. {} -- {}",
            i + 1,
            n.what.trim_end_matches('.'),
            n.why_now
        ));
    }
    let more = all.len() - shown.len();
    if more > 0 {
        s.push_str(&format!(" And {more} more on Home."));
    }
    s.push_str(" Say \"not now\" to put these off for a few hours.");
    s
}

fn words(n: usize) -> &'static str {
    match n {
        2 => "Two",
        3 => "Three",
        _ => "Some",
    }
}

/// "What needs me?" and its everyday shapes.
pub fn asks(said: &str) -> bool {
    let s = said.trim().trim_end_matches(['?', '.', '!']).to_lowercase();
    let s = s.replace("what's", "what is");
    [
        "what needs me",
        "what needs my attention",
        "what needs you",
        "anything need me",
        "does anything need me",
        "what do i need to deal with",
        "what is waiting on me",
    ]
    .iter()
    .any(|p| s == *p)
}

/// "Not now", right after the list: put off what was just said.
pub fn puts_off(said: &str) -> bool {
    let s = said.trim().trim_end_matches(['.', '!']).to_lowercase();
    ["not now", "later", "remind me later", "put those off"]
        .iter()
        .any(|p| s == *p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{Ended, Open, Stopped};

    const NOW: u64 = 1_800_000_000;

    fn open() -> Open {
        Open {
            waiting: vec![(
                "Approve sending the invoice to Harbor Lane.".into(),
                "It's a send, so it waits for your yes.".into(),
                "/hub/outstanding".into(),
            )],
            failed: vec![Ended {
                what: "Back up the photos folder.".into(),
                detail: "The drive was full.".into(),
                when: String::new(),
                at: NOW - 600,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn soon_comes_first_then_your_move_then_what_stopped_each_with_why() {
        let soon = vec![(7, "Dentist".to_string(), NOW + 25 * 60)];
        let all = rank(&open(), &soon, &Snoozed::default(), NOW);
        let order: Vec<Why> = all.iter().map(|n| n.why).collect();
        assert_eq!(order, [Why::StartsSoon, Why::YourMove, Why::Stopped]);
        assert!(
            all[0].why_now.contains("in 25 minutes"),
            "{}",
            all[0].why_now
        );
        assert_eq!(all[1].why_now, "It's a send, so it waits for your yes.");
        assert_eq!(all[2].why_now, "The drive was full.");
    }

    #[test]
    fn three_at_most_and_how_many_more() {
        let mut o = open();
        for i in 0..4 {
            o.waiting
                .push((format!("Question {i}"), String::new(), String::new()));
        }
        let all = rank(&o, &[], &Snoozed::default(), NOW);
        let s = said(&all);
        assert!(s.starts_with("Three things need you."), "{s}");
        assert!(s.contains(" 3. ") && !s.contains(" 4. "), "{s}");
        assert!(s.contains("And 3 more on Home."), "{s}");
        assert!(s.contains("It's waiting for your answer."), "{s}");
    }

    #[test]
    fn put_off_stays_off_until_it_is_due_again() {
        let all = rank(&open(), &[], &Snoozed::default(), NOW);
        let mut sn = Snoozed::default();
        sn.put_off(&all.iter().map(|n| n.key.clone()).collect::<Vec<_>>(), NOW);
        assert!(rank(&open(), &[], &sn, NOW + 60).is_empty());
        assert!(rank(&open(), &[], &sn, NOW + SNOOZE_SECS - 1).is_empty());
        assert_eq!(rank(&open(), &[], &sn, NOW + SNOOZE_SECS + 1).len(), 2);
        // Something new is not covered by an old "not now".
        let mut o = open();
        o.waiting
            .push(("A new question".into(), String::new(), String::new()));
        assert_eq!(rank(&o, &[], &sn, NOW + 60).len(), 1);
    }

    #[test]
    fn nothing_is_said_as_nothing_and_past_events_are_not_soon() {
        let soon = vec![(1, "Already started".to_string(), NOW - 60)];
        assert_eq!(
            said(&rank(&Open::default(), &soon, &Snoozed::default(), NOW)),
            "Nothing needs you right now."
        );
    }

    #[test]
    fn a_stopped_item_says_what_stopped_it() {
        let o = Open {
            blocked: vec![Stopped {
                what: "Render the video".into(),
                tried: String::new(),
                stopped: "ffmpeg is missing".into(),
                needs: String::new(),
                area: None,
            }],
            ..Default::default()
        };
        let all = rank(&o, &[], &Snoozed::default(), NOW);
        assert_eq!(all[0].why_now, "It stopped: ffmpeg is missing.");
    }

    #[test]
    fn the_words() {
        for s in [
            "What needs me?",
            "what's waiting on me",
            "Does anything need me?",
        ] {
            assert!(asks(s), "{s}");
        }
        assert!(!asks("what needs doing in the garden"));
        assert!(puts_off("Not now.") && puts_off("later") && !puts_off("not now, do it"));
    }
}
