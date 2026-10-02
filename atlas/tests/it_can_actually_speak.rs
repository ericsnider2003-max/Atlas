//! A fresh install can answer out loud.
//!
//! The failure this exists to stop: `config/tools.yaml` named
//! `tts_engine.engine: kokoro` with `exe: tools/kokoro/speak.cmd`, and
//! `ATLAS.bat` downloaded **piper**. Nothing downloaded the Kokoro wrapper —
//! `tools.yaml` says in its own comments that you write it yourself. So on a
//! machine that had just run setup, Atlas could hear you and had no program
//! to answer with.
//!
//! Nothing caught it, for three separate reasons, and each is fixed:
//!
//! * `ATLAS.bat`'s first-run check tested two whisper files and nothing on
//!   the speaking side, so setup "finished" and the menu stopped offering it.
//! * `atlas doctor` checked the *voice file* and never the engine's own
//!   executable, so a missing `speak.cmd` reported nothing wrong.
//! * No test compared the shipped config against what the installer fetches.
//!   That is this file.
//!
//! The rule: **the engine the shipped config names must be one the shipped
//! installer actually delivers.** Kokoro and Chatterbox stay as a documented
//! upgrade — an engine you choose and install, not one you are given and
//! then find missing.

use atlas::tts::Engine;
use std::fs;

fn tools_yaml() -> String {
    fs::read_to_string("config/tools.yaml").expect("config/tools.yaml")
}

fn launcher() -> String {
    fs::read_to_string("ATLAS.bat").expect("ATLAS.bat")
}

/// Only the lines that actually **fetch** something.
///
/// The first version of this guard searched the whole of `ATLAS.bat`, and it
/// passed a deliberate mutation back to the broken Kokoro config — because
/// the file's own first-run *check* line names `tools\kokoro\speak.cmd`, and
/// "the launcher mentions this file" is not "the launcher downloads this
/// file". That is the sixth time a guard in this tree has been green for a
/// reason that had nothing to do with the code, and it was mine.
///
/// Until 23 Sep 2026 downloads happened through `call :get_zip` and
/// `call :get_file` in ATLAS.bat. The voice pieces now come from Atlas's own
/// pinned catalogue (`getpieces`), which the launcher calls.
fn what_setup_downloads() -> String {
    // Since 23 Sep 2026 the fetching is Atlas's own (`atlas get`, the same
    // code the setup window runs), and the launcher's setup only calls it —
    // the PowerShell downloads that lived here could not have worked. So the
    // list is the pinned catalogue itself, and the launcher is checked to
    // hand off to it rather than to download anything of its own.
    let bat = launcher();
    assert!(
        bat.contains("\"%EXE%\" get"),
        "ATLAS.bat's setup no longer hands fetching to `atlas get`, so this list \
         is not what a fresh install downloads"
    );
    let mut out = String::new();
    for p in atlas::getpieces::catalogue() {
        out.push_str(p.url);
        out.push('\n');
        out.push_str(p.key_path());
        out.push('\n');
    }
    assert!(
        out.contains("whisper"),
        "the download list is empty, and this guard is now measuring an empty string"
    );
    out
}

fn shipped() -> atlas::config::Config {
    atlas::config::Config::load(std::path::Path::new("config")).expect("the shipped config")
}

#[test]
fn the_shipped_engine_is_the_one_the_installer_downloads() {
    let cfg = shipped();
    let tools = cfg.tools.as_ref().expect("tools.yaml");
    let eng = &tools.tts_engine;

    assert!(
        eng.is_consistent(),
        "the shipped config says the engine is {} and the executable is {:?} — \
         those disagree, and that fault only ever shows up as silence",
        eng.engine.name(),
        eng.exe
    );

    // The executable named in the config has to be something `:setup`
    // actually fetches. Matched on the file name so a path change does not
    // quietly defeat it.
    let exe_name = eng.exe.rsplit(['/', '\\']).next().unwrap_or(&eng.exe);
    let fetched = what_setup_downloads();
    assert!(
        fetched.contains(exe_name),
        "the shipped config drives speech with {exe_name}, and ATLAS.bat never \
         downloads it. A fresh install would hear you and have nothing to \
         answer with."
    );
}

#[test]
fn the_shipped_voice_is_a_file_the_installer_fetches() {
    let cfg = shipped();
    let tools = cfg.tools.as_ref().expect("tools.yaml");
    let id = &tools.voice_settings.voice;
    assert!(!id.is_empty(), "no voice is chosen, so speech has nothing to load");

    // `voice_file_for` applies the engine's own extension — .onnx for piper,
    // .pt for kokoro, .wav for chatterbox. Getting that pairing wrong is the
    // other way this goes silent.
    // Kokoro first (Phase 0.8): its voices come inside its own download,
    // which setup fetches, and piper's default voice -- what speaks until
    // Kokoro is here -- must be fetched too.
    if tools.tts_engine.engine == atlas::tts::Engine::Kokoro {
        assert!(atlas::kokoro::speaker_id(id).is_some(), "{id:?} isn't one of Kokoro's voices");
        let pieces: Vec<String> = atlas::getpieces::setup_pieces().iter().map(|p| p.key_path().to_string()).collect();
        assert!(pieces.iter().any(|k| k.contains("kokoro")), "setup doesn't fetch Kokoro: {pieces:?}");
        let fallback = atlas::tts::VoiceSettings::default().voice;
        assert!(what_setup_downloads().contains(fallback.as_str()), "piper's voice {fallback:?} isn't fetched");
        return;
    }
    let file = tools.tts_engine.voice_file_for(id);
    let name = file.rsplit(['/', '\\']).next().unwrap_or(&file).to_string();
    assert!(
        what_setup_downloads().contains(id.as_str()),
        "the shipped voice is {id:?} and ATLAS.bat downloads no such voice. \
         The engine would start and find nothing to speak with. (resolved \
         file: {name})"
    );
}

#[test]
fn the_launcher_checks_the_speaking_half_before_it_calls_setup_done() {
    // It tested `models\ggml-base.en.bin` and `tools\whisper\whisper-cli.exe`
    // and nothing else. On a machine where those two arrived and the speech
    // download failed, FIRSTRUN went false, the menu stopped offering setup,
    // and Atlas was permanently mute with no prompt to fix it.
    let bat = launcher();
    let firstrun: String = bat
        .lines()
        .filter(|l| l.contains("FIRSTRUN=1"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        firstrun.contains("piper") || firstrun.contains("kokoro") || firstrun.contains("speak"),
        "the first-run check tests only the hearing half, so setup can report \
         itself finished on an Atlas that cannot speak:\n{firstrun}"
    );
}

#[test]
fn doctor_reports_a_missing_speech_program_and_not_only_a_missing_voice() {
    // The check that was absent. `atlas doctor` verified the voice file and
    // never the engine's executable, so the shipped Kokoro config — whose
    // `speak.cmd` nothing installs — reported nothing wrong at all.
    let doctor = fs::read_to_string("src/doctor.rs").expect("src/doctor.rs");
    let flat: String = doctor.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("eng.exe") || flat.contains("exe_path"),
        "doctor never looks at the speech engine's own executable"
    );
    assert!(
        flat.contains("isn't there"),
        "doctor has no sentence for a speech engine whose program is missing"
    );
}

#[test]
fn every_engine_can_still_be_chosen_and_says_what_it_needs() {
    // Switching is a config edit, not a code change — that is the whole point
    // of `tts_engine`. Shipping piper must not have quietly broken the
    // others; the upgrade path has to stay real or the decision to ship the
    // floor becomes a decision to have only the floor.
    for (engine, exe, ext) in [
        (Engine::Piper, "tools/piper/piper.exe", "onnx"),
        (Engine::Kokoro, "tools/kokoro/speak.cmd", "pt"),
        (Engine::Chatterbox, "tools/chatterbox/speak.cmd", "wav"),
    ] {
        let cfg = atlas::tts::EngineConfig {
            engine,
            exe: exe.into(),
            voices_dir: "models".into(),
        };
        assert!(cfg.is_consistent(), "{} no longer accepts its own executable", engine.name());
        assert!(
            cfg.voice_file_for("some-voice").ends_with(ext),
            "{} stopped asking for .{ext} voice files",
            engine.name()
        );
        assert!(!engine.name().is_empty());
    }
}

#[test]
fn the_config_still_documents_the_upgrade_it_no_longer_ships() {
    // Shipping the floor is only defensible if the way up is written down.
    // The previous edition of this file had it backwards — it shipped Kokoro
    // and documented "if you want piper back".
    let y = tools_yaml();
    assert!(
        y.contains("kokoro") && y.contains("chatterbox"),
        "the better engines are no longer mentioned, so the default became the \
         only option"
    );
    assert!(
        y.to_lowercase().contains("you write"),
        "nothing tells you that the upgrade engines need a wrapper you write \
         yourself — which is the reason they are not the default"
    );
}
