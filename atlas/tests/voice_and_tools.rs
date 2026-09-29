use atlas::tools::{expand, which, ExternalTool, Vars};
use atlas::voice::{clean_transcript, ToolsConfig, Voice};

/// A command run through whichever shell this machine has.
///
/// The stand-ins for ffmpeg/whisper/piper have to exist on both Linux and
/// Windows, and `cp`, `cat` and `tee` do not exist on Windows. Every shell
/// does, so everything goes through one.
fn sh(script: &str, stdin_text: bool, result_file: Option<&str>) -> ExternalTool {
    let (command, first) = if cfg!(windows) { ("cmd", "/c") } else { ("sh", "-c") };
    ExternalTool {
        command: command.into(),
        args: vec![first.into(), script.into()],
        stdin_text,
        result_file: result_file.map(str::to_string),
        timeout_secs: 120,
    }
}

/// A binary that exists on every machine, for the PATH lookup test.
fn a_real_binary() -> &'static str {
    if cfg!(windows) { "cmd" } else { "cat" }
}

fn copy(from: &str, to: &str) -> ExternalTool {
    // No quotes on Windows: `cmd /c` strips and re-interprets them in ways
    // that fight Rust's own argument escaping, and these temp paths contain
    // no spaces.
    let script = if cfg!(windows) {
        format!("copy /y {from} {to} >nul")
    } else {
        format!("cp '{from}' '{to}'")
    };
    sh(&script, false, None)
}

fn test_tools() -> ToolsConfig {
    let dir = std::env::temp_dir().join("atlas-voice-test");
    let _ = std::fs::create_dir_all(&dir);
    let fixture = dir.join("heard.txt");
    std::fs::write(&fixture, "boot workspace\n").unwrap();
    let f = fixture.display().to_string();
    let work = dir.display().to_string();

    let write_stdin_to_out =
        if cfg!(windows) { "more > {out_wav}" } else { "cat > '{out_wav}'" };
    let show_out = if cfg!(windows) { "type {out_wav}" } else { "cat '{out_wav}'" };

    ToolsConfig {
        enabled: true,
        work_dir: work,
        record_seconds: 3,
        record: copy(&f, "{in_wav}"),
        stt: {
            let mut t = copy("{in_wav}", "{transcript}");
            t.result_file = Some("{transcript}".into());
            t
        },
        tts: sh(write_stdin_to_out, true, None),
        play: sh(show_out, false, None),
        capture_screen: Some(copy(&f, "{out_png}")),
        // Everything else stays at its default, so adding a config field
        // later does not break this fixture.
        ..Default::default()
    }
}

fn vars(pairs: &[(&str, &str)]) -> Vars {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

// ---------- placeholder expansion ----------

#[test]
fn placeholders_are_substituted() {
    let v = vars(&[("mic", "Realtek Array"), ("seconds", "8")]);
    assert_eq!(expand("audio={mic}", &v), "audio=Realtek Array");
    assert_eq!(expand("{seconds}", &v), "8");
}

#[test]
fn unknown_placeholder_is_left_intact_not_blanked() {
    // Silently emptying a typo'd key is how you get ffmpeg calls with a
    // missing device and a useless error message.
    let v = vars(&[("known", "x")]);
    assert_eq!(expand("{known}/{typo}", &v), "x/{typo}");
}

#[test]
fn braces_that_are_not_placeholders_survive() {
    assert_eq!(expand("a{b", &vars(&[])), "a{b");
    assert_eq!(expand("{}", &vars(&[])), "{}");
}

// ---------- transcript cleaning ----------

#[test]
fn whisper_annotations_are_stripped() {
    // Without this, "[BLANK_AUDIO]" gets parsed as a command every silence.
    assert_eq!(clean_transcript("[BLANK_AUDIO]"), "");
    assert_eq!(clean_transcript(" [00:00.000] boot workspace \n"), "boot workspace");
    assert_eq!(clean_transcript("boot workspace (crosstalk)"), "boot workspace");
}

// ---------- real subprocess execution ----------

#[test]
fn tool_runs_and_returns_stdout() {
    let t = sh("echo {word}", false, None);
    let out = t.run(&vars(&[("word", "hello")]), None).unwrap();
    assert_eq!(out.trim(), "hello");
}

#[test]
fn tool_pipes_stdin_when_configured() {
    let t = sh(if cfg!(windows) { "more" } else { "cat" }, true, None);
    let out = t.run(&vars(&[]), Some("spoken text")).unwrap();
    assert_eq!(out.trim(), "spoken text");
}

#[test]
fn missing_binary_gives_an_actionable_error() {
    let t = ExternalTool {
        command: "definitely-not-installed-xyz".into(),
        args: vec![],
        stdin_text: false,
        result_file: None,
        timeout_secs: 120,
    };
    let e = t.run(&vars(&[]), None).unwrap_err().to_string();
    assert!(e.contains("Connections page") && !e.contains("`atlas"), "error should point somewhere useful, not to a terminal: {e}");
}

#[test]
fn nonzero_exit_is_an_error_not_silent_success() {
    let t = sh(if cfg!(windows) { "exit 1" } else { "exit 1" }, false, None);
    assert!(t.run(&vars(&[]), None).is_err());
}

#[test]
fn which_finds_a_real_binary_and_rejects_a_fake_one() {
    assert!(which(a_real_binary()).is_some());
    assert!(which("definitely-not-installed-xyz").is_none());
}

// ---------- the full voice turn ----------

#[test]
fn listen_records_transcribes_and_returns_text() {
    let cfg = test_tools();
    let heard = Voice::new(&cfg).listen().unwrap();
    assert_eq!(heard, "boot workspace");
}

#[test]
fn transcript_read_from_result_file_not_stdout() {
    // `cp` prints nothing; the text can only have come from result_file.
    let cfg = test_tools();
    assert!(cfg.stt.result_file.is_some());
    assert_eq!(Voice::new(&cfg).listen().unwrap(), "boot workspace");
}

#[test]
fn speak_synthesizes_and_plays() {
    let cfg = test_tools();
    Voice::new(&cfg).speak("workspace online").unwrap();
    let wav = std::fs::read_to_string(format!("{}/turn_out.wav", cfg.work_dir)).unwrap();
    assert_eq!(wav.trim(), "workspace online");
}

#[test]
fn capture_screen_writes_a_file_and_returns_its_path() {
    let cfg = test_tools();
    let path = Voice::new(&cfg).capture_screen().unwrap();
    assert!(std::path::Path::new(&path).exists(), "no file at {path}");
    assert!(path.ends_with(".png"));
}

#[test]
fn work_dir_is_created_on_demand() {
    let mut cfg = test_tools();
    cfg.work_dir = std::env::temp_dir().join("atlas-voice-fresh").display().to_string();
    let _ = std::fs::remove_dir_all(&cfg.work_dir);
    Voice::new(&cfg).listen().unwrap();
    assert!(std::path::Path::new(&cfg.work_dir).is_dir());
}

// ---------- shipped tools.yaml is well-formed ----------

#[test]
fn shipped_tools_yaml_parses_and_declares_every_stage() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: ToolsConfig = serde_yaml::from_str(&y).unwrap();
    assert!(t.capture_screen.is_some());
    assert!(t.tts.stdin_text, "the speech engine reads its text on stdin");
    assert!(t.stt.result_file.is_some(), "whisper writes a .txt file");
    // Every var referenced in a command must be defined or supplied at runtime.
    // Names the code supplies at run time rather than the config declaring.
    //
    // Kept in step with `Voice::vars` by hand, which is the weakness of this
    // list: it went stale the moment the voice settings were wired through as
    // variables, and the failure it produced named `{voice_id}` rather than
    // "this list is out of date". `tests/bug_sweep.rs` asks the same question
    // by reading `insert("...")` out of the source instead, so it cannot go
    // stale this way — worth folding this check into that one rather than
    // maintaining two lists of the same thing.
    let supplied = [
        "in_wav",
        "out_wav",
        "stem",
        "transcript",
        "seconds",
        "work_dir",
        "out_png",
        // The voice settings, wired through so they actually reach the engine.
        "voice_file",
        "voice_id",
        "speed",
        "variation",
        "sentence_gap",
        // The language settings, wired through so a multilingual model
        // actually hears other languages. Supplied at runtime by
        // `add_language_vars`; empty (and dropped) on the English-only
        // default.
        "task_opt",
        "lang_opt",
        "lang_val",
        // Your own words as whisper's --prompt (H11), from the vocabulary
        // Atlas keeps; empty until it has heard a word of yours twice.
        "hint_opt",
        "hint_val",
    ];
    for tool in [&t.record, &t.stt, &t.tts, &t.play, t.capture_screen.as_ref().unwrap()] {
        for a in &tool.args {
            let mut rest = a.as_str();
            while let Some(i) = rest.find('{') {
                let Some(j) = rest[i..].find('}') else { break };
                let key = &rest[i + 1..i + j];
                assert!(
                    t.vars.contains_key(key) || supplied.contains(&key),
                    "tools.yaml uses {{{key}}} but nothing defines it"
                );
                rest = &rest[i + j..];
            }
        }
    }
}

#[test]
fn capture_works_on_a_fresh_install_where_the_folder_does_not_exist_yet() {
    // This failed on Windows: `listen` created the working folder and capture
    // didn't, so the first thing you did after installing decided whether the
    // second thing worked.
    let mut cfg = test_tools();
    cfg.work_dir = std::env::temp_dir().join("atlas-capture-fresh").display().to_string();
    let _ = std::fs::remove_dir_all(&cfg.work_dir);

    let path = Voice::new(&cfg).capture_screen().unwrap();
    assert!(std::path::Path::new(&path).exists(), "no file at {path}");
    assert!(std::path::Path::new(&cfg.work_dir).is_dir());
}

// ---------- the wake word, switched off (27 Sep 2026) ----------

fn with_wake(enabled: bool, marker: &std::path::Path) -> ToolsConfig {
    let mut cfg = test_tools();
    // Recording leaves a mark, so a recording that happened can be seen.
    let fixture = std::env::temp_dir().join("atlas-voice-test").join("heard.txt");
    let script = if cfg!(windows) {
        format!("echo x > {} & copy /y {} {{in_wav}} >nul", marker.display(), fixture.display())
    } else {
        format!("touch '{}' && cp '{}' '{{in_wav}}'", marker.display(), fixture.display())
    };
    cfg.record = sh(&script, false, None);
    cfg.wake = Some(atlas::voice::WakeConfig { enabled, phrase: "boot".into(), clip_seconds: 1, detector: None });
    cfg
}

#[test]
fn a_wake_word_switched_off_is_not_listened_for() {
    // `wake: enabled: false` is what ships. `wake_once` never read it, so
    // every pass of the loop recorded a clip and ran speech-to-text on it.
    let marker = std::env::temp_dir().join("atlas-wake-off-marker");
    let _ = std::fs::remove_file(&marker);
    let cfg = with_wake(false, &marker);
    let started = std::time::Instant::now();
    assert!(!Voice::new(&cfg).wake_once().unwrap(), "a switched-off wake word was heard");
    assert!(!marker.exists(), "it recorded anyway");
    assert!(started.elapsed() < std::time::Duration::from_millis(100));

    // Switched on, the same setup does listen -- so the test above is
    // measuring the switch, not a broken fixture.
    let on_marker = std::env::temp_dir().join("atlas-wake-on-marker");
    let _ = std::fs::remove_file(&on_marker);
    let on = with_wake(true, &on_marker);
    assert!(Voice::new(&on).wake_once().unwrap(), "the phrase in the clip was not heard");
    assert!(on_marker.exists(), "switched on, nothing was recorded");
}

#[test]
fn with_the_wake_word_off_atlas_starts_at_push_to_talk_and_stays_there() {
    use atlas::input::{Tier, Tiers};
    let mut t = Tiers::default();
    t.set_wake(false);
    assert_eq!(t.tier, Tier::PushToTalk, "the wake-word tier with the wake word off");
    for _ in 0..20 {
        assert!(t.succeeded().is_none(), "it announced climbing to a tier that is switched off");
    }
    assert_eq!(t.tier, Tier::PushToTalk);
    // Demoted to typing and recovering, it climbs back to push-to-talk only.
    let mut d = Tiers::default();
    d.set_wake(false);
    for _ in 0..d.patience {
        d.failed();
    }
    assert_eq!(d.tier, Tier::Typed);
    for _ in 0..40 {
        d.succeeded();
    }
    assert_eq!(d.tier, Tier::PushToTalk);
    // Switched back on, the wake word is the tier again.
    t.set_wake(true);
    assert_eq!(t.tier, Tier::Voice);
    // Audio unavailable is left alone by the switch.
    let mut typed = Tiers::default();
    typed.audio_unavailable();
    typed.set_wake(false);
    assert_eq!(typed.tier, Tier::Typed);
}

#[test]
fn the_loop_sets_the_tier_from_the_wake_setting() {
    let src = std::fs::read_to_string("src/daemon.rs").unwrap();
    assert!(src.contains("self.tiers.set_wake(self.wake_on());"), "run no longer starts from the setting");
    assert!(src.contains("self.tiers.set_wake(on);"), "a changed setting no longer moves the tier");
}
