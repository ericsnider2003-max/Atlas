//! Connector definitions as a real file format.
//!
//! The built-in list lives in `config/connectors/builtin.yaml`, embedded at
//! build time. People can add their own as `*.yaml` files in a `connectors`
//! folder, but only calendar-by-link (or "other") ones: read-only, link
//! handshake, a bare hostname. A bank (finance) can only ever be read.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Email,
    Calendar,
    Social,
    Finance,
    Files,
    Messaging,
    Developer,
    Ai,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum May {
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "read+act")]
    ReadAct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rung {
    SignIn,
    DeviceCode,
    AppPassword,
    BrowserSession,
    Key,
    Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Where {
    Accounts,
    Social,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connector {
    pub id: String,
    pub name: String,
    pub group: Group,
    #[serde(rename = "for")]
    pub for_what: String,
    pub may: May,
    pub handshake: Vec<Rung>,
    pub domains: Vec<String>,
    pub if_it_breaks: String,
    #[serde(rename = "where")]
    pub where_: Where,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Builtin,
    Yours,
}

fn is_slug(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_bare_host(d: &str) -> bool {
    d.contains('.')
        && !d.starts_with('.')
        && !d.ends_with('.')
        && d.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

fn parse(text: &str, source: Source) -> Result<Vec<Connector>, String> {
    let list: Vec<Connector> = serde_yaml::from_str(text)
        .map_err(|e| format!("I couldn't read that connector file: {e}"))?;
    let mut seen: Vec<&str> = Vec::new();
    for c in &list {
        let id = c.id.as_str();
        if seen.contains(&id) {
            return Err(format!("Two connectors are called '{id}'. Each needs its own id."));
        }
        seen.push(id);
        if !is_slug(id) {
            return Err(format!("'{id}' isn't a good id. Use lowercase letters, digits and dashes."));
        }
        if c.name.trim().is_empty() {
            return Err(format!("'{id}' has no name."));
        }
        if c.for_what.trim().is_empty() {
            return Err(format!("'{id}' doesn't say what it is for."));
        }
        if c.if_it_breaks.trim().is_empty() {
            return Err(format!("'{id}' doesn't say what to do if it breaks."));
        }
        if c.handshake.is_empty() {
            return Err(format!("'{id}' doesn't say how it connects."));
        }
        if c.group == Group::Finance && c.may == May::ReadAct {
            return Err(format!("'{id}' is a bank-type connector: a bank can only be read."));
        }
        if source == Source::Yours {
            if c.may != May::Read {
                return Err(format!("'{id}' must be read-only. Your own connectors can only read."));
            }
            if c.handshake != [Rung::Link] {
                return Err(format!("'{id}' must connect by link only."));
            }
            if c.domains.is_empty() {
                return Err(format!("'{id}' needs at least one domain."));
            }
            if let Some(d) = c.domains.iter().find(|d| !is_bare_host(d)) {
                return Err(format!(
                    "'{id}' has a domain that isn't a bare hostname ('{d}'). Write it like calendar.example.com, with no https:// and no slash."
                ));
            }
            if !matches!(c.group, Group::Calendar | Group::Other) {
                return Err(format!("'{id}' must be in the calendar or other group."));
            }
        }
    }
    Ok(list)
}

/// The connectors that ship with Atlas. Empty if the embedded file is broken.
pub fn builtin() -> &'static [Connector] {
    static LIST: OnceLock<Vec<Connector>> = OnceLock::new();
    LIST.get_or_init(|| {
        parse(include_str!("../config/connectors/builtin.yaml"), Source::Builtin).unwrap_or_default()
    })
}

pub fn find(id: &str) -> Option<&'static Connector> {
    builtin().iter().find(|c| c.id == id)
}

/// Your own connectors from `dir/connectors/*.yaml`: (accepted, refusals).
pub fn yours(dir: &Path) -> (Vec<Connector>, Vec<String>) {
    let mut ok: Vec<Connector> = Vec::new();
    let mut refused = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir.join("connectors")) else {
        return (ok, refused);
    };
    let mut files: Vec<_> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    files.sort();
    for p in files {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) => {
                refused.push(format!("{name}: I couldn't open it ({e})."));
                continue;
            }
        };
        match parse(&text, Source::Yours) {
            Ok(list) => {
                if let Some(c) = list
                    .iter()
                    .find(|c| find(&c.id).is_some() || ok.iter().any(|o| o.id == c.id))
                {
                    refused.push(format!("{name}: '{}' is already taken by another connector.", c.id));
                } else {
                    ok.extend(list);
                }
            }
            Err(e) => refused.push(format!("{name}: {e}")),
        }
    }
    (ok, refused)
}

/// Built-in plus your accepted connectors.
pub fn all(dir: &Path) -> Vec<Connector> {
    let mut v = builtin().to_vec();
    v.extend(yours(dir).0);
    v
}
