//! Atlas on more than one device, without the copies drifting apart.
//!
//! I argued against this and I was wrong about the shape of the problem. What
//! I was worried about — two copies of your state disagreeing — is a real
//! problem if you sync *state*. It mostly disappears if you sync **what
//! happened**.
//!
//! Every device keeps an append-only log of events: you said this, a note was
//! captured, a task was finished. Merging is replaying both logs in order.
//! Appending can't conflict with appending, so two devices that have never
//! seen each other for six months merge cleanly. Only an edit to the same
//! thing on both sides needs a decision, and for one person that is rare.
//!
//! That means the phone can be a real Atlas rather than a window: it listens,
//! it talks, it thinks with a smaller model, it remembers. What it can't do is
//! touch your laptop's files and windows, which nobody expects of a phone.
//!
//! ## Getting the log across
//!
//! The log is one file. Anything that can move a file can sync Atlas: the same
//! wifi, a cloud folder, a cable, or AirDrop. There is no server anywhere in
//! this.

use serde::{Deserialize, Serialize};

/// What a device can do here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Your laptop. Files, windows, the big model, the vault.
    Full,
    /// Phone or tablet. Listens, talks, thinks smaller, remembers everything.
    ///
    /// Not a window — a real one, just without hands on your desktop.
    Standalone,
    /// Read-only, for something you don't want writing anything.
    Mirror,
}

impl Kind {
    /// Things only the laptop can do, and it isn't a slight on the phone —
    /// they're about hardware that isn't there.
    pub fn cannot(&self) -> &'static [&'static str] {
        match self {
            Kind::Full => &[],
            Kind::Standalone => &[
                "arrange your windows",
                "reach files on the laptop",
                "run the larger model",
                "open the vault",
            ],
            Kind::Mirror => &["anything at all — it only shows you things"],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    /// Stable and unique. Part of how events are ordered.
    pub id: String,
    pub name: String,
    pub kind: Kind,
    /// The last event from this device that others have seen.
    pub seen_up_to: u64,
}

/// One thing that happened. Append-only, never edited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Which device it happened on.
    pub device: String,
    /// Counts up on that device. Never reused.
    pub seq: u64,
    /// Wall-clock seconds, kept for display ("going back 12 days") and as the
    /// ordering key for bundles written before the clock existed.
    pub at: u64,
    /// The causal stamp this event orders by (see `hlc`). Skew-resistant, so
    /// two devices whose clocks disagree still replay in the same order.
    /// Defaults to the zero stamp for events from an older Atlas, which then
    /// fall back to `at` — see `effective_stamp`.
    #[serde(default)]
    pub hlc: crate::hlc::Stamp,
    pub what: What,
}

/// The stamp an event actually sorts by: its HLC when it has one, otherwise its
/// wall-clock second (for events written before the clock existed). Keeping
/// this in one place is what lets old and new bundles merge in one pass.
pub(crate) fn effective_stamp(e: &Event) -> crate::hlc::Stamp {
    if e.hlc.is_set() {
        e.hlc
    } else {
        crate::hlc::Stamp { wall: e.at, count: 0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum What {
    /// Something added. These never conflict with anything.
    Captured { id: String, text: String },
    Said { text: String, from_you: bool },
    /// Something finished.
    Finished { id: String },
    /// Something changed — the only kind that can clash.
    Changed { id: String, field: String, to: String },
    /// Something removed.
    Removed { id: String },
}

impl What {
    /// Can this clash with something on another device?
    ///
    /// Adding can't. Only touching the same field of the same thing can, and
    /// that's the whole conflict surface.
    pub fn can_clash(&self) -> bool {
        matches!(self, What::Changed { .. } | What::Removed { .. })
    }

    pub fn subject(&self) -> Option<&str> {
        match self {
            What::Captured { id, .. }
            | What::Finished { id }
            | What::Changed { id, .. }
            | What::Removed { id } => Some(id),
            What::Said { .. } => None,
        }
    }
}

/// A device's log.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Log {
    pub device: String,
    pub events: Vec<Event>,
    next_seq: u64,
    /// This device's causal clock, carried with the log so a restart never
    /// hands out a stamp it has already used. `serde(default)` so a log stored
    /// before the clock existed loads and starts stamping from zero — its old
    /// events fall back to `at`, and every new one is stamped.
    #[serde(default)]
    clock: crate::hlc::Clock,
}

impl Log {
    pub fn new(device: &str) -> Log {
        Log { device: device.into(), events: Vec::new(), next_seq: 0, clock: crate::hlc::Clock::new() }
    }

    /// Add a local event, stamping it with this device's clock. `at` is the
    /// wall-clock second (`store::now()`); the clock guarantees the stamp is
    /// strictly after every one this device has produced, even within the same
    /// second or across an OS clock that slipped.
    pub fn append(&mut self, what: What, at: u64) -> u64 {
        self.next_seq += 1;
        let hlc = self.clock.tick(at);
        self.events.push(Event {
            device: self.device.clone(),
            seq: self.next_seq,
            at,
            hlc,
            what,
        });
        self.next_seq
    }

    /// Everything after what the other side has seen.
    pub fn since(&self, seq: u64) -> Vec<&Event> {
        self.events.iter().filter(|e| e.seq > seq).collect()
    }

    /// Fold the stamps from a batch of incoming events into this device's
    /// clock, so anything it does *after* taking them in sorts after all of
    /// them. Call it when a bundle is accepted, before appending anything new.
    /// It changes no events — only the clock.
    ///
    /// `now` is this device's wall clock (`store::now()`); it is what the drift
    /// guard measures the incoming stamps against. Returns [`crate::hlc::Skew`]
    /// when the sending device's clock looked wrong — surface it rather than
    /// swallow it, but take the events in either way (nothing is lost).
    pub fn note_seen(&mut self, incoming: &[Event], now: u64) -> Option<crate::hlc::Skew> {
        let newest = incoming.iter().map(effective_stamp).max()?;
        self.clock.observe(newest, now)
    }

    fn latest_seq(&self) -> u64 {
        self.next_seq
    }
}

/// What happened when two logs met.
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    /// Events taken in without question.
    pub clean: usize,
    /// The same thing changed on both sides.
    pub clashes: Vec<Clash>,
    /// How far apart they were.
    pub gap_days: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Clash {
    pub subject: String,
    pub field: String,
    pub here: String,
    pub there: String,
    /// Which was later.
    pub later: &'static str,
}

/// Merge by replaying both in order.
///
/// Ordered by time, then by device id so it's deterministic — both sides doing
/// this independently must reach the same answer, or they'll drift again the
/// moment they part.
pub fn merge(mine: &[Event], theirs: &[Event], now: u64) -> Merged {
    let mut all: Vec<(&Event, bool)> =
        mine.iter().map(|e| (e, true)).chain(theirs.iter().map(|e| (e, false))).collect();
    // Ordered by the causal stamp, then device id, then seq — a total order
    // both sides compute identically. The stamp (not raw `at`) is what makes
    // two skewed clocks agree; `effective_stamp` falls back to `at` only for
    // pre-clock events, so old and new bundles still merge in one pass.
    all.sort_by(|(a, _), (b, _)| {
        effective_stamp(a)
            .cmp(&effective_stamp(b))
            .then(a.device.cmp(&b.device))
            .then(a.seq.cmp(&b.seq))
    });

    // Last change to each field, and where it came from.
    let mut last: std::collections::BTreeMap<(String, String), (&Event, bool, String)> =
        Default::default();
    let mut clean = 0;
    let mut clashes: Vec<Clash> = Vec::new();

    // A removal on one side racing an edit on the other. Keyed by subject,
    // not by field: deleting a thing is a claim about all of it.
    let mut removed: std::collections::BTreeMap<String, bool> = Default::default();

    for (e, is_mine) in &all {
        // Routed through the type's own answer, not a re-derivation of it.
        // This loop used to match `Changed` and send everything else to
        // "additive, simply lands" — while `can_clash` two hundred lines up
        // said `Removed` CAN clash. Two answers to one question, and the
        // named one was the one nobody asked: a delete on one device racing
        // an edit on the other merged clean, and one side's work silently
        // lost. `subject()` is the id both halves of that race share.
        if !e.what.can_clash() {
            clean += 1;
            continue;
        }
        // The id both halves of a race share, by the type's own accessor.
        let Some(subject) = e.what.subject().map(|s| s.to_string()) else {
            clean += 1;
            continue;
        };
        match &e.what {
            What::Changed { id, field, to } => {
                // An edit landing after the other side removed the thing is
                // a clash about existence, not about the field's value.
                if let Some(remover_mine) = removed.get(subject.as_str()) {
                    if remover_mine != is_mine {
                        clashes.push(Clash {
                            subject: subject.clone(),
                            field: field.clone(),
                            here: if *is_mine { to.clone() } else { "removed".into() },
                            there: if *is_mine { "removed".into() } else { to.clone() },
                            later: if *is_mine { "here" } else { "there" },
                        });
                        continue;
                    }
                }
                let _ = id;
                let key = (subject.clone(), field.clone());
                match last.get(&key) {
                    Some((prev, prev_mine, prev_to)) if prev_mine != is_mine && prev_to != to => {
                        // Both sides touched the same field with different
                        // answers.
                        let _ = prev;
                        clashes.push(Clash {
                            subject: subject.clone(),
                            field: field.clone(),
                            here: if *is_mine { to.clone() } else { prev_to.clone() },
                            there: if *is_mine { prev_to.clone() } else { to.clone() },
                            // Events are already in time order, so the one
                            // being looked at now is the later of the two —
                            // which side it came from is the answer.
                            later: if *is_mine { "here" } else { "there" },
                        });
                        last.insert(key, (e, *is_mine, to.clone()));
                    }
                    _ => {
                        last.insert(key, (e, *is_mine, to.clone()));
                        clean += 1;
                    }
                }
            }
            What::Removed { id } => {
                // A removal after the other side edited the same subject is
                // the same race from the other end.
                let _ = id;
                let edited_by_other = last
                    .iter()
                    .find(|((eid, _), (_, prev_mine, _))| *eid == subject && prev_mine != is_mine);
                match edited_by_other {
                    Some(((_, field), (_, _, prev_to))) => {
                        clashes.push(Clash {
                            subject: subject.clone(),
                            field: field.clone(),
                            here: if *is_mine { "removed".into() } else { prev_to.clone() },
                            there: if *is_mine { prev_to.clone() } else { "removed".into() },
                            later: if *is_mine { "here" } else { "there" },
                        });
                    }
                    None => clean += 1,
                }
                removed.insert(subject.clone(), *is_mine);
            }
            // `can_clash` said no for everything else, so this arm is
            // unreachable — kept as a landing, not a decision.
            _ => clean += 1,
        }
    }

    let oldest = all.iter().map(|(e, _)| e.at).min().unwrap_or(now);
    Merged {
        clean,
        clashes,
        gap_days: now.saturating_sub(oldest) / 86_400,
    }
}

/// The file that moves between devices.
///
/// One file, encrypted, self-describing. Anything that can move a file can
/// sync Atlas — including AirDrop, which is the point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub from_device: String,
    pub from_name: String,
    pub made_at: u64,
    /// Where the sender had got to, so the receiver can send back only what's
    /// missing.
    pub up_to_seq: u64,
    pub events: Vec<Event>,
    /// Format version, because a bundle from six months ago has to still open.
    pub version: u32,
    /// Which Atlas this belongs to.
    ///
    /// Two installs merge only when this matches. Decided 18 Sep 2026: two
    /// Atlases with different jobs (a personal one and a work one, say) do not
    /// sync -- they are linked through the business hub, deliberately and item
    /// by item, which is a different thing from replaying one's log into the
    /// other.
    ///
    /// Without this the only thing keeping them apart is nobody pointing both
    /// at the same folder, and "nobody has made that mistake yet" is not a
    /// boundary. A decision recorded in a handover and not in the code is one
    /// nobody can rely on.
    ///
    /// Defaulted rather than required so a bundle written before this field
    /// existed still opens, and reads as personal -- which is what those
    /// bundles were.
    #[serde(default = "personal")]
    pub belongs_to: String,
}

fn personal() -> String {
    "personal".into()
}

pub const BUNDLE_VERSION: u32 = 1;

pub fn make_bundle(log: &Log, name: &str, since: u64, now: u64) -> Bundle {
    Bundle {
        from_device: log.device.clone(),
        from_name: name.into(),
        made_at: now,
        up_to_seq: log.latest_seq(),
        events: log.since(since).into_iter().cloned().collect(),
        version: BUNDLE_VERSION,
        belongs_to: personal(),
    }
}

/// Is this bundle from the same Atlas as the one reading it?
///
/// Not a security boundary against a hostile party -- a bundle in a folder you
/// control is not an attack. It is the boundary between *your two Atlases*,
/// which have different jobs and different material. A work Atlas's
/// captures have no business in your personal notes, and a shared folder set
/// by accident is exactly how they would get there.
///
/// Both sides empty means both are personal, which is what every bundle
/// written before this existed was.
pub fn from_the_same_atlas(b: &Bundle, mine: &str) -> std::result::Result<(), String> {
    let theirs = if b.belongs_to.trim().is_empty() { "personal" } else { b.belongs_to.trim() };
    let mine = if mine.trim().is_empty() { "personal" } else { mine.trim() };
    if theirs.eq_ignore_ascii_case(mine) {
        return Ok(());
    }
    Err(format!(
        "a bundle from your {theirs} Atlas is in this folder, and this one is {mine} — \
         I've left it alone"
    ))
}

/// Can this bundle be opened?
pub fn can_open(b: &Bundle) -> Result<(), String> {
    if b.version > BUNDLE_VERSION {
        return Err(format!(
            "that came from a newer Atlas — version {} against my {BUNDLE_VERSION}. Update this \
             one first",
            b.version
        ));
    }
    Ok(())
}

/// A bundle isn't a file you manage.
///
/// It's registered as its own kind of thing, so sharing it offers Atlas and
/// the other side takes it straight in. You tap share, you pick Atlas, and the
/// other device says "14 things from your phone" — no folder, no filename,
/// nothing to tidy up afterwards.
pub const NOT_A_FILE_YOU_MANAGE: &str =
    "You don't handle a file. Share it, pick Atlas on the other device, and it's taken in — the \
     bundle is registered as its own kind of thing, so it never lands in Files or your Downloads \
     folder. Sending twice is harmless too: anything already seen is skipped.";

/// The same bundle arriving twice.
///
/// Sending again is the normal response to not being sure it landed, so it has
/// to be free rather than something that duplicates your notes.
pub fn already_seen(bundle_events: &[Event], seen_up_to: &[(String, u64)]) -> (usize, usize) {
    let mut new = 0;
    let mut skipped = 0;
    for e in bundle_events {
        let seen = seen_up_to
            .iter()
            .find(|(d, _)| *d == e.device)
            .map(|(_, s)| e.seq <= *s)
            .unwrap_or(false);
        if seen {
            skipped += 1;
        } else {
            new += 1;
        }
    }
    (new, skipped)
}

/// How a bundle gets across.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Carry {
    /// Both on the same wifi. Automatic, nothing stored in between.
    SameNetwork,
    /// A folder both can see. Works when they're never on together.
    CloudFolder,
    /// The phone plugged in. No network of any kind needed.
    Cable,
    /// One tap, no network, no account.
    AirDrop,
}

impl Carry {
    pub fn needs_internet(&self) -> bool {
        matches!(self, Carry::CloudFolder)
    }
    pub fn automatic(&self) -> bool {
        matches!(self, Carry::SameNetwork | Carry::CloudFolder)
    }
    pub fn plain(&self) -> &'static str {
        match self {
            Carry::SameNetwork => "same wifi — happens by itself, nothing stored in between",
            Carry::CloudFolder => "through your cloud folder — works when they're never on together",
            Carry::Cable => "plugged in — no wifi, no service, no account",
            Carry::AirDrop => "AirDropped — one tap, phone to iPad, nothing online",
        }
    }
}

/// Pick how to sync, from what's actually available.
pub fn how_to_carry(
    same_network: bool,
    plugged_in: bool,
    cloud_available: bool,
    both_apple: bool,
) -> (Carry, &'static str) {
    // A connection first, because most of the time there is one and it needs
    // nothing from you. The cable is for when there isn't.
    if same_network {
        return (Carry::SameNetwork, "same wifi — nothing for you to do");
    }
    if plugged_in {
        return (Carry::Cable, "no network, but you're plugged in — I'll use that");
    }
    if cloud_available {
        return (Carry::CloudFolder, "through the cloud folder — it'll land next time both are on");
    }
    if both_apple {
        return (Carry::AirDrop, "AirDrop it and the other side will take it in");
    }
    (Carry::Cable, "nothing automatic is available — plug it in when you can")
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SyncConfig {
    pub enabled: bool,
    /// This device's name.
    pub name: String,
    pub kind: String,
    /// Sync by itself when it can.
    pub automatic: bool,
    /// Warn when devices have been apart this long.
    pub apart_warning_days: u64,
    /// Which Atlas this install is.
    ///
    /// Two installs carry each other only when this matches. Leave it as
    /// `personal` for your own machines. A work Atlas sets its own, and
    /// then even one shared folder cannot merge the two -- they are linked
    /// through the business hub instead, deliberately and item by item.
    pub belongs_to: String,
    /// The folder bundles are written to and read from.
    ///
    /// A folder is the carrier this design already names -- `Carry::CloudFolder`,
    /// "it'll land next time both are on" -- and it is the only one that needs
    /// nothing running on the other machine and no network of its own. Point
    /// both devices at the same synced folder, a USB stick, or a share, and
    /// they carry each other. Empty means bundles are not written anywhere,
    /// and `Intent::Sync` says so rather than pretending.
    pub folder: String,
    /// Seal bundles before they are written to the carrier.
    ///
    /// **Off, and it ships off on purpose.** On means a bundle is unreadable
    /// without the household key phrase — including by you, and including by
    /// this machine if the key is lost. Somebody setting sync up for the
    /// first time should not be able to lock themselves out of their own
    /// folder before they have understood what the phrase is for.
    ///
    /// Turning it on with no key set does not write plaintext as a fallback.
    /// It refuses and says so: a switch that quietly does the opposite of
    /// what it says is the failure this whole campaign is about.
    ///
    /// Reading is not gated by it. A sealed bundle opens whenever the key is
    /// there, on or off, so turning it off again does not strand what was
    /// already written.
    pub encrypt_bundles: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            enabled: true,
            name: String::new(),
            kind: "full".into(),
            automatic: true,
            apart_warning_days: 30,
            belongs_to: personal(),
            folder: String::new(),
            encrypt_bundles: false,
        }
    }
}

/// What Atlas says after a merge.
pub fn spoken(m: &Merged, from: &str) -> String {
    if m.clean == 0 && m.clashes.is_empty() {
        return format!("Nothing new from {from}.");
    }
    let mut s = format!("{} things from {from}", m.clean);
    if m.gap_days > 30 {
        s.push_str(&format!(", going back {} days", m.gap_days));
    }
    s.push('.');

    if !m.clashes.is_empty() {
        let c = &m.clashes[0];
        s.push_str(&format!(
            " {} clashed — {} is \"{}\" here and \"{}\" there. The {} one is newer.",
            m.clashes.len(),
            c.subject,
            c.here,
            c.there,
            c.later
        ));
        s.push_str(" Which?");
    }
    s
}

/// Two devices that haven't met in a while.
pub fn drifting(days_apart: u64, cfg: &SyncConfig) -> Option<String> {
    if days_apart < cfg.apart_warning_days {
        return None;
    }
    Some(format!(
        "{days_apart} days since these two last talked. Nothing's lost — it all merges — but \
         each one's picture of the other is that old."
    ))
}

/// Why two real copies is fine, now that it's said out loud.
pub const WHY_THIS_WORKS: &str =
    "Appending can't conflict with appending. Two devices that haven't seen each other since \
     March merge cleanly, because neither is trying to overwrite the other — they're both just \
     saying what happened, and I replay both in order. The only thing that needs you is the same \
     thing changed on both sides, and there's only one of you.";

/// What the phone genuinely can't do, said plainly rather than dressed up.
pub const WHAT_THE_PHONE_MISSES: &str =
    "It won't arrange your windows or reach files on the laptop, because they aren't there. It \
     thinks with a smaller model, so long reasoning is better on the laptop. Everything else — \
     talking, capturing, remembering, your projects, your notes — is the same Atlas.";

// ---------------------------------------------------------------------------
// Sealing a bundle
// ---------------------------------------------------------------------------
//
// Until 18 Sep 2026 a bundle was written with `serde_json::to_string_pretty`
// into whatever folder carries it -- which for most people is a cloud folder
// somebody else's servers hold a copy of -- and `cloudsync::WHY_ONEDRIVE`
// told the reader "I encrypt before anything is written either way". It did
// not. What follows is that sentence made true, and it is **off by default**:
// `sync.encrypt_bundles` ships `false` so that nobody is locked out of their
// own carrier while setting one up.
//
// ## The design, and what it is built backwards from
//
// Not from the cipher. From "Eric needs to read one of these on a machine
// where Atlas is broken." Everything below follows from that:
//
// - **The phrase is the key.** `vault::key_from_words` derives the 32 bytes
//   from the phrase with a fixed context label, so typing the same phrase
//   anywhere produces the same key. There is no wrapped blob that has to
//   travel, and no device that holds the only copy.
// - **The header stays in the clear.** Which device wrote it, which Atlas it
//   belongs to, and when. That is what lets a machine skip its own bundle and
//   refuse another household's without holding a key at all -- and it is the
//   only thing a reader of the folder learns.
// - **A sealed bundle says it is sealed.** `atlas sync read` on one without a
//   key says so in a sentence naming the command that fixes it, rather than
//   failing to parse.
//
// ## What this protects against, and what it does not
//
// It protects the folder: the cloud provider, anyone signed into that
// account, anyone the folder is shared with. It does nothing against someone
// at your unlocked machine, and it is not meant to -- the vault is that job,
// and the vault never leaves.

/// The context label for the household key. Fixed, and never reused.
///
/// A phrase used for two purposes must not produce the same key twice; this
/// is what keeps a sync key from being a vault key even if somebody types the
/// same words into both.
pub const KEY_CONTEXT: &[u8] = b"atlas-sync-household-key-v1";

/// Where the phrase is kept on this device.
pub const KEY_FILE: &str = "sync_key";

/// A fresh household key phrase.
///
/// The vault's recovery-key alphabet and grouping, on purpose: twenty-four
/// characters in groups of four, no letters that look like digits, and
/// `vault::tidy_recovery_key` forgiving the ways a person copying it from
/// paper gets it wrong.
pub fn new_key_phrase() -> String {
    crate::vault::new_recovery_key()
}

/// The key those words stand for.
pub fn key_from_phrase(phrase: &str) -> Result<Vec<u8>, String> {
    let tidy = crate::vault::tidy_recovery_key(phrase);
    if tidy.len() < 16 {
        return Err(
            "that isn't a household key -- they're 24 characters in groups of four".into()
        );
    }
    crate::vault::key_from_words(&tidy, KEY_CONTEXT)
}

/// The phrase as this device keeps it.
///
/// Sealed with the operating system's own protection where that exists, which
/// today means Windows. Everywhere else it is stored as typed, and
/// `at_rest_says` is what says so out loud rather than leaving it to be
/// assumed — a key file that is only as protected as the folder it sits in is
/// a fact about your machine, not a detail.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeptKey {
    /// Base64 of the OS-sealed phrase, or the phrase itself when there is no
    /// OS protection to use.
    pub blob: String,
    pub sealed_by_os: bool,
    pub set_at: u64,
}

impl KeptKey {
    pub fn is_set(&self) -> bool {
        !self.blob.trim().is_empty()
    }

    /// The phrase back, whichever way it was kept.
    pub fn phrase(&self) -> Result<String, String> {
        if !self.is_set() {
            return Err("no household key is set on this device".into());
        }
        if !self.sealed_by_os {
            return Ok(self.blob.clone());
        }
        let raw = crate::b64::decode(&self.blob).map_err(|e| e.to_string())?;
        let open = crate::vault::unseal_bytes(&raw, KEY_CONTEXT)?;
        String::from_utf8(open).map_err(|_| "the stored key is not readable text".into())
    }

    /// Keep a phrase, sealed by the OS where that is possible.
    pub fn keeping(phrase: &str, now: u64) -> KeptKey {
        match crate::vault::seal_bytes(phrase.as_bytes(), KEY_CONTEXT) {
            Ok(sealed) => {
                KeptKey { blob: crate::b64::encode(&sealed), sealed_by_os: true, set_at: now }
            }
            // No OS protection here. Kept as typed rather than not kept at
            // all, and said plainly by `at_rest_says`.
            Err(_) => KeptKey { blob: phrase.to_string(), sealed_by_os: false, set_at: now },
        }
    }

    pub fn at_rest_says(&self) -> &'static str {
        if self.sealed_by_os {
            "Kept sealed by Windows, so a copy of the file taken off this machine is useless."
        } else {
            "Kept as text in your data folder: this machine has no OS key protection I can \
             use, so anyone who can read that folder — or a backup of it — has the key."
        }
    }
}

/// A bundle with its contents sealed.
///
/// The fields outside `body` are the ones a machine needs before it can
/// decide whether to try: its own bundle is skipped by `from_device`, another
/// Atlas's by `belongs_to`. Everything that came out of your notes is inside
/// `body`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SealedBundle {
    /// Format marker, and what tells a reader this is not a plain bundle.
    pub atlas_sealed_bundle: u32,
    pub from_device: String,
    pub belongs_to: String,
    pub made_at: u64,
    /// Base64 of nonce followed by ciphertext, as `vault::seal_aead` writes it.
    pub body: String,
}

pub const SEALED_VERSION: u32 = 1;

/// Seal a bundle for the folder.
pub fn seal(b: &Bundle, key: &[u8]) -> Result<String, String> {
    let plain = serde_json::to_vec(b).map_err(|e| e.to_string())?;
    let sealed = crate::vault::seal_aead(&plain, key)?;
    let env = SealedBundle {
        atlas_sealed_bundle: SEALED_VERSION,
        from_device: b.from_device.clone(),
        belongs_to: b.belongs_to.clone(),
        made_at: b.made_at,
        body: crate::b64::encode(&sealed),
    };
    serde_json::to_string_pretty(&env).map_err(|e| e.to_string())
}

/// Is this file a sealed bundle, and if so what does its header say?
pub fn peek(text: &str) -> Option<SealedBundle> {
    let env: SealedBundle = serde_json::from_str(text).ok()?;
    (env.atlas_sealed_bundle > 0).then_some(env)
}

/// Read a bundle file, sealed or not.
///
/// One function for both so that no call site has to remember to check, which
/// is how a plaintext path survives a feature like this. A sealed bundle with
/// no key is an error that names the command that fixes it.
pub fn read_bundle(text: &str, key: Option<&[u8]>) -> Result<Bundle, String> {
    read_bundle_for(text, key, Reader::Page)
}

/// Who is reading a bundle, which decides where "you have no key" points.
///
/// Until 27 Sep 2026 the message named `atlas sync key set` wherever it was
/// said — including out of the running Atlas, to a person who never opens a
/// terminal. The Sync page now takes a key from another device, so the
/// running Atlas points there; the command line still names its command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reader {
    /// `atlas sync read`, printed in a terminal.
    Command,
    /// The running Atlas: said aloud, logged, or shown on a hub page.
    Page,
}

/// As `read_bundle`, saying where to set a missing key in the reader's terms.
pub fn read_bundle_for(text: &str, key: Option<&[u8]>, reader: Reader) -> Result<Bundle, String> {
    let Some(env) = peek(text) else {
        return serde_json::from_str::<Bundle>(text)
            .map_err(|e| format!("that isn't a bundle I can read: {e}"));
    };
    if env.atlas_sealed_bundle > SEALED_VERSION {
        return Err(format!(
            "that bundle was sealed by a newer Atlas — format {} against my {SEALED_VERSION}",
            env.atlas_sealed_bundle
        ));
    }
    let Some(key) = key else {
        return Err(match reader {
            Reader::Command => format!(
                "{} left a sealed bundle and I have no household key. Set it with `atlas sync \
                 key set <phrase>` — the phrase was printed when the key was made.",
                env.from_device
            ),
            Reader::Page => format!(
                "{} left a sealed bundle and I have no household key. Put the key in on the \
                 Sync page, under \u{201c}Use a key from another device\u{201d} — it is shown on \
                 that device's own Sync page.",
                env.from_device
            ),
        });
    };
    let raw = crate::b64::decode(&env.body).map_err(|e| e.to_string())?;
    let plain = crate::vault::unseal_aead(&raw, key).map_err(|_| {
        format!(
            "{}'s bundle didn't open. Either this device has a different household key, or \
             the file has been altered.",
            env.from_device
        )
    })?;
    serde_json::from_slice::<Bundle>(&plain)
        .map_err(|e| format!("that opened but isn't a bundle: {e}"))
}

/// What a device says when it cannot make or keep a key at all.
///
/// Not the same sentence as before. Turning sealing on used to refuse and
/// send you to a terminal; Atlas makes the key itself now, and this is only
/// for the case where making or keeping one genuinely failed — a read-only
/// data folder, a full disk. It still does not fall back to plaintext: that
/// would be the switch doing the opposite of what it says.
pub const NO_KEY_YET: &str =
    "Sealing is on and I could not make a key to seal with, so I have not written the \
     bundle -- writing it in the clear is the thing you just switched off. The sync page in \
     the hub has a button to try again.";

// ---------------------------------------------------------------------------
// The key, without anything to remember
// ---------------------------------------------------------------------------
//
// The first version of this made you run `atlas sync key new`, write a phrase
// on paper, and type it on every other device. Eric's objection was exact and
// correct: he cannot promise never to lose a piece of paper, does not want to
// depend on remembering, and does not remember commands.
//
// Both halves of that are fixable, and the second one is the reason the first
// one matters less than it looks.
//
// **Losing the key does not lose anything.** A bundle is a courier, not an
// archive. `Daemon::carry_to_your_other_devices` builds one with
// `make_bundle(.., since = 0, ..)` — every bundle is the device's whole log
// from the beginning, and that log lives in `data/state` on the machine that
// wrote it. So the worst case of losing every copy of the key is: bundles
// sitting in the folder become unreadable, you make a new key, and the next
// bundle each device writes carries everything again. The receiving side
// skips what it has already taken, so nothing arrives twice either.
//
// That is what makes this safe to hand to someone who expects to lose things.
// The key protects a folder somebody else holds a copy of; it is not what
// stands between you and your notes.
//
// So: Atlas makes the key itself the moment sealing is switched on, keeps it
// on each device, writes a recovery card as a *file* you can copy anywhere,
// and the way back from "I have lost all of it" is one button.

/// Where the recovery card is written.
pub fn card_path() -> std::path::PathBuf {
    crate::roots::data_sub("recovery").join("household-key.txt")
}

/// What a fresh key left behind.
#[derive(Debug, Clone)]
pub struct KeySetup {
    pub phrase: String,
    /// Where the card was written, if it could be.
    pub card: Option<std::path::PathBuf>,
}

/// The card, in the words a person needs six months from now.
///
/// A file rather than a line on a screen, because a screen is gone when you
/// close it. Plain text rather than a PDF so that any machine, in any state,
/// can show it.
pub fn recovery_card(phrase: &str) -> String {
    format!(
        "ATLAS — HOUSEHOLD KEY\n\
         =====================\n\n\
         {phrase}\n\n\
         WHAT THIS IS\n\
         Atlas seals the bundles it leaves in your sync folder, so that a cloud\n\
         provider holds something unreadable rather than your notes. This is the\n\
         key it seals them with. Every one of your devices has a copy.\n\n\
         WHAT YOU DO WITH IT\n\
         Nothing, normally. Your devices already have it.\n\n\
         To read a bundle by hand on a machine that does not:\n\
             atlas sync read <the .bundle file> --card <this file>\n\n\
         IF YOU LOSE THIS\n\
         You lose nothing that matters. A bundle is a courier, not where your\n\
         notes live -- those are in Atlas's own data folder on each machine, and\n\
         every bundle carries the whole record from the beginning. Open the hub,\n\
         go to the sync page, and press \"Make a new key\". Your devices start\n\
         using it, the next bundle carries everything again, and the only thing\n\
         gone is whatever was still sitting unread in the folder.\n\n\
         So: keep this somewhere convenient, copy it anywhere you like, and do\n\
         not lose sleep over it.\n"
    )
}

/// Write the card, and say where it went.
pub fn write_card(phrase: &str) -> Result<std::path::PathBuf, String> {
    let path = card_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, recovery_card(phrase)).map_err(|e| e.to_string())?;
    Ok(path)
}

/// A phrase read back out of a card file.
///
/// The card is mostly prose; the phrase is the one line that looks like a
/// phrase. Found rather than parsed by position, so a card someone has added
/// a note to still works.
pub fn phrase_in_card(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| {
            let tidy = crate::vault::tidy_recovery_key(l);
            tidy.len() == 24 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        .map(|l| l.to_string())
}

/// Keep a household key typed in from another device: the phrase that device
/// showed, checked before it is kept. Shared by `atlas sync key set` and the
/// hub's Sync page (27 Sep 2026).
///
/// A mistyped phrase stored happily is a device that looks set up and opens
/// nothing, so it is refused here. And a device that already has a key keeps
/// it unless `replace` says otherwise: the new phrase would make everything
/// sealed under the old one unreadable here.
pub fn set_key(store: &crate::store::Store, phrase: &str, replace: bool, now: u64) -> Result<String, String> {
    let typed = phrase.trim();
    if typed.is_empty() {
        return Err("Nothing typed — the key is unchanged.".into());
    }
    let kept: KeptKey = store.load(KEY_FILE);
    if kept.is_set() && !replace {
        if kept.phrase().ok().map(|p| tidy_same(&p, typed)).unwrap_or(false) {
            return Ok("That's the key this device already has.".into());
        }
        return Err(
            "This device already has a household key. Using another one makes every bundle \
             sealed under the old one unreadable here — tick \u{201c}replace my key\u{201d} if that \
             is what you mean."
                .into(),
        );
    }
    key_from_phrase(typed)?;
    let keeping = KeptKey::keeping(typed, now);
    store.save(KEY_FILE, &keeping).map_err(|e| format!("I couldn't keep that: {e}"))?;
    Ok(format!(
        "Household key set on this device. {} If it is the same phrase as your other machine, \
         sealed bundles from it will open here. If it isn't, they won't, and I'll say so rather \
         than failing quietly.",
        keeping.at_rest_says()
    ))
}

fn tidy_same(a: &str, b: &str) -> bool {
    crate::vault::tidy_recovery_key(a) == crate::vault::tidy_recovery_key(b)
}

/// Make a key, keep it, and write the card. Replaces any key already here.
pub fn new_key(store: &crate::store::Store, now: u64) -> Result<KeySetup, String> {
    let phrase = new_key_phrase();
    store
        .save(KEY_FILE, &KeptKey::keeping(&phrase, now))
        .map_err(|e| format!("I made a key and couldn't keep it: {e}"))?;
    let card = write_card(&phrase).ok();
    Ok(KeySetup { phrase, card })
}

/// The key this device uses, making one if it has none.
///
/// Returns the key and, when it had to make one, what it left behind — so the
/// caller can say so out loud rather than a key appearing silently.
pub fn ensure_key(
    store: &crate::store::Store,
    now: u64,
) -> Result<(Vec<u8>, Option<KeySetup>), String> {
    let kept: KeptKey = store.load(KEY_FILE);
    if kept.is_set() {
        let phrase = kept.phrase()?;
        return Ok((key_from_phrase(&phrase)?, None));
    }
    let made = new_key(store, now)?;
    let key = key_from_phrase(&made.phrase)?;
    Ok((key, Some(made)))
}

/// What Atlas says the first time it makes one for you.
pub fn made_one(setup: &KeySetup) -> String {
    match &setup.card {
        Some(path) => format!(
            " I made the key for it and wrote it down for you in {} — you don't need to do \
             anything with that file, it's there for the day something goes wrong.",
            path.display()
        ),
        None => format!(
            " I made the key for it. I couldn't write the card file, so here it is once: {}. \
             Losing it costs you nothing durable -- the sync page has a button that makes a \
             new one.",
            setup.phrase
        ),
    }
}

// ---------------------------------------------------------------------------
// Handing the key to a new device
// ---------------------------------------------------------------------------
//
// The remaining friction after the hub switch: a second machine still had to
// be given the key, which meant copying a file or typing a phrase. Neither is
// something to ask of somebody who does not want to handle keys at all.
//
// It rides the pairing that already happens. `atlas household pair` mints a
// code with `server::new_token` -- OS entropy, twenty-four characters, not a
// six-digit PIN -- which is shown on one device and typed on the other. That
// is a shared secret, used once and expiring in three minutes, and it is
// enough to carry the household key across the folder the two devices
// already share.
//
// What is in the folder while that window is open is the key sealed under a
// key derived from the pairing code by Argon2id at 64MiB. Somebody who copies
// the file has to guess the code to use it, and it is deleted the moment it
// is taken or the window closes. The honest description of the trade: for the
// three minutes between "pair" and "join", a blob exists that a cloud
// provider could keep. It is worth that to make the alternative -- a person
// copying key files between machines by hand, or not turning sealing on at
// all -- unnecessary.

pub const HANDOFF_CONTEXT: &[u8] = b"atlas-sync-key-handoff-v1";

/// The key, on its way to a device that has just been paired.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyHandoff {
    pub atlas_key_handoff: u32,
    /// Which household it is for, so a folder two households share never
    /// offers one the other's.
    ///
    /// Named `for_household` on purpose, and the prose here avoids writing
    /// the other name out with a dot in front of it.
    ///
    /// The deadness scans count a field read by name alone, anywhere in the
    /// tree, so a field named after the one on `HouseholdConfig` would have
    /// quietly taken a dead setting off the list without wiring anything. One
    /// of those scans reads comments as well as code, so the first version of
    /// this note re-created the problem while explaining it -- which is the
    /// third time a guard in this tree has been broken by a comment.
    pub for_household: String,
    pub made_at: u64,
    pub valid_secs: u64,
    /// Base64 of the sealed phrase.
    pub body: String,
}

/// Write a file another device reads -- a bundle, a key handoff, an
/// invitation -- whole or not at all (28 Sep 2026).
///
/// These were `fs::write` straight onto the name the other side looks for,
/// in a folder a cloud client is watching and another Atlas is reading. That
/// truncates the old file first and fills it in after, so for the length of
/// the write the file under that name is half of one: the other device's sync
/// pass reads it, fails to open it, and reports the bundle as unreadable (or
/// a pairing code as wrong), and a cloud client can pick it up half-written
/// and carry that across. Written beside it under a name nothing reads
/// (`.writing`, which no reader's extension matches) and renamed into place,
/// the name only ever holds a whole file.
pub fn write_whole(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".writing");
    let tmp = path.with_file_name(name);
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

pub const HANDOFF_VERSION: u32 = 1;

fn handoff_key(code: &str, household: &str) -> Result<Vec<u8>, String> {
    // The household id is mixed in so the same code in two households cannot
    // produce the same wrapping key.
    crate::vault::key_from_words(&format!("{}|{}", code.trim(), household.trim()), HANDOFF_CONTEXT)
}

fn handoff_path(folder: &std::path::Path, household: &str) -> std::path::PathBuf {
    let tidy: String = household
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    folder.join(format!("{tidy}.keyhandoff"))
}

/// Leave the key for the device being paired.
pub fn leave_handoff(
    folder: &std::path::Path,
    household: &str,
    code: &str,
    phrase: &str,
    now: u64,
    valid_secs: u64,
) -> Result<std::path::PathBuf, String> {
    let key = handoff_key(code, household)?;
    let sealed = crate::vault::seal_aead(phrase.as_bytes(), &key)?;
    let env = KeyHandoff {
        atlas_key_handoff: HANDOFF_VERSION,
        for_household: household.to_string(),
        made_at: now,
        valid_secs,
        body: crate::b64::encode(&sealed),
    };
    let path = handoff_path(folder, household);
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    write_whole(&path, serde_json::to_string_pretty(&env).map_err(|e| e.to_string())?.as_bytes())?;
    Ok(path)
}

/// Take it, on the device that just joined.
///
/// Deletes the file whether or not it opened: a handoff is single-use by
/// design, and one left lying about is the part of this with a cost.
pub fn take_handoff(
    folder: &std::path::Path,
    household: &str,
    code: &str,
    now: u64,
) -> Result<String, String> {
    let path = handoff_path(folder, household);
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| "there's no key waiting in that folder".to_string())?;
    let env: KeyHandoff =
        serde_json::from_str(&raw).map_err(|_| "that isn't a key handoff".to_string())?;
    if env.atlas_key_handoff > HANDOFF_VERSION {
        return Err("that handoff came from a newer Atlas".into());
    }
    if env.for_household != household {
        return Err("that key is for a different household".into());
    }
    if now.saturating_sub(env.made_at) > env.valid_secs {
        let _ = std::fs::remove_file(&path);
        return Err(
            "the key waiting in that folder has expired -- ask for a fresh pairing code".into()
        );
    }
    let key = handoff_key(code, household)?;
    let sealed = crate::b64::decode(&env.body).map_err(|e| e.to_string())?;
    let opened = crate::vault::unseal_aead(&sealed, &key)
        .map_err(|_| "that pairing code doesn't open the key waiting in the folder".to_string())?;
    let phrase =
        String::from_utf8(opened).map_err(|_| "the key that came across isn't readable".to_string())?;
    let _ = std::fs::remove_file(&path);
    Ok(phrase)
}

/// Clear away any handoff whose window has closed.
///
/// Called on every sync pass. The window is three minutes and the file is
/// deleted when it is taken, so this is for the pairing that was started and
/// never finished -- which is exactly the one nobody would remember to clean
/// up.
pub fn sweep_handoffs(folder: &std::path::Path, now: u64) -> usize {
    let Ok(entries) = std::fs::read_dir(folder) else { return 0 };
    let mut gone = 0;
    for p in entries.flatten().map(|e| e.path()) {
        if p.extension().and_then(|x| x.to_str()) != Some("keyhandoff") {
            continue;
        }
        let stale = std::fs::read_to_string(&p)
            .ok()
            .and_then(|raw| serde_json::from_str::<KeyHandoff>(&raw).ok())
            .map(|env| now.saturating_sub(env.made_at) > env.valid_secs)
            // Unreadable and sitting in a shared folder: not ours to keep.
            .unwrap_or(true);
        if stale && std::fs::remove_file(&p).is_ok() {
            gone += 1;
        }
    }
    gone
}

/// The best place to carry bundles through when none is set (H13i: "the best
/// route available"): a cloud folder this machine already syncs, so two
/// devices that are never on together still meet. `None` when there's none.
pub fn best_folder() -> Option<(std::path::PathBuf, Carry)> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok().map(std::path::PathBuf::from);
    let mut tries: Vec<std::path::PathBuf> = Vec::new();
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Ok(p) = std::env::var(var) {
            tries.push(std::path::PathBuf::from(p));
        }
    }
    if let Some(h) = &home {
        tries.push(h.join("Dropbox"));
        tries.push(h.join("Google Drive"));
        tries.push(h.join("iCloudDrive"));
    }
    tries.push(std::path::PathBuf::from("G:\\My Drive"));
    tries.into_iter().find(|p| p.is_dir()).map(|p| (p.join("Atlas sync"), Carry::CloudFolder))
}

/// How a folder carries things: through a cloud service, or on a drive you
/// plug in. Said, so you know whether both machines need to be on.
pub fn route_of(folder: &std::path::Path) -> (Carry, &'static str) {
    let f = folder.display().to_string().to_lowercase();
    let cloud = ["onedrive", "dropbox", "google drive", "my drive", "icloud"].iter().any(|c| f.contains(c));
    // A folder on a drive other than the system one is likely removable.
    let plugged = !cloud && f.len() > 2 && f.as_bytes()[1] == b':' && !f.starts_with("c:");
    how_to_carry(false, plugged, cloud, false)
}
