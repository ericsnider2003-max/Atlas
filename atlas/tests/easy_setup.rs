//! Double-click and it's set up (23 Sep 2026): the settings travel inside the
//! program, Atlas picks its own home, fetches and checks its own pieces, and
//! walks the rest in its own window.

use atlas::firstlaunch::{
    atlas_running, desktop_entry, is_set_up, mark_set_up, move_in_over, settings_missing, where_to_live,
    write_default_config, Where, DEFAULT_CONFIG,
};
use atlas::getpieces::{catalogue, fetch, have, plain_download_error, Lands, Piece, Tools, Unzip};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-easy-setup-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

// ================= the settings travel inside the program =================

#[test]
fn a_copy_with_no_settings_writes_the_shipped_ones_and_they_load() {
    let dir = tmp("defaults").join("config");
    assert!(settings_missing(&dir));
    let wrote = write_default_config(&dir).unwrap();
    assert_eq!(wrote, DEFAULT_CONFIG.len());
    assert!(!settings_missing(&dir));
    // The embedded settings are the whole set: Atlas starts from them alone.
    let cfg = atlas::config::Config::load(&dir).expect("the built-in settings must load on their own");
    assert!(cfg.tools.is_some(), "tools.yaml came out too");
    // And they are the shipped ones, byte for byte.
    for (name, text) in DEFAULT_CONFIG {
        let shipped = std::fs::read_to_string(Path::new("config").join(name)).unwrap();
        assert_eq!(&shipped, text, "{name} built into the program differs from config/{name}");
    }
}

#[test]
fn your_own_settings_are_never_overwritten() {
    let dir = tmp("mine").join("config");
    write_default_config(&dir).unwrap();
    std::fs::write(dir.join("tools.yaml"), "# mine\n").unwrap();
    assert_eq!(write_default_config(&dir).unwrap(), 0, "nothing missing, nothing written");
    assert_eq!(std::fs::read_to_string(dir.join("tools.yaml")).unwrap(), "# mine\n");
}

// ================= Atlas picks its own home =================

#[test]
fn where_atlas_lives() {
    let downloads = Path::new("/somewhere/Downloads");
    let home = Path::new("/somewhere/AppData/Local/Atlas");
    assert_eq!(where_to_live(downloads, false, false, Some(home)), Where::MoveTo(home.to_path_buf()));
    assert_eq!(where_to_live(downloads, true, false, Some(home)), Where::Here, "a working install stays put");
    assert_eq!(where_to_live(downloads, false, true, Some(home)), Where::Here, "ATLAS_HOME is obeyed");
    assert_eq!(where_to_live(home, false, false, Some(home)), Where::Here, "already home");
    assert_eq!(where_to_live(downloads, false, false, None), Where::Here, "nowhere to go");
}

#[test]
fn moving_in_copies_the_program_and_writes_its_settings() {
    let base = tmp("move");
    let downloads = base.join("Downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let exe = downloads.join("atlas.exe");
    std::fs::write(&exe, b"pretend program").unwrap();
    let home = base.join("Atlas");
    let moved = move_in_over(&exe, &home, false, std::time::Duration::ZERO).unwrap();
    assert_eq!(moved, home.join(atlas::firstlaunch::INSTALLED_NAME));
    assert_eq!(std::fs::read(&moved).unwrap(), b"pretend program");
    assert!(exe.is_file(), "the one you double-clicked is left where it was");
    assert!(!settings_missing(&home.join("config")));
    assert!(atlas::roots::looks_like_an_install(&home), "the new home is an install by roots' own rule");
    // Again, as an update would: a newer copy replaces the old one.
    std::fs::write(&exe, b"newer program").unwrap();
    move_in_over(&exe, &home, false, std::time::Duration::ZERO).unwrap();
    assert_eq!(std::fs::read(home.join(atlas::firstlaunch::INSTALLED_NAME)).unwrap(), b"newer program");
    // Handed out as one file with a friendlier name, it still installs as Atlas.
    let setup = downloads.join("Atlas Setup.exe");
    std::fs::write(&setup, b"the download").unwrap();
    assert_eq!(move_in_over(&setup, &home, false, std::time::Duration::ZERO).unwrap(), home.join(atlas::firstlaunch::INSTALLED_NAME));
    assert_eq!(std::fs::read(home.join(atlas::firstlaunch::INSTALLED_NAME)).unwrap(), b"the download");
}

#[test]
fn set_up_is_remembered_and_running_is_measured() {
    let root = tmp("setup-mark");
    assert!(!is_set_up(&root));
    mark_set_up(&root).unwrap();
    assert!(is_set_up(&root));

    // 28 Sep 2026: "running" is Atlas's own lock (and its hub's own
    // answer), not whatever answers on the port. It used to be the port
    // alone, which read another program as Atlas and a switched-off hub as
    // Atlas stopped (`tests/the_hub_is_always_there.rs` has the rest).
    assert!(!atlas_running(&root), "no lock and no hub is not running");
    let lock = atlas::onlyone::OnlyOne::at(&root.join("data"));
    lock.take(atlas::store::now()).unwrap();
    assert!(atlas_running(&root), "a live lock is a running Atlas, hub or no hub");
    lock.release();
    let listening = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listening.local_addr().unwrap().port();
    atlas::server::record_door(&root.join("data").join("state"), &atlas::server::Door { port, id: "someone".into() }).unwrap();
    assert!(!atlas_running(&root), "something else on the port is not Atlas");
    drop(listening);
    assert!(!atlas_running(&root), "nothing answering is not running");
}

#[test]
fn the_linux_menu_entry_opens_the_window() {
    let e = desktop_entry(Path::new("/opt/atlas/atlas"));
    assert!(e.contains("Exec=\"/opt/atlas/atlas\" home"), "{e}");
    assert!(e.contains("Terminal=false"));
}

// ================= Atlas fetches and checks its own pieces =================

#[test]
fn every_piece_is_pinned() {
    let pieces = catalogue();
    assert!(pieces.len() >= 5);
    let mut names = std::collections::HashSet::new();
    for p in &pieces {
        assert!(names.insert(p.name), "two pieces called {}", p.name);
        assert!(p.url.starts_with("https://"), "{}", p.url);
        assert!(!p.url.contains("/latest/"), "{} follows 'latest', which is how the old address died", p.name);
        assert_eq!(p.sha256.len(), 64);
        assert!(p.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(p.bytes > 0);
        assert!(p.key_path().starts_with("tools/") || p.key_path().starts_with("models/"));
    }
    // The paths the rest of Atlas looks in.
    let keys: Vec<&str> = pieces.iter().map(|p| p.key_path()).collect();
    for want in ["tools/whisper/whisper-cli.exe", "models/ggml-base.en.bin", "tools/piper/piper.exe",
                 "models/en_US-amy-medium.onnx", "tools/ffmpeg/ffmpeg.exe"] {
        assert!(keys.contains(&want), "{want} is not fetched");
    }
}

fn can_run(cmd: &str, arg: &str) -> bool {
    std::process::Command::new(cmd).arg(arg).output().is_ok()
}

fn local_tools() -> Option<Tools> {
    (can_run("curl", "--version") && can_run("unzip", "-v"))
        .then(|| Tools { curl: "curl".into(), unzip: Unzip::Unzip, unzipper: "unzip".into() })
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[test]
fn a_piece_is_fetched_checked_and_put_in_place() {
    let Some(tools) = local_tools() else {
        eprintln!("curl or unzip missing here; skipping the real fetch");
        return;
    };
    let src = tmp("fetch-src");
    let body = b"a pretend listening model".to_vec();
    std::fs::write(src.join("model.bin"), &body).unwrap();
    let piece = Piece {
        name: "a test file",
        for_what: "testing",
        url: leak(format!("file://{}", src.join("model.bin").display())),
        sha256: leak(atlas::digest::sha256_hex(&body)),
        bytes: body.len() as u64,
        lands: Lands::File("models/test.bin"),
    };
    let root = tmp("fetch-root");
    let seen = std::cell::Cell::new(0u64);
    fetch(&piece, &root, &tools, &|done, _| seen.set(done)).unwrap();
    assert_eq!(std::fs::read(root.join("models/test.bin")).unwrap(), body);
    assert!(have(&piece, &root));
    assert_eq!(seen.get(), body.len() as u64, "progress reached the end");
    // Again: already here, nothing fetched.
    fetch(&piece, &root, &tools, &|_, _| {}).unwrap();
}

#[test]
fn a_download_that_is_not_the_right_file_is_thrown_away() {
    let Some(tools) = local_tools() else { return };
    let src = tmp("bad-src");
    std::fs::write(src.join("model.bin"), b"substituted").unwrap();
    let piece = Piece {
        name: "a test file",
        for_what: "testing",
        url: leak(format!("file://{}", src.join("model.bin").display())),
        sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        bytes: 11,
        lands: Lands::File("models/test.bin"),
    };
    let root = tmp("bad-root");
    let err = fetch(&piece, &root, &tools, &|_, _| {}).unwrap_err();
    assert!(err.contains("isn't the file it should be"), "{err}");
    assert!(!root.join("models/test.bin").exists(), "a wrong file was put in place");
    assert!(!have(&piece, &root));
}

#[test]
fn a_zip_is_unpacked_from_its_own_folder_into_atlas_tools() {
    let Some(tools) = local_tools() else { return };
    if !can_run("python3", "--version") {
        return;
    }
    let src = tmp("zip-src");
    let zip = src.join("engine.zip");
    // The shape of the real whisper zip: everything inside a `Release/` folder.
    let made = std::process::Command::new("python3")
        .arg("-c")
        .arg(format!(
            "import zipfile;z=zipfile.ZipFile(r'{}','w');z.writestr('Release/whisper-cli.exe','exe');z.writestr('Release/whisper.dll','dll');z.close()",
            zip.display()
        ))
        .status()
        .unwrap();
    assert!(made.success());
    let bytes = std::fs::read(&zip).unwrap();
    let piece = Piece {
        name: "the listening engine",
        for_what: "hearing you",
        url: leak(format!("file://{}", zip.display())),
        sha256: leak(atlas::digest::sha256_hex(&bytes)),
        bytes: bytes.len() as u64,
        lands: Lands::Zip { inside: "Release", dir: "tools/whisper", key: "tools/whisper/whisper-cli.exe" },
    };
    let root = tmp("zip-root");
    fetch(&piece, &root, &tools, &|_, _| {}).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("tools/whisper/whisper-cli.exe")).unwrap(), "exe");
    assert!(root.join("tools/whisper/whisper.dll").is_file(), "the whole folder came across");
    assert!(!root.join("tools/whisper/Release").exists(), "the wrapper folder is gone");
    assert!(have(&piece, &root));
}

#[test]
fn a_failed_download_says_what_to_do() {
    assert!(plain_download_error("the voice", "curl: (6) Could not resolve host: x").contains("doesn't seem to be online"));
    assert!(plain_download_error("the voice", "curl: (22) The requested URL returned error: 404").contains("no longer where"));
    assert!(plain_download_error("the voice", "curl: (28) Operation timed out").contains("try again"));
    assert!(plain_download_error("the voice", "").ends_with("the voice"));
}

// ================= the window walks the rest =================

#[test]
fn only_what_you_can_act_on_is_said() {
    use atlas::doctor::Finding;
    let f = |label: &str, ok: bool, detail: &str| Finding { label: label.into(), ok, detail: detail.into() };
    let findings = [
        f("app 'discord'", false, "not at C:/x. If it's on your taskbar..."),
        f("app 'chrome'", false, "not at C:/y."),
        f("settings that do nothing", false, "21 sections reach no code"),
        f("stt", false, "'tools/whisper/whisper-cli.exe' not on PATH. More words here."),
        f("disk", true, "20 GB free"),
        f("vision", false, "recognising what it sees is switched off in your settings"),
        f("hands", false, "I can't do finding your hands yet"),
    ];
    let said = atlas::setupwin::for_you_to_look_at(&findings, false);
    assert_eq!(said.len(), 3, "{said:?}");
    assert_eq!(said.iter().filter(|s| s.contains("camera features")).count(), 1, "optional extras are one line");
    assert!(said.iter().any(|s| s.starts_with("stt:") && !s.contains("More words")), "{said:?}");
    assert!(said.iter().any(|s| s.contains("discord, chrome") && s.contains("fine if you don't use them")));
    assert!(!said.iter().any(|s| s.contains("reach no code")), "Atlas's own settings backlog is not yours");
    // With a voice piece still missing, its consequences aren't said twice.
    let while_fetching = atlas::setupwin::for_you_to_look_at(&findings, true);
    assert_eq!(while_fetching.len(), said.len() - 1, "{while_fetching:?}");
}

#[test]
fn the_steps_run_to_the_end_and_say_so() {
    // An install that already has everything: the window's walk finishes,
    // every piece says it's already here, and the phone says why there's no
    // code when Tailscale isn't on this machine.
    let root = tmp("walk");
    write_default_config(&root.join("config")).unwrap();
    std::fs::write(root.join("config/machine.yaml"), "").unwrap();
    mark_set_up(&root).unwrap(); // not the first time: no shortcuts made in a test
    // Everything the setup fetches: the voice pieces and Tor.
    for p in atlas::setupwin::setup_pieces() {
        let path = root.join(p.key_path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let f = std::fs::File::create(&path).unwrap();
        if let Lands::File(_) = p.lands {
            f.set_len(p.bytes).unwrap();
        } else {
            // What a finished unpack leaves (28 Sep 2026): the key file, and
            // the mark saying all of it arrived.
            f.set_len(1).unwrap();
            std::fs::write(atlas::getpieces::marker_path(&p, &root).unwrap(), p.sha256).unwrap();
        }
    }
    let place = atlas::setupwin::Place {
        root: root.clone(),
        exe: root.join("atlas.exe"),
        port: 1,
        configured_port: 1,
        token: "x".repeat(24),
    };
    let progress = std::sync::Arc::new(std::sync::Mutex::new(atlas::setupwin::Progress::new()));
    atlas::setupwin::walk_the_steps(&place, &progress, &Tools::default());
    let p = progress.lock().unwrap();
    assert!(p.finished);
    assert_eq!(p.problems(), 0, "{:?}", p.steps);
    let pieces = atlas::setupwin::setup_pieces().len();
    assert!(p.steps[pieces].label.starts_with("Tor"), "Tor isn't among what the setup fetches: {:?}", p.steps);
    for s in &p.steps[1..=pieces] {
        assert_eq!(s.state, atlas::setupwin::StepState::Done("already here".into()), "{}", s.label);
    }
    // The firewall rule for your own devices is its own step; off Windows
    // there's nothing to add, and it says so rather than asking.
    assert_eq!(p.steps[pieces + 1].label, "Letting your own devices reach Atlas");
    assert!(matches!(&p.steps[pieces + 1].state, atlas::setupwin::StepState::Done(_)), "{:?}", p.steps[pieces + 1]);
    assert!(matches!(p.phone, atlas::setupwin::Phone::Why(_) | atlas::setupwin::Phone::Link { .. }));
}

#[test]
fn the_launcher_hands_fetching_to_atlas() {
    let b = std::fs::read_to_string("ATLAS.bat").unwrap();
    // Once for the voice pieces (setup), once for the seeing models (menu 8).
    assert_eq!(b.matches("\"%EXE%\" get\r\n").count() + b.matches("\"%EXE%\" get\n").count(), 1, "the launcher's setup doesn't use Atlas's own fetching");
    assert_eq!(b.matches("\"%EXE%\" get seeing").count(), 1, "the seeing models aren't fetched by Atlas");
    let seeing = atlas::getpieces::seeing().iter().filter(|p| b.contains(p.url)).count();
    assert_eq!(seeing, 0, "a seeing model's address is written in ATLAS.bat as well as in Atlas");
    // Every address lives in Atlas's pinned catalogue, none in the launcher.
    let in_launcher = atlas::getpieces::catalogue().iter().filter(|p| b.contains(p.url)).count();
    assert_eq!(in_launcher, 0, "a download address is written in ATLAS.bat as well as in Atlas");
    assert!(!b.contains(":get_zip"), "the broken PowerShell zip download is still there");
    assert!(!b.contains("releases/latest/download/whisper"), "the dead 'latest' whisper address is back");
    let main = crate::common::source_of("main");
    assert!(main.contains("Some(\"get\")"));
}

// ================= 28 Sep 2026: downloads that can't hang, room checked first, a half-copied piece is not "here" =================

fn a_zip_piece() -> Piece {
    Piece {
        name: "a test zip",
        for_what: "testing",
        url: "http://127.0.0.1:9/none.zip",
        sha256: "ab".repeat(32).leak(),
        bytes: 1_000,
        lands: Lands::Zip { inside: "", dir: "tools/testzip", key: "tools/testzip/tool.exe" },
    }
}

#[test]
fn a_download_that_stalls_is_given_up_on_and_retried() {
    let a = atlas::getpieces::curl_args().join(" ");
    for want in ["--connect-timeout 30", "--speed-limit 10000", "--speed-time 60", "--retry 5", "--retry-all-errors", "--retry-delay 3", "-C -"] {
        assert!(a.contains(want), "curl isn't told `{want}`: {a}");
    }
    // And a whole download has a deadline, scaled by its size.
    let small = atlas::getpieces::deadline_for(1_000);
    let big = atlas::getpieces::deadline_for(2_500_000_000);
    assert!(small >= std::time::Duration::from_secs(600) && big > small);
    assert!(big < std::time::Duration::from_secs(12 * 3600), "a deadline no one would reach is no deadline");
}

#[test]
fn a_half_copied_piece_is_not_here_and_a_finished_one_is() {
    use atlas::getpieces::marker_path;
    let root = tmp("marker");
    let p = a_zip_piece();
    let key = root.join(p.key_path());
    std::fs::create_dir_all(key.parent().unwrap()).unwrap();
    std::fs::write(&key, b"MZ").unwrap();
    // The copy stopped partway: the unpacked copy is still there.
    let unpacked = root.join("data/tmp/downloads/a-test-zip.unpacked");
    std::fs::create_dir_all(&unpacked).unwrap();
    assert!(!have(&p, &root), "a key file alone, beside an unfinished fetch, counted as here");
    assert!(!marker_path(&p, &root).unwrap().exists());
    // An install from before the mark, with nothing unfinished: accepted, and marked.
    std::fs::remove_dir_all(&unpacked).unwrap();
    assert!(have(&p, &root));
    assert_eq!(std::fs::read_to_string(marker_path(&p, &root).unwrap()).unwrap(), p.sha256);
    // A mark for some other file (an older version of the piece) isn't this one.
    std::fs::write(marker_path(&p, &root).unwrap(), "cd".repeat(32)).unwrap();
    assert!(!have(&p, &root));
    // An empty key file is never taken for a finished one.
    std::fs::remove_file(marker_path(&p, &root).unwrap()).unwrap();
    std::fs::write(&key, b"").unwrap();
    assert!(!have(&p, &root));
}

#[test]
fn a_real_unpack_leaves_the_mark_only_when_everything_landed() {
    // The real fetch, from a local file through curl's file:// and the real
    // unzip, so the mark is written by the code that copies.
    let root = tmp("unpack-mark");
    let src = tmp("unpack-mark-src");
    std::fs::create_dir_all(src.join("inside")).unwrap();
    std::fs::write(src.join("inside/tool.exe"), b"MZ tool").unwrap();
    std::fs::write(src.join("inside/helper.dll"), b"helper").unwrap();
    let zip = src.join("piece.zip");
    let zipped = std::process::Command::new("zip")
        .current_dir(&src)
        .args(["-q", "-r", zip.to_str().unwrap(), "inside"])
        .status()
        .is_ok_and(|s| s.success());
    if !zipped {
        println!("SKIP: no zip tool here");
        return;
    }
    let bytes = std::fs::read(&zip).unwrap();
    let url: &'static str = format!("file://{}", zip.display()).leak();
    let p = Piece {
        name: "a local zip",
        for_what: "testing",
        url,
        sha256: atlas::digest::sha256_hex(&bytes).leak(),
        bytes: bytes.len() as u64,
        lands: Lands::Zip { inside: "inside", dir: "tools/local", key: "tools/local/tool.exe" },
    };
    let tools = Tools { curl: "curl".into(), unzip: Unzip::Unzip, unzipper: "unzip".into() };
    fetch(&p, &root, &tools, &|_, _| {}).unwrap();
    assert!(have(&p, &root));
    assert_eq!(std::fs::read_to_string(root.join("tools/local/.atlas-piece")).unwrap(), p.sha256);
    assert!(root.join("tools/local/helper.dll").is_file());
    assert!(!root.join("data/tmp/downloads/a-local-zip.unpacked").exists(), "the unpacked copy is cleared");
}

#[test]
fn setting_up_says_how_much_room_it_needs_and_where_before_downloading() {
    use atlas::getpieces::{clear_unfinished, free_bytes, room_for, space_needed, SPARE_BYTES};
    let root = tmp("room");
    let pieces = vec![a_zip_piece()];
    assert_eq!(space_needed(&pieces, &root), 1_000 * 2 + SPARE_BYTES);
    // What's already downloaded doesn't need room again.
    let part = root.join("data/tmp/downloads/a-test-zip.part");
    std::fs::create_dir_all(part.parent().unwrap()).unwrap();
    std::fs::write(&part, vec![0u8; 400]).unwrap();
    assert_eq!(space_needed(&pieces, &root), 600 * 2 + SPARE_BYTES);
    let e = room_for(&pieces, &root, Some(100_000_000)).unwrap_err();
    println!("LIVE [room] {e}");
    assert!(e.contains("0.5 GB") && e.contains("0.1 GB") && e.contains(&root.display().to_string()), "{e}");
    assert!(e.contains("Free up") && !e.contains("atlas "), "{e}");
    assert!(room_for(&pieces, &root, Some(10_000_000_000)).is_ok());
    assert!(room_for(&pieces, &root, None).is_ok(), "not knowing is no reason to stop");
    // Nothing missing: no room needed.
    assert_eq!(space_needed(&[], &root), 0);
    // Unfinished downloads are given back when room is short.
    assert_eq!(clear_unfinished(&pieces, &root), 400);
    assert!(!part.exists());
    // The disk really is asked.
    assert!(free_bytes(&root).is_some_and(|b| b > 0));
    assert!(free_bytes(&root.join("not/made/yet")).is_some(), "asked of the nearest folder that exists");
}

#[test]
fn the_thinking_engine_is_pinned_at_its_real_size() {
    let llama = atlas::getpieces::pictures().into_iter().find(|p| p.name == "the thinking engine").unwrap();
    assert_eq!(llama.bytes, 34_807_256, "the size measured for llama-b10456-bin-win-vulkan-x64.zip");
}

// ================= running Atlas Setup.exe again over a running Atlas =================

#[test]
#[cfg(target_os = "linux")]
fn setup_run_again_over_a_running_atlas_puts_the_new_one_in_place() {
    // Linux, like Windows, won't write over a running program ("text file
    // busy"); both allow renaming it. So the running one is moved aside.
    let home = tmp("over-running");
    let sleep = ["/bin/sleep", "/usr/bin/sleep"].into_iter().map(PathBuf::from).find(|p| p.is_file()).unwrap();
    let installed = home.join(atlas::firstlaunch::INSTALLED_NAME);
    std::fs::copy(&sleep, &installed).unwrap();
    let mut running = std::process::Command::new(&installed).arg("30").spawn().unwrap();
    let new = home.join("Atlas Setup");
    std::fs::write(&new, b"#!/bin/sh\necho the new atlas\n").unwrap();
    assert!(std::fs::copy(&new, &installed).is_err(), "writing over a running program worked here, so this test proves nothing");
    let at = atlas::firstlaunch::move_in_over(&new, &home, true, std::time::Duration::from_millis(10)).unwrap();
    assert_eq!(at, installed);
    assert_eq!(std::fs::read(&installed).unwrap(), b"#!/bin/sh\necho the new atlas\n");
    assert!(home.join("config/apps.yaml").is_file(), "settings written too");
    let _ = running.kill();
    let _ = running.wait();
    // The one set aside goes once it's no longer running.
    assert_eq!(atlas::firstlaunch::tidy_set_aside(&home), 1);
}

#[test]
fn setup_never_silently_puts_an_older_atlas_over_a_newer_one() {
    use atlas::firstlaunch::{replacing, Replacing};
    assert_eq!(replacing(("0.1.5", "aa"), None), Replacing::Install, "nothing there");
    assert_eq!(replacing(("0.1.5", "aa"), Some((Ok("0.1.5".into()), "aa".into()))), Replacing::Same);
    assert_eq!(replacing(("0.1.9", "aa"), Some((Ok("0.1.5".into()), "bb".into()))), Replacing::Install, "newer over older");
    assert_eq!(
        replacing(("0.1.5", "aa"), Some((Ok("0.1.9".into()), "bb".into()))),
        Replacing::Older { installed: "0.1.9".into(), mine: "0.1.5".into() }
    );
    assert_eq!(replacing(("0.1.5", "aa"), Some((Err("didn't run".into()), "bb".into()))), Replacing::Install, "a broken install is replaced");
}

// ================= the laptop's model from the phone, over Tailscale =================

#[test]
fn a_model_out_of_reach_over_tailscale_says_so_in_plain_words() {
    use atlas::models::{on_tailscale, unreachable_words};
    assert!(on_tailscale("100.101.3.4:8080"));
    assert!(on_tailscale("laptop.tail1234.ts.net:8080"));
    assert!(on_tailscale("[fd7a:115c:a1e0::1]:8080"));
    assert!(!on_tailscale("127.0.0.1:8080"));
    assert!(!on_tailscale("100.200.1.1:8080"), "outside 100.64.0.0/10");
    let w = unreachable_words("100.101.3.4:8080", "Connection refused (os error 111)");
    assert!(w.contains("over Tailscale") && w.contains("awake") && w.contains("try again"), "{w}");
    let local = unreachable_words("127.0.0.1:8080", "Connection refused");
    assert!(!local.contains("Tailscale") && local.contains("try again"), "{local}");
}

#[test]
fn a_model_that_cannot_be_reached_is_said_plainly_after_a_second_try() {
    // A port nothing listens on: the real chat call, its retry, its words.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let req = atlas::brain::ChatRequest { messages: vec![], ..Default::default() };
    let t0 = std::time::Instant::now();
    let e = atlas::models::chat_call(&format!("http://127.0.0.1:{port}/v1/chat/completions"), &req, &mut |_| true).unwrap_err().to_string();
    assert!(t0.elapsed() >= std::time::Duration::from_millis(900), "it didn't try a second time");
    assert!(e.contains("couldn't reach the model") && e.contains("try again"), "{e}");
}

// ================= the releaser's own builds don't pile up =================

#[test]
fn the_releaser_keeps_only_its_newest_builds_to_hand_out() {
    use atlas::update_courier::{keep_for_friends, FILES, KEEP_OWN_FILES};
    let root = tmp("release-files");
    let mut last = String::new();
    for i in 0..6 {
        last = keep_for_friends(&root, format!("build number {i}").as_bytes()).unwrap();
    }
    let kept: Vec<String> = std::fs::read_dir(root.join(FILES)).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert_eq!(kept.len(), KEEP_OWN_FILES, "{kept:?}");
    assert!(kept.contains(&last), "the newest is always kept");
}
