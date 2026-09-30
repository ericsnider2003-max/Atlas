//! **Choosing tools by meaning as well as words (30 Sep 2026).**
//!
//! The router matched words only, so a request that meant a tool without
//! saying its words wasn't offered it. With the encoder running resident
//! (`meaningroute`), meaning adds those tools; words still lead, and small
//! talk still gets nothing.
//!
//! The first tests use made-up vectors and always run. The last runs the
//! real encoder when `ATLAS_EMBED_DIR` names a folder holding the `embed`
//! program, `all-MiniLM-L6-v2.onnx` and `vocab.txt`; that is where
//! `router::MEANING_FLOOR` was set.

use atlas::router::Router;
use std::path::Path;

fn router() -> Router {
    let cfg = atlas::config::Config::load(Path::new("config")).unwrap();
    Router::new(&atlas::intent::ToolBook::new(&cfg.commands))
}

/// A vector pointing at one tool: 1 on its own axis.
fn toward(n: usize, i: usize, how: f32) -> Vec<f32> {
    let mut v = vec![0.0f32; n];
    v[i] = how;
    v[(i + 1) % n] = (1.0 - how * how).max(0.0).sqrt();
    v
}

/// One made-up vector per line of meaning: every line of tool `i` points
/// along axis `i`.
fn lines(r: &Router) -> Vec<Vec<f32>> {
    let n = r.names().len();
    let each = r.meaning_owners();
    each.iter().map(|&i| toward(n, i, 1.0)).collect()
}

#[test]
fn meaning_adds_the_tool_the_words_missed() {
    let r = router();
    let names = r.names();
    let n = names.len();
    let tools = lines(&r);
    let find = names.iter().position(|x| x == "find_file").unwrap();
    // No word of find_file's in it.
    let said = "I can't locate that thing the accountant sent over";
    let words: Vec<String> = r.shortlist(said, 4).into_iter().map(|(e, _)| e.name.clone()).collect();
    assert!(!words.contains(&"find_file".to_string()), "words alone already found it: {words:?}");
    let q = toward(n, find, 0.8);
    let both: Vec<String> = r.shortlist_meaning(said, 4, Some((&q, &tools))).into_iter().map(|(e, _)| e.name.clone()).collect();
    assert!(both.contains(&"find_file".to_string()), "{both:?}");
}

#[test]
fn words_the_meaning_agrees_with_lead_and_small_talk_gets_nothing() {
    let r = router();
    let names = r.names();
    let n = names.len();
    let tools = lines(&r);
    let mail = names.iter().position(|x| x == "mail").unwrap();
    let q = toward(n, mail, 0.9);
    let words: Vec<String> = r.shortlist("check my email", 4).into_iter().map(|(e, _)| e.name.clone()).collect();
    assert_eq!(words.first().map(String::as_str), Some("mail"));
    let both: Vec<String> = r.shortlist_meaning("check my email", 4, Some((&q, &tools))).into_iter().map(|(e, _)| e.name.clone()).collect();
    assert_eq!(both.first().map(String::as_str), Some("mail"), "{both:?}");
    assert!(r.shortlist_meaning("Thanks, have a good day.", 4, Some((&q, &tools))).is_empty());
}

#[test]
fn a_weak_word_match_the_meaning_says_is_far_off_is_dropped() {
    let r = router();
    let names = r.names();
    let n = names.len();
    let tools = lines(&r);
    let said = "I can't locate that thing the accountant sent over";
    let q = toward(n, names.iter().position(|x| x == "find_file").unwrap(), 0.8);
    let like = r.meaning_of(&q, &tools);
    let both: Vec<String> = r.shortlist_meaning(said, 4, Some((&q, &tools))).into_iter().map(|(e, _)| e.name.clone()).collect();
    assert!(both.contains(&"find_file".to_string()), "{both:?}");
    assert!(both.len() <= 4, "{both:?}");
    for (e, score) in r.shortlist(said, 4) {
        let i = names.iter().position(|x| *x == e.name).unwrap();
        if score < atlas::router::FLOOR * 2.0 && like[i] < atlas::router::MEANING_FAR {
            assert!(!both.contains(&e.name), "{} kept on a weak word match: {both:?}", e.name);
        }
    }
}

#[test]
fn a_weak_likeness_is_not_offered() {
    let r = router();
    let names = r.names();
    let n = names.len();
    let tools = lines(&r);
    let q = toward(n, names.iter().position(|x| x == "find_file").unwrap(), atlas::router::MEANING_FLOOR - 0.1);
    let got = r.shortlist_meaning("blorp the wibble", 4, Some((&q, &tools)));
    assert!(got.iter().all(|(e, _)| e.name != "find_file"));
}

#[test]
fn every_tool_has_a_line_to_embed() {
    let r = router();
    assert_eq!(r.texts().len(), r.meaning_owners().len());
    assert!(r.texts().len() >= r.names().len());
    assert!(r.texts().iter().all(|t| t.trim().len() > 3), "{:?}", r.texts().iter().filter(|t| t.trim().len() <= 3).collect::<Vec<_>>());
}

/// Paraphrases of Eric's requests that mostly don't share the words, each
/// with the tool it means.
const MEANT: &[(&str, &str)] = &[
    ("I can't locate that thing the accountant sent over", "find_file"),
    ("my computer feels sluggish lately", "machine_health"),
    ("what have I got going on tomorrow", "agenda"),
    ("jot down that the plumber is coming thursday", "capture"),
    ("anything new in my inbox", "mail"),
    ("my desktop is a mess, sort it out", "tidy_desktop"),
    ("run a checkup on how you're doing", "self_check"),
    ("dig into how small language models can be sped up", "research"),
];

const CHAT: &[&str] = &["Thank you.", "Thanks, have a good day.", "Keep talking about it.", "haha fair enough", "good morning"];

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
    let vars = atlas::tools::Vars::new();
    let r = router();
    let route = atlas::meaningroute::Route::start(&cfg, &vars, None, r.texts().to_vec()).expect("the encoder starts");
    let t0 = std::time::Instant::now();
    while route.tools().is_none() && t0.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let tools = route.tools().expect("every tool embedded");
    eprintln!("{} lines embedded in {:?}", tools.len(), t0.elapsed());
    let names = r.names();
    let mut missed = Vec::new();
    for (said, want) in MEANT {
        let q = route.text(said).unwrap();
        let mut by: Vec<(f32, &String)> = r.meaning_of(&q, tools).into_iter().zip(&names).collect();
        by.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let got: Vec<String> = r.shortlist_meaning(said, 4, Some((&q, tools))).into_iter().map(|(e, _)| e.name.clone()).collect();
        let want_cos = by.iter().find(|(_, n)| n.as_str() == *want).map(|b| b.0).unwrap_or(0.0);
        eprintln!("{said:?} want {want} ({want_cos:.3}); top {:?}; picked {got:?}", &by[..3]);
        if !got.iter().any(|g| g == want) {
            missed.push(*said);
        }
    }
    for said in CHAT {
        let q = route.text(said).unwrap();
        let words = r.shortlist(said, 4).len();
        let got = r.shortlist_meaning(said, 4, Some((&q, tools)));
        let best = r.meaning_of(&q, tools).into_iter().fold(0.0f32, f32::max);
        eprintln!("chat {said:?}: best {best:.3}, words {words}, picked {}", got.len());
        assert!(got.len() <= words, "{said:?}: meaning added tools to small talk");
    }
    // His real requests still get their tool with meaning switched on.
    let mut lost = Vec::new();
    for (said, want) in crate::the_right_tools_for_the_sentence::HIS_REQUESTS {
        let q = route.text(said).unwrap();
        let got: Vec<String> = r.shortlist_meaning(said, 4, Some((&q, tools))).into_iter().map(|(e, _)| e.name.clone()).collect();
        let words: Vec<String> = r.shortlist(said, 4).into_iter().map(|(e, _)| e.name.clone()).collect();
        if words.iter().any(|w| w == want) && !got.iter().any(|g| g == want) {
            lost.push((*said, *want, got));
        }
    }
    assert!(lost.is_empty(), "meaning lost what the words had: {lost:?}");
    // One timed sentence, resident.
    let t = std::time::Instant::now();
    route.sentence("could you look up flights to denver").unwrap();
    eprintln!("one sentence: {:?}", t.elapsed());
    // Two of these are beyond this small encoder, and are kept here as the
    // honest record: "the thing the accountant sent" sits as close to
    // receipts and mail as to files, and "dig into how X can be sped up" is
    // mostly its topic, so it reads as the model tools (30 Sep 2026).
    assert!(missed.len() <= 2, "meaning missed {missed:?}");
}


/// The encoder inside Atlas gives the program's vectors.
#[test]
fn the_encoder_inside_atlas_matches_the_program() {
    let Some(dir) = std::env::var_os("ATLAS_EMBED_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    let root = std::env::temp_dir().join(format!("atlas-native-meaning-{}", std::process::id()));
    let into = root.join("models/understanding");
    std::fs::create_dir_all(&into).unwrap();
    std::fs::copy(dir.join("all-MiniLM-L6-v2.onnx"), into.join("all-MiniLM-L6-v2.onnx")).unwrap();
    std::fs::copy(dir.join("vocab.txt"), into.join("vocab.txt")).unwrap();
    let native = atlas::meaningroute::Resident::native(&root).expect("installed");
    let enc = serde_json::json!({ "encoder": {
        "command": dir.join("embed").to_string_lossy(),
        "args": ["--model", dir.join("all-MiniLM-L6-v2.onnx").to_string_lossy(), "--vocab", dir.join("vocab.txt").to_string_lossy()],
        "stdin_text": true
    }});
    let cfg: atlas::meaning::MeaningConfig = serde_json::from_value(enc).unwrap();
    let program = atlas::meaningroute::Resident::start(&cfg, &atlas::tools::Vars::new()).unwrap();
    for s in ["what do I know about apples", "put the dentist on my calendar for friday at 3", ""] {
        let a = native.embed(s, std::time::Duration::from_secs(30)).unwrap();
        let b = program.embed(s, std::time::Duration::from_secs(30)).unwrap();
        assert_eq!(a.len(), 384);
        let c = atlas::router::cosine(&a, &b);
        assert!(c > 0.9999, "{s:?}: {c}");
    }
    let t = std::time::Instant::now();
    native.embed("could you look up flights to denver", std::time::Duration::from_secs(5)).unwrap();
    eprintln!("inside Atlas, one sentence (debug build): {:?}", t.elapsed());
    let _ = std::fs::remove_dir_all(&root);
}

/// A request that plainly names a tool is sure; his small talk and his
/// questions about the world aren't (`Router::sure_of`, the forced call).
#[test]
fn sure_only_of_plain_requests() {
    let r = router();
    for (said, _) in crate::the_right_tools_for_the_sentence::HIS_REQUESTS {
        eprintln!("sure={} {said}", r.sure_of(said, None));
    }
    for said in ["my laptop is running slow, what's eating the memory", "check my email", "organize my desktop", "how did my last youtube video do"] {
        assert!(r.sure_of(said, None), "{said}");
    }
    for said in [
        "hey, how's it going", "tell me something interesting about octopuses", "why do they have three hearts",
        "Thanks, have a good day.", "haha fair enough", "wait, what do you mean", "what are volcanic islands made of",
        "write a haiku about rain", "continue the story", "what should I eat tonight",
    ] {
        assert!(!r.sure_of(said, None), "{said}");
    }
}


/// "How much space have I got left on this thing" went to the model's own
/// call log with a real 4B model (30 Sep 2026): space is the machine's.
#[test]
fn space_left_is_the_machines_health() {
    let r = router();
    let names: Vec<String> = r.shortlist("how much space have I got left on this thing", 2).into_iter().map(|(e, _)| e.name.clone()).collect();
    assert_eq!(names.first().map(String::as_str), Some("machine_health"), "{names:?}");
}
