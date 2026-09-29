//! `Daemon::readings` returned `Readings::default()` — every field zero — for
//! the entire life of the module.
//!
//! It hid because `assess` only reports on values above zero, so an all-zero
//! machine looks like a machine with nothing wrong. Every test passed. The
//! doctor passed. It only surfaced when `faithful` started naming what it
//! could not read: "2 things didn't work: memory, disk."
//!
//! These tests exist so a stub cannot come back quietly.

use atlas::health::{assess, read_machine, summary, HealthConfig, Readings, Severity};

#[test]
fn the_machine_is_actually_read_rather_than_defaulted() {
    let r = read_machine();
    // Every machine that can run this has memory. A zero here means the read
    // silently failed, which is the exact bug this file exists to catch.
    assert!(
        r.ram_total_gb > 0.0,
        "memory read as zero — readings are stubbed again"
    );
    assert!(
        r.ram_total_gb < 4096.0,
        "{}GB of RAM is a unit error, not a machine",
        r.ram_total_gb
    );
}

#[test]
fn memory_in_use_never_exceeds_memory_installed() {
    let r = read_machine();
    assert!(
        r.ram_used_gb <= r.ram_total_gb,
        "using {}GB of {}GB — the arithmetic is wrong",
        r.ram_used_gb,
        r.ram_total_gb
    );
    assert!(r.ram_used_gb >= 0.0);
}

#[test]
fn free_disk_never_exceeds_total_disk() {
    let r = read_machine();
    // Zero is permitted — on a platform Atlas does not target the read is
    // deliberately not implemented, and `faithful` says so. What is not
    // permitted is a number that cannot be true.
    assert!(
        r.disk_free_gb <= r.disk_total_gb,
        "{}GB free of {}GB total",
        r.disk_free_gb,
        r.disk_total_gb
    );
}

#[test]
fn nothing_is_guessed_when_it_cannot_be_read() {
    let r = read_machine();
    // A reading Atlas could not take must stay zero. A plausible invented
    // number is worse than an admitted blank, because `faithful` can report a
    // blank and cannot report a lie.
    if r.disk_total_gb == 0.0 {
        assert_eq!(r.disk_free_gb, 0.0, "free space invented without a total");
    }
    if r.ram_total_gb == 0.0 {
        assert_eq!(r.ram_used_gb, 0.0, "memory use invented without a total");
    }
}

#[test]
fn an_all_zero_machine_still_looks_healthy_which_is_why_this_hid() {
    // Documents the reason a stub survived: assess finds nothing wrong with
    // nothing. This is correct behaviour and exactly why it needed catching
    // somewhere other than assess.
    let findings = assess(&Readings::default(), &HealthConfig::default());
    assert!(
        findings.iter().all(|f| f.severity != Severity::Urgent),
        "an empty reading should not manufacture an emergency"
    );
}

#[test]
fn a_real_reading_produces_a_summary_that_mentions_the_machine() {
    let r = read_machine();
    let findings = assess(&r, &HealthConfig::default());
    let s = summary(&r, &findings);
    assert!(!s.trim().is_empty(), "a machine that was read should have something said about it");
}

#[test]
fn reading_twice_gives_the_same_installed_memory() {
    // Used memory moves; installed memory does not. A difference here means
    // the read is picking up something other than what it claims to.
    let (a, b) = (read_machine(), read_machine());
    assert!(
        (a.ram_total_gb - b.ram_total_gb).abs() < 0.01,
        "installed memory changed between two reads: {} then {}",
        a.ram_total_gb,
        b.ram_total_gb
    );
}
