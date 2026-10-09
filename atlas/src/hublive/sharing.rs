//! Documents, friends, updates, releases, recommendations, handoffs, business views.
//!
//! Moved out of `hublive.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Daemon<'_> {
    /// Send one of your documents to a paired person, the way every
    /// Atlas-to-Atlas handoff goes (sealed, over the pairing; it waits on
    /// their side until they choose to keep it), and log it on the item.
    /// Pressing Send is the ask; nothing goes by itself.
    ///
    /// The checks are here; the sending (a file of up to 20 MB, over Tor if
    /// need be) is on the crew, and the page shows how it's going (27 Sep
    /// 2026: it ran inside the request and all of Atlas waited with it).
    pub(super) fn send_document(&mut self, id: u64, who: &str, _now: u64) -> Reply {
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
    pub(super) fn add_friend_from_hub(&mut self, link: &str) -> Reply {
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
    pub(super) fn hub_answer(&mut self, page: Page, q: &str) -> Reply {
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
    pub(super) fn updates_view(&self, now: u64, confirming_undo: bool) -> crate::hubpages::UpdatesView {
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
    pub(super) fn make_release_key(&mut self, f: &[(String, String)]) -> Result<(), String> {
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
        if self.vault.set_aside.iter().any(|n| n == crate::release::RELEASE_KEY_NAME) {
            return Err(crate::release::IN_THE_OLD_VAULT.into());
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
    pub(super) fn act_on_recommendation(&mut self, which: Option<String>, drop: bool) -> String {
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
    pub(super) fn how_to_ask_for(&self, abilities: &[String]) -> String {
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
    pub(super) fn take_handoff(&mut self, id: u64, keep: bool) -> String {
        let _state = match self.store.transaction() { Ok(guard) => guard, Err(e) => return format!("Couldn't keep that change yet: {e}") };
        let mut inbox = crate::household::Inbox::load(&self.store);
        let Some(got) = inbox.take(id) else {
            return "That's no longer waiting.".into();
        };
        if !keep {
            if let Err(e) = inbox.save(&self.store) {
                return format!("Couldn't save that: {e}");
            }
            if let Some(f) = &got.file {
                crate::heard!(crate::store::remove_state_file(self.store.root(), &self.store.root().join(&f.stored_at)));
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
                        crate::heard!(crate::store::remove_state_file(self.store.root(), &self.store.root().join(&f.stored_at)));
                    }
                    format!("Kept the one from {}. It's with your documents.", got.from)
                }
                Err(e) => format!("Couldn't save that: {e}"),
            },
            Err(e) => e,
        }
    }

    /// Where a build to send would have been downloaded to.
    pub(super) fn builds_dirs(&self) -> Vec<std::path::PathBuf> {
        if let Some(d) = &self.builds_folder {
            return vec![d.clone()];
        }
        crate::firstlaunch::downloads_and_desktop()
    }

    /// "Send an update to friends": only on the releaser's own Atlas (the
    /// vault holds the release key) and once a build carries that key.
    pub(super) fn send_view(&self, now: u64) -> Option<crate::hubpages::SendBuild> {
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
    pub(super) fn sign_and_send(&mut self, f: &[(String, String)]) -> String {
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
    pub(super) fn feedback_view(&self) -> crate::hubpages::FeedbackView {
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
    pub(super) fn business_views(&self, now: u64) -> Vec<crate::hubpages::BusinessView> {
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
                                let (_, m, dd) = crate::hubpages::ymd(crate::localclock::day_here(d));
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
}
