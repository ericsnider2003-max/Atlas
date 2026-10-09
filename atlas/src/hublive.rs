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

pub(crate) struct SigningPending {
    crew_id: u64,
    root: std::path::PathBuf,
    deadline: std::time::Instant,
    prepared: std::sync::mpsc::Receiver<crate::vault::SigningPrepared>,
    held: Option<crate::vault::SigningPrepared>,
    ack: std::sync::mpsc::Sender<SigningAck>,
    owner: SigningOwner,
}

#[derive(Clone, PartialEq)]
struct SigningOwner { root: std::path::PathBuf, active: std::path::PathBuf, stamps: Vec<Option<(u64, Option<std::time::SystemTime>)>> }
impl SigningOwner {
    fn capture(root: std::path::PathBuf, active: std::path::PathBuf) -> Result<Self, String> {
        let store = crate::store::Store::new(&root);
        let handover: crate::handover::Handover = store.load_checked_bounded("handover", 64 * 1024).map_err(|_| "The owner access record cannot be verified")?.unwrap_or_default();
        let profiles: crate::profiles::Profiles = store.load_checked_bounded("profiles", 64 * 1024).map_err(|_| "The active owner record cannot be verified")?.unwrap_or_default();
        if handover.stance.handed_over() || profiles.active_state_dir(&root).unwrap_or_else(|| root.clone()) != active { return Err("The active owner changed; signing work is held without changing files".into()); }
        let mut stamps = Vec::new();
        for name in ["handover.json", "profiles.json"] {
            match std::fs::symlink_metadata(root.join(name)) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => stamps.push(Some((metadata.len(), metadata.modified().ok()))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => stamps.push(None),
                _ => return Err("The owner record cannot be safely identified".into()),
            }
        }
        Ok(Self { root, active, stamps })
    }
    fn unchanged(&self) -> bool { Self::capture(self.root.clone(), self.active.clone()).ok().as_ref() == Some(self) }
}

pub(crate) enum SigningAck { Saved, Refused(String), Unconfirmed(String) }
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SigningUnconfirmed { pub operation_id: String, pub operation: String, pub message: String, #[serde(default)] pub target: String }
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct SigningReceipt {
    pub operation: String, pub status: String, pub message: String,
    #[serde(default)] pub operation_id: String,
    #[serde(default)] pub prior_unconfirmed: Vec<SigningUnconfirmed>,
    #[serde(default)] pub target: String,
}

fn save_signing_receipt(store: &crate::store::Store, receipt: &SigningReceipt) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let saved = (|| -> crate::error::Result<()> {
            let _guard = store.transaction()?;
            let current: SigningReceipt = store.load_checked_bounded("signing_protection_receipt", 128 * 1024)?.ok_or_else(|| crate::error::AtlasError::Platform("The signing intent disappeared".into()))?;
            if current.operation_id != receipt.operation_id { return Err(crate::error::AtlasError::Platform("A later signing operation owns the receipt".into())); }
            store.save("signing_protection_receipt", receipt)
        })();
        match saved {
            Ok(()) => return Ok(()),
            Err(crate::error::AtlasError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock && std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(_) => return Err("Signing work ended, but its recovery receipt could not be saved. Originals are retained; inspect the selected destination before retrying".into()),
        }
    }
}

impl Daemon<'_> {
    pub(crate) fn signing_post(&mut self, what: &str, name: &str, source: &str, destination: &str, unlock: crate::server::Secret, recovery: bool, nonce: &str) -> Reply {
        let refuse = |message: &str| hub::back_with(Page::Accounts.href(), "", message);
        if !self.shown_once.spend_signing(nonce, what) { return refuse("That signing form was already sent or changed. Nothing new started."); }
        if self.handed_over_now() { return refuse("Signing protection needs the owner's access. Nothing started."); }
        if self.rehearsal { return refuse("Signing protection reads selected private files and needs a real owner action; it cannot run in rehearsal."); }
        if self.signing_pending.is_some() { return refuse("Signing protection is already in progress. Wait for its receipt before starting another action."); }
        if !matches!(what, "protect" | "export") || unlock.reveal().is_empty() || (recovery && !self.vault.has_a_recovery_key()) || (!recovery && !self.vault.has_a_passphrase()) { return refuse("Use an established owner passphrase or recovery key. Nothing started."); }
        let path = std::path::PathBuf::from(if what == "protect" { source } else { destination });
        if !crate::server::signing_local_path(path.to_str().unwrap_or_default()) { return refuse("Choose an explicit local absolute file path; network and device paths are refused. Nothing started."); }
        let install = crate::roots::state_dir();
        let owner_root = if self.store.root().starts_with(&install) { install } else { self.store.root().into() };
        let owner = match SigningOwner::capture(owner_root, self.store.root().into()) { Ok(owner) => owner, Err(_) => return refuse("The current owner/profile cannot be verified. Nothing started.") };
        let root = match std::fs::canonicalize(self.vault_home.root()) { Ok(root) => root, Err(_) => return refuse("The vault's storage cannot be identified. Nothing started.") };
        let operation_id = match crate::server::new_token() { Ok(id) => id, Err(_) => return refuse("A signing operation identity could not be made. Nothing started.") };
        let receipt = (|| -> crate::error::Result<SigningReceipt> {
            let _guard = self.vault_home.transaction()?;
            let previous: Option<SigningReceipt> = self.vault_home.load_checked_bounded("signing_protection_receipt", 128 * 1024)?;
            let mut prior_unconfirmed = Vec::new();
            if let Some(previous) = previous {
                if !matches!(previous.status.as_str(), "pending" | "unconfirmed" | "verified" | "failed") || previous.prior_unconfirmed.len() > 16 { return Err(crate::error::AtlasError::Platform("The previous signing outcome cannot be safely interpreted; it is retained for review".into())); }
                prior_unconfirmed = previous.prior_unconfirmed;
                if matches!(previous.status.as_str(), "pending" | "unconfirmed") { prior_unconfirmed.push(SigningUnconfirmed { operation_id: previous.operation_id, operation: previous.operation, message: previous.message, target: previous.target }); }
            }
            if prior_unconfirmed.len() >= 16 { return Err(crate::error::AtlasError::Platform("Signing recovery has sixteen unresolved outcomes. They are retained; review them before starting more signing work".into())); }
            let receipt = SigningReceipt { operation: what.into(), status: "pending".into(), message: "Signing work was accepted. If Atlas restarts before its final receipt, the outcome is unconfirmed; inspect the protected copies or selected destination before retrying. Originals are retained.".into(), operation_id, prior_unconfirmed, target: if what == "protect" { format!("{name} ({source})") } else { destination.to_owned() } };
            self.vault_home.save("signing_protection_receipt", &receipt)?;
            Ok(receipt)
        })();
        let receipt = match receipt { Ok(receipt) => receipt, Err(_) => return refuse("The signing recovery intent could not be saved, or unresolved history is full/unreadable. Earlier outcomes are retained. No file was read and no new signing work started.") };
        let store = self.vault_home.clone();
        let config = self.tools_cfg().vault.clone();
        let name = name.to_owned(); let operation = what.to_owned();
        let now = crate::store::now();
        let job = self.hub_jobs.start(Page::Accounts, "Protecting signing copies");
        let jobs = self.hub_jobs.clone();
        let (send, prepared) = std::sync::mpsc::channel();
        let (ack, receive_ack) = std::sync::mpsc::channel();
        let worker_owner = owner.clone();
        let declined_receipt = receipt.clone();
        let work: crate::crew::Work = Box::new(move |control| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let stopped = || control.checkpoint_until(deadline);
            let uncertain = std::cell::Cell::new(false);
            let result = (|| -> Result<String, String> {
                if stopped() { return Err("Signing work was cancelled before reading a file".into()); }
                let mut vault: crate::vault::Vault = store.load_checked(crate::vault::Vault::FILE).map_err(|_| "The saved vault cannot be read; nothing was imported or exported")?.ok_or("The owner vault is missing")?;
                if (recovery && !vault.has_a_recovery_key()) || (!recovery && !vault.has_a_passphrase()) { return Err("An established owner unlock is required; nothing was imported or exported".into()); }
                let base = serde_json::to_value(&vault).map_err(|_| "The saved vault cannot be compared")?;
                let opened = if recovery { vault.open_with_recovery_key(unlock.reveal(), now, &config) } else { vault.open(unlock.reveal(), now, &config) };
                opened.map_err(|_| "The owner unlock did not match; no signing file was read")?;
                if stopped() { return Err("Signing work was cancelled after unlock; no copy was saved".into()); }
                if operation == "protect" {
                    let prepared = crate::vault::prepare_signing_copy(vault, &path, &name, now, &config, &stopped, base)?;
                    uncertain.set(true);
                    send.send(prepared).map_err(|_| "Signing protection lost its durable save connection; the original is retained")?;
                    loop {
                        match receive_ack.recv_timeout(std::time::Duration::from_millis(20)) {
                            Ok(SigningAck::Saved) => return Ok("Saved and verified a protected signing copy in your vault. The original file is retained; it has not been migrated or removed.".into()),
                            Ok(SigningAck::Refused(error)) => { uncertain.set(false); return Err(error); },
                            Ok(SigningAck::Unconfirmed(error)) => return Err(error),
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err("Signing protection lost its save receipt; the outcome is unconfirmed and the original is retained".into()),
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => if stopped() { return Err("Signing protection was cancelled or timed out while awaiting its save receipt. The outcome is unconfirmed; inspect the vault before retrying. The original is retained".into()); },
                        }
                    }
                }
                let guard = loop { match store.transaction() {
                    Ok(guard) => break guard,
                    Err(crate::error::AtlasError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock && !stopped() => std::thread::sleep(std::time::Duration::from_millis(20)),
                    Err(_) => return Err("Vault storage is unavailable; no signing backup was exported".into()),
                }};
                let owner_store = crate::store::Store::new(&worker_owner.root);
                let owner_guard = owner_store.transaction().map_err(|_| "The owner access record is busy; no signing copy was exported")?;
                if !worker_owner.unchanged() { return Err("The owner/profile changed while signing export was prepared; nothing was exported".into()); }
                let fresh: crate::vault::Vault = store.load_checked(crate::vault::Vault::FILE).map_err(|_| "The vault cannot be rechecked")?.ok_or("The vault disappeared")?;
                if serde_json::to_value(fresh).map_err(|_| "The vault cannot be compared")? != base { return Err("The vault changed while unlocking; try the export again with a fresh owner unlock".into()); }
                let unlock_kind = if recovery { crate::vault::SigningUnlock::RecoveryKey(unlock.reveal()) } else { crate::vault::SigningUnlock::Passphrase(unlock.reveal()) };
                uncertain.set(true);
                let result = crate::vault::signing_export_until(&vault, &path, unlock_kind, now, &config, &stopped);
                drop(owner_guard); drop(guard); vault.lock(); result
            })();
            let receipt = SigningReceipt { operation, status: if result.is_ok() { "verified" } else if uncertain.get() { "unconfirmed" } else { "failed" }.into(), message: result.clone().unwrap_or_else(|error| error), ..receipt };
            let result = match save_signing_receipt(&store, &receipt) { Ok(()) => result, Err(error) => Err(error) };
            jobs.finish(job, result.clone(), false);
            result
        });
        match self.hand_off_signing(now, work) {
            Some(crew_id) => { self.signing_pending = Some(SigningPending { crew_id, root, deadline: std::time::Instant::now() + std::time::Duration::from_secs(30), prepared, held: None, ack, owner }); hub::back_with(Page::Accounts.href(), &format!("job={job}"), "Signing work is queued. The page will show its actual receipt; originals are retained.") },
            None => {
                let failed = SigningReceipt { status: "failed".into(), message: "Signing work could not be queued; no file was read or changed".into(), ..declined_receipt };
                let saved = (|| -> crate::error::Result<()> { let _guard = self.vault_home.transaction()?; let current: SigningReceipt = self.vault_home.load_checked_bounded("signing_protection_receipt", 128 * 1024)?.ok_or_else(|| crate::error::AtlasError::Platform("The signing intent disappeared".into()))?; if current.operation_id != failed.operation_id { return Err(crate::error::AtlasError::Platform("A later signing action owns the receipt".into())); } self.vault_home.save("signing_protection_receipt", &failed) })();
                if saved.is_err() { self.log.warn("Signing work did not start, but its failed-to-queue recovery receipt is still unconfirmed"); }
                self.hub_jobs.finish(job, Err(failed.message), false); refuse("Signing work could not be queued; no file was read or changed.")
            },
        }
    }

    pub(crate) fn poll_signing_protection(&mut self) {
        let Some(mut pending) = self.signing_pending.take() else { return };
        if pending.held.is_none() { match pending.prepared.try_recv() {
            Ok(prepared) => pending.held = Some(prepared),
            Err(std::sync::mpsc::TryRecvError::Empty) => {},
            Err(std::sync::mpsc::TryRecvError::Disconnected) => if !self.crew.in_hand(pending.crew_id) { return; },
        }}
        if pending.held.is_some() {
            if !self.crew.in_hand(pending.crew_id) || self.crew.cancellation_requested(pending.crew_id) || std::time::Instant::now() >= pending.deadline {
                if pending.ack.send(SigningAck::Refused("Signing protection was stopped before its save acknowledgement. Its original is retained; no new commit started".into())).is_err() { self.log.warn("Signing preparation expired after its worker disconnected; no new commit started"); }
                return;
            }
            if self.attention.is_paused() && !self.crew.cancellation_requested(pending.crew_id) && std::time::Instant::now() < pending.deadline { self.signing_pending = Some(pending); return; }
            let owner_store = crate::store::Store::new(&pending.owner.root);
            let owner_guard = match owner_store.transaction() { Ok(guard) => guard, Err(crate::error::AtlasError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => { self.signing_pending = Some(pending); return; }, Err(_) => { if pending.ack.send(SigningAck::Refused("The owner record could not be verified; no signing copy was committed".into())).is_err() { self.log.warn("Signing ownership verification failed after worker disconnect"); } return; } };
            let invalid = self.crew.cancellation_requested(pending.crew_id) || std::time::Instant::now() >= pending.deadline || self.handed_over_now() || !pending.owner.unchanged() || std::fs::canonicalize(self.vault_home.root()).ok().as_ref() != Some(&pending.root);
            let result = if invalid { Err(crate::error::AtlasError::Platform("Signing protection was cancelled or its owner/storage changed before save; the original is retained".into())) } else { pending.held.as_ref().unwrap().commit(&self.vault_home, &mut self.vault) };
            match result {
                Err(crate::error::AtlasError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => { self.signing_pending = Some(pending); return; },
                result => { let acknowledged = if invalid { SigningAck::Refused("The owner/profile changed before save; no protected copy was committed".into()) } else { match result { Ok(()) => SigningAck::Saved, Err(_) => SigningAck::Unconfirmed("The protected copy could not be durably verified. The original is retained; inspect the vault before retrying".into()) } }; if pending.ack.send(acknowledged).is_err() { self.log.warn("Signing save ended after its worker disconnected; inspect the protected copy before retrying"); } pending.held = None; },
            }
            drop(owner_guard);
        }
        if self.crew.in_hand(pending.crew_id) { self.signing_pending = Some(pending); }
    }
}

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

/// Build an immutable view without consuming notices, changing read receipts,
/// approving work, or exposing accounts. Query/action pages are never cached.
pub(crate) fn readonly_pages(daemon: &Daemon) -> Vec<(Page, Reply)> {
    let now = crate::store::now();
    [
        (Page::Now, daemon.now_page_live()),
        (Page::Outstanding, daemon.outstanding_page_live(now)),
    ].into_iter().map(|(page, html)| {
        let html = hub::with_palette(html, &crate::palette::catalogue(), &daemon.palette);
        let html = hub::with_waiting(html, daemon.waiting_count(now));
        let html = daemon.with_sidebar_names(html);
        let html = daemon.with_handover_banner(html);
        (page, Reply::html(hub::with_appearance(html, &daemon.appearance())))
    }).collect()
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
            Action::Signing { what, name, source, destination, unlock, recovery, nonce } => self.signing_post(&what, &name, &source, &destination, unlock, recovery, &nonce),
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
                match crate::clients::ClientList::load_checked(&self.store) {
                    Ok(list) => Reply::file("clients.vcf", "text/vcard; charset=utf-8", list.to_vcf()),
                    Err(error) => Reply { status: 503, body: serde_json::json!({"error": format!("Client export unavailable: {error}. Saved contacts were left untouched.")}).to_string(), ..Reply::default() },
                }
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
            Page::Outstanding => self.outstanding_page_live(now),
            Page::Activity => hub::list_page_at(
                Some(Page::Activity),
                "What I did",
                "Everything I did without being watched.",
                &self.activity_lines(now),
            ),
            Page::LookingBack => {
                let day = crate::workspace_view::day_of(&self.workspace, now);
                let page = hub::looking_back_page(&day, "Today");
                let recovery = match crate::safety::archived_recovery(self.store.root()) {
                    Ok(records) => hub::recovery_archive_section(&records),
                    Err(error) => format!("<section><h2>Saved recovery files</h2><p>Couldn't read saved recovery files: {}</p></section>", hub::esc(&error.to_string())),
                };
                with_block(page, &recovery)
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

#[cfg(test)]
mod signing_backend_tests {
    use super::*;
    const PHRASE: &str = "Juniper observatory lanterns cross the quiet estuary";
    fn area(tag: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let area = std::env::temp_dir().join(format!("atlas-signing-hub-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(area.join("state")).unwrap(); area
    }
    fn await_receipt(d: &mut Daemon, store: &crate::store::Store) -> SigningReceipt {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            d.tick(crate::store::now());
            let receipt: SigningReceipt = store.load_checked("signing_protection_receipt").unwrap().unwrap();
            if receipt.status != "pending" { return receipt; }
            assert!(std::time::Instant::now() < deadline, "signing work had no terminal durable receipt");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    #[test]
    fn owner_signing_hub_import_and_export_have_durable_verified_receipts() {
        let area = area("roundtrip"); let store = crate::store::Store::new(area.join("state"));
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(&config, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let vault_config = d.tools_cfg().vault.clone(); let now = crate::store::now();
        d.vault.open(PHRASE, now, &vault_config).unwrap(); d.vault.save(&store).unwrap(); d.vault.lock();
        let source = area.join("selected-synthetic-key.bin"); let material = b"synthetic private signing fixture only";
        std::fs::write(&source, material).unwrap();
        let nonce = d.shown_once.mark_signing("protect");
        let reply = d.signing_post("protect", "synthetic", source.to_str().unwrap(), "", crate::server::Secret::new(PHRASE), false, &nonce);
        assert!(reply.body.contains("job="), "{}", reply.body);
        assert!(d.vault.secrets.is_empty(), "the request did not synchronously read or import the file");
        let receipt = await_receipt(&mut d, &store); assert_eq!(receipt.status, "verified", "{}", receipt.message);
        assert_eq!(std::fs::read(&source).unwrap(), material);
        assert!(d.vault.secrets.iter().any(|secret| secret.name == "signing:synthetic"));
        let serialized = std::fs::read(store.root().join("vault.json")).unwrap();
        assert!(!serialized.windows(material.len()).any(|bytes| bytes == material));
        let replay = d.signing_post("protect", "other", source.to_str().unwrap(), "", crate::server::Secret::new(PHRASE), false, &nonce);
        assert!(!replay.body.contains("job="));
        // Drain the finishing crew before accepting another fresh owner action.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while d.signing_pending.is_some() { d.tick(crate::store::now()); assert!(std::time::Instant::now() < deadline); std::thread::sleep(std::time::Duration::from_millis(20)); }
        let destination = area.join("explicit-encrypted-copy.json"); let nonce = d.shown_once.mark_signing("export");
        let reply = d.signing_post("export", "", "", destination.to_str().unwrap(), crate::server::Secret::new(PHRASE), false, &nonce);
        assert!(reply.body.contains("job="), "{}", reply.body);
        let receipt = await_receipt(&mut d, &store); assert_eq!(receipt.status, "verified", "{}", receipt.message);
        let backup = std::fs::read(&destination).unwrap(); assert!(!backup.windows(material.len()).any(|bytes| bytes == material));
        crate::vault::recover_protected_signing_backup(&destination, crate::vault::SigningUnlock::Passphrase(PHRASE), crate::store::now(), &vault_config).unwrap();
        assert_eq!(std::fs::read(source).unwrap(), material);
        drop(d); let _ = std::fs::remove_dir_all(area);
    }
    #[test]
    fn an_unestablished_owner_or_wrong_unlock_never_reads_a_signing_file() {
        let area = area("owner-gate"); let store = crate::store::Store::new(area.join("state"));
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap(); let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(&config, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let missing = area.join("selected-but-unreadable.bin"); let nonce = d.shown_once.mark_signing("protect");
        let reply = d.signing_post("protect", "fixture", missing.to_str().unwrap(), "", crate::server::Secret::new(PHRASE), false, &nonce);
        assert!(!reply.body.contains("job=")); assert!(!store.root().join("signing_protection_receipt.json").exists());
        let vault_config = d.tools_cfg().vault.clone();
        d.vault.open(PHRASE, crate::store::now(), &vault_config).unwrap(); d.vault.save(&store).unwrap(); d.vault.lock();
        let nonce = d.shown_once.mark_signing("protect");
        d.signing_post("protect", "fixture", missing.to_str().unwrap(), "", crate::server::Secret::new("wrong owner phrase"), false, &nonce);
        let receipt = await_receipt(&mut d, &store);
        assert_eq!(receipt.status, "failed"); assert!(receipt.message.contains("owner unlock did not match"), "{}", receipt.message);
        assert!(store.load_checked::<crate::vault::Vault>(crate::vault::Vault::FILE).unwrap().unwrap().secrets.is_empty());
        assert!(!missing.exists()); drop(d); let _ = std::fs::remove_dir_all(area);
    }
    #[test]
    fn a_held_signing_ack_cancels_or_expires_without_waiting_for_root_ownership() {
        for cancelled in [true, false] {
            let area = area(if cancelled { "held-cancel" } else { "held-expire" });
            let store = crate::store::Store::new(area.join("state"));
            let config = crate::config::Config::load(std::path::Path::new("config")).unwrap(); let platform = crate::platform::mock::MockPlatform::new(Vec::new());
            let mut d = Daemon::new(&config, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
            let vault_config = d.tools_cfg().vault.clone(); let now = crate::store::now();
            d.vault.open(PHRASE, now, &vault_config).unwrap(); d.vault.save(&store).unwrap();
            d.vault.lock(); d.vault.open(PHRASE, now, &vault_config).unwrap();
            let before = std::fs::read(store.root().join("vault.json")).unwrap();
            let source = area.join("synthetic.bin"); std::fs::write(&source, b"synthetic retained bytes").unwrap();
            let prepared = crate::vault::prepare_signing_copy(d.vault.clone(), &source, "held", now, &vault_config, &|| false, serde_json::to_value(&d.vault).unwrap()).unwrap();
            let owner = SigningOwner::capture(store.root().into(), store.root().into()).unwrap();
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let crew_id = d.crew.hand("signing cancellation proof", now, Box::new(move |control| { started_tx.send(()).unwrap(); while !control.stopping() { std::thread::sleep(std::time::Duration::from_millis(20)); } Ok("stopped".into()) })).unwrap();
            started_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            let (lock_tx, lock_rx) = std::sync::mpsc::channel(); let (release_tx, release_rx) = std::sync::mpsc::channel();
            let locked_store = store.clone();
            let holder = std::thread::spawn(move || { let _guard = locked_store.transaction().unwrap(); lock_tx.send(()).unwrap(); release_rx.recv().unwrap(); });
            lock_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            let (_prepared_tx, receiver) = std::sync::mpsc::channel(); let (ack, receive_ack) = std::sync::mpsc::channel();
            d.signing_pending = Some(SigningPending { crew_id, root: std::fs::canonicalize(store.root()).unwrap(), deadline: if cancelled { std::time::Instant::now() + std::time::Duration::from_secs(30) } else { std::time::Instant::now() }, prepared: receiver, held: Some(prepared), ack, owner });
            if cancelled { d.crew.ask_to_stop(crew_id); }
            d.poll_signing_protection();
            assert!(d.signing_pending.is_none(), "root contention must not retain a cancelled or expired ACK");
            assert!(matches!(receive_ack.recv_timeout(std::time::Duration::from_millis(100)).unwrap(), SigningAck::Refused(_)));
            assert_eq!(std::fs::read(store.root().join("vault.json")).unwrap(), before);
            assert_eq!(std::fs::read(source).unwrap(), b"synthetic retained bytes");
            d.crew.ask_to_stop(crew_id); release_tx.send(()).unwrap(); holder.join().unwrap(); drop(d);
            let _ = std::fs::remove_dir_all(area);
        }
    }
    #[test]
    fn changed_handover_or_a_profile_away_and_back_refuses_a_prepared_import() {
        for handed in [true, false] {
            let area = area(if handed { "changed-owner" } else { "profile-away-back" }); let store = crate::store::Store::new(area.join("state"));
            let config = crate::config::Config::load(std::path::Path::new("config")).unwrap(); let platform = crate::platform::mock::MockPlatform::new(Vec::new());
            let mut d = Daemon::new(&config, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
            let vault_config = d.tools_cfg().vault.clone(); let now = crate::store::now();
            d.vault.open(PHRASE, now, &vault_config).unwrap(); d.vault.save(&store).unwrap(); d.vault.lock(); d.vault.open(PHRASE, now, &vault_config).unwrap();
            store.save("profiles", &crate::profiles::Profiles::default()).unwrap();
            let before = std::fs::read(store.root().join("vault.json")).unwrap(); let source = area.join("synthetic.bin"); std::fs::write(&source, b"unchanged synthetic original").unwrap();
            let prepared = crate::vault::prepare_signing_copy(d.vault.clone(), &source, "owner-held", now, &vault_config, &|| false, serde_json::to_value(&d.vault).unwrap()).unwrap();
            let owner = SigningOwner::capture(store.root().into(), store.root().into()).unwrap();
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let crew_id = d.crew.hand("signing owner proof", now, Box::new(move |control| { started_tx.send(()).unwrap(); while !control.stopping() { std::thread::sleep(std::time::Duration::from_millis(20)); } Ok("stopped".into()) })).unwrap();
            started_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            let (_send, receiver) = std::sync::mpsc::channel(); let (ack, receive_ack) = std::sync::mpsc::channel();
            d.signing_pending = Some(SigningPending { crew_id, root: std::fs::canonicalize(store.root()).unwrap(), deadline: std::time::Instant::now() + std::time::Duration::from_secs(30), prepared: receiver, held: Some(prepared), ack, owner });
            if handed { store.save("handover", &crate::handover::Handover { stance: crate::handover::Stance::HandedOver, ..Default::default() }).unwrap(); }
            else {
                store.save("profiles", &crate::profiles::Profiles { active: Some("another-person".into()), ..Default::default() }).unwrap();
                store.save("profiles", &crate::profiles::Profiles::default()).unwrap();
            }
            d.poll_signing_protection();
            assert!(matches!(receive_ack.recv_timeout(std::time::Duration::from_millis(100)).unwrap(), SigningAck::Refused(_)));
            assert_eq!(std::fs::read(store.root().join("vault.json")).unwrap(), before);
            assert!(d.vault.secrets.is_empty()); assert_eq!(std::fs::read(source).unwrap(), b"unchanged synthetic original");
            d.crew.ask_to_stop(crew_id); drop(d); let _ = std::fs::remove_dir_all(area);
        }
    }
    #[test]
    fn a_queued_owner_export_rechecks_ownership_before_creating_its_destination() {
        let area = area("queued-owner-export"); let store = crate::store::Store::new(area.join("state"));
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap(); let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(&config, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let vault_config = d.tools_cfg().vault.clone(); let now = crate::store::now();
        d.vault.open(PHRASE, now, &vault_config).unwrap(); d.vault.save(&store).unwrap(); d.vault.lock(); d.vault.open(PHRASE, now, &vault_config).unwrap();
        let source = area.join("synthetic.bin"); std::fs::write(&source, b"original synthetic signing material").unwrap();
        let prepared = crate::vault::prepare_signing_copy(d.vault.clone(), &source, "owner-test", now, &vault_config, &|| false, serde_json::to_value(&d.vault).unwrap()).unwrap(); prepared.commit(&store, &mut d.vault).unwrap(); d.vault.lock();
        d.crew = crate::crew::Crew::new(1);
        let (started_tx, started_rx) = std::sync::mpsc::channel(); let (release_tx, release_rx) = std::sync::mpsc::channel();
        d.crew.hand("controlled export predecessor", now, Box::new(move |control| { started_tx.send(()).unwrap(); loop { match release_rx.recv_timeout(std::time::Duration::from_millis(20)) { Ok(()) => return Ok("released".into()), Err(_) if control.stopping() => return Err("stopped".into()), Err(_) => {} } } })).unwrap();
        started_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let destination = area.join("must-not-be-created.json"); let nonce = d.shown_once.mark_signing("export");
        let reply = d.signing_post("export", "", "", destination.to_str().unwrap(), crate::server::Secret::new(PHRASE), false, &nonce);
        assert!(reply.body.contains("job=")); assert_eq!(d.crew.queued(), 1); assert!(!destination.exists());
        store.save("handover", &crate::handover::Handover { stance: crate::handover::Stance::HandedOver, ..Default::default() }).unwrap();
        release_tx.send(()).unwrap();
        let receipt = await_receipt(&mut d, &store);
        assert_eq!(receipt.status, "failed", "{}", receipt.message); assert!(receipt.message.contains("owner/profile changed"), "{}", receipt.message);
        assert!(!destination.exists()); assert_eq!(std::fs::read(source).unwrap(), b"original synthetic signing material");
        drop(d); let _ = std::fs::remove_dir_all(area);
    }
    #[test]
    fn restart_unknown_outcomes_survive_later_actions_and_full_history_refuses_unchanged() {
        let area = area("unknown-history"); let store = crate::store::Store::new(area.join("state"));
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap(); let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(&config, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let vault_config = d.tools_cfg().vault.clone(); d.vault.open(PHRASE, crate::store::now(), &vault_config).unwrap(); d.vault.save(&store).unwrap(); d.vault.lock();
        let unknown = SigningReceipt { operation: "export".into(), status: "pending".into(), message: "Earlier destination verification was interrupted".into(), operation_id: "earlier-opaque-operation".into(), target: area.join("earlier-selected-encrypted-copy.json").to_string_lossy().into_owned(), ..Default::default() };
        store.save("signing_protection_receipt", &unknown).unwrap(); drop(d);
        let mut d = Daemon::new(&config, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let missing = area.join("unreadable-selected-file.bin"); let nonce = d.shown_once.mark_signing("protect");
        let reply = d.signing_post("protect", "new-copy", missing.to_str().unwrap(), "", crate::server::Secret::new("wrong owner unlock"), false, &nonce);
        assert!(reply.body.contains("job="));
        let receipt = await_receipt(&mut d, &store); assert_eq!(receipt.status, "failed");
        assert_eq!(receipt.prior_unconfirmed.len(), 1); assert_eq!(receipt.prior_unconfirmed[0].operation_id, unknown.operation_id);
        assert_eq!(receipt.prior_unconfirmed[0].message, unknown.message);
        assert_eq!(receipt.prior_unconfirmed[0].target, unknown.target);
        assert_eq!(receipt.target, format!("new-copy ({})", missing.display()));
        let before = std::fs::read(store.root().join("signing_protection_receipt.json")).unwrap();
        assert!(save_signing_receipt(&store, &unknown).is_err(), "an earlier worker cannot overwrite a later operation's receipt");
        assert_eq!(std::fs::read(store.root().join("signing_protection_receipt.json")).unwrap(), before);
        drop(d);
        let mut full = receipt; full.prior_unconfirmed = (0..16).map(|id| SigningUnconfirmed { operation_id: format!("opaque-{id}"), operation: "export".into(), message: "Unconfirmed earlier copy".into(), target: String::new() }).collect();
        store.save("signing_protection_receipt", &full).unwrap();
        let before = std::fs::read(store.root().join("signing_protection_receipt.json")).unwrap();
        let mut d = Daemon::new(&config, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let nonce = d.shown_once.mark_signing("protect"); let reply = d.signing_post("protect", "new-copy", missing.to_str().unwrap(), "", crate::server::Secret::new(PHRASE), false, &nonce);
        assert!(!reply.body.contains("job=")); assert!(reply.body.contains("history"));
        assert_eq!(std::fs::read(store.root().join("signing_protection_receipt.json")).unwrap(), before); assert!(d.signing_pending.is_none());
        assert!(!missing.exists()); drop(d); let _ = std::fs::remove_dir_all(area);
    }
}

