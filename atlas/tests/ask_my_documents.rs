//! "Ask my documents": answers from your own notes and readings, a source
//! after every sentence, each checked (nine-repos report, askdocs, 1 Oct 2026).

use atlas::recall::{check_cites, docs_prompt, docs_question, docs_reply};

fn passages() -> Vec<(String, String)> {
    vec![
        ("lease-2026, lines 1-40".into(), "The notice period is 60 days. Rent is 1,450 a month, due on the 1st.".into()),
        ("bakery-plan, lines 1-20".into(), "Opening day is 3 November. We need two more bakers.".into()),
    ]
}

#[test]
fn the_question_is_taken_from_how_it_was_asked() {
    assert_eq!(docs_question("Ask my documents what the notice period is?").as_deref(), Some("what the notice period is"));
    assert_eq!(docs_question("atlas, what do my documents say about the bakery opening").as_deref(), Some("the bakery opening"));
    assert_eq!(docs_question("what's the weather"), None);
}

#[test]
fn the_passages_are_numbered_and_quoted_not_obeyed() {
    let (system, user) = docs_prompt("notice period", &passages());
    assert!(system.contains("ONLY the numbered passages") && system.contains("not instructions"));
    assert!(user.starts_with("[1] lease-2026") && user.contains("[2] bakery-plan") && user.ends_with("Question: notice period"));
}

#[test]
fn a_sentence_that_cites_nothing_or_misquotes_a_figure_is_dropped() {
    let p = passages();
    let a = check_cites(
        "The notice period is 60 days [1]. Rent is 1,500 a month [1]. Opening day is 3 November [2]. It's a good lease. You owe a deposit [4].",
        &p,
    );
    assert_eq!(a.said, "The notice period is 60 days [1]. Opening day is 3 November [2].");
    assert_eq!(a.used, vec![1, 2]);
    assert_eq!(a.dropped, 3);
    let said = docs_reply(&a, &p);
    assert!(said.contains("From: [1] lease-2026, lines 1-40; [2] bakery-plan, lines 1-20."), "{said}");
    assert!(said.ends_with("I left out 3 sentences I couldn't match to your documents."), "{said}");
}

#[test]
fn not_in_your_documents_is_an_answer() {
    let a = check_cites("Your documents don't say.", &passages());
    assert_eq!((a.said.as_str(), a.dropped), ("Your documents don't say.", 0));
    let none = check_cites("The lease ends in 2027.", &passages());
    assert!(docs_reply(&none, &passages()).starts_with("I couldn't give you an answer I could back"));
}

/// The whole way through: a note on disk, the question by voice, the
/// model's answer checked before it's said.
struct Cites;
impl atlas::brain::Llm for Cites {
    fn complete(&self, system: &str, _user: &str) -> atlas::error::Result<String> {
        if system.contains("ONLY the numbered passages") {
            Ok("The notice period is 60 days [1]. It is 90 days in winter [1].".into())
        } else {
            Ok("Fine.".into())
        }
    }
}

#[test]
fn asked_by_voice_the_answer_is_checked_before_it_is_said() {
    use atlas::daemon::Daemon;
    use atlas::platform::{mock::MockPlatform, Monitor};
    use atlas::proactive::{Proactive, ProactiveConfig};
    let mut c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let notes = std::env::temp_dir().join(format!("atlas-askdocs-notes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&notes);
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("lease.md"), "# Lease\n\nThe notice period is 60 days. Rent is due on the 1st.\n").unwrap();
    if let Some(t) = c.tools.as_mut() {
        t.research.notes_dir = notes.display().to_string();
    }
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-askdocs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut d = Daemon::new(&c, &plat, Some(std::sync::Arc::new(Cites)), atlas::store::Store::new(dir), Proactive::new(ProactiveConfig::default()));
    let mut t = atlas::store::now();
    assert_eq!(d.turn("ask my documents what the notice period is", t), "Looking through your documents.");
    let mut out = String::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline && out.is_empty() {
        t += 1;
        if let Some(s) = d.tick(t).into_iter().find(|s| s.contains("notice period")) {
            out = s;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(out.contains("The notice period is 60 days [1].") && out.contains("From: [1] Lease"), "{out}");
    assert!(!out.contains("90 days"), "the figure its passage doesn't hold is dropped: {out}");
    assert!(out.contains("I left out 1 sentence"), "{out}");
}

// ---------- a summary on read ----------

#[test]
fn a_long_document_is_sampled_from_start_middle_and_end() {
    let words: Vec<String> = (0..3000).map(|i| format!("w{i}")).collect();
    let text = words.join(" ");
    let s = atlas::recall::summary_sample(&text, 500);
    assert!(s.starts_with("[The start]\nw0 w1"), "{}", &s[..40]);
    let mid = s.split("[The middle]\n").nth(1).unwrap();
    let first: usize = mid.split_whitespace().next().unwrap().trim_start_matches('w').parse().unwrap();
    assert!((1000..2000).contains(&first), "the middle starts at word {first}");
    assert!(s.contains("[The end]") && s.trim_end().ends_with("w2999"));
    assert!(s.chars().count() < 1700);
    // Odd whitespace doesn't split a character.
    let odd = "\u{3000}".repeat(2000) + &"é ".repeat(2000);
    let _ = atlas::recall::summary_sample(&odd, 500);
    assert_eq!(atlas::recall::summary_sample("short text", 500), "short text");
}

#[test]
fn a_summary_sentence_with_a_figure_the_document_lacks_is_taken_out() {
    let doc = "The lease runs 12 months. Rent is 1,450 a month.";
    let said = atlas::recall::summary_checked("It's a 12 month lease. Rent is 1,450. Deposits total 3,900.", doc);
    assert_eq!(said, "It's a 12 month lease. Rent is 1,450.");
}
