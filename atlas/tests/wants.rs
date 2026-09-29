//! "What are you missing?" answered against the machine Atlas is actually on.
//!
//! One of the four test files the 10 Sep package named and the tree never
//! got. Its fix — recommendations measured against `fit`'s real machine
//! rather than `Machine::default()`, which is Eric's laptop written down —
//! did reach the tree; these are its tests, rebuilt 26 Sep 2026.

use atlas::fit;
use atlas::wants::{ask, machine_from, recommend, Observations};

fn measured(total_mb: u64, free_mb: u64, reclaimable_mb: u64, vram_mb: u64, cores: u32) -> fit::Machine {
    fit::Machine {
        total_ram_mb: total_mb,
        free_ram_mb: free_mb,
        cpu_cores: cores,
        vram_mb,
        has_npu: false,
        disk_free_mb: 100_000,
        disk_is_spinning: false,
        reclaimable_mb,
    }
}

#[test]
fn memory_held_by_things_you_arent_using_counts_as_free() {
    // Otherwise the answer on a day with a browser open is "buy more memory"
    // when the honest one is "close some tabs".
    let m = machine_from(&measured(16_384, 2_048, 4_096, 0, 8));
    assert!((m.free_ram_gb - 6.0).abs() < 0.01, "{}", m.free_ram_gb);
    assert!((m.ram_gb - 16.0).abs() < 0.01);
}

#[test]
fn the_machine_is_the_one_measured_not_the_laptop_written_down() {
    let m = machine_from(&measured(65_536, 40_000, 0, 24_576, 24));
    assert_eq!(m.cpu_cores, 24);
    assert!((m.vram_gb - 24.0).abs() < 0.01);
    assert!(!m.has_npu, "an NPU is only claimed when measured");
    assert!(!m.has_nvidia, "CUDA can't be told from VRAM alone, so it isn't guessed");
}

#[test]
fn what_is_possible_here_follows_the_measured_card() {
    let mut obs = Observations::default();
    obs.asked_for_something_missing("generate a video of the beach");
    let big = recommend(&obs, &machine_from(&measured(65_536, 40_000, 0, 24_576, 24)));
    let small = recommend(&obs, &machine_from(&measured(16_384, 4_000, 0, 0, 8)));
    assert!(big.iter().any(|r| r.id.starts_with("missing-") && r.possible_here));
    assert!(small.iter().any(|r| r.id.starts_with("missing-") && !r.possible_here));
}

#[test]
fn the_usable_memory_quoted_is_this_machines() {
    let mut obs = Observations::default();
    for i in 0..5 {
        obs.time("think", 9_000, i);
    }
    let recs = recommend(&obs, &machine_from(&measured(8_192, 1_024, 1_024, 0, 4)));
    let smaller = recs.iter().find(|r| r.id == "llm-smaller").expect("a slow think earns a smaller model");
    assert!(smaller.benefit.contains("2.0GB"), "{}", smaller.benefit);
}

#[test]
fn nothing_measured_nothing_recommended() {
    let recs = recommend(&Observations::default(), &machine_from(&measured(16_384, 8_000, 0, 0, 8)));
    assert!(recs.is_empty(), "never a recommendation without a measurement");
    assert!(!ask(&recs).is_empty(), "it still answers, plainly");
}

#[test]
fn the_daemon_asks_against_the_measured_machine() {
    let d = crate::common::source_of("daemon");
    assert!(d.contains("crate::wants::machine_from(&crate::fit::measure())"));
    let code: String = d.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    assert!(!code.contains("wants::Machine::default()"), "recommendations against the written-down laptop again");
}
