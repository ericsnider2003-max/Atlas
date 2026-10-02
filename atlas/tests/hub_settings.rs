mod common; // `common::source_of`: a module's source wherever its files live

use atlas::hub::{esc, form_field, list_page_at, permissions_page, route, settings_page, status_page, urldecode, Page};
use atlas::settings::{registry, Settings, Value, Weight};
use atlas::voice::ToolsConfig;

fn shipped() -> ToolsConfig {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    serde_yaml::from_str(&y).unwrap()
}
fn s() -> Settings {
    registry(&shipped())
}

// ================= every switch is findable =================

#[test]
fn every_toggle_i_have_added_appears_in_the_hub() {
    // The gap this closes: settings that exist only in YAML are settings
    // nobody can find.
    let s = s();
    for key in [
        "presence.enabled",
        "voice_id.enabled",
        "wake.enabled",
        "proactive.enabled",
        "identity.enabled",
        "server.enabled",
        "ocr.enabled",
        "research.enabled",
        "backup.enabled",
        "quick_input.enabled",
    ] {
        assert!(s.get(key).is_some(), "{key} has no way to be turned on or off");
    }
}

#[test]
fn every_setting_explains_itself_in_plain_language() {
    for item in &s().items {
        assert!(!item.what.is_empty(), "{} has no explanation", item.key);
        assert!(!item.what.contains("bool"), "{} explains itself in code terms", item.key);
        assert!(item.what.ends_with('.'), "{} should read as a sentence", item.key);
    }
}

#[test]
fn anything_with_a_real_cost_states_it() {
    // A toggle that turns on a camera must not look like one that changes
    // sentence length.
    for item in s().items.iter().filter(|i| i.weight.needs_confirming()) {
        assert!(!item.cost.is_empty(), "{} has a consequence but doesn't say what", item.key);
    }
}

#[test]
fn sensors_and_permissions_are_weighted_differently_from_taste() {
    let s = s();
    assert_eq!(s.get("presence.enabled").unwrap().weight, Weight::Sensitive);
    assert_eq!(s.get("proactive.enabled").unwrap().weight, Weight::Permission);
    assert_eq!(s.get("persona.tone").unwrap().weight, Weight::Preference);
    assert!(Weight::Sensitive.needs_confirming());
    assert!(!Weight::Preference.needs_confirming());
}

#[test]
fn the_permissions_page_gathers_everything_consequential() {
    let s = s();
    let c = s.consequential();
    assert!(c.len() >= 4);
    assert!(c.iter().all(|i| i.weight.needs_confirming()));
    // Permissions before sensors, so the sharpest things read first.
    assert_eq!(c[0].weight, Weight::Permission);
}

/// The shipped file is allowed to differ from the code's defaults, but only
/// where someone decided to and said why. Anything else is drift.
///
/// This used to assert the changed set was *empty*, and it passed for the
/// wrong reason: the registry hardcoded its own defaults beside each live
/// value, and those literals happened to have been written to match the
/// shipped file rather than the code. The default is derived from
/// `ToolsConfig::default()` now, so this compares the shipped file against
/// what a fresh install actually does — and the one real difference, which
/// was always there, finally shows up.
const DELIBERATELY_DIFFERENT: &[(&str, &str)] = &[
    (
        "tts_engine.engine",
        "kokoro in the shipped file since Phase 0.8 (1 Oct 2026): Kokoro is the voice Atlas ships with and \
         piper the fallback when its model isn't there yet. The struct default stays piper, the engine that \
         needs nothing downloaded beyond its own voice.",
    ),
    (
        "voice_settings.voice",
        "af_bella in the shipped file, a Kokoro preset, because Kokoro ships as the engine (Phase 0.8). The \
         struct default stays a piper voice, to match the struct's own engine.",
    ),
    (
        "self_work.enabled",
        "on in the shipped file by Eric's ruling of 30 Sep 2026: bug fixes Atlas may make itself, landing \
         still waits for his yes. It's the Self-repair switch, and from 1 Oct 2026 the one that's read. \
         The struct default stays off: an install with no tools.yaml never works on its own code.",
    ),
    (
        "wake.enabled",
        "on in the shipped file from 29 Sep 2026 (Eric: talk freely, push-to-talk only as the fallback). \
         The name is listened for on the running microphone stream and speech-to-text runs only while \
         someone is talking (`utterance`), not on every three seconds of silence as before. With no \
         tools.yaml there is no wake configuration at all, so the struct default stays off.",
    ),
    (
        "editcraft.enabled",
        "on in the shipped file by Eric's ruling of 25 Sep 2026 (I: he makes video content). The struct \
         default stays off: an install with no tools.yaml gives no creator advice.",
    ),
    (
        "routine.enabled",
        "on in the shipped file by Eric's ruling of 25 Sep 2026 (E4: concrete routines run on their own, \
         abstract ones ask first). The struct default stays off: an install with no tools.yaml watches \
         nothing for patterns.",
    ),
    (
        "prose.enabled",
        "on in the shipped file by Eric's ruling of 25 Sep 2026 (H4: fix in place as you type). The struct \
         default stays off: an install with no tools.yaml types into nobody's apps.",
    ),
    (
        "signin.enabled",
        "on by Eric's ruling of 25 Sep 2026 (B4: autofill, yes). It still needs the vault open and a grant \
         per site; the struct default stays off for an install with no file.",
    ),
    (
        "enrol.enabled",
        "on by Eric's ruling of 25 Sep 2026 (B6: Atlas may make accounts). Payment and ID still end a run \
         and a robot check still hands over; the struct default stays off.",
    ),
    (
        "walkthrough.atlas_clicks",
        "on by Eric's ruling of 25 Sep 2026 (B1: \"turn it on or off for me\"), after the read-back and \
         his yes. The struct default stays off.",
    ),
    (
        "confirmed.enabled",
        "on by Eric's ruling of 25 Sep 2026 (B1): security changes with the read-back, one yes each, him \
         at the machine. The struct default stays off.",
    ),
    (
        "server.enabled",
        "true against a struct default of false, and the two disagree on purpose. \
         The struct default is what an install with no tools.yaml at all gets, and \
         there the file's usual rule holds: no config means nothing listens. The \
         shipped file says true because that is what already happened -- \
         `enabled` was never read, and the start-up path forced it on, so every \
         existing install has a dashboard. Shipping false would have been a \
         silent capability removal dressed up as a bug fix. See \
         tests/a_switch_that_does_nothing.rs.",
    ),
    (
        "persona.tone",
        "warm against a struct default of dry. Eric asked for it in as many \
         words -- conversational throughout, including on actions -- when he \
         said he should not have to follow a script to talk to Atlas. `dry` \
         stays the code default because it is the Jarvis register the module \
         was written around, and the shipped file is what HE starts from.",
    ),
    (
        "persona.max_spoken_sentences",
        "8 against a struct default of 3, and it is a CEILING rather than a \
         target. `run_command` resolves the actual length as \
         `max_spoken_sentences.min(mode_cap).min(register.length())`, so any \
         ceiling below `Register::Chatting.length()` (8) is the binding \
         constraint on every conversation and the register can never decide \
         -- which is the clipped-reply problem living in the config file. A \
         task is still two sentences, because `Register::Working.length()` \
         is 2. The struct default stays 3 for callers building a `Persona` \
         with no register to hand.",
    ),
    (
    "models.memory_budget_mb",
    "3500 against a struct default of 6000. 6000MB of model on an 8GB \
     machine leaves the rest of the system fighting for what's left, and \
     this file is what every install starts from. Which of the two numbers \
     is the right default is an open question -- see \
     OUTSTANDING_TASKS_2026-09-14.md -- but until it is answered the \
     cautious one ships.",
    ),
];

#[test]
fn the_shipped_config_only_differs_from_the_defaults_on_purpose() {
    let s = s();
    let unexplained: Vec<&str> = s
        .changed()
        .iter()
        .map(|i| i.key.as_str())
        .filter(|k| !DELIBERATELY_DIFFERENT.iter().any(|(n, _)| n == k))
        .collect();
    assert!(
        unexplained.is_empty(),
        "the shipped config differs from the code's defaults on {unexplained:?} with no \
         reason recorded. Either bring config/tools.yaml back in line, or add each key to \
         DELIBERATELY_DIFFERENT with why it ships differently."
    );
}

#[test]
fn you_can_see_what_you_have_changed_from_the_defaults() {
    let mut s = s();
    let before = s.changed().len();
    s.set("presence.enabled", "on").unwrap();
    assert_eq!(s.changed().len(), before + 1);
    s.reset("presence.enabled").unwrap();
    assert_eq!(s.changed().len(), before, "resetting did not put it back");
}

// ================= changing things =================

#[test]
fn a_toggle_can_be_flipped_and_put_back() {
    let mut s = s();
    assert_eq!(s.get("ocr.enabled").unwrap().value, Value::Toggle(false));
    let said = s.set("ocr.enabled", "on").unwrap();
    assert!(said.contains("is now on"), "got: {said}");
    s.reset("ocr.enabled").unwrap();
    assert_eq!(s.get("ocr.enabled").unwrap().value, Value::Toggle(false));
}

#[test]
fn a_number_outside_its_range_is_refused_with_the_range() {
    let mut s = s();
    let e = s.set("persona.max_spoken_sentences", "40").unwrap_err();
    assert!(e.contains("between 1 and 8"), "got: {e}");
    assert!(s.set("persona.max_spoken_sentences", "banana").is_err());
    assert!(s.set("persona.max_spoken_sentences", "4").is_ok());
}

#[test]
fn a_choice_only_accepts_its_own_options() {
    let mut s = s();
    let e = s.set("persona.tone", "sarcastic").unwrap_err();
    assert!(e.contains("dry"), "the error should list what's allowed: {e}");
    assert!(s.set("persona.tone", "warm").is_ok());
}

#[test]
fn a_list_is_edited_as_plain_comma_separated_text() {
    let mut s = s();
    s.set("identity.trusted_devices", "iPhone, iPad ,, ").unwrap();
    assert_eq!(
        s.get("identity.trusted_devices").unwrap().value,
        Value::List(vec!["iPhone".into(), "iPad".into()]),
        "blank entries dropped"
    );
}

#[test]
fn an_unknown_setting_is_an_error_not_a_silent_no_op() {
    let mut s = s();
    assert!(s.set("nonsense.key", "on").is_err());
    assert!(s.reset("nonsense.key").is_err());
}

// ================= the pages =================

#[test]
fn the_hub_routes_only_where_it_should() {
    // The root is the dashboard now — the thing you open on purpose. The
    // status readout moved to its own address rather than being removed.
    assert_eq!(route("/hub"), Some(Page::Dashboard));
    assert_eq!(route("/hub/status"), Some(Page::Status));
    assert_eq!(route("/hub/settings/"), Some(Page::Settings));
    assert_eq!(route("/hub/permissions"), Some(Page::Permissions));
    assert!(route("/hub/../../etc/passwd").is_none());
    assert!(route("/anything-else").is_none());
}

#[test]
fn text_from_outside_cannot_inject_markup_into_the_page() {
    // Window titles, note names and device names all end up on these pages.
    let nasty = "<script>alert(1)</script>";
    assert!(!esc(nasty).contains("<script"));
    let page = list_page_at(None, "Activity", "what happened", &[nasty.to_string()]);
    assert!(!page.contains("<script>alert"), "escaped in the rendered page");
    assert!(page.contains("&lt;script&gt;"));
}

#[test]
fn the_settings_page_renders_every_group_and_control() {
    let page = settings_page(&s());
    // Every category the registry actually uses, rather than four names
    // written down once. "Acting" held twenty of the forty-odd settings,
    // which is not a category — it is where things went when nobody decided.
    let settings = s();
    for group in settings.groups() {
        assert!(page.contains(&group), "missing group {group}");
        assert!(
            !settings.in_group(&group).is_empty(),
            "{group} is a heading with nothing under it"
        );
    }
    assert!(
        settings.groups().len() >= 5,
        "one or two headings over forty settings is a list with gaps in it"
    );
    for group in settings.groups() {
        let n = settings.in_group(&group).len();
        assert!(
            n <= 12,
            "{group} holds {n} settings — past a dozen you are scanning, not \
             choosing"
        );
    }
    assert!(page.contains("Turn on"), "toggles have a button");
    assert!(page.contains("<select"), "choices have a dropdown");
    assert!(page.contains("type=number"), "numbers have a number field");
}

#[test]
fn turning_on_something_consequential_asks_you_to_confirm() {
    let page = settings_page(&s());
    assert!(page.contains("onsubmit=\"return confirm("), "a camera toggle must confirm");
}

#[test]
fn the_page_works_with_no_internet_and_no_javascript_framework() {
    // Atlas works offline, so the hub has to.
    let page = settings_page(&s());
    assert!(!page.contains("http://") && !page.contains("https://"), "no external requests");
    assert!(!page.to_lowercase().contains("<script"), "no scripts at all");
    assert!(page.contains("<form method=post"), "plain forms");
}

#[test]
fn the_status_page_says_how_much_you_have_changed() {
    let page = status_page(&[("Listening".into(), "the webcam".into())], 3);
    assert!(page.contains("the webcam"));
    assert!(page.contains("3 settings changed"));
}

#[test]
fn every_page_carries_the_same_navigation() {
    for page in [
        settings_page(&s()),
        permissions_page(&s(), &[]),
        list_page_at(None, "Activity", "x", &[]),
        status_page(&[], 0),
    ] {
        assert!(page.contains("/hub/settings"));
        assert!(page.contains("/hub/permissions"));
        assert!(page.contains("<meta name=viewport"), "must work on a phone");
    }
}

// ================= form handling =================

#[test]
fn posted_form_values_are_decoded() {
    let body = "key=identity.trusted_devices&value=iPhone%2C+iPad";
    assert_eq!(form_field(body, "key").as_deref(), Some("identity.trusted_devices"));
    assert_eq!(form_field(body, "value").as_deref(), Some("iPhone, iPad"));
    assert!(form_field(body, "missing").is_none());
}

#[test]
fn a_malformed_percent_escape_does_not_panic() {
    assert_eq!(urldecode("a%zz"), "a%zz");
    assert_eq!(urldecode("trailing%"), "trailing%");
    assert_eq!(urldecode("plain+text"), "plain text");
}

#[test]
fn the_riskiest_new_capabilities_are_findable_and_explain_their_limits() {
    let s = s();
    let money = s.get("finance.enabled").expect("money must be togglable");
    assert!(money.cost.contains("cannot submit"), "the guarantee should be visible: {}", money.cost);
    let calls = s.get("call_notes.enabled").expect("call notes must be togglable");
    assert_eq!(calls.weight, Weight::Permission);
    // Eric, 24 Sep 2026: the others are asked and must say yes.
    assert!(calls.cost.contains("until you ask them and tell me they said yes"), "{}", calls.cost);
}

#[test]
fn a_number_you_never_touched_is_not_reported_as_changed() {
    // An f32 config value widened to f64 is 0.3499999940395355, not 0.35.
    // Comparing exactly makes the hub claim you changed things you didn't.
    //
    // 29 Sep 2026: this used `persona.wit`, the one f32 setting it had to
    // hand; wit became a choice (off / dry / full), so the f32 now checked is
    // the voice's speed, which has the same widening.
    let s = s();
    let speed = s.get("voice_settings.speed").unwrap();
    assert!(!speed.changed(), "a fresh config should have nothing changed");
    let mut s2 = s;
    s2.set("voice_settings.speed", "1.4").unwrap();
    assert!(s2.get("voice_settings.speed").unwrap().changed());
}

#[test]
fn the_settings_page_works_when_atlas_does_not() {
    // The whole point of a voice assistant is that you talk to it, which is
    // no help when the broken thing is the listening.
    use atlas::hub::{works_without_voice, Page};
    assert!(works_without_voice(Page::Settings));
    assert!(works_without_voice(Page::Permissions));
    assert!(works_without_voice(Page::Status));
    assert!(!works_without_voice(Page::Now), "the live view needs a running daemon");
}

#[test]
fn the_settings_page_tells_you_how_to_reach_it_without_speaking() {
    let page = settings_page(&s());
    // 28 Sep 2026: the way in when Atlas won't start is Atlas's own window
    // (Start menu → Settings), which needs no running Atlas. ATLAS.bat isn't
    // in what Eric is given (only atlas.exe is), so the page no longer
    // points at it.
    assert!(page.contains("Start menu"));
    assert!(!page.contains("ATLAS.bat"));
    assert!(page.contains("when the voice is down"));
}

#[test]
fn there_is_a_way_in_that_needs_no_voice_and_no_memory() {
    // The recovery path. It's option 3 in the one launcher now rather than a
    // separate file nobody could pick out of fourteen.
    let bat = std::fs::read_to_string("ATLAS.bat").unwrap();
    // Matched on the subcommand rather than on `atlas.exe settings`, because
    // the launcher now calls Atlas by full path (`"%EXE%" settings`). It used
    // to invoke a bare `atlas.exe` after walking the working directory up to
    // four levels looking for one — so which Atlas answered, and which
    // folder's settings you were editing, depended on where you happened to
    // be standing.
    assert!(bat.contains("settings"), "starts in settings-only mode");
    assert!(
        bat.contains("\"%EXE%\" settings"),
        "the launcher must call Atlas by full path, not rely on the working \
         directory to find it"
    );
    assert!(bat.contains("works even when Atlas won't"), "and says why it's there");
    assert!(bat.contains("Could not find atlas.exe"), "says so plainly when it cannot start");

    // And the files it downloads must land where Atlas will look for them:
    // `roots::install_root()` is the exe's folder or `ATLAS_HOME`, never the
    // working directory.
    assert!(
        bat.contains("ATLAS_HOME"),
        "the launcher ignores ATLAS_HOME, so with it set it downloads 400MB \
         into a folder Atlas never reads"
    );
}

#[test]
fn there_really_is_a_settings_subcommand() {
    // RUN-SETTINGS.bat had called `atlas.exe settings` since it was written
    // and no such subcommand existed, so it fell through to the parser and
    // said it didn't understand. The .bat was fine; the program wasn't.
    let main = crate::common::source_of("main");
    assert!(main.contains(r#"== Some("settings")"#), "no settings subcommand");
    assert!(main.contains("fn run_hub"), "and nothing serving the hub");
}

#[test]
fn settings_mode_starts_nothing_that_can_be_broken() {
    // It's the recovery path. If it needed the model or the voice it would be
    // down exactly when you need it.
    let main = crate::common::source_of("main");
    let hub = main.split("fn run_hub").nth(1).unwrap();
    for heavy in ["Voice::new", "ShellLlm", "Daemon::new", "Memory::"] {
        assert!(!hub.contains(heavy), "settings mode starts {heavy}");
    }
}

#[test]
fn the_hub_listens_where_you_said_and_nowhere_public() {
    // This used to assert the literal `("127.0.0.1", cfg.port)` bind, on the
    // reasoning that reaching it from another device was "the VPN's job".
    // That was half true: a VPN gives your phone a route to the machine, and
    // loopback still refuses it -- so the sync page's buttons, which are the
    // fix for sync being broken, were unreachable from the thing you are
    // most likely to be holding when you notice.
    //
    // The rule moved into `bind_address`, where it can be tested rather than
    // asserted about a string. What has not moved is the refusal.
    use atlas::server::bind_address;

    assert_eq!(bind_address("").unwrap().to_string(), "127.0.0.1", "empty is this machine only");
    assert_eq!(bind_address("  ").unwrap().to_string(), "127.0.0.1");

    for private in ["10.0.0.4", "192.168.1.20", "172.20.0.9", "100.101.102.103", "127.0.0.1"] {
        assert!(bind_address(private).is_ok(), "{private} is a private address");
    }

    let wide = bind_address("0.0.0.0").unwrap_err();
    assert!(wide.contains("every network"), "got: {wide}");
    assert!(bind_address("::").is_err());

    for public in ["8.8.8.8", "203.0.113.5", "172.32.0.1"] {
        let said = bind_address(public).unwrap_err();
        assert!(said.contains("public address"), "{public} should be refused: {said}");
    }

    let nonsense = bind_address("my-laptop").unwrap_err();
    assert!(nonsense.contains("isn't an address"), "got: {nonsense}");

    // And the listener asks that question rather than hardcoding an answer.
    // The hub's own listener, that is: the door other people's Atlases knock
    // on (`SignalListener`) listens everywhere on purpose since 25 Sep --
    // friends reach it over the internet -- and is held to a different rule,
    // checked below: from anywhere public it reads sealed envelopes only.
    let src = crate::common::source_of("server");
    assert!(src.contains("bind_address(&cfg.reachable_from)"));
    let door = src.find("pub struct SignalListener").expect("the peer door");
    let hub = &src[..door];
    assert!(
        !hub.contains(r#"bind(("0.0.0.0"#),
        "every network this machine is on is never the answer"
    );
    let peer_door = &src[door..];
    assert!(
        peer_door.contains("Some(req) if !sealed_only && crate::onion::is_local_origin(remote.ip()) => self.answer(&req, None)"),
        "the peer door listens everywhere, so a request in the clear must be refused from anywhere public -- and on the socket Tor feeds, always"
    );
}

// ================= the token =================
//
// An outside review found this: the token came from an xorshift sequence
// seeded with a nanosecond timestamp. It looked like 118 bits of entropy and
// wasn't — the whole sequence is determined by the seed, and process start
// time is knowable to within a narrow window.

#[test]
fn the_token_does_not_come_from_the_clock() {
    let src = crate::common::source_of("server");
    let f = src.split("pub fn new_token").nth(1).unwrap();
    let body: String = f.chars().take(900).collect();
    assert!(!body.contains("SystemTime"), "seeded from the clock again");
    assert!(!body.contains("<< 13"), "the xorshift is back");
    assert!(body.contains("os_random"), "not from the operating system");
}

#[test]
fn two_tokens_are_never_the_same() {
    let a = atlas::server::new_token().unwrap();
    let b = atlas::server::new_token().unwrap();
    assert_ne!(a, b);
    assert!(a.len() >= 24, "and long enough to matter: {}", a.len());
}

#[test]
fn it_stops_rather_than_falling_back_when_there_is_no_entropy() {
    // A token quietly generated from a timestamp because the source was
    // unavailable is worse than not starting — everything downstream assumes
    // it's strong and you'd never know.
    let src = crate::common::source_of("server");
    assert!(src.contains("fn new_token() -> Result<String>"), "it has to be able to fail");
    let main = crate::common::source_of("main");
    assert!(main.contains("couldn't make a secure token"));
    assert!(main.contains("process::exit"), "and actually stop");
}

#[test]
fn a_run_of_failed_attempts_earns_a_delay_that_widens() {
    use atlas::server::Failures;
    let mut f = Failures::default();
    assert_eq!(f.delay_ms(), 0);
    f.failed(0);
    assert!(f.delay_ms() > 0);
    for i in 1..8 {
        f.failed(i);
    }
    assert!(f.delay_ms() >= 500);
}

#[test]
fn the_delay_is_capped_so_it_cannot_hold_the_one_connection_open() {
    use atlas::server::Failures;
    let mut f = Failures::default();
    for i in 0..500 {
        f.failed(i);
    }
    assert!(f.delay_ms() <= 2000, "an unbounded delay is its own denial of service");
}

#[test]
fn a_gap_with_no_failures_forgets_them() {
    // Otherwise one fat-fingered attempt this morning slows you down all day.
    use atlas::server::Failures;
    let mut f = Failures::default();
    f.failed(0);
    f.failed(1);
    f.failed(1000);
    assert_eq!(f.delay_ms(), 100, "counted as the first again");
}

#[test]
fn a_success_clears_the_count_entirely() {
    use atlas::server::Failures;
    let mut f = Failures::default();
    f.failed(0);
    f.failed(1);
    f.succeeded();
    assert_eq!(f.delay_ms(), 0);
}

#[test]
fn a_run_of_failures_is_worth_telling_you_about_and_a_couple_is_not() {
    // The first few are you, or a stale tab. A run is something else on the
    // machine trying.
    use atlas::server::Failures;
    let mut f = Failures::default();
    f.failed(0);
    f.failed(1);
    assert!(f.worth_mentioning().is_none());
    for i in 2..7 {
        f.failed(i);
    }
    assert!(f.worth_mentioning().unwrap().contains("not you mistyping"));
}

#[test]
fn a_permission_that_is_off_can_be_turned_on_from_the_permissions_page() {
    // 29 Sep 2026: rows that were off showed "off" and no button, so the
    // page listed what Atlas couldn't do and gave no way to allow it.
    let s = s();
    let page = permissions_page(&s, &[]);
    let off = s
        .consequential()
        .into_iter()
        .find(|i| matches!(i.value, atlas::settings::Value::Toggle(false)))
        .expect("the shipped settings have something consequential switched off");
    let row = page.split("<div class=row>").find(|r| r.contains(&esc(&off.name))).expect("its row");
    assert!(row.contains("action=/hub/set"), "no way to change {}: {row}", off.name);
    assert!(row.contains("Turn on"), "{row}");
    // Turning it on still asks first.
    assert!(row.contains("onsubmit=\"return confirm("), "{row}");
}

#[test]
fn the_setting_badges_describe_rather_than_instruct() {
    // "changes permissions" read as "you need to change permissions".
    for w in [Weight::Preference, Weight::Resource, Weight::Sensitive, Weight::Permission] {
        let l = w.label();
        assert!(!l.contains("permission"), "{l}");
    }
    assert_eq!(Weight::Permission.label(), "lets Atlas act without asking");
}
