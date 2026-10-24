//! The correction log (23 Oct 2026): what Atlas got wrong, and what was right,
//! kept on this laptop for the nightly training job to read.
//!
//! One JSON object per line in `data/learning/corrections.jsonl` (schema v1,
//! `docs/LEARNING_LOG_FORMAT_2026-10-23.md`). A line is written when
//!
//! - you corrected Atlas ("no, I meant ...", "that's wrong"): `correction`;
//! - you undid what it had just done: `undo`;
//! - you said yes or no to a question it asked before acting: `approval` and
//!   `rejection`;
//! - it asked a specific question because it wasn't sure, and your answer
//!   settled it: `clarified` -- the labelled example.
//!
//! ## Where it stays
//!
//! `data/learning/` is beside `data/state/`, not inside it, and is not one of
//! the folders a backup keeps (`safety::OWNED_FOLDERS`), so it is never in a
//! backup, a restore, or what anyone else's Atlas is sent. Nothing here makes
//! a request. `tests/the_correction_log.rs` reads the backup and sync sources
//! for it.
//!
//! ## Your own devices (24 Oct 2026)
//!
//! Eric: sync the correction log "between Eric's own paired devices. Keep it
//! encrypted/limited to his own devices, still honour 'forget' and
//! secret-scrubbing, and never send it to anyone else's peer." So a line, once
//! written (already redacted, as above), travels in your own sync log as a
//! `Changed` event under [`SYNC_PREFIX`] -- only when your sync bundles are
//! sealed with your household key -- and a forget travels the same way. A
//! device takes them only from a sealed bundle, keeps nothing while you've
//! said "stop learning from me" there, and refuses a line that still looks
//! like it carries a secret. The door to other people's Atlas (`kin`) never
//! carries the sync log at all.
//!
//! ## What it never holds
//!
//! Every text field goes through `redact` (keys, tokens, card, IBAN and SSN
//! shapes, emails and phone numbers) and through `mask_secret_words` (what
//! follows "password", "pin", "passcode", "verification code" ...). A turn
//! that handled a secret -- sign-in, a two-factor code, account creation, the
//! vault, a pairing -- writes nothing at all, nor does one whose own words
//! look like a password being said. Nothing is written while a masked read
//! (`typed::ask_quietly`) is on screen. When a turn rested on something
//! someone else wrote (a mail, a page: SEC2) only your own words are kept.
//!
//! ## Bounds
//!
//! At most 5 MB and 20,000 lines: over either, the oldest lines go (an atomic
//! rewrite, down to 90% so it is not redone on every line). One line is at
//! most 4 KB: fields are cut until it fits. Appending is one small write; a
//! write that fails is logged once and never shown as a failed turn.
//!
//! ## Forgetting, and stopping
//!
//! "Forget that" drops this chain's lines, "forget what I said about X" the
//! lines that mention X, "forget everything" empties the file; "stop learning
//! from me" writes nothing from then on until "start learning again".

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;
/// The file is trimmed from the front past either bound.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_LINES: usize = 20_000;
/// One line, newline not counted.
pub const MAX_LINE_BYTES: usize = 4096;
const REPLY_CHARS: usize = 300;
const DETAIL_CHARS: usize = 80;
const SAID_CHARS: usize = 400;
/// What the file is trimmed to when it is over: room to grow.
const TRIM_TO_PERCENT: usize = 90;
/// Lines written this long after the last one are a new chain, for "forget
/// that".
const CHAIN_GAP_SECS: u64 = 180;
/// The saved record behind "stop learning from me".
pub const SWITCH: &str = "learning_log";
/// Sync ids for the correction log: `corr:<record id>`, or `corr:*` for
/// "forget everything".
pub const SYNC_PREFIX: &str = "corr:";
/// The field a synced line arrives in, and the one a forget does.
pub const SYNC_LINE: &str = "line";
pub const SYNC_FORGOTTEN: &str = "forgotten";

/// Why the line was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Correction,
    Undo,
    Approval,
    Rejection,
    Clarified,
}

/// Where the `right` answer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum From {
    OwnerSaid,
    OwnerAnswer,
    Undo,
    Approval,
    Rejection,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Prev {
    pub said: String,
    pub reply: String,
    pub intent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Context {
    /// The foreground program's file name only: never a window title.
    pub app: String,
    /// The last (up to) three commands Atlas ran.
    pub recent: Vec<String>,
    /// How the turn was read: `phrase`, `floor`, `fast`, `model`, `ask`, `none`.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Did {
    pub intent: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Right {
    pub intent: Option<String>,
    pub detail: Option<String>,
    pub from: From,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ask {
    pub question: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub v: u32,
    pub id: String,
    pub ts: u64,
    pub kind: Kind,
    pub said: String,
    pub prev: Prev,
    pub context: Context,
    pub did: Did,
    pub right: Right,
    pub ask: Option<Ask>,
    /// How many spans were replaced in the text fields.
    pub redacted: u32,
}

/// Everything a record is made from, as the turn had it, before any
/// redaction or cutting. `build` decides what of it may be kept.
#[derive(Debug, Clone)]
pub struct Facts {
    pub kind: Kind,
    pub ts: u64,
    pub said: String,
    pub prev: Prev,
    pub context: Context,
    pub did_intent: Option<String>,
    pub did_detail: Option<String>,
    pub right_intent: Option<String>,
    pub right_detail: Option<String>,
    pub from: From,
    pub ask: Option<(String, Vec<String>)>,
    /// The turn rested on something someone else wrote (SEC2).
    pub outside: bool,
}

impl Facts {
    /// Does any of the words in these facts contain `needle` (lower case)?
    pub fn mentions(&self, needle: &str) -> bool {
        let has = |s: &str| s.to_lowercase().contains(needle);
        let opt = |s: &Option<String>| s.as_deref().is_some_and(has);
        has(&self.said)
            || has(&self.prev.said)
            || has(&self.prev.reply)
            || opt(&self.did_detail)
            || opt(&self.right_detail)
            || self
                .ask
                .as_ref()
                .is_some_and(|(q, o)| has(q) || o.iter().any(|x| has(x)))
    }
}

/// Commands whose turn is a secret's: signing in, a two-factor code, a new
/// account, the vault passphrase and keys, pairing and handing over.
fn is_secret_command(name: &str) -> bool {
    matches!(
        name,
        "sign_in"
            | "create_account"
            | "type_code"
            | "two_factor"
            | "set_key"
            | "unlock"
            | "pair"
            | "accept_pairing"
            | "hand_over"
            | "take_it_back"
    ) || ["vault", "signing", "passphrase", "credential", "link_account", "sign_up"]
        .iter()
        .any(|w| name.contains(w))
}

fn clip(s: &str, chars: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Words after which what follows is a secret.
const SECRET_WORDS: &[&str] = &[
    "password",
    "passwords",
    "passwd",
    "passcode",
    "passphrase",
    "pin",
    "otp",
    "secret",
    "token",
    "pwd",
    "2fa",
    "mfa",
];
/// Words that are a secret only when a value follows: "the code is 4829",
/// "my key is abc", not "open the code editor".
const SECRET_IF_VALUE: &[&str] = &["code", "codes", "key", "keys"];
const NOT_A_VALUE: &[&str] = &[
    "manager", "reset", "field", "page", "box", "for", "on", "in", "vault", "screen", "prompt",
    "entry", "change", "to", "and", "or", "with", "from", "again", "protected", "hint", "editor",
    "board", "chain", "frame", "light", "lock",
];
const LINKING: &[&str] = &["is", "was", "are", "=", ":", "its", "it's", "equals", "be"];
const SPOKEN_DIGITS: &[&str] = &[
    "zero", "oh", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];

fn bare(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
        .to_lowercase()
}

fn has_digit(w: &str) -> bool {
    w.chars().any(|c| c.is_ascii_digit()) || SPOKEN_DIGITS.contains(&bare(w).as_str())
}

/// Does this word end a clause? A full stop or semicolon on it.
fn ends_clause(w: &str) -> bool {
    w.ends_with('.') || w.ends_with(';') || w.ends_with('!') || w.ends_with('?')
}

/// `text` with everything after a word like "password", "pin", "code" or
/// "2fa", up to the end of that clause (a full stop, a semicolon or "and
/// then"), replaced; and how many such stretches there were. Redact's
/// scanners cannot catch "my password for gmail is hunter2" or a code said
/// as "4 8 2 9": they have the shape of any words. "code" and "key" count
/// only when a value (a digit, or "is ...") follows.
pub fn mask_secret_words(text: &str) -> (String, u32) {
    let words: Vec<&str> = text.split(' ').collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut n = 0u32;
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        // "password:hunter2", "pin=1234"
        if let Some((left, right)) = w.split_once([':', '=']) {
            let key = bare(left);
            if !right.is_empty()
                && (SECRET_WORDS.contains(&key.as_str()) || SECRET_IF_VALUE.contains(&key.as_str()))
            {
                out.push(format!("{left}:⟦secret⟧"));
                n += 1;
                i += 1;
                continue;
            }
        }
        out.push(w.to_string());
        let key = bare(w);
        let always = SECRET_WORDS.contains(&key.as_str());
        let with_value = SECRET_IF_VALUE.contains(&key.as_str());
        if !(always || with_value) || ends_clause(w) {
            i += 1;
            continue;
        }
        // The rest of the clause.
        let mut end = i + 1;
        while end < words.len() {
            let cur = words[end];
            let next_is_then = bare(cur) == "and" && words.get(end + 1).is_some_and(|x| bare(x) == "then");
            if next_is_then {
                break;
            }
            end += 1;
            if ends_clause(cur) {
                break;
            }
        }
        let rest = &words[i + 1..end];
        let linked = rest.first().is_some_and(|f| LINKING.contains(&bare(f).as_str()));
        let digits = rest.iter().any(|r| has_digit(r));
        let first = rest.first().map(|f| bare(f)).unwrap_or_default();
        // "my password manager": nothing said to hide, unless a value
        // comes later ("the password for gmail is hunter2").
        let masked = !rest.is_empty()
            && if with_value {
                digits || linked
            } else {
                linked || digits || !NOT_A_VALUE.contains(&first.as_str())
            };
        if masked {
            out.push("⟦secret⟧".to_string());
            n += 1;
            i = end;
        } else {
            i += 1;
        }
    }
    (out.join(" "), n)
}

/// Redacted and cut: the text, and how many spans were replaced.
fn redact_text(text: &str, chars: usize) -> (String, u32) {
    let (masked, a) = mask_secret_words(text);
    let mut s = crate::redact::Scrubber::default();
    let scrubbed = s.scrub(&masked);
    let b = s.found_count() as u32;
    (clip(&scrubbed, chars), a + b)
}

/// Words that mean a secret is being talked about, wherever they stand
/// ("Hunter2 is the password" has the value first).
const STRONG_WORDS: &[&str] = &[
    "password", "passwords", "passwd", "passcode", "passphrase", "pin", "otp", "2fa", "token",
    "tokens", "secret", "secrets", "pwd",
];
/// Two-word forms: a "code" or "key" that is a secret by what is said of it.
const STRONG_PAIRS: &[(&str, &str)] = &[
    ("verification", "code"),
    ("security", "code"),
    ("recovery", "code"),
    ("two-factor", "code"),
    ("two", "factor"),
    ("one-time", "code"),
    ("onetime", "code"),
    ("api", "key"),
    ("seed", "phrase"),
    ("secret", "key"),
    ("private", "key"),
];

/// Does any word say a strong secret word, anywhere in the text?
fn has_a_strong_secret_word(text: &str) -> bool {
    let words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    words.iter().any(|w| STRONG_WORDS.contains(&w.as_str()))
        || words
            .windows(2)
            .any(|p| STRONG_PAIRS.iter().any(|(a, b)| p[0] == *a && p[1] == *b))
}

/// Do these words carry a secret? A strong secret word anywhere, or a
/// value after a weak one ("the code is 4829").
fn says_a_secret(text: &str) -> bool {
    has_a_strong_secret_word(text) || mask_secret_words(text).1 > 0
}

fn random_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ (std::process::id() as u64).rotate_left(32) ^ N.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    // splitmix64, twice: 16 hex characters.
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    format!("{z:016x}")
}

fn opt(s: &Option<String>, chars: usize, n: &mut u32) -> Option<String> {
    s.as_ref().map(|s| {
        let (t, k) = redact_text(s, chars);
        *n += k;
        t
    })
}

/// The record these facts make, or `None` when the turn must leave nothing:
/// it handled a secret, or its own words said one, or a masked read is open.
pub fn build(f: &Facts) -> Option<Record> {
    if crate::typed::masked_read_active() {
        return None;
    }
    let names = [
        f.prev.intent.as_deref(),
        f.did_intent.as_deref(),
        f.right_intent.as_deref(),
    ];
    if names.iter().flatten().any(|n| is_secret_command(n))
        || f.context.recent.iter().any(|n| is_secret_command(n))
    {
        return None;
    }
    // A secret in any of the words that would be kept leaves nothing: the
    // line is not edited, it is not written.
    let mut kept: Vec<&str> = vec![&f.said, &f.prev.said];
    if !f.outside {
        kept.push(&f.prev.reply);
        kept.extend(f.did_detail.as_deref());
        kept.extend(f.right_detail.as_deref());
        if let Some((q, opts)) = &f.ask {
            kept.push(q);
            kept.extend(opts.iter().map(|o| o.as_str()));
        }
    }
    if kept.iter().any(|t| says_a_secret(t)) {
        return None;
    }
    let mut n = 0u32;
    let mut text = |s: &str, chars: usize| {
        let (t, k) = redact_text(s, chars);
        n += k;
        t
    };
    let said = text(&f.said, SAID_CHARS);
    let prev_said = text(&f.prev.said, SAID_CHARS);
    // Someone else's words (a mail, a page) can be in what Atlas said back,
    // in what it did to what, and in what it asked: only yours are kept.
    let prev_reply = if f.outside { String::new() } else { text(&f.prev.reply, REPLY_CHARS) };
    let ask = match (&f.ask, f.outside) {
        (Some((q, opts)), false) => Some(Ask {
            question: text(q, REPLY_CHARS),
            options: opts.iter().take(6).map(|o| text(o, DETAIL_CHARS)).collect(),
        }),
        _ => None,
    };
    let app = text(&f.context.app, 60);
    let mut m = 0u32;
    let (did_detail, right_detail) = if f.outside {
        (None, None)
    } else {
        (
            opt(&f.did_detail, DETAIL_CHARS, &mut m),
            opt(&f.right_detail, DETAIL_CHARS, &mut m),
        )
    };
    let rec = Record {
        v: SCHEMA,
        id: random_id(),
        ts: f.ts,
        kind: f.kind,
        said,
        prev: Prev {
            said: prev_said,
            reply: prev_reply,
            intent: f.prev.intent.clone(),
        },
        context: Context {
            app,
            recent: f.context.recent.iter().rev().take(3).rev().cloned().collect(),
            source: f.context.source.clone(),
        },
        did: Did { intent: f.did_intent.clone(), detail: did_detail },
        right: Right { intent: f.right_intent.clone(), detail: right_detail, from: f.from },
        ask,
        redacted: n + m,
    };
    Some(fit(rec))
}

/// The record cut until its line is within `MAX_LINE_BYTES`.
fn fit(mut r: Record) -> Record {
    for chars in [usize::MAX, 200, 100, 50, 20] {
        if serde_json::to_string(&r).map(|s| s.len()).unwrap_or(0) <= MAX_LINE_BYTES {
            return r;
        }
        let cut = |s: &mut String| *s = clip(s, chars);
        cut(&mut r.said);
        cut(&mut r.prev.said);
        cut(&mut r.prev.reply);
        for d in [&mut r.did.detail, &mut r.right.detail].into_iter().flatten() {
            cut(d);
        }
        if let Some(a) = r.ask.as_mut() {
            cut(&mut a.question);
            a.options.iter_mut().for_each(cut);
            if chars <= 50 {
                a.options.truncate(2);
            }
        }
    }
    r
}

/// The file and what is known of its length.
#[derive(Debug)]
pub struct Log {
    path: PathBuf,
    lines: Option<usize>,
    /// The file's length as this process last left it: a different length
    /// means another process wrote, and the lines are counted again.
    bytes_seen: u64,
}

/// A lock file beside the log, held across an append or a rewrite so two
/// Atlas processes cannot lose each other's lines. Taken with an exclusive
/// create; one older than `STALE_LOCK_SECS` is a crashed holder's.
struct LockGuard(PathBuf);

const STALE_LOCK_SECS: u64 = 30;
const LOCK_WAIT_MS: u64 = 5_000;

impl Drop for LockGuard {
    fn drop(&mut self) {
        crate::heard!(std::fs::remove_file(&self.0));
    }
}

impl Log {
    /// `data/learning/corrections.jsonl` under `data_dir`.
    pub fn at(data_dir: &Path) -> Log {
        Log {
            path: data_dir.join("learning").join("corrections.jsonl"),
            lines: None,
            bytes_seen: 0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> std::io::Result<LockGuard> {
        let lock = self.path.with_extension("jsonl.lock");
        if let Some(dir) = lock.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let started = std::time::Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
            {
                Ok(_) => return Ok(LockGuard(lock)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let old = std::fs::metadata(&lock)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age.as_secs() >= STALE_LOCK_SECS);
                    if old {
                        crate::heard!(std::fs::remove_file(&lock));
                    } else if started.elapsed().as_millis() as u64 > LOCK_WAIT_MS {
                        return Err(std::io::Error::other("the correction log is busy"));
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
                // Windows reports a file being deleted by its holder as a
                // permission error: wait and try again like any other busy.
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    if started.elapsed().as_millis() as u64 > LOCK_WAIT_MS {
                        return Err(e);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn raw_lines(&self) -> Vec<String> {
        std::fs::read_to_string(&self.path)
            .map(|t| {
                t.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every record that reads; a damaged line is skipped, not an error.
    pub fn read(&self) -> Vec<Record> {
        self.raw_lines()
            .iter()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// One line on the end, then trimmed from the front if over a bound.
    pub fn append(&mut self, r: &Record) -> std::io::Result<()> {
        use std::io::Write;
        let mut line = serde_json::to_string(r).map_err(std::io::Error::other)?;
        if line.len() > MAX_LINE_BYTES {
            return Err(std::io::Error::other(
                "a line over the size cap was not written",
            ));
        }
        line.push('\n');
        let _lock = self.lock()?;
        let on_disk = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if self.lines.is_none() || on_disk != self.bytes_seen {
            self.lines = Some(self.raw_lines().len());
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(line.as_bytes())?;
        let n = self.lines.map(|n| n + 1).unwrap_or(1);
        self.lines = Some(n);
        let size = f.metadata().map(|m| m.len()).unwrap_or(0);
        drop(f);
        self.bytes_seen = size;
        if n > MAX_LINES || size > MAX_BYTES {
            self.trim_held()?;
        }
        Ok(())
    }

    /// The oldest lines dropped until within 90% of both bounds.
    pub fn trim(&mut self) -> std::io::Result<()> {
        let _lock = self.lock()?;
        self.trim_held()
    }

    fn trim_held(&mut self) -> std::io::Result<()> {
        let mut lines = self.raw_lines();
        let max_lines = MAX_LINES * TRIM_TO_PERCENT / 100;
        let max_bytes = (MAX_BYTES as usize) * TRIM_TO_PERCENT / 100;
        let mut bytes: usize = lines.iter().map(|l| l.len() + 1).sum();
        let mut drop_n = lines.len().saturating_sub(max_lines);
        for l in lines.iter().take(drop_n) {
            bytes -= l.len() + 1;
        }
        while bytes > max_bytes && drop_n < lines.len() {
            bytes -= lines[drop_n].len() + 1;
            drop_n += 1;
        }
        lines.drain(0..drop_n);
        self.rewrite(&lines)
    }

    /// The whole file replaced atomically. The lock is held by the caller.
    fn rewrite(&mut self, lines: &[String]) -> std::io::Result<()> {
        let mut body = lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        crate::store::write_whole(&self.path, body.as_bytes())?;
        self.lines = Some(lines.len());
        self.bytes_seen = body.len() as u64;
        Ok(())
    }

    /// Lines for which `drop` is true are removed. How many.
    fn remove_where(
        &mut self,
        drop: impl Fn(&serde_json::Value) -> bool,
    ) -> std::io::Result<usize> {
        let _lock = self.lock()?;
        let lines = self.raw_lines();
        let kept: Vec<String> = lines
            .iter()
            .filter(|l| {
                !serde_json::from_str::<serde_json::Value>(l)
                    .map(|v| drop(&v))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        let gone = lines.len() - kept.len();
        if gone > 0 {
            self.rewrite(&kept)?;
        }
        Ok(gone)
    }

    fn forget_ids(&mut self, ids: &[String]) -> std::io::Result<usize> {
        self.remove_where(|v| v["id"].as_str().is_some_and(|i| ids.iter().any(|x| x == i)))
    }

    /// The newest record only.
    fn forget_newest(&mut self) -> std::io::Result<usize> {
        let _lock = self.lock()?;
        let mut lines = self.raw_lines();
        if lines.pop().is_none() {
            return Ok(0);
        }
        self.rewrite(&lines)?;
        Ok(1)
    }

    /// Every record with `needle` in any of its text, any case.
    pub fn forget_about(&mut self, needle: &str) -> std::io::Result<usize> {
        let n = needle.trim().to_lowercase();
        if n.is_empty() {
            return Ok(0);
        }
        self.remove_where(|v| mentions(v, &n))
    }

    /// The file emptied. How many lines it held.
    pub fn clear(&mut self) -> std::io::Result<usize> {
        let _lock = self.lock()?;
        let n = self.raw_lines().len();
        if self.path.exists() {
            self.rewrite(&[])?;
        }
        Ok(n)
    }
}

fn mentions(v: &serde_json::Value, needle: &str) -> bool {
    match v {
        serde_json::Value::String(s) => s.to_lowercase().contains(needle),
        serde_json::Value::Array(a) => a.iter().any(|x| mentions(x, needle)),
        serde_json::Value::Object(m) => m.values().any(|x| mentions(x, needle)),
        _ => false,
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Switch {
    on: bool,
}
impl Default for Switch {
    fn default() -> Self {
        Switch { on: true }
    }
}

/// The log as the daemon holds it: the file, the switch, the chain of lines
/// "forget that" takes back, and a correction waiting for what was right.
#[derive(Debug)]
pub struct Keeper {
    pub log: Log,
    pub on: bool,
    chain: Vec<String>,
    chain_at: u64,
    /// "That's wrong", held until the next turn says what was right.
    pub held: Option<Facts>,
    /// Atlas's last specific question and what it offered: set where it asks
    /// because it is unsure, read when the next turn answers.
    pub unsure: std::cell::RefCell<Option<(String, Vec<String>)>>,
    warned: bool,
}

impl Keeper {
    pub fn open(store: &crate::store::Store) -> Keeper {
        // A second person's profile is not the owner's: nothing of theirs is
        // kept here.
        let guest = store
            .root()
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "profiles");
        let sw: Switch = store.load(SWITCH);
        Keeper {
            log: Log::at(&store.data_dir()),
            on: sw.on && !guest,
            chain: Vec::new(),
            chain_at: 0,
            held: None,
            unsure: std::cell::RefCell::new(None),
            warned: false,
        }
    }

    /// Turned on or off, and kept so a restart doesn't undo it.
    pub fn set_on(&mut self, store: &crate::store::Store, on: bool) -> std::io::Result<()> {
        self.on = on;
        if !on {
            self.held = None;
        }
        store.save(SWITCH, &Switch { on }).map_err(std::io::Error::other)
    }

    /// Write `rec`. `Some(why)` the first time a write fails: the caller logs
    /// it; the turn never sees it.
    pub fn write(&mut self, rec: &Record) -> Option<String> {
        if !self.on || crate::typed::masked_read_active() {
            return None;
        }
        match self.log.append(rec) {
            Ok(()) => {
                if rec.ts.saturating_sub(self.chain_at) > CHAIN_GAP_SECS {
                    self.chain.clear();
                }
                self.chain_at = rec.ts;
                self.chain.push(rec.id.clone());
                None
            }
            Err(e) if !self.warned => {
                self.warned = true;
                Some(format!("couldn't write the correction log: {e}"))
            }
            Err(_) => None,
        }
    }

    /// The ids of every line kept, oldest first: what a forget is measured
    /// against, so it can be told to your other devices.
    pub(crate) fn ids(&self) -> Vec<String> {
        self.log.read().into_iter().map(|r| r.id).collect()
    }

    /// A line from your other device. `Ok(true)` when it was kept; `Ok(false)`
    /// when there was nothing to do (already here, learning stopped here, or
    /// not a record). Refused outright if it still carries what the scrubber
    /// would take out: that line never reaches this device's file.
    pub(crate) fn take_synced(&mut self, line: &str) -> std::io::Result<bool> {
        if !self.on {
            return Ok(false);
        }
        let Ok(rec) = serde_json::from_str::<Record>(line) else {
            return Ok(false);
        };
        if rec.v != SCHEMA || line.len() > MAX_LINE_BYTES {
            return Ok(false);
        }
        let mut s = crate::redact::Scrubber::default();
        s.scrub(line);
        if s.found_count() > 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "a synced correction still looked like it held a secret; not kept",
            ));
        }
        if self.ids().iter().any(|i| *i == rec.id) {
            return Ok(false);
        }
        self.log.append(&rec)?;
        Ok(true)
    }

    /// A forget from your other device: one line, or `*` for everything.
    pub(crate) fn forget_synced(&mut self, id: &str) -> std::io::Result<usize> {
        if id == "*" {
            self.log.clear()
        } else {
            self.log.forget_ids(&[id.to_string()])
        }
    }

    /// "Forget that": this chain's lines, or the newest one when the chain is
    /// gone (a restart). How many went.
    pub fn forget_that(&mut self) -> std::io::Result<usize> {
        self.held = None;
        // The chain is let go only once the rewrite has worked.
        let ids = self.chain.clone();
        let n = if ids.is_empty() {
            self.log.forget_newest()?
        } else {
            self.log.forget_ids(&ids)?
        };
        self.chain.clear();
        Ok(n)
    }

    pub fn forget_about(&mut self, needle: &str) -> std::io::Result<usize> {
        if self
            .held
            .as_ref()
            .is_some_and(|h| h.mentions(&needle.trim().to_lowercase()))
        {
            self.held = None;
        }
        let n = self.log.forget_about(needle)?;
        let live: Vec<String> = self.log.read().into_iter().map(|r| r.id).collect();
        self.chain.retain(|c| live.contains(c));
        Ok(n)
    }

    pub fn forget_everything(&mut self) -> std::io::Result<usize> {
        self.held = None;
        self.chain.clear();
        self.log.clear()
    }
}

/// What was asked of the log by voice or typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    Last,
    About(String),
    Everything,
    Stop,
    Start,
}

/// "Forget that", "forget what I said about Sam", "forget everything",
/// "stop learning from me", "start learning again". Narrow shapes only.
pub fn asked_of_the_log(said: &str) -> Option<Asked> {
    let t = crate::intent::normalize(said);
    let t = crate::intent::without_fillers(&t);
    let t = t.as_str();
    if matches!(
        t,
        "forget that"
            | "forget the last thing"
            | "forget the last thing i said"
            | "forget what i just said"
            | "forget that i said that"
            | "forget my last correction"
            | "dont keep that"
    ) {
        return Some(Asked::Last);
    }
    for lead in [
        "forget what i said about ",
        "forget everything i said about ",
        "forget anything i said about ",
        "forget what i told you about ",
    ] {
        if let Some(rest) = t.strip_prefix(lead) {
            if !rest.trim().is_empty() {
                return Some(Asked::About(rest.trim().to_string()));
            }
        }
    }
    if matches!(
        t,
        "forget everything"
            | "forget everything you learned"
            | "forget everything youve learned"
            | "forget everything youve learned about me"
            | "clear what youve learned"
            | "clear what youve learned about me"
            | "clear everything youve learned"
            | "clear my corrections"
            | "clear the corrections log"
    ) {
        return Some(Asked::Everything);
    }
    if matches!(
        t,
        "stop learning from me"
            | "stop learning from what i say"
            | "dont learn from me"
            | "stop learning from my corrections"
            | "stop logging my corrections"
    ) {
        return Some(Asked::Stop);
    }
    if matches!(
        t,
        "start learning again"
            | "start learning from me again"
            | "learn from me again"
            | "resume learning"
            | "keep learning from me"
    ) {
        return Some(Asked::Start);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(kind: Kind) -> Facts {
        Facts {
            kind,
            ts: 1_790_900_000,
            said: "no, I meant open calculator".into(),
            prev: Prev {
                said: "open notepad".into(),
                reply: "Opening notepad.".into(),
                intent: Some("open_app".into()),
            },
            context: Context {
                app: "explorer.exe".into(),
                recent: vec!["clock".into(), "open_app".into()],
                source: "phrase".into(),
            },
            did_intent: Some("open_app".into()),
            did_detail: Some("notepad".into()),
            right_intent: Some("open_app".into()),
            right_detail: Some("calculator".into()),
            from: From::OwnerSaid,
            ask: None,
            outside: false,
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("atlas-corrlog-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn each_kind_builds_with_the_schema_fields() {
        for (kind, from) in [
            (Kind::Correction, From::OwnerSaid),
            (Kind::Undo, From::Undo),
            (Kind::Approval, From::Approval),
            (Kind::Rejection, From::Rejection),
            (Kind::Clarified, From::OwnerAnswer),
        ] {
            let mut f = facts(kind);
            f.from = from;
            if kind == Kind::Clarified {
                f.ask = Some(("Pay which of the two?".into(), vec!["a".into(), "b".into()]));
            }
            if kind == Kind::Undo || kind == Kind::Rejection {
                f.right_intent = None;
                f.right_detail = None;
            }
            let r = build(&f).expect("builds");
            let v = serde_json::to_value(&r).unwrap();
            assert_eq!(v["v"], 1);
            assert_eq!(v["id"].as_str().unwrap().len(), 16);
            assert!(v["id"].as_str().unwrap().chars().all(|c| c.is_ascii_hexdigit()));
            for k in ["ts", "kind", "said", "prev", "context", "did", "right", "ask", "redacted"] {
                assert!(v.get(k).is_some(), "{kind:?} lacks {k}");
            }
            let want = match kind {
                Kind::Correction => "correction",
                Kind::Undo => "undo",
                Kind::Approval => "approval",
                Kind::Rejection => "rejection",
                Kind::Clarified => "clarified",
            };
            assert_eq!(v["kind"], want);
            assert_eq!(v["right"]["from"], match from {
                From::OwnerSaid => "owner_said",
                From::OwnerAnswer => "owner_answer",
                From::Undo => "undo",
                From::Approval => "approval",
                From::Rejection => "rejection",
            });
            assert_eq!(v["ask"].is_null(), kind != Kind::Clarified);
            assert_eq!(r.context.recent, vec!["clock", "open_app"]);
        }
    }

    #[test]
    fn a_password_and_a_code_never_appear() {
        let (m, n) = mask_secret_words("my password is hunter2 and the verification code is 482913");
        assert!(!m.contains("hunter2") && !m.contains("482913"), "{m}");
        assert!(n >= 1);
        // Every way of saying it, to the end of the clause.
        for (said, secret) in [
            ("the password for gmail is hunter2", "hunter2"),
            ("my pin number is 1234", "1234"),
            ("the code is 4 8 2 9", "4 8 2 9"),
            ("the code is four eight two nine", "four eight two nine"),
            ("2fa code 123456", "123456"),
            ("my api key is abc123xyz", "abc123xyz"),
            ("password is correct horse battery staple", "horse battery"),
            ("passcode: 8842", "8842"),
        ] {
            let (m, n) = mask_secret_words(said);
            assert!(n >= 1 && !m.contains(secret), "{said:?} -> {m:?}");
            // Never kept, in any field: the line is not written.
            let mut f = facts(Kind::Correction);
            f.said = said.into();
            assert!(build(&f).is_none(), "{said:?} was written");
            let mut f = facts(Kind::Correction);
            f.prev.reply = said.into();
            assert!(build(&f).is_none(), "{said:?} in a reply was written");
            let mut f = facts(Kind::Correction);
            f.right_detail = Some(said.into());
            assert!(build(&f).is_none(), "{said:?} in a detail was written");
        }
        // The clause ends at a full stop; what is after it is not hidden.
        let (m, _) = mask_secret_words("my pin is 1234. open notepad");
        assert!(m.contains("open notepad") && !m.contains("1234"), "{m}");
        assert_eq!(mask_secret_words("press the enter key").1, 0);
        // The value before the word, and the word anywhere at all.
        for said in [
            "Hunter2 is the password",
            "my pin's 1234 by the way",
            "the verification code was 482913",
            "here is my API key",
            "that was my seed phrase",
            "use the two-factor code from my phone",
            "a secret",
        ] {
            let mut f = facts(Kind::Correction);
            f.said = said.into();
            assert!(build(&f).is_none(), "{said:?} was written");
            let mut f = facts(Kind::Correction);
            f.prev.said = said.into();
            assert!(build(&f).is_none(), "{said:?} as prev.said was written");
            let mut f = facts(Kind::Clarified);
            f.ask = Some((said.into(), vec![]));
            assert!(build(&f).is_none(), "{said:?} in a question was written");
        }
        // A weak word alone still logs.
        for said in ["open the code folder", "press the enter key", "write the code for a timer"] {
            let mut f = facts(Kind::Correction);
            f.said = said.into();
            assert!(build(&f).is_some(), "{said:?} was dropped");
        }
        assert_eq!(mask_secret_words("open the code editor").1, 0);
        assert_eq!(mask_secret_words("write the code for a timer").1, 0);
        assert_eq!(mask_secret_words("open my password manager").1, 0);
        // A secret said in the words drops the record whole.
        let mut f = facts(Kind::Correction);
        f.said = "no, my password is hunter2".into();
        assert!(build(&f).is_none());
        // One in what Atlas said back is cut out of the line.
        let mut f = facts(Kind::Approval);
        f.prev.reply = "Sent AKIAIOSFODNN7EXAMPLE and mailed sam@example.com".into();
        let r = build(&f).unwrap();
        let line = serde_json::to_string(&r).unwrap();
        assert!(!line.contains("AKIAIOSFODNN7EXAMPLE") && !line.contains("sam@example.com"), "{line}");
        assert!(r.redacted >= 2, "{}", r.redacted);
    }

    #[test]
    fn nothing_is_built_while_a_masked_read_is_open() {
        let _g = crate::typed::MaskedGuard::enter();
        assert!(build(&facts(Kind::Approval)).is_none());
        let dir = tmp("masked");
        let mut log = Log::at(&dir);
        let mut k = Keeper { log: Log::at(&dir), on: true, chain: vec![], chain_at: 0, held: None, unsure: Default::default(), warned: false };
        drop(_g);
        let rec = build(&facts(Kind::Approval)).unwrap();
        let _g = crate::typed::MaskedGuard::enter();
        assert!(k.write(&rec).is_none());
        assert!(log.read().is_empty(), "written during a masked read");
        let _ = &mut log;
    }

    #[test]
    fn a_turn_that_handled_a_secret_leaves_nothing() {
        for name in ["sign_in", "create_account", "type_code", "two_factor", "set_key", "unlock", "pair", "hand_over"] {
            let mut f = facts(Kind::Approval);
            f.did_intent = Some(name.into());
            assert!(build(&f).is_none(), "{name} was kept");
            let mut f = facts(Kind::Undo);
            f.prev.intent = Some(name.into());
            assert!(build(&f).is_none(), "{name} was kept (prev)");
        }
        let mut f = facts(Kind::Approval);
        f.context.recent = vec!["sign_in".into()];
        assert!(build(&f).is_none());
    }

    #[test]
    fn a_turn_that_read_outside_text_keeps_only_your_words() {
        let mut f = facts(Kind::Approval);
        f.outside = true;
        f.prev.reply = "The email from Dana says the wire goes to account 12345.".into();
        f.did_detail = Some("wire to Dana".into());
        f.ask = Some(("Pay Dana?".into(), vec!["yes".into()]));
        let r = build(&f).unwrap();
        assert_eq!(r.prev.reply, "");
        assert!(r.did.detail.is_none() && r.right.detail.is_none() && r.ask.is_none());
        assert_eq!(r.said, "no, I meant open calculator");
        assert_eq!(r.prev.said, "open notepad");
    }

    #[test]
    fn a_line_is_cut_to_four_kilobytes() {
        let mut f = facts(Kind::Clarified);
        f.said = "word ".repeat(2000);
        f.prev.said = "x".repeat(9000);
        f.prev.reply = "y".repeat(9000);
        f.did_detail = Some("z".repeat(9000));
        f.ask = Some(("q".repeat(9000), (0..40).map(|i| format!("{i}{}", "o".repeat(500))).collect()));
        let r = build(&f).unwrap();
        let line = serde_json::to_string(&r).unwrap();
        assert!(line.len() <= MAX_LINE_BYTES, "{}", line.len());
        assert!(r.prev.reply.chars().count() <= 300);
        assert!(r.did.detail.unwrap().chars().count() <= 80);
    }

    fn rec(said: &str, ts: u64) -> Record {
        let mut f = facts(Kind::Approval);
        f.said = said.into();
        f.ts = ts;
        build(&f).unwrap()
    }

    #[test]
    fn a_synced_line_is_kept_once_refused_if_it_carries_a_secret_and_forgotten_by_id() {
        let store = crate::store::Store::new(tmp("synced"));
        let mut k = Keeper::open(&store);
        let r = rec("open spotify", 1_000);
        let line = serde_json::to_string(&r).unwrap();
        assert!(k.take_synced(&line).unwrap());
        assert!(!k.take_synced(&line).unwrap(), "a line is kept once");
        assert_eq!(k.ids(), vec![r.id.clone()]);
        // A line that slipped past its own device's scrubbing is not kept.
        let mut leaky = rec("email me at someone@example.com", 1_001);
        leaky.said = "email me at someone@example.com".into();
        let bad = serde_json::to_string(&leaky).unwrap();
        let err = k.take_synced(&bad).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(k.ids().len(), 1, "a leaky line reached the file");
        // Not a record, or another schema: nothing.
        assert!(!k.take_synced("not json").unwrap());
        // Forgotten by id, then everything.
        assert_eq!(k.forget_synced(&r.id).unwrap(), 1);
        k.take_synced(&line).unwrap();
        assert_eq!(k.forget_synced("*").unwrap(), 1);
        assert!(k.ids().is_empty());
        // Learning stopped here: nothing is taken.
        k.on = false;
        assert!(!k.take_synced(&line).unwrap());
        assert!(k.ids().is_empty());
    }

    #[test]
    fn over_the_cap_the_oldest_lines_go() {
        let dir = tmp("cap");
        let mut log = Log::at(&dir);
        for i in 0..(MAX_LINES + 5) {
            log.append(&rec(&format!("line number {i}"), 1_000 + i as u64)).unwrap();
        }
        let all = log.read();
        assert!(all.len() <= MAX_LINES, "{}", all.len());
        // A line is about 350 bytes, so the byte bound is the one reached first.
        assert!(all.len() < MAX_LINES + 5, "the oldest lines should have gone");
        assert_eq!(all.last().unwrap().said, format!("line number {}", MAX_LINES + 4));
        assert!(all[0].said != "line number 0", "the oldest should have gone");
        let bytes = std::fs::metadata(log.path()).unwrap().len();
        assert!(bytes <= MAX_BYTES);
    }

    #[test]
    fn over_the_byte_cap_the_oldest_lines_go() {
        let dir = tmp("bytes");
        let mut log = Log::at(&dir);
        let big = |i: usize| {
            let mut f = facts(Kind::Approval);
            f.said = format!("n{i} {}", "w".repeat(380));
            f.prev.said = "p".repeat(380);
            f.prev.reply = "r".repeat(290);
            f.ask = Some(("q".repeat(290), vec!["o".repeat(78); 6]));
            build(&f).unwrap()
        };
        let one = serde_json::to_string(&big(0)).unwrap().len() + 1;
        let n = (MAX_BYTES as usize / one) + 40;
        assert!(n < MAX_LINES, "lines would trim first ({n})");
        for i in 0..n {
            log.append(&big(i)).unwrap();
        }
        let bytes = std::fs::metadata(log.path()).unwrap().len();
        assert!(bytes <= MAX_BYTES, "{bytes}");
        let all = log.read();
        assert!(all.len() < n);
        assert!(all.last().unwrap().said.starts_with(&format!("n{} ", n - 1)));
        assert!(!all[0].said.starts_with("n0 "));
    }

    #[test]
    fn forgetting_by_chain_by_word_and_everything() {
        let dir = tmp("forget");
        let store = crate::store::Store::new(dir.join("s"));
        let mut k = Keeper::open(&store);
        for (i, s) in ["call Sam about lunch", "open notepad", "read the tax file"].iter().enumerate() {
            assert!(k.write(&rec(s, 1_000 + i as u64 * 1000)).is_none());
        }
        // The chain is the lines written close together: only the last.
        assert_eq!(k.forget_that().unwrap(), 1);
        let said: Vec<String> = k.log.read().into_iter().map(|r| r.said).collect();
        assert_eq!(said, vec!["call Sam about lunch", "open notepad"]);
        // By word, any case.
        assert_eq!(k.forget_about("SAM").unwrap(), 1);
        assert_eq!(k.log.read().len(), 1);
        assert_eq!(k.forget_about("nothing like this").unwrap(), 0);
        // After a restart the chain is gone: the newest line.
        let mut k2 = Keeper::open(&store);
        assert_eq!(k2.forget_that().unwrap(), 1);
        assert!(k2.log.read().is_empty());
        k2.write(&rec("one", 5)).unwrap_or_default();
        k2.write(&rec("two", 6)).unwrap_or_default();
        assert_eq!(k2.forget_that().unwrap(), 2, "one chain, two lines");
        k2.write(&rec("three", 9_000)).unwrap_or_default();
        assert_eq!(k2.forget_everything().unwrap(), 1);
        assert!(k2.log.read().is_empty());
    }

    #[test]
    fn a_failed_forget_keeps_the_chain_for_the_next_try() {
        let dir = tmp("chainkeep");
        // A file where the folder should be: every rewrite fails.
        std::fs::write(dir.join("learning"), "in the way").unwrap();
        let mut k = Keeper { log: Log::at(&dir), on: true, chain: vec!["abc".into()], chain_at: 10, held: None, unsure: Default::default(), warned: false };
        assert!(k.forget_that().is_err());
        assert_eq!(k.chain, vec!["abc".to_string()], "the chain was let go before the rewrite worked");
    }

    #[test]
    fn two_writers_do_not_lose_each_others_lines() {
        let dir = tmp("two");
        let threads: Vec<_> = (0..6)
            .map(|w| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    // Each its own Log, as two processes would have.
                    let mut log = Log::at(&dir);
                    for i in 0..40 {
                        log.append(&rec(&format!("w{w} line {i}"), 1_000 + i)).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let log = Log::at(&dir);
        assert_eq!(log.read().len(), 6 * 40);
        assert!(!log.path().with_extension("jsonl.lock").exists(), "the lock was left behind");
    }

    #[test]
    fn a_rewrite_by_one_does_not_drop_a_line_another_added() {
        let dir = tmp("rewrite");
        let mut a = Log::at(&dir);
        let mut b = Log::at(&dir);
        a.append(&rec("first", 1)).unwrap();
        b.append(&rec("from the other process", 2)).unwrap();
        // `a` has not seen b's line; its forget must still keep it.
        assert_eq!(a.forget_about("first").unwrap(), 1);
        let said: Vec<String> = b.read().into_iter().map(|r| r.said).collect();
        assert_eq!(said, vec!["from the other process"]);
        b.append(&rec("after", 3)).unwrap();
        assert_eq!(Log::at(&dir).read().len(), 2);
    }

    #[test]
    fn logging_off_writes_nothing_and_survives_a_restart() {
        let dir = tmp("off");
        let store = crate::store::Store::new(dir.join("s"));
        let mut k = Keeper::open(&store);
        k.set_on(&store, false).unwrap();
        assert!(k.write(&rec("while off", 1)).is_none());
        assert!(!k.log.path().exists(), "a file appeared while off");
        let mut k = Keeper::open(&store);
        assert!(!k.on, "off did not last a restart");
        k.set_on(&store, true).unwrap();
        k.write(&rec("while on", 2));
        assert_eq!(k.log.read().len(), 1);
    }

    #[test]
    fn a_failed_write_is_said_once() {
        let dir = tmp("fail");
        // A file where the folder should be.
        std::fs::write(dir.join("learning"), "in the way").unwrap();
        let mut k = Keeper { log: Log::at(&dir), on: true, chain: vec![], chain_at: 0, held: None, unsure: Default::default(), warned: false };
        assert!(k.write(&rec("a", 1)).is_some());
        assert!(k.write(&rec("b", 2)).is_none());
    }

    #[test]
    fn the_asks_are_narrow() {
        assert_eq!(asked_of_the_log("Forget that."), Some(Asked::Last));
        assert_eq!(asked_of_the_log("forget the last thing"), Some(Asked::Last));
        assert_eq!(asked_of_the_log("forget what I said about Sam"), Some(Asked::About("sam".into())));
        assert_eq!(asked_of_the_log("Forget everything"), Some(Asked::Everything));
        assert_eq!(asked_of_the_log("clear what you've learned"), Some(Asked::Everything));
        assert_eq!(asked_of_the_log("stop learning from me"), Some(Asked::Stop));
        assert_eq!(asked_of_the_log("start learning again"), Some(Asked::Start));
        assert_eq!(asked_of_the_log("forget that phrase"), None);
        assert_eq!(asked_of_the_log("forget it"), None);
        assert_eq!(asked_of_the_log("open notepad"), None);
    }
}
