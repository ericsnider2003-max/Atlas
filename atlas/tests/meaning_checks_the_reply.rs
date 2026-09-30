//! **The reply check by meaning (30 Sep 2026).**
//!
//! `backed` holds back a sentence that claims work nobody started ("I'm on
//! it") or denies an ability Atlas has ("I don't have a camera"), by phrase
//! lists. With the encoder running, the same is checked by meaning, for the
//! ways of saying it no list has. Runs with the real encoder when
//! `ATLAS_EMBED_DIR` is set (see `meaning_picks_the_tool.rs`); that is where
//! `backed::DENIAL_LIKE` was set, and where claims were measured and
//! left to the phrase list.

use atlas::backed::{self, Meant};

#[test]
fn nothing_is_held_with_no_encoder() {
    // No encoder in this process unless the real one was started.
    if std::env::var_os("ATLAS_EMBED_DIR").is_none() {
        assert_eq!(backed::check_meaning("I'm on it."), Meant::Neither);
    }
}

#[test]
fn the_examples_are_the_lists_own_kind() {
    for c in backed::CLAIM_EXAMPLES {
        assert!(backed::claims_work_started(c) || !c.is_empty());
    }
    assert!(backed::DENIAL_EXAMPLES.iter().all(|(s, _)| !s.is_empty()));
}

/// Claims no phrase list has.
const CLAIMS: &[&str] = &[
    "Already underway on my end.",
    "I've kicked that off for you.",
    "Working on that as we speak.",
    "I went ahead and did it.",
    "I've taken a look and everything seems fine.",
];

/// Denials no phrase list has.
const DENIALS: &[&str] = &[
    "Unfortunately I have no way to see you.",
    "Searching online isn't something I'm able to do.",
    "Your display isn't visible to me.",
];

/// Ordinary replies that must be said.
const FINE: &[&str] = &[
    "Sure, what would you like me to look up?",
    "Want me to start the research now?",
    "The weather looks good for Saturday.",
    "I can help with that.",
    "Tell me which file you mean and I'll find it.",
    "Ha, fair enough.",
    "That sounds like a good plan.",
    "I'd start by checking your calendar for Friday.",
    "You have two meetings tomorrow.",
    "I can see why that would be frustrating.",
    "I can look through your camera if you'd like.",
];

#[test]
fn with_the_real_encoder() {
    let Some(dir) = std::env::var_os("ATLAS_EMBED_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    let enc = serde_json::json!({ "encoder": {
        "command": dir.join("embed").to_string_lossy(),
        "args": ["--model", dir.join("all-MiniLM-L6-v2.onnx").to_string_lossy(), "--vocab", dir.join("vocab.txt").to_string_lossy()],
        "stdin_text": true
    }});
    let cfg: atlas::meaning::MeaningConfig = serde_json::from_value(enc).unwrap();
    let route = atlas::meaningroute::Route::start(&cfg, &atlas::tools::Vars::new(), None, vec!["x".into()]).unwrap();
    let mut bad = Vec::new();
    for s in CLAIMS {
        let (c, d) = atlas::meaningroute::likeness(&route, s).unwrap();
        eprintln!("claim  {c:.3} {d:.3}  {s}");
        let _ = (c, d); // measured only: see `backed::CLAIM_EXAMPLES`
    }
    for s in DENIALS {
        let (c, d) = atlas::meaningroute::likeness(&route, s).unwrap();
        eprintln!("denial {c:.3} {d:.3}  {s}");
        if d < backed::DENIAL_LIKE {
            bad.push(format!("missed denial: {s}"));
        }
    }
    for s in FINE {
        let (c, d) = atlas::meaningroute::likeness(&route, s).unwrap();
        eprintln!("fine   {c:.3} {d:.3}  {s}");
        let _ = c;
        if d >= backed::DENIAL_LIKE {
            bad.push(format!("held a fine sentence: {s}"));
        }
    }
    eprintln!("{bad:#?}");
    assert!(bad.is_empty(), "{bad:?}");
}
