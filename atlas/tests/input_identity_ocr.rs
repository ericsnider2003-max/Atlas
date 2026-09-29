use atlas::identity::{explain, grace_remaining, From, Gate, Hello, Identity, IdentityConfig, Proof};
use atlas::ocr::{args, parse_tsv, tidy, OcrConfig};
use atlas::quickinput::{parse_hotkey, Action, QuickInput, QuickInputConfig, State, Surface};

// ================= somewhere to type =================

fn qi() -> QuickInput {
    QuickInput::default()
}

#[test]
fn one_key_summons_a_box_over_whatever_you_are_doing() {
    let mut q = qi();
    assert_eq!(q.hotkey(Some("chrome".into()), 100), Action::Show);
    assert!(q.is_open());
    assert_eq!(q.previous_focus.as_deref(), Some("chrome"), "so you land back where you were");
}

#[test]
fn the_same_key_dismisses_it() {
    let mut q = qi();
    q.hotkey(None, 100);
    assert_eq!(q.hotkey(None, 110), Action::Hide);
    assert!(!q.is_open());
}

#[test]
fn typing_and_pressing_enter_sends_the_command() {
    let mut q = qi();
    q.hotkey(None, 100);
    for c in "boot workspace".chars() {
        q.typed(c, 100);
    }
    assert_eq!(q.submit(), Action::Submit("boot workspace".into()));
    assert_eq!(q.state, State::Hidden, "and it gets out of the way");
}

#[test]
fn enter_on_an_empty_box_means_go_away_not_run_nothing() {
    let mut q = qi();
    q.hotkey(None, 100);
    assert_eq!(q.submit(), Action::Hide);
}

#[test]
fn escape_closes_without_running_anything() {
    let mut q = qi();
    q.hotkey(None, 100);
    q.typed('x', 100);
    assert_eq!(q.escape(), Action::Hide);
}

#[test]
fn a_box_you_wandered_away_from_closes_itself() {
    let mut q = qi();
    q.hotkey(None, 0);
    assert_eq!(q.tick(10), Action::Nothing);
    assert_eq!(q.tick(100), Action::Hide);
}

#[test]
fn a_half_typed_thought_is_never_thrown_away_by_a_timeout() {
    // Losing what you were typing is worse than leaving a box open.
    let mut q = qi();
    q.hotkey(None, 0);
    q.typed('h', 0);
    assert_eq!(q.tick(100_000), Action::Nothing);
    assert!(q.is_open());
}

#[test]
fn backspace_works_and_does_not_underflow_on_an_empty_box() {
    let mut q = qi();
    q.hotkey(None, 0);
    q.backspace(0);
    q.typed('a', 0);
    q.backspace(0);
    assert_eq!(q.submit(), Action::Hide);
}

#[test]
fn the_hotkey_cannot_fire_while_typing_and_needs_no_alt() {
    assert!(parse_hotkey("a").is_none(), "a bare letter would fire constantly");
    assert!(parse_hotkey("tab").is_none());
    assert!(parse_hotkey("space").is_none());
    // Eric has no Alt key (26 Sep 2026): Ctrl, Shift or Win alone are enough,
    // and a key nobody types with works on its own.
    assert_eq!(parse_hotkey("ctrl+space"), Some((2, 0x20)));
    assert_eq!(parse_hotkey("win+j"), Some((8, 'J' as u32)));
    assert_eq!(parse_hotkey("f9"), Some((0, 0x78)));
    assert_eq!(parse_hotkey("insert"), Some((0, 0x2D)));
    assert!(parse_hotkey("rightctrl").is_none(), "a modifier on its own can't be registered");
    assert_eq!(parse_hotkey("ctrl+alt+a"), Some((2 | 1, 'A' as u32)));
    assert_eq!(parse_hotkey("ctrl+shift+space"), Some((2 | 4, 0x20)));
    assert_eq!(parse_hotkey("alt+f12"), Some((1, 0x7B)));
    assert!(parse_hotkey("ctrl+f99").is_none());
}

#[test]
fn the_default_hotkey_is_not_a_key_you_press_while_writing() {
    let cfg = QuickInputConfig::default();
    assert_eq!(cfg.hotkey, "ctrl+shift+space", "no Alt in the default: Eric's keyboard has none");
    assert!(parse_hotkey(&cfg.hotkey).is_some());
    assert_eq!(cfg.surface, Surface::Console);
}

// ================= proving it's you, rarely =================

fn on(kinds: &[&str]) -> IdentityConfig {
    IdentityConfig {
        enabled: true,
        verify_for: kinds.iter().map(|s| s.to_string()).collect(),
        trusted_devices: vec!["iPhone".into()],
        ..Default::default()
    }
}

#[test]
fn almost_nothing_ever_asks_you_to_prove_anything() {
    let id = Identity::default();
    let cfg = on(&["workspace_off"]);
    for kind in ["open_app", "research", "draft_post", "view_display", "workspace_on"] {
        let g = id.gate(kind, &From::Here, Hello::Available, &cfg, 100);
        assert!(!g.interrupts(), "{kind} must never prompt");
    }
}

#[test]
fn proving_once_covers_the_rest_of_the_working_day() {
    // The sudo model. Being asked every time is how a security feature gets
    // switched off.
    let mut id = Identity::default();
    let cfg = on(&["workspace_off"]);
    assert!(matches!(id.gate("workspace_off", &From::Here, Hello::Available, &cfg, 0), Gate::AskHello(_)));

    id.record(Proof::Verified, 0);
    let later = 3 * 3600;
    assert!(!id.gate("workspace_off", &From::Here, Hello::Available, &cfg, later).interrupts());
    assert!(grace_remaining(&id, &cfg, later).is_some());

    let tomorrow = 20 * 3600;
    assert!(id.gate("workspace_off", &From::Here, Hello::Available, &cfg, tomorrow).interrupts());
}

#[test]
fn your_phone_already_proved_it_so_atlas_does_not_ask_twice() {
    // It unlocked with your face before Atlas heard the request.
    let id = Identity::default();
    let cfg = on(&["workspace_off"]);
    let g = id.gate("workspace_off", &From::TrustedDevice("iPhone".into()), Hello::Available, &cfg, 0);
    assert!(!g.interrupts());
    assert!(g.reason().contains("trust"));
}

#[test]
fn an_unenrolled_device_still_has_to_confirm_but_not_with_a_pin() {
    // A Hello prompt would appear on a screen nobody is looking at.
    let id = Identity::default();
    let cfg = on(&["workspace_off"]);
    let g = id.gate("workspace_off", &From::UnknownDevice, Hello::Available, &cfg, 0);
    assert!(matches!(g, Gate::AskAloud(_)), "got {g:?}");
    assert!(g.reason().contains("not at the machine"));
}

#[test]
fn no_windows_hello_means_a_spoken_yes_not_a_lockout() {
    let id = Identity::default();
    let cfg = on(&["workspace_off"]);
    for h in [Hello::NotSetUp, Hello::Unavailable, Hello::Unknown] {
        let g = id.gate("workspace_off", &From::Here, h, &cfg, 0);
        assert!(matches!(g, Gate::AskAloud(_)), "{h:?} should degrade to a spoken yes");
    }
}

#[test]
fn unavailable_never_silently_means_yes() {
    let id = Identity::default();
    let cfg = on(&["workspace_off"]);
    let g = id.gate("workspace_off", &From::Here, Hello::Unavailable, &cfg, 0);
    assert!(g.interrupts(), "it still asks — just differently");
}

#[test]
fn a_spoken_yes_confirms_the_action_but_does_not_extend_the_grace_window() {
    let mut id = Identity::default();
    let cfg = on(&["workspace_off"]);
    assert!(!id.record(Proof::CouldNotAsk, 100));
    assert!(!id.within_grace(&cfg, 100), "only a real verification counts as proof");
}

#[test]
fn walking_away_voids_the_proof() {
    let mut id = Identity::default();
    let cfg = on(&["workspace_off"]);
    id.record(Proof::Verified, 0);
    id.forget();
    assert!(id.gate("workspace_off", &From::Here, Hello::Available, &cfg, 10).interrupts());
}

#[test]
fn with_the_feature_off_nothing_ever_asks() {
    let id = Identity::default();
    let cfg = IdentityConfig::default();
    assert!(!cfg.enabled, "off by default");
    assert!(!id.gate("workspace_off", &From::Here, Hello::Available, &cfg, 0).interrupts());
}

#[test]
fn atlas_can_tell_you_exactly_when_it_will_ask() {
    let said = explain(&on(&["workspace_off"]));
    assert!(said.contains("workspace_off") && said.contains("4 hours"), "got: {said}");
    assert!(said.contains("iPhone"), "and which devices never ask: {said}");
    assert!(explain(&IdentityConfig::default()).contains("never ask"));
}

// ================= reading text =================

const TSV: &str = "level\tpage\tblock\tpar\tline\tword\tleft\ttop\twidth\theight\tconf\ttext
5\t1\t1\t1\t1\t1\t10\t10\t50\t20\t96.5\tInvoice
5\t1\t1\t1\t1\t2\t70\t10\t40\t20\t94.2\tnumber
5\t1\t1\t1\t2\t1\t10\t40\t60\t20\t91.0\t2026-104
5\t1\t1\t1\t2\t2\t80\t40\t30\t20\t12.0\trn
4\t1\t1\t1\t2\t0\t0\t0\t0\t0\t-1\t";

#[test]
fn text_is_read_back_with_its_lines_intact() {
    let r = parse_tsv(TSV, 0.55);
    assert_eq!(r.text, "Invoice number\n2026-104");
    assert_eq!(r.words, 3);
}

#[test]
fn words_the_engine_was_unsure_about_are_dropped() {
    // OCR fails by producing plausible nonsense, not by erroring. "rn" at 12%
    // confidence is a smudge, not a word.
    let r = parse_tsv(TSV, 0.55);
    assert_eq!(r.discarded, 1);
    assert!(!r.text.contains("rn"));
}

#[test]
fn a_bad_scan_is_reported_as_unreadable_rather_than_handed_on_as_text() {
    let poor = "level\tpage\tblock\tpar\tline\tword\tl\tt\tw\th\tconf\ttext
5\t1\t1\t1\t1\t1\t0\t0\t0\t0\t58.0\tsomething
5\t1\t1\t1\t1\t2\t0\t0\t0\t0\t60.0\tblurry";
    let r = parse_tsv(poor, 0.55);
    assert!(!r.trustworthy());
    assert!(r.summary().contains("half-read"), "got: {}", r.summary());
    assert!(r.summary().contains("screenshot instead"), "and says what to do");
}

#[test]
fn nothing_readable_says_so_plainly() {
    let r = parse_tsv("level\tpage\tblock\tpar\tline\tword\tl\tt\tw\th\tconf\ttext", 0.55);
    assert_eq!(r.words, 0);
    assert!(r.summary().contains("couldn't read any text"));
}

#[test]
fn a_good_scan_is_trusted() {
    let r = parse_tsv(TSV, 0.55);
    assert!(r.trustworthy());
    assert!(r.summary().contains("Read 3 words"));
}

#[test]
fn edge_artefacts_are_tidied_away() {
    let messy = "| \nInvoice number\n~\n2026-104\n_";
    assert_eq!(tidy(messy), "Invoice number\n2026-104");
}

#[test]
fn the_engine_is_asked_for_confidence_data_not_just_words() {
    let a = args("page.png", "eng");
    assert!(a.contains(&"tsv".to_string()), "confidence is the whole point");
    assert!(a.contains(&"-".to_string()), "straight to stdout, no temp file");
}

#[test]
fn ocr_is_off_until_an_engine_is_installed() {
    let cfg = OcrConfig::default();
    assert!(!cfg.enabled);
    assert!(cfg.engine.is_none());
    assert!(cfg.min_confidence > 0.0 && cfg.min_confidence < 1.0);
}

#[test]
fn a_proof_at_time_zero_is_still_a_proof() {
    // Using 0 as "never proved" makes a genuine proof at time zero vanish.
    let mut id = Identity::default();
    let cfg = on(&["workspace_off"]);
    assert!(id.proved_at.is_none(), "never is not the same as zero");
    id.record(Proof::Verified, 0);
    assert!(id.within_grace(&cfg, 0));
    assert!(!id.gate("workspace_off", &From::Here, Hello::Available, &cfg, 60).interrupts());
}

#[test]
fn the_shipped_config_is_cautious_about_all_three() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();
    assert!(t.quick_input.enabled, "typing must always be reachable");
    assert!(parse_hotkey(&t.quick_input.hotkey).is_some(), "hotkey needs a modifier");
    assert!(!t.identity.enabled, "identity checks are opt-in");
    assert!(t.identity.fall_back_to_spoken, "no Hello must never mean locked out");
    assert!(!t.ocr.enabled, "OCR waits until an engine is installed");
}
