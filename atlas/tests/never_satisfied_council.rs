use atlas::brain::Llm;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::error::Result;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct NeverSatisfied(Mutex<u32>);

impl Llm for NeverSatisfied {
    fn complete(&self, _s: &str, _user: &str) -> Result<String> {
        let mut n = self.0.lock().unwrap();
        *n += 1;
        Ok(format!(
            "Against. Still a risk. It would be OK if change number {n} were made."
        ))
    }
}

#[test]
fn what_is_still_a_no_after_the_retests_is_yours_to_decide() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let platform = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let seats = Arc::new(NeverSatisfied(Mutex::new(0)));
    let root = std::env::temp_dir().join(format!(
        "atlas-never-satisfied-{}-{}",
        std::process::id(),
        atlas::store::now()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut daemon = Daemon::new(
        cfg,
        &platform,
        Some(seats.clone()),
        Store::new(root),
        Proactive::new(ProactiveConfig::default()),
    );
    let _ = daemon.ask_the_room("is it safe to open the phone link to the internet?");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut t = atlas::store::now() + 50;
    let mut out = Vec::new();
    while Instant::now() < deadline && !out.iter().any(|l: &String| l.contains("yours to decide")) {
        out.extend(daemon.tick(t));
        t += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    let said = out.join(" ");
    assert!(said.contains("After 2 rounds of changes"), "{said}");
    assert!(said.contains("yours to decide"), "{said}");
    assert_eq!(*seats.0.lock().unwrap(), 5 + 2 * 10);
}
