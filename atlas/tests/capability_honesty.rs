//! The capability list must not flatter the system.
//!
//! `Intent::Capabilities` is how you find out what Atlas can do. It reads a
//! hand-maintained list in `capability::all()`. Nothing ever checked that list
//! against reality, and it drifted: seven capabilities were advertised as
//! `Working` while the module behind them was unreachable from the entrypoint.
//!
//! That is the worst possible place for drift. Everything else being wrong
//! costs you a feature; this being wrong costs you the ability to tell.
//!
//! These tests bind the advertised list to `tests/wiring.rs`, so a capability
//! cannot claim to work until its module is actually wired in.
//!
//! # Both directions, since 19 Sep 2026
//!
//! Every test in this file read **unwired ⇒ must be Planned**. None read the
//! reverse, and that is the half that was failing. `Planned` is the state a
//! capability starts in, so with nothing checking it a capability could be
//! built, wired, tested and shipped while the catalogue still said "not built
//! yet" — and **27 of the 29 `Planned` entries had done exactly that**, 26 of
//! them still carrying `needs: "wiring in"` beside eight or nine or fifteen
//! production call sites.
//!
//! Under-reporting is the same defect as over-reporting and is worse in one
//! respect: a capability that claims too much is found out the first time
//! somebody tries it, and one that claims too little is never tried.
//!
//! `a_planned_capability_says_why_it_is_planned` closes it, with the same
//! shape `dead_capabilities.rs` uses for `ORPHANS`: not a count, a name and a
//! written reason each.

use atlas::capability::{self, State};
use std::collections::BTreeSet;
use std::fs;

/// Read the orphan list from the wiring guard — one source of truth.
fn unwired() -> BTreeSet<String> {
    let text = fs::read_to_string("tests/wiring.rs").expect("wiring.rs must exist");
    let after = text
        .split("UNWIRED_BASELINE")
        .nth(1)
        .expect("UNWIRED_BASELINE must exist");
    let body = after
        .split_once("= &[")
        .and_then(|(_, rest)| rest.split_once("];"))
        .map(|(inside, _)| inside)
        .expect("baseline must be a slice literal");

    body.split(',')
        .filter_map(|piece| {
            let t = piece.trim();
            let t = t.strip_prefix('"')?.strip_suffix('"')?;
            (!t.is_empty()).then(|| t.to_string())
        })
        .collect()
}

/// Capability ids that deliberately do not map to a module of the same name.
/// Kept short and explicit — a long list here would defeat the check.
const NOT_A_MODULE: &[&str] = &[
    "wake", "speak", "apps", "panels", "accents", "translate", "tax", "backup",
    // Umbrella entries covering several modules rather than one.
    "reason",
    // Named for the action, not the file: its module is `online`.
    "delegate_online",
    // Named for what you ask for, not the file. `design` lives in `taste`
    // (reviewing a page against a house style) and `animate` in `motion`
    // (drawing an SVG animation) — the id is the verb a person would use.
    "design",
    "animate",
    // `remember` is the fact book — its module is `facts`, but "remember" is
    // the word a person uses for it.
    "remember",
    // Added 28 Sep 2026: both entries were already in the catalogue and this
    // test was failing on the round-3 merge. `cutin` (cutting in while it
    // speaks) lives in `micthread`; `background` is an umbrella over
    // `notifyicon`, `startup`, `crash`, `goodbye` and `onlyone`.
    "cutin",
    "background",
];

#[test]
fn nothing_unreachable_is_advertised_as_working() {
    let orphans = unwired();
    let lying: Vec<String> = capability::all()
        .into_iter()
        .filter(|c| orphans.contains(c.id))
        .filter(|c| c.state == State::Working)
        .map(|c| format!("{} — claims \"{}\" but nothing can reach it", c.id, c.what))
        .collect();

    assert!(
        lying.is_empty(),
        "the capability list is telling you Atlas can do things it cannot:\n  {}\n\n\
         Either wire the module in and remove it from UNWIRED_BASELINE, or mark \
         the capability State::Planned.",
        lying.join("\n  ")
    );
}

#[test]
fn unreachable_capabilities_are_marked_planned() {
    // Stronger than the test above: an unreachable module is not `Off` (which
    // implies it works and is switched off) and not `Untested` (which implies
    // it runs but has not met hardware). It is `Planned`.
    let orphans = unwired();
    let mislabelled: Vec<String> = capability::all()
        .into_iter()
        .filter(|c| orphans.contains(c.id))
        .filter(|c| c.state != State::Planned)
        .map(|c| format!("{} is {:?}, should be Planned", c.id, c.state))
        .collect();

    assert!(
        mislabelled.is_empty(),
        "unreachable modules must be advertised as not-built-yet:\n  {}",
        mislabelled.join("\n  ")
    );
}

/// The other unwired list — the stricter one.
///
/// `wiring.rs` asks whether a module is reachable from `main`. That is the
/// loose question, and a module can pass it while the capability it is named
/// for does not exist: `atlas mesh` reaches `mesh.rs` to give advice about a
/// private network, and reaching another device directly is still not built.
/// `capability_wiring.rs` holds that stricter notion, and a `Planned` entry
/// may be justified by either.
fn capability_unwired() -> BTreeSet<String> {
    let text =
        fs::read_to_string("tests/capability_wiring.rs").expect("capability_wiring.rs must exist");
    let after = text.split("CAPABILITY_UNWIRED").nth(1).expect("CAPABILITY_UNWIRED must exist");
    let body = after
        .split_once("= &[")
        .and_then(|(_, rest)| rest.split_once("];"))
        .map(|(inside, _)| inside)
        .expect("the list must be a slice literal");
    // Line by line, not `split(',')`.
    //
    // `unwired()` above splits on commas, and that works on
    // `UNWIRED_BASELINE` only because its comments happen to contain them.
    // The comment above `"mesh"` in `CAPABILITY_UNWIRED` ends in a full stop,
    // so the comma-split piece is three lines of prose with the entry on the
    // end — it does not start with a quote, and `mesh` was silently dropped.
    // The guard below then reported `mesh` as unexplained when the list
    // explains it at length. Found the first time this ran.
    //
    // An entry in these lists is a line that is exactly `"name",`, which is
    // how every one of them is written and is a rule prose cannot break.
    body.lines()
        .filter_map(|line| {
            let t = line.trim();
            let t = t.strip_suffix(',').unwrap_or(t);
            let t = t.strip_prefix('"')?.strip_suffix('"')?;
            (!t.is_empty() && !t.contains(char::is_whitespace)).then(|| t.to_string())
        })
        .collect()
}

/// `Planned` entries that no list accounts for, each with the reason.
///
/// A capability is
/// `Planned` because something in this tree records that it cannot be
/// reached — its id or one of its modules on `UNWIRED_BASELINE` or
/// `CAPABILITY_UNWIRED`. This list is for the case that is genuinely neither:
/// reachable, and still not a thing Atlas can do. Adding a row costs a
/// sentence saying why, which is the point.
const PLANNED_FOR_A_REASON_OF_ITS_OWN: &[(&str, &str)] = &[(
    "mesh",
    "reaching another device directly. Its code is reached now -- `nearby::look` \
     shouts on the local network, `mesh::on_this_network` asks whether the machine \
     you sync with answered, and `mesh::choose` returns `Path::SameNetwork` from a \
     real observation rather than the hardcoded literal every caller used to pass. \
     So it came off CAPABILITY_UNWIRED honestly. What is still not built is the \
     sending: the bundle goes through a folder both machines can see, and nothing \
     hands it straight across. Found, and still not reached.",
)];

#[test]
fn a_planned_capability_says_why_it_is_planned() {
    // The half this file was missing. See the header: 27 of 29 `Planned`
    // entries were wired, and nothing here looked.
    let mut accounted = unwired();
    accounted.extend(capability_unwired());
    let named: BTreeSet<&str> =
        PLANNED_FOR_A_REASON_OF_ITS_OWN.iter().map(|(id, _)| *id).collect();

    let unexplained: Vec<String> = capability::all()
        .into_iter()
        .filter(|c| c.state == State::Planned)
        .filter(|c| !named.contains(c.id))
        .filter(|c| {
            !accounted.contains(c.id) && !c.modules.iter().any(|m| accounted.contains(*m))
        })
        .map(|c| format!("{} — \"{}\"", c.id, c.what))
        .collect();

    assert!(
        unexplained.is_empty(),
        "these are advertised as not built yet, and nothing in the tree says why:\n  {}\n\n\
         `Planned` is where a capability starts, so it is also where one gets left \
         after it is wired — which is how 27 of these accumulated. Either give the \
         entry the state the evidence supports, or name it in \
         PLANNED_FOR_A_REASON_OF_ITS_OWN with a sentence saying what is missing.",
        unexplained.join("\n  ")
    );
}

#[test]
fn every_reason_of_its_own_is_a_real_one() {
    // A list of names with no reasons is a ceiling again, just longer — the
    // lesson `dead_capabilities.rs` learned the expensive way.
    let ids: BTreeSet<&str> = capability::all().into_iter().map(|c| c.id).collect();
    for (id, why) in PLANNED_FOR_A_REASON_OF_ITS_OWN {
        assert!(ids.contains(id), "{id} is not a capability");
        assert!(why.len() > 60, "{id} is listed with no real explanation: {why:?}");
    }
}

#[test]
fn nothing_still_claims_it_is_waiting_to_be_wired_while_something_calls_it() {
    // `needs: "wiring in"` means nothing can reach it. Twenty-six entries
    // carried that phrase beside real production call sites, which is the
    // part of the drift that was legible in the data itself and still went
    // unread for eight days.
    let mut accounted = unwired();
    accounted.extend(capability_unwired());

    let lying: Vec<String> = capability::all()
        .into_iter()
        .filter(|c| c.needs.is_some_and(|n| n.contains("wiring in")))
        .filter(|c| {
            !accounted.contains(c.id) && !c.modules.iter().any(|m| accounted.contains(*m))
        })
        .map(|c| format!("{} says it is waiting to be wired in", c.id))
        .collect();

    assert!(
        lying.is_empty(),
        "{:?}\n\nNothing records these as unreachable, so \"wiring in\" is not what \
         they are waiting on. Say what is, or drop the `needs`.",
        lying
    );
}

#[test]
fn usable_only_means_working() {
    // Guards the definition the tests above depend on.
    assert!(State::Working.usable());
    assert!(!State::Off.usable());
    assert!(!State::Untested.usable());
    assert!(!State::Blocked.usable());
    assert!(!State::Planned.usable());
}

#[test]
fn capability_ids_are_unique() {
    let all = capability::all();
    let ids: BTreeSet<&str> = all.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), all.len(), "duplicate capability id");
}

#[test]
fn capability_ids_mostly_name_a_real_module() {
    // Not every capability is one module, but most are, and a typo'd id would
    // silently exempt itself from the honesty checks above.
    //
    // A bare `src/{id}.rs` used to be the whole check, and it was right for
    // as long as every module lived at the top of `src/`. The market-merge
    // changed that: `market` became a directory (`src/market/mod.rs`), and
    // `claims`, `timeframe`, `events` and `feed` moved to files nested inside
    // it (`src/market/claims.rs` and siblings) rather than disappearing.
    // Checking only the top level read all five as gone and would have
    // demanded either a `NOT_A_MODULE` exemption that was a lie (they do
    // name real modules) or renaming ids that other code and docs already
    // reference by their short name. Searching the whole tree for the
    // basename is what actually answers "does a module by this name exist",
    // regardless of how deep the merge nested it.
    let has_module = |id: &str| -> bool {
        let top = format!("src/{id}.rs");
        let top_dir = format!("src/{id}/mod.rs");
        if std::path::Path::new(&top).exists() || std::path::Path::new(&top_dir).exists() {
            return true;
        }
        fn search(dir: &std::path::Path, want: &str) -> bool {
            let Ok(entries) = std::fs::read_dir(dir) else { return false };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if search(&path, want) {
                        return true;
                    }
                } else if path.file_name().and_then(|n| n.to_str())
                    == Some(&format!("{want}.rs"))
                {
                    return true;
                }
            }
            false
        }
        search(std::path::Path::new("src"), id)
    };

    let missing: Vec<&str> = capability::all()
        .into_iter()
        .map(|c| c.id)
        .filter(|id| !NOT_A_MODULE.contains(id))
        .filter(|id| !has_module(id))
        .collect();

    assert!(
        missing.is_empty(),
        "these capability ids match no module anywhere under src/, and no exemption — typo, \
         or add them to NOT_A_MODULE deliberately: {missing:?}"
    );
}

#[test]
fn a_blocked_capability_says_what_it_is_waiting_on() {
    let vague: Vec<&str> = capability::all()
        .into_iter()
        .filter(|c| c.state == State::Blocked && c.needs.is_none())
        .map(|c| c.id)
        .collect();
    assert!(
        vague.is_empty(),
        "blocked with no stated cause is useless to you: {vague:?}"
    );
}
