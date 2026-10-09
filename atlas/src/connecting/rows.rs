//! Everything connected, as one list (N1).
//!
//! Atlas kept its connections on four hub pages, each with its own idea of a
//! row. This is the one list: a row for every mail account, calendar, and kept
//! sign-in, and a row for every service in `connectors` that isn't connected
//! yet, so the page shows what *could* be connected as well as what is. Each
//! row says what it is for, whether it may only read or also act, what state
//! it is in, what breaks if it stops, and has one fix button and one
//! Disconnect. It leads both the Accounts and the Connections pages.
//!
//! State is only "working" when something worked. A kept sign-in nothing has
//! tried yet is "kept", not "fine": silence is not health.

use super::{again, disconnect, esc};
use crate::connect::{self, CalendarLink};
use crate::connectors::{self, Connector, May, Where};
use crate::daemon::Daemon;
use crate::oauthlink::{self, Provider};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Something worked, lately.
    Working,
    /// Kept, and nothing has tried it yet.
    Kept,
    /// Needs you: a refused sign-in, one button to fix.
    NeedsYou,
    /// Tried and failed.
    NotWorking,
    /// Not connected.
    NotSetUp,
}

impl State {
    fn tag(self) -> (&'static str, &'static str) {
        match self {
            State::Working => ("tag ok", "Working"),
            State::Kept => ("tag", "Kept, not tried yet"),
            State::NeedsYou => ("tag bad", "Needs you"),
            State::NotWorking => ("tag bad", "Not working"),
            State::NotSetUp => ("tag", "Not set up"),
        }
    }
}

/// One line of the list.
#[derive(Debug, Clone)]
pub struct Row {
    /// The `connectors` entry this is an instance of.
    pub connector: String,
    /// Which one (an address, a calendar's name), or the service's own name.
    pub which: String,
    pub state: State,
    /// What the state came from, in words ("worked 3 hours ago", the failure).
    pub said: String,
    /// The single button that fixes it, as a form.
    pub fix: String,
    /// The single button that takes it away, as a form, or a note on why not.
    pub take_away: String,
}

fn ago(at: u64, now: u64) -> String {
    let s = now.saturating_sub(at);
    match s {
        0..=89 => "just now".into(),
        90..=5_399 => format!("{} minutes ago", (s / 60).max(2)),
        5_400..=129_599 => format!("{} hours ago", (s / 3_600).max(2)),
        _ => format!("{} days ago", s / 86_400),
    }
}

fn health_state(h: Option<connect::Health>, now: u64) -> (State, String) {
    match h {
        Some(h) if h.ok => (State::Working, format!("worked {}", ago(h.at, now))),
        Some(h) => (State::NotWorking, h.said),
        None => (State::Kept, "not read yet".into()),
    }
}

fn social_form(what: &str, name: &str, button: &str) -> String {
    format!(
        "<form method=post action='/hub/social' class=inline><input type=hidden name=what value='{what}'>\
         <input type=hidden name=name value='{name}'><button>{button}</button></form>"
    )
}

/// Every row, connected ones first (in the order they were found), then what
/// could be connected.
pub fn connected_rows(d: &mut Daemon) -> Vec<Row> {
    let now = crate::store::now();
    let mut out: Vec<Row> = Vec::new();

    // Mail.
    let mine: Vec<crate::mail::Account> = d.store.load(crate::daemon::CONNECTED_ACCOUNTS);
    let accounts = d.tools_cfg().mail.accounts.clone();
    for a in &accounts {
        let ours = mine.iter().any(|m| m.address.eq_ignore_ascii_case(&a.address));
        let id = if a.oauth {
            "outlook"
        } else if a.address.to_lowercase().ends_with("@gmail.com") || a.address.to_lowercase().ends_with("@googlemail.com") {
            "gmail"
        } else {
            "imap-mail"
        };
        let refused = a.oauth && connect::sign_in_refused(&d.store, &a.address);
        let (state, said) = if refused {
            (State::NeedsYou, "Microsoft no longer accepts this sign-in".to_string())
        } else {
            health_state(connect::health_of(&d.store, &a.address), now)
        };
        out.push(Row {
            connector: id.into(),
            which: a.address.clone(),
            state,
            said,
            fix: if refused { again(d, Provider::Microsoft) } else { String::new() },
            take_away: if ours { disconnect("mail", &a.address) } else { "<span class=note>(listed in tools.yaml)</span>".into() },
        });
    }

    // Calendars by link or by sign-in.
    let links: Vec<CalendarLink> = d.store.load(connect::CALENDAR_LINKS);
    for l in &links {
        let parsed = oauthlink::parse_calendar_key(&l.url);
        let id = match parsed.as_ref().map(|(p, _)| *p) {
            Some(Provider::Google) => "google-calendar",
            Some(_) => "outlook-calendar",
            None => "calendar-link",
        };
        let (state, said) = if l.needs_signin {
            (State::NeedsYou, "its sign-in was refused".to_string())
        } else {
            health_state((l.last_read > 0).then(|| connect::Health { at: l.last_read, ok: l.last_ok, said: l.last_said.clone() }), now)
        };
        let fix = match parsed {
            Some((p, _)) if l.needs_signin => again(d, p),
            _ => String::new(),
        };
        out.push(Row { connector: id.into(), which: l.name.clone(), state, said, fix, take_away: disconnect("calendar", &l.url) });
    }

    // Kept sign-ins: the names in the vault are readable while it is sealed.
    let kept: Vec<String> = d.vault.list().iter().map(|(n, _)| n.to_string()).collect();
    let has = |n: &str| kept.iter().any(|k| k == n);
    use crate::social::{VAULT_FACEBOOK, VAULT_INSTAGRAM, VAULT_THREADS, VAULT_TIKTOK, VAULT_YOUTUBE_KEY, VAULT_YOUTUBE_OAUTH};
    if has(VAULT_YOUTUBE_OAUTH) || has(VAULT_YOUTUBE_KEY) {
        out.push(Row {
            connector: "youtube".into(),
            which: "YouTube".into(),
            state: State::Kept,
            said: if has(VAULT_YOUTUBE_OAUTH) { "signed in with Google".into() } else { "an API key".into() },
            fix: String::new(),
            take_away: if has(VAULT_YOUTUBE_OAUTH) {
                "<form method=post action='/hub/social' class=inline><input type=hidden name=what value=youtube-disconnect><button>Disconnect</button></form>".into()
            } else {
                String::new()
            },
        });
    }
    for (vault, id, label) in [
        (VAULT_INSTAGRAM, "instagram", "Instagram"),
        (VAULT_THREADS, "threads", "Threads"),
        (VAULT_FACEBOOK, "facebook", "Facebook Page"),
        (VAULT_TIKTOK, "tiktok", "TikTok"),
    ] {
        if has(vault) {
            out.push(Row {
                connector: id.into(),
                which: label.into(),
                state: State::Kept,
                said: "a kept sign-in".into(),
                fix: String::new(),
                take_away: social_form("token-disconnect", id, "Disconnect"),
            });
        }
    }
    let handle = d.social_cfg().bluesky_handle.trim().trim_start_matches('@').to_string();
    if !handle.is_empty() {
        out.push(Row {
            connector: "bluesky".into(),
            which: format!("@{handle}"),
            state: State::Kept,
            said: "your public numbers, by handle".into(),
            fix: String::new(),
            take_away: social_form("bluesky-disconnect", "bluesky", "Disconnect"),
        });
    }
    if crate::muse::has_key() {
        out.push(Row {
            connector: "muse".into(),
            which: "Muse Spark".into(),
            state: State::Kept,
            said: crate::muse::spent_sentence(now),
            fix: String::new(),
            take_away: "<form method=post action='/hub/connect' class=inline><input type=hidden name=what value=muse-disconnect><button>Disconnect</button></form>".into(),
        });
    }

    // What isn't connected yet.
    let dir = crate::roots::config_dir();
    let all: Vec<Connector> = connectors::all(&dir);
    for c in &all {
        if out.iter().any(|r| r.connector == c.id) {
            continue;
        }
        let page = match c.where_ {
            Where::Accounts => "/hub/accounts",
            Where::Social => "/hub/social",
        };
        out.push(Row {
            connector: c.id.clone(),
            which: c.name.clone(),
            state: State::NotSetUp,
            said: String::new(),
            fix: format!("<a class=button href='{page}'>Connect</a>"),
            take_away: String::new(),
        });
    }
    out
}

/// The list as it appears on a page.
pub fn the_one_list(d: &mut Daemon) -> String {
    let rows = connected_rows(d);
    let dir = crate::roots::config_dir();
    let all = connectors::all(&dir);
    let find = |id: &str| all.iter().find(|c| c.id == id);
    let mut out = String::from("<section id=connected aria-labelledby=connected-h><h2 id=connected-h>Everything connected</h2>");
    if rows.iter().all(|r| r.state == State::NotSetUp) {
        out.push_str("<p class=note>Nothing connected yet.</p>");
    }
    out.push_str("<ul class=connected>");
    for r in &rows {
        let def = find(&r.connector);
        let (class, word) = r.state.tag();
        let may = match def.map(|c| c.may) {
            Some(May::ReadAct) => "<span class=tag>read + act</span> ",
            Some(May::Read) => "<span class=tag>read only</span> ",
            None => "",
        };
        let name = match def {
            Some(c) if !r.which.contains(&c.name) => format!("{} <span class=note>({})</span>", esc(&r.which), esc(&c.name)),
            _ => esc(&r.which),
        };
        out.push_str(&format!("<li><b>{name}</b> {may}<span class='{class}'>{word}</span>"));
        if !r.said.is_empty() {
            out.push_str(&format!(" <span class=note>{}</span>", esc(&r.said)));
        }
        out.push_str(&format!(" {} {}", r.fix, r.take_away));
        if let Some(c) = def {
            out.push_str(&format!("<br><span class=note>{}. {}</span>", esc(&c.for_what), esc(&c.if_it_breaks)));
        }
        out.push_str("</li>");
    }
    out.push_str("</ul></section>");
    out
}
