//! Talking to a model the way Atlas does, timed and checked.
//!
//! `atlas talk-bench <model.gguf> [port]` runs a fixed conversation through
//! the real daemon -- the same prompt, tools, history and trimming a spoken
//! turn gets -- against a llama.cpp server already serving that model on
//! `port`, and prints how long each answer took and what it said.
//!
//! 30 Sep 2026: which model the laptop should run was a guess ("the largest
//! that fits"), and Eric asked for options measured on his own machine.
//! Speed alone would pick the smallest model and quality alone the largest,
//! so each answer is also checked for what went wrong on the 29th: claims to
//! be doing something no tool started, "I can't research", emphasis marks,
//! and answers that run past the length asked for.

use std::time::Instant;

/// The conversation every model is given. Mixed on purpose: small talk,
/// advice and a follow-up, questions about Atlas itself, a rough moment, and
/// two things the phrase parser does without the model (timed too, as the
/// floor).
pub const SCRIPT: &[&str] = &[
    "hey, how's it going?",
    "what's a good way to get better at playing guitar?",
    "why that one first?",
    "what are you, and what's your job?",
    "can you do research for me?",
    "I've had a long day and I'm tired",
    "give me three ideas for dinner tonight",
    "what did I ask you about earlier?",
    "what would make you better at your job?",
    "remind me in 20 minutes to stretch",
];

/// One answer, measured.
#[derive(Debug, Clone)]
pub struct Answer {
    pub said: String,
    pub reply: String,
    pub ms: u128,
    /// What was wrong with it, in words; empty when nothing was.
    pub faults: Vec<String>,
}

/// What's wrong with a reply, by the things Eric heard on 29 Sep 2026.
pub fn faults_in(reply: &str, max_sentences: usize) -> Vec<String> {
    let low = reply.to_lowercase();
    let mut out = Vec::new();
    if reply.trim().is_empty() {
        out.push("said nothing".to_string());
    }
    if reply.contains('*') {
        out.push("emphasis marks".to_string());
    }
    for claim in ["i'm already on it", "i'm on it", "i'll get started", "working on it now"] {
        if low.contains(claim) {
            out.push(format!("claims work no tool started (\"{claim}\")"));
        }
    }
    for untrue in [
        "can't do research",
        "cannot do research",
        "don't have a research",
        "not capable of",
        "because you asked",
        "don't care",
        "like how i am",
    ] {
        if low.contains(untrue) {
            out.push(format!("says something untrue of Atlas (\"{untrue}\")"));
        }
    }
    // Made-up shared history or a life of its own (the laptop, 30 Sep 2026:
    // "that time we got stuck in traffic", "that Thai place you like", "an
    // anomaly in network traffic from last Tuesday").
    for invented in [
        "that time we",
        "remember when",
        "reminds me of this time",
        "reminds me of the time",
        "i was just reviewing",
        "i was just thinking",
        "i've been thinking about that",
        "place you like",
        "you always",
        "last tuesday",
        "last night",
        "i'm already doing",
        "already doing it",
    ] {
        if low.contains(invented) {
            out.push(format!("invents history or work (\"{invented}\")"));
        }
    }
    if low.contains(" am ") && (low.contains("2:5") || low.contains("3 am") || low.contains("this hour") || low.contains("this late")) {
        out.push("remarks on the time unasked".to_string());
    }
    let sentences = reply.split(['.', '?', '!']).filter(|s| s.trim().split_whitespace().count() >= 2).count();
    if sentences > max_sentences {
        out.push(format!("{sentences} sentences (asked for at most {max_sentences})"));
    }
    out
}

/// The report, as it's printed.
pub fn report(model: &str, answers: &[Answer]) -> String {
    let mut s = format!("Model: {model}\n");
    let model_ms: Vec<u128> = answers.iter().map(|a| a.ms).collect();
    for a in answers {
        s.push_str(&format!("\n[{} ms] you: {}\n  atlas: {}\n", a.ms, a.said, a.reply.replace('\n', " ")));
        for f in &a.faults {
            s.push_str(&format!("  ! {f}\n"));
        }
    }
    let total: u128 = model_ms.iter().sum();
    let mut sorted = model_ms.clone();
    sorted.sort();
    let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0);
    let faults: usize = answers.iter().map(|a| a.faults.len()).sum();
    s.push_str(&format!(
        "\nSUMMARY {model}: {} answers, median {median} ms, total {total} ms, {faults} faults\n",
        answers.len()
    ));
    s
}

/// Run the script through a daemon talking to `llm`.
pub fn run(cfg: &crate::config::Config, llm: std::sync::Arc<dyn crate::brain::Llm>, store_dir: &std::path::Path) -> Vec<Answer> {
    let plat = crate::platform::mock::MockPlatform::new(vec![crate::platform::Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let _ = std::fs::remove_dir_all(store_dir);
    let _ = std::fs::create_dir_all(store_dir);
    let store = crate::store::Store::new(store_dir.to_path_buf());
    let mut d = crate::daemon::Daemon::new(
        cfg,
        &plat,
        Some(llm),
        store,
        crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
    );
    // A spoken answer (`register::CHAT_SENTENCES`), one over for a list.
    let max = crate::register::CHAT_SENTENCES + 1;
    let mut t = crate::store::now();
    let mut out = Vec::new();
    for said in SCRIPT {
        let started = Instant::now();
        let reply = d.turn(said, t);
        let ms = started.elapsed().as_millis();
        out.push(Answer { said: said.to_string(), faults: faults_in(&reply, max), reply, ms });
        t += 20;
    }
    out
}
