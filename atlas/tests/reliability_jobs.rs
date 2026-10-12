//! Focused core-job checks; also included in the consolidated full suite.
#![allow(
    clippy::all,
    reason = "test file; CI lints the library only (`cargo clippy --lib`)"
)]
mod common;
#[path = "finishing_what_it_starts.rs"]
mod finishing_what_it_starts;
#[path = "taking_things_off_outstanding.rs"]
mod taking_things_off_outstanding;
