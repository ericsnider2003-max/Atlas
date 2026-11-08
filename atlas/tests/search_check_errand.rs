use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;
use std::time::{Duration, Instant};

fn scratch() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "atlas-search-check-{}-{}",
        std::process::id(),
        atlas::store::now()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn platform() -> MockPlatform {
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
    let dir = scratch();
    for (file, body) in [
        ("boiler.md", "# Boiler service\n\nThe boiler was serviced by Hartley Heating, who replaced the pressure valve and topped up the expansion vessel."),
        ("car.md", "# Car insurance\n\nThe car insurance renews in March with Admiral; the excess is three hundred and fifty pounds for any claim."),
        ("garden.md", "# Garden plan\n\nPlant the tomatoes along the south fence after the last frost and keep the courgettes well watered."),
    ] {
        std::fs::write(dir.join(file), body).unwrap();
    }
    atlas::recall::library_from_dir(&dir)
}

#[test]
fn the_search_check_runs_as_an_errand_and_is_kept() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = platform();
    let mut d = Daemon::new(
        cfg,
        &p,
        None,
        Store::new(scratch()),
        Proactive::new(ProactiveConfig::default()),
    );
    d.library = notes();
    let said = d.start_search_check();
    assert!(said.starts_with("Measuring"), "{said}");
    assert!(d.crew.errands().iter().any(|e| e.name == "search-check"));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut t = atlas::store::now() + 50;
    let mut out = Vec::new();
    while Instant::now() < deadline && !out.iter().any(|l: &String| l.contains("Search check")) {
        out.extend(d.tick(t));
        t += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(out.iter().any(|l| l.contains("Search check")), "{out:?}");
    let kept: Vec<atlas::recall::SearchCheck> = d.store.load("search_checks");
    assert_eq!(kept.len(), 1);
    use atlas::which_errand::{describe, Candidate};
    let c = Candidate {
        id: 1,
        label: "search-check".into(),
        topic: None,
        started: 0,
        paused: false,
        can_hold: true,
    };
    assert_eq!(describe(&c), "the search check");
}
