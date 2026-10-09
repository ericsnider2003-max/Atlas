//! One record name, one type.
//!
//! ## The defect this exists for
//!
//! `store` is a key-value namespace: `store.save("thread", &self.thread)`
//! writes `data/state/thread.json`. Nothing guarded that namespace, and on
//! 17 Sep 2026 it had a collision in it:
//!
//! * `notify::Outbox { held: Vec<Note> }` — notifications held while you are
//!   away — saved as `"outbox"`.
//! * `outbox::Outbox { replies: Vec<PendingReply> }` — client and brand
//!   replies Atlas has drafted and not sent — also saved as `"outbox"`.
//!
//! Both wrote `data/state/outbox.json`. The sequence needed no unusual
//! conditions: Atlas drafts a reply and saves `{"replies":[…]}`; on the next
//! tick `Daemon::persist` saves the other type over it as `{"held":[…]}`.
//! Neither field is `#[serde(default)]`, so the other side's next `load` fails
//! both the envelope parse and the bare fallback, `Store::preserve` renames
//! the file aside, and the caller is handed `Default::default()`.
//!
//! Every unsent draft destroyed within one tick. Every held notification
//! destroyed the first time a draft was saved. And both modules carried a
//! comment boasting that this exact class of loss had been fixed — each had
//! fixed it for the case it was looking at.
//!
//! ## Why the existing guards could not see it
//!
//! `name_collisions.rs` watches *function* names, and watches them because a
//! bare-name match makes a dead function look alive. This is the same hazard
//! one namespace along, and it is worse: a function-name collision misleads a
//! guard, a record-name collision **loses the user's data**.
//!
//! ## What this checks
//!
//! Every `store.load("x")` and `store.save("x", …)` in `src/`, grouped by
//! name. A name used from two different modules is a collision unless it is
//! on `SHARED_ON_PURPOSE` with a reason — because two modules reading one
//! record is legitimate (a writer and a reporter) and two modules *writing*
//! different shapes to it is not.

use std::collections::BTreeMap;
use std::path::Path;

/// Record names deliberately touched from more than one module.
///
/// Each entry must say why. The bar: the two modules agree on the **type**,
/// so one of them is reading what the other wrote rather than writing a
/// different shape to the same file.
const SHARED_ON_PURPOSE: &[(&str, &str)] = &[
    (
        "undo_history",
        "The daemon and its file-move worker use the same safety::History type. Each delta save strictly reloads and merges the current history under the root transaction; the worker records completed moves before reporting completion.",
    ),
    (
        "dropped",
        "Same field, the daemon's own `dropped` list: the hub's Outstanding page takes an item off it \
         (Chat A, 2 Oct 2026) and saves it exactly as `daemon/late.rs` does -- one value, two places it changes.",
    ),
    (
        "synclog",
        "Same type (sync::Log), one writer at a time: `atlas doc` takes queued edits into the log only \
         while it holds the OnlyOne lock, which a running daemon holds for its whole life, so the CLI \
         writes only when no daemon is running (round 4, 23 Sep 2026).",
    ),
    (
        "security_changes",
        "the trail of security changes: `atlas walkthrough yes` (main) and a spoken yes (daemon) both append a \
         `confirmed::Record` made by `confirmed::record` — one type, one trail, two doors into it",
    ),
];

/// Records read by something that runs, and written by nothing.
///
/// A named list rather than a silence, because each one is a feature
/// reporting on a file that will never exist — and the cost is not an error,
/// it is a **confident wrong answer**. Both entries here were found by the
/// test below and both now say they cannot tell you, instead of reporting
/// zero. The reason is what earns the entry; "it isn't finished" is not a
/// reason on its own.
///
/// An entry comes off this list when something writes the record. The
/// companion tests named in each reason fail if the note and the wiring ever
/// disagree, so the caveat is deleted by the change that makes it false.
const READ_ONLY_FOR_NOW: &[(&str, &str)] = &[
    (
        "budget_ledger",
        "`Ledger::record` has no caller in src/, so the spend total is always zero \
         and the daily and monthly caps bound one job at a time rather than a \
         total. `budget::untracked()` says so in the report and in every refusal; \
         tests/the_budget_knows_what_it_does_not_know.rs holds the two together",
    ),
    (
        "phone_mirror",
        "there is no phone-side client to capture into it and no sync to save it, \
         so `mirrored_at` is always None. `companion::unbuilt()` says so rather \
         than letting it read as a fact about the phone; \
         tests/the_mirror_knows_it_was_never_filled.rs holds the two together",
    ),
    (
        "facts",
        "the pre-sharding single-file fact store, kept only as a one-way upgrade \
         source. `Book::load` reads it solely when no `facts-0..15` shard exists, \
         and immediately marks every shard dirty so the first save moves the whole \
         book into the shards -- which are what `Book::save` writes from then on. \
         It reports no default as a measurement: an absent legacy key just means \
         an empty book built from shards. Added 21 Sep 2026 with the sharded \
         persistence; it should never gain a writer, because writing `facts` \
         again would recreate the single file this replaced",
    ),
];

/// A `(record, module, call)` for every store access in the tree.
fn accesses() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![Path::new("src").to_path_buf()];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for e in std::fs::read_dir(&p).expect("readable dir").flatten() {
                stack.push(e.path());
            }
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        // A file inside a split module's folder is that module (29 Sep 2026):
        // `src/daemon/tick.rs` and `src/daemon/running.rs` are both `daemon`.
        // Keyed by file stem, the split of daemon.rs made one module read as
        // several, and a record the daemon saves from two places looked
        // like two modules writing it.
        let module = crate::common::split_parent(&p)
            .unwrap_or_else(|| p.file_stem().unwrap().to_string_lossy().to_string());
        let raw = std::fs::read_to_string(&p).unwrap_or_default();
        let (text, _) = crate::common::split_production_and_tests(&raw);
        for line in text.lines() {
            let t = line.trim_start();
            // Comments quote record names when they explain a collision --
            // including the one in `notify.rs` that explains this very
            // guard. Reading them as code makes the fix fail its own test.
            if t.starts_with("//") {
                continue;
            }
            for call in [".load(\"", ".load_checked(\"", ".save(\""] {
                let mut from = 0;
                while let Some(at) = line[from..].find(call) {
                    let start = from + at + call.len();
                    let Some(end) = line[start..].find('"') else { break };
                    let name = &line[start..start + end];
                    // A record name is a plain identifier. Anything else is
                    // some other `.load(`/`.save(` — a config loader, a
                    // model file — and not this namespace.
                    if !name.is_empty()
                        && name.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
                    {
                        out.push((
                            name.to_string(),
                            module.clone(),
                            if call.starts_with(".load") { "load".into() } else { "save".into() },
                        ));
                    }
                    from = start + end;
                }
            }
        }
    }
    out
}

#[test]
fn no_two_modules_write_the_same_record() {
    let mut writers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (record, module, call) in accesses() {
        if call != "save" {
            continue;
        }
        let e = writers.entry(record).or_default();
        if !e.contains(&module) {
            e.push(module);
        }
    }

    let allowed: Vec<&str> = SHARED_ON_PURPOSE.iter().map(|(r, _)| *r).collect();
    let mut clashes = Vec::new();
    for (record, mods) in &writers {
        if mods.len() > 1 && !allowed.contains(&record.as_str()) {
            clashes.push(format!("{record}.json  <-  {}", mods.join(", ")));
        }
    }

    assert!(
        clashes.is_empty(),
        "these state records are written by more than one module, so each save \
         overwrites the other module's data and the next load of either finds a \
         shape it cannot parse:\n  {}\n\nRename one of them, or add the record to \
         SHARED_ON_PURPOSE with the reason the two modules agree on the type.",
        clashes.join("\n  ")
    );
}

#[test]
fn the_collision_that_prompted_this_is_gone() {
    // Named rather than left to the general rule, so a future rename cannot
    // quietly put these two back together.
    let notify = std::fs::read_to_string("src/notify.rs").expect("notify.rs");
    let outbox = std::fs::read_to_string("src/outbox.rs").expect("outbox.rs");

    let saves = |src: &str| -> Vec<String> {
        src.lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .filter_map(|l| {
                let at = l.find(".save(")?;
                let rest = &l[at + 6..];
                if let Some(q) = rest.strip_prefix('"') {
                    return q.split('"').next().map(str::to_string);
                }
                // `store.save(RECORD, self)` -- a constant, which is fine
                // and is what the fix used.
                rest.split(',').next().map(|s| s.trim().to_string())
            })
            .collect()
    };

    let a = saves(&notify);
    let b = saves(&outbox);
    let shared: Vec<&String> = a.iter().filter(|x| b.contains(x)).collect();
    assert!(
        shared.is_empty(),
        "`notify` and `outbox` write the same record again: {shared:?}. They hold \
         different types -- held notifications and drafted replies -- so each save \
         destroys the other's list."
    );
}

#[test]
fn every_record_name_is_a_plain_file_name() {
    // A record name becomes a file name. A `/`, a `..` or a `\` in one would
    // put a state file somewhere other than the state folder, and this is
    // the only place the whole set is visible at once.
    let mut bad = Vec::new();
    for (record, module, _) in accesses() {
        if record.contains('/') || record.contains('\\') || record.contains("..") {
            bad.push(format!("{module}: {record:?}"));
        }
    }
    assert!(bad.is_empty(), "these record names are not plain names: {bad:?}");
}

#[test]
fn the_reader_of_a_record_is_the_module_that_writes_it() {
    // The weaker half, and reported rather than asserted into a ratchet: a
    // module that only ever READS someone else's record is usually a
    // reporter (doctor, the hub) and is fine. A module that reads a record
    // nobody writes is reading a file that will never exist, which is a
    // different bug and worth seeing.
    let all = accesses();
    let written: Vec<&String> = all.iter().filter(|(_, _, c)| c == "save").map(|(r, _, _)| r).collect();
    let named: Vec<&str> = READ_ONLY_FOR_NOW.iter().map(|(r, _)| *r).collect();
    let mut orphans = Vec::new();
    for (record, module, call) in &all {
        if call == "load" && !written.contains(&record) && !named.contains(&record.as_str()) {
            orphans.push(format!("{module} loads {record:?}, which nothing saves"));
        }
    }
    orphans.sort();
    orphans.dedup();
    assert!(
        orphans.is_empty(),
        "these records are read and never written, so whatever reads them always \
         gets a default -- and a default reported as a measurement is a confident \
         wrong answer:\n  {}\n\nWire a writer, or add the record to \
         READ_ONLY_FOR_NOW with what it costs and what says so.",
        orphans.join("\n  ")
    );

    // Both directions. A record on the list that HAS gained a writer means
    // the caveat attached to it is now false and should come off with it.
    let mut fixed = Vec::new();
    for (record, why) in READ_ONLY_FOR_NOW {
        if written.iter().any(|w| w.as_str() == *record) {
            fixed.push(format!("{record} -- {why}"));
        }
    }
    assert!(
        fixed.is_empty(),
        "these are on READ_ONLY_FOR_NOW and now have a writer. Good -- take them \
         off the list, and delete the caveat the reason names:\n  {}",
        fixed.join("\n  ")
    );
}
