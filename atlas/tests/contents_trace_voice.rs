use atlas::contents::{boot, drift, what_to_open, Contents, Line, MAX_LINES};
use atlas::trace::{from_lines, to_line, Call, Trace, KEEP, STORES_NO_CONTENT};
use atlas::tts::{Engine, EngineConfig};

// ========================= the index pattern =========================

fn master() -> Contents {
    let mut c = Contents::new("");
    c.add(Line::folder("areas", "projects on the go — homelab, the rack, atlas"));
    c.add(Line::new("spain-trip", "flights booked, hotel undecided, going in May"));
    c.add(Line::new("homelab", "the home lab — server, backups, open work"));
    c
}

#[test]
fn boot_opens_the_index_and_nothing_else() {
    let b = boot(&master());
    assert_eq!(b.loaded, 1, "anything above one means notes are being loaded eagerly again");
    assert_eq!(b.known_of, 3);
    assert!(b.plain().contains("know about 3"));
}

#[test]
fn a_query_loads_the_one_note_that_holds_it_and_no_siblings() {
    let open = what_to_open(&master(), "when is the hotel booked for spain");
    assert_eq!(open, vec!["spain-trip"], "it opened more than it needed: {open:?}");
}

#[test]
fn a_subject_the_index_has_nothing_on_opens_nothing() {
    let open = what_to_open(&master(), "sourdough starter");
    assert!(
        open.is_empty(),
        "opening everything on the chance something matches is the problem, not the fix"
    );
}

#[test]
fn the_same_query_twice_opens_the_same_notes_in_the_same_order() {
    let m = master();
    assert_eq!(what_to_open(&m, "atlas homelab"), what_to_open(&m, "atlas homelab"));
}

#[test]
fn a_line_that_only_repeats_the_filename_is_no_use() {
    assert!(!Line::new("spain-trip", "spain trip").is_useful());
    assert!(!Line::new("spain-trip", "spain-trip.md").is_useful());
    assert!(Line::new("spain-trip", "flights booked, hotel undecided").is_useful());
}

#[test]
fn useless_lines_are_findable_so_an_index_can_be_improved() {
    let mut c = Contents::new("");
    c.add(Line::new("notes", "notes"));
    c.add(Line::new("spain-trip", "flights booked, hotel undecided"));
    let bad = c.useless_lines();
    assert_eq!(bad.len(), 1);
    assert_eq!(bad[0].name, "notes");
}

#[test]
fn an_index_too_long_to_read_says_it_needs_splitting() {
    let mut c = Contents::new("");
    for i in 0..=MAX_LINES {
        c.add(Line::new(&format!("note-{i:03}"), &format!("something about subject {i}")));
    }
    assert!(c.needs_splitting(), "past {MAX_LINES} it is a list you scroll, not a map you read");
}

#[test]
fn adding_the_same_name_twice_updates_rather_than_duplicates() {
    let mut c = Contents::new("");
    c.add(Line::new("a", "the old description of a"));
    c.add(Line::new("a", "the new description of a"));
    assert_eq!(c.lines.len(), 1);
    assert!(c.lines[0].says.contains("new"));
}

#[test]
fn a_note_on_disk_the_index_never_mentions_is_reported() {
    let d = drift(&master(), &["spain-trip".into(), "areas".into(), "homelab".into(), "secret".into()]);
    assert_eq!(d.unlisted, vec!["secret"]);
    assert!(!d.is_clean());
    assert!(d.plain().contains("can't find"));
}

#[test]
fn a_note_the_index_promises_and_cannot_deliver_is_reported() {
    let d = drift(&master(), &["spain-trip".into(), "areas".into()]);
    assert_eq!(d.missing, vec!["homelab"]);
}

#[test]
fn an_index_that_matches_its_folder_is_clean_and_says_so_plainly() {
    let d = drift(&master(), &["areas".into(), "homelab".into(), "spain-trip".into()]);
    assert!(d.is_clean());
    assert_eq!(d.plain(), "The index matches what's there.");
}

#[test]
fn drift_is_raised_as_a_problem_rather_than_tidied_away_quietly() {
    let d = drift(&master(), &["spain-trip".into()]);
    let n = atlas::nudge::drifted(&d).expect("a wrong index is a fault, not a detail");
    assert!(n.relief.is_some(), "and Atlas offers to fix it");
    assert!(atlas::nudge::drifted(&drift(&master(), &["areas".into(), "homelab".into(), "spain-trip".into()])).is_none());
}

// ========================= the flight recorder =========================

fn call(who: &str, at: u64) -> Call {
    Call::new(who, "qwen2.5-7b", at).finished(400, "a prompt", "a reply")
}

#[test]
fn the_words_of_a_prompt_are_never_written_to_disk() {
    let c = call("brief", 1);
    let line = to_line(&c);
    assert!(!line.contains("a prompt"), "the prompt text reached the log: {line}");
    assert!(!line.contains("a reply"), "the reply text reached the log: {line}");
    assert!(line.contains("prompt_chars"), "but its size is kept: {line}");
    assert!(STORES_NO_CONTENT.contains("never written to disk"));
}

#[test]
fn a_log_survives_a_truncated_last_line_after_a_crash() {
    let good = to_line(&call("brief", 1));
    let text = format!("{good}\n{good}\n{{\"at\":3,\"asked_by\":\"br");
    let t = from_lines(&text);
    assert_eq!(t.calls.len(), 2, "one bad line should not cost you the rest of the log");
}

#[test]
fn a_round_trip_through_the_file_keeps_everything_that_matters() {
    let c = call("council", 7);
    let back = from_lines(&to_line(&c));
    assert_eq!(back.calls[0], c);
}

#[test]
fn the_typical_time_is_a_median_so_one_stall_does_not_define_a_model() {
    let mut t = Trace::default();
    for ms in [300, 320, 340, 30_000] {
        t.record(Call::new("brief", "qwen2.5-7b", 0).finished(ms, "", ""));
    }
    let typical = t.typical_ms("qwen2.5-7b");
    assert!(typical < 1_000, "one thirty-second stall made a fast model look slow: {typical}ms");
}

#[test]
fn a_failed_call_does_not_count_towards_how_fast_a_model_is() {
    let mut t = Trace::default();
    t.record(Call::new("brief", "m", 0).finished(100, "", ""));
    t.record(Call::new("brief", "m", 1).finished(99_999, "", "").broke("timeout"));
    assert_eq!(t.typical_ms("m"), 100);
    assert_eq!(t.failure_rate("brief"), 0.5);
}

#[test]
fn who_is_actually_using_the_model_is_answerable() {
    let mut t = Trace::default();
    for i in 0..5 {
        t.record(call("brief", i));
    }
    t.record(call("council", 9));
    assert_eq!(t.busiest(), Some(("brief".to_string(), 5)));
}

#[test]
fn a_call_can_be_tied_to_the_correction_it_caused() {
    let mut t = Trace::default();
    t.record(call("draft", 1));
    assert!(t.blame("draft", "too long"));
    assert_eq!(t.caused_corrections().len(), 1);
    assert!(!t.blame("nobody", "x"), "blaming a module that never called is a lie in the log");
}

#[test]
fn an_outcome_attaches_to_the_most_recent_call_from_that_module() {
    let mut t = Trace::default();
    t.record(call("brief", 1));
    t.record(call("brief", 2));
    assert!(t.grade_last("brief", false));
    assert_eq!(t.calls[1].graded, Some(false));
    assert_eq!(t.calls[0].graded, None);
}

#[test]
fn the_log_stops_growing_and_says_how_much_it_dropped() {
    let mut t = Trace::default();
    for i in 0..(KEEP + 10) {
        t.record(call("brief", i as u64));
    }
    assert_eq!(t.calls.len(), KEEP);
    assert_eq!(t.dropped, 10);
    assert!(t.spoken().contains("rolled off"));
}

#[test]
fn an_empty_trace_says_so_rather_than_reporting_zeroes() {
    let t = Trace::default();
    assert_eq!(t.spoken(), "I haven't asked the model anything yet.");
    assert_eq!(t.failure_rate("anyone"), 0.0);
    assert_eq!(t.typical_ms("anything"), 0);
}

// ========================= the pluggable voice =========================

#[test]
fn piper_remains_the_default_because_it_runs_on_anything() {
    assert_eq!(Engine::default(), Engine::Piper);
    // Defaulting to the better engine and failing is worse than defaulting to
    // the plainer one and working.
    assert_eq!(EngineConfig::default().engine, Engine::Piper);
}

#[test]
fn speed_is_translated_rather_than_passed_through() {
    // Updated 28 Sep 2026: the setting is a pace (larger is slower -- what
    // "a bit slower" does to it, and what Settings says), which is what
    // piper's length scale already means, so piper takes it as it is and the
    // multiplier engines take its reciprocal. This test used to call 2.0
    // "faster", while `adjust` and tests/tts.rs treated a larger number as
    // slower; the old conversion made "slower" speed piper up.
    assert!(!Engine::Piper.speed_is_inverted());
    assert!(Engine::Kokoro.speed_is_inverted());
    let faster = 0.8; // a quicker pace than normal
    assert_eq!(Engine::Piper.speed_value(faster), 0.8, "piper's length scale is a pace already");
    assert!(Engine::Kokoro.speed_value(faster) > 1.0, "Kokoro wants a larger multiplier to go faster");
    let slower = atlas::tts::adjust(&atlas::tts::VoiceSettings::default(), "slower").0.speed;
    assert!(Engine::Piper.speed_value(slower) > 1.0, "\"slower\" must reach piper as a longer length scale");
    assert!(Engine::Kokoro.speed_value(slower) < 1.0, "\"slower\" must reach Kokoro as a smaller multiplier");
}

#[test]
fn each_engine_looks_for_its_own_kind_of_voice_file() {
    let v = atlas::tts::catalogue().into_iter().next().unwrap();
    let mut cfg = EngineConfig::default();
    assert!(cfg.voice_file(&v).ends_with(".onnx"));
    cfg.engine = Engine::Kokoro;
    assert!(cfg.voice_file(&v).ends_with(".pt"));
}

#[test]
fn an_engine_pointed_at_the_wrong_executable_is_caught_before_it_goes_silent() {
    let mut cfg = EngineConfig::default();
    assert!(cfg.is_consistent());
    cfg.engine = Engine::Chatterbox;
    assert!(!cfg.is_consistent(), "chatterbox pointed at piper.exe would just never speak");
    cfg.exe = "tools/voicebox/voicebox.exe".into();
    assert!(cfg.is_consistent());
}

#[test]
fn only_the_engine_that_can_clone_says_it_can() {
    assert!(Engine::Chatterbox.can_clone());
    assert!(!Engine::Piper.can_clone());
    assert!(!Engine::Kokoro.can_clone());
}
