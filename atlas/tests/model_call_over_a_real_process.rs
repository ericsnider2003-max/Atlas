//! The model-call primitive, proven against a REAL subprocess.
//!
//! `brain::Llm` + `ShellLlm` are the tool-slot primitive — "call a model, get
//! text back" — and they were wired into the daemon's decide loop, the
//! council, `explain`, `build_it` and `overnight` during the 18–22 Sep work.
//! But every existing test drove them through `MockLlm` (canned replies in
//! process); the two `ShellLlm` tests assert only config plumbing and say in
//! as many words that "the behaviour needs a running server". So the one thing
//! never shown was the actual round-trip: request template → a real external
//! process on stdin → its JSON on stdout → `response_path` → text a consumer
//! uses.
//!
//! These close that. The stub is a shell one-liner that ignores its stdin and
//! prints a fixed completion in the JSON shape a real endpoint uses — which is
//! all the primitive's contract asks of whatever `tools.yaml` points at. It is
//! not a language model, and it does not make `explain` a "working" capability
//! (that needs a real generative model on real hardware, the same way meaning
//! search needs the embedding model installed). What it proves is that the
//! seam carries text from a separate process into a real consumer, so the only
//! thing standing between this and a live model is the model itself.

#[cfg(unix)]
use atlas::brain::{Llm, LlmConfig, ShellLlm};
#[cfg(unix)]
use atlas::tools::{ExternalTool, Vars};

/// A stand-in "model": reads and discards the request on stdin, prints one
/// canned completion as JSON. `response_path` digs the text back out.
#[cfg(unix)]
fn stub_llm(reply_text: &str) -> ShellLlm {
    let script = format!("cat >/dev/null; printf '%s' '{{\"text\":\"{reply_text}\"}}'");
    ShellLlm {
        cfg: LlmConfig {
            tool: ExternalTool {
                command: "sh".into(),
                args: vec!["-c".into(), script],
                stdin_text: true,
                result_file: None,
                timeout_secs: 30,
            },
            request: "{\"system\":\"{system}\",\"user\":\"{user}\"}".into(),
            response_path: "text".into(),
            vision_request: None,
        },
        vars: Vars::new(),
    }
}

/// The primitive itself: template expanded, a real process run, its JSON
/// parsed, the text at `response_path` returned. Nothing mocked.
#[cfg(unix)]
#[test]
fn complete_runs_a_real_process_and_returns_its_text() {
    let llm = stub_llm("the sum of the two arguments");
    let out = llm.complete("you are terse", "explain fn add(a,b){a+b}").expect("the stub process ran");
    assert_eq!(
        out, "the sum of the two arguments",
        "the primitive did not carry the process's text back through response_path"
    );
}

/// A real consumer end to end: `explain::in_plain_english` drafts through the
/// primitive against the live process and hands back its text. This is the
/// chain a real model will run unchanged — consumer → Llm → subprocess → text.
#[cfg(unix)]
#[test]
fn a_consumer_drafts_through_the_primitive_against_a_live_process() {
    let llm = stub_llm("This function adds its two arguments and returns the sum.");
    let explained = atlas::explain::in_plain_english("fn add(a: i32, b: i32) -> i32 { a + b }", &llm, 2);
    let text = explained.expect("a live model process should yield an explanation");
    assert!(
        text.contains("adds its two arguments"),
        "the consumer did not surface the model's text: {text}"
    );
}

/// A model that errors is surfaced as an error, not silently swallowed — the
/// property that lets a caller fall back rather than act on nothing. A stub
/// that exits non-zero stands in for a model that failed.
#[cfg(unix)]
#[test]
fn a_failed_model_process_is_an_error_not_an_empty_answer() {
    let llm = ShellLlm {
        cfg: LlmConfig {
            tool: ExternalTool {
                command: "sh".into(),
                args: vec!["-c".into(), "echo 'out of memory' >&2; exit 1".into()],
                stdin_text: true,
                result_file: None,
                timeout_secs: 30,
            },
            request: "{\"user\":\"{user}\"}".into(),
            response_path: "text".into(),
            vision_request: None,
        },
        vars: Vars::new(),
    };
    assert!(llm.complete("s", "u").is_err(), "a non-zero model process must be an error");
}
