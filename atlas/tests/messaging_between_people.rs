//! Messaging inside Atlas: the parts that break when two clocks disagree and
//! when somebody leaves a business.
//!
//! # What this is defending
//!
//! The requirement is the ordinary one — you write a message when you think
//! of it, whether or not the other person's laptop is on, and it carries the
//! time *you* wrote it. Three things follow from that, and each is a way this
//! quietly goes wrong:
//!
//! * **Sending cannot wait.** A `post` that could fail because somebody is
//!   offline would make the feature useless at exactly the moment people use
//!   it — late, alone, thinking of something.
//! * **The sender's timestamp is evidence, not a guess.** Rewriting it on
//!   arrival turns a message written Tuesday into one written Thursday, and
//!   the conversation becomes a record of when laptops were open rather than
//!   of when things were said.
//! * **So the wall clock cannot be what orders it.** Two machines disagree,
//!   and one that has been shut for a week can come back genuinely behind.
//!   Sorting by clock puts a reply above the question it answers, which is
//!   not a cosmetic bug: it changes what the conversation means.
//!
//! And one that is about people rather than clocks: a room in a business is
//! only for people in that business, checked on every send and every arrival
//! rather than once when the room was made.

use atlas::chat::{Chats, Delivery, Message, Refused, ME};
use atlas::earned::Space;
use atlas::kin::{Pairings, Peer};
use atlas::roster::Roster;

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

/// A message as it would arrive from somebody else.
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

// --- sending never waits for anybody ---------------------------------------

#[test]
fn a_message_can_be_written_with_everybody_else_offline() {
    // The requirement, stated as the first test because everything else is
    // arranged around it. Nothing in `post` consults a network, and there is
    // no state in which writing is refused for being early or late.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();

    let (msg, left_out) = chats
        .post(&room, "the roof quote came back high", 1_000, -300, &roster, &pairings)
        .expect("writing a message required something of the network");
    assert!(left_out.is_empty());
    assert_eq!(msg.from, ME);
    assert_eq!(msg.sent_at, 1_000, "the message did not carry the moment it was written");
    assert_eq!(msg.to, vec![("Sam".to_string(), Delivery::Waiting)]);
    assert!(!msg.fully_arrived(), "an unsent message claimed to have arrived");
    assert_eq!(msg.still_waiting(), vec!["Sam"]);

    // And it is in the outbox, visibly, rather than gone quiet.
    assert_eq!(chats.outbox().len(), 1, "a message nobody has yet is invisible");
}

#[test]
fn arriving_does_not_rewrite_when_it_was_written() {
    // A message written Tuesday and collected Thursday is a Tuesday message.
    // The alternative makes the whole record a log of when laptops were open.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();

    let tuesday = 1_000_000;
    let thursday = tuesday + 2 * 86_400;
    let mut msg = from_them("Sam", "can you look at this today", tuesday, 1);
    msg.to = vec![(ME.to_string(), Delivery::Arrived(thursday))];
    chats.receive(&room, msg, &roster, &pairings).unwrap();

    let held = &chats.room(&room).unwrap().messages[0];
    assert_eq!(held.sent_at, tuesday, "the timestamp was moved to when it was collected");
}

#[test]
fn a_message_carries_the_senders_own_offset_so_their_evening_is_their_evening() {
    // Kept beside the timestamp rather than worked out later, because by the
    // time anybody looks at it the sender may have moved.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    let (msg, _) = chats.post(&room, "night", 1_000, -300, &roster, &pairings).unwrap();
    assert_eq!(msg.sent_offset_mins, -300);
    assert_eq!(msg.as_they_saw_it(), 1_000 - 5 * 3600, "their local reading is wrong");
}

// --- the clocks disagree ---------------------------------------------------

#[test]
fn a_slow_clock_cannot_put_an_answer_above_its_question() {
    // The failure this is all for. Their laptop has been shut for a week and
    // comes back genuinely behind; they answer something you asked an hour
    // ago. Sorted by wall clock, their answer appears above your question and
    // the conversation reads as though they said it first.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();

    let now = 2_000_000;
    let (question, _) = chats.post(&room, "are we still on for Friday", now, 0, &roster, &pairings).unwrap();

    // Their reply: written after seeing the question, but stamped a week ago.
    let their_clock = now - 7 * 86_400;
    let reply = from_them("Sam", "yes, Friday works", their_clock, question.after + 1);
    chats.receive(&room, reply, &roster, &pairings).unwrap();

    let order: Vec<&str> = chats
        .room(&room)
        .unwrap()
        .in_order()
        .iter()
        .map(|m| m.body.as_str())
        .collect();
    assert_eq!(
        order,
        vec!["are we still on for Friday", "yes, Friday works"],
        "a wrong clock reordered the conversation"
    );

    // And the wall clocks really do disagree, so this test is exercising the
    // case it claims to. Without this, the same assertion would pass on an
    // implementation that simply sorted by time.
    let held = chats.room(&room).unwrap();
    assert!(
        held.in_order()[1].sent_at < held.in_order()[0].sent_at,
        "the fixture no longer has the clocks out of order, so it proves nothing"
    );
}

#[test]
fn my_next_message_sorts_after_whatever_i_have_just_seen() {
    // The other half of the same mechanism: our clock moves past theirs on
    // arrival, so the next thing written here cannot land above it however
    // far ahead their machine was.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();

    // Their machine is a year ahead and has a high counter.
    chats
        .receive(&room, from_them("Sam", "from the future", 99_999_999, 500), &roster, &pairings)
        .unwrap();
    let (mine, _) = chats.post(&room, "replying now", 1_000, 0, &roster, &pairings).unwrap();
    assert!(mine.after > 500, "the local counter ignored a higher one it had just seen");

    let order: Vec<&str> =
        chats.room(&room).unwrap().in_order().iter().map(|m| m.body.as_str()).collect();
    assert_eq!(order, vec!["from the future", "replying now"]);
}

#[test]
fn two_people_typing_at_once_is_a_coincidence_not_an_ordering_question() {
    // Genuinely concurrent messages -- neither saw the other -- are ordered
    // by wall clock, and that is a presentation choice rather than a claim
    // about what happened. What matters is that the order is *stable*: the
    // same two messages must not swap places between two readings.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();

    chats.receive(&room, from_them("Sam", "a", 1_000, 1), &roster, &pairings).unwrap();
    chats.receive(&room, from_them("Sam", "b", 1_000, 1), &roster, &pairings).unwrap();
    let first: Vec<String> = chats
        .room(&room)
        .unwrap()
        .in_order()
        .iter()
        .map(|m| m.body.clone())
        .collect();
    let again: Vec<String> = chats
        .room(&room)
        .unwrap()
        .in_order()
        .iter()
        .map(|m| m.body.clone())
        .collect();
    assert_eq!(first, again, "the same conversation read back in two different orders");
}

// --- delivery is per person ------------------------------------------------

#[test]
fn delivered_never_means_delivered_to_some_of_them() {
    // In a group, a single tick that means "at least one" reads as
    // reassurance and is not one -- you would see it and assume the person
    // you were actually talking to had it.
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
    let (msg, _) = chats.post(&room, "starting Monday", 1_000, 0, &roster, &pairings).unwrap();

    assert!(chats.mark_arrived(&room, &msg.id, "Sam", 1_100));
    let held = &chats.room(&room).unwrap().messages[0];
    assert!(!held.fully_arrived(), "one of two counted as everybody");
    assert_eq!(held.still_waiting(), vec!["Ali"]);
    assert_eq!(chats.outbox().len(), 1, "it left the outbox with one person still missing it");

    assert!(chats.mark_arrived(&room, &msg.id, "Ali", 1_200));
    assert!(chats.room(&room).unwrap().messages[0].fully_arrived());
    assert!(chats.outbox().is_empty());
}

#[test]
fn something_that_will_not_arrive_says_so_rather_than_waiting_forever() {
    // `Waiting` and `Stuck` are different because a person can act on one and
    // can only sit through the other.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    let (msg, _) = chats.post(&room, "hello", 1_000, 0, &roster, &pairings).unwrap();

    assert!(chats.mark_stuck(&room, &msg.id, "Sam", "no route to them for six days"));
    let held = &chats.room(&room).unwrap().messages[0];
    assert!(matches!(held.to[0].1, Delivery::Stuck(_)));
    assert!(held.to[0].1.plain().contains("six days"), "the reason was dropped");

    // And a late arrival still wins over having given up on it.
    assert!(chats.mark_arrived(&room, &msg.id, "Sam", 2_000));
    assert!(chats.room(&room).unwrap().messages[0].fully_arrived());
}

#[test]
fn the_same_message_delivered_twice_appears_once() {
    // Ordinary on any store-and-forward path: an acknowledgement goes
    // missing and the far side sends again. Two copies of one message is a
    // conversation that reads as though somebody repeated themselves.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();

    let msg = from_them("Sam", "did you see this", 1_000, 1);
    assert!(chats.receive(&room, msg.clone(), &roster, &pairings).unwrap(), "first copy refused");
    assert!(!chats.receive(&room, msg, &roster, &pairings).unwrap(), "a duplicate was reported as new");
    assert_eq!(chats.room(&room).unwrap().messages.len(), 1);
}

// --- who is allowed in the room --------------------------------------------

#[test]
fn a_business_room_cannot_be_opened_with_somebody_outside_the_business() {
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    // Paired, but not on Acme's roster. This is the distinction `roster.rs`
    // exists for: being paired proves who somebody is, not what they may see.
    let mut wider = pairings.clone();
    wider.peers.push(Peer::new("Stranger", "token"));

    let refused = chats
        .open(
            "Roof job",
            Space::Business("Acme".into()),
            &["Sam".into(), "Stranger".into()],
            &roster,
            &wider,
        )
        .unwrap_err();
    assert_eq!(refused, Refused::NotInThatBusiness("Stranger".into()));
    assert!(chats.rooms.is_empty(), "the room was made anyway");
}

#[test]
fn somebody_removed_from_the_business_stops_receiving_without_the_room_changing() {
    // Membership is re-checked on every send rather than once when the room
    // was made. A room that outlives the standing that justified it is how
    // somebody keeps reading a business they were removed from.
    let (mut roster, pairings) = business_with(&["Sam", "Ali"]);
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
    let (before, left_out) = chats.post(&room, "before", 1_000, 0, &roster, &pairings).unwrap();
    assert_eq!(before.to.len(), 2);
    assert!(left_out.is_empty());

    roster.remove("Acme", "Ali");

    let (after, left_out) = chats.post(&room, "after", 1_100, 0, &roster, &pairings).unwrap();
    assert_eq!(after.to.len(), 1, "a removed member was still sent to");
    assert_eq!(after.to[0].0, "Sam");
    assert_eq!(left_out, vec!["Ali"], "you were not told who was left out");
}

#[test]
fn a_message_claiming_to_be_from_a_non_member_is_dropped_at_the_door() {
    // Dropped rather than stored and filtered when displayed. A thing that is
    // held and then hidden is one display bug away from being shown.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();

    let refused = chats
        .receive(&room, from_them("Stranger", "hello", 1_000, 1), &roster, &pairings)
        .unwrap_err();
    assert_eq!(refused, Refused::NotInThatBusiness("Stranger".into()));
    assert!(chats.room(&room).unwrap().messages.is_empty(), "it was stored anyway");
}

#[test]
fn a_personal_room_needs_no_business_at_all() {
    // The other half: not every conversation is work. A personal room is
    // between paired people and the roster has nothing to say about it.
    let pairings = paired(&["Sam"]);
    let roster = Roster::default();
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Personal, &["Sam".into()], &roster, &pairings)
        .expect("a personal conversation demanded a business");
    assert!(chats.post(&room, "pub friday?", 1_000, 0, &roster, &pairings).is_ok());
}

// --- reading it back -------------------------------------------------------

#[test]
fn unread_counts_what_they_said_and_not_what_i_said() {
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();

    chats.post(&room, "mine", 1_000, 0, &roster, &pairings).unwrap();
    assert!(chats.room(&room).unwrap().unread().is_empty(), "my own message was unread to me");

    chats.receive(&room, from_them("Sam", "theirs", 1_100, 9), &roster, &pairings).unwrap();
    assert_eq!(chats.room(&room).unwrap().unread().len(), 1);
}

#[test]
fn what_it_says_out_loud_leads_with_what_is_waiting_on_you() {
    // The only actionable half. A running commentary on undelivered mail is
    // the notification noise this codebase is written against, so it comes
    // second and only when there is any.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    assert_eq!(chats.spoken(), "Nothing new.");

    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    chats.receive(&room, from_them("Sam", "you there?", 1_000, 1), &roster, &pairings).unwrap();
    let said = chats.spoken();
    assert!(said.starts_with("1 unread"), "{said}");
    assert!(said.contains("Sam"), "it does not say who: {said}");

    // Replying marks what was already there as read -- you saw it, you
    // answered it -- so a second message has to arrive for there to be both
    // halves to put in order.
    chats.post(&room, "here", 1_100, 0, &roster, &pairings).unwrap();
    assert!(
        chats.room(&room).unwrap().unread().is_empty(),
        "answering somebody left their message unread"
    );
    chats.receive(&room, from_them("Sam", "and another", 1_200, 40), &roster, &pairings).unwrap();

    let said = chats.spoken();
    assert!(said.contains("waiting to be picked up"), "{said}");
    let unread_at = said.find("unread").expect("the unread part vanished");
    let waiting_at = said.find("waiting").expect("the waiting part vanished");
    assert!(unread_at < waiting_at, "it led with what is waiting on somebody else: {said}");
}

#[test]
fn a_conversation_survives_being_written_down_and_read_back() {
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    chats.post(&room, "keep this", 1_000, -300, &roster, &pairings).unwrap();

    let yaml = serde_yaml::to_string(&chats).unwrap();
    let back: Chats = serde_yaml::from_str(&yaml).unwrap();
    let held = &back.room(&room).unwrap().messages[0];
    assert_eq!(held.body, "keep this");
    assert_eq!(held.sent_at, 1_000);
    assert_eq!(held.sent_offset_mins, -300);
    assert_eq!(back.clock, chats.clock, "the ordering counter did not survive a restart");
}

#[test]
fn the_pieces_the_outbox_is_built_from_answer_on_their_own() {
    // `arrived` and `undelivered` are what every "has it landed" question in
    // this module is made of. Tested directly rather than only through the
    // outbox, because a helper that is only ever exercised two layers up is
    // one whose behaviour nobody has actually stated.
    assert!(!Delivery::Waiting.arrived());
    assert!(Delivery::Arrived(1).arrived());
    assert!(!Delivery::Stuck("no route".into()).arrived(), "giving up counted as arriving");

    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room_id = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    assert!(chats.room(&room_id).unwrap().undelivered().is_empty(), "an empty room owed something");

    let (mine, _) = chats.post(&room_id, "mine", 1_000, 0, &roster, &pairings).unwrap();
    chats.receive(&room_id, from_them("Sam", "theirs", 1_100, 9), &roster, &pairings).unwrap();
    let owed = chats.room(&room_id).unwrap().undelivered();
    assert_eq!(owed.len(), 1, "their message counted as something I owe them");
    assert_eq!(owed[0].id, mine.id);

    chats.mark_arrived(&room_id, &mine.id, "Sam", 1_200);
    assert!(chats.room(&room_id).unwrap().undelivered().is_empty());
}

#[test]
fn a_conversation_survives_being_put_down_and_picked_up_from_disk() {
    // Through `Store`, the way the daemon does it, rather than through serde
    // directly: the file name and the load-on-missing behaviour are part of
    // what has to work, and neither is exercised by a round-trip in memory.
    let dir = std::env::temp_dir().join("atlas-chat-store");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = atlas::store::Store::new(&dir);

    // A store with nothing in it yet reads as an empty set of conversations
    // rather than failing -- the first run of anything has to work.
    assert!(Chats::load(&store).rooms.is_empty());

    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room_id = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    chats.post(&room_id, "written before a restart", 1_000, -300, &roster, &pairings).unwrap();
    chats.save(&store).unwrap();

    let back = Chats::load(&store);
    assert_eq!(back.room(&room_id).unwrap().messages[0].body, "written before a restart");
    assert_eq!(back.clock, chats.clock, "the ordering counter did not survive");
    assert_eq!(back.outbox().len(), 1, "what was still owed was forgotten across a restart");
}

#[test]
fn a_machines_offset_is_read_rather_than_configured() {
    // The offset that goes on every message. Split out from the command that
    // produces it precisely so it can be tested without caring what this
    // machine's clock happens to be set to -- and then it needs a test, or
    // the split bought nothing.
    use atlas::daemon::parse_offset;
    assert_eq!(parse_offset("+0000"), 0);
    assert_eq!(parse_offset("-0500"), -300, "US eastern in winter");
    assert_eq!(parse_offset("+0530"), 330, "the half-hour offsets are real places");
    assert_eq!(parse_offset("-0930"), -570);

    // Anything it cannot read is 0 rather than a panic or a guess: a message
    // shown in UTC is a smaller failure than a message that would not send.
    for junk in ["", "nonsense", "0500", "+05", "++0500"] {
        assert_eq!(parse_offset(junk), 0, "{junk:?} was not handled as unreadable");
    }
}

#[test]
fn an_offset_that_is_not_a_real_place_is_refused() {
    // Found by the test above rather than by reading: "++0500" used to come
    // back as fifty minutes, because each field fell back to zero on its own
    // and the leftovers still parsed. A parser that cannot fail prefers a
    // wrong answer to no answer.
    use atlas::daemon::parse_offset;
    assert_eq!(parse_offset("+9900"), 0, "no zone is ninety-nine hours out");
    assert_eq!(parse_offset("+0099"), 0, "ninety-nine minutes is not a zone");
    assert_eq!(parse_offset("+05x0"), 0);
    assert_eq!(parse_offset("+1400"), 840, "Kiritimati is real and is the furthest out");
}

// ===================== the platforms list, which was dead ===============
//
// `messaging.platforms` is a list you write and nothing read. Two of the six
// things you can put in it can never work — WhatsApp has no personal-account
// interface and Signal is deliberately closed — so a person who wrote
// `platforms: [whatsapp]` got exactly the same silence as one who wrote
// `[telegram]`, on the one list where that difference is the whole question.
//
// The daemon's answer to "any messages?" used to be
// `messaging::spoken(&[], ..)`, which on an empty slice returns literally
// "0 messages, all group chat" — a count of an inbox nothing had read, stated
// as a fact. It still refuses to give a count. It now says what it can
// honestly say about the list instead.

use atlas::messaging::{what_you_asked_for, Platform};

#[test]
fn a_platform_you_named_is_the_one_you_get() {
    assert_eq!(Platform::named("telegram"), Some(Platform::Telegram));
    // Spelling as people write it in a yaml file.
    assert_eq!(Platform::named("  Telegram "), Some(Platform::Telegram));
    assert_eq!(Platform::named("GroupMe"), Some(Platform::GroupMe));
    assert_eq!(Platform::named("group-me"), Some(Platform::GroupMe));
    assert_eq!(Platform::named("Whats App"), Some(Platform::WhatsApp));

    // A typo is not guessed at. Answering "telegrma" with Telegram is how you
    // end up connected to something you did not ask for, and the list is
    // six long.
    assert_eq!(Platform::named("telegrma"), None);
    assert_eq!(Platform::named(""), None);
}

#[test]
fn what_each_platform_permits_is_said_in_its_own_words() {
    // Called directly as well as through `what_you_asked_for`, because these
    // strings are the answer a person acts on and a test that only reads the
    // assembled paragraph cannot say which platform a sentence belonged to.
    assert!(Platform::Telegram.what_it_permits().contains("own account rather than only for bots"));
    assert!(Platform::GroupMe.what_it_permits().contains("free, public interface"));
    assert!(Platform::Discord.what_it_permits().contains("Not your private messages"));
    assert!(Platform::Slack.what_it_permits().contains("workspaces you add it to"));

    // The two that cannot work say why, and the WhatsApp one refuses the
    // workaround by name rather than just declining.
    assert!(Platform::WhatsApp.what_it_permits().contains("pretending to be you"));
    assert!(Platform::WhatsApp.what_it_permits().contains("won't do that"));
    assert!(Platform::Signal.what_it_permits().contains("that's the point of Signal"));

    // And it is not the same string for two platforms, which is the way a
    // table like this rots.
    let all = [
        Platform::Telegram,
        Platform::GroupMe,
        Platform::WhatsApp,
        Platform::Signal,
        Platform::Discord,
        Platform::Slack,
    ];
    let mut said: Vec<&str> = all.iter().map(|p| p.what_it_permits()).collect();
    said.sort();
    let before = said.len();
    said.dedup();
    assert_eq!(said.len(), before, "two platforms share an answer");
}

#[test]
fn the_ones_that_can_never_work_are_said_so_rather_than_left_out() {
    let said = what_you_asked_for(&["whatsapp".into(), "signal".into()]);
    assert!(said.contains("WhatsApp"), "{said}");
    assert!(said.contains("no way to do this for a personal account"), "{said}");
    assert!(said.contains("Signal"), "{said}");

    // And they are not given a "needs" line. "Needs nothing that exists"
    // reads as an instruction you failed to follow.
    assert!(!said.contains("Needs nothing that exists"), "{said}");
}

#[test]
fn what_you_can_act_on_comes_first() {
    // A list that buries "Telegram takes two minutes" under two paragraphs
    // about WhatsApp is a list that gets skimmed.
    let said = what_you_asked_for(&["whatsapp".into(), "telegram".into()]);
    let telegram = said.find("Telegram").expect("Telegram missing");
    let whatsapp = said.find("WhatsApp").expect("WhatsApp missing");
    assert!(telegram < whatsapp, "the dead end came first: {said}");
    assert!(said.contains("BotFather"), "it didn't say what Telegram needs: {said}");
}

#[test]
fn a_name_it_does_not_know_is_named_rather_than_dropped() {
    let said = what_you_asked_for(&["matrix".into()]);
    assert!(said.contains("matrix"), "the unknown entry vanished: {said}");
    assert!(said.contains("Telegram"), "it didn't say what it can do: {said}");
}

#[test]
fn it_still_refuses_to_count_an_inbox_it_has_not_read() {
    // The defect this replaces, kept as an assertion so it cannot come back
    // by someone reaching for `spoken` again.
    for list in [vec!["telegram".to_string()], vec!["whatsapp".to_string()], vec![]] {
        let said = what_you_asked_for(&list);
        assert!(!said.contains("all group chat"), "{said}");
        assert!(!said.contains("0 messages"), "{said}");
    }
    assert!(what_you_asked_for(&["telegram".into()]).contains("nothing in this build reads"));

    // An empty list is its own answer rather than the "none are connected"
    // sentence with nothing above it.
    let none = what_you_asked_for(&[]);
    assert!(none.contains("haven't named any platforms"), "{none}");
}

#[test]
fn the_same_platform_twice_is_said_once() {
    let said = what_you_asked_for(&["telegram".into(), "Telegram".into(), "telegram ".into()]);
    assert_eq!(said.matches("BotFather").count(), 1, "{said}");
}

#[test]
fn the_daemon_is_what_reaches_it_rather_than_this_test() {
    let raw = crate::common::source_of("daemon");
    assert!(
        raw.contains("crate::messaging::what_you_asked_for(&cfg.platforms)"),
        "nothing reads the platforms list"
    );
    // Comment lines stripped first. The comment in `daemon.rs` explaining
    // why the empty-slice call is gone contains the call it is about, so a
    // plain `contains` fails on the note describing the fix -- the same
    // self-reference trap `dead_capabilities.rs` documents for its own
    // ceiling constant, and the reason `called_names` skips `//` lines.
    let code: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!code.contains("messaging::spoken(&[]"), "the empty-slice count is back");
    let yaml = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(yaml.contains("platforms: [telegram]"), "there is nowhere to set it");
    // `your_names` went the other way in the morning -- `#[serde(skip)]`, out
    // of the yaml, and named in PROMISES_ABOUT_WHAT_IS_NOT_BUILT, because
    // nothing read messages and so there was nothing for a name to be found
    // in. It came back the same afternoon, when `telegram.rs` gave `sort`
    // something to sort.
    //
    // **That is how a promise about what is not built is supposed to end.**
    // The missing capability arrives and the pinned field becomes a setting
    // again; a PROMISES list that only ever grows is a list of things nobody
    // intends to build. This assertion was the morning's, and the afternoon
    // made it false, which is the right direction for an assertion to break.
    assert!(yaml.contains("your_names:"), "your_names is unsettable and there is a reader now");
    assert!(
        atlas::messaging::MessagingConfig::default().your_names.is_empty(),
        "it should ship empty -- Atlas is not guessing what you are called"
    );
    // And the reader is what earns it.
    let raw = crate::common::source_of("main");
    assert!(
        raw.contains("atlas::messaging::sort(m, &mcfg.your_names)"),
        "nothing sorts a real message against the names you go by"
    );
}

// ================= a message from a peer, filed the way the daemon files it =================
//
// The receive path without a socket: the door names the sender from the token,
// then the message is opened into a room for that sender and received. What
// lands is in *your* conversation with them, stamped with *their* clock — and
// which business a message may enter is decided by your own roster, never by
// anything the sender put on the wire.

#[test]
fn a_message_from_a_peer_is_filed_into_your_conversation_with_them() {
    use atlas::kin::Door;
    let (roster, pairings) = business_with(&["Sam"]);
    let mut door = Door::new(vec![Peer::new("Sam", "sam-token")]);
    let c = door
        .receive_chat(
            "sam-token",
            Some("Acme".into()),
            "the roof quote came back high",
            7_000,
            -300,
            3,
            "acme-sam-7000-3",
            None,
            None,
            Vec::new(),
            9_000,
        )
        .unwrap();

    let mut chats = Chats::default();
    let room = chats
        .open(&c.from, Space::Business("Acme".into()), &[c.from.clone()], &roster, &pairings)
        .unwrap();
    let msg = Message {
        id: c.id.clone(),
        from: c.from.clone(),
        body: c.body.clone(),
        sent_at: c.sent_at,
        sent_offset_mins: c.sent_offset_mins,
        after: c.after,
        to: vec![(ME.to_string(), Delivery::Arrived(9_000))],
    };
    assert!(chats.receive(&room, msg, &roster, &pairings).unwrap());

    let held = &chats.room(&room).unwrap().messages[0];
    assert_eq!(held.from, "Sam", "filed under your name for them, from the token");
    assert_eq!(held.sent_at, 7_000, "the sender's clock is kept, not the arrival time");
    assert_eq!(held.body, "the roof quote came back high");
}

#[test]
fn a_peer_cannot_place_a_message_in_a_business_your_roster_says_they_are_not_in() {
    use atlas::kin::Door;
    // Sam is in Acme on this machine, and nowhere else.
    let (roster, pairings) = business_with(&["Sam"]);
    let mut door = Door::new(vec![Peer::new("Sam", "sam-token")]);
    // The wire claims Northwind. The door accepts the message (Sam is a peer),
    // but filing it is a separate decision.
    let c = door
        .receive_chat("sam-token", Some("Northwind".into()), "let me in", 1, 0, 1, "x", None, None, Vec::new(), 2)
        .unwrap();

    let mut chats = Chats::default();
    let opened = chats.open(
        &c.from,
        Space::Business("Northwind".into()),
        &[c.from.clone()],
        &roster,
        &pairings,
    );
    assert!(
        opened.is_err(),
        "the receiver's roster, not the sender's claim, decides which business a message may enter"
    );
}

// ================= groups: the mesh, and the shared id =================
//
// A group is the same room with more than one member. What makes it work
// across instances is a *shared id*, minted once and carried on every message,
// because each member's Atlas lists a different set of people (everyone but
// themselves) and so could never derive the same id. Membership is resolved
// against your own pairings: you reach the people you're paired with, and the
// entry that is you seen through the sender's eyes never matches a pairing and
// is dropped.

#[test]
fn a_group_message_is_written_to_every_member() {
    let (roster, pairings) = business_with(&["Sam", "Jordan"]);
    let mut chats = Chats::default();
    let room = chats
        .open_group(
            "grp-1",
            "Sam, Jordan",
            Space::Business("Acme".into()),
            &["Sam".into(), "Jordan".into()],
            &roster,
            &pairings,
        )
        .unwrap();
    let (msg, left) = chats.post(&room, "standup at 9", 1_000, 0, &roster, &pairings).unwrap();
    assert!(left.is_empty());
    let waiting = msg.still_waiting();
    assert_eq!(waiting.len(), 2, "a group message is owed to every member");
    assert!(waiting.contains(&"Sam") && waiting.contains(&"Jordan"));
}

#[test]
fn opening_a_group_again_folds_in_members_it_did_not_know() {
    let (roster, pairings) = business_with(&["Sam", "Jordan"]);
    let mut chats = Chats::default();
    chats
        .open_group("grp-2", "g", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    // Later we hear Jordan is in it too — learned, not duplicated, id unchanged.
    chats
        .open_group("grp-2", "g", Space::Business("Acme".into()), &["Jordan".into()], &roster, &pairings)
        .unwrap();
    let m = &chats.room("grp-2").unwrap().members;
    assert_eq!(m.len(), 2);
    assert!(m.iter().any(|x| x == "Sam") && m.iter().any(|x| x == "Jordan"));
}

#[test]
fn a_group_message_from_a_peer_reconstructs_the_shared_room_with_who_you_can_reach() {
    use atlas::kin::Door;
    // Paired with Eric (the sender) and Maya, but not with "Jordan".
    let mut pairings = Pairings::default();
    for n in ["Eric", "Maya"] {
        pairings.peers.push(Peer::new(n, "t"));
    }
    let mut roster = Roster::default();
    for n in ["Eric", "Maya"] {
        roster.add("Acme", n, &pairings).unwrap();
    }

    let mut door = Door::new(vec![Peer::new("Eric", "eric-token")]);
    let c = door
        .receive_chat(
            "eric-token",
            Some("Acme".into()),
            "standup at 9",
            7_000,
            0,
            3,
            "grp-7000-3",
            Some("grp".into()),
            Some("Northwind".into()),
            vec!["Maya".into(), "Jordan".into()],
            9_000,
        )
        .unwrap();
    assert_eq!(c.group_id.as_deref(), Some("grp"));

    // The daemon's resolution step: the sender (from the token) plus members
    // I'm paired with; the rest dropped.
    let mut members = vec![c.from.clone()];
    for m in &c.members {
        if pairings.has_peer(m) {
            members.push(m.clone());
        }
    }
    let mut chats = Chats::default();
    let room = chats
        .open_group(
            c.group_id.as_deref().unwrap(),
            "Northwind",
            Space::Business("Acme".into()),
            &members,
            &roster,
            &pairings,
        )
        .unwrap();
    let msg = Message {
        id: c.id.clone(),
        from: c.from.clone(),
        body: c.body.clone(),
        sent_at: c.sent_at,
        sent_offset_mins: c.sent_offset_mins,
        after: c.after,
        to: vec![(ME.to_string(), Delivery::Arrived(9_000))],
    };
    assert!(chats.receive(&room, msg, &roster, &pairings).unwrap());

    let m = &chats.room(&room).unwrap().members;
    assert!(m.iter().any(|x| x == "Eric"), "the sender is in it");
    assert!(m.iter().any(|x| x == "Maya"), "a member you're paired with is in it");
    assert!(!m.iter().any(|x| x == "Jordan"), "a member you cannot reach is left off");
    assert_eq!(chats.room(&room).unwrap().messages[0].sent_at, 7_000, "their clock is kept");
}

// ================= naming a group, and finding it by name =================

#[test]
fn a_group_can_be_named_and_found_by_that_name() {
    let (roster, pairings) = business_with(&["Sam", "Jordan"]);
    let mut chats = Chats::default();
    let room = chats
        .open_group(
            "grp-n",
            "Sam, Jordan",
            Space::Business("Acme".into()),
            &["Sam".into(), "Jordan".into()],
            &roster,
            &pairings,
        )
        .unwrap();

    assert!(chats.name_group(&room, "Roofing crew"), "a group can be renamed");
    // Found case-insensitively by the new name, so "message the roofing crew" lands.
    assert_eq!(chats.group_named("roofing crew").map(|r| r.id.as_str()), Some(room.as_str()));
    assert!(chats.group_named("Sam, Jordan").is_none(), "the old name no longer matches");
}

#[test]
fn a_one_to_one_is_not_a_group_and_cannot_be_named() {
    let (roster, pairings) = business_with(&["Sam"]);
    let mut chats = Chats::default();
    let room = chats
        .open("Sam", Space::Business("Acme".into()), &["Sam".into()], &roster, &pairings)
        .unwrap();
    assert!(!chats.name_group(&room, "not a group"), "a 1:1 is not a group to name");
    assert!(chats.group_named("Sam").is_none(), "a 1:1 is never found as a group");
}
