//! Keeping your Atlas yours, and your friends' theirs.
//!
//! You're handing this to people. That means the default has to be that two
//! installs know nothing about each other — not "they're separate unless you
//! link them", but **separate in a way that can't be undone by accident**.
//!
//! The danger isn't malice, it's convenience. Discovery on a shared wifi, a
//! private network someone joins to help you, a cloud folder in a family
//! account: any of those could quietly put two people's Atlas in the same
//! room. So belonging is decided by a key that only your devices have, and
//! nothing else — not the network, not the folder, not the account.

use serde::{Deserialize, Serialize};

/// One person's Atlas, across their devices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[derive(Default)]
pub struct Household {
    /// Random, made once, never derived from anything about you.
    pub id: String,
    /// What you call it.
    pub name: String,
    pub made_at: u64,
    /// Devices that belong.
    pub devices: Vec<String>,
}

/// What happens when one Atlas meets another.
#[derive(Debug, Clone, PartialEq)]
pub enum Meeting {
    /// Same household. Sync.
    Mine,
    /// Someone else's. Say nothing, do nothing, don't announce yourself.
    NotMine,
    /// A device of yours that hasn't been paired yet.
    NeedsPairing { code: String },
}

pub fn meets(my_household: &str, their_household: &str, paired: bool) -> Meeting {
    if my_household != their_household {
        // The important case. No offer, no prompt, no "would you like to
        // connect" — a friend on your sofa shouldn't get a popup.
        return Meeting::NotMine;
    }
    if paired {
        Meeting::Mine
    } else {
        Meeting::NeedsPairing { code: String::new() }
    }
}

/// Adding a device of your own.
///
/// The code is shown on one and typed on the other, so possession of both is
/// what proves it — not being on the same network, which anyone can be.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pairing {
    pub code: String,
    pub made_at: u64,
    /// Short on purpose.
    pub valid_secs: u64,
    pub used: bool,
}

impl Pairing {
    pub fn still_good(&self, now: u64) -> bool {
        !self.used && now.saturating_sub(self.made_at) < self.valid_secs
    }
}

pub fn new_pairing(code: &str, now: u64) -> Pairing {
    Pairing {
        code: code.into(),
        made_at: now,
        // Long enough to walk to the other device, short enough that a code
        // left on a screen is worthless by the time anyone finds it.
        valid_secs: 180,
        used: false,
    }
}

const PAIRING_TAG: &str = "ATLAS-HOUSEHOLD-1";

/// A pairing code carries the household it belongs to, not just the
/// challenge -- unlike `kin.rs`'s peer invites, a brand-new device has no
/// existing household to check the code against, so the code itself has to
/// be the thing that tells it which household it's joining. Possession of
/// the code (shown on one device, typed on the other) is still what proves
/// it, the same as `Pairing`'s own doc says.
pub fn encode_pairing(household_id: &str, household_name: &str, p: &Pairing) -> Option<String> {
    if household_id.contains('|') || household_name.contains('|') || p.code.contains('|') {
        return None;
    }
    Some(format!(
        "{PAIRING_TAG}:{household_id}|{household_name}|{}|{}|{}",
        p.code, p.made_at, p.valid_secs
    ))
}

/// The other half. Returns the household this code belongs to and the
/// pairing challenge it carries -- `still_good` still has to be checked by
/// the caller against the current time, this only decodes the shape.
pub fn decode_pairing(code: &str) -> Option<(String, String, Pairing)> {
    let code = code.trim();
    let rest = code.strip_prefix(&format!("{PAIRING_TAG}:"))?;
    let parts: Vec<&str> = rest.split('|').collect();
    let [id, name, challenge, made_at, valid_secs] = parts[..] else { return None };
    if id.trim().is_empty() || name.trim().is_empty() || challenge.trim().is_empty() {
        return None;
    }
    let made_at: u64 = made_at.parse().ok()?;
    let valid_secs: u64 = valid_secs.parse().ok()?;
    Some((
        id.to_string(),
        name.to_string(),
        Pairing { code: challenge.to_string(), made_at, valid_secs, used: false },
    ))
}

impl Household {
    pub fn load(store: &crate::store::Store) -> Household {
        store.load("household")
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save("household", self)
    }

    /// Whether this is a real household or just `Default`'s placeholder.
    /// `meets()`/`accept_bundle` must never compare two of these as if an
    /// empty id were a real one two installs happen to share.
    pub fn is_set(&self) -> bool {
        !self.id.is_empty()
    }
}

/// What to call this device when none was given: the machine's own name, or
/// a plain phrase when the system gives none.
pub fn this_device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "this device".into())
}

/// Start a household on this device: a fresh random id, its name, and this
/// device as its first member. Shared by `atlas household init` and the hub's
/// Sync page (27 Sep 2026: a fresh install's Sync page used to answer with the
/// command, which is no answer to somebody who never opens a terminal).
///
/// There is no re-init. A second household id for the same devices is exactly
/// the confusion this file exists to prevent, so a device that already has
/// one is told so rather than given another.
pub fn init(store: &crate::store::Store, name: &str, device: &str, now: u64) -> Result<Household, String> {
    let have = Household::load(store);
    if have.is_set() {
        return Err(format!(
            "This device already belongs to {}. There's no second household on one device -- \
             two ids for the same devices is the confusion this exists to prevent.",
            have.name
        ));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the household a name first -- anything you'd recognise, like \
                    \u{201c}Sam's devices\u{201d}."
            .into());
    }
    let device = if device.trim().is_empty() { this_device_name() } else { device.trim().to_string() };
    let id = crate::server::new_token().map_err(|e| format!("Couldn't generate a household id: {e}"))?;
    let h = Household { id, name: name.to_string(), made_at: now, devices: vec![device] };
    h.save(store).map_err(|e| format!("Couldn't save that: {e}"))?;
    Ok(h)
}


#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct HouseholdConfig {
    /// What to call this machine when it joins, so you are not asked on a
    /// phone keyboard. The join form is pre-filled with it and falls back to
    /// it when the box is left empty.
    pub device_name: String,
    // Two fields were deleted here on 19 Sep 2026, and the reasons differ.
    //
    // `household` was the household's id, "made at first run, never shared".
    // It was a second place to put an identity that `Household::load` already
    // holds in the store, and the worse of the two: an id in a text file is a
    // thing people copy between machines, which is how two devices end up
    // claiming the same household without either of them having been invited.
    // Nothing read it, which is the only reason that never happened.
    //
    // `discoverable` — "announce yourself on the local network" — described a
    // capability this tree does not have. Nothing announces anything: `mesh`
    // is unwired and named as such, and `server.reachable_from` is the
    // opposite approach, where you say the address yourself. A switch for a
    // behaviour that does not exist reads as a behaviour you have turned off,
    // which is worse than not offering it.
    //
    // Both are in `config::NO_FIELD_TO_LAND_IN`, so a file still setting
    // either is told by `atlas doctor` rather than ignored in silence.
}


/// A bundle that arrived from somewhere else.
///
/// The check that matters: a bundle carries its household, and one from
/// another is dropped without being read. Somebody AirDropping you their Atlas
/// export by mistake should be a non-event.
pub fn accept_bundle(my_household: &str, bundle_household: &str) -> Result<(), String> {
    if my_household != bundle_household {
        return Err(
            "that's someone else's Atlas — I've not opened it, and there's nothing for you to do"
                .into(),
        );
    }
    Ok(())
}

/// Sharing something deliberately with a friend.
///
/// Separate from syncing, and one-way: a note or a file, handed over as
/// content, with nothing about your household attached. Their Atlas takes it
/// in as something you sent, not as part of your world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handoff {
    /// What you're sending.
    pub what: String,
    /// Your name, so they know who it's from. Not your household id.
    pub from: String,
    /// Nothing else travels.
    pub carries_history: bool,
}

pub fn share_with_friend(what: &str, your_name: &str) -> Handoff {
    Handoff {
        what: what.into(),
        from: your_name.into(),
        carries_history: false,
    }
}

/// Something a friend's Atlas handed to yours, waiting for you to decide.
///
/// Deliberately not a tray item. The tray's rule is that handing Atlas a link
/// says "look at this" and Atlas then goes and reads it — which is right for
/// something *you* sent from your own phone, and wrong for something that
/// arrived unasked. Landing a friend's handoff straight in the tray would
/// mean a peer credential could make your Atlas fetch a URL, and "a friend I
/// paired with" is not the same trust as "me, from my other device".
///
/// So it waits here, as text in a list, until you say to keep it. `atlas
/// handoffs keep <id>` is the deliberate act that moves it into the tray, and
/// that act — not the arrival — is what lets Atlas look at it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Received {
    pub id: u64,
    /// The note, as sent.
    ///
    /// **Shown, never obeyed**, exactly like `tray::Item::found`. This is
    /// text from outside; the whole point of the separate inbox is that
    /// nothing reads it as an instruction.
    pub what: String,
    /// Who it came from — the name *you* gave that peer when you paired, not
    /// a name the sender claims in the message. A sender can put anything in
    /// a body; only the token says who they are.
    pub from: String,
    pub at: u64,
    /// The file, when one came with it — never the bytes themselves.
    ///
    /// The bytes are on disk under `handoffs/`, and this records where. Two
    /// reasons they are not in here: this whole list is loaded and re-saved
    /// as JSON on every arrival, and a peer's file has no business being
    /// held in memory for the life of the process.
    #[serde(default)]
    pub file: Option<ReceivedFile>,
}

/// A file waiting, described rather than held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReceivedFile {
    /// Already sanitised at the door by `kin`. Never used to build a path
    /// without `stored_at`, which is the path this actually wrote.
    pub name: String,
    pub size: u64,
    /// Where the bytes went, relative to the store root.
    pub stored_at: String,
}

/// Where a peer's file waits before you decide about it.
///
/// Deliberately not `tray::FOLDER`. The tray is what Atlas reads; this is
/// the doorstep. A file here has been written to disk and nothing else has
/// happened to it — not opened, not sniffed, not indexed.
pub const HANDOFF_FOLDER: &str = "handoffs";

/// The waiting list of things friends have handed over.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inbox {
    #[serde(default)]
    pub items: Vec<Received>,
    #[serde(default)]
    next_id: u64,
}

/// A cap, so a peer that has gone wrong fills a list rather than a disk.
/// Oldest goes first: a flood should not be able to push out the thing you
/// were actually waiting for by arriving after it, but it should also not
/// grow without end.
pub const MAX_WAITING: usize = 100;

impl Inbox {
    pub fn load(store: &crate::store::Store) -> Inbox {
        store.load("handoffs")
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save("handoffs", self)
    }

    /// Take one in. Returns its id.
    ///
    /// The same note from the same friend twice is the same note — a resend
    /// because the first did not seem to land should not become two entries.
    pub fn add(&mut self, what: &str, from: &str, at: u64) -> u64 {
        self.add_with(what, from, at, None)
    }

    /// The same, for a handoff that brought a file.
    ///
    /// The dedup key is deliberately different when a file is involved: two
    /// files from the same friend with the same covering line are two files,
    /// and collapsing them would silently drop one. Same name *and* same
    /// size is treated as a resend.
    pub fn add_with(
        &mut self,
        what: &str,
        from: &str,
        at: u64,
        file: Option<ReceivedFile>,
    ) -> u64 {
        let what = what.trim();
        let same = |i: &Received| match (&i.file, &file) {
            (Some(a), Some(b)) => i.from == from && a.name == b.name && a.size == b.size,
            (None, None) => i.what == what && i.from == from,
            _ => false,
        };
        if let Some(existing) = self.items.iter().find(|i| same(i)) {
            return existing.id;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Received {
            id,
            what: what.to_string(),
            from: from.to_string(),
            at,
            file,
        });
        if self.items.len() > MAX_WAITING {
            let drop = self.items.len() - MAX_WAITING;
            self.items.drain(0..drop);
        }
        id
    }

    pub fn get(&self, id: u64) -> Option<&Received> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Remove it, handing it back — for `keep`, which then puts it in the
    /// tray, and for `drop`, which does not.
    pub fn take(&mut self, id: u64) -> Option<Received> {
        let at = self.items.iter().position(|i| i.id == id)?;
        Some(self.items.remove(at))
    }

    /// One line each, for `atlas handoffs list` and for saying out loud.
    pub fn spoken(&self) -> String {
        match self.items.len() {
            0 => "Nothing from anyone.".into(),
            1 => format!("One thing, from {}.", self.items[0].from),
            n => {
                let mut who: Vec<&str> = self.items.iter().map(|i| i.from.as_str()).collect();
                who.sort_unstable();
                who.dedup();
                format!("{n} things, from {}.", who.join(" and "))
            }
        }
    }
}

/// What Atlas says when it sees another one.
pub fn saw_another(m: &Meeting) -> String {
    match m {
        // Silence is the answer. Saying "I can see someone else's Atlas" is
        // itself a small leak about them.
        Meeting::NotMine => String::new(),
        Meeting::Mine => "Talking to your other device.".into(),
        Meeting::NeedsPairing { .. } => {
            "There's another Atlas of yours here that we haven't paired. Want to?".into()
        }
    }
}

/// The rule, stated for anyone reading the source.
pub const SEPARATE_BY_DEFAULT: &str =
    "Two installs know nothing about each other unless the same person paired them, device to \
     device, with a code. Being on the same wifi doesn't do it. Being in the same private network \
     doesn't do it. Sharing a cloud folder doesn't do it. Nothing about your Atlas reaches your \
     friend's, and there's no setting that changes that.";

/// What a friend's copy is.
pub const THEIRS_IS_THEIRS: &str =
    "When you hand this to someone it makes its own household on first run. Their notes, their \
     projects, their vault — none of it comes near yours, and none of yours goes near theirs. \
     You can send them a thing on purpose, and that's the only way anything crosses.";

// ---------------------------------------------------------------------------
// Pairing by a short code
// ---------------------------------------------------------------------------
//
// `encode_pairing` puts everything in the code itself — household id, name,
// a `server::new_token` challenge, two timestamps — which makes a string
// nobody types. In practice it is pasted, which means it is emailed or
// messaged to yourself, which is the least secure thing in the whole design
// and the most annoying.
//
// The fix is to move what is bulky into the folder the two devices already
// share, and leave the person with the one thing that has to travel out of
// band: a short secret. Ten characters, typed once, in two groups of five.
//
// **What the folder holds is useless without it.** The invitation is a sealed
// blob and its header says only when it was made and how long it is good for
// — not whose it is, not what household, not what is inside. Somebody reading
// the folder learns that a pairing happened.
//
// The household key rides along inside the sealed part, so a device that
// joins can read sealed bundles immediately. That is the whole point: the
// alternative was copying a key file by hand, and a step like that is where
// people stop.
//
// The long block still works. Anyone mid-pairing when they update should not
// find their code rejected.

/// How long an invitation waits in the shared folder (30 Sep 2026: was
/// three minutes). It travels by OneDrive or Dropbox, which can take several
/// minutes to reach the other machine, so three minutes often ran out before
/// the file had even arrived. Sealed under the code and deleted once taken.
pub const INVITE_WAIT_SECS: u64 = 900;

/// How many characters a pairing code is. See `vault::short_code`.
pub const CODE_LEN: usize = 10;

/// What travels out of band.
pub fn new_invite_code() -> String {
    crate::vault::short_code(CODE_LEN)
}

/// An invitation, as it sits in the shared folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invitation {
    pub atlas_invite: u32,
    pub made_at: u64,
    pub valid_secs: u64,
    /// Base64 of the sealed [`Inside`].
    pub body: String,
}

/// What the code opens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inside {
    /// `for_household` for the same reason `sync::KeyHandoff` uses that name:
    /// the deadness scans count a field read by its bare name anywhere in the
    /// tree, so a field named after the one on `HouseholdConfig` takes a dead
    /// setting off the list without wiring a thing. It caught this twice in
    /// one sitting, which is the guard earning its place.
    pub for_household: String,
    pub name: String,
    /// The household key phrase, when the inviting device has one, so that
    /// joining and being able to read sealed bundles are the same act.
    #[serde(default)]
    pub key_phrase: Option<String>,
}

pub const INVITE_VERSION: u32 = 1;
const INVITE_CONTEXT: &[u8] = b"atlas-household-invitation-v1";

fn invite_key(code: &str) -> Result<Vec<u8>, String> {
    let tidy = crate::vault::tidy_recovery_key(code);
    if tidy.len() < CODE_LEN {
        return Err(format!("a pairing code is {CODE_LEN} characters in two groups of five"));
    }
    crate::vault::key_from_words(&tidy, INVITE_CONTEXT)
}

fn invite_path(folder: &std::path::Path, code: &str) -> std::path::PathBuf {
    // Named after the code's own fingerprint rather than the household, so
    // the filename tells a reader of the folder nothing, and two pairings at
    // once cannot collide.
    let tidy = crate::vault::tidy_recovery_key(code);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in tidy.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    folder.join(format!("{h:016x}.invite"))
}

/// Leave an invitation for whoever types the code.
pub fn leave_invitation(
    folder: &std::path::Path,
    household: &str,
    name: &str,
    code: &str,
    key_phrase: Option<&str>,
    now: u64,
    valid_secs: u64,
) -> Result<std::path::PathBuf, String> {
    let key = invite_key(code)?;
    let inside = Inside {
        for_household: household.to_string(),
        name: name.to_string(),
        key_phrase: key_phrase.map(|p| p.to_string()),
    };
    let plain = serde_json::to_vec(&inside).map_err(|e| e.to_string())?;
    let sealed = crate::vault::seal_aead(&plain, &key)?;
    let env = Invitation {
        atlas_invite: INVITE_VERSION,
        made_at: now,
        valid_secs,
        body: crate::b64::encode(&sealed),
    };
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let path = invite_path(folder, code);
    crate::sync::write_whole(&path, serde_json::to_string_pretty(&env).map_err(|e| e.to_string())?.as_bytes())?;
    Ok(path)
}

/// Take the invitation this code opens.
///
/// Tries every invitation in the folder rather than looking for one by name:
/// the name is a fingerprint of the code, so finding the right file *is* the
/// check, and trying them all keeps that true even if a sync client has
/// renamed one on the way through.
pub fn take_invitation(
    folder: &std::path::Path,
    code: &str,
    now: u64,
) -> Result<Inside, String> {
    let key = invite_key(code)?;
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Err("I can't see that folder from here".into());
    };
    let mut expired = false;
    for path in entries.flatten().map(|e| e.path()) {
        if path.extension().and_then(|x| x.to_str()) != Some("invite") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else { continue };
        let Ok(env) = serde_json::from_str::<Invitation>(&raw) else { continue };
        if env.atlas_invite > INVITE_VERSION {
            continue;
        }
        let Ok(sealed) = crate::b64::decode(&env.body) else { continue };
        let Ok(plain) = crate::vault::unseal_aead(&sealed, &key) else { continue };
        // It opened, so this is ours whatever the file is called. Expiry is
        // checked after opening rather than before: an invitation we cannot
        // open is not ours to have an opinion about.
        if now.saturating_sub(env.made_at) > env.valid_secs {
            expired = true;
            crate::heard!(std::fs::remove_file(&path));
            continue;
        }
        let inside: Inside =
            serde_json::from_slice(&plain).map_err(|_| "that invitation is damaged".to_string())?;
        crate::heard!(std::fs::remove_file(&path));
        return Ok(inside);
    }
    Err(if expired {
        "that code was right and the invitation had expired -- ask for a fresh one".into()
    } else {
        "no invitation in that folder opens with that code. Check the code, and that both \
         machines are pointed at the same folder."
            .into()
    })
}

/// Clear away invitations whose window has closed.
pub fn sweep_invitations(folder: &std::path::Path, now: u64) -> usize {
    let Ok(entries) = std::fs::read_dir(folder) else { return 0 };
    let mut gone = 0;
    for p in entries.flatten().map(|e| e.path()) {
        if p.extension().and_then(|x| x.to_str()) != Some("invite") {
            continue;
        }
        let stale = std::fs::read_to_string(&p)
            .ok()
            .and_then(|raw| serde_json::from_str::<Invitation>(&raw).ok())
            .map(|env| now.saturating_sub(env.made_at) > env.valid_secs)
            .unwrap_or(true);
        if stale && std::fs::remove_file(&p).is_ok() {
            gone += 1;
        }
    }
    gone
}
