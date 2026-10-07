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
                Reply::ok(serde_json::json!({ "v": v.to_string(), "busy": busy }).to_string())
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
                    // Reminders still to come (item 15): the iPhone hands
                    // these to iOS when the app goes to the background, so
                    // they ring with the app closed -- nothing online.
                    "upcoming": crate::phonealarms::upcoming(&self.scheduler, now),
                });
                Reply::ok(body.to_string())
            }
            Action::VoiceSample(id) => match self.voice_sample(&id) {
                Ok(bytes) => Reply::media("audio/mpeg", bytes),
                Err(why) => Reply { status: 404, body: serde_json::json!({ "error": why }).to_string(), ..Reply::default() },
            },
            Action::PushToken(body) => {
                // `{"token": "<hex>", "env": "production"}` from the iPhone
                // app: carried to your other devices as a sync event, so the
                // laptop can reach this phone with Atlas closed (`apns`).
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let token = v.get("token").and_then(|t| t.as_str()).unwrap_or("").trim().to_lowercase();
                let env = v.get("env").and_then(|t| t.as_str()).unwrap_or("production");
                if !crate::apns::looks_like_a_token(&token) || !matches!(env, "production" | "sandbox") {
                    return Reply { status: 400, body: serde_json::json!({ "error": "That isn't a push address." }).to_string(), ..Reply::default() };
                }
                self.carry_push_address(&token, env, crate::store::now());
                Reply::ok(serde_json::json!({ "kept": true }).to_string())
            }
            Action::WebPushEndpoint(body) => {
                // `{"endpoint": "https://…", "p256dh": "…", "auth": "…"}` from
                // the Android app's UnifiedPush registration: carried to your
                // other devices, so the laptop can reach this phone with Atlas
                // closed (`webpush`).
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let get = |k: &str| v.get(k).and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
                let (endpoint, p256dh, auth) = (get("endpoint"), get("p256dh"), get("auth"));
                if !crate::webpush::looks_like_an_address(&endpoint, &p256dh, &auth) {
                    return Reply { status: 400, body: serde_json::json!({ "error": "That isn't a push address." }).to_string(), ..Reply::default() };
                }
                self.carry_web_push_address(&endpoint, &p256dh, &auth, crate::store::now());
                Reply::ok(serde_json::json!({ "kept": true }).to_string())
            }
            Action::PhoneCalendar(body) => {
                let now = crate::store::now();
                Reply::ok(self.phone_calendar(&body, now).to_string())
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
                Reply::ok(serde_json::json!({ "recent": recent, "pending": pending, "thinking": self.talk_is_thinking() }).to_string())
            }
            Action::GlanceJson => {
                let now = crate::store::now();
                let g = self.glance(now);
                Reply::ok(serde_json::to_string(&g).unwrap_or_default())
            }
            Action::Pause(on) => {
                // The same path as saying it, so a paused Atlas from the hub
                // is exactly as paused as one told out loud.
                // unheard-ok: returns `String`, not a Result
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
                Reply::ok(format!(
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
                Reply::ok(format!(
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
                Reply::ok(format!(
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
                                                crate::heard!(crate::sync::write_card(&phrase));
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
                // unheard-ok: returns `Reply`, not a Result
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
                // What you've asked it to be able to do (`growth`).
                let page = with_block(hub::recommendations_page(&recs, None, &free), &crate::growth::section(&self.store.load(crate::growth::STORE)));
                with_block(page, &self.how_you_talk_block(now))
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
                // Connecting an account leads the page (2 Oct 2026): it's
                // what "Calendars & accounts" in Settings is opened for.
                let page = self.with_vault_section(page);
                let connect = crate::connecting::section(self, None);
                hub::with_block_after_heading(page, &connect)
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


    // ---------- the lists behind the cards ----------


}


#[derive(Clone)]
enum IpaRead {
    Reading,
    Done(Option<(String, Vec<String>)>),
}

type IpaSeen = Option<(std::path::PathBuf, u64, std::time::SystemTime, IpaRead)>;
static IPA_SEEN: std::sync::Mutex<IpaSeen> = std::sync::Mutex::new(None);


impl Daemon<'_> {


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

// The rest of this module, by what it does (audit Q6, 6 Oct 2026).
mod cards;
mod phone_and_voice;
mod deck;
mod settings_and_outstanding;
mod status_and_now;
pub use status_and_now::*;
mod queries;
mod sharing;
mod posts;

