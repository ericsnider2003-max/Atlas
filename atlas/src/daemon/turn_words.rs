//! A turn's words: rephrasing, model-server waits, what this machine can't do.
//!
//! Moved out of `daemon.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// Does a daemon given `store` keep the install's vault
/// (`roots::install_state`, `vault::Vault::FILE`: one vault per copy of
/// Atlas, whoever is using it)? Yes when `store` is this install's -- the
/// owner's or a profile's -- or when you named the install (`ATLAS_HOME`).
/// Otherwise the vault is kept in `store` itself.
///
/// 30 Sep 2026: every daemon went to the install's state whatever store it
/// was given, so every test daemon, each in its own temporary store, shared
/// the one vault in the checkout's `data/state`. One test saved a vault
/// under its own passphrase there, and from then on every test that opened
/// the vault with another was told "that isn't the passphrase" -- eleven
/// failures in a full run, from a file no test had meant to create.
pub(super) fn keeps_the_install_vault(store: &crate::store::Store) -> bool {
    // The install vault belongs to the install state, not to every profile
    // opened while ATLAS_HOME is configured.  Sharing it with a temporary or
    // profile store lets another daemon change the on-disk snapshot underneath
    // this one, so a legitimate sign-in is rejected as stale.
    store.root() == crate::roots::install_state().root()
}

/// The tools whose result is information to put into words, not an action
/// to acknowledge.
pub(super) fn reads_back(i: &Intent) -> bool {
    matches!(
        i,
        Intent::Agenda(_)
            | Intent::FindFile(_)
            | Intent::WhatIHave(_)
            | Intent::MarketDay(_)
            | Intent::Recap
            | Intent::MachineHealth
            | Intent::WaitingFor(_)
            | Intent::TimeSpent(_)
            | Intent::HowAmIDoing
            | Intent::KnowledgeSize
            | Intent::Outstanding
    )
}

/// Long or list-shaped enough to be worth a model call to say naturally.
pub(super) fn worth_rephrasing(reply: &str) -> bool {
    let r = reply.trim();
    !r.is_empty() && !is_a_failure(r) && (r.split_whitespace().count() > 25 || r.lines().filter(|l| !l.trim().is_empty()).count() > 2)
}

/// A tool result that says something didn't happen. It is said as written:
/// handed to the model to reword, "error: unknown app" came back as "I'm
/// focusing on the quarterly budget now" (self-test, 30 Sep 2026).
pub(super) fn is_a_failure(reply: &str) -> bool {
    let r = reply.trim_start().to_lowercase();
    ["error", "i couldn't", "i could not", "i can't", "i cannot", "i don't know", "couldn't ", "can't ", "no such", "nothing matched", "unknown "]
        .iter()
        .any(|p| r.starts_with(p))
}

/// What a worker that died without an answer amounts to.
pub(super) fn no_answer_came_back() -> brain::Decision {
    brain::Decision {
        intent: Intent::Say(String::new()),
        say: "Model unreachable: the model stopped before it answered.".into(),
        model: brain::Reached::No,
    }
}

/// What's left of `reply` once the sentences already spoken are taken out.
pub(super) fn not_yet_said(reply: &str, spoken: &[String]) -> String {
    let mut rest = reply.to_string();
    for s in spoken {
        for candidate in [s.trim().to_string(), crate::persona::strip_filler(s).trim().to_string()] {
            if candidate.is_empty() {
                continue;
            }
            if let Some(i) = rest.find(&candidate) {
                rest.replace_range(i..i + candidate.len(), "");
                break;
            }
        }
    }
    rest.split_whitespace().collect::<Vec<_>>().join(" ").trim_start_matches(['.', ',', ' ']).to_string()
}

/// Is `rest` nothing but the stock acknowledgement for the action
/// (`brain::default_say`) -- "Opening Chrome." -- with nothing it found or
/// failed to do?
pub(super) fn only_acknowledges(rest: &str, stock: &str) -> bool {
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>();
    let (r, k) = (norm(rest), norm(stock));
    let r = r.split_whitespace().collect::<Vec<_>>().join(" ");
    let k = k.split_whitespace().collect::<Vec<_>>().join(" ");
    !r.is_empty() && (r == k || (r.starts_with(&k) && r.split_whitespace().count() <= k.split_whitespace().count() + 2))
}

/// What is said about the other things asked for in the same breath, which
/// weren't done.
pub(super) fn one_at_a_time(also: &[String]) -> String {
    format!("One thing at a time: I haven't done {} -- ask me for that next.", also.join(" or "))
}

/// The pause before starting the model server again, after `deaths` young
/// deaths in a row: a minute, doubling, at most half an hour.
pub fn model_server_pause(deaths: u32) -> std::time::Duration {
    let secs = MODEL_SERVER_RECHECK.as_secs().saturating_mul(1u64 << deaths.min(5));
    std::time::Duration::from_secs(secs.min(30 * 60))
}

/// Does the sentence open by asking for the ways to do something ("what are
/// the ways…", "list the ways…", "how else could you…")? `ways_in_help`
/// answered "other ways" anywhere in a sentence.
pub(super) fn opens_with_ways(said: &str) -> bool {
    let w = crate::intent::normalize(said);
    let w = crate::intent::without_fillers(&w);
    [
        "what are the ways", "what are all the ways", "what are the other ways", "what other ways", "what ways",
        "list the ways", "list all the ways", "how else could you", "how else can you", "all the ways", "every way",
        "other ways", "give me the ways", "tell me the ways",
    ]
    .iter()
    .any(|p| w.starts_with(p))
}

/// Does the sentence open as a decision to work through ("should I…",
/// "help me decide…"), or ask to get back to one? `decide::wants_working`
/// matched "should i" anywhere, so "what should I eat" became a decision.
pub(super) fn opens_with_deciding(said: &str) -> bool {
    let w = crate::intent::normalize(said);
    let w = crate::intent::without_fillers(&w);
    if w.contains("back to the decision") || w.contains("that decision again") || w.starts_with("set aside ") || w.starts_with("rule out ") {
        return true;
    }
    [
        "should i ", "should we ", "help me decide", "i cant decide", "cant decide", "i can't decide", "which should i",
        "which should we", "which one should", "is it worth", "im torn between", "i'm torn between", "torn between",
        "do i go with", "would it be better to",
    ]
    .iter()
    .any(|p| w.starts_with(p))
}

/// A failed model call, in words: what went wrong and that the next message
/// tries again (`brain` puts "Model unreachable: <why>" in `say`).
pub fn model_failed_words(why: &str) -> String {
    // The phone app with no model yet: what to say to get one, not an error.
    if why.contains(crate::phonemode::ASK_ONLINE) {
        return crate::phonemode::ASK_ONLINE.to_string();
    }
    if why.contains("no language model on this phone yet") {
        return crate::phonemode::NO_MODEL_YET.to_string();
    }
    let mut why = why.trim().trim_start_matches("Model unreachable:").trim().to_string();
    // The error's kind, once or twice over ("platform: platform: I couldn't
    // reach..."), and the promise to try again, said twice (the real-model
    // run, 30 Sep 2026): once each, in words.
    while let Some(rest) = why.strip_prefix("platform:").or_else(|| why.strip_prefix("Platform:")) {
        why = rest.trim().to_string();
    }
    for again in ["I'll try again with your next message.", "I'll try again with your next message"] {
        why = why.replace(again, "");
    }
    let why = why.trim().trim_end_matches(['.', ' ']);
    // Names the source the way the connections board does
    // (`integrations::MODEL`) and says what it cost: the answer is missing.
    let model = crate::integrations::MODEL;
    if why.is_empty() {
        format!("I couldn't get an answer from {model}, so the answer you asked for is missing. I'll try again with your next message.")
    } else {
        format!("I couldn't get an answer from {model} ({why}), so the answer you asked for is missing. I'll try again with your next message.")
    }
}

/// The start-up "what I can't do here", checked against what is really
/// installed (29 Sep 2026: Eric's Atlas said "I can't look at your screen
/// and understand it" at every start, from a sizing rule that wanted 6 GB of
/// graphics memory of its own, while the picture reader setup fetched --
/// Qwen3-VL and its picture encoder -- was on the laptop and working). The
/// sizing plan says what a machine like this could run; the files say what
/// this one does. `pictures` is the picture reader's own readiness.
pub fn what_this_machine_cant_do(limits: Vec<String>, pictures: Option<std::result::Result<(), String>>, have_model: bool) -> Vec<String> {
    limits
        .into_iter()
        .filter_map(|l| {
            if l.contains("look at your screen") {
                return match &pictures {
                    Some(Ok(())) => None,
                    Some(Err(why)) => Some(format!("I can't look at your screen and understand it: {why}.")),
                    None => Some(l),
                };
            }
            if l.starts_with("No language model fits") && have_model {
                return None;
            }
            Some(l)
        })
        .collect()
}

/// What to say when a fresh pick differs from the microphone in use; `None`
/// when it is the same one.
pub fn microphone_change(now_name: &str, now_device: &str, picked: &crate::hearing::Picked) -> Option<String> {
    if picked.device == now_device || (!now_name.is_empty() && picked.name == now_name) {
        return None;
    }
    Some(format!(
        "Listening with {} now -- {}.",
        crate::hearing::short(&picked.name),
        picked.why.trim().trim_end_matches('.')
    ))
}
