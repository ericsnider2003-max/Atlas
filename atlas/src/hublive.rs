//! The hub, served by the Atlas that is actually running.
//!
//! Every page except Settings and Access used to answer *"needs the full Atlas
//! running. This is settings-only mode."* — and there was no other mode. One
//! serve loop existed, in `run_hub`, with no daemon behind it. So `Now`,
//! `Outstanding`, `What I did`, `Connections` and the rest were written,
//! tested, routed, rendered and permanently empty. That is the `hollow`
//! pattern at the size of a whole feature: each part worked, and the thing
//! made of them did nothing.
//!
//! This module is the join. It takes a live `Daemon` and answers a hub
//! request from what that daemon actually knows right now — no cache, no
//! second copy of the state, no placeholder text.
//!
//! ## Nothing here may print like code
//!
//! Debug formatting (`{:?}`) on an internal enum reaches the screen as
//! `LookingBack` or `Kind::Upkeep`. That is a variable name leaking into a
//! product. Every string that reaches a page comes from a `plain()`-style
//! method written for a person, and `tests/hub_is_not_code.rs` fails the
//! build if debug formatting shows up in a page again.

use crate::dash::Card;
use crate::intent::Intent;
use crate::daemon::Daemon;
use crate::hub::{self, Page};
use crate::server::{Action, Reply};
use crate::daemon::local_offset_mins;

/// Atlas's own ideas you said weren't worth it (the Improvements page).
const RECS_DROPPED: &str = "self_audit_dropped";

/// Answer one hub request from live state.
///
/// A free function rather than a method so the daemon's call site names this
/// module out loud. An inherent `impl` on `Daemon` in a second file is
/// invisible to the wiring guard — the module would read as unreachable while
/// being the thing that serves every page.
/// A command as a sentence for the deck: capital first, full stop last.
fn sentence(said: &str) -> String {
    let t = said.trim();
    let mut c = t.chars();
    let mut out = match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => return String::new(),
    };
    if !out.ends_with(['.', '?', '!']) {
        out.push('.');
    }
    out
}

pub fn reply(daemon: &mut Daemon, action: Action) -> Reply {
    daemon.hub_reply(action)
}

impl Daemon<'_> {
    /// Answer one hub request from live state.
    fn hub_reply(&mut self, action: Action) -> Reply {
        match action {
            // Every page, from one place. See `hub::with_palette`.
            Action::Hub(page) => self.hub_answer(page, ""),
            Action::HubQ(page, q) => self.hub_answer(page, &q),
            Action::HubBack(page, said) => hub::back_with(page.href(), "", &said),
            Action::HubPost { path, fields } => self.hub_post(&path, &fields),
            // The pages that replaced terminal commands (`hubvault`).
            Action::Vault { what, old, new, again, nonce } => self.vault_post(&what, &old, &new, &again, &nonce),
            Action::TakeBack { phrase, nonce } => self.take_back_post(&phrase, &nonce),
            Action::SyncKeySet { phrase, replace } => self.sync_key_set_post(&phrase, replace),
            Action::HouseholdInit { name, device, key } => self.household_init_post(&name, &device, key),
            // Inviting, joining and a new or written-out household key hand
            // out or change what makes a device yours: not while handed over.
            Action::SyncKey(_) | Action::SyncJoin { .. } if self.handed_over_now() => self
                .sync_refused_while_handed_over("the household key")
                .unwrap_or_else(|| Reply::redirect(Page::Sync.href())),
            Action::Changed(which) => {
                let (v, busy) = self.live_page_state(&which);
                Reply::ok(&serde_json::json!({ "v": v.to_string(), "busy": busy }).to_string())
            }
            Action::LiveJson => {
                let now = crate::store::now();
                let off = crate::localclock::offset_secs();
                let deck = self.deck(now, off);
                let working = self.mind.focus().map(|w| {
                    serde_json::json!({
                        "title": sentence(&w.asked),
                        "step": w.thoughts.last().map(|t| sentence(&t.text)).unwrap_or_default(),
                        "stage": w.stage.label(),
                        "started": w.started,
                    })
                });
                let ready: Vec<serde_json::Value> = deck
                    .asks
                    .iter()
                    .map(|(what, href)| serde_json::json!({ "title": what, "href": href }))
                    .collect();
                let body = serde_json::json!({
                    "status": deck.status,
                    "working": working,
                    "ready": ready,
                    "waiting": self.waiting_count(now),
                    "brief": deck.brief,
                    // What the background said (a reminder, a finished job),
                    // numbered: an app shows each new one as a notification.
                    "said": self.said_for_apps.iter().map(|(id, text)| serde_json::json!({ "id": id, "text": text })).collect::<Vec<_>>(),
                });
                Reply::ok(&body.to_string())
            }
            Action::VoiceSample(id) => match self.voice_sample(&id) {
                Ok(bytes) => Reply::media("audio/mpeg", bytes),
                Err(why) => Reply { status: 404, body: serde_json::json!({ "error": why }).to_string(), ..Reply::default() },
            },
            Action::PhoneCalendar(body) => {
                let now = crate::store::now();
                Reply::ok(&self.phone_calendar(&body, now).to_string())
            }
            Action::TalkJson => {
                let recent: Vec<serde_json::Value> = self
                    .thread
                    .recent
                    .iter()
                    .rev()
                    .take(12)
                    .rev()
                    .map(|e| serde_json::json!({ "said": e.said, "reply": e.reply }))
                    .collect();
                let pending: Vec<&String> = self.talk_queue.iter().map(|(s, _)| s).collect();
                Reply::ok(&serde_json::json!({ "recent": recent, "pending": pending, "thinking": self.talk_is_thinking() }).to_string())
            }
            Action::GlanceJson => {
                let now = crate::store::now();
                let g = self.glance(now);
                Reply::ok(&serde_json::to_string(&g).unwrap_or_default())
            }
            Action::Pause(on) => {
                // The same path as saying it, so a paused Atlas from the hub
                // is exactly as paused as one told out loud.
                let _ = self.turn(if on { "pause" } else { "carry on" }, crate::store::now());
                hub::back_with(
                    Page::Now.href(),
                    "",
                    if on {
                        "Paused. Nothing new starts, and the microphone is off, until you resume."
                    } else {
                        "Carrying on. The microphone is back on."
                    },
                )
            }
            Action::Appearance { what, to } => {
                let mut a = self.appearance();
                if a.choose(&what, &to) {
                    if let Err(e) = self.store.save(crate::hub::APPEARANCE_KEY, &a) {
                        self.log.info(&format!("couldn't keep that appearance choice: {e}"));
                    }
                } else if let Some(done) = crate::appearance::choose(&what, &to) {
                    // From Settings → How it looks. A colourway chosen there
                    // clears the Aa menu's theme, which would otherwise win.
                    match done {
                        Ok(_) if what == "look.theme" && !a.theme.is_empty() => {
                            a.theme.clear();
                            if let Err(e) = self.store.save(crate::hub::APPEARANCE_KEY, &a) {
                                self.log.info(&format!("couldn't keep that appearance choice: {e}"));
                            }
                        }
                        Ok(_) => {}
                        Err(e) => self.log.info(&format!("couldn't keep that appearance choice: {e}")),
                    }
                    return Reply::redirect(&format!("{}#how-it-looks", crate::hub::Page::Settings.href()));
                }
                Reply::redirect("/hub")
            }
            Action::DashArrange(on) => {
                self.arranging = on;
                Reply::redirect("/hub")
            }
            Action::DashMove(m) => {
                // Only write when something actually moved. A file rewritten
                // on every click is the file that gets corrupted on the one
                // click during a power cut.
                if self.dashboard.apply(&m) {
                    // Said when it didn't stick (28 Sep 2026): the move is
                    // held in memory and would be gone at the next start,
                    // while the page showed it done.
                    if let Err(e) = self.dashboard.save(&self.store) {
                        self.log.info(&format!("couldn't keep the dashboard's arrangement: {e}"));
                        return hub::back_with("/hub", "", &with_keeping("Moved.".into(), Err(e)));
                    }
                }
                Reply::redirect("/hub")
            }
            Action::Find(query) => {
                let mut palette_unsaved: Option<crate::error::AtlasError> = None;
                let entries = crate::palette::catalogue();
                let hits = crate::palette::find(&entries, &query, &self.palette);
                // Picking is what teaches it, and the plain page is the one
                // place a pick is observable server-side. The overlay's own
                // links are ordinary navigation, so a pick there shows up as
                // the page you landed on rather than as a palette event —
                // which is why the memory is deliberately a small nudge and
                // never the thing that decides an order.
                if let Some(first) = hits.first() {
                    if !query.trim().is_empty() {
                        self.palette.picked(first.id());
                        // Only a nudge to the order, so the page still
                        // answers; but a failed save is said in the log and
                        // on the page rather than quietly forgotten at the
                        // next start (28 Sep 2026).
                        if let Err(e) = self.palette.save(&self.store) {
                            self.log.info(&format!("couldn't keep what you picked in the palette: {e}"));
                            palette_unsaved = Some(e);
                        }
                    }
                }
                let maybe = if hits.is_empty() && !query.trim().is_empty() {
                    crate::palette::did_you_mean(&entries, &query)
                } else {
                    None
                };
                let mut html = hub::find_page(&query, &hits, maybe);
                if let Some(e) = palette_unsaved {
                    html = crate::hub::with_said(html, Some(&format!(
                        "I couldn't save what you picked ({e}), so the order it learned is only here until I next start."
                    )));
                }
                let html = crate::hub::with_palette(html, &entries, &self.palette);
                let html = crate::hub::with_waiting(html, self.waiting_count(crate::store::now()));
                let html = self.with_sidebar_names(html);
                Reply::html(crate::hub::with_appearance(html, &self.appearance()))
            }
            Action::Hand { what, space, from, asked } => {
                let space = space_named(space);
                let said = match self.tray.hand(&what, &space, &from, crate::store::now()) {
                    Ok(id) => {
                        if let Some(a) = asked.as_deref() {
                            self.tray.ask_about(id, a);
                        }
                        let said = match asked {
                            Some(a) if !a.trim().is_empty() => {
                                format!("Got it. I'll read it and tell you: {}", a.trim())
                            }
                            _ => "Got it. I'll read it and tell you what's in it.".to_string(),
                        };
                        with_keeping(said, self.tray.save(&self.store))
                    }
                    Err(why) => why,
                };
                // JSON, not a page: this is answered to a phone, which wants a
                // line to show in a share sheet rather than a dashboard.
                Reply::ok(&format!(
                    "{{\"said\":{}}}",
                    serde_json::to_string(&said).unwrap_or_else(|_| "\"\"".into())
                ))
            }
            Action::HandFile { name, base64, space, from, asked } => {
                let space = space_named(space);
                let said = match crate::tray::from_base64(&base64) {
                    Err(why) => why,
                    Ok(bytes) => match self.tray.hand_file(
                        &name,
                        &bytes,
                        &space,
                        &from,
                        asked.as_deref(),
                        crate::store::now(),
                        self.store.root(),
                    ) {
                        Ok(_) => with_keeping(
                            format!(
                                "Got your {}. I'll look at it and tell you what's in it.",
                                crate::tray::Sort::of_file(&name).title().to_lowercase()
                            ),
                            self.tray.save(&self.store),
                        ),
                        Err(why) => why,
                    },
                };
                Reply::ok(&format!(
                    "{{\"said\":{}}}",
                    serde_json::to_string(&said).unwrap_or_else(|_| "\"\"".into())
                ))
            }
            Action::ExportCalendar => {
                let now = crate::store::now();
                Reply::file("calendar.ics", "text/calendar; charset=utf-8", self.calendar.to_ics(now))
            }
            Action::ExportClients => {
                let list = crate::clients::ClientList::load(&self.store);
                Reply::file("clients.vcf", "text/vcard; charset=utf-8", list.to_vcf())
            }
            Action::BringIn { name, base64 } => {
                let said = match crate::tray::from_base64(&base64) {
                    Err(why) => why,
                    Ok(bytes) => self.bring_in(&name, &bytes),
                };
                Reply::ok(&format!(
                    "{{\"said\":{}}}",
                    serde_json::to_string(&said).unwrap_or_else(|_| "\"\"".into())
                ))
            }
            Action::TrayDone(id) => {
                let said = if !self.tray.done(id) {
                    "That was already done with."
                } else if self.tray.save(&self.store).is_err() {
                    "Done with it here, but I couldn't keep that, so it may come back."
                } else {
                    "Done with it."
                };
                hub::back_with(Page::Dashboard.href(), "", said)
            }
            Action::Implement(title) => {
                // The implement button is the same act as saying "implement
                // <title>", so it goes through the same method.
                // What happened, said on the page -- "don't know the folder",
                // "nothing written" -- rather than thrown away.
                let said = self.implement_change(&title);
                hub::back_with(Page::Workshop.href(), "", &said)
            }
            Action::Account(change) => {
                use crate::accounts::Change;
                let said = if !self.accounts.apply(&change) {
                    "Nothing changed.".to_string()
                } else if let Err(e) = self.accounts.save(&self.store) {
                    format!("I couldn't keep that: {e}")
                } else {
                    match &change {
                        Change::Note(site) => format!("Noted {site}."),
                        Change::Forget(site) => format!("Forgot {site}."),
                        Change::Factor(site, _) | Change::Reused(site, _) => format!("Noted that about {site}."),
                    }
                };
                hub::back_with(Page::Accounts.href(), "", &said)
            }
            Action::SyncKey(what) => {
                let now = crate::store::now();
                // Some of these carry a secret (an invitation code, a key's
                // words), so none goes in the address: it's shown once on
                // the Sync page (`hubjobs::Flash`).
                let said = match what.as_str() {
                    "new" => match crate::sync::new_key(&self.store, now) {
                        Ok(setup) => {
                            self.log.info("made a new household key from the hub");
                            match &setup.card {
                                Some(path) => format!(
                                    "New key made. Your devices will start using it the next \
                                     time they carry anything, and everything they hold goes \
                                     with it. Written down in {}.",
                                    path.display()
                                ),
                                None => format!(
                                    "New key made: {}. I couldn't write the card file, so \
                                     that is the only place it is shown.",
                                    setup.phrase
                                ),
                            }
                        }
                        Err(why) => why,
                    },
                    "pair" => {
                        let house = crate::household::Household::load(&self.store);
                        let folder = self.tools_cfg().sync.folder.clone();
                        if !house.is_set() {
                            // Pointed at the page since 27 Sep 2026; this
                            // named the terminal command before.
                            "There's no household yet, so there's nothing to invite a device \
                             into. \u{201c}Start one here\u{201d}, on this page, makes one."
                                .to_string()
                        } else if folder.trim().is_empty() {
                            "Set a sync folder first -- the invitation goes there, and it is \
                             what the other machine reads it from."
                                .to_string()
                        } else {
                            let code = crate::household::new_invite_code();
                            let kept: crate::sync::KeptKey =
                                self.store.load(crate::sync::KEY_FILE);
                            let phrase = kept.is_set().then(|| kept.phrase().ok()).flatten();
                            match crate::household::leave_invitation(
                                std::path::Path::new(folder.trim()),
                                &house.id,
                                &house.name,
                                &code,
                                phrase.as_deref(),
                                now,
                                crate::household::INVITE_WAIT_SECS,
                            ) {
                                Ok(_) => format!(
                                    "Type this on the other machine within fifteen minutes: \
                                     {code}{}",
                                    if phrase.is_some() {
                                        " — the household key goes with it."
                                    } else {
                                        ""
                                    }
                                ),
                                Err(why) => why,
                            }
                        }
                    }
                    _ => {
                        let kept: crate::sync::KeptKey = self.store.load(crate::sync::KEY_FILE);
                        match kept.phrase().and_then(|p| crate::sync::write_card(&p)) {
                            Ok(path) => format!("Written down again in {}.", path.display()),
                            Err(why) => why,
                        }
                    }
                };
                crate::hubjobs::keep_flash(&mut self.flash_once, Page::Sync, crate::hubjobs::Flash::Said(said), now);
                Reply::redirect(Page::Sync.href())
            }
            Action::SyncJoin { code, device } => {
                let now = crate::store::now();
                let folder = self.tools_cfg().sync.folder.clone();
                // The form wins, and `household.device_name` is what it is
                // pre-filled with and what it falls back to. Until 19 Sep
                // 2026 nothing read that setting at all, so a person who had
                // named this machine in their config was asked to name it
                // again on a phone keyboard.
                let this_machine = if device.trim().is_empty() {
                    self.tools_cfg().household.device_name.trim().to_string()
                } else {
                    device.trim().to_string()
                };
                let mine = crate::household::Household::load(&self.store);
                let said = if folder.trim().is_empty() {
                    "Set a sync folder first -- the invitation is waiting in the one the \
                     other machine used."
                        .to_string()
                } else if this_machine.is_empty() {
                    "Give this machine a name as well, so the other one knows what joined. \
                     Name it once on the Sync page and you won't be asked again."
                        .to_string()
                } else {
                    match crate::household::take_invitation(
                        std::path::Path::new(folder.trim()),
                        &code,
                        now,
                    ) {
                        Err(why) => why,
                        Ok(inside) if mine.is_set() && mine.id != inside.for_household => format!(
                            "This device already belongs to {} -- joining {} would mean two \
                             households on one machine.",
                            mine.name, inside.name
                        ),
                        Ok(inside) => {
                            let joined = crate::household::Household {
                                id: inside.for_household.clone(),
                                name: inside.name.clone(),
                                made_at: now,
                                devices: vec![this_machine.clone()],
                            };
                            match joined.save(&self.store) {
                                Err(e) => format!("Couldn't save that: {e}"),
                                Ok(()) => {
                                    let mut said =
                                        format!("Joined {}.", joined.name);
                                    if let Some(phrase) = inside.key_phrase {
                                        let keeping =
                                            crate::sync::KeptKey::keeping(&phrase, now);
                                        match self.store.save(crate::sync::KEY_FILE, &keeping) {
                                            Ok(()) => {
                                                let _ = crate::sync::write_card(&phrase);
                                                said.push_str(
                                                    " The household key came with it, so \
                                                     sealed bundles from the other machine \
                                                     open here.",
                                                );
                                            }
                                            Err(e) => said.push_str(&format!(
                                                " I got the key and couldn't keep it: {e}"
                                            )),
                                        }
                                    }
                                    said
                                }
                            }
                        }
                    }
                };
                // Nothing secret is said here -- the key itself never is.
                hub::back_with(Page::Sync.href(), "", &said)
            }
            Action::HubSet { key, value } => {
                // This used to mutate a `Settings` that was rebuilt per
                // request and dropped at the end of the handler, log "Voice
                // is now on", and redirect to a page that re-rendered from
                // the unchanged config. Every toggle on this page reported
                // success and wrote nothing -- including the ones marked
                // `Permission` and `Sensitive`.
                //
                // Validated first, then written. Validating first matters:
                // `Settings::set` is what knows a toggle from a number from
                // a name, and writing an unparseable value into the settings
                // file would turn a switch that did nothing into one that
                // stops Atlas starting.
                let said = self.apply_setting(&key, &value);
                self.log.info(&said);
                // Back to the setting itself, saying what happened -- a
                // value it wouldn't take included.
                hub::back_with(&format!("{}#set-{key}", Page::Settings.href()), "", &said)
            }
            // Taking access away, which is what the access page has always
            // said it does. The buttons were rendered and posted to routes
            // that did not exist -- a revoke button that does nothing is
            // worse than no button, because you press it and believe it.
            //
            // Immediate and saved at once rather than at the next persist: a
            // revoke that is still in memory when the machine goes down is a
            // revoke that did not happen, and this is the one page where that
            // matters.
            Action::RevokeAccess(domain) => {
                let gone = self.access.revoke(&domain);
                let said = if !gone {
                    format!("Atlas didn't have access to {domain}, so there was nothing to take away.")
                } else {
                    self.log.info(&format!("access to {domain} taken away"));
                    match self.access.save(&self.store) {
                        Ok(()) => format!("Took away access to {domain}."),
                        Err(e) => format!("Took away access to {domain} for now, but I couldn't keep that ({e}), so it comes back when Atlas restarts. Try again."),
                    }
                };
                hub::back_with(Page::Access.href(), "", &said)
            }
            Action::RevokeAllAccess => {
                let n = self.access.revoke_all();
                let said = if n == 0 {
                    "Atlas didn't have access to any sites, so there was nothing to take away.".to_string()
                } else {
                    self.log.info(&format!("{n} site logins taken away"));
                    let sites = if n == 1 { "one site".to_string() } else { format!("{n} sites") };
                    match self.access.save(&self.store) {
                        Ok(()) => format!("Took away access to {sites}."),
                        Err(e) => format!("Took away access to {sites} for now, but I couldn't keep that ({e}), so it comes back when Atlas restarts. Try again."),
                    }
                };
                hub::back_with(Page::Access.href(), "", &said)
            }
            // Saved at once by the functions themselves, and read fresh by
            // every step an add-on runs -- so a permission taken away here
            // stops the add-on at its next step, even mid-sequence.
            // Sharing and recommending talk to other people, so they are the
            // running Atlas's to do.
            Action::AddOn { what, id, key, .. } if what == "recommend" => {
                let said = self.recommend_addon(&id, &key);
                self.log.info(&format!("add-ons: {said}"));
                hub::back_with(Page::AddOns.href(), "", &said)
            }
            // Sharing waits on each person's Atlas, over Tor if need be: on
            // the crew, so the page and the rest of Atlas don't wait with it.
            Action::AddOn { what, id, key, .. } if what == "share" => match self.prepare_addon_share(&id, &key) {
                Err(why) => hub::back_with(Page::AddOns.href(), "", &why),
                Ok(share) => {
                    let label = format!("Sharing \"{}\"", share.name);
                    let (name, group) = (share.name.clone(), share.group.clone());
                    self.hub_errand(
                        "addon-share",
                        Page::AddOns,
                        "",
                        &label,
                        Box::new(move || match share.send() {
                            Err(why) => (Err(why), String::new()),
                            Ok((reached, missed)) => (
                                Ok(crate::daemon::addon_share_said(&share.name, share.group.as_ref(), &reached, &missed)),
                                String::new(),
                            ),
                        }),
                        |job| crate::daemon::HubAfter::AddonShare { job, name, group },
                    )
                }
            },
            Action::AddOn { what, id, key, sha } => {
                let done = crate::plugins::hub_action(
                    &self.store,
                    &self.plugins_dir,
                    &self.cfg.commands,
                    &self.trash,
                    &what,
                    &id,
                    &key,
                    &sha,
                );
                if let Ok(said) = &done {
                    self.log.info(&format!("add-ons: {said}"));
                }
                hub::after_button(Page::AddOns, done)
            }
            // Saved at once; the members are sent the new list on this same
            // tick's upkeep, and every one of their Atlases checks your
            // signature on it.
            // Every friend button shows the page again with what it did on
            // it: a link made is shown once, right there.
            // Every friend button sends you back to the page (so a refresh
            // doesn't press it again) with what it did said there. A link
            // made is shown once and never put in the address: it's a
            // one-time key to this Atlas.
            Action::Friend { what, who, link } => {
                let said = match what.as_str() {
                    "link" => match self.friend_link() {
                        Ok(l) => {
                            crate::hubjobs::keep_flash(
                                &mut self.flash_once,
                                Page::Friends,
                                crate::hubjobs::Flash::FriendLink(l),
                                crate::store::now(),
                            );
                            return Reply::redirect(Page::Friends.href());
                        }
                        Err(e) => e,
                    },
                    // Adding knocks on their Atlas, which can take a while:
                    // on the crew. The link typed is read before, never after.
                    "add" => return self.add_friend_from_hub(&link),
                    "accept" => match self.take_friend_request(&who) {
                        Ok(link) => return self.add_friend_from_hub(&link),
                        Err(said) => said,
                    },
                    "decline" => self.decline_friend_request(&who),
                    "request" => self.send_friend_request(&who),
                    "forget" => self.unfriend(&who),
                    _ => "That button isn't wired to anything, so nothing changed.".to_string(),
                };
                hub::back_with(Page::Friends.href(), "", &said)
            }
            Action::GroupChange { what, group, .. } if what == "adopt" => {
                let done = self.adopt_group(&group);
                hub::after_button(Page::Groups, done)
            }
            Action::GroupChange { what, group, who, role } => {
                let done = crate::groups::act(&self.store, &self.peer_dir, &what, &group, &who, &role);
                if done.is_ok() {
                    self.settle_owned_groups();
                }
                hub::after_button(Page::Groups, done)
            }
            Action::ForgetEdit { file, path } => hub::after_button(
                Page::Edits,
                crate::yourchanges::forget(&crate::roots::config_dir(), &file, &path)
                    .map(|_| "Put back to how it shipped.".to_string()),
            ),
            // The plain API another Atlas reads (`elsewhere::ask`: "how's the
            // homelab Atlas?"). These were routed and then fell through to
            // "That isn't a page", so every check-in read HTML as its answer
            // (2 Oct 2026).
            Action::Health => Reply::ok("ok"),
            Action::Status => Reply::ok(self.api_status()),
            Action::Outstanding => Reply::ok(
                crate::brief::from_backlog(&self.backlog).iter().map(|i| i.headline()).collect::<Vec<_>>().join("\n"),
            ),
            Action::Queued => Reply::ok(self.on_queued()),
            Action::Say(text) => {
                let _ = self.hub_post("/hub/talk", &[("text".to_string(), text)]);
                Reply::ok("heard")
            }
            // Everything else on this port belongs to the API, not the hub.
            _ => Reply::html(hub::shell(
                "Atlas",
                "<p class=note>That isn't a page.</p>",
            )),
        }
    }

    /// One line: running, what's in hand, what's waiting.
    fn api_status(&self) -> String {
        let doing = self.crew.active();
        let waiting = self.crew.queued();
        let paused = if self.attention.is_paused() { " Paused." } else { "" };
        format!("Running. {doing} errand{} in hand, {waiting} waiting.{paused}", if doing == 1 { "" } else { "s" })
    }

    fn hub_page(&mut self, page: Page) -> String {
        let now = crate::store::now();
        match page {
            Page::Dashboard => {
                let bodies = self.dashboard_cards(now);
                let waiting = self.waiting_count(now);
                let deck = self.deck(now, crate::localclock::offset_secs());
                hub::dashboard_deck(&self.dashboard, &bodies, self.arranging, waiting, &deck)
            }
            Page::Now => self.now_page_live(),
            Page::Messages
            | Page::Documents
            | Page::Business
            | Page::SharedTasks
            | Page::Clients
            | Page::Partners
            | Page::Sound
            | Page::Trusted
            | Page::Give
            | Page::Offline
            | Page::Talk
            | Page::Help
            | Page::Updates
            | Page::Feedback
            | Page::Social
            | Page::Opportunities
            | Page::Phone => self.hub_page_q(page, ""),
            Page::Gestures => {
                // What he has taught, or the defaults read off his recordings
                // if he has not taught anything yet.
                let cfg = self.tools_cfg();
                let mine = self.gestures.gestures.clone();
                let showing = if mine.is_empty() {
                    crate::handshape::as_demonstrated()
                } else {
                    mine
                };
                hub::gestures_page(
                    &showing,
                    &crate::handshape::needs_deciding(),
                    cfg.answering.accept_gestures,
                    cfg.answering.gestures_may_approve_anything,
                )
            }
            Page::Workspace => self.workspace_page_live(now),
            Page::Workshop => self.hub_page_q(Page::Workshop, ""),
            Page::Calendar => hub::calendar_page(&self.calendar, now, &self.home_zone()),
            Page::Outstanding => hub::outstanding_page(&self.open_work(now)),
            Page::Activity => hub::list_page_at(
                Some(Page::Activity),
                "What I did",
                "Everything I did without being watched.",
                &self.activity_lines(now),
            ),
            Page::LookingBack => {
                let day = crate::workspace_view::day_of(&self.workspace, now);
                hub::looking_back_page(&day, "Today")
            }
            // One page, with or without a notice (30 Sep 2026: opened
            // plainly, it showed neither the other programs' tools, the
            // helper model nor the two models -- only the page a button
            // came back to did).
            Page::Connections => self.hub_page_q(Page::Connections, ""),
            Page::Recommendations => {
                self.refresh_signals();
                let cfg = self.tools_cfg().self_audit.clone();
                let recs = self.recommendations_shown(cfg.most_at_once);
                // Ways Atlas can get better on the hardware he already has.
                // `improve` listed them and nothing ever asked.
                let free = self.free_wins();
                // How you talk, and what went wrong this week (2 Oct 2026,
                // `learning`).
                with_block(hub::recommendations_page(&recs, None, &free), &self.how_you_talk_block(now))
            }
            Page::Status => {
                let settings = crate::settings::registry(&self.tools_cfg());
                // `diagnose` checks Atlas on itself and had no caller. The
                // health page is where a person looks when something feels
                // wrong, so it is where the symptoms belong.
                let symptoms = crate::diagnose::diagnose(&self.vitals());
                let mut lines = self.status_lines();
                if !symptoms.is_empty() {
                    lines.push((
                        "Wrong with me".to_string(),
                        crate::diagnose::report(&symptoms),
                    ));
                }
                let page = hub::status_page(&lines, settings.changed().len());
                // "Make it run well" beside it, and "Sort my files" (2 Oct 2026).
                // Neither is about a phone: what starts with Windows, and a
                // PC's folders (`phonemode`).
                if crate::phonemode::on() {
                    with_block(page, &self.space_section_live())
                } else {
                    with_block(page, &format!("{}{}{}", self.space_section_live(), hub::speed_section(), hub::sorting_section()))
                }
            }
            Page::Settings => {
                // What's kept, not what this run started with: a change made
                // here is written straight to settings.yaml but only read at
                // the next start, so rendering from the running config showed
                // the switch you had just flipped back where it was — every
                // toggle looked like it hadn't taken. The same reading the
                // Atlas window's Settings page uses.
                let kept = crate::settingswin::current_settings(&crate::roots::config_dir())
                    .unwrap_or_else(|_| crate::settings::registry(&self.tools_cfg()));
                hub::settings_page(&kept)
            }
            // By where this device stands (`hubvault`, 27 Sep 2026).
            Page::Sync => self.sync_page_live(),
            Page::Permissions => hub::permissions_page(
                &crate::settings::registry(&self.tools_cfg()),
                &self.permissions.granted_apps(),
            ),
            Page::AddOns => {
                let groups: Vec<String> =
                    self.chats.rooms.iter().filter(|r| r.members.len() > 1 || crate::groups::is_owned_id(&r.id)).map(|r| r.name.clone()).collect();
                let people: Vec<String> =
                    crate::kin::Pairings::load(&self.peer_dir).contacts.iter().map(|c| c.name.clone()).collect();
                let share_to: Vec<String> = groups.iter().chain(people.iter()).cloned().collect();
                hub::addons_page_with(
                    &crate::plugins::scan(
                        &self.plugins_dir,
                        &self.cfg.commands,
                        &crate::plugins::Approvals::load(&self.store),
                    ),
                    &crate::plugins::Offers::load(&self.store).items,
                    &share_to,
                    &groups,
                )
            }
            Page::Edits => {
                let (kept, problems) = crate::yourchanges::all_kept(&crate::roots::config_dir());
                hub::edits_page(&kept, &problems)
            }
            Page::Friends => {
                let mut v = self.friends_view();
                // A link just made, shown this once.
                if let Some(crate::hubjobs::Flash::FriendLink(l)) = crate::hubjobs::take_flash(&mut self.flash_once, Page::Friends, now) {
                    v.link = Some(l);
                }
                hub::friends_page(&v)
            }
            Page::Groups => {
                let (views, addable) = crate::groups::views(&self.store, &self.peer_dir);
                let ownerless: Vec<String> = self
                    .chats
                    .rooms
                    .iter()
                    .filter(|r| r.members.len() > 1 && !crate::groups::is_owned_id(&r.id))
                    .map(|r| r.name.clone())
                    .collect();
                hub::groups_page_with(&views, &addable, &ownerless)
            }
            Page::Accounts => {
                let advice = self.accounts.advice();
                let undescribed = self.accounts.undescribed();
                let safety = self.account_safety();
                let page = hub::accounts_page(
                    &self.accounts.accounts,
                    &advice,
                    &undescribed,
                    &self.stored_secrets(),
                    self.vault.state() == crate::vault::State::Open,
                    &safety,
                );
                self.with_vault_section(page)
            }
            Page::Access => {
                // Only what is genuinely reachable on this machine. Listing
                // the whole catalogue would tell you Atlas holds a mail
                // password it has never been given.
                let all = crate::credentials::all();
                let held: Vec<&crate::credentials::Credential> = all
                    .iter()
                    .filter(|c| self.holds(c))
                    .collect();
                let misplaced = crate::credentials::misplaced();
                // The sites half, which was passed as an empty slice until
                // 19 Sep 2026. This page's own docstring warns that an access
                // page saying nothing reads as "nothing to worry about" --
                // and then did exactly that for the half that lists the doors
                // currently open. `signin::hub_rows` had no caller anywhere.
                let sites = crate::signin::hub_rows(&self.access, now);
                hub::access_page_full(&held, &misplaced, &sites)
            }
        }
    }

    // ---------- the cards ----------

    /// What each dashboard card shows, from live state.
    ///
    /// A card with nothing in it says so in a sentence a person would say. It
    /// never renders an empty frame, because an empty frame and a broken one
    /// look identical.
    pub fn dashboard_cards(&mut self, now: u64) -> Vec<(Card, String)> {
        vec![
            (Card::Outstanding, self.card_outstanding(now)),
            (Card::Projects, self.card_projects()),
            (Card::Today, self.card_today(now)),
            (Card::Machine, self.card_machine()),
            (Card::Activity, self.card_activity(now)),
            (Card::Connections, self.card_connections(now)),
            (Card::Ideas, self.card_ideas()),
            (Card::Stuck, self.card_stuck()),
            (Card::Handed, self.card_handed()),
            (Card::Trust, self.card_trust()),
        ]
    }

    /// Waiting on you: the command deck's first card. Two big numbers — what's
    /// yours to act on, in ember, and what Atlas has proposed, in blue — then
    /// the things themselves.
    fn card_outstanding(&self, now: u64) -> String {
        let o = crate::workspace_view::overview(&self.workspace, now);
        // The same things the "waiting" count at the top of every page
        // counts (`waiting_count`), so the two can never disagree: your open
        // items, and what Atlas stopped on and needs you for.
        let mut lines = self.outstanding_lines(now);
        let stopped: Vec<String> = self
            .backlog
            .items
            .iter()
            .filter(|i| !i.done)
            .map(crate::backlog::Backlog::phrase)
            .collect();
        lines.extend(stopped.iter().cloned());
        let proposed: Vec<(String, String)> = self
            .workshop
            .projects
            .iter()
            .flat_map(|p| p.ready().into_iter().map(move |c| (c.title.clone(), p.name.clone())))
            .collect();
        if lines.is_empty() && proposed.is_empty() {
            return hub::nothing("Nothing waiting on you. Genuinely, not just unread.");
        }
        let mut s = hub::deck_figures(&[
            ("To act", o.needs_you + o.overdue + stopped.len(), hub::Dot::Act),
            ("Proposed", proposed.len(), hub::Dot::Proposed),
        ]);
        let mut rows: Vec<(hub::Dot, String, String)> = lines
            .iter()
            .take(4)
            .map(|l| (hub::Dot::Act, l.clone(), String::new()))
            .collect();
        for (title, project) in proposed.iter().take(4usize.saturating_sub(rows.len()).max(1)) {
            rows.push((hub::Dot::Proposed, format!("Approve {title}"), project.clone()));
        }
        s.push_str(&hub::rows(&rows));
        s.push_str(&hub::more("/hub/outstanding", "Everything outstanding"));
        s
    }

    /// Projects: each one, and what it is waiting on.
    fn card_projects(&self) -> String {
        if self.workshop.projects.is_empty() {
            return hub::nothing(
                "No projects yet. Say \"on the <name> project, …\" and one starts here.",
            );
        }
        let rows: Vec<(hub::Dot, String, String)> = self
            .workshop
            .projects
            .iter()
            .take(5)
            .map(|p| {
                let ready = p.ready().len();
                let working = p.in_progress().len();
                if ready > 0 {
                    (hub::Dot::Act, p.name.clone(), format!("{ready} ready"))
                } else if working > 0 {
                    (hub::Dot::Proposed, p.name.clone(), "building".to_string())
                } else {
                    (hub::Dot::Record, p.name.clone(), "quiet".to_string())
                }
            })
            .collect();
        let mut s = hub::rows(&rows);
        s.push_str(&hub::more("/hub/workshop", "Open the workshop"));
        s
    }

    fn card_today(&self, now: u64) -> String {
        let day = crate::workspace_view::day_of(&self.workspace, now);
        let said = crate::workspace_view::day_spoken(&day);
        if said.trim().is_empty() {
            return hub::nothing("Nothing has moved yet today.");
        }
        format!(
            "<p class=said>{}</p>{}",
            hub::esc(&said),
            hub::more("/hub/back", "Look at earlier days")
        )
    }

    /// Health, as rings: memory, disk, and the battery when there is one.
    fn card_machine(&self) -> String {
        let r = crate::health::read_machine();
        if r.ram_total_gb <= 0.0 && r.disk_total_gb <= 0.0 {
            return hub::nothing("I can't read this machine's memory or disk.");
        }
        let mut s = String::new();
        if r.ram_total_gb > 0.0 {
            let free = 1.0 - fraction(r.ram_used_gb, r.ram_total_gb);
            s.push_str(&hub::gauge(
                &format!("{:.1} GB", r.ram_total_gb),
                &format!("memory, {:.0}% free", free * 100.0),
                fraction(r.ram_used_gb, r.ram_total_gb),
            ));
        }
        if r.disk_total_gb > 0.0 {
            let free = fraction(r.disk_free_gb, r.disk_total_gb);
            s.push_str(&hub::gauge(
                &format!("{:.0} GB", r.disk_total_gb),
                &format!("disk, {:.0}% free", free * 100.0),
                1.0 - free,
            ));
        }
        if let Some(b) = r.battery_percent {
            s.push_str(&hub::gauge(
                &format!("{b}%"),
                if r.on_battery { "battery, on battery" } else { "battery, plugged in" },
                1.0 - (b as f32 / 100.0),
            ));
        }
        s
    }

    /// What I did without being asked: the last day of Atlas's own work,
    /// newest first, each with the time on your clock.
    fn card_activity(&self, now: u64) -> String {
        let off = crate::localclock::offset_secs();
        let since = now.saturating_sub(24 * 3600);
        let rows: Vec<(hub::Dot, String, String)> = self
            .journal
            .since(since)
            .into_iter()
            .rev()
            .filter(|e| e.kind != crate::activity::Kind::Upkeep)
            .take(5)
            .map(|e| (hub::Dot::Record, e.what.clone(), crate::localclock::hhmm(e.at, off)))
            .collect();
        if rows.is_empty() {
            return hub::nothing("I haven't done anything on my own today.");
        }
        let mut s = hub::rows(&rows);
        s.push_str(&hub::more("/hub/activity", "Full activity"));
        s
    }

    fn card_connections(&self, now: u64) -> String {
        let lines = self.connection_lines(now);
        if lines.is_empty() {
            return hub::nothing("Nothing outside this machine is connected yet.");
        }
        hub::lines(&lines)
    }

    fn card_ideas(&mut self) -> String {
        self.refresh_signals();
        let cfg = self.tools_cfg().self_audit.clone();
        if !cfg.enabled {
            return hub::nothing("Looking at myself is switched off.");
        }
        let recs = crate::selfaudit::recommend(&self.signals, cfg.most_at_once);
        if recs.is_empty() {
            return hub::nothing("Nothing about myself I'd change right now.");
        }
        let lines: Vec<String> = recs.iter().map(|r| r.symptom.clone()).collect();
        let mut s = hub::lines(&lines);
        s.push_str(&hub::more("/hub/recommendations", "Read the reasoning"));
        s
    }

    fn card_stuck(&self) -> String {
        let stuck: Vec<String> = self
            .backlog
            .items
            .iter()
            .filter(|i| !i.done)
            .map(|i| crate::backlog::Backlog::phrase(i))
            .collect();
        if stuck.is_empty() {
            return hub::nothing("Nothing is waiting on you.");
        }
        hub::lines(&stuck[..stuck.len().min(5)])
    }

    /// What you handed over, and what was in it.
    ///
    /// The text shown here came from outside this machine. It is rendered and
    /// escaped like any other outside text and never goes anywhere near the
    /// part of Atlas that decides what to do.
    fn card_handed(&self) -> String {
        let open = self.tray.open();
        if open.is_empty() {
            return hub::nothing(
                "Nothing waiting. Send a link from your phone and I'll read it.",
            );
        }
        let mut s = String::from("<ul class=tight>");
        for item in open.iter().take(5) {
            let state = match item.state {
                crate::tray::State::Waiting => "not read yet".to_string(),
                crate::tray::State::Read => item
                    .found
                    .clone()
                    .unwrap_or_else(|| "read".to_string())
                    .chars()
                    .take(180)
                    .collect(),
                crate::tray::State::Stuck => item
                    .found
                    .clone()
                    .unwrap_or_else(|| "I couldn't open it".to_string()),
                crate::tray::State::Done => continue,
            };
            s.push_str(&format!(
                "<li><b>{}</b> <span class=chip>{}</span><br>\
                 <span class=what>{}</span>\
                 <form class=inline method=post action=/hub/tray>\
                 <input type=hidden name=id value='{}'>\
                 <button class=go>Finished with it</button></form></li>",
                hub::esc(&item.title()),
                hub::esc(item.sort.title()),
                hub::esc(&state),
                item.id
            ));
        }
        s.push_str("</ul>");
        s
    }

    /// What Atlas is trusted with, and what would change it.
    ///
    /// Shown rather than kept internal on purpose. "Not confident enough" with
    /// no reason attached is what makes a system feel arbitrary; naming the
    /// thing that would change it makes getting better something you can both
    /// see happening.
    fn card_trust(&self) -> String {
        let mut s = String::new();
        for space in std::iter::once(crate::earned::Space::Personal).chain(
            self.earned
                .businesses()
                .into_iter()
                .map(crate::earned::Space::Business),
        ) {
            // Named even when it is the only one. "Your own work" being a
            // heading is what makes a business appearing beneath it read as a
            // separate record rather than more of the same.
            s.push_str(&format!("<h4 class=spacename>{}</h4>", hub::esc(&space.title())));
            s.push_str(&self.trust_rows(&space));
        }
        s
    }

    fn trust_rows(&self, space: &crate::earned::Space) -> String {
        let mut s = String::from("<ul class=tight>");
        for (kind, rope, good, total) in self.earned.standing_in(space) {
            s.push_str(&format!(
                "<li><b>{}</b> — {}<br><span class=what>{}</span></li>",
                hub::esc(kind.title()),
                hub::esc(rope.plain()),
                hub::esc(&if total == 0 {
                    format!("Nothing yet. {}", self.earned.what_would_earn_more_in(space, kind))
                } else {
                    format!(
                        "{good} right out of {total}. {}",
                        self.earned.what_would_earn_more_in(space, kind)
                    )
                })
            ));
        }
        s.push_str("</ul>");
        s
    }

    // ---------- the lists behind the cards ----------

    /// How you've chosen the hub to look.
    fn appearance(&self) -> crate::hub::Appearance {
        // Kept until the file changes: read for every page (27 Sep 2026).
        self.store.load_kept(crate::hub::APPEARANCE_KEY)
    }

    /// What you'd have Atlas call you: what you said out loud ("call me …")
    /// wins over the setting, the same rule `returning_cfg` follows.
    fn what_to_call_you(&self) -> Option<String> {
        // The same answer every reply uses (`Daemon::persona_now`): what you
        // said out loud over the settings, and nobody's name to a guest —
        // this greeted whoever was holding the laptop as its owner.
        let said = self.persona_now().address.trim().to_string();
        (!said.is_empty()).then_some(said)
    }

    /// The top of the command deck: greeting, whether Atlas is on, what it is
    /// doing, and today down the spine. `off` is this machine's offset from
    /// UTC, passed in so a test can pin the clock.
    /// A catalogue voice's sample: kept after the first listen, fetched and
    /// checked against its pin before that (`voicepick`).
    ///
    /// Fetched on its own thread, never inside the request: every hub page
    /// is answered on the daemon's thread, so a download here held all of
    /// Atlas (27 Sep 2026). The first press starts it; press play again once
    /// it's in.
    fn voice_sample(&self, id: &str) -> Result<Vec<u8>, String> {
        static FETCHING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let s = crate::voicepick::source(id).ok_or_else(|| format!("{id} isn't one of the voices on offer."))?;
        let root = crate::roots::install_root();
        let piece = crate::voicepick::sample_piece(s, &self.tools_cfg().tts_engine.voices_dir);
        let at = root.join(piece.key_path());
        if let Ok(bytes) = std::fs::read(&at) {
            // Checked against its pin when it was fetched; a file there is one
            // `getpieces` kept.
            return Ok(bytes);
        }
        let mut fetching = FETCHING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !fetching.iter().any(|f| f == id) {
            fetching.push(id.to_string());
            let id = id.to_string();
            std::thread::spawn(move || {
                let _ = crate::getpieces::fetch(&piece, &root, &crate::getpieces::Tools::default(), &|_, _| {});
                FETCHING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|f| *f != id);
            });
        }
        Err("The sample is on its way. Press play again in a moment.".into())
    }

    /// Start downloading a catalogue voice on its own thread; the Sound page
    /// shows how far it's got.
    fn get_voice(&self, id: &str) -> String {
        let Some(s) = crate::voicepick::source(id) else {
            return format!("{id} isn't one of the voices on offer.");
        };
        let name = crate::tts::find(id).map(|v| v.name).unwrap_or_else(|| id.to_string());
        let dir = self.tools_cfg().tts_engine.voices_dir.clone();
        if std::path::Path::new(&self.tools_cfg().tts_engine.voice_file_for(id)).exists() {
            return format!("{name} is already here.");
        }
        let of = s.model.1 + s.settings.1;
        if !self.voice_downloads.begin(id, of) {
            return format!("{name} is already on the way.");
        }
        let downloads = self.voice_downloads.clone();
        let root = crate::roots::install_root();
        std::thread::spawn(move || {
            crate::voicepick::fetch_voice(s, &root, &dir, &downloads, &crate::getpieces::Tools::default())
        });
        format!("Getting {name} ({}). You can choose it here once it's in.", crate::voicepick::size_said(s))
    }

    /// Start downloading Kokoro (its library and model) on its own thread;
    /// the Sound page shows how far it's got.
    fn get_kokoro(&self) -> String {
        let root = crate::roots::install_root();
        if crate::kokoro::check(&root).is_ok() {
            return "Kokoro is already here.".into();
        }
        let pieces = crate::kokoro::pieces();
        if pieces.is_empty() {
            return "Kokoro isn't available on this kind of computer yet.".into();
        }
        if let Err(why) = crate::getpieces::room_for(&pieces, &root, crate::getpieces::free_bytes(&root)) {
            return why;
        }
        let of: u64 = pieces.iter().map(|p| p.bytes).sum();
        if !self.voice_downloads.begin(crate::kokoro::DOWNLOAD_ID, of) {
            return "Kokoro is already on the way.".into();
        }
        let downloads = self.voice_downloads.clone();
        std::thread::spawn(move || crate::kokoro::fetch_all(&root, &downloads, &crate::getpieces::Tools::default()));
        format!(
            "Getting Kokoro ({} MB). Choose it as the engine here; it's used from Atlas's next start.",
            crate::kokoro::download_mb()
        )
    }

    /// What the code's server hands over when a phone replies, for a test
    /// that has no phone.
    pub fn phones_heard_for_test(&self, d: crate::phoneadd::Device) {
        if let Ok(mut h) = self.phones_heard.lock() {
            h.push(d);
        }
    }

    /// Keep the iPhones the code's server heard, and send each to whoever
    /// sends Atlas out. Called from the page and from the tick, so a phone
    /// added while nobody's looking still goes.
    pub(crate) fn take_heard_phones(&mut self, now: u64) -> Vec<String> {
        let heard: Vec<crate::phoneadd::Device> = match self.phones_heard.lock() {
            Ok(mut h) => std::mem::take(&mut *h),
            Err(_) => return Vec::new(),
        };
        let mut said = Vec::new();
        let mut mine: Vec<crate::phoneadd::Device> = self.store.load(crate::phoneadd::MINE);
        let before = mine.clone();
        for mut d in heard {
            d.at = now;
            crate::phoneadd::keep(&mut mine, d);
        }
        // Every one not yet sent on: new, or one whose sending failed before
        // (not yet in anyone's release channel, say). Tried again each time,
        // never dropped without a word.
        for d in mine.iter_mut().filter(|d| !d.sent) {
            match crate::phoneadd::send_to_releaser(&self.store, &self.peer_dir, d, now) {
                Ok(s) => {
                    d.sent = true;
                    said.push(format!("{} told Atlas its ID. {s}", d.name));
                }
                Err(why) => said.push(format!("{} told Atlas its ID, but it couldn't be sent on: {why}", d.name)),
            }
        }
        if mine != before {
            if let Err(e) = self.store.save(crate::phoneadd::MINE, &mine) {
                said.push(format!("The phones list couldn't be kept: {e}"));
            }
        }
        said
    }

    fn phone_view(&mut self, kind: Option<crate::phoneadd::Kind>) -> crate::hubpages::PhoneView {
        use crate::phoneadd::{app_file, Kind};
        let now = crate::store::now();
        let _ = self.take_heard_phones(now);
        // A code that came up on the crew is shown now, not at the next tick.
        let coming: Vec<_> = self
            .hub_after
            .values()
            .filter_map(|a| match a {
                crate::daemon::HubAfter::PhoneCode { slot, stop, .. } => Some((slot.clone(), stop.clone())),
                _ => None,
            })
            .collect();
        for (slot, stop) in coming {
            self.take_phone_code(&slot, &stop);
        }
        if self.phone_code.as_ref().is_some_and(|s| s.until <= now) {
            self.phone_code = None;
        }
        let root = self.store.install_root();
        let mine: Vec<crate::phoneadd::Device> = self.store.load(crate::phoneadd::MINE);
        let ipa_file = app_file(&root, Kind::Apple, &self.builds_dirs());
        // A closure, not `.and_then(ipa_facts)`: the reachability guards find
        // a call by its `name(`.
        let ipa = ipa_file.as_deref().and_then(|f| ipa_facts(f)).map(|(version, devices)| {
            let fits = mine.iter().any(|d| devices.iter().any(|u| u.eq_ignore_ascii_case(&d.udid)));
            (version, fits)
        });
        let ipa_reading = ipa.is_none() && ipa_file.as_deref().is_some_and(|f| !ipa_facts_ready(f));
        let apk = app_file(&root, Kind::Android, &self.builds_dirs()).and_then(|f| std::fs::metadata(f).ok()).map(|m| m.len() / 1_000_000);
        let code = self.phone_code.as_ref().filter(|s| Some(s.kind) == kind).map(|s| crate::hubpages::PhoneCode {
            what: s.what.clone(),
            url: s.url.clone(),
            qr: crate::phonelink::qr_svg(&s.url).unwrap_or_default(),
            minutes_left: s.until.saturating_sub(now).div_ceil(60),
        });
        crate::hubpages::PhoneView {
            kind,
            code,
            mine,
            ipa,
            ipa_reading,
            apk,
            waiting: self.store.load(crate::phoneadd::WAITING),
        }
    }

    /// "Show the code": the add-this-device page or the app's install page,
    /// on Tailscale for fifteen minutes.
    ///
    /// On the crew: reading the app (hundreds of megabytes) and asking
    /// Tailscale (up to twenty seconds a call) held all of Atlas inside the
    /// request (27 Sep 2026). The page shows how it's going and then the code.
    fn start_phone_code(&mut self, kind: crate::phoneadd::Kind, what: &str) -> Reply {
        use crate::phoneadd::{app_file, Kind, Showing, MINUTES};
        use std::sync::atomic::AtomicBool;
        // The phone app never serves an app to install (`phonemode`).
        if crate::phonemode::on() {
            return hub::back_with(Page::Dashboard.href(), "", "That's done from Atlas on a computer, not from the phone.");
        }
        if let Some(old) = self.phone_code.take() {
            old.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let root = self.store.install_root();
        let dirs = self.builds_dirs();
        let heard = self.phones_heard.clone();
        let what = what.to_string();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let slot: std::sync::Arc<std::sync::Mutex<Option<Showing>>> = Default::default();
        let (stop2, slot2) = (stop.clone(), slot.clone());
        let extra = format!("kind={}", kind.slug());
        let work = move || -> Result<String, String> {
            let app = if what == "install" {
                let Some(f) = app_file(&root, kind, &dirs) else {
                    return Err("The app for that phone isn't on this computer yet. Download it into Downloads and this page finds it.".into());
                };
                match std::fs::read(&f).map_err(|e| e.to_string()).and_then(|b| crate::ota::Package::read(&b).map(|p| (b, p))) {
                    Ok(x) => Some(x),
                    Err(why) => return Err(format!("The app on this computer couldn't be read: {why}")),
                }
            } else {
                None
            };
            let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Atlas couldn't open a place for the phone to reach: {e}"))?;
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            let base = crate::phoneadd::reach_out(port)?;
            if stop2.load(std::sync::atomic::Ordering::Relaxed) {
                crate::phoneadd::stop_reaching_out();
                return Err("Stopped: the code no longer works.".into());
            }
            let token = crate::ota::fresh_token();
            let url = format!("{}/{token}/", base.trim_end_matches('/'));
            let serving = std::sync::Arc::new(AtomicBool::new(false));
            match app {
                None => {
                    let (t, b, s) = (token.clone(), base.clone(), serving.clone());
                    std::thread::spawn(move || {
                        crate::phoneadd::serve_enrol(listener, &t, &b, MINUTES, &s, &move |d| {
                            if let Ok(mut h) = heard.lock() {
                                h.push(d);
                            }
                        })
                    });
                }
                Some((bytes, pkg)) => {
                    drop(listener);
                    let (t, b) = (token.clone(), base.clone());
                    std::thread::spawn(move || crate::ota::serve_install(&bytes, &pkg, port, &t, Some(&b), MINUTES, &mut |_| {}));
                }
            }
            *slot2.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(Showing { kind, what: what.clone(), url, until: crate::store::now() + MINUTES * 60, stop: serving });
            Ok(match (what.as_str(), kind) {
                ("install", _) => "Scan the code with the phone.".into(),
                (_, Kind::Apple) => "Scan the code with the iPhone or iPad.".into(),
                _ => "Scan the code with the phone.".into(),
            })
        };
        self.hub_errand(
            "phone-code",
            Page::Phone,
            &extra,
            "Getting the code ready",
            Box::new(move || (work(), String::new())),
            |job| crate::daemon::HubAfter::PhoneCode { job, slot, stop },
        )
    }

    /// H7: the phone's calendar in, Atlas's own events out. Off when
    /// `calendar.sync_native` is off, and says so rather than failing.
    fn phone_calendar(&mut self, body: &str, now: u64) -> serde_json::Value {
        let cal = self.tools_cfg().calendar.clone();
        if !cal.enabled || !cal.sync_native {
            return serde_json::json!({ "off": true, "atlas": [] });
        }
        let Ok(batch) = serde_json::from_str::<crate::calendar::PhoneBatch>(body) else {
            return serde_json::json!({ "error": "That wasn't a calendar Atlas could read." });
        };
        let (changed, removed) = self.calendar.sync_from_phone(batch.events(now), batch.from, batch.to, now);
        if changed + removed > 0 {
            if let Err(e) = self.calendar.save(&self.store) {
                return serde_json::json!({ "error": format!("Atlas couldn't keep the calendar: {e}") });
            }
        }
        // Each occurrence in the window, so a repeat arrives as its days; the
        // id carries the start, so the phone keeps one event per occurrence.
        let atlas: Vec<serde_json::Value> = self
            .calendar
            .occurrences_between(batch.from, batch.to)
            .into_iter()
            .filter(|e| e.source == crate::calendar::Source::Atlas)
            .map(|e| {
                serde_json::json!({
                    "id": format!("{}-{}", e.id, e.start), "title": e.title, "start": e.start, "end": e.end,
                    "all_day": e.all_day, "place": e.place,
                })
            })
            .collect();
        serde_json::json!({ "changed": changed, "removed": removed, "atlas": atlas })
    }

    /// What a phone widget shows: today's next thing, the waiting count and
    /// whether Atlas is on, made safe for a home or lock screen (`glance`).
    pub fn glance(&self, now: u64) -> crate::glance::Glance {
        let deck = self.deck(now, crate::localclock::offset_secs());
        let working = self.mind.focus().map(|w| sentence(&w.asked));
        let later = deck
            .spine
            .iter()
            .filter(|(_, _, m)| *m == hub::Mark::Later)
            .map(|(t, w, _)| (t.clone(), w.clone()))
            .collect();
        let facts = crate::glance::Facts {
            status: deck.status.clone(),
            tone: deck.tone.to_string(),
            working,
            later,
            waiting: self.waiting_count(now),
        };
        crate::glance::for_widgets(&facts, now, self.tools_cfg().phone.widget_titles_on_lock_screen)
    }

    pub fn deck(&self, now: u64, off: i64) -> hub::Deck {
        use crate::localclock::{day, hhmm, hour};
        let name = self.what_to_call_you();
        let greeting = match (crate::nudge::Part::from_hour(hour(now, off)), &name) {
            (Some(part), Some(n)) => format!("{}, {n}.", part.greeting()),
            (Some(part), None) => format!("{}.", part.greeting()),
            (None, Some(n)) => format!("Hello, {n}."),
            (None, None) => "Hello.".to_string(),
        };

        let cfg = self.tools_cfg();
        let hears = cfg.enabled
            && self.audio_devices.as_ref().map(|d| !d.is_empty()).unwrap_or(false);
        let (status, tone) = if self.attention.is_paused() {
            ("Paused".to_string(), "held")
        } else if hears {
            ("On, listening".to_string(), "")
        } else {
            ("On, typing".to_string(), "")
        };

        // Everything is a real moment, placed on your wall clock.
        let today = day(now, off);
        let now_wall = now as i64 + off;
        let mut events: Vec<(i64, String, String, hub::Mark)> = Vec::new();
        let start_of_day = crate::localclock::midnight(now, off);
        for e in self.calendar.occurrences_between(start_of_day, start_of_day + 86_400) {
            if day(e.start, off) != today && !e.all_day {
                continue;
            }
            let at = e.start as i64 + off;
            let (label, mark) = if e.all_day {
                ("All day".to_string(), hub::Mark::Later)
            } else if (e.end as i64 + off) <= now_wall {
                (hhmm(e.start, off), hub::Mark::Done)
            } else {
                (hhmm(e.start, off), hub::Mark::Later)
            };
            events.push((if e.all_day { today * 86_400 } else { at }, label, e.title.clone(), mark));
        }
        for e in self.journal.since(now.saturating_sub(86_400)) {
            if e.kind == crate::activity::Kind::Upkeep || day(e.at, off) != today {
                continue;
            }
            events.push((e.at as i64 + off, hhmm(e.at, off), e.what.clone(), hub::Mark::Done));
        }
        events.sort_by_key(|(w, ..)| *w);

        let running = self
            .queue
            .tasks
            .iter()
            .find(|t| t.state == crate::lanes::TaskState::Running);
        let waiting_to_start = self.queue.tasks.iter().find(|t| {
            matches!(t.state, crate::lanes::TaskState::Queued | crate::lanes::TaskState::WaitingForGap)
        });
        let next = events.iter().find(|(w, _, _, m)| *m == hub::Mark::Later && *w >= now_wall);
        let (now_line, sub, now_short) = if let Some(t) = running {
            let more = self.queue.tasks.iter().filter(|t| !matches!(t.state, crate::lanes::TaskState::Done | crate::lanes::TaskState::Failed)).count().saturating_sub(1);
            (
                sentence(&t.command),
                if more > 0 {
                    format!("Started {}. {more} more after this.", hhmm(t.created, off))
                } else {
                    format!("Started {}.", hhmm(t.created, off))
                },
                sentence(&t.command),
            )
        } else if let Some(t) = waiting_to_start {
            (
                format!("About to start: {}", t.command.trim()),
                if t.state == crate::lanes::TaskState::WaitingForGap {
                    "Waiting for you to stop typing, so it doesn't get in your way.".to_string()
                } else {
                    "Next in line.".to_string()
                },
                "About to start".to_string(),
            )
        } else if self.attention.is_paused() {
            ("Paused.".to_string(), "Say \"carry on\" when you want me back.".to_string(), "Paused".to_string())
        } else {
            (
                "Nothing underway.".to_string(),
                match next {
                    Some((_, time, what, _)) => format!("Next: {what} at {time}."),
                    None if crate::phonemode::on() => "Say what you need, or tap the search to find anything.".to_string(),
                    None => "Say what you need, or press Ctrl K to find anything.".to_string(),
                },
                "You're here".to_string(),
            )
        };

        // The last three things done, now, and the next three.
        let done: Vec<_> = events.iter().filter(|(w, ..)| *w <= now_wall).collect();
        let later: Vec<_> = events.iter().filter(|(w, ..)| *w > now_wall).collect();
        let mut spine: Vec<(String, String, hub::Mark)> = done
            .iter()
            .skip(done.len().saturating_sub(3))
            .map(|(_, t, w, _)| (t.clone(), w.clone(), hub::Mark::Done))
            .collect();
        if !events.is_empty() || running.is_some() {
            spine.push(("NOW".to_string(), now_short, hub::Mark::Now));
        }
        spine.extend(later.iter().take(3).map(|(_, t, w, m)| (t.clone(), w.clone(), *m)));

        let asks = self.home_asks(now);
        let brief = self.brief_line(&asks, running.is_some(), &now_line);
        let first_run = events.is_empty()
            && self.workspace.is_empty()
            && self.workshop.projects.is_empty()
            && self.calendar.is_empty()
            && self.journal.since(0).is_empty()
            && asks.is_empty();
        hub::Deck {
            greeting,
            status,
            tone,
            now: now_line,
            now_sub: sub,
            spine,
            brief,
            asks,
            businesses: self.glances(now),
            first_run,
        }
    }

    /// What's waiting on you, for the Brief on Home and the first lane of
    /// Outstanding: your own open items, what Atlas stopped on and needs you
    /// for, and changes it built that wait on your yes. The same things the
    /// waiting count at the top of every page counts.
    fn home_asks(&self, now: u64) -> Vec<(String, String)> {
        self.home_asks_keyed(now).into_iter().map(|(what, href, _)| (what, href)).collect()
    }

    /// `home_asks`, each with the key its Drop it button sends (see
    /// `hub::Drops`). Worked out here, where each ask is made, rather than
    /// matched back up by its words afterwards: two things can read the same.
    fn home_asks_keyed(&self, now: u64) -> Vec<(String, String, Option<String>)> {
        let mut out: Vec<(String, String, Option<String>)> = Vec::new();
        if let Some(view) = crate::workspace_view::shipped().into_iter().find(|v| v.name == "Now") {
            for i in crate::workspace_view::apply(&self.workspace, &view, now) {
                out.push((i.title.clone(), hub::Page::Workspace.href().to_string(), Some(format!("w:{}", i.id))));
            }
        }
        for b in self.backlog.items.iter().filter(|i| !i.done && !i.dismissed) {
            if waits_on_you(&b.blocker) {
                out.push((sentence(&b.request), hub::Page::Outstanding.href().to_string(), Some(format!("b:{}", b.id))));
            }
        }
        for p in &self.workshop.projects {
            for c in p.ready() {
                out.push((
                    format!("Approve \"{}\" ({})", c.title, p.name),
                    hub::Page::Workshop.href().to_string(),
                    Some(format!("c:{}:{}", c.id, p.name)),
                ));
            }
        }
        out
    }

    /// The Brief, in Atlas's words: how many things want you, what it's on,
    /// and — when nothing does — that it's quiet, said plainly.
    fn brief_line(&self, asks: &[(String, String)], working: bool, now_line: &str) -> String {
        let doing = if working { format!(" I'm on it now: {}", now_line.trim_end_matches('.')) } else { String::new() };
        let doing = if doing.is_empty() { doing } else { format!("{doing}.") };
        match asks.len() {
            0 => format!("It's quiet. Nothing needs you.{doing}"),
            1 => format!("One thing wants you.{doing}"),
            n => {
                const WORDS: [&str; 9] = ["Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten"];
                let said = WORDS.get(n - 2).map(|w| w.to_string()).unwrap_or_else(|| n.to_string());
                format!("{said} things want you.{doing}")
            }
        }
    }

    /// Your businesses at a glance: each business on the roster, its open
    /// work (items filed under it as the client), and its people.
    fn glances(&self, _now: u64) -> Vec<hub::Glance> {
        let roster = crate::roster::Roster::load(&self.store);
        roster
            .businesses()
            .into_iter()
            .map(|name| {
                let open = self
                    .workspace
                    .iter()
                    .filter(|i| i.status.live() && i.client.as_deref().map(|c| crate::kin::same_name(c, &name)).unwrap_or(false))
                    .count();
                let people = roster.members(&name).into_iter().map(|m| (m, "on the roster".to_string())).collect();
                hub::Glance { name, open, people }
            })
            .collect()
    }

    /// Validate a setting, then keep it, then take it up at once. The one
    /// path every settings form uses (Settings, Sound & voice), so a switch
    /// can't report success and write nothing.
    pub(crate) fn apply_setting(&mut self, key: &str, value: &str) -> String {
        // Validated first, then written. Validating first matters:
        // `Settings::set` is what knows a toggle from a number from a name,
        // and writing an unparseable value into the settings file would turn
        // a switch that did nothing into one that stops Atlas starting.
        let mut settings = crate::settings::registry(&self.tools_cfg());
        let said = settings.set_and_keep(key, value, &crate::roots::config_dir());
        // What taking it up says wins: it knows a change that waits for the
        // next start from one that is live now (29 Sep 2026: "is now on" was
        // shown for Voice, Push-to-talk and the speaking voice, which only
        // change when Atlas starts again).
        let took = self.pick_up_settings();
        if !took.is_empty() {
            return took.join(" ");
        }
        if crate::settings::needs_a_restart(key) && !said.to_lowercase().contains("couldn") {
            return format!("{said} It takes effect when Atlas next starts.");
        }
        said
    }

    /// A turn typed in the hub: answered like any other, and — under
    /// "hands-free only" — kept on screen rather than said.
    /// Someone found something in the way (Help → Report a barrier). On your
    /// own Atlas it goes on your own feedback list; on a friend's, to whoever
    /// sends them Atlas — only because they pressed send on exactly this text.
    fn report_barrier(&mut self, text: &str, now: u64) -> String {
        let words = format!("Accessibility: {text}");
        let f = match crate::feedback::compose_feedback(&words, None, now) {
            Ok(f) => f,
            Err(why) => return why,
        };
        let groups = crate::groups::Groups::load(&self.store);
        let sender = groups.held.values().map(|h| &h.state).find(|g| g.release_channel).map(|c| c.owner.clone());
        let me = crate::peerkey::Identity::load_or_create(&self.peer_dir).ok().map(|i| i.public()).unwrap_or_default();
        match sender {
            Some(owner) if owner != me && !me.is_empty() => {
                let to = crate::kin::Pairings::load(&self.peer_dir).name_of_key(&owner);
                match to {
                    Some(to) => {
                        crate::feedback::queue_feedback(&self.store, f, &to);
                        format!("Sent to {to}. They'll see exactly what you wrote, and nothing else.")
                    }
                    None => "I can't reach whoever gave you Atlas, so it's kept here for now.".to_string(),
                }
            }
            _ => {
                let _ = crate::feedback::heard_feedback(&self.store, "you", &serde_json::to_string(&f).unwrap_or_default());
                "It's on your own list to fix, on your Feedback page.".to_string()
            }
        }
    }

    /// The sidebar's parts only the running Atlas knows: your businesses, and
    /// your name on the brand.
    fn with_sidebar_names(&self, html: String) -> String {
        let businesses = crate::roster::Roster::load(&self.store).businesses();
        let html = hub::with_business(html, &businesses);
        let name = match crate::returning::Address::load(&self.store) {
            // A guest's hub doesn't carry the owner's name either.
            _ if self.handover().stance.handed_over() => None,
            crate::returning::Address::Name(n) => Some(n),
            _ if !self.store.exists("address") => {
                let n = self.tools_cfg().persona.address.clone();
                // A title ("sir") is how to address you, not your name.
                (!n.trim().is_empty() && n.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)).then_some(n)
            }
            _ => None,
        };
        hub::with_owner(html, name.as_deref())
    }

    /// Outstanding, in the design's four lanes.
    fn open_work(&self, now: u64) -> hub::Open {
        let mut drops = hub::Drops::default();
        let mut waiting: Vec<(String, String, String)> = Vec::new();
        for (what, href, key) in self.home_asks_keyed(now) {
            waiting.push((what, String::new(), href));
            drops.waiting.push(key);
        }
        // A question or a yes it's waiting on carries its reason with it.
        for w in waiting.iter_mut() {
            if let Some(i) = self.backlog.items.iter().find(|i| !i.done && !i.dismissed && sentence(&i.request) == w.0) {
                w.1 = sentence(&i.blocker.explain());
            }
        }
        let mut blocked: Vec<hub::Stopped> = Vec::new();
        for i in self.backlog.items.iter().filter(|i| !i.done && !i.dismissed && !waits_on_you(&i.blocker)) {
            blocked.push(hub::Stopped {
                what: sentence(&i.request),
                tried: format!("To {}.", i.request.trim().trim_end_matches('.')),
                stopped: sentence(&i.blocker.explain()),
                needs: i.blocker.needs(),
                area: None,
            });
            drops.blocked.push(Some(format!("b:{}", i.id)));
        }
        let mut in_progress: Vec<(String, String)> = Vec::new();
        for t in self
            .queue
            .tasks
            .iter()
            .filter(|t| !matches!(t.state, crate::lanes::TaskState::Done | crate::lanes::TaskState::Failed))
        {
            let how = match t.state {
                crate::lanes::TaskState::Running => "Running now.",
                crate::lanes::TaskState::WaitingForGap => "Waiting for a pause in your work.",
                _ => "Next in line.",
            };
            in_progress.push((sentence(&t.command), how.to_string()));
            // A queued task runs inside the tick, start to finish, with no
            // way to be told to stop part way: one marked running gets no
            // button rather than one that can't do what it says.
            drops.in_progress.push((t.state != crate::lanes::TaskState::Running).then(|| format!("t:{}", t.id)));
        }
        for e in self.crew.errands() {
            in_progress.push((e.name.clone(), "Handed to a worker — I check what comes back before you see it.".into()));
            drops.in_progress.push(Some(format!("e:{}", e.id)));
        }
        let today = crate::localclock::midnight(now, crate::localclock::offset_secs());
        let mut carried: Vec<(String, u64)> = Vec::new();
        for i in self.workspace.iter().filter(|i| i.at < today && i.status.live()) {
            carried.push((i.title.clone(), ((today - i.at) / 86_400).max(1)));
            drops.carried.push(Some(format!("w:{}", i.id)));
        }
        hub::Open { waiting, blocked, in_progress, carried, drops }
    }

    /// Everything on the Outstanding page that can be taken off it, in the
    /// page's own order, as (key, what to call it). What "remove the second
    /// one from my outstanding list" counts through, so the second one is
    /// the second one you can see (2 Oct 2026: the spoken path only knew the
    /// backlog, so anything else on the page "wasn't on the list").
    ///
    /// A workspace item can be on the page twice (waiting on you, and
    /// carried over): it's listed once. A backlog item is called what you
    /// said, not the page's tidied sentence, because that's what you'll say
    /// back.
    pub(crate) fn outstanding_removable(&self, now: u64) -> Vec<(String, String)> {
        let o = self.open_work(now);
        let titles = o
            .waiting
            .iter()
            .map(|w| w.0.clone())
            .chain(o.blocked.iter().map(|b| b.what.clone()))
            .chain(o.in_progress.iter().map(|p| p.0.clone()))
            .chain(o.carried.iter().map(|c| c.0.clone()));
        let pad = |v: &Vec<Option<String>>, n: usize| (0..n).map(|i| v.get(i).cloned().flatten()).collect::<Vec<_>>();
        let keys = pad(&o.drops.waiting, o.waiting.len())
            .into_iter()
            .chain(pad(&o.drops.blocked, o.blocked.len()))
            .chain(pad(&o.drops.in_progress, o.in_progress.len()))
            .chain(pad(&o.drops.carried, o.carried.len()));
        let mut out: Vec<(String, String)> = Vec::new();
        for (key, title) in keys.zip(titles) {
            let Some(key) = key else { continue };
            if out.iter().any(|(k, _)| *k == key) {
                continue;
            }
            let title = key
                .strip_prefix("b:")
                .and_then(|id| id.parse::<u64>().ok())
                .and_then(|id| self.backlog.items.iter().find(|i| i.id == id))
                .map(|i| i.request.trim().to_string())
                .unwrap_or(title);
            out.push((key, title));
        }
        out
    }

    /// Take one thing off the Outstanding page, by the key its button (or
    /// `outstanding_removable`) gave it, and keep that. The one way every
    /// path removes -- the hub's buttons, "take it off my outstanding list",
    /// "drop the task" -- so none of them can forget to save, and none can
    /// say gone while it comes back after a restart (2 Oct 2026).
    ///
    /// Each kind goes the way its own model already has for it: a backlog
    /// item is dismissed (and kept with what you dropped, so "bring back
    /// what I dropped" finds it), a workspace item is marked dropped, a
    /// queued task that hasn't started is taken out of the queue, a worker's
    /// errand is asked to stop, and a project change waiting for your yes is
    /// dropped. `Err` is what to say when there was nothing to take off.
    pub(crate) fn drop_outstanding(&mut self, key: &str, now: u64) -> Result<OffTheList, String> {
        let (kind, rest) = key.split_once(':').unwrap_or((key, ""));
        let gone = || "That's already off the list.".to_string();
        match kind {
            "b" => {
                let id: u64 = rest.parse().map_err(|_| gone())?;
                let item = self.backlog.outstanding().into_iter().find(|i| i.id == id).cloned().ok_or_else(gone)?;
                self.backlog.dismiss(id);
                let request = item.request.trim().to_string();
                self.dropped.retain(|d| d.title != request);
                self.dropped.push(crate::daily::Dropped {
                    title: request.clone(),
                    when: now,
                    carried_for: (now.saturating_sub(item.first_seen) / 86_400) as u32,
                    about: None,
                    thinking: Vec::new(),
                });
                let kept = self.backlog.save(&self.store).and_then(|_| self.store.save("dropped", &self.dropped));
                Ok(OffTheList { title: request, unsaved: kept.err().map(|e| e.to_string()), stopping: false, can_bring_back: true })
            }
            "w" => {
                let i = self.workspace.iter_mut().find(|i| i.id == rest && i.status.live()).ok_or_else(gone)?;
                i.status = crate::workspace_view::Status::Dropped;
                i.closed_at = Some(now);
                let title = i.title.clone();
                let kept = self.store.save("workspace", &self.workspace);
                Ok(OffTheList { title, unsaved: kept.err().map(|e| e.to_string()), stopping: false, can_bring_back: false })
            }
            "t" => {
                let id: u64 = rest.parse().map_err(|_| gone())?;
                let t = self.queue.tasks.iter().find(|t| t.id == id).ok_or_else(gone)?;
                match t.state {
                    crate::lanes::TaskState::Queued | crate::lanes::TaskState::WaitingForGap => {}
                    crate::lanes::TaskState::Running => {
                        return Err("That one's already running, and a queued job can't be stopped part way. It'll be off the list when it finishes.".into())
                    }
                    _ => return Err(gone()),
                }
                let title = t.command.trim().to_string();
                self.queue.tasks.retain(|t| t.id != id);
                let kept = self.queue.save(&self.store);
                Ok(OffTheList { title, unsaved: kept.err().map(|e| e.to_string()), stopping: false, can_bring_back: false })
            }
            "e" => {
                let id: u64 = rest.parse().map_err(|_| gone())?;
                let e = self.crew.errands().into_iter().find(|e| e.id == id).ok_or_else(gone)?;
                // Not waited on: the errand stops at its next safe point and
                // its ending comes back through `settle` like any other. One
                // still waiting for a hand is simply dropped.
                self.crew.ask_to_stop(id);
                let stopping = self.crew.in_hand(id);
                Ok(OffTheList { title: e.name, unsaved: None, stopping, can_bring_back: false })
            }
            "c" => {
                let (id, project) = rest.split_once(':').ok_or_else(gone)?;
                let id: u64 = id.parse().map_err(|_| gone())?;
                let p = self.workshop.projects.iter_mut().find(|p| p.name == project).ok_or_else(gone)?;
                let c = p
                    .changes
                    .iter_mut()
                    .find(|c| c.id == id && c.state == crate::workshop::State::Ready)
                    .ok_or_else(gone)?;
                c.state = crate::workshop::State::Dropped;
                let title = format!("Approve \"{}\" ({})", c.title, project);
                let kept = self.workshop.save(&self.store);
                Ok(OffTheList { title, unsaved: kept.err().map(|e| e.to_string()), stopping: false, can_bring_back: false })
            }
            _ => Err(gone()),
        }
    }

    pub(crate) fn outstanding_lines(&self, now: u64) -> Vec<String> {
        let now_view = crate::workspace_view::shipped()
            .into_iter()
            .find(|v| v.name == "Now");
        let Some(view) = now_view else {
            return Vec::new();
        };
        crate::workspace_view::apply(&self.workspace, &view, now)
            .into_iter()
            .map(|i| i.title.clone())
            .collect()
    }

    fn activity_lines(&self, now: u64) -> Vec<String> {
        let since = now.saturating_sub(24 * 3600);
        self.journal
            .since(since)
            .into_iter()
            .rev()
            .map(|e| e.what.clone())
            .collect()
    }

    fn connection_lines(&self, now: u64) -> Vec<String> {
        self.connections
            .integrations
            .iter()
            .map(|i| i.line(now))
            .collect()
    }

    /// What would go wrong, from the three modules built for exactly this and
    /// called by nothing.
    ///
    /// `goingaway` works out what would lock him out; `codes` finds accounts
    /// with no recovery codes; `recovery` finds the vault with no way back in.
    /// All three were complete and tested and none had ever been given an
    /// account. Empty inputs are not a problem here — "you have no recovery
    /// codes for anything" is the answer, and a real one.
    fn account_safety(&self) -> Vec<(String, String, f32)> {
        let mut out: Vec<(String, String, f32)> = Vec::new();
        let cfg = self.tools_cfg();

        for p in crate::goingaway::plan(&self.accounts.accounts) {
            out.push((
                format!("{} — {}", p.site, p.what),
                p.why.clone(),
                p.urgency,
            ));
        }

        let names: Vec<String> = self
            .accounts
            .accounts
            .iter()
            .map(|a| a.site.clone())
            .collect();
        for g in crate::codes::gaps(&self.code_sets, &names, &cfg.codes) {
            out.push((
                format!("{} — {}", g.site, g.what),
                g.url.clone().unwrap_or_else(|| {
                    "Recovery codes are the one thing that still works when \
                     everything else is locked."
                        .to_string()
                }),
                g.urgency,
            ));
        }

        for g in crate::recovery::gaps(&self.vault_recovery, &cfg.recovery, crate::store::now()) {
            out.push((g.what.clone(), g.why.clone(), g.urgency));
        }
        // Each way back in, with its weakness said plainly (Eric, B5).
        for s in &self.vault_recovery {
            out.push(("A way back into the vault".to_string(), crate::recovery::described(s), 0.0));
        }

        // Worst first, like everything else on this page.
        out.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    /// What the vault is holding, by name and kind only.
    ///
    /// Never a value, and never `get` — reading a secret to render a list
    /// would touch every entry's last-used date and defeat `stale()`, quite
    /// apart from putting the secrets themselves through a page.
    fn stored_secrets(&self) -> Vec<(String, String)> {
        self.vault
            .list()
            .into_iter()
            .map(|(name, kind)| (name.to_string(), kind_word(kind).to_string()))
            .collect()
    }

    /// Does Atlas actually have this on this machine?
    ///
    /// The inventory says what *kinds* of credential exist. Whether one is
    /// really here is a different question, and answering it from the
    /// catalogue would have the access page claim Atlas holds a mail password
    /// nobody ever gave it — a page that overstates its own reach is as
    /// useless as one that understates it.
    fn holds(&self, c: &crate::credentials::Credential) -> bool {
        match c.kept {
            crate::credentials::Kept::NotHeld => false,
            // A session exists only once there is a browser profile with
            // sites configured for it. An empty list means Atlas has never
            // been signed into anything.
            crate::credentials::Kept::YourBrowserSession => {
                !self.tools_cfg().browser.sites.is_empty()
            }
            crate::credentials::Kept::Vault => self
                .vault
                .list()
                .iter()
                .any(|(name, _)| c.name.contains(name) || name.contains(c.name)),
            crate::credentials::Kept::PlainConfig => true,
        }
    }

    /// How many things are actually waiting on you, for the header.
    ///
    /// One number, visible from every page. Anything that needs you and is
    /// only discoverable by going looking for it will be found late.
    pub fn waiting_count(&self, now: u64) -> usize {
        let o = crate::workspace_view::overview(&self.workspace, now);
        o.needs_you + o.overdue + self.backlog.items.iter().filter(|i| !i.done).count()
    }

    fn status_lines(&self) -> Vec<(String, String)> {
        let r = crate::health::read_machine();
        let mut out = vec![
            (
                "Listening".to_string(),
                // 30 Sep 2026: this said "Yes." whenever Atlas wasn't
                // paused -- with no microphone running, or after the wake
                // word had dropped to push-to-talk. Now it's what the loop is
                // actually doing.
                if self.attention.is_paused() {
                    "Paused — say \"carry on\" when you want me back.".to_string()
                } else if !self.mic_running() {
                    "No — the microphone isn't running, so type to me here.".to_string()
                } else {
                    let phrase = self.tools_cfg().wake.as_ref().map(|w| w.phrase.clone()).unwrap_or_default();
                    match self.tiers.tier {
                        crate::input::Tier::Voice if !phrase.trim().is_empty() => format!("Yes — listening for \"{}\".", phrase.trim()),
                        crate::input::Tier::Voice => "Yes — listening for my name.".to_string(),
                        crate::input::Tier::PushToTalk if !self.tiers.wake_on() => "When you hold the talk key (the wake word is off).".to_string(),
                        crate::input::Tier::PushToTalk => "Only when you hold the talk key — the wake word stopped working, and I'll try it again shortly.".to_string(),
                        crate::input::Tier::Typed => "No — I can't hear audio right now, so type to me here.".to_string(),
                    }
                },
            ),
            (
                "Things open".to_string(),
                format!("{}", self.workspace.iter().filter(|i| i.status.live()).count()),
            ),
        ];
        // What the crew has in hand, and whether it's too small: work that
        // takes ten minutes and work that *waits* ten minutes look the same
        // from outside, and only one of them is fixed by more hands.
        let errands = self.crew.errands();
        let recent = self.crew.recently_finished();
        if !errands.is_empty() || !recent.is_empty() {
            let mut line = if errands.is_empty() {
                "Nothing running.".to_string()
            } else {
                let names: Vec<String> = errands
                    .iter()
                    .map(|e| match self.crew.why_waiting(e.id) {
                        Some(why) => format!("{} (waiting: {why})", e.name),
                        None => format!("{} (running)", e.name),
                    })
                    .collect();
                names.join("; ")
            };
            if let Some(last) = recent.last() {
                line.push_str(&format!(
                    " Last finished: {}, waited {}, ran {}.",
                    last.name,
                    crate::crew::spoken_ms(last.waited_ms),
                    crate::crew::spoken_ms(last.ran_ms)
                ));
            }
            if let Some((name, ms)) = self.crew.longest_wait() {
                if ms >= 1000 {
                    line.push_str(&format!(" Longest wait for a hand: {name}, {}.", crate::crew::spoken_ms(ms)));
                }
            }
            out.push(("Work in hand".to_string(), line));
        }
        // What watching your folders costs, as a number: scans since start
        // and the gap to the next, which grows while nothing changes.
        out.push((
            "Folder scans".to_string(),
            format!(
                "{} since I started; next in about {}.",
                self.awareness.scans(),
                crate::crew::spoken_ms(self.awareness.how_long_to_wait(crate::store::now()) * 1000)
            ),
        ));
        if r.ram_total_gb > 0.0 {
            out.push((
                "Memory".to_string(),
                format!("{:.1} GB of {:.1} GB in use", r.ram_used_gb, r.ram_total_gb),
            ));
        }
        if r.disk_total_gb > 0.0 {
            out.push((
                "Disk".to_string(),
                format!("{:.0} GB free of {:.0} GB", r.disk_free_gb, r.disk_total_gb),
            ));
        }
        out
    }

    fn now_page_live(&self) -> String {
        hub::now_page(&self.now_view())
    }

    /// Where a live page stands, as a number that changes when what it
    /// shows does, and whether Atlas is still busy with it: `now` (the Now
    /// page's view) or `talk` (the conversation, the queue and the reply so
    /// far). Cheap -- nothing is drawn.
    fn live_page_state(&self, which: &str) -> (u64, bool) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match which {
            "now" => {
                let v = self.now_view();
                return (hub::live_version(&v), v.working);
            }
            "talk" => {
                self.thread.len().hash(&mut h);
                self.thread.last().map(|e| e.reply.len()).hash(&mut h);
                self.talk_queue.len().hash(&mut h);
                self.talk_is_thinking().hash(&mut h);
                self.talk_so_far().len().hash(&mut h);
                (h.finish(), !self.talk_queue.is_empty() || self.talk_is_thinking())
            }
            _ => (0, false),
        }
    }

    fn now_view(&self) -> hub::NowView {
        use crate::mind::Stage;
        let now = crate::store::now();
        let off = crate::localclock::offset_secs();
        let paused = self.attention.is_paused();
        let background: Vec<String> = self.mind.background().iter().map(|w| sentence(&w.asked)).collect();
        let held: Vec<String> = self.outbox.held.iter().map(|n| n.title.clone()).collect();
        let Some(w) = self.mind.focus() else {
            let title = if paused { "Paused." } else { "Waiting for you." };
            return hub::NowView {
                title: title.into(),
                since: if paused {
                    "Say \"carry on\", or press Carry on, when you want me back.".into()
                } else {
                    "Nothing underway right now.".into()
                },
                steps: Vec::new(),
                plain_from: 0,
                spent: None,
                fallback: "When a step fails I stop and tell you what stopped it — I don't cut the corner to finish.".into(),
                paused,
                working: false,
                background,
                held,
            };
        };
        let mut steps: Vec<(hub::Step, String)> = Vec::new();
        for t in &w.thoughts {
            let kind = match t.stage {
                Stage::Gathering | Stage::Planning => hub::Step::Plan,
                Stage::Doing => hub::Step::Doing,
                Stage::Verifying | Stage::Done => hub::Step::Checked,
                Stage::Rethinking => hub::Step::Rerouted,
                Stage::Waiting => hub::Step::Waiting,
                Stage::Stuck => hub::Step::Stuck,
            };
            steps.push((kind, sentence(&t.text)));
        }
        for s in w.steps.iter().filter(|s| s.failed.is_some()) {
            steps.push((hub::Step::Rerouted, format!("{} didn't work: {}", sentence(&s.what).trim_end_matches('.'), s.failed.clone().unwrap_or_default())));
        }
        for e in self.crew.errands() {
            steps.push((hub::Step::Delegated, format!("Handed to a worker: {}. I check what comes back before you see it.", e.name)));
        }
        let mut open = w.steps.iter().filter(|s| !s.done && s.failed.is_none());
        if let Some(cur) = open.next() {
            steps.push((hub::Step::Now, sentence(&cur.what)));
        }
        for n in open {
            steps.push((hub::Step::Next, sentence(&n.what)));
        }
        let plain_from = steps.iter().filter(|(k, _)| !matches!(k, hub::Step::Now | hub::Step::Next)).count().saturating_sub(5);
        let mins = now.saturating_sub(w.started) / 60;
        let fallback = match (&w.blocked_on, w.thoughts.iter().rev().find(|t| t.stage == Stage::Rethinking)) {
            (Some(b), _) => format!("It's waiting on {b}. If that doesn't come, I'll stop and tell you rather than guess."),
            (None, Some(r)) => format!("Already rerouted once: {}. If this way fails too, I stop and tell you what stopped it.", r.text.trim_end_matches('.')),
            (None, None) => "If a step fails I try another way, and if that fails I stop and tell you what stopped it — I don't cut the corner.".into(),
        };
        hub::NowView {
            title: sentence(&w.asked),
            since: format!("Started {} · {}", crate::localclock::hhmm(w.started, off), w.stage.label()),
            steps,
            plain_from,
            spent: Some(if mins == 0 { "under a minute".into() } else { format!("{mins} min") }),
            fallback,
            paused,
            working: !paused,
            background,
            held,
        }
    }

    fn workspace_page_live(&self, now: u64) -> String {
        // All three of these read a setting that reached nothing until
        // 19 Sep 2026. `views.first()` happened to be "Now", which is also
        // what `default_view` ships as -- so the hardcoded behaviour and the
        // shipped default agreed, and nothing looked wrong until somebody
        // changed the setting.
        let wcfg = self.tools_cfg().workspace.clone();
        let views = crate::workspace_view::shipped();
        let Some(view) = crate::workspace_view::pick(&views, &wcfg.default_view) else {
            return hub::list_page_at(Some(hub::Page::Workspace), "Workspace", "", &[]);
        };
        let items = crate::workspace_view::apply(&self.workspace, &view, now);
        let items = crate::workspace_view::still_worth_showing(&items, wcfg.keep_done_days, now);
        let by = crate::workspace_view::grouping(view.grouped_by, wcfg.group_by_project);
        let groups = crate::workspace_view::grouped(&items, by);
        let overview = crate::workspace_view::overview(&self.workspace, now);
        let mut others: Vec<String> = views.iter().map(|v| v.name.clone()).collect();
        // A name that matches nothing is said rather than ignored. Silently
        // showing a different view than the one somebody wrote down is how
        // they conclude the dashboard is broken.
        if !crate::workspace_view::is_a_view(&views, &wcfg.default_view) {
            others.push(format!(
                "(there is no view called \"{}\" -- showing {})",
                wcfg.default_view.trim(),
                view.name
            ));
        }
        hub::workspace_page(&view, &groups, &overview, &others)
    }
}

/// A blocker that is a question for you or a yes from you: it belongs in
/// "Waiting on you", not "Blocked" — nothing stopped Atlas but your answer.
fn waits_on_you(b: &crate::backlog::Blocker) -> bool {
    matches!(b, crate::backlog::Blocker::NeedsApproval | crate::backlog::Blocker::NeedsYourDecision { .. })
}

/// What a stored thing is, said rather than named.
fn kind_word(k: crate::vault::Kind) -> &'static str {
    match k {
        crate::vault::Kind::Login => "a username and password",
        crate::vault::Kind::TotpSeed => "the seed behind an authenticator code",
        crate::vault::Kind::RecoveryCodes => "recovery codes",
        crate::vault::Kind::ApiKey => "an API key",
        crate::vault::Kind::Note => "a note",
    }
}

/// Yours, or a named business.
fn space_named(name: Option<String>) -> crate::earned::Space {
    match name {
        Some(n) if !n.trim().is_empty() => crate::earned::Space::Business(n.trim().to_string()),
        _ => crate::earned::Space::Personal,
    }
}

fn fraction(part: f32, whole: f32) -> f32 {
    if whole <= 0.0 {
        return 0.0;
    }
    (part / whole).clamp(0.0, 1.0)
}

impl Daemon<'_> {
    /// A file brought in from the hub, by click rather than a typed command:
    /// an invite or calendar (.ics) goes into the calendar on your clock, a
    /// contacts file (.vcf) into the client list (duplicates named, never
    /// merged), anything else to the tray to be read like a shared file.
    fn bring_in(&mut self, name: &str, bytes: &[u8]) -> String {
        let now = crate::store::now();
        let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        let text = || String::from_utf8(bytes.to_vec()).map_err(|_| format!("{name} isn't text, so it isn't a calendar or contacts file"));
        match ext.as_str() {
            "ics" | "ical" => match text().and_then(|t| self.calendar.import_ics(&t, now, &self.home_zone())) {
                Ok((n, unknown)) => {
                    if let Err(e) = self.calendar.save(&self.store) {
                        return with_keeping(format!("{n} event{} read from {name}.", if n == 1 { "" } else { "s" }), Err(e));
                    }
                    let mut said = format!("{n} event{} added or updated from {name}.", if n == 1 { "" } else { "s" });
                    for z in unknown {
                        said.push_str(&format!(" It names a time zone I don't know (\"{z}\"), read as yours."));
                    }
                    said
                }
                Err(why) => format!("{name} didn't read as a calendar: {why}"),
            },
            "vcf" | "vcard" => {
                let mut list = crate::clients::ClientList::load(&self.store);
                match text().and_then(|t| list.import_vcf(&t, now)) {
                    Ok((added, skipped, notes)) => {
                        if let Err(e) = list.save(&self.store) {
                            return didnt_stick(&e);
                        }
                        let mut said = format!("{added} added to your clients from {name}");
                        if skipped > 0 {
                            said.push_str(&format!(", {skipped} left out (no email)"));
                        }
                        said.push('.');
                        for n in notes.iter().take(5) {
                            said.push_str(&format!(" {n}"));
                        }
                        said
                    }
                    Err(why) => format!("{name} didn't read as contacts: {why}"),
                }
            }
            // Teaching the ears, from the hub's "hearing" buttons. Each
            // button names its file with what it is for.
            "wav" => {
                let (samples, rate) = match crate::diarize::read_wav(bytes) {
                    Ok(x) => x,
                    Err(e) => return format!("{name}: {e}"),
                };
                let lower = name.to_lowercase();
                if lower.starts_with("wake-") {
                    crate::wakeword::add_take(&self.store, &samples, rate).unwrap_or_else(|e| e)
                } else if lower.starts_with("room-") || lower.starts_with("you-") {
                    let room_path = self.store.data_dir().join("hearing-room.wav");
                    let you_path = self.store.data_dir().join("hearing-you.wav");
                    let here = if lower.starts_with("room-") { &room_path } else { &you_path };
                    if let Err(e) = std::fs::write(here, bytes) {
                        return format!("couldn't keep {name}: {e}");
                    }
                    match (std::fs::read(&room_path), std::fs::read(&you_path)) {
                        (Ok(room), Ok(you)) => {
                            let current = self.tools_cfg().endpoint.vad_params();
                            let said = match crate::vadcal::from_recordings(&room, &you, current, &crate::roots::config_dir()) {
                                Ok(o) => crate::vadcal::Outcome::say(&o),
                                Err(e) => e,
                            };
                            let _ = std::fs::remove_file(&room_path);
                            let _ = std::fs::remove_file(&you_path);
                            said
                        }
                        (Ok(_), Err(_)) => "Got the room. Now bring in a recording of you talking somewhere quiet.".into(),
                        _ => "Got you. Now bring in a recording of the room with nobody talking.".into(),
                    }
                } else {
                    match crate::speaker::learn_background(&samples, rate, &self.store) {
                        Ok(n) => {
                            let bg = crate::speaker::background(&self.store);
                            if bg.ready() {
                                format!("Learned from {n} stretch{} of speech in {name}. I've heard enough voices now to tell yours apart.", if n == 1 { "" } else { "es" })
                            } else {
                                format!("Learned from {n} stretch{} of speech in {name}. {}", if n == 1 { "" } else { "es" }, crate::speaker::still_learning(&bg))
                            }
                        }
                        Err(e) => format!("{name}: {e}"),
                    }
                }
            }
            _ => match self.tray.hand_file(name, bytes, &crate::earned::Space::Personal, "the hub", None, now, self.store.root()) {
                Ok(_) => with_keeping(format!("Got {name}. I'll look at it and tell you what's in it."), self.tray.save(&self.store)),
                Err(why) => why,
            },
        }
    }
}

/// An iPhone app's version and the devices it's built for, remembered by the
/// file's size and time: the Your phone page read and unpacked the whole app
/// on every render (27 Sep 2026).
///
/// And read on a thread of its own the first time (28 Sep 2026): the first
/// visit read and unpacked the whole app -- tens of megabytes -- on the
/// daemon's loop, so everything else waited. Until it has been read this is
/// `None`, and the page shows it on the next visit.
#[doc(hidden)]
pub fn ipa_facts(f: &std::path::Path) -> Option<(String, Vec<String>)> {
    match ipa_seen(f)? {
        IpaRead::Done(facts) => facts,
        IpaRead::Reading => None,
    }
}

/// Has the app at `f` been read yet (whatever it said)?
#[doc(hidden)]
pub fn ipa_facts_ready(f: &std::path::Path) -> bool {
    matches!(ipa_seen(f), Some(IpaRead::Done(_)))
}

#[derive(Clone)]
enum IpaRead {
    Reading,
    Done(Option<(String, Vec<String>)>),
}

type IpaSeen = Option<(std::path::PathBuf, u64, std::time::SystemTime, IpaRead)>;
static IPA_SEEN: std::sync::Mutex<IpaSeen> = std::sync::Mutex::new(None);

/// Where the reading of `f` is, starting it when it hasn't begun.
fn ipa_seen(f: &std::path::Path) -> Option<IpaRead> {
    let meta = std::fs::metadata(f).ok()?;
    let (len, at) = (meta.len(), meta.modified().ok()?);
    let mut seen = IPA_SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((p, l, t, state)) = seen.as_ref() {
        if p == f && *l == len && *t == at {
            return Some(state.clone());
        }
    }
    *seen = Some((f.to_path_buf(), len, at, IpaRead::Reading));
    drop(seen);
    let path = f.to_path_buf();
    let spawned = std::thread::Builder::new().name("atlas-ipa".into()).spawn(move || {
        let facts = std::fs::read(&path).ok().and_then(|b| crate::ota::Ipa::read(&b).ok()).map(|i| (i.version, i.devices));
        let mut seen = IPA_SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((p, l, t, state)) = seen.as_mut() {
            if *p == path && *l == len && *t == at {
                *state = IpaRead::Done(facts);
            }
        }
    });
    if spawned.is_err() {
        // No thread: say nothing about it rather than stall the page.
        let mut seen = IPA_SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *seen = Some((f.to_path_buf(), len, at, IpaRead::Done(None)));
    }
    Some(IpaRead::Reading)
}

/// Put `block` at the end of a page's main content.
fn with_block(page: String, block: &str) -> String {
    match page.rfind("</main>").or_else(|| page.rfind("</body>")) {
        Some(at) => format!("{}{block}{}", &page[..at], &page[at..]),
        None => page,
    }
}

/// The phone's own language model, on the Connections page of a phone build
/// (P.7): which model is in use, how its download is going, and the button
/// to fetch it. Nothing on a computer, which uses its models folder.
fn phone_model_block(said: Option<&str>) -> String {
    let notice = said.map(|s| format!("<p class=notice role=status>{}</p>", hub::esc(s))).unwrap_or_default();
    #[cfg(feature = "phone-llm")]
    {
        let attached = crate::phonemodel::attached();
        let state = crate::phonemodel::download_said(crate::phonemodel::download_state().as_ref(), attached.as_deref());
        let button = if attached.is_none() {
            "<form class=inline method=post action='/hub/phonemodel'><input type=hidden name=what value=get>\
             <button class=primary>Get this phone's own model</button></form>\
             <p class=note>0.6 to 1.8 GB, chosen for this phone's memory — best on wifi. It stays on the phone, and what \
             you say to it never leaves.</p>"
        } else {
            ""
        };
        return format!("{notice}<h2>This phone's own model</h2><p>{}</p>{button}", hub::esc(&state));
    }
    #[allow(unreachable_code)]
    notice
}

/// A field of a query or a posted form, decoded.
fn field_of(fields: &[(String, String)], name: &str) -> Option<String> {
    fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

/// "Personal" or the business's name: the firewall, as a word.
fn area_of(space: &crate::earned::Space) -> String {
    match space {
        crate::earned::Space::Personal => "Personal".into(),
        crate::earned::Space::Business(b) => b.clone(),
    }
}

impl Daemon<'_> {
    /// The pages the locked design added (`hubpages`), from live state.
    /// `q` is the address's query: which conversation, business or view, a
    /// line to say once (`said`), or what a phone's share sheet sent.
    fn hub_page_q(&mut self, page: Page, q: &str) -> String {
        let now = crate::store::now();
        let off = crate::localclock::offset_secs();
        let fields = hub::form_fields(q);
        let said = field_of(&fields, "said");
        let when = |t: u64| {
            let today = crate::localclock::day(now, off);
            if crate::localclock::day(t, off) == today {
                crate::localclock::hhmm(t, off)
            } else {
                let (_, m, d) = crate::hubpages::ymd(crate::localclock::day(t, off) as i64);
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
                let today = crate::localclock::day(now, off) as i64;
                let mut rows = Vec::new();
                for b in &businesses {
                    for t in tasks.for_space(&crate::earned::Space::Business(b.clone())) {
                        let due_day = t.due.map(|d| crate::localclock::day(d, off) as i64);
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
                    let (y, m, d) = crate::hubpages::ymd(crate::localclock::day(now, off) as i64);
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

    /// Send one of your documents to a paired person, the way every
    /// Atlas-to-Atlas handoff goes (sealed, over the pairing; it waits on
    /// their side until they choose to keep it), and log it on the item.
    /// Pressing Send is the ask; nothing goes by itself.
    ///
    /// The checks are here; the sending (a file of up to 20 MB, over Tor if
    /// need be) is on the crew, and the page shows how it's going (27 Sep
    /// 2026: it ran inside the request and all of Atlas waited with it).
    fn send_document(&mut self, id: u64, who: &str, _now: u64) -> Reply {
        let back = |said: &str| hub::back_with(Page::Documents.href(), "", said);
        if let Some(no) = self.handed_over_refusal(&Intent::HandOver(String::new())) {
            return back(&no);
        }
        let Some(item) = self.tray.items.iter().find(|i| i.id == id).cloned() else {
            return back("That document isn't here any more.");
        };
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let Some(contact) = pairings.contacts.iter().find(|c| crate::kin::same_name(&c.name, who)).cloned() else {
            return back(&format!("You aren't paired with {who}."));
        };
        let me = crate::friends::my_name(self.tools_cfg().kin.my_name.as_deref());
        let file = item.stored_at.as_deref().map(std::path::PathBuf::from).filter(|p| p.is_file());
        // A file goes under its title; a link or words go as the text itself.
        let h = match &file {
            Some(_) => crate::household::share_with_friend(&item.title(), &me),
            None => crate::household::share_with_friend(&item.what, &me),
        };
        let link = self.peer_link(&pairings);
        let (title, to) = (item.title(), contact.name.clone());
        let label = format!("Sending \"{title}\" to {to}");
        let sent_to = to.clone();
        self.hub_errand(
            "hub-send",
            Page::Documents,
            "",
            &label,
            Box::new(move || match link.hand_note(&to, &h, file.as_deref()) {
                Ok(()) => (
                    Ok(format!("Sent \"{title}\" to {to}. It waits on their side until they choose to keep it.")),
                    "sent".to_string(),
                ),
                Err(why) => (Err(why), String::new()),
            }),
            |job| crate::daemon::HubAfter::Sent { job, doc: id, to: sent_to },
        )
    }

    /// Add a friend from the hub: the link read and them recorded here, the
    /// knock on their Atlas on the crew.
    fn add_friend_from_hub(&mut self, link: &str) -> Reply {
        match self.prepare_friend(link) {
            Err(why) => hub::back_with(Page::Friends.href(), "", &why),
            Ok(k) => {
                let label = format!("Reaching {}'s Atlas", k.keep.link.name);
                let keep = k.keep.clone();
                self.hub_errand(
                    "friend-knock",
                    Page::Friends,
                    "",
                    &label,
                    Box::new(move || {
                        let outcome = crate::friends::knock(&k.identity, &k.keep.link, &k.keep.hello, k.socks, std::time::Duration::from_secs(10));
                        (Ok(crate::daemon::knock_said(&k.keep, &outcome)), crate::daemon::knock_tag(&outcome).to_string())
                    }),
                    |job| crate::daemon::HubAfter::FriendAdd { job, keep },
                )
            }
        }
    }

    /// One page, from live state, with what just happened on it: `said=` from
    /// the address, the job it's waiting on (`job=`) -- looked at again every
    /// two seconds until it has an answer -- or answers from earlier jobs not
    /// shown yet, and anything held to show once (`hubjobs`).
    fn hub_answer(&mut self, page: Page, q: &str) -> Reply {
        let now = crate::store::now();
        let fields = hub::form_fields(q);
        // A phone's share sheet (the web app's share target) arrives as a GET
        // on Give with what was shared in the query. Taken once, then on to
        // the page's own address, so a reload doesn't add it again.
        if page == Page::Give {
            let shared: Vec<String> = ["title", "text", "url"]
                .iter()
                .filter_map(|k| field_of(&fields, k))
                .filter(|v| !v.trim().is_empty())
                .collect();
            if !shared.is_empty() {
                let what = shared.join(" — ");
                let said = match self.tray.hand(&what, &crate::earned::Space::Personal, "your phone's share sheet", now) {
                    Ok(_) => with_keeping("Sent to Atlas. Reading it now — I'll tell you what I find.".to_string(), self.tray.save(&self.store)),
                    Err(why) => why,
                };
                return hub::back_with(Page::Give.href(), "", &said);
            }
        }
        let job = field_of(&fields, "job").and_then(|j| j.parse::<u64>().ok()).and_then(|id| self.hub_jobs.look(id));
        // Waiting on a job, its state is what's said, not the line the
        // button left in the address.
        let q = if job.is_some() {
            fields
                .iter()
                .filter(|(k, _)| k != "said" && k != "job")
                .map(|(k, v)| format!("{k}={}", crate::research::urlencode(v)))
                .collect::<Vec<_>>()
                .join("&")
        } else {
            q.to_string()
        };
        let mut html = if q.is_empty() { self.hub_page(page) } else { self.hub_page_q(page, &q) };
        let mut refresh = false;
        match &job {
            Some(j) => {
                let (said, again) = crate::hubjobs::notice_for(j);
                html = hub::with_said(html, Some(&said));
                refresh = again;
            }
            None => {
                for said in self.hub_jobs.unseen_for(page) {
                    html = hub::with_said(html, Some(&said));
                }
            }
        }
        if let Some(crate::hubjobs::Flash::Said(said)) = crate::hubjobs::take_flash(&mut self.flash_once, page, now) {
            html = hub::with_said(html, Some(&said));
        }
        // A saved file set aside in the last day, on every page until then.
        let set_aside: Vec<String> =
            crate::store::set_aside_since(&self.store, now.saturating_sub(86_400)).into_iter().map(|s| s.name).collect();
        if !set_aside.is_empty() {
            let mut names = set_aside;
            names.dedup();
            html = hub::with_said(html, Some(&crate::store::set_aside_sentence(&names)));
        }
        if refresh {
            html = hub::with_refresh(html, 2);
        }
        let entries = crate::palette::catalogue();
        let html = crate::hub::with_palette(html, &entries, &self.palette);
        let html = crate::hub::with_waiting(html, self.waiting_count(now));
        let html = self.with_sidebar_names(html);
        let html = self.with_handover_banner(html);
        Reply::html(crate::hub::with_appearance(html, &self.appearance()))
    }

    /// The Updates page, from what this install knows (OPEN_GAPS 8.2).
    fn updates_view(&self, now: u64, confirming_undo: bool) -> crate::hubpages::UpdatesView {
        use crate::update_apply::AutoUpdate;
        let root = self.store.install_root();
        let a = crate::update_courier::Available::load(&self.store);
        let pending = crate::update_apply::pending(&self.store).map(|p| p.version);
        let installed = crate::release::Installed::load(&self.store);
        let available = (pending.is_none() && a.notice.is_some() && a.sequence > installed.sequence_in_this_era())
            .then(|| (a.version.clone(), !a.downloaded.is_empty(), a.size / 1_000_000));
        let mine = crate::feedback::release_sender(&self.store, &self.peer_dir).is_some_and(|(_, _, mine)| mine);
        let mut failures: Vec<(String, Vec<String>, bool, String)> = Vec::new();
        if mine {
            for r in crate::update_apply::failure_reports(&self.store) {
                let why: Vec<&str> = r.reasons.iter().map(|x| x.as_str()).filter(|x| !x.starts_with("(passed)")).collect();
                let line = format!(
                    "{} ({}), at its {}: {}",
                    if r.from.is_empty() { "this device" } else { &r.from },
                    r.platform,
                    r.stage,
                    why.join("; ")
                );
                match failures.iter_mut().find(|f| f.0 == r.version) {
                    Some(f) => f.1.push(line),
                    None => failures.push((
                        r.version.clone(),
                        vec![line],
                        crate::update_apply::is_halted(self.store.root(), &r.sha256),
                        r.sha256.clone(),
                    )),
                }
            }
        }
        crate::hubpages::UpdatesView {
            version: crate::upgrade::version().to_string(),
            heard: crate::update_courier::status(&self.store, now),
            available,
            pending,
            trial: crate::upgrade::current_trial(&root).map(|t| {
                format!("Atlas {} is on trial after replacing {}: {} of {} starts so far haven't got through.", t.new, t.previous, t.starts, crate::upgrade::TRIAL_STARTS)
            }),
            previous: crate::update_apply::previous_build(&root).map(|(v, _)| v),
            mode: match crate::update_apply::chosen_mode(&self.store) {
                Some(AutoUpdate::Automatic) => "on",
                Some(AutoUpdate::Ask) => "ask",
                Some(AutoUpdate::Off) => "off",
                None => "default",
            }
            .into(),
            history: crate::upgrade::update_history(&root)
                .iter()
                .rev()
                .take(8)
                .map(|line| {
                    let (at, what) = line.split_once(' ').unwrap_or(("", line));
                    let when = at.parse::<u64>().map(|t| crate::freshness::ago(now.saturating_sub(t))).unwrap_or_default();
                    (when, what.to_string())
                })
                .collect(),
            failures,
            confirming_undo,
            key: if crate::release::anchor_configured() {
                crate::hubpages::ReleaseKey::InTheBuild
            } else {
                match self.store.load::<String>(crate::release::KEY_CARD) {
                    card if !card.is_empty() => crate::hubpages::ReleaseKey::Made { card },
                    _ => crate::hubpages::ReleaseKey::NotMade { vault_set: self.vault.has_a_passphrase() },
                }
            },
            send: self.send_view(now),
        }
    }

    /// "Make my release key": opens the vault with what was typed (a first
    /// passphrase sets it), makes the key, keeps it, and shows the recovery
    /// key on the Updates page once. The recovery key must not go into an
    /// address, a history entry or a log, so it is held in memory for the
    /// next visit to the page (`hubjobs::Flash`) and the form is answered
    /// with a plain redirect -- a refresh can't make a second key (27 Sep
    /// 2026: the page was the POST's own answer, so it could).
    fn make_release_key(&mut self, f: &[(String, String)]) -> Result<(), String> {
        let phrase = field_of(f, "passphrase").unwrap_or_default();
        if phrase.is_empty() {
            return Err("Type your vault passphrase first.".into());
        }
        // A first passphrase chosen here would be the two-step escape
        // `would_hand_out_the_way_back` stops on the command line and the
        // Accounts page (found 27 Sep 2026: this form set one while handed
        // over, and the handover could then be taken back with it).
        if crate::handover::would_hand_out_the_way_back(
            self.handover().stance.handed_over(),
            self.vault.has_a_passphrase(),
            Some("set"),
        ) {
            return Err(crate::handover::not_yours_to_set());
        }
        if !self.vault.has_a_passphrase() && field_of(f, "again").unwrap_or_default() != phrase {
            return Err("The two passphrases were different, so nothing was made. Try again.".into());
        }
        let now = crate::store::now();
        self.vault.open(&phrase, now, &self.tools_cfg().vault)?;
        let made = crate::release::make_release_key(&mut self.vault, now)?;
        self.vault.save(&self.vault_home).map_err(|e| {
            format!("The key was made but I couldn't write your vault ({e}), so it isn't kept. Nothing was lost; try again.")
        })?;
        let kept = self.store.save(crate::release::KEY_CARD, &made.card);
        let said = match kept {
            Ok(()) => "Your release key is made and in your vault.".to_string(),
            Err(e) => format!("Your release key is made and in your vault. The card below couldn't be kept for later ({e}), so copy it now."),
        };
        crate::hubjobs::keep_flash(
            &mut self.flash_once,
            Page::Updates,
            crate::hubjobs::Flash::ReleaseKey { card: made.card, recovery: made.recovery, said },
            now,
        );
        Ok(())
    }

    /// The Improvements page's list, less the ones you said weren't worth it.
    pub(crate) fn recommendations_shown(&self, most: usize) -> Vec<crate::selfaudit::Recommendation> {
        let dropped: Vec<String> = self.store.load(RECS_DROPPED);
        crate::selfaudit::recommend(&self.signals, most + dropped.len())
            .into_iter()
            .filter(|r| !dropped.contains(&r.symptom))
            .take(most)
            .collect()
    }

    /// "Have a go" or "Not worth it" on one of Atlas's own ideas. Having a go
    /// opens the same self-work session the unprompted look opens, with the
    /// diagnosis already answered, and returns at once: the work runs in the
    /// background and shows on the Workshop, landing only with your OK.
    fn act_on_recommendation(&mut self, which: Option<String>, drop: bool) -> String {
        self.refresh_signals();
        let cfg = self.tools_cfg().self_audit.clone();
        let recs = self.recommendations_shown(cfg.most_at_once);
        // Found by what it says (27 Sep 2026: by its place in the list, which
        // recomputed here could be a different idea from the one pressed).
        let Some(r) = which.and_then(|w| recs.iter().find(|r| r.symptom == w.trim()).cloned()) else {
            return "That idea has changed since the page was drawn. Here's the list as it stands now.".into();
        };
        // Abilities never asked for: "have a go" is how to ask, not code work.
        if r.where_ == crate::signals::UNUSED && !drop {
            return self.how_to_ask_for(&self.used.unasked(crate::store::now()));
        }
        if drop {
            let mut dropped: Vec<String> = self.store.load(RECS_DROPPED);
            dropped.push(r.symptom.clone());
            return match self.store.save(RECS_DROPPED, &dropped) {
                Ok(()) => format!("Left alone: {}.", r.symptom),
                Err(e) => format!("I couldn't keep that ({e}), so it may come back."),
            };
        }
        // Working on its own code needs Atlas's source and a compiler, which an
        // installed copy doesn't have. Then the idea goes where Atlas is
        // built, as feedback, rather than a switch-off message and nothing.
        if !self.tools_cfg().pipeline.enabled {
            let words = format!("Atlas's own idea: {} -- it thinks {}. {}", r.symptom, r.cause, r.because);
            let sent = crate::feedback::compose_feedback(&words, None, crate::store::now())
                .and_then(|fb| crate::feedback::send_decided(&self.store, &self.peer_dir, fb).map(|s| s.plain()));
            return match sent {
                Ok(s) => format!("This copy can't change its own code, so the idea went to whoever builds Atlas. {s}"),
                Err(why) => format!("This copy can't change its own code, and the idea couldn't be sent on: {why}"),
            };
        }
        let session = crate::selfwork::Session::from_recommendation(&r, 0);
        self.selfwork = Some(session);
        self.work_on_myself(&r.symptom)
    }

    /// How to ask for each of these abilities: what it does, and something
    /// to say for it, from the request table.
    fn how_to_ask_for(&self, abilities: &[String]) -> String {
        let book = crate::intent::ToolBook::new(&self.cfg.commands);
        let caps = crate::capability::all();
        let mut lines = Vec::new();
        for a in abilities.iter().take(6) {
            let Some(c) = caps.iter().find(|c| c.id == a) else { continue };
            let kinds: Vec<&str> = crate::used::FOR_KIND.iter().filter(|(_, x)| x == a).map(|(k, _)| *k).collect();
            let say = book
                .entries()
                .iter()
                .find(|e| kinds.contains(&e.name.as_str()))
                .and_then(|e| e.phrases.iter().find(|p| p.split_whitespace().count() >= 2).cloned());
            lines.push(match say {
                Some(p) => format!("{} -- try \u{201c}{}\u{201d}.", sentence(c.what).trim_end_matches('.'), p),
                None => sentence(c.what),
            });
        }
        if lines.is_empty() {
            return "You've tried everything there's a way to ask for.".into();
        }
        format!("A few things I can do that you haven't asked for yet: {}", lines.join(" "))
    }

    /// Keep or bin something a friend handed you (the Documents page).
    /// Keeping is the moment it becomes something Atlas may read.
    fn take_handoff(&mut self, id: u64, keep: bool) -> String {
        let mut inbox = crate::household::Inbox::load(&self.store);
        let Some(got) = inbox.take(id) else {
            return "That's no longer waiting.".into();
        };
        if !keep {
            if let Err(e) = inbox.save(&self.store) {
                return format!("Couldn't save that: {e}");
            }
            if let Some(f) = &got.file {
                let _ = std::fs::remove_file(self.store.root().join(&f.stored_at));
            }
            return format!("Binned the one from {}.", got.from);
        }
        let space = crate::earned::Space::Personal;
        let handed = match &got.file {
            None => self.tray.hand(&got.what, &space, &got.from, got.at),
            Some(f) => match std::fs::read(self.store.root().join(&f.stored_at)) {
                // A friend's covering line is theirs, not yours: never `asked`.
                Ok(bytes) => self.tray.hand_file(&f.name, &bytes, &space, &got.from, None, got.at, self.store.root()),
                Err(e) => Err(format!("the file wasn't where it was left: {e}")),
            },
        };
        match handed {
            Ok(_) => match self.tray.save(&self.store).and_then(|()| inbox.save(&self.store)) {
                Ok(()) => {
                    if let Some(f) = &got.file {
                        let _ = std::fs::remove_file(self.store.root().join(&f.stored_at));
                    }
                    format!("Kept the one from {}. It's with your documents.", got.from)
                }
                Err(e) => format!("Couldn't save that: {e}"),
            },
            Err(e) => e,
        }
    }

    /// Where a build to send would have been downloaded to.
    fn builds_dirs(&self) -> Vec<std::path::PathBuf> {
        if let Some(d) = &self.builds_folder {
            return vec![d.clone()];
        }
        crate::firstlaunch::downloads_and_desktop()
    }

    /// "Send an update to friends": only on the releaser's own Atlas (the
    /// vault holds the release key) and once a build carries that key.
    fn send_view(&self, now: u64) -> Option<crate::hubpages::SendBuild> {
        use crate::hubpages::SendBuild;
        if self.release_anchor == [0u8; 32] || !self.vault.list().iter().any(|(n, _)| *n == crate::release::RELEASE_KEY_NAME) {
            return None;
        }
        Some(match crate::release::find_build(&self.builds_dirs()) {
            None => SendBuild::NothingFound,
            Some(Err((name, why))) => SendBuild::Unusable { name, why },
            Some(Ok(b)) => {
                let sent: String = self.store.load(crate::release::SENT_SHA);
                if sent == b.sha256 {
                    SendBuild::AlreadySent { version: b.version, sequence: self.store.load(crate::release::LAST_SEQUENCE) }
                } else {
                    SendBuild::Ready {
                        name: b.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                        ago: crate::freshness::ago(now.saturating_sub(b.modified)),
                        version: b.version,
                        platform: match b.platform {
                            "windows-aarch64" => "Windows on Arm".into(),
                            _ => "Windows".into(),
                        },
                        sha: b.sha256,
                    }
                }
            }
        })
    }

    /// "Sign and send": signs exactly the build the page showed, keeps it for
    /// friends' Atlases to fetch, and queues the notice for every release
    /// channel you own. The vault is locked again afterwards if it was locked
    /// before; the passphrase is never in an address.
    fn sign_and_send(&mut self, f: &[(String, String)]) -> String {
        let phrase = field_of(f, "passphrase").unwrap_or_default();
        if phrase.is_empty() {
            return "Type your vault passphrase first.".into();
        }
        let found = match crate::release::find_build(&self.builds_dirs()) {
            Some(Ok(b)) if field_of(f, "sha").as_deref() == Some(b.sha256.as_str()) => b,
            _ => return "The build in your Downloads changed since this page was drawn, so nothing was signed. Look at it again below.".into(),
        };
        let state = self.store.clone();
        if state.load::<String>(crate::release::SENT_SHA) == found.sha256 {
            return format!("Atlas {} already went out. Nothing was sent twice.", found.version);
        }
        let now = crate::store::now();
        let was_open = self.vault.state() == crate::vault::State::Open;
        if let Err(e) = self.vault.open(&phrase, now, &self.tools_cfg().vault) {
            return e;
        }
        let signed = crate::release::sign_build(&mut self.vault, &found, state.load(crate::release::LAST_SEQUENCE), now, &self.release_anchor);
        if !was_open {
            self.vault.lock();
        }
        let signed = match signed {
            Ok(s) => s,
            Err(why) => return why,
        };
        // The number is what makes "newer" mean something: kept before
        // anything goes out, or the next release would reuse it.
        if let Err(e) = state.save(crate::release::LAST_SEQUENCE, &signed.sequence) {
            return format!("Signed, but release number {} couldn't be kept ({e}), so nothing was sent. Try again.", signed.sequence);
        }
        if let Err(e) = crate::update_courier::keep_for_friends(self.store.root(), &found.program) {
            return format!("Signed, but the program couldn't be put aside for friends to fetch ({e}), so nothing was sent.");
        }
        let text = serde_json::to_string_pretty(&signed.signed).unwrap_or_default();
        let name = format!("atlas-release-{}.json", found.version);
        let outbox = self.store.root().join(crate::update_courier::OUTBOX);
        if let Err(e) = std::fs::create_dir_all(&outbox).and_then(|_| std::fs::write(outbox.join(&name), &text)) {
            return format!("Signed, but the notice couldn't be queued ({e}), so nothing was sent.");
        }
        let kept = state.save(crate::release::SENT_SHA, &found.sha256);
        let channels = crate::groups::Groups::load(&self.store).held.values().filter(|h| h.state.release_channel).count();
        let mut said = format!("Signed Atlas {} as release {}.", found.version, signed.sequence);
        said.push_str(if channels == 0 {
            " No group of friends gets updates from you yet, so it waits here until one does."
        } else {
            " It goes out in your release channel now; friends' Atlases fetch it from this one."
        });
        if let Err(e) = kept {
            said.push_str(&format!(" (This page may still offer it again: {e}.)"));
        }
        said
    }

    /// The Feedback page, from what's been sent and what's come in (8.14).
    fn feedback_view(&self) -> crate::hubpages::FeedbackView {
        let to = crate::feedback::release_sender(&self.store, &self.peer_dir).map(|(_, name, mine)| {
            if mine {
                "your own list".to_string()
            } else {
                name.unwrap_or_else(|| "whoever sends you Atlas".into())
            }
        });
        crate::hubpages::FeedbackView {
            to,
            failure_here: crate::update_apply::last_failure(&self.store).map(|f| format!("Atlas {}, at its {}", f.version, f.stage)),
            draft: self.feedback_draft.as_ref().map(crate::feedback::feedback_preview),
            inbox: crate::feedback::feedback_inbox(&self.store)
                .iter()
                .enumerate()
                .map(|(i, f)| (i + 1, f.from.clone(), f.version.clone(), f.platform.clone(), f.words.clone(), f.attached.is_some(), f.status.plain()))
                .collect(),
            sent: crate::feedback::feedback_sent(&self.store)
                .iter()
                .map(|f| (f.to.clone(), f.words.clone(), f.status.plain(), f.replies.iter().map(|(_, n)| n.clone()).collect()))
                .collect(),
        }
    }

    /// Each business on the roster, for its Overview.
    fn business_views(&self, now: u64) -> Vec<crate::hubpages::BusinessView> {
        let off = crate::localclock::offset_secs();
        let roster = crate::roster::Roster::load(&self.store);
        let pairings = crate::kin::Pairings::load(&self.peer_dir);
        let tasks = crate::shared_task::Tasks::load(&self.store);
        let clients = crate::clients::ClientList::load(&self.store).len();
        let held: Vec<String> = crate::firewall::Firewall::load(&self.store).waiting().iter().map(|h| h.what.clone()).collect();
        roster
            .businesses()
            .into_iter()
            .map(|name| {
                let open: Vec<(String, String, bool)> = tasks
                    .for_space(&crate::earned::Space::Business(name.clone()))
                    .into_iter()
                    .filter(|t| !t.done)
                    .map(|t| {
                        let due = t
                            .due
                            .map(|d| {
                                let (_, m, dd) = crate::hubpages::ymd(crate::localclock::day(d, off) as i64);
                                format!("{} {dd}", crate::hubpages::MONTHS[(m - 1) as usize])
                            })
                            .unwrap_or_else(|| "No date".into());
                        (t.description.clone(), due, t.due.map(|d| d < now).unwrap_or(false))
                    })
                    .collect();
                let partners: Vec<(String, bool, bool)> = roster
                    .members(&name)
                    .into_iter()
                    .map(|m| {
                        let p = pairings.has_peer(&m);
                        let t = pairings.is_trusted(&m);
                        (m, p, t)
                    })
                    .collect();
                let overdue = open.iter().filter(|t| t.2).count();
                let mut read = match (open.len(), overdue) {
                    (0, _) => "Nothing open.".to_string(),
                    (n, 0) => format!("{n} open task{}, nothing overdue.", if n == 1 { "" } else { "s" }),
                    (n, o) => format!("{n} open task{}, {o} overdue — that's the first thing.", if n == 1 { "" } else { "s" }),
                };
                if !held.is_empty() {
                    read.push_str(&format!(" {} thing{} waiting at the firewall for your yes.", held.len(), if held.len() == 1 { " is" } else { "s are" }));
                }
                let unpaired = partners.iter().filter(|p| !p.1).count();
                if unpaired > 0 {
                    read.push_str(&format!(" {unpaired} partner{} not paired, so nothing reaches them.", if unpaired == 1 { " is" } else { "s are" }));
                }
                crate::hubpages::BusinessView { name, open_tasks: open, clients, partners, held: held.clone(), read }
            })
            .collect()
    }

    /// A form posted to one of the design's added pages. Every change goes
    /// through the same module the spoken command uses, and comes back to
    /// the page with what happened said once.
    fn hub_post(&mut self, path: &str, f: &[(String, String)]) -> Reply {
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
                    let space = self.shared_space(&[who.clone()], &roster, &pairings);
                    return match self.chats.open(&who, space, &[who.clone()], &roster, &pairings) {
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
                            .map(|d| (d * 86_400 + 17 * 3600 - crate::localclock::offset_secs()).max(0) as u64);
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

/// What a hub form says when the change it made couldn't be written down
/// (28 Sep 2026). The client list and the shared tasks are read from disk for
/// each click, so a failed save is the change gone at once -- and the page
/// used to say "Added" anyway.
/// One thing taken off the Outstanding page, as `drop_outstanding` did it.
pub(crate) struct OffTheList {
    /// What it was called, as you'd say it back.
    pub title: String,
    /// Why keeping it failed, when it did: it's off for now and may come
    /// back after a restart, and that is said rather than hidden.
    pub unsaved: Option<String>,
    /// A worker was asked to stop and hasn't yet: it leaves the page when it
    /// winds down, not this second.
    pub stopping: bool,
    /// "bring back what I dropped" finds it again.
    pub can_bring_back: bool,
}

impl OffTheList {
    /// What the hub's notice says.
    pub(crate) fn said(&self) -> String {
        let mut said = if self.stopping {
            format!("Asked \"{}\" to stop. It finishes the step it's on, then it's off the list.", self.title)
        } else {
            format!("\"{}\" is off your outstanding list.", self.title)
        };
        if self.can_bring_back {
            said.push_str(" \"Bring back what I dropped\" finds it again.");
        }
        if let Some(e) = &self.unsaved {
            said.push_str(&format!(" But I couldn't save that ({e}), so it may come back after a restart."));
        }
        said
    }
}

fn didnt_stick(e: &crate::error::AtlasError) -> String {
    format!("I couldn't save that, so it didn't stick: {e}. Try again in a moment.")
}

/// Something handed in from a phone is held in memory and kept on disk; if
/// keeping it failed, it's here only until Atlas next starts -- said, rather
/// than a plain "Got it" (28 Sep 2026).
fn with_keeping(said: String, kept: crate::error::Result<()>) -> String {
    match kept {
        Ok(()) => said,
        Err(e) => format!("{said} But I couldn't save it ({e}), so it's only here until I next start -- send it again later to be sure."),
    }
}
