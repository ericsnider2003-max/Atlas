//! An external tool cannot block the caller forever.
//!
//! `ExternalTool::run` ended in `child.wait_with_output()`, which returns when
//! the process does and not before. Every voice tool goes through it: the
//! recorder, whisper, piper. So did the model call and the web fetch.
//!
//! A whisper build sitting on stdin, an ffmpeg waiting on a device that went
//! away, a model stalling on load — any of them held the turn loop for as long
//! as the process lived. From outside, Atlas is dead: no error, nothing in the
//! log, no recovery.
//!
//! These use `sleep` and `cat` on Unix. On Windows the same code path runs
//! against whatever the config names; the timeout logic is the same.

use atlas::tools::{ExternalTool, Vars};
#[cfg(unix)]
use std::time::Instant;

fn tool(command: &str, args: &[&str], timeout_secs: u64) -> ExternalTool {
    ExternalTool {
        command: command.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        stdin_text: false,
        result_file: None,
        timeout_secs,
    }
}

#[cfg(unix)]
#[test]
fn a_process_that_never_finishes_is_killed() {
    let t = tool("sleep", &["30"], 1);
    let started = Instant::now();
    let r = t.run(&Vars::default(), None);
    let took = started.elapsed();

    assert!(r.is_err(), "a hung tool returned success");
    assert!(
        took.as_secs() < 5,
        "the caller was held for {took:?} — the deadline did nothing"
    );
}

#[cfg(unix)]
#[test]
fn the_error_says_what_hung_and_what_to_change() {
    // A timeout that just says "failed" sends you looking in the wrong place.
    let err = tool("sleep", &["30"], 1)
        .run(&Vars::default(), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("sleep"), "got: {err}");
    assert!(err.contains("timeout_secs"), "got: {err}");
}

#[cfg(unix)]
#[test]
fn a_quick_tool_is_untouched() {
    // The fix must not add latency to the common case.
    let started = Instant::now();
    let out = tool("echo", &["hello"], 30)
        .run(&Vars::default(), None)
        .expect("echo should work");
    assert!(out.contains("hello"));
    assert!(started.elapsed().as_millis() < 2000);
}

#[cfg(unix)]
#[test]
fn a_tool_that_writes_a_lot_does_not_deadlock() {
    // The reason the reader threads exist. A child whose pipe buffer fills
    // blocks on write, so a caller that polls without draining deadlocks
    // against the process it is trying to time out. Whisper writes progress
    // to stderr the whole way through, which is exactly this shape.
    let big = tool("sh", &["-c", "for i in $(seq 1 20000); do echo line-$i; done"], 20);
    let started = Instant::now();
    let out = big.run(&Vars::default(), None).expect("should finish");
    assert!(out.lines().count() > 19_000, "output was truncated");
    assert!(
        started.elapsed().as_secs() < 15,
        "it deadlocked on a full pipe buffer"
    );
}

#[cfg(unix)]
#[test]
fn stderr_filling_up_does_not_deadlock_either() {
    let noisy = tool("sh", &["-c", "for i in $(seq 1 20000); do echo err-$i >&2; done; echo ok"], 20);
    let out = noisy.run(&Vars::default(), None).expect("should finish");
    assert!(out.contains("ok"));
}

#[cfg(unix)]
#[test]
fn a_big_piece_of_text_handed_to_a_tool_does_not_deadlock() {
    // The remaining half of the same bug, and the half the drains did not
    // fix. `run` wrote stdin itself, *before* calling `wait_or_kill`:
    //
    //     pipe.write_all(..)?;                  // blocking
    //     let out = wait_or_kill(child, ..)?;   // where the drains start
    //
    // A pipe holds 64KB on Linux and as little as 4KB on Windows. Hand a
    // tool more than that and the write blocks until the tool reads. A tool
    // that is also writing fills its own output pipe, which nobody is
    // draining yet, and blocks too. Neither side can move, and the deadline
    // lives inside the function the write never returns from -- so this
    // hung forever rather than failing after `timeout_secs`.
    //
    // 1MB, because the deadlock needs both pipes full: ~64KB into stdout
    // stops `cat` reading, and ~64KB more into stdin stops us writing.
    let big = "x".repeat(1024 * 1024);
    let mut t = tool("cat", &[], 20);
    t.stdin_text = true;

    let started = Instant::now();
    let out = t.run(&Vars::default(), Some(&big)).expect("cat should finish");
    let took = started.elapsed();

    assert_eq!(out.len(), big.len(), "the text did not come back whole");
    assert!(
        took.as_secs() < 10,
        "1MB through a tool took {took:?}, which is the deadlock, not slowness"
    );
}

#[cfg(unix)]
#[test]
fn a_tool_that_never_reads_its_input_is_still_killed_on_time() {
    // The same shape with the tool refusing to co-operate: it never reads
    // stdin at all, so the pipe fills and stays full. Before, that was a
    // permanent hang. Now the deadline covers the feeding too -- the child
    // is killed, which closes the pipe, and the writer thread gets EPIPE
    // and ends.
    let big = "y".repeat(1024 * 1024);
    let mut t = tool("sh", &["-c", "sleep 30"], 2);
    t.stdin_text = true;

    let started = Instant::now();
    let r = t.run(&Vars::default(), Some(&big));
    let took = started.elapsed();

    assert!(r.is_err(), "a tool that ignored its input and hung returned success");
    assert!(
        took.as_secs() < 8,
        "held for {took:?} against a 2s deadline -- the write blocked outside it"
    );
}

#[cfg(unix)]
#[test]
fn a_failing_tool_still_reports_its_own_error_text() {
    // Draining stderr on a thread must not lose it.
    let err = tool("sh", &["-c", "echo something-specific >&2; exit 3"], 10)
        .run(&Vars::default(), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("something-specific"), "stderr was lost: {err}");
    assert!(err.contains('3'), "the exit code was lost: {err}");
}

#[test]
fn zero_means_the_default_rather_than_no_limit() {
    // A config that omits the field, or sets it to 0, must not mean "wait
    // forever" — that is the behaviour being removed.
    let t = tool("echo", &["x"], 0);
    assert_eq!(t.timeout_secs, 0);
    // Runs and returns; the point is that it does not hang.
    assert!(t.run(&Vars::default(), None).is_ok());
}

#[test]
fn the_default_is_long_enough_to_transcribe_something() {
    let parsed: ExternalTool =
        serde_yaml::from_str("command: whisper\nargs: []\n").expect("minimal config parses");
    assert!(
        parsed.timeout_secs >= 60,
        "the default of {}s would cut off a long transcription",
        parsed.timeout_secs
    );
}

#[test]
fn an_omitted_timeout_gets_the_default_not_zero() {
    let parsed: ExternalTool = serde_yaml::from_str("command: piper\n").expect("parses");
    assert_eq!(parsed.timeout_secs, 120);
}
