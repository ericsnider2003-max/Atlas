//! Indexing engine: "know what exists in the workspace".
//!
//! Metadata-first by design — path, name, extension, size, timestamp, asset
//! class. Reading file *contents* is on-demand enrichment, never part of the
//! background scan, because a laptop that reindexes Documents by content will
//! be unusable while it does.

use crate::error::Result;
use crate::store::{now, Store};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    Document,
    Image,
    Video,
    Audio,
    Code,
    Archive,
    Other,
}

fn read_enrichment_text(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "indexed text exceeds the enrichment limit",
        ));
    }
    let mut text = String::new();
    file.take(max_bytes + 1).read_to_string(&mut text)?;
    if text.len() as u64 > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "indexed text grew past the enrichment limit while reading",
        ));
    }
    Ok(text)
}

fn read_enrichment_archive(path: &Path, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "indexed archive exceeds the enrichment limit",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "indexed archive grew past the enrichment limit while reading",
        ));
    }
    Ok(bytes)
}

impl AssetClass {
    pub fn of(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "pdf" | "docx" | "doc" | "txt" | "md" | "rtf" | "odt" | "pptx" | "xlsx" | "csv" => {
                Self::Document
            }
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "svg" | "heic" => {
                Self::Image
            }
            "mp4" | "mov" | "mkv" | "avi" | "webm" | "wmv" | "m4v" => Self::Video,
            "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" => Self::Audio,
            "rs" | "py" | "js" | "ts" | "go" | "c" | "cpp" | "h" | "java" | "cs" | "rb" | "sh"
            | "ps1" | "yaml" | "yml" | "json" | "toml" => Self::Code,
            "zip" | "7z" | "rar" | "tar" | "gz" => Self::Archive,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub modified: u64,
    pub class: AssetClass,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IndexConfig {
    /// Allowlist. Nothing outside these is ever touched.
    pub roots: Vec<String>,
    #[serde(default)]
    pub exclude_dirs: Vec<String>,
    #[serde(default)]
    pub exclude_exts: Vec<String>,
    #[serde(default = "d_depth")]
    pub max_depth: u32,
    /// Files above this are indexed by metadata but never content-enriched.
    #[serde(default = "d_maxmb")]
    pub max_enrich_mb: u64,
}
fn d_depth() -> u32 {
    8
}
fn d_maxmb() -> u64 {
    20
}

impl IndexConfig {
    fn excluded_dir(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        self.exclude_dirs.iter().any(|d| d.to_lowercase() == n)
    }
    fn excluded_ext(&self, ext: &str) -> bool {
        let e = ext.to_lowercase();
        self.exclude_exts.iter().any(|d| d.to_lowercase() == e)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Index {
    pub entries: BTreeMap<String, Entry>,
    pub last_scan: u64,
    /// What the scan could not read.
    ///
    /// Without this, "search found nothing" and "search could not look" are
    /// the same answer. That is the shape `hollow` names — absence of a
    /// finding read as absence of a problem — and it is worse here than
    /// elsewhere, because a search returning nothing feels like a definite
    /// result rather than a failed one.
    #[serde(default)]
    pub missed: Missed,
}

/// Everything a scan walked past, and why.
///
/// Counts for all of it, paths for the first few. A tally tells you the scale
/// and a handful of examples tells you the cause, which together is usually
/// enough to know whether to care — and keeping every path would make this
/// grow with the size of the disk.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Missed {
    /// Directories that could not be opened. Usually permissions.
    pub unreadable_dirs: u32,
    /// Files whose type or metadata could not be read.
    pub unreadable_files: u32,
    /// Directories below the depth limit, never looked at.
    pub too_deep: u32,
    /// Files indexed with no usable timestamp.
    ///
    /// Counted separately because these are not missing from the index — they
    /// are in it, dated to the epoch, and `recall` ranks by recency. An
    /// unreadable timestamp does not hide a file, it buries it.
    pub undated: u32,
    /// A few of each, for working out the cause.
    pub examples: Vec<String>,
    /// Why the saved index (or the scan that should have replaced it) did not
    /// arrive, in plain words (21 Oct 2026). Set when the load thread died or
    /// the saved file had to be set aside; without it an index that failed to
    /// load looked exactly like an empty disk. Never saved: it describes this
    /// run, and a new scan that works clears it.
    #[serde(skip)]
    pub not_loaded: Option<String>,
}

/// How many example paths to keep.
const KEEP_EXAMPLES: usize = 8;

/// The most the saved index may be, written or read (20 Oct 2026).
///
/// The store's default record budget is 16 MiB, and a real index of 41,797
/// files was 17 MB of pretty JSON: valid, but over the budget, so every start
/// set it aside as unreadable, began empty, rescanned and wrote another 17 MB
/// file. The index grows with the disk, so it names its own, much larger,
/// budget, and saves and loads with the same number so that Atlas can never
/// write an index it will then refuse to read. It is still a bound: a file
/// past it is set aside as oversized rather than read without limit.
///
/// 64 MiB (21 Oct 2026; it was 256 MiB). Loading holds the text and its parsed
/// copy at once, so the peak is several times the file: at 256 MiB that was
/// gigabytes. 64 MiB is still about four times the 17 MB real index.
pub const INDEX_MAX_BYTES: u64 = 64 * 1024 * 1024;

impl Missed {
    fn note(&mut self, path: &Path) {
        if self.examples.len() < KEEP_EXAMPLES {
            self.examples.push(path.to_string_lossy().to_string());
        }
    }

    pub fn anything(&self) -> bool {
        self.not_loaded.is_some()
            || self.unreadable_dirs > 0
            || self.unreadable_files > 0
            || self.too_deep > 0
            || self.undated > 0
    }

    /// What to say when a search comes back thin.
    ///
    /// Returns `None` when the scan saw everything, so a clean run adds no
    /// noise to an ordinary answer.
    pub fn caveat(&self) -> Option<String> {
        if !self.anything() {
            return None;
        }
        if let Some(why) = &self.not_loaded {
            let rest = Missed {
                not_loaded: None,
                ..self.clone()
            }
            .caveat()
            .map(|c| format!(" {c}"))
            .unwrap_or_default();
            return Some(format!(
                "My file index didn't load ({why}), so I may be missing files --                  say 'rebuild the index' and I'll read your folders again.{rest}"
            ));
        }
        let mut bits = Vec::new();
        if self.unreadable_dirs > 0 {
            bits.push(format!("{} folders I couldn't open", self.unreadable_dirs));
        }
        if self.too_deep > 0 {
            bits.push(format!("{} nested too deep to reach", self.too_deep));
        }
        if self.unreadable_files > 0 {
            bits.push(format!("{} files I couldn't read", self.unreadable_files));
        }
        if self.undated > 0 {
            bits.push(format!(
                "{} with no readable date, so they'll rank as old",
                self.undated
            ));
        }
        Some(format!("Worth knowing: {}.", bits.join(", ")))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContentHit {
    pub path: String,
    pub name: String,
    pub excerpt: String,
    /// `notes.md:12-18` — the lines the excerpt came from.
    pub cite: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub removed: Vec<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.removed.is_empty()
    }
    pub fn count(&self) -> usize {
        self.added.len() + self.modified.len() + self.removed.len()
    }
}

impl Index {
    pub fn scan(cfg: &IndexConfig) -> Index {
        let mut idx = Index {
            entries: BTreeMap::new(),
            last_scan: now(),
            missed: Missed::default(),
        };
        for root in &Self::roots_of(cfg) {
            let mut missed = std::mem::take(&mut idx.missed);
            walk(root, cfg, 0, &mut idx.entries, &mut missed);
            idx.missed = missed;
        }
        idx
    }

    /// The folders this scan will actually look in.
    ///
    /// The shipped allowlist is `%USERPROFILE%/Desktop` and friends, which is
    /// a Windows environment variable. On Linux and macOS nothing sets it, so
    /// `expand_env` leaves the string untouched, the path does not exist, and
    /// the walk finds nothing — file awareness was quietly switched off on
    /// two of the three platforms Atlas targets, with no error anywhere.
    ///
    /// `default_roots()` was written for exactly this and never called: it
    /// tries `USERPROFILE` *and* `HOME`, and keeps only directories that
    /// exist. It is used here as a fallback rather than a replacement, and
    /// only when **none** of the configured roots resolve — this list is an
    /// allowlist ("Atlas never looks outside these"), so widening it is the
    /// one thing that must not happen quietly. Falling back to the same three
    /// folders under the real home directory is the config's own intent
    /// resolved for this machine, not a broader reach.
    fn roots_of(cfg: &IndexConfig) -> Vec<PathBuf> {
        let configured: Vec<PathBuf> = cfg
            .roots
            .iter()
            // %USERPROFILE% and friends, so a config with no username in it
            // still points at the right folders on any machine.
            .map(|r| PathBuf::from(crate::doctor::expand_env(r)))
            .filter(|p| p.is_dir())
            .collect();
        if configured.is_empty() && !cfg.roots.is_empty() {
            return default_roots();
        }
        configured
    }

    /// Incremental update. Returns what changed, so the awareness layer can
    /// react without diffing the whole tree itself.
    pub fn rescan(&mut self, cfg: &IndexConfig) -> Changes {
        self.apply(Index::scan(cfg))
    }

    /// Take a fresh walk's result in place of this one, returning what
    /// changed. Split from `rescan` so the walk can happen on another thread
    /// (`awareness::Awareness::scan_in_background`).
    pub fn apply(&mut self, fresh: Index) -> Changes {
        let mut c = Changes::default();
        for (path, e) in &fresh.entries {
            match self.entries.get(path) {
                None => c.added.push(path.clone()),
                Some(old) if old.modified != e.modified || old.size != e.size => {
                    c.modified.push(path.clone())
                }
                _ => {}
            }
        }
        for path in self.entries.keys() {
            if !fresh.entries.contains_key(path) {
                c.removed.push(path.clone());
            }
        }
        // The old map is let go of on another thread: tens of thousands of
        // paths freed one by one was part of the half-second (30 Sep 2026).
        let old = std::mem::replace(&mut self.entries, fresh.entries);
        if old.len() > 5_000 {
            crate::heard!(std::thread::Builder::new()
                .name("atlas-index-free".into())
                .spawn(move || drop(old)));
        }
        self.last_scan = fresh.last_scan;
        // What this walk could not read replaces what the last one could not;
        // it was never copied over before. A walk that completed also ends
        // any "didn't load" note: there is a real index now.
        self.missed = fresh.missed;
        c
    }

    /// Filename search, ranked: exact > prefix > substring. Deliberately not
    /// fuzzy — a wrong file confidently returned is worse than no result.
    pub fn search(&self, query: &str) -> Vec<&Entry> {
        let q = query.to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(u8, &Entry)> = self
            .entries
            .values()
            .filter_map(|e| {
                let n = e.name.to_lowercase();
                let stem = n.rsplit_once('.').map(|(s, _)| s).unwrap_or(&n);
                if stem == q {
                    Some((0, e))
                } else if n.starts_with(&q) {
                    Some((1, e))
                } else if n.contains(&q) {
                    Some((2, e))
                } else {
                    None
                }
            })
            .collect();
        // Rank, then newest first within a rank.
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.modified.cmp(&a.1.modified)));
        hits.into_iter().map(|(_, e)| e).collect()
    }

    /// Search inside files, not just filenames. Only text-ish classes, only
    /// under the size cap — this is the expensive one, so it is on demand and
    /// never part of the background scan.
    ///
    /// Ranked, and cited to the line (23 Sep ports). Each file is cut into
    /// heading-bounded chunks with their line spans (`chunker`), the chunks
    /// are ranked by BM25 with stemming (`bm25`, `stemmer`), and each file
    /// is represented by its best chunk — so the answer can say
    /// `plan.md:112-131` and quote the paragraph that matched, rather than
    /// the first place a word happened to appear in the newest file.
    pub fn search_content(
        &self,
        query: &str,
        cfg: &IndexConfig,
        max_hits: usize,
    ) -> Vec<ContentHit> {
        let stems: Vec<String> = crate::stemmer::stems_of(query)
            .into_iter()
            .filter(|w| w.chars().count() > 2)
            .collect();
        if stems.is_empty() {
            return Vec::new();
        }
        let cap = cfg.max_enrich_mb * 1024 * 1024;
        let mut candidates: Vec<&Entry> = self
            .entries
            .values()
            .filter(|e| matches!(e.class, AssetClass::Document | AssetClass::Code) && e.size <= cap)
            .collect();
        candidates.sort_by_key(|b| std::cmp::Reverse(b.modified));

        let mut ix = crate::bm25::Index::default();
        // (the file, the passage, and the file inside it when it's an archive)
        let mut chunks: Vec<(&Entry, crate::chunker::Chunk, Option<String>)> = Vec::new();
        let shape = crate::chunker::ChunkConfig {
            max: 800,
            overlap: 0,
        };
        // Every text to search: plain files, and — inside .zip archives
        // (`zipread`), behind the same unpack guard that refuses a zip bomb
        // before anything is inflated — the text files they hold.
        let mut sources: Vec<(&Entry, String, Option<String>)> = Vec::new();
        for e in candidates {
            if let Ok(text) = read_enrichment_text(Path::new(&e.path), cap) {
                sources.push((e, text, None));
            }
        }
        for e in self.entries.values().filter(|e| {
            e.class == AssetClass::Archive
                && e.name.to_lowercase().ends_with(".zip")
                && e.size <= cap
        }) {
            let Ok(bytes) = read_enrichment_archive(Path::new(&e.path), cap) else {
                continue;
            };
            let Ok(members) =
                crate::zipread::texts_inside(&bytes, &crate::files::FilesConfig::default(), cap)
            else {
                continue;
            };
            sources.extend(members.into_iter().map(|(m, t)| (e, t, Some(m))));
        }
        for (e, text, member) in sources {
            // Cheap refusal before the chunking: a file that holds none of the
            // query's stems cannot rank.
            let lower = text.to_lowercase();
            if !stems.iter().any(|s| lower.contains(s.as_str())) {
                continue;
            }
            for c in crate::chunker::chunk(&text, shape).unwrap_or_default() {
                ix.add(chunks.len() as u64, &c.heading, &c.text);
                chunks.push((e, c, member.clone()));
            }
        }
        let mut hits: Vec<ContentHit> = Vec::new();
        for (id, _) in ix.search(query, max_hits.saturating_mul(4).max(8)) {
            let (e, c, member) = &chunks[id as usize];
            let where_ = match member {
                Some(m) => format!("{} › {m}", e.name),
                None => e.name.clone(),
            };
            if hits.iter().any(|h| h.path == e.path && h.name == where_) {
                continue;
            }
            let flat = c.text.split_whitespace().collect::<Vec<_>>().join(" ");
            let excerpt = if flat.chars().count() > 220 {
                format!("{}…", flat.chars().take(220).collect::<String>())
            } else {
                flat
            };
            hits.push(ContentHit {
                path: e.path.clone(),
                name: where_.clone(),
                excerpt,
                cite: c.cite(&where_),
            });
            if hits.len() >= max_hits {
                break;
            }
        }
        hits
    }

    pub fn recent(&self, n: usize) -> Vec<&Entry> {
        let mut v: Vec<&Entry> = self.entries.values().collect();
        v.sort_by_key(|b| std::cmp::Reverse(b.modified));
        v.into_iter().take(n).collect()
    }

    pub fn load(store: &Store) -> Index {
        store.load_bounded("index", INDEX_MAX_BYTES)
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save_compact_bounded("index", self, INDEX_MAX_BYTES)
    }
    /// What changes whenever the index does: how many entries, and when it
    /// was last scanned (`apply` sets that on every scan). Cheap to ask
    /// every turn, where writing the index out to compare it is not.
    pub fn written_as(&self) -> (usize, u64) {
        (self.entries.len(), self.last_scan)
    }
}

/// The file index being read from disk at start, on a thread of its own
/// (28 Sep 2026).
///
/// It was read inside `Daemon::new`, before the hub was bound: a person's
/// Desktop, Documents and Downloads are tens of thousands of entries and
/// tens of megabytes of text, and every one of them was parsed before the
/// hub could answer its first page. Now it is read beside everything else,
/// and until it arrives the index is empty and says so (`is_loading`):
/// "nothing matches" and "I haven't finished reading the list yet" are
/// different answers.
pub struct Loading {
    rx: Option<std::sync::mpsc::Receiver<Index>>,
}

/// The saved index, with a plain reason on it if the file had to be set aside
/// while reading (damaged, locked, or too big): `Index::load` answers an empty
/// index for those, and an empty index must not pass for "no files".
fn read_saved_index(store: &Store) -> Index {
    let began = now();
    let mut index = Index::load(store);
    if let Some(aside) = crate::store::set_aside_since(store, began)
        .into_iter()
        .find(|s| s.name == "index")
    {
        index.missed.not_loaded = Some(match aside.why.as_str() {
            "oversized" => "the saved list had grown too big to read".to_string(),
            "unreadable" => "the saved list was damaged or locked".to_string(),
            other => format!("the saved list was set aside: {other}"),
        });
    }
    index
}

/// What `Loading::settle` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// Still being read.
    StillLoading,
    /// It arrived and is now the index; this is what is on disk.
    Took((usize, u64)),
    /// It arrived, but the index in memory had already been filled another
    /// way (a scan, or a test setting it), which is newer: that one is kept.
    KeptNewer,
    /// Nothing was being read.
    Ready,
}

impl Loading {
    /// Start reading `store`'s index on its own thread.
    pub fn start(store: &Store) -> Loading {
        let (tx, rx) = std::sync::mpsc::channel();
        let theirs = store.clone();
        let started = std::thread::Builder::new()
            .name("atlas-index-load".into())
            .spawn(move || {
                let index = read_saved_index(&theirs);
                crate::heard!(tx
                    .send(index)
                    .map_err(|_| "nothing was waiting for the saved file index any more"));
            });
        match started {
            Ok(_) => Loading { rx: Some(rx) },
            // No thread: read it here, as before, rather than not at all.
            Err(_) => {
                let (tx, rx) = std::sync::mpsc::channel();
                crate::heard!(tx
                    .send(read_saved_index(store))
                    .map_err(|_| "nothing was waiting for the saved file index"));
                Loading { rx: Some(rx) }
            }
        }
    }

    /// A read that finishes only when the test sends the index.
    #[doc(hidden)]
    pub fn held_for_test() -> (Loading, std::sync::mpsc::Sender<Index>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Loading { rx: Some(rx) }, tx)
    }

    pub fn is_loading(&self) -> bool {
        self.rx.is_some()
    }

    /// Take the index if it has been read. It replaces `current` only while
    /// `current` is untouched (nothing in it and never scanned); otherwise
    /// what is in memory is newer and stays.
    pub fn settle(&mut self, current: &mut Index) -> Settled {
        let Some(rx) = &self.rx else {
            return Settled::Ready;
        };
        let loaded = match rx.try_recv() {
            Ok(i) => i,
            Err(std::sync::mpsc::TryRecvError::Empty) => return Settled::StillLoading,
            // The reading thread ended without an answer. Not "no files": say
            // so, where a search will read it (21 Oct 2026).
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
                if current.written_as() == (0, 0) {
                    current.missed.not_loaded =
                        Some("the part of me that reads it stopped before it finished".into());
                    crate::heard!(Err::<(), _>(
                        "the file index load thread ended without an answer"
                    ));
                }
                return Settled::Ready;
            }
        };
        self.rx = None;
        if current.written_as() == (0, 0) {
            *current = loaded;
            Settled::Took(current.written_as())
        } else {
            Settled::KeptNewer
        }
    }

    /// `settle`, waiting up to `max` for the read to finish.
    pub fn wait(&mut self, current: &mut Index, max: std::time::Duration) -> Settled {
        let until = std::time::Instant::now() + max;
        loop {
            let s = self.settle(current);
            if s != Settled::StillLoading || std::time::Instant::now() >= until {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

/// What to say when a search finds nothing while the index is still being
/// read (`Loading`).
pub const STILL_READING: &str =
    "I'm still reading the list of your files from when I last looked -- \
     ask again in a few seconds and I'll have all of it.";

fn walk(
    dir: &Path,
    cfg: &IndexConfig,
    depth: u32,
    out: &mut BTreeMap<String, Entry>,
    missed: &mut Missed,
) {
    if depth > cfg.max_depth {
        missed.too_deep += 1;
        missed.note(dir);
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        // The big one. A permission-denied folder used to take its whole tree
        // out of the index with nothing said, and the search that followed
        // looked like a definite "not here".
        missed.unreadable_dirs += 1;
        missed.note(dir);
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else {
            missed.unreadable_files += 1;
            missed.note(&p);
            continue;
        };
        if ft.is_symlink() {
            continue; // symlink loops are how an indexer hangs forever
        }
        if ft.is_dir() {
            if !cfg.excluded_dir(&name) {
                walk(&p, cfg, depth + 1, out, missed);
            }
            continue;
        }
        let ext = p
            .extension()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if cfg.excluded_ext(&ext) {
            continue;
        }
        let Ok(md) = e.metadata() else {
            missed.unreadable_files += 1;
            missed.note(&p);
            continue;
        };
        let stamp = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        if stamp.is_none() {
            missed.undated += 1;
            missed.note(&p);
        }
        let modified = stamp.unwrap_or(0);
        let key = p.to_string_lossy().to_string();
        out.insert(
            key.clone(),
            Entry {
                path: key,
                name,
                ext: ext.clone(),
                size: md.len(),
                modified,
                class: AssetClass::of(&ext),
            },
        );
    }
}

pub fn default_roots() -> Vec<PathBuf> {
    ["USERPROFILE", "HOME"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .flat_map(|h| {
            ["Desktop", "Documents", "Downloads"]
                .iter()
                .map(|d| PathBuf::from(&h).join(d))
                .collect::<Vec<_>>()
        })
        .filter(|p| p.is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_index_past_the_default_record_budget_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("atlas-index-large-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);

        let mut index = Index {
            last_scan: 1_792_000_000,
            ..Index::default()
        };
        // Long paths, so a few thousand entries clear 16 MiB even compact.
        let long = "d".repeat(3000);
        for i in 0..6000u64 {
            let path = format!("C:/Users/someone/{long}/file-{i:06}.txt");
            index.entries.insert(
                path.clone(),
                Entry {
                    path,
                    name: format!("file-{i:06}.txt"),
                    ext: "txt".to_string(),
                    size: i,
                    modified: 1_700_000_000 + i,
                    class: AssetClass::Document,
                },
            );
        }
        let pretty = serde_json::to_string_pretty(&index).unwrap();
        let compact = serde_json::to_string(&index).unwrap();
        assert!(compact.len() as u64 > 16 * 1024 * 1024, "{}", compact.len());
        assert!(pretty.len() > compact.len());

        index.save(&store).unwrap();
        let back = Index::load(&store);

        assert_eq!(back.entries, index.entries);
        assert_eq!(back.last_scan, index.last_scan);
        assert!(store.preserved().is_empty(), "{:?}", store.preserved());
        assert!(crate::store::take_failed_saves(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_load_thread_that_died_says_so_instead_of_an_empty_ready_index() {
        let (mut loading, tx) = Loading::held_for_test();
        drop(tx);
        let mut index = Index::default();
        assert_eq!(loading.settle(&mut index), Settled::Ready);
        let said = index.missed.caveat().expect("a caveat, not silence");
        assert!(said.contains("didn't load"), "{said}");
        assert!(said.contains("rebuild the index"), "{said}");
    }

    #[test]
    fn a_saved_index_that_cannot_be_read_is_reported_when_it_is_loaded() {
        let dir = std::env::temp_dir().join(format!("atlas-index-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        std::fs::write(dir.join("index.json"), "{ this is not an index").unwrap();
        let mut loading = Loading::start(&store);
        let mut index = Index::default();
        loading.wait(&mut index, std::time::Duration::from_secs(10));
        let said = index.missed.caveat().expect("a caveat, not an empty Ready");
        assert!(said.contains("didn't load"), "{said}");
        assert!(said.contains("rebuild the index"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fresh_walk_clears_the_didnt_load_note() {
        let mut index = Index::default();
        index.missed.not_loaded = Some("test".into());
        index.apply(Index::default());
        assert!(index.missed.caveat().is_none());
    }
}
