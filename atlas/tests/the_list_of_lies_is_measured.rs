//! The one list that talks straight to the user, and nothing checked it.
//!
//! `config::PARSED_AND_NEVER_READ` and `config::NO_FIELD_TO_LAND_IN` name the
//! sections of `tools.yaml` that do nothing — a section that parses into a
//! field nothing reads, or one with no field to parse into at all.
//! `settings_that_do_nothing` intersects them with *your* file, and
//! `atlas doctor` reads the result out to you.
//!
//! That is the most directly user-facing honesty in the tree, and it was
//! **hand-maintained with no ratchet on it.** Both ways of going wrong were
//! live:
//!
//! - **A stale entry lies in the other direction.** `sync` sat on the list
//!   after it was wired, so `atlas doctor` told you your sync settings did
//!   nothing while they were being read.
//! - **A new dead section is invisible.** Nothing failed when a field stopped
//!   being read, so the list only grew when somebody noticed.
//!
//! This measures it in both directions. It is the same ratchet discipline the
//! deadness guards use, applied to the list whose audience is a person rather
//! than a maintainer.

use std::collections::BTreeSet;
use std::fs;

mod common;

/// `ToolsConfig`'s fields and the config type each one holds.
fn tools_fields() -> Vec<String> {
    let voice = fs::read_to_string("src/voice.rs").expect("src/voice.rs");
    let body = voice
        .split("pub struct ToolsConfig")
        .nth(1)
        .expect("ToolsConfig is gone");
    let body = &body[..body.find("\n}").expect("unterminated ToolsConfig")];
    body.lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix("pub ")
                .and_then(|r| r.split(':').next())
                .map(|n| n.trim().to_string())
        })
        .filter(|n| !n.is_empty())
        .collect()
}

/// Is `.field` read anywhere in `src/`, ignoring test blocks?
///
/// Deliberately the same generous rule `tests/dead_config.rs` uses: any `.name`
/// anywhere counts as a read, even on an unrelated struct that happens to share
/// the name. That biases hard toward calling a setting *alive*, so everything
/// this flags is unambiguous.
fn read_somewhere(field: &str) -> bool {
    let needle = format!(".{field}");
    let dir = std::path::Path::new("src");
    let Ok(entries) = fs::read_dir(dir) else { return false };
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
    drop(entries);
    walk(dir, &mut paths);

    // `voice.rs` is *not* skipped, though it declares `ToolsConfig`. The
    // declaration reads `pub retention: ...` and never `.retention`, so it
    // cannot match — and voice.rs has real logic of its own that reads
    // settings. Skipping it wholesale was this test's own first bug: it
    // reported `record_seconds` as dead when voice.rs reads it a few hundred
    // lines below the struct.
    for p in paths {
        let Ok(src) = fs::read_to_string(&p) else { continue };
        // Production only. A setting read solely by its own module's tests is
        // exactly the shape this whole file exists to catch.
        let (live, _) = common::split_production_and_tests(&src);
        for (i, _) in live.match_indices(&needle) {
            let after = live[i + needle.len()..].chars().next().unwrap_or(' ');
            if !after.is_alphanumeric() && after != '_' {
                return true;
            }
        }
    }
    false
}

fn listed_as_dead() -> BTreeSet<String> {
    atlas::config::PARSED_AND_NEVER_READ
        .iter()
        .map(|(k, _)| k.to_string())
        .collect()
}

#[test]
fn nothing_on_the_list_is_quietly_alive() {
    // The failure that was live when this test was written: `sync` was wired
    // and stayed listed, so `atlas doctor` told you a working setting did
    // nothing. A list that is wrong in this direction is worse than no list —
    // it teaches you to ignore your own config.
    let fields: BTreeSet<String> = tools_fields().into_iter().collect();
    let mut alive_but_listed = Vec::new();

    for key in listed_as_dead() {
        if fields.contains(&key) && read_somewhere(&key) {
            alive_but_listed.push(key);
        }
    }

    assert!(
        alive_but_listed.is_empty(),
        "these are listed as doing nothing and are now read — `atlas doctor` is \
         telling the user a working setting is dead:\n  {}\n\nDelete the lines \
         from PARSED_AND_NEVER_READ.",
        alive_but_listed.join("\n  ")
    );
}

#[test]
fn nothing_dead_is_missing_from_the_list() {
    // The other direction. A section that stops being read must not become a
    // false promise nobody records — `atlas doctor` can only be honest about
    // what it has been told.
    let mut dead_but_unlisted = Vec::new();
    let listed = listed_as_dead();

    for field in tools_fields() {
        if !read_somewhere(&field) && !listed.contains(&field) {
            dead_but_unlisted.push(field);
        }
    }

    assert!(
        dead_but_unlisted.is_empty(),
        "these parse into a field nothing reads, and are not on the list, so \
         `atlas doctor` will not warn anyone about them:\n  {}\n\nWire it, or \
         add it to PARSED_AND_NEVER_READ with the reason.",
        dead_but_unlisted.join("\n  ")
    );
}

#[test]
fn every_entry_says_why() {
    // A key with no reason is a key nobody can act on. `atlas doctor` reads
    // the reason out loud, so an empty one reaches the user as a shrug.
    for (key, why) in atlas::config::PARSED_AND_NEVER_READ {
        assert!(
            why.len() > 12,
            "{key} is listed with no useful reason: {why:?}"
        );
    }
    for (key, why) in atlas::config::NO_FIELD_TO_LAND_IN {
        assert!(why.len() > 12, "{key} is listed with no useful reason: {why:?}");
    }
}

#[test]
fn a_section_with_no_field_really_has_none() {
    // `NO_FIELD_TO_LAND_IN` is the harder kind: a section in the shipped file
    // with nowhere to parse into at all. If someone adds the field and stops
    // there, the key belongs on the *other* list, not this one — the section
    // would then parse and still do nothing, which is a different sentence to
    // say to the user.
    let fields: BTreeSet<String> = tools_fields().into_iter().collect();
    let mut now_have_a_field = Vec::new();

    for (key, _) in atlas::config::NO_FIELD_TO_LAND_IN {
        if fields.contains(*key) {
            now_have_a_field.push(*key);
        }
    }

    assert!(
        now_have_a_field.is_empty(),
        "these now have a field on ToolsConfig, so they are no longer \
         'nowhere to land':\n  {}\n\nMove them to PARSED_AND_NEVER_READ if \
         nothing reads them yet, or delete the line if something does.",
        now_have_a_field.join("\n  ")
    );
}

#[test]
fn the_scan_still_works() {
    // A scan that silently starts finding nothing turns all of the above into
    // tests that pass for any tree at all — the failure mode this suite has
    // had more than once.
    assert!(
        read_somewhere("certainty"),
        "the read-detector cannot see a setting that is definitely read"
    );
    assert!(
        !read_somewhere("a_field_that_does_not_exist_anywhere"),
        "the read-detector says yes to everything"
    );
    assert!(tools_fields().len() > 40, "ToolsConfig's fields stopped being found");
}
