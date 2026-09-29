//! What a phone widget shows: a glance, never the workspace.
//!
//! The design (UPDATE_COURIER_SPEC §10): read-only glances, no secrets.
//! - **Now / next:** what Atlas is doing, or the next thing today.
//! - **Waiting:** how many things wait on you, the same count as the hub's
//!   Outstanding badge.
//! - **Status:** on, paused or not running, and "as of" when this was
//!   written, so a widget the phone hasn't refreshed says how old it is
//!   instead of passing old news off as current.
//! - **Capture:** a tap into Give (`atlas://hub/give`); a widget can't run
//!   Atlas, so it opens the app rather than taking text itself.
//!
//! Widgets can't reach Atlas's data. The iPhone widget runs in its own
//! process with its own sandbox, and Android's is drawn by the launcher. So
//! Atlas hands over this small projection instead. The phone app fetches it
//! from `/hub/glance.json` and hands it to the widget (iOS through the app
//! group, Android through `AppWidgetManager`). The widget never holds a
//! token, the vault, or anything this file leaves out.
//!
//! **Two views, because of where they're seen.** A home-screen widget is only
//! seen on an unlocked phone. A lock-screen one is seen by anyone who picks
//! the phone up. So `home` carries titles (scrubbed the way anything leaving
//! the laptop is, and cut short) while `lock` carries only times and counts,
//! unless you turn on `phone.widget_titles_on_lock_screen`. That is the same
//! rule as the phone's notifications (`phone.include_detail`): off by default,
//! because Atlas can't know who's looking.

use serde::Serialize;

/// The longest title a widget gets. A small widget shows about two lines;
/// anything longer is cut at a word with an ellipsis rather than by the OS
/// mid-word.
pub const TITLE_MAX: usize = 60;

/// Where Give opens from a widget: the app's own link scheme, which both
/// shells route to `/hub/give`.
pub const CAPTURE_LINK: &str = "atlas://hub/give";

/// The next thing today, at its wall-clock time.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Next {
    /// "14:30".
    pub at: String,
    /// Empty on the lock screen unless titles are allowed there.
    pub what: String,
}

/// One view of the glance: the home screen's or the lock screen's.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct View {
    /// What Atlas is working on. On the lock screen without titles, just
    /// "Working" so the fact shows and the subject doesn't.
    pub working: Option<String>,
    pub next: Option<Next>,
    pub waiting: usize,
}

/// What a widget reads.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Glance {
    /// When this was made, in seconds. The widget shows "as of HH:MM" once
    /// it's more than a few minutes old.
    pub as_of: u64,
    /// "On, listening", "Paused", "Not running".
    pub status: String,
    /// `""` live, `"held"` paused, `"off"` not running: the hub's own tones,
    /// so the widget picks the same icon the hub's status pill uses.
    pub tone: String,
    pub home: View,
    pub lock: View,
    /// Where the capture tap goes.
    pub capture: String,
}

/// The live facts a glance is made from, gathered by the daemon.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub status: String,
    pub tone: String,
    /// What Atlas is working on, if anything, as the hub says it.
    pub working: Option<String>,
    /// Today's remaining moments, in order: (wall-clock time, what). The
    /// daemon passes the hub's spine with what's done and "NOW" taken out.
    pub later: Vec<(String, String)>,
    pub waiting: usize,
}

/// Make the glance. `titles_on_lock` is `phone.widget_titles_on_lock_screen`.
pub fn for_widgets(f: &Facts, now: u64, titles_on_lock: bool) -> Glance {
    let mut scrub = crate::redact::Scrubber::default();
    let mut safe = |s: &str| short(&scrub.scrub(s));
    let working = f.working.as_deref().filter(|w| !w.trim().is_empty()).map(&mut safe);
    let next = f.later.first().map(|(at, what)| Next { at: at.clone(), what: safe(what) });
    let home = View { working, next, waiting: f.waiting };
    let lock = if titles_on_lock {
        home.clone()
    } else {
        View {
            working: home.working.as_ref().map(|_| "Working".to_string()),
            next: home.next.as_ref().map(|n| Next { at: n.at.clone(), what: String::new() }),
            waiting: home.waiting,
        }
    };
    Glance {
        as_of: now,
        status: f.status.clone(),
        tone: f.tone.clone(),
        home,
        lock,
        capture: CAPTURE_LINK.to_string(),
    }
}

/// A title cut to [`TITLE_MAX`] characters at a word, with an ellipsis.
fn short(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= TITLE_MAX {
        return s;
    }
    let cut: String = s.chars().take(TITLE_MAX - 1).collect();
    let at_word = cut.rfind(' ').filter(|&i| i > TITLE_MAX / 2).unwrap_or(cut.len());
    format!("{}…", cut[..at_word].trim_end_matches([',', ';', ':', '-', ' ']))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            status: "On, listening".into(),
            tone: String::new(),
            working: Some("Drafting the reply to Priya about the lease".into()),
            later: vec![("14:30".into(), "Dentist on Elm Street".into()), ("16:00".into(), "Call Sam".into())],
            waiting: 3,
        }
    }

    #[test]
    fn the_lock_screen_gets_times_and_counts_not_subjects() {
        let g = for_widgets(&facts(), 1_000, false);
        assert_eq!(g.home.next.as_ref().unwrap().what, "Dentist on Elm Street");
        assert_eq!(g.lock.next, Some(Next { at: "14:30".into(), what: String::new() }));
        assert_eq!(g.lock.working.as_deref(), Some("Working"));
        assert_eq!(g.lock.waiting, 3);
        let json = serde_json::to_string(&g.lock).unwrap();
        assert!(!json.contains("Priya") && !json.contains("Dentist"), "{json}");
    }

    #[test]
    fn titles_on_the_lock_screen_only_when_turned_on() {
        let g = for_widgets(&facts(), 1_000, true);
        assert_eq!(g.lock, g.home);
    }

    #[test]
    fn titles_are_scrubbed_like_anything_leaving_the_laptop() {
        let mut f = facts();
        f.working = Some("Pay card 4111 1111 1111 1111 today".into());
        let g = for_widgets(&f, 1_000, true);
        let w = g.home.working.unwrap();
        assert!(!w.contains("4111 1111"), "{w}");
    }

    #[test]
    fn a_long_title_is_cut_at_a_word() {
        let s = short("Go through every one of the forty-two unread messages from the accountant before Friday");
        assert!(s.chars().count() <= TITLE_MAX && s.ends_with('…'), "{s}");
        assert!(!s.contains("Fri"), "{s}");
    }

    #[test]
    fn nothing_on_means_nothing_shown() {
        let g = for_widgets(&Facts { status: "Paused".into(), tone: "held".into(), ..Facts::default() }, 5, false);
        assert_eq!(g.home, View { working: None, next: None, waiting: 0 });
        assert_eq!(g.capture, CAPTURE_LINK);
        assert_eq!((g.status.as_str(), g.tone.as_str(), g.as_of), ("Paused", "held", 5));
    }
}
