//! Reclaiming disk space without ever being the reason something was lost.
//!
//! This is the one module in Atlas that deletes outside its own folder, so the
//! tests are about what it *refuses* far more than what it finds. A missed
//! cache costs a few hundred megabytes. A wrong deletion costs something that
//! cannot be re-downloaded.

use atlas::reclaim::{classify, forbidden, spoken, survey, Candidate, Kind, KNOWN, NEVER};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-rc-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A folder with `mb` megabytes in it, backdated by `days`.
fn make(root: &Path, rel: &str, mb: u64, days: u64) -> PathBuf {
    let dir = root.join(rel);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("blob.bin"), vec![0u8; (mb * 1024 * 1024) as usize]).unwrap();
    // Backdating is done by lying to `survey` about the time instead of
    // touching mtimes, which is not portable. `days` is what the caller then
    // passes as `now`.
    let _ = days;
    dir
}

fn now_plus(days: u64) -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + days * 86_400
}

// --- what it refuses --------------------------------------------------------

#[test]
fn it_never_looks_inside_the_places_your_work_lives() {
    // The failure that would end this feature: Atlas offering up something you
    // made. These are checked as whole path components, so a folder merely
    // *containing* the word is unaffected.
    for name in ["Documents", "Desktop", "Pictures", "OneDrive", "Dropbox"] {
        assert!(
            forbidden(&PathBuf::from("C:\\Users\\eric").join(name).join("node_modules")),
            "{name} was walkable"
        );
    }
}

#[test]
fn a_build_folder_inside_documents_is_still_inside_documents() {
    // Nesting is the way an allowlist gets fooled. `node_modules` is on the
    // allowlist; its location is what disqualifies it.
    let p = PathBuf::from("/home/eric/Documents/thesis/node_modules");
    assert!(forbidden(&p), "a known-safe name let Atlas into a forbidden folder");
}

#[test]
fn it_never_touches_the_recycle_bin() {
    // A tool that empties your recycle bin to save space has removed the undo
    // you already had. That is a worse trade than the megabytes are worth.
    assert!(forbidden(Path::new("C:\\$Recycle.Bin\\S-1-5-21")));
    assert!(forbidden(Path::new("/home/eric/.local/share/Trash/files")));
}

#[test]
fn it_never_touches_the_operating_system() {
    for name in ["Windows", "System32", "Program Files"] {
        assert!(forbidden(&PathBuf::from("C:\\").join(name)), "{name} was walkable");
    }
}

#[test]
fn git_history_is_not_build_output() {
    // `.git` holds every version of everything you have written. It is small
    // enough to be tempting and irreplaceable enough to be a disaster.
    assert!(forbidden(Path::new("/home/eric/code/project/.git")));
}

#[test]
fn anything_not_on_the_allowlist_is_invisible() {
    // The core safety property: this is an allowlist, not a blocklist. A name
    // nobody added cannot be removed by something nobody reviewed.
    assert!(classify("my-important-folder").is_none());
    assert!(classify("archive").is_none());
    assert!(classify("backups").is_none());
    assert!(classify("node_modules").is_some());
}

// --- what it finds ----------------------------------------------------------

#[test]
fn it_finds_an_old_build_folder() {
    let root = tmp("finds");
    make(&root, "code/project/node_modules", 60, 0);
    // 100 days into the future, so the folder reads as 100 days old.
    let found = survey(&[root.clone()], now_plus(100));
    assert_eq!(found.len(), 1, "got: {found:?}");
    assert_eq!(found[0].kind, Kind::BuildOutput);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn it_leaves_a_build_folder_you_are_still_using() {
    // A project you built yesterday is a project you are working on. Age
    // thresholds are generous because the cost of waiting is small and the
    // cost of being wrong is your afternoon.
    let root = tmp("recent");
    make(&root, "code/project/node_modules", 60, 0);
    let found = survey(&[root.clone()], now_plus(1));
    assert!(found.is_empty(), "it offered up a folder from yesterday: {found:?}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn something_too_small_to_matter_is_not_mentioned() {
    // Below the threshold the noise costs more than the space saves, and a
    // list of forty 3MB entries is a list nobody reads.
    let root = tmp("small");
    make(&root, "code/project/node_modules", 2, 0);
    assert!(survey(&[root.clone()], now_plus(100)).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn every_finding_says_what_it_costs_you() {
    // Never a bare size. "4.2 GB" invites a yes; "4.2 GB, and the next build
    // takes longer once" is a decision.
    for (_, kind) in KNOWN {
        assert!(!kind.costs_you().is_empty(), "{kind:?} has no stated cost");
    }
    let c = Candidate {
        path: PathBuf::from("/x/node_modules"),
        size_mb: 4200,
        kind: Kind::BuildOutput,
        age_days: 90,
    };
    let line = c.line();
    assert!(line.contains("4200"), "{line}");
    assert!(line.contains("Costs you"), "{line}");
    assert!(line.contains("takes longer"), "{line}");
}

#[test]
fn every_age_threshold_is_generous() {
    // A week at minimum, because a cache written this morning is a cache in
    // use.
    for (_, kind) in KNOWN {
        assert!(kind.min_age_days() >= 7, "{kind:?} would offer something a few days old");
    }
}

// --- the shape of the whole thing ------------------------------------------

#[test]
fn surveying_removes_nothing() {
    // `survey` is read-only, and that has to stay true. Everything else in
    // this module depends on Atlas being able to look without acting.
    let root = tmp("readonly");
    let dir = make(&root, "code/project/node_modules", 60, 0);
    let _ = survey(&[root.clone()], now_plus(100));
    assert!(dir.join("blob.bin").exists(), "survey deleted something");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn nothing_is_deleted_outright_even_when_chosen() {
    // Everything goes to Atlas's trash, which keeps it 30 days. If Atlas is
    // ever wrong about a file, you get it back.
    let src = std::fs::read_to_string("src/reclaim.rs").expect("src/reclaim.rs");
    assert!(
        !src.contains("remove_dir_all") && !src.contains("remove_file"),
        "reclaim.rs deletes directly instead of going through the trash"
    );
    assert!(src.contains("trash.take("), "it no longer routes through the trash");
}

#[test]
fn the_forbidden_list_is_checked_again_at_the_moment_of_removal() {
    // The survey and the removal are separated by a person reading a list.
    // This is the last point at which a mistake is still cheap.
    let src = std::fs::read_to_string("src/reclaim.rs").expect("src/reclaim.rs");
    let reclaim_fn = src.split("pub fn reclaim(").nth(1).unwrap_or("");
    assert!(
        reclaim_fn.contains("forbidden(&c.path)"),
        "reclaim trusts its caller's list without rechecking it"
    );
}

#[test]
fn a_partial_reclaim_is_not_reported_as_a_whole_one() {
    // Returning only what moved would send you looking for space that was
    // never freed.
    let src = std::fs::read_to_string("src/reclaim.rs").expect("src/reclaim.rs");
    assert!(
        src.contains("refused.push("),
        "reclaim doesn't report what it couldn't move"
    );
}

#[test]
fn it_says_the_space_is_reversible_when_it_offers() {
    let found = vec![Candidate {
        path: PathBuf::from("/x/node_modules"),
        size_mb: 4200,
        kind: Kind::BuildOutput,
        age_days: 90,
    }];
    let said = spoken(&found);
    assert!(said.contains("reversible") || said.contains("30 days"), "{said}");
}

#[test]
fn finding_nothing_reads_as_nothing_rather_than_as_a_failure() {
    let said = spoken(&[]);
    assert!(said.contains("Nothing worth clearing"), "{said}");
    assert!(!said.to_lowercase().contains("error"), "{said}");
}

#[test]
fn the_never_list_covers_the_obvious_disasters() {
    for must in ["Documents", "Desktop", ".git", "Windows", "System32"] {
        assert!(NEVER.contains(&must), "{must} fell off the never-touch list");
    }
}

// --- looking at the whole disk ---------------------------------------------
//
// The design error the first version made: it confined what Atlas could *look
// at* to the same allowlist that governs what Atlas may *remove*. Safe, and
// close to useless — the space is rarely in the caches. It is in a folder of
// video from 2019, an application you stopped using, and four years of
// screenshots.
//
// Looking is read-only and therefore safe anywhere. Removing is not, and stays
// confined to things that rebuild themselves.

#[test]
fn the_two_lists_are_governed_by_different_rules() {
    // The line the whole module turns on.
    for k in [Kind::Temp, Kind::PackageCache, Kind::BuildOutput, Kind::Installer, Kind::Debris] {
        assert!(k.atlas_may_move(), "{k:?} rebuilds itself and should be movable");
    }
    for k in [Kind::BigAndOld, Kind::Screenshots, Kind::App, Kind::BigFolder] {
        assert!(!k.atlas_may_move(), "{k:?} is yours to judge and Atlas must not move it");
    }
}

#[test]
fn a_big_old_file_is_reported_but_never_movable() {
    // A four-gigabyte video from 2019 is either a wedding or a download you
    // forgot. Atlas cannot tell, and the cost of guessing wrong is absolute.
    let root = tmp("bigold");
    let dir = root.join("Videos-archive");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("holiday.mp4"), vec![0u8; 250 * 1024 * 1024]).unwrap();
    let found = atlas::reclaim::whole_disk(&[root.clone()], now_plus(400));
    let big: Vec<_> = found.iter().filter(|c| c.kind == Kind::BigAndOld).collect();
    assert_eq!(big.len(), 1, "the big old file wasn't reported: {found:?}");
    assert!(!big[0].kind.atlas_may_move(), "Atlas offered to move your video");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_recent_big_file_is_not_mentioned_at_all() {
    // Something you saved last week is something you are using.
    let root = tmp("bigrecent");
    let dir = root.join("work");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("current.mp4"), vec![0u8; 250 * 1024 * 1024]).unwrap();
    let found = atlas::reclaim::whole_disk(&[root.clone()], now_plus(2));
    assert!(found.is_empty(), "it flagged a file from two days ago: {found:?}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn screenshots_are_named_as_a_folder_not_four_thousand_files() {
    let root = tmp("shots");
    let dir = root.join("Pictures-x").join("Screenshots");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..3 {
        std::fs::write(dir.join(format!("shot{i}.png")), vec![0u8; 40 * 1024 * 1024]).unwrap();
    }
    let found = atlas::reclaim::whole_disk(&[root.clone()], now_plus(200));
    let shots: Vec<_> = found.iter().filter(|c| c.kind == Kind::Screenshots).collect();
    assert_eq!(shots.len(), 1, "expected one folder entry, got: {found:?}");
    assert!(shots[0].size_mb >= 100, "it didn't total the folder: {:?}", shots[0]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_private_folder_is_sized_but_not_read_into() {
    // Reading inside somebody's Documents to count bytes is still reading
    // inside them. The size is reported so you can see where the weight is;
    // the contents are not examined and nothing inside is ever listed.
    let root = tmp("private");
    let docs = root.join("Documents").join("taxes");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("return.pdf"), vec![0u8; 600 * 1024 * 1024]).unwrap();
    let found = atlas::reclaim::whole_disk(&[root.clone()], now_plus(400));
    assert!(
        found.iter().all(|c| !c.path.to_string_lossy().contains("return.pdf")),
        "it listed a file inside Documents: {found:?}"
    );
    assert!(
        found.iter().any(|c| c.kind == Kind::BigFolder),
        "it didn't even say where the space went: {found:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_application_is_reported_not_deleted() {
    // Deleting an application folder leaves its registry entries, services and
    // scheduled tasks behind — a broken uninstall that is harder to fix than
    // the space was worth.
    let root = tmp("apps");
    let app = root.join("Program Files").join("SomeBigApp");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("app.exe"), vec![0u8; 150 * 1024 * 1024]).unwrap();
    let apps = atlas::reclaim::installed_apps(&[root.clone()], now_plus(400));
    assert_eq!(apps.len(), 1, "got: {apps:?}");
    assert_eq!(apps[0].kind, Kind::App);
    assert!(!apps[0].kind.atlas_may_move());
    assert!(
        apps[0].kind.costs_you().contains("Uninstall"),
        "it doesn't say to uninstall properly: {}",
        apps[0].kind.costs_you()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn every_line_says_whether_atlas_will_act_on_it() {
    // Two lists in one output is only useful if each line says which it is.
    let mine = Candidate {
        path: PathBuf::from("/x/node_modules"),
        size_mb: 900,
        kind: Kind::BuildOutput,
        age_days: 90,
    };
    let yours = Candidate {
        path: PathBuf::from("/x/holiday.mp4"),
        size_mb: 4200,
        kind: Kind::BigAndOld,
        age_days: 900,
    };
    assert!(mine.line().contains("I can move this"), "{}", mine.line());
    assert!(yours.line().contains("Yours to decide"), "{}", yours.line());
}

#[test]
fn the_summary_separates_what_atlas_can_do_from_what_it_cannot() {
    let all = vec![
        Candidate { path: PathBuf::from("/a"), size_mb: 100, kind: Kind::BuildOutput, age_days: 90 },
        Candidate { path: PathBuf::from("/b"), size_mb: 900, kind: Kind::BigAndOld, age_days: 900 },
    ];
    let said = atlas::reclaim::report(&all);
    // The numbers must be attributed to the right half. Summing them together
    // would tell you a thousand megabytes are available when only a hundred
    // are anything Atlas can act on.
    let mine: u64 = all.iter().filter(|c| c.kind.atlas_may_move()).map(|c| c.size_mb).sum();
    let yours: u64 = all.iter().filter(|c| !c.kind.atlas_may_move()).map(|c| c.size_mb).sum();
    assert_eq!(mine, 100);
    assert_eq!(yours, 900);
    assert!(said.contains("100 MB myself"), "{said}");
    assert!(said.contains("900 MB is yours"), "{said}");
    assert!(said.contains("won't touch"), "{said}");
}

#[test]
fn only_the_movable_half_can_ever_reach_the_remover() {
    // Belt and braces: main.rs filters, and `reclaim` re-checks. This asserts
    // the filter exists, because losing it would send reported items — your
    // videos, your applications — into the trash.
    let src = crate::common::source_of("main");
    assert!(
        src.contains("found.retain(|c| c.kind.atlas_may_move())"),
        "the reclaim command no longer filters to the movable half"
    );
}
