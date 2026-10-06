//! Status and Now, and the phone app's build facts.
//!
//! Moved out of `hublive.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Daemon<'_> {
    /// How many things are actually waiting on you, for the header.
    ///
    /// One number, visible from every page. Anything that needs you and is
    /// only discoverable by going looking for it will be found late.
    pub fn waiting_count(&self, now: u64) -> usize {
        let o = crate::workspace_view::overview(&self.workspace, now);
        o.needs_you + o.overdue + self.backlog.items.iter().filter(|i| !i.done).count()
    }

    pub(super) fn status_lines(&self) -> Vec<(String, String)> {
        let r = self.plat.readings();
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

    pub(super) fn now_page_live(&self) -> String {
        hub::now_page(&self.now_view())
    }

    /// Where a live page stands, as a number that changes when what it
    /// shows does, and whether Atlas is still busy with it: `now` (the Now
    /// page's view) or `talk` (the conversation, the queue and the reply so
    /// far). Cheap -- nothing is drawn.
    pub(super) fn live_page_state(&self, which: &str) -> (u64, bool) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match which {
            "now" => {
                let v = self.now_view();
                (hub::live_version(&v), v.working)
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

    pub(super) fn now_view(&self) -> hub::NowView {
        use crate::mind::Stage;
        let now = crate::store::now();
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
            since: format!("Started {} · {}", crate::localclock::hhmm_here(w.started), w.stage.label()),
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

    pub(super) fn workspace_page_live(&self, now: u64) -> String {
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
pub(super) fn waits_on_you(b: &crate::backlog::Blocker) -> bool {
    matches!(b, crate::backlog::Blocker::NeedsApproval | crate::backlog::Blocker::NeedsYourDecision { .. })
}

/// What a stored thing is, said rather than named.
pub(super) fn kind_word(k: crate::vault::Kind) -> &'static str {
    match k {
        crate::vault::Kind::Login => "a username and password",
        crate::vault::Kind::TotpSeed => "the seed behind an authenticator code",
        crate::vault::Kind::RecoveryCodes => "recovery codes",
        crate::vault::Kind::ApiKey => "an API key",
        crate::vault::Kind::Note => "a note",
    }
}

/// Yours, or a named business.
pub(super) fn space_named(name: Option<String>) -> crate::earned::Space {
    match name {
        Some(n) if !n.trim().is_empty() => crate::earned::Space::Business(n.trim().to_string()),
        _ => crate::earned::Space::Personal,
    }
}

pub(super) fn fraction(part: f32, whole: f32) -> f32 {
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
    pub(super) fn bring_in(&mut self, name: &str, bytes: &[u8]) -> String {
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
                            crate::heard!(std::fs::remove_file(&room_path));
                            crate::heard!(std::fs::remove_file(&you_path));
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

/// Where the reading of `f` is, starting it when it hasn't begun.
pub(super) fn ipa_seen(f: &std::path::Path) -> Option<IpaRead> {
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
pub(super) fn with_block(page: String, block: &str) -> String {
    match page.rfind("</main>").or_else(|| page.rfind("</body>")) {
        Some(at) => format!("{}{block}{}", &page[..at], &page[at..]),
        None => page,
    }
}

/// The phone's own language model, on the Connections page of a phone build
/// (P.7): which model is in use, how its download is going, and the button
/// to fetch it. Nothing on a computer, which uses its models folder.
pub(super) fn phone_model_block(said: Option<&str>) -> String {
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
pub(super) fn field_of(fields: &[(String, String)], name: &str) -> Option<String> {
    fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

/// "Personal" or the business's name: the firewall, as a word.
pub(super) fn area_of(space: &crate::earned::Space) -> String {
    match space {
        crate::earned::Space::Personal => "Personal".into(),
        crate::earned::Space::Business(b) => b.clone(),
    }
}
