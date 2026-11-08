//! Isolated process target for the daemon-backed brief tests.
//!
//! These tests start asynchronous daemon workers and exercise process-wide
//! clock/state behavior. Keeping them in their own binary prevents unrelated
//! integration modules from sharing that process-global state.

mod common;

#[path = "a_brief_without_an_inbox.rs"]
mod a_brief_without_an_inbox;
