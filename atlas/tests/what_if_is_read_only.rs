//! The market what-if is analysis only (Plan F, 24 Oct 2026). Charter s8:
//! Atlas never places a trade. The what-if reads a bars file and says odds,
//! labelled a simulation; these tests hold it to having no path to an
//! order, a broker, the network, a write, or the screen.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

#[test]
fn a_what_if_over_a_bars_file_answers_labelled_and_touches_nothing() {
    let dir = std::env::temp_dir().join(format!("atlas-whatif-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A day a bar, gently zigzagging up, with dates.
    let mut csv = String::from("time,open,high,low,close\n");
    let mut p = 100.0f64;
    for i in 0..300i64 {
        let step = if i % 7 < 4 { 0.6 } else { -0.5 };
        let next = p + step;
        csv.push_str(&format!(
            "{},{p},{},{},{next}\n",
            1_735_689_600_000 + i * 86_400_000,
            p.max(next) + 0.2,
            p.min(next) - 0.2
        ));
        p = next;
    }
    let file = dir.join("bars.csv");
    std::fs::write(&file, csv).unwrap();
    let before = std::fs::read(&file).unwrap();

    let c = Config::load(Path::new("config")).unwrap();
    let plat = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let mut d = Daemon::new(
        &c,
        &plat,
        None,
        Store::new(dir.join("state")),
        Proactive::new(ProactiveConfig::default()),
    );
    let said = d.turn(
        &format!(
            "what if I'm in at 100 with stop 99.5 and target 102 over 10 bars using {}",
            file.display()
        ),
        atlas::store::now(),
    );
    eprintln!("{said}");
    assert!(
        said.starts_with("Simulated, not a forecast or advice -- nothing is bought or sold."),
        "{said}"
    );
    assert!(
        said.contains("Replaying all 290 past 10-bar stretches"),
        "{said}"
    );
    assert!(said.contains("2025-01-01 to 2025-10-27"), "{said}");
    // A real spread, not a degenerate all-neither answer.
    assert!(!said.contains("target 102 first in 0%"), "{said}");
    assert!(!said.contains("stop 99.5 first in 0%"), "{said}");
    assert!(
        plat.log.borrow().is_empty(),
        "touched the screen: {:?}",
        plat.log.borrow()
    );
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "the bars file changed"
    );
    // And the same question again gives the same odds.
    let again = d.turn(
        &format!(
            "what if I'm in at 100 with stop 99.5 and target 102 over 10 bars using {}",
            file.display()
        ),
        atlas::store::now(),
    );
    assert_eq!(again, said);
}

/// The code, without comments or its own test module.
fn code_of(file: &str) -> String {
    let src = std::fs::read_to_string(file).unwrap();
    let src = src.split("#[cfg(test)]").next().unwrap();
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_what_if_has_no_path_to_an_order_a_network_a_write_or_the_screen() {
    let forbidden = [
        "order",
        "broker",
        "submit",
        "execute",
        "trade(",
        "buy(",
        "sell(",
        "http",
        "Command",
        "std::net",
        "TcpStream",
        "plat",
        "finance::",
        "fs::write",
        "File::create",
        "OpenOptions",
        "remove",
        "store.save",
        "click",
        "type_text",
        "press(",
        "unsafe",
    ];
    let mut found = Vec::new();
    for file in ["src/whatif.rs", "src/daemon/whatif.rs"] {
        let code = code_of(file);
        for f in forbidden {
            if code.contains(f) {
                found.push(format!("{file}: {f}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "the what-if reaches past reading: {found:?}"
    );
    // What it may use: the market readers, a file read, and nothing else.
    let uses: Vec<String> = code_of("src/whatif.rs")
        .lines()
        .filter(|l| l.trim_start().starts_with("use "))
        .map(|l| l.trim().to_string())
        .collect();
    assert!(
        uses.iter().all(|u| u.starts_with("use crate::market::")),
        "{uses:?}"
    );
    let handler = code_of("src/daemon/whatif.rs");
    assert_eq!(
        handler.matches("std::fs::").count(),
        1,
        "one file read, no more"
    );
    assert!(handler.contains("std::fs::read_to_string"));
}

#[test]
fn the_guard_would_see_an_order_path_if_one_were_written() {
    // Not vacuous: the same scan finds money-moving words where they live.
    let finance = code_of("src/finance.rs");
    assert!(finance.contains("submit") || finance.contains("order"));
}
