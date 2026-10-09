//! The one rule for "does anything call this", shared.
//!
//! This lived twice: byte-identical copies in `tests/dead_capabilities.rs` and
//! `tests/new_capabilities_are_wired.rs`, plus a third in `tests/bug_sweep.rs`
//! until the summed ceiling was removed. Those files keep two lists that have
//! to agree about which functions are dead, and they were agreeing by the two
//! copies happening to be the same text.
//!
//! This repo has the receipts for what happens next: the detector was
//! corrected four times in one day, and the two numbers built on it came to
//! disagree. A rule copied is a rule that drifts.
//!
//! One copy, two readers. Nothing here knows about lists or ceilings — it
//! answers one question and the callers decide what it means.

#![allow(dead_code)]

/// Does this text use `name` — call it, or pass it by name?
///
/// Two shapes count, and the second was missing. `name(` is a call.
/// `::name` not followed by `(` is a function passed as a value, which is how
/// `daemon.rs` reaches `nudge::link_broke`: `.map(crate::nudge::link_broke)`.
/// Looking only for the parenthesis reported that function, and others like
/// it, as never called while a real call site sat three characters away.
///
/// Comment lines are skipped. Every name in this tree appears in prose
/// somewhere — usually in the doc comment explaining why it is not wired yet
/// — and counting that as a caller means a note about dead code is enough to
/// make the code look alive.
pub fn calls(text: &str, name: &str) -> bool {
    let call = format!("{name}(");
    let referenced = format!("::{name}");
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        if whole_word(line, &call) {
            return true;
        }
        if let Some(rest) = after(line, &referenced) {
            // `::name` followed by an identifier character is a longer name.
            if !rest.chars().next().map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false) {
                return true;
            }
        }
    }
    false
}

/// `needle` present, and not as the tail of a longer identifier.
pub fn whole_word(line: &str, needle: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(i) = line[from..].find(needle) {
        let at = from + i;
        match if at == 0 { None } else { Some(bytes[at - 1]) } {
            Some(c) if c.is_ascii_alphanumeric() || c == b'_' => {}
            _ => return true,
        }
        from = at + 1;
    }
    false
}

fn after<'a>(line: &'a str, needle: &str) -> Option<&'a str> {
    line.find(needle).map(|i| &line[i + needle.len()..])
}

use std::collections::HashSet;

/// Every name `calls` would answer `true` for in this text, computed once.
///
/// **Why this exists: the three guards built on `calls` were costing 131
/// seconds of every verification run.** Measured 17 Sep 2026:
///
/// ```text
/// dead_capabilities            60.52s
/// capability_wiring            41.58s
/// new_capabilities_are_wired   28.62s
/// ```
///
/// against 0.35s for the two guards written later, which index instead of
/// rescanning. The rule itself was never slow. The shape of its use was:
/// `calls(text, name)` walks every line of a file, and the guards call it once
/// per (file, candidate) pair, so the tree is re-read roughly a thousand times
/// over. Build the set once per file and the same question becomes a hash
/// lookup.
///
/// **This must agree with `calls` exactly**, or the ceilings move for a reason
/// that has nothing to do with the code — the failure this tree has a
/// documented history of. `the_index_agrees_with_the_rule_it_replaces` in
/// `tests/guards.rs` pins that across the whole source tree; it is the only
/// thing making this safe to use.
///
/// A slow guard is a guard someone eventually stops running, which makes it a
/// comment. That is the real cost, not the minutes.
/// **Blind spot worth knowing about: a function passed as a value.**
///
/// This finds a call by looking for `name(`. So `.map(sentences_for)`,
/// `.and_then(parse_it)`, or storing a `fn` in a table is not seen as a
/// caller, and a function used only that way is reported dead by every guard
/// built on this — `dead_capabilities.rs`, `dead_methods.rs`,
/// `new_capabilities_are_wired.rs`, `bug_sweep.rs`.
///
/// The dangerous direction is not a false alarm: it is somebody acting on
/// one. A function reported dead invites deletion, and this one would still
/// compile away only at the call site that passes it, so the fix looks safe
/// and is not. It happened on 18 Sep to `modes::sentences_for`, which was
/// called from `daemon.rs` on the line above the report.
///
/// Until this parses Rust rather than text, the rule is: write
/// `.map(|v| f(v))` in `src/`, not `.map(f)`.
pub fn called_names(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        let b = line.as_bytes();

        // `name(` -- a call. Whole-word, matching `whole_word`'s rule: the
        // character before must not be an identifier character.
        let mut i = 0;
        while let Some(rel) = line[i..].find('(') {
            let at = i + rel;
            let mut s = at;
            while s > 0 {
                let c = b[s - 1];
                if c.is_ascii_alphanumeric() || c == b'_' {
                    s -= 1;
                } else {
                    break;
                }
            }
            if s < at {
                out.insert(line[s..at].to_string());
            }
            i = at + 1;
        }

        // `::name` not followed by an identifier character -- a function
        // passed by name, which is how `daemon.rs` reaches `nudge::link_broke`
        // as `.map(crate::nudge::link_broke)`. Missing this shape is the bug
        // that made `calls` report live functions as dead.
        let mut j = 0;
        while let Some(rel) = line[j..].find("::") {
            let at = j + rel;
            let start = at + 2;
            let mut e = start;
            while e < b.len() {
                let c = b[e];
                if c.is_ascii_alphanumeric() || c == b'_' {
                    e += 1;
                } else {
                    break;
                }
            }
            if start < e {
                let follows_ok = b
                    .get(e)
                    .map(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
                    .unwrap_or(true);
                if follows_ok {
                    out.insert(line[start..e].to_string());
                }
            }
            j = at + 2;
        }
    }
    out
}
/// A call, not a mention of a path.
///
/// `calls` accepts two shapes: `name(` and a bare `::name` reference. The
/// second is right inside one crate, where `foo::bar` in a `use` is evidence
/// that something reaches for it.
///
/// Across a crate boundary it is wrong, and wrong in a way that quietly
/// resurrects dead code: a `use other_crate::against::run_against` would match
/// `otherside::against`, and a method call to another type's `.rules()` would
/// match `publishing::rules`. Functions look alive because other code happened
/// to reuse the word.
///
/// So a cross-crate scan asks for an invocation. It does not fix the method
/// collision — `id.rules()` is a real call to a different `rules`, and no
/// name-based rule can tell those apart; that is what the main tree's
/// `name_collisions.rs` is for. It does fix every collision with a module
/// path, which is the larger and dumber half.
pub fn calls_invocation(text: &str, name: &str) -> bool {
    let call = format!("{name}(");
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .any(|l| whole_word(l, &call))
}

/// Split a source file into the program and its inline `#[cfg(test)]` blocks.
///
/// # Why this exists
///
/// `dead_capabilities.rs` reads `tests/` to decide what is tested, and reads
/// whole source files to decide what is called. A `#[cfg(test)] mod tests`
/// inside `src` is invisible to the first and counted by the second, which
/// gets the answer wrong three separate ways:
///
/// 1. a function called only by **another** module's inline test looked like
///    it had a production caller, so it was dropped from the measurement
///    entirely;
/// 2. a function called only by **its own** inline test looked like a private
///    helper (`own = true`), because the test lives in the same file;
/// 3. and neither counted as *tested*, so the honest classification —
///    "proven, never reached by the program" — was unreachable.
///
/// Together those made a tested function look like an untested helper: the
/// opposite of true, in 36 of 275 files. Found by adding a unit test beside
/// the code and watching `HELPER_UNTESTED_MAX` fail to move.
///
/// # The rule, and why it is this one rather than a brace matcher
///
/// A block starts at a line beginning `#[cfg(test)]` at column zero and ends
/// at the next line that is exactly `}`. That is the shape every one of the
/// 36 files uses, and `tests/the_splitter_is_honest.rs` asserts it of all of
/// them rather than trusting it.
///
/// A real brace matcher was written first and discarded. Rust needs a small
/// lexer to count braces safely — raw strings, nested block comments, and
/// `'a` lifetimes that are not char literals, all of which appear in this
/// tree — and a subtly wrong lexer silently mis-slices a file, which is a
/// worse failure than the one being fixed. A rule that is simple enough to
/// check exhaustively, plus a test that checks it exhaustively, is the safer
/// trade.
///
/// A doc comment that merely mentions the attribute is left alone, because
/// the line must *begin* with it.
pub fn split_production_and_tests(text: &str) -> (String, String) {
    // An inner file attribute applies to the whole module, including a
    // private test module loaded through #[path] by its production parent.
    if text.lines().take_while(|line| line.trim().is_empty() || line.starts_with("//") || line.starts_with("#!"))
        .any(|line| line.trim_end() == "#![cfg(test)]") {
        return (String::new(), text.to_string());
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let (mut prod, mut tests) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("#[cfg(test)]") {
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim_end() != "}" {
                j += 1;
            }
            assert!(
                j < lines.len(),
                "an inline `#[cfg(test)]` block starting at line {} never closes with `}}` at \
                 column zero. The splitter's rule no longer matches this tree -- fix the rule \
                 and `tests/the_splitter_is_honest.rs` together, rather than letting the \
                 deadness numbers quietly go wrong again.",
                i + 1
            );
            tests.extend_from_slice(&lines[i..=j]);
            i = j + 1;
            continue;
        }
        prod.push(lines[i]);
        i += 1;
    }
    (prod.join("\n"), tests.join("\n"))
}

// ---------------------------------------------------------------------------
// Carried over from the improvements tree, 18 Sep 2026, when the two trees
// were collapsed into this one. It was the only thing that side had which
// this one did not.
// ---------------------------------------------------------------------------

/// Free functions the deadness scan calls "reached" on the strength of a bare
/// name match it cannot resolve.
///
/// The argument is in `tests/one_word_is_not_an_address.rs`. In short: a
/// function not inside an `impl` block, sharing its name with a free function
/// in another module, not called within its own module, and reached only by
/// unqualified `name(` calls — no caller anywhere writes `module::name(`.
///
/// Methods are excluded deliberately. `book.save()` is a real call that no
/// regex can attribute, and a list containing every one of those (614 here)
/// would be a list nobody could act on — the failure this suite keeps
/// relearning.
pub fn ambiguous_free_functions() -> std::collections::BTreeSet<String> {
    use std::collections::{BTreeMap, BTreeSet};

    fn read(dir: &str) -> Vec<(String, String)> {
        fn walk(d: &std::path::Path, base: &std::path::Path, out: &mut Vec<(String, String)>) {
            let Ok(entries) = std::fs::read_dir(d) else { return };
            let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                if p.is_dir() {
                    walk(&p, base, out);
                } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                    if let Ok(t) = std::fs::read_to_string(&p) {
                        let name = p
                            .strip_prefix(base)
                            .unwrap_or(&p)
                            .with_extension("")
                            .to_string_lossy()
                            .replace('\\', "/");
                        out.push((name, t));
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(std::path::Path::new(dir), std::path::Path::new(dir), &mut out);
        // A split module's pieces are one module (27 Sep 2026); none yet.
        fold_split_modules(out)
    }

    let src = read("src");
    let clean: BTreeMap<String, String> = src
        .iter()
        .map(|(m, t)| {
            (
                m.clone(),
                t.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n"),
            )
        })
        .collect();

    // Free functions only: track whether we are inside an `impl` block.
    let mut defs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (module, body) in &src {
        let mut depth: i32 = 0;
        let mut impl_at: Option<i32> = None;
        for line in body.lines() {
            let t = line.trim_start();
            if impl_at.is_none() && (t.starts_with("impl ") || t.starts_with("impl<")) {
                impl_at = Some(depth);
            }
            if impl_at.is_none() {
                for pre in ["pub fn ", "pub(crate) fn "] {
                    if let Some(rest) = t.strip_prefix(pre) {
                        let n: String =
                            rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                        if !n.is_empty() {
                            defs.entry(n).or_default().insert(module.clone());
                        }
                        break;
                    }
                }
            }
            depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
            if let Some(a) = impl_at {
                if depth <= a {
                    impl_at = None;
                }
            }
        }
    }

    let mut out = BTreeSet::new();
    for (name, where_) in &defs {
        if where_.len() < 2 {
            continue;
        }
        for module in where_ {
            let stem = module.rsplit('/').next().unwrap_or(module);
            let own = &clean[module];
            let def_a = format!("pub fn {name}(");
            let def_b = format!("pub(crate) fn {name}(");
            let body_wo_def: String = own
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    !t.starts_with(&def_a) && !t.starts_with(&def_b)
                })
                .collect::<Vec<_>>()
                .join("\n");
            if calls_invocation(&body_wo_def, name) {
                continue; // its own module uses it
            }
            let callers: Vec<&String> = clean
                .iter()
                .filter(|(o, t)| *o != module && calls_invocation(t, name))
                .map(|(o, _)| o)
                .collect();
            if callers.is_empty() {
                continue; // already counted as dead by the other scans
            }
            let qualified = format!("{stem}::{name}(");
            if callers.iter().any(|o| whole_word(&clean[*o], &qualified)) {
                continue; // somebody says which one they mean
            }
            out.insert(format!("{stem}::{name}"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Reading a module's source as text, wherever its files live.
// ---------------------------------------------------------------------------
//
// Added 27 Sep 2026 to prepare the daemon.rs / main.rs split. About 150 tests
// read `src/daemon.rs` or `src/main.rs` as text and look for a function body
// or a string in it. The day `daemon.rs` becomes `daemon.rs` plus
// `daemon/late.rs`, `daemon/tick.rs`, ... every one of those tests would stop
// seeing the code that moved, and a test that looks for a string and no
// longer finds the file it lives in fails -- or, worse, a test that asserts a
// string is ABSENT passes for the wrong reason. Reading through these helpers
// means the split moves code without moving what the tests can see.
//
// Today no `src/<name>/` folder exists for any of the files the tests read, so
// `source_of(name)` returns exactly the text `read_to_string("src/<name>.rs")`
// did, byte for byte.

/// The files that make up module `name`, in reading order: `src/<name>.rs`
/// (or `src/<name>/mod.rs` when that is how the module is laid out), then every
/// `.rs` file under `src/<name>/`, sorted by path, descending into subfolders.
///
/// `name` may be a path under `src` (`"platform"`, `"market/book"`), with or
/// without a trailing `.rs`, and with or without a leading `src/`.
pub fn source_files_of(name: &str) -> Vec<std::path::PathBuf> {
    let name = name.trim_start_matches("src/").trim_end_matches(".rs");
    let root = std::path::Path::new("src");
    let file = root.join(format!("{name}.rs"));
    let dir = root.join(name);
    let mut out = Vec::new();
    if file.is_file() {
        out.push(file);
    }
    let mut under = Vec::new();
    collect_rs(&dir, &mut under);
    under.sort();
    // `mod.rs` is the module's root when there is no `<name>.rs`; read it first.
    if let Some(i) = under.iter().position(|p| p == &dir.join("mod.rs")) {
        let m = under.remove(i);
        out.insert(0, m);
    }
    out.extend(under);
    out
}

fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for p in entries.flatten().map(|e| e.path()) {
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// The whole source of module `name` as one text: see `source_files_of` for
/// which files and in what order. A file that does not end in a newline gets
/// one before the next file starts, so no two lines are ever glued together.
///
/// **Text the module builds in from `assets/` comes after it.** The hub's
/// stylesheet and scripts were ~1,000 lines of string constants in hub.rs
/// until 27 Sep 2026, when they moved to `assets/hub/*` behind `include_str!`.
/// They are still the hub's text -- in the page it serves and in what the
/// tests read -- so a guard looking for `pointerdown` or a colour token in
/// hub.rs keeps finding it. Only `assets/` is followed: `config/*.yaml` is
/// data the module ships, not its code.
///
/// Panics when the module has no file at all -- a test reading a module that
/// is gone should fail by saying so, not scan an empty string and pass.
pub fn source_of(name: &str) -> String {
    let mut files = source_files_of(name);
    let mut assets = Vec::new();
    for f in &files {
        for a in included_assets(f) {
            if !assets.contains(&a) {
                assets.push(a);
            }
        }
    }
    files.extend(assets);
    assert!(
        !files.is_empty(),
        "no source for module `{name}`: neither src/{n}.rs nor anything under src/{n}/",
        n = name.trim_start_matches("src/").trim_end_matches(".rs")
    );
    let mut out = String::new();
    for f in &files {
        let text = std::fs::read_to_string(f)
            .unwrap_or_else(|e| panic!("reading {}: {e}", f.display()));
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        // Source guards parse logical lines. Windows checkouts may contain
        // CRLF even when the same source is LF on CI.
        out.push_str(&text.replace("\r\n", "\n"));
    }
    out
}

/// `source_of` for a path a test names the old way (`"src/daemon.rs"`), and
/// a plain read for anything outside `src/` (`"tests/wiring.rs"`,
/// `"assets/hub/hub.css"`). `None` when nothing is there.
pub fn read_source_path(path: &str) -> Option<String> {
    let under_src = path.starts_with("src/") && path.ends_with(".rs");
    if under_src && !source_files_of(path).is_empty() {
        return Some(source_of(path));
    }
    std::fs::read_to_string(path).ok()
}

/// Every `.rs` file under `src/`, recursively, sorted, as
/// `(module path without .rs, relative to src -- "daemon", "platform/mod",
/// "daemon/late"; text)`.
pub fn source_file_set() -> Vec<(String, String)> {
    let mut files = Vec::new();
    collect_rs(std::path::Path::new("src"), &mut files);
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let name = p
                .strip_prefix("src")
                .unwrap_or(&p)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            Some((name, text))
        })
        .collect()
}

/// For a file inside a split module's folder -- `src/daemon/late.rs` when
/// `src/daemon.rs` exists -- the parent module's name (`"daemon"`). `None` for
/// every other path, including `src/platform/*.rs` (no `src/platform.rs`) and
/// top-level files. Descends: `src/daemon/tick/x.rs` is still `daemon`.
///
/// For whole-tree guards that key results by module: after the split the
/// children must count as the module they came out of, or every list keyed
/// `daemon::...` goes stale and every call between two child files looks like
/// a call from another module.
pub fn split_parent(path: &std::path::Path) -> Option<String> {
    let rel = path.strip_prefix("src").ok()?;
    let mut parts = rel.components();
    let first = parts.next()?.as_os_str().to_str()?.to_string();
    parts.next()?; // must be inside a folder
    if std::path::Path::new("src").join(format!("{first}.rs")).is_file() {
        Some(first)
    } else {
        None
    }
}

/// Is this (already trimmed) line the start of a function definition, at any
/// visibility and with any qualifiers? `fn x(`, `pub fn x(`,
/// `pub(crate) fn x(`, `pub(super) fn x(`, `pub(in crate::daemon) fn x(`,
/// `pub const fn`, `async fn`, `unsafe fn`, `pub(crate) async unsafe fn` ...
pub fn is_fn_definition(t: &str) -> bool {
    let mut rest = t;
    if let Some(r) = rest.strip_prefix("pub") {
        if let Some(r) = r.strip_prefix(' ') {
            rest = r;
        } else if r.starts_with('(') {
            match r.find(')') {
                Some(close) => rest = r[close + 1..].trim_start(),
                None => return false,
            }
        } else {
            return false; // `public_thing(` is not a definition
        }
    }
    loop {
        let before = rest;
        for q in ["const ", "async ", "unsafe ", "default "] {
            if let Some(r) = rest.strip_prefix(q) {
                rest = r;
            }
        }
        if let Some(r) = rest.strip_prefix("extern ") {
            // `extern "C" fn`
            rest = r.trim_start();
            if rest.starts_with('"') {
                if let Some(end) = rest[1..].find('"') {
                    rest = rest[end + 2..].trim_start();
                }
            }
        }
        if rest == before {
            break;
        }
    }
    rest.starts_with("fn ")
}

/// Fold the children of a split module into the module, for a list keyed by
/// path under `src` (`"daemon"`, `"daemon/late"`, `"platform/win"`).
///
/// An entry `"<p>/..."` joins entry `"<p>"` when `"<p>"` itself is in the
/// list -- that is, when `src/<p>.rs` exists beside `src/<p>/`. Folders with no
/// file of their own (`platform/`, `market/`) are left exactly as they are.
/// The first entry keeps its position; the texts are joined with a newline.
pub fn fold_split_modules(entries: Vec<(String, String)>) -> Vec<(String, String)> {
    let tops: std::collections::HashSet<String> =
        entries.iter().filter(|(m, _)| !m.contains('/')).map(|(m, _)| m.clone()).collect();
    let mut out: Vec<(String, String)> = Vec::new();
    for (m, text) in entries {
        let key = match m.split_once('/') {
            Some((p, _)) if tops.contains(p) => p.to_string(),
            _ => m,
        };
        if let Some(e) = out.iter_mut().find(|(k, _)| *k == key) {
            e.1.push('\n');
            e.1.push_str(&text);
        } else {
            out.push((key, text));
        }
    }
    out
}

/// The `assets/...` files a source file builds in with `include_str!`,
/// resolved against the crate root, in the order they appear.
pub fn included_assets(file: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let dir = file.parent().unwrap_or(std::path::Path::new("."));
    let mut out = Vec::new();
    for part in text.split("include_str!(\"").skip(1) {
        let Some(rel) = part.split('"').next() else { continue };
        // Normalise `src/../assets/hub/hub.css` to `assets/hub/hub.css`.
        let mut path = std::path::PathBuf::new();
        for c in dir.join(rel).components() {
            match c {
                std::path::Component::ParentDir => {
                    path.pop();
                }
                std::path::Component::CurDir => {}
                other => path.push(other),
            }
        }
        if path.starts_with("assets") && path.is_file() && !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// Add a file to a module list keyed by name (stem or path), folding a split
/// module's pieces into one entry: `src/daemon.rs` and every file under
/// `src/daemon/` all land in the entry keyed `daemon`, whichever arrives
/// first. Every other file is pushed under the name the caller gave it,
/// exactly as before. Today no module is split, so this is a plain push.
pub fn push_module(
    out: &mut Vec<(String, String)>,
    path: &std::path::Path,
    name: String,
    text: String,
) {
    let top_of_split = path.parent() == Some(std::path::Path::new("src"))
        && path.with_extension("").is_dir();
    let key = split_parent(path).or_else(|| {
        top_of_split.then(|| path.file_stem().unwrap_or_default().to_string_lossy().to_string())
    });
    match key {
        Some(k) => match out.iter_mut().find(|(m, _)| *m == k) {
            Some(e) => {
                e.1.push('\n');
                e.1.push_str(&text);
            }
            None => out.push((k, text)),
        },
        None => out.push((name, text)),
    }
}

/// A stand-in program that prints `text`, on any platform (6 Oct 2026).
///
/// Tests faked the search and fetch tools with `sh -c "echo '...'"`. Windows
/// has no `sh`, so there the stand-in failed to start, research found "no
/// sources", and the tests failed for a reason that had nothing to do with
/// them. The text goes in a file and the file is printed (`cat`, or `cmd /C
/// type`), because Windows quotes an argument with spaces and `echo` would
/// print the quotes too.
pub fn printing(text: &str) -> atlas::tools::ExternalTool {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("atlas-printing-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder for the stand-in's text");
    let file = dir.join(format!("{n}.txt"));
    std::fs::write(&file, format!("{text}\n")).expect("the stand-in's text");
    let file = file.display().to_string();
    if cfg!(windows) {
        atlas::tools::ExternalTool { command: "cmd".into(), args: vec!["/C".into(), "type".into(), file], ..Default::default() }
    } else {
        atlas::tools::ExternalTool { command: "cat".into(), args: vec![file], ..Default::default() }
    }
}

// ---------------------------------------------------------------------------
// Wall-clock bounds that hold on a busy machine (Q19, 8 Oct 2026)
// ---------------------------------------------------------------------------
//
// About forty tests say "this must not wait more than N ms". They were right
// to: the A2 pass on 7 Oct found two real loop stalls (a 2.4 s ffmpeg listing,
// a `tasklist` waited on before a CPU sample) because a 2 s bound caught them.
// But on a laptop that was also compiling and running Atlas's own self-test,
// 19 of them failed and every one passed alone -- the bound was measuring how
// busy the machine was, not whether the code waited.
//
// The remedy keeps the bound and asks the machine how far behind it is. A
// sleeping thread is woken late in proportion to the contention, so a few
// short sleeps give a factor to widen the bound by. On an idle machine the
// factor is about 1 and the bound is the bound: a 2.4 s stall still fails a
// 2 s test. Only a machine that is itself running late is given the slack,
// and a failure says how much, so a real regression can't hide behind it.

use std::time::{Duration, Instant};

/// The widening for a machine this late: 1 while a short sleep overruns by no
/// more than ordinary timer noise (2 ms), then one more for every 5 ms past
/// that, at most 8.
pub fn load_factor_for(overran: Duration) -> f64 {
    let late_ms = (overran.as_secs_f64() * 1000.0 - 2.0).max(0.0);
    (1.0 + late_ms / 5.0).clamp(1.0, 8.0)
}

/// How late this machine is running right now (>= 1.0). Measured about once each
/// second: four 5 ms sleeps, worst overrun, so a measurement costs about 20 ms.
pub fn load_factor() -> f64 {
    // Measured at most once a second: a polling loop asks on every pass, and
    // twenty milliseconds of sleeping per pass would change what it measures.
    static LAST: std::sync::Mutex<Option<(Instant, f64)>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, f)) = *last {
        if at.elapsed() < Duration::from_secs(1) {
            return f;
        }
    }
    let f = measure_load_factor();
    *last = Some((Instant::now(), f));
    f
}

fn measure_load_factor() -> f64 {
    let mut worst = Duration::ZERO;
    for _ in 0..4 {
        let t = Instant::now();
        std::thread::sleep(Duration::from_millis(5));
        worst = worst.max(t.elapsed().saturating_sub(Duration::from_millis(5)));
    }
    load_factor_for(worst)
}

/// `bound`, widened by how late the machine is running now.
pub fn allowed(bound: Duration) -> Duration {
    bound.mul_f64(load_factor())
}

/// Did `took` stay within `bound` on a machine running at `factor`?
pub fn is_prompt(took: Duration, bound: Duration, factor: f64) -> bool {
    took <= bound.mul_f64(factor)
}

/// Assert that something that should not have waited did not, allowing for a
/// busy machine. The message names the factor that was applied.
#[track_caller]
pub fn assert_prompt(took: Duration, bound: Duration, what: &str) {
    if took <= bound {
        return; // on time without any slack: the common case, and no sleeping
    }
    let factor = load_factor();
    assert!(
        is_prompt(took, bound, factor),
        "{what}: took {took:?}, over the {bound:?} bound (widened x{factor:.1} for this machine's load)"
    );
}
