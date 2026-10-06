//! Appearance, voices, and the phone's view and code.
//!
//! Moved out of `hublive.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Daemon<'_> {
    /// How you've chosen the hub to look.
    pub(super) fn appearance(&self) -> crate::hub::Appearance {
        // Kept until the file changes: read for every page (27 Sep 2026).
        self.store.load_kept(crate::hub::APPEARANCE_KEY)
    }

    /// What you'd have Atlas call you: what you said out loud ("call me …")
    /// wins over the setting, the same rule `returning_cfg` follows.
    pub(super) fn what_to_call_you(&self) -> Option<String> {
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
    pub(super) fn voice_sample(&self, id: &str) -> Result<Vec<u8>, String> {
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
                crate::heard!(crate::getpieces::fetch(&piece, &root, &crate::getpieces::Tools::default(), &|_, _| {}));
                FETCHING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|f| *f != id);
            });
        }
        Err("The sample is on its way. Press play again in a moment.".into())
    }

    /// Start downloading a catalogue voice on its own thread; the Sound page
    /// shows how far it's got.
    pub(super) fn get_voice(&self, id: &str) -> String {
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
    pub(super) fn get_kokoro(&self) -> String {
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
        if let Ok(mut h) = self.phones_heard.lock().or_else(crate::crash::unpoison) {
            h.push(d);
        }
    }

    /// Keep the iPhones the code's server heard, and send each to whoever
    /// sends Atlas out. Called from the page and from the tick, so a phone
    /// added while nobody's looking still goes.
    pub(crate) fn take_heard_phones(&mut self, now: u64) -> Vec<String> {
        let heard: Vec<crate::phoneadd::Device> = match self.phones_heard.lock().or_else(crate::crash::unpoison) {
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

    pub(super) fn phone_view(&mut self, kind: Option<crate::phoneadd::Kind>) -> crate::hubpages::PhoneView {
        use crate::phoneadd::{app_file, Kind};
        let now = crate::store::now();
        // unheard-ok: returns `Vec<String>`, not a Result
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
        let ipa = ipa_file.as_deref().and_then(|p| ipa_facts(p)).map(|(version, devices)| {
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
    pub(super) fn start_phone_code(&mut self, kind: crate::phoneadd::Kind, what: &str) -> Reply {
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
                            if let Ok(mut h) = heard.lock().or_else(crate::crash::unpoison) {
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
    pub(super) fn phone_calendar(&mut self, body: &str, now: u64) -> serde_json::Value {
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
}
