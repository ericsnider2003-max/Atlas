//! Adding a friend: you send a link, they open it, you're friends. Nothing to
//! send back, nobody waits for a confirmation.
//!
//! Real daemons with real doors on real sockets: Eric's Atlas makes the link,
//! Sam's Atlas opens it and knocks on Eric's door over TCP, and Eric's Atlas
//! answers the knock the way its loop does (`answer_peer_door`). What's checked
//! is what each Atlas ends up believing about the other.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::friends::{Link, Outbox, Requests};
use atlas::groups::{self, Groups};
use atlas::kin::{Chatted, GroupList, Pairings, Peer};
use atlas::peerkey::Identity;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::SignalListener;
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

#[derive(Clone)]
struct Person {
    root: PathBuf,
    key: String,
}

impl Person {
    fn new(test: &str, who: &str) -> Person {
        let root = std::env::temp_dir().join(format!("atlas-friends-{test}-{who}-{}", std::process::id()));
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
    fn pairings(&self) -> Pairings {
        Pairings::load(&self.peers())
    }
    /// A daemon with its own door open on a free port, reachable at 127.0.0.1.
    fn daemon<'a>(&self, c: &'a Config, p: &'a MockPlatform) -> Daemon<'a> {
        let mut d = Daemon::new(c, p, None, self.store(), Proactive::new(ProactiveConfig::default()));
        d.peer_dir = self.peers();
        d.plugins_dir = self.root.join("plugins");
        d.friend_host = Some("127.0.0.1".into());
        // No Tor unless a test brings its own network (see the Tor test below).
        d.tor_instead = Some(None);
        let peers = self.pairings().peers;
        d.with_signal_listener(SignalListener::bind(0, peers).unwrap())
    }
    fn paired_with(&self, others: &[(&str, &Person)]) {
        let mut p = Pairings::default();
        for (name, other) in others {
            let mut peer = Peer::new(name, &format!("{name}-token"));
            peer.key = Some(other.key.clone());
            p.peers.push(peer);
        }
        p.save(&self.peers()).unwrap();
    }
}

/// Run `their_side` as another Atlas on its own thread while `mine` keeps
/// answering its door, the way its loop would. Returns what they were told.
fn while_answering(mine: &mut Daemon, their_side: impl FnOnce() -> String + Send) -> (String, Vec<String>) {
    std::thread::scope(|s| {
        let h = s.spawn(their_side);
        let mut heard = Vec::new();
        while !h.is_finished() {
            if let Some(said) = mine.answer_peer_door(1_000) {
                heard.push(said);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Anything that landed in the last moment.
        while let Some(said) = mine.answer_peer_door(1_000) {
            heard.push(said);
        }
        (h.join().unwrap(), heard)
    })
}

fn link_in(said: &str) -> String {
    let l = Link::decode(said).unwrap_or_else(|e| panic!("no link in {said:?}: {e}"));
    l.encode()
}

#[test]
fn a_link_sent_and_opened_makes_two_friends_with_nothing_sent_back() {
    let (c, p) = (cfg(), plat());
    let (eric, sam) = (Person::new("onestep", "eric"), Person::new("onestep", "sam"));
    let mut e = eric.daemon(&c, &p);
    let said = e.turn("add a friend", 1_000);
    assert!(said.contains("friend link"), "{said}");
    let link = link_in(&said);
    assert_eq!(Link::decode(&link).unwrap().key, eric.key, "the link must carry Eric's key");

    let sam2 = sam.clone();
    let pasted = format!("add me on atlas! {link}");
    let (told_sam, heard) = while_answering(&mut e, move || {
        let (c, p) = (cfg(), plat());
        let mut s = sam2.daemon(&c, &p);
        s.turn(&pasted, 1_000)
    });
    assert!(told_sam.contains("friends now"), "Sam was told: {told_sam}");
    assert!(heard.iter().any(|h| h.contains("is your friend now")), "Eric was told: {heard:?}");

    // Both sides: a pairing each way, and the other's key pinned from the link.
    let ep = eric.pairings();
    let sp = sam.pairings();
    let sam_name = ep.name_of_key(&sam.key).expect("Eric's Atlas has Sam, by key");
    let eric_name = sp.name_of_key(&eric.key).expect("Sam's Atlas has Eric, by key");
    assert!(ep.contacts.iter().any(|c| c.name == sam_name), "Eric can't reach Sam");
    assert!(sp.contacts.iter().any(|c| c.name == eric_name), "Sam can't reach Eric");
    // One shared secret, both ways.
    let e_tok = &ep.peers.iter().find(|x| x.name == sam_name).unwrap().token;
    let s_tok = &sp.contacts.iter().find(|x| x.name == eric_name).unwrap().token;
    assert_eq!(e_tok, s_tok, "the token Sam sends isn't the one Eric's door lets in");
}

#[test]
fn a_link_works_once_and_a_second_use_is_refused_and_undone() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, maya) =
        (Person::new("once", "eric"), Person::new("once", "sam"), Person::new("once", "maya"));
    let mut e = eric.daemon(&c, &p);
    let link = link_in(&e.turn("make a friend link", 1_000));

    let (sam2, l1) = (sam.clone(), link.clone());
    let (told, _) = while_answering(&mut e, move || {
        let (c, p) = (cfg(), plat());
        let mut d = sam2.daemon(&c, &p);
        let said = d.turn(&l1, 1_000);
        said
    });
    assert!(told.contains("friends now"), "{told}");

    // The same link, forwarded to Maya.
    let maya2 = maya.clone();
    let (told, _) = while_answering(&mut e, move || {
        let (c, p) = (cfg(), plat());
        let mut d = maya2.daemon(&c, &p);
        let said = d.turn(&link, 1_000);
        said
    });
    assert!(told.contains("turned that link down"), "a used link was taken: {told}");
    assert!(maya.pairings().peers.is_empty(), "Maya's Atlas kept half a friendship");
    assert!(eric.pairings().name_of_key(&maya.key).is_none(), "Eric's Atlas let Maya in");
}

#[test]
fn a_friend_who_is_offline_is_kept_and_tried_again() {
    let (c, p) = (cfg(), plat());
    let (eric, sam) = (Person::new("offline", "eric"), Person::new("offline", "sam"));
    // A link to a door nobody is behind.
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = dead.local_addr().unwrap().port();
    drop(dead);
    let link = Link {
        name: "Eric".into(),
        key: eric.key.clone(),
        invite: "a".repeat(32),
        routes: atlas::kin::Routes { addrs: vec![format!("127.0.0.1:{port}")], onion: String::new() },
    }
    .encode();
    let mut s = sam.daemon(&c, &p);
    let told = s.turn(&link, 1_000);
    assert!(told.contains("keep trying"), "{told}");
    let out = Outbox::load(&Store::new(sam.peers()));
    assert_eq!(out.pending.len(), 1, "nothing kept to try again");
    assert!(sam.pairings().name_of_key(&eric.key).is_some(), "Eric isn't recorded yet");
    // Unfriending stops the trying too.
    let said = s.unfriend("Eric");
    assert!(said.contains("Unfriended"), "{said}");
    assert!(Outbox::load(&Store::new(sam.peers())).pending.is_empty(), "still trying someone you unfriended");
}

#[test]
fn your_own_link_and_a_broken_one_are_said_plainly() {
    let (c, p) = (cfg(), plat());
    let eric = Person::new("own", "eric");
    let mut e = eric.daemon(&c, &p);
    let link = link_in(&e.turn("add a friend", 1_000));
    assert!(e.turn(&link, 1_000).contains("your own friend link"));
    let broken = format!("{}!!", &link[..20]);
    let said = e.add_friend(&broken);
    assert!(said.contains("damaged") || said.contains("isn't"), "{said}");
}

#[test]
fn the_friend_door_takes_no_token_and_opens_nothing_else() {
    let (c, p) = (cfg(), plat());
    let eric = Person::new("door", "eric");
    let mut e = eric.daemon(&c, &p);
    let link = Link::decode(&link_in(&e.turn("add a friend", 1_000))).unwrap();
    let port = atlas::onion::read_addr(&link.routes.addrs[0]).unwrap().port();
    let host = format!("127.0.0.1:{port}");
    let t = std::time::Duration::from_secs(2);
    // A made-up secret: refused.
    let wrong = format!(
        "{{\"invite\":\"{}\",\"name\":\"X\",\"host\":\"h\",\"port\":1,\"key\":\"{}\",\"token\":\"{}\"}}",
        "b".repeat(32),
        eric.key,
        "c".repeat(32)
    );
    let r = std::thread::scope(|s| {
        let h = s.spawn(|| atlas::http::post_json(&host, "/friend", &wrong, t));
        while !h.is_finished() {
            e.answer_peer_door(1_000);
        }
        h.join().unwrap()
    })
    .unwrap();
    assert!(!r.ok(), "a secret nobody made opened the friend door");
    // Without a token, nothing but the friend door answers.
    let r = std::thread::scope(|s| {
        let h = s.spawn(|| atlas::http::post_json(&host, "/chat", "{\"body\":\"hi\",\"id\":\"1\"}", t));
        while !h.is_finished() {
            e.answer_peer_door(1_000);
        }
        h.join().unwrap()
    })
    .unwrap();
    assert_eq!(r.status, 401, "a message got in with no token");
    assert!(eric.pairings().peers.is_empty());
}

fn chat(from: &str, gid: &str, body: &str, id: &str, on_behalf_of: Option<&str>) -> Chatted {
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
        on_behalf_of: on_behalf_of.map(String::from),
    }
}

#[test]
fn a_friend_request_through_a_group_reaches_only_them_and_one_press_accepts_it() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, maya) = (Person::new("req", "eric"), Person::new("req", "sam"), Person::new("req", "maya"));
    // Sam and Maya are both in Eric's group, and not paired with each other.
    eric.paired_with(&[("Sam", &sam), ("Maya", &maya)]);
    sam.paired_with(&[("Eric", &eric)]);
    maya.paired_with(&[("Eric", &eric)]);
    groups::act(&eric.store(), &eric.peers(), "new", "Friends", "Sam, Maya", "").unwrap();
    let gid = Groups::load(&eric.store()).named("Friends").unwrap().state.group_id.clone();
    let signed = Groups::load(&eric.store()).named("Friends").unwrap().signed.clone();

    let mut s = sam.daemon(&c, &p);
    s.receive_group(&GroupList { from: "Eric".into(), signed: signed.clone(), at: 1 });
    let said = s.turn("send Maya a friend request", 1_000);
    assert!(said.contains("friend request") && said.contains("Friends"), "{said}");
    let out = Outbox::load(&Store::new(sam.peers()));
    assert_eq!(out.requests.len(), 1, "the request isn't queued for the owner");
    let body = out.requests[0].body.clone();

    // Eric's Atlas, as owner: passes it to Maya alone and files nothing.
    let c2 = cfg();
    let mut e = eric.daemon(&c2, &p);
    e.receive_chat(&chat("Sam", &gid, &body, "fr1", None));
    let g = Groups::load(&eric.store());
    let to: Vec<&str> = g.relay_owed.iter().filter(|r| r.body == body).map(|r| r.to.as_str()).collect();
    assert_eq!(to, vec![maya.key.as_str()], "the request went somewhere other than Maya");
    assert!(
        e.chats.room(&gid).map_or(true, |r| r.messages.is_empty()),
        "a friend request was filed as a group message"
    );
    assert!(Requests::load(&Store::new(eric.peers())).waiting.is_empty(), "Eric's Atlas kept a request not for him");

    // Maya's Atlas: a request from Sam, waiting for her.
    let mut m = maya.daemon(&c, &p);
    m.receive_group(&GroupList { from: "Eric".into(), signed, at: 1 });
    m.receive_chat(&chat("Eric", &gid, &body, "fr1", Some(&sam.key)));
    let reqs = Requests::load(&Store::new(maya.peers()));
    assert_eq!(reqs.waiting.len(), 1, "no request waiting for Maya");
    assert_eq!(reqs.waiting[0].from, "Sam");
    assert!(m.chats.room(&gid).map_or(true, |r| r.messages.is_empty()), "the request showed up as a message");
    drop(m);

    // One press on Maya's side; Sam's door answers.
    let maya2 = maya.clone();
    let (told, heard) = while_answering(&mut s, move || {
        let (c, p) = (cfg(), plat());
        let mut d = maya2.daemon(&c, &p);
        let said = d.turn("accept friend request from Sam", 1_000);
        said
    });
    assert!(told.contains("friends now"), "Maya was told: {told}");
    assert!(heard.iter().any(|h| h.contains("is your friend now")), "Sam was told: {heard:?}");
    assert!(maya.pairings().name_of_key(&sam.key).is_some());
    assert!(sam.pairings().name_of_key(&maya.key).is_some());
}

#[test]
fn a_request_whose_link_is_someone_elses_is_ignored() {
    let (c, p) = (cfg(), plat());
    let (eric, sam, maya) = (Person::new("spoof", "eric"), Person::new("spoof", "sam"), Person::new("spoof", "maya"));
    eric.paired_with(&[("Sam", &sam), ("Maya", &maya)]);
    maya.paired_with(&[("Eric", &eric)]);
    groups::act(&eric.store(), &eric.peers(), "new", "Friends", "Sam, Maya", "").unwrap();
    let gid = Groups::load(&eric.store()).named("Friends").unwrap().state.group_id.clone();
    let signed = Groups::load(&eric.store()).named("Friends").unwrap().signed.clone();
    let mut m = maya.daemon(&c, &p);
    m.receive_group(&GroupList { from: "Eric".into(), signed, at: 1 });
    // "From Sam", but carrying a link that adds somebody else entirely.
    let stranger = Identity::from_seed_for_test([42; 32]).public();
    let link = Link {
        name: "Sam".into(),
        key: stranger,
        invite: "d".repeat(32),
        routes: atlas::kin::Routes { addrs: vec!["198.51.101.1:1".into()], onion: String::new() },
    }
    .encode();
    let body = atlas::friends::request_body(&maya.key, &link);
    m.receive_chat(&chat("Eric", &gid, &body, "fr2", Some(&sam.key)));
    assert!(Requests::load(&Store::new(maya.peers())).waiting.is_empty(), "a request with someone else's link was kept");
}

#[test]
fn your_phone_posts_in_your_group_as_you_and_hears_it_too() {
    let (c, p) = (cfg(), plat());
    let (eric, phone, sam, maya) = (
        Person::new("phone", "eric"),
        Person::new("phone", "phone"),
        Person::new("phone", "sam"),
        Person::new("phone", "maya"),
    );
    eric.paired_with(&[("Sam", &sam), ("Maya", &maya), ("Phone", &phone)]);
    phone.paired_with(&[("Eric", &eric)]);
    sam.paired_with(&[("Eric", &eric)]);
    groups::act(&eric.store(), &eric.peers(), "new", "Friends", "Sam, Maya", "").unwrap();
    let me = Identity::load_or_create(&eric.peers()).unwrap();
    let mut g = Groups::load(&eric.store());
    assert_eq!(g.vouch_everywhere(&me, &phone.key), 1);
    g.save(&eric.store()).unwrap();
    let held = g.named("Friends").unwrap().clone();
    let gid = held.state.group_id.clone();

    // The owner hands the phone the list too, and the phone's room is Eric.
    assert!(g.owed(&eric.key).iter().any(|(_, k)| *k == phone.key), "the phone is never sent the list");
    let mut ph = phone.daemon(&c, &p);
    let said = ph.receive_group(&GroupList { from: "Eric".into(), signed: held.signed.clone(), at: 1 });
    assert!(said.is_empty(), "the phone was told it had been added to its own group: {said}");
    assert_eq!(ph.chats.room(&gid).expect("the group on the phone").members, vec!["Eric".to_string()]);

    // The phone may post: it speaks as the owner.
    let mut reply = ph.turn("message the Friends group: hi from my phone", 2_000);
    if reply.contains("Go ahead?") {
        reply = ph.turn("yes", 2_001);
    }
    assert!(!reply.contains("not post"), "{reply}");
    let posted = ph.chats.room(&gid).unwrap().messages.last().cloned().unwrap_or_else(|| panic!("not written on the phone: {reply}"));

    // Eric's laptop files it as his own and passes it on to everyone but the phone.
    let mut e = eric.daemon(&c, &p);
    e.settle_owned_groups();
    assert!(e.chats.room(&gid).unwrap().members.iter().any(|m| m == "Phone"), "what Eric posts never reaches his phone");
    e.receive_chat(&chat("Phone", &gid, &posted.body, &posted.id, None));
    let filed = e.chats.room(&gid).unwrap().messages.iter().find(|m| m.id == posted.id).cloned().expect("filed on the laptop");
    assert_eq!(filed.from, atlas::chat::ME, "the phone's message isn't Eric's own on his laptop");
    let owed = Groups::load(&eric.store());
    let mut to: Vec<String> = owed.relay_owed.iter().filter(|r| r.id == posted.id).map(|r| r.to.clone()).collect();
    to.sort();
    let mut want = vec![sam.key.clone(), maya.key.clone()];
    want.sort();
    assert_eq!(to, want, "passed on to the wrong people");
    assert!(owed.relay_owed.iter().filter(|r| r.id == posted.id).all(|r| r.author == eric.key), "passed on as someone other than Eric");

    // Sam hears it from Eric.
    let mut s = sam.daemon(&c, &p);
    s.receive_group(&GroupList { from: "Eric".into(), signed: held.signed.clone(), at: 1 });
    s.receive_chat(&chat("Eric", &gid, &posted.body, &posted.id, Some(&eric.key)));
    let got = s.chats.room(&gid).unwrap().messages.iter().find(|m| m.id == posted.id).cloned().expect("Sam got it");
    assert_eq!(got.from, "Eric");

    // And a member's message reaches the phone through the laptop.
    e.receive_chat(&chat("Sam", &gid, "hello Eric", "s1", None));
    let owed = Groups::load(&eric.store());
    assert!(owed.relay_owed.iter().any(|r| r.id == "s1" && r.to == phone.key), "the phone never hears the group");
}

#[test]
fn a_release_file_travels_to_a_friend_over_the_pairing_in_checked_pieces() {
    use atlas::release::{self, anchor_of, seal_manifest, signing_key_from_seed, Artifact, Installed, Manifest, TrustState};
    use atlas::update_courier::{self as courier, Available, Fetched};
    let (c, p) = (cfg(), plat());
    let (eric, sam) = (Person::new("release", "eric"), Person::new("release", "sam"));
    let Some(platform) = release::this_platform() else { return };

    // Eric signs a release; its file is put aside to hand out.
    let bytes: Vec<u8> = (0..(courier::CHUNK * 3 + 17)).map(|i| (i * 7 % 256) as u8).collect();
    let sha = courier::keep_for_friends(eric.store().root(), &bytes).unwrap();
    let key = signing_key_from_seed(&[5; 32]);
    let m = Manifest {
        format: release::MANIFEST_FORMAT,
        sequence: 3,
        version: "1.3.0".into(),
        min_data_format: 1,
        data_format: atlas::upgrade::DATA_FORMAT,
        released_at: 1_000,
        next_word_by: 1_000 + 30 * 86_400,
        artifacts: vec![Artifact { platform: platform.into(), file: "atlas-new".into(), size: bytes.len() as u64, sha256: sha }],
    };
    let notice = courier::announcement(&seal_manifest(&key, &m));

    // Eric's door, and Sam paired to it.
    let mut ep = Pairings::default();
    let mut sam_peer = Peer::new("Sam", "sam-release-token-000000");
    sam_peer.key = Some(sam.key.clone());
    ep.peers.push(sam_peer.clone());
    ep.save(&eric.peers()).unwrap();
    let mut d = Daemon::new(&c, &p, None, eric.store(), Proactive::new(ProactiveConfig::default()));
    d.peer_dir = eric.peers();
    let l = SignalListener::bind(0, vec![sam_peer]).unwrap();
    let port = l.port();
    let mut e = d.with_signal_listener(l);
    let mut sp = Pairings::default();
    sp.contacts.push(atlas::kin::Contact { name: "Eric".into(), host: "127.0.0.1".into(), port, token: "sam-release-token-000000".into() });
    sp.save(&sam.peers()).unwrap();

    // Sam's Atlas trusts the release key and hears the notice from Eric's channel.
    let mut i = Installed::load(&sam.store());
    i.trust = TrustState { current: anchor_of(&key), rotations: 0 };
    i.save(&sam.store()).unwrap();
    assert!(courier::heard(&sam.store(), &notice, &eric.key, 2_000).is_some());
    assert_eq!(Available::load(&sam.store()).from, eric.key, "doesn't know where to fetch it from");

    // Fetched over the real door, a piece at a time, while Eric's loop answers.
    let s_store = sam.store();
    let link = atlas::kin::PeerLink::from_state(&sp, &atlas::chat::Chats::default());
    let got = std::thread::scope(|s| {
        let h = s.spawn(|| {
            let first = courier::fetch_step(&s_store, 2, |sha, off| link.fetch_release("Eric", sha, off));
            let rest = courier::fetch_step(&s_store, 10, |sha, off| link.fetch_release("Eric", sha, off));
            (first, rest)
        });
        while !h.is_finished() {
            e.answer_peer_door(2_000);
        }
        h.join().unwrap()
    });
    assert_eq!(got.0, Fetched::Partway(2 * courier::CHUNK as u64, bytes.len() as u64));
    assert!(matches!(got.1, Fetched::Ready(_)), "{:?}", got.1);
    assert_eq!(std::fs::read(Available::load(&sam.store()).downloaded).unwrap(), bytes);

    // Someone Eric isn't paired with gets nothing.
    let mut stranger = Pairings::default();
    stranger.contacts.push(atlas::kin::Contact { name: "Eric".into(), host: "127.0.0.1".into(), port, token: "not-a-token-at-all-00000".into() });
    let sl = atlas::kin::PeerLink::from_state(&stranger, &atlas::chat::Chats::default());
    let sha = m.artifacts[0].sha256.clone();
    let r = std::thread::scope(|s| {
        let h = s.spawn(|| sl.fetch_release("Eric", &sha, 0));
        while !h.is_finished() {
            e.answer_peer_door(2_000);
        }
        h.join().unwrap()
    });
    assert!(r.is_none(), "a release file went to someone who isn't paired");
    drop(e);

    // Gap AD: Eric's Atlas is off now. Maya heard the same notice; Sam's
    // Atlas, which has the file and checked it, hands it on over its own
    // real door -- and Maya keeps it only because it matches Eric's signature.
    let maya = Person::new("release", "maya");
    let mut sp2 = Pairings::default();
    let mut maya_peer = Peer::new("Maya", "maya-release-token-00000");
    maya_peer.key = Some(maya.key.clone());
    sp2.peers.push(maya_peer.clone());
    sp2.save(&sam.peers()).unwrap();
    let mut sd = Daemon::new(&c, &p, None, sam.store(), Proactive::new(ProactiveConfig::default()));
    sd.peer_dir = sam.peers();
    let sl2 = SignalListener::bind(0, vec![maya_peer]).unwrap();
    let sam_port = sl2.port();
    let mut sd = sd.with_signal_listener(sl2);
    let mut mp = Pairings::default();
    // Eric's door is shut: nothing listens where Maya would find it.
    let shut = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    mp.contacts.push(atlas::kin::Contact { name: "Eric".into(), host: "127.0.0.1".into(), port: shut, token: "maya-eric-token-00000000".into() });
    mp.contacts.push(atlas::kin::Contact { name: "Sam".into(), host: "127.0.0.1".into(), port: sam_port, token: "maya-release-token-00000".into() });
    let mut i = Installed::load(&maya.store());
    i.trust = TrustState { current: anchor_of(&key), rotations: 0 };
    i.save(&maya.store()).unwrap();
    assert!(courier::heard(&maya.store(), &notice, &eric.key, 2_000).is_some());
    let m_store = maya.store();
    let mlink = atlas::kin::PeerLink::from_state(&mp, &atlas::chat::Chats::default());
    let friends = vec!["Eric".to_string(), "Sam".to_string()];
    let got = std::thread::scope(|s| {
        let h = s.spawn(|| courier::fetch_from_any(&m_store, 10, Some("Eric"), &friends, 3, |who, sha, off| mlink.fetch_release(who, sha, off)));
        while !h.is_finished() {
            sd.answer_peer_door(2_000);
        }
        h.join().unwrap()
    });
    assert!(matches!(got, Fetched::Ready(_)), "Sam didn't pass the release on: {got:?}");
    assert_eq!(std::fs::read(Available::load(&maya.store()).downloaded).unwrap(), bytes);
}

// ---- Between people, with no private network ------------------------------

/// A friendship already made: each has the other's key pinned, one token both
/// ways, and knows where the other's door is.
fn befriend(a: &Person, a_port: u16, b: &Person, b_port: u16, a_name: &str, b_name: &str) {
    let token = format!("{a_name}-{b_name}-token-0000000000");
    for (me, other, other_name, other_port) in [(a, b, b_name, b_port), (b, a, a_name, a_port)] {
        let mut p = me.pairings();
        let mut peer = Peer::new(other_name, &token);
        peer.key = Some(other.key.clone());
        p.peers.retain(|x| x.name != other_name);
        p.peers.push(peer);
        p.contacts.retain(|x| x.name != other_name);
        p.contacts.push(atlas::kin::Contact { name: other_name.into(), host: "127.0.0.1".into(), port: other_port, token: token.clone() });
        p.save(&me.peers()).unwrap();
    }
}

#[test]
fn a_token_on_someone_elses_envelope_is_refused() {
    let (c, p) = (cfg(), plat());
    let (eric, sam) = (Person::new("tokenkey", "eric"), Person::new("tokenkey", "sam"));
    let mut e = eric.daemon(&c, &p);
    let link = Link::decode(&link_in(&e.turn("add a friend", 1_000))).unwrap();
    let addr = link.routes.addrs[0].clone();
    // Eric's door knows Sam by token and key.
    befriend(&eric, 1, &sam, 1, "Eric", "Sam");
    e.admit_peer_for_test(eric.pairings().peers[0].clone());
    // Someone with Sam's token, sealing with their own key.
    let thief = Identity::from_seed_for_test([66; 32]);
    let token = eric.pairings().peers[0].token.clone();
    let r = std::thread::scope(|s| {
        let h = s.spawn(|| {
            atlas::kin::sealed_post(&thief, &eric.key, atlas::kin::Via::Direct(&addr), "/hello", &token, &format!("{{\"key\":\"{}\"}}", thief.public()), std::time::Duration::from_secs(2))
        });
        while !h.is_finished() {
            e.answer_peer_door(1_000);
        }
        h.join().unwrap()
    });
    assert!(r.map_or(true, |r| !r.ok()), "a stolen token worked on someone else's envelope");
    // Sam himself, the same way: taken.
    let sam_id = Identity::load_or_create(&sam.peers()).unwrap();
    let r = std::thread::scope(|s| {
        let h = s.spawn(|| {
            atlas::kin::sealed_post(&sam_id, &eric.key, atlas::kin::Via::Direct(&addr), "/hello", &token, &format!("{{\"key\":\"{}\"}}", sam.key), std::time::Duration::from_secs(2))
        });
        while !h.is_finished() {
            e.answer_peer_door(1_000);
        }
        h.join().unwrap()
    });
    assert!(r.unwrap().ok());
}

/// Two Atlases that share no network at all, reaching each other only
/// through Tor: Eric's link carries nothing but his onion address, Sam's
/// Atlas opens it and knocks through Tor, and the sealed answer comes back
/// the same way.
///
/// Needs a Tor network, which this can't reach from a build machine -- so it
/// runs against a private one: Tor's own test network (`chutney`, hs-v3-min).
/// Start it, then point this at its client's settings:
///
/// ```text
/// ATLAS_TOR_TESTNET=/path/to/chutney/net/nodes/007c/torrc \
///   cargo test --test all through_tor -- --ignored
/// ```
#[test]
#[ignore = "needs a Tor network (a private one: see the doc comment)"]
fn two_atlases_with_no_network_in_common_become_friends_through_tor() {
    let client = std::env::var("ATLAS_TOR_TESTNET").expect("ATLAS_TOR_TESTNET: a chutney client torrc");
    let extra: Vec<String> = std::fs::read_to_string(client)
        .unwrap()
        .lines()
        .filter(|l| {
            ["TestingTorNetwork", "DirAuthority", "AddressDisableIPv6", "UseMicrodescriptors"]
                .iter()
                .any(|k| l.starts_with(k))
        })
        .map(String::from)
        .collect();
    let tor = atlas::onion::find_tor(None).expect("tor installed");
    let (eric, sam) = (Person::new("tor", "eric"), Person::new("tor", "sam"));
    let with_tor = |who: &Person, c: &'static Config, p: &'static MockPlatform| {
        let mut d = who.daemon(c, p);
        // Nothing direct: the only way is Tor.
        d.friend_host = None;
        d.tor_instead = Some(Some((tor.clone(), extra.clone())));
        d
    };
    let c: &'static Config = Box::leak(Box::new(cfg()));
    let p: &'static MockPlatform = Box::leak(Box::new(plat()));
    let mut e = with_tor(&eric, c, p);
    e.start_tor().unwrap();
    // Wait for Eric's Tor, and publish his onion service.
    let ready = |d: &mut Daemon, secs: u64| {
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < secs {
            if d.friends_view().reach.contains("from anywhere") {
                return true;
            }
            d.answer_peer_door(1);
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        false
    };
    assert!(ready(&mut e, 180), "Eric's Tor never connected");
    let said = e.turn("add a friend", 1_000);
    let link = Link::decode(&link_in(&said)).unwrap();
    assert!(atlas::onion::is_onion(&link.routes.onion), "no onion address in the link: {said}");
    let only_tor = Link { routes: atlas::kin::Routes { addrs: vec![], onion: link.routes.onion.clone() }, ..link };
    let pasted = only_tor.encode();

    let sam2 = sam.clone();
    let (tor2, extra2) = (tor.clone(), extra.clone());
    let (told, heard) = while_answering(&mut e, move || {
        let (c, p) = (cfg(), plat());
        let mut s = sam2.daemon(&c, &p);
        s.friend_host = None;
        s.tor_instead = Some(Some((tor2, extra2)));
        s.start_tor().unwrap();
        let start = std::time::Instant::now();
        while !s.friends_view().reach.contains("from anywhere") && start.elapsed().as_secs() < 180 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        // Onion services take a little while to be findable after they start.
        let mut said = String::new();
        for _ in 0..12 {
            said = s.turn(&pasted, 1_000);
            if said.contains("friends now") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
            // Undo the half-friendship a failed try leaves, and try again.
            s.unfriend("Eric");
            s.unfriend("A friend");
        }
        said
    });
    assert!(told.contains("friends now"), "Sam was told: {told}");
    assert!(heard.iter().any(|h| h.contains("is your friend now")), "Eric was told: {heard:?}");
    assert!(sam.pairings().name_of_key(&eric.key).is_some());
    assert!(eric.pairings().name_of_key(&sam.key).is_some());
}

/// Feedback, both ways, over real doors on real sockets: Sam tells Eric
/// something's wrong (Sam's decision), Eric's Atlas files it, Eric answers,
/// and Sam's Atlas hears the answer.
#[test]
fn feedback_travels_to_eric_and_his_answer_travels_back_over_the_pairing() {
    use atlas::feedback::{self as fb, FeedbackStatus};
    let (c, p) = (cfg(), plat());
    let (eric, sam) = (Person::new("feedback", "eric"), Person::new("feedback", "sam"));

    // Eric's door, with Sam paired to it; Sam's door, with Eric paired to it.
    let mut ep = Pairings::default();
    let sam_peer = Peer::new("Sam", "sam-feedback-token-00000");
    ep.peers.push(sam_peer.clone());
    let mut d = Daemon::new(&c, &p, None, eric.store(), Proactive::new(ProactiveConfig::default()));
    d.peer_dir = eric.peers();
    let el = SignalListener::bind(0, vec![sam_peer]).unwrap();
    let eric_port = el.port();
    let mut e = d.with_signal_listener(el);

    let mut sp = Pairings::default();
    let eric_peer = Peer::new("Eric", "eric-feedback-token-0000");
    sp.peers.push(eric_peer.clone());
    sp.contacts.push(atlas::kin::Contact { name: "Eric".into(), host: "127.0.0.1".into(), port: eric_port, token: "sam-feedback-token-00000".into() });
    sp.save(&sam.peers()).unwrap();
    let mut sd = Daemon::new(&c, &p, None, sam.store(), Proactive::new(ProactiveConfig::default()));
    sd.peer_dir = sam.peers();
    let sl = SignalListener::bind(0, vec![eric_peer]).unwrap();
    let sam_port = sl.port();
    let mut s = sd.with_signal_listener(sl);
    ep.contacts.push(atlas::kin::Contact { name: "Sam".into(), host: "127.0.0.1".into(), port: sam_port, token: "eric-feedback-token-0000".into() });
    ep.save(&eric.peers()).unwrap();

    // Sam decides something's wrong, writes it, and sends it.
    let f = fb::compose_feedback("After updating, my morning brief is empty", None, 1_000).unwrap();
    fb::queue_feedback(&sam.store(), f, "Eric");
    let (to, body, id) = fb::feedback_outbox(&sam.store()).remove(0);
    let sam_link = atlas::kin::PeerLink::from_state(&sp, &atlas::chat::Chats::default());
    let took = std::thread::scope(|sc| {
        let h = sc.spawn(|| sam_link.send_feedback(&to, "/feedback", &body));
        while !h.is_finished() {
            e.answer_peer_door(2_000);
        }
        h.join().unwrap()
    });
    assert!(took, "Eric's door didn't take the feedback");
    fb::feedback_delivered(&sam.store(), &id);
    let inbox = fb::feedback_inbox(&eric.store());
    assert_eq!(inbox.len(), 1, "Eric has no report");
    assert_eq!((inbox[0].from.as_str(), inbox[0].words.as_str()), ("Sam", "After updating, my morning brief is empty"));

    // Eric answers; it goes back to Sam's door.
    fb::answer_feedback(&eric.store(), 1, FeedbackStatus::Fixing, "found it -- the brief skips empty mailboxes wrongly", 1_100).unwrap();
    let (to, body, _) = fb::answers_out(&eric.store()).remove(0);
    let eric_link = atlas::kin::PeerLink::from_state(&Pairings::load(&eric.peers()), &atlas::chat::Chats::default());
    let took = std::thread::scope(|sc| {
        let h = sc.spawn(|| eric_link.send_feedback(&to, "/feedback-answer", &body));
        while !h.is_finished() {
            s.answer_peer_door(2_000);
        }
        h.join().unwrap()
    });
    assert!(took, "Sam's door didn't take the answer");
    let mine = fb::feedback_sent(&sam.store());
    println!("LIVE [feedback over real sockets] Sam's feedback now: {} ({:?})", mine[0].status.plain(), mine[0].replies);
    assert_eq!(mine[0].status, FeedbackStatus::Fixing);
    assert!(mine[0].replies[0].1.contains("found it"));
}

// ---- Keeping a friend's Tor connection open (gap AN, OPEN_GAPS 8.7) ----------

/// A stand-in for Atlas's own `tor`: a SOCKS5 port that joins each connection
/// to `door` (the sealed door, where Tor delivers what reaches an onion
/// address), counting how many it had to open -- each one is a new meeting
/// point through the Tor network, the thing that costs seconds.
fn stand_in_tor(door: u16) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>, std::sync::Arc<std::sync::Mutex<Vec<std::net::TcpStream>>>) {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let opened = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let live: std::sync::Arc<std::sync::Mutex<Vec<std::net::TcpStream>>> = Default::default();
    let (o, lv) = (opened.clone(), live.clone());
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(mut c) = c else { continue };
            o.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut g = [0u8; 3];
            if c.read_exact(&mut g).is_err() {
                continue;
            }
            let _ = c.write_all(&[5, 0]);
            let mut h = [0u8; 5];
            let _ = c.read_exact(&mut h);
            let mut name = vec![0u8; h[4] as usize + 2];
            let _ = c.read_exact(&mut name);
            let _ = c.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
            let up = std::net::TcpStream::connect(("127.0.0.1", door)).unwrap();
            lv.lock().unwrap().push(c.try_clone().unwrap());
            let (mut c2, mut up2) = (c.try_clone().unwrap(), up.try_clone().unwrap());
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut c2, &mut up2);
                let _ = up2.shutdown(std::net::Shutdown::Both);
            });
            let (mut up3, mut c3) = (up, c);
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut up3, &mut c3);
                let _ = c3.shutdown(std::net::Shutdown::Both);
            });
        }
    });
    (port, opened, live)
}

#[test]
fn a_friends_tor_connection_stays_open_between_messages() {
    use atlas::update_courier as courier;
    let (c, p) = (cfg(), plat());
    let (eric, sam) = (Person::new("keptopen", "eric"), Person::new("keptopen", "sam"));
    // Eric keeps a file to hand out; Sam is paired, with Eric reachable only
    // at his onion address.
    let bytes: Vec<u8> = (0..(courier::CHUNK * 4)).map(|i| (i % 253) as u8).collect();
    let sha = courier::keep_for_friends(eric.store().root(), &bytes).unwrap();
    let mut ep = Pairings::default();
    let mut sam_peer = Peer::new("Sam", "sam-kept-token-0000000000");
    sam_peer.key = Some(sam.key.clone());
    ep.peers.push(sam_peer.clone());
    ep.save(&eric.peers()).unwrap();
    let mut d = Daemon::new(&c, &p, None, eric.store(), Proactive::new(ProactiveConfig::default()));
    d.peer_dir = eric.peers();
    let l = SignalListener::bind(0, vec![sam_peer]).unwrap();
    let sealed = l.sealed_port();
    let mut e = d.with_signal_listener(l);
    let (socks, opened, live) = stand_in_tor(sealed);

    let mut sp = Pairings::default();
    sp.contacts.push(atlas::kin::Contact { name: "Eric".into(), host: String::new(), port: 0, token: "sam-kept-token-0000000000".into() });
    let mut eric_peer = Peer::new("Eric", "sam-kept-token-0000000000");
    eric_peer.key = Some(eric.key.clone());
    sp.peers.push(eric_peer);
    let onion = atlas::onion::my_address(&Identity::load_or_create(&eric.peers()).unwrap());
    sp.routes.insert("eric".into(), atlas::kin::Routes { addrs: vec![], onion });
    let kept = std::sync::Arc::new(atlas::kin::TorConnections::default());
    let link = atlas::kin::PeerLink::from_state(&sp, &atlas::chat::Chats::default())
        .sealing_as(Identity::load_or_create(&sam.peers()).ok())
        .through_tor(Some(socks))
        .keeping(kept.clone());

    // Four pieces, four sealed requests: one connection through "Tor".
    let got: Vec<Option<usize>> = std::thread::scope(|s| {
        let h = s.spawn(|| (0..4).map(|i| link.fetch_release("Eric", &sha, (i * courier::CHUNK) as u64).map(|(b, _)| b.len())).collect());
        while !h.is_finished() {
            e.answer_peer_door(2_000);
        }
        h.join().unwrap()
    });
    assert_eq!(got, vec![Some(courier::CHUNK); 4], "a piece didn't arrive through the kept connection");
    assert_eq!(opened.load(std::sync::atomic::Ordering::SeqCst), 1, "a new Tor connection was opened for every request");
    assert_eq!(kept.open_now_for_test(), 1);

    // The connection dies (Tor restarted, a network change): the next request
    // notices, opens a new one, and still gets its answer.
    for s in live.lock().unwrap().drain(..) {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let again = std::thread::scope(|s| {
        let h = s.spawn(|| link.fetch_release("Eric", &sha, 0).map(|(b, _)| b.len()));
        while !h.is_finished() {
            e.answer_peer_door(2_000);
        }
        h.join().unwrap()
    });
    assert_eq!(again, Some(courier::CHUNK), "a dead kept connection wasn't replaced");
    assert_eq!(opened.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn only_the_door_friends_reach_through_tor_keeps_connections_and_only_so_many() {
    // In the clear, on your own network: one request, one connection, as before.
    let l = SignalListener::bind(0, vec![]).unwrap();
    let port = l.port();
    let h = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(b"POST /signal HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    });
    let start = std::time::Instant::now();
    while !h.is_finished() && start.elapsed().as_secs() < 5 {
        let _ = l.poll_once(1);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let out = h.join().unwrap();
    assert!(out.contains("Connection: close"), "{out}");
    assert_eq!(l.kept_open_for_test(), 0);
    assert!(atlas::kin::MAX_KEPT <= 16, "more open connections allowed than the door should hold");
}
