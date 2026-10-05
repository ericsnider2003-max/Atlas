//! Proving an update is really from you — the release signature.
//!
//! Atlas already has symmetric crypto: the household key seals a bundle so only
//! your own devices can open it. That proves *from this household*. It cannot
//! prove *authored by you, specifically* — everyone in the household holds the
//! same key and could forge with it. An update channel that runs over a mesh
//! needs the stronger statement, because a forged update is code execution on
//! every device that takes it. So a release is signed with an **ed25519** key
//! whose **private half never leaves the build machine**, and whose **public
//! half is baked into every Atlas as the one trust anchor**. A device stages an
//! update only if its signature verifies against that anchor.
//!
//! This module is that primitive — sign, verify, and the trust anchor. The
//! signed *manifest* that lists the per-platform artifacts and their hashes is
//! built on top of it (next step of the update courier), and the daemon wires
//! the check into the sync pass after that. Until then this is the crypto root,
//! proven by its own tests and not yet reached from production — deliberately,
//! because the root must be right before anything is allowed to depend on it.
//!
//! ## The safe default
//!
//! The baked-in anchor starts as an all-zero **placeholder**, because the real
//! keypair is generated once, by you, as part of build-account setup. A
//! placeholder is treated as *no anchor configured*: verification returns
//! `NoAnchor` and the updater trusts **nothing**. A forgotten or unset key can
//! therefore never masquerade as trust — the failure mode is "refuse every
//! update", never "accept an unsigned one".

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// The public half of the release key — the single trust anchor every Atlas
/// verifies updates against. The private half lives only on the build machine
/// and never ships.
///
/// All-zero until the real keypair is generated (build-account setup). See the
/// module docs: a placeholder means "trust nothing", never "trust everything".
///
/// Read from `release-keys.txt` at compile time (27 Sep 2026), so putting the
/// key in is one line in a text file rather than an edit to Rust source. A
/// line that isn't 64 hex characters fails the build instead of shipping a
/// wrong key.
pub const RELEASE_PUBLIC_KEY: [u8; 32] = key_in(KEYS_FILE, b"release");

/// The build's key card (`release-keys.txt`).
const KEYS_FILE: &str = include_str!("../release-keys.txt");

/// The 32 bytes after `name=` at the start of a line, or the placeholder when
/// the line says `none` or isn't there. Runs at compile time.
const fn key_in(file: &str, name: &[u8]) -> [u8; 32] {
    let b = file.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < b.len() {
        let mut j = 0;
        let mut matches = true;
        while j < name.len() {
            if i + j >= b.len() || b[i + j] != name[j] {
                matches = false;
                break;
            }
            j += 1;
        }
        if matches && i + name.len() < b.len() && b[i + name.len()] == b'=' {
            let s = i + name.len() + 1;
            if s + 4 <= b.len() && b[s] == b'n' && b[s + 1] == b'o' && b[s + 2] == b'n' && b[s + 3] == b'e' {
                return out;
            }
            assert!(s + 64 <= b.len(), "release-keys.txt: a key is 64 hex characters");
            let mut k = 0;
            while k < 32 {
                out[k] = hex_value(b[s + 2 * k]) * 16 + hex_value(b[s + 2 * k + 1]);
                k += 1;
            }
            return out;
        }
        while i < b.len() && b[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }
    out
}

const fn hex_value(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("release-keys.txt: a key is 64 hex characters"),
    }
}

/// Where the release key is kept in your vault.
pub const RELEASE_KEY_NAME: &str = "atlas-release-key";

/// The two public halves, as the lines `release-keys.txt` takes. Kept (in
/// the store) when the key is made, so it can be shown again without the
/// vault: it's public by design.
pub const KEY_CARD: &str = "release_key_card";

/// A release key just made: its card, and the recovery key to write down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeKey {
    /// `release=<hex>` and `recovery=<hex>`, the lines for `release-keys.txt`.
    pub card: String,
    /// The recovery key, in groups of eight, shown once and stored nowhere.
    pub recovery: String,
}

/// Make the release key into an open vault. Refuses a second one: every copy
/// handed out trusts the first, and a second would make them refuse your
/// updates. The caller saves the vault.
pub fn make_release_key(vault: &mut crate::vault::Vault, now: u64) -> Result<MadeKey, String> {
    if vault.list().iter().any(|(n, _)| *n == RELEASE_KEY_NAME) {
        return Err("There's already a release key in your vault. A second one would make every copy of \
                    Atlas you've handed out refuse your updates. If the key is lost or stolen, the \
                    recovery key is what replaces it."
            .into());
    }
    let seed = new_seed();
    let recovery = new_seed();
    vault.put(RELEASE_KEY_NAME, crate::vault::Kind::ApiKey, &seed_hex(&seed), now)?;
    let public = anchor_of(&signing_key_from_seed(&seed));
    let recovery_public = anchor_of(&signing_key_from_seed(&recovery));
    let hex = seed_hex(&recovery);
    let grouped: Vec<String> = hex.as_bytes().chunks(8).map(|c| String::from_utf8_lossy(c).to_string()).collect();
    Ok(MadeKey {
        card: format!("release={}\nrecovery={}", seed_hex(&public), seed_hex(&recovery_public)),
        recovery: grouped.join(" "),
    })
}

/// Where the last release number is kept (the releaser's store): signing takes the
/// next, so "newer" always means newer.
pub const LAST_SEQUENCE: &str = "release_last_sequence";
/// The fingerprint of the last program sent from the hub, so the same build
/// is never offered or sent twice.
pub const SENT_SHA: &str = "release_last_sent_sha256";

/// The all-zero placeholder, named so intent is unmistakable at the call sites
/// that check for it.
const PLACEHOLDER_ANCHOR: [u8; 32] = [0u8; 32];

/// Whether a real trust anchor has been set into this build (not the placeholder).
pub fn anchor_configured() -> bool {
    RELEASE_PUBLIC_KEY != PLACEHOLDER_ANCHOR
}

/// The result of checking a signature against a trust anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Signed by the release key. Safe to stage.
    Trusted,
    /// A signature was present but does not match the anchor — refuse it.
    Forged,
    /// No real trust anchor is configured (placeholder, or an unreadable
    /// anchor). Refuse everything, by design.
    NoAnchor,
}

impl Verdict {
    /// The one question callers actually ask: may this update be staged?
    /// True only for `Trusted`; both other verdicts are a refusal.
    pub fn is_trusted(self) -> bool {
        matches!(self, Verdict::Trusted)
    }
}

/// The core check, with the anchor passed in — so the shipped `verify` pins the
/// baked-in key while tests can exercise the real logic against a known key.
/// A placeholder or an anchor that is not a valid public key is `NoAnchor`
/// (refuse), never a silent pass.
pub fn verify_against(anchor: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> Verdict {
    if anchor == &PLACEHOLDER_ANCHOR {
        return Verdict::NoAnchor;
    }
    let Ok(key) = VerifyingKey::from_bytes(anchor) else {
        return Verdict::NoAnchor;
    };
    let sig = Signature::from_bytes(signature);
    match key.verify(message, &sig) {
        Ok(()) => Verdict::Trusted,
        Err(_) => Verdict::Forged,
    }
}

// --- Build side. None of the below ever runs on a device: the secret key is
// never shipped, and these are the calls the build machine's release command
// makes. Kept in the same module so the sign and verify halves are read and
// tested together, which is how a signature scheme avoids the classic mistake
// of the two halves drifting apart.

/// Load the build machine's signing key from its 32-byte secret seed.
pub fn signing_key_from_seed(seed: &[u8; 32]) -> SigningKey {
    SigningKey::from_bytes(seed)
}

/// The anchor bytes (public key) for a signing key — exactly what gets baked
/// into the app as `RELEASE_PUBLIC_KEY` at build time.
pub fn anchor_of(key: &SigningKey) -> [u8; 32] {
    key.verifying_key().to_bytes()
}

/// Sign `message` with the build machine's key, returning a detached 64-byte
/// signature. Deterministic (RFC 8032), so no randomness is needed here.
fn sign(key: &SigningKey, message: &[u8]) -> [u8; 64] {
    key.sign(message).to_bytes()
}

// =============================================================================
// The signed release manifest.
//
// One release is one small document: the release number, a display version,
// the oldest data format it can open, and one entry per platform naming the
// file, its exact size, and its SHA-256. Eric signs the document once. A device
// verifies the signature, then checks the file it received against its entry.
// So one signature binds every platform's build, and a device never has to
// trust bytes that were not fingerprinted by the signed document.
//
// The order of checks is the security design, not an implementation detail:
//   1. size cap      — refuse absurd input before doing any work on it;
//   2. signature     — verify BEFORE parsing, so untrusted bytes never reach
//                      the JSON parser (a parser bug can't be the way in);
//   3. format        — only formats this build understands;
//   4. well-formed   — no duplicate platforms, real hashes, safe file names;
//   5. release order — never go backwards unless explicitly rolling back;
//   6. data format   — never install a build that can't open this device's data;
//   7. platform      — find this device's entry.
// The file itself is checked separately (`check_artifact`) once downloaded.
// =============================================================================

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The manifest format this build understands. A manifest in any other format
/// is refused rather than half-read.
pub const MANIFEST_FORMAT: u32 = 1;

/// The largest signed manifest a device will look at. A real one is a few
/// hundred bytes per platform; anything past this is refused before the
/// signature is even checked, so a peer can't make a device chew on megabytes.
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;

/// Domain separation. The release key signs `DOMAIN || manifest`, never the bare
/// manifest. If the same key ever signs anything else (a key-rotation notice, a
/// plugin), that signature can never be replayed as a release, and vice versa.
const MANIFEST_DOMAIN: &[u8] = b"atlas-release-manifest-v1\n";

/// Every platform name a release may use, spelled exactly once, here. The
/// build side and the device side both take names from this list (and the
/// device's own from `this_platform`), so `win64` vs `windows-x86_64` can never
/// silently mean "no build for you". A manifest naming anything else is refused.
pub const KNOWN_PLATFORMS: &[&str] = &[
    "windows-x86_64",
    "windows-aarch64",
    "macos-x86_64",
    "macos-aarch64",
    "linux-x86_64",
    "linux-aarch64",
    "android-aarch64",
    "ios-aarch64",
];

/// This device's platform name, derived from what the binary was compiled
/// for — never typed, never guessed. Always one of `KNOWN_PLATFORMS` on a
/// supported target; `None` on a target Atlas does not ship for.
pub fn this_platform() -> Option<&'static str> {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return None;
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return None;
    };
    let name = format!("{os}-{arch}");
    KNOWN_PLATFORMS.iter().copied().find(|p| *p == name)
}

/// One platform's build inside a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    /// Which device this build is for, e.g. `windows-x86_64`, `macos-aarch64`,
    /// `linux-x86_64`, `android-aarch64`, `ios-aarch64`.
    pub platform: String,
    /// The file's bare name. Never a path — see `validate`.
    pub file: String,
    /// Exact size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
}

/// A release, as signed.
///
/// `deny_unknown_fields`: a field this build doesn't know is a refusal, never
/// silently ignored. That matters for one field in particular — nothing that
/// arrives over the mesh may carry "and roll back" (see `Direction`), and a
/// manifest smuggling such a field in is rejected outright.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Must equal `MANIFEST_FORMAT`.
    pub format: u32,
    /// The release number. Strictly increases with every release, and is the
    /// only thing ever compared to decide "newer" — the downgrade/replay defense.
    pub sequence: u64,
    /// Human label, e.g. "1.4.0". Shown to people; never compared.
    pub version: String,
    /// The oldest local data format this release can open or migrate from.
    /// Compared against the device's `upgrade::DATA_FORMAT`.
    pub min_data_format: u32,
    /// The data format this release writes. Higher than the device's means
    /// the update migrates stored data — the courier backs up first.
    pub data_format: u32,
    /// When it was released (unix seconds).
    pub released_at: u64,
    /// The latest date (unix seconds) by which Eric promises another signed
    /// notice — a new release, or the same builds re-announced. A device that
    /// passes this date without hearing anything newer says so (see
    /// `freshness`): being quietly held back is the one thing the release
    /// number alone cannot detect.
    pub next_word_by: u64,
    /// One entry per platform.
    pub artifacts: Vec<Artifact>,
}

/// A manifest plus its signature, as it travels over the mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedManifest {
    /// The exact JSON bytes that were signed, carried verbatim — so checking
    /// never depends on re-serializing the manifest identically.
    pub manifest: String,
    /// The 64-byte signature, lowercase hex.
    pub signature: String,
}

/// Why a release (or a downloaded file) was refused. Every path out of the
/// checks that is not "accepted" is one of these — nothing fails silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    TooLarge(usize),
    Unreadable(String),
    NotSigned(Verdict),
    UnsupportedFormat(u32),
    Malformed(String),
    AlreadyInstalled(u64),
    Downgrade { offered: u64, installed: u64 },
    DataTooOld { needs: u32, have: u32 },
    NotForThisPlatform(String),
    WrongSize { expected: u64, got: u64 },
    WrongHash,
}

impl Refusal {
    /// What to tell the person, in plain words.
    pub fn plain(&self) -> String {
        match self {
            Refusal::TooLarge(n) => format!("That update file is far too big to be a real release ({n} bytes), so I ignored it."),
            Refusal::Unreadable(why) => format!("That update couldn't be read ({why}), so I ignored it."),
            Refusal::NotSigned(Verdict::NoAnchor) => "I can't accept updates yet — this copy of Atlas has no release key set, so it trusts none.".into(),
            Refusal::NotSigned(_) => "That update is not signed by your release key, so I refused it. Nothing was installed.".into(),
            Refusal::UnsupportedFormat(f) => format!("That update is in a newer format ({f}) than this Atlas understands. Install a newer build first."),
            Refusal::Malformed(why) => format!("That update is signed but not well-formed ({why}), so I refused it."),
            Refusal::AlreadyInstalled(s) => format!("You already have release {s}."),
            Refusal::Downgrade { offered, installed } => format!("That's an older release ({offered}) than the one you have ({installed}), so I didn't go backwards."),
            Refusal::DataTooOld { needs, have } => format!("That release can't open your data yet (it needs format {needs}, you're on {have}). Take the release in between first."),
            Refusal::NotForThisPlatform(p) => format!("That release has no build for this device ({p})."),
            Refusal::WrongSize { expected, got } => format!("The downloaded file is the wrong size ({got} bytes, expected {expected}) — it may be incomplete. Not installed."),
            Refusal::WrongHash => "The downloaded file doesn't match the signed release — it was altered or corrupted. Not installed.".into(),
        }
    }
}

/// A release that passed every check, and this device's entry in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub manifest: Manifest,
    pub artifact: Artifact,
}

impl Manifest {
    /// This device's entry, if the release has one.
    fn artifact_for(&self, platform: &str) -> Option<&Artifact> {
        self.artifacts.iter().find(|a| a.platform == platform)
    }

    /// Structural checks a signed manifest must still pass. Signing proves who
    /// wrote it; this catches a well-signed mistake before it reaches anyone.
    fn validate(&self) -> Result<(), Refusal> {
        if self.format != MANIFEST_FORMAT {
            return Err(Refusal::UnsupportedFormat(self.format));
        }
        if self.sequence == 0 {
            return Err(Refusal::Malformed("release number must start at 1".into()));
        }
        if self.artifacts.is_empty() {
            return Err(Refusal::Malformed("no builds listed".into()));
        }
        if self.data_format < self.min_data_format {
            return Err(Refusal::Malformed("it writes an older data format than it requires".into()));
        }
        if self.next_word_by <= self.released_at {
            return Err(Refusal::Malformed("its next-notice date is not after its release date".into()));
        }
        let mut seen: Vec<&str> = Vec::new();
        for a in &self.artifacts {
            if !KNOWN_PLATFORMS.contains(&a.platform.as_str()) {
                return Err(Refusal::Malformed(format!("unknown platform {:?}", a.platform)));
            }
            if seen.contains(&a.platform.as_str()) {
                return Err(Refusal::Malformed(format!("two builds for {}", a.platform)));
            }
            seen.push(&a.platform);
            if !safe_file_name(&a.file) {
                return Err(Refusal::Malformed(format!("unsafe file name {:?}", a.file)));
            }
            if a.size == 0 {
                return Err(Refusal::Malformed(format!("{} is empty", a.file)));
            }
            if a.sha256.len() != 64 || !a.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return Err(Refusal::Malformed(format!("{} has no valid fingerprint", a.file)));
            }
        }
        Ok(())
    }
}

/// A file name that can only ever mean "a file in the folder it's put in":
/// no path separators, no `..`, no drive letters, no leading dot, no control
/// characters. The installer writes this name to disk, so a name like
/// `../../startup/evil.exe` must never survive to that point.
fn safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && !name.contains("..")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Build side: sign a manifest into the form that travels over the mesh.
pub fn seal_manifest(key: &SigningKey, manifest: &Manifest) -> SignedManifest {
    #[allow(clippy::expect_used, reason = "a plain struct of strings and numbers always serializes to JSON")]
    let text = serde_json::to_string(manifest).expect("a manifest always serializes");
    let signature = sign(key, &domain_bytes(text.as_bytes()));
    SignedManifest { manifest: text, signature: to_hex(&signature) }
}

/// Proof that a person, at this device, asked to go back to an older release.
///
/// The downgrade defense must not be switchable from outside. So rolling back
/// is not a `bool` any code path can pass along — it is this type, whose only
/// constructor is named for what it means, and which is never serialized:
/// nothing arriving over the mesh can contain one (and `Manifest` rejects
/// unknown fields, so no message can even try). Only the local command or the
/// hub's own confirmation may construct it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalApproval(());

impl LocalApproval {
    /// Call only from a local, deliberate action by the person — the rollback
    /// command, or the hub's confirm button. Never from message handling.
    pub fn given_by_the_person_at_this_device() -> LocalApproval {
        LocalApproval(())
    }
}

/// Which way an update may move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The normal case: only a newer release is accepted.
    Forward,
    /// Going back to an older signed release, because the person asked here.
    Rollback(LocalApproval),
}

/// Device side, against the anchor this device currently trusts (the baked-in
/// key, or the one a signed rotation replaced it with — see `TrustState`).
/// This is the call the update courier makes on every release it receives.
pub fn accept(
    installed: &Installed,
    signed: &SignedManifest,
    local_data_format: u32,
    platform: &str,
    direction: Direction,
) -> Result<Accepted, Refusal> {
    accept_against(
        &installed.trust.current,
        signed,
        installed.sequence_in_this_era(),
        local_data_format,
        platform,
        direction,
    )
}

/// The full check, with the anchor passed in (so tests can use a known key).
/// See the numbered order at the top of this section.
pub fn accept_against(
    anchor: &[u8; 32],
    signed: &SignedManifest,
    installed_sequence: u64,
    local_data_format: u32,
    platform: &str,
    direction: Direction,
) -> Result<Accepted, Refusal> {
    let allow_older = matches!(direction, Direction::Rollback(_));
    // 1. Size cap, before any work.
    let total = signed.manifest.len() + signed.signature.len();
    if total > MAX_MANIFEST_BYTES {
        return Err(Refusal::TooLarge(total));
    }
    // 2. Signature, before parsing.
    let sig = from_hex_64(&signed.signature)
        .ok_or_else(|| Refusal::Unreadable("the signature isn't valid hex".into()))?;
    let verdict = verify_against(anchor, &domain_bytes(signed.manifest.as_bytes()), &sig);
    if !verdict.is_trusted() {
        return Err(Refusal::NotSigned(verdict));
    }
    // Only now is it safe to parse.
    let manifest: Manifest = serde_json::from_str(&signed.manifest)
        .map_err(|e| Refusal::Unreadable(format!("signed but unparseable: {e}")))?;
    // 3–4. Format and structure.
    manifest.validate()?;
    // 5. Release order.
    if manifest.sequence == installed_sequence {
        return Err(Refusal::AlreadyInstalled(installed_sequence));
    }
    if manifest.sequence < installed_sequence && !allow_older {
        return Err(Refusal::Downgrade { offered: manifest.sequence, installed: installed_sequence });
    }
    // 6. Data format.
    if local_data_format < manifest.min_data_format {
        return Err(Refusal::DataTooOld { needs: manifest.min_data_format, have: local_data_format });
    }
    // 7. This device's build.
    let artifact = manifest
        .artifact_for(platform)
        .cloned()
        .ok_or_else(|| Refusal::NotForThisPlatform(platform.to_string()))?;
    Ok(Accepted { manifest, artifact })
}

// =============================================================================
// The build side: what `atlas release keygen | sign` use.
// =============================================================================

/// A fresh 32-byte seed from the operating system's random source.
pub fn new_seed() -> [u8; 32] {
    use chacha20poly1305::aead::rand_core::RngCore;
    let mut s = [0u8; 32];
    chacha20poly1305::aead::OsRng.fill_bytes(&mut s);
    s
}

/// A seed as hex, for keeping in the vault or writing on a card.
pub fn seed_hex(seed: &[u8; 32]) -> String {
    to_hex(seed)
}

/// Back from hex.
pub fn seed_from_hex(s: &str) -> Option<[u8; 32]> {
    from_hex_32(&s.chars().filter(|c| !c.is_whitespace()).collect::<String>())
}

/// A public key written as the Rust array that is baked into the build.
pub fn as_rust_array(key: &[u8; 32]) -> String {
    let bytes: Vec<String> = key.iter().map(|b| format!("0x{b:02x}")).collect();
    format!("[{}]", bytes.join(", "))
}

/// The manifest for a new release, from the files for each platform:
/// `(platform, file name, bytes)`. Sizes and fingerprints are measured here,
/// never typed.
pub fn manifest_for(
    sequence: u64,
    version: &str,
    min_data_format: u32,
    released_at: u64,
    next_word_by: u64,
    files: &[(String, String, Vec<u8>)],
) -> Result<Manifest, Refusal> {
    let m = Manifest {
        format: MANIFEST_FORMAT,
        sequence,
        version: version.to_string(),
        min_data_format,
        data_format: crate::upgrade::DATA_FORMAT,
        released_at,
        next_word_by,
        artifacts: files
            .iter()
            .map(|(platform, file, bytes)| Artifact {
                platform: platform.clone(),
                file: file.clone(),
                size: bytes.len() as u64,
                sha256: crate::digest::sha256_hex(bytes),
            })
            .collect(),
    };
    m.validate()?;
    Ok(m)
}

/// A release notice that verifies against this device's trusted key and is
/// well-formed, whatever its number -- for freshness, where an announcement of
/// the release you already have still counts as having heard from Eric.
/// Checked in the same order as `accept_against` (size, signature, parse,
/// structure), and nothing about installing follows from it.
pub fn verified_notice(installed: &Installed, signed: &SignedManifest) -> Option<Manifest> {
    if signed.manifest.len() + signed.signature.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    let sig = from_hex_64(&signed.signature)?;
    if !verify_against(&installed.trust.current, &domain_bytes(signed.manifest.as_bytes()), &sig).is_trusted() {
        return None;
    }
    let m: Manifest = serde_json::from_str(&signed.manifest).ok()?;
    m.validate().ok()?;
    Some(m)
}

/// Check a downloaded file against its signed entry: exact size, then exact
/// fingerprint. Only a file that passes this may be handed to the installer.
/// For a file already in memory; a download uses `Fingerprint` directly.
pub fn check_artifact(artifact: &Artifact, bytes: &[u8]) -> Result<(), Refusal> {
    let mut f = Fingerprint::for_artifact(artifact);
    f.update(bytes)?;
    f.finish()
}

/// Checking a file while it downloads, a piece at a time.
///
/// A phone build is tens to hundreds of megabytes; holding the whole file to
/// hash it afterwards is wasteful on a phone and pointless when SHA-256 can be
/// fed incrementally. This also stops a download the moment it runs past the
/// signed size, so a peer can't stream endless bytes into a device.
pub struct Fingerprint {
    expected_size: u64,
    expected_hash: String,
    seen: u64,
    hasher: Sha256,
}

impl Fingerprint {
    pub fn for_artifact(artifact: &Artifact) -> Fingerprint {
        Fingerprint {
            expected_size: artifact.size,
            expected_hash: artifact.sha256.clone(),
            seen: 0,
            hasher: Sha256::new(),
        }
    }

    /// Feed the next piece. Refuses as soon as the total passes the signed size.
    pub fn update(&mut self, chunk: &[u8]) -> Result<(), Refusal> {
        self.seen += chunk.len() as u64;
        if self.seen > self.expected_size {
            return Err(Refusal::WrongSize { expected: self.expected_size, got: self.seen });
        }
        self.hasher.update(chunk);
        Ok(())
    }

    /// The whole file has arrived: exact size, then exact fingerprint.
    pub fn finish(self) -> Result<(), Refusal> {
        if self.seen != self.expected_size {
            return Err(Refusal::WrongSize { expected: self.expected_size, got: self.seen });
        }
        if to_hex(&self.hasher.finalize()) != self.expected_hash {
            return Err(Refusal::WrongHash);
        }
        Ok(())
    }
}

// =============================================================================
// What this device remembers about releases — kept in data/state, which is in
// `upgrade::YOURS`, so an update can never reset it. Without this the release
// number that powers downgrade protection would start from zero after every
// install, and "never go backwards" would mean nothing.
// =============================================================================

/// The store name for `Installed`.
pub const INSTALLED_FILE: &str = "release_installed";

/// The release this device is running, and the trust it has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    /// 0 before any courier-installed release (a hand-installed first copy).
    pub sequence: u64,
    /// Which key era `sequence` was recorded in (`trust.rotations` at the time).
    /// Release numbers are compared only within one era. Why: if the release
    /// key is ever stolen, the thief can sign a release numbered near the
    /// maximum; after the recovery key rotates the thief out, every genuine
    /// release would otherwise be refused as "older" forever. A rotation starts
    /// a new era, so the count starts fresh under the new key.
    pub sequence_era: u32,
    pub version: String,
    /// The fingerprint of the build installed, so a re-announcement of the same
    /// builds (see `next_word_by`) is recognised and not reinstalled.
    pub sha256: String,
    /// The newest release notice seen from Eric, installed or not — what the
    /// freshness check measures from.
    pub newest_seen_released_at: u64,
    pub newest_seen_next_word_by: u64,
    pub trust: TrustState,
}

impl Default for Installed {
    fn default() -> Self {
        Installed {
            sequence: 0,
            sequence_era: 0,
            version: String::new(),
            sha256: String::new(),
            newest_seen_released_at: 0,
            newest_seen_next_word_by: 0,
            trust: TrustState::from_build(),
        }
    }
}

impl Installed {
    /// Loaded from the store; a missing file is a fresh device on the build's key.
    pub fn load(store: &crate::store::Store) -> Installed {
        store.load(INSTALLED_FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(INSTALLED_FILE, self)
    }

    /// Note a verified release notice, installed or not, for freshness.
    pub fn saw(&mut self, manifest: &Manifest) {
        if manifest.released_at > self.newest_seen_released_at {
            self.newest_seen_released_at = manifest.released_at;
            self.newest_seen_next_word_by = manifest.next_word_by;
        }
    }

    /// The release number to compare against — the installed one if it was
    /// recorded in the current key era, otherwise nothing (a new era starts
    /// its count fresh; see `sequence_era`).
    pub fn sequence_in_this_era(&self) -> u64 {
        if self.sequence_era == self.trust.rotations {
            self.sequence
        } else {
            0
        }
    }

    /// Record a completed install.
    pub fn installed(&mut self, accepted: &Accepted) {
        self.sequence = accepted.manifest.sequence;
        self.sequence_era = self.trust.rotations;
        self.version = accepted.manifest.version.clone();
        self.sha256 = accepted.artifact.sha256.clone();
        self.saw(&accepted.manifest);
    }

    /// True when an accepted release carries a different build for this
    /// device. A re-announcement of the same bytes is noted, not reinstalled.
    pub fn needs_install(&self, accepted: &Accepted) -> bool {
        accepted.artifact.sha256 != self.sha256
    }
}

// =============================================================================
// Freshness — the freeze defense.
//
// The release number stops a device being rolled *back*. It can't tell a
// device it is being held *back*: a peer that simply never passes on new
// releases leaves a device on an old one, silently. Every release carries
// `next_word_by`, Eric's signed promise of the latest date he'll send another
// notice; past that date with nothing newer, the device says so. No key is
// ever held unlocked for this — Eric just releases or re-announces in time.
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Nothing heard yet (a first copy) — nothing to measure.
    NoNoticeYet,
    Current,
    Overdue { days: u64 },
}

impl Freshness {
    pub fn plain(&self) -> Option<String> {
        match self {
            Freshness::Overdue { days } => Some(format!(
                "I haven't heard a release notice in {days} day{} past when one was due. \
                 I may be cut off from updates — worth checking with whoever gave you Atlas.",
                if *days == 1 { "" } else { "s" }
            )),
            _ => None,
        }
    }
}

pub fn freshness(installed: &Installed, now: u64) -> Freshness {
    if installed.newest_seen_next_word_by == 0 {
        return Freshness::NoNoticeYet;
    }
    if now <= installed.newest_seen_next_word_by {
        return Freshness::Current;
    }
    Freshness::Overdue { days: (now - installed.newest_seen_next_word_by) / 86_400 }
}

// =============================================================================
// Key rotation and the recovery key.
//
// The trusted anchor is not frozen at build time forever. It starts as the
// baked-in `RELEASE_PUBLIC_KEY` and can be replaced by a signed rotation
// notice. Two keys can sign one:
//   * the current release key — for planned rotation;
//   * the RECOVERY key — kept OFFLINE (paper/USB, never on a connected
//     machine, never in the vault), baked in beside the release key, and used
//     only if the release key is lost or stolen. A thief holding the release
//     key could rotate to their own key; the recovery key rotates it back, and
//     a recovery-signed rotation always wins.
// Rotations are numbered and never replayable; a rotation to the placeholder
// or to a non-key is refused.
// =============================================================================

/// The offline recovery key's public half. Placeholder until generated at
/// setup, alongside the release key. A placeholder recovery key signs nothing.
/// From `release-keys.txt`, like the release key.
pub const RECOVERY_PUBLIC_KEY: [u8; 32] = key_in(KEYS_FILE, b"recovery");

const ROTATION_DOMAIN: &[u8] = b"atlas-key-rotation-v1\n";

/// The anchor this device currently trusts, and how many rotations it has taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustState {
    pub current: [u8; 32],
    pub rotations: u32,
}

impl TrustState {
    /// Where every device starts: the key baked into its build.
    fn from_build() -> TrustState {
        TrustState { current: RELEASE_PUBLIC_KEY, rotations: 0 }
    }
}

/// "From now on, trust this key instead."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rotation {
    /// Must be exactly one more than the device's current count — no skipping,
    /// no replaying.
    pub number: u32,
    /// The new anchor, lowercase hex.
    pub new_anchor: String,
    /// Said to the person when it applies.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedRotation {
    pub rotation: String,
    pub signature: String,
}

/// Build side: sign a rotation with the current release key or the recovery key.
pub fn seal_rotation(key: &SigningKey, rotation: &Rotation) -> SignedRotation {
    #[allow(clippy::expect_used, reason = "a plain struct of strings and numbers always serializes to JSON")]
    let text = serde_json::to_string(rotation).expect("a rotation always serializes");
    let mut msg = ROTATION_DOMAIN.to_vec();
    msg.extend_from_slice(text.as_bytes());
    SignedRotation { rotation: text, signature: to_hex(&sign(key, &msg)) }
}

/// Device side: apply a rotation against the device's trust and the baked-in
/// recovery key.
pub fn apply_rotation(trust: &TrustState, signed: &SignedRotation) -> Result<TrustState, Refusal> {
    apply_rotation_with(trust, &RECOVERY_PUBLIC_KEY, signed)
}

pub fn apply_rotation_with(
    trust: &TrustState,
    recovery: &[u8; 32],
    signed: &SignedRotation,
) -> Result<TrustState, Refusal> {
    if signed.rotation.len() + signed.signature.len() > MAX_MANIFEST_BYTES {
        return Err(Refusal::TooLarge(signed.rotation.len() + signed.signature.len()));
    }
    let sig = from_hex_64(&signed.signature)
        .ok_or_else(|| Refusal::Unreadable("the rotation signature isn't valid hex".into()))?;
    let mut msg = ROTATION_DOMAIN.to_vec();
    msg.extend_from_slice(signed.rotation.as_bytes());
    // Signed by the key trusted now, or by the offline recovery key.
    let by_current = verify_against(&trust.current, &msg, &sig).is_trusted();
    let by_recovery = verify_against(recovery, &msg, &sig).is_trusted();
    if !by_current && !by_recovery {
        return Err(Refusal::NotSigned(Verdict::Forged));
    }
    let rotation: Rotation = serde_json::from_str(&signed.rotation)
        .map_err(|e| Refusal::Unreadable(format!("signed but unparseable rotation: {e}")))?;
    // A planned rotation (current key) must be exactly the next one. A
    // recovery rotation only has to be *later* than anything this device has
    // taken: after a theft, devices may have seen a different number of the
    // thief's rotations, and the recovery must land on all of them anyway.
    let in_order = if by_current && !by_recovery {
        rotation.number == trust.rotations + 1
    } else {
        rotation.number > trust.rotations
    };
    if !in_order {
        return Err(Refusal::Malformed(format!(
            "rotation {} out of order (this device has taken {})",
            rotation.number, trust.rotations
        )));
    }
    let new = from_hex_32(&rotation.new_anchor)
        .ok_or_else(|| Refusal::Malformed("the new key isn't valid hex".into()))?;
    if new == PLACEHOLDER_ANCHOR || VerifyingKey::from_bytes(&new).is_err() || new == *recovery {
        return Err(Refusal::Malformed("the new key isn't a usable release key".into()));
    }
    Ok(TrustState { current: new, rotations: rotation.number })
}

fn from_hex_32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Some(out)
}

fn domain_bytes(manifest: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(MANIFEST_DOMAIN.len() + manifest.len());
    v.extend_from_slice(MANIFEST_DOMAIN);
    v.extend_from_slice(manifest);
    v
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

fn from_hex_64(s: &str) -> Option<[u8; 64]> {
    if s.len() != 128 {
        return None;
    }
    let mut out = [0u8; 64];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Some(out)
}

// ---------------------------------------------------------------- sending an update from the hub
//
// Eric (27 Sep 2026): no command prompt, anywhere. `atlas release sign` wanted
// file paths typed into a terminal; the hub's Updates page finds the build
// itself instead: the newest Atlas program (or the zip GitHub hands out) in
// the folders downloads land in. The file is named by what's inside it -- the
// program's own machine type and the version written into it -- never by what
// the download happened to be called.

/// A build of Atlas found on this computer, ready to sign.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundBuild {
    /// The file found: the program itself, or the zip it came in.
    pub path: std::path::PathBuf,
    /// One of `KNOWN_PLATFORMS`, from the program's own header.
    pub platform: &'static str,
    /// From the version written into the program (its VERSIONINFO).
    pub version: String,
    /// The program itself, taken out of the zip if it came in one.
    pub program: Vec<u8>,
    pub sha256: String,
    /// When it landed (seconds since 1970).
    pub modified: u64,
}

/// Downloads a person might have made of Atlas: the program, `Atlas Setup.exe`,
/// or the `Atlas-Windows-…` zip from GitHub. Not any `.exe`.
fn could_be_a_build(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("atlas") && (n.ends_with(".exe") || n.ends_with(".zip"))
}

/// The newest usable build in `dirs`: `None` when there's nothing that could
/// be one; `Some(Err((file name, why)))` when nothing that could be one is a
/// usable Atlas program, naming the newest and why, so the page can say which
/// file instead of going quiet.
///
/// Newest first, and a file that isn't a usable build is passed over for the
/// next (28 Sep 2026): until then only the newest `atlas*.zip|exe` was looked
/// at, so an unrelated `atlas-notes.zip` downloaded after the real build hid
/// it, and the page said there was nothing to send.
pub fn find_build(dirs: &[std::path::PathBuf]) -> Option<Result<FoundBuild, (String, String)>> {
    let mut found: Vec<(u64, std::path::PathBuf)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() || !could_be_a_build(&e.file_name().to_string_lossy()) {
                continue;
            }
            let at = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            found.push((at, e.path()));
        }
    }
    // Newest first; the same time, by name, so the order is always the same.
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut first_problem: Option<(String, String)> = None;
    for (modified, path) in found {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        match look_at_build(&path, modified) {
            Ok(b) => return Some(Ok(b)),
            Err(why) => {
                first_problem.get_or_insert((name, why));
            }
        }
    }
    first_problem.map(Err)
}

/// Read one download and say what's in it, or why it isn't a build.
///
/// Remembered by the file's size and time: the Updates page asks on every
/// render, and reading, unzipping and hashing a build each time held the hub
/// for a second or more (27 Sep 2026). A file that changes is read again.
/// Several are remembered, since the page now looks past files that aren't
/// builds to the one that is.
fn look_at_build(path: &std::path::Path, modified: u64) -> Result<FoundBuild, String> {
    type Seen = Vec<(std::path::PathBuf, u64, Option<std::time::SystemTime>, Result<FoundBuild, String>)>;
    static SEEN: std::sync::Mutex<Seen> = std::sync::Mutex::new(Vec::new());
    let meta = std::fs::metadata(path).ok();
    let (len, at) = (meta.as_ref().map(|m| m.len()).unwrap_or(0), meta.and_then(|m| m.modified().ok()));
    {
        let seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, _, _, found)) = seen.iter().find(|(p, l, t, _)| p == path && *l == len && *t == at && at.is_some()) {
            return found.clone();
        }
    }
    let found = read_build(path, modified);
    let mut seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    seen.retain(|(p, ..)| p != path);
    // Only a few: a usable build holds the whole program in memory.
    if seen.len() >= 4 {
        // unheard-ok: returns `(PathBuf, u64, Option<SystemTime>, ...)`, not a Result
        let _ = seen.remove(0);
    }
    seen.push((path.to_path_buf(), len, at, found.clone()));
    found
}

fn read_build(path: &std::path::Path, modified: u64) -> Result<FoundBuild, String> {
    const MAX: u64 = 400_000_000;
    let zip = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("zip"));
    let bytes = std::fs::read(path).map_err(|e| format!("it couldn't be read ({e})"))?;
    let program = if zip {
        let wanted = |n: &str| {
            let base = n.rsplit('/').next().unwrap_or(n).to_ascii_lowercase();
            !n.contains("tor/") && base.starts_with("atlas") && base.ends_with(".exe")
        };
        match crate::zipread::file_inside(&bytes, wanted, MAX).map_err(|e| format!("it isn't a zip I can open ({e})"))? {
            Some((_, program)) => program,
            None => return Err("there's no Atlas program inside it".into()),
        }
    } else {
        bytes
    };
    let platform = pe_platform(&program).ok_or("it isn't a Windows program")?;
    let version = version_in_exe(&program).ok_or("it doesn't say it's Atlas, or which version it is")?;
    Ok(FoundBuild { path: path.to_path_buf(), platform, version, sha256: crate::digest::sha256_hex(&program), program, modified })
}

/// A Windows program's machine type, from its PE header.
fn pe_platform(b: &[u8]) -> Option<&'static str> {
    if b.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u32::from_le_bytes(b.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if b.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    match u16::from_le_bytes(b.get(pe + 4..pe + 6)?.try_into().ok()?) {
        0x8664 => Some("windows-x86_64"),
        0xAA64 => Some("windows-aarch64"),
        _ => None,
    }
}

/// The version written into an Atlas program's VERSIONINFO (`windows/atlas.rc`,
/// generated from Cargo.toml). A program carries other programs' version
/// blocks too (the WebView2 part says it's Microsoft Edge), so only a block
/// whose ProductName is "Atlas" counts.
fn version_in_exe(b: &[u8]) -> Option<String> {
    let names = utf16_values(b, "ProductName");
    let versions = utf16_values(b, "ProductVersion");
    names.iter().filter(|(_, v)| v == "Atlas").find_map(|(at, _)| {
        versions.iter().filter(|(p, _)| p.abs_diff(*at) < 512).map(|(_, v)| v.clone()).next()
    })
}

/// Every `key` in a version block, with where it sits and its value.
fn utf16_values(b: &[u8], key: &str) -> Vec<(usize, String)> {
    let mut needle: Vec<u8> = key.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    needle.extend([0, 0]);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = b.get(from..).and_then(|rest| rest.windows(needle.len()).position(|w| w == needle.as_slice())) {
        let at = from + i;
        // Then up to three zero units of padding (the value starts on a
        // 4-byte boundary), then the value, which never starts with a zero.
        let mut p = at + needle.len();
        let mut skipped = 0;
        while skipped < 3 && b.get(p..p + 2) == Some(&[0u8, 0][..]) {
            p += 2;
            skipped += 1;
        }
        let mut units = Vec::new();
        while let Some(u) = b.get(p..p + 2).map(|x| u16::from_le_bytes([x[0], x[1]])) {
            if u == 0 || units.len() > 64 {
                break;
            }
            units.push(u);
            p += 2;
        }
        if let Ok(v) = String::from_utf16(&units) {
            if !v.is_empty() {
                out.push((at, v));
            }
        }
        from = at + 2;
    }
    out
}

/// Signing a found build: the notice friends' Atlases check, and its number.
#[derive(Debug, Clone)]
pub struct SignedBuild {
    pub sequence: u64,
    pub signed: SignedManifest,
}

/// Sign `found` as release `last_sequence + 1`, with the key in the (open)
/// vault. Refused when that key isn't `trusted`, the one Atlas is built to
/// trust (`RELEASE_PUBLIC_KEY`): friends' copies would refuse the release, so
/// it would go out and do nothing.
pub fn sign_build(
    vault: &mut crate::vault::Vault,
    found: &FoundBuild,
    last_sequence: u64,
    now: u64,
    trusted: &[u8; 32],
) -> Result<SignedBuild, String> {
    let seed = vault.get(RELEASE_KEY_NAME, now).map_err(|_| "There's no release key in your vault.".to_string())?;
    let seed = seed_from_hex(&seed).ok_or("The release key in your vault is damaged.")?;
    let key = signing_key_from_seed(&seed);
    if anchor_of(&key) != *trusted {
        return Err("The key in your vault isn't the one this Atlas was built with, so friends' copies would refuse \
                    this update. Nothing was signed."
            .into());
    }
    let sequence = last_sequence + 1;
    let files = [(found.platform.to_string(), "atlas.exe".to_string(), found.program.clone())];
    let manifest = manifest_for(sequence, &found.version, crate::upgrade::DATA_FORMAT, now, now + 30 * 86_400, &files)
        .map_err(|r| r.plain())?;
    Ok(SignedBuild { sequence, signed: seal_manifest(&key, &manifest) })
}

#[cfg(test)]
mod tests {
    use super::*;

    // A fixed seed so the whole suite is deterministic — a real build key comes
    // from OS entropy at setup time, but a test wants the same key every run.
    fn test_key() -> SigningKey {
        signing_key_from_seed(&[7u8; 32])
    }

    #[test]
    fn the_key_card_is_read_at_compile_time_and_none_means_no_key() {
        const CARD: &str = "# comment\nrelease=0101010101010101010101010101010101010101010101010101010101010101\nrecovery=none\n";
        assert_eq!(key_in(CARD, b"release"), [1u8; 32]);
        assert_eq!(key_in(CARD, b"recovery"), [0u8; 32]);
        assert_eq!(key_in("", b"release"), PLACEHOLDER_ANCHOR);
        // The tree's own file says none until Eric makes the key.
        let shipped = KEYS_FILE.lines().find(|l| l.starts_with("release=")).unwrap();
        assert_eq!(anchor_configured(), !shipped.ends_with("none"));
    }

    #[test]
    fn making_the_key_puts_it_in_the_vault_and_hands_back_a_card_that_matches() {
        let dir = std::env::temp_dir().join(format!("atlas-relkey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = crate::store::Store::new(&dir);
        let mut vault = crate::vault::Vault::load(&store);
        vault.open("a long passphrase", 1_000, &crate::vault::VaultConfig::default()).unwrap();
        let made = make_release_key(&mut vault, 1_000).unwrap();
        let seed = seed_from_hex(&vault.get(RELEASE_KEY_NAME, 1_000).unwrap()).unwrap();
        let public = seed_hex(&anchor_of(&signing_key_from_seed(&seed)));
        let lines: Vec<&str> = made.card.lines().collect();
        assert_eq!(lines[0], format!("release={public}"));
        assert!(lines[1].starts_with("recovery=") && lines[1].len() == 9 + 64);
        // The card parses the way the build will read it.
        let card: &'static str = Box::leak(made.card.clone().into_boxed_str());
        assert_eq!(seed_hex(&key_in(card, b"release")), public);
        assert_eq!(made.recovery.split(' ').count(), 8, "{}", made.recovery);
        assert_eq!(vault.list().len(), 1, "only the release key is kept; the recovery key never is");
        assert!(make_release_key(&mut vault, 1_000).is_err(), "a second key would strand every copy");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_real_signature_verifies_against_its_own_anchor() {
        let key = test_key();
        let anchor = anchor_of(&key);
        let msg = b"atlas release 1.4.0 manifest bytes";
        let sig = sign(&key, msg);
        assert_eq!(verify_against(&anchor, msg, &sig), Verdict::Trusted);
        assert!(verify_against(&anchor, msg, &sig).is_trusted());
    }

    #[test]
    fn a_tampered_message_is_forged_not_trusted() {
        // The whole point: change one byte of what was signed and the signature
        // no longer holds. This is what stops a mesh peer altering an update.
        let key = test_key();
        let anchor = anchor_of(&key);
        let sig = sign(&key, b"install version 1.4.0");
        assert_eq!(verify_against(&anchor, b"install version 9.9.9", &sig), Verdict::Forged);
    }

    #[test]
    fn a_tampered_signature_is_forged() {
        let key = test_key();
        let anchor = anchor_of(&key);
        let msg = b"a manifest";
        let mut sig = sign(&key, msg);
        sig[0] ^= 0x01; // flip a bit in the signature
        assert_eq!(verify_against(&anchor, msg, &sig), Verdict::Forged);
    }

    #[test]
    fn a_signature_from_a_different_key_does_not_verify() {
        // Someone else signs their own build with their own key; it must not
        // pass against your anchor. This is authorship, not just integrity.
        let mine = test_key();
        let theirs = signing_key_from_seed(&[9u8; 32]);
        let msg = b"a manifest";
        let their_sig = sign(&theirs, msg);
        assert_eq!(verify_against(&anchor_of(&mine), msg, &their_sig), Verdict::Forged);
    }

    #[test]
    fn the_placeholder_anchor_trusts_nothing() {
        // The safe default. Even a syntactically valid signature is refused
        // when no real anchor is set — refuse-everything, never accept-unsigned.
        let key = test_key();
        let msg = b"a manifest";
        let sig = sign(&key, msg);
        assert_eq!(verify_against(&PLACEHOLDER_ANCHOR, msg, &sig), Verdict::NoAnchor);
        assert!(!verify_against(&PLACEHOLDER_ANCHOR, msg, &sig).is_trusted());
    }

    #[test]
    fn the_shipped_build_carries_erics_release_key() {
        // Eric made his key on 27 Sep 2026 and sent the card; every build from
        // here trusts it. Changing this line is changing who Atlas trusts.
        assert!(anchor_configured());
        assert_eq!(seed_hex(&RELEASE_PUBLIC_KEY), "107ad00abcc9a466ddacb816ca55ccc31134ffa38052149a17069b0f86d2536c");
        assert!(VerifyingKey::from_bytes(&RELEASE_PUBLIC_KEY).is_ok(), "the card's key isn't a real public key");
        assert_eq!(verify_against(&RELEASE_PUBLIC_KEY, b"anything", &[0u8; 64]), Verdict::Forged);
    }

    #[test]
    fn a_garbage_anchor_is_never_trusted_and_never_crashes() {
        // The trust root cannot be a place the program can be made to fall over,
        // and no non-key bytes may ever yield Trusted. Some 32-byte patterns
        // decompress to a valid point (→ Forged against a real signature) and
        // some do not (→ NoAnchor); the security property is the same either
        // way — never Trusted — so that is what we pin, across a range of junk.
        let sig = sign(&test_key(), b"m");
        for pattern in [[0xFFu8; 32], [0x01u8; 32], [0xABu8; 32], [0u8; 32]] {
            let v = verify_against(&pattern, b"m", &sig);
            assert!(!v.is_trusted(), "junk anchor {pattern:?} was trusted");
        }
    }

    // ---------------- the signed manifest ----------------

    const WIN_FILE: &[u8] = b"pretend this is AtlasSetup.exe";
    const IOS_FILE: &[u8] = b"pretend this is Atlas.ipa";

    fn release(sequence: u64) -> Manifest {
        Manifest {
            format: MANIFEST_FORMAT,
            sequence,
            version: format!("1.{sequence}.0"),
            min_data_format: 3,
            data_format: 3,
            released_at: 1_790_000_000,
            next_word_by: 1_790_000_000 + 90 * 86_400,
            artifacts: vec![
                Artifact {
                    platform: "windows-x86_64".into(),
                    file: "AtlasSetup-1.4.0.exe".into(),
                    size: WIN_FILE.len() as u64,
                    sha256: crate::digest::sha256_hex(WIN_FILE),
                },
                Artifact {
                    platform: "ios-aarch64".into(),
                    file: "Atlas-1.4.0.ipa".into(),
                    size: IOS_FILE.len() as u64,
                    sha256: crate::digest::sha256_hex(IOS_FILE),
                },
            ],
        }
    }

    fn accept_on_windows(signed: &SignedManifest, installed: u64) -> Result<Accepted, Refusal> {
        accept_against(&anchor_of(&test_key()), signed, installed, 3, "windows-x86_64", Direction::Forward)
    }

    #[test]
    fn a_signed_release_is_accepted_and_hands_back_this_devices_build() {
        let signed = seal_manifest(&test_key(), &release(4));
        let ok = accept_on_windows(&signed, 3).expect("a good release is accepted");
        assert_eq!(ok.manifest.sequence, 4);
        assert_eq!(ok.artifact.platform, "windows-x86_64");
        // And the same release hands an iPhone its own build.
        let ios = accept_against(&anchor_of(&test_key()), &signed, 3, 3, "ios-aarch64", Direction::Forward).unwrap();
        assert_eq!(ios.artifact.file, "Atlas-1.4.0.ipa");
    }

    #[test]
    fn editing_the_manifest_after_signing_breaks_it() {
        // A mesh peer bumps the release number, or swaps a fingerprint for
        // their own file's. Either way the signature no longer holds.
        let mut signed = seal_manifest(&test_key(), &release(4));
        signed.manifest = signed.manifest.replace("\"sequence\":4", "\"sequence\":99");
        assert_eq!(accept_on_windows(&signed, 3), Err(Refusal::NotSigned(Verdict::Forged)));

        let mut swapped = seal_manifest(&test_key(), &release(4));
        swapped.manifest = swapped.manifest.replace(&crate::digest::sha256_hex(WIN_FILE), &crate::digest::sha256_hex(b"evil"));
        assert_eq!(accept_on_windows(&swapped, 3), Err(Refusal::NotSigned(Verdict::Forged)));
    }

    #[test]
    fn a_release_signed_by_someone_else_is_refused() {
        let signed = seal_manifest(&signing_key_from_seed(&[9u8; 32]), &release(4));
        assert_eq!(accept_on_windows(&signed, 3), Err(Refusal::NotSigned(Verdict::Forged)));
    }

    #[test]
    fn with_no_release_key_set_every_release_is_refused() {
        let signed = seal_manifest(&test_key(), &release(4));
        let r = accept_against(&PLACEHOLDER_ANCHOR, &signed, 3, 3, "windows-x86_64", Direction::Forward);
        assert_eq!(r, Err(Refusal::NotSigned(Verdict::NoAnchor)));
        // The build as shipped trusts Eric's key only (27 Sep 2026): anyone
        // else's signature is refused as not his.
        assert_eq!(accept(&Installed::default(), &signed, 3, "windows-x86_64", Direction::Forward), Err(Refusal::NotSigned(Verdict::Forged)));
    }

    #[test]
    fn a_plain_signature_without_the_domain_tag_is_not_a_release() {
        // Domain separation: something the key signed that was NOT a release
        // must never be accepted as one, even if the bytes are identical.
        let text = serde_json::to_string(&release(4)).unwrap();
        let bare = SignedManifest { signature: to_hex(&sign(&test_key(), text.as_bytes())), manifest: text };
        assert_eq!(accept_on_windows(&bare, 3), Err(Refusal::NotSigned(Verdict::Forged)));
    }

    #[test]
    fn an_older_release_is_refused_unless_explicitly_rolling_back() {
        let old = seal_manifest(&test_key(), &release(2));
        assert_eq!(accept_on_windows(&old, 5), Err(Refusal::Downgrade { offered: 2, installed: 5 }));
        // A deliberate rollback is allowed — and still has to be signed.
        let back = accept_against(&anchor_of(&test_key()), &old, 5, 3, "windows-x86_64", Direction::Rollback(LocalApproval::given_by_the_person_at_this_device()));
        assert!(back.is_ok());
    }

    #[test]
    fn the_same_release_twice_is_already_installed_not_reinstalled() {
        let same = seal_manifest(&test_key(), &release(5));
        assert_eq!(accept_on_windows(&same, 5), Err(Refusal::AlreadyInstalled(5)));
    }

    #[test]
    fn a_release_that_cant_open_this_devices_data_is_refused() {
        let signed = seal_manifest(&test_key(), &release(4)); // needs data format 3
        let r = accept_against(&anchor_of(&test_key()), &signed, 3, 2, "windows-x86_64", Direction::Forward);
        assert_eq!(r, Err(Refusal::DataTooOld { needs: 3, have: 2 }));
    }

    #[test]
    fn a_release_with_no_build_for_this_device_says_so() {
        let signed = seal_manifest(&test_key(), &release(4));
        let r = accept_against(&anchor_of(&test_key()), &signed, 3, 3, "android-aarch64", Direction::Forward);
        assert_eq!(r, Err(Refusal::NotForThisPlatform("android-aarch64".into())));
    }

    #[test]
    fn a_path_in_a_file_name_is_refused_even_when_signed() {
        // Signing proves who wrote it, not that it's safe. A name that escapes
        // the download folder must never reach the installer.
        for bad in ["../../Startup/evil.exe", "..\\evil.exe", "C:evil.exe", "/etc/passwd", ".hidden", "a b.exe", ""] {
            let mut m = release(4);
            m.artifacts[0].file = bad.into();
            let signed = seal_manifest(&test_key(), &m);
            assert!(
                matches!(accept_on_windows(&signed, 3), Err(Refusal::Malformed(_))),
                "file name {bad:?} got through"
            );
        }
    }

    #[test]
    fn two_builds_for_one_platform_or_a_fake_fingerprint_are_refused() {
        let mut dup = release(4);
        dup.artifacts[1].platform = "windows-x86_64".into();
        assert!(matches!(accept_on_windows(&seal_manifest(&test_key(), &dup), 3), Err(Refusal::Malformed(_))));

        let mut fake = release(4);
        fake.artifacts[0].sha256 = "not-a-hash".into();
        assert!(matches!(accept_on_windows(&seal_manifest(&test_key(), &fake), 3), Err(Refusal::Malformed(_))));

        let mut empty = release(4);
        empty.artifacts.clear();
        assert!(matches!(accept_on_windows(&seal_manifest(&test_key(), &empty), 3), Err(Refusal::Malformed(_))));
    }

    #[test]
    fn a_manifest_in_a_format_this_build_doesnt_know_is_refused() {
        let mut m = release(4);
        m.format = 99;
        assert_eq!(accept_on_windows(&seal_manifest(&test_key(), &m), 3), Err(Refusal::UnsupportedFormat(99)));
    }

    #[test]
    fn an_absurdly_large_manifest_is_refused_before_any_checking() {
        let mut signed = seal_manifest(&test_key(), &release(4));
        signed.manifest.push_str(&" ".repeat(MAX_MANIFEST_BYTES));
        assert!(matches!(accept_on_windows(&signed, 3), Err(Refusal::TooLarge(_))));
    }

    #[test]
    fn a_garbled_signature_is_unreadable_not_a_crash() {
        let mut signed = seal_manifest(&test_key(), &release(4));
        signed.signature = "zz".repeat(64);
        assert!(matches!(accept_on_windows(&signed, 3), Err(Refusal::Unreadable(_))));
        signed.signature = "ab".into();
        assert!(matches!(accept_on_windows(&signed, 3), Err(Refusal::Unreadable(_))));
    }

    #[test]
    fn the_downloaded_file_must_match_its_signed_fingerprint_exactly() {
        let ok = accept_on_windows(&seal_manifest(&test_key(), &release(4)), 3).unwrap();
        assert_eq!(check_artifact(&ok.artifact, WIN_FILE), Ok(()));
        // One byte different: refused.
        let mut altered = WIN_FILE.to_vec();
        altered[0] ^= 1;
        assert_eq!(check_artifact(&ok.artifact, &altered), Err(Refusal::WrongHash));
        // Cut short (an interrupted download): refused, and said as such.
        assert!(matches!(check_artifact(&ok.artifact, &WIN_FILE[..5]), Err(Refusal::WrongSize { .. })));
    }

    #[test]
    fn every_refusal_says_something_plain() {
        // No path out of the checks may be silent or empty to the person.
        let all = [
            Refusal::TooLarge(1),
            Refusal::Unreadable("x".into()),
            Refusal::NotSigned(Verdict::Forged),
            Refusal::NotSigned(Verdict::NoAnchor),
            Refusal::UnsupportedFormat(2),
            Refusal::Malformed("x".into()),
            Refusal::AlreadyInstalled(1),
            Refusal::Downgrade { offered: 1, installed: 2 },
            Refusal::DataTooOld { needs: 2, have: 1 },
            Refusal::NotForThisPlatform("p".into()),
            Refusal::WrongSize { expected: 2, got: 1 },
            Refusal::WrongHash,
        ];
        for r in all {
            assert!(r.plain().len() > 15, "{r:?} says too little");
        }
    }

    // ---------------- gap B: checking while downloading ----------------

    #[test]
    fn a_file_checked_in_pieces_matches_the_same_as_whole() {
        let ok = accept_on_windows(&seal_manifest(&test_key(), &release(4)), 3).unwrap();
        let mut f = Fingerprint::for_artifact(&ok.artifact);
        for piece in WIN_FILE.chunks(7) {
            f.update(piece).unwrap();
        }
        assert_eq!(f.finish(), Ok(()));
    }

    #[test]
    fn a_download_is_stopped_the_moment_it_runs_past_the_signed_size() {
        // A peer streaming endless bytes is cut off at the signed size, not
        // after the phone's storage fills.
        let ok = accept_on_windows(&seal_manifest(&test_key(), &release(4)), 3).unwrap();
        let mut f = Fingerprint::for_artifact(&ok.artifact);
        f.update(WIN_FILE).unwrap();
        assert!(matches!(f.update(b"one byte too many"), Err(Refusal::WrongSize { .. })));
    }

    // ---------------- gap C: what the device remembers ----------------

    #[test]
    fn the_installed_release_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("atlas-release-installed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = crate::store::Store::new(&dir);
        let mut mine = Installed::load(&store);
        assert_eq!(mine.sequence, 0, "a fresh device has installed nothing");
        let ok = accept_on_windows(&seal_manifest(&test_key(), &release(4)), 0).unwrap();
        mine.installed(&ok);
        mine.save(&store).unwrap();
        let back = Installed::load(&store);
        assert_eq!(back.sequence, 4);
        assert_eq!(back.sha256, crate::digest::sha256_hex(WIN_FILE));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_same_builds_re_announced_are_noted_not_reinstalled() {
        let mut mine = Installed::default();
        let first = accept_on_windows(&seal_manifest(&test_key(), &release(4)), 0).unwrap();
        mine.installed(&first);
        // Eric re-announces the same builds under a new number (a notice).
        let again = accept_on_windows(&seal_manifest(&test_key(), &release(5)), 4).unwrap();
        assert!(!mine.needs_install(&again), "identical bytes must not be reinstalled");
    }

    // ---------------- gap D: rollback only from here ----------------

    #[test]
    fn a_manifest_cannot_smuggle_in_a_rollback_or_any_unknown_field() {
        // Even signed, a manifest with a field this build doesn't know is
        // refused — so no message can ever carry "and roll back".
        let mut v = serde_json::to_value(release(2)).unwrap();
        v["allow_older"] = serde_json::json!(true);
        let text = serde_json::to_string(&v).unwrap();
        let mut msg = MANIFEST_DOMAIN.to_vec();
        msg.extend_from_slice(text.as_bytes());
        let signed = SignedManifest { signature: to_hex(&sign(&test_key(), &msg)), manifest: text };
        assert!(matches!(accept_on_windows(&signed, 5), Err(Refusal::Unreadable(_))));
    }

    // ---------------- gap E: one name for each platform ----------------

    #[test]
    fn this_device_has_a_known_platform_name() {
        let me = this_platform().expect("the build host is a platform Atlas ships for");
        assert!(KNOWN_PLATFORMS.contains(&me));
    }

    #[test]
    fn a_misspelled_platform_is_refused_at_signing_time_not_found_missing_later() {
        let mut m = release(4);
        m.artifacts[0].platform = "win64".into();
        assert!(matches!(accept_on_windows(&seal_manifest(&test_key(), &m), 3), Err(Refusal::Malformed(_))));
    }

    #[test]
    fn every_known_platform_can_be_named_in_a_release() {
        for p in KNOWN_PLATFORMS {
            let mut m = release(4);
            m.artifacts.truncate(1);
            m.artifacts[0].platform = (*p).into();
            let signed = seal_manifest(&test_key(), &m);
            let ok = accept_against(&anchor_of(&test_key()), &signed, 3, 3, p, Direction::Forward);
            assert!(ok.is_ok(), "{p} did not round-trip");
        }
    }

    // ---------------- gap F: freshness ----------------

    #[test]
    fn a_device_says_so_when_release_notices_stop_arriving() {
        let mut mine = Installed::default();
        assert_eq!(freshness(&mine, 1_800_000_000), Freshness::NoNoticeYet);
        mine.saw(&release(4));
        let due = release(4).next_word_by;
        assert_eq!(freshness(&mine, due - 1), Freshness::Current);
        let late = freshness(&mine, due + 10 * 86_400);
        assert_eq!(late, Freshness::Overdue { days: 10 });
        assert!(late.plain().unwrap().contains("cut off"));
        assert!(Freshness::Current.plain().is_none());
    }

    #[test]
    fn a_release_whose_next_notice_date_is_not_in_its_future_is_refused() {
        let mut m = release(4);
        m.next_word_by = m.released_at;
        assert!(matches!(accept_on_windows(&seal_manifest(&test_key(), &m), 3), Err(Refusal::Malformed(_))));
    }

    #[test]
    fn a_release_that_writes_an_older_format_than_it_needs_is_refused() {
        let mut m = release(4);
        m.data_format = 2; // min_data_format is 3
        assert!(matches!(accept_on_windows(&seal_manifest(&test_key(), &m), 3), Err(Refusal::Malformed(_))));
    }

    // ---------------- gap G: rotation and recovery ----------------

    fn recovery_key() -> SigningKey {
        signing_key_from_seed(&[42u8; 32])
    }

    fn rotation_to(number: u32, key: &SigningKey) -> Rotation {
        Rotation { number, new_anchor: to_hex(&anchor_of(key)), reason: "planned".into() }
    }

    fn trusting_test_key() -> TrustState {
        TrustState { current: anchor_of(&test_key()), rotations: 0 }
    }

    #[test]
    fn a_planned_rotation_moves_trust_to_the_new_key() {
        let next = signing_key_from_seed(&[11u8; 32]);
        let signed = seal_rotation(&test_key(), &rotation_to(1, &next));
        let t = apply_rotation_with(&trusting_test_key(), &anchor_of(&recovery_key()), &signed).unwrap();
        assert_eq!(t.current, anchor_of(&next));
        // Releases now verify under the new key and not the old one.
        let by_new = seal_manifest(&next, &release(1));
        assert!(accept_against(&t.current, &by_new, 0, 3, "windows-x86_64", Direction::Forward).is_ok());
        let by_old = seal_manifest(&test_key(), &release(9));
        assert!(accept_against(&t.current, &by_old, 0, 3, "windows-x86_64", Direction::Forward).is_err());
    }

    #[test]
    fn a_rotation_not_signed_by_a_trusted_key_is_refused() {
        let stranger = signing_key_from_seed(&[13u8; 32]);
        let signed = seal_rotation(&stranger, &rotation_to(1, &stranger));
        let r = apply_rotation_with(&trusting_test_key(), &anchor_of(&recovery_key()), &signed);
        assert_eq!(r, Err(Refusal::NotSigned(Verdict::Forged)));
    }

    #[test]
    fn a_planned_rotation_cannot_be_replayed_or_skip_ahead() {
        let next = signing_key_from_seed(&[11u8; 32]);
        let signed = seal_rotation(&test_key(), &rotation_to(1, &next));
        let after = apply_rotation_with(&trusting_test_key(), &anchor_of(&recovery_key()), &signed).unwrap();
        // Replaying #1 on a device already at 1: refused (and it's not even
        // signed by the key trusted now).
        assert!(apply_rotation_with(&after, &anchor_of(&recovery_key()), &signed).is_err());
        let skip = seal_rotation(&test_key(), &rotation_to(3, &next));
        assert!(matches!(
            apply_rotation_with(&trusting_test_key(), &anchor_of(&recovery_key()), &skip),
            Err(Refusal::Malformed(_))
        ));
    }

    #[test]
    fn the_recovery_key_takes_back_a_stolen_release_key() {
        // A thief with the release key rotates devices to their own key...
        let thief = signing_key_from_seed(&[66u8; 32]);
        let stolen = seal_rotation(&test_key(), &rotation_to(1, &thief));
        let hijacked = apply_rotation_with(&trusting_test_key(), &anchor_of(&recovery_key()), &stolen).unwrap();
        assert_eq!(hijacked.current, anchor_of(&thief));
        // ...and signs a release numbered at the very top.
        let poisoned = seal_manifest(&thief, &release(u64::MAX - 1));
        let mut device = Installed { trust: hijacked.clone(), ..Installed::default() };
        let taken = accept(&device, &poisoned, 3, "windows-x86_64", Direction::Forward).unwrap();
        device.installed(&taken);
        // Eric recovers with the offline key, at any later number.
        let fresh = signing_key_from_seed(&[12u8; 32]);
        let recover = seal_rotation(&recovery_key(), &rotation_to(5, &fresh));
        device.trust = apply_rotation_with(&device.trust, &anchor_of(&recovery_key()), &recover).unwrap();
        assert_eq!(device.trust.current, anchor_of(&fresh));
        // The thief's key is dead, and the poisoned release number doesn't
        // lock Eric out: a new era counts fresh.
        assert!(accept(&device, &seal_manifest(&thief, &release(u64::MAX)), 3, "windows-x86_64", Direction::Forward).is_err());
        assert!(accept(&device, &seal_manifest(&fresh, &release(1)), 3, "windows-x86_64", Direction::Forward).is_ok());
    }

    #[test]
    fn a_rotation_to_a_useless_key_is_refused() {
        for bad in [to_hex(&PLACEHOLDER_ANCHOR), "zz".repeat(32), "ab".into(), to_hex(&anchor_of(&recovery_key()))] {
            let r = Rotation { number: 1, new_anchor: bad.clone(), reason: "x".into() };
            let signed = seal_rotation(&test_key(), &r);
            assert!(
                matches!(apply_rotation_with(&trusting_test_key(), &anchor_of(&recovery_key()), &signed), Err(Refusal::Malformed(_))),
                "rotated to {bad}"
            );
        }
    }

    #[test]
    fn with_no_recovery_key_set_only_the_current_key_can_rotate() {
        // The shipped placeholder recovery key signs nothing.
        let next = signing_key_from_seed(&[11u8; 32]);
        let by_recovery = seal_rotation(&recovery_key(), &rotation_to(1, &next));
        assert!(apply_rotation(&trusting_test_key(), &by_recovery).is_err());
    }

    // ---------------- gap A: one number for the shape of stored data ----------------

    #[test]
    fn a_device_on_the_current_data_format_accepts_a_release_for_it() {
        let mut m = release(4);
        m.min_data_format = crate::upgrade::DATA_FORMAT;
        m.data_format = crate::upgrade::DATA_FORMAT;
        let signed = seal_manifest(&test_key(), &m);
        let r = accept_against(&anchor_of(&test_key()), &signed, 3, crate::upgrade::DATA_FORMAT, "windows-x86_64", Direction::Forward);
        assert!(r.is_ok());
    }

    #[test]
    fn a_release_is_measured_from_its_files_not_typed() {
        let m = manifest_for(3, "1.3.0", 1, 100, 200, &[("windows-x86_64".into(), "atlas.exe".into(), b"hello".to_vec())]).unwrap();
        assert_eq!(m.artifacts[0].size, 5);
        assert_eq!(m.artifacts[0].sha256, crate::digest::sha256_hex(b"hello"));
        assert_eq!(m.data_format, crate::upgrade::DATA_FORMAT);
        assert!(manifest_for(3, "1.3.0", 1, 100, 200, &[("win64".into(), "a.exe".into(), b"x".to_vec())]).is_err(), "unknown platform");
        assert!(manifest_for(3, "1.3.0", 1, 100, 200, &[("windows-x86_64".into(), "../a.exe".into(), b"x".to_vec())]).is_err());
        let seed = new_seed();
        assert_eq!(seed_from_hex(&seed_hex(&seed)), Some(seed));
        assert!(as_rust_array(&[0xab; 32]).starts_with("[0xab, 0xab"));
    }


    // ------------------------------------------------ sending an update from the hub

    /// A version block the way Windows lays one out: the key in UTF-16, its
    /// terminator, padding to 4 bytes, then the value.
    fn version_block(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut b = Vec::new();
        for (k, v) in pairs {
            b.extend([0u8; 6]); // length, value length, type
            b.extend(k.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()));
            while b.len() % 4 != 0 {
                b.push(0);
            }
            b.extend(v.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()));
            while b.len() % 4 != 0 {
                b.push(0);
            }
        }
        b
    }

    fn fake_program(machine: u16, blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut b = vec![0u8; 0x40];
        b[..2].copy_from_slice(b"MZ");
        b[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        b.extend(b"PE\0\0");
        b.extend(machine.to_le_bytes());
        b.extend([0u8; 200]);
        for block in blocks {
            b.extend(block);
            b.extend([0u8; 700]);
        }
        b
    }

    fn edge() -> Vec<u8> {
        version_block(&[("ProductName", "Microsoft Edge"), ("ProductVersion", "129.0.2792.65")])
    }

    fn atlas_block(v: &str) -> Vec<u8> {
        version_block(&[("FileDescription", "Atlas"), ("ProductName", "Atlas"), ("ProductVersion", v)])
    }

    #[test]
    fn a_program_is_known_by_its_own_header_and_the_atlas_version_block_not_the_edge_one() {
        let p = fake_program(0x8664, &[edge(), atlas_block("0.2.0")]);
        assert_eq!(pe_platform(&p), Some("windows-x86_64"));
        assert_eq!(version_in_exe(&p).as_deref(), Some("0.2.0"), "the WebView2 part's version must not be taken for Atlas's");
        assert_eq!(pe_platform(&fake_program(0xAA64, &[])), Some("windows-aarch64"));
        assert_eq!(pe_platform(&fake_program(0x014C, &[])), None, "32-bit Windows isn't a platform Atlas ships");
        assert_eq!(pe_platform(b"#!/bin/sh\necho atlas"), None);
        assert_eq!(version_in_exe(&fake_program(0x8664, &[edge()])), None, "a program that doesn't say it's Atlas");
    }

    #[test]
    fn the_real_version_block_reads_as_this_version() {
        // The compiled resource build.rs links into atlas.exe.
        let res = include_bytes!("../windows/atlas-res.o");
        assert_eq!(version_in_exe(res).as_deref(), Some(env!("CARGO_PKG_VERSION")));
    }

    fn builds_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("atlas-builds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_newest_atlas_download_is_found_and_other_files_are_passed_over() {
        let d = builds_dir("newest");
        assert!(find_build(std::slice::from_ref(&d)).is_none(), "nothing there: nothing found");
        std::fs::write(d.join("atlas-old.exe"), fake_program(0x8664, &[atlas_block("0.1.0")])).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(d.join("Atlas Setup.exe"), fake_program(0x8664, &[edge(), atlas_block("0.2.0")])).unwrap();
        std::fs::write(d.join("holiday.exe"), b"MZ").unwrap();
        let other = builds_dir("newest-other");
        let found = find_build(&[d.clone(), other.clone()]).unwrap().unwrap();
        assert_eq!((found.version.as_str(), found.platform), ("0.2.0", "windows-x86_64"));
        assert!(found.path.ends_with("Atlas Setup.exe"));
        assert_eq!(found.sha256, crate::digest::sha256_hex(&found.program));
        // A newer file that isn't a build is passed over for the build behind
        // it (28 Sep 2026: it used to hide it).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(other.join("atlas-notes.zip"), b"not a zip").unwrap();
        let found = find_build(&[d.clone(), other.clone()]).unwrap().unwrap();
        assert!(found.path.ends_with("Atlas Setup.exe"), "{:?}", found.path);
        // Nothing usable at all: the page is told which file, and why.
        let (name, why) = find_build(std::slice::from_ref(&other)).unwrap().unwrap_err();
        assert_eq!(name, "atlas-notes.zip");
        assert!(why.contains("isn't a zip"), "{why}");
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn the_zip_github_hands_out_is_opened_for_the_program_not_tor() {
        let d = builds_dir("zip");
        let work = d.join("work");
        std::fs::create_dir_all(work.join("tor")).unwrap();
        std::fs::write(work.join("atlas.exe"), fake_program(0x8664, &[atlas_block("0.3.0")])).unwrap();
        std::fs::write(work.join("tor").join("tor.exe"), fake_program(0x8664, &[])).unwrap();
        let zipped = crate::tools::command("zip")
            .current_dir(&work)
            .args(["-q", "-r", d.join("Atlas-Windows-57.zip").to_str().unwrap(), "tor", "atlas.exe"])
            .status()
            .is_ok_and(|s| s.success());
        if !zipped {
            crate::outln!("SKIP: no zip tool here");
            return;
        }
        std::fs::remove_dir_all(&work).unwrap();
        let found = find_build(std::slice::from_ref(&d)).unwrap().unwrap();
        assert_eq!(found.version, "0.3.0");
        assert!(found.path.ends_with("Atlas-Windows-57.zip"));
        assert_eq!(found.sha256, crate::digest::sha256_hex(&fake_program(0x8664, &[atlas_block("0.3.0")])), "the program is what's signed, not the zip");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_found_build_is_signed_as_the_next_release_and_verifies() {
        let dir = builds_dir("sign");
        let store = crate::store::Store::new(&dir);
        let mut vault = crate::vault::Vault::load(&store);
        vault.open("a long passphrase", 1_000, &crate::vault::VaultConfig::default()).unwrap();
        let found = FoundBuild {
            path: dir.join("atlas.exe"),
            platform: "windows-x86_64",
            version: "0.2.0".into(),
            program: fake_program(0x8664, &[atlas_block("0.2.0")]),
            sha256: String::new(),
            modified: 0,
        };
        let refused = sign_build(&mut vault, &found, 3, 1_000, &[7; 32]).unwrap_err();
        assert!(refused.contains("no release key"), "{refused}");
        make_release_key(&mut vault, 1_000).unwrap();
        let seed = seed_from_hex(&vault.get(RELEASE_KEY_NAME, 1_000).unwrap()).unwrap();
        let mine = anchor_of(&signing_key_from_seed(&seed));
        let refused = sign_build(&mut vault, &found, 3, 1_000, &[7; 32]).unwrap_err();
        assert!(refused.contains("isn't the one this Atlas was built with"), "{refused}");
        let sent = sign_build(&mut vault, &found, 3, 1_000, &mine).unwrap();
        assert_eq!(sent.sequence, 4);
        let sig = from_hex_64(&sent.signed.signature).unwrap();
        assert!(verify_against(&mine, &domain_bytes(sent.signed.manifest.as_bytes()), &sig).is_trusted());
        let m: Manifest = serde_json::from_str(&sent.signed.manifest).unwrap();
        assert_eq!((m.sequence, m.version.as_str()), (4, "0.2.0"));
        assert_eq!(m.artifacts[0].platform, "windows-x86_64");
        assert_eq!(m.artifacts[0].sha256, crate::digest::sha256_hex(&found.program));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
