//! Tor ships with Atlas (OPEN_GAPS 8.3) and gets round a network that blocks
//! it (8.6).
//!
//! Friends reach each other only through Tor (`onion`), and until 27 Sep 2026
//! nothing put `tor` beside Atlas: the Friends page said "only friends on your
//! home network can reach you", which was true on every install. Now the
//! setup fetches the Tor Project's own expert bundle (pinned, checked), the
//! Windows build ships it in the zip, and the bridges inside that bundle are
//! what Atlas switches to when a network blocks Tor.

use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-torships-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn the_setup_fetches_tor_pinned_from_the_tor_projects_archive() {
    let tor = atlas::getpieces::tor();
    assert_eq!(tor.len(), 1);
    let t = &tor[0];
    assert!(t.url.starts_with("https://archive.torproject.org/tor-package-archive/torbrowser/"), "{}", t.url);
    assert!(t.url.contains("tor-expert-bundle-"), "{}", t.url);
    assert_eq!(t.sha256.len(), 64, "Tor isn't pinned by its SHA-256");
    // It lands where `onion::find_tor` looks: `tor/tor(.exe)` beside Atlas.
    let exe = if cfg!(windows) { "tor/tor.exe" } else { "tor/tor" };
    assert_eq!(t.key_path(), exe);
    assert!(atlas::setupwin::setup_pieces().iter().any(|p| p.name == "Tor"), "the setup window doesn't fetch Tor");
    assert!(atlas::getpieces::set(Some("tor")).is_some(), "`atlas get tor` isn't a set");
}

#[test]
fn the_windows_build_ships_the_same_tor_the_setup_fetches() {
    let yml = std::fs::read_to_string("../.github/workflows/windows.yml").unwrap();
    // The Windows piece, whatever system this test runs on: its URL and hash
    // are written once in getpieces.rs, and the workflow must match them.
    let src = std::fs::read_to_string("src/getpieces.rs").unwrap();
    let win = src.split("if cfg!(windows) {").nth(1).and_then(|r| r.split("} else {").next()).unwrap();
    let url = win.split("url: \"").nth(1).unwrap().split('"').next().unwrap();
    let sha = win.split("sha256: \"").nth(1).unwrap().split('"').next().unwrap();
    assert!(yml.contains(url), "windows.yml fetches a different Tor from the setup's: {url}");
    assert!(yml.contains(sha), "windows.yml checks a different SHA-256 from the setup's");
    assert!(yml.contains("sha256sum -c"), "windows.yml doesn't check Tor before shipping it");
    assert!(yml.contains("dist/tor"), "Tor isn't put beside atlas.exe in the build");
    assert!(yml.contains("path: dist/"), "the uploaded build leaves Tor out");
}

/// A bundle shaped like the Tor Project's (`tor/tor`, `tor/pluggable_transports/`),
/// gzip'd tar, fetched through the same path the setup uses. Proves the
/// unpacking of a `.tar.gz` on a system whose unzipper is `unzip`.
#[test]
fn a_tor_bundle_unpacks_into_tor_beside_atlas() {
    let work = tmp("unpack");
    let src = work.join("src");
    std::fs::create_dir_all(src.join("tor/pluggable_transports")).unwrap();
    std::fs::create_dir_all(src.join("data")).unwrap();
    std::fs::write(src.join("tor/tor"), b"#!/bin/sh\necho tor\n").unwrap();
    std::fs::write(src.join("tor/tor.exe"), b"MZ").unwrap();
    std::fs::write(src.join("tor/pluggable_transports/pt_config.json"), b"{}").unwrap();
    std::fs::write(src.join("data/geoip"), b"x").unwrap();
    let tgz = work.join("bundle.tar.gz");
    let ok = std::process::Command::new("tar").arg("-czf").arg(&tgz).arg("-C").arg(&src).arg("tor").arg("data").status().unwrap();
    assert!(ok.success());
    let bytes = std::fs::read(&tgz).unwrap();
    let sha = atlas::digest::sha256_hex(&bytes);
    let key = if cfg!(windows) { "tor/tor.exe" } else { "tor/tor" };
    let piece = atlas::getpieces::Piece {
        name: "Tor",
        for_what: "friends reaching your Atlas from anywhere",
        url: Box::leak(format!("file://{}", tgz.display()).into_boxed_str()),
        sha256: Box::leak(sha.into_boxed_str()),
        bytes: bytes.len() as u64,
        lands: atlas::getpieces::Lands::Zip { inside: "tor", dir: "tor", key },
    };
    let root = work.join("install");
    std::fs::create_dir_all(&root).unwrap();
    atlas::getpieces::fetch(&piece, &root, &atlas::getpieces::Tools::default(), &|_, _| {}).unwrap();
    assert!(root.join(key).is_file());
    assert!(root.join("tor/pluggable_transports/pt_config.json").is_file(), "the bridges' settings didn't come with it");
    assert!(!root.join("data/geoip").exists(), "only the tor folder is taken from the bundle");
    let _ = std::fs::remove_dir_all(&work);
}

/// With the real Tor bundle (`ATLAS_TOR_BUNDLE` = the folder holding its
/// `tor/`): real `tor` accepts the settings Atlas writes for each kind of
/// bridge, and starting it bridged really starts the bridge program.
/// Run: `ATLAS_TOR_BUNDLE=... cargo test --test all tor_ships -- --ignored`.
#[test]
#[ignore = "needs the Tor expert bundle unpacked on this machine"]
fn real_tor_accepts_every_kind_of_bridge_atlas_switches_to() {
    let bundle = PathBuf::from(std::env::var("ATLAS_TOR_BUNDLE").expect("ATLAS_TOR_BUNDLE"));
    let tor = bundle.join("tor").join(if cfg!(windows) { "tor.exe" } else { "tor" });
    assert!(tor.is_file(), "{}", tor.display());
    for kind in atlas::onion::BRIDGE_KINDS {
        let lines = atlas::onion::bridge_lines(&tor, kind).unwrap_or_else(|| panic!("the real bundle has no {kind} bridges"));
        let dir = tmp(&format!("verify-{kind}"));
        let rc = dir.join("torrc");
        std::fs::write(&rc, atlas::onion::torrc(&dir, &dir.join("onion"), 19050, 19051, &lines)).unwrap();
        let out = std::process::Command::new(&tor)
            .current_dir(tor.parent().unwrap())
            .arg("--verify-config")
            .arg("-f")
            .arg(&rc)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && said.contains("Configuration was valid"), "{kind}: {said}");
    }
    // Started through obfs4, tor launches lyrebird itself and connects to it
    // ("Connected to pluggable transport", conn_done_pt). Whether it then
    // gets through depends on the network; this proves the bridge machinery.
    let me = atlas::peerkey::Identity::from_seed_for_test([21; 32]);
    let dir = tmp("bridged");
    let mut t = atlas::onion::Tor::start_bridged(&tor, &dir, &me, 1, "obfs4", &[]).unwrap();
    let mut log = String::new();
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        log = std::fs::read_to_string(dir.join("tor.log")).unwrap_or_default();
        if log.contains("conn_done_pt") {
            break;
        }
    }
    assert!(!t.stopped(), "tor refused to run bridged: {log}");
    assert!(log.contains("conn_done_pt"), "tor never reached its bridge program (lyrebird): {log}");
    assert_eq!(t.bridges.as_deref(), Some("obfs4"));
    drop(t);
}

/// 28 Sep 2026: a Tor left by an Atlas that crashed held the data folder's
/// lock and every later Tor failed on it. The one written down is stopped
/// at the next start -- and only if it really is that Tor.
#[test]
#[cfg(target_os = "linux")]
fn a_tor_left_running_is_stopped_but_nothing_else_is() {
    let dir = std::env::temp_dir().join(format!("atlas-orphan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sleep = ["/bin/sleep", "/usr/bin/sleep"].into_iter().map(std::path::PathBuf::from).find(|p| p.is_file()).unwrap();
    let tor = dir.join("tor");
    std::fs::copy(&sleep, &tor).unwrap();
    let mut orphan = std::process::Command::new(&tor).arg("60").spawn().unwrap();
    std::fs::write(atlas::onion::pid_file(&dir), format!("{}\n{}\n", orphan.id(), tor.display())).unwrap();
    assert_eq!(atlas::onion::stop_orphan(&dir, &tor), Some(orphan.id()));
    assert!(!orphan.wait().unwrap().success(), "it was stopped, not left to finish");
    assert!(!atlas::onion::pid_file(&dir).exists());
    // A process that isn't Tor, under a number written down: left alone.
    let mut other = std::process::Command::new(&sleep).arg("60").spawn().unwrap();
    std::fs::write(atlas::onion::pid_file(&dir), format!("{}\n{}\n", other.id(), tor.display())).unwrap();
    assert_eq!(atlas::onion::stop_orphan(&dir, &tor), None);
    assert!(other.try_wait().unwrap().is_none(), "some other program was stopped");
    let _ = other.kill();
    let _ = other.wait();
    assert_eq!(atlas::onion::stop_orphan(&dir, &tor), None, "nothing written down, nothing done");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tor_goes_when_this_atlas_goes() {
    let t = atlas::onion::torrc(std::path::Path::new("/x"), std::path::Path::new("/x/onion"), 9051, 40000, &["UseBridges 1".into()]);
    assert!(t.contains(&format!("__OwningControllerProcess {}\n", std::process::id())), "{t}");
    assert!(t.ends_with("UseBridges 1\n"), "the extra lines still come last");
}
