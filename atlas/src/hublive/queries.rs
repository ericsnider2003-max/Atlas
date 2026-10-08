//! Answering a page asked with a query (hub_page_q).
//!
//! Moved out of `hublive.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Daemon<'_> {
    /// The pages the locked design added (`hubpages`), from live state.
    /// `q` is the address's query: which conversation, business or view, a
    /// line to say once (`said`), or what a phone's share sheet sent.
    pub(super) fn hub_page_q(&mut self, page: Page, q: &str) -> String {
        let now = crate::store::now();
        let off = crate::localclock::offset_secs();
        let fields = hub::form_fields(q);
        let said = field_of(&fields, "said");
        let when = |t: u64| {
            let today = crate::localclock::day_here(now);
            if crate::localclock::day_here(t) == today {
                crate::localclock::hhmm_here(t)
            } else {
                let (_, m, d) = crate::hubpages::ymd(crate::localclock::day_here(t));
                format!("{} {d}", crate::hubpages::MONTHS[(m - 1) as usize])
            }
        };
        match page {
            Page::Messages => {
                let pairings = crate::kin::Pairings::load(&self.peer_dir);
                let mut rooms: Vec<&crate::chat::Room> = self.chats.rooms.iter().collect();
                rooms.sort_by_key(|r| std::cmp::Reverse(r.messages.iter().map(|m| m.sent_at).max().unwrap_or(0)));
                let rows = rooms
                    .iter()
                    .map(|r| {
                        let last = r.in_order().last().map(|m| (*m).clone());
                        crate::hubpages::RoomRow {
                            id: r.id.clone(),
                            name: r.name.clone(),
                            area: area_of(&r.space),
                            last: last.as_ref().map(|m| m.body.chars().take(60).collect()).unwrap_or_default(),
                            when: last.as_ref().map(|m| when(m.sent_at)).unwrap_or_default(),
                            unread: r.unread().len(),
                            group: r.is_group(),
                        }
                    })
                    .collect();
                let open_id = field_of(&fields, "room").or_else(|| rooms.first().map(|r| r.id.clone()));
                let open = open_id.and_then(|id| self.chats.room(&id).cloned()).map(|r| {
                    let said: Vec<crate::hubpages::Said> = r
                        .in_order()
                        .iter()
                        .map(|m| {
                            let mine = m.from == crate::chat::ME;
                            let waiting = m.still_waiting();
                            let (state, held) = if !mine {
                                (String::new(), false)
                            } else if m.fully_read() {
                                ("Read".into(), false)
                            } else if m.fully_arrived() {
                                ("Delivered".into(), false)
                            } else {
                                (format!("Sent · held for {}", waiting.join(", ")), true)
                            };
                            crate::hubpages::Said {
                                mine,
                                from: m.from.clone(),
                                body: m.body.clone(),
                                when: when(m.sent_at),
                                state,
                                held,
                            }
                        })
                        .collect();
                    (r.id.clone(), r.name.clone(), area_of(&r.space), r.members.clone(), said)
                });
                // Reading a conversation here is reading it.
                if let Some((id, ..)) = &open {
                    if let Some(room) = self.chats.room_mut(id) {
                        let top = room.messages.iter().map(|m| m.after).max().unwrap_or(0);
                        if top > room.read_through {
                            room.read_through = top;
                            // Where you've read up to: small, but said when
                            // it can't be kept (28 Sep 2026).
                            if let Err(e) = self.chats.save(&self.store) {
                                self.log.info(&format!("couldn't keep where you've read up to in messages: {e}"));
                            }
                        }
                    }
                }
                let people = pairings.contacts.iter().map(|c| c.name.clone()).collect();
                crate::hubpages::messages_page(&crate::hubpages::MessagesView { rooms: rows, open, people, notice: said })
            }
            Page::Documents => {
                let mut items: Vec<&crate::tray::Item> = self.tray.items.iter().collect();
                items.sort_by_key(|i| std::cmp::Reverse(i.at));
                let docs: Vec<crate::hubpages::DocRow> = items
                    .iter()
                    .map(|i| crate::hubpages::DocRow {
                        id: i.id,
                        name: i.title(),
                        kind: i.sort.title().to_string(),
                        area: area_of(&i.space),
                        // Nothing handed to Atlas leaves the machine on its
                        // own; a send is its own act (the Send button), and
                        // each one is logged on the item.
                        shared: {
                            let mut who: Vec<String> = Vec::new();
                            for (w, _) in &i.shared {
                                if !who.contains(w) {
                                    who.push(w.clone());
                                }
                            }
                            match i.shared.iter().map(|(_, at)| *at).max() {
                                Some(last) => format!("Sent to {} · {}", who.join(", "), when(last)),
                                None => String::new(),
                            }
                        },
                        private: i.shared.is_empty(),
                        when: when(i.at),
                        state: match i.state {
                            crate::tray::State::Waiting => "Waiting to be read",
                            crate::tray::State::Read => "Read",
                            crate::tray::State::Stuck => "Couldn't read it",
                            crate::tray::State::Done => "Done with",
                        }
                        .into(),
                        photo: (i.sort == crate::tray::Sort::Image)
                            .then(|| i.stored_at.clone())
                            .flatten()
                            .filter(|p| std::path::Path::new(p).is_file()),
                    })
                    .collect();
                let people: Vec<String> =
                    crate::kin::Pairings::load(&self.peer_dir).contacts.iter().map(|c| c.name.clone()).collect();
                let page = crate::hubpages::documents_page(&docs, &people, said.as_deref());
                // What friends handed you, waiting for your yes. Before 27 Sep
                // 2026 the only way to take one in was a terminal command.
                let inbox = crate::household::Inbox::load(&self.store);
                if inbox.items.is_empty() {
                    page
                } else {
                    let mut w = String::from("<section aria-labelledby=fromfriends><h2 id=fromfriends>Waiting from friends</h2>\
                        <p class=note>Nothing here has been opened. Keep puts it with your documents, where Atlas may read it.</p><ul class=plainlist>");
                    for i in &inbox.items {
                        let file = i.file.as_ref().map(|f| format!(" ({}, {} KB)", f.name, f.size.div_ceil(1024))).unwrap_or_default();
                        w.push_str(&format!(
                            "<li><span>{}{}</span><span class=meta>from {} · {}</span>\
                             <form class=inline method=post action='/hub/documents'><input type=hidden name=what value=keep>\
                             <input type=hidden name=id value='{id}'><button>Keep</button></form>\
                             <form class=inline method=post action='/hub/documents'><input type=hidden name=what value=drop>\
                             <input type=hidden name=id value='{id}'><button class=revoke>Bin it</button></form></li>",
                            hub::esc(&i.what),
                            hub::esc(&file),
                            hub::esc(&i.from),
                            when(i.at),
                            id = i.id
                        ));
                    }
                    w.push_str("</ul></section>");
                    match page.rfind("</main>") {
                        Some(at) => format!("{}{w}{}", &page[..at], &page[at..]),
                        None => page,
                    }
                }
            }
            Page::Business => {
                let all = self.business_views(now);
                crate::hubpages::business_page(&all, field_of(&fields, "b").as_deref())
            }
            Page::SharedTasks => {
                let roster = crate::roster::Roster::load(&self.store);
                let businesses = roster.businesses();
                let tasks = crate::shared_task::Tasks::load(&self.store);
                let today = crate::localclock::day(now, off);
                let mut rows = Vec::new();
                for b in &businesses {
                    for t in tasks.for_space(&crate::earned::Space::Business(b.clone())) {
                        let due_day = t.due.map(|d| crate::localclock::day_here(d));
                        rows.push(crate::hubpages::TaskRow {
                            id: t.id,
                            what: t.description.clone(),
                            business: b.clone(),
                            due: t.due.map(when).unwrap_or_default(),
                            due_day,
                            done: t.done,
                            overdue: !t.done && t.due.map(|d| d < now).unwrap_or(false),
                            from_personal: t.shared_from_personal,
                        });
                    }
                }
                let view = crate::hubpages::TaskView::from_query(&field_of(&fields, "view").unwrap_or_default());
                crate::hubpages::shared_tasks_page(&rows, &businesses, field_of(&fields, "b").as_deref(), view, today)
            }
            Page::Clients => {
                let list = crate::clients::ClientList::load(&self.store);
                let sent = crate::outbox::Outbox::load(&self.store);
                let rows: Vec<crate::hubpages::ClientRow> = list
                    .all()
                    .iter()
                    .map(|c| {
                        let to_them = sent.sent_to(&c.address);
                        crate::hubpages::ClientRow {
                            address: c.address.clone(),
                            name: c.name_or_address().to_string(),
                            phone: c.phone.clone(),
                            notes: c.notes.clone(),
                            added: when(c.added_at),
                            sent: to_them.len(),
                            last_sent: to_them.iter().map(|r| r.created_at).max().map(when).unwrap_or_default(),
                        }
                    })
                    .collect();
                crate::hubpages::clients_page(&rows, field_of(&fields, "c").as_deref(), said.as_deref())
            }
            Page::Partners => {
                let roster = crate::roster::Roster::load(&self.store);
                let pairings = crate::kin::Pairings::load(&self.peer_dir);
                let mut people: std::collections::BTreeMap<String, Vec<String>> = Default::default();
                for b in roster.businesses() {
                    for m in roster.members(&b) {
                        people.entry(m).or_default().push(b.clone());
                    }
                }
                let rows: Vec<(String, Vec<String>, crate::hubpages::Reach, bool)> = people
                    .into_iter()
                    .map(|(p, bs)| {
                        use crate::hubpages::Reach;
                        let reach = if !pairings.has_peer(&p) {
                            Reach::NotPaired
                        } else {
                            match self.reached.last(&p) {
                                None => Reach::Unheard,
                                Some(at) if now.saturating_sub(at) < crate::kin::ONLINE_SECS => Reach::Online,
                                Some(at) => Reach::LastHeard(crate::freshness::ago(now.saturating_sub(at))),
                            }
                        };
                        let trusted = pairings.is_trusted(&p);
                        (p, bs, reach, trusted)
                    })
                    .collect();
                crate::hubpages::partners_page(&rows)
            }
            Page::Sound => {
                let cfg = self.tools_cfg();
                let root = crate::roots::install_root();
                let kokoro_chosen = cfg.tts_engine.engine == crate::tts::Engine::Kokoro;
                let kokoro_ready = crate::kokoro::check(&root).is_ok();
                let engine = crate::hubpages::EngineView {
                    kokoro_chosen,
                    kokoro_ready,
                    kokoro_here: crate::kokoro::runtime_piece().is_some(),
                    kokoro_mb: crate::kokoro::download_mb(),
                    getting: self.voice_downloads.state(crate::kokoro::DOWNLOAD_ID).map(|g| g.said()),
                    fell_back: crate::kokoro::last_note(),
                };
                // Under Kokoro, its shortlist (`tts::SHORTLIST`), which comes
                // with the one download; under piper, piper's catalogue.
                let kokoro_voices: Vec<crate::hubpages::VoiceRow> = if kokoro_chosen {
                    let (now, _) = crate::kokoro::voice_or_default(&cfg.voice_settings.voice);
                    crate::tts::SHORTLIST
                        .iter()
                        .map(|(id, why)| crate::hubpages::VoiceRow {
                            id: id.to_string(),
                            name: crate::kokoro::display_name(id),
                            what: format!("{} · {why}", crate::kokoro::accent(id)),
                            installed: kokoro_ready,
                            chosen: *id == now,
                            hear: false,
                            size: String::new(),
                            getting: None,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let voices = if kokoro_chosen { kokoro_voices } else { crate::tts::catalogue()
                    .into_iter()
                    .map(|v| {
                        // Where piper looks (under the install folder), not
                        // relative to wherever Atlas was started from.
                        let installed = std::path::Path::new(&cfg.tts_engine.voice_file_for(&v.id)).exists()
                            || cfg.tts_engine.engine.can_clone();
                        let source = crate::voicepick::source(&v.id);
                        crate::hubpages::VoiceRow {
                            chosen: v.id == cfg.voice_settings.voice,
                            what: format!("{} · {}", v.accent, v.character),
                            installed,
                            hear: source.is_some(),
                            size: source.map(crate::voicepick::size_said).unwrap_or_default(),
                            getting: self.voice_downloads.state(&v.id).map(|g| g.said()),
                            name: v.name,
                            id: v.id,
                        }
                    })
                    .collect() };
                let wake = cfg.wake.clone();
                let view = crate::hubpages::SoundView {
                    engine,
                    voices,
                    speed: cfg.voice_settings.speed,
                    speak_replies: cfg.sound.speak_replies.clone(),
                    volume: cfg.sound.volume,
                    muted: cfg.sound.muted,
                    wake_on: wake.as_ref().map(|w| w.enabled).unwrap_or(false),
                    wake_phrase: wake.map(|w| w.phrase).unwrap_or_else(|| "Atlas".into()),
                    ptt_on: cfg.push_to_talk.enabled,
                    ptt_key: cfg.push_to_talk.key.clone(),
                    typing_key: cfg.quick_input.hotkey.clone(),
                    quiet_on: cfg.sound.quiet_hours,
                    quiet_from: cfg.sound.quiet_from.clone(),
                    quiet_to: cfg.sound.quiet_to.clone(),
                    mics: self.audio_devices.as_ref().map(|d| d.iter().map(|x| x.name.clone()).collect()).unwrap_or_default(),
                };
                crate::hubpages::sound_page(&view, said.as_deref())
            }
            Page::Trusted => {
                let pairings = crate::kin::Pairings::load(&self.peer_dir);
                let people: Vec<(String, bool)> =
                    pairings.contacts.iter().map(|c| (c.name.clone(), pairings.is_trusted(&c.name))).collect();
                crate::hubpages::trusted_page(&people, said.as_deref())
            }
            Page::Give => {
                // What a phone's share sheet sent is taken in `hub_answer`,
                // which then moves the browser on; here it's only the page.
                let notice = said;
                let mut items: Vec<&crate::tray::Item> = self.tray.items.iter().collect();
                items.sort_by_key(|i| std::cmp::Reverse(i.at));
                let recent: Vec<(String, String, String, String)> = items
                    .iter()
                    .take(8)
                    .map(|i| {
                        (
                            i.title(),
                            i.sort.title().to_string(),
                            when(i.at),
                            match i.state {
                                crate::tray::State::Waiting => "waiting to be read",
                                crate::tray::State::Read => "read",
                                crate::tray::State::Stuck => "couldn't read it",
                                crate::tray::State::Done => "done with",
                            }
                            .to_string(),
                        )
                    })
                    .collect();
                // `draft=`: words put in the box for you to look at and send
                // yourself -- what the phone apps' links and share open
                // (28 Sep 2026). Only showing them; nothing is handed over
                // until you press the button.
                let draft = field_of(&fields, "draft");
                crate::hubpages::give_page(&recent, notice.as_deref(), draft.as_deref())
            }
            Page::Offline => {
                let online = self.connectivity.status_now(now) == crate::connectivity::Reach::Online;
                let mut waiting: Vec<(String, String)> = Vec::new();
                for (room, m) in self.chats.outbox() {
                    let name = self.chats.room(room).map(|r| r.name.clone()).unwrap_or_default();
                    waiting.push((format!("A message to {name}"), when(m.sent_at)));
                }
                for r in crate::outbox::Outbox::load(&self.store).waiting() {
                    waiting.push((format!("A reply to {}", r.to_name), when(r.created_at)));
                }
                for t in self.queue.tasks.iter().filter(|t| t.needs_net && !matches!(t.state, crate::lanes::TaskState::Done | crate::lanes::TaskState::Failed)) {
                    waiting.push((sentence(&t.command), when(t.created)));
                }
                let live = vec![
                    "Your calendar".to_string(),
                    "Your files, notes and documents".to_string(),
                    "Drafting and writing".to_string(),
                    "Projects and builds, in a copy".to_string(),
                    "Voice, on this machine".to_string(),
                ];
                crate::hubpages::offline_page(&crate::hubpages::OfflineView { online, live, waiting })
            }
            Page::Talk => {
                let ex: Vec<(String, String)> = self
                    .thread
                    .recent
                    .iter()
                    .rev()
                    .take(12)
                    .rev()
                    .map(|e| (e.said.clone(), e.reply.clone()))
                    .collect();
                // The page's own hold-to-talk shows only inside the phone app,
                // which listens on the phone; the laptop has its keys.
                let pending: Vec<String> = self.talk_queue.iter().map(|(s, _)| s.clone()).collect();
                let page = crate::hubpages::talk_page(&ex, &pending, false);
                // Texts Atlas wrote, with the button that opens Messages.
                let texts = crate::texting::Texts::load(&self.store);
                let card = crate::texting::card(&texts.current(crate::store::now()));
                let page = if card.is_empty() { page } else { page.replacen("<form class=compose", &format!("{card}<form class=compose"), 1) };
                // The reply so far, in place of "thinking…", while the model
                // is still writing it (27 Sep 2026: replies stream now). The
                // page already looks again every two seconds.
                let so_far = if self.talk_is_thinking() { self.talk_so_far() } else { String::new() };
                if so_far.trim().is_empty() {
                    page
                } else {
                    page.replacen(
                        "<span class=note>thinking…</span>",
                        &format!("{}<span class=note> …</span>", hub::esc(so_far.trim())),
                        1,
                    )
                }
            }
            Page::Help => {
                let reviewed = {
                    let (y, m, d) = crate::hubpages::ymd(crate::localclock::day(now, off));
                    format!("{d} {} {y}", crate::hubpages::MONTHS[(m - 1) as usize])
                };
                crate::hubpages::help_page(&reviewed, said.as_deref())
            }
            Page::Workshop => {
                let page = hub::workshop_page(&self.workshop);
                let notice = said.map(|s| format!("<p class=notice role=status>{}</p>", hub::esc(&s))).unwrap_or_default();
                let form = crate::hubpages::new_project_form();
                match page.rfind("</main>") {
                    Some(at) => format!("{}{notice}{form}{}", &page[..at], &page[at..]),
                    None => page,
                }
            }
            Page::Connections => {
                let page = hub::list_page_at(
                    Some(Page::Connections),
                    "Connections",
                    "Whether the things I rely on are answering.",
                    &self.connection_lines(now),
                );
                // The one list of everything connected leads the page (N1).
                let page = hub::with_block_after_heading(page, &crate::connecting::rows::html(self));
                let page = with_block(page, &phone_model_block(said.as_deref()));
                let page = with_block(page, &self.mcp_block());
                let page = with_block(page, &self.brains_block());
                with_block(page, &self.draft_block())
            }
            Page::Updates => {
                let mut v = self.updates_view(now, field_of(&fields, "confirm").as_deref() == Some("undo"));
                let mut said = said;
                // A release key just made: its recovery key, this once.
                if let Some(crate::hubjobs::Flash::ReleaseKey { card, recovery, said: made }) =
                    crate::hubjobs::take_flash(&mut self.flash_once, Page::Updates, now)
                {
                    v.key = crate::hubpages::ReleaseKey::JustMade { card, recovery };
                    said = Some(made);
                }
                crate::hubpages::updates_page(&v, said.as_deref())
            }
            Page::Feedback => crate::hubpages::feedback_page(&self.feedback_view(), said.as_deref()),
            Page::Opportunities => crate::hunting::opportunities_page(self, &fields),
            Page::Accounts => {
                let page = self.hub_page(Page::Accounts);
                let asked = field_of(&fields, "connect");
                let page = match asked.as_deref() {
                    // The plain page has the section already; this one says the next step too.
                    Some(a) => page.replacen(&crate::connecting::section(self, None), &crate::connecting::section(self, Some(a)), 1),
                    None => page,
                };
                hub::with_said(page, said.as_deref())
            }
            // What "Have a go" or "Not worth it" did, said on the page -- the
            // same as every other page (below).
            Page::Social => self.social_page(said.as_deref()),
            Page::Phone => {
                let kind = field_of(&fields, "kind").and_then(|k| crate::phoneadd::Kind::parse(&k));
                let v = self.phone_view(kind);
                crate::hubpages::phone_page(&v, said.as_deref())
            }
            // Any other page: what a button just did, said under its heading.
            other => hub::with_said(self.hub_page(other), said.as_deref()),
        }
    }
}
