use atlas::clipboard::{classify, prompt, refers_to_clipboard, take, ClipboardConfig, Kind};
use atlas::config::Config;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::rehearse::{from_actions, preamble};
use std::path::Path;

fn cfg() -> ClipboardConfig {
    ClipboardConfig::default()
}

// ================= the clipboard =================

#[test]
fn what_you_copied_is_recognised_by_its_shape() {
    assert_eq!(classify("https://example.com/thing"), Kind::Url);
    assert_eq!(classify("C:\\Users\\me\\notes.md"), Kind::Path);
    assert_eq!(classify("thread 'main' panicked at src/lib.rs:12"), Kind::Error);
    assert_eq!(classify("fn main() {\n  let x = 1;\n}"), Kind::Code);
    assert_eq!(classify("Just a sentence about something."), Kind::Prose);
    assert_eq!(classify("   "), Kind::Empty);
}

#[test]
fn a_stack_trace_reads_as_an_error_rather_than_as_code() {
    // Both descriptions fit; the error reading is the useful one.
    let trace = "error[E0308]: mismatched types\n  --> src/main.rs:4:9\n   fn thing() {";
    assert_eq!(classify(trace), Kind::Error);
}

#[test]
fn a_pasted_table_is_recognised() {
    let t = "Date\tAmount\tPayee\n2026-08-01\t-4.50\tCoffee\n2026-08-02\t-12.00\tLunch";
    assert_eq!(classify(t), Kind::Table);
}

#[test]
fn atlas_knows_what_to_do_with_each_kind_without_being_told() {
    assert_eq!(Kind::Error.default_action(), "work out what that error means");
    assert_eq!(Kind::Url.default_action(), "read that page");
    assert!(Kind::Empty.default_action().contains("empty"));
}

#[test]
fn what_you_say_beats_the_guess() {
    let g = take("https://example.com", &cfg());
    let p = prompt("check if this is still up", &g);
    assert!(p.starts_with("check if this is still up"));
    assert!(p.contains("https://example.com"));
}

#[test]
fn with_no_instruction_the_guess_is_used() {
    let g = take("error: something broke", &cfg());
    assert!(prompt("", &g).starts_with("work out what that error means"));
}

#[test]
fn something_enormous_is_truncated_and_says_so() {
    let huge = "word ".repeat(20_000);
    let g = take(&huge, &cfg());
    assert!(g.truncated);
    assert!(g.text.chars().count() <= cfg().max_chars);
    assert!(g.describe().contains("first"), "got: {}", g.describe());
}

#[test]
fn atlas_tells_you_what_it_picked_up_so_you_know_it_got_the_right_thing() {
    let g = take("fn main() { let x = 1; }", &cfg());
    assert!(g.describe().contains("some code"));
    assert!(take("", &cfg()).describe().contains("nothing on the clipboard"));
}

#[test]
fn atlas_never_watches_the_clipboard_in_the_background() {
    // A clipboard monitor would see every password you copy. Asking about
    // the clipboard reads it once, when you ask.
    assert!(cfg().only_on_request);
    // Round 11's clipboard history is the one reader that runs on its own,
    // and only if you turn it on: it ships off, in the code and in the file.
    assert!(!atlas::cliphist::HistoryConfig::default().enabled);
    let tools = std::fs::read_to_string("config/tools.yaml").unwrap();
    let block = tools.split("clipboard_history:").nth(1).expect("the history's settings are in the shipped file");
    assert!(block.lines().nth(1).map(|l| l.trim() == "enabled: false").unwrap_or(false), "clipboard history must ship off");
    // And what it keeps is never written down: nothing saves the history.
    let wiring = std::fs::read_to_string("src/workday.rs").unwrap();
    assert!(!wiring.lines().any(|l| l.contains("save(") && l.contains("clips")), "clipboard history must stay in memory");
}

#[test]
fn phrases_that_mean_use_what_i_copied_are_recognised() {
    assert!(refers_to_clipboard("explain this"));
    assert!(refers_to_clipboard("what does that mean"));
    assert!(refers_to_clipboard("summarise what i just copied"));
    assert!(!refers_to_clipboard("open chrome"));
}

// ================= rehearsal =================

fn plat() -> MockPlatform {
    MockPlatform::new(vec![
        Monitor { id: 1, x: 0, y: 0, width: 2560, height: 1392, primary: true },
        Monitor { id: 2, x: 2560, y: 0, width: 2560, height: 1392, primary: false },
    ])
}

#[test]
fn a_rehearsal_shows_every_step_without_touching_anything() {
    // It runs against the same fake operating system the test suite uses, so
    // there is no code path from a rehearsal to your real windows.
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = plat();
    atlas::workspace::workspace_on(&cfg, &p).unwrap();

    let r = from_actions("boot workspace", &p.actions());
    assert!(r.beats.len() >= 4, "got {}", r.beats.len());
    assert!(r.beats.iter().any(|b| b.what.starts_with("open")));
    assert!(r.beats.iter().any(|b| b.what.starts_with("move")));
}

#[test]
fn waiting_is_not_shown_as_a_step() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = plat();
    atlas::workspace::workspace_on(&cfg, &p).unwrap();
    let r = from_actions("boot workspace", &p.actions());
    assert!(!r.beats.iter().any(|b| b.what.contains("sleep")), "nobody wants to read a wait");
}

#[test]
fn irreversible_steps_are_marked_as_such() {
    use atlas::platform::mock::Action;
    use atlas::platform::Button;
    let actions = vec![
        Action::Launch("chrome.exe".into()),
        Action::Close("notepad.exe".into()),
        Action::Type("some text".into()),
        Action::Click(10, 20, Button::Left),
    ];
    let r = from_actions("tidy up", &actions);
    assert!(r.touches_anything_irreversible());
    assert!(!r.beats[0].consequential, "opening something is not");
    assert!(r.beats[1].consequential, "closing can lose unsaved work");
    assert!(r.beats[2].consequential, "typing into a window is");
}

#[test]
fn a_command_that_would_do_nothing_says_so() {
    let r = from_actions("something harmless", &[]);
    assert_eq!(r.summary(), "Nothing would happen.");
}

#[test]
fn the_walk_through_reads_as_a_list_of_plain_steps() {
    use atlas::platform::mock::Action;
    let r = from_actions("boot workspace", &[
        Action::Launch("chrome.exe".into()),
        Action::Close("discord.exe".into()),
    ]);
    let d = r.detail();
    assert!(d.contains("If you said \"boot workspace\""));
    assert!(d.contains("open chrome"), "app names are tidied: {d}");
    assert!(d.contains("!2. close discord"), "irreversible steps are marked: {d}");
}

#[test]
fn a_rehearsal_is_never_mistaken_for_the_real_thing() {
    assert!(preamble("boot workspace").contains("nothing here actually happens"));
}

#[test]
fn long_typed_text_is_shortened_in_the_walk_through() {
    use atlas::platform::mock::Action;
    let r = from_actions("write", &[Action::Type("x".repeat(200))]);
    assert!(r.beats[0].what.chars().count() < 60, "got: {}", r.beats[0].what);
}
