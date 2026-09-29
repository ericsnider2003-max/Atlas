use atlas::handoff::{
    extract_blocks, read_answer, should_ask, spoken, write_brief, HandoffConfig, Problem, Snippet,
    Try, Usable,
};
use atlas::sandbox::Attempt;

fn cfg() -> HandoffConfig {
    HandoffConfig::default()
}

fn attempt(passed: bool) -> Attempt {
    Attempt { command: "cargo test".into(), passed, output: "…".into(), at: 0 }
}

fn stuck() -> Problem {
    Problem {
        goal: "make the wake word survive a device change".into(),
        error: "thread 'main' panicked at src/voice.rs:212: no such device".into(),
        tried: vec![
            Try {
                theory: "the device list was stale".into(),
                change: "re-enumerated before each turn".into(),
                outcome: "same panic".into(),
                worked: false,
            },
            Try {
                theory: "the name had trailing whitespace".into(),
                change: "trimmed it".into(),
                outcome: "different error, now it finds nothing at all".into(),
                worked: false,
            },
        ],
        snippets: vec![Snippet {
            path: "src/voice.rs".into(),
            from_line: 200,
            text: "fn listen() {\n    let d = device(&cfg.name);\n}".into(),
        }],
        test_output: "test wake_survives_device_change ... FAILED".into(),
        theory: Some("the device handle is cached somewhere I haven't found".into()),
        ruled_out: vec!["the config is being read correctly".into()],
    }
}

// ================= when to stop trying =================

#[test]
fn atlas_works_through_every_angle_before_interrupting_you() {
    // Three attempts is only meaningful if they're three different attempts,
    // and a model left alone will retry the same idea with the wording
    // changed. So this counts distinct approaches, and the default is the
    // whole ladder.
    let c = cfg();
    assert!(c.attempts_before_asking >= 12);
    let few: Vec<_> = (0..3).map(|_| attempt(false)).collect();
    assert!(!should_ask(&few, &c), "three tries is not enough to give up");

    let many: Vec<_> = (0..12).map(|_| attempt(false)).collect();
    assert!(should_ask(&many, &c));
}

#[test]
fn you_can_still_shorten_it_if_you_want_it_to_ask_sooner() {
    let impatient = HandoffConfig { attempts_before_asking: 2, ..cfg() };
    assert!(should_ask(&[attempt(false), attempt(false)], &impatient));
}

#[test]
fn a_problem_that_solved_itself_is_not_escalated() {
    let solved = vec![attempt(false), attempt(false), attempt(false), attempt(true)];
    assert!(!should_ask(&solved, &cfg()), "the last attempt worked");
}

// ================= the brief =================

#[test]
fn the_brief_leads_with_what_it_was_trying_to_do() {
    let b = write_brief(&stuck(), &cfg());
    let goal = b.find("What I'm trying to do").unwrap();
    let error = b.find("What happens").unwrap();
    assert!(goal < error, "intent before symptom");
}

#[test]
fn everything_already_tried_is_listed_so_it_is_not_suggested_again() {
    // This is most of the difference between a good bug report and a bad one.
    let b = write_brief(&stuck(), &cfg());
    assert!(b.contains("re-enumerated before each turn"));
    assert!(b.contains("trimmed it"));
    assert!(b.contains("Already ruled out"));
    assert!(b.contains("config is being read correctly"));
}

#[test]
fn the_error_and_the_test_output_are_included_verbatim() {
    let b = write_brief(&stuck(), &cfg());
    assert!(b.contains("no such device"));
    assert!(b.contains("wake_survives_device_change"));
}

#[test]
fn only_the_code_that_matters_is_sent_not_the_whole_file() {
    let b = write_brief(&stuck(), &cfg());
    assert!(b.contains("src/voice.rs"));
    assert!(b.contains("from line 200"), "so the reader can find it");
    assert!(b.len() < cfg().max_chars);
}

#[test]
fn a_huge_snippet_is_cut_rather_than_pasted_whole() {
    let mut p = stuck();
    p.snippets[0].text = (0..500).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let b = write_brief(&p, &cfg());
    assert!(b.contains("more lines"), "it says what it cut");
    assert!(b.len() < cfg().max_chars + 200);
}

#[test]
fn a_wall_of_output_keeps_both_ends() {
    // The first error and the final summary are the useful parts.
    let mut p = stuck();
    p.error = format!("error: the real problem\n{}\nsummary: 3 failed", "noise\n".repeat(2000));
    let b = write_brief(&p, &cfg());
    assert!(b.contains("error: the real problem"));
    assert!(b.contains("trimmed"));
}

#[test]
fn the_brief_asks_for_the_change_rather_than_an_explanation() {
    let b = write_brief(&stuck(), &cfg());
    assert!(b.contains("the change itself"), "so it can be applied and tested");
}

#[test]
fn atlas_says_out_loud_what_it_is_stuck_on_before_handing_it_over() {
    let said = spoken(&stuck());
    assert!(said.contains("Tried 2 things"));
    assert!(said.contains("want to hand it over?"), "you decide: {said}");
}

// ================= taking the answer back =================

#[test]
fn code_is_pulled_out_of_a_reply() {
    let reply = "The problem is the cached handle.\n\nIn `src/voice.rs`:\n\n\
                 ```rust\nfn listen() { let d = fresh_device(); }\n```\n\nThat should do it.";
    match read_answer(reply) {
        Usable::Apply(blocks) => {
            assert_eq!(blocks.len(), 1);
            assert!(blocks[0].code.contains("fresh_device"));
            assert_eq!(blocks[0].path.as_deref(), Some("src/voice.rs"), "so it knows where it goes");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn several_files_in_one_answer_each_keep_their_own_path() {
    let reply = "First, src/voice.rs:\n```rust\nfn a() {}\n```\n\
                 Then src/config.rs:\n```rust\nfn b() {}\n```";
    let blocks = extract_blocks(reply);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].path.as_deref(), Some("src/voice.rs"));
    assert_eq!(blocks[1].path.as_deref(), Some("src/config.rs"));
}

#[test]
fn an_answer_that_asks_a_question_back_is_recognised_as_such() {
    // Common, and not a failure.
    let reply = "That's odd. Which version of the audio driver are you on?";
    match read_answer(reply) {
        Usable::Asked(q) => assert!(q.ends_with('?')),
        o => panic!("{o:?}"),
    }
}

#[test]
fn an_explanation_with_no_code_says_what_it_still_needs() {
    let reply = "The handle is being cached in the config struct, which is why re-enumerating \
                 doesn't help. You'll want to move it.";
    match read_answer(reply) {
        Usable::NeedsCode(why) => assert!(why.contains("ask for the change itself")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn an_empty_code_fence_is_not_mistaken_for_an_answer() {
    assert!(matches!(read_answer("Here:\n```\n```\nthat's it."), Usable::NeedsCode(_)));
}

#[test]
fn the_language_of_each_block_is_kept() {
    let blocks = extract_blocks("```yaml\nkey: value\n```");
    assert_eq!(blocks[0].language, "yaml");
    assert!(blocks[0].code.contains("key: value"));
}
