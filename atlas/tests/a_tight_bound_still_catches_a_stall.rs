//! The load allowance for wall-clock bounds (Q19) must not be a way to stop
//! catching stalls. The two stalls that A2 found on 7 Oct were 2.4 s and
//! 3.5-6 s against bounds of 2 s: on an idle machine they must still fail.

use crate::common::{is_prompt, load_factor_for};
use std::time::Duration;

#[test]
fn an_idle_machine_gets_no_slack() {
    assert_eq!(load_factor_for(Duration::ZERO), 1.0);
    assert!(load_factor_for(Duration::from_millis(2)) == 1.0, "a sleep that is 2 ms late is timer noise, not a busy machine");
}

#[test]
fn the_ffmpeg_listing_stall_is_still_caught_when_the_machine_is_idle() {
    let idle = load_factor_for(Duration::from_millis(2));
    assert!(!is_prompt(Duration::from_millis(2400), Duration::from_secs(2), idle), "a 2.4 s stall passed a 2 s bound");
    assert!(!is_prompt(Duration::from_millis(3500), Duration::from_secs(2), idle), "a 3.5 s stall passed a 2 s bound");
}

#[test]
fn a_machine_running_late_gets_slack_in_proportion_and_no_more_than_eight_times() {
    let busy = load_factor_for(Duration::from_millis(40));
    assert!(busy >= 8.0 - f64::EPSILON, "{busy}");
    assert!(is_prompt(Duration::from_millis(2400), Duration::from_secs(2), load_factor_for(Duration::from_millis(10))));
    assert!(!is_prompt(Duration::from_secs(20), Duration::from_secs(2), busy), "even the busiest machine has a limit");
}
