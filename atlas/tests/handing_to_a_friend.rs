//! Handing a note to a friend's Atlas, end to end.
//!
//! `household::share_with_friend` built a `Handoff` and nothing sent it, for
//! as long as it had existed — it was the last piece of that module still
//! unwired after the device-pairing round, named in the handover as needing a
//! decision on what the first real use looks like. The decision: a note, over
//! the peer channel a pairing already established.
//!
//! What that ruling did *not* settle, and what most of this file is about, is
//! where a note lands once it arrives. It does not land in the tray. The
//! tray's rule is that handing Atlas something says "look at this", and Atlas
//! then goes and reads it — right for something you sent from your own phone,
//! wrong for something that arrived unasked, because it would mean a peer
//! credential could make your Atlas fetch a URL. So a note waits in a list
//! until you say to keep it, and `keep` is the deliberate act that grants
//! everything further.

use atlas::earned::Space;
use atlas::household::{share_with_friend, Handoff, Inbox, MAX_WAITING};
use atlas::kin::{
    as_waiting, json_string_for_test, Delivered, Door, Peer, Refused, MAX_HANDOFFS_PER_WINDOW,
    MAX_PER_WINDOW,
};
use atlas::server::{route, route_handoff, route_signal, Action, Request};
use atlas::store::Store;
use std::path::PathBuf;

const PEER_TOKEN: &str = "the-peer-token-0123456789abcdef";
const OTHER_TOKEN: &str = "some-other-token-0123456789abcd";

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-hf-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn door() -> Door {
    Door::new(vec![Peer::new("Priya", PEER_TOKEN)])
}

fn locked() -> std::sync::Mutex<Door> {
    std::sync::Mutex::new(door())
}

fn handoff_request(what: &str) -> Request {
    Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/handoff".into(),
        token: Some(PEER_TOKEN.into()),
        body: format!(r#"{{"what":"{what}","from":"Your Accountant"}}"#),
    }
}

// ================= the sending half =================

#[test]
fn what_gets_sent_carries_nothing_about_your_household() {
    // The type's whole promise, checked at the point it is built rather than
    // assumed from the doc comment.
    let h = share_with_friend("the spreadsheet template we talked about", "Eric");
    assert_eq!(h.what, "the spreadsheet template we talked about");
    assert_eq!(h.from, "Eric");
    assert!(!h.carries_history, "a handoff must never carry your history");
}

#[test]
fn a_handoff_claiming_to_carry_history_is_refused_before_it_leaves() {
    // Unreachable through `share_with_friend`, which hard-codes false.
    // Refused anyway at the last point before the bytes leave the machine,
    // because "nothing else travels" is a promise and not a hope.
    let bad = Handoff {
        what: "here".into(),
        from: "Eric".into(),
        carries_history: true,
    };
    let to = atlas::kin::Contact {
        name: "Priya".into(),
        // Deliberately unroutable: this must fail on the claim, before it
        // ever tries to connect, or the test would be proving the network is
        // down rather than that the check works.
        host: "127.0.0.1".into(),
        port: 1,
        token: PEER_TOKEN.into(),
    };
    let err = link_to(&to, std::time::Duration::from_millis(50)).hand_note(&to.name, &bad, None)
        .expect_err("a handoff claiming history should not be sent");
    assert!(err.contains("carry history"), "refused for the wrong reason: {err}");
}

#[test]
fn a_note_with_quotes_and_newlines_survives_being_put_on_the_wire() {
    // Notes are free text. The first version of the sender pasted them
    // straight into a JSON body, which either breaks the parse or -- worse
    // -- lets the note's own content close a field and open another.
    let s = json_string_for_test("she said \"no\"\nand\tleft \\ ");
    assert_eq!(s, "\"she said \\\"no\\\"\\nand\\tleft \\\\ \"");
    let ctrl = json_string_for_test("a\u{1}b");
    assert_eq!(ctrl, "\"a\\u0001b\"");
}

// ================= the door =================

#[test]
fn a_token_nobody_registered_hands_over_nothing() {
    let mut d = door();
    assert_eq!(d.receive_handoff(OTHER_TOKEN, "have this", 100), Err(Refused::UnknownPeer));
}

#[test]
fn the_name_recorded_is_the_one_you_gave_them_not_the_one_they_claim() {
    // A sender can write anything in a body. Only the token says who they
    // are, and the name you see is the name you chose when you paired --
    // otherwise a paired friend could make a note appear to come from your
    // accountant.
    let d = locked();
    let got = route_handoff(&handoff_request("the invoice is ready"), &d, PEER_TOKEN);
    match got {
        Some(Action::Handed(h)) => assert_eq!(h.from, "Priya"),
        other => panic!("expected a handoff: {other:?}"),
    }
}

#[test]
fn an_empty_note_is_refused_rather_than_filed() {
    let mut d = door();
    assert_eq!(d.receive_handoff(PEER_TOKEN, "   ", 100), Err(Refused::Empty));
}

#[test]
fn notes_are_rate_limited_on_their_own_counter_not_the_signal_one() {
    // Sharing a bucket would mean three notes from a friend blocking their
    // next urgent signal for an hour, which is the wrong failure: a signal
    // interrupts you and a note waits in a list.
    let mut d = door();
    for i in 0..MAX_HANDOFFS_PER_WINDOW {
        assert!(
            d.receive_handoff(PEER_TOKEN, &format!("note {i}"), 100).is_ok(),
            "refused note {i} of {MAX_HANDOFFS_PER_WINDOW}"
        );
    }
    assert_eq!(d.receive_handoff(PEER_TOKEN, "one too many", 100), Err(Refused::TooMany));

    // And having exhausted the note budget, an urgent signal still gets
    // through -- the whole point of the separate counter.
    assert!(
        d.receive(PEER_TOKEN, "the server is down", atlas::kin::Urgency::Urgent, 100).is_ok(),
        "notes starved out a signal"
    );
    assert!(MAX_HANDOFFS_PER_WINDOW > MAX_PER_WINDOW, "the quieter thing gets the looser cap");
}

#[test]
fn the_note_budget_recovers_after_the_window() {
    let mut d = door();
    for i in 0..MAX_HANDOFFS_PER_WINDOW {
        d.receive_handoff(PEER_TOKEN, &format!("note {i}"), 100).unwrap();
    }
    assert!(d.receive_handoff(PEER_TOKEN, "later", 100 + atlas::kin::WINDOW_SECS).is_ok());
}

// ================= the boundary =================

#[test]
fn the_handoff_endpoint_and_the_signal_endpoint_do_not_answer_for_each_other() {
    // Two endpoints, two destinations, no shared path. A note arriving as a
    // signal would interrupt you with content; a signal arriving as a note
    // would silently file something urgent.
    let d = locked();
    let mut signal = handoff_request("have this");
    signal.path = "/signal".into();
    assert!(
        route_handoff(&signal, &d, PEER_TOKEN).is_none(),
        "the handoff router answered for /signal"
    );

    let d2 = locked();
    let note = handoff_request("have this");
    assert!(
        route_signal(&note, &d2, PEER_TOKEN).is_none(),
        "the signal router answered for /handoff"
    );
}

#[test]
fn a_peer_token_still_reaches_nothing_in_the_general_action_space() {
    // The claim `kin_server_boundary.rs` makes about signals, re-made now
    // that a second peer-reachable endpoint exists. Adding one is exactly
    // when a hole gets opened by accident.
    let say = Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/handoff".into(),
        token: Some(PEER_TOKEN.into()),
        body: r#"{"what":"open chrome","text":"open chrome"}"#.into(),
    };
    assert_eq!(route(&say), None, "/handoff is reachable through the general router");
}

// ================= where it lands =================

#[test]
fn a_note_waits_in_a_list_rather_than_going_into_the_tray() {
    // The safety property this whole design turns on. If a handoff went
    // straight into the tray, arriving would be enough to make Atlas go and
    // read it -- and a link is a fetch.
    let dir = tmp("waits");
    let store = Store::new(&dir);
    let mut inbox = Inbox::load(&store);
    let d = Delivered {
        from: "Priya".into(),
        what: "https://example.com/the-thing".into(),
        at: 100,
        file: None,
    };
    let id = as_waiting(&d, &mut inbox, &dir).unwrap();
    inbox.save(&store).unwrap();

    assert_eq!(inbox.get(id).map(|r| r.what.as_str()), Some("https://example.com/the-thing"));
    let tray = atlas::tray::Tray::load(&store);
    assert!(tray.open().is_empty(), "an unasked note reached the tray on arrival");
}

#[test]
fn keeping_it_is_what_puts_it_in_the_tray() {
    // The other half: the deliberate act works, and the note leaves the
    // waiting list when it does, so it cannot be kept twice.
    let store = Store::new(tmp("keep"));
    let mut inbox = Inbox::load(&store);
    let id = inbox.add("https://example.com/the-thing", "Priya", 100);

    let got = inbox.take(id).expect("it was waiting");
    let mut tray = atlas::tray::Tray::load(&store);
    let tid = tray.hand(&got.what, &Space::Personal, &got.from, got.at).unwrap();

    assert_eq!(tray.open().len(), 1);
    assert_eq!(tray.open()[0].id, tid);
    assert_eq!(tray.open()[0].from, "Priya", "the tray should say who it came from");
    assert!(inbox.get(id).is_none(), "it is still waiting as well as kept");
}

#[test]
fn the_same_note_twice_from_the_same_friend_is_one_entry() {
    // A resend because the first did not seem to land is one intention.
    let mut inbox = Inbox::default();
    let a = inbox.add("the spreadsheet template", "Priya", 100);
    let b = inbox.add("the spreadsheet template", "Priya", 200);
    assert_eq!(a, b);
    assert_eq!(inbox.items.len(), 1);
}

#[test]
fn the_same_words_from_a_different_friend_are_two_things() {
    let mut inbox = Inbox::default();
    inbox.add("have a look at this", "Priya", 100);
    inbox.add("have a look at this", "Sam", 100);
    assert_eq!(inbox.items.len(), 2, "two people saying the same thing is two things");
}

#[test]
fn the_waiting_list_is_bounded() {
    // A peer that has gone wrong should fill a list, not a disk.
    let mut inbox = Inbox::default();
    for i in 0..(MAX_WAITING + 20) {
        inbox.add(&format!("note {i}"), "Priya", 100);
    }
    assert_eq!(inbox.items.len(), MAX_WAITING);
    // Oldest dropped, newest kept: what just arrived is what you have not
    // seen yet.
    assert!(inbox.items.iter().any(|i| i.what == format!("note {}", MAX_WAITING + 19)));
    assert!(!inbox.items.iter().any(|i| i.what == "note 0"));
}

#[test]
fn it_can_say_what_is_waiting_without_reading_any_of_it_out() {
    let mut inbox = Inbox::default();
    assert_eq!(inbox.spoken(), "Nothing from anyone.");
    inbox.add("the spreadsheet template", "Priya", 100);
    assert_eq!(inbox.spoken(), "One thing, from Priya.");
    inbox.add("and this one", "Sam", 100);
    let two = inbox.spoken();
    assert!(two.contains("Priya") && two.contains("Sam"), "{two}");
    assert!(
        !two.contains("spreadsheet"),
        "the contents of an unasked note were read out loud: {two}"
    );
}

// ================= the whole round trip, over a real socket =============

/// Everything above tests one end or the other. This is the only test that
/// runs the real sender against the real listener: `share_with_friend`
/// builds it, `PeerLink::hand_note` puts it on a socket through the real HTTP
/// client, the real door checks the token, and a `Delivered` comes out the
/// far side. Loopback rather than two machines — that first run is still
/// owed, and is named as such in the outstanding list.
#[test]
fn a_note_sent_by_the_real_sender_arrives_at_the_real_door() {
    use std::time::{Duration, Instant};

    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let listener =
        atlas::server::SignalListener::bind(port, vec![Peer::new("Priya", PEER_TOKEN)]).unwrap();

    let to = atlas::kin::Contact {
        name: "Priya".into(),
        host: "127.0.0.1".into(),
        port,
        token: PEER_TOKEN.into(),
    };
    // A note with the characters that broke the first version of the sender.
    let note = "the \"template\" we talked about\nsecond line";
    let handoff = share_with_friend(note, "Eric");

    let sender = std::thread::spawn(move || {
        link_to(&to, Duration::from_secs(5)).hand_note(&to.name, &handoff, None)
    });

    // The daemon's loop polls; a single poll racing a client that has not
    // reached connect() yet is a real possibility, not a flake.
    let start = Instant::now();
    let arrived = loop {
        if let Some(a) = listener.poll_once(1) {
            break Some(a);
        }
        if start.elapsed() > Duration::from_secs(3) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    sender.join().unwrap().expect("the send should succeed");

    let Some(atlas::kin::Arrived::Handoff(d)) = arrived else {
        panic!("nothing arrived, or it arrived as the wrong thing: {arrived:?}")
    };
    assert_eq!(d.what, note, "the note did not survive the wire intact");
    assert_eq!(d.from, "Priya");

    // And the far side files it where a note goes, not in the tray.
    let dir = tmp("roundtrip");
    let store = Store::new(&dir);
    let mut inbox = Inbox::load(&store);
    as_waiting(&d, &mut inbox, &dir).unwrap();
    inbox.save(&store).unwrap();
    assert_eq!(inbox.items.len(), 1);
    assert!(atlas::tray::Tray::load(&store).open().is_empty());
}

// ================= files =================
//
// Three sub-decisions had to be settled before any of this could be built,
// and each is asserted here rather than only written down:
//
//   size cap  — 8MB, below the tray's own 20MB. base64 inflates by 4/3 and
//               the whole body is read into memory by a listener that
//               services one connection at a time; and this arrives unasked,
//               which deserves a tighter bound than something you handed
//               Atlas yourself.
//   where     — a `handoffs/` doorstep folder, never the tray folder. The
//               tray is what Atlas reads.
//   may open  — no. Not on arrival, not to list it. `keep` is what grants
//               that, exactly as for a note.

use atlas::household::{ReceivedFile, HANDOFF_FOLDER};
use atlas::kin::{DeliveredFile, MAX_HANDOFF_FILE_BYTES};

fn delivered_file(name: &str, bytes: &[u8]) -> Delivered {
    Delivered {
        from: "Priya".into(),
        what: "the thing we talked about".into(),
        at: 100,
        file: Some(DeliveredFile { name: name.into(), bytes: bytes.to_vec() }),
    }
}

#[test]
fn a_file_lands_on_the_doorstep_and_not_in_the_tray() {
    let dir = tmp("file-waits");
    let store = Store::new(&dir);
    let mut inbox = Inbox::load(&store);

    let id = as_waiting(&delivered_file("report.pdf", b"%PDF-1.7 pretend"), &mut inbox, &dir)
        .expect("it should be kept");
    inbox.save(&store).unwrap();

    let got = inbox.get(id).expect("it is waiting");
    let f = got.file.as_ref().expect("the file is recorded");
    assert_eq!(f.name, "report.pdf");
    assert_eq!(f.size, 16);
    assert!(f.stored_at.starts_with(HANDOFF_FOLDER), "stored somewhere else: {}", f.stored_at);
    assert!(dir.join(&f.stored_at).is_file(), "the bytes were not written");

    // The whole point. Nothing has looked at it, and it is not where Atlas
    // looks.
    assert!(atlas::tray::Tray::load(&store).open().is_empty(), "a peer's file reached the tray");
    assert!(!dir.join(atlas::tray::FOLDER).join("report.pdf").exists());
}

#[test]
fn a_file_over_the_cap_is_refused_at_the_door() {
    let mut d = door();
    let big = vec![0u8; MAX_HANDOFF_FILE_BYTES + 1];
    assert_eq!(
        d.receive_handoff_file(PEER_TOKEN, "here", "huge.bin", big, 100),
        Err(Refused::TooBig)
    );
    // And says something useful rather than just no.
    assert!(Refused::TooBig.plain().contains("send the link instead"));
}

#[test]
fn the_peer_cap_is_below_the_trays_own() {
    // Not a tautology: these are two different numbers for two different
    // situations, and the peer one being the smaller is the decision.
    assert!(
        MAX_HANDOFF_FILE_BYTES < atlas::tray::MAX_FILE_BYTES,
        "a file arriving unasked should not be allowed to be larger than one you handed over"
    );
}

#[test]
fn an_empty_file_is_refused_rather_than_filed_as_an_empty_note() {
    let mut d = door();
    assert_eq!(
        d.receive_handoff_file(PEER_TOKEN, "here", "nothing.txt", Vec::new(), 100),
        Err(Refused::Empty)
    );
}

#[test]
fn a_file_with_no_covering_line_is_still_a_handoff() {
    // `atlas share --file report.pdf --to Priya` is a complete sentence.
    let mut d = door();
    let got = d
        .receive_handoff_file(PEER_TOKEN, "", "report.pdf", b"data".to_vec(), 100)
        .expect("a file needs no covering note");
    assert_eq!(got.file.expect("the file").name, "report.pdf");
}

#[test]
fn a_filename_from_another_machine_cannot_escape_the_folder() {
    // The single most dangerous field in this whole feature: a string chosen
    // by the sender that is about to be turned into a path.
    let dir = tmp("escape");
    let store = Store::new(&dir);
    let mut d = door();

    for nasty in ["../../etc/passwd", "..\\..\\windows\\system32\\evil.dll", "/etc/shadow"] {
        let got = d
            .receive_handoff_file(PEER_TOKEN, "here", nasty, b"x".to_vec(), 100)
            .expect("it is accepted, just renamed");
        let name = &got.file.as_ref().unwrap().name;
        assert!(!name.contains(".."), "{nasty} kept its dots: {name}");
        assert!(!name.contains('/') && !name.contains('\\'), "{nasty} kept a separator: {name}");

        let mut inbox = Inbox::load(&store);
        let id = as_waiting(&got, &mut inbox, &dir).expect("kept");
        let stored = inbox.get(id).unwrap().file.as_ref().unwrap().stored_at.clone();
        let full = dir.join(&stored);
        assert!(
            full.starts_with(dir.join(HANDOFF_FOLDER)),
            "{nasty} wrote outside the doorstep: {}",
            full.display()
        );
    }
}

#[test]
fn two_different_files_from_one_friend_are_two_things() {
    // The note dedup keys on the words. Two files sent with the same
    // covering line are two files, and collapsing them would drop one.
    let dir = tmp("twofiles");
    let store = Store::new(&dir);
    let mut inbox = Inbox::load(&store);
    as_waiting(&delivered_file("one.pdf", b"aaaa"), &mut inbox, &dir).unwrap();
    as_waiting(&delivered_file("two.pdf", b"bbbbbb"), &mut inbox, &dir).unwrap();
    assert_eq!(inbox.items.len(), 2, "the second file replaced the first");
}

#[test]
fn the_same_file_sent_twice_is_one_thing() {
    let dir = tmp("resend");
    let store = Store::new(&dir);
    let mut inbox = Inbox::load(&store);
    let a = as_waiting(&delivered_file("one.pdf", b"aaaa"), &mut inbox, &dir).unwrap();
    let b = as_waiting(&delivered_file("one.pdf", b"aaaa"), &mut inbox, &dir).unwrap();
    assert_eq!(a, b);
    assert_eq!(inbox.items.len(), 1);
}

#[test]
fn keeping_a_file_is_what_puts_it_where_atlas_reads() {
    let dir = tmp("file-keep");
    let store = Store::new(&dir);
    let mut inbox = Inbox::load(&store);
    let id = as_waiting(&delivered_file("report.pdf", b"%PDF-1.7 pretend"), &mut inbox, &dir)
        .unwrap();

    let got = inbox.take(id).expect("waiting");
    let f: ReceivedFile = got.file.clone().expect("a file");
    let bytes = std::fs::read(dir.join(&f.stored_at)).expect("the doorstep copy");
    let mut tray = atlas::tray::Tray::load(&store);
    let tid = tray
        .hand_file(&f.name, &bytes, &Space::Personal, &got.from, None, got.at, &dir)
        .expect("kept");

    assert_eq!(tray.open().len(), 1);
    assert_eq!(tray.open()[0].id, tid);
    assert_eq!(tray.open()[0].from, "Priya");
    // The covering line their end wrote must never land in `asked` -- that
    // field is documented as the only text on an item Atlas treats as coming
    // from you.
    assert_eq!(tray.open()[0].asked, None, "a friend's words were filed as your instruction");
}

#[test]
fn a_file_sent_by_the_real_sender_arrives_intact_at_the_real_door() {
    // The note version of this test is what caught the sender putting its
    // token in the body instead of the header — both halves were
    // individually correct and the pair was broken. A file adds base64, a
    // much larger body, and the listener's per-endpoint body cap, so it gets
    // its own round trip rather than trusting the note one to cover it.
    use std::time::{Duration, Instant};

    let dir = tmp("file-roundtrip");
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let listener =
        atlas::server::SignalListener::bind(port, vec![Peer::new("Priya", PEER_TOKEN)]).unwrap();

    // Deliberately bigger than the old flat 4096-byte body cap, and full of
    // bytes that are not text: a cap that truncates, or an encoder that
    // mangles high bytes, both show up here and nowhere else.
    let payload: Vec<u8> = (0..40_000u32).map(|i| (i % 251) as u8).collect();
    let src = dir.join("photo.bin");
    std::fs::write(&src, &payload).unwrap();

    let to = atlas::kin::Contact {
        name: "Priya".into(),
        host: "127.0.0.1".into(),
        port,
        token: PEER_TOKEN.into(),
    };
    let handoff = share_with_friend("the photo from Saturday", "Eric");
    let sender = std::thread::spawn(move || {
        link_to(&to, Duration::from_secs(30)).hand_note(&to.name, &handoff, Some(&src))
    });

    let start = Instant::now();
    let arrived = loop {
        if let Some(a) = listener.poll_once(1) {
            break Some(a);
        }
        if start.elapsed() > Duration::from_secs(10) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    sender.join().unwrap().expect("the send should succeed");

    let Some(atlas::kin::Arrived::Handoff(d)) = arrived else {
        panic!("nothing arrived, or the wrong thing: {arrived:?}")
    };
    let f = d.file.as_ref().expect("a file came with it");
    assert_eq!(f.name, "photo.bin");
    assert_eq!(f.bytes.len(), payload.len(), "the file was truncated on the way");
    assert_eq!(f.bytes, payload, "the bytes changed on the way");
    assert_eq!(d.what, "the photo from Saturday");
}

#[test]
fn a_body_claiming_a_file_that_does_not_decode_is_refused_outright() {
    // Not delivered as a bare note. Half a file looks exactly like a whole
    // one on a list, and the covering line would still read fine.
    let d = locked();
    let req = Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/handoff".into(),
        token: Some(PEER_TOKEN.into()),
        body: r#"{"what":"here","name":"x.bin","data":"!!!not base64!!!"}"#.into(),
    };
    assert!(route_handoff(&req, &d, PEER_TOKEN).is_none(), "a corrupt file was accepted");
}

#[test]
fn a_body_with_a_name_but_no_data_is_refused_rather_than_read_as_a_note() {
    let d = locked();
    let req = Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/handoff".into(),
        token: Some(PEER_TOKEN.into()),
        body: r#"{"what":"here","name":"x.bin"}"#.into(),
    };
    assert!(route_handoff(&req, &d, PEER_TOKEN).is_none(), "a headless file became a note");
}

/// The link a paired Atlas sends through, with just this one contact.
fn link_to(c: &atlas::kin::Contact, t: std::time::Duration) -> atlas::kin::PeerLink {
    let mut p = atlas::kin::Pairings::default();
    p.contacts.push(c.clone());
    atlas::kin::PeerLink::from_state(&p, &atlas::chat::Chats::default()).waiting(t)
}
