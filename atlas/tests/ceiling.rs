//! The guards, checked from outside them.
//!
//! ## Why this is a separate file
//!
//! Across five versions, every *new* test file added here has survived and
//! been extended. Every *edit to an existing* test file has been lost. The
//! ceiling enforcement has now gone missing three times, each time as a small
//! edit inside `tests/wiring.rs`; `tests/freshness.rs` and
//! `tests/vault_crypto.rs` arrived whole and are still here.
//!
//! So this reads the other guard files as text rather than editing them.
//! Nothing here needs a line added anywhere else, which means it cannot be
//! lost by the thing that keeps losing lines.
//!
//! ## What it checks
//!
//! `tests/guards.rs` checks that a *name* is present. That is not enough on
//! its own, and this repo has the receipts: `UNWIRED_CEILING` stayed in
//! `tests/wiring.rs` across three versions while `the_baseline_never_grows` —
//! the test that enforced it — was absent. The manifest matched the constant
//! and reported everything fine.
//!
//! That is HOW_TO_FIND_PROBLEMS §1 committed by the detector itself: a check
//! satisfiable without the thing being true. So this file enforces the
//! *effect* rather than the presence of the code that enforces it.


fn read(path: &str) -> Option<String> {
    crate::common::read_source_path(path)
}

/// Pull `const NAME: usize = N;` out of a source file.
fn usize_const(text: &str, name: &str) -> Option<usize> {
    let at = text.find(&format!("const {name}: usize"))?;
    let rest = &text[at..];
    let eq = rest.find('=')?;
    let semi = rest.find(';')?;
    rest[eq + 1..semi].trim().replace('_', "").parse().ok()
}

/// Count the string literals inside `const NAME: &[&str] = &[ ... ];`
fn slice_len(text: &str, name: &str) -> Option<usize> {
    let at = text.find(&format!("const {name}"))?;
    let rest = &text[at..];
    let open = rest.find("= &[")? + 4;
    let close = rest[open..].find("];")? + open;
    Some(
        rest[open..close]
            .lines()
            .filter(|l| l.trim_start().starts_with('"'))
            .count(),
    )
}

// --- the effect, not the code that produces it ------------------------------

#[test]
fn the_unwired_list_is_within_its_ceiling() {
    // Enforced here rather than trusting that the test inside wiring.rs still
    // exists. It has not, three times.
    let Some(text) = read("tests/wiring.rs") else {
        panic!("tests/wiring.rs is gone — the wiring guard no longer exists at all");
    };
    let ceiling = usize_const(&text, "UNWIRED_CEILING")
        .expect("UNWIRED_CEILING is missing from tests/wiring.rs");
    let listed =
        slice_len(&text, "UNWIRED_BASELINE").expect("UNWIRED_BASELINE is missing or malformed");

    assert!(
        listed <= ceiling,
        "the unwired list has grown to {listed}, past its ceiling of {ceiling}.\n\
         Wiring a module in is the fix. Raising the ceiling is admitting you \
         built {} more things nothing can reach.",
        listed - ceiling
    );
}

#[test]
fn the_ceiling_has_not_drifted_above_what_is_listed() {
    // A ceiling far above the real count stops being a constraint.
    let Some(text) = read("tests/wiring.rs") else { return };
    let (Some(ceiling), Some(listed)) = (
        usize_const(&text, "UNWIRED_CEILING"),
        slice_len(&text, "UNWIRED_BASELINE"),
    ) else {
        return;
    };
    assert!(
        ceiling.saturating_sub(listed) <= 5,
        "the ceiling is {ceiling} but only {listed} names are listed — lower it \
         to {listed} so it means something again"
    );
}

// --- guard files must not quietly lose tests --------------------------------

/// How many tests each guard file must still contain.
///
/// A name check cannot see a guard file that survives with a test removed
/// from it. This can.
const TEST_FLOOR: &[(&str, usize)] = &[
    ("tests/wiring.rs", 4),
    ("tests/guards.rs", 3),
    ("tests/retrospective.rs", 5),
    ("tests/vault_crypto.rs", 9),
    ("tests/ceiling.rs", 6),
];

#[test]
fn no_guard_file_has_quietly_lost_a_test() {
    // Assembled rather than written literally: a test file containing the
    // bare attribute in a string truncates its own body for any scanner
    // reading these files, including tests/retrospective.rs.
    let attr = concat!("#[", "test]");
    let mut shrunk = Vec::new();
    for (path, floor) in TEST_FLOOR {
        match read(path) {
            None => shrunk.push(format!("{path} is gone entirely")),
            Some(text) => {
                let count = text.matches(attr).count();
                if count < *floor {
                    shrunk.push(format!("{path} has {count} tests, floor is {floor}"));
                }
            }
        }
    }
    assert!(
        shrunk.is_empty(),
        "a guard file lost a test:\n  {}\n\nRestore it, or lower the floor deliberately.",
        shrunk.join("\n  ")
    );
}

// --- the load-bearing guards, by effect -------------------------------------

#[test]
fn the_vault_actually_protects_what_it_stores() {
    // This was `the_vault_still_refuses_what_it_cannot_protect`, and it said
    // in its own failure message to replace it once the crypto became real.
    // It has, so this checks the other direction — still through the API
    // rather than by grepping for the constant, so deleting the flag and
    // leaving the name behind would not pass.
    use atlas::vault::{Kind, Vault, VaultConfig};
    let mut v = Vault::default();
    v.open("a long enough passphrase here", 0, &VaultConfig::default())
        .expect("passphrase length is fine");
    v.put("x", Kind::Login, "secret", 0).expect("a credential must store now");

    let stored = v.secrets.iter().find(|s| s.name == "x").expect("it was stored");
    assert!(stored.real, "stored under the stand-in cipher");
    assert_ne!(
        stored.sealed, b"secret",
        "the value was written out in the clear"
    );
    // Round-trips, so "encrypted" does not quietly mean "lost".
    assert_eq!(v.get("x", 0).unwrap(), "secret");
    assert!(v.weakly_sealed().is_empty(), "a credential is only weakly sealed");
}

#[test]
fn atlas_metrics_is_still_dispatched() {
    let Some(main) = read("src/main.rs") else {
        panic!("src/main.rs is gone");
    };
    assert!(
        main.contains("fn run_metrics"),
        "run_metrics() is gone — docs/METRICS.md will rot while still looking current"
    );
    assert!(
        main.contains("Some(\"metrics\")"),
        "nothing dispatches `atlas metrics` any more"
    );
}

#[test]
fn selfgrant_still_fails_closed_on_its_own_limits() {
    // The side doors: tests and config reach the same place as policy.rs.
    use atlas::selfgrant::{self, Reach};
    for p in [
        "tests/wiring.rs",
        "tests/ceiling.rs",
        "config/tools.yaml",
        "src/policy.rs",
        "src/selfgrant.rs",
    ] {
        assert_eq!(
            selfgrant::reach_of(p),
            Reach::ItsOwnLimits,
            "{p} became grantable — Atlas can now edit what constrains it"
        );
    }
}

// A test was added here that read all three not-yet-wired lists and worked out
// for itself whether each entry was stale. It was deleted before it ever
// passed, and the reason is worth keeping.
//
// Deciding "is this module wired" is a *rule*, and that rule already lives in
// two places that need it -- `wiring.rs` (which strips config-only references,
// because a module named solely as a `ToolsConfig` field is not wired) and
// `capability_wiring.rs` (which distinguishes a borrowed helper from the
// capability). A third copy here got a different answer on its first run:
// eleven modules reported stale that are not. Of course it did. It was a
// cheaper rule wearing the same name.
//
// The problem those three lists actually cause is not that they exist -- the
// checks are complementary and `capability_wiring.rs` says so in its own code.
// It is that wiring one module means deleting it from more than one, and each
// deletion was discovered a full run apart.
//
// That is a *message* problem, and it is fixed where the messages are: each
// of the three now names the other two. No fourth copy of the rule, no fourth
// list, and one failure tells you everywhere to look.
