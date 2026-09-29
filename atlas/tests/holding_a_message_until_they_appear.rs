//! The courier: what happens between "I said it" and "they have it".
//!
//! # Why this is testable at all without a network
//!
//! The transport is a trait, and these tests hand it one that exists only
//! here. That is the whole reason this part could be built before the
//! reachability question is settled: whether two Atlases find each other over
//! a private network, across a local wifi, or some other way, *none of the
//! behaviour below changes*. Only `hand_over` does.
//!
//! What is being defended:
//!
//! * a message is held, not lost, while nobody is up;
//! * it goes to the **person**, on whichever device answers, and never twice;
//! * an attempt is not a delivery — only an acknowledgement is;
//! * six messages waiting for a laptop that has been off all week arrive in
//!   the order they were written;
//! * and trying forever is its own failure, so after long enough the word
//!   changes from "waiting" to "stuck", with a reason.

use atlas::chat::{Chats, Delivery};
use atlas::courier::{run, Attempts, Device, Handoff, Kind, Round, Transport, Tries};
use atlas::earned::Space;
use atlas::kin::{Pairings, Peer};
use atlas::roster::Roster;
use std::cell::RefCell;
use std::collections::HashMap;

fn paired(names: &[&str]) -> Pairings {
    let mut p = Pairings::default();
    for n in names {
        p.peers.push(Peer::new(n, "some-token"));
    }
    p
}

fn business_with(members: &[&str]) -> (Roster, Pairings) {
    let pairings = paired(members);
    let mut roster = Roster::default();
    for m in members {
        roster.add("Acme", m, &pairings).unwrap();
    }
    (roster, pairings)
}

/// A transport that does exactly what the test tells it to.
///
/// Records every attempt, because "did it try the phone before the laptop"
/// and "did it try the laptop *as well*" are both things worth asserting and
/// neither is visible in the result.
struct Fake {
    /// Which devices each person has.
    devices: HashMap<String, Vec<Device>>,
    /// What each device does when handed something, by address.
    answers: RefCell<HashMap<String, Handoff>>,
    /// Every (address, message id) tried, in order.
    tried: RefCell<Vec<(String, String)>>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            devices: HashMap::new(),
            answers: RefCell::new(HashMap::new()),
            tried: RefCell::new(Vec::new()),
        }
    }

    fn with(mut self, peer: &str, kind: Kind, address: &str, answer: Handoff) -> Fake {
        self.devices.entry(peer.to_string()).or_default().push(Device {
            peer: peer.to_string(),
            kind,
            address: address.to_string(),
        });
        self.answers.borrow_mut().insert(address.to_string(), answer);
        self
    }

    fn set(&self, address: &str, answer: Handoff) {
        self.answers.borrow_mut().insert(address.to_string(), answer);
    }

    fn attempts_on(&self, address: &str) -> usize {
        self.tried.borrow().iter().filter(|(a, _)| a == address).count()
    }

    fn order(&self) -> Vec<String> {
        self.tried.borrow().iter().map(|(a, _)| a.clone()).collect()
    }
}

impl Transport for Fake {
    fn hand_over(&self, device: &Device, msg: &atlas::chat::Message) -> Handoff {
        self.tried.borrow_mut().push((device.address.clone(), msg.id.clone()));
        self.answers.borrow().get(&device.address).cloned().unwrap_or(Handoff::NotUp)
    }

    fn devices_for(&self, peer: &str) -> Vec<Device> {
        self.devices.get(peer).cloned().unwrap_or_default()
    }
}

/// A conversation with one message in it, waiting to go.
fn one_waiting(now: u64) -> (Chats, String, String, Roster, Pairings) {
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    let (msg, _) = chats.post(&room, "the roof quote came back high", now, 0, &roster, &pairings).unwrap();
    (chats, room, msg.id, roster, pairings)
}

// --- holding it ------------------------------------------------------------

#[test]
fn nobody_up_means_held_not_lost() {
    // The case the whole design exists for: written at midnight, everybody
    // else asleep.
    let now = 1_000_000;
    let (mut chats, room, msg_id, _, _) = one_waiting(now);
    let post = Fake::new()
        .with("Sam", Kind::Phone, "sam-phone", Handoff::NotUp)
        .with("Sam", Kind::Computer, "sam-laptop", Handoff::NotUp);
    let mut tries = Tries::default();

    let round = run(&mut chats, &post, &mut tries, &Attempts::default(), now);
    assert_eq!(round.delivered, vec![]);
    assert_eq!(round.still_waiting, vec!["Sam"]);
    assert_eq!(chats.outbox().len(), 1, "the message was dropped when nobody answered");
    let held = &chats.room(&room).unwrap().messages[0];
    assert_eq!(held.id, msg_id);
    assert_eq!(held.to[0].1, Delivery::Waiting, "an attempt was recorded as something more");

    // Both devices were tried, not just the first.
    assert_eq!(post.attempts_on("sam-phone"), 1);
    assert_eq!(post.attempts_on("sam-laptop"), 1);
}

#[test]
fn it_goes_the_moment_one_device_appears() {
    // Their phone comes back. Nothing about the message changed while it
    // waited -- it is the same message, with the same timestamp.
    let now = 1_000_000;
    let (mut chats, room, msg_id, _, _) = one_waiting(now);
    let post = Fake::new()
        .with("Sam", Kind::Phone, "sam-phone", Handoff::NotUp)
        .with("Sam", Kind::Computer, "sam-laptop", Handoff::NotUp);
    let mut tries = Tries::default();
    let attempts = Attempts::default();

    run(&mut chats, &post, &mut tries, &attempts, now);
    post.set("sam-phone", Handoff::Took);

    // Far enough ahead that the backoff allows another try.
    let later = now + 120;
    let round = run(&mut chats, &post, &mut tries, &attempts, later);
    assert_eq!(round.delivered, vec![("Sam".to_string(), msg_id.clone())]);
    assert!(chats.outbox().is_empty(), "it stayed in the outbox after landing");

    let held = &chats.room(&room).unwrap().messages[0];
    assert!(held.fully_arrived());
    assert_eq!(held.sent_at, now, "the timestamp moved to when it was delivered");
}

#[test]
fn the_person_having_it_ends_the_attempt_for_that_person() {
    // Their phone takes it. The laptop must not also get it -- a conversation
    // that repeats itself on the second device is worse than one that arrives
    // late.
    let now = 1_000_000;
    let (mut chats, _room, _msg_id, _, _) = one_waiting(now);
    let post = Fake::new()
        .with("Sam", Kind::Phone, "sam-phone", Handoff::Took)
        .with("Sam", Kind::Computer, "sam-laptop", Handoff::Took);
    let mut tries = Tries::default();

    run(&mut chats, &post, &mut tries, &Attempts::default(), now);
    assert_eq!(post.attempts_on("sam-phone"), 1);
    assert_eq!(post.attempts_on("sam-laptop"), 0, "it delivered to both devices");
}

#[test]
fn a_dead_phone_falls_through_to_the_computer() {
    // The other half of the same rule: one device being down is not the
    // person being unreachable.
    let now = 1_000_000;
    let (mut chats, _room, msg_id, _, _) = one_waiting(now);
    let post = Fake::new()
        .with("Sam", Kind::Phone, "sam-phone", Handoff::NotUp)
        .with("Sam", Kind::Computer, "sam-laptop", Handoff::Took);
    let mut tries = Tries::default();

    let round = run(&mut chats, &post, &mut tries, &Attempts::default(), now);
    assert_eq!(round.delivered, vec![("Sam".to_string(), msg_id)]);
    assert_eq!(post.order(), vec!["sam-phone", "sam-laptop"], "it did not try them in order");
}

// --- an attempt is not a delivery ------------------------------------------

#[test]
fn only_an_acknowledgement_counts_as_arrival() {
    // `Handoff::NotUp` is what a socket that opened and then went quiet looks
    // like. Nothing about having tried may mark a message arrived: that is
    // the fact this whole module is arranged to keep honest.
    let now = 1_000_000;
    let (mut chats, room, _msg_id, _, _) = one_waiting(now);
    let post = Fake::new().with("Sam", Kind::Phone, "sam-phone", Handoff::NotUp);
    let mut tries = Tries::default();
    let attempts = Attempts::default();

    let mut at = now;
    for _ in 0..5 {
        run(&mut chats, &post, &mut tries, &attempts, at);
        at += 4 * 3600;
    }
    let held = &chats.room(&room).unwrap().messages[0];
    assert!(!held.fully_arrived(), "repeated attempts added up to a delivery");
    assert_eq!(chats.outbox().len(), 1);
}

#[test]
fn a_refusal_is_recorded_with_its_reason_rather_than_retried_silently() {
    // Their Atlas answered and said no -- somebody removed from the business,
    // most likely, which is the roster working. A refusal that looked like
    // silence would be retried forever and never explained.
    let now = 1_000_000;
    let (mut chats, room, _msg_id, _, _) = one_waiting(now);
    let post = Fake::new().with(
        "Sam",
        Kind::Phone,
        "sam-phone",
        Handoff::Refused("not a member of Acme".into()),
    );
    let mut tries = Tries::default();

    let round = run(&mut chats, &post, &mut tries, &Attempts::default(), now);
    assert_eq!(round.refused, vec![("Sam".to_string(), "not a member of Acme".to_string())]);
    let held = &chats.room(&room).unwrap().messages[0];
    match &held.to[0].1 {
        Delivery::Stuck(why) => assert!(why.contains("not a member"), "{why}"),
        other => panic!("a refusal was recorded as {other:?}"),
    }
}

// --- order after a long absence --------------------------------------------

#[test]
fn a_week_of_messages_arrives_in_the_order_they_were_written() {
    // Their laptop has been off since Monday. Six things are waiting. They
    // must land oldest first -- `chat.rs` would sort them correctly once they
    // are all there, but while they land one at a time, out of order is a
    // conversation rearranging itself in front of somebody.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    let start = 1_000_000;
    for i in 0..6u64 {
        chats
            .post(&room, &format!("message {i}"), start + i * 3600, 0, &roster, &pairings)
            .unwrap();
    }

    let post = Fake::new().with("Sam", Kind::Computer, "sam-laptop", Handoff::Took);
    let mut tries = Tries::default();
    let round = run(&mut chats, &post, &mut tries, &Attempts::default(), start + 7 * 86_400);

    let bodies: Vec<String> = round
        .delivered
        .iter()
        .map(|(_, id)| {
            chats
                .room(&room)
                .unwrap()
                .messages
                .iter()
                .find(|m| &m.id == id)
                .unwrap()
                .body
                .clone()
        })
        .collect();
    assert_eq!(
        bodies,
        (0..6).map(|i| format!("message {i}")).collect::<Vec<_>>(),
        "they were delivered out of order"
    );
    assert!(chats.outbox().is_empty());
}

#[test]
fn one_that_fails_holds_the_rest_of_that_conversation_behind_it() {
    // Skipping ahead past a failure would deliver message 3 before message 2
    // and leave the far side reading an answer to something they have not
    // been told yet.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    let start = 1_000_000;
    for i in 0..3u64 {
        chats.post(&room, &format!("m{i}"), start + i, 0, &roster, &pairings).unwrap();
    }

    // A transport that takes the first and then goes down.
    struct FirstThenDown {
        taken: RefCell<usize>,
    }
    impl Transport for FirstThenDown {
        fn hand_over(&self, _d: &Device, _m: &atlas::chat::Message) -> Handoff {
            let mut n = self.taken.borrow_mut();
            *n += 1;
            if *n == 1 {
                Handoff::Took
            } else {
                Handoff::NotUp
            }
        }
        fn devices_for(&self, peer: &str) -> Vec<Device> {
            vec![Device { peer: peer.into(), kind: Kind::Phone, address: "p".into() }]
        }
    }

    let post = FirstThenDown { taken: RefCell::new(0) };
    let mut tries = Tries::default();
    let round = run(&mut chats, &post, &mut tries, &Attempts::default(), start + 10);

    assert_eq!(round.delivered.len(), 1, "it carried on past a failure");
    assert_eq!(chats.outbox().len(), 2, "the rest were not still owed");
}

// --- not hammering, and not pretending -------------------------------------

#[test]
fn a_shut_laptop_is_not_contacted_every_tick() {
    // The courier is called on a tick. Without backoff, a peer away for a
    // fortnight is contacted every few seconds for a fortnight.
    let now = 1_000_000;
    let (mut chats, _room, _msg_id, _, _) = one_waiting(now);
    let post = Fake::new().with("Sam", Kind::Phone, "sam-phone", Handoff::NotUp);
    let mut tries = Tries::default();
    let attempts = Attempts::default();

    // Ten ticks inside the first backoff window. Two seconds apart rather
    // than five: five would cross the thirty-second first wait, so the second
    // attempt would be correct behaviour and the test would be asserting the
    // implementation was broken. Found by watching this fail.
    for i in 0..10 {
        run(&mut chats, &post, &mut tries, &attempts, now + i * 2);
    }
    assert_eq!(post.attempts_on("sam-phone"), 1, "it tried on every tick");

    // And it does try again, once enough time has passed.
    run(&mut chats, &post, &mut tries, &attempts, now + 120);
    assert_eq!(post.attempts_on("sam-phone"), 2, "it never tried again");
}

#[test]
fn after_days_undelivered_the_word_changes_from_waiting_to_stuck() {
    // Still held, still tried, still delivered the day they reappear. What
    // changes is what Atlas calls it -- because a person can act on "stuck"
    // and can only sit through "waiting".
    let now = 1_000_000;
    let (mut chats, room, _msg_id, _, _) = one_waiting(now);
    let post = Fake::new().with("Sam", Kind::Phone, "sam-phone", Handoff::NotUp);
    let mut tries = Tries::default();
    let attempts = Attempts::default();

    run(&mut chats, &post, &mut tries, &attempts, now);
    assert_eq!(chats.room(&room).unwrap().messages[0].to[0].1, Delivery::Waiting);

    let four_days = now + 4 * 86_400;
    let round = run(&mut chats, &post, &mut tries, &attempts, four_days);
    assert_eq!(round.gone_stuck.len(), 1, "a four-day-old message still read as waiting");
    match &chats.room(&room).unwrap().messages[0].to[0].1 {
        Delivery::Stuck(why) => assert!(why.contains("days"), "{why}"),
        other => panic!("{other:?}"),
    }

    // And it still goes when they finally appear -- "stuck" is a word, not a
    // decision to stop.
    post.set("sam-phone", Handoff::Took);
    let round = run(&mut chats, &post, &mut tries, &attempts, four_days + 3600);
    assert_eq!(round.delivered.len(), 1, "a stuck message was abandoned");
    assert!(chats.room(&room).unwrap().messages[0].fully_arrived());
}

#[test]
fn a_delivered_message_stops_costing_anything() {
    // The retry record is about the network, not the conversation, so it has
    // to be dropped once there is nothing left to retry -- otherwise it grows
    // for the life of the install.
    let now = 1_000_000;
    let (mut chats, _room, msg_id, _, _) = one_waiting(now);
    let post = Fake::new().with("Sam", Kind::Phone, "sam-phone", Handoff::NotUp);
    let mut tries = Tries::default();
    let attempts = Attempts::default();

    // Read through the same two methods `run` itself uses, rather than
    // through a counter that exists only for this assertion -- a capability
    // whose only caller is the test that checks it is the shape this
    // codebase keeps finding.
    let key = (msg_id.clone(), "Sam".to_string());
    run(&mut chats, &post, &mut tries, &attempts, now);
    let (count, last) = tries.of(&key);
    assert_eq!(count, 1, "the attempt was not recorded");
    assert_eq!(last, now);

    post.set("sam-phone", Handoff::Took);
    run(&mut chats, &post, &mut tries, &attempts, now + 120);
    assert_eq!(
        tries.of(&key),
        (0, 0),
        "the retry record outlived the thing it was tracking, so it grows for the \
         life of the install"
    );
}

// --- a group ---------------------------------------------------------------

#[test]
fn in_a_group_each_person_is_delivered_to_separately() {
    // One person being up is not the message having landed. This is the same
    // rule `chat.rs` states about ticks, seen from the delivery side.
    let (roster, pairings) = business_with(&["Sam", "Ali"]);
    let mut chats = Chats::default();
    let room = chats
        .open(
            "Roof job",
            Space::Business("Acme".into()),
            &["Sam".into(), "Ali".into()],
            &roster,
            &pairings,
        )
        .unwrap();
    let now = 1_000_000;
    chats.post(&room, "starting Monday", now, 0, &roster, &pairings).unwrap();

    let post = Fake::new()
        .with("Sam", Kind::Phone, "sam-phone", Handoff::Took)
        .with("Ali", Kind::Phone, "ali-phone", Handoff::NotUp);
    let mut tries = Tries::default();
    let round = run(&mut chats, &post, &mut tries, &Attempts::default(), now);

    assert_eq!(round.delivered.len(), 1);
    assert_eq!(round.still_waiting, vec!["Ali"]);
    let held = &chats.room(&room).unwrap().messages[0];
    assert!(!held.fully_arrived(), "one of two counted as everybody");
    assert_eq!(held.still_waiting(), vec!["Ali"]);
    assert_eq!(chats.outbox().len(), 1, "it left the outbox with Ali still missing it");
}

// --- what it says ----------------------------------------------------------

#[test]
fn a_quiet_round_says_nothing_at_all() {
    // Called on a tick. A courier that announces "nothing happened" every
    // thirty seconds is the notification noise this codebase is written
    // against.
    let round = Round::default();
    assert!(round.nothing_happened());
    assert_eq!(round.spoken(), "");

    let waiting_only = Round { still_waiting: vec!["Sam".into()], ..Default::default() };
    assert!(waiting_only.nothing_happened(), "somebody being offline is not news");
    assert_eq!(waiting_only.spoken(), "");
}

#[test]
fn what_it_says_names_people_rather_than_counting_messages() {
    let round = Round {
        delivered: vec![("Sam".into(), "a".into()), ("Sam".into(), "b".into())],
        ..Default::default()
    };
    let said = round.spoken();
    assert!(said.contains("Sam"), "{said}");
    assert!(!said.contains('2'), "it counted instead of naming: {said}");

    let refused = Round {
        refused: vec![("Ali".into(), "not a member of Acme".into())],
        ..Default::default()
    };
    assert!(refused.spoken().contains("not a member of Acme"), "the reason was dropped");
}

// --- what this install can actually do today -------------------------------

#[test]
fn with_no_transport_nothing_is_tried_and_nothing_is_claimed() {
    // `NoLink` is the honest state of an install where two Atlases cannot yet
    // find each other. The failure to avoid is a stub that looks like a
    // delivery -- so it reports no devices at all, and nothing can be marked
    // arrived by it even by accident.
    use atlas::courier::NoLink;
    let now = 1_000_000;
    let (mut chats, room, _msg_id, _, _) = one_waiting(now);
    let mut tries = Tries::default();

    assert!(NoLink.devices_for("Sam").is_empty());
    let round = run(&mut chats, &NoLink, &mut tries, &Attempts::default(), now);
    assert!(round.delivered.is_empty(), "a transport that cannot reach anybody delivered something");
    assert_eq!(round.still_waiting, vec!["Sam"]);
    assert_eq!(chats.outbox().len(), 1, "the message was not kept");
    assert_eq!(chats.room(&room).unwrap().messages[0].to[0].1, Delivery::Waiting);

    // And the honest sentence about why, rather than blaming the recipient.
    let said = atlas::courier::nothing_can_move_yet(1);
    assert!(said.contains("no link"), "{said}");
    assert!(said.contains("kept"), "it does not say the message is safe: {said}");
    assert!(!said.to_lowercase().contains("hasn't picked"), "it blamed the recipient: {said}");
    assert_eq!(atlas::courier::nothing_can_move_yet(0), "", "it said something with nothing waiting");
}

#[test]
fn the_backoff_curve_is_stated_rather_than_only_observed() {
    // `wait_after` is exercised through `run` by the tick test above, which
    // proves the behaviour and says nothing about the shape. The shape is
    // worth pinning: it is the difference between a peer away for a fortnight
    // being contacted twice an hour and being contacted every thirty seconds
    // for a fortnight.
    let a = Attempts::default();
    assert_eq!(a.wait_after(0), 0, "the first attempt waits for nothing");
    assert_eq!(a.wait_after(1), 30);
    assert_eq!(a.wait_after(2), 60);
    assert_eq!(a.wait_after(3), 120);
    assert_eq!(a.wait_after(40), a.longest_wait, "the backoff ran away");
    // The one that matters for a long absence: it must not overflow into a
    // tiny wait, which is how an exponential backoff becomes a flood.
    assert_eq!(a.wait_after(u32::MAX), a.longest_wait);

    assert!(a.due(0, 0, 0), "the first attempt was not due immediately");
    assert!(!a.due(1, 100, 110), "it retried inside the first wait");
    assert!(a.due(1, 100, 131), "it never retried");
}
