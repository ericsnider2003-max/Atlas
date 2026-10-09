//! The deck, the home asks and the glances.
//!
//! Moved out of `hublive.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Daemon<'_> {
    /// What a phone widget shows: today's next thing, the waiting count and
    /// whether Atlas is on, made safe for a home or lock screen (`glance`).
    pub fn glance(&self, now: u64) -> crate::glance::Glance {
        let deck = self.deck(now, crate::localclock::offset_secs());
        let working = self.mind.focus().map(|w| sentence(&w.asked));
        let later = deck
            .spine
            .iter()
            .filter(|(_, _, m)| *m == hub::Mark::Later)
            .map(|(t, w, _)| (t.clone(), w.clone()))
            .collect();
        let facts = crate::glance::Facts {
            status: deck.status.clone(),
            tone: deck.tone.to_string(),
            working,
            later,
            waiting: self.waiting_count(now),
        };
        crate::glance::for_widgets(&facts, now, self.tools_cfg().phone.widget_titles_on_lock_screen)
    }

    pub fn deck(&self, now: u64, off: i64) -> hub::Deck {
        use crate::localclock::{day, hhmm, hour};
        let name = self.what_to_call_you();
        let greeting = match (crate::nudge::Part::from_hour(hour(now, off)), &name) {
            (Some(part), Some(n)) => format!("{}, {n}.", part.greeting()),
            (Some(part), None) => format!("{}.", part.greeting()),
            (None, Some(n)) => format!("Hello, {n}."),
            (None, None) => "Hello.".to_string(),
        };

        let cfg = self.tools_cfg();
        let hears = cfg.enabled
            && self.audio_devices.as_ref().map(|d| !d.is_empty()).unwrap_or(false);
        let (status, tone) = if self.attention.is_paused() {
            ("Paused".to_string(), "held")
        } else if hears {
            ("On, listening".to_string(), "")
        } else {
            ("On, typing".to_string(), "")
        };

        // Everything is a real moment, placed on your wall clock.
        let today = day(now, off);
        let now_wall = now as i64 + off;
        let mut events: Vec<(i64, String, String, hub::Mark)> = Vec::new();
        let start_of_day = crate::localclock::midnight(now, off);
        let calendar_available = self.calendar.availability_error().is_none();
        for e in self.calendar.occurrences_between(start_of_day, start_of_day + 86_400).into_iter().filter(|_| calendar_available) {
            if day(e.start, off) != today && !e.all_day {
                continue;
            }
            let at = e.start as i64 + off;
            let (label, mark) = if e.all_day {
                ("All day".to_string(), hub::Mark::Later)
            } else if (e.end as i64 + off) <= now_wall {
                (hhmm(e.start, off), hub::Mark::Done)
            } else {
                (hhmm(e.start, off), hub::Mark::Later)
            };
            events.push((if e.all_day { today * 86_400 } else { at }, label, e.title.clone(), mark));
        }
        for e in self.journal.since(now.saturating_sub(86_400)) {
            if e.kind == crate::activity::Kind::Upkeep || day(e.at, off) != today {
                continue;
            }
            events.push((e.at as i64 + off, hhmm(e.at, off), e.what.clone(), hub::Mark::Done));
        }
        events.sort_by_key(|(w, ..)| *w);

        let running = self
            .queue
            .tasks
            .iter()
            .find(|t| t.state == crate::lanes::TaskState::Running);
        let waiting_to_start = self.queue.tasks.iter().find(|t| {
            matches!(t.state, crate::lanes::TaskState::Queued | crate::lanes::TaskState::WaitingForGap)
        });
        let next = events.iter().find(|(w, _, _, m)| *m == hub::Mark::Later && *w >= now_wall);
        let (now_line, mut sub, now_short) = if let Some(t) = running {
            let more = self.queue.tasks.iter().filter(|t| !matches!(t.state, crate::lanes::TaskState::Done | crate::lanes::TaskState::Failed)).count().saturating_sub(1);
            (
                sentence(&t.command),
                if more > 0 {
                    format!("Started {}. {more} more after this.", hhmm(t.created, off))
                } else {
                    format!("Started {}.", hhmm(t.created, off))
                },
                sentence(&t.command),
            )
        } else if let Some(t) = waiting_to_start {
            (
                format!("About to start: {}", t.command.trim()),
                if t.state == crate::lanes::TaskState::WaitingForGap {
                    "Waiting for you to stop typing, so it doesn't get in your way.".to_string()
                } else {
                    "Next in line.".to_string()
                },
                "About to start".to_string(),
            )
        } else if self.attention.is_paused() {
            ("Paused.".to_string(), "Say \"carry on\" when you want me back.".to_string(), "Paused".to_string())
        } else {
            (
                "Nothing underway.".to_string(),
                match next {
                    Some((_, time, what, _)) => format!("Next: {what} at {time}."),
                    None if crate::phonemode::on() => "Say what you need, or tap the search to find anything.".to_string(),
                    None => "Say what you need, or press Ctrl K to find anything.".to_string(),
                },
                "You're here".to_string(),
            )
        };

        if !calendar_available {
            sub.push_str(" Calendar unavailable: today's events cannot be confirmed. Refresh or recover it.");
        }
        // The last three things done, now, and the next three.
        let done: Vec<_> = events.iter().filter(|(w, ..)| *w <= now_wall).collect();
        let later: Vec<_> = events.iter().filter(|(w, ..)| *w > now_wall).collect();
        let mut spine: Vec<(String, String, hub::Mark)> = done
            .iter()
            .skip(done.len().saturating_sub(3))
            .map(|(_, t, w, _)| (t.clone(), w.clone(), hub::Mark::Done))
            .collect();
        if !events.is_empty() || running.is_some() {
            spine.push(("NOW".to_string(), now_short, hub::Mark::Now));
        }
        spine.extend(later.iter().take(3).map(|(_, t, w, m)| (t.clone(), w.clone(), *m)));

        let asks = self.home_asks(now);
        let brief = self.brief_line(&asks, running.is_some(), &now_line);
        let first_run = calendar_available && events.is_empty()
            && self.workspace.is_empty()
            && self.workshop.projects.is_empty()
            && self.calendar.is_empty()
            && self.journal.since(0).is_empty()
            && asks.is_empty();
        hub::Deck {
            greeting,
            status,
            tone,
            now: now_line,
            now_sub: sub,
            spine,
            brief,
            asks,
            businesses: self.glances(now),
            first_run,
        }
    }

    /// What's waiting on you, for the Brief on Home and the first lane of
    /// Outstanding: your own open items, what Atlas stopped on and needs you
    /// for, and changes it built that wait on your yes. The same things the
    /// waiting count at the top of every page counts.
    pub(super) fn home_asks(&self, now: u64) -> Vec<(String, String)> {
        self.home_asks_keyed(now).into_iter().map(|(what, href, _)| (what, href)).collect()
    }

    /// `home_asks`, each with the key its Drop it button sends (see
    /// `hub::Drops`). Worked out here, where each ask is made, rather than
    /// matched back up by its words afterwards: two things can read the same.
    pub(super) fn home_asks_keyed(&self, now: u64) -> Vec<(String, String, Option<String>)> {
        let mut out: Vec<(String, String, Option<String>)> = Vec::new();
        if let Some(view) = crate::workspace_view::shipped().into_iter().find(|v| v.name == "Now") {
            for i in crate::workspace_view::apply(&self.workspace, &view, now) {
                out.push((i.title.clone(), hub::Page::Workspace.href().to_string(), Some(format!("w:{}", i.id))));
            }
        }
        for b in self.backlog.items.iter().filter(|i| !i.done && !i.dismissed) {
            if waits_on_you(&b.blocker) {
                out.push((sentence(&b.request), hub::Page::Outstanding.href().to_string(), Some(format!("b:{}", b.id))));
            }
        }
        for p in &self.workshop.projects {
            for c in p.ready() {
                out.push((
                    format!("Approve \"{}\" ({})", c.title, p.name),
                    hub::Page::Workshop.href().to_string(),
                    Some(format!("c:{}:{}", c.id, p.name)),
                ));
            }
        }
        out
    }

    /// The Brief, in Atlas's words: how many things want you, what it's on,
    /// and — when nothing does — that it's quiet, said plainly.
    pub(super) fn brief_line(&self, asks: &[(String, String)], working: bool, now_line: &str) -> String {
        let doing = if working { format!(" I'm on it now: {}", now_line.trim_end_matches('.')) } else { String::new() };
        let doing = if doing.is_empty() { doing } else { format!("{doing}.") };
        match asks.len() {
            0 => format!("It's quiet. Nothing needs you.{doing}"),
            1 => format!("One thing wants you.{doing}"),
            n => {
                const WORDS: [&str; 9] = ["Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten"];
                let said = WORDS.get(n - 2).map(|w| w.to_string()).unwrap_or_else(|| n.to_string());
                format!("{said} things want you.{doing}")
            }
        }
    }

    /// Your businesses at a glance: each business on the roster, its open
    /// work (items filed under it as the client), and its people.
    pub(super) fn glances(&self, _now: u64) -> Vec<hub::Glance> {
        let roster = crate::roster::Roster::load(&self.store);
        roster
            .businesses()
            .into_iter()
            .map(|name| {
                let open = self
                    .workspace
                    .iter()
                    .filter(|i| i.status.live() && i.client.as_deref().map(|c| crate::kin::same_name(c, &name)).unwrap_or(false))
                    .count();
                let people = roster.members(&name).into_iter().map(|m| (m, "on the roster".to_string())).collect();
                hub::Glance { name, open, people }
            })
            .collect()
    }
}
