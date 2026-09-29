//! Three things Atlas described in its own voice and nobody had built.
//!
//! Found by `tests/it_can_do_what_it_says.rs`, which reads every phrase Atlas
//! tells you to say and checks something listens. These three did not:
//!
//! * **"finish setting up"** — `firstrun.rs` tells you there are things to
//!   come back to and to say this when you want to. `FirstRun::resume` had
//!   existed since firstrun was written, called by nothing.
//! * **"stop telling me about the backups"** — `config/tools.yaml` offers it
//!   as the alternative to hand-editing `interrupt.muted`.
//!   `interrupt::mute_from` had existed, called by nothing.
//! * **"this is me"** — `config/tools.yaml` says to say it to the camera once
//!   and the album has your face. `Album::remember_face` had existed, reached
//!   only by `NameThis`, which cannot be said without naming something.
//!
//! The pattern in all three is the same and is worth stating once: the
//! *capability* was built and tested, and the *door* was a sentence in a
//! string literal. Nothing connected them, and nothing failed, because
//! nothing was wrong with either half on its own.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::interrupt::{mute_from, unmute_from, Muted};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;

fn parser() -> Parser {
    let cfg = Config::load(std::path::Path::new("config")).expect("config");
    Parser::new(&cfg.commands)
}

fn cfg() -> Config {
    Config::load(std::path::Path::new("config")).expect("config")
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-three-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

// --- the doors open now ----------------------------------------------------

#[test]
fn the_three_sentences_reach_something() {
    // The narrow claim, and the one that was false for weeks.
    let p = parser();
    assert_eq!(p.parse("finish setting up"), Intent::FinishSetup);
    assert_eq!(p.parse("this is me"), Intent::ThisIsMe);
    assert!(
        matches!(p.parse("stop telling me about the backups"), Intent::MuteTopic(_)),
        "muting by saying so still reaches nothing"
    );
}

#[test]
fn the_way_back_from_a_mute_is_a_sentence_too() {
    // A switch that only goes one way is how somebody wonders, three weeks
    // later, why Atlas never mentions their backups. `unmute_from` was
    // written with this test, not before it.
    let p = parser();
    assert!(matches!(
        p.parse("start telling me about the backups again"),
        Intent::MuteTopic(_)
    ));
    assert!(matches!(p.parse("unmute the backups"), Intent::MuteTopic(_)));
}

#[test]
fn a_muted_topic_keeps_the_whole_sentence_so_the_subject_survives() {
    // `raw_argument`, because `mute_from` reads the lead-in as well as what
    // follows it. A trimmed argument would arrive here as "the backups" with
    // no way to tell muting from unmuting.
    match parser().parse("stop telling me about the backups") {
        Intent::MuteTopic(said) => {
            assert!(said.contains("stop telling me about"), "the lead-in was trimmed off: {said}");
            assert!(said.contains("backups"), "the subject was lost: {said}");
        }
        other => panic!("{other:?}"),
    }
}

// --- muting, and unmuting --------------------------------------------------

#[test]
fn unmuting_is_read_before_muting_or_it_would_never_happen() {
    // The trap in this pair is "unmute the backups". `mute_from` looks for
    // the lead-in "mute" anywhere in the sentence, and "unmute" contains it,
    // so the shortest way of asking to hear about something again is *also* a
    // valid mute. Read in the wrong order it silences the topic instead, and
    // reports that it has -- the failure nobody catches, because the reply is
    // the one you'd get either way.
    assert_eq!(unmute_from("unmute the backups"), Some("the backups".into()));
    assert_eq!(
        mute_from("unmute the backups"),
        Some("the backups".into()),
        "this test is pointless unless the mute reader also claims the sentence"
    );
    // And the longer phrasing, which only the unmute reader claims.
    assert_eq!(
        unmute_from("start telling me about the backups again"),
        Some("the backups".into())
    );
    assert_eq!(mute_from("start telling me about the backups again"), None);
}

#[test]
fn a_mute_survives_a_restart_without_touching_your_config() {
    // Stored, not written back into `config/tools.yaml`. Nothing in this tree
    // rewrites a person's config file, and a spoken aside reformatting a file
    // they hand-edit -- comments, ordering and all -- is not where to start.
    let store = Store::new(tmp("muted"));
    let mut muted = Muted::load(&store);
    assert!(muted.topics.is_empty(), "a fresh install starts quiet about nothing");

    assert!(muted.mute("the backups"));
    assert!(!muted.mute("the backups"), "muting twice reported as a change");
    muted.save(&store).unwrap();

    let back = Muted::load(&store);
    assert_eq!(back.topics, vec!["the backups".to_string()]);
    assert!(back.spoken().contains("the backups"), "{}", back.spoken());
}

#[test]
fn unmuting_something_that_was_never_muted_says_so() {
    // Rather than reporting success. "I'll mention that again" about
    // something never silenced is a small lie that teaches you the command
    // works when it did nothing.
    let mut muted = Muted::default();
    assert!(!muted.unmute("the backups"));
    muted.mute("the backups");
    assert!(muted.unmute("the backups"));
    assert!(muted.topics.is_empty());
    assert!(muted.spoken().contains("not keeping quiet"), "{}", muted.spoken());
}

#[test]
fn muting_is_case_and_space_insensitive_because_speech_is() {
    let mut muted = Muted::default();
    assert!(muted.mute("  The Backups  "));
    assert!(!muted.mute("the backups"), "the same topic went in twice in two spellings");
    assert!(muted.unmute("THE BACKUPS"));
}

#[test]
fn an_empty_topic_is_not_a_mute() {
    // Otherwise "mute" on its own silences a topic called "", which matches
    // every announcement `interrupt` ever considers -- `Thing::about`
    // contains the empty string always. That is Atlas going permanently
    // silent because of one clipped sentence.
    let mut muted = Muted::default();
    assert!(!muted.mute(""));
    assert!(!muted.mute("   "));
    assert!(muted.topics.is_empty());
}

// --- the same ordering, where the decision actually lives ------------------

#[test]
fn the_daemon_reads_an_unmute_as_an_unmute() {
    // The tests above prove the two *readers* disagree about "unmute the
    // backups", which is only half the claim. The order they are consulted in
    // is a line in `daemon::mute_topic`, and until this test existed nothing
    // in the tree executed that function at all -- swapping the two blocks
    // left the whole suite green. A comment explaining why the order matters,
    // guarding nothing, is how the order gets swapped by somebody tidying up.
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "mute-order");

    let muted = d.execute(&Intent::MuteTopic("stop telling me about the backups".into()));
    assert!(muted.contains("stop mentioning the backups"), "{muted}");

    let back = d.execute(&Intent::MuteTopic("unmute the backups".into()));
    assert!(
        back.contains("mention the backups again"),
        "asking to hear about it again was read as another mute: {back}"
    );

    // And it really is un-muted, not merely spoken about.
    let quiet = d.execute(&Intent::MuteTopic("what are you keeping quiet about".into()));
    assert!(quiet.contains("not keeping quiet about anything"), "{quiet}");
}

#[test]
fn the_daemon_says_the_way_back_when_it_mutes() {
    // The reply carries its own undo. Nothing else in the tree tells you the
    // sentence, so a mute whose reply omits it is a one-way door.
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "mute-wayback");
    let said = d.execute(&Intent::MuteTopic("stop telling me about the backups".into()));
    assert!(
        said.contains("start telling me about the backups"),
        "the reply does not say how to undo it: {said}"
    );
    // And that sentence is one the parser can actually hear -- an undo
    // instruction Atlas cannot follow is worse than none.
    assert!(matches!(
        parser().parse("start telling me about the backups"),
        Intent::MuteTopic(_)
    ));
}

#[test]
fn a_mute_said_out_loud_is_still_muted_after_a_restart() {
    // End to end, through the daemon rather than through `Muted` directly:
    // the store is what carries it, and a daemon that muted in memory only
    // would pass every test above.
    let c = cfg();
    let p = plat();
    {
        let mut d = daemon(&c, &p, "mute-restart");
        d.execute(&Intent::MuteTopic("stop telling me about the backups".into()));
    }
    let store = Store::new(std::env::temp_dir().join("atlas-three-mute-restart"));
    let back = Muted::load(&store);
    assert_eq!(back.topics, vec!["the backups".to_string()]);
}

// --- finishing setup -------------------------------------------------------

#[test]
fn finishing_setup_comes_back_to_what_was_skipped() {
    use atlas::firstrun::{FirstRun, Step};
    let store = Store::new(tmp("firstrun"));
    let mut fr = FirstRun::load(&store);

    // Nothing skipped: there is nothing to come back to, and saying so is
    // the answer rather than starting setup over.
    assert_eq!(fr.resume(), None);

    fr.record(Step::Microphones, "", true);
    fr.record(Step::Voice, "", true);
    fr.save(&store).unwrap();

    let mut back = FirstRun::load(&store);
    let step = back.resume().expect("a skipped step was not offered again");
    assert!(
        matches!(step, Step::Microphones | Step::Voice),
        "it resumed something that was never skipped"
    );
    // And it says what it is in words rather than as a variant name.
    assert!(!step.plain().is_empty());
    assert!(!step.plain().contains("Step"), "a Rust name reached the sentence");
}

#[test]
fn every_step_can_say_what_it_is() {
    // `hub_is_not_code.rs` catches `{:?}` reaching a person. This is the
    // other half: the type has something to say instead.
    use atlas::firstrun::Step;
    for step in [
        Step::Hello,
        Step::FindApps,
        Step::Monitors,
        Step::Microphones,
        Step::Voice,
        Step::Missing,
        Step::HowToUse,
        Step::Done,
    ] {
        let said = step.plain();
        assert!(said.len() > 4, "{said:?} is not a description");
        assert!(
            said.chars().next().unwrap().is_lowercase(),
            "{said:?} is written to sit mid-sentence"
        );
    }
}

// --- and the guard that found them -----------------------------------------

#[test]
fn nothing_here_is_still_listed_as_a_gap() {
    // The list in `it_can_do_what_it_says.rs` names phrases Atlas says that
    // reach nothing. All three of these were on it. A built thing that stays
    // on a list of what is owed is how the list stops being read.
    let text = std::fs::read_to_string("tests/it_can_do_what_it_says.rs").expect("the guard");
    let after = text.split("NOT_A_COMMAND_ON_ITS_OWN").nth(1).expect("the list");
    let body = &after[..after.find("];").unwrap_or(after.len())];
    // Comment lines are not entries. The block that held these three still
    // names them, in the note explaining why it is empty -- which is the
    // record worth keeping, and not the same thing as an exemption.
    let entries: String = body
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for phrase in ["finish setting up", "stop telling me about the backups", "this is me"] {
        assert!(
            !entries.contains(&format!("\"{phrase}\"")),
            "\"{phrase}\" is built and still listed as an unbuilt gap"
        );
    }
}
