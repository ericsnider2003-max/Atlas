//! Watching something from the outside.
//!
//! A machine you run elsewhere (a server, a home lab, a cloud box) is
//! watched cleanly: Atlas can confirm it is alive and reachable **without any path into it**. A TCP
//! connect and nothing else — no credentials, no control, no data. If the
//! Atlas laptop were ever compromised, this gives an attacker nothing they
//! could not learn by port-scanning.
//!
//! The design problem is not the probe. It is not crying wolf: networks blip,
//! and an alert on every dropped packet is an alert you learn to ignore.

use crate::error::Result;
use crate::store::Store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum Health {
    Up,
    Down,
    /// Going up and down repeatedly — worse than plainly down, and easy to
    /// miss if you only alert on transitions.
    Flapping,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Target {
    pub name: String,
    /// host:port. A listening port is all Atlas ever touches.
    pub address: String,
    #[serde(default = "d_every")]
    pub check_every: u64,
    /// Consecutive failures before it counts as down.
    #[serde(default = "d_fails")]
    pub fails_before_down: u32,
    /// Transitions within this window that mean "flapping".
    #[serde(default = "d_flap_window")]
    pub flap_window: u64,
    #[serde(default = "d_flap_count")]
    pub flap_transitions: u32,
    #[serde(default)]
    pub enabled: bool,
}
fn d_every() -> u64 { 120 }
fn d_fails() -> u32 { 3 }
fn d_flap_window() -> u64 { 1800 }
fn d_flap_count() -> u32 { 4 }

impl Default for Target {
    fn default() -> Self {
        Target {
            name: String::new(),
            address: String::new(),
            check_every: d_every(),
            fails_before_down: d_fails(),
            flap_window: d_flap_window(),
            flap_transitions: d_flap_count(),
            enabled: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub health: Health,
    consecutive_fails: u32,
    last_checked: u64,
    /// When health last changed, newest last.
    transitions: Vec<u64>,
    pub down_since: Option<u64>,
    pub last_up: Option<u64>,
}


/// Something worth telling you about.
#[derive(Debug, Clone, PartialEq)]
pub enum Alert {
    None,
    WentDown { name: String, say: String },
    CameBack { name: String, say: String },
    Flapping { name: String, say: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Watcher {
    pub targets: Vec<Target>,
    pub status: std::collections::BTreeMap<String, Status>,
}

impl Watcher {
    pub fn load(store: &Store) -> Watcher {
        store.load("watch")
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("watch", self)
    }

    pub fn add(&mut self, t: Target) {
        self.targets.retain(|x| x.name != t.name);
        self.targets.push(t);
    }

    pub fn due(&self, name: &str, t: u64) -> bool {
        let Some(target) = self.targets.iter().find(|x| x.name == name) else { return false };
        if !target.enabled {
            return false;
        }
        let last = self.status.get(name).map(|s| s.last_checked).unwrap_or(0);
        t.saturating_sub(last) >= target.check_every
    }

    /// Record one probe result and decide whether to say anything.
    ///
    /// A single failed probe is never an alert — that is a dropped packet, not
    /// an outage.
    pub fn observe(&mut self, name: &str, reachable: bool, t: u64) -> Alert {
        let Some(target) = self.targets.iter().find(|x| x.name == name).cloned() else {
            return Alert::None;
        };
        let s = self.status.entry(name.to_string()).or_default();
        s.last_checked = t;
        let before = s.health;

        if reachable {
            s.consecutive_fails = 0;
            s.last_up = Some(t);
            s.health = Health::Up;
            s.down_since = None;
        } else {
            s.consecutive_fails += 1;
            if s.consecutive_fails >= target.fails_before_down {
                if s.health != Health::Down {
                    s.down_since = Some(t);
                }
                s.health = Health::Down;
            }
        }

        if s.health == before {
            return Alert::None;
        }
        s.transitions.push(t);
        s.transitions.retain(|x| t.saturating_sub(*x) <= target.flap_window);

        if s.transitions.len() as u32 >= target.flap_transitions {
            s.health = Health::Flapping;
            return Alert::Flapping {
                name: name.into(),
                say: format!("{name} keeps dropping in and out."),
            };
        }

        match s.health {
            Health::Down => Alert::WentDown {
                name: name.into(),
                say: format!("{name} has stopped responding."),
            },
            Health::Up if before != Health::Unknown => Alert::CameBack {
                name: name.into(),
                say: format!("{name} is back."),
            },
            _ => Alert::None,
        }
    }

    pub fn health(&self, name: &str) -> Health {
        self.status.get(name).map(|s| s.health).unwrap_or(Health::Unknown)
    }

    pub fn down_for(&self, name: &str, t: u64) -> Option<u64> {
        self.status.get(name)?.down_since.map(|at| t.saturating_sub(at))
    }

    pub fn summary(&self) -> String {
        let on: Vec<&Target> = self.targets.iter().filter(|t| t.enabled).collect();
        if on.is_empty() {
            return "Not watching anything.".into();
        }
        let bad: Vec<String> = on
            .iter()
            .filter(|t| !matches!(self.health(&t.name), Health::Up))
            .map(|t| format!("{} is {:?}", t.name, self.health(&t.name)).to_lowercase())
            .collect();
        if bad.is_empty() {
            format!("All {} up.", on.len())
        } else {
            bad.join(", ")
        }
    }
}

/// A read-only reachability check. Deliberately the weakest possible probe:
/// it proves something is listening and nothing more.
pub fn reachable(address: &str, timeout_ms: u64) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;
    address
        .to_socket_addrs()
        .map(|addrs| {
            addrs
                .into_iter()
                .any(|a| TcpStream::connect_timeout(&a, Duration::from_millis(timeout_ms)).is_ok())
        })
        .unwrap_or(false)
}
