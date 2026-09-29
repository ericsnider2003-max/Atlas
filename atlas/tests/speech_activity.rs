use atlas::activity::{Journal, Kind};
use atlas::speech::{acknowledge, is_interruption, split, Delivery};
use atlas::store::Store;
use std::cell::RefCell;

/// A reply said the way Atlas says one, with `say` as the speaker (false:
/// playback failed) and `listen` asked before each chunk and once at the
/// end. 28 Sep 2026: `speech::deliver` did this and nothing in Atlas called
/// it any more -- replies go through `speakthread::Saying` -- so these tests
/// drive `Saying` itself (on the loop, as a speaker that can't be handed to
/// a thread is), keeping every rule they pinned.
fn deliver(text: &str, say: &mut dyn FnMut(&str) -> bool, listen: &mut dyn FnMut() -> Option<String>) -> Delivery {
    struct Say<'a>(RefCell<&'a mut dyn FnMut(&str) -> bool>);
    impl atlas::daemon::Mouth for Say<'_> {
        fn speak(&self, text: &str) -> atlas::error::Result<()> {
            if (self.0.borrow_mut())(text) {
                Ok(())
            } else {
                Err(atlas::error::AtlasError::Platform("the player failed".into()))
            }
        }
    }
    struct Nothing;
    impl atlas::speakthread::Host for Nothing {
        fn line(&mut self, _chunk: &str) {}
        fn between(&mut self) {}
    }
    let mouth = Say(RefCell::new(say));
    let mut s = atlas::speakthread::Saying::start(&mouth, false, None);
    s.add(text);
    s.wait(&mut Nothing, listen);
    s.finish().delivery
}

fn recorder() -> (RefCell<Vec<String>>, ) {
    (RefCell::new(Vec::new()),)
}

// ================= interrupting Atlas =================

#[test]
fn a_reply_is_broken_into_sentence_sized_chunks() {
    // Sentence granularity is where stopping sounds deliberate rather than
    // glitchy.
    let c = split("Workspace online. Chrome is up. Anything else?");
    assert_eq!(c.len(), 3);
    assert_eq!(c[0], "Workspace online.");
}

#[test]
fn a_rambling_sentence_is_still_interruptible() {
    let long = format!("{} , and then some more text after that.", "word ".repeat(40));
    assert!(split(&long).len() > 1, "a 200-char sentence must not be one unstoppable chunk");
}

#[test]
fn a_short_reply_is_a_single_chunk() {
    assert_eq!(split("Working."), vec!["Working."]);
    assert_eq!(split("no punctuation here"), vec!["no punctuation here"]);
}

#[test]
fn only_an_explicit_stop_or_pause_counts_as_an_interruption() {
    for s in ["stop", "pause", "hold on", "wait", "cancel"] {
        assert!(is_interruption(s), "{s:?} should interrupt");
    }
}

#[test]
fn a_cough_a_colleague_or_a_backchannel_does_not_cut_atlas_off() {
    // Pure energy detection treats "mm-hmm" as an interruption when it means
    // keep going. This is the false-barge-in case that makes assistants feel
    // twitchy.
    for s in ["mm", "mm hmm", "yeah", "uh huh", "right", "okay so anyway I told him"] {
        assert!(!is_interruption(s), "{s:?} must not interrupt");
    }
}

#[test]
fn an_uninterrupted_reply_is_spoken_in_full() {
    let (said,) = recorder();
    let d = deliver(
        "One. Two. Three.",
        &mut |c| { said.borrow_mut().push(c.to_string()); true },
        &mut || None,
    );
    assert_eq!(said.borrow().len(), 3);
    assert!(!d.was_interrupted());
    assert!(d.unspoken.is_empty());
}

#[test]
fn saying_stop_cuts_it_off_at_the_next_chunk_boundary() {
    let (said,) = recorder();
    let heard = RefCell::new(vec![None, Some("stop".to_string())]);
    let d = deliver(
        "One. Two. Three.",
        &mut |c| { said.borrow_mut().push(c.to_string()); true },
        &mut || heard.borrow_mut().remove(0),
    );
    assert_eq!(said.borrow().len(), 1, "stopped after the first sentence");
    assert!(d.was_interrupted());
    assert_eq!(d.interrupted_by.as_deref(), Some("stop"));
}

#[test]
fn what_was_cut_off_is_never_recorded_as_having_been_said() {
    // The specific bug this prevents: Atlas believing it told you something
    // you never heard, then referring back to it.
    let heard = RefCell::new(vec![None, Some("stop".to_string())]);
    let d = deliver("First part. Second part. Third part.", &mut |_| true, &mut || {
        heard.borrow_mut().remove(0)
    });
    assert_eq!(d.spoken, vec!["First part.".to_string()]);
    assert_eq!(d.remaining_text(), "Second part. Third part.");
}

#[test]
fn the_rest_can_be_picked_back_up() {
    let heard = RefCell::new(vec![None, Some("hold on".to_string())]);
    let d = deliver("Alpha. Beta. Gamma.", &mut |_| true, &mut || heard.borrow_mut().remove(0));
    assert_eq!(d.remaining_text(), "Beta. Gamma.");
}

#[test]
fn unrelated_speech_during_a_reply_is_ignored_and_the_reply_completes() {
    let (said,) = recorder();
    let heard = RefCell::new(vec![
        Some("yeah".to_string()),
        Some("mm hmm".to_string()),
        Some("sarah did you see this".to_string()),
        None,
    ]);
    let d = deliver(
        "One. Two. Three.",
        &mut |c| { said.borrow_mut().push(c.to_string()); true },
        &mut || heard.borrow_mut().remove(0),
    );
    assert_eq!(said.borrow().len(), 3, "must finish despite the noise");
    assert!(!d.was_interrupted());
}

#[test]
fn stop_at_the_very_end_is_still_registered() {
    let heard = RefCell::new(vec![None, None, Some("stop".to_string())]);
    let d = deliver("One. Two.", &mut |_| true, &mut || heard.borrow_mut().remove(0));
    assert!(d.was_interrupted());
    assert!(d.unspoken.is_empty(), "everything was said, but you still asked it to stop");
}

#[test]
fn playback_failing_does_not_pretend_the_rest_was_said() {
    let n = RefCell::new(0);
    let d = deliver("One. Two. Three.", &mut |_| {
        let mut c = n.borrow_mut();
        *c += 1;
        *c < 2
    }, &mut || None);
    assert_eq!(d.spoken.len(), 1);
    assert_eq!(d.unspoken.len(), 2);
    assert!(!d.was_interrupted(), "a failure is not an interruption");
}

#[test]
fn the_acknowledgement_is_short_because_you_wanted_the_floor() {
    let heard = RefCell::new(vec![Some("stop".to_string())]);
    let d = deliver("Long explanation here.", &mut |_| true, &mut || heard.borrow_mut().remove(0));
    let a = acknowledge(&d);
    assert_eq!(a, "Stopped.");
    assert!(a.len() < 20);
}

// ================= what happened while you were away =================

#[test]
fn nothing_happening_is_said_plainly() {
    assert!(Journal::default().brief(0).contains("Nothing happened"));
}

#[test]
fn irreversible_things_are_reported_first() {
    let mut j = Journal::default();
    j.record_at(Kind::Upkeep, "rotated logs", true, 100);
    j.record_at(Kind::Scheduled, "index refresh", true, 110);
    j.record_at(Kind::Published, "posted to X", true, 120);
    let b = j.brief(0);
    let post_at = b.find("posted to X").expect("the post must be mentioned");
    let job_at = b.find("scheduled job").expect("the job must be mentioned");
    assert!(post_at < job_at, "what went public leads: {b}");
}

#[test]
fn housekeeping_is_never_reported() {
    let mut j = Journal::default();
    j.record_at(Kind::Upkeep, "compacted approvals", true, 100);
    j.record_at(Kind::Upkeep, "reaped a helper", true, 110);
    assert!(j.brief(0).contains("Nothing happened"), "you don't need to hear about log rotation");
}

#[test]
fn failures_are_named_not_glossed_over() {
    let mut j = Journal::default();
    j.record_at(Kind::Published, "posted to LinkedIn", false, 100);
    assert!(j.brief(0).contains("failed"), "got: {}", j.brief(0));
}

#[test]
fn a_flood_of_posts_is_summarised_rather_than_listed() {
    let mut j = Journal::default();
    for i in 0..9 {
        j.record_at(Kind::Published, &format!("posted item {i}"), true, 100 + i);
    }
    let b = j.brief(0);
    assert!(b.contains("6 more posts"), "got: {b}");
    assert!(b.len() < 200, "must stay speakable: {b}");
}

#[test]
fn only_events_since_you_left_are_reported() {
    let mut j = Journal::default();
    j.record_at(Kind::Published, "old post", true, 100);
    j.record_at(Kind::Published, "new post", true, 500);
    let b = j.brief(400);
    assert!(b.contains("new post") && !b.contains("old post"), "got: {b}");
}

#[test]
fn there_is_an_audit_of_everything_that_changed_the_world() {
    let mut j = Journal::default();
    j.record_at(Kind::Published, "posted to X", true, 100);
    j.record_at(Kind::Offered, "suggested indexing", true, 110);
    j.record_at(Kind::Scheduled, "ran backup", true, 120);
    let c = j.consequential(0);
    assert_eq!(c.len(), 2, "offers changed nothing outside Atlas");
}

#[test]
fn the_journal_is_bounded() {
    let mut j = Journal::default();
    for i in 0..900 {
        j.record_at(Kind::Asked, &format!("thing {i}"), true, i);
    }
    assert!(j.events.len() <= 400);
    assert!(j.last().unwrap().what.contains("899"), "the recent end is what's kept");
}

// ================= state surviving an upgrade =================

#[test]
fn saved_state_round_trips() {
    let d = std::env::temp_dir().join("atlas-schema-a");
    let _ = std::fs::remove_dir_all(&d);
    let s = Store::new(&d);
    let mut j = Journal::default();
    j.record_at(Kind::Published, "posted", true, 1);
    j.save(&s).unwrap();
    assert_eq!(Journal::load(&s).events.len(), 1);
}

#[test]
fn an_older_file_from_before_versioning_still_loads() {
    // An existing install must not lose its state on upgrade.
    let d = std::env::temp_dir().join("atlas-schema-b");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("activity.json"),
        r#"{"events":[{"at":1,"kind":"published","what":"legacy post","ok":true}]}"#,
    )
    .unwrap();
    let j = Journal::load(&Store::new(&d));
    assert_eq!(j.events.len(), 1, "pre-envelope files must still load");
}

#[test]
fn a_file_from_a_future_version_is_preserved_not_destroyed() {
    // Without this, downgrading or a bad upgrade silently wipes months of
    // learning: parse fails, default is returned, next save overwrites.
    let d = std::env::temp_dir().join("atlas-schema-c");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("activity.json"), r#"{"schema":99,"data":{"events":[]}}"#).unwrap();

    let s = Store::new(&d);
    let _: Journal = Journal::load(&s);
    assert_eq!(s.preserved().len(), 1, "the old file must be kept aside");
    assert!(!d.join("activity.json").exists(), "and moved out of the way");
}

#[test]
fn a_corrupt_file_is_preserved_rather_than_clobbered() {
    let d = std::env::temp_dir().join("atlas-schema-d");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("activity.json"), "{ half written").unwrap();

    let s = Store::new(&d);
    let j = Journal::load(&s);
    assert!(j.events.is_empty(), "starts clean");
    assert_eq!(s.preserved().len(), 1, "but the damaged file is kept for you");
}
