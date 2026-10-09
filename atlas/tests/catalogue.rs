//! The catalogue must describe this tree, not a remembered one.
//!
//! `capability::all()` is Atlas's inventory of itself. It is what answers
//! "what can you do?", what `atlas catalog` prints, and — since Atlas is
//! meant to work on its own code — what Atlas reads to find out which files
//! matter. A catalogue that has drifted is worse than no catalogue, because
//! you stop checking it against the source and start believing it.
//!
//! Three things are checked here, and each one has been wrong before:
//!
//! 1. **Every module a capability names is a real file.** A typo'd name
//!    quietly exempts a capability from every other check.
//! 2. **The platform answer is derived, never typed.** No capability has a
//!    field saying "works on Android". It states what it needs of the
//!    machine, `portable::how` states what a platform grants, and the answer
//!    is the worse of the two. A hand-written platform claim is a claim
//!    somebody has to remember to update.
//! 3. **Every module is a capability or named plumbing.** Six capabilities
//!    that were built, wired and tested — the vault, the recovery key, sync,
//!    the household, the hub, reclaim — were absent from this list entirely on
//!    19 Sep 2026. A count was added; on 28 Sep it turned out to be holding
//!    about 130 unlisted features. Now each module is claimed or listed in
//!    `capability::PLUMBING` with a reason, and there is no third kind.

use atlas::capability::{self, State};
use atlas::portable::{self, How, Needs, Platform};
use std::collections::BTreeSet;
use std::path::Path;

const EVERY_PLATFORM: &[Platform] = &[
    Platform::Windows,
    Platform::Mac,
    Platform::Linux,
    Platform::Ios,
    Platform::Android,
    Platform::Web,
];

#[test]
fn connection_descriptions_and_removal_receipts_belong_to_existing_mail_capability() {
    let all = capability::all();
    let mail = all.iter().find(|capability| capability.id == "mail").unwrap();
    assert!(mail.modules.contains(&"connectors") && mail.modules.contains(&"connection_removal"));
    assert!(modules_in_tree().contains("connectors") && modules_in_tree().contains("connection_removal"));
    assert_eq!(modules_in_tree().len(), capability::MODULES_IN_TREE);
}

/// Every module in the tree, by the name a capability would call it.
///
/// `src/market/claims.rs` is `claims`; `src/platform/mod.rs` is `platform`.
/// `lib` and `main` are entrypoints rather than modules. `src/daemon/*.rs`
/// and `src/main/*.rs` are pieces of `daemon` and `main` (a split module keeps
/// its `src/<m>.rs` beside `src/<m>/`), so they add nothing.
fn modules_in_tree() -> BTreeSet<String> {
    fn walk(dir: &Path, out: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            if path.extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            // A file inside a split module's folder is part of that module,
            // not a module of its own (29 Sep 2026): `src/daemon/tick.rs` is
            // `daemon`, and `src/main/*.rs` are the binary's entrypoint.
            // Before the split of daemon.rs and main.rs this read every
            // `src/daemon/<x>.rs` as a new module `<x>` that nothing claims.
            if crate::common::split_parent(&path).is_some() {
                continue;
            }
            match stem {
                "lib" | "main" => {}
                "mod" => {
                    if let Some(parent) = dir.file_name().and_then(|s| s.to_str()) {
                        out.insert(parent.to_string());
                    }
                }
                other => {
                    out.insert(other.to_string());
                }
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(Path::new("src"), &mut out);
    out
}

// ============ 1. the catalogue points at files that exist ============

#[test]
fn every_module_a_capability_claims_is_a_real_file() {
    let tree = modules_in_tree();
    let missing: Vec<String> = capability::all()
        .into_iter()
        .flat_map(|c| c.modules.iter().map(move |m| (c.id, *m)))
        .filter(|(_, m)| !tree.contains(*m))
        .map(|(id, m)| format!("{id} claims {m}, and there is no {m}.rs anywhere under src/"))
        .collect();

    assert!(
        missing.is_empty(),
        "the catalogue points at files that don't exist:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn every_capability_names_where_it_lives_and_what_it_needs() {
    let vague: Vec<String> = capability::all()
        .into_iter()
        .filter(|c| c.modules.is_empty() || c.runs.is_empty())
        .map(|c| {
            format!(
                "{} — modules: {}, runs: {}",
                c.id,
                c.modules.len(),
                c.runs.len()
            )
        })
        .collect();
    assert!(
        vague.is_empty(),
        "a capability that doesn't say where it lives can't be checked against the tree, and one \
         that doesn't say what it needs can't be placed on a platform:\n  {}",
        vague.join("\n  ")
    );
}

#[test]
fn the_pinned_size_of_the_tree_is_still_right() {
    // `MODULES_IN_TREE` is pinned because a running Atlas has a binary and no
    // source tree to count. Pinned is fine; pinned and unchecked is not.
    let real = modules_in_tree().len();
    assert_eq!(
        real,
        capability::MODULES_IN_TREE,
        "the tree has {real} modules and capability::MODULES_IN_TREE says {}. Update the \
         constant — `atlas catalog` prints a coverage figure built on it.",
        capability::MODULES_IN_TREE
    );
}

// ============ 2. the platform answer is worked out, not written ============

#[test]
fn a_capability_is_no_better_than_the_hardest_thing_it_asks_for() {
    // `dictate` wants the microphone and the ability to type into somebody
    // else's app. On an iPhone the microphone is fine and the second is a
    // wall, so dictate is a wall. Taking the best of its needs rather than
    // the worst would advertise it as working there.
    let dictate = capability::all().into_iter().find(|c| c.id == "dictate").unwrap();
    assert!(dictate.runs.contains(&Needs::Audio));
    assert!(dictate.runs.contains(&Needs::ActInApps));

    assert_eq!(portable::how(Platform::Ios, Needs::Audio), How::Possible);
    assert_eq!(portable::how(Platform::Ios, Needs::ActInApps), How::Never);
    assert_eq!(capability::runs_on(&dictate, Platform::Ios), How::Never);

    // And the reason given is the need that actually walls it, not the one
    // that happens to be first in the list.
    assert_eq!(capability::blocked_by(&dictate, Platform::Ios), Some(Needs::ActInApps));
}

#[test]
fn nothing_claims_a_platform_it_was_not_computed_for() {
    // The whole join, checked the long way round for every capability on
    // every platform. If `runs_on` ever grows a hand-written shortcut this
    // stops agreeing.
    for p in EVERY_PLATFORM {
        for c in capability::all() {
            let worst = c
                .runs
                .iter()
                .map(|n| portable::how(*p, *n))
                .map(|h| match h {
                    How::Built => 0,
                    How::Possible => 1,
                    How::Awkward => 2,
                    How::Never => 3,
                })
                .max()
                .unwrap();
            let got = match capability::runs_on(&c, *p) {
                How::Built => 0,
                How::Possible => 1,
                How::Awkward => 2,
                How::Never => 3,
            };
            assert_eq!(got, worst, "{} on {}", c.id, p.name());
        }
    }
}

#[test]
fn the_phones_are_the_ones_with_walls_and_windows_is_not() {
    // The shape of the answer Atlas is supposed to give about itself:
    // everything is possible on the three desktops, and a named handful is
    // not possible on a phone.
    for p in [Platform::Windows, Platform::Mac, Platform::Linux] {
        assert!(
            capability::walled_on(p).is_empty(),
            "{} is a desktop — nothing there should be impossible",
            p.name()
        );
        assert_eq!(capability::on(p).len(), capability::all().len());
    }

    let ios = capability::walled_on(Platform::Ios);
    assert!(!ios.is_empty(), "an iPhone has real walls and the catalogue should know them");
    assert!(
        capability::on(Platform::Ios).len() < capability::all().len(),
        "if nothing is walled on iOS the join isn't being applied"
    );

    // Android and iOS differ, and the difference is the one people ask about.
    let wake = capability::all().into_iter().find(|c| c.id == "wake").unwrap();
    assert_eq!(capability::runs_on(&wake, Platform::Ios), How::Never);
    assert!(capability::runs_on(&wake, Platform::Android).is_effort_not_a_wall());
}

#[test]
fn a_wall_is_always_explained_rather_than_just_declared() {
    // "Not on your phone" ends the conversation. "Not on your phone because
    // it would have to read your screen" is one you can argue with.
    for p in EVERY_PLATFORM {
        for (c, n) in capability::walled_on(*p) {
            let said = capability::says(&c, *p);
            assert!(
                said.contains(n.plain()),
                "{} on {} is walled by {:?} and doesn't say so: {said}",
                c.id,
                p.name(),
                n
            );
            assert!(said.starts_with("never on "), "{said}");
        }
    }
}

#[test]
fn being_allowed_by_the_platform_is_not_reported_as_being_built() {
    // The defect this whole file exists to prevent: reporting the platform's
    // verdict alone printed "works" for a capability nothing can reach.
    //
    // This test used to name `certainty` as its example of something every
    // platform allows and nothing reaches — and on 19 Sep 2026 that example
    // turned out to be **wired**, with eight production call sites in
    // `daemon.rs`. It was one of 27 capabilities left at `Planned` after
    // being built, and this assertion was quietly pinning the catalogue's
    // mistake in place: correcting the entry failed a test whose subject was
    // something else entirely.
    //
    // So the property is asserted over the whole list first, and the named
    // example second. A rule that holds for every row cannot be invalidated
    // by one row moving.
    for c in capability::all() {
        if c.state != State::Planned {
            continue;
        }
        for p in [Platform::Ios, Platform::Android, Platform::Windows, Platform::Mac] {
            let said = capability::says(&c, p);
            assert!(
                !said.contains("works"),
                "{} is not built yet and {} reports: {said}",
                c.id,
                p.bare()
            );
        }
    }

    // And one named, because a loop over an empty list also passes. `mesh` is
    // the standing example now that `plainchange` is wired and Untested — a
    // capability the platform allows (iOS permits it, with a compromise) and
    // that is still not built. The property is the same whether the platform is
    // a clean yes or a yes-with-a-catch: being allowed is not being built.
    let example = capability::all()
        .into_iter()
        .find(|c| c.id == "mesh")
        .expect("mesh is the standing example of allowed-but-built-nowhere");
    assert_eq!(example.state, State::Planned);
    assert!(
        matches!(capability::runs_on(&example, Platform::Ios), How::Built | How::Awkward),
        "iOS allows mesh (cleanly or with a catch): {:?}",
        capability::runs_on(&example, Platform::Ios)
    );
    // Awkward appends the platform's catch ("…then it's suspended"), so this
    // leads with "not built yet" rather than being only that. The state is what
    // matters: allowed, and still not built.
    assert!(
        capability::says(&example, Platform::Ios).starts_with("not built yet"),
        "a Planned capability the platform allows still leads with not-built: {}",
        capability::says(&example, Platform::Ios)
    );
}

#[test]
fn a_blocked_capability_says_what_it_is_waiting_on_out_loud() {
    let wake = capability::all().into_iter().find(|c| c.id == "wake").unwrap();
    assert_eq!(wake.state, State::Blocked);
    let said = capability::says(&wake, Platform::Windows);
    assert!(said.contains("whisper"), "\"waiting on something\" is not an answer: {said}");
}

#[test]
fn the_platform_listing_warns_that_there_is_only_one_build() {
    // Every line under a heading like "WHAT ATLAS DOES ON ANDROID" reads as a
    // promise unless something at the top says there is no Android build.
    let here = atlas::platform::what_am_i();
    let elsewhere = if here == Platform::Ios { Platform::Android } else { Platform::Ios };
    let text = capability::on_platform_full(elsewhere);
    assert!(
        text.contains(&format!("There is no {} build yet", elsewhere.bare())),
        "the listing reads as a promise: {}",
        &text[..text.len().min(300)]
    );
    // And it doesn't say that about the machine it's actually on.
    assert!(!capability::on_platform_full(here).contains("build yet"));
}

#[test]
fn the_summary_groups_walls_by_cause_rather_than_listing_them() {
    let said = capability::on_platform_summary(Platform::Ios);
    assert!(said.contains("On an iPhone or iPad"));
    assert!(said.contains("can't"), "{said}");
    // One rule blocking three things is one rule, not three.
    assert!(
        said.contains("need moving your windows") || said.contains("needs moving your windows"),
        "{said}"
    );
    // Counted, never enumerated — a summary that lists every wall is the full
    // listing with extra steps.
    assert!(said.len() < 400, "this is a summary: {said}");

    let desktop = capability::on_platform_summary(Platform::Windows);
    assert!(desktop.contains("Nothing is off the table"), "{desktop}");
}

// ============ 3. every module is a capability or named plumbing ============
//
// This was a ratchet until 28 Sep 2026: `UNCLAIMED_MAX` (last 171) capped how
// many modules no capability claimed, on the theory that most were plumbing.
// It held the number and said nothing about what was in it, and when Eric
// asked for the full list, about 130 of the 159 unclaimed modules turned out
// to be features -- the video editor, the morning brief, the overnight run,
// the council, undo, the phone pieces, the hub's own settings -- that "what
// can you do?" had never mentioned. A count cannot tell plumbing from a
// feature nobody wrote down; a name and a reason can.
//
// So there is no third kind now. Each module is claimed by a capability or
// named in `capability::PLUMBING` with why it is not one.

#[test]
fn every_module_is_a_capability_or_named_plumbing() {
    let tree = modules_in_tree();
    let claimed = capability::claimed_modules();
    let plumbing: BTreeSet<&str> = capability::PLUMBING.iter().map(|(m, _)| *m).collect();
    let unaccounted: Vec<&str> = tree
        .iter()
        .map(|m| m.as_str())
        .filter(|m| !claimed.contains(m) && !plumbing.contains(m))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "these modules are claimed by no capability and not named as plumbing:\n  {}\n\n\
         If it does something a person would ask for, add a `Capability` in \
         `capability::all()` whose `modules` names it -- that is how \"what can you do?\" \
         learns about it. If it is genuinely plumbing, add it to `capability::PLUMBING` \
         with the reason. There is no third option: that is how the video editor, the \
         morning brief and undo went unlisted.",
        unaccounted.join("\n  ")
    );
}

#[test]
fn nothing_is_both_a_capability_and_plumbing() {
    let claimed = capability::claimed_modules();
    let both: Vec<&str> = capability::PLUMBING
        .iter()
        .map(|(m, _)| *m)
        .filter(|m| claimed.contains(m))
        .collect();
    assert!(
        both.is_empty(),
        "claimed by a capability and also listed as plumbing -- take it off PLUMBING: {both:?}"
    );
}

#[test]
fn every_plumbing_entry_is_a_real_module_with_a_reason() {
    let tree = modules_in_tree();
    let mut seen = BTreeSet::new();
    for (m, why) in capability::PLUMBING {
        assert!(tree.contains(*m), "PLUMBING names {m}, and there is no {m}.rs under src/");
        assert!(seen.insert(*m), "{m} is on PLUMBING twice");
        assert!(
            why.split_whitespace().count() >= 4,
            "{m} is listed as plumbing without a real reason: {why:?}"
        );
        // A to-do is not plumbing. "not wired yet" belongs in the catalogue
        // as `Planned`, where it is counted as unfinished.
        assert!(
            !why.contains("not wired") && !why.contains(" yet") && !why.contains("TODO"),
            "{m}'s reason is really a to-do: {why:?}"
        );
    }
    // And the two lists together are the whole tree, which is what the
    // footer of docs/CAPABILITIES.md says.
    let (claimed, total) = capability::module_coverage();
    assert_eq!(claimed + capability::PLUMBING.len(), total);
}

#[test]
fn the_features_that_were_missing_are_in_the_catalogue_by_name() {
    // Named rather than counted, like the vault test below: these are what
    // Eric reasonably thought Atlas lacked, because the list never said.
    let ids: BTreeSet<&str> = capability::all().into_iter().map(|c| c.id).collect();
    for id in [
        "edit", "grade", "voiceover", "viewing", "content", "opsec", "opportunity", "brief",
        "overnight", "council", "decide", "orders", "booking", "finance", "flow", "routine",
        "undo", "phone", "remote", "companion", "chat", "handover", "workspace", "rehearse",
        "later", "backlog", "nudge", "daily", "health", "anticipate", "reference",
    ] {
        assert!(ids.contains(id), "{id} is built and not in the catalogue");
    }
    for m in ["editors", "publishing", "delivery", "outbox", "outreach", "otherside", "safety", "glance", "why", "modes", "profiles", "register"] {
        assert!(
            !capability::what_uses(m).is_empty(),
            "{m} is built and no capability says so"
        );
    }
}

#[test]
fn what_the_vault_and_the_hub_do_is_in_the_catalogue_at_all() {
    // Named rather than counted, because these are the six that were missing
    // and a count would go on passing if they were swapped for six others.
    let ids: BTreeSet<&str> = capability::all().into_iter().map(|c| c.id).collect();
    for id in ["vault", "recovery", "sync", "household", "hub", "reclaim"] {
        assert!(ids.contains(id), "{id} is built and wired and not in the catalogue");
    }
}

#[test]
fn a_module_can_be_traced_back_to_what_it_is_for() {
    // The direction Atlas needs when it is working on its own code: a file is
    // open, and the question is what breaks if this goes wrong.
    let uses = capability::what_uses("hublive");
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].id, "hub");

    // A module shared by two capabilities comes back as both.
    let language = capability::what_uses("language");
    let ids: BTreeSet<&str> = language.iter().map(|c| c.id).collect();
    assert!(ids.contains("accents") && ids.contains("translate"), "{ids:?}");

    // And plumbing honestly comes back empty rather than guessing.
    assert!(capability::what_uses("config").is_empty());
}

#[test]
fn the_document_about_what_atlas_does_is_the_catalogue_itself() {
    // docs/CAPABILITIES.md was hand-written and had drifted: it said "say it
    // doesn't know rather than guessing — working" for a capability nothing
    // in the running program could reach, and had said so for at least a
    // week. A separate document about what a system does becomes a document
    // about what somebody once meant to build.
    //
    // It is generated now, and this is what stops it being edited back into a
    // second opinion.
    // Line endings set aside: a Windows checkout has CRLF (found running the
    // suite natively on Windows, 26 Sep 2026).
    let on_disk = std::fs::read_to_string("docs/CAPABILITIES.md")
        .expect("docs/CAPABILITIES.md must exist")
        .replace("\r\n", "\n");
    let generated = capability::as_markdown();
    assert_eq!(
        on_disk, generated,
        "docs/CAPABILITIES.md no longer matches the catalogue. Either the list changed and the \
         document wasn't regenerated, or somebody edited the document. Run:\n\n    \
         atlas catalog --markdown > docs/CAPABILITIES.md\n"
    );
}

#[test]
fn the_document_carries_the_platform_columns_and_the_split_they_depend_on() {
    // Not just "it matches": that would pass if `as_markdown` returned an
    // empty string and the file were empty too.
    let doc = capability::as_markdown();
    for p in EVERY_PLATFORM {
        assert!(doc.contains(p.bare()), "{} has no column", p.bare());
    }
    assert!(doc.contains("| ready |"), "the platform legend is missing");
    assert!(
        doc.contains("keep a password, and hand it back when you ask"),
        "the vault is missing from the document"
    );
    // The split the whole document depends on: state and platform are
    // different facts, and something can be unbuilt on a platform that allows
    // it.
    assert!(doc.contains("not built yet"));
    assert!(doc.contains("the platform forbids it"));
    assert!(doc.contains("**no**"), "no wall is marked anywhere");
}

#[test]
fn the_script_that_regenerates_the_module_reference_can_still_find_the_tree() {
    // `docs/genref.py` had `ROOT = "/home/claude/atlas_v2"` — a directory
    // that stopped existing at the merge. The script could not be run, so
    // `docs/MODULE_REFERENCE_*.md` froze on 14 Sep while the tree kept
    // moving, and nothing said so. A generator nobody can run is a stale
    // document with extra steps.
    let script = std::fs::read_to_string("docs/genref.py").expect("docs/genref.py must exist");
    let root_line = script
        .lines()
        .find(|l| l.trim_start().starts_with("ROOT ="))
        .expect("genref.py must set ROOT");
    assert!(
        !root_line.contains("\"/") && !root_line.contains("'/"),
        "genref.py points at an absolute path that will be wrong on any other machine: {root_line}"
    );
    assert!(
        root_line.contains("__file__"),
        "ROOT should be worked out from where the script sits: {root_line}"
    );
    // And the output name is dated by the run, not typed in — otherwise
    // regenerating overwrites an old snapshot instead of making a new one.
    assert!(
        script.contains("datetime.date.today()"),
        "genref.py writes a fixed filename, so a regeneration destroys a dated snapshot"
    );
}

#[test]
fn every_area_is_in_the_listing() {
    // An area missing from `EVERY_AREA` is a whole section of the catalogue
    // that silently stops printing.
    let used: BTreeSet<String> =
        capability::all().into_iter().map(|c| format!("{:?}", c.area)).collect();
    let listed: BTreeSet<String> =
        capability::EVERY_AREA.iter().map(|a| format!("{a:?}")).collect();
    let dropped: Vec<&String> = used.difference(&listed).collect();
    assert!(dropped.is_empty(), "capabilities in an area nothing prints: {dropped:?}");

    // And the full listing really does print them all.
    let full = capability::full();
    for a in capability::EVERY_AREA {
        assert!(
            full.contains(&a.plain().to_uppercase()),
            "{} is missing from the listing",
            a.plain()
        );
    }
}

// ============ what it says before you run into a wall ============

#[test]
fn a_machine_with_walls_is_warned_about_and_one_without_is_not() {
    use atlas::portable::PortableConfig;
    let cfg = PortableConfig { warn_up_front: true };

    // The three desktops have nothing off the table, so saying "everything
    // here is possible" would be reassurance that teaches you to skip the
    // section — and then to miss the one that matters.
    for p in [Platform::Windows, Platform::Mac, Platform::Linux] {
        assert!(
            capability::heads_up(p, &cfg).is_none(),
            "{} has no walls and is being warned about them",
            p.name()
        );
    }

    // A phone does, and what it says is which of the things Atlas does are
    // affected, not which machine powers are missing.
    let said = capability::heads_up(Platform::Ios, &cfg).expect("an iPhone has walls");
    assert!(said.contains("On an iPhone or iPad"), "{said}");
    assert!(said.contains("can't work at all"), "{said}");
    assert!(said.contains("hearing you without opening it"), "{said}");
    assert!(said.contains("hear a wake word"), "the capability isn't named: {said}");

    // Off means silent, which is the half that makes it a setting.
    let off = PortableConfig { warn_up_front: false };
    assert!(capability::heads_up(Platform::Ios, &off).is_none());
    assert!(PortableConfig::default().warn_up_front, "it ships on");

    // And `atlas doctor` is what reads it, rather than a test.
    let doctor = std::fs::read_to_string("src/doctor.rs").expect("doctor.rs");
    assert!(
        doctor.contains("capability::heads_up("),
        "nothing says what this machine can't do"
    );
    // There is a section for it to arrive in now. `PortableConfig` had no
    // field anywhere in `ToolsConfig`, so no config file could set it.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("\nportable:"), "no portable block is shipped");
    assert!(raw.contains("warn_up_front:"));
    let tools: atlas::voice::ToolsConfig =
        serde_yaml::from_str("portable:\n  warn_up_front: false\n").expect("it parses");
    assert!(!tools.portable.warn_up_front, "the block still lands nowhere");
}
