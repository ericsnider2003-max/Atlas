//! Read receipts across in-house messaging.
//!
//! Delivery already answered "did it get to their machine". A read receipt
//! answers the narrower, separate question — "has the person actually read
//! it" — and the whole point of keeping it a separate state is that the two
//! are not the same claim. These defend that the receipt is recorded only
//! when the far side sends one, that it travels the wire as its own thing
//! (never a message with a body), and that the reader emits one exactly once
//! when their read mark passes your message.

use atlas::chat::{Chats, Delivery, Message, Room, ME};
use atlas::courier::{send_receipts, Receipts};
use atlas::earned::Space;
use atlas::kin::{Door, Peer};

/// A message as it would arrive from somebody else, already on their machine.
fn from_them(from: &str, body: &str, sent_at: u64, after: u64) -> Message {
    Message {
        id: format!("{from}-{sent_at}-{after}"),
        from: from.to_string(),
        body: body.to_string(),
        sent_at,
        sent_offset_mins: 0,
        after,
        to: vec![(ME.to_string(), Delivery::Arrived(sent_at))],
    }
}

/// One of your own messages, delivered to everyone named.
fn mine(id: &str, after: u64, to: &[(&str, Delivery)]) -> Message {
    Message {
        id: id.to_string(),
        from: ME.to_string(),
        body: "ok".into(),
        sent_at: 1_000,
        sent_offset_mins: 0,
        after,
        to: to.iter().map(|(n, d)| (n.to_string(), d.clone())).collect(),
    }
}

fn one_to_one(name: &str, messages: Vec<Message>, read_through: u64) -> Chats {
    let mut chats = Chats::default();
    chats.rooms.push(Room {
        id: format!("room-{name}"),
        name: name.to_string(),
        space: Space::Personal,
        members: vec![name.to_string()],
        messages,
        read_through,
        receipt_through: 0,
    });
    chats
}

// --- the state itself -------------------------------------------------------

#[test]
fn read_is_a_stronger_fact_than_arrived() {
    // Read implies arrived — a message cannot have been read without first
    // getting there — so `arrived()` is true for it, but `read()` is the
    // narrower question only `Read` answers yes.
    let r = Delivery::Read(9);
    assert!(r.arrived(), "a read message has, necessarily, arrived");
    assert!(r.read());
    assert!(!Delivery::Arrived(9).read(), "arrived is not the same as read");
    assert_eq!(r.plain(), "read");
}

#[test]
fn a_receipt_upgrades_the_recipient_and_finds_it_by_id() {
    // The reader cannot name the sender's room — a one-to-one id is built from
    // member names, which differ on each end — so a receipt carries the
    // message id and this looks it up across rooms.
    let m = mine("roof-1000-3", 3, &[("Maya", Delivery::Arrived(1_100))]);
    let mut chats = one_to_one("Maya", vec![m], 0);

    assert!(chats.mark_read("roof-1000-3", "Maya", 2_000));
    let state = &chats.rooms[0].messages[0].to[0].1;
    assert_eq!(*state, Delivery::Read(2_000));
}

#[test]
fn a_receipt_for_a_non_recipient_is_dropped_not_invented() {
    let m = mine("roof-1000-3", 3, &[("Maya", Delivery::Arrived(1_100))]);
    let mut chats = one_to_one("Maya", vec![m], 0);

    assert!(!chats.mark_read("roof-1000-3", "Somebody-Else", 2_000));
    assert_eq!(chats.rooms[0].messages[0].to[0].1, Delivery::Arrived(1_100));
}

#[test]
fn a_read_is_never_downgraded_by_a_later_stuck() {
    // A message can be marked stuck by a failed delivery attempt and still
    // have been read on a path this end could not see. The read is ground
    // truth and wins; `mark_stuck` must not overwrite it.
    let m = mine("roof-1000-3", 3, &[("Maya", Delivery::Read(2_000))]);
    let mut chats = one_to_one("Maya", vec![m], 0);

    assert!(!chats.mark_stuck("room-Maya", "roof-1000-3", "Maya", "no route"));
    assert_eq!(chats.rooms[0].messages[0].to[0].1, Delivery::Read(2_000));
}

#[test]
fn fully_read_and_arrived_unread_are_the_summary_and_the_detail() {
    // The two halves a group needs: "has everyone seen it" and, when not,
    // "who hasn't".
    let seen = mine(
        "g-1000-2",
        2,
        &[("Ali", Delivery::Read(10)), ("Jordan", Delivery::Read(11))],
    );
    assert!(seen.fully_read());
    assert!(seen.arrived_unread().is_empty());

    let partly = mine(
        "g-1000-3",
        3,
        &[("Ali", Delivery::Read(10)), ("Jordan", Delivery::Arrived(11))],
    );
    assert!(!partly.fully_read());
    assert_eq!(partly.arrived_unread(), vec!["Jordan"]);
}

#[test]
fn per_message_read_accessors() {
    let m = mine(
        "g-1000-4",
        4,
        &[
            ("Ali", Delivery::Read(10)),
            ("Jordan", Delivery::Arrived(11)),
            ("Maya", Delivery::Waiting),
        ],
    );
    assert_eq!(m.read_by(), vec!["Ali"]);
    assert!(m.any_arrived(), "Ali and Jordan have it");

    // A message with nobody having it yet.
    let waiting = mine("g-1000-5", 5, &[("Ali", Delivery::Waiting)]);
    assert!(!waiting.any_arrived());
}

#[test]
fn last_mine_is_your_most_recent_message_in_the_room() {
    let mut chats = one_to_one(
        "Sam",
        vec![
            from_them("Sam", "you there?", 900, 1),
            mine("room-Sam-1000-2", 2, &[("Sam", Delivery::Arrived(1_000))]),
        ],
        0,
    );
    let last = chats.rooms[0].last_mine().expect("you sent one");
    assert_eq!(last.id, "room-Sam-1000-2");
    // A room where you've said nothing has no last-of-yours.
    chats.rooms[0].messages.retain(|m| m.from != ME);
    assert!(chats.rooms[0].last_mine().is_none());
}

// --- emitting a receipt when you read --------------------------------------

/// Records every receipt it is asked to send, and can be told to fail for one
/// person so the "held, not guessed" retry can be checked.
#[derive(Default)]
struct RecordingLink {
    sent: std::cell::RefCell<Vec<(String, Vec<String>)>>,
    refuse: Vec<String>,
}

impl Receipts for RecordingLink {
    fn read(&self, peer: &str, ids: &[String]) -> bool {
        if self.refuse.iter().any(|r| r == peer) {
            return false;
        }
        self.sent.borrow_mut().push((peer.to_string(), ids.to_vec()));
        true
    }
}

#[test]
fn reading_their_message_owes_them_one_receipt_then_no_more() {
    // Their message is read (read_through past it); we owe Sam a receipt.
    let incoming = from_them("Sam", "you around?", 900, 4);
    let mut chats = one_to_one("Sam", vec![incoming], 4);

    let link = RecordingLink::default();
    let accepted = send_receipts(&mut chats, &link, 5_000);
    assert_eq!(accepted, 1);
    assert_eq!(link.sent.borrow().len(), 1, "exactly one receipt");
    assert_eq!(link.sent.borrow()[0].0, "Sam");
    assert_eq!(link.sent.borrow()[0].1, vec!["Sam-900-4".to_string()]);

    // Sent once: a second pass with nothing newly read sends nothing.
    let again = send_receipts(&mut chats, &link, 5_100);
    assert_eq!(again, 0, "a receipt is owed once, not on every pass");
    assert_eq!(link.sent.borrow().len(), 1);
}

#[test]
fn a_receipt_that_did_not_land_is_re_owed_not_lost() {
    let incoming = from_them("Sam", "you around?", 900, 4);
    let mut chats = one_to_one("Sam", vec![incoming], 4);

    // Sam is unreachable this pass.
    let refusing = RecordingLink { refuse: vec!["Sam".into()], ..Default::default() };
    assert_eq!(send_receipts(&mut chats, &refusing, 5_000), 0);
    assert_eq!(chats.rooms[0].receipt_through, 0, "a failed pass does not advance the mark");

    // Next pass, Sam is up: the receipt is still owed and now goes.
    let ok = RecordingLink::default();
    assert_eq!(send_receipts(&mut chats, &ok, 5_100), 1);
    assert_eq!(ok.sent.borrow()[0].0, "Sam");
}

#[test]
fn your_own_messages_never_owe_you_a_receipt() {
    // A room where the only thing past the read mark is your own message.
    let m = mine("room-Sam-1000-4", 4, &[("Sam", Delivery::Arrived(1_100))]);
    let mut chats = one_to_one("Sam", vec![m], 4);

    let link = RecordingLink::default();
    assert_eq!(send_receipts(&mut chats, &link, 5_000), 0);
    assert!(link.sent.borrow().is_empty(), "you don't send yourself a receipt");
    // The mark still advances so the range is not reconsidered every pass.
    assert_eq!(chats.rooms[0].receipt_through, 4);
}

// --- the door: a receipt is not a message ----------------------------------

#[test]
fn the_door_names_the_reader_from_the_token_not_the_body() {
    let mut door = Door::new(vec![Peer::new("Sam", "sam-token")]);
    let got = door
        .receive_read("sam-token", vec!["roof-1000-3".into()], 2_000)
        .expect("a known peer's receipt is accepted");
    assert_eq!(got.from, "Sam", "who read it comes from the token");
    assert_eq!(got.ids, vec!["roof-1000-3".to_string()]);
    assert_eq!(got.at, 2_000);
}

#[test]
fn an_empty_receipt_is_refused() {
    // A receipt with no ids is not a receipt; it must not become a bare knock
    // an idle peer can use to reach you.
    let mut door = Door::new(vec![Peer::new("Sam", "sam-token")]);
    assert!(door.receive_read("sam-token", vec![], 2_000).is_err());
    assert!(door.receive_read("sam-token", vec!["   ".into()], 2_000).is_err());
}

#[test]
fn a_stranger_cannot_send_a_receipt() {
    let mut door = Door::new(vec![Peer::new("Sam", "sam-token")]);
    assert!(door.receive_read("not-a-peer", vec!["x".into()], 2_000).is_err());
}

// --- the glance-at view -----------------------------------------------------

// Everyone is reachable, for the tests that don't care about the mesh limit.
fn all_reachable(_who: &str) -> bool {
    true
}

#[test]
fn read_state_names_who_read_and_who_hasnt_in_a_group() {
    let mut chats = Chats::default();
    // A one-to-one where Maya has read it.
    chats.rooms.push(Room {
        id: "room-maya".into(),
        name: "Maya".into(),
        space: Space::Personal,
        members: vec!["Maya".into()],
        messages: vec![mine("room-maya-1000-2", 2, &[("Maya", Delivery::Read(2_000))])],
        read_through: 0,
        receipt_through: 0,
    });
    // A group where Ali read it and Jordan only received it.
    chats.rooms.push(Room {
        id: "grp".into(),
        name: "Roof crew".into(),
        space: Space::Personal,
        members: vec!["Ali".into(), "Jordan".into()],
        messages: vec![mine(
            "grp-1000-2",
            2,
            &[("Ali", Delivery::Read(2_100)), ("Jordan", Delivery::Arrived(1_500))],
        )],
        read_through: 0,
        receipt_through: 0,
    });

    let lines = chats.read_state(all_reachable);
    assert!(lines.iter().any(|l| l.contains("Maya read your last message")), "{lines:?}");
    // The group names both sides: who read it and who hasn't.
    assert!(
        lines.iter().any(|l| l.contains("Roof crew")
            && l.contains("read by Ali")
            && l.contains("not yet by Jordan")),
        "a group names who has and hasn't seen it: {lines:?}"
    );
}

#[test]
fn a_member_you_cant_reach_is_no_receipt_not_unread() {
    // The mesh's honest half: you get a read receipt only from a member you're
    // paired with. For anyone else, "hasn't read it" would be a lie — you
    // simply can't tell.
    let mut chats = Chats::default();
    chats.rooms.push(Room {
        id: "grp".into(),
        name: "Roof crew".into(),
        space: Space::Personal,
        members: vec!["Ali".into(), "Jordan".into()],
        messages: vec![mine(
            "grp-1000-2",
            2,
            &[("Ali", Delivery::Read(2_100)), ("Jordan", Delivery::Arrived(1_500))],
        )],
        read_through: 0,
        receipt_through: 0,
    });

    // You're paired with Ali but not Jordan.
    let lines = chats.read_state(|who| who == "Ali");
    let line = lines.iter().find(|l| l.contains("Roof crew")).expect("a line for the group");
    assert!(line.contains("read by Ali"), "{line}");
    assert!(
        line.contains("no receipt from Jordan") && line.contains("not paired"),
        "an unreachable member is named as such, not as 'hasn't read': {line}"
    );
    assert!(!line.contains("not yet by Jordan"), "must not claim they haven't read it: {line}");
}

#[test]
fn read_state_says_nothing_about_a_message_still_in_flight() {
    // "Not read" about something that hasn't even arrived would read as a snub
    // when it is a network. Silent until it lands on at least one machine.
    let chats = one_to_one("Sam", vec![mine("room-Sam-1000-2", 2, &[("Sam", Delivery::Waiting)])], 0);
    assert!(chats.read_state(all_reachable).is_empty());
}
