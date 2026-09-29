//! An Atlas somewhere else, that you can ask about.
//!
//! `kin.rs` is the door another Atlas knocks on, and its first rule is the one
//! that makes it safe: **a signal can become exactly one thing, a `Nudge`.**
//! Never a command, never an action. That rule is right and nothing here
//! weakens it.
//!
//! But it only runs one way. A server-side Atlas can tell you something is
//! urgent; you cannot ask it how it is getting on. On a server with nobody
//! logged in, that is the wrong way round — the machine that most needs
//! looking in on is the one nobody looks at.
//!
//! ## What this is
//!
//! Your Atlas asking another Atlas a question you already had the right to
//! ask, over the hub API that Atlas already serves: `GET /status`,
//! `GET /outstanding`, `GET /queued`. Read-only, token on every request, and
//! **you** started it.
//!
//! ## The rule this keeps
//!
//! A brief is **words you read**. It never becomes an `Intent`, an `Action`,
//! or an approval, and there is no function in this file that turns one into
//! any of those. That is the same rule `kin.rs` holds itself to, for the same
//! reason: another Atlas saying "sell everything" is a sentence, not an
//! instruction, whichever direction it travelled in.
//!
//! The difference between this and `kin` is only who started it. An unbidden
//! message from another machine is a nudge; an answer to a question you asked
//! is a report. Neither is a command.
//!
//! ## What it deliberately is not
//!
//! Not a way to run something over there. Handing work to another Atlas is a
//! different problem with a different answer — it arrives as something a
//! person approves, and the hub already has `POST /approve` for that. Mixing
//! "tell me how you are" with "do this" in one channel is how a read-only
//! door stops being one.

use serde::Deserialize;
use std::time::Duration;

/// An Atlas you can reach and the token that proves you may.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct Elsewhere {
    /// What you call it out loud: "homelab", "the server".
    pub name: String,
    pub host: String,
    /// The hub's port, not `kin`'s. Two doors, two trust levels — `kin.rs`
    /// says why they must never share a listener, and the same reasoning
    /// applies to which one you point a question at.
    pub port: u16,
    /// The port this peer listens for *direct device sync* on — a third door,
    /// separate again from the hub and from `kin`, because a bundle is your
    /// notes and travels sealed. `None` means the fixed default
    /// (`transport::SYNC_PORT`), which is what both machines use out of the box;
    /// set it only when that port is taken on the other machine. This is also
    /// what lets a test point the dial at an ephemeral port rather than the one
    /// fixed port every real install shares.
    #[serde(default)]
    pub sync_port: Option<u16>,
    pub token: String,
}

impl Default for Elsewhere {
    fn default() -> Self {
        Elsewhere {
            name: String::new(),
            host: String::new(),
            port: 8787,
            sync_port: None,
            token: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ElsewhereConfig {
    pub enabled: bool,
    /// The Atlases you have chosen to be able to ask about.
    ///
    /// Named one at a time, deliberately, the same way `kin` adds a peer.
    /// Being reachable on a network is not the same as being yours to ask.
    pub known: Vec<Elsewhere>,
    /// Seconds to wait. A server that is asleep should not hold up a sentence.
    pub timeout_secs: u64,
}

impl Default for ElsewhereConfig {
    fn default() -> Self {
        ElsewhereConfig { enabled: true, known: Vec::new(), timeout_secs: 5 }
    }
}

impl ElsewhereConfig {
    /// Case-insensitively, the way every other place a person types a name
    /// does it. `kin.rs` learned this once already.
    pub fn find(&self, name: &str) -> Option<&Elsewhere> {
        let n = name.trim();
        self.known.iter().find(|e| e.name.trim().eq_ignore_ascii_case(n))
    }

    /// What you could ask about, for when you name one that isn't there.
    pub fn names(&self) -> Vec<&str> {
        self.known.iter().map(|e| e.name.trim()).filter(|n| !n.is_empty()).collect()
    }
}

/// What came back. Words, and nothing that can act.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Brief {
    pub name: String,
    /// How it says it is.
    pub status: String,
    /// What it could not do.
    pub outstanding: Vec<String>,
    /// What it is holding.
    pub queued: String,
}

/// Ask one for its brief.
///
/// Three reads rather than one endpoint, because those three already exist and
/// serve exactly this. Adding a `/brief` that returns their concatenation
/// would be a fourth thing to keep in step with the other three.
///
/// A failure on any one of them is not a failure of the whole: a server that
/// answers `/status` and times out on `/outstanding` has still told you
/// something, and saying nothing because part of it was missing is how a
/// check-in becomes useless exactly when it matters.
pub fn ask(e: &Elsewhere, cfg: &ElsewhereConfig) -> Result<Brief, String> {
    if !cfg.enabled {
        return Err("asking your other Atlases is switched off in your settings".into());
    }
    let host = format!("{}:{}", e.host.trim(), e.port);
    let t = Duration::from_secs(cfg.timeout_secs.max(1));

    let status = read(&host, "/status", &e.token, t);
    // Nothing at all came back from the first read: say so once, plainly,
    // rather than three times.
    if let Err(why) = &status {
        return Err(format!("I couldn't reach {} — {why}", e.name.trim()));
    }
    Ok(Brief {
        name: e.name.trim().to_string(),
        status: status.unwrap_or_default(),
        outstanding: read(&host, "/outstanding", &e.token, t)
            .map(|s| {
                s.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        queued: read(&host, "/queued", &e.token, t).unwrap_or_default(),
    })
}

fn read(host: &str, path: &str, token: &str, t: Duration) -> Result<String, String> {
    match crate::http::get_with_token(host, path, token, t) {
        Ok(r) if r.ok() => Ok(r.body.trim().to_string()),
        Ok(r) => Err(format!("it answered {}", r.status)),
        Err(e) => Err(format!("{e}")),
    }
}

/// The brief, said.
///
/// Leads with what is wrong. A report that opens with "all fine" and closes
/// with three things it could not do is a report whose reader stops at the
/// first clause — the same reasoning `overnight::morning_brief` follows.
pub fn spoken(b: &Brief) -> String {
    let name = if b.name.is_empty() { "it" } else { &b.name };
    let mut parts: Vec<String> = Vec::new();

    if !b.outstanding.is_empty() {
        parts.push(format!(
            "{} thing{} it couldn't do: {}",
            b.outstanding.len(),
            if b.outstanding.len() == 1 { "" } else { "s" },
            b.outstanding.join("; ")
        ));
    }
    if !b.queued.trim().is_empty() && !b.queued.trim().eq_ignore_ascii_case("nothing") {
        parts.push(format!("waiting: {}", b.queued.trim()));
    }
    if !b.status.trim().is_empty() {
        parts.push(b.status.trim().to_string());
    }

    if parts.is_empty() {
        return format!("{name} answered, but had nothing to say about itself.");
    }
    format!("{name}: {}", parts.join(". "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ElsewhereConfig {
        ElsewhereConfig {
            enabled: true,
            known: vec![Elsewhere {
                name: "Homelab".into(),
                host: "10.0.0.9".into(),
                port: 8787,
                sync_port: None,
                token: "x".into(),
            }],
            timeout_secs: 5,
        }
    }

    #[test]
    fn a_name_is_matched_the_way_a_person_types_it() {
        assert!(cfg().find("homelab").is_some());
        assert!(cfg().find("  HOMELAB ").is_some());
        assert!(cfg().find("the server").is_none());
    }

    #[test]
    fn nothing_is_reachable_just_because_it_is_on_the_network() {
        // The same rule `kin` holds: a peer is named once, deliberately.
        let empty = ElsewhereConfig::default();
        assert!(empty.known.is_empty(), "something was trusted by default");
        assert!(empty.find("homelab").is_none());
    }

    #[test]
    fn the_brief_leads_with_what_is_wrong() {
        let b = Brief {
            name: "Homelab".into(),
            status: "running, 4 days up".into(),
            outstanding: vec!["reconnect the broker feed".into()],
            queued: "nothing".into(),
        };
        let said = spoken(&b);
        let trouble = said.find("couldn't do").expect("the trouble is missing");
        let fine = said.find("running").expect("the status is missing");
        assert!(trouble < fine, "it opened with the good news: {said}");
    }

    #[test]
    fn a_silent_machine_is_not_reported_as_a_healthy_one() {
        let b = Brief { name: "Homelab".into(), ..Default::default() };
        let said = spoken(&b);
        assert!(
            said.contains("nothing to say"),
            "an empty brief read as a clean bill of health: {said}"
        );
    }

    #[test]
    fn switched_off_it_asks_nothing() {
        let off = ElsewhereConfig { enabled: false, ..cfg() };
        let e = off.known[0].clone();
        assert!(ask(&e, &off).is_err(), "it went to the network with the setting off");
    }
}
