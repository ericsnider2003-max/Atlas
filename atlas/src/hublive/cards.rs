//! The dashboard's cards.
//!
//! Moved out of `hublive.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Daemon<'_> {
    /// What each dashboard card shows, from live state.
    ///
    /// A card with nothing in it says so in a sentence a person would say. It
    /// never renders an empty frame, because an empty frame and a broken one
    /// look identical.
    pub fn dashboard_cards(&mut self, now: u64) -> Vec<(Card, String)> {
        vec![
            (Card::Outstanding, self.card_outstanding(now)),
            (Card::Projects, self.card_projects()),
            (Card::Today, self.card_today(now)),
            (Card::Machine, self.card_machine()),
            (Card::Activity, self.card_activity(now)),
            (Card::Connections, self.card_connections(now)),
            (Card::Ideas, self.card_ideas()),
            (Card::Stuck, self.card_stuck()),
            (Card::Handed, self.card_handed()),
            (Card::Trust, self.card_trust()),
        ]
    }

    /// Waiting on you: the command deck's first card. Two big numbers — what's
    /// yours to act on, in ember, and what Atlas has proposed, in blue — then
    /// the things themselves.
    pub(super) fn card_outstanding(&self, now: u64) -> String {
        let o = crate::workspace_view::overview(&self.workspace, now);
        // The same things the "waiting" count at the top of every page
        // counts (`waiting_count`), so the two can never disagree: your open
        // items, and what Atlas stopped on and needs you for.
        let mut lines = self.outstanding_lines(now);
        let stopped: Vec<String> = self
            .backlog
            .items
            .iter()
            .filter(|i| !i.done)
            .map(|i| crate::backlog::Backlog::phrase(i))
            .collect();
        lines.extend(stopped.iter().cloned());
        let proposed: Vec<(String, String)> = self
            .workshop
            .projects
            .iter()
            .flat_map(|p| p.ready().into_iter().map(move |c| (c.title.clone(), p.name.clone())))
            .collect();
        if lines.is_empty() && proposed.is_empty() {
            return hub::nothing("Nothing waiting on you. Genuinely, not just unread.");
        }
        let mut s = hub::deck_figures(&[
            ("To act", o.needs_you + o.overdue + stopped.len(), hub::Dot::Act),
            ("Proposed", proposed.len(), hub::Dot::Proposed),
        ]);
        let mut rows: Vec<(hub::Dot, String, String)> = lines
            .iter()
            .take(4)
            .map(|l| (hub::Dot::Act, l.clone(), String::new()))
            .collect();
        for (title, project) in proposed.iter().take(4usize.saturating_sub(rows.len()).max(1)) {
            rows.push((hub::Dot::Proposed, format!("Approve {title}"), project.clone()));
        }
        s.push_str(&hub::rows(&rows));
        s.push_str(&hub::more("/hub/outstanding", "Everything outstanding"));
        s
    }

    /// Projects: each one, and what it is waiting on.
    pub(super) fn card_projects(&self) -> String {
        if self.workshop.projects.is_empty() {
            return hub::nothing(
                "No projects yet. Say \"on the <name> project, …\" and one starts here.",
            );
        }
        let rows: Vec<(hub::Dot, String, String)> = self
            .workshop
            .projects
            .iter()
            .take(5)
            .map(|p| {
                let ready = p.ready().len();
                let working = p.in_progress().len();
                if ready > 0 {
                    (hub::Dot::Act, p.name.clone(), format!("{ready} ready"))
                } else if working > 0 {
                    (hub::Dot::Proposed, p.name.clone(), "building".to_string())
                } else {
                    (hub::Dot::Record, p.name.clone(), "quiet".to_string())
                }
            })
            .collect();
        let mut s = hub::rows(&rows);
        s.push_str(&hub::more("/hub/workshop", "Open the workshop"));
        s
    }

    pub(super) fn card_today(&self, now: u64) -> String {
        let day = crate::workspace_view::day_of(&self.workspace, now);
        let said = crate::workspace_view::day_spoken(&day);
        if said.trim().is_empty() {
            return hub::nothing("Nothing has moved yet today.");
        }
        format!(
            "<p class=said>{}</p>{}",
            hub::esc(&said),
            hub::more("/hub/back", "Look at earlier days")
        )
    }

    /// Health, as rings: memory, disk, and the battery when there is one.
    pub(super) fn card_machine(&self) -> String {
        let r = self.plat.readings();
        if r.ram_total_gb <= 0.0 && r.disk_total_gb <= 0.0 {
            return hub::nothing("I can't read this machine's memory or disk.");
        }
        let mut s = String::new();
        if r.ram_total_gb > 0.0 {
            let free = 1.0 - fraction(r.ram_used_gb, r.ram_total_gb);
            s.push_str(&hub::gauge(
                &format!("{:.1} GB", r.ram_total_gb),
                &format!("memory, {:.0}% free", free * 100.0),
                fraction(r.ram_used_gb, r.ram_total_gb),
            ));
        }
        if r.disk_total_gb > 0.0 {
            let free = fraction(r.disk_free_gb, r.disk_total_gb);
            s.push_str(&hub::gauge(
                &format!("{:.0} GB", r.disk_total_gb),
                &format!("disk, {:.0}% free", free * 100.0),
                1.0 - free,
            ));
        }
        if let Some(b) = r.battery_percent {
            s.push_str(&hub::gauge(
                &format!("{b}%"),
                if r.on_battery { "battery, on battery" } else { "battery, plugged in" },
                1.0 - (b as f32 / 100.0),
            ));
        }
        s
    }

    /// What I did without being asked: the last day of Atlas's own work,
    /// newest first, each with the time on your clock.
    pub(super) fn card_activity(&self, now: u64) -> String {
        let since = now.saturating_sub(24 * 3600);
        let rows: Vec<(hub::Dot, String, String)> = self
            .journal
            .since(since)
            .into_iter()
            .rev()
            .filter(|e| e.kind != crate::activity::Kind::Upkeep)
            .take(5)
            .map(|e| (hub::Dot::Record, e.what.clone(), crate::localclock::hhmm_here(e.at)))
            .collect();
        if rows.is_empty() {
            return hub::nothing("I haven't done anything on my own today.");
        }
        let mut s = hub::rows(&rows);
        s.push_str(&hub::more("/hub/activity", "Full activity"));
        s
    }

    pub(super) fn card_connections(&self, now: u64) -> String {
        let lines = self.connection_lines(now);
        if lines.is_empty() {
            return hub::nothing("Nothing outside this machine is connected yet.");
        }
        hub::lines(&lines)
    }

    pub(super) fn card_ideas(&mut self) -> String {
        self.refresh_signals();
        let cfg = self.tools_cfg().self_audit.clone();
        if !cfg.enabled {
            return hub::nothing("Looking at myself is switched off.");
        }
        let recs = crate::selfaudit::recommend(&self.signals, cfg.most_at_once);
        if recs.is_empty() {
            return hub::nothing("Nothing about myself I'd change right now.");
        }
        let lines: Vec<String> = recs.iter().map(|r| r.symptom.clone()).collect();
        let mut s = hub::lines(&lines);
        s.push_str(&hub::more("/hub/recommendations", "Read the reasoning"));
        s
    }

    pub(super) fn card_stuck(&self) -> String {
        let stuck: Vec<String> = self
            .backlog
            .items
            .iter()
            .filter(|i| !i.done)
            .map(crate::backlog::Backlog::phrase)
            .collect();
        if stuck.is_empty() {
            return hub::nothing("Nothing is waiting on you.");
        }
        hub::lines(&stuck[..stuck.len().min(5)])
    }

    /// What you handed over, and what was in it.
    ///
    /// The text shown here came from outside this machine. It is rendered and
    /// escaped like any other outside text and never goes anywhere near the
    /// part of Atlas that decides what to do.
    pub(super) fn card_handed(&self) -> String {
        let open = self.tray.open();
        if open.is_empty() {
            return hub::nothing(
                "Nothing waiting. Send a link from your phone and I'll read it.",
            );
        }
        let mut s = String::from("<ul class=tight>");
        for item in open.iter().take(5) {
            let state = match item.state {
                crate::tray::State::Waiting => "not read yet".to_string(),
                crate::tray::State::Read => item
                    .found
                    .clone()
                    .unwrap_or_else(|| "read".to_string())
                    .chars()
                    .take(180)
                    .collect(),
                crate::tray::State::Stuck => item
                    .found
                    .clone()
                    .unwrap_or_else(|| "I couldn't open it".to_string()),
                crate::tray::State::Done => continue,
            };
            s.push_str(&format!(
                "<li><b>{}</b> <span class=chip>{}</span><br>\
                 <span class=what>{}</span>\
                 <form class=inline method=post action=/hub/tray>\
                 <input type=hidden name=id value='{}'>\
                 <button class=go>Finished with it</button></form></li>",
                hub::esc(&item.title()),
                hub::esc(item.sort.title()),
                hub::esc(&state),
                item.id
            ));
        }
        s.push_str("</ul>");
        s
    }

    /// What Atlas is trusted with, and what would change it.
    ///
    /// Shown rather than kept internal on purpose. "Not confident enough" with
    /// no reason attached is what makes a system feel arbitrary; naming the
    /// thing that would change it makes getting better something you can both
    /// see happening.
    pub(super) fn card_trust(&self) -> String {
        let mut s = String::new();
        for space in std::iter::once(crate::earned::Space::Personal).chain(
            self.earned
                .businesses()
                .into_iter()
                .map(crate::earned::Space::Business),
        ) {
            // Named even when it is the only one. "Your own work" being a
            // heading is what makes a business appearing beneath it read as a
            // separate record rather than more of the same.
            s.push_str(&format!("<h4 class=spacename>{}</h4>", hub::esc(&space.title())));
            s.push_str(&self.trust_rows(&space));
        }
        s
    }

    pub(super) fn trust_rows(&self, space: &crate::earned::Space) -> String {
        let mut s = String::from("<ul class=tight>");
        for (kind, rope, good, total) in self.earned.standing_in(space) {
            s.push_str(&format!(
                "<li><b>{}</b> — {}<br><span class=what>{}</span></li>",
                hub::esc(kind.title()),
                hub::esc(rope.plain()),
                hub::esc(&if total == 0 {
                    format!("Nothing yet. {}", self.earned.what_would_earn_more_in(space, kind))
                } else {
                    format!(
                        "{good} right out of {total}. {}",
                        self.earned.what_would_earn_more_in(space, kind)
                    )
                })
            ));
        }
        s.push_str("</ul>");
        s
    }
}
