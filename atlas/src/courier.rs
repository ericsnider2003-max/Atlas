//! Holding a message until the other person's Atlas can take it.
//!
//! # The shape, in one paragraph
//!
//! You say something. Your Atlas writes it down with your clock on it
//! (`chat.rs`) and hands it here. This tries to give it to *their* Atlas —
//! on whichever of their devices is up, phone or computer, it does not care
//! which. If none is up it keeps hold of it and tries again. When one
//! appears, it goes, and the timestamp is still the moment you wrote it,
//! exactly like a text sent while their phone was off.
//!
//! # What is deliberately not decided here
//!
//! **How two Atlases reach each other.** Whether that is a direct connection
//! over a private network, a link across a local wifi, or something else, it
//! arrives here as a `Transport` and this file never learns which. That is
//! not tidiness: the reachability problem is a platform problem — carrier
//! NAT, and what a phone operating system will let an app do in the
//! background — and the answer differs per platform and will change again.
//! Everything in this file is the same whichever way it resolves, which is
//! why it is built first and tested against a transport that exists only in a
//! test.
//!
//! # Delivery is to a person, not to a device
//!
//! Somebody has a phone and a computer. A message is theirs once *either* has
//! taken it, and the second device must not receive a duplicate — a
//! conversation that repeats itself on the laptop because it also arrived on
//! the phone is worse than one that arrives late. So devices are tried in
//! turn, one acknowledgement ends the attempt for that person, and their
//! other devices get it from their own Atlas rather than from yours.
//!
//! # An attempt is not a delivery
//!
//! Handing bytes to a socket is not arrival, and `chat::Delivery` has no
//! state for it on purpose. Only an acknowledgement from the far side marks
//! a message `Arrived`. Anything else leaves it `Waiting`, which is the
//! honest answer and the one that keeps it in the outbox where you can see
//! it.
//!
//! # Trying forever is its own failure
//!
//! A message that has been failing for days is not "waiting", it is stuck,
//! and the difference matters because a person can act on one and can only
//! sit through the other. `Attempts` backs off so a shut laptop is not
//! hammered, and after long enough says so in words rather than going quiet.

use crate::chat::{Chats, Message};

/// A way to hand a message to somebody else's Atlas.
///
/// One method, on purpose. Everything a transport could usefully tell us is
/// in the answer to "did they take it", and a wider trait would invite this
/// file to start caring about addresses, which is the thing it must not know.
pub trait Transport {
    /// Try one device. This blocks for as long as the transport thinks
    /// reasonable and returns what happened.
    fn hand_over(&self, device: &Device, msg: &Message) -> Handoff;

    /// Which devices this person has, in the order worth trying. A phone
    /// first is usually right — it is the one that is on — but that is the
    /// transport's judgement, not this file's.
    fn devices_for(&self, peer: &str) -> Vec<Device>;
}

/// A way to tell somebody's Atlas that you have read their messages.
///
/// The reverse direction of `Transport`, and deliberately its own trait: a
/// receipt is not a message — it carries no body, has no clock of its own, and
/// must not be able to turn into one. Kept separate so the thing that sends
/// "I've read these" can never be handed a body to deliver, and the thing that
/// delivers a body can never be made to emit a receipt.
pub trait Receipts {
    /// Tell `peer`'s Atlas that these message ids of *theirs* have been read.
    /// Returns whether the far side accepted it. A `false` is not an error —
    /// the person is simply offline, and the receipt is re-owed next pass
    /// rather than being fabricated as delivered.
    fn read(&self, peer: &str, ids: &[String]) -> bool;
}

/// One machine belonging to somebody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Whose it is, by the name the roster knows.
    pub peer: String,
    /// What kind, for the sake of saying it out loud. Never for deciding
    /// anything: a phone that is off is exactly as useful as a laptop that
    /// is off.
    pub kind: Kind,
    /// How the transport finds it. Opaque here on purpose.
    pub address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Phone,
    Computer,
}

impl Kind {
    pub fn plain(&self) -> &'static str {
        match self {
            Kind::Phone => "phone",
            Kind::Computer => "computer",
        }
    }
}

/// What came back from trying one device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handoff {
    /// Their Atlas has it and said so. The only thing that counts.
    Took,
    /// Nothing there right now. Ordinary, expected, not an error: a shut
    /// laptop and a phone in a tunnel both land here.
    NotUp,
    /// It answered and refused, and this is why. A different thing from
    /// silence -- somebody removed from the business refusing a message is
    /// the roster working, and retrying it forever would be pointless.
    Refused(String),
}

/// How long to wait before trying again, and when to stop calling it waiting.
#[derive(Debug, Clone, Copy)]
pub struct Attempts {
    /// First retry, in seconds.
    pub first_wait: u64,
    /// Never wait longer than this between tries.
    pub longest_wait: u64,
    /// After this long undelivered, say it is stuck rather than waiting.
    pub give_up_saying_waiting_after: u64,
}

impl Default for Attempts {
    fn default() -> Self {
        Attempts {
            // Quick enough that somebody opening their laptop gets the
            // message within a minute, slow enough that a peer who is away
            // for a fortnight is not contacted every thirty seconds.
            first_wait: 30,
            longest_wait: 15 * 60,
            // Three days. Long enough that a weekend away is still "waiting",
            // short enough that a message nobody will ever get does not sit
            // there looking healthy.
            give_up_saying_waiting_after: 3 * 86_400,
        }
    }
}

impl Attempts {
    /// Doubling, capped. The point is not the exact curve, it is that a
    /// device which has been down for a day is not polled at the same rate
    /// as one that has been down for a minute.
    pub fn wait_after(&self, tries: u32) -> u64 {
        if tries == 0 {
            return 0;
        }
        let doubled = self.first_wait.saturating_mul(1u64 << (tries - 1).min(20));
        doubled.min(self.longest_wait)
    }

    /// Is it time to try this one again?
    pub fn due(&self, tries: u32, last_try: u64, now: u64) -> bool {
        now.saturating_sub(last_try) >= self.wait_after(tries)
    }
}

/// What one pass of the courier did, for saying out loud and for tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Round {
    /// (person, message id) for each one that landed.
    pub delivered: Vec<(String, String)>,
    /// People whose devices were all down.
    pub still_waiting: Vec<String>,
    /// (person, why) where the far side actively refused.
    pub refused: Vec<(String, String)>,
    /// Messages now old enough that "waiting" is no longer the honest word.
    pub gone_stuck: Vec<String>,
}

impl Round {
    pub fn nothing_happened(&self) -> bool {
        self.delivered.is_empty() && self.refused.is_empty() && self.gone_stuck.is_empty()
    }

    /// For a person. Named rather than counted -- "Sam hasn't picked up two"
    /// tells you whether to phone him; "2 pending" tells you nothing.
    pub fn spoken(&self) -> String {
        if self.nothing_happened() {
            return String::new();
        }
        let mut parts = Vec::new();
        if !self.delivered.is_empty() {
            let mut who: Vec<&str> = self.delivered.iter().map(|(p, _)| p.as_str()).collect();
            who.sort_unstable();
            who.dedup();
            parts.push(format!("{} got what you sent.", who.join(" and ")));
        }
        for (who, why) in &self.refused {
            parts.push(format!("{who}'s Atlas wouldn't take it: {why}"));
        }
        if !self.gone_stuck.is_empty() {
            parts.push(format!(
                "{} message{} been undelivered for days now.",
                self.gone_stuck.len(),
                if self.gone_stuck.len() == 1 { " has" } else { "s have" }
            ));
        }
        parts.join(" ")
    }
}

/// One attempt at everything outstanding.
///
/// Called on a tick, and on whatever the transport can offer as "somebody
/// just appeared". Safe to call often: `Attempts` decides what is actually
/// due, so a fast tick costs a loop over a short list rather than a burst of
/// connections.
///
/// # Why oldest-first, per room
///
/// A laptop that has been off all week comes back to six messages. Delivered
/// newest-first they arrive backwards; delivered in parallel they arrive in
/// whatever order the network settles on. `chat.rs` would still *sort* them
/// correctly on the far side -- that is what the causal counter is for -- but
/// they would appear one at a time in the wrong order while they landed,
/// which is a conversation reordering itself in front of somebody. So: oldest
/// first, and a failure stops that room's run rather than skipping ahead.
pub fn run(
    chats: &mut Chats,
    transport: &dyn Transport,
    tries: &mut Tries,
    attempts: &Attempts,
    now: u64,
) -> Round {
    let mut round = Round::default();

    // Collected first, because delivering mutates `chats` and the borrow
    // cannot be held across it.
    let mut outstanding: Vec<(String, String, String, u64)> = Vec::new();
    for room in &chats.rooms {
        for msg in room.undelivered() {
            for who in msg.still_waiting() {
                outstanding.push((
                    room.id.clone(),
                    msg.id.clone(),
                    who.to_string(),
                    msg.sent_at,
                ));
            }
        }
    }
    // Oldest first, and stable within a room.
    outstanding.sort_by(|a, b| a.3.cmp(&b.3).then(a.1.cmp(&b.1)));

    let mut room_blocked: Vec<(String, String)> = Vec::new();
    for (room_id, msg_id, who, _) in outstanding {
        // An earlier message to this person in this room did not get through.
        // Sending the later one now would arrive out of order.
        if room_blocked.iter().any(|(r, p)| *r == room_id && *p == who) {
            continue;
        }
        let key = (msg_id.clone(), who.clone());
        let (count, last) = tries.of(&key);
        if !attempts.due(count, last, now) {
            room_blocked.push((room_id.clone(), who.clone()));
            continue;
        }
        let Some(msg) = chats.room(&room_id).and_then(|r| r.messages.iter().find(|m| m.id == msg_id))
        else {
            continue;
        };
        let msg = msg.clone();

        let mut outcome = Handoff::NotUp;
        for device in transport.devices_for(&who) {
            match transport.hand_over(&device, &msg) {
                Handoff::Took => {
                    outcome = Handoff::Took;
                    // One device taking it is the person having it. Their
                    // other devices are their own Atlas's problem, and
                    // sending to both is how a conversation duplicates.
                    break;
                }
                Handoff::Refused(why) => {
                    outcome = Handoff::Refused(why);
                    break;
                }
                Handoff::NotUp => continue,
            }
        }

        tries.note(&key, now);
        match outcome {
            Handoff::Took => {
                chats.mark_arrived(&room_id, &msg_id, &who, now);
                tries.forget(&key);
                round.delivered.push((who, msg_id));
            }
            Handoff::Refused(why) => {
                chats.mark_stuck(&room_id, &msg_id, &who, &why);
                round.refused.push((who.clone(), why));
                room_blocked.push((room_id, who));
            }
            Handoff::NotUp => {
                let waited = now.saturating_sub(msg.sent_at);
                if waited >= attempts.give_up_saying_waiting_after {
                    // Still tried, still kept, still delivered the day they
                    // reappear. What changes is the word used for it.
                    chats.mark_stuck(
                        &room_id,
                        &msg_id,
                        &who,
                        &format!("{} hasn't been reachable in days", who),
                    );
                    round.gone_stuck.push(msg_id);
                }
                if !round.still_waiting.contains(&who) {
                    round.still_waiting.push(who.clone());
                }
                room_blocked.push((room_id, who));
            }
        }
    }
    round
}

/// One pass of read-receipt sending, the mirror of `run`.
///
/// When your read mark (`Room::read_through`) has moved past somebody else's
/// message, you owe them a receipt. This finds those, sends one per sender,
/// and advances the room's `receipt_through` so the receipt is sent once
/// rather than on every tick.
///
/// The honesty rules are the same as delivery's, in reverse. A receipt that
/// the far side did not accept does not advance `receipt_through`, so it is
/// re-owed next pass rather than dropped — and because `mark_read` is
/// idempotent, re-sending a receipt that *did* land but whose room could not
/// advance (a group where one member was reachable and another was not) is
/// harmless. `receipt_through` therefore advances only when every sender owed
/// a receipt in this pass accepted it; otherwise it is left where it is and
/// the whole owed set is retried, the same "held, not guessed" stance the
/// courier takes for messages.
///
/// Returns how many receipts were accepted, for a caller that wants to know
/// whether anything moved.
pub fn send_receipts<R: Receipts>(chats: &mut Chats, link: &R, now: u64) -> usize {
    use crate::chat::ME;
    let _ = now;
    let mut accepted = 0usize;
    for room in chats.rooms.iter_mut() {
        if room.read_through <= room.receipt_through {
            continue;
        }
        // Everything incoming that has been read since we last told its
        // sender, grouped by that sender. Ordered so the batch is stable.
        let mut by_sender: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for m in &room.messages {
            if m.from != ME && m.after > room.receipt_through && m.after <= room.read_through {
                by_sender.entry(m.from.clone()).or_default().push(m.id.clone());
            }
        }
        if by_sender.is_empty() {
            // The read mark moved past only our own messages — nothing to
            // receipt. Advance so this range is not reconsidered every pass.
            room.receipt_through = room.read_through;
            continue;
        }
        let mut all_ok = true;
        for (sender, ids) in &by_sender {
            if link.read(sender, ids) {
                accepted += ids.len();
            } else {
                all_ok = false;
            }
        }
        // Only advance when everyone owed a receipt got it. A partial pass
        // leaves the mark, and the whole set is retried next time —
        // re-sending an already-recorded receipt costs nothing because
        // `mark_read` is idempotent.
        if all_ok {
            room.receipt_through = room.read_through;
        }
    }
    accepted
}

/// How many times each message-to-person has been tried, and when last.
///
/// Kept apart from the messages themselves because it is about the *network*
/// rather than about the conversation: it is worthless after a restart in a
/// way the conversation never is, and mixing the two would put a retry
/// counter into the record of what people said.
#[derive(Debug, Clone, Default)]
pub struct Tries {
    seen: std::collections::HashMap<(String, String), (u32, u64)>,
}

impl Tries {
    pub fn of(&self, key: &(String, String)) -> (u32, u64) {
        self.seen.get(key).copied().unwrap_or((0, 0))
    }

    pub fn note(&mut self, key: &(String, String), now: u64) {
        let e = self.seen.entry(key.clone()).or_insert((0, 0));
        e.0 = e.0.saturating_add(1);
        e.1 = now;
    }

    pub fn forget(&mut self, key: &(String, String)) {
        self.seen.remove(key);
    }

}

/// What this install has today: no way to reach anybody else's Atlas.
///
/// **Not a placeholder, and not a stub that pretends.** It is the accurate
/// state of the world on an install where the link between two Atlases has
/// not been built yet, and saying so in code is what lets the rest of the
/// system behave correctly around it: messages are written, held, listed in
/// the outbox, and reported as undelivered — all of which is true, and all of
/// which is what will happen on a real transport when nobody is up.
///
/// The one thing it must never do is look like a delivery. It reports no
/// devices, so nothing is ever tried and nothing is ever marked arrived.
///
/// Replaced, not extended, the day two Atlases can find each other.
pub struct NoLink;

impl Transport for NoLink {
    fn hand_over(&self, _device: &Device, _msg: &Message) -> Handoff {
        // Unreachable while `devices_for` is empty, and written honestly
        // anyway: if some future caller hands this a device, the answer is
        // still that nothing was delivered.
        Handoff::NotUp
    }

    fn devices_for(&self, _peer: &str) -> Vec<Device> {
        Vec::new()
    }
}

/// Why nothing is moving, for a person, when there is no transport.
///
/// Said rather than left to be inferred from messages that sit there. "Sam
/// hasn't picked it up" is true and misleading when the real answer is that
/// there is no way to reach Sam at all yet.
pub fn nothing_can_move_yet(waiting: usize) -> String {
    if waiting == 0 {
        return String::new();
    }
    format!(
        "{waiting} message{} waiting — there's no link to anyone else's Atlas set up \
         yet, so nothing can go out. They're kept, in order, and they'll send \
         themselves when there is one.",
        if waiting == 1 { "" } else { "s" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_grows_and_then_stops_growing() {
        let a = Attempts::default();
        assert_eq!(a.wait_after(0), 0, "the first attempt waits for nothing");
        assert_eq!(a.wait_after(1), 30);
        assert_eq!(a.wait_after(2), 60);
        assert_eq!(a.wait_after(3), 120);
        assert_eq!(a.wait_after(40), a.longest_wait, "the backoff ran away");
        // And it does not overflow on a peer who has been away for years.
        assert_eq!(a.wait_after(u32::MAX), a.longest_wait);
    }

    #[test]
    fn a_device_kind_is_for_saying_out_loud_only() {
        assert_eq!(Kind::Phone.plain(), "phone");
        assert_eq!(Kind::Computer.plain(), "computer");
    }
}
