//! Adding a friend in one step each: you send a link, they tap it, done.
//!
//! Pairing used to be three hand-offs: type their name, send them a code,
//! they paste it and send a *second* code back, you paste that. Every app
//! people actually use is one step each side, and this is that:
//!
//! 1. **You** say "add a friend" (or press it on the hub). Atlas makes a
//!    one-time **friend link** -- your name, how to reach your Atlas, your
//!    Atlas's public key, and a one-time secret -- and you send it any way you
//!    like: a text, a QR code, in person.
//! 2. **They** open it in their Atlas and press **Add**. Their Atlas pairs
//!    with yours and tells yours in the same moment, carrying the one-time
//!    secret. Your Atlas recognises its own invitation and completes the pair.
//!    You chose them when you sent the link; nobody is asked twice, and
//!    nobody waits for a confirmation.
//!
//! And from inside a group: tap someone you're in a group with and **send a
//! friend request**. It travels through the group's owner to them alone, and
//! they accept with one press -- underneath, it is the same friend link.
//!
//! **Why this is safe with nothing in the middle.** The secret is 128 random
//! bits, good for one use and seven days. Only whoever holds the link can use
//! it, so sending it is the decision. Both Atlases pin each other's public key
//! from the exchange, so later messages are checked against the key that was
//! in the link you sent -- a link copied by someone else is used up the moment
//! your friend uses it, and a used one is refused.
//!
//! **How their Atlas reaches yours -- with nothing in the middle.** Tailscale
//! may join *your own* devices together; it never joins yours to a friend's,
//! and there's no server and no friend holding anyone's messages (Eric,
//! 25 Sep). The link carries your Atlas's onion address, reachable through Tor
//! from anywhere it's online (`onion`), and its home address for a friend on
//! the same wifi. Everything between you is sealed by Atlas itself (`wire`).
//! When your friend's Atlas is off, yours keeps what it was sending and sends
//! it when theirs is back.

use serde::{Deserialize, Serialize};

/// How a friend link starts.
pub const PREFIX: &str = "atlas-friend:";
/// A friend request sent through a group, inside a message: `PREFIX_REQ`
/// + the target's key + ":" + a friend link.
pub const REQUEST_PREFIX: &str = "atlas-friend-request:";
const INVITES: &str = "friend_invites";
const REQUESTS: &str = "friend_requests";
/// How long a friend link stays good.
pub const LINK_DAYS: u64 = 7;
/// More outstanding links than anyone needs; the oldest go first.
const MAX_INVITES: usize = 50;

/// What a friend link carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// What they'll see you as. Only what you call yourself.
    pub name: String,
    /// Your Atlas's public key (`peerkey`), pinned by theirs -- and the key
    /// their first knock is sealed to.
    pub key: String,
    /// The one-time secret.
    pub invite: String,
    /// Every way your Atlas can be reached, and friends of yours that hold
    /// your mail when it can't be (`kin::Routes`).
    pub routes: crate::kin::Routes,
}

fn url_safe(b: &[u8]) -> String {
    crate::b64::encode(b).replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
}

fn from_url_safe(s: &str) -> Option<Vec<u8>> {
    let mut t = s.trim().replace('-', "+").replace('_', "/");
    while t.len() % 4 != 0 {
        t.push('=');
    }
    crate::b64::decode(&t).ok()
}

impl Link {
    pub fn encode(&self) -> String {
        format!("{PREFIX}{}", url_safe(serde_json::to_string(self).unwrap_or_default().as_bytes()))
    }

    /// Read a link, from wherever in a pasted message it sits.
    pub fn decode(text: &str) -> Result<Link, String> {
        let at = text.find(PREFIX).ok_or("that isn't an Atlas friend link")?;
        let body: String =
            text[at + PREFIX.len()..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        let bytes = from_url_safe(&body).ok_or("that friend link is damaged -- check it was copied whole")?;
        let l: Link = serde_json::from_slice(&bytes).map_err(|_| "that friend link is damaged -- check it was copied whole")?;
        if l.name.trim().is_empty() || l.name.chars().count() > 60 {
            return Err("that friend link is missing who it's from".into());
        }
        let l = Link { routes: l.routes.checked(), ..l };
        if l.routes.is_empty() {
            return Err("that friend link doesn't say how to reach its Atlas".into());
        }
        if !crate::peerkey::is_public_key(&l.key) || l.invite.len() < 16 {
            return Err("that friend link is damaged -- check it was copied whole".into());
        }
        Ok(l)
    }
}

/// A link you made and haven't had used yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub secret: String,
    pub made_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invites {
    pub open: Vec<Invite>,
}

impl Invites {
    pub fn load(store: &crate::store::Store) -> Invites {
        store.load(INVITES)
    }
    fn save(&self, store: &crate::store::Store) -> Result<(), String> {
        store.save(INVITES, self).map_err(|e| format!("couldn't save it: {e}"))
    }
}

/// Who you are, for a link: your name, your Atlas's key, and every way to
/// reach it that it worked out itself (`Daemon::keep_reach`) -- so adding a
/// friend never starts with editing config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub name: String,
    pub key: String,
    pub routes: crate::kin::Routes,
}

/// Make a one-time friend link.
pub fn make_link(store: &crate::store::Store, me: &Me, now: u64) -> Result<String, String> {
    let secret = crate::server::new_token().map_err(|e| format!("couldn't make a secure link: {e}"))?;
    let mut inv = Invites::load(store);
    inv.open.retain(|i| now.saturating_sub(i.made_at) < LINK_DAYS * 86_400);
    inv.open.push(Invite { secret: secret.clone(), made_at: now });
    while inv.open.len() > MAX_INVITES {
        inv.open.remove(0);
    }
    inv.save(store)?;
    Ok(Link { name: me.name.clone(), key: me.key.clone(), invite: secret, routes: me.routes.clone() }.encode())
}

/// Someone used one of your links. Returns `Ok` only for a secret you made,
/// within its seven days, never used before -- and uses it up.
pub fn redeem(store: &crate::store::Store, secret: &str, now: u64) -> Result<(), String> {
    let mut inv = Invites::load(store);
    let at = inv
        .open
        .iter()
        .position(|i| crate::server::token_matches(&i.secret, Some(secret)))
        .ok_or("that friend link isn't one of mine, or it's already been used")?;
    let i = inv.open.remove(at);
    inv.save(store)?;
    if now.saturating_sub(i.made_at) >= LINK_DAYS * 86_400 {
        return Err("that friend link has expired".into());
    }
    Ok(())
}

/// Your side of a friendship, as your Atlas sends it to theirs when you add
/// them from their link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    /// The one-time secret from their link.
    pub invite: String,
    pub name: String,
    pub key: String,
    /// How to reach the Atlas that's knocking.
    pub routes: crate::kin::Routes,
    /// The shared secret this pair will use from now on (one token both
    /// ways, as every pairing does).
    pub token: String,
}

/// A name nobody in your pairings has yet: "Sam", then "Sam (2)".
fn free_name(pairings: &crate::kin::Pairings, wanted: &str) -> String {
    let base = wanted.trim();
    if !pairings.has_peer(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} ({n})")).find(|n| !pairings.has_peer(n)).unwrap_or_else(|| base.to_string())
}

/// Record a friend in your pairings: they may reach you, you may reach them,
/// with their key pinned and every way to reach them kept. Returns the name
/// they're kept under.
pub fn record(pairings: &mut crate::kin::Pairings, name: &str, routes: &crate::kin::Routes, key: &str, token: &str) -> String {
    // The address the contact is made with: the first one given. The rest
    // (and the onion address) are kept beside it.
    let (host, port) = routes
        .addrs
        .first()
        .and_then(|a| crate::onion::read_addr(a))
        .map(|a| (a.ip().to_string(), a.port()))
        .unwrap_or_default();
    // Already a friend under that key: refresh where to reach them, keep the
    // name you have for them.
    if let Some(existing) = pairings.name_of_key(key) {
        pairings.contacts.retain(|c| !c.name.eq_ignore_ascii_case(&existing));
        pairings.add_contact(crate::kin::Contact { name: existing.clone(), host, port, token: token.into() });
        if let Some(p) = pairings.peers.iter_mut().find(|p| p.name.eq_ignore_ascii_case(&existing)) {
            p.token = token.to_string();
        }
        pairings.routes.insert(existing.to_lowercase(), routes.clone());
        return existing;
    }
    let name = free_name(pairings, name);
    let mut peer = crate::kin::Peer::new(&name, token);
    peer.key = Some(key.to_string());
    pairings.add_peer(peer);
    pairings.add_contact(crate::kin::Contact { name: name.clone(), host, port, token: token.into() });
    pairings.routes.insert(name.to_lowercase(), routes.clone());
    name
}

/// What friends see you as when config doesn't say: the name you log in
/// with, capitalised. Set `kin.my_name` to choose it.
pub fn my_name(configured: Option<&str>) -> String {
    if let Some(n) = configured.map(str::trim).filter(|n| !n.is_empty()) {
        return n.chars().take(60).collect();
    }
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    let user = user.trim();
    let mut c = user.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).take(60).collect(),
        None => "A friend".into(),
    }
}

const OUTBOX: &str = "friend_outbox";

/// A friend you added from their link whose Atlas you couldn't reach yet:
/// your Atlas keeps trying every few minutes until their link would have run
/// out. Nobody else holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// What they're kept as in your pairings.
    pub name: String,
    pub link: Link,
    pub hello: Hello,
    pub until: u64,
}

/// A friend request you sent through a group you don't own, owed to the
/// group's owner to pass on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentRequest {
    /// The owner, by your name for them.
    pub via: String,
    pub group_id: String,
    pub group_name: String,
    pub id: String,
    pub body: String,
    pub at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outbox {
    pub pending: Vec<Pending>,
    #[serde(default)]
    pub requests: Vec<SentRequest>,
}

impl Outbox {
    pub fn load(store: &crate::store::Store) -> Outbox {
        store.load(OUTBOX)
    }
    pub fn save(&self, store: &crate::store::Store) -> Result<(), String> {
        store.save(OUTBOX, self).map_err(|e| format!("couldn't save it: {e}"))
    }
    pub fn add(&mut self, p: Pending) {
        self.pending.retain(|x| x.hello.invite != p.hello.invite);
        self.pending.push(p);
    }
}

/// How knocking on a friend's door went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Knock {
    /// Their Atlas took the link: you're friends both ways.
    Taken,
    /// Their Atlas answered and refused it: used, expired, or not theirs.
    Refused,
    /// Nothing answered: their Atlas is off, or Tor isn't up yet here.
    Unreachable(String),
}

/// Knock on the door a friend link points at, carrying your side -- sealed to
/// the key in the link, so only the Atlas that made it can open it, and it
/// can tell the knock really comes from the key you introduce. Straight to
/// its home address when you're on the same network; otherwise through Tor
/// (`tor`: its SOCKS port) to its onion address.
pub fn knock(me: &crate::peerkey::Identity, link: &Link, hello: &Hello, tor: Option<u16>, timeout: std::time::Duration) -> Knock {
    use crate::kin::{sealed_post, Via};
    let body = serde_json::to_string(hello).unwrap_or_default();
    let mut why = String::from("nothing answered");
    let direct = link.routes.addrs.iter().map(|a| Via::Direct(a.as_str()));
    let through_tor = match (tor, link.routes.onion.is_empty()) {
        (Some(socks), false) => Some(Via::Tor { socks, onion: &link.routes.onion }),
        (None, false) => {
            why = "Tor isn't running here yet".into();
            None
        }
        _ => None,
    };
    for via in direct.chain(through_tor) {
        match sealed_post(me, &link.key, via, "/friend", "", &body, timeout) {
            Ok(r) if r.ok() => return Knock::Taken,
            Ok(_) => return Knock::Refused,
            Err(e) => why = e,
        }
    }
    Knock::Unreachable(why)
}

/// What was said about friends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spoken {
    /// "add a friend": make a link to send.
    Link,
    /// A friend link pasted in: add them.
    Add,
    /// "friend requests": what's waiting.
    Requests,
    Accept(String),
    Decline(String),
    /// "send Sam a friend request": through a group you share.
    Request(String),
}

/// Read a sentence about friends. Only the shapes below; anything else is
/// not about friends.
pub fn read_spoken(said: &str) -> Option<Spoken> {
    if said.contains(PREFIX) {
        return Some(Spoken::Add);
    }
    let n = crate::intent::normalize(said);
    let n = n.trim();
    const LINK: &[&str] = &[
        "add a friend",
        "add friend",
        "add a new friend",
        "friend link",
        "my friend link",
        "make a friend link",
        "make me a friend link",
        "give me a friend link",
        "new friend link",
    ];
    if LINK.contains(&n) {
        return Some(Spoken::Link);
    }
    if matches!(n, "friend requests" | "any friend requests" | "show friend requests" | "show my friend requests") {
        return Some(Spoken::Requests);
    }
    let after = |prefixes: &[&str]| -> Option<String> {
        prefixes.iter().find_map(|p| n.strip_prefix(p)).map(|r| r.trim().to_string()).filter(|r| !r.is_empty())
    };
    if let Some(who) = after(&["accept the friend request from ", "accept friend request from ", "accept a friend request from "]) {
        return Some(Spoken::Accept(who));
    }
    if let Some(who) = after(&["decline the friend request from ", "decline friend request from ", "ignore the friend request from "]) {
        return Some(Spoken::Decline(who));
    }
    if let Some(who) = after(&["send a friend request to ", "send friend request to ", "friend request ", "be friends with "]) {
        return Some(Spoken::Request(who));
    }
    // "send Sam a friend request"
    if let Some(who) = n.strip_prefix("send ").and_then(|r| r.strip_suffix(" a friend request")).map(str::trim) {
        if !who.is_empty() {
            return Some(Spoken::Request(who.to_string()));
        }
    }
    None
}

/// A friend request that reached you through a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub from: String,
    pub in_group: String,
    pub link: String,
    pub at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requests {
    pub waiting: Vec<Request>,
}

impl Requests {
    pub fn load(store: &crate::store::Store) -> Requests {
        store.load(REQUESTS)
    }
    pub fn save(&self, store: &crate::store::Store) -> Result<(), String> {
        store.save(REQUESTS, self).map_err(|e| format!("couldn't save it: {e}"))
    }
    /// A request, unless the same person already has one waiting.
    pub fn add(&mut self, r: Request) {
        self.waiting.retain(|x| !x.from.eq_ignore_ascii_case(&r.from));
        self.waiting.push(r);
        while self.waiting.len() > 50 {
            self.waiting.remove(0);
        }
    }
    pub fn take(&mut self, from: &str) -> Option<Request> {
        let at = self.waiting.iter().position(|r| r.from.eq_ignore_ascii_case(from))?;
        Some(self.waiting.remove(at))
    }
}

/// The message body that carries a friend request to one person in a group.
pub fn request_body(to_key: &str, link: &str) -> String {
    format!("{REQUEST_PREFIX}{to_key}:{link}")
}

/// Read one: (who it's for, the link).
pub fn read_request(body: &str) -> Option<(String, String)> {
    let rest = body.trim().strip_prefix(REQUEST_PREFIX)?;
    let (key, link) = rest.split_once(':')?;
    crate::peerkey::is_public_key(key).then(|| (key.to_string(), link.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tag: &str) -> crate::store::Store {
        let d = std::env::temp_dir().join(format!("atlas-friends-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        crate::store::Store::new(d)
    }

    fn me() -> Me {
        Me {
            name: "Eric".into(),
            key: crate::peerkey::Identity::from_seed_for_test([1; 32]).public(),
            routes: crate::kin::Routes { addrs: vec!["192.168.1.7:8788".into()], onion: String::new() },
        }
    }

    #[test]
    fn a_link_travels_whole_through_a_text_message() {
        let s = store("link");
        let link = make_link(&s, &me(), 1_000).unwrap();
        let pasted = format!("hey add me on atlas {link} thanks!");
        let l = Link::decode(&pasted).unwrap();
        assert_eq!(l.name, "Eric");
        assert_eq!(l.key, me().key);
        assert!(Link::decode("atlas-friend:not-a-link").is_err());
        assert!(Link::decode("hello").is_err());
    }

    #[test]
    fn a_link_works_once_within_seven_days_and_only_if_it_is_mine() {
        let s = store("redeem");
        let l = Link::decode(&make_link(&s, &me(), 1_000).unwrap()).unwrap();
        assert!(redeem(&s, "not-mine-at-all-000000", 2_000).is_err());
        redeem(&s, &l.invite, 2_000).unwrap();
        assert!(redeem(&s, &l.invite, 2_001).is_err(), "a link was used twice");
        let old = Link::decode(&make_link(&s, &me(), 1_000).unwrap()).unwrap();
        assert!(redeem(&s, &old.invite, 1_000 + LINK_DAYS * 86_400).is_err(), "an expired link worked");
    }

    #[test]
    fn a_friend_is_recorded_both_ways_with_their_key_and_a_free_name() {
        let mut p = crate::kin::Pairings::default();
        p.add_peer(crate::kin::Peer::new("Sam", "old-token"));
        let key = crate::peerkey::Identity::from_seed_for_test([2; 32]).public();
        let at = |a: &str| crate::kin::Routes { addrs: vec![a.into()], onion: String::new() };
        let name = record(&mut p, "Sam", &at("198.51.101.4:8788"), &key, "tok");
        assert_eq!(name, "Sam (2)", "a second Sam must not replace the first");
        assert_eq!(p.key_of("Sam (2)"), Some(key.as_str()));
        assert!(p.contacts.iter().any(|c| c.name == "Sam (2)" && c.host == "198.51.101.4"));
        // The same person again: refreshed, not duplicated.
        assert_eq!(record(&mut p, "Samuel", &at("198.51.101.9:8788"), &key, "tok2"), "Sam (2)");
        assert_eq!(p.routes["sam (2)"].addrs, vec!["198.51.101.9:8788".to_string()], "where to reach them wasn't refreshed");
        assert_eq!(p.peers.len(), 2);
    }

    #[test]
    fn a_group_friend_request_names_who_it_is_for() {
        let key = crate::peerkey::Identity::from_seed_for_test([2; 32]).public();
        let body = request_body(&key, "atlas-friend:abc");
        assert_eq!(read_request(&body), Some((key, "atlas-friend:abc".into())));
        assert_eq!(read_request("hello"), None);
    }

    #[test]
    fn what_is_said_about_friends_is_read_and_nothing_else_is() {
        assert_eq!(read_spoken("add a friend"), Some(Spoken::Link));
        assert_eq!(read_spoken("Add friend"), Some(Spoken::Link));
        assert_eq!(read_spoken("here atlas-friend:abc"), Some(Spoken::Add));
        assert_eq!(read_spoken("accept friend request from Sam"), Some(Spoken::Accept("sam".into())));
        assert_eq!(read_spoken("decline the friend request from Sam"), Some(Spoken::Decline("sam".into())));
        assert_eq!(read_spoken("send Maya a friend request"), Some(Spoken::Request("maya".into())));
        assert_eq!(read_spoken("be friends with Maya"), Some(Spoken::Request("maya".into())));
        assert_eq!(read_spoken("friend requests"), Some(Spoken::Requests));
        assert_eq!(read_spoken("add milk to the list"), None);
        assert_eq!(read_spoken("friend"), None);
    }

    #[test]
    fn with_no_name_set_a_link_still_says_who_it_is_from() {
        assert_eq!(my_name(Some("  Eric ")), "Eric");
        assert!(!my_name(None).is_empty());
    }
}
