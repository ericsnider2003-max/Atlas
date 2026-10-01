//! Messages, groups and friends: signals, handovers, reading and sending
//! messages, handoffs between your devices, Tor, friend requests, add-on shares
//! and the peer upkeep.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    /// Open the door this daemon will check every tick. Not called unless
    /// you have actually registered a peer -- an empty-but-listening door is
    /// a different, worse thing than no door at all.
    /// The friends' door couldn't be opened at start: try again once a
    /// minute (29 Sep 2026: a busy port kept it shut for the whole session,
    /// where the hub already tried again).
    pub fn with_signal_door_later(mut self, port: u16, peers: Vec<crate::kin::Peer>) -> Self {
        self.signal_retry = Some((port, peers, 0));
        self
    }

    pub fn open_signal_door_again(&mut self, t: u64) {
        let Some((port, peers, at)) = self.signal_retry.as_ref() else { return };
        if self.signal_listener.is_some() {
            self.signal_retry = None;
            return;
        }
        if t < *at {
            return;
        }
        let (port, peers) = (*port, peers.clone());
        match crate::server::SignalListener::bind(port, peers.clone()) {
            Ok(l) => {
                self.log.info(&format!("the door for friends is open now, on port {port}"));
                self.fit_door(&l);
                self.signal_listener = Some(l);
                self.signal_retry = None;
                if let Err(e) = self.start_tor() {
                    self.log.warn(&format!("Tor didn't start: {e}"));
                }
            }
            Err(_) => self.signal_retry = Some((port, peers, t + 60)),
        }
    }

    pub fn with_signal_listener(mut self, l: crate::server::SignalListener) -> Self {
        self.fit_door(&l);
        self.signal_listener = Some(l);
        self
    }

    /// Give this daemon somewhere a passphrase can be typed.
    ///
    /// Without it, "I'm back" is answered honestly -- the way out is named,
    /// and nothing is claimed to have been asked.
    /// Start the model server when it's needed and keep it warm.
    ///
    /// Before 27 Sep 2026 nothing did: the server Atlas's own model
    /// connection talks to was started only if you asked "which model", and
    /// even then the helper budget (600 MB, sized for a browser) refused it.
    /// On a fresh machine — a friend's — every question went to a server
    /// that wasn't there.
    pub fn starting_the_model_server(mut self) -> Self {
        self.starts_model_server = true;
        self
    }

    pub fn with_typed_prompt(mut self, asker: Box<dyn crate::typed::AsksQuietly>) -> Self {
        self.asks_quietly = Some(asker);
        self
    }

    /// The handover, from the install's own state.
    ///
    /// Never from `self.store`, which is the *active person's* directory: a
    /// handover is a fact about the machine, and one kept inside a profile
    /// would be cleared by switching profiles, which passes through no gate
    /// at all.
    pub(crate) fn handover(&self) -> crate::handover::Handover {
        crate::handover::Handover::load(&crate::roots::install_state())
    }

    /// What Atlas says instead of doing this, while it is handed over.
    ///
    /// `None` when it is yours, or when the action is an ordinary one that a
    /// guest is welcome to.
    ///
    /// # Why this is here and not only in `main.rs`
    ///
    /// It was only in `main.rs`, in `gate_with_identity`, and that function
    /// guards the two paths where a *person at a keyboard* types a line. The
    /// daemon's own listening loop -- the hands-free one, the one that is
    /// running when somebody is holding your laptop and talking to it --
    /// calls `turn_from` directly and passed through no such gate. So the
    /// restriction held for the path least likely to be used by a stranger
    /// and not for the path most likely to be.
    ///
    /// This is the chokepoint: every spoken turn goes through `turn_from`,
    /// including the ones `main.rs` gates, so the strictest of the two wins
    /// and neither can be the only one.
    pub(crate) fn handed_over_refusal(&self, intent: &Intent) -> Option<String> {
        let kind = kind_of(intent);
        if !crate::handover::refuses(kind) {
            return None;
        }
        if !self.handover().stance.handed_over() {
            return None;
        }
        Some(crate::handover::refusal(kind))
    }

    /// What has come in, said rather than counted.
    ///
    /// Leads with what people said, because that is the part you asked for.
    /// What has not been picked up comes after, named rather than counted --
    /// "Sam hasn't picked up two" tells you whether to phone him, where
    /// "2 waiting" tells you nothing you can act on.
    /// Give a group a name, so you can reach it by that name later
    /// ("message the Northwind group: …"). The argument is "<which> to <name>"
    /// or "<which> as <name>", where <which> is the group's current name or
    /// its member list. A name is a label on your own copy — the shared id is
    /// what actually threads the conversation across everyone's Atlas.
    pub(super) fn rename_group(&mut self, arg: &str) -> String {
        let Some((which, name)) = arg
            .split_once(" to ")
            .or_else(|| arg.split_once(" as "))
            .or_else(|| arg.split_once(" the "))
        else {
            return "Tell me which group and what to call it — \
                    \"name the Jordan, Maya group as Northwind project\"."
                .into();
        };
        let which = which.trim().strip_prefix("the ").unwrap_or(which.trim());
        let which = which.strip_suffix(" group").unwrap_or(which).trim();
        let name = name.trim();
        if name.is_empty() {
            return "What should I call it?".into();
        }
        let Some(id) = self.chats.group_named(which).map(|r| r.id.clone()) else {
            return format!("You don't have a group called \"{which}\".");
        };
        if self.chats.name_group(&id, name) {
            let _ = self.chats.save(&self.store);
            format!("Renamed it to \"{name}\". You can say \"message the {name} group\" now.")
        } else {
            format!("\"{which}\" isn't a group I can rename.")
        }
    }

    /// Leave a group.
    ///
    /// Three things happen, and saying which of them held is the honest part.
    /// Your copy of the room goes and its id is tombstoned, so a message
    /// already in flight cannot pull you back in — that always works, it is
    /// local. Then the others are told, over the mesh, and that is best-effort:
    /// a member who is offline will not hear it now and will still show you in
    /// the group until their Atlas next tries to reach it. So the reply
    /// distinguishes "told everyone" from "couldn't reach so-and-so yet",
    /// rather than claiming a clean exit that only half happened.
    pub(super) fn leave_group(&mut self, arg: &str) -> String {
        let key = arg.trim().strip_prefix("the ").unwrap_or(arg.trim());
        let key = key.strip_suffix(" group").unwrap_or(key).trim();
        if key.is_empty() {
            return "Which group? Try \"leave the Northwind group\".".into();
        }
        let Some((id, name)) = self.chats.group_named(key).map(|r| (r.id.clone(), r.name.clone()))
        else {
            return format!("You don't have a group called \"{key}\".");
        };
        if crate::groups::is_owned_id(&id) {
            let groups = crate::groups::Groups::load(&self.store);
            if groups.held.get(&id).map(|h| Some(h.state.owner.clone()) == self.my_key()).unwrap_or(false) {
                return format!(
                    "You made \"{name}\", so you're its owner -- take the others out instead \
                     (on the Groups page), or keep it."
                );
            }
        }
        // Built before leaving, so it holds a snapshot of who to tell; the
        // contacts it needs come from pairings, not the room that is about to
        // go.
        let link = {
            let pairings = crate::kin::Pairings::load(&self.peer_dir);
            self.peer_link(&pairings)
        };
        let members = self.chats.leave_group(&id);
        let _ = self.chats.save(&self.store);

        let mut told = Vec::new();
        let mut unreached = Vec::new();
        for m in &members {
            if link.tell_left(m, &id) {
                told.push(m.clone());
            } else {
                unreached.push(m.clone());
            }
        }
        let mut s = format!("Left \"{name}\".");
        if !told.is_empty() {
            s.push_str(&format!(" Told {}.", told.join(", ")));
        }
        if !unreached.is_empty() {
            s.push_str(&format!(
                " Couldn't reach {} to say so yet — they'll find out when their Atlas next \
                 tries the group.",
                unreached.join(", ")
            ));
        }
        s
    }

    /// Who's in a group, and — the mesh's honest half — which of them you can
    /// actually reach. In a peer-to-peer group you deliver only to members you
    /// are yourself paired with; the rest are reached by other members who
    /// are. Saying who you can't reach is the difference between "they haven't
    /// replied" and "your messages never went to them in the first place".
    pub(super) fn who_is_in(&self, arg: &str) -> String {
        let key = arg.trim().strip_prefix("the ").unwrap_or(arg.trim());
        let key = key.strip_suffix(" group").unwrap_or(key).trim();
        if key.is_empty() {
            return "Which group? Try \"who's in the Northwind group\".".into();
        }
        let Some(room) = self.chats.group_named(key) else {
            return format!("You don't have a group called \"{key}\".");
        };
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let mut reachable = Vec::new();
        let mut cannot = Vec::new();
        for m in &room.members {
            if pairings.contacts.iter().any(|c| crate::kin::same_name(&c.name, m)) {
                reachable.push(m.clone());
            } else {
                cannot.push(m.clone());
            }
        }
        let n = room.members.len();
        let mut s = format!(
            "{} has {n} member{}: {}.",
            room.name,
            if n == 1 { "" } else { "s" },
            room.members.join(", ")
        );
        if !cannot.is_empty() {
            s.push_str(&format!(
                " You're not paired with {}, so your messages don't reach them directly — \
                 someone else in the group who is paired with them carries those.",
                cannot.join(", ")
            ));
        } else {
            s.push_str(" You can reach all of them.");
        }
        s
    }

    pub(super) fn read_messages(&mut self) -> String {
        let mut lines = Vec::new();
        for room in &self.chats.rooms {
            let unread = room.unread();
            if unread.is_empty() {
                continue;
            }
            for m in unread {
                // Said in their time when their time is not yours. Somebody
                // messaging at 11pm their time is a different fact from
                // somebody messaging at 4pm yours, and it is the one the
                // timestamp was kept for.
                let mine = local_offset_mins();
                let theirs = if m.sent_offset_mins == mine {
                    String::new()
                } else {
                    let hour = ((m.as_they_saw_it().rem_euclid(86_400)) / 3600) as u32;
                    format!(" (their {})", oclock(hour))
                };
                // In a group, say which one — a bare name reads as a
                // one-to-one and loses that it went to everybody.
                let who = if room.is_group() {
                    format!("in {} — {}", room.name, m.from)
                } else {
                    m.from.clone()
                };
                lines.push(format!("{}{}: {}", who, theirs, m.body));
            }
        }
        // Marked read only once it has actually been said out loud. Marking
        // on the way in would lose a message to a turn that was interrupted.
        let now_read: Vec<(String, u64)> = self
            .chats
            .rooms
            .iter()
            .map(|r| (r.id.clone(), r.messages.iter().map(|m| m.after).max().unwrap_or(0)))
            .collect();

        let mut waiting: Vec<String> = Vec::new();
        for (_, m) in self.chats.outbox() {
            for who in m.still_waiting() {
                if !waiting.iter().any(|w| w == who) {
                    waiting.push(who.to_string());
                }
            }
        }

        // The read side of the ticks: which of your messages have been seen.
        // A glance-at fact, shown because you asked about your messages, not a
        // proactive ping — so it counts as something to show, but does not on
        // its own turn a quiet check into news.
        // Reachability, the same test `who_is_in` uses: you get a read receipt
        // back only from a member you're paired with directly. For anyone else
        // in a group, their read can't reach you, and `read_state` says so
        // rather than calling it unread.
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let read_lines = self
            .chats
            .read_state(|who| pairings.contacts.iter().any(|c| crate::kin::same_name(&c.name, who)));

        if lines.is_empty() && waiting.is_empty() && read_lines.is_empty() {
            return "Nothing new.".into();
        }
        // Why nothing is moving, when the answer is "there is nowhere for it
        // to go yet" rather than "they haven't looked". Saying the first as
        // though it were the second is how somebody waits three days for a
        // message that was never going to leave the machine.
        let held = self.chats.outbox().len();
        let no_link = crate::courier::nothing_can_move_yet(held);
        let mut said = if lines.is_empty() {
            String::new()
        } else {
            for (id, through) in now_read {
                if let Some(r) = self.chats.room_mut(&id) {
                    r.read_through = r.read_through.max(through);
                }
            }
            let _ = self.chats.save(&self.store);
            lines.join(". ")
        };
        if !no_link.is_empty() {
            if !said.is_empty() {
                said.push(' ');
            }
            said.push_str(&no_link);
        } else if !waiting.is_empty() {
            if !said.is_empty() {
                said.push(' ');
            }
            said.push_str(&format!(
                "{} {} picked up what you sent.",
                waiting.join(" and "),
                if waiting.len() == 1 { "hasn't" } else { "haven't" }
            ));
        }
        if !read_lines.is_empty() {
            if !said.is_empty() {
                said.push(' ');
            }
            said.push_str(&read_lines.join(" "));
        }
        said
    }

    /// "Stop telling me about the backups" — and the way back.
    ///
    /// Stored rather than written into `config/tools.yaml`: nothing in this
    /// tree rewrites a person's config, and a spoken aside reformatting a
    /// file they hand-edit — comments, ordering and all — is not where to
    /// start. `worth_saying` consults both lists.
    pub(super) fn mute_topic(&mut self, said: &str) -> String {
        let mut muted = crate::interrupt::Muted::load(&self.store);
        // Unmuting is checked first: "start telling me about the backups"
        // contains "tell me about", and a mute lead-in would otherwise claim
        // the sentence and silence the very thing being asked for.
        if let Some(topic) = crate::interrupt::unmute_from(said) {
            let said_back = if muted.unmute(&topic) {
                format!("I'll mention {topic} again.")
            } else {
                format!("I wasn't keeping quiet about {topic}.")
            };
            let _ = muted.save(&self.store);
            return said_back;
        }
        if let Some(topic) = crate::interrupt::mute_from(said) {
            let said_back = if muted.mute(&topic) {
                // Says the way back in the same breath. A switch that only
                // goes one way is how somebody wonders, three weeks later,
                // why Atlas never mentions their backups.
                format!("I'll stop mentioning {topic}. Say \"start telling me about {topic}\" to undo that.")
            } else {
                format!("I was already keeping quiet about {topic}.")
            };
            let _ = muted.save(&self.store);
            return said_back;
        }
        muted.spoken()
    }

    /// Say something to somebody, in a room inside Atlas.
    ///
    /// # Why this always succeeds once it knows who you meant
    ///
    /// Everything about the message is decided here and nothing about
    /// delivery is. The person may be asleep, their laptop may be shut, the
    /// link may not exist yet -- none of that is a reason to refuse to write
    /// something down with your name and your clock on it. See `chat.rs`.
    /// "text Sam saying I'm running late": written for your phone to send
    /// (`texting`). `None` when it wasn't asked.
    pub(super) fn text_help(&mut self, said: &str) -> Option<String> {
        let (who, message) = crate::texting::text_asked(said)?;
        Some(self.write_a_text(&who, &message))
    }

    /// A text to `who`, written and offered: on the phone as a notification
    /// that opens Messages, and on the hub's Talk page. Never said to be sent.
    pub(super) fn write_a_text(&mut self, who: &str, message: &str) -> String {
        let people = self.people_known().clone();
        let (name, number) = match people.find(who) {
            crate::people::Found::One(k) => match people.by_key.get(k) {
                Some(c) => match c.phones.first() {
                    Some(n) => (c.name.clone(), n.clone()),
                    None => {
                        return format!("I don't have a number for {}. Tell me \"{}'s number is\" and the number, then ask again.", c.name, c.name)
                    }
                },
                None => return format!("I don't have a number for {who}."),
            },
            crate::people::Found::Several(names) => return format!("Which {who}? {}.", names.join(" or ")),
            crate::people::Found::None => {
                return format!("I don't have a number for {who}. Tell me \"{who}'s number is\" and the number, then ask again.")
            }
        };
        let body = crate::outbox::body_from_spoken(message);
        let now = clock();
        let mut texts = crate::texting::Texts::load(&self.store);
        texts.add(crate::texting::Waiting { to_name: name.clone(), number: number.clone(), body: body.clone(), at: now });
        if let Err(e) = texts.save(&self.store) {
            return format!("I wrote it but couldn't keep it ({e}), so it isn't waiting on your phone.");
        }
        let link = crate::texting::sms_link(&number, &body);
        let note = crate::notify::Note::new(&format!("Text to {name}"), &format!("{body} -- tap to open it in Messages."), crate::notify::Urgency::Routine, now);
        let phone = self.phone_cfg();
        let on_phone = phone.enabled && crate::phone::send_with_click(&note, &phone, &link).is_ok();
        let where_ = if on_phone {
            "It's on your phone -- tap the notification, then Send."
        } else {
            "Open it from Talk on the hub on your phone, then tap Send."
        };
        format!("Text to {name}: \"{body}\" {where_}")
    }

    pub(super) fn send_message(&mut self, raw: &str) -> String {
        let (who, body) = split_who_and_what(raw);
        if who.is_empty() {
            return "Who should I send that to?".into();
        }
        if body.is_empty() {
            return format!("What should I say to {who}?");
        }
        let dir = self.peer_dir.clone();
        let pairings = crate::kin::Pairings::load(&dir);
        let roster = crate::roster::Roster::load(&self.store);

        // "message the Northwind group: ..." targets a group you already named,
        // rather than starting a fresh one. Checked before names, so a group
        // name never gets mistaken for a person to pair with.
        let key = who.trim().strip_prefix("the ").unwrap_or(who.trim());
        let key = key.strip_suffix(" group").unwrap_or(key).trim();
        if let Some(g) = self.chats.group_named(key) {
            let (room, name) = (g.id.clone(), g.name.clone());
            if crate::groups::is_owned_id(&room) {
                let groups = crate::groups::Groups::load(&self.store);
                let role = self.my_key().and_then(|k| groups.held.get(&room).and_then(|h| h.state.speaks_as(&k)).map(|(_, r)| r));
                if !role.is_some_and(|r| r.may_post()) {
                    return format!("You can read \"{name}\" but not post in it -- whoever made it decides that.");
                }
            }
            let now = clock();
            return match self.chats.post(&room, &body, now, local_offset_mins(), &roster, &pairings)
            {
                Ok((msg, left_out)) => {
                    let _ = self.chats.save(&self.store);
                    let mut said = if msg.fully_arrived() {
                        format!("Told the group ({name}).")
                    } else {
                        format!("Written to the group ({name}) — it'll go when they're reachable.")
                    };
                    if !left_out.is_empty() {
                        said.push_str(&format!(
                            " {} isn't in that business any more, so I left them out.",
                            left_out.join(", ")
                        ));
                    }
                    said
                }
                Err(e) => e.plain(),
            };
        }

        // One recipient or several: "Jordan and Maya: ..." is a group. Split on
        // "and" and commas, then keep the ones you've actually paired with.
        let names: Vec<String> = who
            .replace(" and ", ",")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let (known, unknown): (Vec<String>, Vec<String>) =
            names.into_iter().partition(|n| pairings.has_peer(n));
        if known.is_empty() {
            let who = unknown.into_iter().next().unwrap_or(who);
            // Not another Atlas, but someone whose number you've given:
            // a text, which is where most conversations are (30 Sep 2026).
            let people = self.people_known().clone();
            let has_number = match people.find(&who) {
                crate::people::Found::One(k) => people.by_key.get(k).is_some_and(|c| !c.phones.is_empty()),
                _ => false,
            };
            if has_number {
                return self.write_a_text(&who, &body);
            }
            return format!(
                "I don't know anyone called {who}. Say \"add a friend\", or use the Friends page, to pair with them first."
            );
        }

        let now = clock();
        let offset = local_offset_mins();
        // The firewall side: a business only if *every* member shares that one,
        // personal otherwise — a group spanning two businesses has no single
        // side to sit on.
        let space = self.shared_space(&known, &roster, &pairings);

        let (room, label) = if known.len() == 1 {
            let who = &known[0];
            match self.chats.open(who, space, &[who.clone()], &roster, &pairings) {
                Ok(id) => (id, who.clone()),
                Err(e) => return e.plain(),
            }
        } else {
            // Reuse the group you already have with exactly these people, or
            // mint a new shared id. That id travels on every message so every
            // member's Atlas files the conversation as the same one.
            let id = self
                .chats
                .rooms
                .iter()
                .find(|r| r.members.len() > 1 && same_member_set(&r.members, &known))
                .map(|r| r.id.clone())
                .or_else(|| crate::server::new_token().ok());
            let Some(id) = id else {
                return "I couldn't start that group just now.".into();
            };
            let name = known.join(", ");
            match self.chats.open_group(&id, &name, space, &known, &roster, &pairings) {
                Ok(id) => (id, format!("the group ({name})")),
                Err(e) => return e.plain(),
            }
        };

        match self.chats.post(&room, &body, now, offset, &roster, &pairings) {
            Ok((msg, left_out)) => {
                let _ = self.chats.save(&self.store);
                let mut said = if msg.fully_arrived() {
                    format!("Told {label}.")
                } else {
                    // The honest half. It is written and it is yours; it has
                    // not landed, and saying "sent" is the word this module
                    // refuses to use.
                    format!("Written to {label} — it'll go when they're reachable.")
                };
                if !unknown.is_empty() {
                    said.push_str(&format!(
                        " I don't have {}, so they're not in it — pair with them first.",
                        unknown.join(", ")
                    ));
                }
                if !left_out.is_empty() {
                    said.push_str(&format!(
                        " {} isn't in that business any more, so I left them out.",
                        left_out.join(", ")
                    ));
                }
                said
            }
            Err(e) => e.plain(),
        }
    }

    /// The firewall side a message to these people belongs on: a business only
    /// when every one of them has standing in that same one, personal
    /// otherwise. For a single recipient this is just "the business you share,
    /// if it's the only one".
    pub(crate) fn shared_space(
        &self,
        members: &[String],
        roster: &crate::roster::Roster,
        pairings: &crate::kin::Pairings,
    ) -> crate::earned::Space {
        let common: Vec<String> = roster
            .standing(&members[0], pairings)
            .into_iter()
            .filter(|b| members.iter().all(|m| roster.may_see(b, m, pairings)))
            .collect();
        match common.len() {
            1 => crate::earned::Space::Business(common[0].clone()),
            _ => crate::earned::Space::Personal,
        }
    }

    /// "I'm back" — summon the passphrase prompt, and nothing else.
    ///
    /// # What this is and is not
    ///
    /// The phrase is a **summons**. Everything it can do is put a prompt in
    /// front of somebody; the passphrase decides the rest, and it decides it
    /// in `vault::Vault::open` and `handover::take_back`, which are the same
    /// two functions `atlas handover back` goes through. There is no branch
    /// here that ends a handover without them, which is why saying "I'm
    /// back" in a room where somebody else is holding the laptop costs
    /// exactly nothing.
    ///
    /// It is worth having anyway, because the alternative was walking to
    /// wherever the laptop is to type a command whose name you have to
    /// remember. The one step it removes is the one you would otherwise skip.
    ///
    /// # Why the passphrase is not spoken, when everything else here is
    ///
    /// `Intent::Unlock` takes a spoken passphrase, and that is a deliberate
    /// convenience for a room with one person in it. This is the other case
    /// by construction: Atlas is handed over precisely when somebody else is
    /// standing there, so a spoken secret is a secret said in front of the
    /// person it is being kept from.
    ///
    /// # The wrinkle, said plainly
    ///
    /// The prompt blocks, so Atlas cannot say "check the terminal" first and
    /// ask second -- the sentence it speaks is the one that comes back after
    /// you have typed. The prompt therefore has to explain itself on its own,
    /// which is what the wording below is for.
    pub(super) fn take_it_back(&mut self) -> String {
        let state = crate::roots::install_state();
        let h = crate::handover::Handover::load(&state);
        if !h.stance.handed_over() {
            return "This is yours already — nothing's being held back.".into();
        }
        if !self.vault.has_a_passphrase() {
            // The same refusal `handover::take_back` gives, said before the
            // prompt rather than after: there is no point asking someone to
            // type a passphrase that would prove nothing when they did.
            //
            // Pointed at the Accounts page since 27 Sep 2026, which is where a
            // passphrase is set now; this used to name `atlas vault`.
            return format!(
                "{} That has to be done while this is yours, on the hub's Accounts page, \
                 under Vault.",
                crate::handover::NO_PASSPHRASE_YET
            );
        }
        let asked = match self.asks_quietly.as_ref() {
            Some(asker) => asker.ask(
                "Atlas: type the vault passphrase to take this back (it won't be shown): ",
            ),
            // No screen of Atlas's own. Said rather than pretended: the place
            // that has a box for it is named, and the stance is untouched.
            // (Named the terminal command until 27 Sep 2026; the Accounts
            // page takes it now.)
            None => {
                return "I've nowhere to put a prompt you could type into from here. The hub's \
                        Accounts page has a box for it, under Vault — \u{201c}Take it back\u{201d}."
                    .into()
            }
        };
        let Some(phrase) = asked else {
            // Cancelled, or an empty line. Not a wrong passphrase: nothing
            // was offered, so nothing is counted against anyone.
            return "Nothing typed — it's still handed over.".into();
        };
        // Checked by the vault, counted by the handover, saved, and the vault
        // locked again -- one sequence, shared with `atlas handover back` and
        // the Accounts page (`handover::take_back_with`). No lockout: being
        // locked out of your own machine by your own typing is a worse day
        // than being told twice that the passphrase was wrong.
        let cfg = self.tools_cfg().vault.clone();
        crate::handover::take_back_with(&state, &mut self.vault, &phrase, &cfg, clock())
    }

    pub fn receive_signal(&mut self, i: &crate::kin::Incoming) {
        let n = crate::kin::as_nudge(i);
        let offer = crate::proactive::from_nudge(&n);
        self.session.ask(&offer.message);
        self.pending_offer = Some(offer);
    }

    /// A friend's Atlas handed something over.
    ///
    /// The other half of `household::share_with_friend`, which built the
    /// `Handoff` and had no caller until this existed. Note what this does
    /// *not* do: it does not put the note in the tray, does not look at it,
    /// does not park a question, and does not ask you anything. It says one
    /// line and puts it in a list. Everything further is `atlas handoffs
    /// keep`, which is you deciding — and that deliberate act, not the
    /// arrival, is what lets Atlas read it.
    ///
    /// Saying one line rather than nothing is the deliberate part of the
    /// other direction: a note that arrives in total silence is a note you
    /// find in three weeks.
    pub fn receive_handoff(&mut self, d: &crate::kin::Delivered) -> String {
        // An add-on from a paired Atlas goes on the shelf of things offered
        // to you -- not installed, not approved, not run. Taking it is your
        // choice, made seeing what it would be allowed to do.
        if let Some(f) = &d.file {
            match crate::plugins::offered(&self.store, &self.cfg.commands, &d.from, &f.name, &f.bytes, &d.what, d.at) {
                crate::plugins::Offer::NotAnAddOn => {}
                crate::plugins::Offer::Shelved(said) | crate::plugins::Offer::Refused(said) => {
                    if !said.is_empty() {
                        self.log.info(&said);
                    }
                    return said;
                }
            }
        }
        self.file_handoff(d)
    }

    fn file_handoff(&mut self, d: &crate::kin::Delivered) -> String {
        let mut inbox = crate::household::Inbox::load(&self.store);
        let before = inbox.items.len();
        let id = match crate::kin::as_waiting(d, &mut inbox, self.store.root()) {
            Ok(id) => id,
            Err(e) => {
                // A file that could not be written is not filed as though it
                // had been. The list would name something that is not there.
                self.log.warn(&format!("couldn't take a handoff from {}: {e}", d.from));
                return format!("{} sent you something, and I couldn't keep it: {e}", d.from);
            }
        };
        let is_new = inbox.items.len() > before;
        if let Err(e) = inbox.save(&self.store) {
            self.log.warn(&format!("couldn't save a handoff from {}: {e}", d.from));
            return format!("{} sent you something, and I couldn't put it away.", d.from);
        }
        self.journal.record_at(
            crate::activity::Kind::Blocked,
            &format!("{} sent you something — it's waiting on your Documents page (number {id})", d.from),
            true,
            d.at,
        );
        if !is_new {
            // A resend of something already waiting. Recorded once, said
            // once -- repeating it would make a friend's flaky connection
            // into your notification problem.
            return String::new();
        }
        format!("{} sent you something. It's waiting on your Documents page.", d.from)
    }

    /// A chat message from a peer, filed into the conversation for them.
    ///
    /// The room is opened for the sender the *token* named — never a name the
    /// body claimed — on the firewall side the message states. `open` and
    /// `receive` accept a business room only if this machine's own roster
    /// agrees the sender may see that business, so a peer cannot file
    /// themselves into a business they are not in: the deciding check is the
    /// receiver's roster, right here, not anything that travelled on the wire.
    /// Held quietly rather than spoken — a message is not an interruption; it
    /// waits in the conversation until you look, the way the design's pop-up
    /// rules ask.
    pub fn receive_chat(&mut self, c: &crate::kin::Chatted) {
        use crate::chat::{Delivery, Message, ME};
        // A group with an owner: only its signed list says who may post.
        if let Some(gid) = c.group_id.as_deref().filter(|g| crate::groups::is_owned_id(g)) {
            let mut groups = crate::groups::Groups::load(&self.store);
            let Some(held) = groups.held.get(gid).cloned() else {
                // The message beat the list here. Held, not dropped and not
                // filed on the sender's say-so; filed when the list arrives.
                groups.hold(crate::groups::Waiting {
                    group_id: gid.to_string(),
                    from: c.from.clone(),
                    body: c.body.clone(),
                    sent_at: c.sent_at,
                    sent_offset_mins: c.sent_offset_mins,
                    after: c.after,
                    id: c.id.clone(),
                    held_at: clock(),
                    on_behalf_of: c.on_behalf_of.clone(),
                });
                let _ = groups.save(&self.store);
                return;
            };
            let pairings = crate::kin::Pairings::load(&self.peer_dir);
            let sender_key = pairings.key_of(&c.from).map(String::from);
            // Who wrote it. The peer who handed it over, unless that peer is
            // the group's owner passing on a member's message -- the owner's
            // word on who said what is the same trust as its list.
            // A friend request rides a group only to reach one person; it is
            // never filed as a message (`friends`).
            if crate::friends::read_request(&c.body).is_some() {
                let author = match (&c.on_behalf_of, &sender_key) {
                    (Some(k), Some(s)) if *s == held.state.owner => Some(k.clone()),
                    _ => sender_key.clone(),
                };
                self.friend_request_in_group(gid, &held.state, author.as_deref(), sender_key.as_deref(), &c.body);
                return;
            }
            let author_key = match (&c.on_behalf_of, &sender_key) {
                (Some(k), Some(s)) if *s == held.state.owner => Some(k.clone()),
                _ => sender_key.clone(),
            };
            // Your own phone, posting as you: it speaks as the owner.
            let (author_key, role) = match author_key.as_deref().and_then(|k| held.state.speaks_as(k)) {
                Some((k, r)) => (Some(k), Some(r)),
                None => (author_key, None),
            };
            if !role.is_some_and(|r| r.may_post()) {
                self.log.warn(&format!(
                    "declined a message in \"{}\" from {}: {}",
                    held.state.name,
                    c.from,
                    match role {
                        Some(_) => "they can read that group but not post in it",
                        None => "they aren't in that group",
                    }
                ));
                return;
            }
            let author_key = author_key.unwrap_or_default();
            let me_now = self.my_key();
            let author = if me_now.as_deref() == Some(author_key.as_str()) {
                // Written by you on another of your devices.
                ME.to_string()
            } else {
                pairings.name_of_key(&author_key).unwrap_or_else(|| {
                    held.state.seat(&author_key).map(|s| s.name.clone()).unwrap_or_else(|| c.from.clone())
                })
            };
            self.settle_owned_groups();
            let now = clock();
            let msg = Message {
                id: c.id.clone(),
                from: author.clone(),
                body: c.body.clone(),
                sent_at: c.sent_at,
                sent_offset_mins: c.sent_offset_mins,
                after: c.after,
                to: vec![(ME.to_string(), Delivery::Arrived(now))],
            };
            match self.chats.receive_vouched(gid, msg) {
                Ok(fresh) => {
                    let _ = self.chats.save(&self.store);
                    // As the owner: pass a member's message on to everyone
                    // else in the group, so members who aren't paired with
                    // each other still hear each other.
                    if fresh {
                        if let Some(me) = self.my_key() {
                            let handed_by = sender_key.clone().unwrap_or_default();
                            if held.state.owner == me && handed_by != me {
                                groups.relay(
                                    &me,
                                    gid,
                                    &author_key,
                                    &handed_by,
                                    &crate::groups::Waiting {
                                        group_id: gid.to_string(),
                                        from: author.clone(),
                                        body: c.body.clone(),
                                        sent_at: c.sent_at,
                                        sent_offset_mins: c.sent_offset_mins,
                                        after: c.after,
                                        id: c.id.clone(),
                                        held_at: now,
                                        on_behalf_of: None,
                                    },
                                );
                                let _ = groups.save(&self.store);
                            }
                        }
                    }
                }
                Err(e) => self.log.warn(&format!("declined a message from {}: {}", c.from, e.plain())),
            }
            // A signed update in a release channel is the courier's to read.
            if held.state.release_channel && role == Some(crate::groups::Role::Owner) {
                if let Some(said) = crate::update_courier::heard(&self.store, &c.body, &held.state.owner, now) {
                    self.log.info(&said);
                    self.journal.record_at(crate::activity::Kind::Blocked, &said, true, now);
                }
            }
            return;
        }
        let space = match &c.business {
            Some(b) => crate::earned::Space::Business(b.clone()),
            None => crate::earned::Space::Personal,
        };
        let roster = crate::roster::Roster::load(&self.store);
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let room = if let Some(gid) = &c.group_id {
            // The members I can actually reach: the sender (named by the
            // token), plus any member named on the wire that I am myself
            // paired with. Everyone else is left off — including the entry
            // that is me seen through the sender's eyes, which never matches
            // a pairing because you are not paired with yourself.
            let mut members = vec![c.from.clone()];
            for m in &c.members {
                if pairings.has_peer(m) && !members.iter().any(|x| crate::kin::same_name(x, m)) {
                    members.push(m.clone());
                }
            }
            let name = c.group_name.clone().unwrap_or_else(|| c.from.clone());
            match self.chats.open_group(gid, &name, space, &members, &roster, &pairings) {
                Ok(id) => id,
                Err(e) => {
                    self.log
                        .warn(&format!("declined a group message from {}: {}", c.from, e.plain()));
                    return;
                }
            }
        } else {
            match self.chats.open(&c.from, space, &[c.from.clone()], &roster, &pairings) {
                Ok(id) => id,
                Err(e) => {
                    self.log
                        .warn(&format!("declined a message from {}: {}", c.from, e.plain()));
                    return;
                }
            }
        };
        let now = clock();
        let msg = Message {
            id: c.id.clone(),
            from: c.from.clone(),
            body: c.body.clone(),
            sent_at: c.sent_at,
            sent_offset_mins: c.sent_offset_mins,
            after: c.after,
            to: vec![(ME.to_string(), Delivery::Arrived(now))],
        };
        match self.chats.receive(&room, msg, &roster, &pairings) {
            Ok(_) => {
                if let Err(e) = self.chats.save(&self.store) {
                    self.log
                        .warn(&format!("couldn't save a message from {}: {e}", c.from));
                }
            }
            Err(e) => {
                self.log
                    .warn(&format!("declined a message from {}: {}", c.from, e.plain()));
            }
        }
    }

    /// A paired Atlas introduced its key: pin it to that pairing, once.
    pub fn receive_hello(&mut self, h: &crate::kin::Hello) -> String {
        self.pin_hello(h);
        String::new()
    }

    fn pin_hello(&mut self, h: &crate::kin::Hello) {
        let mut pairings = crate::kin::Pairings::load(&self.peer_dir);
        match pairings.pin_key(&h.from, &h.key) {
            Ok(true) => {
                if let Err(e) = pairings.save(&self.peer_dir) {
                    self.log.warn(&format!("couldn't save {}'s key: {e}", h.from));
                }
                // Anyone waiting on this key to be seated in a group can be now.
                self.settle_owned_groups();
            }
            Ok(false) => {}
            Err(why) => {
                self.log.warn(&why);
                self.journal.record_at(crate::activity::Kind::Blocked, &why, false, h.at);
            }
        }
    }

    /// A group's signed list arrived. Taken only if the owner's signature
    /// holds and it's newer; then the group on this end is made to match it.
    /// Returns anything worth saying.
    pub fn receive_group(&mut self, g: &crate::kin::GroupList) -> String {
        let mut groups = crate::groups::Groups::load(&self.store);
        let taken = match groups.take(g.signed.clone()) {
            Ok(t) => t,
            Err(why) => {
                self.log.warn(&format!("refused a group list {} handed over: {why}", g.from));
                return String::new();
            }
        };
        if taken == crate::groups::Taken::NotNewer {
            return String::new();
        }
        let Some(held) = crate::groups::open(&g.signed).ok() else { return String::new() };
        let gid = held.group_id.clone();
        let waiting = groups.release_waiting(&gid);
        if let Err(e) = groups.save(&self.store) {
            self.log.warn(&format!("couldn't save a group list: {e}"));
            return String::new();
        }
        let me = self.my_key();
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let owner = pairings.name_of_key(&held.owner).unwrap_or_else(|| g.from.clone());
        let was_in = self.chats.room(&gid).is_some();
        let role = me.as_deref().and_then(|k| held.speaks_as(k)).map(|(_, r)| r);
        // One of your own devices: it's your group, nobody "added" you.
        let is_my_device = me.as_deref().is_some_and(|k| held.is_delegate(k));
        self.settle_owned_groups();
        for w in waiting {
            self.receive_chat(&crate::kin::Chatted {
                from: w.from,
                business: None,
                body: w.body,
                sent_at: w.sent_at,
                sent_offset_mins: w.sent_offset_mins,
                after: w.after,
                id: w.id,
                group_id: Some(gid.clone()),
                group_name: Some(held.name.clone()),
                members: Vec::new(),
                on_behalf_of: w.on_behalf_of,
            });
        }
        match (was_in, role) {
            _ if is_my_device => String::new(),
            (false, Some(r)) => format!("{owner} added you to \"{}\" as a {}.", held.name, r.plain()),
            (true, None) => format!("{owner} took you out of \"{}\".", held.name),
            _ => String::new(),
        }
    }

    /// Why this would be refused however you answered, if it would -- so the
    /// refusal comes first and the question never does.
    pub(super) fn refused_before_asking(&self, intent: &Intent) -> Option<String> {
        let Intent::Message(raw) = intent else { return None };
        let (who, _) = split_who_and_what(raw);
        let key = who.trim().strip_prefix("the ").unwrap_or(who.trim());
        let key = key.strip_suffix(" group").unwrap_or(key).trim();
        let room = self.chats.group_named(key)?;
        if !crate::groups::is_owned_id(&room.id) {
            return None;
        }
        let groups = crate::groups::Groups::load(&self.store);
        let role = self.my_key().and_then(|k| groups.held.get(&room.id).and_then(|h| h.state.speaks_as(&k)).map(|(_, r)| r));
        (!role.is_some_and(|r| r.may_post()))
            .then(|| format!("You can read \"{}\" but not post in it -- whoever made it decides that.", room.name))
    }

    // ---- Friends (`friends`) ----------------------------------------------

    /// Where friend links, requests and the friends still being reached are
    /// kept: beside the pairings, because a friend is a pairing -- the
    /// install's, not one profile's (`kin::where_pairings_live`).
    pub(super) fn friend_store(&self) -> Store {
        Store::new(self.peer_dir.clone())
    }

    /// Open the door friends knock on, if it isn't already, and let links
    /// made here be used on it.
    fn open_friend_door(&mut self) -> std::result::Result<(), String> {
        if self.signal_listener.is_none() {
            let kin = self.tools_cfg().kin.clone();
            let port = if kin.port != 0 { kin.port } else { crate::kin::DEFAULT_PORT };
            let peers = crate::kin::Pairings::load(&self.peer_dir).peers;
            match crate::server::SignalListener::bind(port, peers) {
                Ok(l) => {
                    self.fit_door(&l);
                    self.signal_listener = Some(l);
                }
                // Taken already: the Atlas that's running holds it (this is
                // the command line), and links made here are kept where that
                // door reads them. Anything else that isn't Atlas answering
                // there would fail at the friend's end, not silently here.
                Err(_) if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() => return Ok(()),
                Err(e) => return Err(format!("I couldn't open the door friends knock on: {e}")),
            }
        }
        Ok(())
    }

    /// What every door this Atlas opens can do beyond the pairings it was
    /// opened with: take friend links made here, whenever they were made;
    /// open envelopes sealed to this Atlas's key; hold sealed mail for
    /// friends who ask; and hand out release files kept here to people it's
    /// paired with.
    fn fit_door(&self, l: &crate::server::SignalListener) {
        l.accept_friends(self.friend_store());
        l.serve_releases(self.store.root().to_path_buf());
        l.note_reached(self.reached.clone());
        if let Ok(me) = crate::peerkey::Identity::load_or_create(&self.peer_dir) {
            l.serve_sealed(me);
        }
    }

    /// The link to every paired Atlas, sealing with this one's key, and
    /// reaching friends through Tor once it's ready.
    pub(crate) fn peer_link(&self, pairings: &crate::kin::Pairings) -> crate::kin::PeerLink {
        crate::kin::PeerLink::from_state(pairings, &self.chats)
            .sealing_as(crate::peerkey::Identity::load_or_create(&self.peer_dir).ok())
            .through_tor(self.tor_socks())
            .keeping(self.tor_connections.clone())
            .noting(self.reached.clone())
    }

    /// Tor's SOCKS port, when Atlas's own Tor is connected.
    fn tor_socks(&self) -> Option<u16> {
        self.tor.as_ref().filter(|t| t.progress() >= 100).map(|t| t.socks)
    }

    /// Start Atlas's own Tor, if it isn't running: this Atlas's onion service
    /// pointed at the door's sealed port, and a way out to friends' onion
    /// addresses. Nothing for anyone to set up -- the `tor` program ships
    /// beside Atlas (`onion::find_tor`). Returns why not, if it can't.
    pub fn start_tor(&mut self) -> std::result::Result<(), String> {
        if let Some(t) = self.tor.as_mut() {
            if !t.stopped() {
                return Ok(());
            }
            self.tor = None;
        }
        let Some(door) = self.signal_listener.as_ref().map(|l| l.sealed_port()).filter(|p| *p != 0) else {
            return Err("the door friends reach isn't open".into());
        };
        let kin = self.tools_cfg().kin.clone();
        let (binary, extra) = match &self.tor_instead {
            Some(None) => return Err("Tor is switched off here.".into()),
            Some(Some((b, e))) => (Some(b.clone()), e.clone()),
            None => (crate::onion::find_tor(kin.tor.as_deref()), kin.tor_extra.clone()),
        };
        let Some(binary) = binary else {
            return Err("Tor isn't installed beside Atlas, so friends outside your home network can't reach it. \
                        Opening Atlas's setup again fetches it."
                .into());
        };
        let me = crate::peerkey::Identity::load_or_create(&self.peer_dir).map_err(|e| format!("I couldn't read this Atlas's key: {e}"))?;
        let dir = self.peer_dir.join("tor");
        // A network that blocked Tor before: straight back to the kind of
        // bridge that got through (`tor_bridges`). Only when you haven't set
        // Tor's lines yourself (`kin.tor_extra`) -- yours are never overridden.
        let remembered: String = self.store.load(TOR_BRIDGES);
        // A new Tor: connections through the old one died with it.
        self.tor_connections.close_all();
        self.tor = Some(match remembered.as_str() {
            k if !k.is_empty() && extra.is_empty() => crate::onion::Tor::start_bridged(&binary, &dir, &me, door, k, &extra)
                .or_else(|_| crate::onion::Tor::start(&binary, &dir, &me, door, &extra))?,
            _ => crate::onion::Tor::start(&binary, &dir, &me, door, &extra)?,
        });
        self.tor_binary = Some((binary, extra.is_empty()));
        Ok(())
    }

    /// "Any updates?", "install the update", "go back to the last version"
    /// (OPEN_GAPS 8.2) -- the same steps as `atlas update`, `atlas update
    /// install` and `atlas update undo`, said rather than typed.
    pub(crate) fn updates_said(&mut self, what: &str) -> String {
        let root = self.store.install_root();
        match what {
            "install" => {
                let a = crate::update_courier::Available::load(&self.store);
                let Some(platform) = crate::release::this_platform() else {
                    return "Atlas doesn't ship updates for this kind of device.".into();
                };
                if a.notice.is_none() {
                    return "There's no update waiting.".into();
                }
                if a.downloaded.is_empty() {
                    return format!("Atlas {} is out, but it hasn't finished arriving yet. I'll install it once it's here and checked.", a.version);
                }
                crate::update_apply::say_yes(&self.store, &a.version);
                match crate::update_apply::stage_update(&self.store, &root, platform) {
                    Ok(v) => format!(
                        "Atlas {v} is checked against your release key. I'll restart into it in a moment; it has to pass its \
                         health check first, and the version you're on now is kept."
                    ),
                    Err(why) => why,
                }
            }
            "undo" => {
                let Some((previous, _)) = crate::update_apply::previous_build(&root) else {
                    return "There's no previous version kept here to go back to.".into();
                };
                let now_on = crate::upgrade::version();
                // The question and how to answer it lead: a spoken reply is
                // cut to a few sentences, and this is the part that matters.
                let q = format!("Say yes to go back from Atlas {now_on} to {previous} -- {now_on} won't be offered again.");
                self.session.await_approval(Intent::Updates("undo-confirmed".into()), &q);
                q
            }
            // Only the yes to the question above builds this.
            "undo-confirmed" => {
                let Ok(running) = std::env::current_exe() else {
                    return "I couldn't tell where this program is, so nothing changed.".into();
                };
                let yes = crate::release::LocalApproval::given_by_the_person_at_this_device();
                match crate::update_apply::undo_update(&self.store, &root, &running, yes) {
                    Ok(v) => format!("Going back to Atlas {v}. I'll restart onto it in a moment."),
                    Err(why) => why,
                }
            }
            _ => {
                let mut out = vec![format!("This is Atlas {}.", crate::upgrade::version())];
                let heard = crate::update_courier::status(&self.store, crate::store::now());
                let a = crate::update_courier::Available::load(&self.store);
                out.push(heard);
                if let Some(p) = crate::update_apply::pending(&self.store) {
                    out.push(format!("Atlas {} is checked and goes in at the next start.", p.version));
                } else if a.notice.is_some() && !a.downloaded.is_empty() {
                    out.push("Say \"install the update\" to put it in now.".into());
                }
                if let Some(t) = crate::upgrade::current_trial(&root) {
                    out.push(format!("{} is on trial after replacing {}.", t.new, t.previous));
                }
                if crate::update_apply::previous_build(&root).is_some() {
                    out.push("The version before is kept; say \"go back to the last version\" to return to it.".into());
                }
                out.join(" ")
            }
        }
    }

    /// Feedback, by voice (OPEN_GAPS 8.14): "report a bug, the brief is
    /// wrong" reads back exactly what will be sent -- with what was written
    /// down about a failed update, if there is one -- and sends only on your
    /// yes. On the releaser's side, "any feedback" and "answer feedback 2
    /// fixing".
    pub(super) fn feedback_said(&mut self, what: &str) -> String {
        let now = crate::store::now();
        if what == "list" {
            return crate::feedback::spoken_list(&self.store);
        }
        if let Some(rest) = what.strip_prefix("reply:") {
            let Some((n, status, note)) = crate::feedback::read_reply(rest) else {
                return "Say \"answer feedback\", its number, and seen, fixing, fixed and the version, or wont -- for example \
                        \"answer feedback 2 fixing\"."
                    .into();
            };
            return match crate::feedback::answer_feedback(&self.store, n, status.clone(), &note, now) {
                Ok(to) => format!("Marked {}; {to} will hear it the next time I reach them.", status.plain()),
                Err(why) => why,
            };
        }
        if what == "confirmed-send" {
            let Some(f) = self.feedback_draft.take() else { return "There's nothing waiting to send.".into() };
            return match crate::feedback::send_decided(&self.store, &self.peer_dir, f) {
                Ok(s) => s.plain(),
                Err(why) => why,
            };
        }
        let (attach_failure, words) = match what.split_once(':') {
            Some(("send", w)) => (true, w.trim()),
            Some(("bare", w)) => (false, w.trim()),
            _ => (false, ""),
        };
        let Some((_, name, mine)) = crate::feedback::release_sender(&self.store, &self.peer_dir) else {
            return "This Atlas isn't in anyone's release channel, so there's no one to send feedback to.".into();
        };
        if words.is_empty() {
            return "Say it all in one go: \"report a bug\" and then what's wrong, in your own words.".into();
        }
        let failure = crate::update_apply::last_failure(&self.store);
        let attach = if attach_failure { failure.clone() } else { None };
        let f = match crate::feedback::compose_feedback(words, attach, now) {
            Ok(f) => f,
            Err(why) => return why,
        };
        let to = if mine { "your own feedback list".to_string() } else { name.clone().unwrap_or_else(|| "whoever sends you Atlas".into()) };
        // The question leads (a spoken reply is cut to a few sentences); then
        // exactly what goes.
        let mut q = format!("Send this to {to}? Say yes to send it.");
        if f.attached.is_some() {
            q.push_str(" It includes what was written down about the update that failed here, with your name and home folder taken out; to send just your words, say no, then \"report a bug without the failure\" and what's wrong.");
        }
        q.push_str(&format!(" This is exactly what will go: {}", crate::feedback::feedback_preview(&f)));
        self.feedback_draft = Some(f);
        self.session.await_approval(Intent::Feedback("confirmed-send".into()), &q);
        q
    }

    /// The phone's own language model (P.7): "get your own model" downloads
    /// the one that suits this phone's memory and loads it when it's here;
    /// "how's the model download" says where it's got to. On a computer it
    /// says what it is and where the model is here instead.
    pub(crate) fn phone_model_said(&mut self, what: &str) -> String {
        #[cfg(feature = "phone-llm")]
        {
            let attached = crate::phonemodel::attached();
            if what == "get" && attached.is_none() {
                let dir = crate::models::Registry::dir_for(&self.tools_cfg().models);
                if let Some((path, m)) = crate::phonemodel::present(&dir) {
                    std::thread::spawn(move || {
                        let _ = crate::phonemodel::attach(&path);
                    });
                    return format!("{} is already on this phone; loading it now.", m.name);
                }
                return crate::phonemodel::start_download(dir, |path| {
                    let _ = crate::phonemodel::attach(&path);
                });
            }
            return crate::phonemodel::download_said(crate::phonemodel::download_state().as_ref(), attached.as_deref());
        }
        #[allow(unreachable_code)]
        {
            let _ = what;
            "That's for Atlas on a phone, which runs a small model inside the app. Here, Atlas uses the model in its \
             models folder -- say \"which model\" to hear which."
                .into()
        }
    }

    /// A network that blocks Tor (gap AM): when Tor has sat without getting
    /// further, start it again through the next kind of bridge Tor ships with
    /// (obfs4, then Snowflake, then meek), and remember the one that gets
    /// through. If none does, say so once and go back to connecting directly,
    /// trying the round again later. Returns what to tell you, if anything.
    fn keep_tor_getting_through(&mut self, t: u64) -> Option<String> {
        const ROUND_AGAIN_SECS: u64 = 30 * 60;
        let (binary, ours) = self.tor_binary.clone()?;
        if !ours {
            return None; // your own Tor lines: left alone.
        }
        let tor = self.tor.as_mut()?;
        if tor.progress() >= 100 {
            // Through: remember how, so the next start goes straight there.
            let how = tor.bridges.clone().unwrap_or_default();
            let known: String = self.store.load(TOR_BRIDGES);
            if known != how {
                let _ = self.store.save(TOR_BRIDGES, &how);
                if !how.is_empty() {
                    return Some(format!(
                        "Tor got through using {how} bridges -- this network blocks Tor, and friends can reach you again."
                    ));
                }
            }
            return None;
        }
        if !tor.stalled(t) {
            return None;
        }
        // A whole round failed a little while ago: stay direct until it's
        // time for the next round, rather than cycling every two minutes.
        if self.peer_tries.get("tor:round").is_some_and(|at| t.saturating_sub(*at) < ROUND_AGAIN_SECS) {
            return None;
        }
        let door = self.signal_listener.as_ref().map(|l| l.sealed_port()).filter(|p| *p != 0)?;
        let me = crate::peerkey::Identity::load_or_create(&self.peer_dir).ok()?;
        let dir = self.peer_dir.join("tor");
        let mut kind = crate::onion::next_bridge_kind(tor.bridges.as_deref());
        // Skip kinds this copy of Tor doesn't have.
        while let Some(k) = kind {
            if crate::onion::bridge_lines(&binary, k).is_some() {
                break;
            }
            kind = crate::onion::next_bridge_kind(Some(k));
        }
        let was_direct = tor.bridges.is_none();
        match kind {
            Some(k) => {
                self.tor = None; // stops the stuck one first: one Tor, one data folder.
                self.tor_connections.close_all();
                self.tor = crate::onion::Tor::start_bridged(&binary, &dir, &me, door, k, &[]).ok();
                self.log.warn(&format!("Tor was stuck connecting; trying {k} bridges"));
                was_direct.then(|| {
                    "Tor can't connect on this network -- it looks like it's blocked here. I'm trying Tor's bridges, \
                     which disguise the connection; friends outside your home network can't reach you until one works."
                        .to_string()
                })
            }
            None => {
                // Every kind tried. Back to direct, and the round again later.
                self.peer_tries.insert("tor:round".to_string(), t);
                self.tor = None;
                self.tor_connections.close_all();
                let _ = self.store.save(TOR_BRIDGES, &String::new());
                self.tor = crate::onion::Tor::start(&binary, &dir, &me, door, &[]).ok();
                Some(
                    "Tor couldn't get through on this network, even through its bridges. Friends on your home network \
                     still reach you; I'll try again in half an hour, and straight away on another network."
                        .to_string(),
                )
            }
        }
    }

    /// Where friends reach this Atlas, in a sentence, for the Friends page.
    fn reach_said(&self) -> String {
        match self.tor.as_ref() {
            None => "Tor isn't running, so only friends on your home network can reach your Atlas right now.".into(),
            Some(t) => match t.progress() {
                100 if t.bridges.is_some() => format!(
                    "Friends reach your Atlas from anywhere through Tor, using {} bridges because this network blocks Tor.",
                    t.bridges.as_deref().unwrap_or_default()
                ),
                100 => "Friends reach your Atlas from anywhere through Tor -- no server, nothing in the middle that can read or log who you talk to.".into(),
                p => format!("Tor is connecting ({p}%). Friends can reach your Atlas once it's done."),
            },
        }
    }

    /// Who you are to a friend's Atlas, and every way it reaches you.
    fn friend_me(&mut self) -> std::result::Result<crate::friends::Me, String> {
        let kin = self.tools_cfg().kin.clone();
        let key = crate::peerkey::Identity::load_or_create(&self.peer_dir)
            .map_err(|e| format!("I couldn't read this Atlas's key: {e}"))?
            .public();
        let me = crate::peerkey::Identity::load_or_create(&self.peer_dir).map_err(|e| format!("I couldn't read this Atlas's key: {e}"))?;
        let port = self.signal_listener.as_ref().map(|l| l.port()).unwrap_or(0);
        let mut addrs = Vec::new();
        match &self.friend_host {
            Some(h) => addrs.push(format!("{h}:{port}")),
            None => {
                if let Some(lan) = crate::onion::lan_v4() {
                    addrs.push(format!("{lan}:{port}"));
                }
            }
        }
        // The onion address goes in whenever Tor is running here -- it's
        // made from this Atlas's key, so it's right even before Tor finishes
        // connecting.
        let tor_up = self.start_tor();
        let onion = if tor_up.is_ok() || self.tor.is_some() { crate::onion::my_address(&me) } else { String::new() };
        if onion.is_empty() && addrs.is_empty() {
            return Err(tor_up.err().unwrap_or_else(|| "I can't find any way for another Atlas to reach this one.".into()));
        }
        Ok(crate::friends::Me { name: crate::friends::my_name(kin.my_name.as_deref()), key, routes: crate::kin::Routes { addrs, onion } })
    }

    /// A one-time friend link to send someone.
    pub fn friend_link(&mut self) -> std::result::Result<String, String> {
        self.open_friend_door()?;
        let me = self.friend_me()?;
        crate::friends::make_link(&self.friend_store(), &me, clock())
    }

    /// Let a friend in on the door that's open now, not only at next start.
    fn admit_friend(&self, pairings: &crate::kin::Pairings, name: &str) {
        if let (Some(l), Some(p)) = (&self.signal_listener, pairings.peers.iter().find(|p| crate::kin::same_name(&p.name, name))) {
            l.admit_peer(p.clone());
        }
    }

    /// Take a friendship back that didn't complete: both halves, and the door.
    pub(super) fn unfriend_half(&self, name: &str) {
        let mut pairings = crate::kin::Pairings::load(&self.peer_dir);
        if pairings.forget(name) {
            let _ = pairings.save(&self.peer_dir);
        }
        if let Some(l) = &self.signal_listener {
            l.forget_peer(name);
        }
    }

    /// Add a friend from their link: one step, no code to send back. Returns
    /// what to tell you.
    ///
    /// Waits for the knock (up to ten seconds): said out loud, the answer is
    /// what you're waiting for. The hub's button knocks on the crew instead
    /// (`hublive`, `HubAfter::FriendAdd`), so the page and the rest of Atlas
    /// don't wait with it.
    pub fn add_friend(&mut self, text: &str) -> String {
        match self.prepare_friend(text) {
            Err(said) => said,
            Ok(k) => {
                let outcome = crate::friends::knock(&k.identity, &k.keep.link, &k.keep.hello, k.socks, std::time::Duration::from_secs(10));
                self.after_friend_knock(&k.keep, &outcome)
            }
        }
    }

    /// Everything about adding a friend except the knock: the link read, the
    /// door open, them recorded, Tor started. What the knock needs, or what
    /// to say instead.
    pub(crate) fn prepare_friend(&mut self, text: &str) -> std::result::Result<FriendKnock, String> {
        use crate::friends::{record, Hello, Link, Pending, LINK_DAYS};
        let link = match Link::decode(text) {
            Ok(l) => l,
            Err(e) => {
                let mut c = e.chars();
                return Err(match c.next() {
                    Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str()),
                    None => "That isn't a friend link.".into(),
                });
            }
        };
        if self.my_key().as_deref() == Some(link.key.as_str()) {
            return Err("That's your own friend link -- send it to the person you want to add.".into());
        }
        self.open_friend_door()?;
        let me = self.friend_me()?;
        let Ok(identity) = crate::peerkey::Identity::load_or_create(&self.peer_dir) else {
            return Err("I couldn't read this Atlas's key.".into());
        };
        let token = crate::server::new_token().map_err(|e| format!("I couldn't make a secure pairing: {e}"))?;
        let mut pairings = crate::kin::Pairings::load(&self.peer_dir);
        let name = record(&mut pairings, &link.name, &link.routes, &link.key, &token);
        if let Err(e) = pairings.save(&self.peer_dir) {
            return Err(format!("I couldn't save {name} as a friend: {e}"));
        }
        self.admit_friend(&pairings, &name);
        let hello = Hello { invite: link.invite.clone(), name: me.name, key: me.key, routes: me.routes, token };
        let keep = Pending { name, link, hello, until: clock() + LINK_DAYS * 86_400 };
        let _ = self.start_tor();
        Ok(FriendKnock { identity, keep, socks: self.tor_socks() })
    }

    /// What a knock on a new friend's Atlas came to, and what's kept because
    /// of it: nothing more when they took it, them dropped again when they
    /// refused, and a week of trying again when nothing answered.
    pub(super) fn after_friend_knock(&mut self, keep: &crate::friends::Pending, outcome: &crate::friends::Knock) -> String {
        use crate::friends::{Knock, Outbox};
        let said = knock_said(keep, outcome);
        match outcome {
            Knock::Taken => said,
            Knock::Refused => {
                self.unfriend_half(&keep.name);
                said
            }
            Knock::Unreachable(_) => {
                let store = self.friend_store();
                let mut out = Outbox::load(&store);
                out.add(keep.clone());
                match out.save(&store) {
                    Ok(()) => said,
                    Err(e) => {
                        self.unfriend_half(&keep.name);
                        format!(
                            "I couldn't reach {}'s Atlas, and I couldn't save it to try again ({e}). Try the link again later.",
                            keep.name
                        )
                    }
                }
            }
        }
    }

    /// Someone used one of your friend links: its secret is already spent at
    /// the door. Record them both ways and let them in. Returns what to say.
    pub fn receive_friend(&mut self, b: &crate::kin::Befriended) -> String {
        let h = &b.hello;
        let mut pairings = crate::kin::Pairings::load(&self.peer_dir);
        let name = crate::friends::record(&mut pairings, &h.name, &h.routes, &h.key, &h.token);
        if let Err(e) = pairings.save(&self.peer_dir) {
            let said = format!("{name} used your friend link, but I couldn't save them ({e}). Send them a new link.");
            self.log.warn(&said);
            return said;
        }
        self.admit_friend(&pairings, &name);
        // Anyone they share a group with can be seated now their key is known.
        self.settle_owned_groups();
        let said = format!("{name} is your friend now -- they used the link you sent.");
        self.journal.record_at(crate::activity::Kind::Upkeep, &said, true, b.at);
        said
    }

    /// Friends you added whose Atlas wasn't reachable, and friend requests
    /// you sent through a group: tried again every few minutes. Returns what
    /// to tell you about any that finished.
    fn friend_upkeep(&mut self, t: u64) -> Vec<String> {
        use crate::friends::{knock, Outbox};
        const RETRY_SECS: u64 = 300;
        let store = self.friend_store();
        let mut out = Outbox::load(&store);
        if out.pending.is_empty() && out.requests.is_empty() {
            return Vec::new();
        }
        let Ok(identity) = crate::peerkey::Identity::load_or_create(&self.peer_dir).map(std::sync::Arc::new) else { return Vec::new() };
        let mut said = Vec::new();
        let mut changed = false;
        for p in out.pending.clone() {
            let k = format!("friend:{}", p.hello.invite);
            if t > p.until {
                out.pending.retain(|x| x.hello.invite != p.hello.invite);
                self.unfriend_half(&p.name);
                said.push(format!(
                    "I couldn't reach {}'s Atlas all week, so their friend link has run out. Ask them for a new one.",
                    p.name
                ));
                changed = true;
                continue;
            }
            if self.peer_tries.get(&k).is_some_and(|at| t.saturating_sub(*at) < RETRY_SECS) {
                continue;
            }
            self.peer_tries.insert(k, t);
            // Knocked on the crew, not here: up to ten seconds a friend,
            // every five minutes, used to hold the whole tick (27 Sep 2026).
            // Its answer is taken in `take_crew_news` (`HubAfter::FriendRetry`).
            let (me, link, hello, socks) = (identity.clone(), p.link.clone(), p.hello.clone(), self.tor_socks());
            let work: crew::Work = Box::new(move |_| {
                Ok(knock_tag(&knock(&me, &link, &hello, socks, std::time::Duration::from_secs(10))).to_string())
            });
            if let Some(id) = self.hand_off_as("friend-knock", t, work, None, SpeakPolicy::ViaWatcher) {
                self.hub_after.insert(id, HubAfter::FriendRetry { keep: p.clone() });
            }
        }
        // Requests sent through a group, owed to its owner.
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let link = self.peer_link(&pairings);
        let me = self.my_key().unwrap_or_default();
        for r in out.requests.clone() {
            let k = format!("friendreq:{}", r.via);
            if self.peer_tries.get(&k).is_some_and(|at| t.saturating_sub(*at) < RETRY_SECS) {
                continue;
            }
            if link.relay(&r.via, &r.group_id, &r.group_name, &me, &r.id, &r.body, r.at, 0, 0) {
                out.requests.retain(|x| x.id != r.id);
                changed = true;
            } else {
                self.peer_tries.insert(k, t);
            }
        }
        if changed {
            if let Err(e) = out.save(&store) {
                self.log.warn(&format!("couldn't save the friends still being reached: {e}"));
            }
        }
        said
    }

    /// Send someone you're in a group with a friend request. It goes to them
    /// alone, through the group's owner (who everyone in it is paired with),
    /// and carries a one-time friend link -- accepting it is adding you.
    pub fn send_friend_request(&mut self, who: &str) -> String {
        let who = who.trim().trim_start_matches("to ").trim();
        if who.is_empty() {
            return "Who should I send a friend request to?".into();
        }
        let Some(me) = self.my_key() else {
            return "I couldn't read this Atlas's key.".into();
        };
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        if pairings.has_peer(who) {
            return format!("You and {who} are already friends.");
        }
        let groups = crate::groups::Groups::load(&self.store);
        // The first group you share with someone of that name.
        let found = groups.held.values().find_map(|h| {
            h.state.speaks_as(&me)?;
            let seat = h.state.seats.iter().find(|s| {
                s.key != me
                    && (s.name.eq_ignore_ascii_case(who)
                        || pairings.name_of_key(&s.key).is_some_and(|n| n.eq_ignore_ascii_case(who)))
            })?;
            Some((h.state.clone(), seat.key.clone(), seat.name.clone()))
        });
        let Some((state, target, their_name)) = found else {
            return format!(
                "I can't find {who} in any group you're in. To add someone you don't share a group with, \
                 say \"add a friend\" and send them the link."
            );
        };
        if pairings.name_of_key(&target).is_some() {
            return format!("You and {their_name} are already friends.");
        }
        let link = match self.friend_link() {
            Ok(l) => l,
            Err(e) => return e,
        };
        let body = crate::friends::request_body(&target, &link);
        let id = match crate::server::new_token() {
            Ok(t) => format!("fr-{}", &t[..t.len().min(16)]),
            Err(e) => return format!("I couldn't make a secure link: {e}"),
        };
        let now = clock();
        if state.owner == me {
            // Your own group: straight to them, as its owner.
            let mut groups = groups;
            groups.relay_owed.push(crate::groups::Relay {
                group_id: state.group_id.clone(),
                to: target,
                author: me,
                id,
                body,
                sent_at: now,
                sent_offset_mins: 0,
                after: 0,
            });
            if let Err(e) = groups.save(&self.store) {
                return format!("I couldn't queue the friend request: {e}");
            }
        } else {
            let Some(via) = pairings.name_of_key(&state.owner) else {
                return format!("I can't reach the owner of \"{}\" to pass it on.", state.name);
            };
            let store = self.friend_store();
            let mut out = crate::friends::Outbox::load(&store);
            out.requests.push(crate::friends::SentRequest {
                via,
                group_id: state.group_id.clone(),
                group_name: state.name.clone(),
                id,
                body,
                at: now,
            });
            if let Err(e) = out.save(&store) {
                return format!("I couldn't queue the friend request: {e}");
            }
        }
        format!("Sent {their_name} a friend request through \"{}\". Once they accept, you're friends.", state.name)
    }

    /// A friend request travelling through a group you're in: yours to keep
    /// if it's for you, yours to pass on (to that one person only) if you own
    /// the group, and nobody else's business.
    fn friend_request_in_group(
        &mut self,
        gid: &str,
        state: &crate::groups::GroupState,
        author: Option<&str>,
        handed_by: Option<&str>,
        body: &str,
    ) {
        let Some((target, link_text)) = crate::friends::read_request(body) else { return };
        let Some(author) = author else { return };
        // Only someone in the group asks through it.
        let Some((speaks_as, _)) = state.speaks_as(author) else { return };
        let Some(me) = self.my_key() else { return };
        if target == me {
            let Ok(link) = crate::friends::Link::decode(&link_text) else { return };
            // The link must be the asker's own: the person the group vouches
            // for is the person you'd be adding.
            let theirs = link.key == author || (speaks_as == state.owner && state.is_delegate(&link.key));
            if !theirs {
                self.log.warn(&format!("ignored a friend request in \"{}\" whose link wasn't the sender's", state.name));
                return;
            }
            let pairings = crate::kin::Pairings::load(&self.peer_dir);
            if pairings.name_of_key(&link.key).is_some() {
                return;
            }
            let from = pairings
                .name_of_key(&speaks_as)
                .or_else(|| state.seat(&speaks_as).map(|s| s.name.clone()))
                .unwrap_or_else(|| link.name.clone());
            let store = self.friend_store();
            let mut reqs = crate::friends::Requests::load(&store);
            reqs.add(crate::friends::Request { from: from.clone(), in_group: state.name.clone(), link: link_text, at: clock() });
            if let Err(e) = reqs.save(&store) {
                self.log.warn(&format!("couldn't keep {from}'s friend request: {e}"));
                return;
            }
            let said = format!(
                "{from} (from \"{}\") wants to be friends. Say \"accept friend request from {from}\", or open Friends on the hub.",
                state.name
            );
            self.journal.record_at(crate::activity::Kind::Offered, &said, true, clock());
            let note = crate::notify::Note::new("Friend request", &said, crate::notify::Urgency::Routine, clock());
            let _ = self.reach_you(note, clock());
            return;
        }
        // As the owner: on to that one person, never to the whole group.
        if state.owner == me && handed_by != Some(me.as_str()) && state.speaks_as(&target).is_some() {
            let mut groups = crate::groups::Groups::load(&self.store);
            let id = format!("fr-{}", &crate::digest::sha256_hex(body.as_bytes())[..16]);
            if groups.relay_owed.iter().any(|r| r.id == id) {
                return;
            }
            groups.relay_owed.push(crate::groups::Relay {
                group_id: gid.to_string(),
                to: target,
                author: author.to_string(),
                id,
                body: body.to_string(),
                sent_at: clock(),
                sent_offset_mins: 0,
                after: 0,
            });
            let _ = groups.save(&self.store);
        }
    }

    /// Say yes to a friend request: the same as adding them from their link.
    fn accept_friend_request(&mut self, from: &str) -> String {
        match self.take_friend_request(from) {
            Ok(link) => self.add_friend(&link),
            Err(said) => said,
        }
    }

    /// The link in a waiting friend request, taken off the list -- what
    /// accepting then adds (said, or from the hub on the crew).
    pub(crate) fn take_friend_request(&mut self, from: &str) -> std::result::Result<String, String> {
        let store = self.friend_store();
        let mut reqs = crate::friends::Requests::load(&store);
        let Some(r) = reqs.take(from.trim()) else {
            return Err(format!("There's no friend request from {} waiting.", from.trim()));
        };
        if let Err(e) = reqs.save(&store) {
            return Err(format!("I couldn't update your friend requests: {e}"));
        }
        Ok(r.link)
    }

    /// Say no. Nothing is sent back; their link simply runs out.
    pub fn decline_friend_request(&mut self, from: &str) -> String {
        let store = self.friend_store();
        let mut reqs = crate::friends::Requests::load(&store);
        match reqs.take(from.trim()) {
            None => format!("There's no friend request from {} waiting.", from.trim()),
            Some(r) => match reqs.save(&store) {
                Ok(()) => format!("Declined {}'s friend request. They aren't told.", r.from),
                Err(e) => format!("I couldn't update your friend requests: {e}"),
            },
        }
    }

    /// One knock on the door other Atlases use, if there is one: handled, and
    /// what's worth saying about it (often nothing). `None` when nobody
    /// knocked -- it never waits for anyone.
    pub fn answer_peer_door(&mut self, t: u64) -> Option<String> {
        let l = self.signal_listener.take()?;
        let arrived = l.poll_once(t);
        self.signal_listener = Some(l);
        Some(self.handle_arrived(arrived?))
    }

    /// Whatever came through the door -- knocked, or collected from a friend
    /// holding this Atlas's mail -- handled; and what's worth saying.
    fn handle_arrived(&mut self, arrived: crate::kin::Arrived) -> String {
        match arrived {
            crate::kin::Arrived::Signal(i) => {
                self.receive_signal(&i);
                String::new()
            }
            crate::kin::Arrived::Handoff(d) => self.receive_handoff(&d),
            crate::kin::Arrived::Chat(c) => {
                self.receive_chat(&c);
                String::new()
            }
            crate::kin::Arrived::Read(r) => {
                self.receive_read(&r);
                String::new()
            }
            crate::kin::Arrived::Left(l) => {
                self.receive_left(&l);
                String::new()
            }
            crate::kin::Arrived::Hello(h) => self.receive_hello(&h),
            crate::kin::Arrived::Group(g) => self.receive_group(&g),
            crate::kin::Arrived::Friend(b) => self.receive_friend(&b),
            crate::kin::Arrived::Feedback(f) => {
                let said = crate::feedback::heard_feedback(&self.store, &f.from, &f.body).unwrap_or_default();
                if !said.is_empty() {
                    self.journal.record_at(crate::activity::Kind::Blocked, &said, true, f.at);
                    // `reach_you` speaks it itself when you're here; said
                    // again by the caller only when it went elsewhere.
                    if matches!(self.reach_you(crate::notify::Note::new("Feedback", &said, crate::notify::Urgency::Routine, f.at), f.at), crate::notify::Sent::Spoken) {
                        return String::new();
                    }
                }
                said
            }
            crate::kin::Arrived::FeedbackAnswer(f) => {
                let said = crate::feedback::heard_answer(&self.store, &f.from, &f.body).unwrap_or_default();
                if !said.is_empty() {
                    self.journal.record_at(crate::activity::Kind::Upkeep, &said, true, f.at);
                    // `reach_you` speaks it itself when you're here; said
                    // again by the caller only when it went elsewhere.
                    if matches!(self.reach_you(crate::notify::Note::new("Your feedback", &said, crate::notify::Urgency::Routine, f.at), f.at), crate::notify::Sent::Spoken) {
                        return String::new();
                    }
                }
                said
            }
        }
    }

    /// Let a peer in on the open door, as a restart would. For tests that
    /// change pairings on disk while a door is open.
    /// Tests sign with a key of their own: trust it, and look for builds in `dir`.
    pub fn release_setup_for_test(&mut self, anchor: [u8; 32], dir: std::path::PathBuf) {
        self.release_anchor = anchor;
        self.builds_folder = Some(dir);
    }

    /// Tests keep their downloads in a folder of their own.
    pub fn downloads_for_test(&mut self, dir: std::path::PathBuf) {
        self.builds_folder = Some(dir);
    }

    pub fn admit_peer_for_test(&self, peer: crate::kin::Peer) {
        if let Some(l) = &self.signal_listener {
            l.admit_peer(peer);
        }
    }

    /// Take a friend back: they can't reach you and you can't reach them.
    pub fn unfriend(&mut self, who: &str) -> String {
        let who = who.trim();
        let mut pairings = crate::kin::Pairings::load(&self.peer_dir);
        if !pairings.forget(who) {
            return format!("{who} isn't one of your friends.");
        }
        if let Err(e) = pairings.save(&self.peer_dir) {
            return format!("I couldn't write that down ({e}), so {who} comes back when Atlas restarts.");
        }
        if let Some(l) = &self.signal_listener {
            l.forget_peer(who);
        }
        let store = self.friend_store();
        let mut out = crate::friends::Outbox::load(&store);
        let before = out.pending.len();
        out.pending.retain(|p| !crate::kin::same_name(&p.name, who));
        if out.pending.len() != before {
            if let Err(e) = out.save(&store) {
                self.log.warn(&format!("couldn't stop trying to reach {who}: {e}"));
            }
        }
        format!("Unfriended {who}. They can't reach your Atlas any more, and you can't reach theirs.")
    }

    /// Everything the Friends page shows.
    pub fn friends_view(&self) -> crate::hub::FriendsView {
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let store = self.friend_store();
        let me = self.my_key().unwrap_or_default();
        let mut could_ask: Vec<(String, String)> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for h in crate::groups::Groups::load(&self.store).held.values() {
            if h.state.speaks_as(&me).is_none() {
                continue;
            }
            for s in &h.state.seats {
                if s.key == me || pairings.name_of_key(&s.key).is_some() || seen.contains(&s.key) || h.state.is_delegate(&s.key) {
                    continue;
                }
                seen.push(s.key.clone());
                could_ask.push((s.name.clone(), h.state.name.clone()));
            }
        }
        crate::hub::FriendsView {
            friends: pairings.peers.iter().map(|p| p.name.clone()).collect(),
            requests: crate::friends::Requests::load(&store).waiting.into_iter().map(|r| (r.from, r.in_group)).collect(),
            could_ask,
            reaching: crate::friends::Outbox::load(&store).pending.into_iter().map(|p| p.name).collect(),
            link: None,
            said: None,
            reach: self.reach_said(),
        }
    }

    /// "add a friend", a pasted link, "accept friend request from Sam", ...
    pub(super) fn friend(&mut self, said: &str) -> String {
        use crate::friends::Spoken;
        match crate::friends::read_spoken(said) {
            Some(Spoken::Link) => match self.friend_link() {
                Ok(link) => format!(
                    "Here's your friend link -- send it to them any way you like. It works once, for a week: {link}"
                ),
                Err(e) => e,
            },
            Some(Spoken::Add) => self.add_friend(said),
            Some(Spoken::Requests) => {
                let reqs = crate::friends::Requests::load(&self.friend_store());
                if reqs.waiting.is_empty() {
                    "No friend requests waiting.".into()
                } else {
                    let names: Vec<String> =
                        reqs.waiting.iter().map(|r| format!("{} (from \"{}\")", r.from, r.in_group)).collect();
                    format!("Friend requests from {}.", names.join(", "))
                }
            }
            Some(Spoken::Accept(who)) => self.accept_friend_request(&who),
            Some(Spoken::Decline(who)) => self.decline_friend_request(&who),
            Some(Spoken::Request(who)) => self.send_friend_request(&who),
            None => "Say \"add a friend\" for a link to send, or paste a friend link someone sent you.".into(),
        }
    }

    /// Share one of your add-ons: with one paired person, or with a group
    /// (everyone in it you can reach, plus a line in the group saying so).
    /// It reaches them as an offer on their shelf; nobody gets it installed by
    /// being sent it. Returns what to tell you.
    /// Sharing an add-on (the Add-ons page's Share button): everything that
    /// doesn't wait on the network -- the add-on read, who it goes to, and
    /// the link to reach them. The sending is `AddonShare::send`, which the
    /// hub runs on the crew; the line in the group's chat and the sentence
    /// follow when it's done (`addon_shared_in_group`, `addon_share_said`).
    pub(crate) fn prepare_addon_share(&mut self, id: &str, with: &str) -> std::result::Result<AddonShare, String> {
        let (file_name, bytes, name) = crate::plugins::to_share(&self.plugins_dir, id)?;
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let key = with.trim().strip_prefix("the ").unwrap_or(with.trim());
        let key = key.strip_suffix(" group").unwrap_or(key).trim();
        if key.is_empty() {
            return Err("Say which group or person to share it with.".into());
        }
        let (people, group): (Vec<String>, Option<(String, String)>) = match self.chats.group_named(key) {
            Some(g) => (g.members.clone(), Some((g.id.clone(), g.name.clone()))),
            None if pairings.has_peer(key) => (vec![key.to_string()], None),
            None => return Err(format!("There's no group or paired person called \"{key}\".")),
        };
        let covering = match &group {
            Some((_, gname)) => format!("{}{gname}", crate::plugins::SHARED_IN),
            None => format!("the add-on {name}"),
        };
        let link = self.peer_link(&pairings);
        Ok(AddonShare { name, file_name, bytes, covering, people, group, pairings, link })
    }

    /// The line in the group's chat when an add-on was shared in a group.
    pub(super) fn addon_shared_in_group(&mut self, name: &str, group: Option<&(String, String)>) {
        if let Some((gid, _)) = group {
            let pairings = crate::kin::Pairings::load(&self.peer_dir);
            let roster = crate::roster::Roster::load(&self.store);
            let line = format!("I shared the add-on \"{name}\" — it's on your Add-ons page if you want it.");
            if self.chats.post(gid, &line, clock(), local_offset_mins(), &roster, &pairings).is_ok() {
                let _ = self.chats.save(&self.store);
            }
        }
    }

    /// Give a group made before groups had owners an owner -- you. A new
    /// group with the same people (those whose Atlas has introduced itself)
    /// and the same name; the old one is renamed "(before)" and told where the
    /// conversation went, and left exactly as it was otherwise.
    pub fn adopt_group(&mut self, name: &str) -> std::result::Result<String, String> {
        let Some(old) = self.chats.group_named(name).cloned() else {
            return Err(format!("there's no group called \"{name}\""));
        };
        if crate::groups::is_owned_id(&old.id) {
            return Err(format!("\"{}\" already has an owner", old.name));
        }
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let (keyed, unkeyed): (Vec<String>, Vec<String>) =
            old.members.iter().cloned().partition(|m| pairings.key_of(m).is_some());
        self.chats.name_group(&old.id, &format!("{} (before)", old.name));
        let done = crate::groups::act(&self.store, &self.peer_dir, "new", &old.name, &keyed.join(", "), "");
        if let Err(e) = &done {
            self.chats.name_group(&old.id, &old.name);
            return Err(e.clone());
        }
        let roster = crate::roster::Roster::load(&self.store);
        let line = format!("I've started \"{}\" again as a group with an owner — let's carry on there.", old.name);
        let _ = self.chats.post(&old.id, &line, clock(), local_offset_mins(), &roster, &pairings);
        self.settle_owned_groups();
        let _ = self.chats.save(&self.store);
        let mut said = format!("\"{}\" now has an owner: you.", old.name);
        if !unkeyed.is_empty() {
            said.push_str(&format!(
                " {} couldn't be added yet — their Atlas hasn't introduced itself. Add them once it has.",
                unkeyed.join(", ")
            ));
        }
        Ok(said)
    }

    /// "Add Sam to the Friends group" and its kin, said out loud.
    pub(super) fn change_group(&mut self, said: &str) -> String {
        let Some((what, who, group, role)) = crate::groups::read_spoken(said) else {
            return "Say it like \"add Sam to the Friends group\" or \"make Maya a reader in the Friends group\".".into();
        };
        match crate::groups::act(&self.store, &self.peer_dir, what, &group, &who, &role) {
            Ok(done) => {
                self.settle_owned_groups();
                format!("{done} Everyone's Atlas gets the new list next time they're reachable.")
            }
            Err(why) => why,
        }
    }

    /// Tell a group you're using an add-on and recommend it -- the "if it
    /// helps, others pick it up" half of sharing, said by a person in the
    /// group rather than counted by anything.
    pub fn recommend_addon(&mut self, id: &str, group: &str) -> String {
        let Ok((_, _, name)) = crate::plugins::to_share(&self.plugins_dir, id) else {
            return format!("There's no add-on called {id}.");
        };
        let Some(g) = self.chats.group_named(group).map(|g| g.id.clone()) else {
            return format!("There's no group called \"{group}\".");
        };
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let roster = crate::roster::Roster::load(&self.store);
        let line = format!("I'm using the add-on \"{name}\" and I'd recommend it.");
        match self.chats.post(&g, &line, clock(), local_offset_mins(), &roster, &pairings) {
            Ok(_) => {
                let _ = self.chats.save(&self.store);
                format!("Told {group} you recommend \"{name}\".")
            }
            Err(e) => e.plain(),
        }
    }

    /// Release notices you signed and asked to announce (`atlas release
    /// announce`), posted into every release channel you own. The command is
    /// the decision -- you ran it, here -- so nothing asks again. A notice
    /// that isn't a signed release is left where it is and said once.
    pub(super) fn post_release_notices(&mut self, t: u64) -> Option<String> {
        let dir = self.store.root().join(crate::update_courier::OUTBOX);
        let entries = std::fs::read_dir(&dir).ok()?;
        let files: Vec<std::path::PathBuf> =
            entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        if files.is_empty() {
            return None;
        }
        let me = self.my_key()?;
        let groups = crate::groups::Groups::load(&self.store);
        let channels: Vec<String> = groups
            .held
            .values()
            .filter(|h| h.state.release_channel && h.state.owner == me)
            .map(|h| h.state.group_id.clone())
            .collect();
        if channels.is_empty() {
            return None;
        }
        self.settle_owned_groups();
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let roster = crate::roster::Roster::load(&self.store);
        let mut posted = Vec::new();
        for f in files {
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            // A release notice, or a change of release key (`atlas release rotate`).
            let body = if let Ok(signed) = serde_json::from_str::<crate::release::SignedManifest>(&text) {
                crate::update_courier::announcement(&signed)
            } else if let Ok(rotation) = serde_json::from_str::<crate::release::SignedRotation>(&text) {
                crate::update_apply::rotation_notice(&rotation)
            } else {
                let _ = std::fs::rename(&f, f.with_extension("not-a-notice"));
                continue;
            };
            let mut all = true;
            for gid in &channels {
                if self.chats.post(gid, &body, t, local_offset_mins(), &roster, &pairings).is_err() {
                    all = false;
                }
            }
            if all {
                let _ = std::fs::rename(&f, f.with_extension("posted"));
                posted.push(f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
            }
        }
        if posted.is_empty() {
            return None;
        }
        let _ = self.chats.save(&self.store);
        Some(format!("Announced {} in your release channel.", posted.join(", ")))
    }

    /// This Atlas's own public key, if it has one.
    pub(super) fn my_key(&self) -> Option<String> {
        crate::peerkey::Identity::load_or_create(&self.peer_dir).ok().map(|i| i.public())
    }

    /// Make every owned group on this end match its signed list: who is in
    /// the room (only people this Atlas is paired with and has a key for --
    /// the ones it can actually reach), what it's called, and whether you're
    /// still in it at all. The list is the single source of truth; the room
    /// is how the conversation is kept.
    pub fn settle_owned_groups(&mut self) {
        let groups = crate::groups::Groups::load(&self.store);
        if groups.held.is_empty() {
            return;
        }
        let Some(me) = self.my_key() else { return };
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let mut changed = false;
        for (gid, h) in &groups.held {
            if h.state.is_delegate(&me) {
                // One of the owner's own devices: it talks to the group
                // through the owner's Atlas, which passes things on both ways.
                let members: Vec<String> = pairings.name_of_key(&h.state.owner).into_iter().collect();
                changed |= self.chats.settle_owned(gid, &h.state.name, members);
            } else if h.state.seat(&me).is_some() {
                // Everyone seated you can reach -- and, as the owner, your own
                // other devices, so your phone gets what you post here.
                let mut members: Vec<String> = h
                    .state
                    .seats
                    .iter()
                    .filter(|s| s.key != me)
                    .filter_map(|s| pairings.name_of_key(&s.key))
                    .collect();
                if h.state.owner == me {
                    members.extend(h.state.delegates.iter().filter_map(|d| pairings.name_of_key(&d.device)));
                }
                changed |= self.chats.settle_owned(gid, &h.state.name, members);
            } else if self.chats.room(gid).is_some() {
                self.chats.leave_group(gid);
                changed = true;
            }
        }
        if changed {
            let _ = self.chats.save(&self.store);
        }
    }

    /// The owner's Atlas's upkeep with its paired devices, once a tick:
    /// introduce this Atlas's key to anyone who hasn't taken it, and hand
    /// every member of a group you own the latest list (and anyone just taken
    /// out, the list without them). One that's offline is retried every few
    /// minutes, not every tick.
    pub(super) fn peer_upkeep(&mut self, t: u64) {
        const RETRY_SECS: u64 = 300;
        // Tor, kept running while there's anyone to reach or be reached by.
        if self.signal_listener.is_some() && self.tor.as_mut().map_or(true, |x| x.stopped()) {
            let k = "tor:start".to_string();
            if !self.peer_tries.get(&k).is_some_and(|at| t.saturating_sub(*at) < RETRY_SECS) {
                self.peer_tries.insert(k, t);
                if let Err(e) = self.start_tor() {
                    self.log.warn(&e);
                }
            }
        }
        if let Some(said) = self.keep_tor_getting_through(t) {
            self.journal.record_at(crate::activity::Kind::Upkeep, &said, true, t);
            let _ = self.reach_you(crate::notify::Note::new("Friends", &said, crate::notify::Urgency::Routine, t), t);
        }
        for said in self.friend_upkeep(t) {
            self.journal.record_at(crate::activity::Kind::Upkeep, &said, true, t);
            let _ = self.reach_you(crate::notify::Note::new("Friends", &said, crate::notify::Urgency::Routine, t), t);
        }
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        if pairings.contacts.is_empty() {
            return;
        }
        let Ok(me) = crate::peerkey::Identity::load_or_create(&self.peer_dir) else { return };
        let me = me.public();
        let link = self.peer_link(&pairings);
        let mut told: std::collections::BTreeMap<String, String> = self.store.load("peer_hello_told");
        let mut told_changed = false;
        for c in &pairings.contacts {
            if told.get(&c.name.to_lowercase()) == Some(&me) {
                continue;
            }
            let k = format!("hello:{}", c.name.to_lowercase());
            if self.peer_tries.get(&k).is_some_and(|at| t.saturating_sub(*at) < RETRY_SECS) {
                continue;
            }
            self.peer_tries.insert(k, t);
            if link.say_hello(&c.name, &me) {
                told.insert(c.name.to_lowercase(), me.clone());
                told_changed = true;
            }
        }
        if told_changed {
            let _ = self.store.save("peer_hello_told", &told);
        }
        // A release heard about: its file, a few pieces a tick, from the Atlas
        // whose channel announced it -- or from any friend who already has it
        // (gap AD: the signature is the check, not who hands it over).
        // Resumes where it stopped.
        let avail = crate::update_courier::Available::load(&self.store);
        if avail.notice.is_some() && avail.downloaded.is_empty() {
            let k = "release:fetch".to_string();
            let owner = (!avail.from.is_empty()).then(|| pairings.name_of_key(&avail.from)).flatten();
            let friends: Vec<String> = pairings.contacts.iter().map(|c| c.name.clone()).collect();
            if !self.peer_tries.get(&k).is_some_and(|at| t.saturating_sub(*at) < RETRY_SECS) {
                use crate::update_courier::Fetched;
                match crate::update_courier::fetch_from_any(&self.store, 8, owner.as_deref(), &friends, 3, |who, sha, off| {
                    link.fetch_release(who, sha, off)
                }) {
                    Fetched::Waiting => {
                        self.peer_tries.insert(k, t);
                    }
                    Fetched::Ready(said) | Fetched::Bad(said) => {
                        self.journal.record_at(crate::activity::Kind::Upkeep, &said, true, t);
                        let _ = self.reach_you(crate::notify::Note::new("Atlas update", &said, crate::notify::Urgency::Routine, t), t);
                    }
                    Fetched::Partway(..) => {}
                    // Nothing to fetch although an update was announced: its
                    // notice names no usable file. Looked at again after the
                    // usual pause and said in the log (29 Sep 2026: tried on
                    // every tick, silently).
                    Fetched::Nothing => {
                        if !self.peer_tries.contains_key(&k) {
                            self.log.warn("an Atlas update was announced, but its notice names no file I can fetch; I'll look again later");
                        }
                        self.peer_tries.insert(k, t);
                    }
                }
            }
        }
        // Step 2: a release that's here goes in -- by itself at a quiet
        // moment on your own devices, asked about on friends' copies
        // (`update_apply`). Staged, then Atlas starts itself again so the
        // new build is health-checked and put on probation (O1).
        if let Some(platform) = crate::release::this_platform() {
            let me = self.my_key().unwrap_or_default();
            let held = crate::groups::Groups::load(&self.store);
            let channel = held
                .held
                .values()
                .map(|h| &h.state)
                .find(|g| g.release_channel && (avail.from.is_empty() || g.owner == avail.from));
            let moment = crate::update_apply::UpdateMoment {
                idle_secs: self.plat.input_idle_secs(),
                os: self.plat.quiet_state(),
                on_call: crate::notify::on_a_call(self.awareness.last_active_window()),
                work_in_hand: false,
            };
            // A failure on your own devices goes straight into your own list.
            // On a friend's device it stays there; the friend decides whether
            // to tell you (`atlas feedback send`).
            if channel.is_some_and(|c| !me.is_empty() && (c.owner == me || c.is_delegate(&me))) {
                if let Some(report) = crate::update_apply::unfiled_own_failure(&self.store) {
                    crate::update_apply::file_own_report(&self.store, &report);
                }
            }
            // iPhones that told the Your phone page their ID while nobody was
            // looking: kept, and queued for whoever sends Atlas out.
            for said in self.take_heard_phones(t) {
                self.journal.record_at(crate::activity::Kind::Upkeep, &said, true, t);
            }
            // Feedback your friend chose to send, and your answers to theirs:
            // sent over the pairing, kept until taken.
            for (to, body, id) in crate::feedback::feedback_outbox(&self.store) {
                let k = format!("feedback:{id}");
                if self.peer_tries.get(&k).is_some_and(|at| t.saturating_sub(*at) < RETRY_SECS) {
                    continue;
                }
                self.peer_tries.insert(k, t);
                if link.send_feedback(&to, "/feedback", &body) {
                    crate::feedback::feedback_delivered(&self.store, &id);
                }
            }
            for (to, body, id) in crate::feedback::answers_out(&self.store) {
                let k = format!("feedback-answer:{id}");
                if self.peer_tries.get(&k).is_some_and(|at| t.saturating_sub(*at) < RETRY_SECS) {
                    continue;
                }
                self.peer_tries.insert(k, t);
                if link.send_feedback(&to, "/feedback-answer", &body) {
                    crate::feedback::answer_delivered(&self.store, &id);
                }
            }
            match crate::update_apply::update_tick(&self.store, &self.store.install_root(), platform, &me, channel, &moment, t) {
                crate::update_apply::Ticked::Say(said) => {
                    self.journal.record_at(crate::activity::Kind::Upkeep, &said, true, t);
                    let _ = self.reach_you(crate::notify::Note::new("Atlas update", &said, crate::notify::Urgency::Routine, t), t);
                }
                crate::update_apply::Ticked::Restart(said) => {
                    self.journal.record_at(crate::activity::Kind::Upkeep, &said, true, t);
                    let args: Vec<String> = std::env::args().skip(1).collect();
                    // Stopped properly before the new copy starts, not after
                    // (`restart_as`): see there for what it cost.
                    match self.restart_as(|| {
                        std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| crate::update_apply::relaunch_self(&exe, &args))
                    }) {
                        Ok(()) => std::process::exit(0),
                        Err(why) => self.log.info(&format!("couldn't restart into the update: {why}")),
                    }
                }
                crate::update_apply::Ticked::Nothing => {}
            }
        }
        let mut groups = crate::groups::Groups::load(&self.store);
        let mut delivered = false;
        for (gid, key) in groups.owed(&me) {
            let Some(name) = pairings.name_of_key(&key) else { continue };
            let k = format!("group:{gid}:{key}");
            if self.peer_tries.get(&k).is_some_and(|at| t.saturating_sub(*at) < RETRY_SECS) {
                continue;
            }
            self.peer_tries.insert(k, t);
            let Some(signed) = groups.held.get(&gid).map(|h| h.signed.clone()) else { continue };
            if link.push_group(&name, &signed) {
                groups.delivered_to(&gid, &key);
                delivered = true;
            }
        }
        // Members' messages owed to other members, passed on as the owner.
        let owed: Vec<crate::groups::Relay> = groups.relay_owed.clone();
        let mut passed = false;
        for r in owed {
            let Some(name) = pairings.name_of_key(&r.to) else { continue };
            let k = format!("relay:{}", r.to);
            if self.peer_tries.get(&k).is_some_and(|at| t.saturating_sub(*at) < RETRY_SECS) {
                continue;
            }
            let gname = groups.held.get(&r.group_id).map(|h| h.state.name.clone()).unwrap_or_default();
            if link.relay(&name, &r.group_id, &gname, &r.author, &r.id, &r.body, r.sent_at, r.sent_offset_mins, r.after) {
                groups.relay_owed.retain(|x| !(x.id == r.id && x.to == r.to));
                passed = true;
            } else {
                // One failed send puts that person off until the next retry
                // window, rather than trying each of their queued messages.
                self.peer_tries.insert(k, t);
            }
        }
        if delivered || passed {
            let _ = groups.save(&self.store);
        }
    }

    /// A peer told us they read some of our messages. Record it against each
    /// id and save. Best-effort by nature: a receipt for a message we no
    /// longer hold, or for a recipient who was never on it, is `mark_read`'s
    /// to drop, and nothing here fabricates a read we weren't told about.
    pub fn receive_read(&mut self, r: &crate::kin::ReadReceipt) {
        let mut changed = false;
        for id in &r.ids {
            if self.chats.mark_read(id, &r.from, r.at) {
                changed = true;
            }
        }
        if changed {
            if let Err(e) = self.chats.save(&self.store) {
                self.log
                    .warn(&format!("couldn't save a read receipt from {}: {e}", r.from));
            }
        }
    }

    /// A peer told us they've left a group. Take them out of our copy of its
    /// membership and save. Best-effort by nature: if we don't hold that group
    /// (or they weren't in our copy of it), there is nothing to do and nothing
    /// is invented.
    pub fn receive_left(&mut self, l: &crate::kin::LeftGroup) {
        // Somebody left a group you own: take them off its list, so every
        // member's Atlas agrees they've gone -- the owner's list is the one
        // everybody else goes by.
        if crate::groups::is_owned_id(&l.group_id) {
            let pairings = crate::kin::Pairings::load(&self.peer_dir);
            if let (Some(key), Ok(me)) =
                (pairings.key_of(&l.from).map(String::from), crate::peerkey::Identity::load_or_create(&self.peer_dir))
            {
                let mut groups = crate::groups::Groups::load(&self.store);
                if groups.held.get(&l.group_id).is_some_and(|h| h.state.owner == me.public())
                    && groups.remove(&me, &l.group_id, &key).is_ok()
                {
                    let _ = groups.save(&self.store);
                    self.settle_owned_groups();
                }
            }
            return;
        }
        if self.chats.member_left(&l.group_id, &l.from) {
            if let Err(e) = self.chats.save(&self.store) {
                self.log
                    .warn(&format!("couldn't save {} leaving a group: {e}", l.from));
            }
        }
    }
}
