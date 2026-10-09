//! Things you handed Atlas, from wherever you were standing.
//!
//! You find a link on your phone. Today that means mailing it to yourself, or
//! remembering it, or losing it. What you want is to hand it over and have it
//! be waiting, read, when you sit down.
//!
//! The pieces for the reading already existed: `browser.rs` drives a real
//! headless Chrome, `research.rs` fetches a page and strips it down. What was
//! missing was somewhere to put a thing before Atlas gets to it, and a rule
//! about what handing something over does and does not mean.
//!
//! ## The rule that matters
//!
//! **Handing Atlas a link says "look at this". It never says "do what this
//! says."**
//!
//! That distinction is the whole safety property here, and it is easy to lose
//! by accident: the natural next step after fetching a page is to feed the text
//! to the part of Atlas that works out what to do, and at that moment any page
//! on the internet can issue Atlas instructions in your name. So fetched text
//! is stored as something to be *shown*, never parsed into an intent, and
//! `tests/tray.rs` fails the build if a route from `found` to the parser ever
//! appears.
//!
//! ## Why it waits
//!
//! An item is read, then offered. Whether Atlas goes further — replies to it,
//! files it, acts on what it found — is a question for `earned`, per kind of
//! work, exactly like anything else it does on its own. Dropping something in
//! is not consent to act on it, and a tray that acted immediately would be a
//! way to get around every bar Atlas has.
//!
//! ## Where it syncs
//!
//! Nowhere new. Your phone already reaches your desktop through the hub's own
//! door — same person, same machine, a token you already hold. `kin.rs` exists
//! because another Atlas is a different trust boundary; your own phone is not,
//! and inventing a second sync path for it would mean two doors to keep honest
//! instead of one.

use serde::{Deserialize, Serialize};

/// What kind of thing was handed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// A web address.
    Link,
    /// A photo or a screenshot. Read with OCR — the common case is a picture
    /// of something written down: a receipt, a whiteboard, a page of a
    /// contract, a parking sign.
    Image,
    /// Video. The words in it are usually the point, so the sound is what
    /// gets read.
    Video,
    /// A voice memo or a recording.
    Audio,
    /// Something written: a PDF, a spreadsheet, a text file.
    Document,
    /// Source code, in any language.
    Code,
    /// A file on this machine, kind unknown.
    File,
    /// Something you typed or pasted.
    Words,
}

impl Sort {
    /// Worked out from the thing itself. Asking would mean a share sheet with
    /// a dropdown on it, which is how a two-second action becomes one you stop
    /// bothering with.
    pub fn of(what: &str) -> Sort {
        let w = what.trim();
        if w.starts_with("http://") || w.starts_with("https://") {
            return Sort::Link;
        }
        let looks_like_a_path = w.starts_with('/')
            || w.starts_with("~/")
            || (w.len() > 3 && w.as_bytes()[1] == b':' && w.contains('\\'));
        if looks_like_a_path {
            return Sort::of_file(w);
        }
        Sort::Words
    }

    /// What a file is, from its name.
    ///
    /// The extension, not the bytes. Reading the first few bytes would be more
    /// certain and would mean opening every file the moment it arrives, before
    /// anyone has decided it should be opened at all.
    pub fn of_file(name: &str) -> Sort {
        let ext = name
            .rsplit('.')
            .next()
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "tif" | "tiff" => {
                Sort::Image
            }
            "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" => Sort::Video,
            "mp3" | "wav" | "m4a" | "aac" | "ogg" | "flac" | "opus" => Sort::Audio,
            "pdf" | "doc" | "docx" | "txt" | "md" | "rtf" | "csv" | "xlsx" | "odt" => {
                Sort::Document
            }
            // Code is its own thing. A `.py` read as a document gets its first
            // four sentences summarised, which for code is the import block --
            // the least interesting part of the file by some distance.
            "rs" | "py" | "pyi" | "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "go"
            | "java" | "cs" | "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "rb" | "php"
            | "sh" | "bash" | "zsh" | "sql" => Sort::Code,
            _ => Sort::File,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Sort::Link => "Link",
            Sort::Image => "Photo",
            Sort::Video => "Video",
            Sort::Audio => "Recording",
            Sort::Document => "Document",
            Sort::Code => "Code",
            Sort::File => "File",
            Sort::Words => "Note",
        }
    }
}

/// Where an item is up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Handed over, not yet looked at.
    Waiting,
    /// Read. `found` says what is in it.
    Read,
    /// Atlas tried and couldn't. `found` says why.
    Stuck,
    /// You're finished with it.
    Done,
}

/// One thing you handed over.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    /// The link, path, or words themselves.
    pub what: String,
    pub sort: Sort,
    /// Yours, or a named business. Recorded, not enforced — the boundary
    /// between the two is a conversation Eric has asked to have before
    /// anything decides it.
    #[serde(default)]
    pub space: crate::earned::Space,
    pub state: State,
    /// When it arrived.
    pub at: u64,
    /// Which device it came from, in your words, so "where did I send that
    /// from" has an answer.
    #[serde(default)]
    pub from: String,
    /// What you wanted done with it, if you said.
    ///
    /// The gap that made the first version half a feature: you can hand over a
    /// contract, and what you actually mean is "tell me if the payment terms
    /// changed". Without somewhere to say that, Atlas reads it and reports the
    /// first four sentences, which is almost never the answer to the question
    /// you had in mind when you sent it.
    ///
    /// **Yours, so it is read as a request. The thing itself is not.** This is
    /// the only text on an item that Atlas treats as coming from you.
    #[serde(default)]
    pub asked: Option<String>,
    /// Where the file itself is kept, for things that arrived as bytes.
    #[serde(default)]
    pub stored_at: Option<String>,
    /// What Atlas found in it, once it has looked.
    ///
    /// **Shown, never obeyed.** This is text from outside; treating it as an
    /// instruction would let any page on the internet drive Atlas in your
    /// name.
    #[serde(default)]
    pub found: Option<String>,
    /// Whether Atlas owns the stored bytes — a copy it wrote under
    /// `data/tray/` — or is only pointing at a file that lives elsewhere on
    /// your machine. A large local file is taken in *by reference* (no size
    /// cap, no second copy), so forgetting it must not delete your original.
    /// Defaults true, because every item written before this field existed
    /// was an owned copy.
    #[serde(default = "owned_by_default")]
    pub owned: bool,
    /// Who it has been sent to, and when: the Documents page's share log
    /// (26 Sep's gap: every item showed Private because nothing recorded a
    /// send). Only a send you made from Atlas lands here.
    #[serde(default)]
    pub shared: Vec<(String, u64)>,
}

/// A copy Atlas made is Atlas's to delete; the default matches every item
/// that predates the `owned` field, all of which were owned copies.
fn owned_by_default() -> bool {
    true
}

impl Item {
    /// A short way to refer to it out loud.
    pub fn title(&self) -> String {
        match self.sort {
            Sort::Link => self
                .what
                .split("://")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .unwrap_or(&self.what)
                .to_string(),
            Sort::File => self
                .what
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&self.what)
                .to_string(),
            Sort::Image | Sort::Video | Sort::Audio | Sort::Document | Sort::Code => self
                .stored_at
                .as_deref()
                .unwrap_or(&self.what)
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&self.what)
                .to_string(),
            Sort::Words => self.what.chars().take(48).collect(),
        }
    }
}

/// Everything handed over, in the order it arrived.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tray {
    pub items: Vec<Item>,
    next_id: u64,
}

/// Where it is kept.
pub const FILE: &str = "tray";

/// How many finished items to keep.
///
/// Enough to answer "what was that thing I sent last week", few enough that
/// the file does not become an archive of everything you have ever glanced at.
pub const KEEP_DONE: usize = 40;

/// The longest thing that can be handed over in one go.
///
/// A link is short and a pasted note is not a document. Anything larger is a
/// file, and a file is handed over by path — so this is a guard against a
/// mistake or a flood rather than a limit anyone would meet honestly.
pub const MAX_LEN: usize = 4_000;

impl Tray {
    pub fn load(store: &crate::store::Store) -> Tray {
        store.load::<Tray>(FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(FILE, self)
    }

    /// Hand something over. Returns its id, or why not.
    ///
    /// The same thing twice is the same thing: sending a link from the phone
    /// and then again from the laptop is one intention, and two entries would
    /// mean reading it twice and offering it twice.
    pub fn hand(
        &mut self,
        what: &str,
        space: &crate::earned::Space,
        from: &str,
        at: u64,
    ) -> Result<u64, String> {
        let what = what.trim();
        if what.is_empty() {
            return Err("There was nothing in that.".into());
        }
        if what.chars().count() > MAX_LEN {
            return Err("That's longer than I take in one go — save it as a file and \
                 hand me the path instead.".to_string());
        }
        if let Some(existing) = self
            .items
            .iter()
            .find(|i| i.what == what && i.state != State::Done)
        {
            return Ok(existing.id);
        }

        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Item {
            shared: Vec::new(),
            id,
            what: what.to_string(),
            sort: Sort::of(what),
            space: space.clone(),
            state: State::Waiting,
            at,
            from: from.to_string(),
            asked: None,
            stored_at: None,
            found: None,
            // A note carries no file, so ownership is moot — true keeps the
            // "Atlas made this" default rather than implying a foreign file.
            owned: true,
        });
        Ok(id)
    }

    /// The next thing to look at, oldest first.
    ///
    /// Oldest rather than newest: the thing you sent this morning and forgot
    /// is exactly the one that should not be permanently overtaken by whatever
    /// you sent a minute ago.
    pub fn next_to_read(&self) -> Option<&Item> {
        self.items.iter().find(|i| i.state == State::Waiting)
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// Atlas looked, and this is what was in it.
    pub fn read(&mut self, id: u64, found: &str) -> bool {
        match self.find_mut(id) {
            Some(i) => {
                i.state = State::Read;
                i.found = Some(found.to_string());
                true
            }
            None => false,
        }
    }

    /// Atlas tried and couldn't.
    ///
    /// A separate state from read, because "I couldn't open it" and "I opened
    /// it and there was nothing in it" send you to different places, and one
    /// state for both would send you to neither.
    pub fn stuck(&mut self, id: u64, why: &str) -> bool {
        match self.find_mut(id) {
            Some(i) => {
                i.state = State::Stuck;
                i.found = Some(why.to_string());
                true
            }
            None => false,
        }
    }

    pub fn done(&mut self, id: u64) -> bool {
        let ok = match self.find_mut(id) {
            Some(i) if i.state != State::Done => {
                i.state = State::Done;
                true
            }
            _ => false,
        };
        if ok {
            self.forget_old_finished();
        }
        ok
    }

    /// Trim finished items only. Anything still waiting or read stays however
    /// long it has been there — dropping those would lose something you handed
    /// over and never got an answer about, which is the one thing this must
    /// not do.
    fn forget_old_finished(&mut self) {
        let mut seen = 0;
        let mut keep = vec![true; self.items.len()];
        for i in (0..self.items.len()).rev() {
            if self.items[i].state == State::Done {
                seen += 1;
                if seen > KEEP_DONE {
                    keep[i] = false;
                }
            }
        }
        let mut i = 0;
        self.items.retain(|_| {
            let k = keep[i];
            i += 1;
            k
        });
    }

    /// Everything not finished with.
    pub fn open(&self) -> Vec<&Item> {
        self.items
            .iter()
            .filter(|i| i.state != State::Done)
            .collect()
    }

    /// Things read and waiting for you to see them.
    pub fn ready(&self) -> Vec<&Item> {
        self.items
            .iter()
            .filter(|i| i.state == State::Read)
            .collect()
    }

    /// What to say when you ask what's in the tray.
    pub fn spoken(&self) -> String {
        let waiting = self
            .items
            .iter()
            .filter(|i| i.state == State::Waiting)
            .count();
        let ready = self.ready().len();
        let stuck = self
            .items
            .iter()
            .filter(|i| i.state == State::Stuck)
            .count();

        if waiting + ready + stuck == 0 {
            return "You haven't handed me anything.".into();
        }
        let mut parts = Vec::new();
        if ready > 0 {
            parts.push(format!(
                "{ready} thing{} I've read and can tell you about",
                if ready == 1 { "" } else { "s" }
            ));
        }
        if waiting > 0 {
            parts.push(format!("{waiting} I haven't got to yet"));
        }
        if stuck > 0 {
            parts.push(format!(
                "{stuck} I couldn't open — {}",
                if stuck == 1 { "it's" } else { "they're" }
            ));
        }
        format!("{}.", parts.join(", "))
    }
}

/// Where files handed over from another device are kept.
pub const FOLDER: &str = "tray";

/// The largest file taken in one go.
///
/// Photos, documents, voice memos and short clips fit comfortably. A long
/// video does not, and that is deliberate: the whole file has to sit in memory
/// on the way through, and the tray is for something you want looked at rather
/// than a sync folder. Anything bigger is refused with a sentence saying what
/// to do instead, rather than filling the disk quietly or timing out.
pub const MAX_FILE_BYTES: usize = 20 * 1024 * 1024;

impl Tray {
    /// Hand over the file itself, not a path to it.
    ///
    /// A photo on your phone is not a path this machine can read, which is
    /// what made the first version desktop-only in practice. The bytes are
    /// written under `data/tray/` and the item points at them.
    ///
    /// Deduplicated by content, not by name: phones name things `IMG_0042.jpg`
    /// with enthusiasm, and two different photos sharing a name must not
    /// collapse into one, while the same photo sent twice should.
    pub fn hand_file(
        &mut self,
        name: &str,
        bytes: &[u8],
        space: &crate::earned::Space,
        from: &str,
        asked: Option<&str>,
        at: u64,
        root: &std::path::Path,
    ) -> Result<u64, String> {
        let _state = crate::store::state_transaction(root).map_err(|e| format!("I couldn't keep that: {e}"))?;
        if bytes.is_empty() {
            return Err("That file was empty.".into());
        }
        if bytes.len() > MAX_FILE_BYTES {
            return Err(format!(
                "That's {} MB — bigger than I take in one go. Put it somewhere \
                 on this machine and hand me the path instead.",
                bytes.len() / (1024 * 1024)
            ));
        }

        let stamp = fingerprint(bytes);
        if let Some(existing) = self
            .items
            .iter()
            .find(|i| i.what == stamp && i.state != State::Done)
        {
            return Ok(existing.id);
        }

        let safe = safe_name(name);
        let dir = root.join(FOLDER);
        std::fs::create_dir_all(&dir).map_err(|e| format!("I couldn't keep that: {e}"))?;
        let path = dir.join(format!("{stamp}-{safe}"));
        std::fs::write(&path, bytes).map_err(|e| format!("I couldn't keep that: {e}"))?;

        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Item {
            shared: Vec::new(),
            id,
            // The fingerprint is the identity. The name is decoration: two
            // phones will both send you an IMG_0042.
            what: stamp,
            sort: Sort::of_file(&safe),
            space: space.clone(),
            state: State::Waiting,
            at,
            from: from.to_string(),
            asked: asked.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()),
            stored_at: Some(path.to_string_lossy().to_string()),
            found: None,
            // Bytes copied into `data/tray/` — Atlas's own copy to delete.
            owned: true,
        });
        Ok(id)
    }

    /// Take in a file already on this machine, by reference.
    ///
    /// The counterpart to `hand_file`, for the desktop case the design calls
    /// "any file, any size". A file that already lives here does not have to
    /// be copied through memory to be taken in, so there is no cap: the item
    /// points at the file where it is, and `owned` is false so forgetting the
    /// item never touches your original. Deduplicated by the same content
    /// fingerprint as `hand_file`, streamed rather than held, so a 2 GB backup
    /// costs a read, not 2 GB of memory.
    pub fn hand_local(
        &mut self,
        path: &std::path::Path,
        space: &crate::earned::Space,
        from: &str,
        asked: Option<&str>,
        at: u64,
    ) -> Result<u64, String> {
        let meta = std::fs::metadata(path)
            .map_err(|e| format!("I couldn't reach {}: {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!("{} isn't a file I can take in.", path.display()));
        }
        if meta.len() == 0 {
            return Err("That file was empty.".into());
        }
        let stamp = fingerprint_file(path)
            .map_err(|e| format!("I couldn't read {}: {e}", path.display()))?;
        // Same dedup rule as the bytes path, so handing me a file twice — by
        // reference or as bytes — is one item, not two.
        if let Some(existing) = self
            .items
            .iter()
            .find(|i| i.what == stamp && i.state != State::Done)
        {
            return Ok(existing.id);
        }
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let name = abs
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Item {
            shared: Vec::new(),
            id,
            what: stamp,
            sort: Sort::of_file(&name),
            space: space.clone(),
            state: State::Waiting,
            at,
            from: from.to_string(),
            asked: asked.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()),
            stored_at: Some(abs.to_string_lossy().to_string()),
            found: None,
            // Points at your file where it lives — not Atlas's to delete.
            owned: false,
        });
        Ok(id)
    }

    /// Say what you wanted done with something already handed over.
    pub fn ask_about(&mut self, id: u64, asked: &str) -> bool {
        match self.items.iter_mut().find(|i| i.id == id) {
            Some(i) => {
                i.asked = Some(asked.trim().to_string()).filter(|a| !a.is_empty());
                true
            }
            None => false,
        }
    }

    /// Delete the kept file too.
    ///
    /// A photo of a contract is not something to leave lying in a folder
    /// because the entry that mentioned it scrolled off a list.
    /// Record that item `id` went to `who` at `at`.
    pub fn sent_to(&mut self, id: u64, who: &str, at: u64) -> bool {
        match self.find_mut(id) {
            Some(i) => {
                i.shared.push((who.to_string(), at));
                true
            }
            None => false,
        }
    }

    pub fn forget(&mut self, id: u64) -> bool {
        let Some(pos) = self.items.iter().position(|i| i.id == id) else {
            return false;
        };
        if let Some(path) = self.items[pos].stored_at.clone() {
            let root = match crate::store::state_root_for(std::path::Path::new(&path).parent().unwrap_or(std::path::Path::new("."))) { Ok(root) => root, Err(_) => return false };
            let _state = match root.as_ref().map(|root| crate::store::state_transaction(root)).transpose() { Ok(guard) => guard, Err(_) => return false };
            // Only delete a copy Atlas made. A file taken in by reference
            // (`owned == false`) is your original, sitting where you keep it,
            // and forgetting the tray item must never reach out and delete it.
            if self.items[pos].owned {
                crate::heard!(std::fs::remove_file(&path));
            }
            // Frames kept from watching a video live beside it. Deleting the
            // video and leaving forty pictures of it behind would be the
            // stored-file problem again, one directory over. These are Atlas's
            // own derived files, so they go even for a referenced original.
            if let Some(dir) = std::path::Path::new(&path).parent() {
                crate::heard!(std::fs::remove_dir_all(dir.join(format!("frames-{}", self.items[pos].id))));
            }
        }
        self.items.remove(pos);
        true
    }
}

/// A short, stable name for these exact bytes.
///
/// Not cryptography — this only has to tell two files apart and recognise the
/// same one twice, and a collision means one duplicate photo, not a security
/// failure.
fn fingerprint(bytes: &[u8]) -> String {
    let mut a: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        a ^= *b as u64;
        a = a.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{a:016x}")
}

/// The same fingerprint, computed by streaming the file instead of holding it.
///
/// Identical arithmetic to [`fingerprint`], byte for byte, so a file taken in
/// by reference and the same file taken in as bytes produce the same stamp and
/// deduplicate against each other. Reads in fixed-size chunks, so the memory
/// cost is one buffer regardless of the file's size — which is what lets
/// `hand_local` accept a file of any size without the cap `hand_file` needs.
fn fingerprint_file(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut a: u64 = 0xcbf2_9ce4_8422_2325;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for b in &buf[..n] {
            a ^= *b as u64;
            a = a.wrapping_mul(0x1000_0000_01b3);
        }
    }
    Ok(format!("{a:016x}"))
}

/// A filename that cannot escape the folder it belongs in.
///
/// Names arrive from a phone, which means they arrive from outside. `../` in a
/// filename is the oldest trick there is.
/// Strip a filename down to something safe to make a path out of.
///
/// `pub(crate)` rather than private since `kin.rs` sanitises a peer's
/// claimed filename at the door — a name from another machine is
/// attacker-controlled text, and there is one door but several places that
/// eventually write it.
pub(crate) fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '))
        .take(60)
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "file".into()
    } else {
        cleaned
    }
}

/// Decode the bytes a phone sent.
///
/// Written here rather than pulled in, because it is twenty lines and a
/// dependency added for twenty lines is a dependency to keep updated forever.
/// Rejects anything that is not valid base64 rather than decoding as much as it
/// can — a half-decoded photo is a corrupt file that looks like a real one.
pub fn from_base64(text: &str) -> Result<Vec<u8>, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let cleaned: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        // Some senders use the URL-safe alphabet without saying so.
        .map(|b| match b {
            b'-' => b'+',
            b'_' => b'/',
            other => other,
        })
        .collect();
    let cleaned: Vec<u8> = cleaned
        .into_iter()
        .take_while(|b| *b != b'=')
        .collect();

    let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for b in cleaned {
        let Some(v) = ALPHABET.iter().position(|a| *a == b) else {
            return Err("That file didn't arrive intact — the encoding was wrong.".into());
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    if out.is_empty() {
        return Err("That file was empty.".into());
    }
    Ok(out)
}
