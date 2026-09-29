//! The last section on the list with a type to land in and no reader.
//!
//! `trading:` is twenty lines of defended numbers in `tools.yaml` — the
//! fraction of the balance one trade may risk, how far beyond structure a stop
//! sits. `TradingConfig` existed, the section parsed into it, and every
//! reader of those numbers built `Rules::default()` at the call site. `config::PARSED_AND_NEVER_READ` said so in as many words:
//! *"main.rs uses the defaults"*.
//!
//! `atlas market` was worse than defaulting. It carried a **third** copy of
//! three of the numbers as bare literals — `structure::recent(&view, 120, 2)`
//! and `levels(&view, 2, 50.0)` are `structure_bars`, `pivot_reach` and
//! `level_span_pips` written out again — so one command read structure one way
//! and levels another and neither was the file. A literal that happens to
//! equal the default is the hardest kind to find: nothing is wrong until
//! somebody changes the file, and then nothing happens.
//!
//! ## Why this file runs the program
//!
//! `your_settings_reach_the_code.rs` checks main.rs's call sites by reading the
//! source, and says why: a behavioural test of the retention pass would need a
//! 500MB data directory. This one has no such excuse. `atlas market` takes a
//! file of bars and prints what it read, so the whole claim — *the number in
//! your file is the number the program used* — can be made by running it twice
//! against two configs and one set of bars. A source guard can only show that
//! a call site names the config. This shows the answer changing.
//!
//! Mutation-checked on 18 Sep 2026: with the literal `50.0` put back in
//! `main.rs`, `a_span_you_set_is_the_span_it_looks_over` fails — both runs
//! print the same levels.

mod common; // `common::source_of`: a module's source wherever its files live

use std::path::{Path, PathBuf};
use std::process::Command;

// ---------- the call sites ----------

#[test]
fn nothing_in_main_builds_its_own_trading_numbers() {
    let m = crate::common::source_of("main");
    let live: String = m
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        live.contains("fn trading_cfg()"),
        "the helper that reads your `trading:` section is gone"
    );
    assert!(
        !live.contains("atlas::levels::Rules::default()"),
        "a call site is building the risk limits again, so your `trading.levels` \
         section does nothing there"
    );
}

#[test]
fn the_market_command_does_not_carry_the_numbers_twice() {
    // The literals, specifically. These are the copies that made the section
    // look wired from a distance: the program read 120, 2 and 50.0, and the
    // file said 120, 2 and 50.0, and the two had nothing to do with each
    // other.
    let m = crate::common::source_of("main");
    let live: String = m
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    for literal in ["recent(&view, 120, 2)", "levels(&view, 2, 50.0)"] {
        assert!(
            !live.contains(literal),
            "`{literal}` is back — that is `structure_bars`, `pivot_reach` and \
             `level_span_pips` written out again beside a config that holds them"
        );
    }
    assert!(
        live.contains("trading.levels.structure_bars")
            && live.contains("trading.levels.pivot_reach")
            && live.contains("trading.levels.level_span_pips"),
        "the market command is not reading those three from your config"
    );
}

#[test]
fn it_is_no_longer_listed_as_a_setting_that_does_nothing() {
    // `atlas doctor` reads this list out loud. A wired section left on it
    // tells the user a working setting is dead, which is the same lie the
    // other way round.
    for (key, _) in atlas::config::PARSED_AND_NEVER_READ {
        assert_ne!(*key, "trading", "trading is wired and still listed as doing nothing");
    }
}

// ---------- the file and the struct agree ----------

fn shipped() -> atlas::voice::ToolsConfig {
    atlas::config::Config::load(Path::new("config"))
        .expect("config/ loads")
        .tools
        .expect("tools.yaml loads")
}

fn number_in_file(key: &str) -> f64 {
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    let trading = raw.split("\ntrading:").nth(1).expect("no trading: section");
    trading
        .lines()
        .find_map(|l| l.trim().strip_prefix(&format!("{key}:")))
        .and_then(|v| v.trim().split_whitespace().next())
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("{key} is not in the shipped trading: section"))
}

#[test]
fn the_shipped_file_still_carries_the_numbers() {
    // Wiring a setting is half of it. A section that quietly left the shipped
    // file would leave nobody knowing the behaviour was theirs to change.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    let trading = raw.split("\ntrading:").nth(1).expect("no trading: section");
    for key in [
        "risk_fraction",
        "buffer_ranges",
        "min_stop_ranges",
        "max_cost_share",
        "min_reward",
        "range_window",
        "pivot_reach",
        "level_span_pips",
        "structure_bars",
    ] {
        assert!(
            trading.contains(&format!("{key}:")),
            "{key} is no longer in the shipped trading: section"
        );
    }
}

#[test]
fn the_value_in_the_file_is_the_value_on_the_struct() {
    let t = shipped();
    assert_eq!(t.trading.levels.risk_fraction, number_in_file("risk_fraction"));
    assert_eq!(t.trading.levels.level_span_pips, number_in_file("level_span_pips"));
    assert_eq!(t.trading.levels.structure_bars as f64, number_in_file("structure_bars"));
    assert_eq!(t.trading.levels.pivot_reach as f64, number_in_file("pivot_reach"));
}

// ---------- the program, run twice ----------

/// 320 hourly bars with swings in them, written out the way `atlas market`
/// reads them. Deterministic: no clock, no randomness, same file every run.
fn bars_file(dir: &Path) -> PathBuf {
    let mut s = String::from("time,open,high,low,close\n");
    let mut price = 1.1000f64;
    for i in 0..320i64 {
        let o = price;
        // A sine with a drift on it. The sine puts turning points in — a flat
        // series has no swing pivots and `levels` would find nothing for
        // either config, which is a test that passes for the wrong reason.
        let c = o + 0.0004 * ((i as f64) / 3.0).sin() + 0.00008;
        let h = o.max(c) + 0.0003;
        let l = o.min(c) - 0.0003;
        s.push_str(&format!(
            "{},{:.5},{:.5},{:.5},{:.5}\n",
            1_700_000_000i64 + i * 3600,
            o,
            h,
            l,
            c
        ));
        price = c;
    }
    let p = dir.join("bars.csv");
    std::fs::write(&p, s).expect("bars file");
    p
}

/// The shipped config, with one line rewritten.
///
/// A one-key `tools.yaml` would be a complete `ToolsConfig` — the struct is
/// `#[serde(default)]` throughout — but `Config::load` wants the rest of the
/// directory and `main` stops when it cannot have it. Copying the real one is
/// the better test anyway: the two runs differ by exactly one line of the file
/// Eric actually ships, not by a file written to make a point.
fn config_with_span(dir: &Path, span_pips: f64) -> PathBuf {
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
    let mut edited = String::with_capacity(raw.len());
    let mut done = false;
    for line in raw.lines() {
        if !done && line.trim_start().starts_with("level_span_pips:") {
            edited.push_str(&format!("    level_span_pips: {span_pips}\n"));
            done = true;
        } else {
            edited.push_str(line);
            edited.push('\n');
        }
    }
    assert!(done, "no level_span_pips: line in the shipped tools.yaml to change");
    std::fs::write(&tools, edited).expect("write tools.yaml");
    cfg
}

fn run_market(home: &Path, cfg: &Path, bars: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_atlas"))
        .args(["market", &bars.to_string_lossy(), "EURUSD"])
        .env("ATLAS_HOME", home)
        .env("ATLAS_CONFIG", cfg)
        .output()
        .expect("atlas market runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-trading-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("scratch dir");
    p
}

#[test]
fn a_span_you_set_is_the_span_it_looks_over() {
    // The whole claim, end to end and in the program's own words. Same bars,
    // same command, two files: one says look fifty pips either side of price,
    // the other says look a fifth of a pip. Before this was wired both runs
    // printed the first answer, because `main.rs` said 50.0 and your file was
    // never opened.
    let home = scratch("span");
    let bars = bars_file(&home);

    let wide = config_with_span(&home.join("wide"), 50.0);
    let narrow = config_with_span(&home.join("narrow"), 0.2);

    let with_wide = run_market(&home, &wide, &bars);
    let with_narrow = run_market(&home, &narrow, &bars);

    assert!(
        with_wide.contains("levels in reach:"),
        "fifty pips either side of price found nothing, so this test cannot tell \
         a read config from an ignored one:\n{with_wide}"
    );
    assert!(
        with_narrow.contains("no levels within reach of price"),
        "a fifth of a pip either side of price still found levels, so the number \
         in the file is not the number the program used:\n{with_narrow}"
    );
    assert_ne!(
        with_wide, with_narrow,
        "two different configs produced identical output"
    );

    let _ = std::fs::remove_dir_all(&home);
}
