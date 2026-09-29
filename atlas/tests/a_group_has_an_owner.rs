//! A group chat with an owner, through three real daemons in one process.
//!
//! Eric makes a group; Sam and Maya are in it. Each has their own Atlas -- own
//! store, own pairings, own key -- and the lists and messages are handed
//! between them the way the pairing door hands them over (`receive_group`,
//! `receive_chat`), so what's checked is what each Atlas actually does with
//! them, not what a helper says it would.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::groups::{self, Groups};
use atlas::kin::{Chatted, GroupList, Hello, Pairings, Peer};
use atlas::peerkey::Identity;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

struct Person {
    root: PathBuf,
    key: String,
}

impl Person {
    fn new(test: &str, who: &str) -> Person {
        let root = std::env::temp_dir().join(format!("atlas-owned-{test}-{who}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("state")).unwrap();
        std::fs::create_dir_all(root.join("peers")).unwrap();
        let key = Identity::load_or_create(&root.join("peers")).unwrap().public();
        Person { root, key }
    }
    fn store(&self) -> Store {
        Store::new(self.root.join("state"))
    }
    fn peers(&self) -> PathBuf {
        self.root.join("peers")
    }
    /// Paired with these people, each already introduced (key pinned).
    fn paired_with(&self, others: &[(&str, &Person)]) {
        let mut p = Pairings::default();
        for (name, other) in others {
            let mut peer = Peer::new(name, &format!("{name}-token"));
            peer.key = Some(other.key.clone());
            p.peers.push(peer);
        }
        p.save(&self.peers()).unwrap();
    }
    fn daemon<'a>(&self, c: &'a Config, p: &'a MockPlatform) -> Daemon<'a> {
        let mut d = Daemon::new(c, p, None, self.store(), Proactive::new(ProactiveConfig::default()));
        d.peer_dir = self.peers();
        d.plugins_dir = self.root.join("plugins");
        d
    }
    fn signed(&self, group: &str) -> atlas::groups::Signed {
        let g = Groups::load(&self.store());
        g.named(group).expect("the group").signed.clone()
    }
}

fn chat(from: &str, gid: &str, body: &str, id: &str) -> Chatted {
    Chatted {
        from: from.into(),
        business: None,
        body: body.into(),
        sent_at: 1_000,
        sent_offset_mins: 0,
        after: 1,
        id: id.into(),
        group_id: Some(gid.into()),
        group_name: Some("Friends".into()),
        members: vec![],
        on_behalf_of: None,
    }
}

fn setup(test: &str) -> (Person, Person, Person, String) {
    let (eric, sam, maya) = (Person::new(test, "eric"), Person::new(test, "sam"), Person::new(test, "maya"));
    eric.paired_with(&[("Sam", &sam), ("Maya", &maya)]);
    sam.paired_with(&[("Eric", &eric), ("Maya", &maya)]);
    maya.paired_with(&[("Eric", &eric), ("Sam", &sam)]);
    groups::act(&eric.store(), &eric.peers(), "new", "Friends", "Sam, Maya", "").unwrap();
    groups::act(&eric.store(), &eric.peers(), "role", "Friends", "Maya", "reader").unwrap();
    let gid = Groups::load(&eric.store()).named("Friends").unwrap().state.group_id.clone();
    (eric, sam, maya, gid)
}

#[test]
fn a_member_joins_from_the_owners_list_and_only_posters_are_heard() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, _maya, gid) = setup("join");
    let mut s = sam.daemon(&c, &p);
    let said = s.receive_group(&GroupList { from: "Eric".into(), signed: eric.signed("Friends"), at: 1 });
    assert!(said.contains("Eric added you to \"Friends\" as a member"), "{said}");
    let room = s.chats.room(&gid).expect("the group is open on Sam's Atlas");
    assert!(room.members.iter().any(|m| m == "Eric") && room.members.iter().any(|m| m == "Maya"));

    s.receive_chat(&chat("Eric", &gid, "hello all", "m1"));
    s.receive_chat(&chat("Maya", &gid, "I'm only a reader", "m2"));
    let room = s.chats.room(&gid).unwrap();
    assert!(room.messages.iter().any(|m| m.id == "m1"), "the owner's message was not filed");
    assert!(!room.messages.iter().any(|m| m.id == "m2"), "a reader's message was filed");
}

#[test]
fn a_reader_cannot_post() {
    let (c, p) = (cfg(), plat());
    let (eric, _sam, maya, _gid) = setup("reader");
    let mut m = maya.daemon(&c, &p);
    let said = m.receive_group(&GroupList { from: "Eric".into(), signed: eric.signed("Friends"), at: 1 });
    assert!(said.contains("reader"), "{said}");
    let reply = m.turn("message the Friends group: can I say something?", 2_000);
    // Refused straight away -- not asked "go ahead?" and then refused.
    assert!(!reply.contains("Go ahead?"), "asked a question whose yes could only be refused: {reply}");
    assert!(reply.contains("can read") && reply.contains("not post"), "{reply}");
    let room = m.chats.room(&_gid).expect("open");
    assert!(room.messages.is_empty(), "a reader's message was written anyway");
}

#[test]
fn nobody_else_can_rewrite_the_list() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, maya, gid) = setup("forged");
    let mut m = maya.daemon(&c, &p);
    m.receive_group(&GroupList { from: "Eric".into(), signed: eric.signed("Friends"), at: 1 });

    // Sam writes a list making himself the owner, signed with his own key.
    let mut state = groups::open(&eric.signed("Friends")).unwrap();
    state.version += 1;
    state.owner = sam.key.clone();
    for seat in state.seats.iter_mut() {
        if seat.role == groups::Role::Owner {
            seat.role = groups::Role::Member;
        }
        if seat.key == sam.key {
            seat.role = groups::Role::Owner;
        }
    }
    let text = serde_json::to_string(&state).unwrap();
    let sig = Identity::load_or_create(&sam.peers()).unwrap().sign(groups::DOMAIN, text.as_bytes());
    m.receive_group(&GroupList { from: "Sam".into(), signed: groups::Signed { state: text, signature: sig, signer: String::new() }, at: 2 });

    let held = Groups::load(&maya.store());
    assert_eq!(held.held[&gid].state.owner, eric.key, "Sam took the group over");
    assert_eq!(held.held[&gid].state.version, 2);
}

#[test]
fn the_owner_takes_someone_out_and_their_atlas_closes_the_group() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, _maya, gid) = setup("removed");
    let mut s = sam.daemon(&c, &p);
    s.receive_group(&GroupList { from: "Eric".into(), signed: eric.signed("Friends"), at: 1 });
    assert!(s.chats.room(&gid).is_some());

    groups::act(&eric.store(), &eric.peers(), "remove", "Friends", "Sam", "").unwrap();
    assert!(Groups::load(&eric.store()).owed(&eric.key).iter().any(|(_, k)| *k == sam.key), "Sam must be told");
    let said = s.receive_group(&GroupList { from: "Eric".into(), signed: eric.signed("Friends"), at: 2 });
    assert!(said.contains("took you out"), "{said}");
    assert!(s.chats.room(&gid).is_none(), "the group stayed open after Sam was taken out");
    s.receive_chat(&chat("Eric", &gid, "after you left", "m9"));
    assert!(s.chats.room(&gid).is_none(), "a message re-opened a group Sam was taken out of");

    // Put back in: open again.
    groups::act(&eric.store(), &eric.peers(), "add", "Friends", "Sam", "").unwrap();
    s.receive_group(&GroupList { from: "Eric".into(), signed: eric.signed("Friends"), at: 3 });
    assert!(s.chats.room(&gid).is_some(), "being added back didn't re-open it");
}

#[test]
fn a_message_that_beats_its_list_waits_for_it() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, _maya, gid) = setup("early");
    let mut s = sam.daemon(&c, &p);
    s.receive_chat(&chat("Eric", &gid, "early bird", "m1"));
    assert!(s.chats.room(&gid).is_none(), "filed without a list to check it against");
    s.receive_group(&GroupList { from: "Eric".into(), signed: eric.signed("Friends"), at: 2 });
    let room = s.chats.room(&gid).expect("open");
    assert!(room.messages.iter().any(|m| m.id == "m1"), "the held message was lost");
}

#[test]
fn the_owner_cant_walk_out_but_a_member_leaving_is_taken_off_the_list() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, _maya, gid) = setup("leave");
    let mut e = eric.daemon(&c, &p);
    e.settle_owned_groups();
    let reply = e.turn("leave the Friends group", 1_000);
    assert!(reply.contains("owner"), "{reply}");
    e.receive_left(&atlas::kin::LeftGroup { from: "Sam".into(), group_id: gid.clone(), at: 2 });
    let held = Groups::load(&eric.store());
    assert!(held.held[&gid].state.seat(&sam.key).is_none(), "Sam left and is still on the list");
}

#[test]
fn an_introduction_is_pinned_once_and_a_different_key_is_refused() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, _maya, _gid) = setup("hello");
    let mut e = eric.daemon(&c, &p);
    let mut pairings = Pairings::load(&eric.peers());
    pairings.peers.iter_mut().for_each(|x| x.key = None);
    pairings.save(&eric.peers()).unwrap();
    e.receive_hello(&Hello { from: "Sam".into(), key: sam.key.clone(), at: 1 });
    assert_eq!(Pairings::load(&eric.peers()).key_of("Sam"), Some(sam.key.as_str()));
    let other = Identity::from_seed_for_test([42; 32]).public();
    e.receive_hello(&Hello { from: "Sam".into(), key: other, at: 2 });
    assert_eq!(Pairings::load(&eric.peers()).key_of("Sam"), Some(sam.key.as_str()), "a new key replaced the pinned one");
}

#[test]
fn a_release_channel_is_read_only_and_its_owners_signed_notice_is_heard() {
    use atlas::release::{anchor_of, seal_manifest, signing_key_from_seed, Artifact, Installed, Manifest, TrustState};
    let (c, p) = (cfg(), plat());
    let (eric, sam, _maya, _gid) = setup("release");
    groups::act(&eric.store(), &eric.peers(), "new", "Atlas updates", "Sam", "release").unwrap();
    let chan = Groups::load(&eric.store()).named("Atlas updates").unwrap().state.clone();
    assert_eq!(chan.role_of(&sam.key), Some(groups::Role::Reader));

    // Sam's Atlas trusts a test release key.
    let key = signing_key_from_seed(&[5; 32]);
    let mut inst = Installed::load(&sam.store());
    inst.trust = TrustState { current: anchor_of(&key), rotations: 0 };
    inst.save(&sam.store()).unwrap();

    let mut s = sam.daemon(&c, &p);
    s.receive_group(&GroupList { from: "Eric".into(), signed: eric.signed("Atlas updates"), at: 1 });
    let platform = atlas::release::this_platform().expect("a known platform").to_string();
    let m = Manifest {
        format: atlas::release::MANIFEST_FORMAT,
        sequence: 3,
        version: "1.3.0".into(),
        min_data_format: 1,
        data_format: 1,
        released_at: 1_000,
        next_word_by: 1_000 + 86_400 * 30,
        artifacts: vec![Artifact { platform, file: "atlas-new".into(), size: 10, sha256: "cd".repeat(32) }],
    };
    let body = atlas::update_courier::announcement(&seal_manifest(&key, &m));
    let mut msg = chat("Eric", &chan.group_id, &body, "r1");
    msg.group_name = Some("Atlas updates".into());
    s.receive_chat(&msg);
    let avail = atlas::update_courier::Available::load(&sam.store());
    assert_eq!(avail.version, "1.3.0", "the signed notice in the channel wasn't heard");

    // The same text from a reader is not even read.
    let fresh = Person::new("release2", "sam");
    let _ = fresh;
    let mut from_reader = chat("Maya", &chan.group_id, &body, "r2");
    from_reader.group_name = Some("Atlas updates".into());
    s.receive_chat(&from_reader);
    assert!(!s.chats.room(&chan.group_id).unwrap().messages.iter().any(|m| m.id == "r2"));
}

// ---------------------------------------------------------------- the doors

fn post(path: &str, body: String) -> atlas::server::Request {
    atlas::server::Request {
        method: "POST".into(),
        path: path.into(),
        query: String::new(),
        token: None,
        token_from_url: false,
        body,
    }
}

#[test]
fn the_introduction_door_takes_a_real_key_from_a_known_peer_and_nothing_else() {
    let door = std::sync::Mutex::new(atlas::kin::Door::new(vec![Peer::new("Sam", "sam-token")]));
    let key = Identity::from_seed_for_test([11; 32]).public();
    let good = post("/hello", format!("{{\"key\":\"{key}\"}}"));
    match atlas::server::route_hello(&good, &door, "sam-token") {
        Some(atlas::server::Action::PeerHello(h)) => {
            assert_eq!(h.from, "Sam", "the name comes from the token");
            assert_eq!(h.key, key);
        }
        other => panic!("{other:?}"),
    }
    assert!(atlas::server::route_hello(&good, &door, "stranger").is_none(), "an unknown token was let in");
    assert!(atlas::server::route_hello(&post("/hello", "{\"key\":\"zz\"}".into()), &door, "sam-token").is_none());
    assert!(atlas::server::route_hello(&post("/chat", good.body.clone()), &door, "sam-token").is_none(), "wrong door");
}

#[test]
fn the_group_door_carries_a_list_whose_truth_is_its_signature() {
    let (eric, _sam, _maya, _gid) = setup("door");
    let signed = eric.signed("Friends");
    let door = std::sync::Mutex::new(atlas::kin::Door::new(vec![Peer::new("Eric", "eric-token")]));
    let body = serde_json::json!({ "state": signed.state, "signature": signed.signature }).to_string();
    match atlas::server::route_group(&post("/group", body.clone()), &door, "eric-token") {
        Some(atlas::server::Action::PeerGroup(g)) => {
            assert_eq!(g.from, "Eric");
            assert!(groups::open(&g.signed).is_ok(), "the list arrived altered");
        }
        other => panic!("{other:?}"),
    }
    assert!(atlas::server::route_group(&post("/group", body), &door, "nobody").is_none());
    let huge = serde_json::json!({ "state": "x".repeat(groups::MAX_STATE_BYTES + 1), "signature": "00" }).to_string();
    assert!(atlas::server::route_group(&post("/group", huge), &door, "eric-token").is_none(), "an oversized list was taken");
}

// ---------------------------------------------------------------- relaying

#[test]
fn the_owner_passes_a_members_message_on_to_the_rest() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, maya, gid) = setup("relay");
    let mut e = eric.daemon(&c, &p);
    e.settle_owned_groups();
    e.receive_chat(&chat("Sam", &gid, "Sam here", "s1"));
    let owed = Groups::load(&eric.store()).relay_owed;
    assert!(owed.iter().any(|r| r.id == "s1" && r.to == maya.key && r.author == sam.key), "{owed:?}");
    assert!(!owed.iter().any(|r| r.to == sam.key), "passed back to its own author");
}

#[test]
fn a_member_hears_a_member_they_arent_paired_with_through_the_owner_only() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, maya, gid) = setup("relayed");
    // Maya is paired with Eric only.
    maya.paired_with(&[("Eric", &eric)]);
    groups::act(&eric.store(), &eric.peers(), "role", "Friends", "Maya", "member").unwrap();
    let mut m = maya.daemon(&c, &p);
    m.receive_group(&GroupList { from: "Eric".into(), signed: eric.signed("Friends"), at: 1 });

    let mut relayed = chat("Eric", &gid, "Sam here, via Eric", "s1");
    relayed.on_behalf_of = Some(sam.key.clone());
    m.receive_chat(&relayed);
    let room = m.chats.room(&gid).expect("open");
    let got = room.messages.iter().find(|x| x.id == "s1").expect("the relayed message was not filed");
    assert_eq!(got.from, "Sam", "it should read as Sam's, by the name on the owner's list");

    // Somebody who isn't the owner can't speak for someone else.
    maya.paired_with(&[("Eric", &eric), ("Sam", &sam)]);
    let mut forged = chat("Sam", &gid, "pretending to be Eric", "f1");
    forged.on_behalf_of = Some(eric.key.clone());
    m.receive_chat(&forged);
    let room = m.chats.room(&gid).unwrap();
    let f = room.messages.iter().find(|x| x.id == "f1").expect("Sam's own message is still Sam's");
    assert_eq!(f.from, "Sam", "a member was believed speaking for someone else");
}

// ---------------------------------------------------------------- by voice, and older groups

#[test]
fn a_group_can_be_changed_by_saying_so() {
    let (c, p) = (cfg(), plat());
    let (eric, _sam, maya, _gid) = setup("voice");
    let mut e = eric.daemon(&c, &p);
    let reply = e.turn("let Maya post in the Friends group", 1_000);
    assert!(reply.contains("member"), "{reply}");
    assert!(!reply.contains("Go ahead?"), "a change you said out loud was asked about again: {reply}");
    let held = Groups::load(&eric.store());
    assert_eq!(held.named("Friends").unwrap().state.role_of(&maya.key), Some(groups::Role::Member));
    let reply = e.turn("take Maya out of the Friends group", 1_010);
    assert!(reply.contains("Took Maya out"), "{reply}");
}

#[test]
fn an_older_group_can_be_given_an_owner() {
    let (c, p) = (cfg(), plat());
    let (eric, _sam, _maya, _gid) = setup("adopt");
    let mut e = eric.daemon(&c, &p);
    let roster = atlas::roster::Roster::load(&eric.store());
    let pairings = Pairings::load(&eric.peers());
    e.chats
        .open_group("legacy-1", "Old crew", atlas::earned::Space::Personal, &["Sam".into(), "Maya".into()], &roster, &pairings)
        .unwrap();
    let said = e.adopt_group("Old crew").unwrap();
    assert!(said.contains("now has an owner"), "{said}");
    assert!(Groups::load(&eric.store()).named("Old crew").is_some());
    assert_eq!(e.chats.room("legacy-1").unwrap().name, "Old crew (before)");
    assert!(e.adopt_group("Old crew").is_err(), "adopted twice");
}

#[test]
fn recommending_an_add_on_says_so_in_the_group() {
    let (c, p) = (cfg(), plat());
    let (eric, _sam, _maya, gid) = setup("recommend");
    let mut e = eric.daemon(&c, &p);
    e.settle_owned_groups();
    std::fs::create_dir_all(e.plugins_dir.join("tidy")).unwrap();
    std::fs::write(
        e.plugins_dir.join("tidy/plugin.yaml"),
        "plugin_api: 1\nid: tidy\nname: Tidy up\nauthor: Eric\npermissions: [basics]\nflows:\n  - name: tidy\n    triggers: [\"tidy things up\"]\n    steps:\n      - command: resume\n",
    )
    .unwrap();
    let said = e.recommend_addon("tidy", "Friends");
    assert!(said.contains("recommend"), "{said}");
    let room = e.chats.room(&gid).unwrap();
    assert!(room.messages.iter().any(|m| m.body.contains("Tidy up") && m.body.contains("recommend")));
}

#[test]
fn a_notice_you_queue_is_posted_in_your_release_channel() {
    use atlas::release::{manifest_for, seal_manifest, signing_key_from_seed};
    let (c, p) = (cfg(), plat());
    let (eric, _sam, _maya, _gid) = setup("announce");
    groups::act(&eric.store(), &eric.peers(), "new", "Atlas updates", "Sam", "release").unwrap();
    let key = signing_key_from_seed(&[5; 32]);
    let m = manifest_for(1, "1.0.1", 1, 1_000, 1_000 + 86_400, &[("windows-x86_64".into(), "atlas.exe".into(), b"exe".to_vec())])
        .unwrap();
    let dir = eric.root.join("state").join(atlas::update_courier::OUTBOX);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("atlas-release-1.0.1.json"), serde_json::to_string(&seal_manifest(&key, &m)).unwrap()).unwrap();

    let mut e = eric.daemon(&c, &p);
    let said = e.tick(5_000).join(" ");
    assert!(said.contains("Announced atlas-release-1.0.1"), "{said}");
    let chan = Groups::load(&eric.store()).named("Atlas updates").unwrap().state.group_id.clone();
    let room = e.chats.room(&chan).expect("the channel");
    assert!(room.messages.iter().any(|m| m.body.starts_with(atlas::update_courier::PREFIX)));
    assert!(!dir.join("atlas-release-1.0.1.json").exists(), "posted twice waiting to happen");
    let again = e.tick(5_010).join(" ");
    assert!(!again.contains("Announced"), "{again}");
}
