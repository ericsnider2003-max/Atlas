//! Durable local removal receipts. Local removal never proves provider revocation.
use serde::{Deserialize, Serialize};
pub const KEY: &str = "connection_removals";
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase { PendingLocal, LocalDisabled }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Receipt { pub id: u64, pub connection: String, pub provider: String, pub at: u64, pub phase: Phase, pub detail: String, #[serde(default)] pub provider_state: ProviderState }
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ProviderState { #[default] Unconfirmed, Pending, Confirmed, SharedRetained }
pub fn begin(store: &crate::store::Store, connection: &str, provider: &str, at: u64) -> Result<u64, String> {
    let _guard = store.transaction().map_err(|error| error.to_string())?;
    let mut receipts: Vec<Receipt> = store.load_checked(KEY).map_err(|error| error.to_string())?.unwrap_or_default();
    let id = receipts.iter().map(|receipt| receipt.id).max().unwrap_or(0).checked_add(1).ok_or("removal receipt identity exhausted")?;
    receipts.push(Receipt { id, connection: connection.into(), provider: provider.into(), at, phase: Phase::PendingLocal, provider_state: ProviderState::Unconfirmed,
        detail: "Local removal pending. Provider permission has not been revoked or verified.".into() });
    store.save(KEY, &receipts).map_err(|error| error.to_string())?; Ok(id)
}

fn provider_receipt(store: &crate::store::Store, id: u64, state: ProviderState, detail: &str) -> Result<(), String> {
    let _transaction = store.transaction().map_err(|error| error.to_string())?;
    let mut receipts: Vec<Receipt> = store.load_checked(KEY).map_err(|error| error.to_string())?.unwrap_or_default();
    let receipt = receipts.iter_mut().find(|receipt| receipt.id == id && receipt.provider == "Google" && receipt.phase == Phase::LocalDisabled).ok_or("local Google removal was not durably completed")?;
    receipt.provider_state = state; receipt.detail = detail.into(); store.save(KEY, &receipts).map_err(|error| error.to_string())
}

struct RevocationNet<'a> { stop: &'a dyn Fn() -> bool }
impl crate::social::apis::Net for RevocationNet<'_> {
    fn get(&self, _: &str, _: &str, _: &[(&str, &str)]) -> Result<crate::social::apis::Reply, String> { Err("revocation only permits its exact POST".into()) }
    fn post_json(&self, _: &str, _: &str, _: &[(&str, &str)], _: &str) -> Result<crate::social::apis::Reply, String> { Err("revocation only permits its exact form POST".into()) }
    fn post_form(&self, host: &str, path: &str, form: &str) -> Result<crate::social::apis::Reply, String> {
        if host != "oauth2.googleapis.com" || path != "/revoke" { return Err("unexpected revocation endpoint".into()); }
        let tool = crate::tools::ExternalTool { command: "curl".into(), args: vec!["--silent".into(), "--show-error".into(), "--max-time".into(), "10".into(), "--connect-timeout".into(), "3".into(), "-X".into(), "POST".into(), "-H".into(), "Content-Type: application/x-www-form-urlencoded".into(), "--data-binary".into(), "@-".into(), "--write-out".into(), "\n%{http_code}".into(), "https://oauth2.googleapis.com/revoke".into()], stdin_text: true, timeout_secs: 12, ..Default::default() };
        let output = tool.run_stoppable(&crate::tools::Vars::new(), Some(form), self.stop).map_err(|_| "Google revocation transport failed; permission unconfirmed".to_string())?.ok_or("Google revocation stopped; permission unconfirmed")?;
        let (body, status) = output.rsplit_once('\n').ok_or("Google revocation returned no status receipt")?;
        let status = status.trim().parse().map_err(|_| "Google revocation returned an invalid status receipt")?;
        Ok(crate::social::apis::Reply { status, body: body.into(), last_modified: None, retry_after: None })
    }
}

pub(crate) fn revoke_google_checked(store: &crate::store::Store, vault_home: &crate::store::Store, id: u64, token: &str, net: &dyn crate::social::apis::Net, stop: &dyn Fn() -> bool) -> Result<String, String> {
    if stop() { return Err("Google revocation stopped before submission; provider permission unconfirmed".into()); }
    // The same durable fence is checked under this root transaction by both
    // Google connection completion paths. No general state lock spans HTTPS.
    let reservation = store.transaction().map_err(|error| error.to_string())?;
    google_connection_allowed(store)?;
    let links: Vec<crate::connect::CalendarLink> = store.load_checked(crate::connect::CALENDAR_LINKS).map_err(|error| error.to_string())?.unwrap_or_default();
    let vault: crate::vault::Vault = vault_home.load_checked(crate::vault::Vault::FILE).map_err(|error| error.to_string())?.unwrap_or_default();
    if links.iter().any(|link| crate::oauthlink::parse_calendar_key(&link.url).is_some_and(|(provider, _)| provider == crate::oauthlink::Provider::Google)) || vault.secrets.iter().any(|secret| secret.name == crate::social::VAULT_YOUTUBE_OAUTH) {
        let detail = "Local access removed. Google grant retained because another local service may share it; provider permission is unconfirmed.";
        provider_receipt(store, id, ProviderState::SharedRetained, detail)?; return Err(detail.into());
    }
    provider_receipt(store, id, ProviderState::Pending, "Local access removed; Google revocation pending. A restart or lost worker leaves provider permission unconfirmed; no automatic retry.")?;
    drop(reservation);
    let result = if stop() { Err("revocation stopped".into()) } else { crate::oauthlink::revoke_google(net, token) };
    let (state, detail) = match result { Ok(()) if !stop() => (ProviderState::Confirmed, "Local access removed and Google confirmed the grant revoked.".to_string()), _ => (ProviderState::Unconfirmed, "Local access removed; Google revocation failed, stopped or returned without a confirmed receipt. Review provider permissions; no automatic retry.".to_string()) };
    provider_receipt(store, id, state, &detail)?;
    if state == ProviderState::Confirmed { Ok(detail) } else { Err(detail) }
}

pub(crate) fn google_connection_allowed(store: &crate::store::Store) -> Result<(), String> {
    let receipts: Vec<Receipt> = store.load_checked(KEY).map_err(|error| format!("Google grant coverage is unreadable; nothing connected: {error}"))?.unwrap_or_default();
    if receipts.iter().any(|receipt| receipt.provider == "Google" && receipt.provider_state == ProviderState::Pending) {
        return Err("A Google grant removal is pending or its outcome is unconfirmed after interruption. Nothing connected; finish checking that removal before authorizing a new Google connection.".into());
    }
    Ok(())
}

/// Startup-only: the serving entry point calls this after taking its native
/// exclusive running lock and before creating any daemon or worker. Ordinary
/// daemon construction must never clear a possibly live provider reservation.
pub fn reconcile_interrupted_provider_removals(store: &crate::store::Store) -> Result<usize, String> {
    let _transaction = store.transaction().map_err(|error| error.to_string())?;
    let mut receipts: Vec<Receipt> = store.load_checked(KEY).map_err(|error| error.to_string())?.unwrap_or_default();
    let mut changed = 0;
    for receipt in receipts.iter_mut().filter(|receipt| receipt.provider == "Google" && receipt.provider_state == ProviderState::Pending) {
        receipt.provider_state = ProviderState::Unconfirmed;
        receipt.detail = "The previous Atlas worker is gone. Local access remains disabled; Google's revocation outcome is unconfirmed. No revocation was retried. A fresh owner-approved Google connection is allowed; review the provider's permissions if needed.".into();
        changed += 1;
    }
    if changed > 0 { store.save(KEY, &receipts).map_err(|error| error.to_string())?; }
    Ok(changed)
}

pub(crate) fn google_revocation_work(store: crate::store::Store, vault_home: crate::store::Store, id: u64, token: String) -> crate::crew::Work {
        let work: crate::crew::Work = Box::new(move |control| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let stop = || control.stopping() || std::time::Instant::now() >= deadline;
            revoke_google_checked(&store, &vault_home, id, &token, &RevocationNet { stop: &stop }, &stop)
        });
        work
}
pub fn complete(store: &crate::store::Store, id: u64, detail: &str) -> Result<(), String> {
    let _guard = store.transaction().map_err(|error| error.to_string())?;
    let mut receipts: Vec<Receipt> = store.load_checked(KEY).map_err(|error| error.to_string())?.unwrap_or_default();
    let receipt = receipts.iter_mut().find(|receipt| receipt.id == id).ok_or("saved removal intent disappeared")?;
    receipt.phase = Phase::LocalDisabled; receipt.detail = detail.into();
    store.save(KEY, &receipts).map_err(|error| error.to_string())
}
/// Restore the original in-memory vault when its durable save fails. No provider call.
pub fn remove_credentials(vault_home: &crate::store::Store, vault: &mut crate::vault::Vault, names: &[String]) -> Result<(), String> {
    let previous = vault.clone();
    vault.secrets.retain(|secret| !names.contains(&secret.name));
    if let Err(error) = vault.save(vault_home) { *vault = previous; return Err(format!("credential removal was not saved; original vault restored: {error}")); }
    Ok(())
}
pub fn provider_check(provider: &str) -> String {
    let page = match provider { "Google" => "https://myaccount.google.com/connections", "Microsoft" => crate::oauthlink::MICROSOFT_PERMISSIONS, "Meta" => "https://www.facebook.com/settings?tab=business_tools", _ => "the provider's account permissions page" };
    format!("Provider permission remains unconfirmed. Review and revoke it on {page}; shared grants may still serve another connection. An already running operation may still finish.")
}

#[cfg(test)]
mod durable_local_removal {
    use super::*;
    fn fixture() -> (std::path::PathBuf, crate::store::Store, crate::vault::Vault) {
        let root = std::env::temp_dir().join(format!("atlas-removal-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = crate::store::Store::new(root.join("state"));
        let mut vault = crate::vault::Vault::default();
        vault.secrets.push(crate::vault::Secret { name: "synthetic provider".into(), kind: crate::vault::Kind::Login, sealed: vec![1,2,3], real: false, added: 1, last_used: 1 });
        (root, store, vault)
    }
    struct ProviderFixture { calls: std::cell::Cell<usize>, status: u16, body: &'static str }
    impl crate::social::apis::Net for ProviderFixture {
        fn get(&self, _: &str, _: &str, _: &[(&str, &str)]) -> Result<crate::social::apis::Reply, String> { panic!("revocation must only POST") }
        fn post_json(&self, _: &str, _: &str, _: &[(&str, &str)], _: &str) -> Result<crate::social::apis::Reply, String> { panic!("revocation must only form POST") }
        fn post_form(&self, host: &str, path: &str, form: &str) -> Result<crate::social::apis::Reply, String> {
            assert_eq!((host, path, form), ("oauth2.googleapis.com", "/revoke", "token=synthetic")); self.calls.set(self.calls.get()+1);
            Ok(crate::social::apis::Reply { status: self.status, body: self.body.into(), last_modified: None, retry_after: None })
        }
    }
    #[test]
    fn google_provider_receipts_confirm_only_valid_results_and_keep_failure_unconfirmed() {
        for (status, body, expected) in [(200, "", ProviderState::Confirmed), (400, r#"{"error":"invalid_token"}"#, ProviderState::Confirmed), (500, r#"{"error":"invalid_token"}"#, ProviderState::Unconfirmed)] {
            let (root, store, vault) = fixture(); vault.save(&store).unwrap(); let id = begin(&store, "Synthetic account", "Google", 1).unwrap(); complete(&store, id, "Local access removed; provider unconfirmed").unwrap();
            let net = ProviderFixture { calls: std::cell::Cell::new(0), status, body };
            let result = revoke_google_checked(&store, &store, id, "synthetic", &net, &|| false); assert_eq!(result.is_ok(), expected == ProviderState::Confirmed); assert_eq!(net.calls.get(), 1);
            let receipts: Vec<Receipt> = store.load_checked(KEY).unwrap().unwrap(); assert_eq!(receipts[0].provider_state, expected); assert!(!receipts[0].detail.contains("synthetic")); crate::heard!(std::fs::remove_dir_all(root));
        }
    }
    #[test]
    fn google_shared_or_unknown_coverage_prevents_provider_calls() {
        for corrupt in [false, true] {
            let (root, store, vault) = fixture(); vault.save(&store).unwrap(); let id = begin(&store, "Synthetic account", "Google", 1).unwrap(); complete(&store, id, "Local access removed; provider unconfirmed").unwrap();
            let links = vec![crate::connect::CalendarLink { name: "Shared Google calendar".into(), url: crate::oauthlink::calendar_key(crate::oauthlink::Provider::Google, "other@example.com"), ..Default::default() }]; store.save(crate::connect::CALENDAR_LINKS, &links).unwrap();
            if corrupt { std::fs::write(store.root().join(format!("{}.json", crate::connect::CALENDAR_LINKS)), b"broken").unwrap(); }
            let net = ProviderFixture { calls: std::cell::Cell::new(0), status: 200, body: "" };
            let result = revoke_google_checked(&store, &store, id, "synthetic", &net, &|| false); if corrupt { assert!(result.is_err()); } else { assert!(result.unwrap_err().contains("share")); }
            assert_eq!(net.calls.get(), 0); let receipts: Vec<Receipt> = store.load_checked(KEY).unwrap().unwrap(); assert_ne!(receipts[0].provider_state, ProviderState::Confirmed); crate::heard!(std::fs::remove_dir_all(root));
        }
    }
    #[test]
    fn google_pending_receipt_survives_restart_without_replaying_transport() {
        let (root, store, vault) = fixture(); vault.save(&store).unwrap(); let id = begin(&store, "Synthetic account", "Google", 1).unwrap(); complete(&store, id, "Local access removed").unwrap();
        provider_receipt(&store, id, ProviderState::Pending, "Provider outcome unconfirmed; no automatic retry").unwrap();
        let restarted = crate::store::Store::new(store.root()); let receipts: Vec<Receipt> = restarted.load_checked(KEY).unwrap().unwrap(); assert_eq!(receipts[0].phase, Phase::LocalDisabled); assert_eq!(receipts[0].provider_state, ProviderState::Pending); assert!(receipts[0].detail.contains("unconfirmed"));
        let net = ProviderFixture { calls: std::cell::Cell::new(0), status: 200, body: "" }; assert!(revoke_google_checked(&store, &store, id, "synthetic", &net, &|| true).is_err()); assert_eq!(net.calls.get(), 0); crate::heard!(std::fs::remove_dir_all(root));
    }
    #[test]
    fn google_busy_receipt_save_prevents_provider_submission() {
        let (root, store, vault) = fixture(); vault.save(&store).unwrap(); let id = begin(&store, "Synthetic account", "Google", 1).unwrap(); complete(&store, id, "Local access removed").unwrap();
        let owned = store.clone(); let (ready, arrived) = std::sync::mpsc::sync_channel(1); let (release, wait) = std::sync::mpsc::sync_channel(1); let holder = std::thread::spawn(move || { let _guard = owned.transaction().unwrap(); ready.send(()).unwrap(); wait.recv().unwrap(); }); arrived.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let net = ProviderFixture { calls: std::cell::Cell::new(0), status: 200, body: "" }; let result = revoke_google_checked(&store, &store, id, "synthetic", &net, &|| false); release.send(()).unwrap(); holder.join().unwrap(); assert!(result.is_err()); assert_eq!(net.calls.get(), 0); crate::heard!(std::fs::remove_dir_all(root));
    }
    #[test]
    fn google_cancellation_after_submission_never_claims_provider_confirmation() {
        let (root, store, vault) = fixture(); vault.save(&store).unwrap(); let id = begin(&store, "Synthetic account", "Google", 1).unwrap(); complete(&store, id, "Local access removed").unwrap();
        let checks = std::cell::Cell::new(0); let stop = || { checks.set(checks.get()+1); checks.get() >= 3 };
        let net = ProviderFixture { calls: std::cell::Cell::new(0), status: 200, body: "" }; assert!(revoke_google_checked(&store, &store, id, "synthetic", &net, &stop).is_err()); assert_eq!(net.calls.get(), 1);
        let receipts: Vec<Receipt> = store.load_checked(KEY).unwrap().unwrap(); assert_eq!(receipts[0].phase, Phase::LocalDisabled); assert_eq!(receipts[0].provider_state, ProviderState::Unconfirmed); crate::heard!(std::fs::remove_dir_all(root));
    }
    struct HeldProvider { entered: std::sync::mpsc::SyncSender<()>, release: std::sync::mpsc::Receiver<()> }
    impl crate::social::apis::Net for HeldProvider {
        fn get(&self, _: &str, _: &str, _: &[(&str, &str)]) -> Result<crate::social::apis::Reply, String> { panic!("unexpected GET") }
        fn post_json(&self, _: &str, _: &str, _: &[(&str, &str)], _: &str) -> Result<crate::social::apis::Reply, String> { panic!("unexpected JSON") }
        fn post_form(&self, host: &str, path: &str, _: &str) -> Result<crate::social::apis::Reply, String> { assert_eq!((host, path), ("oauth2.googleapis.com", "/revoke")); self.entered.send(()).unwrap(); self.release.recv_timeout(std::time::Duration::from_secs(5)).unwrap(); Ok(crate::social::apis::Reply { status: 200, ..Default::default() }) }
    }
    #[test]
    fn pending_provider_reservation_atomically_blocks_both_new_google_completion_paths() {
        let (root, store, vault) = fixture(); vault.save(&store).unwrap(); let id = begin(&store, "Synthetic account", "Google", 1).unwrap(); complete(&store, id, "Local access removed").unwrap();
        let before = std::fs::read(store.root().join(crate::vault::Vault::FILE.to_string()+".json")).unwrap();
        let (entered, arrived) = std::sync::mpsc::sync_channel(1); let (release, wait) = std::sync::mpsc::sync_channel(1); let worker_store = store.clone();
        let worker = std::thread::spawn(move || revoke_google_checked(&worker_store, &worker_store, id, "synthetic", &HeldProvider { entered, release: wait }, &|| false));
        arrived.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let mut cfg = crate::config::Config::load(std::path::Path::new("config")).unwrap(); cfg.tools.as_mut().unwrap().browser.launch = None;
        let platform = crate::platform::mock::MockPlatform::new(vec![crate::platform::Monitor { id: 1, x: 0, y: 0, width: 1280, height: 800, primary: true }]);
        let mut daemon = crate::daemon::Daemon::new(&cfg, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let calendar = crate::connecting::keep_sign_in(&mut daemon, &crate::oauthlink::SignedIn { provider: crate::oauthlink::Provider::Google, email: "new@example.com".into(), refresh_token: "new-synthetic-token".into() }, 1);
        let youtube = daemon.social_news("social-google", &crate::crew::Ending::Done(Ok(r#"{"client_id":"synthetic","client_secret":"synthetic","refresh_token":"new-synthetic-token","obtained":1}"#.into())), 1);
        let after = std::fs::read(store.root().join(crate::vault::Vault::FILE.to_string()+".json")).unwrap(); let links: Vec<crate::connect::CalendarLink> = store.load_checked(crate::connect::CALENDAR_LINKS).unwrap().unwrap_or_default();
        release.send(()).unwrap(); let result = worker.join().unwrap();
        assert!(result.is_ok()); assert!(calendar.contains("Nothing connected"), "{calendar}"); assert!(youtube.unwrap().contains("Nothing connected")); assert_eq!(before, after); assert!(links.is_empty()); assert_eq!(daemon.crew.active(), 0); assert!(google_connection_allowed(&store).is_ok());
        drop(daemon); crate::heard!(std::fs::remove_dir_all(root));
    }
    #[test]
    fn exclusive_startup_reconciles_lost_worker_without_provider_retry_and_allows_fresh_connect() {
        let (root, store, vault) = fixture(); vault.save(&store).unwrap(); let id = begin(&store, "Synthetic account", "Google", 1).unwrap(); complete(&store, id, "Local access removed").unwrap(); provider_receipt(&store, id, ProviderState::Pending, "Previous provider worker pending").unwrap();
        let mut cfg = crate::config::Config::load(std::path::Path::new("config")).unwrap(); cfg.tools.as_mut().unwrap().browser.launch = None;
        let platform = crate::platform::mock::MockPlatform::new(vec![crate::platform::Monitor { id: 1, x: 0, y: 0, width: 1280, height: 800, primary: true }]);
        let other_daemon = crate::daemon::Daemon::new(&cfg, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        assert!(google_connection_allowed(&store).is_err(), "an unowned extra daemon must not clear a live reservation"); drop(other_daemon);
        let only = crate::onlyone::OnlyOne::at(&store.data_dir());
        let previous_owner = only.hold(crate::store::now()).unwrap();
        let rival = crate::onlyone::OnlyOne::at(&store.data_dir());
        assert!(rival.hold(crate::store::now()).is_err(), "a rival cannot acquire startup ownership while the existing native lease is held");
        assert!(google_connection_allowed(&store).is_err());
        let still_pending: Vec<Receipt> = store.load_checked(KEY).unwrap().unwrap(); assert_eq!(still_pending[0].provider_state, ProviderState::Pending);
        drop(previous_owner);
        let owned = only.hold(crate::store::now()).unwrap();
        assert_eq!(reconcile_interrupted_provider_removals(&store).unwrap(), 1); assert_eq!(reconcile_interrupted_provider_removals(&store).unwrap(), 0); assert!(google_connection_allowed(&store).is_ok());
        let receipts: Vec<Receipt> = store.load_checked(KEY).unwrap().unwrap(); assert_eq!(receipts[0].provider_state, ProviderState::Unconfirmed); assert!(receipts[0].detail.contains("No revocation was retried"));
        let mut daemon = crate::daemon::Daemon::new(&cfg, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let outcome = crate::connecting::keep_sign_in(&mut daemon, &crate::oauthlink::SignedIn { provider: crate::oauthlink::Provider::Google, email: "new@example.com".into(), refresh_token: "fresh-synthetic-token".into() }, crate::store::now());
        assert!(outcome.starts_with("Connected"), "{outcome}"); let links: Vec<crate::connect::CalendarLink> = store.load_checked(crate::connect::CALENDAR_LINKS).unwrap().unwrap(); assert_eq!(links.len(), 1); assert_eq!(daemon.crew.active(), 0);
        drop(daemon); drop(owned); crate::heard!(std::fs::remove_dir_all(root));
    }
    #[test]
    fn failed_vault_save_restores_memory_and_leaves_durable_pending_receipt() {
        let (root, store, mut vault) = fixture();
        let id = begin(&store, "Synthetic account", "Google", 1).unwrap();
        let original = vault.secrets.clone();
        std::fs::create_dir_all(root.join("state/vault.json")).unwrap();
        assert!(remove_credentials(&store, &mut vault, &["synthetic provider".into()]).is_err());
        assert_eq!(vault.secrets, original);
        let restarted: Vec<Receipt> = crate::store::Store::new(root.join("state")).load_checked(KEY).unwrap().unwrap();
        assert_eq!(restarted[0].id, id); assert_eq!(restarted[0].phase, Phase::PendingLocal);
        crate::heard!(std::fs::remove_dir_all(root));
    }
    #[test]
    fn unreadable_intent_refuses_removal_before_changing_the_vault() {
        let (root, store, vault) = fixture(); std::fs::create_dir_all(root.join("state")).unwrap();
        std::fs::write(root.join("state/connection_removals.json"), b"broken").unwrap();
        assert!(begin(&store, "Synthetic account", "Google", 1).is_err());
        assert_eq!(vault.secrets.len(), 1);
        assert_eq!(std::fs::read(root.join("state/connection_removals.json")).unwrap(), b"broken");
        crate::heard!(std::fs::remove_dir_all(root));
    }
    #[test]
    fn successful_local_removal_survives_restart_without_claiming_provider_revocation() {
        let (root, store, mut vault) = fixture();
        let id = begin(&store, "Synthetic account", "Google", 1).unwrap();
        remove_credentials(&store, &mut vault, &["synthetic provider".into()]).unwrap();
        let detail = provider_check("Google"); complete(&store, id, &detail).unwrap();
        let disk_vault: crate::vault::Vault = store.load_checked(crate::vault::Vault::FILE).unwrap().unwrap();
        assert!(disk_vault.secrets.is_empty());
        let receipts: Vec<Receipt> = store.load_checked(KEY).unwrap().unwrap();
        assert_eq!(receipts[0].phase, Phase::LocalDisabled); assert_eq!(receipts[0].detail, detail);
        assert!(receipts[0].detail.contains("remains unconfirmed"));
        crate::heard!(std::fs::remove_dir_all(root));
    }
}
