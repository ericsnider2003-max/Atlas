//! **The floor (20 Oct 2026): what Atlas does with no language model loaded.**
//!
//! The owner's requirement: Atlas never becomes useless when memory is low,
//! even offline. The everyday commands -- open and close an app, make, read
//! and move a file, back up a folder, timers, the time, "what's using my
//! memory" -- are read from the words by the phrase parser and done by plain
//! code; none of them needs a model to understand or to carry out. The free
//! online model that was answering "Create a text file ..." with "On it" and
//! making nothing is exactly what this removes from the path.
//!
//! This module is the plain half, with no daemon in it: reading the shapes of
//! sentence the table's prefix phrases cannot (`read_spoken`), doing each job
//! and checking its own result (`create_file`, `read_file`, `move_file`,
//! `plan_backup` and `run_backup`), reading the machine's memory
//! (`biggest`, `memory_said`), and deciding what to say when a question needs
//! the model and the memory to load it isn't there (`decide`, `say`). The
//! daemon's half is `daemon/floor.rs`: jobs recorded, follow-ups kept,
//! errands started.
//!
//! Every job returns a `Done`: whether it worked, and the words, written from
//! what was read back afterwards -- never from what was hoped.

use crate::intent::Intent;
use std::path::{Path, PathBuf};

/// How a job ended, in words that are true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub ok: bool,
    pub said: String,
}

impl Done {
    pub fn yes(said: impl Into<String>) -> Done {
        Done {
            ok: true,
            said: said.into(),
        }
    }
    pub fn no(said: impl Into<String>) -> Done {
        Done {
            ok: false,
            said: said.into(),
        }
    }
}

// ------------------------------------------------------------ reading the words

/// Leading words that change nothing: "please", "atlas", "ok".
fn strip_leading(input: &str) -> &str {
    let mut rest = input.trim();
    loop {
        let (word, tail) = match rest.split_once(char::is_whitespace) {
            Some((w, t)) => (w, t),
            None => return rest,
        };
        let bare = word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        if matches!(
            bare.as_str(),
            "please" | "atlas" | "hey" | "ok" | "okay" | "just" | "kindly"
        ) {
            rest = tail.trim_start();
        } else {
            return rest;
        }
    }
}

/// A word that is a file's name: letters or digits, a dot, then an extension
/// with a letter in it ("atlas-test.txt", "notes.md"). "3.5" and "v2.0" are
/// not, and neither is an address.
fn is_file_name(token: &str) -> bool {
    let t = token.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | '(' | ')'));
    let t = t.trim_end_matches(['.', '!', '?']);
    if t.starts_with("http") || t.contains("://") || t.contains('@') {
        return false;
    }
    let name = t.rsplit(['\\', '/']).next().unwrap_or(t);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && stem.chars().any(|c| c.is_alphabetic())
        && (1..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic())
}

/// The extensions of files read as plain text.
const TEXTY: &[&str] = &[
    "txt", "md", "log", "json", "csv", "xml", "yaml", "yml", "toml", "ini", "cfg", "conf", "rs",
    "py", "js", "html", "css", "bat", "ps1", "sh", "tsv", "rtf",
];

fn is_text_file_name(token: &str) -> bool {
    let t = token
        .trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | '(' | ')'))
        .trim_end_matches(['.', '!', '?']);
    is_file_name(t)
        && t.rsplit_once('.')
            .is_some_and(|(_, e)| TEXTY.contains(&e.to_ascii_lowercase().as_str()))
}

/// "create a text file named x.txt in my Desktop folder ..." and its kin.
fn opens_a_file(w: &[&str]) -> bool {
    if !matches!(
        w.first().copied(),
        Some("create" | "make" | "write" | "save")
    ) {
        return false;
    }
    // "create notes.txt on my desktop"
    if w.get(1).is_some_and(|t| is_text_file_name(t)) {
        return true;
    }
    let Some(at) = w.iter().skip(1).take(6).position(|t| *t == "file") else {
        return false;
    };
    let at = at + 1;
    const FILLER: &[&str] = &[
        "a", "an", "the", "new", "text", "txt", "plain", "empty", "notes", "note", "markdown",
        "md", "small", "simple", "quick", "blank",
    ];
    if !w[1..at].iter().all(|t| FILLER.contains(t)) {
        return false;
    }
    match w.get(at + 1) {
        None => true,
        Some(n) => {
            matches!(
                *n,
                "named"
                    | "called"
                    | "titled"
                    | "in"
                    | "on"
                    | "at"
                    | "into"
                    | "to"
                    | "with"
                    | "containing"
                    | "saying"
                    | "inside"
            ) || (
                // "that says ...", not "that renames photos" (a build request).
                matches!(*n, "that" | "which")
                    && matches!(
                        w.get(at + 2).copied(),
                        Some("says" | "contains" | "reads" | "has")
                    )
            )
        }
    }
}

fn reads_a_file(w: &[&str], lower: &str) -> bool {
    let opener = matches!(
        w.first().copied(),
        Some("read" | "show" | "cat" | "print" | "display")
    ) || lower.starts_with("what's in ")
        || lower.starts_with("whats in ")
        || lower.starts_with("what is in ")
        || (lower.starts_with("what does ")
            && lower.trim_end_matches(['?', '.']).ends_with(" say"));
    opener && w.iter().skip(1).any(|t| is_text_file_name(t))
}

fn moves_a_file(w: &[&str]) -> bool {
    w.first() == Some(&"move")
        && w.iter().skip(1).any(|t| *t == "to" || *t == "into")
        && w.iter().skip(1).any(|t| is_file_name(t))
}

/// Names a folder in a way a person means a real one: a path, or one of the
/// folders called by name, or a word "folder"/"directory".
fn names_a_folder(lower: &str) -> bool {
    let has_path = lower.contains(":\\")
        || lower.contains(":/")
        || lower.contains("\\\\")
        || lower
            .split_whitespace()
            .any(|t| t.starts_with('~') || t.starts_with('/'));
    let words: Vec<&str> = lower.split_whitespace().collect();
    let by_name = words.iter().any(|t| {
        matches!(
            t.trim_matches(|c: char| !c.is_alphanumeric()),
            "documents" | "desktop" | "downloads" | "pictures" | "photos" | "videos" | "music"
        )
    });
    has_path || by_name || words.iter().any(|t| matches!(*t, "folder" | "directory"))
}

fn backs_up_a_folder(w: &[&str], lower: &str) -> bool {
    let rest_at = match (w.first().copied(), w.get(1).copied()) {
        (Some("back"), Some("up")) => 2,
        (Some("backup"), _) => 1,
        _ => return false,
    };
    if w.len() <= rest_at {
        return false;
    }
    // Atlas's own state keeps its own command ("back up", "back everything
    // up", "back up atlas").
    let rest = w[rest_at..].join(" ");
    if matches!(
        rest.as_str(),
        "atlas" | "everything" | "my stuff" | "all of it" | "now"
    ) {
        return false;
    }
    names_a_folder(&lower[lower.find(w[rest_at]).unwrap_or(0)..])
}

/// A sentence the table's prefix phrases cannot read: the shapes that put
/// the file or the folder after a verb and the place after a "to". Read from
/// the words alone -- no model -- and returned with the name of the command
/// it is, the way `workday::read_first` is. `None` for ordinary talk.
pub(crate) fn read_spoken(input: &str) -> Option<(Intent, &'static str)> {
    let said = strip_leading(input);
    let lower = said.to_lowercase();
    let w: Vec<&str> = lower.split_whitespace().collect();
    // A bare "quit" (with "please"/"now" after it) stops Atlas, the same as
    // the tray's Quit. It can't be a phrase of `quit_atlas` as well as of
    // `close_app` ("quit chrome"), and no two commands may share a phrase.
    if w.first() == Some(&"quit")
        && w[1..].iter().all(|t| {
            matches!(
                t.trim_matches(|c: char| !c.is_alphanumeric()),
                "please" | "now" | "atlas" | "thanks"
            )
        })
    {
        return Some((Intent::QuitAtlas, "quit_atlas"));
    }
    // "Is the backup finished?" is a question about a job, answered from the
    // job records (`status_question`). It rides the floor's read command: it
    // reads, changes nothing, and is already kept from a model's choice.
    if status_question(said).is_some() {
        return Some((Intent::ReadFile(said.to_string()), "read_file"));
    }
    if opens_a_file(&w) {
        return Some((Intent::CreateFile(said.to_string()), "create_file"));
    }
    if moves_a_file(&w) {
        return Some((Intent::MoveFile(said.to_string()), "move_file"));
    }
    if reads_a_file(&w, &lower) {
        return Some((Intent::ReadFile(said.to_string()), "read_file"));
    }
    if backs_up_a_folder(&w, &lower) {
        return Some((Intent::BackUpFolder(said.to_string()), "back_up_folder"));
    }
    None
}

// ---------------------------------------------------------------- word cutting

/// Byte position of `needle` in `lower` at word boundaries (ASCII lowercase,
/// so positions are the same in the original).
fn find_word(lower: &str, needle: &str, from: usize) -> Option<usize> {
    let mut at = from;
    while let Some(p) = lower.get(at..)?.find(needle) {
        let start = at + p;
        let end = start + needle.len();
        let before = lower[..start].chars().next_back();
        let after = lower[end..].chars().next();
        let edge = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
        if (edge(before) || needle.starts_with(' ')) && (edge(after) || needle.ends_with(' ')) {
            return Some(start);
        }
        at = start + 1;
    }
    None
}

/// The earliest of `needles` in `lower` at or after `from`: (position, length).
fn earliest(lower: &str, needles: &[&str], from: usize) -> Option<(usize, usize)> {
    needles
        .iter()
        .filter_map(|n| find_word(lower, n, from).map(|p| (p, n.len())))
        .min_by_key(|(p, _)| *p)
}

/// One token from `at`: quoted whole, or up to the next space; trailing
/// sentence punctuation off.
fn token_at(raw: &str, at: usize) -> Option<String> {
    let rest = raw.get(at..)?.trim_start();
    let first = rest.chars().next()?;
    let tok = if matches!(first, '"' | '\'' | '\u{201c}' | '\u{2018}') {
        let close = match first {
            '\u{201c}' => '\u{201d}',
            '\u{2018}' => '\u{2019}',
            c => c,
        };
        let inner = &rest[first.len_utf8()..];
        inner.split(close).next()?.to_string()
    } else {
        rest.split_whitespace().next()?.to_string()
    };
    let tok = tok.trim_end_matches([',', ';', '!', '?']).to_string();
    let tok = tok.trim_end_matches('.').to_string();
    (!tok.is_empty()).then_some(tok)
}

/// Trim a phrase for use as a folder name: quotes, ends, "my"/"the" kept for
/// `organize::named_folder` to read.
fn folder_words(s: &str) -> String {
    s.trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '.' | ',' | ';' | '!' | '?'))
        .trim()
        .to_string()
}

/// What to write in the file, as asked: the words after "containing",
/// "saying" and the like, quotes taken off, "today's date" made the date.
fn contents_asked(raw: &str, lower: &str, today: &str) -> String {
    const MARK: &[&str] = &[
        " containing ",
        " that contains ",
        " that says ",
        " which says ",
        " saying ",
        " with the text ",
        " with text ",
        " with the contents ",
        " with contents ",
        " with the words ",
        " with the line ",
        " with ",
    ];
    let Some((p, n)) = earliest(lower, MARK, 0) else {
        return String::new();
    };
    let start = p + n;
    let tail = &raw[start..];
    let first = tail.trim_start().chars().next();
    let body = if let Some(q) = first.filter(|c| matches!(c, '"' | '\'' | '\u{201c}')) {
        let close = if q == '\u{201c}' { '\u{201d}' } else { q };
        let inner = &tail.trim_start()[q.len_utf8()..];
        inner.split(close).next().unwrap_or("").to_string()
    } else {
        const END: &[&str] = &[
            ", then ",
            " then ",
            ", and then ",
            " and then ",
            " and verify ",
            ", verify ",
            " and check ",
            ", check ",
            " and read ",
            ", and ",
        ];
        let tl = lower[start..].to_string();
        let cut = earliest(&tl, END, 0).map(|(p, _)| p).unwrap_or(tl.len());
        tail[..cut]
            .trim()
            .trim_end_matches(['.', ',', ';'])
            .to_string()
    };
    let body = body.trim().to_string();
    // "the current date": the date, not those words.
    let low = body.to_ascii_lowercase();
    for phrase in [
        "today's date",
        "todays date",
        "the current date",
        "current date",
        "the date",
        "today",
    ] {
        if let Some(i) = low.find(phrase) {
            return format!("{}{}{}", &body[..i], today, &body[i + phrase.len()..]);
        }
    }
    body
}

/// What was asked of "create a file ...".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateSpec {
    pub name: String,
    /// The folder, in the words used ("my Desktop folder").
    pub folder: Option<String>,
    pub contents: String,
    /// Said outright that an existing file may be replaced.
    pub replace: bool,
}

/// Read "create a text file named x.txt in my Desktop folder containing ...".
/// `today` is what "today's date" becomes. The error is a question to ask.
pub fn read_create(raw: &str, today: &str) -> Result<CreateSpec, String> {
    let said = strip_leading(raw);
    let lower = said.to_ascii_lowercase();
    let named = earliest(&lower, &[" named ", " called ", " titled "], 0);
    let mut name = named.and_then(|(p, n)| token_at(said, p + n));
    if name.is_none() {
        name = said.split_whitespace().find(|t| is_file_name(t)).map(|t| {
            t.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';'))
                .trim_end_matches(['.', '!', '?'])
                .to_string()
        });
    }
    let Some(mut name) = name else {
        return Err(
            "What should the file be called, and where should it go? For example: \
                    create a text file named notes.txt in my Documents folder containing hello."
                .into(),
        );
    };
    if !name.contains('.') && lower.contains("text file") {
        name.push_str(".txt");
    }
    // Where: the earliest "in/on/into ..." before what it should hold.
    let content_at = earliest(
        &lower,
        &[
            " containing ",
            " that contains ",
            " that says ",
            " which says ",
            " saying ",
            " with the text ",
            " with text ",
            " with the contents ",
            " with contents ",
            " with the words ",
            " with ",
        ],
        0,
    )
    .map(|(p, _)| p)
    .unwrap_or(lower.len());
    let name_end = named.map(|(p, n)| p + n).unwrap_or(0);
    let head = &lower[..content_at.max(name_end).min(lower.len())];
    let folder = earliest(
        head,
        &[" in ", " on ", " into ", " inside ", " at ", " to "],
        name_end.min(head.len()),
    )
    .map(|(p, n)| {
        let after = &said[p + n..head.len().max(p + n)];
        // Up to where the next clause starts.
        let al = after.to_ascii_lowercase();
        let cut = earliest(&al, &[" and ", " then ", ",", ";"], 0)
            .map(|(c, _)| c)
            .unwrap_or(al.len());
        folder_words(&after[..cut])
    })
    .filter(|f| !f.is_empty());
    let replace = [
        "overwrite",
        "replace it if",
        "replace the existing",
        "even if it exists",
    ]
    .iter()
    .any(|m| lower.contains(m));
    Ok(CreateSpec {
        name,
        folder,
        contents: contents_asked(said, &lower, today),
        replace,
    })
}

/// A file named, and where to look for it ("read atlas-test.txt on my
/// Desktop"): the name, and the folder in the words used.
pub(crate) fn read_file_ask(raw: &str) -> Option<(String, Option<String>)> {
    let said = strip_leading(raw);
    let lower = said.to_ascii_lowercase();
    let name = said.split_whitespace().find(|t| is_file_name(t)).map(|t| {
        t.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';'))
            .trim_end_matches(['.', '!', '?'])
            .to_string()
    })?;
    let after_name = lower
        .find(&name.to_ascii_lowercase())
        .map(|p| p + name.len())
        .unwrap_or(0);
    let folder = earliest(
        &lower,
        &[" on ", " in ", " from ", " inside ", " under "],
        after_name,
    )
    .map(|(p, n)| folder_words(&said[p + n..]))
    .filter(|f| !f.is_empty());
    Some((name, folder))
}

/// "move x.txt from my Desktop to my Documents folder": the file, where it is
/// (if said), where it goes.
pub fn read_move(raw: &str) -> Option<(String, Option<String>, String)> {
    let said = strip_leading(raw);
    let lower = said.to_ascii_lowercase();
    let (p, n) = earliest(&lower, &[" to ", " into "], 0)?;
    // The last "to": a folder may be called "go to".
    let (p, n) = [" to ", " into "]
        .iter()
        .filter_map(|m| lower.rfind(m).map(|p| (p, m.len())))
        .max_by_key(|(p, _)| *p)
        .unwrap_or((p, n));
    let dest = folder_words(&said[p + n..]);
    let left = &said[..p];
    let name = left.split_whitespace().find(|t| is_file_name(t)).map(|t| {
        t.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';'))
            .to_string()
    })?;
    let ll = left.to_ascii_lowercase();
    let at = ll
        .find(&name.to_ascii_lowercase())
        .map(|p| p + name.len())
        .unwrap_or(0);
    let from = earliest(&ll, &[" from ", " on ", " in "], at)
        .map(|(f, fl)| folder_words(&left[f + fl..]))
        .filter(|f| !f.is_empty());
    (!dest.is_empty()).then_some((name, from, dest))
}

/// "back up my Documents folder to D:\Backups": the folder, and where to put
/// the copy if it was said.
pub fn read_backup(raw: &str) -> Option<(String, Option<String>)> {
    let said = strip_leading(raw);
    let lower = said.to_ascii_lowercase();
    let start = if lower.starts_with("back up ") {
        8
    } else if lower.starts_with("backup ") {
        7
    } else {
        return None;
    };
    let rest = &said[start..];
    let rl = rest.to_ascii_lowercase();
    let (folder, dest) = match [" to ", " into ", " onto "]
        .iter()
        .filter_map(|m| rl.find(m).map(|p| (p, m.len())))
        .min_by_key(|(p, _)| *p)
    {
        Some((p, n)) => (&rest[..p], Some(folder_words(&rest[p + n..]))),
        None => (rest, None),
    };
    // "the folder X", "a copy of X", "the contents of X": X is what follows.
    let mut folder = folder.trim();
    loop {
        let low = folder.to_ascii_lowercase();
        let Some(cut) = [
            "a copy of ",
            "the contents of ",
            "of ",
            "the folder ",
            "my folder ",
            "folder ",
            "the directory ",
            "directory ",
        ]
        .iter()
        .find(|p| low.starts_with(**p))
        .map(|p| p.len()) else {
            break;
        };
        folder = folder[cut..].trim();
    }
    let folder = folder_words(folder);
    (!folder.is_empty()).then_some((folder, dest.filter(|d| !d.is_empty())))
}

// ------------------------------------------------------------- finding places

/// Where "my Desktop folder" and the like are on this machine, asked of
/// Windows itself, so a Desktop OneDrive has moved is the one found.
pub fn folder_named(words: &str) -> Option<PathBuf> {
    crate::organize::named_folder(words)
}

/// The places a file is looked for when only its name was said.
fn usual_places() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = ["desktop", "documents", "downloads"]
        .iter()
        .filter_map(|n| crate::organize::user_folder(n))
        .collect();
    if let Some(h) =
        crate::doctor::lookup_env("USERPROFILE").or_else(|| crate::doctor::lookup_env("HOME"))
    {
        v.push(PathBuf::from(h));
    }
    v.dedup();
    v
}

/// The file `name` is: a full path as said, or in `folder`, or the first of
/// the usual places that has it.
pub(crate) fn locate(name: &str, folder: Option<&Path>) -> Result<PathBuf, String> {
    let direct = Path::new(name);
    if direct.is_absolute() {
        return if direct.is_file() {
            Ok(direct.to_path_buf())
        } else {
            Err(format!("There's no file at {}.", direct.display()))
        };
    }
    if let Some(f) = folder {
        let p = f.join(name);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(format!("There's no file called {name} in {}.", f.display()))
        };
    }
    let places = usual_places();
    for f in &places {
        let p = f.join(name);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(format!(
        "I couldn't find a file called {name} in {}. Tell me which folder it's in.",
        places
            .iter()
            .take(3)
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

// -------------------------------------------------------------------- the jobs

fn bytes_said(n: u64) -> String {
    if n >= 10 * 1024 * 1024 {
        format!("{:.1} MB", n as f64 / 1_048_576.0)
    } else if n >= 10 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} byte{}", if n == 1 { "" } else { "s" })
    }
}

fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n).collect::<String>())
    }
}

/// A file name Windows would accept, and nothing that reaches outside the
/// folder.
fn name_problem(name: &str) -> Option<String> {
    if name.is_empty() || name.len() > 120 {
        return Some("that file name is empty or too long".into());
    }
    if name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) || name.contains("..") {
        return Some(format!(
            "\"{name}\" has a character a file name can't have (or reaches outside the folder)"
        ));
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    if matches!(
        stem.as_str(),
        "con" | "prn" | "aux" | "nul" | "com1" | "com2" | "com3" | "lpt1" | "lpt2" | "lpt3"
    ) {
        return Some(format!("\"{name}\" is a name Windows keeps for itself"));
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Some("a file name can't end in a space or a dot".into());
    }
    None
}

/// Make the file in `folder`, then read it back. Never overwrites unless the
/// words said it may.
pub(crate) fn create_file(spec: &CreateSpec, folder: &Path) -> Done {
    if let Some(why) = name_problem(&spec.name) {
        return Done::no(format!("I didn't make it: {why}."));
    }
    if !folder.is_dir() {
        return Done::no(format!(
            "I didn't make it: {} isn't a folder that exists.",
            folder.display()
        ));
    }
    let path = folder.join(&spec.name);
    let bytes = spec.contents.as_bytes();
    let existed = match std::fs::symlink_metadata(&path) {
        Ok(m) => Some(m.len()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Done::no(format!("I couldn't look at {}: {e}.", path.display()));
        }
    };
    if let (Some(len), false) = (existed, spec.replace) {
        return Done::no(format!(
            "{} already exists ({}), so I didn't touch it. Pick another name, or say \"overwrite\" and I'll replace it.",
            path.display(),
            bytes_said(len)
        ));
    }
    // An overwrite goes to a temporary file beside it and is renamed over the
    // old one, so a failure leaves the old file whole and a link is never
    // written through (2026-10-21). A link there is refused, not followed.
    if existed.is_some()
        && std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Done::no(format!(
            "I didn't overwrite {}: it is a link (a shortcut to somewhere else), and writing through it would change a file I wasn't pointed at.",
            path.display()
        ));
    }
    let written = (|| -> std::io::Result<()> {
        use std::io::Write;
        if existed.is_some() {
            return crate::store::write_whole(&path, bytes);
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = written {
        return Done::no(format!(
            "I couldn't make {}: {e}. Nothing was left half-written that I know of.",
            path.display()
        ));
    }
    // Read back, not assumed.
    match std::fs::read(&path) {
        Ok(back) if back == bytes => {
            let shown = if bytes.is_empty() {
                "it's empty, as asked".to_string()
            } else {
                format!("it says: \"{}\"", clip(&spec.contents, 200))
            };
            Done::yes(format!(
                "{} {} ({}). I read it back and {shown}.",
                if existed.is_some() {
                    "Replaced"
                } else {
                    "Created"
                },
                path.display(),
                bytes_said(bytes.len() as u64)
            ))
        }
        Ok(back) => Done::no(format!(
            "I wrote {} but reading it back gave {} instead of {}, so don't trust it.",
            path.display(),
            bytes_said(back.len() as u64),
            bytes_said(bytes.len() as u64)
        )),
        Err(e) => Done::no(format!(
            "I wrote {} but couldn't read it back to check: {e}.",
            path.display()
        )),
    }
}

/// Read a plain-text file: its size, and the start of it.
pub(crate) fn read_file(path: &Path) -> Done {
    use std::io::Read;
    let len = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m.len(),
        Ok(_) => return Done::no(format!("{} isn't a file.", path.display())),
        Err(e) => return Done::no(format!("I couldn't open {}: {e}.", path.display())),
    };
    let mut buf = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(4096).read_to_end(&mut buf));
    if let Err(e) = read {
        return Done::no(format!("I couldn't read {}: {e}.", path.display()));
    }
    let text = match String::from_utf8(buf.clone()) {
        Ok(t) => t,
        // A cut in the middle of a character is still text.
        Err(e) if e.utf8_error().error_len().is_none() => {
            String::from_utf8_lossy(&buf[..e.utf8_error().valid_up_to()]).into_owned()
        }
        Err(_) => {
            return Done::no(format!(
                "{} ({}) doesn't look like plain text, so I won't read it out.",
                path.display(),
                bytes_said(len)
            ))
        }
    };
    if text.trim().is_empty() {
        return Done::yes(format!(
            "{} is {} and has nothing in it.",
            path.display(),
            bytes_said(len)
        ));
    }
    let more = if len > 4096 { " (the start of it)" } else { "" };
    Done::yes(format!(
        "{} is {}{more}. It says: \"{}\"",
        path.display(),
        bytes_said(len),
        clip(&text, 600)
    ))
}

/// Move one file into a folder, never replacing anything, then check: the
/// same bytes are in the new place and the old place is empty.
pub(crate) fn move_file(src: &Path, dest_dir: &Path) -> Done {
    let Some(file_name) = src.file_name() else {
        return Done::no("I didn't move it: that isn't a file.");
    };
    if !src.is_file() {
        return Done::no(format!(
            "I didn't move it: {} isn't a file I can see.",
            src.display()
        ));
    }
    if !dest_dir.is_dir() {
        return Done::no(format!(
            "I didn't move it: {} isn't a folder that exists.",
            dest_dir.display()
        ));
    }
    let dest = dest_dir.join(file_name);
    if dest.exists() {
        return Done::no(format!(
            "I didn't move it: {} already exists, and I don't replace files.",
            dest.display()
        ));
    }
    if src.parent().is_some_and(|p| same_place(p, dest_dir)) {
        return Done::no(format!(
            "{} is already in {}.",
            src.display(),
            dest_dir.display()
        ));
    }
    let before = match (std::fs::metadata(src), crate::digest::sha256_file_hex(src)) {
        (Ok(m), Ok(h)) => (m.len(), h),
        (Err(e), _) | (_, Err(e)) => {
            return Done::no(format!(
                "I couldn't read {} to move it: {e}.",
                src.display()
            ))
        }
    };
    if let Err(e) =
        crate::safety::move_without_overwrite_recorded(src, &dest, &|| false, &mut || Ok(()))
    {
        return Done::no(format!(
            "I couldn't move {} to {}: {e}. {}",
            src.display(),
            dest.display(),
            if src.exists() {
                "The original is still where it was."
            } else {
                "Check both places."
            }
        ));
    }
    let after = (
        std::fs::metadata(&dest).map(|m| m.len()),
        crate::digest::sha256_file_hex(&dest),
    );
    let gone = !src.exists();
    match (after, gone) {
        ((Ok(len), Ok(h)), true) if len == before.0 && h == before.1 => Done::yes(format!(
            "Moved {} to {} ({}). Checked: the same contents are there and it's gone from the old place. To put it back, say: move {} to {}.",
            src.display(),
            dest.display(),
            bytes_said(len),
            file_name.to_string_lossy(),
            src.parent().map(|p| p.display().to_string()).unwrap_or_default()
        )),
        _ => Done::no(format!(
            "I moved {} to {}, but the check afterwards didn't pass (old place empty: {gone}). Look in both places before relying on it.",
            src.display(),
            dest.display()
        )),
    }
}

fn same_place(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

// ------------------------------------------------------------ folder backups

/// Most files, and most bytes, one backup of this kind takes.
pub const BACKUP_MAX_FILES: u64 = 20_000;
pub const BACKUP_MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// What a folder backup will do, decided before anything is copied.
#[derive(Debug, Clone)]
pub struct BackupPlan {
    pub src: PathBuf,
    pub target: PathBuf,
}

/// Is the place the copy would go inside the folder being copied? A copy into
/// itself never ends.
pub fn dest_inside_source(src: &Path, dest: &Path) -> bool {
    let canon = |p: &Path| {
        let mut probe = p.to_path_buf();
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        // The destination may not exist yet: its nearest existing parent is
        // what can be compared.
        loop {
            if let Ok(c) = probe.canonicalize() {
                let mut full = c;
                for t in tail.iter().rev() {
                    full.push(t);
                }
                return full;
            }
            match (probe.file_name().map(|n| n.to_os_string()), probe.parent()) {
                (Some(n), Some(parent)) => {
                    tail.push(n);
                    probe = parent.to_path_buf();
                }
                _ => return p.to_path_buf(),
            }
        }
    };
    let (s, d) = (canon(src), canon(dest));
    d.starts_with(&s)
}

/// The checks that need no walk: the folder is a folder, the destination is
/// somewhere a copy can go and not inside what is being copied. The
/// timestamped folder inside `dest_root` is named here so nothing already in
/// `dest_root` is ever overwritten.
pub fn plan_backup(src: &Path, dest_root: &Path, stamp: &str) -> Result<BackupPlan, String> {
    if !src.is_dir() {
        return Err(format!("{} isn't a folder I can see.", src.display()));
    }
    if dest_inside_source(src, dest_root) {
        return Err(format!(
            "I won't back up {} into {}: that is inside the folder being copied, so the copy would copy itself. Pick a place outside it.",
            src.display(),
            dest_root.display()
        ));
    }
    if !dest_root.is_dir() {
        let parent_ok = dest_root.parent().is_some_and(|p| p.is_dir());
        if !parent_ok {
            return Err(format!(
                "{} doesn't exist, and neither does the folder above it. Name a place that does.",
                dest_root.display()
            ));
        }
    }
    let base = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "folder".into());
    let mut target = dest_root.join(format!("{base} backup {stamp}"));
    let mut n = 2;
    while target.exists() {
        target = dest_root.join(format!("{base} backup {stamp} ({n})"));
        n += 1;
    }
    Ok(BackupPlan {
        src: src.to_path_buf(),
        target,
    })
}

/// Every file under `dir` as (path relative to it, size), counted for at most
/// the caps, and how many links (symbolic links or junctions) it left out;
/// `Err` names why it stopped. A link is skipped and counted, never followed
/// and never a reason to refuse (2026-10-21).
fn walk(dir: &Path, stopped: &dyn Fn() -> bool) -> Result<(Vec<(PathBuf, u64)>, u64), String> {
    let mut left_out = 0u64;
    let mut out: Vec<(PathBuf, u64)> = Vec::new();
    let mut bytes = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if stopped() {
            return Err("Stopped before it finished.".into());
        }
        let rd =
            std::fs::read_dir(&d).map_err(|e| format!("I couldn't read {}: {e}.", d.display()))?;
        for e in rd {
            let e = e.map_err(|e| format!("I couldn't read inside {}: {e}.", d.display()))?;
            let m = std::fs::symlink_metadata(e.path())
                .map_err(|x| format!("I couldn't look at {}: {x}.", e.path().display()))?;
            if m.file_type().is_symlink() {
                left_out += 1;
                continue;
            }
            if m.is_dir() {
                stack.push(e.path());
            } else {
                bytes += m.len();
                out.push((
                    e.path()
                        .strip_prefix(dir)
                        .unwrap_or(&e.path())
                        .to_path_buf(),
                    m.len(),
                ));
                if out.len() as u64 > BACKUP_MAX_FILES || bytes > BACKUP_MAX_BYTES {
                    return Err(format!(
                        "That folder is bigger than a quick backup here takes ({} files, {} so far; the limit is {} files or {}). Nothing was copied. Pick a smaller folder.",
                        out.len(),
                        bytes_said(bytes),
                        BACKUP_MAX_FILES,
                        bytes_said(BACKUP_MAX_BYTES)
                    ));
                }
            }
        }
    }
    Ok((out, left_out))
}

/// "left out 2 links (shortcuts to other folders)", or nothing when none.
fn links_said(n: u64) -> String {
    match n {
        0 => String::new(),
        1 => " It left out 1 link (a shortcut to another folder), which a backup doesn't follow."
            .into(),
        n => format!(
            " It left out {n} links (shortcuts to other folders), which a backup doesn't follow."
        ),
    }
}

/// Copy the folder into its own new timestamped folder, then check the copy
/// against the original by file count and by SHA-256 of every file. Runs on
/// the crew's thread; `stopped` is asked between files.
pub fn run_backup(plan: &BackupPlan, stopped: &dyn Fn() -> bool) -> Done {
    let (files, links) = match walk(&plan.src, stopped) {
        Ok(f) => f,
        Err(why) => return Done::no(why),
    };
    let total: u64 = files.iter().map(|(_, n)| *n).sum();
    if let Some(parent) = plan.target.parent() {
        if !parent.is_dir() {
            if let Err(e) = std::fs::create_dir(parent) {
                return Done::no(format!(
                    "I couldn't make {}: {e}. Nothing was copied.",
                    parent.display()
                ));
            }
        }
    }
    let copy_skipped = std::cell::Cell::new(0u64);
    if let Err(e) = crate::safety::copy_without_overwrite_verified_leaving_links(
        &plan.src,
        &plan.target,
        &|| stopped(),
        &|_| Ok(()),
        &copy_skipped,
    ) {
        return Done::no(format!(
            "The copy of {} didn't finish: {e}. The original is untouched; no partial copy was left under {}.",
            plan.src.display(),
            plan.target.display()
        ));
    }
    // The copy, read from disk again and compared file by file.
    let copied = match walk(&plan.target, stopped) {
        Ok((f, _)) => f,
        Err(why) => {
            return Done::no(format!(
                "I copied {} to {} but couldn't check the copy: {why}",
                plan.src.display(),
                plan.target.display()
            ))
        }
    };
    if copied.len() != files.len() {
        return Done::no(format!(
            "I copied {} to {} but the check failed: {} files in the original, {} in the copy. Don't rely on that copy.",
            plan.src.display(),
            plan.target.display(),
            files.len(),
            copied.len()
        ));
    }
    let mut bad: Vec<String> = Vec::new();
    for (rel, _) in &files {
        if stopped() {
            return Done::no(format!(
                "Copied {} files to {}, then stopped before checking them all. Treat the copy as unchecked.",
                files.len(),
                plan.target.display()
            ));
        }
        let a = crate::digest::sha256_file_hex(&plan.src.join(rel));
        let b = crate::digest::sha256_file_hex(&plan.target.join(rel));
        if !matches!((&a, &b), (Ok(x), Ok(y)) if x == y) {
            bad.push(rel.display().to_string());
            if bad.len() >= 5 {
                break;
            }
        }
    }
    if !bad.is_empty() {
        return Done::no(format!(
            "I copied {} files to {} but these differ from the originals: {}. Don't rely on that copy.",
            files.len(),
            plan.target.display(),
            bad.join(", ")
        ));
    }
    Done::yes(format!(
        "Backed up {} to {}: {} file{}, {}. Checked: same file count and the same SHA-256 for every file. Nothing already there was touched.{}",
        plan.src.display(),
        plan.target.display(),
        files.len(),
        if files.len() == 1 { "" } else { "s" },
        bytes_said(total),
        links_said(links.max(copy_skipped.get()))
    ))
}

// ----------------------------------------------------------- memory, as asked

/// One program's memory, as Task Manager groups it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    pub name: String,
    pub mb: u64,
    /// May be offered for closing: not Windows, not Atlas or anything it
    /// started, not a helper of another program.
    pub closable: bool,
}

/// The programs holding the most memory, biggest first, from one reading of
/// the process list (`tune::snapshot`: working set, every process of a
/// program added together). `me` is Atlas's own process number: it and
/// everything it started are listed but never closable.
pub fn biggest(procs: &[crate::tune::Proc], me: u32, keep: &[String]) -> Vec<Use> {
    let family = crate::tune::atlas_family(procs, me);
    let mut loads = crate::tune::sample_load(procs, procs, 1, 1);
    loads.sort_by(|a, b| b.mem_mb.cmp(&a.mem_mb).then(a.name.cmp(&b.name)));
    loads
        .into_iter()
        .filter(|l| l.mem_mb > 0)
        .map(|l| {
            let ours = l.pids.iter().any(|p| family.contains(p));
            Use {
                closable: !l.system && l.alone && !ours && crate::tune::may_close(&l.name, keep),
                name: l.name,
                mb: l.mem_mb,
            }
        })
        .collect()
}

/// "1.8 GB" or "900 MB".
pub(crate) fn mb_text(mb: u64) -> String {
    if mb >= 1024 {
        format!("{:.1} GB", mb as f64 / 1024.0)
    } else {
        format!("{mb} MB")
    }
}

/// Total and free memory in megabytes, as Windows says (0, 0 when it can't).
pub fn memory_now() -> (u64, u64) {
    let r = crate::health::read_machine();
    let total = (r.ram_total_gb * 1024.0) as u64;
    let used = (r.ram_used_gb * 1024.0) as u64;
    (total, total.saturating_sub(used))
}

/// "what's using my memory": the five biggest and the totals.
pub fn memory_said(top: &[Use], total_mb: u64, free_mb: u64) -> String {
    let head = if total_mb == 0 {
        "I couldn't read how much memory this computer has.".to_string()
    } else {
        format!(
            "This computer has {} of memory and {} is free ({}% in use).",
            mb_text(total_mb),
            mb_text(free_mb),
            100 - (free_mb * 100 / total_mb).min(100)
        )
    };
    if top.is_empty() {
        return format!("{head} I couldn't read the list of running programs, so I can't say which ones are using it.");
    }
    let list = top
        .iter()
        .take(5)
        .map(|u| format!("{} {}", u.name, mb_text(u.mb)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{head} The biggest five right now: {list}. (Each is all of that program's processes added together.)")
}

/// Words that point at something said earlier rather than name a program.
fn is_pronoun(w: &str) -> bool {
    matches!(
        w,
        "it" | "that" | "this" | "them" | "one" | "the one" | "those" | "these"
    )
}

/// "it", "that", or nothing at all: said instead of a program's name.
pub(crate) fn names_nothing(name: &str) -> bool {
    let n = crate::intent::normalize(name);
    n.is_empty() || is_pronoun(&n)
}

/// The program a "close <name>" means, from `uses`: the name exactly, then
/// whole words of it ("chrome" in "Google Chrome"), then a prefix ("goo").
/// Never a stray substring, and never "it" or "that" -- closing the wrong
/// program loses its unsaved work (2026-10-21). `None` when nothing matches
/// or the name is a pronoun.
pub(crate) fn pick_program<'a>(name: &str, uses: &'a [Use]) -> Option<&'a Use> {
    let bare = |s: &str| {
        let t = s.trim();
        let t = t
            .strip_suffix(".exe")
            .or_else(|| t.strip_suffix(".EXE"))
            .unwrap_or(t);
        crate::intent::normalize(t)
    };
    let want = bare(name);
    if want.is_empty() || is_pronoun(&want) {
        return None;
    }
    let norm = |u: &Use| bare(&u.name);
    uses.iter()
        .find(|u| norm(u) == want)
        .or_else(|| {
            uses.iter().find(|u| {
                let n = norm(u);
                let (nw, ww): (Vec<&str>, Vec<&str>) = (
                    n.split_whitespace().collect(),
                    want.split_whitespace().collect(),
                );
                !ww.is_empty() && nw.windows(ww.len()).any(|win| win == ww.as_slice())
            })
        })
        .or_else(|| {
            // A prefix needs a few letters to mean anything.
            (want.chars().count() >= 3)
                .then(|| uses.iter().find(|u| norm(u).starts_with(&want)))
                .flatten()
        })
}

// ------------------------------------------ a question the model can't answer

/// What is known when a question needs the language model.
#[derive(Debug, Clone)]
pub struct Facts {
    /// A model is loaded and answering, or an own model is configured.
    pub loaded: bool,
    /// The model that would be loaded, and what it takes in megabytes. `None`:
    /// none installed.
    pub model: Option<(String, u64)>,
    pub free_mb: u64,
    pub total_mb: u64,
    /// The setting allows the free online models (`models.online_second`).
    pub online_allowed: bool,
    /// The connection is known to be down.
    pub offline_known: bool,
    /// Programs that may be offered for closing, biggest first.
    pub closable: Vec<Use>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowAsk {
    pub model: String,
    pub need_mb: u64,
    pub free_mb: u64,
    pub shortfall_mb: u64,
    /// Offer the online answer.
    pub online: bool,
    /// Programs to offer closing, biggest first.
    pub apps: Vec<Use>,
    /// Closing all of `apps` would free enough.
    pub closing_would_do: bool,
    /// The model wants more than this computer can spare even with
    /// everything closed.
    pub hopeless: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Nothing to say: ask the model as usual.
    Proceed,
    /// No language model is installed at all.
    NoModel,
    /// Installed, not loaded, not enough memory free to load it.
    Low(LowAsk),
}

/// Decide, from the facts alone, whether to ask the model or to answer at
/// once with a choice. Pure, so it is tested without a machine.
pub fn decide(f: &Facts) -> Choice {
    if f.loaded {
        return Choice::Proceed;
    }
    let Some((id, need)) = &f.model else {
        return Choice::NoModel;
    };
    if f.free_mb >= *need {
        return Choice::Proceed;
    }
    let shortfall = need - f.free_mb;
    let apps: Vec<Use> = f.closable.iter().take(4).cloned().collect();
    let closable_mb: u64 = apps.iter().map(|a| a.mb).sum();
    Choice::Low(LowAsk {
        model: id.clone(),
        need_mb: *need,
        free_mb: f.free_mb,
        shortfall_mb: shortfall,
        online: f.online_allowed && !f.offline_known,
        closing_would_do: closable_mb >= shortfall,
        hopeless: f.total_mb > 0 && need + 1024 > f.total_mb,
        apps,
    })
}

/// What no usable model means: said plainly, with the next step. Reached
/// when nothing is installed that Atlas may run; a model that is installed but
/// does not fit the memory free right now is `Choice::Low` instead (21 Oct
/// 2026), with how much is free against how much is needed.
pub fn no_model_said() -> String {
    "I can't answer that: no language model is installed on this computer that I can run, so I didn't start anything. \
     Next step: open the Connections page in the Atlas hub and press \"Get the better model\", then ask again. \
     Until then I still do the everyday things without one -- open and close apps, make, read and move files, timers, the time, backups."
        .to_string()
}

/// The choice put to you, in words, when memory is short. The two ways
/// forward are both named, the online one only when it is really available.
pub fn say(a: &LowAsk) -> String {
    let mut s = format!(
        "I can't load my language model right now: it needs about {} and only {} is free.",
        mb_text(a.need_mb),
        mb_text(a.free_mb)
    );
    if a.hopeless {
        s.push_str(
            " That is more than this computer can spare even with everything else closed, so closing apps won't fix it.",
        );
        if a.online {
            s.push_str(" Say \"answer online\" and I'll send this one question to a free online model; its answer will be labelled as online, and the question leaves this computer.");
        } else {
            s.push_str(" A smaller model would fit: open the Connections page in the Atlas hub.");
        }
        return s;
    }
    let mut ways: Vec<String> = Vec::new();
    if a.online {
        ways.push(
            "say \"answer online\" and I'll send this one question to a free online model (its answer will be labelled as online, and the question leaves this computer)"
                .into(),
        );
    }
    if let Some(first) = a.apps.first() {
        let list = a
            .apps
            .iter()
            .map(|u| format!("{} {}", u.name, mb_text(u.mb)))
            .collect::<Vec<_>>()
            .join(", ");
        ways.push(format!(
            "free memory -- the biggest apps are {list}; say \"close {}\" and I'll load the model once it's closed, then answer{}",
            first.name,
            if a.closing_would_do {
                String::new()
            } else {
                format!(" (it needs about {} more, so you may need to close more than one)", mb_text(a.shortfall_mb))
            }
        ));
    } else {
        ways.push(format!(
            "free about {} yourself (I can't see an app I'm allowed to offer closing) and ask again",
            mb_text(a.shortfall_mb)
        ));
    }
    if !a.online {
        s.push_str(
            " I can't answer online either (the online fallback is off or I'm offline), so:",
        );
    } else {
        s.push_str(" Two ways forward:");
    }
    for (i, w) in ways.iter().enumerate() {
        s.push_str(&format!(" {}) {w}.", i + 1));
    }
    s
}

// ------------------------------------------------- "is the backup finished?"

/// A question about how some work of Atlas's is going (21 Oct 2026). With no
/// model, "is the backup of src finished?" fell through to the outstanding
/// list and was answered "I'm preparing your brief" (backup/restore e2e,
/// row 2). It is answered from the job records instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusAsk {
    /// "backup", "download", "move", "restore" or "research"; `None` for "is
    /// it done?", which means whatever ran last.
    pub subject: Option<&'static str>,
    /// A name that narrows it ("src" in "the backup of src"), lower case.
    pub detail: Option<String>,
}

/// How a job stands, from either record kept for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Life {
    Running,
    Done(String),
    Failed(String),
}

/// One job, as the Hub's job list or the durable job records know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobView {
    pub label: String,
    pub life: Life,
    /// Wall-clock seconds when it ended.
    pub ended: Option<u64>,
}

/// The kinds of work a status question can name: the word said, what the
/// answer calls it, and what its job's label contains.
const SUBJECTS: &[(&[&str], &str, &[&str])] = &[
    (&["backup", "backups", "backing"], "backup", &["backing up", "backup", "back up"]),
    (&["download", "downloads", "downloading"], "download", &["download"]),
    (&["move", "moved", "moving"], "move", &["moving"]),
    (&["restore", "restoring", "restored"], "restore", &["restor"]),
    (&["research", "researching"], "research", &["research"]),
];

/// "Is the backup finished?", "how's the backup going", "did the backup of
/// src finish", "is it done". `None` for anything else.
pub(crate) fn status_question(raw: &str) -> Option<StatusAsk> {
    status_ask(raw, false)
}

/// The same, also reading "is it done?", "is that finished?", "how's it
/// going" -- a pronoun for a subject. Only for a caller that knows a job was
/// started or finished a moment ago (22 Oct 2026): with no job in context
/// "how's it going" is small talk, and `status_question` leaves it alone.
pub(crate) fn status_question_about_it(raw: &str) -> Option<StatusAsk> {
    status_ask(raw, true)
}

fn status_ask(raw: &str, pronoun_ok: bool) -> Option<StatusAsk> {
    let lower = raw.to_lowercase();
    let mut w: Vec<String> = lower
        .split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| matches!(c, '?' | '!' | ',' | '.' | '"' | '\''))
                .to_string()
        })
        .filter(|t| !t.is_empty())
        .collect();
    while matches!(
        w.first().map(String::as_str),
        Some("please" | "hey" | "atlas" | "so" | "and" | "ok" | "okay")
    ) {
        w.remove(0);
    }
    let has = |set: &[&str]| w.iter().any(|t| set.contains(&t.as_str()));
    let asks_done = matches!(
        w.first().map(String::as_str),
        Some("is" | "are" | "has" | "have" | "did" | "does" | "was")
    ) && has(&["finished", "finish", "done", "complete", "completed", "ready"]);
    let asks_going = has(&["how's", "hows"])
        || (w.first().map(String::as_str) == Some("how")
            && matches!(w.get(1).map(String::as_str), Some("is" | "are")));
    let asks_going = asks_going && has(&["going", "progressing", "along", "doing"]);
    let asks_status = w.windows(2).any(|p| {
        matches!(
            (p[0].as_str(), p[1].as_str()),
            ("status", "of") | ("progress", "of") | ("progress", "on")
        )
    });
    if !(asks_done || asks_going || asks_status) {
        return None;
    }
    let subject = SUBJECTS
        .iter()
        .find(|(words, _, _)| has(words))
        .map(|(_, noun, _)| *noun)
        // "back up" said as two words
        .or_else(|| {
            w.windows(2)
                .any(|p| p[0] == "back" && p[1] == "up")
                .then_some("backup")
        });
    if subject.is_none() {
        // "how's it going", "is it done yet" name no job: the existing
        // what's-running handler answers those, from what is running --
        // unless a job is in context (`status_question_about_it`), when "it"
        // is that job.
        let about_it = has(&["it", "that"]);
        if !(pronoun_ok && about_it) {
            return None;
        }
    }
    let detail = w
        .iter()
        .position(|t| t == "of" || t == "for")
        .and_then(|at| {
            w[at + 1..].iter().find(|t| {
                !matches!(
                    t.as_str(),
                    "the" | "my" | "a" | "an" | "this" | "that" | "folder" | "directory"
                )
            })
        })
        .filter(|t| {
            !matches!(
                t.as_str(),
                "finished" | "finish" | "done" | "complete" | "completed" | "it" | "going"
            )
        })
        .map(|t| t.to_lowercase());
    Some(StatusAsk { subject, detail })
}

/// "3 minutes ago" for something that ended `at`.
fn ago(now: u64, at: u64) -> String {
    let s = now.saturating_sub(at);
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match s {
        0..=59 => "just now".into(),
        60..=3599 => plural(s / 60, "minute"),
        3600..=86_399 => plural(s / 3600, "hour"),
        _ => plural(s / 86_400, "day"),
    }
}

/// The answer from the job records: running with what it is doing, done with
/// when and where, failed with why, or that there is no such job. Which job:
/// one still running first, else the newest that ended; a name in the
/// question ("of src") narrows it.
pub(crate) fn status_said(ask: &StatusAsk, jobs: &[JobView], now: u64, offset: i64) -> String {
    let stems: &[&str] = ask
        .subject
        .and_then(|s| SUBJECTS.iter().find(|(_, noun, _)| *noun == s))
        .map(|(_, _, stems)| *stems)
        .unwrap_or(&[]);
    let fits: Vec<&JobView> = jobs
        .iter()
        .filter(|j| {
            let l = j.label.to_lowercase();
            (stems.is_empty() || stems.iter().any(|s| l.contains(s)))
                && ask.detail.as_deref().is_none_or(|d| l.contains(d))
        })
        .collect();
    let pick = fits
        .iter()
        .find(|j| j.life == Life::Running)
        .or_else(|| fits.iter().max_by_key(|j| j.ended.unwrap_or(0)));
    let Some(job) = pick else {
        let what = match (ask.subject, &ask.detail) {
            (Some(s), Some(d)) => format!("a {s} of {d}"),
            (Some(s), None) => format!("a {s}"),
            (None, _) => "any work".to_string(),
        };
        return format!(
            "I don't have {what} running or recently finished. Anything still open is on the Outstanding page."
        );
    };
    let more = if fits.len() > 1 && ask.detail.is_none() {
        let noun = ask.subject.unwrap_or("job");
        format!(
            " That is the latest of {} {noun}s I know about; name the folder if you meant another.",
            fits.len()
        )
    } else {
        String::new()
    };
    let when = |at: Option<u64>| match at {
        Some(t) => format!(
            "at {} ({})",
            crate::localclock::hhmm(t, offset),
            ago(now, t)
        ),
        None => "earlier".into(),
    };
    match &job.life {
        Life::Running => format!(
            "Not yet -- it is still running: {}. I only know it is under way (it gives no count while it copies); I'll say when it is done and checked.{more}",
            job.label
        ),
        Life::Done(said) => format!(
            "Yes -- {} finished {}. {said}{more}",
            job.label,
            when(job.ended)
        ),
        Life::Failed(why) => format!(
            "No -- {} failed {}: {why}{more}",
            job.label,
            when(job.ended)
        ),
    }
}

/// What a reply to the choice means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Follow {
    Online,
    /// "close Chrome": the name said.
    Close(String),
    Cancel,
    /// Anything else: a new request.
    Other,
}

pub fn follow_up(said: &str) -> Follow {
    let n = crate::intent::normalize(said);
    let n = crate::intent::without_fillers(&n);
    for o in [
        "answer online",
        "answer it online",
        "answer that online",
        "answer this online",
        "online",
        "go online",
        "use online",
        "ask online",
        "yes online",
        "do it online",
        "online please",
    ] {
        if n == o {
            return Follow::Online;
        }
    }
    for c in ["close ", "quit ", "shut ", "end "] {
        if let Some(rest) = n.strip_prefix(c) {
            let rest = rest.strip_prefix("down ").unwrap_or(rest).trim();
            if !rest.is_empty() {
                return Follow::Close(rest.to_string());
            }
        }
    }
    if matches!(
        n.as_str(),
        "no" | "nope"
            | "cancel"
            | "never mind"
            | "nevermind"
            | "forget it"
            | "stop"
            | "leave it"
            | "not now"
            | "no thanks"
    ) {
        return Follow::Cancel;
    }
    Follow::Other
}

/// A destination said in answer to "where should the backup go?": the words,
/// or `None` when the answer isn't one (a new request, or a no).
pub(crate) fn backup_destination(said: &str) -> Option<String> {
    let t = said.trim();
    if matches!(follow_up(t), Follow::Cancel) {
        return None;
    }
    let lower = t.to_lowercase();
    let t = ["to ", "into ", "put it in ", "put it on ", "in ", "on "]
        .iter()
        .find_map(|p| lower.strip_prefix(p).map(|_| t[p.len()..].trim()))
        .unwrap_or(t);
    let l = t.to_lowercase();
    // A path, or a folder by name -- not a sentence.
    (names_a_folder(&l) && t.split_whitespace().count() <= 8).then(|| folder_words(t))
}

/// Whether the whole reply is a plain yes. It used to be the first word, so
/// "do not close it" and "please don't" closed the program and lost unsaved
/// work (2026-10-21). Now the whole reply must be an affirmative phrase, and
/// anything with a no in it (not, don't, never, wait, stop, cancel) is a no.
pub(crate) fn yes(said: &str) -> bool {
    let n = crate::intent::normalize(said);
    if n.split_whitespace().any(|w| {
        matches!(
            w,
            "not" | "dont" | "no" | "never" | "wait" | "stop" | "cancel"
        )
    }) {
        return false;
    }
    matches!(
        n.as_str(),
        "yes"
            | "yeah"
            | "yep"
            | "yup"
            | "ok"
            | "okay"
            | "sure"
            | "confirm"
            | "go ahead"
            | "do it"
            | "close it"
            | "yes please"
            | "please do"
            | "go on"
            | "yes close it"
            | "yes go ahead"
            | "yes do it"
            | "sure go ahead"
            | "ok go ahead"
            | "okay go ahead"
    )
}

/// What the daemon keeps while it waits for your answer.
#[derive(Debug, Clone)]
pub enum Pending {
    /// "Back up <folder>" with no place said: waiting to be told one.
    BackupDest { folder: PathBuf },
    /// Memory is short: waiting for "answer online" or "close <app>".
    LowMemory {
        request: String,
        online: bool,
        need_mb: u64,
    },
    /// "close <app>" for memory, asked to be confirmed: the program, what it
    /// holds, and the request to answer after.
    CloseForMemory {
        request: String,
        name: String,
        mb: u64,
        need_mb: u64,
    },
}

/// What the daemon keeps for the floor between turns, and the seams a test
/// stands things in for.
#[derive(Default)]
pub struct State {
    /// The question Atlas is waiting on an answer to.
    pub pending: Option<Pending>,
    /// Set while a request is being asked again on purpose (the online
    /// answer), so the choice isn't put twice.
    pub bypass: bool,
    /// Test seam: the online model (otherwise the free online services).
    pub online: Option<std::sync::Arc<dyn crate::brain::Llm>>,
    /// Test seam: the facts about model and memory (otherwise measured).
    pub facts: Option<std::sync::Arc<dyn Fn() -> Facts + Send + Sync>>,
    /// Test seam: closing a program (otherwise really closed).
    pub closer: Option<std::sync::Arc<dyn Fn(&str) -> Done + Send + Sync>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_whole_plain_yes_closes_a_program() {
        for no in [
            "do not close it",
            "please don't",
            "Please, don't!",
            "no",
            "yes but wait",
            "yes wait",
            "go ahead and cancel",
            "sure not now",
            "stop",
            "never",
            "do you have to",
            "ok so what is it holding",
            "",
        ] {
            assert!(!yes(no), "{no:?} must not count as yes");
        }
        for ok in [
            "yes",
            "Yeah",
            "yep",
            "OK.",
            "okay",
            "sure",
            "go ahead",
            "Do it!",
            "close it",
            "yes please",
            "please do",
            "go on",
        ] {
            assert!(yes(ok), "{ok:?} must count as yes");
        }
    }

    #[test]
    fn a_program_is_picked_by_name_never_by_it() {
        let u = |name: &str| Use {
            name: name.into(),
            mb: 500,
            closable: true,
        };
        let all = vec![
            u("Google Chrome"),
            u("Code"),
            u("Microsoft Edge"),
            u("edgeview"),
        ];
        assert_eq!(pick_program("chrome", &all).unwrap().name, "Google Chrome");
        assert_eq!(
            pick_program("Google Chrome", &all).unwrap().name,
            "Google Chrome"
        );
        assert_eq!(pick_program("code", &all).unwrap().name, "Code");
        assert_eq!(pick_program("edge", &all).unwrap().name, "Microsoft Edge");
        assert_eq!(pick_program("edgev", &all).unwrap().name, "edgeview");
        for no in ["it", "that", "this", "them", "", "co", "e"] {
            assert!(pick_program(no, &all).is_none(), "{no:?}");
        }
        assert!(names_nothing("it") && names_nothing("That") && !names_nothing("chrome"));
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("atlas-floor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A junction (no privilege needed) or else a directory symlink; `None`
    /// when this machine won't make either.
    fn link_dir(target: &Path, at: &Path) -> bool {
        #[cfg(windows)]
        {
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(at)
                .arg(target)
                .output()
                .is_ok_and(|o| o.status.success());
            if made {
                return true;
            }
            std::os::windows::fs::symlink_dir(target, at).is_ok()
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, at).is_ok()
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = (target, at);
            false
        }
    }

    #[test]
    fn a_backup_leaves_links_out_and_says_so() {
        let root = temp("links");
        let src = root.join("Documents");
        let other = root.join("Elsewhere");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(src.join("a.txt"), "alpha").unwrap();
        std::fs::write(src.join("sub").join("b.txt"), "beta").unwrap();
        std::fs::write(other.join("big.txt"), "not in the backup").unwrap();
        if !link_dir(&other, &src.join("My Music")) {
            eprintln!("skipped: this machine won't make a junction or directory link");
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let (files, links) = walk(&src, &|| false).expect("a link is not a reason to refuse");
        assert_eq!(files.len(), 2, "{files:?}");
        assert_eq!(links, 1);
        let dest = root.join("Backups");
        std::fs::create_dir_all(&dest).unwrap();
        let plan = plan_backup(&src, &dest, "20261021-000000").expect("plan");
        let done = run_backup(&plan, &|| false);
        assert!(done.ok, "{}", done.said);
        assert!(done.said.contains("left out 1 link"), "{}", done.said);
        assert!(plan.target.join("sub").join("b.txt").is_file());
        assert!(!plan.target.join("My Music").exists());
        // Remove the link itself, not what it points at.
        let _ = std::fs::remove_dir(src.join("My Music"));
        assert!(other.join("big.txt").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn overwriting_replaces_whole_and_refuses_a_link() {
        let root = temp("overwrite");
        let spec = |name: &str, text: &str| CreateSpec {
            name: name.into(),
            folder: None,
            contents: text.into(),
            replace: true,
        };
        std::fs::write(root.join("note.txt"), "old old old old").unwrap();
        let done = create_file(&spec("note.txt", "new"), &root);
        assert!(done.ok, "{}", done.said);
        assert_eq!(
            std::fs::read_to_string(root.join("note.txt")).unwrap(),
            "new"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec!["note.txt".to_string()], "no temp file left");
        #[cfg(any(windows, unix))]
        {
            let real = root.join("real.txt");
            std::fs::write(&real, "keep me").unwrap();
            #[cfg(windows)]
            let made = std::os::windows::fs::symlink_file(&real, root.join("link.txt")).is_ok();
            #[cfg(unix)]
            let made = std::os::unix::fs::symlink(&real, root.join("link.txt")).is_ok();
            if made {
                let done = create_file(&spec("link.txt", "clobber"), &root);
                assert!(!done.ok, "{}", done.said);
                assert_eq!(std::fs::read_to_string(&real).unwrap(), "keep me");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- "is the backup finished?" (21 Oct 2026) ------------------------

    fn view(label: &str, life: Life, ended: Option<u64>) -> JobView {
        JobView {
            label: label.into(),
            life,
            ended,
        }
    }

    #[test]
    fn status_questions_are_read_and_other_talk_is_not() {
        for (said, subject, detail) in [
            ("is the backup of src finished?", Some("backup"), Some("src")),
            ("Is the backup done?", Some("backup"), None),
            ("how's the backup going", Some("backup"), None),
            ("did the backup of Documents finish", Some("backup"), Some("documents")),
            ("has the download finished", Some("download"), None),
            ("please, is the back up finished?", Some("backup"), None),
        ] {
            let ask = status_question(said).unwrap_or_else(|| panic!("{said:?} not read"));
            assert_eq!(ask.subject, subject, "{said:?}");
            assert_eq!(ask.detail.as_deref(), detail, "{said:?}");
        }
        // With a job in context a pronoun is that job; without one (the plain
        // reading) it is still nothing.
        for said in ["is it done?", "is it done yet", "is that finished", "how's it going"] {
            let ask = status_question_about_it(said).unwrap_or_else(|| panic!("{said:?}"));
            assert_eq!(ask.subject, None, "{said:?}");
            assert!(status_question(said).is_none(), "{said:?}");
        }
        assert!(status_question_about_it("how's the weather going").is_none());
        for not in [
            "back up my Documents folder to D:\\Backups",
            "what's the weather",
            "is the report finished being written by the intern in london today",
            "how's the weather going",
            "how's it going",
            "is it done",
            "how's that going",
            "finish the backup",
        ] {
            assert!(status_question(not).is_none(), "{not:?} is not a status question");
        }
    }

    #[test]
    fn a_status_answer_comes_from_the_job_and_says_running_done_or_failed() {
        let ask = status_question("is the backup of src finished?").unwrap();
        let now = 1_000_000;
        let label = "Backing up C:\\work\\src to D:\\b\\src backup 2026-10-21";
        // Running: says so, and does not claim it is done.
        let said = status_said(&ask, &[view(label, Life::Running, None)], now, 0);
        assert!(said.starts_with("Not yet"), "{said}");
        assert!(said.contains("still running") && said.contains("src"), "{said}");
        // Done: when and where.
        let said = status_said(
            &ask,
            &[view(
                label,
                Life::Done("Backed up C:\\work\\src to D:\\b: 3 files.".into()),
                Some(now - 300),
            )],
            now,
            0,
        );
        assert!(said.starts_with("Yes"), "{said}");
        assert!(said.contains("5 minutes ago") && said.contains("D:\\b"), "{said}");
        // Failed: why.
        let said = status_said(
            &ask,
            &[view(label, Life::Failed("the disk is full".into()), Some(now - 30))],
            now,
            0,
        );
        assert!(said.starts_with("No") && said.contains("the disk is full"), "{said}");
        // Another folder's backup is not this one's, and nothing at all is said as it is.
        let other = view("Backing up C:\\work\\docs to D:\\b", Life::Running, None);
        let said = status_said(&ask, &[other], now, 0);
        assert_eq!(said, "I don't have a backup of src running or recently finished. Anything still open is on the Outstanding page.");
        let said = status_said(&status_question("is the backup finished?").unwrap(), &[], now, 0);
        assert_eq!(said, "I don't have a backup running or recently finished. Anything still open is on the Outstanding page.");
    }

    #[test]
    fn a_running_job_is_picked_before_a_finished_one_and_a_name_narrows_it() {
        let now = 1_000_000;
        let jobs = [
            view("Backing up C:\\a to D:\\x", Life::Done("ok".into()), Some(now - 100)),
            view("Backing up C:\\b to D:\\x", Life::Running, None),
        ];
        let said = status_said(&status_question("is the backup finished").unwrap(), &jobs, now, 0);
        assert!(said.starts_with("Not yet") && said.contains("C:\\b"), "{said}");
        assert!(said.contains("latest of 2"), "{said}");
        let said = status_said(&status_question("is the backup of c:\\a finished").unwrap(), &jobs, now, 0);
        assert!(said.starts_with("Yes") && !said.contains("latest of"), "{said}");
    }
}
