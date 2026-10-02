//! Local-first model with an optional stronger fallback.
//!
//! The project rule is offline-first, online-secondary. `FallbackLlm` makes
//! that a type rather than a habit: everyday completions run on the local
//! model, the hard drafts (a self-fix, code from a description) escalate to a
//! stronger model when one is configured, and a failed local call falls
//! through to the secondary rather than failing outright. With no secondary,
//! it is exactly the local model — an offline install is unchanged.

use atlas::brain::{FallbackLlm, Llm};
use atlas::error::{AtlasError, Result};
use std::sync::Arc;

/// A model that answers with a fixed label, so a test can see which one ran.
struct Named(&'static str);
impl Llm for Named {
    fn complete(&self, _: &str, _: &str) -> Result<String> {
        Ok(self.0.to_string())
    }
}

/// A model that is down.
struct Down;
impl Llm for Down {
    fn complete(&self, _: &str, _: &str) -> Result<String> {
        Err(AtlasError::Platform("connection refused".into()))
    }
}

#[test]
fn with_no_secondary_everything_is_the_local_model() {
    let f = FallbackLlm::new(Arc::new(Named("local")), None);
    assert_eq!(f.complete("", "").unwrap(), "local", "ordinary work is local");
    assert_eq!(f.complete_hard("", "").unwrap(), "local", "even a hard task is local when there's nothing stronger");
}

#[test]
fn ordinary_work_stays_local_even_with_a_secondary() {
    let f = FallbackLlm::new(Arc::new(Named("local")), Some(Arc::new(Named("cloud"))));
    // A plain completion must not reach for the online model — that would put
    // every prompt on the network, which is the thing the rule forbids.
    assert_eq!(f.complete("", "").unwrap(), "local");
}

#[test]
fn a_hard_task_escalates_to_your_own_stronger_model() {
    let f = FallbackLlm::new(Arc::new(Named("local")), Some(Arc::new(Named("server")))).secondary_is_your_own();
    assert_eq!(f.complete_hard("", "").unwrap(), "server", "the hard draft goes to your own stronger model");
}

#[test]
fn a_hard_task_stays_local_when_the_secondary_is_online() {
    // Offline first (Eric, 30 Sep 2026): the free online models are a
    // fallback for a hard task, never the first choice.
    let f = FallbackLlm::new(Arc::new(Named("local")), Some(Arc::new(Named("cloud"))));
    assert_eq!(f.complete_hard("", "").unwrap(), "local");
    let f = FallbackLlm::new(Arc::new(Down), Some(Arc::new(Named("cloud"))));
    assert_eq!(f.complete_hard("", "").unwrap(), "cloud", "online only when local can't answer");
}

#[test]
fn a_failed_local_call_falls_through_to_the_secondary() {
    let f = FallbackLlm::new(Arc::new(Down), Some(Arc::new(Named("cloud"))));
    // The local model is down; rather than fail, the completion runs on the
    // secondary.
    assert_eq!(f.complete("", "").unwrap(), "cloud", "resilience: a down local model falls through");
}

#[test]
fn a_hard_task_falls_back_to_local_if_the_secondary_is_down() {
    let f = FallbackLlm::new(Arc::new(Named("local")), Some(Arc::new(Down)));
    // The secondary was configured but is unreachable (offline). The hard task
    // still gets done on the local model rather than failing the whole job.
    assert_eq!(f.complete_hard("", "").unwrap(), "local", "a down secondary doesn't sink the task");
}

#[test]
fn with_no_secondary_a_down_local_still_errors() {
    // Nothing to fall through to: the error is real and surfaces.
    let f = FallbackLlm::new(Arc::new(Down), None);
    assert!(f.complete("", "").is_err(), "a down local with no backup is an honest failure");
}

#[test]
fn the_self_fix_draft_path_really_uses_the_stronger_model() {
    // Not just the wrapper in isolation — the code-generation path that self-fix
    // and build both run drafts through `complete_hard`, so with a secondary
    // configured the draft comes from the stronger model.
    let f = FallbackLlm::new(Arc::new(Named("local model output")), Some(Arc::new(Named("stronger model output"))))
        .secondary_is_your_own();
    let drafted = atlas::build_it::fix_draft("some broken code", "the tool complained", atlas::craft::Lang::Python, &f)
        .expect("it drafts a fix");
    assert_eq!(drafted, "stronger model output", "the fix draft routed to the secondary via complete_hard");
}
