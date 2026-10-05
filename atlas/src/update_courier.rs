//! Hearing about updates: the release channel, read by Atlas.
//!
//! Eric posts a signed release notice into a release channel -- a group he
//! owns where everyone else is a reader (`groups`). Every friend's Atlas is in
//! that group, so the notice reaches them the way any group message does, over
//! their own pairings, with nothing hosted anywhere. This file is what their
//! Atlas does when one arrives:
//!
//! 1. Only a message from the channel's **owner** is read at all (the daemon
//!    checks the signed group list before calling `heard`).
//! 2. The notice must carry a **release signature** that verifies against the
//!    key built into this Atlas (`release`). Who posted it doesn't make it
//!    true; the signature does. A notice that doesn't verify is refused and
//!    said, once.
//! 3. A verified notice counts as **having heard from Eric** even if it's for
//!    the release you already have -- that is what stops a device being
//!    quietly held back (`release::freshness`).
//! 4. A verified notice for a newer release that fits this device and can open
//!    its data is **recorded as available** and said once. Nothing downloads
//!    or installs from here: that is the desktop and phone apply step, and it
//!    will ask you before it changes anything.
//! 5. The file itself then **travels the same way**: in pieces, from the
//!    channel owner's Atlas over the pairing, resuming where it stopped, and
//!    kept only if the whole file's fingerprint is the one the signed notice
//!    gave. Eric's Atlas serves only files `atlas release sign` put aside,
//!    named by that fingerprint, and only to people it's paired with.

use crate::release::{self, Direction, Installed, SignedManifest};
use crate::store::Store;
use serde::{Deserialize, Serialize};

/// Where `atlas release announce` leaves a signed notice for the running
/// Atlas to post, inside the state folder.
pub const OUTBOX: &str = "release_outbox";

/// How a release notice starts, inside a channel message.
pub const PREFIX: &str = "atlas-release:";
const AVAILABLE: &str = "update_available";

/// A newer release this device has heard about and could take.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Available {
    pub version: String,
    pub sequence: u64,
    /// This device's build in it.
    pub file: String,
    pub size: u64,
    pub sha256: String,
    pub heard_at: u64,
    /// The notice itself, exactly as signed, so the apply step checks the
    /// very same signature again rather than trusting this record.
    pub notice: Option<SignedManifest>,
    /// The last refusal said, so the same one isn't said on every message.
    #[serde(default)]
    pub last_refusal: String,
    /// Whose release channel it came from, by key: where the file is fetched.
    #[serde(default)]
    pub from: String,
    /// Where the whole file is, once it's arrived and matched its fingerprint.
    #[serde(default)]
    pub downloaded: String,
}

impl Available {
    pub fn load(store: &Store) -> Available {
        store.load(AVAILABLE)
    }
    fn save(&self, store: &Store) {
        let _ = store.save(AVAILABLE, self);
    }

    /// Throw the downloaded file away but keep the signed notice, so the
    /// next ticks fetch it again from the start. For a file that arrived
    /// right and was damaged on disk afterwards: that's not the release's
    /// fault, and the release isn't given up on.
    pub fn fetch_again(store: &Store) {
        let mut a = Available::load(store);
        if !a.downloaded.is_empty() {
            crate::heard!(std::fs::remove_file(&a.downloaded));
        }
        a.downloaded.clear();
        a.save(store);
    }

    /// Forget the offered release: it's installed, or it failed here and
    /// won't be offered again. The downloaded file goes with it.
    pub fn forget_offer(store: &Store) {
        let a = Available::load(store);
        if !a.downloaded.is_empty() {
            crate::heard!(std::fs::remove_file(&a.downloaded));
        }
        let _ = store.save(AVAILABLE, &Available::default());
    }
}

/// The text to post into a release channel for a signed notice.
pub fn announcement(signed: &SignedManifest) -> String {
    format!("{PREFIX}{}", serde_json::to_string(signed).unwrap_or_default())
}

/// A message from the owner of a release channel (`from`, by key). Returns
/// what to tell you, if anything.
pub fn heard(store: &Store, body: &str, from: &str, now: u64) -> Option<String> {
    // A change of release key travels the same channel.
    if body.trim_start().starts_with(crate::update_apply::ROTATION_PREFIX) {
        return crate::update_apply::heard_rotation(store, body);
    }
    let said = heard_as(store, body, now, release::this_platform()?);
    let mut a = Available::load(store);
    if a.notice.is_some() && a.from.is_empty() {
        a.from = from.to_string();
        a.save(store);
    }
    said
}

/// Where the channel owner's Atlas keeps the files it will hand out, inside
/// its state folder, each named by its SHA-256.
pub const FILES: &str = "release_files";
/// Where a device puts a release while it arrives, inside its state folder.
pub const DOWNLOADS: &str = "release_download";
/// One piece of a file, as it travels.
pub const CHUNK: usize = 256 * 1024;

fn is_fingerprint(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Put a signed release's file aside to hand out, under its fingerprint.
pub fn keep_for_friends(state_root: &std::path::Path, bytes: &[u8]) -> Result<String, String> {
    let sha = crate::digest::sha256_hex(bytes);
    let dir = state_root.join(FILES);
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    std::fs::write(dir.join(&sha), bytes).map_err(|e| format!("couldn't keep the file to hand out: {e}"))?;
    prune_own_files(state_root, &sha);
    Ok(sha)
}

/// How many of the releaser's own signed builds are kept to hand out: the
/// newest, and two before it for a device that's behind.
pub const KEEP_OWN_FILES: usize = 3;

/// Every build signed here stayed in `release_files/` for ever -- a whole
/// program each (28 Sep 2026). Keep the newest `KEEP_OWN_FILES` (always
/// including `just_kept`); files passed on for another releaser are pruned
/// by their own rule (`prune_passed_on`) and left alone here.
fn prune_own_files(state_root: &std::path::Path, just_kept: &str) {
    let store = Store::new(state_root.to_path_buf());
    let passed: Vec<PassedOn> = store.load(PASSING_ON);
    let Ok(entries) = std::fs::read_dir(state_root.join(FILES)) else { return };
    let mut own: Vec<(std::time::SystemTime, String, std::path::PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !is_fingerprint(&name) || name == just_kept || passed.iter().any(|p| p.sha256 == name) {
                return None;
            }
            Some((e.metadata().and_then(|m| m.modified()).ok()?, name, e.path()))
        })
        .collect();
    own.sort();
    let extra = own.len().saturating_sub(KEEP_OWN_FILES.saturating_sub(1));
    for (_, _, path) in own.into_iter().take(extra) {
        crate::heard!(std::fs::remove_file(path));
    }
}

/// One piece of a kept file: the bytes from `offset`, and the whole size.
/// `None` for a name that isn't a fingerprint or a file that isn't kept.
pub fn chunk(state_root: &std::path::Path, sha: &str, offset: u64) -> Option<(Vec<u8>, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    if !is_fingerprint(sha) {
        return None;
    }
    // A build a failure report came back about isn't handed out while it's
    // being fixed (`update_apply::is_halted`).
    if crate::update_apply::is_halted(state_root, sha) {
        return None;
    }
    // A file this device only passes on (it isn't the releaser's own): not
    // once a newer release has been heard of, and not if it failed here.
    if !still_passing_on(&Store::new(state_root.to_path_buf()), sha) {
        return None;
    }
    let mut f = std::fs::File::open(state_root.join(FILES).join(sha)).ok()?;
    let total = f.metadata().ok()?.len();
    if offset > total {
        return None;
    }
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = Vec::with_capacity(CHUNK);
    f.take(CHUNK as u64).read_to_end(&mut buf).ok()?;
    Some((buf, total))
}

/// How fetching went this time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// Nothing to fetch, or already here.
    Nothing,
    /// Some arrived: (have, of).
    Partway(u64, u64),
    /// The whole file arrived and matched: say this.
    Ready(String),
    /// It arrived wrong and was thrown away: say this.
    Bad(String),
    /// Nobody answered; try later.
    Waiting,
}

/// Fetch up to `pieces` more of the available release, with `fetch(sha,
/// offset)` asking the channel owner's Atlas for the piece at `offset`.
/// Picks up where the last call stopped: what's already on disk is kept.
pub fn fetch_step(
    store: &Store,
    pieces: usize,
    mut fetch: impl FnMut(&str, u64) -> Option<(Vec<u8>, u64)>,
) -> Fetched {
    use std::io::Write;
    let mut a = Available::load(store);
    if a.notice.is_none() || !a.downloaded.is_empty() || !is_fingerprint(&a.sha256) {
        return Fetched::Nothing;
    }
    let dir = store.root().join(DOWNLOADS);
    if std::fs::create_dir_all(&dir).is_err() {
        return Fetched::Waiting;
    }
    let part = dir.join(format!("{}.part", a.sha256));
    let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    for _ in 0..pieces {
        if have >= a.size {
            break;
        }
        let Some((bytes, total)) = fetch(&a.sha256, have) else {
            return if have == 0 { Fetched::Waiting } else { Fetched::Partway(have, a.size) };
        };
        // The size is the notice's, not the sender's: a sender that says
        // otherwise is sending something else.
        if total != a.size || bytes.is_empty() || have + bytes.len() as u64 > a.size {
            crate::heard!(std::fs::remove_file(&part));
            return Fetched::Bad(format!(
                "The Atlas {} file being sent isn't the size the signed notice gave, so I threw it away.",
                a.version
            ));
        }
        let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&part) else {
            return Fetched::Waiting;
        };
        if f.write_all(&bytes).is_err() {
            return Fetched::Waiting;
        }
        have += bytes.len() as u64;
    }
    if have < a.size {
        return Fetched::Partway(have, a.size);
    }
    let Ok(all) = std::fs::read(&part) else { return Fetched::Waiting };
    if crate::digest::sha256_hex(&all) != a.sha256 {
        crate::heard!(std::fs::remove_file(&part));
        return Fetched::Bad(format!(
            "The Atlas {} file that arrived doesn't match the signed notice, so I threw it away and will fetch it again.",
            a.version
        ));
    }
    let name = std::path::Path::new(&a.file).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let done = dir.join(if name.is_empty() { a.sha256.clone() } else { name });
    if std::fs::rename(&part, &done).is_err() {
        return Fetched::Waiting;
    }
    a.downloaded = done.to_string_lossy().to_string();
    a.save(store);
    pass_on(store, &a, &all);
    Fetched::Ready(format!(
        "Atlas {} has arrived and matches what your release key signed. I'll ask you before installing it.",
        a.version
    ))
}

// ---- Passing a release on (gap AD, OPEN_GAPS 8.11) ------------------------
//
// A friend whose Atlas has a release that matched the signed notice hands it
// on the same way the releaser's does: a friend of theirs who can't reach the
// releaser (asleep, offline, or simply further away) gets it from them. The
// check is the signature, not who sent it -- the fetching side keeps a file
// only if its SHA-256 is the one your release key signed, whoever it came
// from (`fetch_step`). So passing on needs no trust in the one passing on.

/// Files this device passes on, and which release each was.
const PASSING_ON: &str = "release_passing_on";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct PassedOn {
    sha256: String,
    version: String,
    sequence: u64,
}

/// Keep a release that arrived and matched, to hand on to friends who ask
/// for it by fingerprint. The bytes are the checked ones.
fn pass_on(store: &Store, a: &Available, bytes: &[u8]) {
    let mut list: Vec<PassedOn> = store.load(PASSING_ON);
    if list.iter().any(|p| p.sha256 == a.sha256) {
        return;
    }
    if keep_for_friends(store.root(), bytes).is_err() {
        return;
    }
    list.push(PassedOn { sha256: a.sha256.clone(), version: a.version.clone(), sequence: a.sequence });
    let _ = store.save(PASSING_ON, &list);
    prune_passed_on(store);
}

/// Stop passing on anything older than the newest release heard of: a newer
/// one is out (often the fix for it), and friends should get that instead.
fn prune_passed_on(store: &Store) {
    let list: Vec<PassedOn> = store.load(PASSING_ON);
    if list.is_empty() {
        return;
    }
    let newest = list.iter().map(|p| p.sequence).max().unwrap_or(0).max(Available::load(store).sequence);
    let (keep, gone): (Vec<PassedOn>, Vec<PassedOn>) = list.into_iter().partition(|p| p.sequence >= newest);
    for g in &gone {
        crate::heard!(std::fs::remove_file(store.root().join(FILES).join(&g.sha256)));
    }
    if !gone.is_empty() {
        let _ = store.save(PASSING_ON, &keep);
    }
}

/// Is `sha` a file this device may hand out? The releaser's own files (put
/// aside by `atlas release sign`) always; a passed-on one only while it's the
/// newest release heard of and it hasn't failed on this device.
fn still_passing_on(store: &Store, sha: &str) -> bool {
    let list: Vec<PassedOn> = store.load(PASSING_ON);
    let Some(p) = list.iter().find(|p| p.sha256 == sha) else { return true };
    let newest = Available::load(store).sequence.max(p.sequence);
    p.sequence >= newest && crate::update_apply::not_offered_again(store, sha, &p.version).is_none()
}

/// Who has helped with the file now arriving, who answered last, and who
/// sent a file that turned out wrong -- per release file, so a new release
/// starts clean.
const FETCHING: &str = "release_fetching";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Fetching {
    sha256: String,
    last: String,
    helped: Vec<String>,
    bad: Vec<String>,
}

/// Who to ask for a release's pieces, in order: whoever answered last time,
/// then the releaser, then every other friend (any of them may have it by
/// now) -- leaving out anyone who sent this file wrong before.
fn sources(last: Option<&str>, owner: Option<&str>, friends: &[String], bad: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in last.into_iter().chain(owner).chain(friends.iter().map(String::as_str)) {
        if n.is_empty() || bad.iter().any(|b| b.eq_ignore_ascii_case(n)) {
            continue;
        }
        if !out.iter().any(|o| o.eq_ignore_ascii_case(n)) {
            out.push(n.to_string());
        }
    }
    out
}

/// `fetch_step`, asking several friends: each piece from the first who has
/// it, moving on when one doesn't answer, and starting next time with
/// whoever answered last. At most `max_asked` friends are tried in one call,
/// so one tick can't spend minutes on friends who are all away.
///
/// Pieces from different friends go into one file, which is only kept if the
/// whole matches the signed fingerprint. If it doesn't, everyone who sent a
/// piece of it -- except the releaser, whose own files are the ones signed --
/// isn't asked for this file again: one friend with a damaged copy can't
/// keep a download going round in circles.
pub fn fetch_from_any(
    store: &Store,
    pieces: usize,
    owner: Option<&str>,
    friends: &[String],
    max_asked: usize,
    mut fetch: impl FnMut(&str, &str, u64) -> Option<(Vec<u8>, u64)>,
) -> Fetched {
    let a = Available::load(store);
    let mut f: Fetching = store.load(FETCHING);
    if f.sha256 != a.sha256 {
        f = Fetching { sha256: a.sha256.clone(), ..Default::default() };
    }
    let order = sources((!f.last.is_empty()).then_some(f.last.as_str()), owner, friends, &f.bad);
    let mut at = 0usize;
    let mut helped: Vec<String> = Vec::new();
    let got = fetch_step(store, pieces, |sha, off| loop {
        if at >= order.len().min(max_asked) {
            return None;
        }
        let who = &order[at];
        if let Some(piece) = fetch(who, sha, off) {
            if !helped.contains(who) {
                helped.push(who.clone());
            }
            return Some(piece);
        }
        at += 1;
    });
    for h in helped {
        if !f.helped.contains(&h) {
            f.helped.push(h.clone());
        }
        f.last = h;
    }
    match &got {
        Fetched::Bad(_) => {
            let owner = owner.unwrap_or_default();
            for h in std::mem::take(&mut f.helped) {
                if !h.eq_ignore_ascii_case(owner) && !f.bad.contains(&h) {
                    f.bad.push(h);
                }
            }
            f.last.clear();
        }
        Fetched::Ready(_) => f.helped.clear(),
        _ => {}
    }
    let _ = store.save(FETCHING, &f);
    got
}

fn heard_as(store: &Store, body: &str, now: u64, platform: &str) -> Option<String> {
    let json = body.trim().strip_prefix(PREFIX)?;
    let mut avail = Available::load(store);
    let say_once = |avail: &mut Available, what: String| -> Option<String> {
        if avail.last_refusal == what {
            return None;
        }
        avail.last_refusal = what.clone();
        avail.save(store);
        Some(what)
    };
    let Ok(signed) = serde_json::from_str::<SignedManifest>(json) else {
        return say_once(&mut avail, "A release notice arrived that I couldn't read, so I ignored it.".into());
    };
    let mut installed = Installed::load(store);
    if let Some(m) = release::verified_notice(&installed, &signed) {
        installed.saw(&m);
        let _ = installed.save(store);
    }
    match release::accept(&installed, &signed, crate::upgrade::DATA_FORMAT, platform, Direction::Forward) {
        Ok(acc) if installed.needs_install(&acc) => {
            if avail.sequence >= acc.manifest.sequence && avail.notice.is_some() {
                return None;
            }
            // Failed here before, or you went back from it: not offered again.
            if let Some(why) = crate::update_apply::not_offered_again(store, &acc.artifact.sha256, &acc.manifest.version) {
                return say_once(&mut avail, format!("Atlas {} is out, but this build isn't being offered here: {why}.", acc.manifest.version));
            }
            let said = format!(
                "Atlas {} is out, signed by your release key. I'll ask you before installing it.",
                acc.manifest.version
            );
            avail = Available {
                version: acc.manifest.version.clone(),
                sequence: acc.manifest.sequence,
                file: acc.artifact.file.clone(),
                size: acc.artifact.size,
                sha256: acc.artifact.sha256.clone(),
                heard_at: now,
                notice: Some(signed),
                last_refusal: String::new(),
                from: String::new(),
                downloaded: String::new(),
            };
            avail.save(store);
            prune_passed_on(store);
            Some(said)
        }
        Ok(_) | Err(release::Refusal::AlreadyInstalled(_)) => None,
        Err(e) => say_once(&mut avail, e.plain()),
    }
}

/// For `atlas update`: what this device has heard, in a sentence or two.
pub fn status(store: &Store, now: u64) -> String {
    let installed = Installed::load(store);
    let fresh = match release::freshness(&installed, now) {
        release::Freshness::NoNoticeYet => "No release notice heard yet.".to_string(),
        release::Freshness::Current => "Release notices are arriving as expected.".to_string(),
        f => f.plain().unwrap_or_default(),
    };
    let a = Available::load(store);
    if a.notice.is_some() && a.sequence > installed.sequence_in_this_era() {
        let arrived = if a.downloaded.is_empty() { "" } else { " It's downloaded and checked." };
        format!("Atlas {} is available (heard from your release channel).{arrived} {fresh}", a.version)
    } else {
        fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::{anchor_of, seal_manifest, signing_key_from_seed, Artifact, Manifest, TrustState};

    fn store(tag: &str) -> Store {
        let d = std::env::temp_dir().join(format!("atlas-courier-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Store::new(d)
    }

    fn trusting(store: &Store, seed: [u8; 32]) {
        let mut i = Installed::load(store);
        i.trust = TrustState { current: anchor_of(&signing_key_from_seed(&seed)), rotations: 0 };
        i.save(store).unwrap();
    }

    fn notice(seed: [u8; 32], sequence: u64) -> String {
        let m = Manifest {
            format: release::MANIFEST_FORMAT,
            sequence,
            version: format!("1.{sequence}.0"),
            min_data_format: 1,
            data_format: 1,
            released_at: 1_000,
            next_word_by: 1_000 + 30 * 86_400,
            artifacts: vec![Artifact {
                platform: "linux-x86_64".into(),
                file: "atlas-linux".into(),
                size: 10,
                sha256: "ab".repeat(32),
            }],
        };
        announcement(&seal_manifest(&signing_key_from_seed(&seed), &m))
    }

    #[test]
    fn a_signed_newer_release_is_recorded_and_said_once() {
        let s = store("newer");
        trusting(&s, [9; 32]);
        let said = heard_as(&s, &notice([9; 32], 2), 2_000, "linux-x86_64").expect("said");
        assert!(said.contains("1.2.0"), "{said}");
        assert_eq!(Available::load(&s).sequence, 2);
        assert!(heard_as(&s, &notice([9; 32], 2), 2_100, "linux-x86_64").is_none(), "said twice");
        // Hearing it counts as hearing from you.
        assert_eq!(release::freshness(&Installed::load(&s), 2_000), release::Freshness::Current);
    }

    #[test]
    fn a_notice_not_signed_by_your_key_is_refused_and_recorded_nowhere() {
        let s = store("forged");
        trusting(&s, [9; 32]);
        let said = heard_as(&s, &notice([8; 32], 5), 2_000, "linux-x86_64").expect("refusal is said");
        assert!(said.contains("not signed"), "{said}");
        assert!(Available::load(&s).notice.is_none());
        assert_eq!(release::freshness(&Installed::load(&s), 2_000), release::Freshness::NoNoticeYet);
        assert!(heard_as(&s, &notice([8; 32], 6), 2_010, "linux-x86_64").is_none(), "the same refusal twice");
    }

    #[test]
    fn with_no_release_key_set_nothing_is_trusted() {
        let s = store("nokey");
        // The shipped build trusts Eric's key (27 Sep 2026), so a notice
        // signed by any other key is refused, and said.
        let said = heard_as(&s, &notice([9; 32], 2), 2_000, "linux-x86_64").expect("said");
        assert!(said.contains("not signed by your release key"), "{said}");
    }

    #[test]
    fn ordinary_messages_are_not_read_as_notices() {
        let s = store("chatter");
        assert!(heard_as(&s, "morning all", 2_000, "linux-x86_64").is_none());
        assert!(heard_as(&s, "atlas-release: not json", 2_000, "linux-x86_64").is_some_and(|m| m.contains("couldn't read")));
    }

    #[test]
    fn a_release_with_nothing_for_this_device_is_said_not_recorded() {
        let s = store("otherplatform");
        trusting(&s, [9; 32]);
        let said = heard_as(&s, &notice([9; 32], 2), 2_000, "macos-aarch64").expect("said");
        assert!(said.contains("no build for this device"), "{said}");
        assert!(Available::load(&s).notice.is_none());
        // It still counts as having heard from you.
        assert_eq!(release::freshness(&Installed::load(&s), 2_000), release::Freshness::Current);
    }

    fn with_file(tag: &str, bytes: &[u8]) -> (Store, std::path::PathBuf) {
        let s = store(tag);
        trusting(&s, [9; 32]);
        let owner = std::env::temp_dir().join(format!("atlas-courier-owner-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&owner);
        let sha = keep_for_friends(&owner, bytes).unwrap();
        let m = Manifest {
            format: release::MANIFEST_FORMAT,
            sequence: 2,
            version: "1.2.0".into(),
            min_data_format: 1,
            data_format: 1,
            released_at: 1_000,
            next_word_by: 1_000 + 30 * 86_400,
            artifacts: vec![Artifact { platform: "linux-x86_64".into(), file: "atlas-linux".into(), size: bytes.len() as u64, sha256: sha }],
        };
        heard_as(&s, &announcement(&seal_manifest(&signing_key_from_seed(&[9; 32]), &m)), 2_000, "linux-x86_64").unwrap();
        (s, owner)
    }

    #[test]
    fn a_release_arrives_in_pieces_resumes_and_is_kept_only_if_it_matches() {
        let bytes: Vec<u8> = (0..(CHUNK * 2 + 100)).map(|i| (i % 251) as u8).collect();
        let (s, owner) = with_file("pieces", &bytes);
        // One piece, then the sender goes away.
        let got = fetch_step(&s, 1, |sha, off| chunk(&owner, sha, off));
        assert_eq!(got, Fetched::Partway(CHUNK as u64, bytes.len() as u64));
        assert_eq!(fetch_step(&s, 5, |_, _| None), Fetched::Partway(CHUNK as u64, bytes.len() as u64));
        // Back: it carries on from where it stopped, not from the start.
        let mut asked = Vec::new();
        let got = fetch_step(&s, 5, |sha, off| {
            asked.push(off);
            chunk(&owner, sha, off)
        });
        assert!(matches!(got, Fetched::Ready(_)), "{got:?}");
        assert_eq!(asked.first(), Some(&(CHUNK as u64)), "started again from the beginning");
        let a = Available::load(&s);
        assert_eq!(std::fs::read(&a.downloaded).unwrap(), bytes);
        assert_eq!(fetch_step(&s, 5, |sha, off| chunk(&owner, sha, off)), Fetched::Nothing);
    }

    #[test]
    fn a_file_that_does_not_match_the_signed_notice_is_thrown_away() {
        let bytes = vec![7u8; 1000];
        let (s, _owner) = with_file("tampered", &bytes);
        let got = fetch_step(&s, 5, |_, off| (off == 0).then(|| (vec![8u8; 1000], 1000)));
        assert!(matches!(got, Fetched::Bad(ref m) if m.contains("doesn't match")), "{got:?}");
        assert!(Available::load(&s).downloaded.is_empty());
        // A sender claiming another size is refused straight away.
        let got = fetch_step(&s, 5, |_, _| Some((vec![7u8; 10], 5000)));
        assert!(matches!(got, Fetched::Bad(ref m) if m.contains("size")), "{got:?}");
    }

    #[test]
    fn only_kept_files_are_handed_out_and_only_by_fingerprint() {
        let owner = std::env::temp_dir().join(format!("atlas-courier-kept-{}", std::process::id()));
        let sha = keep_for_friends(&owner, b"release").unwrap();
        assert_eq!(chunk(&owner, &sha, 0).unwrap(), (b"release".to_vec(), 7));
        assert!(chunk(&owner, "../../etc/passwd", 0).is_none());
        assert!(chunk(&owner, &"0".repeat(64), 0).is_none());
        assert!(chunk(&owner, &sha, 99).is_none());
    }

    // ---- passing a release on (gap AD) ----

    fn release_bytes() -> Vec<u8> {
        (0..(CHUNK + 500)).map(|i| (i % 241) as u8).collect()
    }

    #[test]
    fn a_friend_who_has_the_release_hands_it_on_when_the_releaser_is_away() {
        let bytes = release_bytes();
        // Sam fetched it from Eric's Atlas, and it matched.
        let (sam, eric) = with_file("passon-sam", &bytes);
        assert!(matches!(fetch_from_any(&sam, 8, Some("Eric"), &[], 3, |_, sha, off| chunk(&eric, sha, off)), Fetched::Ready(_)));
        let sha = Available::load(&sam).sha256;
        // Maya heard the notice too, but Eric's Atlas is off. Sam's has it.
        let (maya, _) = with_file("passon-maya", &bytes);
        let mut asked = Vec::new();
        let got = fetch_from_any(&maya, 8, Some("Eric"), &["Eric".into(), "Sam".into()], 3, |who, sha, off| {
            asked.push(who.to_string());
            match who {
                "Sam" => chunk(sam.root(), sha, off),
                _ => None,
            }
        });
        assert!(matches!(got, Fetched::Ready(_)), "{got:?}");
        assert_eq!(asked.first().map(String::as_str), Some("Eric"), "the releaser is asked first");
        assert_eq!(std::fs::read(Available::load(&maya).downloaded).unwrap(), bytes);
        // And Maya now hands it on in turn, by the same fingerprint.
        assert_eq!(chunk(maya.root(), &sha, 0).map(|(b, t)| (b.len(), t)), Some((CHUNK, bytes.len() as u64)));
    }

    #[test]
    fn a_passed_on_release_stops_being_handed_out_once_a_newer_one_is_heard() {
        let bytes = release_bytes();
        let (sam, eric) = with_file("passon-newer", &bytes);
        fetch_from_any(&sam, 8, Some("Eric"), &[], 3, |_, sha, off| chunk(&eric, sha, off));
        let old = Available::load(&sam).sha256;
        assert!(chunk(sam.root(), &old, 0).is_some());
        // 1.3.0 is announced (often the fix for 1.2.0): Sam stops passing 1.2.0 on.
        heard_as(&sam, &notice([9; 32], 3), 3_000, "linux-x86_64").unwrap();
        assert!(chunk(sam.root(), &old, 0).is_none(), "an older release is still being passed on");
        assert!(!sam.root().join(FILES).join(&old).exists(), "the old file was kept");
    }

    #[test]
    fn a_release_that_failed_here_is_not_handed_on() {
        let bytes = release_bytes();
        let (sam, eric) = with_file("passon-failed", &bytes);
        fetch_from_any(&sam, 8, Some("Eric"), &[], 3, |_, sha, off| chunk(&eric, sha, off));
        let a = Available::load(&sam);
        // As `update_apply` records a build that failed here.
        #[derive(serde::Serialize)]
        struct NotAgain {
            builds: Vec<(String, String, String)>,
        }
        sam.save("update_not_again", &NotAgain { builds: vec![(a.sha256.clone(), a.version.clone(), "it failed here".into())] }).unwrap();
        assert!(crate::update_apply::not_offered_again(&sam, &a.sha256, &a.version).is_some(), "the record's shape changed");
        assert!(chunk(sam.root(), &a.sha256, 0).is_none());
        // The releaser's own files aren't affected by anything of the sort.
        assert!(chunk(&eric, &a.sha256, 0).is_some());
    }

    #[test]
    fn a_friend_with_a_damaged_copy_is_not_asked_again_for_it() {
        let bytes = release_bytes();
        let (maya, eric) = with_file("passon-bad", &bytes);
        // Sam's copy is damaged (right size, wrong bytes); Eric is away the first time.
        let damaged: Vec<u8> = bytes.iter().map(|b| b ^ 1).collect();
        let sam = |off: u64| {
            let end = (off as usize + CHUNK).min(damaged.len());
            (off as usize <= damaged.len()).then(|| (damaged[off as usize..end].to_vec(), damaged.len() as u64))
        };
        let got = fetch_from_any(&maya, 8, Some("Eric"), &["Sam".into()], 3, |who, _, off| (who == "Sam").then(|| sam(off)).flatten());
        assert!(matches!(got, Fetched::Bad(_)), "{got:?}");
        // Next time Sam isn't asked at all; Eric's back, and it arrives right.
        let mut asked = Vec::new();
        let got = fetch_from_any(&maya, 8, Some("Eric"), &["Sam".into()], 3, |who, sha, off| {
            asked.push(who.to_string());
            if who == "Sam" { sam(off) } else { chunk(&eric, sha, off) }
        });
        assert!(matches!(got, Fetched::Ready(_)), "{got:?}");
        assert!(!asked.iter().any(|w| w == "Sam"), "the friend with the damaged copy was asked again: {asked:?}");
    }

    #[test]
    fn friends_are_asked_in_a_sensible_order_and_only_a_few_per_try() {
        let f = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(sources(Some("Sam"), Some("Eric"), &f(&["Eric", "Maya", "sam"]), &[]), f(&["Sam", "Eric", "Maya"]));
        assert_eq!(sources(None, Some("Eric"), &f(&["Maya"]), &f(&["maya"])), f(&["Eric"]));
        let bytes = release_bytes();
        let (s, _) = with_file("passon-few", &bytes);
        let mut asked = 0;
        let got = fetch_from_any(&s, 8, None, &f(&["A", "B", "C", "D", "E"]), 3, |_, _, _| {
            asked += 1;
            None
        });
        assert_eq!(got, Fetched::Waiting);
        assert_eq!(asked, 3, "more friends than allowed were tried in one go");
    }
}
