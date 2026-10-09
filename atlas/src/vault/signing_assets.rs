//! Prepared signing adapters. Import copies never remove owner plaintext.
//! No installed signing path or CI credential is selected automatically.
use super::{How, Kind, Vault, VaultConfig};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::{Read, Write}, path::Path};

const MAX: u64 = 4 * 1024 * 1024;
const PREFIX: &str = "signing:";

pub enum SigningUnlock<'a> { Passphrase(&'a str), RecoveryKey(&'a str) }

/// Encrypted work prepared off the daemon thread. Publishing it requires an
/// exact comparison against both the live and saved vault under one root lock.
pub(crate) struct SigningPrepared { base: serde_json::Value, pub(crate) vault: Vault }
impl SigningPrepared {
    pub(crate) fn commit(&self, store: &crate::store::Store, live: &mut Vault) -> crate::error::Result<()> {
        let _guard = store.transaction()?;
        let saved: Vault = store.load_checked(Vault::FILE)?.ok_or_else(|| crate::error::AtlasError::Platform("The saved vault disappeared; the signing original is retained".into()))?;
        let value = |vault: &Vault| serde_json::to_value(vault).map_err(|_| crate::error::AtlasError::Platform("The vault cannot be compared; no signing copy was committed".into()));
        if value(&saved)? != self.base || value(live)? != self.base {
            return Err(crate::error::AtlasError::Platform("The vault changed while signing protection was being prepared; no protected copy was saved. Try again with a fresh unlock".into()));
        }
        self.vault.save(store)?;
        let reloaded: Vault = store.load_checked(Vault::FILE)?.ok_or_else(|| crate::error::AtlasError::Platform("The protected vault disappeared after saving; the original signing file is retained".into()))?;
        if value(&reloaded)? != value(&self.vault)? { return Err(crate::error::AtlasError::Platform("The saved signing copy did not verify; its original is retained and recovery needs inspection".into())); }
        *live = self.vault.clone();
        live.lock();
        Ok(())
    }
}

pub(crate) fn prepare_signing_copy(mut vault: Vault, source: &Path, name: &str, now: u64, config: &VaultConfig, stop: &dyn Fn() -> bool, base: serde_json::Value) -> Result<SigningPrepared, String> {
    owner(&vault, now, config)?;
    if stop() { return Err("Signing protection was cancelled; the original is retained".into()); }
    let name_key = key(name)?;
    if vault.secrets.iter().any(|secret| secret.name == name_key) { return Err("That protected signing copy already exists; it was not replaced".into()); }
    let mut bytes = read_bounded_until(source, MAX, stop)?;
    if stop() { bytes.fill(0); return Err("Signing protection was cancelled; the original is retained".into()); }
    let asset = Asset { version: 1, name: name.into(), bytes: crate::b64::encode(&bytes), sha256: format!("{:x}", Sha256::digest(&bytes)) };
    let prepared = serde_json::to_string(&asset).map_err(|_| "The signing copy cannot be encoded")?;
    vault.put(&name_key, Kind::Note, &prepared, now)?;
    let mut roundtrip = decode(&mut vault, name, now)?;
    let verified = bytes == roundtrip; bytes.fill(0); roundtrip.fill(0);
    if !verified { return Err("The prepared protected copy did not match; the original is retained".into()); }
    if stop() { return Err("Signing protection was cancelled; the original is retained".into()); }
    if serde_json::to_vec(&vault).map_err(|_| "The prepared vault cannot be sized")?.len() > 512 * 1024 { return Err("This signing copy would exceed the 512 KiB interactive vault-save budget. The original is retained; use a dedicated signing workflow for larger files".into()); }
    vault.lock();
    Ok(SigningPrepared { base, vault })
}

pub(crate) fn signing_export_until(vault: &Vault, destination: &Path, unlock: SigningUnlock<'_>, now: u64, config: &VaultConfig, stop: &dyn Fn() -> bool) -> Result<String, String> {
    if stop() { return Err("Signing export was cancelled before saving a copy".into()); }
    export_protected_signing_backup(vault, destination, now, config)?;
    if stop() { return Err("An encrypted copy was saved at the selected destination, but recovery verification was cancelled. It is not a verified recovery copy".into()); }
    let mut recovered = recover_protected_signing_backup(destination, unlock, now, config)?;
    recovered.lock();
    Ok("Verified an encrypted recovery copy at your selected destination. Originals are retained. Its off-laptop location has not been verified.".into())
}

#[derive(Serialize, Deserialize)]
struct Asset { version: u32, name: String, bytes: String, sha256: String }

fn owner(vault: &Vault, now: u64, config: &VaultConfig) -> Result<(), String> {
    if !super::real_crypto_here() { return Err("Signing material requires real encryption; nothing was exported or removed".into()); }
    if !vault.proved_it() || !matches!(vault.opened_with(), Some(How::Passphrase | How::RecoveryKey)) || vault.should_lock(now, false, config) {
        return Err("Unlock the vault with your passphrase or recovery key before protecting or using signing material".into());
    }
    Ok(())
}
fn key(name: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 100 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) { return Err("The signing asset name is invalid".into()); }
    Ok(format!("{PREFIX}{name}"))
}
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    read_bounded_until(path, max, &|| false)
}
fn read_bounded_until(path: &Path, max: u64, stop: &dyn Fn() -> bool) -> Result<Vec<u8>, String> {
    let before = std::fs::symlink_metadata(path).map_err(|_| "The selected signing copy cannot be read")?;
    if before.file_type().is_symlink() || !before.is_file() || before.len() > max { return Err("The selected signing copy must be an ordinary bounded file".into()); }
    let mut options = std::fs::OpenOptions::new(); options.read(true);
    #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; options.custom_flags(0x0020_0000).share_mode(1); }
    let mut file = options.open(path).map_err(|_| "The selected signing copy cannot be opened")?;
    let opened = file.metadata().map_err(|_| "The selected signing copy cannot be inspected")?;
    if opened.file_type().is_symlink() || !opened.is_file() { return Err("The selected signing copy changed type; it was not read".into()); }
    let mut bytes = Vec::new(); let mut buffer = [0u8; 64 * 1024];
    loop {
        if stop() { bytes.fill(0); buffer.fill(0); return Err("Signing work was cancelled while reading; the original is retained".into()); }
        let count = match file.read(&mut buffer) { Ok(count) => count, Err(_) => { bytes.fill(0); buffer.fill(0); return Err("The selected signing copy cannot be read completely".into()); } };
        if count == 0 { break; }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() as u64 > max { bytes.fill(0); buffer.fill(0); return Err("The selected signing copy exceeds its bounded size; the original is retained".into()); }
    }
    buffer.fill(0);
    let after = file.metadata().map_err(|_| "The selected signing copy cannot be verified")?;
    if bytes.len() as u64 > max || bytes.len() as u64 != before.len() || before.len() != after.len() || before.modified().ok() != after.modified().ok() { bytes.fill(0); return Err("The selected signing copy changed while being read; its original is retained".into()); }
    Ok(bytes)
}
fn decode(vault: &mut Vault, name: &str, now: u64) -> Result<Vec<u8>, String> {
    let value = vault.get(&key(name)?, now).map_err(|_| "The protected signing asset cannot be opened")?;
    let asset: Asset = serde_json::from_str(&value).map_err(|_| "The protected signing asset is malformed")?;
    if asset.version != 1 || asset.name != name || asset.bytes.len() > (MAX * 2) as usize { return Err("The protected signing asset has an unsupported identity or size".into()); }
    let bytes = crate::tray::from_base64(&asset.bytes).map_err(|_| "The protected signing asset cannot be decoded")?;
    if bytes.len() as u64 > MAX || format!("{:x}", Sha256::digest(&bytes)) != asset.sha256 { return Err("The protected signing asset failed content verification".into()); }
    Ok(bytes)
}

/// Explicit copy protection only. The original file is always retained.


/// Portable ciphertext only; Windows-login wrapping is excluded. Existing
/// destinations are never overwritten, and owner recovery keys are not copied.
pub fn export_protected_signing_backup(vault: &Vault, destination: &Path, now: u64, config: &VaultConfig) -> Result<(), String> {
    owner(vault, now, config)?;
    let mut portable: Vault = serde_json::from_value(serde_json::to_value(vault).map_err(|_| "The vault cannot be exported")?).map_err(|_| "The vault cannot be copied")?;
    portable.secrets.retain(|secret| secret.name.starts_with(PREFIX));
    if portable.secrets.is_empty() { return Err("No protected signing assets are available to export".into()); }
    portable.wraps.retain(|wrap| wrap.how != How::ThisLogin);
    if !portable.has_a_passphrase() && !portable.has_a_recovery_key() { return Err("A portable owner unlock is required before exporting signing material".into()); }
    let bytes = serde_json::to_vec_pretty(&portable).map_err(|_| "The protected backup cannot be encoded")?;
    if bytes.len() as u64 > MAX * 4 { return Err("The protected signing backup exceeds its safe size budget".into()); }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(destination).map_err(|_| "The protected backup destination already exists or cannot be created; nothing was overwritten")?;
    file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|_| "The protected backup could not be saved completely; it is not a verified recovery copy".to_string())
}
pub fn recover_protected_signing_backup(source: &Path, unlock: SigningUnlock<'_>, now: u64, config: &VaultConfig) -> Result<Vault, String> {
    let bytes = read_bounded(source, MAX * 4)?;
    let mut vault: Vault = serde_json::from_slice(&bytes).map_err(|_| "The protected signing backup cannot be read")?;
    match unlock { SigningUnlock::Passphrase(passphrase) => vault.open(passphrase, now, config), SigningUnlock::RecoveryKey(recovery) => vault.open_with_recovery_key(recovery, now, config) }.map_err(|_| "The protected signing backup could not be unlocked")?;
    owner(&vault, now, config)?;
    let names: Vec<_> = vault.secrets.iter().map(|secret| secret.name.strip_prefix(PREFIX).ok_or("The backup contains an unexpected asset").map(str::to_owned)).collect::<Result<_, _>>()?;
    if names.is_empty() { return Err("The protected backup contains no signing assets".into()); }
    for name in names { let mut verified = decode(&mut vault, &name, now)?; verified.fill(0); }
    Ok(vault)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    const PHRASE: &str = "Juniper observatory lanterns cross the quiet estuary";
    const MATERIAL: &[u8] = b"synthetic signing bytes never a real key";
    fn fixture() -> (PathBuf, crate::store::Store, Vault, VaultConfig, PathBuf) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let area = std::env::temp_dir().join(format!("atlas-signing-proof-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir(&area).unwrap(); std::fs::write(area.join("synthetic-signing-fixture"), b"disposable").unwrap();
        let store = crate::store::Store::new(area.join("state")); let config = VaultConfig::default(); let mut vault = Vault::default();
        vault.open(PHRASE, 10, &config).unwrap(); vault.save(&store).unwrap(); vault.lock(); vault.open(PHRASE, 11, &config).unwrap();
        let source = area.join("synthetic-original.bin"); std::fs::write(&source, MATERIAL).unwrap(); (area, store, vault, config, source)
    }
    #[test] fn prepared_copy_waits_for_ack_and_refuses_a_changed_live_or_saved_vault() {
        let (area, store, mut vault, config, source) = fixture();
        let base = serde_json::to_value(&vault).unwrap();
        let prepared = prepare_signing_copy(vault.clone(), &source, "prepared", 12, &config, &|| false, base).unwrap();
        assert_eq!(prepared.vault.state(), super::super::State::Sealed);
        assert!(store.load_checked::<Vault>(Vault::FILE).unwrap().unwrap().secrets.is_empty());
        vault.put("later-owner-note", Kind::Note, "a later change", 12).unwrap();
        assert!(prepared.commit(&store, &mut vault).is_err());
        assert!(store.load_checked::<Vault>(Vault::FILE).unwrap().unwrap().secrets.is_empty());
        vault.save(&store).unwrap();
        let mut stale: Vault = store.load_checked(Vault::FILE).unwrap().unwrap();
        assert!(prepared.commit(&store, &mut stale).is_err());
        assert_eq!(std::fs::read(source).unwrap(), MATERIAL);
        let _ = std::fs::remove_dir_all(area);
    }
    #[test] fn prepared_copy_failed_save_keeps_both_cached_and_saved_vault_unchanged() {
        let (area, store, mut vault, config, source) = fixture();
        let base = serde_json::to_value(&vault).unwrap();
        let prepared = prepare_signing_copy(vault.clone(), &source, "prepared", 12, &config, &|| false, base.clone()).unwrap();
        std::fs::create_dir(store.root().join(format!("vault.{}.json.tmp", std::process::id()))).unwrap();
        assert!(prepared.commit(&store, &mut vault).is_err());
        assert_eq!(serde_json::to_value(&vault).unwrap(), base);
        assert_eq!(serde_json::to_value(store.load_checked::<Vault>(Vault::FILE).unwrap().unwrap()).unwrap(), base);
        assert_eq!(std::fs::read(source).unwrap(), MATERIAL);
        let _ = std::fs::remove_dir_all(area);
    }
    #[test] fn prepared_copy_cancellation_and_interactive_size_limit_preserve_original() {
        let (area, store, vault, config, source) = fixture();
        let base = serde_json::to_value(&vault).unwrap();
        assert!(prepare_signing_copy(vault.clone(), &source, "cancelled", 12, &config, &|| true, base.clone()).is_err());
        std::fs::write(&source, vec![0x51; 512 * 1024]).unwrap();
        let error = prepare_signing_copy(vault.clone(), &source, "large", 12, &config, &|| false, base.clone()).err().unwrap();
        assert!(error.contains("512 KiB"), "{error}");
        assert_eq!(std::fs::metadata(source).unwrap().len(), 512 * 1024);
        assert_eq!(serde_json::to_value(store.load_checked::<Vault>(Vault::FILE).unwrap().unwrap()).unwrap(), base);
        let _ = std::fs::remove_dir_all(area);
    }
    fn prepare_and_commit(store: &crate::store::Store, vault: &mut Vault, source: &Path, name: &str, now: u64, config: &VaultConfig) -> Result<(), String> {
        let prepared = prepare_signing_copy(vault.clone(), source, name, now, config, &|| false, serde_json::to_value(&*vault).unwrap())?;
        prepared.commit(store, vault).map_err(|error| error.to_string())?;
        vault.open(PHRASE, now, config)?;
        Ok(())
    }
    #[test] fn imported_copy_is_ciphertext_and_original_is_always_retained() {
        let (area, store, mut vault, config, source) = fixture();
        prepare_and_commit(&store, &mut vault, &source, "android-test", 12, &config).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), MATERIAL);
        let saved = std::fs::read(store.root().join("vault.json")).unwrap(); assert!(!saved.windows(MATERIAL.len()).any(|bytes| bytes == MATERIAL));
        assert_eq!(decode(&mut vault, "android-test", 12).unwrap(), MATERIAL);
        assert!(prepare_and_commit(&store, &mut vault, &source, "android-test", 12, &config).is_err());
        let _ = std::fs::remove_dir_all(area);
    }
    #[test] fn failed_vault_save_rolls_back_memory_and_keeps_original_and_saved_vault() {
        let (area, store, mut vault, config, source) = fixture(); let before = std::fs::read(store.root().join("vault.json")).unwrap();
        std::fs::create_dir(store.root().join(format!("vault.{}.json.tmp", std::process::id()))).unwrap();
        assert!(prepare_and_commit(&store, &mut vault, &source, "failure-test", 12, &config).is_err());
        assert!(vault.secrets.is_empty()); assert_eq!(std::fs::read(&source).unwrap(), MATERIAL); assert_eq!(std::fs::read(store.root().join("vault.json")).unwrap(), before);
        let _ = std::fs::remove_dir_all(area);
    }
    #[test] fn locked_and_unattended_unlocks_cannot_import_or_export_signing_assets() {
        let (area, store, mut vault, config, source) = fixture(); vault.lock();
        assert!(prepare_and_commit(&store, &mut vault, &source, "locked", 12, &config).is_err()); assert!(!area.join("export.json").exists());
        #[cfg(windows)] {
            vault.open(PHRASE, 13, &config).unwrap(); vault.seal_to_this_login(13).unwrap(); vault.save(&store).unwrap(); vault.lock(); vault.open_unattended(14).unwrap();
            assert!(prepare_and_commit(&store, &mut vault, &source, "unattended", 14, &config).is_err());
            assert!(export_protected_signing_backup(&vault, &area.join("export.json"), 14, &config).is_err());
        }
        assert_eq!(std::fs::read(source).unwrap(), MATERIAL); let _ = std::fs::remove_dir_all(area);
    }
    #[test] fn portable_ciphertext_recovers_without_login_wrap_and_never_overwrites_destination() {
        let (area, store, mut vault, config, source) = fixture();
        prepare_and_commit(&store, &mut vault, &source, "portable-test", 12, &config).unwrap();
        let recovery = vault.issue_recovery_key(13, &config).unwrap();
        #[cfg(windows)] vault.seal_to_this_login(13).unwrap();
        let export = area.join("protected-export.json"); export_protected_signing_backup(&vault, &export, 14, &config).unwrap();
        let saved = std::fs::read(&export).unwrap(); assert!(!saved.windows(MATERIAL.len()).any(|bytes| bytes == MATERIAL));
        let sealed: Vault = serde_json::from_slice(&saved).unwrap(); assert!(!sealed.sealed_to_this_login());
        assert!(export_protected_signing_backup(&vault, &export, 14, &config).is_err()); assert_eq!(std::fs::read(&export).unwrap(), saved);
        let mut from_passphrase = recover_protected_signing_backup(&export, SigningUnlock::Passphrase(PHRASE), 15, &config).unwrap();
        let mut from_recovery = recover_protected_signing_backup(&export, SigningUnlock::RecoveryKey(&recovery), 15, &config).unwrap();
        assert_eq!(decode(&mut from_passphrase, "portable-test", 15).unwrap(), MATERIAL); assert_eq!(decode(&mut from_recovery, "portable-test", 15).unwrap(), MATERIAL);
        assert!(recover_protected_signing_backup(&export, SigningUnlock::Passphrase("a completely different owner passphrase"), 15, &config).is_err());
        let mut tampered: Vault = serde_json::from_slice(&saved).unwrap(); tampered.secrets[0].sealed[0] ^= 1;
        let bad = area.join("tampered.json"); std::fs::write(&bad, serde_json::to_vec(&tampered).unwrap()).unwrap();
        assert!(recover_protected_signing_backup(&bad, SigningUnlock::Passphrase(PHRASE), 15, &config).is_err());
        assert_eq!(std::fs::read(source).unwrap(), MATERIAL); let _ = std::fs::remove_dir_all(area);
    }
}
