use atlas::retention::{discard_audio, RetentionConfig};
use atlas::tune::{actionable, examine, storage_plan, summary, worth_it, Fix, Survey, TuneConfig};

fn on() -> TuneConfig {
    TuneConfig { enabled: true, ..Default::default() }
}

fn laptop() -> Survey {
    Survey {
        memory_by_app: vec![
            ("Teams".into(), 900, false),
            ("Chrome".into(), 1800, true),
            ("Realtek Audio Console".into(), 400, false),
        ],
        startup_items: vec![
            ("Spotify".into(), 2.4, false),
            ("Intel Graphics Command Center".into(), 1.8, false),
            ("Discord".into(), 1.2, true),
        ],
        disposable: vec![("Windows Temp".into(), 3200), ("Chrome cache".into(), 150)],
        disk_free_gb: 12.0,
        disk_total_gb: 476.0,
        ram_used_gb: 13.9,
        ram_total_gb: 15.7,
        other_drives: vec![("D:".into(), 220.0), ("E:".into(), 58.0)],
        atlas_mb: 1400,
    }
}

// ================= recordings =================

#[test]
fn a_recording_is_deleted_the_moment_it_has_been_transcribed() {
    // The transcript is what you wanted. A folder of recordings of yourself
    // is a liability with no upside.
    let d = std::env::temp_dir().join("atlas-audio-test");
    let _ = std::fs::create_dir_all(&d);
    let wav = d.join("turn.wav");
    std::fs::write(&wav, b"fake audio").unwrap();

    assert!(discard_audio(&wav, &RetentionConfig::default()));
    assert!(!wav.exists(), "the audio should be gone within seconds, not hours");
}

#[test]
fn keeping_audio_is_something_you_have_to_turn_on_deliberately() {
    assert!(RetentionConfig::default().delete_audio_after_transcribing);
    let keep = RetentionConfig { delete_audio_after_transcribing: false, ..Default::default() };
    let d = std::env::temp_dir().join("atlas-audio-keep");
    let _ = std::fs::create_dir_all(&d);
    let wav = d.join("turn.wav");
    std::fs::write(&wav, b"x").unwrap();
    assert!(!discard_audio(&wav, &keep));
    assert!(wav.exists());
}

// ================= tuning the laptop =================

#[test]
fn memory_held_by_things_you_are_not_using_is_found() {
    let f = examine(&laptop(), &on());
    assert!(f.iter().any(|x| x.id == "mem:Teams"));
    assert!(!f.iter().any(|x| x.id == "mem:Chrome"), "you're using Chrome");
}

#[test]
fn drivers_and_system_components_are_never_suggested_for_removal() {
    // Suggesting someone disable their audio driver is how software breaks
    // machines.
    let f = examine(&laptop(), &on());
    assert!(!f.iter().any(|x| x.id.contains("Realtek")));
    assert!(!f.iter().any(|x| x.id.contains("Intel Graphics")));
}

#[test]
fn a_startup_program_you_never_open_is_flagged_with_what_it_costs() {
    let f = examine(&laptop(), &on());
    let spotify = f.iter().find(|x| x.id == "boot:Spotify").expect("should be flagged");
    assert!(spotify.what.contains("2.4s"), "the actual number, so you can disagree: {}", spotify.what);
    assert!(spotify.what.contains("haven't opened it this week"));
}

#[test]
fn turning_off_a_startup_item_is_reversible_and_treated_as_such() {
    let f = examine(&laptop(), &on());
    let spotify = f.iter().find(|x| x.id == "boot:Spotify").unwrap();
    assert!(matches!(spotify.fix, Fix::Reversible { .. }), "it goes back with one click");
}

#[test]
fn deleting_temporary_files_is_marked_as_one_way() {
    let f = examine(&laptop(), &on());
    let junk = f.iter().find(|x| x.id.contains("Windows Temp")).unwrap();
    assert!(matches!(junk.fix, Fix::OneWay { .. }));
}

#[test]
fn small_things_are_not_mentioned_at_all() {
    // A list of forty tiny wins is a list nobody reads.
    let f = examine(&laptop(), &on());
    assert!(!f.iter().any(|x| x.id.contains("Chrome cache")), "150 MB isn't worth a sentence");
    assert!(!f.iter().any(|x| x.id == "boot:Discord"), "you use Discord");
}

#[test]
fn the_biggest_win_is_said_first() {
    let f = examine(&laptop(), &on());
    assert!(f[0].what.contains("Windows Temp"), "3.2 GB leads: {}", f[0].what);
}

#[test]
fn atlas_holds_itself_to_the_same_standard() {
    let f = examine(&laptop(), &on());
    let own = f.iter().find(|x| x.id == "atlas").expect("its own footprint counts too");
    assert!(own.what.contains("1400 MB"));
}

#[test]
fn a_full_disk_gets_a_specific_way_out_not_free_up_space() {
    let f = examine(&laptop(), &on());
    let disk = f.iter().find(|x| x.id == "disk").unwrap();
    match &disk.fix {
        Fix::Yours { action } => {
            assert!(action.contains("D:"), "it names the drive with room: {action}");
            assert!(action.contains("220"), "and how much: {action}");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn atlas_only_offers_to_do_the_reversible_things_itself() {
    let f = examine(&laptop(), &on());
    for a in actionable(&f) {
        assert!(a.fix.atlas_can_do_it());
        assert!(!matches!(a.fix, Fix::Yours { .. }));
    }
}

#[test]
fn you_are_told_the_total_so_you_can_decide_if_it_is_worth_bothering() {
    let (mb, secs) = worth_it(&examine(&laptop(), &on()));
    assert!(mb > 4000, "several gigabytes on this machine");
    assert!(secs > 2.0);
    assert!(summary(&examine(&laptop(), &on())).contains("off startup"));
}

#[test]
fn a_tidy_machine_is_left_alone() {
    let clean = Survey {
        disk_free_gb: 300.0,
        disk_total_gb: 476.0,
        ram_used_gb: 5.0,
        ram_total_gb: 15.7,
        ..Default::default()
    };
    assert!(examine(&clean, &on()).is_empty());
    assert_eq!(summary(&[]), "Nothing worth changing.");
}

#[test]
fn it_does_nothing_unless_you_turn_it_on() {
    // On by default since 1 Oct 2026 (Eric: optimizing should do the work);
    // it only finds and explains, and acts on a yes. Off is still off.
    assert!(examine(&laptop(), &TuneConfig { enabled: false, ..TuneConfig::default() }).is_empty());
    assert!(TuneConfig::default().enabled);
}

// ================= where the big files go =================

/// An Atlas folder with real files in it: the plan measures sizes now,
/// rather than using invented ones (G5).
fn atlas_root() -> String {
    // One folder per call: tests running side by side used to rewrite the
    // same files under each other and measure a half-written tree (26 Sep).
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "atlas-tune-root-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    for (sub, mb) in [("models", 3usize), ("data/video", 2), ("data/state", 1), ("notes", 1)] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
        std::fs::write(d.join(sub).join("f.bin"), vec![0u8; mb * 1_000_000 + 10]).unwrap();
    }
    d.display().to_string()
}

#[test]
fn models_move_to_the_drive_with_the_most_room() {
    let plan = storage_plan(&laptop(), &atlas_root()).expect("you have two spare drives");
    assert!(plan.moves.iter().all(|(_, to, _)| to.starts_with("D:")), "D: has 220 GB");
    assert_eq!(plan.frees_mb, 5, "measured: 3 MB of models and 2 of video");
}

#[test]
fn only_things_that_are_large_static_and_re_downloadable_move() {
    let plan = storage_plan(&laptop(), &atlas_root()).unwrap();
    let moved: Vec<&String> = plan.moves.iter().map(|(from, _, _)| from).collect();
    assert!(moved.iter().any(|m| m.contains("models")));
    assert!(!moved.iter().any(|m| m.contains("state")), "what Atlas has learned stays put");
    assert!(!moved.iter().any(|m| m.contains("notes")));
    assert!(!moved.iter().any(|m| m.contains("backup")));
}

#[test]
fn the_reasoning_is_explained_rather_than_just_done() {
    let plan = storage_plan(&laptop(), &atlas_root()).unwrap();
    assert!(plan.note.contains("re-downloadable"));
    assert!(plan.note.contains("stay on C"), "and what doesn't move, and why");
}

#[test]
fn with_no_spare_drive_there_is_no_plan_rather_than_a_bad_one() {
    let single = Survey { other_drives: vec![("E:".into(), 0.5)], ..laptop() };
    assert!(storage_plan(&single, &atlas_root()).is_none());
}
