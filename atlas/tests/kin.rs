use atlas::kin::{as_nudge, Door, Peer, Refused, Urgency, MAX_PER_WINDOW, WINDOW_SECS};

const T: u64 = 1_000_000;

fn door() -> Door {
    Door::new(vec![Peer::new("Homelab", "peer-token-not-the-phone-token")])
}

// ================= trust is named, not inherited =================

#[test]
fn an_unregistered_token_is_refused_no_matter_what_it_sends() {
    let mut d = door();
    let r = d.receive("some-other-token", "sell everything", Urgency::Urgent, T);
    assert_eq!(r, Err(Refused::UnknownPeer));
}

#[test]
fn being_reachable_on_the_network_is_not_the_same_as_being_trusted() {
    // No peers registered at all -- a Door with nobody in it refuses
    // everything, which is the correct default for a channel nobody asked to
    // open.
    let mut empty = Door::new(vec![]);
    assert_eq!(
        empty.receive("anything", "hello", Urgency::Info, T),
        Err(Refused::UnknownPeer)
    );
}

#[test]
fn a_registered_peers_own_token_is_accepted() {
    let mut d = door();
    let got = d.receive("peer-token-not-the-phone-token", "position closed", Urgency::Info, T);
    assert!(got.is_ok());
    assert_eq!(got.unwrap().from, "Homelab");
}

#[test]
fn every_incoming_message_says_which_peer_it_came_from() {
    let mut d = door();
    let i = d.receive("peer-token-not-the-phone-token", "hi", Urgency::Info, T).unwrap();
    assert_eq!(i.from, "Homelab");
}

// ================= the one-way rule =================

#[test]
fn an_incoming_message_can_only_become_a_nudge() {
    let mut d = door();
    let i = d.receive("peer-token-not-the-phone-token", "margin call", Urgency::Urgent, T).unwrap();
    let n = as_nudge(&i);
    assert!(n.message.contains("Homelab"));
    assert!(n.message.contains("margin call"));
    assert!(n.relief.is_none(), "there is nothing Atlas can offer to take off your hands here");
}

#[test]
fn urgency_is_reflected_in_confidence_but_never_bypasses_the_nudge_pipeline() {
    let mut d = door();
    let urgent = d.receive("peer-token-not-the-phone-token", "a", Urgency::Urgent, T).unwrap();
    let info = d.receive("peer-token-not-the-phone-token", "b", Urgency::Info, T + 1).unwrap();
    assert!(as_nudge(&urgent).confidence > as_nudge(&info).confidence);
    // Still a Nudge, still going through Offer like everything else -- see
    // server.rs's route_signal, which never constructs an Intent or Action
    // other than Action::Signal.
}

// ================= a channel that is always right stops being read =================

#[test]
fn a_peer_can_be_refused_for_sending_too_much() {
    let mut d = door();
    for i in 0..MAX_PER_WINDOW {
        assert!(d.receive("peer-token-not-the-phone-token", "x", Urgency::Info, T + i as u64).is_ok());
    }
    let over = d.receive("peer-token-not-the-phone-token", "one more", Urgency::Info, T + 99);
    assert_eq!(over, Err(Refused::TooMany));
}

#[test]
fn the_rate_limit_clears_once_the_window_passes() {
    let mut d = door();
    for i in 0..MAX_PER_WINDOW {
        d.receive("peer-token-not-the-phone-token", "x", Urgency::Info, T + i as u64).unwrap();
    }
    assert!(d
        .receive("peer-token-not-the-phone-token", "later", Urgency::Info, T + WINDOW_SECS + 1)
        .is_ok());
}

#[test]
fn standing_reports_the_same_thing_receive_would_enforce() {
    let mut d = door();
    assert!(d.standing("Homelab", T));
    for i in 0..MAX_PER_WINDOW {
        d.receive("peer-token-not-the-phone-token", "x", Urgency::Info, T + i as u64).unwrap();
    }
    assert!(!d.standing("Homelab", T + 10));
}

#[test]
fn an_empty_message_is_refused_rather_than_becoming_a_blank_nudge() {
    let mut d = door();
    assert_eq!(
        d.receive("peer-token-not-the-phone-token", "   ", Urgency::Info, T),
        Err(Refused::Empty)
    );
}

#[test]
fn two_different_peers_have_independent_rate_limits() {
    let mut d = Door::new(vec![
        Peer::new("Homelab", "token-a"),
        Peer::new("SomeOtherAtlas", "token-b"),
    ]);
    for i in 0..MAX_PER_WINDOW {
        d.receive("token-a", "x", Urgency::Info, T + i as u64).unwrap();
    }
    // token-a is now maxed out, but token-b has sent nothing.
    assert_eq!(d.receive("token-a", "one more", Urgency::Info, T + 50), Err(Refused::TooMany));
    assert!(d.receive("token-b", "hello", Urgency::Info, T + 50).is_ok());
}

// Which `Refused` variant fires under which condition is already covered,
// with a real `assert_eq!` on the variant rather than its wording, by
// `an_unregistered_token_is_refused_no_matter_what_it_sends`,
// `a_peer_can_be_refused_for_sending_too_much`, and
// `an_empty_message_is_refused_rather_than_becoming_a_blank_nudge` above. A
// dedicated test of `.plain()`'s wording would only ever assert a phrase is
// present in a constant string — exactly the shape this project's own
// `tests/retrospective.rs` holds a line against, so it stays out rather than
// being added back in a different shape.

// ================= chat: the door and the transport =================

use atlas::chat::{Delivery, Message};
use atlas::kin::{chat_wire_body, Contact, Pairings, PeerLink, RoomMeta, MAX_CHATS_PER_WINDOW};

fn a_message(room_id: &str, body: &str, sent_at: u64, after: u64) -> Message {
    Message {
        id: format!("{room_id}-{sent_at}-{after}"),
        from: atlas::chat::ME.to_string(),
        body: body.to_string(),
        sent_at,
        sent_offset_mins: -300,
        after,
        to: vec![("Sam".to_string(), Delivery::Waiting)],
    }
}

#[test]
fn a_chat_names_its_sender_from_the_token_and_keeps_the_business_claim() {
    let mut d = Door::new(vec![Peer::new("Sam", "sam-token")]);
    let c = d
        .receive_chat("sam-token", Some("Acme".into()), "roof quote is in", 1_000, -300, 5, "r-1000-5", None, None, Vec::new(), T)
        .unwrap();
    assert_eq!(c.from, "Sam", "the sender is the token's peer, never a body claim");
    assert_eq!(c.business.as_deref(), Some("Acme"));
    assert_eq!(c.sent_at, 1_000, "the sender's clock is kept exactly");
    assert_eq!(c.after, 5);
}

#[test]
fn a_chat_from_an_unregistered_token_is_refused() {
    let mut d = Door::new(vec![Peer::new("Sam", "sam-token")]);
    assert_eq!(
        d.receive_chat("not-a-peer", None, "hi", 1, 0, 1, "x", None, None, Vec::new(), T),
        Err(Refused::UnknownPeer)
    );
}

#[test]
fn an_empty_chat_is_refused() {
    let mut d = Door::new(vec![Peer::new("Sam", "sam-token")]);
    assert_eq!(
        d.receive_chat("sam-token", None, "   ", 1, 0, 1, "x", None, None, Vec::new(), T),
        Err(Refused::Empty)
    );
}

#[test]
fn a_peer_flooding_chat_is_eventually_refused() {
    let mut d = Door::new(vec![Peer::new("Sam", "sam-token")]);
    for i in 0..MAX_CHATS_PER_WINDOW {
        d.receive_chat("sam-token", None, "spam", 1, 0, i as u64, &format!("m{i}"), None, None, Vec::new(), T)
            .expect("under the cap these go through");
    }
    assert_eq!(
        d.receive_chat("sam-token", None, "one too many", 1, 0, 999, "over", None, None, Vec::new(), T),
        Err(Refused::TooMany),
        "a peer must not be able to flood you one message at a time forever"
    );
}

#[test]
fn the_wire_form_never_carries_who_it_is_from() {
    // The receiver takes the sender from the token; a `from` in the body would
    // be a name anyone holding the token could write.
    let msg = a_message("sam-personal", "night", 1_000, 5);
    let room = RoomMeta { id: "sam-personal".into(), business: None, name: "Sam".into(), members: vec!["Sam".into()] };
    let wire = chat_wire_body(&room, &msg);
    assert!(!wire.contains("from"), "the body must not carry a sender: {wire}");
    assert!(wire.contains("\"body\":\"night\""));
    assert!(wire.contains("\"sent_at\":1000"));
    assert!(wire.contains("\"after\":5"));
    assert!(wire.contains("\"business\":null"), "a personal message carries a null business");
    assert!(!wire.contains("group_id"), "a one-to-one carries no group fields");
}

#[test]
fn the_wire_form_carries_the_business_for_a_business_room() {
    let msg = a_message("acme-sam", "the invoice", 2_000, 9);
    let room = RoomMeta { id: "acme-sam".into(), business: Some("Acme".into()), name: "Sam".into(), members: vec!["Sam".into()] };
    let wire = chat_wire_body(&room, &msg);
    assert!(wire.contains("\"business\":\"Acme\""));
}

#[test]
fn the_wire_form_of_a_group_carries_its_id_name_and_members() {
    let msg = a_message("grp", "standup", 1_000, 5);
    let room = RoomMeta {
        id: "grp".into(),
        business: None,
        name: "Northwind".into(),
        members: vec!["Jordan".into(), "Maya".into()],
    };
    let wire = chat_wire_body(&room, &msg);
    assert!(wire.contains("\"group_id\":\"grp\""));
    assert!(wire.contains("\"group_name\":\"Northwind\""));
    assert!(wire.contains("\"members\":[\"Jordan\",\"Maya\"]"));
    assert!(!wire.contains("\"from\""), "even a group never carries who it is from");
}

#[test]
fn the_transport_offers_a_paired_contact_as_a_device_and_a_stranger_none() {
    use atlas::courier::Transport;
    let link = PeerLink::from_state(&paired_with_contact("Sam"), &atlas::chat::Chats::default());
    let devices = link.devices_for("Sam");
    assert_eq!(devices.len(), 1, "a paired contact is one reachable device");
    assert_eq!(devices[0].peer, "Sam");
    assert!(link.devices_for("Nobody").is_empty(), "an unpaired name reaches nothing");
}

#[test]
fn the_transport_holds_rather_than_guesses_when_it_cannot_place_the_room() {
    use atlas::courier::{Device, Handoff, Kind, Transport};
    // A contact exists, but the message belongs to no room the snapshot knows,
    // so its firewall side can't be determined: held (NotUp), never sent onto
    // a guessed side. No network is touched, because this is decided first.
    let link = PeerLink::from_state(&paired_with_contact("Sam"), &atlas::chat::Chats::default());
    let msg = a_message("unknown-room", "hi", 1_000, 1);
    let device = Device { peer: "Sam".into(), kind: Kind::Computer, address: "127.0.0.1:1".into() };
    assert_eq!(link.hand_over(&device, &msg), Handoff::NotUp);
}

fn paired_with_contact(name: &str) -> Pairings {
    let mut p = Pairings::default();
    p.peers.push(Peer::new(name, "tok"));
    p.contacts.push(Contact { name: name.into(), host: "10.0.0.9".into(), port: 8788, token: "tok".into() });
    p
}
