//! Knowing the map without carrying the territory.
//!
//! A memory folder that has grown for a year cannot be read at boot. Atlas
//! either loads all of it — slow, and most of it irrelevant to whatever you
//! just asked — or loads none of it and does not know what it has.
//!
//! The way out is an index note in every folder, a bullet index at the top of
//! every daily note, and one master index at the root outside every folder.
//! **At boot Atlas reads the master index and nothing else.** It learns what
//! exists and where, and loads a note only when a task needs it.
//!
//! The difference is between reading forty notes to find one fact and reading
//! one line to know which note holds it.
//!
//! Two rules stop this decaying into a second thing to maintain:
//!
//! * **One line per item, and the line says what is inside, not what it is
//!   called.** "spain-trip — flights booked, hotel undecided" is worth reading.
//!   "spain-trip.md" is the filename again.
//! * **An index that disagrees with its folder is worse than none**, because
//!   Atlas trusts it and stops looking. `drift` finds that, and a stale index
//!   is a fault rather than a detail.

use serde::{Deserialize, Serialize};

/// One line in an index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// The note or folder this points at.
    pub name: String,
    /// What is inside it, in one line. Never the name again.
    pub says: String,
    /// True when this points at a folder that has its own index.
    pub is_folder: bool,
}

impl Line {
    pub fn new(name: &str, says: &str) -> Line {
        Line { name: name.into(), says: says.into(), is_folder: false }
    }

    pub fn folder(name: &str, says: &str) -> Line {
        Line { name: name.into(), says: says.into(), is_folder: true }
    }

    /// A line that only restates the filename tells you nothing you did not
    /// have from the directory listing.
    pub fn is_useful(&self) -> bool {
        let says = self.says.trim().to_lowercase();
        let name = self.name.trim().to_lowercase().replace(['-', '_'], " ");
        let bare = says.trim_end_matches(".md");
        !says.is_empty() && bare != name && says.len() > name.len() + 4
    }
}

/// The index for one folder, or for the whole store.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contents {
    /// Folder this describes. Empty string is the root.
    pub folder: String,
    pub lines: Vec<Line>,
}

/// The most lines an index may carry before it needs splitting into folders.
///
/// Past this it is a list you scroll rather than a map you read, which is the
/// problem it was built to solve.
pub const MAX_LINES: usize = 40;

impl Contents {
    pub fn new(folder: &str) -> Contents {
        Contents { folder: folder.into(), lines: Vec::new() }
    }

    pub fn add(&mut self, l: Line) {
        self.lines.retain(|x| x.name != l.name);
        self.lines.push(l);
        self.lines.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn needs_splitting(&self) -> bool {
        self.lines.len() > MAX_LINES
    }

    /// Which lines say nothing the filename did not already say.
    pub fn useless_lines(&self) -> Vec<&Line> {
        self.lines.iter().filter(|l| !l.is_useful()).collect()
    }

    /// The one thing loaded at boot.
    pub fn as_markdown(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            let mark = if l.is_folder { "/" } else { "" };
            out.push_str(&format!("- {}{} — {}\n", l.name, mark, l.says));
        }
        out
    }
}

/// Words that appear in every description and so match everything.
///
/// Without this, "when is the hotel booked for spain" matches any line
/// containing "the", which is all of them — and an index that opens
/// everything is the folder it replaced.
const COMMON: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "what", "when", "where", "who", "why", "how",
    "was", "are", "been", "have", "has", "did", "does", "not", "any", "all", "some", "into",
    "from", "about", "over", "under", "out", "off", "its", "it's", "you", "your", "yours",
];

/// What a query should load, given only the master index.
///
/// Returns the names worth opening, best first. Empty means the index has
/// nothing on the subject — which is a real answer and better than opening
/// everything on the chance something matches.
pub fn what_to_open(c: &Contents, query: &str) -> Vec<String> {
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.len() > 2 && !COMMON.contains(&w.as_str()))
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, &Line)> = c
        .lines
        .iter()
        .map(|l| {
            let hay = format!("{} {}", l.name.to_lowercase(), l.says.to_lowercase());
            (words.iter().filter(|w| hay.contains(w.as_str())).count(), l)
        })
        .filter(|(n, _)| *n > 0)
        .collect();
    // Most matches first; ties broken by name so two runs of the same query
    // load the same notes in the same order.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
    scored.into_iter().map(|(_, l)| l.name.clone()).collect()
}

/// Where an index and its folder disagree.
///
/// An index Atlas trusts and that is wrong is worse than no index, because it
/// stops looking. Both directions are faults: something on disk the index
/// never mentions is invisible, and something the index promises that is not
/// there is a dead end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drift {
    /// On disk, absent from the index. Atlas cannot find these at all.
    pub unlisted: Vec<String>,
    /// In the index, absent from disk. Atlas will go looking and find nothing.
    pub missing: Vec<String>,
}

impl Drift {
    pub fn is_clean(&self) -> bool {
        self.unlisted.is_empty() && self.missing.is_empty()
    }

    pub fn plain(&self) -> String {
        if self.is_clean() {
            return "The index matches what's there.".into();
        }
        let mut parts = Vec::new();
        if !self.unlisted.is_empty() {
            parts.push(format!(
                "{} note{} I can't find from the index",
                self.unlisted.len(),
                if self.unlisted.len() == 1 { "" } else { "s" }
            ));
        }
        if !self.missing.is_empty() {
            parts.push(format!(
                "{} the index promises that aren't there",
                self.missing.len()
            ));
        }
        parts.join(", ")
    }
}

/// Compare an index against what is actually in the folder.
pub fn drift(c: &Contents, on_disk: &[String]) -> Drift {
    let listed: std::collections::BTreeSet<&str> =
        c.lines.iter().map(|l| l.name.as_str()).collect();
    let actual: std::collections::BTreeSet<&str> = on_disk.iter().map(|s| s.as_str()).collect();
    Drift {
        unlisted: actual.difference(&listed).map(|s| s.to_string()).collect(),
        missing: listed.difference(&actual).map(|s| s.to_string()).collect(),
    }
}

/// What Atlas loads at boot: the master index, and a count of what it did not
/// load, so the saving is visible rather than assumed.
#[derive(Debug, Clone, PartialEq)]
pub struct Boot {
    pub loaded: usize,
    pub known_of: usize,
}

impl Boot {
    pub fn plain(&self) -> String {
        format!(
            "I know about {} thing{} and have {} open.",
            self.known_of,
            if self.known_of == 1 { "" } else { "s" },
            self.loaded
        )
    }
}

/// Boot from the master index alone.
///
/// `loaded` is one — the index itself. Anything higher means something is
/// eagerly loading notes again, which is the whole problem returning.
pub fn boot(master: &Contents) -> Boot {
    Boot { loaded: 1, known_of: master.lines.len() }
}

// ---------------------------------------------------------------------------
// The half that was missing.
//
// Everything above this line is the shape of an index: what a line is, what
// makes one worth reading, how to search one, how to tell when it has gone
// wrong. All of it was complete, tested, and unreachable, because nothing in
// the program ever built a `Contents` out of a real folder. An index nothing
// writes cannot drift, so `nudge::drifted` had nothing to be raised about.
//
// What follows is the territory half: reading a folder, summarising what is
// in it, writing the master index down, and reading it back.
// ---------------------------------------------------------------------------

use std::path::{Path, PathBuf};

/// The master index's filename.
///
/// It lives *beside* the folder it describes, not inside it, for the reason
/// in the module header: it is the one thing read at boot, so it must be
/// findable without first listing the thing it exists to save you listing.
/// Keeping it outside also keeps it from indexing itself.
pub const MASTER_FILE: &str = "index.md";

/// A line nobody should hand-edit, said in the file itself.
///
/// The index is rebuilt from the folder on request, so an edit here is lost
/// at the next rebuild. Better to say that in the file than to let someone
/// find it out.
pub const HEADER: &str = "\
# What Atlas knows it has

Built from the notes folder. Rebuilt on request, so edits here do not last —
change the note, not this line.
";

/// Where the master index for `notes_dir` lives.
///
/// Sibling of the folder, so `data/notes` is described by `data/index.md`.
/// A folder with no parent (bare `notes`) puts it in the working directory,
/// which is the same relationship one level up.
pub fn master_path(notes_dir: &Path) -> PathBuf {
    match notes_dir.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(MASTER_FILE),
        _ => PathBuf::from(MASTER_FILE),
    }
}

/// What is actually in the folder, by the names an index uses.
///
/// Notes are named by their stem (`spain-trip`, not `spain-trip.md`) because
/// that is what a person calls a note. Sub-folders are named as they are.
/// Anything that is neither a `.md` file nor a directory is not something the
/// index claims to describe, so it is not drift when it is absent from one.
pub fn names_on_disk(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if let Some(n) = path.file_name().and_then(|n| n.to_str()) {
                out.push(n.to_string());
            }
            continue;
        }
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        // The master index sits outside the folder, but a copy left inside
        // one must never be listed as a note about itself.
        if path.file_name().and_then(|n| n.to_str()) == Some(MASTER_FILE) {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            out.push(stem.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The longest a line may run before it stops being a line you scan.
pub const MAX_SAYS_CHARS: usize = 110;

/// What a note is about, from the note itself.
///
/// Returns the empty string when the note has no prose in it — deliberately,
/// rather than falling back to the title. A line that restates the filename
/// is the thing `is_useful` exists to reject, and inventing one here would
/// defeat the check that catches it. An empty line shows up in
/// `useless_lines()`, which is the honest outcome: Atlas could not say what
/// is in this note.
pub fn says_for(text: &str) -> String {
    let mut best = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("- ") {
            continue;
        }
        // A URL on its own is a source, not a description.
        if line.starts_with("http://") || line.starts_with("https://") {
            continue;
        }
        if line.split_whitespace().count() < 3 {
            continue;
        }
        best = line.to_string();
        break;
    }
    if best.is_empty() {
        return best;
    }
    // One sentence is the unit. Past that it is the note again, and the whole
    // point is not to read the note.
    if let Some(end) = best.find(". ") {
        best.truncate(end + 1);
    }
    // The markdown format is one line per item, so a description carrying a
    // newline would produce an index that does not parse back.
    let mut said: String = best.split_whitespace().collect::<Vec<_>>().join(" ");
    if said.chars().count() > MAX_SAYS_CHARS {
        let cut = said
            .char_indices()
            .take(MAX_SAYS_CHARS)
            .map(|(i, _)| i)
            .last()
            .unwrap_or(0);
        said.truncate(cut);
        // Back off to a word boundary so a line never ends mid-word.
        if let Some(space) = said.rfind(' ') {
            said.truncate(space);
        }
        said.push('…');
    }
    said
}

/// The line describing one note, read off disk.
fn line_for(dir: &Path, name: &str) -> Line {
    let path = dir.join(format!("{name}.md"));
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    Line::new(name, &says_for(&text))
}

/// The line describing one sub-folder.
///
/// A folder's own index is what should describe it; until it has one, the
/// count is the most that can be said truthfully, and saying how much is in
/// there is still worth more than repeating its name.
fn line_for_folder(dir: &Path, name: &str) -> Line {
    let inside = names_on_disk(&dir.join(name)).len();
    Line::folder(
        name,
        &format!("{} note{} inside", inside, if inside == 1 { "" } else { "s" }),
    )
}

/// Build the index for a folder by reading it.
///
/// This is the function whose absence made everything above unreachable.
pub fn from_folder(dir: &Path) -> Contents {
    let mut c = Contents::new(&dir.to_string_lossy());
    for name in names_on_disk(dir) {
        if dir.join(&name).is_dir() {
            c.add(line_for_folder(dir, &name));
        } else {
            c.add(line_for(dir, &name));
        }
    }
    c
}

/// Read an index back from the markdown it was written as.
///
/// The inverse of `as_markdown`, and it has to stay that way: the index is
/// written to disk and read back on the next boot, so anything that does not
/// survive the round trip reads as drift that never happened.
pub fn parse(folder: &str, md: &str) -> Contents {
    let mut c = Contents::new(folder);
    for raw in md.lines() {
        let line = raw.trim();
        let Some(rest) = line.strip_prefix("- ") else { continue };
        // The em dash is the separator `as_markdown` writes. A bullet without
        // one is prose in the file, not a line of the index.
        //
        // A note Atlas could not summarise is written with nothing after the
        // dash, and trimming the line leaves no trailing space to split on.
        // Splitting alone dropped those lines entirely, so a blank summary
        // came back as a note that had vanished — drift that never happened,
        // reported against the one thing this file must never be wrong about.
        let (name, says) = match rest.split_once(" — ") {
            Some(pair) => pair,
            None => match rest.strip_suffix(" —") {
                Some(name) => (name, ""),
                None => continue,
            },
        };
        let name = name.trim();
        let says = says.trim();
        if let Some(folder_name) = name.strip_suffix('/') {
            c.add(Line::folder(folder_name, says));
        } else {
            c.add(Line::new(name, says));
        }
    }
    c
}

/// Write the master index where boot will look for it.
pub fn save(c: &Contents, at: &Path) -> crate::error::Result<()> {
    if let Some(parent) = at.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    crate::store::write_owned_file(at, format!("{}\n{}", HEADER, c.as_markdown()).as_bytes())?;
    Ok(())
}

/// Read the master index, or `None` if there is not one yet.
///
/// `None` is not drift. A folder that has never been indexed disagrees with
/// nothing; it is only once an index exists and is trusted that being wrong
/// starts to cost anything.
pub fn load(folder: &str, at: &Path) -> Option<Contents> {
    let md = std::fs::read_to_string(at).ok()?;
    Some(parse(folder, &md))
}

/// Rebuild the index from the folder and write it down.
///
/// What the drift nudge offers to do.
pub fn rebuild(dir: &Path) -> crate::error::Result<Contents> {
    let c = from_folder(dir);
    save(&c, &master_path(dir))?;
    Ok(c)
}
