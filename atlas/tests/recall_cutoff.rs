//! A weak hit beside a strong one.
//!
//! The absolute floor keeps out noise. It does not keep out the thing that
//! actually spoils an answer: a mediocre match sitting next to a good one.
//!
//! Ask how many days holiday you get. The policy scores 0.92, a note saying
//! holidays are counted separately scores 0.87, and an old draft with a
//! different number scores 0.51. All three clear any absolute floor. Handing
//! all three onward supplies two contradictory answers and no way to tell
//! which is real — and a contradiction is worse than a gap, because a gap
//! sends you to look.

use atlas::recall::{Clarity, Hit, Library};

fn hit(title: &str, score: f32) -> Hit {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    Hit {
        id: NEXT.fetch_add(1, Ordering::Relaxed),
        title: title.to_string(),
        source: "note".into(),
        score,
        why: String::new(),
        quote: String::new(),
    }
}

// --- clarity ----------------------------------------------------------------

#[test]
fn two_near_tied_hits_are_reported_as_a_disagreement() {
    // Not a confident answer with a runner-up. A question the notes answer
    // two ways.
    let hits = vec![hit("holiday policy 2026", 0.92), hit("holiday policy 2024", 0.89)];
    match Library::clarity(&hits) {
        Clarity::TwoEquallyGood { first, second } => {
            assert_eq!(first, "holiday policy 2026");
            assert_eq!(second, "holiday policy 2024");
        }
        other => panic!("a tie was read as a clear answer: {other:?}"),
    }
}

#[test]
fn a_clear_winner_is_left_alone() {
    let hits = vec![hit("the policy", 0.92), hit("a passing mention", 0.30)];
    assert_eq!(Library::clarity(&hits), Clarity::OneClearAnswer);
    assert!(Library::clarity(&hits).caveat().is_none());
}

#[test]
fn a_single_hit_is_a_clear_answer() {
    assert_eq!(Library::clarity(&[hit("only one", 0.4)]), Clarity::OneClearAnswer);
}

#[test]
fn nothing_found_says_so_rather_than_reading_as_clear() {
    assert_eq!(Library::clarity(&[]), Clarity::NothingFound);
}

#[test]
fn the_disagreement_caveat_names_both_and_asks_which_is_current() {
    let hits = vec![hit("policy A", 0.9), hit("policy B", 0.88)];
    let c = Library::clarity(&hits).caveat().unwrap();
    assert!(c.contains("policy A") && c.contains("policy B"));
    assert!(c.contains("current"));
}

#[test]
fn the_threshold_is_a_whisker_not_a_gulf() {
    // 0.7 against 0.9 is a real preference and must not be called a tie, or
    // every search reads as ambiguous and the caveat stops meaning anything.
    let hits = vec![hit("strong", 0.9), hit("weaker", 0.7)];
    assert_eq!(Library::clarity(&hits), Clarity::OneClearAnswer);
}

// --- the relative floor -----------------------------------------------------

#[test]
fn the_default_relative_floor_is_set_and_sane() {
    use atlas::recall::RecallConfig;
    let c = RecallConfig::default();
    assert!(c.relative_floor > 0.0 && c.relative_floor < 1.0);
    // Above 0.5 and a genuine second-best gets cut; below 0.25 and it does
    // nothing the absolute floor was not already doing.
    assert!((0.25..=0.7).contains(&c.relative_floor), "got {}", c.relative_floor);
}
