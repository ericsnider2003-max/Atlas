//! Delegating work to online sub-agents, when the machine is online.
//!
//! Atlas is offline-first: the crew runs errands on this machine, on this
//! machine's model, and that never stops being true. This is the *other*
//! half — when there is a connection, heavy background work can be handed out
//! to a faster worker instead of grinding on the local 3B, and the result
//! pulled back, checked for accuracy here, and either handed to you or used
//! to finish the task without interrupting whatever Atlas is already doing.
//!
//! The first provider is Cloudflare's free tier: Workers AI for the model
//! call, and (later) a Worker for whole compute jobs and Browser Rendering
//! for fetching pages server-side. Nothing here is Cloudflare-specific in the
//! code, though. A provider is reached through the same `LlmConfig`/`curl`
//! path the local model uses (`brain.rs`), so the endpoint, the model name
//! and the bearer token all live in `config/tools.yaml` and the vault — this
//! module is the provider-agnostic delegation logic that sits on top: is a
//! provider ready, send the task, get the result, and **check it before
//! trusting it**.
//!
//! ## Offline is not a downgrade, and online is not a default
//!
//! When there is no connection, or no provider is configured, the local crew
//! does the work exactly as before — this module reports `Blocked` and the
//! caller falls through. When a provider *is* ready, delegation is still the
//! caller's choice per task, not a global switch: the point is to make the
//! machine more capable when it can be, never to move your work off it
//! without asking.
//!
//! ## The check is the point
//!
//! A result from somewhere else is a claim until Atlas has looked at it. Every
//! delegated result is graded here — for how well-grounded it is
//! (`certainty`), and optionally by a second, local pass that reads the answer
//! back and says whether it holds. A result that fails the check is handed
//! over *with the doubt attached*, never as a clean fact. That is what makes
//! "have it analysed in the background for accuracy" true rather than a slogan.

use crate::brain::{Llm, LlmConfig};
use crate::certainty::{self, CertaintyConfig, Confidence, Grounding};
use crate::tools::ExternalTool;
use serde::Deserialize;

/// The Cloudflare provider, as configured in `tools.yaml`.
///
/// Ships disabled and empty: this is the one thing here that reaches a
/// third-party service, so it waits to be asked for and to be given an
/// account and a token, exactly as research does.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CloudflareConfig {
    /// Off until you turn it on. Offline crews do not depend on this.
    pub enabled: bool,
    /// Your Cloudflare account id. Empty means not set up.
    pub account_id: String,
    /// The Workers AI model to run, e.g. a Llama or Mistral variant on the
    /// free tier.
    pub model: String,
    /// The vault entry (a `Kind::ApiKey`) holding the API token. The token
    /// itself is never in config — only the name to look it up by.
    pub token_vault: String,
    /// Re-check every delegated result locally before handing it over. On by
    /// default: the whole reason to pull a result back is to look at it.
    #[serde(default = "CloudflareConfig::verify_default")]
    pub verify: bool,
    /// How to reach Workers AI — a `curl` template exactly like the local
    /// `llm:` block, with `{account_id}` and `{token}` available as vars.
    /// Absent means the provider cannot be reached, however else it is set.
    pub inference: Option<LlmConfig>,
    /// Cloudflare Browser Rendering, for fetching and stripping a page
    /// server-side instead of on this machine's headless Chrome. Optional.
    pub fetch: Option<ExternalTool>,
}

impl CloudflareConfig {
    fn verify_default() -> bool {
        true
    }

    /// Plain-language setup guidance for onboarding and diagnostics.
    /// Cloudflare is an optional per-user speed-up; local crews do not depend
    /// on it and no friend needs an account to use Atlas.
    pub const fn setup_steps() -> [&'static str; 5] {
        [
            "Atlas's local subagents work without Cloudflare or an internet connection.",
            "If you want online delegation, use your own Cloudflare account; never share another user's token.",
            "Create a narrowly scoped Workers AI API token and keep it out of config files.",
            "Save the token in Atlas's protected vault, then enter your account id and chosen model.",
            "Run the readiness check before enabling online delegation; failed checks fall back to local work.",
        ]
    }
}

/// Whether a provider can actually be used right now, and if not, the plain
/// reason — so the capability catalogue and `doctor` can tell the truth
/// rather than claiming an online booster that would fail on first use.
#[derive(Debug, Clone, PartialEq)]
pub enum Readiness {
    Ready,
    Blocked(String),
}

impl Readiness {
    pub fn ready_now(&self) -> bool {
        matches!(self, Readiness::Ready)
    }
    pub fn why(&self) -> &str {
        match self {
            Readiness::Ready => "ready",
            Readiness::Blocked(r) => r,
        }
    }
}

/// Can Atlas delegate to Cloudflare right now? `token_present` is whether the
/// vault holds the named token — checked by the caller, because only the
/// caller can unlock the vault. Order matters: the cheapest, most-likely
/// misconfiguration is named first.
pub fn readiness(cfg: &CloudflareConfig, token_present: bool) -> Readiness {
    if !cfg.enabled {
        return Readiness::Blocked("online delegation is switched off".into());
    }
    if cfg.account_id.trim().is_empty() {
        return Readiness::Blocked("no Cloudflare account id set".into());
    }
    if cfg.model.trim().is_empty() {
        return Readiness::Blocked("no Workers AI model chosen".into());
    }
    if cfg.inference.is_none() {
        return Readiness::Blocked("no inference endpoint configured".into());
    }
    if cfg.token_vault.trim().is_empty() {
        return Readiness::Blocked("no vault entry named for the API token".into());
    }
    if !token_present {
        return Readiness::Blocked(format!(
            "the API token isn't in the vault under \"{}\"",
            cfg.token_vault
        ));
    }
    Readiness::Ready
}

/// The system prompt handed to a delegated worker. Kept plain and
/// task-shaped: the worker is doing a piece of Atlas's work, not holding a
/// conversation.
pub const DELEGATE_SYSTEM: &str =
    "You are a worker completing one task for a personal assistant. Do the task \
     directly and completely. Return only the result, no preamble. If the task \
     cannot be done from what you were given, say exactly what is missing.";

/// The system prompt for the local accuracy check. The verifier's job is to
/// find the problem, not to praise the answer.
pub const VERIFY_SYSTEM: &str =
    "You are checking another worker's answer for a task. Judge only accuracy and \
     completeness. If the answer is sound, reply with the single token OK. \
     Otherwise reply with one sentence naming what is wrong or missing. Do not \
     rewrite the answer.";

/// A delegated result, after it has been checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Delegated {
    pub task: String,
    pub result: String,
    /// How much Atlas trusts it, after grading.
    pub confidence: Confidence,
    /// The doubt to attach when handing it over, empty when there is none.
    pub note: String,
    /// The local check ran and was satisfied.
    pub verified: bool,
}

impl Delegated {
    /// What to actually say when handing this over — the result, with the
    /// doubt attached when there is any, and withheld outright when the check
    /// says it cannot be trusted.
    pub fn spoken(&self) -> String {
        match self.confidence {
            Confidence::Withhold => format!(
                "A worker came back on \"{}\", but I couldn't stand behind it{}. \
                 I'd rather check it again than pass it on as is.",
                self.task,
                if self.note.is_empty() { String::new() } else { format!(" — {}", self.note) }
            ),
            Confidence::Qualify => {
                if self.note.is_empty() {
                    self.result.clone()
                } else {
                    format!("{} ({})", self.result, self.note)
                }
            }
            Confidence::Fine => self.result.clone(),
        }
    }
}

/// Send a task to a delegated worker, pull the result back, and check it.
///
/// `remote` is the worker (a provider's model, built from its `LlmConfig`);
/// `verify` is an optional local model that reads the answer back. Both are
/// `&dyn Llm`, so this is provider-agnostic and testable offline with a mock.
/// A remote that errors is a real failure — reported, not smoothed over.
pub fn dispatch_task(
    task: &str,
    remote: &dyn Llm,
    verify: Option<&dyn Llm>,
    ccfg: &CertaintyConfig,
) -> Result<Delegated, String> {
    let result = remote
        .complete(DELEGATE_SYSTEM, task)
        .map_err(|e| format!("the worker couldn't finish: {e}"))?;
    let result = result.trim().to_string();
    if result.is_empty() {
        return Err("the worker returned nothing".into());
    }

    // A result from a worker that actually did the task is grounded in that
    // work — but it is not about your own machine or files, so it is graded
    // the way a research note is: by whether there is something behind it.
    let grounding = Grounding { from_a_source: true, about_your_world: false, had_the_context: true };
    let (base_confidence, _score, base_note) = certainty::assess(&result, &grounding, ccfg);

    let (confidence, note, verified) =
        verify_result(task, &result, verify, base_confidence, base_note);

    Ok(Delegated { task: task.to_string(), result, confidence, note, verified })
}

/// The local accuracy pass, on its own so both a delegated task and a
/// delegated research summary can be checked the same way. A worker's answer
/// is a claim until Atlas has looked at it; this is where "analysed in the
/// background for accuracy" actually happens.
///
/// Takes the confidence and note the answer already earned (from grounding,
/// say) and can only hold them or lower them — a flagged answer never reads
/// cleaner than it did before the check. A verifier that itself errors does
/// not get to fail the result: the check simply didn't run, which is said
/// rather than treated as a fault in the answer.
pub fn verify_result(
    task: &str,
    result: &str,
    verify: Option<&dyn Llm>,
    base_confidence: Confidence,
    base_note: String,
) -> (Confidence, String, bool) {
    let mut confidence = base_confidence;
    let mut note = base_note;
    let Some(v) = verify else {
        return (confidence, note, false);
    };
    let prompt = format!("Task: {task}\n\nProposed answer:\n{result}");
    match v.complete(VERIFY_SYSTEM, &prompt) {
        Ok(verdict) => {
            let verdict = verdict.trim();
            if says_ok(verdict) {
                return (confidence, note, true);
            }
            // The check found something. Never silently upgrade: a flagged
            // answer can only stay where it is or drop.
            if confidence == Confidence::Fine {
                confidence = Confidence::Qualify;
            }
            note = if note.is_empty() {
                format!("the check flagged: {verdict}")
            } else {
                format!("{note}; the check flagged: {verdict}")
            };
            (confidence, note, false)
        }
        Err(_) => {
            note = if note.is_empty() {
                "couldn't double-check this locally".into()
            } else {
                format!("{note}; couldn't double-check this locally")
            };
            (confidence, note, false)
        }
    }
}

/// Did the verifier say the answer is sound? Deliberately strict: only a bare
/// OK (or an OK-led first word) counts, so a sentence that merely contains the
/// letters "ok" inside a complaint does not read as approval.
fn says_ok(verdict: &str) -> bool {
    let t = verdict.trim().to_lowercase();
    let first: String = t.chars().take_while(|c| c.is_alphanumeric()).collect();
    first == "ok" || first == "okay" || first == "sound" || first == "correct"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::MockLlm;

    fn cc() -> CloudflareConfig {
        CloudflareConfig {
            enabled: true,
            account_id: "acct123".into(),
            model: "@cf/meta/llama".into(),
            token_vault: "cloudflare token".into(),
            verify: true,
            inference: None,
            fetch: None,
        }
    }

    #[test]
    fn readiness_names_the_missing_piece() {
        let mut c = cc();
        c.enabled = false;
        assert!(matches!(readiness(&c, true), Readiness::Blocked(_)));
        let mut c = cc();
        c.account_id = String::new();
        assert_eq!(readiness(&c, true).why(), "no Cloudflare account id set");
        // inference is None in cc(), so even a fully-named setup is blocked
        // until the endpoint is configured.
        assert!(!readiness(&cc(), true).ready_now());
        // With an endpoint but no token, the reason names the vault entry.
        let mut c = cc();
        c.inference = Some(dummy_llm_cfg());
        assert!(readiness(&c, true).ready_now());
        assert!(readiness(&c, false).why().contains("cloudflare token"));
    }

    #[test]
    fn setup_is_optional_and_self_contained() {
        let steps = CloudflareConfig::setup_steps();
        assert_eq!(steps.len(), 5);
        assert!(steps[0].contains("without Cloudflare"));
        assert!(steps[1].contains("your own"));
        assert!(steps[2].contains("narrowly scoped"));
        assert!(steps[4].contains("fall back to local"));
    }

    fn dummy_llm_cfg() -> LlmConfig {
        // Minimal parse of an llm block, just to have Some(_) — never run.
        serde_yaml::from_str(
            "command: curl\nargs: []\nrequest: \"{user}\"\nresponse_path: result.response",
        )
        .unwrap()
    }

    #[test]
    fn a_clean_result_comes_back_fine() {
        let remote = MockLlm("The capital of France is Paris.".into());
        let verify = MockLlm("OK".into());
        let d = dispatch_task("capital of France?", &remote, Some(&verify), &CertaintyConfig::default())
            .unwrap();
        assert!(d.verified);
        assert_eq!(d.confidence, Confidence::Fine);
        assert_eq!(d.spoken(), "The capital of France is Paris.");
    }

    #[test]
    fn a_flagged_result_is_handed_over_with_the_doubt() {
        let remote = MockLlm("The capital of France is Berlin.".into());
        let verify = MockLlm("Wrong — the capital of France is Paris, not Berlin.".into());
        let d = dispatch_task("capital of France?", &remote, Some(&verify), &CertaintyConfig::default())
            .unwrap();
        assert!(!d.verified, "a flagged answer is not verified");
        assert_ne!(d.confidence, Confidence::Fine, "a flagged answer must not read as clean");
        assert!(d.spoken().to_lowercase().contains("flagged"), "the doubt must be attached: {}", d.spoken());
    }

    #[test]
    fn a_worker_that_returns_nothing_is_an_error_not_an_answer() {
        let remote = MockLlm("   ".into());
        let e = dispatch_task("do the thing", &remote, None, &CertaintyConfig::default());
        assert!(e.is_err(), "empty output is a failure, not a silent success");
    }

    #[test]
    fn no_verifier_means_it_runs_but_is_not_marked_verified() {
        let remote = MockLlm("A plain answer.".into());
        let d = dispatch_task("q", &remote, None, &CertaintyConfig::default()).unwrap();
        assert!(!d.verified, "no local check ran, so it is not marked verified");
        assert_eq!(d.result, "A plain answer.");
    }

    #[test]
    fn says_ok_is_strict() {
        assert!(says_ok("OK"));
        assert!(says_ok("okay, that's right"));
        assert!(!says_ok("not ok — the date is wrong"));
        assert!(!says_ok("this looks broken"));
    }
}
