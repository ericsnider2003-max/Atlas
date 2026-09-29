//! Leaving a group.
//!
//! Leaving has to be more than deleting your copy of the room, because the
//! other members' Atlases do not all know at once. A message already in flight
//! — or one from a member who never got your notice — would re-open the room
//! through `open_group` and put you back in a conversation you walked out of.
//! So leaving tombstones the group id, and these defend that the tombstone
//! holds, that a member leaving is taken out of your copy, and that the notice
//! crosses the peer boundary as its own thing.

use atlas::chat::{Chats, Delivery, Message, Refused, Room, ME};
use atlas::earned::Space;
use atlas::kin::{Door, Pairings, Peer};
use atlas::roster::Roster;

fn group_room(id: &str, name: &str, members: &[&str]) -> Room {
    Room {
        id: id.into(),
        name: name.into(),
        space: Space::Personal,
        members: members.iter().map(|m| m.to_string()).collect(),
        messages: vec![Message {
            id: format!("{id}-1000-1"),
            from: "Ali".into(),
            body: "hi".into(),
            sent_at: 1_000,
            sent_offset_mins: 0,
            after: 1,
            to: vec![(ME.to_string(), Delivery::Arrived(1_000))],
        }],
        read_through: 0,
        receipt_through: 0,
    }
}

fn with_group(id: &str, name: &str, members: &[&str]) -> Chats {
    let mut chats = Chats::default();
    chats.rooms.push(group_room(id, name, members));
    chats
}

// --- the tombstone ----------------------------------------------------------

#[test]
fn leaving_removes_the_room_and_remembers_the_id() {
    let mut chats = with_group("grp-1", "Roof crew", &["Ali", "Jordan"]);
    let members = chats.leave_group("grp-1");

    assert_eq!(members, vec!["Ali".to_string(), "Jordan".to_string()], "leaving hands back who to tell");
    assert!(chats.group_named("Roof crew").is_none(), "the room is gone");
    assert!(chats.has_left("grp-1"), "and the id is tombstoned");
}

#[test]
fn a_message_in_flight_cannot_reopen_a_left_group() {
    let mut chats = with_group("grp-1", "Roof crew", &["Ali", "Jordan"]);
    chats.leave_group("grp-1");

    // open_group is what the receive path calls for an incoming group message.
    // For a left group it must refuse rather than quietly re-create the room.
    let roster = Roster::default();
    let pairings = Pairings::default();
    let got = chats.open_group(
        "grp-1",
        "Roof crew",
        Space::Personal,
        &["Ali".into()],
        &roster,
        &pairings,
    );
    assert_eq!(got, Err(Refused::LeftThatGroup("grp-1".into())));
    assert!(chats.group_named("Roof crew").is_none(), "still gone after the attempt");
}

#[test]
fn leaving_a_fresh_group_with_the_same_people_still_works() {
    // Leaving is keyed by id, not by who is in it. A brand-new group the same
    // people start later mints a new id, which the tombstone does not block.
    let mut chats = with_group("grp-1", "Roof crew", &["Ali", "Jordan"]);
    chats.leave_group("grp-1");

    let roster = Roster::default();
    let pairings = Pairings::default();
    let got =
        chats.open_group("grp-2", "Roof crew II", Space::Personal, &["Ali".into()], &roster, &pairings);
    assert!(got.is_ok(), "a new group id is not blocked by an old tombstone: {got:?}");
}

#[test]
fn leaving_is_idempotent() {
    let mut chats = with_group("grp-1", "Roof crew", &["Ali", "Jordan"]);
    assert_eq!(chats.leave_group("grp-1").len(), 2);
    // Leaving again — or leaving one you never had — is empty but still keeps
    // the block.
    assert!(chats.leave_group("grp-1").is_empty());
    assert!(chats.leave_group("never-had-it").is_empty());
    assert!(chats.has_left("never-had-it"));
}

// --- somebody else leaving --------------------------------------------------

#[test]
fn a_member_leaving_is_taken_out_of_your_copy() {
    let mut chats = with_group("grp-1", "Roof crew", &["Ali", "Jordan", "Maya"]);
    assert!(chats.member_left("grp-1", "Jordan"));

    let room = chats.group_named("Roof crew").unwrap();
    assert!(!room.members.iter().any(|m| m == "Jordan"), "Jordan is gone");
    assert_eq!(room.members.len(), 2);
    // Their past messages stay — they said those while they were in it.
    assert_eq!(room.messages.len(), 1);
}

#[test]
fn a_member_leaving_a_group_you_dont_hold_is_a_no_op() {
    let mut chats = with_group("grp-1", "Roof crew", &["Ali", "Jordan"]);
    assert!(!chats.member_left("some-other-group", "Jordan"));
    assert!(!chats.member_left("grp-1", "Nobody-By-That-Name"));
}

// --- the door: a leave notice is not a message ------------------------------

#[test]
fn the_door_names_who_left_from_the_token() {
    let mut door = Door::new(vec![Peer::new("Ali", "ali-token")]);
    let got = door
        .receive_left("ali-token", "grp-1", 2_000)
        .expect("a known peer's leave notice is accepted");
    assert_eq!(got.from, "Ali", "who left comes from the token");
    assert_eq!(got.group_id, "grp-1");
}

#[test]
fn a_leave_notice_with_no_group_is_refused() {
    let mut door = Door::new(vec![Peer::new("Ali", "ali-token")]);
    assert!(door.receive_left("ali-token", "", 2_000).is_err());
    assert!(door.receive_left("ali-token", "   ", 2_000).is_err());
}

#[test]
fn a_stranger_cannot_send_a_leave_notice() {
    let mut door = Door::new(vec![Peer::new("Ali", "ali-token")]);
    assert!(door.receive_left("not-a-peer", "grp-1", 2_000).is_err());
}
