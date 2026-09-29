//! Twelve settings that could be changed and changed nothing.
//!
//! `tests/dead_config.rs` has counted these for days: fields of a `*Config`
//! struct, in a module that runs, that no line of `src/` reads. A dead
//! function sits there quietly; a dead setting actively tells the person
//! holding the file that Atlas has a behaviour it does not have.
//!
//! Four of these twelve were worse than inert — the shipped default said one
//! thing and the code did the other:
//!
//! - `opsec.always_strip_metadata: true` while `publishing::export_args`
//!   emitted no `-map_metadata`, so an export carried the original's GPS and
//!   camera serial. `opsec::Risk::Metadata::fix` told you it was stripped
//!   "by default".
//! - `capture.never_ask_on_capture: true` — "ask nothing at capture time,
//!   this is the whole point" — and the capture path asked anyway.
//! - `retention.logs_mb: 4` while the log rotated at 2MB.
//! - `viewing.longest_minutes: 90` while a three-hour file was scanned and
//!   read in full.
//!
//! The rest were the quieter shape: a literal beside the config that happened
//! to equal it. Those are the hardest to find, because nothing is wrong until
//! somebody edits the file — and then nothing happens and there is no error
//! to follow.
//!
//! Each behaviour below is tested by running it, not by reading the source,
//! wherever the code allows. Where the call site is inside `Daemon::new` and
//! cannot be observed from outside, the source check says so and says why.
//!
//! Every assertion here was mutation-checked on 18 Sep 2026 — the wiring
//! reverted, the test confirmed to fail. Two of them only fail *because* of
//! what that check found: the capture case originally used a sentence Atlas
//! was never going to ask about, so it passed with the wiring torn out, and
//! the `sync-setup` case originally hung instead of failing, because an
//! unread provider falls through to the hub and the hub does not return.

mod common; // `common::source_of`: a module's source wherever its files live

use atlas::config::Config;
use std::path::Path;

fn tools() -> atlas::voice::ToolsConfig {
    Config::load(Path::new("config")).expect("config/ loads").tools.expect("tools.yaml")
}

fn live_source(path: &str) -> String {
    crate::common::read_source_path(path)
        .unwrap_or_else(|| panic!("{path}"))
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------- viewing.longest_minutes ----------

#[test]
fn a_video_longer_than_you_allow_is_refused_rather_than_half_watched() {
    use atlas::viewing::{too_long, ViewConfig};
    let cfg = ViewConfig { longest_minutes: 90, ..ViewConfig::default() };

    assert!(too_long(60.0 * 60.0, &cfg).is_none(), "an hour is inside ninety minutes");
    let said = too_long(3.0 * 3600.0, &cfg).expect("three hours is not");
    assert!(said.contains("180 minutes"), "say how long it actually is: {said}");
    assert!(said.contains("longest_minutes"), "and which setting to change: {said}");

    // The setting is what decides it, not the number that happens to ship.
    let patient = ViewConfig { longest_minutes: 240, ..ViewConfig::default() };
    assert!(too_long(3.0 * 3600.0, &patient).is_none(), "four hours allowed means four hours");
    let strict = ViewConfig { longest_minutes: 5, ..ViewConfig::default() };
    assert!(too_long(10.0 * 60.0, &strict).is_some());

    // Zero has to keep meaning "no limit" — a cap of nothing would refuse
    // every video, which is not what an empty setting says.
    let no_limit = ViewConfig { longest_minutes: 0, ..ViewConfig::default() };
    assert!(too_long(10.0 * 3600.0, &no_limit).is_none());
    // ffmpeg not saying the duration reads as 0.0. Refusing on that would
    // make an unreadable header look like a three-hour film.
    assert!(too_long(0.0, &cfg).is_none());
}

#[test]
fn the_watcher_asks_before_it_starts() {
    let d = live_source("src/daemon.rs");
    assert!(
        d.contains("viewing::too_long(duration, &view)"),
        "`watch` is back to scanning whatever it is handed"
    );
}

// ---------- reference.shelves ----------

#[test]
fn the_shelves_you_name_are_the_shelves_it_uses() {
    use atlas::reference::{chosen, worth_having, ReferenceConfig};
    let all = worth_having();
    assert!(all.len() >= 2, "this test needs more than one shelf to tell a subset from the lot");
    let one = all[0].name.clone();

    let off = ReferenceConfig { enabled: false, ..ReferenceConfig::default() };
    assert!(chosen(&off).is_empty(), "disabled still means no shelf");

    let empty = ReferenceConfig { enabled: true, ..ReferenceConfig::default() };
    assert_eq!(
        chosen(&empty).len(),
        atlas::reference::for_trading().len(),
        "`[]` is what ships, and it must keep meaning the trading set"
    );

    let named = ReferenceConfig {
        enabled: true,
        shelves: vec![one.to_uppercase()],
        ..ReferenceConfig::default()
    };
    let got = chosen(&named);
    assert_eq!(got.len(), 1, "one named shelf is one shelf, not all of them");
    assert_eq!(got[0].name, one, "and matching is not case-sensitive");

    let nonsense =
        ReferenceConfig { enabled: true, shelves: vec!["nothing like this".into()], ..named };
    assert!(chosen(&nonsense).is_empty(), "a name that matches nothing conjures nothing");
}

// ---------- presence.discreet_with_strangers ----------

#[test]
fn someone_else_in_the_room_turns_a_note_into_a_knock() {
    use atlas::presence::{keep_it_to_yourself, Presence, PresenceConfig};
    let on = PresenceConfig { enabled: true, discreet_with_strangers: true, ..Default::default() };

    assert!(keep_it_to_yourself(Presence::Stranger, &on));
    assert!(keep_it_to_yourself(Presence::NotAlone, &on));
    assert!(!keep_it_to_yourself(Presence::AtDesk, &on));
    assert!(!keep_it_to_yourself(Presence::Away, &on));
    // An unread camera is not an empty room, and it is not a full one either.
    assert!(
        !keep_it_to_yourself(Presence::Unknown, &on),
        "a machine with no camera must not go quiet forever"
    );

    let off = PresenceConfig { discreet_with_strangers: false, ..on.clone() };
    assert!(!keep_it_to_yourself(Presence::Stranger, &off), "the setting is what decides");
    let blind = PresenceConfig { enabled: false, ..on };
    assert!(
        !keep_it_to_yourself(Presence::Stranger, &blind),
        "with the camera off there is no reading to act on"
    );
}

#[test]
fn a_private_note_knocks_on_both_routes() {
    // `Note::shown` was the screen's behaviour only: the spoken route logged
    // the body in full, so the same note disclosed or knocked depending on
    // which way it went out.
    let n = atlas::notify::Note::new(
        "Card declined",
        "the card ending 42",
        atlas::notify::Urgency::Routine,
        0,
    )
    .private();
    let (title, body) = n.shown();
    assert_eq!(title, "Card declined");
    assert!(!body.contains("42"), "the content waits until you ask");

    let d = live_source("src/daemon.rs");
    assert!(
        d.contains("let (title, body) = note.shown();"),
        "the spoken route is reading the raw body again"
    );
    assert!(
        d.contains("presence::keep_it_to_yourself(here, &self.eyes.cfg)"),
        "`reach_you` has stopped consulting your discretion setting"
    );
    assert!(
        d.contains("self.eyes.state") && d.contains("let seen ="),
        "`reach_you` is guessing presence from idle time again while the camera runs"
    );
}

// ---------- retention.session_turns / .logs_mb / .approvals_detailed ----------

#[test]
fn the_conversation_keeps_the_number_of_turns_you_asked_for() {
    use atlas::intent::Intent;
    use atlas::session::Session;

    let mut s = Session::holding(3);
    for i in 0..10 {
        s.record(&format!("say {i}"), &Intent::Unknown(String::new()), "fine");
    }
    assert_eq!(s.turns.len(), 3, "`retention.session_turns` decides this, not a literal 40");
    assert!(s.turns[2].said.contains('9'), "the newest turns are the ones kept");

    // Default stays where it was, for callers with no config to hand.
    let mut d = Session::default();
    for i in 0..60 {
        d.record(&format!("say {i}"), &Intent::Unknown(String::new()), "fine");
    }
    assert_eq!(d.turns.len(), 40);
}

#[test]
fn the_log_and_the_approval_ledger_are_sized_from_your_file() {
    // Both are built inside `Daemon::new`, where the sizes are not observable
    // from outside — `Log` does not report its cap and `compact_approvals`
    // takes the number by value. So this checks the call sites, and the test
    // above it runs the one of the three that can be run.
    let d = live_source("src/daemon.rs");
    assert!(
        d.contains("Log::new(logs_dir, retention.logs_mb * 1024 * 1024)"),
        "the log is rotating at a hardcoded size again -- it was 2MB while the file said 4"
    );
    assert!(
        !d.contains("compact_approvals(200)"),
        "the approval ledger is back to a literal 200 beside a setting that says 200"
    );
    assert!(
        d.contains("retention.approvals_detailed"),
        "nothing reads `retention.approvals_detailed`"
    );
    assert!(
        d.contains("Session::holding(retention.session_turns)"),
        "the daemon is building a default session again"
    );

    // And the file the daemon reads still carries all three.
    let t = tools();
    assert!(t.retention.logs_mb > 0);
    assert!(t.retention.approvals_detailed > 0);
    assert!(t.retention.session_turns > 0);
}

// ---------- opsec.always_strip_metadata ----------

#[test]
fn the_export_reads_your_opsec_setting_rather_than_a_literal() {
    // What the arguments do with the flag is tested by running them in
    // `tests/plainly_publishing.rs`; what matters here is that the flag comes
    // from the file.
    let m = live_source("src/main.rs");
    assert!(
        m.contains("t.opsec.always_strip_metadata"),
        "`atlas video export` is deciding this for itself again"
    );
    assert!(
        !m.contains("export_args(&e, path, &out, true)"),
        "the setting has been replaced by a hardcoded true"
    );
    assert!(tools().opsec.always_strip_metadata, "the shipped default is to strip");
}

// ---------- capture.never_ask_on_capture / .tasks_become_work ----------

#[test]
fn catching_a_thought_asks_nothing_and_puts_a_task_on_the_list() {
    use atlas::daemon::Daemon;
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::store::Store;

    let c = Config::load(Path::new("config")).expect("config/ loads");
    assert!(
        c.tools.as_ref().map(|t| t.capture.never_ask_on_capture).unwrap_or(false),
        "the shipped file says ask nothing at capture time"
    );
    assert!(c.tools.as_ref().map(|t| t.capture.tasks_become_work).unwrap_or(false));

    // Phrased so the question would fire if anything let it: a title and no
    // project is exactly what `Spoken::worth_asking` answers "which project?"
    // to. Picking a sentence that was never going to be asked about would
    // make this test pass with the wiring torn out -- which is what the first
    // version of it did, and what the mutation check caught.
    let said = "note that called dentist follow-up";
    let spoken = atlas::capture::read_spoken("called dentist follow-up", &[], &[]);
    assert_eq!(
        spoken.worth_asking(),
        Some("which project?"),
        "this sentence has to be one Atlas would want to ask about"
    );
    assert!(!spoken.is_a_whole_item(), "or it takes the other branch entirely");

    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let dir = std::env::temp_dir().join(format!("atlas-capture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(dir.clone()),
        Proactive::new(ProactiveConfig::default()),
    );

    let reply = d.turn(said, 100);
    assert!(
        !reply.contains("which project?"),
        "`never_ask_on_capture` ships true and this asked anyway: {reply}"
    );

    // "Turn tasks into outstanding items automatically" -- the other half of
    // the same section, and the reason the note is not the end of it.
    let outstanding: Vec<String> =
        d.backlog.outstanding().iter().map(|i| i.request.clone()).collect();
    assert!(
        outstanding.iter().any(|r| r.contains("dentist")),
        "a captured task never reached the outstanding list: {outstanding:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- cloudsync.provider ----------

/// The shipped config with one line changed, in a directory of its own.
fn config_with_provider(dir: &Path, provider: &str) -> std::path::PathBuf {
    fn copy_into(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("config dir");
        for e in std::fs::read_dir(from).expect("shipped config/").flatten() {
            let (src, dst) = (e.path(), to.join(e.file_name()));
            if src.is_dir() {
                copy_into(&src, &dst);
            } else {
                std::fs::copy(&src, &dst).expect("copy config file");
            }
        }
    }
    let cfg = dir.join("config");
    copy_into(Path::new("config"), &cfg);
    let tools = cfg.join("tools.yaml");
    let raw = std::fs::read_to_string(&tools).expect("tools.yaml");
    let mut out = String::with_capacity(raw.len());
    let mut done = false;
    for line in raw.lines() {
        if !done && line.trim_start().starts_with("provider:") {
            out.push_str(&format!("  provider: {provider}\n"));
            done = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    assert!(done, "no provider: line in the shipped tools.yaml");
    std::fs::write(&tools, out).expect("write tools.yaml");
    cfg
}

#[test]
fn sync_setup_with_nothing_after_it_uses_the_provider_you_configured() {
    // `cloud.provider` shipped `onedrive` and was read by nothing: the
    // provider came from the command line or the command gave up and opened
    // the hub, so the line in the file decided nothing at all.
    //
    // End to end, because the deciding code is `run_sync_setup` in `main.rs`
    // and the honest way to show a config value reached it is to run the
    // program twice against two files.
    let home = std::env::temp_dir().join(format!("atlas-cloud-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("scratch");

    // Spawned with a deadline rather than `output()`, and the reason is the
    // failure this test is guarding against: with the provider unread,
    // `sync-setup` falls through to `run_hub`, which serves the dashboard and
    // never returns. A test that hangs instead of failing is a test nobody
    // can read the result of -- the first version of this one hung for
    // fifteen minutes under its own mutation check.
    let run = |cfg_dir: &Path| -> String {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_atlas"))
            .arg("sync-setup")
            .env("ATLAS_HOME", &home)
            .env("ATLAS_CONFIG", cfg_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("atlas sync-setup starts");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match child.try_wait().expect("waiting on atlas") {
                Some(_) => break,
                None if std::time::Instant::now() > deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!(
                        "`atlas sync-setup` with no provider named did not finish -- it fell \
                         through to the hub, which means the configured provider was never read"
                    );
                }
                None => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        let mut out = String::new();
        if let Some(mut so) = child.stdout.take() {
            use std::io::Read;
            let _ = so.read_to_string(&mut out);
        }
        out
    };

    let one = run(&config_with_provider(&home.join("a"), "onedrive"));
    let two = run(&config_with_provider(&home.join("b"), "dropbox"));

    // Not only `contains`: the two runs must differ, and neither may be the
    // empty output of a program that fell over. `tests/retrospective.rs`
    // rightly flags a test whose every assertion is a phrase lookup -- that
    // proves a wording, not a behaviour.
    assert!(!one.trim().is_empty(), "`atlas sync-setup` printed nothing at all");
    assert_ne!(one, two, "two different providers produced identical output");
    assert!(one.contains("OneDrive"), "the configured provider is not the one it set up:\n{one}");
    assert!(!one.contains("Dropbox"));
    assert!(two.contains("Dropbox"), "changing the file changed nothing:\n{two}");
    assert!(!two.contains("OneDrive"));

    let _ = std::fs::remove_dir_all(&home);
}

// ---------- the shipped file still carries them ----------

#[test]
fn every_setting_wired_today_is_still_in_the_shipped_file() {
    // Wiring a setting and dropping it from `tools.yaml` would leave nobody
    // knowing the behaviour was theirs to change -- the same silence from the
    // other end.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    for key in [
        "never_ask_on_capture:",
        "tasks_become_work:",
        "min_posts_for_patterns:",
        "ask_after_days:",
        "always_strip_metadata:",
        "discreet_with_strangers:",
        "shelves:",
        "approvals_detailed:",
        "logs_mb:",
        "session_turns:",
        "longest_minutes:",
        "provider:",
    ] {
        assert!(raw.contains(key), "{key} is no longer in the shipped config");
    }
}

#[test]
fn none_of_them_is_still_listed_as_doing_nothing() {
    // `dead_config.rs` is a two-way ratchet and would catch this, but the
    // failure it prints is about a list. This one is about the twelve.
    let listed = std::fs::read_to_string("tests/dead_config.rs").expect("dead_config.rs");
    let dead: Vec<&str> = listed
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.starts_with('"') && l.ends_with("\","))
        .collect();
    for key in [
        "capture::CaptureConfig.never_ask_on_capture",
        "capture::CaptureConfig.tasks_become_work",
        "cloudsync::CloudConfig.provider",
        "content::ContentConfig.min_posts_for_patterns",
        "daily::DailyConfig.ask_after_days",
        "opsec::OpsecConfig.always_strip_metadata",
        "presence::PresenceConfig.discreet_with_strangers",
        "reference::ReferenceConfig.shelves",
        "retention::RetentionConfig.approvals_detailed",
        "retention::RetentionConfig.logs_mb",
        "retention::RetentionConfig.session_turns",
        "viewing::ViewConfig.longest_minutes",
    ] {
        assert!(
            !dead.iter().any(|l| l.contains(key)),
            "{key} is wired and still listed as a setting that does nothing"
        );
    }
}

// ---------- grade.target_lufs / .max_true_peak_db / .preset ----------
//
// A different kind from the twelve above: these were not fields nothing read,
// they were lines in `tools.yaml` with **no type at all** to parse into, so
// serde dropped the whole `grade:` block. `config::NO_FIELD_TO_LAND_IN`
// recorded it. The numbers in the file matched `grade.rs`'s constants
// exactly, which is why it went unnoticed — the advice was right and the file
// had nothing to do with it.

#[test]
fn the_loudness_target_you_set_is_the_one_it_measures_against() {
    use atlas::grade::{check_audio, Audio, GradeConfig, MAX_TRUE_PEAK_DB, TARGET_LUFS};

    let quiet = Audio {
        lufs: -20.0,
        true_peak_db: -6.0,
        range_db: 9.0,
        noise_floor_db: -60.0,
        harsh_s: false,
        rumble: false,
    };

    // At the shipped target, -20 LUFS is six under and worth saying.
    let shipped = GradeConfig::default();
    assert_eq!(shipped.target_lufs, TARGET_LUFS, "the constant is still the default");
    assert_eq!(shipped.max_true_peak_db, MAX_TRUE_PEAK_DB);
    let notes = check_audio(&quiet, &shipped);
    assert!(notes.iter().any(|n| n.what.contains("quiet")), "{notes:?}");

    // Mastering for somewhere quieter: the same file is now on target, and
    // the complaint goes away because the setting moved, not the audio.
    let quieter_target = GradeConfig { target_lufs: -20.0, ..GradeConfig::default() };
    let notes = check_audio(&quiet, &quieter_target);
    assert!(
        !notes.iter().any(|n| n.what.contains("quiet") || n.what.contains("too loud")),
        "the loudness note should follow the target: {notes:?}"
    );

    // And the ceiling is read the same way.
    let hot = Audio { true_peak_db: -2.0, ..quiet };
    assert!(!check_audio(&hot, &shipped).iter().any(|n| n.what.contains("clip")));
    let strict = GradeConfig { max_true_peak_db: -3.0, ..GradeConfig::default() };
    assert!(check_audio(&hot, &strict).iter().any(|n| n.what.contains("clip")));
}

#[test]
fn a_preset_name_that_is_not_one_is_said_rather_than_ignored() {
    use atlas::grade::preset_named;
    assert_eq!(preset_named("clean").map(|p| p.name), Some("clean"));
    assert_eq!(preset_named("  CLEAN ").map(|p| p.name), Some("clean"), "trimmed, any case");
    assert!(preset_named("sepia").is_none(), "a name that matches nothing must not fall back");

    // The shipped file's preset has to be a real one, or `atlas video grade`
    // opens by telling you your own config is wrong.
    let named = tools().grade.preset;
    assert!(preset_named(&named).is_some(), "config/tools.yaml ships `preset: {named}`");
}

#[test]
fn how_much_posted_before_a_direction_is_claimed_is_yours() {
    use atlas::reach::{direction, Direction, ReachConfig};

    // Five posts, improving. The shipped floor is six, so nothing is claimed.
    let posts: Vec<atlas::reach::Post> = (0..5)
        .map(|i| atlas::reach::Post {
            id: i.to_string(),
            at: 1_700_000_000 + i as u64 * 86_400,
            views: 1_000 + i as u64 * 500,
            held_at_three: 0.5 + i as f32 * 0.05,
            completion: 0.4 + i as f32 * 0.05,
            saves: 10 + i as u64,
            shares: 2,
            comments: 3,
            follows: 1,
            topic: "fees".into(),
            hook: "question".into(),
            seconds: 30.0,
        })
        .collect();

    let shipped = ReachConfig::default();
    assert_eq!(shipped.min_posts_for_direction, 6);
    assert_eq!(direction(&posts, &shipped, &atlas::judgment::JudgmentConfig::default()).0, Direction::Unclear, "five is under the floor");

    let keener = ReachConfig { min_posts_for_direction: 4 };
    assert_ne!(
        direction(&posts, &keener, &atlas::judgment::JudgmentConfig::default()).0,
        Direction::Unclear,
        "asked for four, and there are five"
    );
}

#[test]
fn neither_section_is_listed_as_having_nowhere_to_land() {
    for (key, _) in atlas::config::NO_FIELD_TO_LAND_IN {
        assert!(
            !["grade", "reach"].contains(key),
            "{key} parses into a type now and is still listed as having none"
        );
    }
    // And both really do parse, out of the shipped file rather than a literal.
    let t = tools();
    assert!(t.grade.target_lufs < 0.0, "LUFS is negative or the section did not parse");
    assert!(t.reach.min_posts_for_direction >= 2);
}

// ---------- the promise that was not kept, and now is ----------
//
// `cloudsync.encrypt_before_writing` was `#[serde(skip)]`, pinned true and
// read by nothing, while `Daemon::carry_to_your_other_devices` wrote the
// bundle with `serde_json::to_string_pretty` into a cloud-synced folder and
// `cloudsync::WHY_ONEDRIVE` told the reader "I encrypt before anything is
// written either way".
//
// The field is gone and the capability is built: `sync.encrypt_bundles`,
// shipping off, sealing with the household key phrase. These tests hold the
// three things that have to stay true — the pages say what is actually the
// case, the switch cannot quietly fall back to plaintext, and a bundle still
// cannot carry anything worth reading if it is not sealed.

#[test]
fn nothing_claims_more_than_the_switch_does() {
    let why = atlas::cloudsync::WHY_ONEDRIVE.to_lowercase();
    assert!(
        !why.contains("encrypt"),
        "`WHY_ONEDRIVE` is claiming encryption again -- sealing is a switch, and it ships off"
    );

    let about = atlas::cloudsync::ABOUT_BUNDLES;
    assert!(about.contains("plain text"), "the default has to be said plainly: {about}");
    // Named the way the person will look for it -- the switch is in the hub
    // now, and a sentence that names a YAML key is a sentence that sends
    // somebody to a file they do not edit.
    assert!(
        about.contains("Seal what I carry between devices"),
        "the switch has to be named as it appears: {about}"
    );
    assert!(
        about.contains("nothing is lost"),
        "and the thing that stops it being frightening: {about}"
    );

    // And it is said where the decision is made, not only in a doc comment.
    let m = live_source("src/main.rs");
    assert!(
        m.contains("cloudsync::ABOUT_BUNDLES"),
        "`atlas sync-setup` no longer says what a bundle is"
    );
}

#[test]
fn turning_sealing_on_makes_its_own_key() {
    // The first version refused and sent you to a terminal. That is a wall in
    // front of a switch, and the switch is in the hub precisely because the
    // person it is for does not use a terminal.
    //
    // Making one unasked is safe for one reason, and the reason is the whole
    // design: losing it costs nothing durable. `a_lost_key_costs_nothing`
    // below is that claim, run.
    let d = live_source("src/daemon.rs");
    let at = d.find("fn carry_to_your_other_devices").expect("the carrier is gone");
    let body = &d[at..(at + 8000).min(d.len())];
    assert!(
        body.contains("if cfg.encrypt_bundles"),
        "the carrier is not consulting `sync.encrypt_bundles`"
    );
    assert!(
        body.contains("crate::sync::ensure_key(&self.store, now)"),
        "sealing with no key must make one, not refuse"
    );
    assert!(
        !body.contains("None => Err(crate::sync::NO_KEY_YET.to_string())"),
        "it is back to refusing rather than making a key"
    );
    // And it still never writes plaintext when sealing is on and the key
    // could not be made at all.
    assert!(
        atlas::sync::NO_KEY_YET.contains("have not written the bundle"),
        "the one remaining failure has to say nothing was written"
    );
    assert!(
        !atlas::sync::SyncConfig::default().encrypt_bundles,
        "sealing ships off -- nobody should be locked out of a folder they are still setting up"
    );
}

#[test]
fn the_switch_is_in_the_hub_and_the_key_has_a_page() {
    // "I don't remember commands" is a requirement, not a preference. Every
    // part of this has to be reachable by clicking.
    let s = atlas::settings::registry(&tools());
    let item = s
        .get("sync.encrypt_bundles")
        .expect("sealing has no switch on the settings page");
    assert!(
        item.cost.contains("nothing is lost"),
        "the switch has to say what it costs: {}",
        item.cost
    );
    assert!(
        item.name.split_whitespace().count() <= 3,
        "a setting is named, then explained -- `{}` is a sentence",
        item.name
    );
    assert!(item.weight.needs_confirming(), "it changes what a folder elsewhere holds");

    assert_eq!(atlas::hub::route("/hub/sync"), Some(atlas::hub::Page::Sync));
    assert!(
        atlas::hub::works_without_voice(atlas::hub::Page::Sync),
        "the day sync breaks may be a day you cannot ask out loud"
    );
    assert!(
        atlas::hub::NAV.iter().any(|(_, pages)| pages.contains(&atlas::hub::Page::Sync)),
        "a page nothing links to is a page nobody opens"
    );

    // The page itself says the thing that stops someone being afraid of it,
    // and offers the button that makes it true.
    let page = sync_page_unplaced(true, "D:/Dropbox/atlas", Some("ABCD-2345"), None, None, "");
    assert!(page.contains("courier"), "the page has to explain what a bundle is");
    assert!(page.contains("Make a new key"));
    assert!(page.contains("action=/hub/sync"), "the button has to post somewhere");
    assert!(page.contains("ABCD-2345"), "the key has to be findable by someone who lost it");
    assert!(!page.to_lowercase().contains("<script"), "the hub works with scripts off");
}

#[test]
fn a_recovery_card_is_a_file_you_can_copy_not_a_thing_to_memorise() {
    let phrase = atlas::sync::new_key_phrase();
    let card = atlas::sync::recovery_card(&phrase);
    assert!(card.contains(&phrase), "the card has to carry the key");
    assert!(card.contains("You lose nothing that matters"), "and the reassurance: {card}");
    assert!(card.contains("--card"), "and how to use it without typing the phrase");

    // Read back out of the card, including one a person has scribbled on.
    assert_eq!(atlas::sync::phrase_in_card(&card).as_deref(), Some(phrase.as_str()));
    let annotated = format!("{card}\n\nkept in the desk drawer, top right\n");
    assert_eq!(atlas::sync::phrase_in_card(&annotated).as_deref(), Some(phrase.as_str()));
    assert!(
        atlas::sync::phrase_in_card("nothing here looks like a key").is_none(),
        "it must not invent one"
    );
}

#[test]
fn a_sealed_bundle_opens_with_the_phrase_and_with_nothing_else() {
    use atlas::sync::{key_from_phrase, new_key_phrase, peek, read_bundle, seal, Bundle};

    let bundle = Bundle {
        from_device: "evening-laptop".into(),
        from_name: "Eric's laptop".into(),
        made_at: 1_700_000_000,
        up_to_seq: 7,
        events: Vec::new(),
        version: 1,
        belongs_to: "personal".into(),
    };

    let phrase = new_key_phrase();
    let key = key_from_phrase(&phrase).expect("a fresh phrase is a usable one");
    let sealed = seal(&bundle, &key).expect("sealing");

    // The contents are not in the file, and the header is.
    assert!(!sealed.contains("Eric's laptop"), "the sender's name is inside the sealed part");
    let header = peek(&sealed).expect("a sealed bundle says it is one");
    assert_eq!(header.from_device, "evening-laptop", "routing works without a key");
    assert_eq!(header.belongs_to, "personal");

    // The phrase is the key: derived again from the words alone, on what
    // might as well be another machine.
    let again = key_from_phrase(&phrase).expect("same phrase");
    let opened = read_bundle(&sealed, Some(&again)).expect("it opens");
    assert_eq!(opened, bundle, "what comes out is what went in");

    // Without it, it says which command fixes that rather than failing to
    // parse.
    let refused = read_bundle(&sealed, None).unwrap_err();
    // 27 Sep 2026: the running Atlas says this to a person who may never
    // open a terminal, so it points at the Sync page's "Use a key from
    // another device" now. The command line keeps naming its command
    // (`Reader::Command`, and the `atlas sync read` test further down).
    assert!(refused.contains("Sync page"), "got: {refused}");
    assert!(!refused.contains("atlas sync"), "a command in the running Atlas's words: {refused}");
    let at_the_prompt =
        atlas::sync::read_bundle_for(&sealed, None, atlas::sync::Reader::Command).unwrap_err();
    assert!(at_the_prompt.contains("atlas sync key set"), "got: {at_the_prompt}");
    assert!(refused.contains("evening-laptop"), "and which device left it: {refused}");

    // A different phrase is a different key, and says so as a wrong key
    // rather than a broken file.
    let other = key_from_phrase(&new_key_phrase()).expect("another phrase");
    let wrong = read_bundle(&sealed, Some(&other)).unwrap_err();
    assert!(wrong.contains("different household key"), "got: {wrong}");

    // And a plain bundle still reads, with or without a key. Turning sealing
    // off must not strand what was written before it went on.
    let plain = serde_json::to_string(&bundle).expect("plain");
    assert_eq!(read_bundle(&plain, None).expect("plain opens"), bundle);
    assert_eq!(read_bundle(&plain, Some(&key)).expect("and with a key"), bundle);
}

#[test]
fn the_phrase_is_forgiving_in_the_ways_paper_is() {
    use atlas::sync::key_from_phrase;
    // Copied from a card: lower case, spaces instead of dashes, and the
    // letters the alphabet avoids typed as the digits they look like.
    let a = key_from_phrase("ABCD-2345-6789-WXYZ-3456-789A").expect("one");
    let b = key_from_phrase("abcd 2345 6789 wxyz 3456 789a").expect("the same, badly copied");
    assert_eq!(a, b, "a phrase written by hand has to open the same file");

    // But it is not guessing. Something that is not a key is refused rather
    // than turned into one.
    assert!(key_from_phrase("hello").is_err());
    assert!(key_from_phrase("").is_err());
}

#[test]
fn the_key_is_kept_in_a_way_the_device_can_describe() {
    use atlas::sync::KeptKey;
    let empty = KeptKey::default();
    assert!(!empty.is_set());
    assert!(empty.phrase().is_err(), "nothing to give back");

    let phrase = atlas::sync::new_key_phrase();
    let kept = KeptKey::keeping(&phrase, 1_700_000_000);
    assert!(kept.is_set());
    assert_eq!(kept.phrase().expect("it comes back"), phrase);
    // Whichever way it was kept, the device says which -- an OS-sealed key
    // and a text one are different facts about your machine, and guessing
    // which you have is not something a person should have to do.
    let said = kept.at_rest_says();
    assert!(said.len() > 40);
    assert_eq!(
        kept.sealed_by_os,
        said.contains("sealed by Windows"),
        "it must describe how it actually kept it"
    );
}

#[test]
fn a_bundle_can_only_carry_the_five_things_it_carries_today() {
    // `sync.bundles_carry_secrets` is pinned false and read by nothing, and
    // what actually keeps it true is that `What` has five variants and none
    // of them is a credential. Sealing does not retire this: a bundle is
    // plain text by default, and even sealed it is readable by anyone who
    // has the phrase -- which is every device in the household.
    let src = std::fs::read_to_string("src/sync.rs").expect("src/sync.rs");
    let body = src.split("pub enum What").nth(1).expect("What is gone");
    let body = &body[..body.find("\n}").expect("unterminated What")];
    let variants: Vec<String> = body
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.starts_with("//") && !l.is_empty())
        .filter_map(|l| {
            let name: String =
                l.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            (!name.is_empty() && name.chars().next().unwrap().is_uppercase()).then_some(name)
        })
        .collect();

    assert_eq!(
        variants,
        vec!["Captured", "Said", "Finished", "Changed", "Removed"],
        "`sync::What` has changed. A bundle is plain text unless sealing is on, in a folder a \
         cloud provider copies -- if the new variant can carry anything you would not want \
         read there, it does not belong in a bundle. Update this list deliberately."
    );
}

#[test]
fn a_bundle_written_by_the_daemon_is_sealed_and_opens_by_hand() {
    // The whole claim, end to end and through the program: Atlas writes a
    // bundle with sealing on, what lands on disk does not contain what you
    // captured, and `atlas sync read` opens it on a machine that holds no
    // key at all — given the phrase.
    //
    // That last part is the requirement the design was built backwards from.
    // An encrypted file you cannot open by hand is not safer, it is lost.
    use atlas::daemon::Daemon;
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::store::Store;

    let root = std::env::temp_dir().join(format!("atlas-sealing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let folder = root.join("carrier");
    std::fs::create_dir_all(&folder).expect("carrier");

    let mut c = Config::load(Path::new("config")).expect("config/ loads");
    let phrase = atlas::sync::new_key_phrase();
    {
        let t = c.tools.as_mut().expect("tools.yaml");
        t.sync.enabled = true;
        t.sync.encrypt_bundles = true;
        t.sync.folder = folder.to_string_lossy().into_owned();
    }

    let store = Store::new(root.join("state"));
    store
        .save(atlas::sync::KEY_FILE, &atlas::sync::KeptKey::keeping(&phrase, 1_700_000_000))
        .expect("keep the key");

    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let mut d = Daemon::new(&c, &p, None, store, Proactive::new(ProactiveConfig::default()));

    // Something worth not leaving in a cloud folder in the clear.
    d.turn("note that called the roof quote from Hendricks", 100);
    let said = d.turn("sync", 200);

    let written: Vec<std::path::PathBuf> = std::fs::read_dir(&folder)
        .expect("carrier readable")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("bundle"))
        .collect();
    assert_eq!(written.len(), 1, "one bundle, got {written:?} -- said: {said}");

    let raw = std::fs::read_to_string(&written[0]).expect("read it back");
    // Case-insensitively, because the capture is stored lower-cased -- a
    // check for the exact spelling would have passed on a plaintext file.
    assert!(
        !raw.to_lowercase().contains("hendricks"),
        "what was captured is sitting in the carrier in the clear:\n{raw}"
    );
    let header = atlas::sync::peek(&raw).expect("it should be a sealed bundle");
    assert!(!header.from_device.is_empty(), "the header has to survive for routing");

    // Opened by hand, by the command, with nothing but the phrase — the
    // process holds no key file of its own.
    let elsewhere = root.join("another-machine");
    std::fs::create_dir_all(&elsewhere).expect("another machine");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_atlas"))
        .args(["sync", "read", &written[0].to_string_lossy(), "--key", &phrase])
        .env("ATLAS_HOME", &elsewhere)
        .env("ATLAS_CONFIG", Path::new("config"))
        .output()
        .expect("atlas sync read runs");
    let printed = String::from_utf8_lossy(&out.stdout);
    assert!(
        printed.to_lowercase().contains("hendricks"),
        "the phrase did not open it on a machine with no key:\n{printed}"
    );

    // And without the phrase, it says which command fixes that.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_atlas"))
        .args(["sync", "read", &written[0].to_string_lossy()])
        .env("ATLAS_HOME", &elsewhere)
        .env("ATLAS_CONFIG", Path::new("config"))
        .output()
        .expect("atlas sync read runs");
    let printed = String::from_utf8_lossy(&out.stdout);
    assert!(
        !printed.to_lowercase().contains("hendricks"),
        "it opened with no key at all:\n{printed}"
    );
    assert!(printed.contains("atlas sync key set"), "got: {printed}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_lost_key_costs_nothing() {
    // The claim the whole design rests on, run rather than asserted.
    //
    // Eric's objection was that he cannot promise never to lose a key, and
    // will not accept a scheme where losing one loses his files. The answer
    // is not a better key ceremony — it is that a bundle is a courier.
    // `make_bundle` is called with `since = 0`, so every bundle carries the
    // device's whole log from the beginning, and the log lives on the device.
    //
    // So: seal something, throw the key away entirely, make a new one, and
    // the next bundle carries the same things again. Nothing is recovered
    // from the old file; nothing needed to be.
    use atlas::daemon::Daemon;
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::store::Store;

    let root = std::env::temp_dir().join(format!("atlas-lostkey-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let folder = root.join("carrier");
    std::fs::create_dir_all(&folder).expect("carrier");

    let mut c = Config::load(Path::new("config")).expect("config/ loads");
    {
        let t = c.tools.as_mut().expect("tools.yaml");
        t.sync.enabled = true;
        t.sync.encrypt_bundles = true;
        t.sync.folder = folder.to_string_lossy().into_owned();
    }
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);

    let state = root.join("state");
    // Found rather than assumed: the file is named after the device, and
    // `sanitise` decides what that looks like on disk.
    let the_bundle = |folder: &std::path::Path| -> std::path::PathBuf {
        std::fs::read_dir(folder)
            .expect("carrier readable")
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|x| x.to_str()) == Some("bundle"))
            .expect("a bundle was written")
    };

    // First run: nothing is set up at all. No key, no card, no commands.
    {
        let store = Store::new(state.clone());
        let mut d =
            Daemon::new(&c, &p, None, store, Proactive::new(ProactiveConfig::default()));
        d.turn("note that called the roof quote from Hendricks", 100);
        let said = d.turn("sync", 200);
        assert!(said.contains("sealed"), "it should have sealed it: {said}");
    }
    let first = std::fs::read_to_string(the_bundle(&folder)).expect("a bundle was written");
    assert!(atlas::sync::peek(&first).is_some(), "sealed, with no key ever asked for");
    assert!(!first.to_lowercase().contains("hendricks"));

    // Now lose it. Not "forget the paper" -- delete every copy this machine
    // has, which is the worst case he described.
    let store = Store::new(state.clone());
    store
        .save(atlas::sync::KEY_FILE, &atlas::sync::KeptKey::default())
        .expect("forget the key");
    let kept: atlas::sync::KeptKey = store.load(atlas::sync::KEY_FILE);
    assert!(!kept.is_set(), "the key is gone");

    // The old bundle is now unreadable, and says so rather than pretending.
    let orphaned = atlas::sync::read_bundle(&first, None).unwrap_err();
    assert!(orphaned.contains("sealed bundle"), "got: {orphaned}");

    // The button on the sync page. One press.
    let made = atlas::sync::new_key(&store, 300).expect("a new key");
    assert!(made.card.is_some(), "and it writes the card without being asked");

    // The next thing it carries has everything in it again.
    {
        let mut d = Daemon::new(
            &c,
            &p,
            None,
            Store::new(state.clone()),
            Proactive::new(ProactiveConfig::default()),
        );
        d.turn("sync", 400);
    }
    let second = std::fs::read_to_string(the_bundle(&folder)).expect("a second bundle");
    assert_ne!(first, second, "it should have been rewritten under the new key");

    let key = atlas::sync::key_from_phrase(&made.phrase).expect("the new key");
    let opened = atlas::sync::read_bundle(&second, Some(&key)).expect("it opens");
    let carried: Vec<String> = opened
        .events
        .iter()
        .map(|e| format!("{:?}", e.what))
        .collect();
    assert!(
        carried.iter().any(|e| e.to_lowercase().contains("hendricks")),
        "the capture from before the key was lost did not come back: {carried:?}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_new_device_gets_the_key_from_the_pairing_it_already_does() {
    // The friction that was left: a paired machine still could not read a
    // sealed bundle until somebody copied a key file across by hand.
    //
    // It rides the pairing code, which is `server::new_token` -- OS entropy,
    // not a six-digit PIN -- so the thing that proves the pairing is the
    // thing that opens the key. Nothing extra is typed.
    use atlas::sync::{leave_handoff, sweep_handoffs, take_handoff};

    let folder = std::env::temp_dir().join(format!("atlas-handoff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("folder");

    let house = "hh-12345";
    let code = atlas::server::new_token().expect("a pairing code");
    let phrase = atlas::sync::new_key_phrase();

    let left = leave_handoff(&folder, house, &code, &phrase, 1_000, 180).expect("left");
    let raw = std::fs::read_to_string(&left).expect("it is a file");
    assert!(!raw.contains(&phrase), "the key is sitting in the folder in the clear");
    assert!(
        raw.contains(house),
        "the header says which household, so the wrong one is refused"
    );
    assert!(
        !raw.contains("\"household\""),
        "a field called `household` here makes the deadness scan read an unrelated dead \
         setting as wired -- it is `for_household` for that reason"
    );

    // The far side, with the code it was given.
    let got = take_handoff(&folder, house, &code, 1_060).expect("it opens");
    assert_eq!(got, phrase, "the same key, so sealed bundles will open");
    assert!(!left.exists(), "single use -- a handoff left lying about is the cost of this");

    // Wrong code, wrong household, and too late: three different sentences,
    // none of them a silent failure.
    let left = leave_handoff(&folder, house, &code, &phrase, 1_000, 180).expect("again");
    let wrong = take_handoff(&folder, house, "not-the-code", 1_060).unwrap_err();
    assert!(wrong.contains("doesn't open"), "got: {wrong}");

    let left2 = leave_handoff(&folder, house, &code, &phrase, 1_000, 180).expect("again");
    assert_eq!(left, left2, "one file per household");
    let other = take_handoff(&folder, "hh-99999", &code, 1_060).unwrap_err();
    assert!(other.contains("no key waiting"), "got: {other}");

    // The check inside the file, not the one in its name. A handoff renamed
    // -- by a sync client resolving a conflict, by somebody curious -- must
    // still be refused by the household it is not for. The name being right
    // is not evidence: that check was unreachable by the test above, and a
    // mutation that deleted it passed until this case existed.
    let planted = folder.join("hh-99999.keyhandoff");
    std::fs::copy(&left2, &planted).expect("plant it under another household's name");
    let renamed = take_handoff(&folder, "hh-99999", &code, 1_060).unwrap_err();
    assert!(
        renamed.contains("different household"),
        "a handoff has to say who it is for from the inside: {renamed}"
    );
    let _ = std::fs::remove_file(&planted);

    let late = take_handoff(&folder, house, &code, 9_999).unwrap_err();
    assert!(late.contains("expired"), "got: {late}");
    assert!(!left.exists(), "and an expired one is cleared as it is found");

    // A pairing somebody started and walked away from.
    leave_handoff(&folder, house, &code, &phrase, 1_000, 180).expect("abandoned");
    assert_eq!(sweep_handoffs(&folder, 1_060), 0, "not while the window is open");
    assert_eq!(sweep_handoffs(&folder, 9_999), 1, "gone once it has closed");
    assert_eq!(sweep_handoffs(&folder, 9_999), 0, "and nothing to do the second time");

    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn pairing_hands_the_key_over_and_the_daemon_sweeps_what_is_left() {
    // The wiring, at the two call sites that make it real.
    let m = live_source("src/main.rs");
    assert!(
        m.contains("atlas::sync::leave_handoff("),
        "`household pair` no longer leaves the key for the new device"
    );
    assert!(
        m.contains("atlas::sync::take_handoff("),
        "`household join` no longer picks it up"
    );
    assert!(
        m.contains("&pairing.code"),
        "the handoff has to be opened with the code that proved the pairing"
    );
    let d = live_source("src/daemon.rs");
    assert!(
        d.contains("crate::sync::sweep_handoffs(dir, now)"),
        "an abandoned handoff would sit in the folder forever"
    );
}

#[test]
fn pairing_is_ten_characters_and_the_folder_carries_the_rest() {
    // `encode_pairing` puts the household id, its name, a 24-character token
    // and two timestamps into one string. Nobody types that; they paste it,
    // which means they message it to themselves, which is the least secure
    // step in the design and the most annoying.
    //
    // What has to travel out of band is the secret. Everything else can wait
    // in the folder both machines already share, sealed.
    use atlas::household::{leave_invitation, new_invite_code, sweep_invitations, take_invitation};

    let folder = std::env::temp_dir().join(format!("atlas-invite-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("folder");

    let code = new_invite_code();
    assert_eq!(code.chars().filter(|c| *c != '-').count(), 10, "ten to type: {code}");
    assert!(code.contains('-'), "grouped, or it is ten characters of soup: {code}");
    assert!(
        !code.chars().any(|c| "ILOU".contains(c)),
        "no letters that are read as digits: {code}"
    );

    let phrase = atlas::sync::new_key_phrase();
    let at = leave_invitation(&folder, "hh-1", "Eric's things", &code, Some(&phrase), 1_000, 180)
        .expect("left");

    // The folder gives nothing away.
    let raw = std::fs::read_to_string(&at).expect("a file");
    assert!(!raw.contains("hh-1"), "the household is in the clear: {raw}");
    assert!(!raw.contains("Eric's things"), "its name is in the clear: {raw}");
    assert!(!raw.contains(&phrase), "the key is in the clear: {raw}");
    assert!(
        !at.file_name().unwrap().to_string_lossy().contains("hh-1"),
        "even the filename must not say whose it is"
    );

    // Typed on the other machine, forgivingly.
    let sloppy = code.to_lowercase().replace('-', " ");
    let inside = take_invitation(&folder, &sloppy, 1_060).expect("it opens");
    assert_eq!(inside.for_household, "hh-1");
    // Named `for_household`, not `household`: a field with that bare name
    // anywhere in the tree makes `HouseholdConfig.household` -- a setting
    // nothing reads -- look wired to the deadness scans. That guard caught
    // this twice while this feature was being written.
    let src = std::fs::read_to_string("src/household.rs").expect("src/household.rs");
    assert!(src.contains("pub for_household: String"));
    assert_eq!(inside.name, "Eric's things");
    assert_eq!(
        inside.key_phrase.as_deref(),
        Some(phrase.as_str()),
        "the key rides along, or joining and reading are two chores instead of one"
    );
    assert!(!at.exists(), "single use");

    // A wrong code says so, and does not consume anyone else's invitation.
    leave_invitation(&folder, "hh-1", "Eric's things", &code, None, 1_000, 180).expect("again");
    let wrong = take_invitation(&folder, &new_invite_code(), 1_060).unwrap_err();
    assert!(wrong.contains("no invitation"), "got: {wrong}");
    assert!(
        take_invitation(&folder, &code, 1_060).is_ok(),
        "a wrong guess must not eat the real invitation"
    );

    // Expiry is its own sentence: "you typed it right and you were slow" is
    // a different problem from "that code is wrong".
    leave_invitation(&folder, "hh-1", "Eric's things", &code, None, 1_000, 180).expect("again");
    let late = take_invitation(&folder, &code, 9_999).unwrap_err();
    assert!(late.contains("expired"), "got: {late}");

    // And one nobody finished is swept on the next sync pass.
    leave_invitation(&folder, "hh-1", "Eric's things", &code, None, 1_000, 180).expect("again");
    assert_eq!(sweep_invitations(&folder, 1_060), 0, "not while it is good");
    assert_eq!(sweep_invitations(&folder, 9_999), 1);

    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_short_code_is_worth_typing_and_the_long_one_still_works() {
    // The arithmetic that makes ten characters enough, written where it can
    // be checked rather than asserted in a comment: a 32-character alphabet,
    // ten of them, is 2^50, and what it protects exists for three minutes
    // behind Argon2id at 64MiB.
    let mut alphabet: std::collections::BTreeSet<char> = std::collections::BTreeSet::new();
    for _ in 0..200 {
        for c in atlas::household::new_invite_code().chars().filter(|c| *c != '-') {
            alphabet.insert(c);
        }
    }
    assert!(alphabet.len() >= 30, "the alphabet collapsed to {} characters", alphabet.len());
    assert_eq!(atlas::household::CODE_LEN, 10);

    // The old long block still opens, so a pairing started before this
    // existed is not rejected by an update.
    let p = atlas::household::new_pairing("some-long-token", 1_000);
    let block = atlas::household::encode_pairing("hh-2", "Eric's things", &p).expect("encodes");
    let (id, name, back) = atlas::household::decode_pairing(&block).expect("still decodes");
    assert_eq!((id.as_str(), name.as_str()), ("hh-2", "Eric's things"));
    assert_eq!(back.code, "some-long-token");

    // And both live in `household pair` / `join`, with the short one first.
    let m = live_source("src/main.rs");
    assert!(m.contains("atlas::household::leave_invitation("), "pair no longer offers the short code");
    assert!(m.contains("atlas::household::take_invitation("), "join no longer accepts one");
    assert!(m.contains("atlas::household::decode_pairing(code)"), "the long block was dropped");
}

#[test]
fn a_device_can_be_invited_and_joined_without_a_terminal() {
    // "I don't remember commands" applies to pairing too -- it was the one
    // part of this still living only in a shell.
    let page = sync_page_unplaced(true, "D:/Dropbox/atlas", Some("ABCD-2345"), None, None, "");
    assert!(page.contains("Invite a device"), "no way to start a pairing from the page");
    assert!(page.contains("name=code"), "and no way to finish one");
    assert!(page.contains("name=device"), "a joining machine has to say what it is called");
    assert!(page.contains("three minutes"), "the page has to say the window is short");

    // And the buttons reach something. A page whose forms post into the void
    // is prose with a border on it -- `tests/retrospective.rs` is right to
    // flag a test that only checks wording, and this is the part that makes
    // it a behaviour.
    let post = |body: &str| atlas::server::Request {
        method: "POST".into(),
        path: "/hub/sync".into(),
        query: String::new(),
        token: Some("x".repeat(24)),
        token_from_url: false,
        body: body.into(),
    };
    assert!(
        matches!(atlas::server::route(&post("what=pair")), Some(atlas::server::Action::SyncKey(w)) if w == "pair"),
        "the invite button posts nowhere"
    );
    assert!(
        matches!(atlas::server::route(&post("what=new")), Some(atlas::server::Action::SyncKey(w)) if w == "new"),
        "the new-key button posts nowhere"
    );
    match atlas::server::route(&post("what=join&code=ABCDE-FGHJK&device=phone")) {
        Some(atlas::server::Action::SyncJoin { code, device }) => {
            assert_eq!(code, "ABCDE-FGHJK");
            assert_eq!(device, "phone");
        }
        other => panic!("the join form does not reach a join: {other:?}"),
    }
    // And a button nobody put there does nothing -- and since 27 Sep 2026
    // says so on the Sync page, rather than "that isn't a page in Atlas".
    assert!(matches!(
        atlas::server::route(&post("what=delete-everything")),
        Some(atlas::server::Action::HubBack(atlas::hub::Page::Sync, said)) if said.contains("nothing changed")
    ));
}

#[test]
fn the_hub_can_be_reached_from_your_phone_without_being_on_the_internet() {
    // "Can I re-key from my phone once it's paired?" The buttons that fix
    // sync live on a page served by a listener bound to 127.0.0.1, which a
    // phone cannot reach even over a VPN -- the VPN gives it a route to the
    // machine, and loopback still refuses it. So the answer was no, and the
    // reason was one line in `Server::bind`.
    use atlas::server::bind_address;

    // Unchanged for anyone who does not ask: this machine only.
    assert_eq!(bind_address("").unwrap().to_string(), "127.0.0.1");
    assert!(atlas::server::ServerConfig::default().reachable_from.is_empty());
    let shipped = tools().server.reachable_from.clone();
    assert!(shipped.is_empty(), "the shipped file must not open a listener for you");

    // A Tailscale address, which is the case this is for.
    assert!(bind_address("100.101.102.103").is_ok());
    // And the two answers that would be worse than no feature at all.
    assert!(bind_address("0.0.0.0").is_err());
    assert!(bind_address("8.8.8.8").is_err());

    // The setting is documented where someone would look for it.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("reachable_from:"), "the switch is not in the shipped file");
    assert!(
        raw.contains("Tailscale") || raw.contains("VPN"),
        "and nothing says how you would get such an address"
    );

    // The page it exists for works on a phone.
    let page = sync_page_unplaced(true, "D:/atlas", Some("ABCD-2345"), None, None, "");
    assert!(page.contains("<meta name=viewport"), "the sync page has to work on a phone");
    assert!(page.contains("Make a new key"), "which is the button you came for");
}

// ---------- self_audit.every_days / .act_without_asking ----------
//
// Both were changeable and both inert, and for one reason: `recommend` was
// only ever reached from `Intent::WorkOnYourself`, which runs because you
// asked. "How often to look" described a looking nothing did, and "act on the
// best one without asking" described an asking that was the only way in.
//
// Harmless while it lasted — `selfaudit.rs` has no acting path of its own, so
// the switch could not have let anything through. That is luck, not design,
// and a safety switch nothing reads is the one you find out about on the day
// something does read it.

fn a_fault() -> atlas::selfaudit::Signal {
    // Enough to clear its own bar: a route that fails most times it is tried.
    atlas::selfaudit::Signal {
        kind: atlas::selfaudit::Kind::RouteFails,
        subject: "opening the trading app".into(),
        seen: 9,
        of: 10,
        example: "yesterday".into(),
    }
}

#[test]
fn how_often_it_looks_at_itself_is_the_number_you_set() {
    use atlas::selfaudit::{time_to_look, SelfAuditConfig};
    let day = 86_400u64;
    let mut cfg = SelfAuditConfig::default();
    cfg.enabled = true;
    cfg.every_days = 7;

    let now = 100 * day;
    // Six days after the last look is not seven.
    assert!(!time_to_look(&cfg, now - 6 * day, now));
    assert!(time_to_look(&cfg, now - 7 * day, now));

    // The number is read, not a literal that happens to equal it.
    cfg.every_days = 30;
    assert!(!time_to_look(&cfg, now - 7 * day, now), "the cadence is still a hardcoded week");
    assert!(time_to_look(&cfg, now - 30 * day, now));

    // Off means off, whatever the cadence says.
    cfg.enabled = false;
    assert!(!time_to_look(&cfg, 0, now));
    cfg.enabled = true;
    // And zero means never, rather than every tick.
    cfg.every_days = 0;
    assert!(!time_to_look(&cfg, 0, now));

    // Never having looked is overdue, not recent. A clock that moved
    // backwards — a laptop asleep across a timezone — is due rather than
    // locked out until real time catches up.
    cfg.every_days = 7;
    assert!(time_to_look(&cfg, 0, now));
    assert!(time_to_look(&cfg, now + day, now));

    // The shipped file still says so.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("every_days:"), "the cadence is not in the shipped file");
    assert_eq!(tools().self_audit.every_days, 7);
    // On in the shipped file since Eric's rulings E1 and E2 (25 Sep 2026):
    // Atlas fixes its own things and starts on its own findings.
    assert!(tools().self_audit.enabled, "looking at itself ships on, by Eric's ruling E2");
}

#[test]
fn telling_it_not_to_ask_is_the_difference_between_a_question_and_a_start() {
    use atlas::selfaudit::{recommend, unprompted, SelfAuditConfig};
    let recs = recommend(&[a_fault()], 3);
    assert!(!recs.is_empty(), "the fixture must clear its own bar or this tests nothing");

    let mut cfg = SelfAuditConfig::default();
    cfg.enabled = true;

    // The default: it asks, and starts nothing.
    assert!(!cfg.act_without_asking);
    let asked = unprompted(&cfg, &recs).expect("a fault is worth saying");
    assert!(asked.goal.is_none(), "it started work without being told it could");
    assert!(asked.said.contains("Want me to have a go?"), "{}", asked.said);

    // Told otherwise: it starts, and still says what it is doing and how to
    // stop it. Acting without asking is not the same as acting without
    // saying.
    cfg.act_without_asking = true;
    let acting = unprompted(&cfg, &recs).expect("a fault is worth saying");
    assert_eq!(acting.goal.as_deref(), Some(recs[0].symptom.as_str()));
    assert!(acting.said.contains("Say stop"), "{}", acting.said);
    assert!(!acting.said.contains("Want me to have a go?"), "{}", acting.said);

    // Nothing found is nothing said, either way. A weekly "nothing about
    // myself I'd change" is a weekly interruption carrying no information.
    assert!(unprompted(&cfg, &[]).is_none());
    cfg.act_without_asking = false;
    assert!(unprompted(&cfg, &[]).is_none());

    // The shipped file still offers the switch, and since Eric's ruling E2
    // (25 Sep 2026) ships it on: it starts on its own findings, and the
    // change still lands only on his OK.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("act_without_asking:"));
    assert!(tools().self_audit.act_without_asking);
}

#[test]
fn the_daemon_is_what_looks_on_the_cadence_rather_than_a_test() {
    // The two tests above would pass with `due` and `unprompted` called by
    // nothing. This is the half that makes them a setting: the tick reads
    // both, on the clock kept in the store rather than in memory — a cadence
    // of days in a process restarted daily would otherwise still do nothing.
    let src = live_source("src/daemon.rs");
    assert!(
        src.contains("selfaudit::time_to_look(&acfg"),
        "nothing in the daemon asks whether it is time to look"
    );
    assert!(
        src.contains("selfaudit::unprompted(&acfg"),
        "the daemon looks and then does nothing with what it found"
    );
    assert!(
        src.contains("selfaudit::LOOK_RECORD"),
        "the clock is not kept anywhere, so every restart resets the cadence"
    );
    assert!(
        src.contains("self.work_on_myself(&goal)"),
        "act_without_asking says it acts and nothing opens the work"
    );
}

// ---------- signin.fill_without_asking ----------
//
// The field was dead, and the hole under it was bigger than the field.
// `Access::may_fill` -- the lookalike-domain check this module exists for --
// had no caller anywhere in the running program. `Intent::SignIn` announced
// "signing you into X as Y" having checked only that the feature was on: not
// the grant, not `Allowed::Nothing`, not the vault.

fn granted(domain: &str) -> atlas::signin::Access {
    let mut a = atlas::signin::Access::default();
    a.grant(domain, "eric", "mine", atlas::signin::Allowed::SignIn, "entry", 1000);
    a
}

#[test]
fn the_check_that_saves_you_from_a_lookalike_domain_is_actually_reachable() {
    use atlas::signin::{Access, Allowed, Refused, SignInConfig};
    let mut cfg = SignInConfig::default();
    cfg.enabled = true;

    let a = granted("mybank.com");
    // The whole point of the module, and it runs without needing a browser.
    assert!(a.may_start("mybank.com", true, true, &cfg).is_ok());
    assert!(matches!(
        a.may_start("mybank.com.evil.co", true, true, &cfg),
        Err(Refused::NotGranted(_))
    ));
    // Subdomains of the same registered domain are the same account.
    assert!(a.may_start("login.mybank.com", true, true, &cfg).is_ok());

    // A locked vault and an empty chair each stop it, and say which.
    assert_eq!(a.may_start("mybank.com", false, true, &cfg), Err(Refused::Locked));
    assert_eq!(a.may_start("mybank.com", true, false, &cfg), Err(Refused::YouAreNotHere));
    cfg.enabled = false;
    assert_eq!(a.may_start("mybank.com", true, true, &cfg), Err(Refused::Disabled));
    cfg.enabled = true;

    // A grant of nothing is not a grant.
    let mut nothing = Access::default();
    nothing.grant("mybank.com", "eric", "mine", Allowed::Nothing, "entry", 1000);
    assert!(matches!(
        nothing.may_start("mybank.com", true, true, &cfg),
        Err(Refused::NotGranted(_))
    ));

    // `may_fill` still answers the same way about the same things, plus the
    // two questions only a browser can answer.
    assert!(a.may_fill("mybank.com", true, true, true, true, &cfg).is_ok());
    assert_eq!(
        a.may_fill("mybank.com", false, true, true, true, &cfg),
        Err(Refused::NotALoginPage)
    );
    assert_eq!(
        a.may_fill("mybank.com", true, false, true, true, &cfg),
        Err(Refused::ArrivedBadly)
    );
}

#[test]
fn whether_it_asks_before_signing_you_in_is_the_switch_you_set() {
    use atlas::signin::{Access, SignInConfig};
    let mut cfg = SignInConfig::default();
    // Shipped: it does not ask.
    assert!(cfg.fill_without_asking);
    assert!(!Access::asks_first(&cfg));
    cfg.fill_without_asking = false;
    assert!(Access::asks_first(&cfg));

    assert!(tools().signin.fill_without_asking, "the shipped file still ships it on");
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("fill_without_asking:"));

    // And the daemon is what reads both, rather than a test.
    let src = live_source("src/daemon.rs");
    assert!(
        src.contains("self.access.may_start(where_"),
        "Intent::SignIn still announces a sign-in without checking the grant"
    );
    assert!(
        src.contains("signin::Access::asks_first(&cfg)"),
        "nothing decides between announcing and asking"
    );
}

// ---------- remote.confirm_side_effects ----------
//
// Ships **on** -- "ask before running something that changes things" -- and
// nothing read it. `atlas remote start 3` marked a request running with no
// look at what the request was.

#[test]
fn a_request_that_is_not_obviously_a_read_asks_before_the_other_machine_runs_it() {
    use atlas::remote::{asking_before_it_runs, needs_your_yes, reading_or_change, Looks, RemoteConfig};
    let cfg = RemoteConfig::default();
    assert!(cfg.confirm_side_effects, "it ships on, which is why being inert mattered");

    // Reads go through.
    for what in ["check the render finished", "what's in the outbox", "list the open files"] {
        assert_eq!(reading_or_change(what), Looks::LikeAReading, "{what}");
        assert!(!needs_your_yes(what, &cfg), "{what}");
    }
    // Anything else is a change until you say otherwise -- policy.rs's rule,
    // borrowed rather than reinvented: unclassified is not safe.
    for what in ["render the draft", "delete the old exports", "send the invoice"] {
        assert_eq!(reading_or_change(what), Looks::Unknown, "{what}");
        assert!(needs_your_yes(what, &cfg), "{what}");
    }
    // An opening word is not a way past the gate.
    assert_eq!(reading_or_change("check the exports and delete the old ones"), Looks::Unknown);
    // Nor is a word that merely starts with one.
    assert_eq!(reading_or_change("checkout the branch"), Looks::Unknown);

    // Turned off, nothing is gated -- which is the other half of it being a
    // setting rather than a behaviour.
    let mut off = RemoteConfig::default();
    off.confirm_side_effects = false;
    assert!(!needs_your_yes("delete the old exports", &off));

    // What it says instead of running names the way through and the way out.
    let said = asking_before_it_runs(3, "delete the old exports");
    assert!(said.contains("atlas remote start 3 --yes"), "{said}");
    assert!(said.contains("confirm_side_effects"), "{said}");

    // And `atlas remote start` is what consults it.
    let src = live_source("src/main.rs");
    assert!(
        src.contains("remote::needs_your_yes(&r.what, &rcfg)"),
        "the start command still runs whatever the text says"
    );
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("confirm_side_effects:"));
}

// ---------- household.device_name ----------
//
// Three fields, one wiring and two deletions. `household` was the household's
// id in a text file, next to the one in the store that is actually used -- a
// second place to put an identity, and the one people copy between machines.
// `discoverable` was a switch for announcing yourself on a local network,
// which nothing here does. Neither had a reader, and both read as behaviour.

#[test]
fn the_name_you_gave_this_machine_is_the_one_the_join_box_offers() {
    // Joining is done on a phone, typing a ten-character code. Being asked to
    // also type a machine name there, when you already wrote it in your
    // config, is the kind of small friction that gets a feature abandoned.
    let page = sync_page_unplaced(true, "D:/atlas", None, None, None, "the study laptop");
    assert!(page.contains("name=device"), "the join box is gone");
    assert!(page.contains("value=\"the study laptop\""), "it isn't filled in");

    // Nothing set means nothing filled in, rather than the word "None".
    let blank = sync_page_unplaced(true, "D:/atlas", None, None, None, "");
    assert!(blank.contains("value=\"\""), "an empty name printed something");

    // A name is somebody's words, so it is escaped like any of them.
    let sneaky = sync_page_unplaced(true, "D:/atlas", None, None, None, "a\"><script>x");
    assert!(!sneaky.contains("<script>x"), "a device name got into the page as markup");

    // And the join handler is what falls back to it.
    let src = live_source("src/hublive.rs");
    assert!(
        src.contains("self.tools_cfg().household.device_name"),
        "nothing reads the name you set"
    );
    assert!(
        src.contains("let this_machine = if device.trim().is_empty()"),
        "the form's answer no longer wins over the config"
    );

    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("device_name:"));
}

#[test]
fn the_two_household_fields_that_described_nothing_are_gone_and_are_named() {
    // Deleting a setting is only honest if a file that still sets it is told,
    // rather than having it ignored in silence.
    let said = atlas::config::settings_that_do_nothing(
        "household:\n  household: \"abc123\"\n  discoverable: true\n  device_name: \"laptop\"\n",
    );
    let named: Vec<&str> = said.iter().map(|(k, _)| *k).collect();
    assert!(named.contains(&"household.household"), "{named:?}");
    assert!(named.contains(&"household.discoverable"), "{named:?}");
    // And the field that does work is not swept up with them.
    assert!(!named.contains(&"household.device_name"), "{named:?}");
    assert!(!named.contains(&"household"), "the section itself is read: {named:?}");

    // The reasons say what to do instead, rather than just "ignored".
    let why: String = said.iter().map(|(_, w)| *w).collect::<Vec<_>>().join(" ");
    assert!(why.contains("lives in the store"), "{why}");
    assert!(why.contains("server.reachable_from"), "{why}");

    // An old file still loads -- the whole section, not just the known keys.
    let cfg: atlas::household::HouseholdConfig = serde_yaml::from_str(
        "household: \"abc123\"\ndiscoverable: false\ndevice_name: \"the study laptop\"\n",
    )
    .expect("an old file must still load");
    assert_eq!(cfg.device_name, "the study laptop");

    // The shipped file stopped offering them.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(!raw.contains("discoverable: true"), "still shipped as a switch");
}

// ---------- workspace.default_view / .keep_done_days / .group_by_project ----------
//
// The worst version of this defect. `WorkspaceConfig` had no field anywhere
// in `ToolsConfig` and no block in `tools.yaml`, so none of the three could be
// set -- and `workspace_page_live` took `views.first()`, which happens to be
// "Now", which is also what `default_view` ships as. The hardcoded behaviour
// and the shipped default agreed, so nothing looked wrong to anyone except
// the one person who tried to change it.

fn an_item(title: &str, closed: Option<u64>) -> atlas::workspace_view::Item {
    atlas::workspace_view::Item {
        id: title.into(),
        title: title.into(),
        kind: atlas::workspace_view::Kind::Task,
        status: if closed.is_some() {
            atlas::workspace_view::Status::Done
        } else {
            atlas::workspace_view::Status::Doing
        },
        due: None,
        project: Some("the house".into()),
        client: None,
        links: Vec::new(),
        blocked_by: None,
        from: atlas::workspace_view::Origin::YouSaid,
        at: 0,
        closed_at: closed,
        tags: Vec::new(),
        estimate_mins: None,
        spent_mins: 0,
        atlas_could: atlas::workspace_view::Handoff::Unknown,
        thinking: Vec::new(),
    }
}

#[test]
fn the_view_the_dashboard_opens_on_is_the_one_you_named() {
    use atlas::workspace_view::{is_a_view, pick, shipped};
    let views = shipped();
    assert!(views.len() > 1, "one view would make this test vacuous");

    // The shipped default and the old hardcoded `views.first()` agree, which
    // is exactly why nothing caught this.
    assert_eq!(pick(&views, "Now").unwrap().name, "Now");
    assert_eq!(views[0].name, "Now");

    // A different name gets a different view -- the half that was broken.
    let other = views[1].name.clone();
    assert_eq!(pick(&views, &other).unwrap().name, other);
    assert_eq!(pick(&views, &other.to_uppercase()).unwrap().name, other);

    // A name that matches nothing draws the usual thing rather than refusing,
    // and says so -- silently showing a different view than the one somebody
    // wrote down is how they conclude the dashboard is broken.
    assert_eq!(pick(&views, "Everything, obviously").unwrap().name, "Now");
    assert!(!is_a_view(&views, "Everything, obviously"));
    assert!(is_a_view(&views, "  now  "), "a stray space is not a typo");
}

#[test]
fn how_long_a_finished_thing_stays_on_the_dashboard_is_yours() {
    use atlas::workspace_view::still_worth_showing;
    let day = 86_400u64;
    let now = 100 * day;
    let live = an_item("still going", None);
    let recent = an_item("done last week", Some(now - 7 * day));
    let old = an_item("done last month", Some(now - 30 * day));
    let all: Vec<&atlas::workspace_view::Item> = vec![&live, &recent, &old];

    let kept = still_worth_showing(&all, 14, now);
    assert_eq!(kept.len(), 2, "{:?}", kept.iter().map(|i| &i.title).collect::<Vec<_>>());
    assert!(kept.iter().any(|i| i.title == "done last week"));
    assert!(!kept.iter().any(|i| i.title == "done last month"));

    // The number is read rather than a fortnight that happens to equal it.
    assert_eq!(still_worth_showing(&all, 60, now).len(), 3);
    // Zero hides them as they close, which is a real answer.
    assert_eq!(still_worth_showing(&all, 0, now).len(), 1);
    // Nothing unfinished is ever hidden by this.
    assert!(still_worth_showing(&all, 0, now).iter().all(|i| i.closed_at.is_none()));
    // A close time in the future is a clock that moved, not an item from
    // tomorrow -- shown, because the failure here should always be too much.
    let future = an_item("clock moved", Some(now + day));
    assert_eq!(still_worth_showing(&[&future], 0, now).len(), 1);
}

#[test]
fn rolling_items_up_into_their_project_is_a_switch_that_moves_them() {
    use atlas::workspace_view::{grouping, Group};
    // The preference is about the dashboard rather than about one view, so it
    // wins -- except over a view that already groups by project, where there
    // is nothing to win.
    assert_eq!(grouping(Group::Status, true), Group::Project);
    assert_eq!(grouping(Group::Project, true), Group::Project);
    assert_eq!(grouping(Group::Status, false), Group::Status);
    assert_eq!(grouping(Group::Nothing, false), Group::Nothing);
}

#[test]
fn the_dashboard_is_what_reads_all_three_rather_than_these_tests() {
    let src = live_source("src/hublive.rs");
    assert!(
        !src.contains("views.first().cloned().unwrap_or_else"),
        "the dashboard still hardcodes the first shipped view"
    );
    for reads in [
        "workspace_view::pick(&views, &wcfg.default_view)",
        "still_worth_showing(&items, wcfg.keep_done_days, now)",
        "grouping(view.grouped_by, wcfg.group_by_project)",
    ] {
        assert!(src.contains(reads), "the dashboard doesn't read it: {reads}");
    }

    // There is a block for them to arrive in now, and it lands in a field.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("\nworkspace:"), "no workspace block is shipped");
    let parsed: atlas::voice::ToolsConfig =
        serde_yaml::from_str("workspace:\n  default_view: Stuck\n  keep_done_days: 3\n")
            .expect("it parses");
    assert_eq!(parsed.workspace.default_view, "Stuck");
    assert_eq!(parsed.workspace.keep_done_days, 3);
    assert_eq!(tools().workspace.default_view, "Now", "the shipped file still opens on Now");
}

// ---------- reference.warn_when_stale ----------
//
// Wiring this turned up something worse than the dead setting.
// `reference::nothing_found` is the sentence Atlas says instead of inventing
// a number, and it answered "my futures contract specs covers tick size, tick
// value..." about a shelf that has never been fetched -- every entry in
// `worth_having` ships with `as_of` empty, because nothing here fetches
// reference material at all.

#[test]
fn it_does_not_claim_to_hold_a_shelf_it_has_never_fetched() {
    use atlas::reference::{chosen, nothing_found, worth_having, ReferenceConfig};

    // Nothing is held today. That is the honest state, and the sentence has
    // to match it.
    assert!(
        worth_having().iter().all(|s| !s.held()),
        "something claims to have been fetched -- has a fetcher been written?"
    );

    let cfg = ReferenceConfig { enabled: true, ..Default::default() };
    let shelves = chosen(&cfg);
    assert!(!shelves.is_empty(), "the shelf list is empty, so this tests nothing");
    let said = nothing_found("what is the tick value", &shelves, cfg.warn_when_stale, 0);
    assert!(
        !said.contains("I could reason from that"),
        "it offered to reason from a shelf it hasn't got: {said}"
    );
    assert!(said.contains("haven't got one"), "{said}");
    assert!(said.contains("would be invented"), "{said}");

    // A shelf that *is* held reads the old way, and says how old it is when
    // you have asked to be told.
    let mut held = shelves[0].clone();
    held.as_of = "2020-01-01".into();
    held.stale_after_days = 180;
    let now = 86_400 * 20_000; // well past 2020
    let warned = nothing_found(&held.covers.clone(), &[held.clone()], true, now);
    assert!(warned.contains("I could reason from that"), "{warned}");
    assert!(warned.contains("days old") || warned.contains("a bit old"), "{warned}");

    // Off means it does not say -- the half that makes it a setting.
    let quiet = nothing_found(&held.covers.clone(), &[held.clone()], false, now);
    assert!(quiet.contains("I could reason from that"), "{quiet}");
    assert!(!quiet.contains("days old"), "{quiet}");

    // An unreadable date is not zero days old.
    let mut bad = held.clone();
    bad.as_of = "sometime last year".into();
    assert_eq!(bad.days_old(now), None);
    assert!(!bad.held() || bad.days_old(now).is_none());

    // And the daemon is what passes the setting in.
    let src = live_source("src/daemon.rs");
    assert!(
        src.contains("cfg.warn_when_stale"),
        "nothing reads the setting on the way to the answer"
    );
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("warn_when_stale:"));
}

// A peer named by address syncs straight across, off the local network.
//
// The same-network path only reaches a peer that answered a LAN broadcast. A
// phone away from home, or a rack Atlas on a tailnet, is never LAN-discovered —
// so before this it could only sync through the folder, slowly and only when
// both were next on it. `elsewhere.known[].host` is a stable address (a tailnet
// 100.x on the WireGuard/Tailscale path already in the tree); the daemon now
// dials it directly on the sync port. This stands a real socket up as that
// peer, points the daemon at it by address alone, and proves the bundle crosses
// straight across — not through the folder.
#[test]
fn a_peer_named_by_address_syncs_straight_across_off_the_local_network() {
    use atlas::daemon::Daemon;
    use atlas::elsewhere::{Elsewhere, ElsewhereConfig};
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::store::Store;
    use atlas::sync::{make_bundle, read_bundle, Log, What};
    use atlas::transport::Server;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    // The other Atlas: a real socket on an OS-chosen port, standing in for a
    // tailnet address. The daemon never learns this from a broadcast — only
    // from the configured `host`/`sync_port`, which is the whole point.
    let peer_server = Server::bind_local_ephemeral().expect("peer listens");
    let addr = peer_server.local_addr().expect("addr");

    // What the peer hands back (one event this machine has never seen), and a
    // slot to capture what it received.
    let mut peer_log = Log::new("homelab");
    peer_log.append(
        What::Captured { id: "peer-note".into(), text: "from the far side".into() },
        500,
    );
    let mut peer_bundle = make_bundle(&peer_log, "homelab", 0, 600);
    peer_bundle.belongs_to = "personal".into();
    let reply = serde_json::to_vec(&peer_bundle).unwrap();

    let got: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let got_w = got.clone();
    let srv = std::thread::spawn(move || loop {
        let served = peer_server
            .poll(Duration::from_secs(3), |incoming| {
                *got_w.lock().unwrap() = Some(incoming);
                reply.clone()
            })
            .unwrap();
        if served {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    });

    // This machine.
    let root = std::env::temp_dir().join(format!("atlas-tailnet-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let folder = root.join("carrier");
    std::fs::create_dir_all(&folder).expect("carrier");

    let mut c = Config::load(Path::new("config")).expect("config/ loads");
    {
        let t = c.tools.as_mut().expect("tools.yaml");
        t.sync.enabled = true;
        t.sync.encrypt_bundles = false; // plaintext, so the assert can read the wire
        t.sync.belongs_to = "personal".into();
        t.sync.folder = folder.to_string_lossy().into_owned();
        t.elsewhere = ElsewhereConfig {
            enabled: true,
            timeout_secs: 5,
            known: vec![Elsewhere {
                name: "homelab".into(),
                host: addr.ip().to_string(),
                port: 8787,
                sync_port: Some(addr.port()),
                token: "a-token-long-enough-to-be-real".into(),
            }],
        };
    }

    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(root.join("state")),
        Proactive::new(ProactiveConfig::default()),
    );

    d.turn("note that the rack invoice is due friday", 100);
    let said = d.turn("sync", 200);
    srv.join().unwrap();

    // 1) The dial fired, the peer answered, and the far-side note came back.
    assert!(
        said.contains("Synced straight across to homelab"),
        "the configured-address dial didn't report success:\n{said}"
    );
    assert!(
        said.contains("took in 1"),
        "the peer's note should have been taken in:\n{said}"
    );

    // 2) This machine's own capture really crossed the socket — by address, not
    //    through the folder.
    let received = got.lock().unwrap().take().expect("the peer got dialed at all");
    let text = String::from_utf8(received).expect("utf8 bundle");
    let crossed = read_bundle(&text, None).expect("a plaintext bundle");
    assert!(
        crossed
            .events
            .iter()
            .any(|e| format!("{:?}", e.what).to_lowercase().contains("invoice")),
        "the capture didn't cross the wire:\n{text}"
    );

    let _ = std::fs::remove_dir_all(&root);
}


/// The Sync page drawn without knowing where this device stands -- what
/// `hub::sync_page` did before the running Atlas moved to `sync_page_with`
/// and the wrapper, called by nothing else, was folded in (28 Sep 2026).
fn sync_page_unplaced(sealing: bool, folder: &str, phrase: Option<&str>, card: Option<&str>, last: Option<&str>, this_device: &str) -> String {
    atlas::hub::sync_page_with(
        sealing,
        folder,
        phrase,
        card,
        last,
        this_device,
        &atlas::hub::SyncView { house: atlas::hub::HouseView::Unknown, suggested_folder: None },
    )
}
