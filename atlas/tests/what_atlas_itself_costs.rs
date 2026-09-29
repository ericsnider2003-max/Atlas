//! The disk report that never mentioned its own folder.
//!
//! `atlas reclaim` walks your home directory and your temp folder, names
//! caches and installers and old downloads, and tells you how much you could
//! get back. It said nothing whatsoever about Atlas's own data folder.
//!
//! The numbers were all there. `retention::survey` walks that folder,
//! `retention::usage` groups it by what each file is for, and
//! `Usage::total_mb` is the answer to "how much" — and that method had no
//! production caller anywhere.
//!
//! It had *looked* like it had one, which is the part worth recording.
//! `install::total_mb` existed until 19 Sep 2026 and had real callers, and the
//! deadness scan matches bare names: `total_mb(` in `main.rs` was enough to
//! mark this one reached. Renaming the install one to `download_mb` — for an
//! unrelated reason, so that a filtered piece list and the megabyte figure
//! under it stopped disagreeing — is what made this visible. One rename, and a
//! capability that had been dead the whole time appeared.
//!
//! That is the third time in this tree a name collision has held a dead
//! function up (`looks_like`, `honest`, `confirmed`, `due`, `spoken`,
//! `review`, `summary`, `gaps`, `answer` before it), and the first time one
//! has been *found* by the collision being cleared rather than created.

use atlas::retention::{self, Class, Item, RetentionConfig, Usage};
use std::path::PathBuf;

fn item(path: &str, class: Class, mb: u64) -> Item {
    Item { path: PathBuf::from(path), class, bytes: mb * 1024 * 1024, modified: 0 }
}

// ================= the number that had no caller =================

#[test]
fn how_much_atlas_is_costing_you_is_a_question_with_an_answer() {
    let items = vec![
        item("data/tmp/a", Class::Scratch, 10),
        item("data/captures/b.wav", Class::Captures, 200),
        item("data/logs/c.log", Class::Logs, 5),
        item("data/notes/d.md", Class::Notes, 30),
        item("data/state/e.json", Class::State, 40),
    ];
    let u = retention::usage(&items);
    assert_eq!(u.total_mb(), 285);

    // And it is the whole folder, not the part this module may delete. A
    // footprint that leaves out the trash understates exactly the folder the
    // person is about to go looking at.
    let with_trash = {
        let mut v = items.clone();
        v.push(item("data/trash/old", Class::NotOurs, 100));
        retention::usage(&v)
    };
    assert_eq!(with_trash.total_mb(), 385);
    assert_eq!(with_trash.not_ours / (1024 * 1024), 100);
}

#[test]
fn rounding_down_is_the_honest_direction_for_a_number_you_will_act_on() {
    // 1.9 MB reported as 2 invites "I'll get 2 MB back" and you get one. The
    // floor is deliberate, and the `.max(1)` trick used elsewhere in the tree
    // for file counts is wrong here: zero really is what a folder with a few
    // kilobytes in it costs you.
    let u = retention::usage(&[Item {
        path: PathBuf::from("data/tmp/a"),
        class: Class::Scratch,
        bytes: 1_900_000,
        modified: 0,
    }]);
    assert_eq!(u.total_mb(), 1);
    assert_eq!(retention::usage(&[]).total_mb(), 0);
}

// ================= the budget it is measured against =================

#[test]
fn over_budget_and_beyond_help_are_told_apart() {
    let cfg = RetentionConfig { total_budget_mb: 100, ..Default::default() };

    // Over, but the over is scratch and recordings — the housekeeping pass
    // fixes this on its own and saying anything else would send you editing
    // config for no reason.
    let fixable = retention::usage(&[
        item("data/captures/a.wav", Class::Captures, 300),
        item("data/notes/b.md", Class::Notes, 10),
    ]);
    assert!(fixable.total_mb() > cfg.total_budget_mb);
    assert!(!retention::irreducible(&fixable, &cfg));

    // Over, and the over is notes and learned state, which `plan` never
    // evicts for space. No amount of cleanup helps; the budget is the thing
    // that is wrong.
    let stuck = retention::usage(&[
        item("data/notes/a.md", Class::Notes, 80),
        item("data/state/b.json", Class::State, 80),
    ]);
    assert!(retention::irreducible(&stuck, &cfg));

    // Under budget is neither.
    let fine = retention::usage(&[item("data/notes/a.md", Class::Notes, 10)]);
    assert!(fine.total_mb() < cfg.total_budget_mb);
    assert!(!retention::irreducible(&fine, &cfg));
}

#[test]
fn the_budget_read_is_yours_rather_than_the_shipped_one() {
    // The same defect the housekeeping errand had until 18 Sep: it built a
    // `RetentionConfig::default()` while the `retention:` block sat unread, so
    // a person who set 2 GB still got 500 MB. Worth one assertion here so the
    // second reader of these numbers does not repeat it.
    let raw = crate::common::source_of("main");
    assert!(
        raw.contains("cfg.tools.as_ref().map(|t| t.retention.clone())"),
        "the footprint report builds its own budget instead of reading yours"
    );
    assert_eq!(RetentionConfig::default().total_budget_mb, 500);
}

// ================= it is actually reachable =================

#[test]
fn the_reclaim_command_is_what_reaches_it_rather_than_this_test() {
    let raw = crate::common::source_of("main");
    assert!(raw.contains("fn report_own_footprint("), "nothing reports the footprint");
    assert!(
        raw.contains("atlas::retention::usage(&items)"),
        "nothing groups the data folder by what the files are for"
    );
    assert!(raw.contains("usage.total_mb()"), "the total is still uncalled");

    // Both ways out of `run_reclaim` say it. The early return is the one that
    // matters: "nothing to reclaim", from a program sitting on 900 MB of its
    // own, is the answer that makes you stop trusting the other one.
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let body = raw
        .split("fn run_reclaim(")
        .nth(1)
        .and_then(|r| r.split("\n}\n").next())
        .expect("run_reclaim");
    assert_eq!(
        body.matches("report_own_footprint(cfg)").count(),
        2,
        "one of the two exits from `atlas reclaim` doesn't mention Atlas's own folder"
    );
}

#[test]
fn the_breakdown_uses_the_classs_own_words_for_the_classs_own_meanings() {
    // `Class::Scratch` is the wav of the turn you are in; `Class::Captures`
    // is screenshots and webcam frames. The first draft of this report
    // labelled `captures` "recordings", which reads as audio and points at
    // the wrong half of the folder — caught by running `atlas reclaim` on a
    // folder with one wav in it and reading the line it printed.
    assert_eq!(retention::classify(&PathBuf::from("data/captures/a.wav")), Class::Scratch);
    assert_eq!(retention::classify(&PathBuf::from("data/captures/a.png")), Class::Captures);

    let raw = crate::common::source_of("main");
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let body = raw
        .split("fn report_own_footprint(")
        .nth(1)
        .and_then(|r| r.split("\n}\n").next())
        .expect("report_own_footprint");
    let line = body
        .lines()
        .find(|l| l.contains("screenshots {} MB"))
        .expect("the breakdown line");
    assert!(
        !line.contains("recordings {} MB"),
        "`captures` is labelled as recordings again: {line}"
    );
}

#[test]
fn reporting_is_all_it_does() {
    // Two things deleting from the same folder on two different rules is how
    // a backup disappears. `atlas reclaim --do-it` moves what it found on
    // *your* disk; the data folder is pruned hourly against the budget, by
    // `retention::plan` and `retention::apply`, and nowhere else.
    let raw = crate::common::source_of("main");
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let body = raw
        .split("fn report_own_footprint(")
        .nth(1)
        .and_then(|r| r.split("\n}\n").next())
        .expect("report_own_footprint");
    for destructive in ["retention::plan(", "retention::apply(", "remove_file", "remove_dir"] {
        assert!(
            !body.contains(destructive),
            "the footprint report calls {destructive} -- it is meant to only count"
        );
    }
}

#[test]
fn the_name_collision_that_hid_this_is_gone_rather_than_renamed_around() {
    // `install::total_mb` is what made this look reached. It is
    // `download_mb` now, and putting the old name back anywhere would hide
    // this function again the same way.
    let install = std::fs::read_to_string("src/install.rs").expect("install.rs");
    assert!(install.contains("pub fn download_mb("));
    assert!(
        !install.contains("pub fn total_mb("),
        "install has a `total_mb` again, which is what masked retention's for weeks"
    );
    // And there is exactly one `pub fn total_mb` in the tree. A second one
    // anywhere re-creates the collision, whichever module it lands in.
    let mut found = Vec::new();
    fn walk(d: &std::path::Path, found: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(d) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, found);
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                if let Ok(t) = std::fs::read_to_string(&p) {
                    if t.contains("pub fn total_mb(") {
                        found.push(p.display().to_string().replace('\\', "/"));
                    }
                }
            }
        }
    }
    walk(std::path::Path::new("src"), &mut found);
    assert_eq!(found, vec!["src/retention.rs".to_string()], "{found:?}");

    // Stated positively so the point survives the assertions: this is the
    // type the number belongs to.
    let u: Usage = retention::usage(&[item("data/notes/a.md", Class::Notes, 3)]);
    assert_eq!(u.total_mb(), 3);
}
