use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "atlas-search-shutdown-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

fn notes() -> atlas::recall::Library {
    let dir = scratch("notes");
    for (name, body) in [
        ("boiler.md", "# Boiler service\n\nThe boiler was serviced by Hartley Heating, who replaced the pressure valve and topped up the expansion vessel."),
        ("car.md", "# Car insurance\n\nThe car insurance renews in March with Admiral; the excess is three hundred and fifty pounds for any claim."),
        ("garden.md", "# Garden plan\n\nPlant the tomatoes along the south fence after the last frost and keep the courgettes well watered."),
    ] {
        std::fs::write(dir.join(name), body).unwrap();
    }
    std::fs::write(dir.join("ignore.txt"), "not a note").unwrap();
    atlas::recall::library_from_dir(&dir)
}

#[test]
fn search_measurement_keeps_no_success_until_its_history_is_saved() {
    let mut cfg = Config::load(Path::new("config")).unwrap();
    cfg.tools.as_mut().unwrap().recall.semantic = false;
    let p = plat();
    for fault in [false, true] {
        let store = Store::new(scratch(if fault {
            "measurement-save-fault"
        } else {
            "measurement-held"
        }));
        let mut d = Daemon::new(
            &cfg,
            &p,
            None,
            store.clone(),
            Proactive::new(ProactiveConfig::default()),
        );
        d.library = notes();
        if fault {
            std::fs::create_dir(store.root().join("search_checks.json")).unwrap();
        }
        let guard = store.transaction().unwrap();
        assert!(d.start_search_check().starts_with("Measuring"));
        std::thread::sleep(Duration::from_millis(80));
        assert!(!store.root().join("search_checks.json").is_file());
        drop(guard);
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut out = Vec::new();
        while d.crew.errands().iter().any(|e| e.name == "search-check") && Instant::now() < deadline
        {
            out.extend(d.tick(atlas::store::now() + 50));
            std::thread::sleep(Duration::from_millis(10));
        }
        if fault {
            assert!(!out.iter().any(|line| line.starts_with("Search check,")));
            assert!(out
                .iter()
                .any(|line| line.contains("history") || line.contains("not recorded")));
        } else {
            assert_eq!(
                store
                    .load::<Vec<atlas::recall::SearchCheck>>("search_checks")
                    .len(),
                1
            );
            assert_eq!(
                out.iter()
                    .filter(|line| line.starts_with("Search check,"))
                    .count(),
                1
            );
        }
    }
}
