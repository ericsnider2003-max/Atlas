//! Reading a project before changing it (2 Oct 2026).
//!
//! A change to one of your projects was written against "the first 40 lines
//! of up to 6 files" -- whichever six `read_dir` happened to list first, in
//! `src/` and the top folder only. A request naming `parse_config` or an
//! error in `db/session.py` was answered without the model ever seeing
//! either, so it guessed at the code around it, and the guesses failed.
//!
//! Now the project is read the way a person would look before touching it:
//!
//! 1. **The tree**, compactly: every source file's path, so the model knows
//!    what exists and where.
//! 2. **What the request points at**: names in it that look like code
//!    (`parse_config`, `SessionStore`, `load_rows()`), file names
//!    (`session.py`), and the words of any error text pasted in. A file
//!    named is read first; a piece that *defines* a name mentioned comes
//!    before one that only uses it.
//! 3. **What it's about**, by BM25 (`bm25`) over the project cut into pieces
//!    (`chunker`, so every piece keeps its line numbers): the pieces whose
//!    words match the request, a name split into its words (`parse_config`
//!    and `parseConfig` both match "parse config").
//!
//! The three rankings are merged by reciprocal rank (`bm25::rrf`), and pieces
//! are taken best first until the budget -- worked out from the model's own
//! context (`budget_for`) -- is spent; then printed file by file in line
//! order, each with its path and lines.
//!
//! Meaning search (`meaning`) isn't used here: it needs a vector for every
//! piece of the project, made by the meaning model one at a time, and a
//! request shouldn't wait for a whole project to be embedded. The names and
//! words above are what a code request carries anyway.
//!
//! Bounded: at most `MOST_FILES` files and `MOST_BYTES` read, no file over
//! `LARGEST_FILE`, and never `target/`, `.git/`, `node_modules/` or the like.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Files looked at, at most.
pub const MOST_FILES: usize = 1500;
/// Bytes read across the project, at most.
pub const MOST_BYTES: u64 = 8 * 1024 * 1024;
/// A file larger than this is listed but not read: generated, or data.
pub const LARGEST_FILE: u64 = 256 * 1024;
/// How deep into folders.
const DEEPEST: usize = 10;
/// Folders never read: build output, version control, dependencies, caches.
const SKIP_DIRS: &[&str] = &[
    "target", ".git", ".hg", ".svn", "node_modules", "__pycache__", ".venv", "venv", "env", "dist", "build",
    ".mypy_cache", ".pytest_cache", ".tox", ".idea", ".vscode", "vendor", ".next", "out", "bin", "obj",
];
/// What counts as part of a project worth reading.
const SOURCE_EXTS: &[&str] = &[
    "rs", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "go", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "java", "kt",
    "cs", "rb", "php", "swift", "lua", "sh", "ps1", "toml", "yaml", "yml", "json", "cfg", "ini", "md", "html", "css",
    "sql",
];
/// Manifests, read whatever their extension.
const MANIFESTS: &[&str] = &["Cargo.toml", "package.json", "pyproject.toml", "requirements.txt", "go.mod", "CMakeLists.txt", "Makefile"];

/// What was read for a request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    /// The text for the model: the tree, then the pieces.
    pub text: String,
    /// The files pieces were taken from, best first.
    pub files: Vec<String>,
    /// About how many tokens `text` is.
    pub tokens: usize,
}

/// About how many tokens a text is. Code runs nearer three characters a
/// token than four, so three: over-counting keeps the prompt inside the
/// model's room.
fn tokens_in(text: &str) -> usize {
    text.len().div_ceil(3)
}

/// How much of a model's context the project may take: a third of it, so a
/// whole file still fits in the reply (`build_it::code_budget` takes what
/// the prompt leaves), between 600 and 12,000 tokens. No context known:
/// `build_it::CONTEXT_ASSUMED`.
pub fn budget_for(context_tokens: Option<u32>) -> usize {
    let ctx = context_tokens.unwrap_or(crate::build_it::CONTEXT_ASSUMED) as usize;
    (ctx * 35 / 100).clamp(600, 12_000)
}

/// One file found.
struct Found {
    rel: String,
    text: Option<String>,
}

/// The project's files: source and manifests, bounded, in path order.
fn walk(root: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    let mut read = 0u64;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut dirs_seen = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        dirs_seen += 1;
        if dirs_seen > MOST_FILES {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= MOST_FILES {
                break;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth < DEEPEST && !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
            if !SOURCE_EXTS.contains(&ext.as_str()) && !MANIFESTS.contains(&name.as_str()) {
                continue;
            }
            // Lock files are long and say nothing a change needs.
            if name.ends_with(".lock") || name == "package-lock.json" {
                continue;
            }
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let size = e.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
            let text = if size <= LARGEST_FILE && read + size <= MOST_BYTES {
                read += size;
                std::fs::read_to_string(&path).ok()
            } else {
                None
            };
            out.push(Found { rel, text });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// A name with its parts as words too: `parse_config` and `parseConfig` both
/// give "parse config", so the request's plain words find them.
fn split_name(word: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in word.chars() {
        if c == '_' || c == '-' || c == '.' || c == ':' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        out.extend(c.to_lowercase());
    }
    out
}

/// What a request points at.
#[derive(Debug, Default, PartialEq)]
struct Pointers {
    /// Names that look like code: `parse_config`, `SessionStore`, `load()`.
    names: Vec<String>,
    /// File names: `session.py`, `src/db/mod.rs`.
    files: Vec<String>,
}

/// The names and file names in a request (and any error text pasted in).
fn pointers(request: &str) -> Pointers {
    let mut p = Pointers::default();
    let mut seen = HashSet::new();
    for raw in request.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ',' | ';' | '[' | ']' | '{' | '}' | '<' | '>' | '=')) {
        let had_call = raw.contains('(');
        let t = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '/' && c != '.' && c != ':' && c != '\\');
        let t = t.split('(').next().unwrap_or("").trim_end_matches(['.', ':']);
        if t.len() < 3 {
            continue;
        }
        // A file: a word with a source extension ("session.py", "src/a.rs",
        // "app.py:42").
        let file_part = t.split(':').next().unwrap_or(t).replace('\\', "/");
        if let Some((_, ext)) = file_part.rsplit_once('.') {
            if SOURCE_EXTS.contains(&ext.to_ascii_lowercase().as_str()) && file_part.len() > ext.len() + 1 {
                if seen.insert(file_part.clone()) {
                    p.files.push(file_part);
                }
                continue;
            }
        }
        // A name: snake_case, camelCase / PascalCase with a hump, a path
        // like `db::open`, or anything written as a call.
        let last = t.rsplit("::").next().unwrap_or(t).rsplit('.').next().unwrap_or(t);
        if last.len() < 3 || !last.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') {
            continue;
        }
        if !last.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let snake = last.contains('_');
        let humped = last.chars().skip(1).any(|c| c.is_uppercase()) && last.chars().any(|c| c.is_lowercase());
        if (snake || humped || had_call || t.contains("::")) && seen.insert(last.to_string()) {
            p.names.push(last.to_string());
        }
    }
    p
}

/// Does this piece define `name` (rather than only use it)?
fn defines(text: &str, name: &str) -> bool {
    const BEFORE: &[&str] = &[
        "fn ", "def ", "class ", "struct ", "enum ", "trait ", "type ", "function ", "const ", "let ", "var ", "interface ",
        "impl ", "mod ", "static ", "func ",
    ];
    text.lines().any(|l| {
        let l = l.trim_start();
        let l = l.strip_prefix("pub ").or_else(|| l.strip_prefix("pub(crate) ")).or_else(|| l.strip_prefix("export ")).or_else(|| l.strip_prefix("async ")).unwrap_or(l);
        let l = l.strip_prefix("async ").unwrap_or(l);
        BEFORE.iter().any(|b| {
            l.strip_prefix(b).is_some_and(|rest| {
                rest.strip_prefix(name).is_some_and(|after| !after.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
            })
        }) || (l.starts_with(name) && l[name.len()..].trim_start().starts_with('=') && !l[name.len()..].trim_start().starts_with("=="))
    })
}

/// Holds `name` as a whole word.
fn mentions(text: &str, name: &str) -> bool {
    let mut from = 0;
    while let Some(i) = text[from..].find(name) {
        let at = from + i;
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !word(before) && !word(after) {
            return true;
        }
        from = at + name.len();
    }
    false
}

/// One piece of a file.
struct Piece {
    file: usize,
    start: usize,
    end: usize,
    text: String,
}

/// The compact tree: every file's path, grouped under its folder, within
/// `budget` tokens; folders past the budget are counted, not listed.
fn tree_of(paths: &[String], budget: usize) -> String {
    let mut by_dir: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in paths {
        let (dir, name) = match p.rsplit_once('/') {
            Some((d, n)) => (format!("{d}/"), n.to_string()),
            None => ("./".to_string(), p.clone()),
        };
        by_dir.entry(dir).or_default().push(name);
    }
    let mut out = String::new();
    let mut left_out = 0usize;
    for (dir, names) in &by_dir {
        let line = format!("{dir} {}\n", names.join(" "));
        if tokens_in(&out) + tokens_in(&line) > budget {
            left_out += names.len();
            continue;
        }
        out.push_str(&line);
    }
    if left_out > 0 {
        out.push_str(&format!("(and {left_out} more files)\n"));
    }
    out
}

/// Read the project in `root` for `request`, within `budget` tokens.
pub fn relevant(root: &Path, request: &str, budget: usize) -> Reading {
    if !root.is_dir() || budget == 0 {
        return Reading::default();
    }
    let files = walk(root);
    if files.is_empty() {
        return Reading::default();
    }
    let paths: Vec<String> = files.iter().map(|f| f.rel.clone()).collect();
    // The tree takes at most a sixth of the room.
    let tree = tree_of(&paths, (budget / 6).max(60));
    let mut text = format!("The project's files:\n{tree}\n");

    // Cut every readable file into pieces that keep their lines.
    let mut pieces: Vec<Piece> = Vec::new();
    let cfg = crate::chunker::ChunkConfig { max: 1500, overlap: 0 };
    for (i, f) in files.iter().enumerate() {
        let Some(t) = &f.text else { continue };
        if t.trim().is_empty() {
            continue;
        }
        let Ok(chunks) = crate::chunker::chunk(t, cfg) else { continue };
        for c in chunks {
            pieces.push(Piece { file: i, start: c.start_line, end: c.end_line, text: c.text });
        }
    }

    let ptr = pointers(request);
    let request_low = request.to_lowercase();

    // 1. Pieces of the files the request names, in order.
    let named_files: Vec<usize> = files
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            ptr.files.iter().any(|n| {
                let n = n.trim_start_matches("./");
                f.rel == n || f.rel.ends_with(&format!("/{n}"))
            }) || {
                // A file named by its stem alone: "the session module".
                let stem = f.rel.rsplit('/').next().unwrap_or(&f.rel).split('.').next().unwrap_or("");
                stem.len() >= 4 && ptr.names.iter().any(|n| n.eq_ignore_ascii_case(stem))
            }
        })
        .map(|(i, _)| i)
        .collect();
    let by_file: Vec<u64> = pieces.iter().enumerate().filter(|(_, p)| named_files.contains(&p.file)).map(|(i, _)| i as u64).collect();

    // 2. Pieces that define a name the request mentions, then those that use
    //    it.
    let mut defining = Vec::new();
    let mut using = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        let def = ptr.names.iter().filter(|n| defines(&p.text, n)).count();
        let uses = ptr.names.iter().filter(|n| mentions(&p.text, n)).count();
        if def > 0 {
            defining.push((i as u64, def));
        } else if uses > 0 {
            using.push((i as u64, uses));
        }
    }
    defining.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    using.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let by_name: Vec<u64> = defining.iter().chain(using.iter()).map(|(i, _)| *i).collect();

    // 3. BM25 over the pieces, names split into their words.
    let mut idx = crate::bm25::Index::default();
    for (i, p) in pieces.iter().enumerate() {
        let title = split_name(&files[p.file].rel);
        let mut body = p.text.clone();
        let mut extra = String::new();
        for w in p.text.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if w.len() > 3 && (w.contains('_') || w.chars().skip(1).any(|c| c.is_uppercase())) {
                extra.push(' ');
                extra.push_str(&split_name(w));
            }
        }
        body.push_str(&extra);
        idx.add(i as u64, &title, &body);
    }
    let query = format!("{request_low} {}", ptr.names.iter().map(|n| split_name(n)).collect::<Vec<_>>().join(" "));
    let by_words: Vec<u64> = idx.search(&query, 60).into_iter().map(|(id, _)| id).collect();

    // Named things lead: a piece the request points at outranks one that
    // only shares its words, so those lists are counted twice.
    let fused = crate::bm25::rrf(&[by_file.clone(), by_file, by_name.clone(), by_name, by_words], crate::bm25::RRF_K);

    // Take the best until the budget is spent.
    const HEADING: &str = "The parts of it that matter here:\n";
    let room = budget.saturating_sub(tokens_in(&text) + tokens_in(HEADING));
    let mut used = 0usize;
    let mut chosen: Vec<usize> = Vec::new();
    for (id, _) in fused {
        let p = &pieces[id as usize];
        let cost = tokens_in(&p.text) + tokens_in(&files[p.file].rel) + 12;
        if used + cost > room {
            continue;
        }
        used += cost;
        chosen.push(id as usize);
    }
    if chosen.is_empty() {
        let tokens = tokens_in(&text);
        return Reading { text, files: vec![], tokens };
    }
    // Which files, best first, for the caller.
    let mut order: Vec<String> = Vec::new();
    for &c in &chosen {
        let rel = &files[pieces[c].file].rel;
        if !order.contains(rel) {
            order.push(rel.clone());
        }
    }
    // Printed file by file, in line order, neighbours run together.
    let mut per_file: HashMap<usize, Vec<usize>> = HashMap::new();
    for &c in &chosen {
        per_file.entry(pieces[c].file).or_default().push(c);
    }
    text.push_str(HEADING);
    for rel in &order {
        let fi = files.iter().position(|f| &f.rel == rel).unwrap_or(0);
        let mut ids = per_file.remove(&fi).unwrap_or_default();
        ids.sort_by_key(|&c| pieces[c].start);
        let mut run: Option<(usize, usize, String)> = None;
        let mut runs = Vec::new();
        for c in ids {
            let p = &pieces[c];
            match &mut run {
                Some((_, end, body)) if p.start <= *end + 1 => {
                    *end = (*end).max(p.end);
                    body.push('\n');
                    body.push_str(&p.text);
                }
                _ => {
                    if let Some(r) = run.take() {
                        runs.push(r);
                    }
                    run = Some((p.start, p.end, p.text.clone()));
                }
            }
        }
        runs.extend(run);
        for (start, end, body) in runs {
            text.push_str(&format!("--- {rel} (lines {start}-{end}) ---\n{}\n", body.trim_end()));
        }
    }
    let tokens = tokens_in(&text);
    Reading { text, files: order, tokens }
}
