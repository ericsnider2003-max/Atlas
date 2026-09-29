//! The shipped config is part of the product, and drifts like code does.
//!
//! Every `*Config` struct in this tree declares its own `Default`, and
//! `config/tools.yaml` is what a real install actually runs on. Nothing was
//! comparing the two. That is how a tree arrived with eighty-four
//! `enabled: false` lines and exactly one `true` — research, notifications,
//! panels, the clipboard, the hub, the quick-input surface and a dozen more
//! were all switched off in the file, against code that says each of them is
//! on unless you turn it off, and against tests elsewhere in this suite that
//! assert exactly that. Fourteen of those tests were failing at once for this
//! single reason.
//!
//! It went unnoticed for a specific, boring reason worth writing down: this
//! sandbox could not link the full test binary set, so the suite was being
//! run in slices, and a config default is the kind of thing only the whole
//! suite disagrees with. One `backup.enabled: false` was caught and fixed by
//! hand during an earlier merge; the other eighty-three were the same bug and
//! nobody could see them.
//!
//! So: one test, comparing the shipped file against the code's own defaults,
//! with an explicit list of the places they are *meant* to differ. A section
//! that ships off against a default of on has to be named here, with the
//! reason, or this fails.

use atlas::voice::ToolsConfig;

fn shipped() -> ToolsConfig {
    let y = std::fs::read_to_string("config/tools.yaml").expect("config/tools.yaml");
    serde_yaml::from_str(&y).expect("config/tools.yaml parses")
}

/// Sections deliberately shipped off despite defaulting on, and why.
///
/// Empty today. It exists so that turning something off in the shipped file
/// is a decision someone wrote down, rather than a line that drifted.
const DELIBERATELY_OFF: &[(&str, &str)] = &[];

/// Every `enabled` flag, paired: what the file says, what the code defaults
/// to. Kept as an explicit list rather than derived, because `ToolsConfig`
/// has no `Serialize` and adding one purely for a test would put a public
/// derive on the config type to serve a private need.
fn flags(t: &ToolsConfig) -> Vec<(&'static str, bool)> {
    vec![
        ("voice", t.enabled),
        ("wake", t.wake.as_ref().map(|w| w.enabled).unwrap_or(false)),
        ("proactive", t.proactive.enabled),
        ("research", t.research.enabled),
        ("brief", t.brief.enabled),
        ("voice_id", t.voice_id.enabled),
        ("presence", t.presence.enabled),
        ("server", t.server.enabled),
        ("backup", t.backup.enabled),
        ("kin", t.kin.enabled),
        ("quick_input", t.quick_input.enabled),
        ("identity", t.identity.enabled),
        ("viewing", t.viewing.enabled),
        ("ocr", t.ocr.enabled),
        ("hub", t.hub.enabled),
        ("finance", t.finance.enabled),
        ("clipboard", t.clipboard.enabled),
        ("certainty", t.certainty.enabled),
        ("budget", t.budget.enabled),
        ("overnight", t.overnight.enabled),
        ("panels", t.panels.enabled),
        ("endpoint", t.endpoint.enabled),
        ("dictate", t.dictate.enabled),
        ("watching", t.watching.enabled),
        ("person", t.person.enabled),
        ("returning", t.returning.enabled),
        ("capture", t.capture.enabled),
        ("walkthrough", t.walkthrough.enabled),
        ("opsec", t.opsec.enabled),
        ("working_set", t.working_set.enabled),
        ("files", t.files.enabled),
        ("daily", t.daily.enabled),
        ("awake", t.awake.enabled),
        ("consolidate", t.consolidate.enabled),
        ("notify", t.notify.enabled),
        ("vision", t.vision.enabled),
    ]
}

#[test]
fn the_shipped_config_does_not_switch_off_what_the_code_ships_on() {
    let file = flags(&shipped());
    let code = flags(&ToolsConfig::default());

    let mut wrong = Vec::new();
    for ((name, on_in_file), (same, on_in_code)) in file.iter().zip(code.iter()) {
        assert_eq!(name, same, "the two lists must stay in the same order");
        if *on_in_code && !*on_in_file && !DELIBERATELY_OFF.iter().any(|(n, _)| n == name) {
            wrong.push(*name);
        }
    }

    assert!(
        wrong.is_empty(),
        "the shipped config switches off {} thing{} the code ships on:\n  {}\n\n\
         Either set them true in config/tools.yaml, or add each one to \
         DELIBERATELY_OFF in this file with the reason it ships off.",
        wrong.len(),
        if wrong.len() == 1 { "" } else { "s" },
        wrong.join("\n  ")
    );
}

#[test]
fn a_section_shipped_on_against_a_default_of_off_is_also_a_decision() {
    // The other direction is not a bug by itself — some things genuinely
    // want to be on in a real install and off in a bare `Default` — but it
    // should not be silent either, because "on in the shipped file" is what
    // a real machine runs, and a sensor or a network reach turned on by a
    // line nobody remembers writing is the shape of problem this whole tree
    // is built to avoid.
    let file = flags(&shipped());
    let code = flags(&ToolsConfig::default());

    // Named, with the reason, exactly like the list above.
    //
    // Empty. It briefly held `research` and `voice`, on the reasoning that
    // `settings.rs`'s registry declared both on even though their own
    // `Default` impls said off — which was not a reason, it was the
    // disagreement itself being used to justify one side of it. With the
    // registry deriving its defaults there is one declaration, both ship
    // off, and the entries went with the reasoning.
    const ON_ON_PURPOSE: &[(&str, &str)] = &[(
        "server",
        "The local dashboard. This is not a new capability being switched on: \
         `Server::bind` never read `enabled`, and the start-up path forced it \
         true on a copy of your config, so the listener came up on every \
         install while the file said `false`. Making the switch real meant \
         choosing which half to change, and shipping `false` would have taken \
         away a dashboard everyone already has. The struct default stays off, \
         so an install with no tools.yaml listens to nothing. Loopback only, \
         token on every request. See tests/a_switch_that_does_nothing.rs.",
    )];

    let mut surprises = Vec::new();
    for ((name, on_in_file), (_, on_in_code)) in file.iter().zip(code.iter()) {
        if *on_in_file && !*on_in_code && !ON_ON_PURPOSE.iter().any(|(n, _)| n == name) {
            surprises.push(*name);
        }
    }

    assert!(
        surprises.is_empty(),
        "the shipped config switches on {:?}, which the code defaults off. \
         Add each to ON_ON_PURPOSE with the reason, or take it out of \
         config/tools.yaml.",
        surprises
    );
}

#[test]
fn the_default_is_actually_derived_and_not_just_a_copy_of_the_value() {
    // The test below asserts that nothing reads as changed when the registry
    // is handed the default config. On its own that is a weaker claim than
    // it looks: `build` fills each item's `default` with a placeholder equal
    // to its own value, so if `registry` ever stopped overwriting them,
    // *nothing* would ever read as changed and both that test and the hub's
    // "changed from default" markers would go quietly, permanently blank
    // while every test still passed.
    //
    // Found by deliberately deleting the derivation and watching the guards
    // not notice. This is the positive half: a config that genuinely differs
    // from the code's default must be reported as differing, by name.
    let mut t = ToolsConfig::default();
    assert!(t.research.enabled, "this test picks research because it defaults on (since 27 Sep 2026)");
    t.research.enabled = false;

    let s = atlas::settings::registry(&t);
    let changed: Vec<&str> = s.changed().iter().map(|i| i.key.as_str()).collect();
    assert_eq!(
        changed,
        vec!["research.enabled"],
        "a config that differs from the code's default in exactly one place was not \
         reported that way -- the registry is not deriving its defaults"
    );
}

#[test]
fn there_is_only_one_place_a_default_is_declared() {
    // `settings.rs`'s registry used to hardcode a default per key beside the
    // live value, so the hub could show "changed from default", while each
    // config struct declared its own `Default` separately. Nothing made them
    // agree and three had drifted: `voice.enabled` and `research.enabled`
    // were written `true` in the registry against `Default` impls saying
    // false, and `models.memory_budget_mb` was 3500 against a struct default
    // of 6000. The hub was reporting three settings as unchanged when they
    // were changed, and vice versa.
    //
    // `registry` derives every default now, by building the same list a
    // second time against `ToolsConfig::default()` and taking the value.
    // There is no second place to write one, so this is not "they happen to
    // agree" — it is structural, and this test says so by asserting the
    // strongest form: fed the default config, *nothing* reads as changed.
    let s = atlas::settings::registry(&ToolsConfig::default());
    let disagree: Vec<&str> = s.changed().iter().map(|i| i.key.as_str()).collect();
    assert!(
        disagree.is_empty(),
        "the registry says {disagree:?} differ from the default while being handed \
         the default config, which can only mean a default has been hardcoded \
         somewhere again rather than derived"
    );
}
