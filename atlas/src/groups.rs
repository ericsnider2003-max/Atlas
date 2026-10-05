//! Groups with an owner: who is in a group chat, and who may say what in it.
//!
//! The first group chats had no owner. Every member's Atlas kept its own list
//! and learned members from the messages it heard, so any member could bring
//! anyone in just by naming them on a message -- and nobody could take
//! anybody out. That is fine for three friends and wrong for everything else,
//! including the one group this project needs most: the release channel,
//! where Eric posts updates and nobody else can.
//!
//! So a group can now have an **owner** -- whoever made it -- and the owner's
//! Atlas alone decides:
//!
//! - who is in it (add, remove),
//! - what each person may do (the `Role`: owner, member, or reader -- a
//!   reader reads and cannot post),
//! - what it is called, and whether it is a release channel.
//!
//! **How every member can trust that.** The owner's Atlas writes the decision
//! down as a `GroupState` -- the whole membership, numbered -- and signs it
//! with its own key (`peerkey`). Every member's Atlas checks the signature and
//! takes only a newer number, so a stale list can't be replayed and no member
//! can forge one. The group's id is made from the owner's key
//! (`og-<fingerprint>-<random>`), so nobody else can publish a list for it: a
//! list signed by any other key for that id doesn't match the id and is
//! refused. People are named by their keys, which every Atlas learns from its
//! paired devices over the pairing channel (`/hello`), so "Sam" means the same
//! person on every member's Atlas even though each of them may call Sam
//! something different.
//!
//! What a member's Atlas enforces with that list: a message from someone not
//! on it, or on it as a reader, is not filed; you can't post where you are a
//! reader; and being taken off the list closes the group on your end.

use crate::peerkey::{self, Identity};
use crate::store::Store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The signed-state format this build reads and writes.
pub const FORMAT: u32 = 1;
/// Keeps a group state signature from ever being replayed as anything else.
pub const DOMAIN: &[u8] = b"atlas-group-state-v1\n";
/// Larger than any honest group's state.
pub const MAX_STATE_BYTES: usize = 32 * 1024;
pub const MAX_SEATS: usize = 64;
const FILE: &str = "owned_groups";
/// Messages held because their group's list hasn't arrived yet.
const MAX_WAITING: usize = 200;

/// What someone may do in a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Made it; decides who's in it and what each may do. Exactly one.
    Owner,
    /// Reads and posts.
    Member,
    /// Reads only. Everyone but the owner, in a release channel.
    Reader,
}

impl Role {
    pub fn may_post(self) -> bool {
        matches!(self, Role::Owner | Role::Member)
    }
    pub fn plain(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Member => "member",
            Role::Reader => "reader (can read, can't post)",
        }
    }
    fn read(s: &str) -> Option<Role> {
        match s.trim().to_lowercase().as_str() {
            "member" | "members" | "poster" => Some(Role::Member),
            "reader" | "readers" | "read only" | "read-only" => Some(Role::Reader),
            _ => None,
        }
    }
}

/// One person in a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seat {
    /// Their Atlas's public key: who they are, the same on every member's Atlas.
    pub key: String,
    /// What the owner calls them. Shown only where your own Atlas has no name
    /// of its own for them.
    pub name: String,
    pub role: Role,
}

/// The whole group, as its owner decided it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupState {
    pub format: u32,
    pub group_id: String,
    /// The owner's public key. The id is made from it.
    pub owner: String,
    pub name: String,
    /// Goes up by one with every change. Only a higher one is ever taken.
    pub version: u64,
    pub seats: Vec<Seat>,
    /// A release channel: Atlas watches it for signed updates (`update_courier`).
    #[serde(default)]
    pub release_channel: bool,
    /// The owner's other devices, each vouched for by the owner's key: any of
    /// them may change the group too (`peerkey::Delegation`).
    #[serde(default)]
    pub delegates: Vec<peerkey::Delegation>,
}

impl GroupState {
    /// May this key change the group: the owner, or a device the owner vouched for.
    pub fn may_change(&self, key: &str) -> bool {
        key == self.owner || self.delegates.iter().any(|d| d.device == key && peerkey::verify_delegation(&self.owner, d))
    }
    pub fn seat(&self, key: &str) -> Option<&Seat> {
        self.seats.iter().find(|s| s.key == key)
    }
    pub fn role_of(&self, key: &str) -> Option<Role> {
        self.seat(key).map(|s| s.role)
    }
    /// Is this one of the owner's other devices, vouched for by the owner's key?
    pub fn is_delegate(&self, key: &str) -> bool {
        key != self.owner && self.delegates.iter().any(|d| d.device == key && peerkey::verify_delegation(&self.owner, d))
    }
    /// Who this key speaks as in the group: its seat, or -- for a device the
    /// owner vouched for, like the owner's phone -- the owner. Your phone is
    /// you, not a second person in the group.
    pub fn speaks_as(&self, key: &str) -> Option<(String, Role)> {
        if let Some(role) = self.role_of(key) {
            return Some((key.to_string(), role));
        }
        self.is_delegate(key).then(|| (self.owner.clone(), Role::Owner))
    }
}

/// A group state and its owner's signature, exactly as signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signed {
    /// The state's JSON, byte for byte as signed. Never re-encoded.
    pub state: String,
    pub signature: String,
    /// Which key signed it, when not the owner's own: one of the owner's
    /// vouched-for devices. Empty means the owner.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signer: String,
}

/// A fresh group id for a group this key owns.
fn new_group_id(owner: &str) -> Option<String> {
    use chacha20poly1305::aead::rand_core::RngCore;
    let fp = peerkey::fingerprint(owner)?;
    let mut r = [0u8; 8];
    chacha20poly1305::aead::OsRng.fill_bytes(&mut r);
    Some(format!("og-{fp}-{}", r.iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

/// Is this the id of a group with an owner? Strict, so no older group's id
/// can be mistaken for one.
pub fn is_owned_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("og-") else { return false };
    let Some((fp, r)) = rest.split_once('-') else { return false };
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    hex(fp, 32) && hex(r, 16)
}

/// Everything that must be true of a state for anyone to take it.
fn check(s: &GroupState) -> Result<(), String> {
    if s.format != FORMAT {
        return Err(format!("it's written in a group format this Atlas doesn't read ({})", s.format));
    }
    if !is_owned_id(&s.group_id) {
        return Err("its id isn't an owned group's".into());
    }
    let fp = peerkey::fingerprint(&s.owner).ok_or("its owner isn't a real key")?;
    if s.group_id.get(3..35) != Some(fp.as_str()) {
        return Err("its id doesn't belong to the key that signed it".into());
    }
    if s.name.trim().is_empty() || s.name.chars().count() > 80 {
        return Err("its name must be 1 to 80 characters".into());
    }
    if s.seats.len() > MAX_SEATS {
        return Err(format!("it has more than {MAX_SEATS} people in it"));
    }
    let owners: Vec<&Seat> = s.seats.iter().filter(|x| x.role == Role::Owner).collect();
    if owners.len() != 1 || owners[0].key != s.owner {
        return Err("it must have exactly one owner, and that must be whoever signed it".into());
    }
    let mut vouched: Vec<&str> = Vec::new();
    for d in &s.delegates {
        if !peerkey::verify_delegation(&s.owner, d) {
            return Err("it names a device its owner didn't vouch for".into());
        }
        if vouched.contains(&d.device.as_str()) {
            return Err("it names one of its owner's devices twice".into());
        }
        vouched.push(&d.device);
    }
    let mut seen: Vec<&str> = Vec::new();
    for seat in &s.seats {
        if !peerkey::is_public_key(&seat.key) {
            return Err(format!("{} isn't named by a real key", seat.name));
        }
        if seen.contains(&seat.key.as_str()) {
            return Err(format!("{} is in it twice", seat.name));
        }
        if seat.name.chars().count() > 80 {
            return Err("a name in it is too long".into());
        }
        seen.push(&seat.key);
    }
    Ok(())
}

/// Read a signed state and check its signature and its rules.
///
/// The state has to be parsed to learn whose key to check it against; it is
/// size-capped and strict (unknown fields refused) before that, and nothing
/// is taken until the signature holds.
pub fn open(signed: &Signed) -> Result<GroupState, String> {
    if signed.state.len() > MAX_STATE_BYTES {
        return Err("it's larger than any group's list could be".into());
    }
    let s: GroupState = serde_json::from_str(&signed.state).map_err(|e| format!("it doesn't read ({e})"))?;
    let signer = if signed.signer.is_empty() { s.owner.as_str() } else { signed.signer.as_str() };
    if !s.may_change(signer) {
        return Err("it's signed by a key that can't change this group".into());
    }
    if !peerkey::verify(signer, DOMAIN, signed.state.as_bytes(), &signed.signature) {
        return Err("its signature doesn't hold".into());
    }
    check(&s)?;
    Ok(s)
}

fn seal(me: &Identity, s: &GroupState) -> Result<Signed, String> {
    check(s)?;
    let state = serde_json::to_string(s).map_err(|e| e.to_string())?;
    let signature = me.sign(DOMAIN, state.as_bytes());
    let signer = if me.public() == s.owner { String::new() } else { me.public() };
    Ok(Signed { state, signature, signer })
}

/// A group this Atlas holds the list for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Held {
    pub signed: Signed,
    pub state: GroupState,
}

/// A group message that arrived before its group's list did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waiting {
    pub group_id: String,
    pub from: String,
    pub body: String,
    pub sent_at: u64,
    pub sent_offset_mins: i16,
    pub after: u64,
    pub id: String,
    pub held_at: u64,
    /// Passed on by the owner for this member, by key (`kin::Chatted`).
    #[serde(default)]
    pub on_behalf_of: Option<String>,
}

/// What taking a state did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// A group this Atlas didn't know.
    New,
    /// A newer list for a group it did.
    Newer,
    /// Not newer than what it has. Ignored.
    NotNewer,
}

/// Every owned group this Atlas knows, and what it still owes whom.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Groups {
    pub held: BTreeMap<String, Held>,
    #[serde(default)]
    pub waiting: Vec<Waiting>,
    /// Which version of each group each person's Atlas has been given, by
    /// their key. What the owner's Atlas uses to know who still needs the
    /// latest list.
    #[serde(default)]
    pub delivered: BTreeMap<String, BTreeMap<String, u64>>,
    /// People taken out of a group who haven't been told yet: (group, key).
    #[serde(default)]
    pub removed_owed: Vec<(String, String)>,
    /// Members' messages the owner's Atlas still has to pass on to others.
    #[serde(default)]
    pub relay_owed: Vec<Relay>,
    /// Your other devices' keys, learned through sealed household sync. Every
    /// group you make vouches for them, so any of your devices can manage it.
    #[serde(default)]
    pub my_devices: Vec<String>,
}

/// A member's message the owner's Atlas passes on to another member.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relay {
    pub group_id: String,
    /// Who it goes to, by key.
    pub to: String,
    /// Who wrote it, by key.
    pub author: String,
    pub id: String,
    pub body: String,
    pub sent_at: u64,
    pub sent_offset_mins: i16,
    pub after: u64,
}

/// More than a busy group's backlog; the oldest goes first.
const MAX_RELAYS: usize = 500;

impl Groups {
    pub fn load(store: &Store) -> Groups {
        store.load(FILE)
    }
    pub fn save(&self, store: &Store) -> crate::error::Result<()> {
        store.save(FILE, self)
    }

    /// Take a signed list from someone. Only a newer one for a group already
    /// held, and only from the same owner.
    pub fn take(&mut self, signed: Signed) -> Result<Taken, String> {
        let s = open(&signed)?;
        if let Some(h) = self.held.get(&s.group_id) {
            if h.state.owner != s.owner {
                return Err("it claims a group somebody else owns".into());
            }
            if s.version <= h.state.version {
                return Ok(Taken::NotNewer);
            }
            self.held.insert(s.group_id.clone(), Held { signed, state: s });
            return Ok(Taken::Newer);
        }
        self.held.insert(s.group_id.clone(), Held { signed, state: s });
        Ok(Taken::New)
    }

    /// Hold a message until its group's list arrives. Oldest dropped past a cap.
    pub fn hold(&mut self, w: Waiting) {
        if self.waiting.iter().any(|x| x.id == w.id) {
            return;
        }
        self.waiting.push(w);
        if self.waiting.len() > MAX_WAITING {
            self.waiting.remove(0);
        }
    }

    /// The held messages for a group, taken out to be filed now.
    pub fn release_waiting(&mut self, group_id: &str) -> Vec<Waiting> {
        let (mine, rest): (Vec<Waiting>, Vec<Waiting>) =
            std::mem::take(&mut self.waiting).into_iter().partition(|w| w.group_id == group_id);
        self.waiting = rest;
        mine
    }

    /// Make a group. You are its owner; `people` are (their key, what you
    /// call them, role). For a release channel, everyone is a reader.
    pub fn create(
        &mut self,
        me: &Identity,
        name: &str,
        people: &[(String, String, Role)],
        release_channel: bool,
    ) -> Result<String, String> {
        let owner = me.public();
        let id = new_group_id(&owner).ok_or("couldn't make an id")?;
        let mut seats = vec![Seat { key: owner.clone(), name: "me".into(), role: Role::Owner }];
        for (key, who, role) in people {
            if *key == owner {
                continue;
            }
            let role = if release_channel { Role::Reader } else { *role };
            seats.push(Seat { key: key.clone(), name: who.clone(), role });
        }
        let s = GroupState {
            format: FORMAT,
            group_id: id.clone(),
            owner,
            name: name.trim().to_string(),
            version: 1,
            seats,
            release_channel,
            delegates: self.my_devices.iter().filter(|d| **d != me.public()).map(|d| me.delegate(d)).collect(),
        };
        let signed = seal(me, &s)?;
        self.held.insert(id.clone(), Held { signed, state: s });
        Ok(id)
    }

    /// Change a group you own. Everything goes through here, so every change
    /// is numbered, checked and signed the same way.
    fn change(
        &mut self,
        me: &Identity,
        group_id: &str,
        f: impl FnOnce(&mut GroupState) -> Result<String, String>,
    ) -> Result<String, String> {
        let h = self.held.get(group_id).ok_or("there's no such group")?;
        if !h.state.may_change(&me.public()) {
            return Err(format!("only whoever made \"{}\" can change who's in it", h.state.name));
        }
        let mut s = h.state.clone();
        let said = f(&mut s)?;
        s.version += 1;
        let signed = seal(me, &s)?;
        self.held.insert(group_id.to_string(), Held { signed, state: s });
        Ok(said)
    }

    pub fn add(&mut self, me: &Identity, group_id: &str, key: &str, name: &str, role: Role) -> Result<String, String> {
        self.change(me, group_id, |s| {
            if s.seat(key).is_some() {
                return Err(format!("{name} is already in it"));
            }
            let role = if s.release_channel { Role::Reader } else { role };
            s.seats.push(Seat { key: key.to_string(), name: name.to_string(), role });
            Ok(format!("Added {name} to \"{}\" as a {}.", s.name, role.plain()))
        })
    }

    pub fn remove(&mut self, me: &Identity, group_id: &str, key: &str) -> Result<String, String> {
        let said = self.change(me, group_id, |s| {
            let seat = s.seat(key).cloned().ok_or("they aren't in it")?;
            if seat.role == Role::Owner {
                return Err("you can't take yourself out of a group you own".into());
            }
            s.seats.retain(|x| x.key != key);
            Ok(format!("Took {} out of \"{}\".", seat.name, s.name))
        })?;
        self.removed_owed.push((group_id.to_string(), key.to_string()));
        Ok(said)
    }

    pub fn set_role(&mut self, me: &Identity, group_id: &str, key: &str, role: Role) -> Result<String, String> {
        self.change(me, group_id, |s| {
            if s.release_channel && role == Role::Member {
                return Err("in a release channel only you post".into());
            }
            let name = s.name.clone();
            let seat = s.seats.iter_mut().find(|x| x.key == key).ok_or("they aren't in it")?;
            if seat.role == Role::Owner {
                return Err("you're the owner; that doesn't change".into());
            }
            seat.role = role;
            Ok(format!("{} is now a {} in \"{name}\".", seat.name, role.plain()))
        })
    }

    pub fn rename(&mut self, me: &Identity, group_id: &str, name: &str) -> Result<String, String> {
        self.change(me, group_id, |s| {
            s.name = name.trim().to_string();
            Ok(format!("Renamed it \"{}\".", s.name))
        })
    }

    /// Vouch for another device of yours in every group you own, so it can
    /// change them too. Only the owner's own key vouches -- a vouched-for
    /// device can't vouch for more. Returns how many groups changed.
    pub fn vouch_everywhere(&mut self, me: &Identity, device: &str) -> usize {
        let mine: Vec<String> = self
            .held
            .iter()
            .filter(|(_, h)| h.state.owner == me.public() && !h.state.delegates.iter().any(|d| d.device == device))
            .map(|(id, _)| id.clone())
            .collect();
        let d = me.delegate(device);
        let mut n = 0;
        for id in mine {
            let d = d.clone();
            if self.change(me, &id, move |s| {
                s.delegates.push(d);
                Ok(String::new())
            })
            .is_ok()
            {
                n += 1;
            }
        }
        n
    }

    /// A group by the name it goes by, if exactly one.
    pub fn named(&self, name: &str) -> Option<&Held> {
        let n = name.trim().trim_start_matches("the ").trim_end_matches(" group").trim();
        let hits: Vec<&Held> = self.held.values().filter(|h| h.state.name.eq_ignore_ascii_case(n)).collect();
        (hits.len() == 1).then(|| hits[0])
    }

    /// Who the owner's Atlas still owes the latest list to: (group id, key).
    /// Only for groups `me` owns. Includes people just taken out -- they need
    /// the list that no longer has them in it, or their Atlas would keep the
    /// group open.
    pub fn owed(&self, me: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (gid, h) in self.held.iter().filter(|(_, h)| h.state.may_change(me)) {
            // Everyone seated, and the owner's other devices: your phone
            // gets the list over its pairing as well as through sync.
            let keys = h.state.seats.iter().map(|s| s.key.as_str()).chain(h.state.delegates.iter().map(|d| d.device.as_str()));
            for key in keys.filter(|k| *k != me) {
                let had = self.delivered.get(key).and_then(|m| m.get(gid)).copied().unwrap_or(0);
                if had < h.state.version {
                    out.push((gid.clone(), key.to_string()));
                }
            }
        }
        for (gid, key) in &self.removed_owed {
            if !out.contains(&(gid.clone(), key.clone())) {
                out.push((gid.clone(), key.clone()));
            }
        }
        out
    }

    /// As the owner: queue a member's message for everyone else in the group
    /// except its author, the device that handed it over, and you -- your own
    /// other devices included, so your phone hears the group too.
    pub fn relay(&mut self, me: &str, group_id: &str, author: &str, sent_by: &str, w: &Waiting) {
        let Some(h) = self.held.get(group_id) else { return };
        if h.state.owner != me {
            // Only the seat that is the owner relays -- the one every member
            // is paired with and believes when it speaks for someone.
            return;
        }
        let targets: Vec<String> = h
            .state
            .seats
            .iter()
            .map(|s| s.key.clone())
            .chain(h.state.delegates.iter().map(|d| d.device.clone()))
            .filter(|k| k != me && k != author && k != sent_by)
            .collect();
        for to in targets {
            if self.relay_owed.iter().any(|r| r.id == w.id && r.to == to) {
                continue;
            }
            self.relay_owed.push(Relay {
                group_id: group_id.to_string(),
                to,
                author: author.to_string(),
                id: w.id.clone(),
                body: w.body.clone(),
                sent_at: w.sent_at,
                sent_offset_mins: w.sent_offset_mins,
                after: w.after,
            });
        }
        while self.relay_owed.len() > MAX_RELAYS {
            self.relay_owed.remove(0);
        }
    }

    /// Their Atlas took the list.
    pub fn delivered_to(&mut self, group_id: &str, key: &str) {
        let v = self.held.get(group_id).map(|h| h.state.version).unwrap_or(0);
        self.delivered.entry(key.to_string()).or_default().insert(group_id.to_string(), v);
        self.removed_owed.retain(|(g, k)| !(g == group_id && k == key));
    }
}

// ------------------------------------------------------------------ for people

/// One group as a person sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub id: String,
    pub name: String,
    /// You made it, so you can change it.
    pub mine: bool,
    pub release_channel: bool,
    pub owner: String,
    /// (your name for them, or the owner's; their role; their key)
    pub seats: Vec<(String, Role, String)>,
}

/// Every owned group, in words: names from your own pairings where you have
/// them. And the people you could add: paired, and introduced (keyed).
pub fn views(store: &Store, peer_dir: &std::path::Path) -> (Vec<View>, Vec<String>) {
    let groups = Groups::load(store);
    let pairings = crate::kin::Pairings::load(peer_dir);
    let me = Identity::load_or_create(peer_dir).ok().map(|i| i.public());
    let named = |s: &Seat| -> String {
        if Some(&s.key) == me.as_ref() {
            "you".into()
        } else {
            pairings.name_of_key(&s.key).unwrap_or_else(|| s.name.clone())
        }
    };
    let views = groups
        .held
        .values()
        .map(|h| View {
            id: h.state.group_id.clone(),
            name: h.state.name.clone(),
            mine: me.as_deref().is_some_and(|k| h.state.may_change(k)),
            release_channel: h.state.release_channel,
            owner: h.state.seat(&h.state.owner).map(named).unwrap_or_default(),
            seats: h.state.seats.iter().map(|s| (named(s), s.role, s.key.clone())).collect(),
        })
        .collect();
    let addable = pairings.peers.iter().filter(|p| p.key.is_some()).map(|p| p.name.clone()).collect();
    (views, addable)
}

/// Read a spoken group change: (what, who, group, role) for `act`.
///
/// "add Sam to the Friends group" / "add Sam to Friends as a reader"
/// "take Sam out of the Friends group" / "remove Sam from the Friends group"
/// "make Maya a reader in the Friends group" / "make Maya a member of Friends"
/// "let Maya post in the Friends group"
pub fn read_spoken(said: &str) -> Option<(&'static str, String, String, String)> {
    let t: String = said.trim().trim_end_matches(['.', '!', '?']).to_string();
    let lower = t.to_lowercase();
    let group_of = |g: &str| -> Option<String> {
        let g = g.trim();
        let g = if g.to_lowercase().starts_with("the ") { &g[4..] } else { g };
        let g = g.strip_suffix(" group").unwrap_or(g).trim();
        (!g.is_empty()).then(|| g.to_string())
    };
    let cut = |from: usize, sep: &str| -> Option<(String, String)> {
        let rest = &t[from..];
        let at = rest.to_lowercase().find(sep)?;
        let who = rest[..at].trim().to_string();
        let tail = rest[at + sep.len()..].trim().to_string();
        // A person is a name, not a sentence: "add milk to the list" has one
        // word too, which is why the group has to be one you have (`act`).
        (!who.is_empty() && who.split_whitespace().count() <= 3).then_some((who, tail))
    };
    if let Some(from) = lower.strip_prefix("add ").map(|_| 4) {
        let (who, tail) = cut(from, " to ")?;
        let (group, role) = match tail.to_lowercase().find(" as a") {
            Some(at) => (tail[..at].to_string(), tail[at..].to_lowercase()),
            None => (tail.clone(), String::new()),
        };
        let role = if role.contains("reader") { "reader" } else { "" };
        return Some(("add", who, group_of(&group)?, role.into()));
    }
    if lower.starts_with("take ") {
        let (who, tail) = cut(5, " out of ")?;
        return Some(("remove", who, group_of(&tail)?, String::new()));
    }
    if lower.starts_with("remove ") {
        let (who, tail) = cut(7, " from ")?;
        return Some(("remove", who, group_of(&tail)?, String::new()));
    }
    if lower.starts_with("make ") {
        for (sep, role) in [(" a reader in ", "reader"), (" a reader of ", "reader"), (" a member of ", "member"), (" a member in ", "member")] {
            if let Some((who, tail)) = cut(5, sep) {
                return Some(("role", who, group_of(&tail)?, role.into()));
            }
        }
        return None;
    }
    if lower.starts_with("let ") {
        let (who, tail) = cut(4, " post in ")?;
        return Some(("role", who, group_of(&tail)?, "member".into()));
    }
    None
}

// ------------------------------------------------------------------ your other devices

/// Sync subjects: a group's signed list, and one of your devices' keys.
pub const SYNC_GROUP: &str = "group:";
pub const SYNC_DEVICE: &str = "device:";
const SYNC_TOLD: &str = "group_sync_told";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Told {
    versions: BTreeMap<String, u64>,
    device: String,
}

/// What your other devices haven't been told yet: every group list newer
/// than the one last carried, and this device's own key. As (subject, field,
/// value) for `sync::What::Changed`.
pub fn changes_to_carry(store: &Store, my_key: &str) -> Vec<(String, String, String)> {
    let groups = Groups::load(store);
    let mut told: Told = store.load(SYNC_TOLD);
    let mut out = Vec::new();
    for (gid, h) in &groups.held {
        if told.versions.get(gid).copied().unwrap_or(0) < h.state.version {
            out.push((format!("{SYNC_GROUP}{gid}"), "state".into(), serde_json::to_string(&h.signed).unwrap_or_default()));
            told.versions.insert(gid.clone(), h.state.version);
        }
    }
    if told.device != my_key && !my_key.is_empty() {
        out.push((format!("{SYNC_DEVICE}{my_key}"), "key".into(), my_key.to_string()));
        told.device = my_key.to_string();
    }
    if !out.is_empty() {
        let _ = store.save(SYNC_TOLD, &told);
    }
    out
}

/// Take in a group list or a device key from your other device.
///
/// A group list is checked by its signature, so it's taken from any bundle.
/// A device key is taken only from a bundle sealed with your household key
/// -- it becomes a device you vouch for, able to change your groups.
pub fn take_synced(store: &Store, me: Option<&Identity>, subject: &str, value: &str, sealed: bool) -> Option<String> {
    let mut groups = Groups::load(store);
    let mut told: Told = store.load(SYNC_TOLD);
    let said = if subject.starts_with(SYNC_GROUP) {
        let signed: Signed = serde_json::from_str(value).ok()?;
        let taken = groups.take(signed).ok()?;
        for (gid, h) in &groups.held {
            let v = told.versions.entry(gid.clone()).or_insert(0);
            *v = (*v).max(h.state.version);
        }
        (taken == Taken::New).then(String::new)
    } else {
        let device = subject.strip_prefix(SYNC_DEVICE)?;
        if !sealed || !peerkey::is_public_key(device) || device != value {
            return None;
        }
        let me = me?;
        if device == me.public() || groups.my_devices.iter().any(|d| d == device) {
            return None;
        }
        groups.my_devices.push(device.to_string());
        let n = groups.vouch_everywhere(me, device);
        Some(if n > 0 {
            format!("Your other device can now manage your groups too ({n} of them).")
        } else {
            String::new()
        })
    };
    groups.save(store).ok()?;
    let _ = store.save(SYNC_TOLD, &told);
    said.filter(|s| !s.is_empty())
}

/// Change groups by name, the way the hub and `atlas group` both do it.
///
/// `what`: `new` (group = its name, `who` = people separated by commas, `role`
/// = "release" for a release channel), `add`, `remove`, `role`, `rename`
/// (`role` = the new name). People are named as you paired them; each must
/// have introduced its key, which a paired Atlas does by itself the next time
/// both are running.
pub fn act(store: &Store, peer_dir: &std::path::Path, what: &str, group: &str, who: &str, role: &str) -> Result<String, String> {
    let me = Identity::load_or_create(peer_dir)?;
    let pairings = crate::kin::Pairings::load(peer_dir);
    let key_of = |name: &str| -> Result<String, String> {
        if !pairings.has_peer(name) {
            return Err(format!("you aren't paired with anyone called {name}"));
        }
        pairings.key_of(name).map(String::from).ok_or_else(|| {
            format!("{name}'s Atlas hasn't introduced itself yet -- it does that by itself the next time both are running")
        })
    };
    let mut groups = Groups::load(store);
    let id_of = |groups: &Groups, g: &str| -> Result<String, String> {
        if groups.held.contains_key(g) {
            return Ok(g.to_string());
        }
        groups.named(g).map(|h| h.state.group_id.clone()).ok_or_else(|| format!("there's no group called \"{g}\""))
    };
    let said = match what {
        "new" => {
            if group.trim().is_empty() {
                return Err("what should the group be called?".into());
            }
            if groups.named(group).is_some() {
                return Err(format!("you already have a group called \"{}\"", group.trim()));
            }
            let mut people = Vec::new();
            for n in who.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                people.push((key_of(n)?, n.to_string(), Role::Member));
            }
            let release = role.trim().eq_ignore_ascii_case("release");
            groups.create(&me, group, &people, release)?;
            if release {
                format!("Made the release channel \"{}\". Only you can post in it.", group.trim())
            } else {
                format!("Made \"{}\". You're its owner: you decide who's in it.", group.trim())
            }
        }
        "add" => {
            let id = id_of(&groups, group)?;
            let r = Role::read(role).unwrap_or(Role::Member);
            groups.add(&me, &id, &key_of(who)?, who, r)?
        }
        "remove" => {
            let id = id_of(&groups, group)?;
            // Someone you've since un-paired can still be taken out, by the
            // name the list knows them by.
            let key = match key_of(who) {
                Ok(k) => k,
                Err(e) => groups
                    .held
                    .get(&id)
                    .and_then(|h| h.state.seats.iter().find(|s| s.name.eq_ignore_ascii_case(who)).map(|s| s.key.clone()))
                    .ok_or(e)?,
            };
            groups.remove(&me, &id, &key)?
        }
        "role" => {
            let id = id_of(&groups, group)?;
            let r = Role::read(role).ok_or_else(|| format!("\"{role}\" isn't a role -- member or reader"))?;
            groups.set_role(&me, &id, &key_of(who)?, r)?
        }
        "rename" => {
            let id = id_of(&groups, group)?;
            groups.rename(&me, &id, role)?
        }
        other => return Err(format!("I don't know how to {other} a group")),
    };
    groups.save(store).map_err(|e| format!("couldn't save it: {e}"))?;
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eric() -> Identity {
        Identity::from_seed_for_test([1; 32])
    }
    fn sam() -> Identity {
        Identity::from_seed_for_test([2; 32])
    }
    fn maya() -> Identity {
        Identity::from_seed_for_test([3; 32])
    }

    fn with_sam() -> (Groups, String) {
        let mut g = Groups::default();
        let id = g.create(&eric(), "Friends", &[(sam().public(), "Sam".into(), Role::Member)], false).unwrap();
        (g, id)
    }

    #[test]
    fn a_group_id_is_bound_to_its_owners_key() {
        let (g, id) = with_sam();
        assert!(is_owned_id(&id));
        assert!(!is_owned_id("og-sam+tom"));
        assert!(!is_owned_id("a1b2c3"));
        assert_eq!(g.held[&id].state.role_of(&eric().public()), Some(Role::Owner));
        assert_eq!(g.held[&id].state.role_of(&sam().public()), Some(Role::Member));
    }

    #[test]
    fn a_member_takes_the_owners_list_and_only_a_newer_one() {
        let (mut owner, id) = with_sam();
        let mut member = Groups::default();
        assert_eq!(member.take(owner.held[&id].signed.clone()), Ok(Taken::New));
        let v1 = owner.held[&id].signed.clone();
        owner.add(&eric(), &id, &maya().public(), "Maya", Role::Reader).unwrap();
        assert_eq!(member.take(owner.held[&id].signed.clone()), Ok(Taken::Newer));
        assert_eq!(member.take(v1), Ok(Taken::NotNewer), "an old list replayed is ignored");
        assert_eq!(member.held[&id].state.role_of(&maya().public()), Some(Role::Reader));
    }

    #[test]
    fn nobody_but_the_owner_can_write_the_list() {
        let (owner, id) = with_sam();
        let mut member = Groups::default();
        member.take(owner.held[&id].signed.clone()).unwrap();

        // Sam edits the list and signs it himself.
        let mut forged = owner.held[&id].state.clone();
        forged.version += 1;
        forged.seats.push(Seat { key: maya().public(), name: "Maya".into(), role: Role::Member });
        let text = serde_json::to_string(&forged).unwrap();
        let sig = sam().sign(DOMAIN, text.as_bytes());
        assert!(member.take(Signed { state: text.clone(), signature: sig, signer: String::new() }).is_err());

        // Sam claims to be the owner of the same id.
        let mut claimed = forged.clone();
        claimed.owner = sam().public();
        claimed.seats[0].key = sam().public();
        let text = serde_json::to_string(&claimed).unwrap();
        let sig = sam().sign(DOMAIN, text.as_bytes());
        assert!(member.take(Signed { state: text, signature: sig, signer: String::new() }).is_err(), "the id is Eric's");

        // Tampered after signing.
        let good = owner.held[&id].signed.clone();
        let tampered = Signed { state: good.state.replace("Friends", "Enemies"), signature: good.signature, signer: String::new() };
        assert!(member.take(tampered).is_err());

        // Sam can't change it through the owner's functions either.
        let mut copy = member.clone();
        assert!(copy.add(&sam(), &id, &maya().public(), "Maya", Role::Member).is_err());
    }

    #[test]
    fn the_owner_adds_removes_and_changes_roles_and_everyone_removed_is_told() {
        let (mut g, id) = with_sam();
        g.add(&eric(), &id, &maya().public(), "Maya", Role::Member).unwrap();
        assert!(g.add(&eric(), &id, &maya().public(), "Maya", Role::Member).is_err());
        g.set_role(&eric(), &id, &maya().public(), Role::Reader).unwrap();
        assert_eq!(g.held[&id].state.role_of(&maya().public()), Some(Role::Reader));
        assert!(g.set_role(&eric(), &id, &eric().public(), Role::Reader).is_err());
        assert!(g.remove(&eric(), &id, &eric().public()).is_err(), "the owner stays");
        g.remove(&eric(), &id, &sam().public()).unwrap();
        assert!(g.held[&id].state.seat(&sam().public()).is_none());
        let owed = g.owed(&eric().public());
        assert!(owed.contains(&(id.clone(), sam().public())), "Sam must be told he's out");
        assert!(owed.contains(&(id.clone(), maya().public())));
        g.delivered_to(&id, &sam().public());
        g.delivered_to(&id, &maya().public());
        assert!(g.owed(&eric().public()).is_empty());
        assert_eq!(g.held[&id].state.version, 4, "refused changes are not numbered");
    }

    #[test]
    fn your_other_device_can_change_your_group_once_you_vouch_for_it() {
        let phone = Identity::from_seed_for_test([9; 32]);
        let (mut g, id) = with_sam();
        let mut member = Groups::default();
        member.take(g.held[&id].signed.clone()).unwrap();
        // Before vouching: the phone can't.
        assert!(g.clone().add(&phone, &id, &maya().public(), "Maya", Role::Member).is_err());
        assert_eq!(g.vouch_everywhere(&eric(), &phone.public()), 1);
        g.add(&phone, &id, &maya().public(), "Maya", Role::Member).unwrap();
        assert_eq!(member.take(g.held[&id].signed.clone()), Ok(Taken::Newer), "a member refused the phone's change");
        // The phone can't vouch for anything itself, and Sam can't sneak in as a device.
        assert_eq!(g.vouch_everywhere(&phone, &sam().public()), 0);
        let mut forged = g.held[&id].state.clone();
        forged.version += 1;
        forged.delegates.push(sam().delegate(&sam().public()));
        let text = serde_json::to_string(&forged).unwrap();
        let sig = sam().sign(DOMAIN, text.as_bytes());
        assert!(member.take(Signed { state: text, signature: sig, signer: sam().public() }).is_err());
    }

    fn store(tag: &str) -> Store {
        let d = std::env::temp_dir().join(format!("atlas-groups-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Store::new(d)
    }

    #[test]
    fn your_phone_learns_your_groups_and_is_vouched_for_only_through_sealed_sync() {
        let (laptop, phone) = (store("sync-laptop"), store("sync-phone"));
        let (g, id) = with_sam();
        g.save(&laptop).unwrap();
        let phone_key = Identity::from_seed_for_test([9; 32]);

        // The laptop's lists reach the phone (checked by signature, sealed or not).
        for (sub, _, val) in changes_to_carry(&laptop, &eric().public()) {
            take_synced(&phone, Some(&phone_key), &sub, &val, false);
        }
        assert!(Groups::load(&phone).held.contains_key(&id));

        // The phone's key reaches the laptop: ignored unsealed, vouched for sealed.
        let from_phone = changes_to_carry(&phone, &phone_key.public());
        let device: Vec<_> = from_phone.iter().filter(|(s, _, _)| s.starts_with(SYNC_DEVICE)).collect();
        assert_eq!(device.len(), 1);
        let (sub, _, val) = device[0];
        assert!(take_synced(&laptop, Some(&eric()), sub, val, false).is_none());
        assert!(!Groups::load(&laptop).held[&id].state.may_change(&phone_key.public()));
        assert!(take_synced(&laptop, Some(&eric()), sub, val, true).is_some());
        assert!(Groups::load(&laptop).held[&id].state.may_change(&phone_key.public()));

        // The vouched list goes back to the phone, which can now change the group.
        for (sub, _, val) in changes_to_carry(&laptop, &eric().public()) {
            take_synced(&phone, Some(&phone_key), &sub, &val, true);
        }
        let mut on_phone = Groups::load(&phone);
        on_phone.add(&phone_key, &id, &maya().public(), "Maya", Role::Member).unwrap();
        // Groups made later vouch for the phone from the start.
        let mut later = Groups::load(&laptop);
        let id2 = later.create(&eric(), "Later", &[], false).unwrap();
        assert!(later.held[&id2].state.may_change(&phone_key.public()));
    }

    #[test]
    fn in_a_release_channel_only_the_owner_posts() {
        let mut g = Groups::default();
        let id = g.create(&eric(), "Atlas updates", &[(sam().public(), "Sam".into(), Role::Member)], true).unwrap();
        assert_eq!(g.held[&id].state.role_of(&sam().public()), Some(Role::Reader));
        assert!(g.set_role(&eric(), &id, &sam().public(), Role::Member).is_err());
        g.add(&eric(), &id, &maya().public(), "Maya", Role::Member).unwrap();
        assert_eq!(g.held[&id].state.role_of(&maya().public()), Some(Role::Reader));
        assert!(!Role::Reader.may_post() && Role::Member.may_post() && Role::Owner.may_post());
    }

    #[test]
    fn spoken_group_changes_are_read_and_other_sentences_are_not() {
        assert_eq!(read_spoken("add Sam to the Friends group"), Some(("add", "Sam".into(), "Friends".into(), "".into())));
        assert_eq!(read_spoken("Add Maya to Friends as a reader."), Some(("add", "Maya".into(), "Friends".into(), "reader".into())));
        assert_eq!(read_spoken("take Sam out of the Friends group"), Some(("remove", "Sam".into(), "Friends".into(), "".into())));
        assert_eq!(read_spoken("remove Sam from Friends"), Some(("remove", "Sam".into(), "Friends".into(), "".into())));
        assert_eq!(read_spoken("make Maya a reader in the Friends group"), Some(("role", "Maya".into(), "Friends".into(), "reader".into())));
        assert_eq!(read_spoken("let Maya post in the Friends group"), Some(("role", "Maya".into(), "Friends".into(), "member".into())));
        assert_eq!(read_spoken("make a cake"), None);
        assert_eq!(read_spoken("take a screenshot"), None);
        assert_eq!(read_spoken("add to Friends"), None);
    }

    #[test]
    fn messages_that_beat_their_list_wait_for_it() {
        let mut g = Groups::default();
        let w = |id: &str| Waiting {
            group_id: "og-x".into(),
            from: "Sam".into(),
            body: "hi".into(),
            sent_at: 1,
            sent_offset_mins: 0,
            after: 1,
            id: id.into(),
            held_at: 1,
            on_behalf_of: None,
        };
        g.hold(w("a"));
        g.hold(w("a"));
        g.hold(w("b"));
        assert_eq!(g.release_waiting("og-x").len(), 2);
        assert!(g.waiting.is_empty());
    }
}
