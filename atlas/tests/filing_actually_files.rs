//! Two ways the same documents got destroyed, and the word "filed".
//!
//! ## 1. `atlas file --do-it` did not file anything
//!
//! The `Verdict::Go` arm created the destination's parent directory and then
//! called `trash.take(from, "filed")`. **Nothing ever moved the file to its
//! destination.** Every file the walker picked from `Downloads`, `Desktop`
//! **and `Documents`** ended up in `data/trash/<id>-<name>`, and stdout said
//! `filed <path>`.
//!
//! ## 2. Then the hourly pass deleted them
//!
//! Those trashed files sit inside `data/`, which is the tree
//! `retention::survey` walks once an hour — and `classify` matched on the
//! extension of the whole path. A filed `photo.png` at
//! `data/trash/12-photo.png` came out `Class::Captures`, limit 24 hours. A
//! filed `.wav` came out `Class::Scratch`, limit ten minutes. `apply` then
//! removed them with `remove_file` — permanently, not through the trash,
//! **while the trash ledger still listed them**, so `atlas undo` afterwards
//! failed on a file the ledger said was there.
//!
//! So the sequence was: say "filed", move the document into Atlas's trash,
//! and permanently delete it within a day. `reclaim.rs`'s promise — *"Nothing
//! is deleted. Things are moved to Atlas's trash, which keeps them 30 days…
//! Every reclaim is reversible for a month"* — was untrue for every `.png`,
//! `.jpg` and `.wav` that went through it.
//!
//! ## 3. And an install path could delete everything
//!
//! `classify` lowercased the **absolute** path and asked whether it contained
//! `"/tmp"`. So an install root with a `tmp` component — `ATLAS_HOME`
//! pointing at `/home/eric/tmp/atlas`, or a zip unpacked to
//! `C:\Users\Eric\tmp\atlas` — made every file under `data/` `Class::Scratch`
//! with a ten-minute limit: the state, the notes, the vault, the backups, the
//! trash, the crash note. Hourly total erasure of everything Atlas knows and
//! every backup it could have been restored from, on a machine whose only
//! distinguishing feature was where it was unzipped.
//!
//! All three are one root cause seen three ways: **a classifier reading the
//! wrong string, and a mover that moved to the wrong place.**

use atlas::retention::{classify_within, plan, survey, Class, RetentionConfig};
use std::path::{Path, PathBuf};

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-filing-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("make a temp dir");
    d
}

fn touch(p: &Path) {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(p, b"some bytes").expect("write");
}

// ------------------------------------------------- the trash is off limits

#[test]
fn nothing_in_the_trash_is_retentions_business() {
    let root = tmpdir("trash");
    for name in [
        "trash/12-photo.png",
        "trash/13-recording.wav",
        "trash/14-report.pdf",
        "trash/ledger.json",
        "backups/2026-09-17/state.json",
        "backups/2026-09-17/shot.jpg",
    ] {
        let p = root.join(name);
        touch(&p);
        assert_eq!(
            classify_within(&root, &p),
            Class::NotOurs,
            "{name} is classified as retention's to delete. The trash keeps things \
             for thirty days and the backups are thinned by their own rule; a second \
             policy deleting out of either is how that promise became untrue."
        );
    }
}

#[test]
fn a_trashed_capture_is_not_deleted_after_a_day() {
    // The behavioural half: not just the class, but the plan built from it.
    let root = tmpdir("plan");
    touch(&root.join("trash/12-photo.png"));
    touch(&root.join("trash/13-clip.wav"));

    let items = survey(&root);
    assert_eq!(items.len(), 2, "the survey did not see the trashed files");

    // Two years old, so every limit in the config is long past.
    let now = items.iter().map(|i| i.modified).max().unwrap_or(0) + 730 * 86_400;
    let plans = plan(&items, &RetentionConfig::default(), now);
    let doomed: Vec<String> = plans
        .iter()
        .filter_map(|p| match p {
            atlas::retention::Plan::Delete { path, .. } => Some(path.display().to_string()),
            atlas::retention::Plan::Keep => None,
        })
        .collect();
    assert!(
        doomed.is_empty(),
        "retention proposed deleting things out of the trash: {doomed:?}. It deletes \
         with `remove_file` while the ledger still lists them, so `atlas undo` then \
         fails on a file the ledger says is there."
    );
}

// ------------------------------------------ the install path cannot poison it

#[test]
fn an_install_under_a_tmp_folder_does_not_delete_its_own_state() {
    // The worst of the three, and the one that needed no user action at all
    // beyond unzipping into the wrong place.
    let root = tmpdir("in-tmp").join("tmp").join("atlas").join("data");
    for name in ["state/vault.json", "state/thread.json", "notes/research.md", "logs/atlas.log"] {
        touch(&root.join(name));
    }

    for name in ["state/vault.json", "state/thread.json"] {
        let p = root.join(name);
        assert_eq!(
            classify_within(&root, &p),
            Class::State,
            "{name} under an install path containing `tmp` is classified as \
             scratch, so the hourly pass deletes Atlas's own state -- and every \
             backup of it -- within ten minutes"
        );
    }

    let items = survey(&root);
    let now = items.iter().map(|i| i.modified).max().unwrap_or(0) + 365 * 86_400;
    let plans = plan(&items, &RetentionConfig::default(), now);
    let doomed: Vec<String> = plans
        .iter()
        .filter_map(|p| match p {
            atlas::retention::Plan::Delete { path, .. } => Some(path.display().to_string()),
            atlas::retention::Plan::Keep => None,
        })
        .collect();
    assert!(
        !doomed.iter().any(|d| d.contains("vault.json") || d.contains("thread.json")),
        "an install under a `tmp` path proposes deleting its own state: {doomed:?}"
    );
}

#[test]
fn a_real_tmp_folder_inside_the_data_tree_is_still_scratch() {
    // So the fix is not "stop classifying anything". A genuine `data/tmp`
    // is scratch, because relative to the data root it really is `tmp/`.
    let root = tmpdir("real-tmp");
    let p = root.join("tmp/working.bin");
    touch(&p);
    assert_eq!(
        classify_within(&root, &p),
        Class::Scratch,
        "a real scratch folder inside data/ stopped being scratch"
    );
}

#[test]
fn the_classes_that_did_work_still_work() {
    // The other side of the same coin: narrowing the match must not have
    // broken the classification the module was built for.
    let root = tmpdir("classes");
    let cases = [
        ("notes/research.md", Class::Notes),
        ("logs/atlas.log", Class::Logs),
        ("logs/atlas.log.1", Class::Logs),
        ("captures/screen.png", Class::Captures),
        ("captures/frame.jpeg", Class::Captures),
        ("turn/said.wav", Class::Scratch),
        ("state/thread.json", Class::State),
    ];
    for (name, want) in cases {
        let p = root.join(name);
        touch(&p);
        assert_eq!(classify_within(&root, &p), want, "{name}");
    }
}

// ------------------------------------------------ the total stays honest

#[test]
fn the_trash_is_counted_in_the_total_even_though_it_is_not_evictable() {
    // Excluded from deletion is not the same as invisible. The trash is real
    // disk the person is paying for, and a usage total that left it out
    // would understate the folder they asked about.
    let root = tmpdir("usage");
    touch(&root.join("trash/12-big.png"));
    touch(&root.join("state/thread.json"));

    let u = atlas::retention::usage(&survey(&root));
    assert!(u.not_ours > 0, "the trash contributed nothing to the usage total");
    assert_eq!(
        u.total(),
        u.scratch + u.captures + u.logs + u.notes + u.state + u.unknown + u.not_ours,
        "the total does not add up to its parts, so one bucket is unreported"
    );
}

// --------------------------------------------- filing moves, and never over

/// The code that moves a filed file (`filing::file_one`), without comments,
/// after checking `atlas file --do-it` goes through it (29 Sep 2026).
fn file_one_body() -> String {
    let main = crate::common::source_of("main");
    let at = main.find("fn run_file(").expect("run_file is gone");
    let body = &main[at..];
    let end = body.find("\n/// Look for reclaimable space").unwrap_or(body.len());
    let run_file: String = body[..end].lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    assert!(run_file.contains("filing::file_one("), "`atlas file --do-it` doesn't move files through `filing::file_one`");
    assert!(!run_file.contains("trash.take("), "`atlas file --do-it` puts files in Atlas's trash");
    let filing = crate::common::source_of("filing");
    let at = filing.find("pub fn file_one(").expect("filing::file_one is gone");
    let body = &filing[at..];
    let end = body[1..].find("\npub fn ").map(|e| e + 1).unwrap_or(body.len());
    body[..end].lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

#[test]
fn filing_writes_the_file_to_where_it_said() {
    // The structural check, because the behavioural one needs a real home
    // directory with Downloads in it. What is asserted is the shape of the
    // code: the `Move` arm must rename to the destination, and must not hand
    // the file to the trash.
    //
    // 29 Sep 2026: the move itself is `filing::file_one`, shared with "tidy
    // my desktop"; `run_file` calls it. So the shape is checked there, and
    // `run_file` is checked to go through it.
    let code = file_one_body();

    assert!(
        code.contains("rename(from, to)"),
        "`atlas file --do-it` does not move the file to its destination. It used \
         to hand it to the trash and print \"filed\"."
    );
    assert!(
        !code.contains("trash.take("),
        "`atlas file --do-it` still puts the file in Atlas's trash. That is not \
         filing it, and the hourly retention pass then deletes anything in there \
         with a picture or audio extension."
    );
    assert!(
        code.contains("to.exists()"),
        "nothing checks whether something is already at the destination. \
         `fs::rename` replaces its destination silently on unix, so filing a \
         second `report.pdf` would destroy the first -- and the first is the one \
         that was already filed on purpose."
    );
}

#[test]
fn a_failed_cross_volume_move_does_not_leave_two_copies() {
    // The other order-of-operations hazard in the same block: copy, then
    // remove. A failure between them must not leave a duplicate with nothing
    // said about which is which.
    let code = file_one_body();

    let copy = code.find("fs::copy(from, to)").expect("no cross-volume fallback");
    let remove = code.find("remove_file(from)").expect("the fallback never removes the original");
    assert!(
        copy < remove,
        "the original is removed before the copy has succeeded, so an interrupted \
         move loses the file entirely"
    );
}
