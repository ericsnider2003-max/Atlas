//! Answering a form posted to the hub (hub_post).
//!
//! Moved out of `hublive.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Daemon<'_> {
    /// A form posted to one of the design's added pages. Every change goes
    /// through the same module the spoken command uses, and comes back to
    /// the page with what happened said once.
    pub(super) fn hub_post(&mut self, path: &str, f: &[(String, String)]) -> Reply {
        let now = crate::store::now();
        let what = field_of(f, "what").unwrap_or_default();
        match path {
            "/hub/social" => {
                let said = self.social_post(f, now);
                hub::back_with(Page::Social.href(), "", &said)
            }
            // Outstanding's Drop it / Stop it (2 Oct 2026). The key is looked
            // up again in `drop_outstanding`, so a stale page (pressed twice,
            // or after the thing finished) says it's already off rather than
            // taking off something else.
            "/hub/outstanding" => {
                let key = field_of(f, "key").unwrap_or_default();
                if what != "drop" || key.is_empty() {
                    return hub::back_with(Page::Outstanding.href(), "", "That button didn't say which thing, so nothing changed.");
                }
                if let Some(no) = self.handed_over_refusal(&Intent::DropTask(String::new())) {
                    return hub::back_with(Page::Outstanding.href(), "", &no);
                }
                let said = match self.drop_outstanding(&key, now) {
                    Ok(off) => off.said(),
                    Err(why) => why,
                };
                hub::back_with(Page::Outstanding.href(), "", &said)
            }
            "/hub/messages" => {
                if what == "start" {
                    let who = field_of(f, "who").unwrap_or_default();
                    let pairings = crate::kin::Pairings::load(&self.peer_dir);
                    let roster = crate::roster::Roster::load(&self.store);
                    let space = self.shared_space(std::slice::from_ref(&who), &roster, &pairings);
                    return match self.chats.open(&who, space, std::slice::from_ref(&who), &roster, &pairings) {
                        Ok(id) => {
                            if let Err(e) = self.chats.save(&self.store) {
                                return hub::back_with(
                                    Page::Messages.href(),
                                    &format!("room={}", crate::research::urlencode(&id)),
                                    &with_keeping("Started.".into(), Err(e)),
                                );
                            }
                            Reply::redirect(&format!("{}?room={}", Page::Messages.href(), crate::research::urlencode(&id)))
                        }
                        Err(e) => hub::back_with(Page::Messages.href(), "", &e.plain()),
                    };
                }
                let room = field_of(f, "room").unwrap_or_default();
                let body = field_of(f, "body").unwrap_or_default();
                if body.trim().is_empty() {
                    return hub::back_with(Page::Messages.href(), &format!("room={}", crate::research::urlencode(&room)), "Nothing to send.");
                }
                let pairings = crate::kin::Pairings::load(&self.peer_dir);
                let roster = crate::roster::Roster::load(&self.store);
                let said = match self.chats.post(&room, body.trim(), now, local_offset_mins(), &roster, &pairings) {
                    Ok((_, left_out)) => {
                        let said = if left_out.is_empty() {
                            "Sent.".to_string()
                        } else {
                            format!("Sent. {} isn't in that business any more, so I left them out.", left_out.join(", "))
                        };
                        with_keeping(said, self.chats.save(&self.store))
                    }
                    Err(e) => e.plain(),
                };
                hub::back_with(Page::Messages.href(), &format!("room={}", crate::research::urlencode(&room)), &said)
            }
            "/hub/tasks" => {
                let b = field_of(f, "b").unwrap_or_default();
                let mut tasks = crate::shared_task::Tasks::load(&self.store);
                let said = if what == "done" {
                    match field_of(f, "id").and_then(|i| i.parse().ok()) {
                        Some(id) if tasks.complete(id) => "Marked done.".to_string(),
                        _ => "I couldn't find that task.".to_string(),
                    }
                } else {
                    let text = field_of(f, "text").unwrap_or_default();
                    let roster = crate::roster::Roster::load(&self.store);
                    if text.trim().is_empty() {
                        "Nothing to add.".to_string()
                    } else if !roster.businesses().iter().any(|x| x.eq_ignore_ascii_case(&b)) {
                        "Shared tasks belong to a business on your roster.".to_string()
                    } else {
                        let due = field_of(f, "due")
                            .and_then(|d| crate::hubpages::days_of(&d))
                            .map(|d| crate::localclock::utc_of_wall(d, 17 * 3600));
                        tasks.add(crate::earned::Space::Business(b.clone()), text.trim(), due, now);
                        format!("Added to {b}.")
                    }
                };
                // Only what was actually kept is said as done: `tasks` was read
                // from disk for this click, so a save that fails loses it now.
                let said = match tasks.save(&self.store) {
                    Ok(()) => said,
                    Err(e) => didnt_stick(&e),
                };
                hub::back_with(Page::SharedTasks.href(), &format!("b={}", crate::research::urlencode(&b)), &said)
            }
            "/hub/clients" => {
                let addr = field_of(f, "address").unwrap_or_default();
                let name = field_of(f, "name").unwrap_or_default();
                let mut list = crate::clients::ClientList::load(&self.store);
                let said = if !addr.contains('@') {
                    "That doesn't look like an email address.".to_string()
                } else if list.is_client(&addr) {
                    "Already a client.".to_string()
                } else {
                    list.add(addr.trim(), name.trim(), "", now);
                    match list.save(&self.store) {
                        Ok(()) => format!("Added {}.", if name.trim().is_empty() { addr.trim() } else { name.trim() }),
                        Err(e) => didnt_stick(&e),
                    }
                };
                hub::back_with(Page::Clients.href(), &format!("c={}", crate::research::urlencode(addr.trim())), &said)
            }
            "/hub/sound" => {
                let key = field_of(f, "key").unwrap_or_default();
                let value = field_of(f, "value").unwrap_or_default();
                let said = match key.as_str() {
                    "voice" => self.apply_setting("voice_settings.voice", &value),
                    "speed" => self.apply_setting("voice_settings.speed", &value),
                    "volume" => self.apply_setting("sound.volume", &value),
                    "speak_replies" => self.apply_setting("sound.speak_replies", &value),
                    "muted" => self.apply_setting("sound.muted", &value),
                    "get-voice" => self.get_voice(&value),
                    "engine" => self.apply_setting("tts_engine.engine", &value),
                    "get-kokoro" => self.get_kokoro(),
                    "wake" => self.apply_setting("wake.enabled", &value),
                    "wake_phrase" => self.apply_setting("wake.phrase", &value),
                    "ptt" => self.apply_setting("push_to_talk.enabled", &value),
                    "quiet" => self.apply_setting("sound.quiet_hours", &value),
                    "quiet_hours" => {
                        let from = field_of(f, "from").unwrap_or_default();
                        let to = field_of(f, "to").unwrap_or_default();
                        if crate::sound::minutes(&from).is_none() || crate::sound::minutes(&to).is_none() {
                            "Those times didn't read — use the hours and minutes, like 22:00.".to_string()
                        } else {
                            let a = self.apply_setting("sound.quiet_from", &from);
                            let b = self.apply_setting("sound.quiet_to", &to);
                            format!("{a} {b}")
                        }
                    }
                    _ => "That isn't a sound setting.".to_string(),
                };
                hub::back_with(Page::Sound.href(), "", &said)
            }
            "/hub/trusted" => {
                let who = field_of(f, "who").unwrap_or_default();
                let yes = field_of(f, "trust").as_deref() == Some("yes");
                let mut pairings = crate::kin::Pairings::load(&self.peer_dir);
                let said = if !pairings.contacts.iter().any(|c| crate::kin::same_name(&c.name, &who)) {
                    format!("{who} isn't paired with you.")
                } else if yes {
                    pairings.trust(&who);
                    format!("{who} is trusted: routine sends go without a prompt. Personal files still ask.")
                } else if pairings.distrust(&who) {
                    format!("{who} asks first again.")
                } else {
                    format!("{who} already asks first.")
                };
                let saved = pairings.save(&self.peer_dir);
                let said = match saved {
                    Ok(()) => said,
                    Err(e) => format!("Couldn't keep that: {e}"),
                };
                hub::back_with(Page::Trusted.href(), "", &said)
            }
            "/hub/opportunities" => crate::hunting::post(self, f),
            "/hub/connect" => crate::connecting::post(self, f),
            "/hub/give" => {
                let text = field_of(f, "text").unwrap_or_default();
                let asked = field_of(f, "asked").filter(|a| !a.trim().is_empty());
                let said = if text.trim().is_empty() {
                    "Nothing to give.".to_string()
                } else {
                    match self.tray.hand(text.trim(), &crate::earned::Space::Personal, "the hub", now) {
                        Ok(id) => {
                            if let Some(a) = asked.as_deref() {
                                self.tray.ask_about(id, a);
                            }
                            with_keeping("Got it. I'll look at it and tell you what's in it.".to_string(), self.tray.save(&self.store))
                        }
                        Err(why) => why,
                    }
                };
                hub::back_with(Page::Give.href(), "", &said)
            }
            "/hub/talk" => {
                let text = field_of(f, "text").unwrap_or_default();
                let spoken = field_of(f, "spoken").as_deref() == Some("1");
                if text.trim().is_empty() {
                    return Reply::redirect(Page::Talk.href());
                }
                // Answered on the next tick, not inside this request: the
                // page comes straight back showing "thinking" and fills in
                // when the reply lands (27 Sep 2026: Talk hung while the model
                // answered, and so did the rest of the hub).
                self.talk_queue.push((text.trim().to_string(), spoken));
                if !spoken {
                    // Typed here, so "hands-free only" keeps the reply on screen.
                    return Reply::redirect(Page::Talk.href());
                }
                // Said out loud to the phone app, which heard it on the phone.
                // The reply is read out by the phone's own voice, if Sound &
                // voice allows it right now (mute, quiet hours, the reply rule).
                if self.sound_allows_speaking() {
                    Reply::redirect(&format!("{}?say=1", Page::Talk.href()))
                } else {
                    Reply::redirect(Page::Talk.href())
                }
            }
            // The Improvements page's two buttons. Before 27 Sep 2026 nothing
            // answered them: "Have a go" showed a bare 404 with no way back.
            "/hub/recommendations/go" => {
                let said = self.act_on_recommendation(
                    field_of(f, "which"),
                    field_of(f, "drop").as_deref() == Some("1"),
                );
                hub::back_with(Page::Recommendations.href(), "", &said)
            }
            // A learned wording's Forget button (Improvements, `learning`).
            "/hub/phrasebook" => {
                let wording = field_of(f, "wording").unwrap_or_default();
                let said = if wording.trim().is_empty() {
                    "Nothing was named, so nothing was forgotten.".to_string()
                } else {
                    self.forget_phrase_from_hub(&wording)
                };
                hub::back_with(Page::Recommendations.href(), "", &said)
            }
            "/hub/help" => {
                let text = field_of(f, "text").unwrap_or_default();
                let said = if text.trim().is_empty() {
                    "Say what got in the way, and where.".to_string()
                } else {
                    self.report_barrier(text.trim(), now)
                };
                hub::back_with(Page::Help.href(), "", &said)
            }
            "/hub/workshop" => {
                let task = field_of(f, "task").unwrap_or_default();
                let mut name = field_of(f, "name").unwrap_or_default();
                if name.trim().is_empty() {
                    // Named from its first few words when you didn't name it.
                    name = task.split_whitespace().take(4).collect::<Vec<_>>().join(" ");
                }
                let said = if name.trim().is_empty() {
                    "Say what the project should take on.".to_string()
                } else if self.workshop.resolve(name.trim()).is_some() {
                    format!("There's already a project called {}.", name.trim())
                } else {
                    self.workshop.register(name.trim(), field_of(f, "folder").unwrap_or_default().trim(), now);
                    if let Some(t) = field_of(f, "task").filter(|t| !t.trim().is_empty()) {
                        self.workshop.add_task(name.trim(), t.trim(), now);
                    }
                    with_keeping(format!("Started {}.", name.trim()), self.workshop.save(&self.store))
                };
                hub::back_with(Page::Workshop.href(), "", &said)
            }
            "/hub/phone" => {
                let kind = field_of(f, "kind").and_then(|k| crate::phoneadd::Kind::parse(&k));
                let said = match (what.as_str(), kind) {
                    ("start", Some(k)) => {
                        let which = field_of(f, "for").unwrap_or_default();
                        return self.start_phone_code(k, if which == "install" { "install" } else { "add" });
                    }
                    ("stop", _) => {
                        // A code still coming up is stopped when it arrives.
                        for a in self.hub_after.values() {
                            if let crate::daemon::HubAfter::PhoneCode { stop, .. } = a {
                                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                        if let Some(s) = self.phone_code.take() {
                            s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                            // Tailscale can take seconds to answer; nothing
                            // here waits for it.
                            std::thread::spawn(crate::phoneadd::stop_reaching_out);
                        }
                        "Stopped: the code no longer works.".to_string()
                    }
                    _ => "Pick which phone you have first.".to_string(),
                };
                let back = kind.or(self.phone_code.as_ref().map(|s| s.kind)).map(|k| format!("kind={}", k.slug())).unwrap_or_default();
                hub::back_with(Page::Phone.href(), &back, &said)
            }
            "/hub/updates" => {
                // Installing, going back and holding are the owner's, not a
                // guest's: the same refusal the voice gets.
                if let Some(no) = self.handed_over_refusal(&Intent::Updates(what.clone())) {
                    return hub::back_with(Page::Updates.href(), "", &no);
                }
                if what == "sign-send" {
                    let said = self.sign_and_send(f);
                    return hub::back_with(Page::Updates.href(), "", &said);
                }
                if what == "make-key" {
                    return match self.make_release_key(f) {
                        Ok(()) => Reply::redirect(Page::Updates.href()),
                        Err(why) => hub::back_with(Page::Updates.href(), "", &why),
                    };
                }
                let said = match what.as_str() {
                    "install" => self.updates_said("install"),
                    // The hub's own confirm button: the person at this device
                    // pressed "Yes, go back" on the question it showed.
                    "undo-confirmed" => self.updates_said("undo-confirmed"),
                    "mode" => {
                        use crate::update_apply::AutoUpdate;
                        let (mode, words) = match field_of(f, "mode").as_deref() {
                            Some("on") => (Some(AutoUpdate::Automatic), "Updates install by themselves at a quiet moment."),
                            Some("ask") => (Some(AutoUpdate::Ask), "I'll ask before installing each update."),
                            Some("off") => (Some(AutoUpdate::Off), "Updates are off; I'll still say what's out."),
                            _ => (None, "Back to the usual: by itself on your own devices, asking first on friends'."),
                        };
                        match crate::update_apply::choose_mode(&self.store, mode) {
                            Ok(()) => words.to_string(),
                            Err(e) => format!("I couldn't keep that: {e}"),
                        }
                    }
                    "hold" => {
                        let mine = crate::feedback::release_sender(&self.store, &self.peer_dir).is_some_and(|(_, _, m)| m);
                        let sha = field_of(f, "sha").unwrap_or_default();
                        if !mine {
                            "Only the person who sends Atlas out can hold a build.".to_string()
                        } else if !crate::update_apply::failure_reports(&self.store).iter().any(|r| r.sha256 == sha) {
                            "That isn't a build anyone has reported.".to_string()
                        } else {
                            crate::update_apply::hold_release(&self.store, &sha);
                            "Held: your Atlas stops handing that build out. The fix goes out as a new release.".to_string()
                        }
                    }
                    _ => "That isn't something the Updates page does.".to_string(),
                };
                hub::back_with(Page::Updates.href(), "", &said)
            }
            "/hub/feedback" => {
                if let Some(no) = self.handed_over_refusal(&Intent::Feedback(what.clone())) {
                    return hub::back_with(Page::Feedback.href(), "", &no);
                }
                let said = match what.as_str() {
                    "preview" => {
                        let words = field_of(f, "words").unwrap_or_default();
                        let attach = (field_of(f, "attach").as_deref() == Some("yes"))
                            .then(|| crate::update_apply::last_failure(&self.store))
                            .flatten();
                        match crate::feedback::compose_feedback(&words, attach, now) {
                            Ok(fb) => {
                                self.feedback_draft = Some(fb);
                                "Here it is, exactly as it will go.".to_string()
                            }
                            Err(why) => why,
                        }
                    }
                    "send" => match self.feedback_draft.take() {
                        Some(fb) => match crate::feedback::send_decided(&self.store, &self.peer_dir, fb) {
                            Ok(s) => s.plain(),
                            Err(why) => why,
                        },
                        None => "There's nothing waiting to send — write it and press Show first.".to_string(),
                    },
                    "discard" => {
                        self.feedback_draft = None;
                        "Not sent. Write it again as you'd like it.".to_string()
                    }
                    "answer" => {
                        let n = field_of(f, "n").and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
                        let version = field_of(f, "version").filter(|v| !v.trim().is_empty());
                        let status = crate::feedback::FeedbackStatus::from_words(&field_of(f, "status").unwrap_or_default(), version.as_deref());
                        match status {
                            None => "Fixed needs the version it was fixed in.".to_string(),
                            Some(st) => {
                                let note = field_of(f, "note").unwrap_or_default();
                                match crate::feedback::answer_feedback(&self.store, n, st.clone(), &note, now) {
                                    Ok(to) => format!("Marked {}; {to} will hear it the next time Atlas reaches them.", st.plain()),
                                    Err(why) => why,
                                }
                            }
                        }
                    }
                    _ => "That isn't something the Feedback page does.".to_string(),
                };
                hub::back_with(Page::Feedback.href(), "", &said)
            }
            "/hub/documents" => {
                let who = field_of(f, "who").unwrap_or_default();
                let said = match field_of(f, "id").and_then(|i| i.parse::<u64>().ok()) {
                    // Sending waits on their Atlas, a file of up to 20 MB over
                    // Tor: on the crew, with the page showing how it's going.
                    Some(id) if what == "send" => return self.send_document(id, &who, now),
                    Some(id) if what == "keep" || what == "drop" => self.take_handoff(id, what == "keep"),
                    _ => "That isn't something the Documents page does.".to_string(),
                };
                hub::back_with(Page::Documents.href(), "", &said)
            }
            "/hub/sync-setup" | "/hub/reclaim" => self.pages_post(path, f),
            "/hub/phonemodel" => {
                let said = self.phone_model_said(if what == "get" { "get" } else { "status" });
                hub::back_with(Page::Connections.href(), "", &said)
            }
            "/hub/draftmodel" => {
                let said = if what == "get" { self.get_draft_model() } else { "That button isn't wired to anything, so nothing changed.".into() };
                hub::back_with(Page::Connections.href(), "", &said)
            }
            // The two models: which one talks, and fetching them (`deepbrain`).
            "/hub/brains" => {
                let said = self.brains_button(&what);
                let back = if field_of(f, "from").as_deref() == Some("ideas") { Page::Recommendations } else { Page::Connections };
                hub::back_with(back.href(), "", &said)
            }
            // Another program's tools, on or off (`mcp`). Not while handed
            // over: they act as you.
            "/hub/mcp" => {
                let said = match self.handed_over_refusal(&Intent::McpTool(String::new())) {
                    Some(no) => no,
                    None => {
                        let server = field_of(f, "server").unwrap_or_default();
                        self.mcp_switch(&server, what == "on")
                    }
                };
                hub::back_with(Page::Connections.href(), "", &said)
            }
            _ => hub::back_with("/hub", "", "That button isn't wired to anything, so nothing changed."),
        }
    }
}
