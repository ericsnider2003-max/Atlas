//! Where an idle tick's time goes, measured (2 Oct 2026). Ignored by
//! default: run with `cargo test --release --test what_a_tick_costs --
//! --ignored --nocapture`.
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

#[test]
#[ignore]
fn an_idle_tick_by_part() {
    let p = std::env::temp_dir().join("atlas-tick-costs");
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    let c = Config::load(Path::new("config")).unwrap();
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &plat, None, Store::new(p), Proactive::new(ProactiveConfig::default()));
    let mut sums: std::collections::BTreeMap<&'static str, u64> = Default::default();
    let start = std::time::Instant::now();
    let n = 300u64;
    for i in 0..n {
        let t = 1_790_000_000 + i * 2;
        let _ = d.observe(t);
        d.tick(t);
        for (name, us) in d.last_tick_cpu() {
            *sums.entry(name).or_default() += us;
        }
    }
    let total = start.elapsed();
    let mut v: Vec<_> = sums.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    println!("{n} ticks in {total:?} ({:.2} ms a tick)", total.as_secs_f64() * 1000.0 / n as f64);
    for (name, ms) in v.iter().take(15) {
        println!("  {name:<28} {:>8.1} ms of CPU", *ms as f64 / 1000.0);
    }
}
