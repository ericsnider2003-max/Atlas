//! Talking to the people you work with, inside Atlas.
//!
//! One-to-one and groups, the shape everybody already knows from WhatsApp or
//! Telegram — except it lives in your hub, beside the tasks and the documents
//! the conversation is *about*, and no company is in the middle of it.
//!
//! # What is in this file and what is deliberately not
//!
//! This is the part that does not depend on how the bytes travel: rooms, who
//! is in them, what was said, when it was said, and whether it has actually
//! arrived. Transport (a direct link, a relay, mail as a fallback) and the
//! encryption over it are a separate concern and a separate file, and keeping
//! them apart is what lets the hard parts here be tested without a network.
//!
//! # The three things that are easy to get wrong
//!
//! **1. Sending must not wait for anybody.** You write a message when you
//! think of it, at your desk, at midnight, with the other person's laptop
//! shut. `post` therefore always succeeds and always returns immediately; the
//! message is real, timestamped and yours the moment it exists. Delivery is a
//! separate fact recorded separately — see `Delivery`.
//!
//! **2. The timestamp is the sender's, and it is never rewritten.** A message
//! carries the wall clock of the person who wrote it, plus the offset their
//! machine was on. If arrival rewrote it, a message written on Tuesday and
//! collected on Thursday would read as Thursday, and the conversation would
//! quietly become a lie about when things were said. So `sent_at` is set once,
//! by the sender, and everything downstream treats it as evidence rather than
//! as something to correct.
//!
//! **3. Which means wall clocks cannot be what orders the conversation.**
//! This is the trap under the previous point. Two machines' clocks disagree —
//! by seconds usually, by hours if somebody's timezone is wrong, and a laptop
//! that has been shut for a week can come back genuinely behind. Sort by
//! wall clock and you get a reply displayed above the question it answers,
//! which is not cosmetic: it changes what the conversation *means*.
//!
//! So every message also carries `after`: a counter that says what the sender
//! had already seen when they wrote it. It is a Lamport clock, and the only
//! property it has is the one that matters here — if A was seen by whoever
//! wrote B, then A sorts before B, whatever the two clocks say. Wall clock
//! breaks ties between messages that genuinely did not know about each other,
//! because two people typing at once is not an ordering question, it is a
//! coincidence.
//!
//! # Delivery is per person, never a single tick
//!
//! In a group of four, "delivered" is four separate facts. A single flag that
//! means "at least one of them has it" is the kind of summary that reads as
//! reassurance and is not one — you would see a tick and assume the person you
//! were actually talking to had read it. So delivery is recorded per
//! recipient, and `fully_delivered` exists for the cases that genuinely want
//! the summary, spelled out rather than implied.
//!
//! # Nothing here deletes anything on anybody else's machine
//!
//! The same rule `mail.rs` states for your mailbox and for the same reason,
//! one step further out: once a message is on somebody else's computer it is
//! theirs. Atlas can stop showing it to you. It cannot reach over and remove
//! it, and an "unsend" that only clears your own copy while claiming more
//! would be worse than not having one.

use crate::earned::Space;
use crate::error::Result;
use crate::store::Store;
use serde::{Deserialize, Serialize};

/// Has it actually got to somebody.
///
/// Deliberately without a `Sent` state. "Sent" is the word every chat app
/// uses for *left my machine*, and on a link that may be a direct connection,
/// a relay, or an email, "left my machine" can mean four different things and
/// none of them is what the person wants to know. The question is only ever
/// whether it arrived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// Written, kept, not yet known to have arrived. The honest default and
    /// the state a message sits in while the other machine is off.
    Waiting,
    /// Their Atlas has it. Recorded only on an acknowledgement from the
    /// far side -- never on having handed it to a relay, which is a fact
    /// about the relay.
    Arrived(u64),
    /// The *person* on the far side has actually read it -- their Atlas
    /// advanced its read mark past this message and sent a receipt back.
    ///
    /// A strictly stronger fact than `Arrived`: read implies arrived, and a
    /// message can never have been read without first having got there. It is
    /// recorded only on a receipt from the far side, exactly as `Arrived` is
    /// recorded only on a delivery acknowledgement -- never inferred from
    /// anything this end can see on its own. Keeping it separate from
    /// `Arrived` is the same honesty the `Sent`-less design is built on:
    /// "it's on their machine" and "they've seen it" are different questions,
    /// and showing one as the other is the lie a read receipt is supposed to
    /// prevent, not commit.
    Read(u64),
    /// It will not arrive without something changing, and this says what.
    /// Distinguished from `Waiting` because a person can act on one and can
    /// only wait on the other.
    Stuck(String),
}

impl Delivery {
    /// Has it got to their machine? True for `Read` too, because a message
    /// that has been read has, necessarily, arrived first.
    pub fn arrived(&self) -> bool {
        matches!(self, Delivery::Arrived(_) | Delivery::Read(_))
    }

    /// Has the person actually read it? The narrower question, and the only
    /// state that answers it yes.
    pub fn read(&self) -> bool {
        matches!(self, Delivery::Read(_))
    }

    /// For a person, in a few words.
    pub fn plain(&self) -> String {
        match self {
            Delivery::Waiting => "waiting".into(),
            Delivery::Arrived(_) => "arrived".into(),
            Delivery::Read(_) => "read".into(),
            Delivery::Stuck(why) => format!("stuck: {why}"),
        }
    }
}

/// One thing somebody said.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Stable, and made by the sender. Two people composing at the same
    /// instant must not collide, and a message re-delivered after a failure
    /// must land as the same message rather than as a second one.
    pub id: String,
    /// Who wrote it, by the name the roster knows them as. `ME` for you.
    pub from: String,
    pub body: String,
    /// The sender's own clock, at the moment they wrote it. Never rewritten
    /// on arrival -- see this module's doc.
    pub sent_at: u64,
    /// The sender's offset from UTC in minutes when they wrote it, so their
    /// "9pm" can be shown as their 9pm as well as as your 2am. Kept beside
    /// the timestamp rather than derived later, because by the time anybody
    /// looks, they may have moved.
    pub sent_offset_mins: i16,
    /// What the sender had already seen. The ordering that survives two
    /// clocks disagreeing.
    pub after: u64,
    /// Per recipient. One entry per person who was in the room when it was
    /// written -- a person added afterwards is not owed a copy of it, which
    /// is a membership decision made here rather than an accident of when
    /// the network happened to work.
    pub to: Vec<(String, Delivery)>,
}

/// You, in a list of names.
pub const ME: &str = "me";

impl Message {
    /// Did this get to everyone it was written for?
    ///
    /// Spelled out rather than implied by a single flag: in a group, the
    /// summary and the detail are different questions and only one of them
    /// is safe to show as a tick.
    pub fn fully_arrived(&self) -> bool {
        !self.to.is_empty() && self.to.iter().all(|(_, d)| d.arrived())
    }

    /// Who has not got it yet, so a person can be told exactly that.
    pub fn still_waiting(&self) -> Vec<&str> {
        self.to
            .iter()
            .filter(|(_, d)| !d.arrived())
            .map(|(who, _)| who.as_str())
            .collect()
    }

    /// Has everyone it was written for actually read it? The same spelled-out
    /// summary as `fully_arrived`, one step narrower: in a group "everyone has
    /// read it" and "someone has read it" are different questions, and only
    /// the first is safe to show as the double-tick people read as "seen".
    pub fn fully_read(&self) -> bool {
        !self.to.is_empty() && self.to.iter().all(|(_, d)| d.read())
    }

    /// Who has it but has not read it yet.
    pub fn arrived_unread(&self) -> Vec<&str> {
        self.to
            .iter()
            .filter(|(_, d)| d.arrived() && !d.read())
            .map(|(who, _)| who.as_str())
            .collect()
    }

    /// Who has read it. The per-person answer to "who's seen this", which a
    /// group needs — "someone read it" and "everyone read it" are different
    /// facts, and so is *which* someone.
    pub fn read_by(&self) -> Vec<&str> {
        self.to
            .iter()
            .filter(|(_, d)| d.read())
            .map(|(who, _)| who.as_str())
            .collect()
    }

    /// Has anyone it was written for got it at all yet? False while it is still
    /// entirely in the outbox — there is no read to report about a message
    /// that has not landed on a single machine.
    pub fn any_arrived(&self) -> bool {
        self.to.iter().any(|(_, d)| d.arrived())
    }

    /// The sender's own wall clock, as they saw it.
    ///
    /// Seconds since the epoch shifted by their offset — for formatting
    /// "their 9pm", not for arithmetic against anything else.
    pub fn as_they_saw_it(&self) -> i64 {
        self.sent_at as i64 + (self.sent_offset_mins as i64) * 60
    }
}

/// A conversation: one-to-one or a group, they are the same thing with a
/// different number of people in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    /// What it is called. A one-to-one room needs no name and gets the other
    /// person's.
    pub name: String,
    /// Which side of the firewall this belongs to.
    ///
    /// The load-bearing field. A room in `Space::Business("x")` may only
    /// contain people the roster says are in business x, checked on every
    /// send and on every arrival rather than once when the room was made —
    /// the same rule `roster::may_see` states, for the same reason: standing
    /// that is checked once is standing that outlives its revocation.
    pub space: Space,
    pub members: Vec<String>,
    pub messages: Vec<Message>,
    /// How far this end has read.
    pub read_through: u64,
    /// How far this end has already told the *senders* it has read — so a
    /// read receipt is sent once when you read their message, not re-sent on
    /// every tick forever. It only ever trails `read_through`, and advances
    /// only when the receipt was actually accepted by the far side, so a
    /// receipt that didn't land is simply re-owed next pass rather than lost.
    /// `serde(default)` so a room stored before read receipts existed loads as
    /// "nothing receipted yet" rather than failing to parse.
    #[serde(default)]
    pub receipt_through: u64,
}

impl Room {
    /// Is this a group rather than a one-to-one? More than one other person,
    /// or a group with an owner -- which stays a group with one other person
    /// in it (Eric and Sam; or your phone, whose only member is your laptop).
    /// Counting members alone made "message the Friends group" answer "I
    /// don't know anyone called the Friends group" in exactly those.
    pub fn is_group(&self) -> bool {
        self.members.len() > 1 || crate::groups::is_owned_id(&self.id)
    }

    /// The conversation in the order it happened.
    ///
    /// Causal first, wall clock only to break ties. Two messages that did not
    /// know about each other are genuinely concurrent, and putting the
    /// earlier clock first is a presentation choice rather than a claim.
    pub fn in_order(&self) -> Vec<&Message> {
        let mut out: Vec<&Message> = self.messages.iter().collect();
        out.sort_by(|a, b| {
            a.after
                .cmp(&b.after)
                .then(a.sent_at.cmp(&b.sent_at))
                .then(a.id.cmp(&b.id))
        });
        out
    }

    /// What you have not read.
    pub fn unread(&self) -> Vec<&Message> {
        self.in_order()
            .into_iter()
            .filter(|m| m.from != ME && m.after > self.read_through)
            .collect()
    }

    /// Everything of yours that has not reached everybody.
    ///
    /// The outbox, and the reason it is worth having: a message that has not
    /// arrived is invisible otherwise, and "I told you on Tuesday" is exactly
    /// the argument this prevents.
    pub fn undelivered(&self) -> Vec<&Message> {
        self.messages.iter().filter(|m| m.from == ME && !m.fully_arrived()).collect()
    }

    fn highest_seen(&self) -> u64 {
        self.messages.iter().map(|m| m.after).max().unwrap_or(0)
    }

    /// Your most recent message in this room, in causal order — the one a "has
    /// it been read" glance is actually asking about.
    pub fn last_mine(&self) -> Option<&Message> {
        self.in_order().into_iter().rev().find(|m| m.from == ME)
    }
}

/// Every conversation this Atlas holds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Chats {
    pub rooms: Vec<Room>,
    /// This end's logical clock. One per install rather than one per room:
    /// it only ever has to move forward, and a single counter cannot get out
    /// of step with itself.
    pub clock: u64,
    /// Group ids you have left. A tombstone, and it has to be one: leaving
    /// only removes your copy of the room, but the other members' Atlases do
    /// not all know yet, so a message already in flight — or one from a member
    /// who never got your "left" notice — would otherwise re-open the room
    /// through `open_group` and quietly put you back in a conversation you
    /// walked out of. Keyed by the shared group id, so leaving blocks exactly
    /// that group and not a fresh one the same people might start later (which
    /// mints a new id). `serde(default)` for rooms stored before this existed.
    #[serde(default)]
    pub left: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Somebody in the room is not (or is no longer) in that business.
    NotInThatBusiness(String),
    /// There is no such room.
    NoSuchRoom(String),
    /// A room with nobody in it is a note to yourself, and there is already
    /// somewhere better for those.
    NobodyToTalkTo,
    /// You left this group, and a message tried to re-open it. Dropped rather
    /// than silently putting you back in the conversation.
    LeftThatGroup(String),
}

impl Refused {
    pub fn plain(&self) -> String {
        match self {
            Refused::NotInThatBusiness(who) => {
                format!("{who} isn't in that business, so I've left them out of it")
            }
            Refused::NoSuchRoom(id) => format!("there's no conversation called {id}"),
            Refused::NobodyToTalkTo => "there's nobody in that conversation".into(),
            Refused::LeftThatGroup(id) => format!("you've left the group {id}"),
        }
    }
}

pub const FILE: &str = "chats";

impl Chats {
    pub fn load(store: &Store) -> Chats {
        store.load(FILE)
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.save(FILE, self)
    }

    /// Start a conversation.
    ///
    /// Membership of a business room is checked here *and* on every send and
    /// arrival. Checking only here would mean a room outlives the standing
    /// that justified it, which is how somebody keeps reading a business
    /// they were removed from.
    pub fn open(
        &mut self,
        name: &str,
        space: Space,
        members: &[String],
        roster: &crate::roster::Roster,
        pairings: &crate::kin::Pairings,
    ) -> std::result::Result<String, Refused> {
        if members.is_empty() {
            return Err(Refused::NobodyToTalkTo);
        }
        if let Space::Business(business) = &space {
            for m in members {
                if !roster.may_see(business, m, pairings) {
                    return Err(Refused::NotInThatBusiness(m.clone()));
                }
            }
        }
        let id = room_id(name, members);
        if !self.rooms.iter().any(|r| r.id == id) {
            self.rooms.push(Room {
                id: id.clone(),
                name: name.to_string(),
                space,
                members: members.to_vec(),
                messages: Vec::new(),
                read_through: 0,
                receipt_through: 0,
            });
        }
        Ok(id)
    }

    /// Open (or update) a group room under an id shared across every member's
    /// Atlas.
    ///
    /// A one-to-one room derives its id from the pair, so both ends compute the
    /// same one. A group cannot: each member's Atlas lists a *different* set of
    /// people (everyone but themselves), so a derived id would never match. So
    /// a group carries a minted id — made once by whoever starts it, then
    /// travelling on every message — and this opens the room under that id.
    ///
    /// If the room already exists, its identity is kept and any members not
    /// yet known are folded in: a group learns who is in it as it hears from
    /// them, which is how a member you are not the one who added still ends up
    /// on your copy of the roster. Business membership is re-checked for every
    /// member, the same rule `open` enforces.
    pub fn open_group(
        &mut self,
        id: &str,
        name: &str,
        space: Space,
        members: &[String],
        roster: &crate::roster::Roster,
        pairings: &crate::kin::Pairings,
    ) -> std::result::Result<String, Refused> {
        // Walked out of, and staying out. Checked before anything else so a
        // message in flight cannot re-open the room and put you back in it.
        if self.has_left(id) {
            return Err(Refused::LeftThatGroup(id.to_string()));
        }
        if let Space::Business(business) = &space {
            for m in members {
                if !roster.may_see(business, m, pairings) {
                    return Err(Refused::NotInThatBusiness(m.clone()));
                }
            }
        }
        if let Some(room) = self.rooms.iter_mut().find(|r| r.id == id) {
            for m in members {
                if !room.members.iter().any(|x| crate::kin::same_name(x, m)) {
                    room.members.push(m.clone());
                }
            }
            return Ok(id.to_string());
        }
        self.rooms.push(Room {
            id: id.to_string(),
            name: name.to_string(),
            space,
            members: members.to_vec(),
            messages: Vec::new(),
            read_through: 0,
            receipt_through: 0,
        });
        Ok(id.to_string())
    }

    /// Make an owned group's room match its owner's signed list: exactly these
    /// members and this name, created if new, and re-opened if you had been
    /// taken out and are back in. Returns whether anything changed.
    ///
    /// Unlike `open_group`, which learns members from whoever writes, this
    /// replaces the member list outright: for a group with an owner, the list
    /// is decided by the owner and nobody else (`groups`).
    pub fn settle_owned(&mut self, id: &str, name: &str, members: Vec<String>) -> bool {
        let rejoined = self.left.iter().any(|l| l == id);
        self.left.retain(|l| l != id);
        if let Some(room) = self.rooms.iter_mut().find(|r| r.id == id) {
            if room.members == members && room.name == name {
                return rejoined;
            }
            room.members = members;
            room.name = name.to_string();
            return true;
        }
        self.rooms.push(Room {
            id: id.to_string(),
            name: name.to_string(),
            space: Space::Personal,
            members,
            messages: Vec::new(),
            read_through: 0,
            receipt_through: 0,
        });
        true
    }

    /// Rename a group — a label for your own copy. The name is not identity
    /// (the shared id is what threads the conversation), so this renames the
    /// room you hold; it doesn't rename anyone else's. Returns whether there
    /// was a group with that id to rename.
    pub fn name_group(&mut self, id: &str, name: &str) -> bool {
        match self.rooms.iter_mut().find(|r| r.id == id && r.is_group()) {
            Some(r) => {
                r.name = name.trim().to_string();
                true
            }
            None => false,
        }
    }

    /// Leave a group: drop your copy of the room and remember its id so a
    /// message still in flight cannot re-open it.
    ///
    /// Returns the members you were in it with, so the caller can tell them
    /// you've gone — because leaving your own copy is not the same as anyone
    /// else knowing, and a group that shows you as still there is the quiet
    /// wrong state this is written against. Only a group leaves this way: a
    /// one-to-one is not a room you are a "member" of, and "leaving" it would
    /// just be forgetting the person, which is `Door::forget`'s job.
    ///
    /// Idempotent: leaving a group you already left, or one you never had,
    /// returns an empty list and records the tombstone anyway, so the block
    /// holds either way.
    pub fn leave_group(&mut self, id: &str) -> Vec<String> {
        let members = self
            .rooms
            .iter()
            .find(|r| r.id == id && r.is_group())
            .map(|r| r.members.clone())
            .unwrap_or_default();
        self.rooms.retain(|r| r.id != id);
        if !self.left.iter().any(|l| l == id) {
            self.left.push(id.to_string());
        }
        members
    }

    /// Have you left this group? The tombstone check `open_group` and the
    /// receive path consult before re-opening a room.
    pub fn has_left(&self, id: &str) -> bool {
        self.left.iter().any(|l| l == id)
    }

    /// Somebody else left a group: take them out of your copy of its
    /// membership, so a later message to the group is not addressed to a
    /// person who has gone. Only touches a group (more than one member) and
    /// only the named person; returns whether they were there to remove.
    /// Their past messages stay — they said those while they were in it.
    pub fn member_left(&mut self, group_id: &str, who: &str) -> bool {
        let Some(room) = self.rooms.iter_mut().find(|r| r.id == group_id && r.is_group())
        else {
            return false;
        };
        let before = room.members.len();
        room.members.retain(|m| !crate::kin::same_name(m, who));
        room.members.len() != before
    }

    /// A group room by the name you gave it, case-insensitively — so
    /// "message the Northwind group" finds the one you named rather than
    /// starting a new conversation.
    pub fn group_named(&self, name: &str) -> Option<&Room> {
        let n = name.trim();
        self.rooms
            .iter()
            .find(|r| r.is_group() && r.name.eq_ignore_ascii_case(n))
    }

    pub fn room(&self, id: &str) -> Option<&Room> {
        self.rooms.iter().find(|r| r.id == id)
    }

    pub fn room_mut(&mut self, id: &str) -> Option<&mut Room> {
        self.rooms.iter_mut().find(|r| r.id == id)
    }

    /// Say something. Always succeeds, never waits for anybody.
    ///
    /// This returning a message is the whole point: the thing exists, with
    /// your time on it, before anything has been asked of the network. What
    /// the network does afterwards changes `Delivery` and nothing else.
    ///
    /// Membership is re-checked, and anybody who has since left the business
    /// is left out of `to` rather than silently sent to. They do not get the
    /// message and you are told they did not.
    pub fn post(
        &mut self,
        room_id: &str,
        body: &str,
        now: u64,
        offset_mins: i16,
        roster: &crate::roster::Roster,
        pairings: &crate::kin::Pairings,
    ) -> std::result::Result<(Message, Vec<String>), Refused> {
        // The clock moves before the message is built, so two messages from
        // this end are never equal and never need the tiebreak.
        let seen = self
            .room(room_id)
            .ok_or_else(|| Refused::NoSuchRoom(room_id.to_string()))?
            .highest_seen();
        self.clock = self.clock.max(seen) + 1;
        let after = self.clock;

        let Some(room) = self.room_mut(room_id) else { return Err(Refused::NoSuchRoom(room_id.to_string())) };
        let space = room.space.clone();
        let mut to = Vec::new();
        let mut left_out = Vec::new();
        for m in &room.members {
            if let Space::Business(business) = &space {
                if !roster.may_see(business, m, pairings) {
                    left_out.push(m.clone());
                    continue;
                }
            }
            to.push((m.clone(), Delivery::Waiting));
        }
        if to.is_empty() {
            return Err(Refused::NobodyToTalkTo);
        }
        let msg = Message {
            id: format!("{}-{}-{}", room_id, now, after),
            from: ME.to_string(),
            body: body.to_string(),
            sent_at: now,
            sent_offset_mins: offset_mins,
            after,
            to,
        };
        room.messages.push(msg.clone());
        // Replying marks what you had *already seen* as read, not everything
        // up to your own counter. The difference shows up in a group: two
        // messages can land while you are typing, and marking those read
        // because you happened to press send is how an unread message is
        // lost without anybody deciding to lose it.
        room.read_through = room.read_through.max(seen);
        Ok((msg, left_out))
    }

    /// Something arrived from somebody else.
    ///
    /// Their timestamp is kept exactly as they set it. Our clock moves to
    /// past theirs, which is what makes our next message sort after this one
    /// no matter whose wall clock is ahead.
    ///
    /// Idempotent by id: a message re-delivered after a failed acknowledgement
    /// must not appear twice, and on any store-and-forward path that happens
    /// as a matter of course rather than as an error.
    pub fn receive(
        &mut self,
        room_id: &str,
        msg: Message,
        roster: &crate::roster::Roster,
        pairings: &crate::kin::Pairings,
    ) -> std::result::Result<bool, Refused> {
        let room = self
            .rooms
            .iter()
            .find(|r| r.id == room_id)
            .ok_or_else(|| Refused::NoSuchRoom(room_id.to_string()))?;
        // Dropped at the door, not filtered after being shown. A message
        // claiming to be from somebody who is not in this business is the
        // exact liability the roster exists for.
        if let Space::Business(business) = &room.space {
            if !roster.may_see(business, &msg.from, pairings) {
                return Err(Refused::NotInThatBusiness(msg.from.clone()));
            }
        }
        if !room.members.iter().any(|m| crate::kin::same_name(m, &msg.from)) {
            return Err(Refused::NotInThatBusiness(msg.from.clone()));
        }
        self.clock = self.clock.max(msg.after) + 1;
        let Some(room) = self.room_mut(room_id) else { return Err(Refused::NoSuchRoom(room_id.to_string())) };
        if room.messages.iter().any(|m| m.id == msg.id) {
            return Ok(false);
        }
        room.messages.push(msg);
        Ok(true)
    }

    /// File a group message whose sender the caller has already checked
    /// against the group's owner-signed list (`groups`), including one passed
    /// on by the owner from a member this Atlas isn't paired with -- so not
    /// someone `receive`'s room-membership check could find. Idempotent by id,
    /// like `receive`.
    pub fn receive_vouched(&mut self, room_id: &str, msg: Message) -> std::result::Result<bool, Refused> {
        let seen = msg.after;
        let room = self.room_mut(room_id).ok_or_else(|| Refused::NoSuchRoom(room_id.to_string()))?;
        if room.messages.iter().any(|m| m.id == msg.id) {
            return Ok(false);
        }
        room.messages.push(msg);
        self.clock = self.clock.max(seen) + 1;
        Ok(true)
    }

    /// Their Atlas said it has it.
    pub fn mark_arrived(&mut self, room_id: &str, message_id: &str, who: &str, now: u64) -> bool {
        let Some(room) = self.room_mut(room_id) else { return false };
        let Some(msg) = room.messages.iter_mut().find(|m| m.id == message_id) else {
            return false;
        };
        for (name, state) in msg.to.iter_mut() {
            if crate::kin::same_name(name, who) {
                *state = Delivery::Arrived(now);
                return true;
            }
        }
        false
    }

    /// Their person actually read it.
    ///
    /// Found by message id across every room rather than by room, because the
    /// reader cannot name the sender's room: a one-to-one room's id is built
    /// from the member names, which differ on each end, so my room with Sam
    /// and Sam's room with me have different ids. The message id does not have
    /// that problem — it is minted once by the sender and travels unchanged —
    /// so it is the thing a receipt carries and the thing this looks up.
    ///
    /// Only ever an upgrade: `Read` is a strictly stronger fact than `Arrived`
    /// or `Waiting` or even `Stuck` (a message can be marked stuck by a failed
    /// delivery attempt and still turn out to have been read on a path this end
    /// couldn't see), so a receipt wins over whatever was there. It never
    /// downgrades a `Read` back, and a receipt for a recipient not on the
    /// message is ignored rather than invented.
    pub fn mark_read(&mut self, message_id: &str, who: &str, now: u64) -> bool {
        for room in self.rooms.iter_mut() {
            if let Some(msg) = room.messages.iter_mut().find(|m| m.id == message_id) {
                for (name, state) in msg.to.iter_mut() {
                    if crate::kin::same_name(name, who) {
                        *state = Delivery::Read(now);
                        return true;
                    }
                }
                // The message is here but this person was never a recipient of
                // it. A receipt claiming otherwise is dropped, not recorded.
                return false;
            }
        }
        false
    }

    /// It is not going to arrive as things stand, and this is why.
    pub fn mark_stuck(&mut self, room_id: &str, message_id: &str, who: &str, why: &str) -> bool {
        let Some(room) = self.room_mut(room_id) else { return false };
        let Some(msg) = room.messages.iter_mut().find(|m| m.id == message_id) else {
            return false;
        };
        for (name, state) in msg.to.iter_mut() {
            if crate::kin::same_name(name, who) && !state.arrived() {
                *state = Delivery::Stuck(why.to_string());
                return true;
            }
        }
        false
    }

    /// Everything still to be delivered, anywhere, so one pass can try them
    /// all when a link comes up.
    pub fn outbox(&self) -> Vec<(&str, &Message)> {
        self.rooms
            .iter()
            .flat_map(|r| r.undelivered().into_iter().map(move |m| (r.id.as_str(), m)))
            .collect()
    }

    /// The read side of the ticks, for when you look at your conversations.
    ///
    /// One line per room, about your most recent message, naming *who* has read
    /// it — because in a group "who's seen this" is a per-person question, not
    /// a single tick. Factual and repeatable, not a notification: the
    /// double-tick you glance at, not a ping.
    ///
    /// # The honest part a mesh forces
    ///
    /// You do not have a direct link to every group member — some you reach
    /// only *through* another member (`who_is_in` says which). A read receipt
    /// has to come back the same way, and today it only comes back over a
    /// direct link. So for a member you are not paired with, their read simply
    /// **cannot reach you**, and reporting that as "hasn't read it" would be a
    /// lie: it is "no receipt possible", a different thing. `reachable` is how
    /// this end knows which is which — pass it the same test `who_is_in` uses
    /// (are you paired with them). A member you can reach who hasn't read it is
    /// "not yet"; one you can't is named as unreachable, not as unread.
    ///
    /// Silent for a room whose last message is someone else's, and for one that
    /// has not landed on a single machine yet (that is the outbox's to report).
    pub fn read_state(&self, reachable: impl Fn(&str) -> bool) -> Vec<String> {
        let mut out = Vec::new();
        for room in &self.rooms {
            let Some(mine) = room.last_mine() else { continue };
            if !mine.any_arrived() {
                continue;
            }
            let read = mine.read_by();
            // Those who have it but haven't read it, split by whether a receipt
            // from them could ever reach you.
            let (awaiting, no_receipt): (Vec<&str>, Vec<&str>) =
                mine.arrived_unread().into_iter().partition(|who| reachable(who));

            let is_group = room.is_group();
            if !is_group {
                // A one-to-one: one other person, so one clear fact.
                if !read.is_empty() {
                    out.push(format!("{} read your last message.", room.name));
                } else if !no_receipt.is_empty() {
                    out.push(format!(
                        "{} has your last message. You're not paired with them directly, so you \
                         won't get a read receipt.",
                        room.name
                    ));
                } else if !awaiting.is_empty() {
                    out.push(format!("{} has your last message but hasn't read it yet.", room.name));
                }
                continue;
            }

            // A group where every recipient has read it — the clean summary,
            // worth saying as one thing rather than listing everyone.
            if mine.fully_read() {
                out.push(format!("everyone in {} has read your last message.", room.name));
                continue;
            }

            // A group: name each set that has anyone in it.
            let mut parts: Vec<String> = Vec::new();
            if !read.is_empty() {
                parts.push(format!("read by {}", read.join(", ")));
            }
            if !awaiting.is_empty() {
                parts.push(format!("not yet by {}", awaiting.join(", ")));
            }
            if !no_receipt.is_empty() {
                parts.push(format!(
                    "no receipt from {} (not paired, so you can't tell)",
                    no_receipt.join(", ")
                ));
            }
            if !parts.is_empty() {
                out.push(format!("{} — {}.", room.name, parts.join("; ")));
            }
        }
        out
    }

    /// What to say out loud when you ask about your messages.
    ///
    /// Leads with what is waiting on *you*, because that is the only part
    /// that is actionable; what is waiting on somebody else is mentioned
    /// second and only when there is any, since a running commentary on
    /// undelivered mail is the notification noise this codebase is written
    /// against.
    pub fn spoken(&self) -> String {
        let unread: usize = self.rooms.iter().map(|r| r.unread().len()).sum();
        let waiting = self.outbox().len();
        match (unread, waiting) {
            (0, 0) => "Nothing new.".into(),
            (0, w) => format!(
                "Nothing new. {w} of yours {} still waiting to be picked up.",
                if w == 1 { "is" } else { "are" }
            ),
            (n, 0) => {
                let who = self.who_is_waiting();
                format!("{n} unread{who}.")
            }
            (n, w) => {
                let who = self.who_is_waiting();
                format!("{n} unread{who}, and {w} of yours still waiting to be picked up.")
            }
        }
    }

    fn who_is_waiting(&self) -> String {
        let mut names: Vec<&str> = self
            .rooms
            .iter()
            .filter(|r| !r.unread().is_empty())
            .map(|r| r.name.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        match names.len() {
            0 => String::new(),
            1 => format!(" from {}", names[0]),
            2 => format!(" from {} and {}", names[0], names[1]),
            n => format!(" across {n} conversations"),
        }
    }
}

/// A room's id: stable for the same people whichever end makes it.
///
/// Sorted, so you and I independently opening a conversation with each other
/// produce the same room rather than two halves of one.
fn room_id(name: &str, members: &[String]) -> String {
    let mut who: Vec<String> = members.iter().map(|m| m.to_lowercase()).collect();
    who.sort();
    who.dedup();
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    format!("{}-{}", slug.trim_matches('-'), who.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_two_people_open_the_same_room_from_either_end() {
        let mine = room_id("Sam", &["Sam".into()]);
        let theirs = room_id("sam", &["SAM".into()]);
        assert_eq!(mine, theirs);
    }

    #[test]
    fn a_group_id_does_not_depend_on_who_listed_the_members_first() {
        let a = room_id("Roof job", &["Sam".into(), "Ali".into()]);
        let b = room_id("Roof job", &["Ali".into(), "Sam".into()]);
        assert_eq!(a, b);
    }

    #[test]
    fn delivery_has_no_state_that_means_left_my_machine() {
        // Asserted rather than trusted, because "Sent" is what every chat app
        // calls it and adding it back would be the obvious change to make.
        // On a link that might be direct, a relay or an email, it would mean
        // four different things and none of them is "they have it".
        let states = [
            Delivery::Waiting,
            Delivery::Arrived(1),
            Delivery::Stuck("no route".into()),
        ];
        for s in &states {
            assert!(!s.plain().contains("sent"), "{}", s.plain());
        }
    }
}
