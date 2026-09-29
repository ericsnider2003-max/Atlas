//! Corrections that survive the conversation they were made in.
//!
//! `revise.rs` shipped complete: a `Correction` that knows the difference
//! between a complaint and a lesson, a `Mending` that waits for the repeat, a
//! `home_for` that decides where a lesson has to live to be read again, a
//! `proposal` that names the exact change before making it, and
//! `repeat_rate` — the one number the module says matters.
//!
//! **Nothing ever built a `Correction`.** `heard` never fired, no `Edit` was
//! ever produced, `nudge::offer_to_mend` had no caller, and `repeat_rate`
//! divided by an empty list. Fourth module this session with that shape.
//!
//! But this one could not be fixed by adding a caller, and that is the point
//! of this file. Its **rule 1** is that *where a lesson is written decides
//! whether it works*: a lesson about how a task is done has to be somewhere
//! read **every time**, or it is a note in a diary nobody opens.
//! `daemon::context()` — everything the model ever sees — was displays, apps,
//! the focused window, recent files and the conversation. Nothing learned. So
//! there was nowhere for a lesson to live, and filing one anyway would have
//! produced precisely the failure the module header names.
//!
//! `revise::standing` is that place, and `context` reads it every turn.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::revise::{
    home_for, standing, subject_of, wanted_in, Correction, Edit, Home, Mending, MAX_STANDING,
    REPEATS_NEEDED,
};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const NOW: u64 = 1_700_000_000;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-mend-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

fn an_edit(what: &str, home: Home) -> Edit {
    Edit { about: subject_of(what), home, replacing: None, becomes: what.into(), said_times: 2 }
}

// ============ the place a lesson can live at all =========================

#[test]
fn what_has_been_learned_is_in_front_of_the_model_every_turn() {
    // The whole reason this module could not just be "wired". Before this,
    // `context()` carried nothing learned, so rule 1 could not be satisfied
    // by any caller.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "context");
    d.mending.applied(an_edit("keep replies to one line", Home::HowTo));

    let ctx = d.context();
    assert!(
        ctx.contains("keep replies to one line"),
        "a lesson Atlas agreed to was not in front of the model: {ctx}"
    );
}

#[test]
fn a_fresh_install_carries_no_heading_for_an_empty_list() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "fresh");
    assert!(!d.context().contains("What you have told me"));
    assert_eq!(standing(&[]), "");
}

#[test]
fn instructions_are_carried_before_preferences_when_the_cap_bites() {
    // `Home::read_every_time` is true only for HowTo. If the cap has to drop
    // something it must drop the preference, not the instruction.
    let mut applied: Vec<Edit> = (0..MAX_STANDING)
        .map(|i| an_edit(&format!("preference number {i}"), Home::AboutYou))
        .collect();
    applied.push(an_edit("always check the date first", Home::HowTo));

    let s = standing(&applied);
    assert!(s.contains("always check the date first"), "the instruction was dropped: {s}");
    assert_eq!(s.lines().count() - 1, MAX_STANDING, "the cap stopped applying");
}

#[test]
fn the_standing_list_is_bounded_so_it_cannot_crowd_out_what_you_just_said() {
    let applied: Vec<Edit> =
        (0..MAX_STANDING + 20).map(|i| an_edit(&format!("rule {i}"), Home::HowTo)).collect();
    assert_eq!(standing(&applied).lines().count() - 1, MAX_STANDING);
}

// ============ a complaint is not a lesson ================================

#[test]
fn the_fix_is_taken_out_of_the_sentence_you_said_it_in() {
    assert_eq!(
        wanted_in("that was too long, keep it to one line").as_deref(),
        Some("keep it to one line")
    );
    // The whole instruction, not the fragment after "instead": a bare
    // imperative following the comma is the commonest shape a fix arrives in,
    // and "use the newest instead" is a better rule than "the newest".
    assert_eq!(
        wanted_in("you opened the wrong one, use the newest instead").as_deref(),
        Some("use the newest instead")
    );
    assert_eq!(wanted_in("next time lead with the number").as_deref(), Some("lead with the number"));
}

#[test]
fn a_complaint_with_no_fix_in_it_yields_no_rule() {
    // "That's wrong" is a signal to ask, not a lesson to file. Filing it
    // produces a rule that forbids one thing and teaches none.
    assert_eq!(wanted_in("that's wrong"), None);
    assert_eq!(wanted_in("that's wrong, again"), None, "more complaint was read as a fix");
    assert_eq!(wanted_in("no"), None);
}

#[test]
fn the_same_complaint_said_two_ways_matches_itself() {
    // `slug` strips filler for exactly this reason — the whole sentence would
    // never match twice.
    assert_eq!(subject_of("that was too long, keep it short"), subject_of("way too long, be brief"));
}

#[test]
fn a_correction_with_no_fix_is_not_actionable() {
    let c = Correction::new("too-long", "said a lot", NOW, 1);
    assert!(!c.is_actionable());
    assert!(c.wanting("one line").is_actionable());
}

#[test]
fn where_a_lesson_goes_follows_what_kind_of_lesson_it_is() {
    let how = Correction::new("order", "did it backwards", NOW, 1).wanting("always check the date first");
    assert_eq!(home_for(&how), Home::HowTo);
    assert!(home_for(&how).read_every_time(), "an instruction that is not read every time is a diary");

    let you = Correction::new("tone", "too formal", NOW, 1).wanting("I prefer it plainer");
    assert_eq!(home_for(&you), Home::AboutYou);
}

// ============ one is a note, two is a rule ===============================

#[test]
fn one_correction_is_noted_and_does_not_become_a_rule() {
    let mut m = Mending::default();
    let got = m.heard(Correction::new("too-long", "said a lot", NOW, 1).wanting("one line"));
    assert!(got.is_none(), "a rule was made from a single irritable evening");
    assert_eq!(m.heard.len(), 1, "the note was not kept as evidence for the next one");
}

#[test]
fn saying_it_twice_in_one_sitting_is_emphasis_not_a_second_occasion() {
    let mut m = Mending::default();
    m.heard(Correction::new("too-long", "said a lot", NOW, 7).wanting("one line"));
    let again = m.heard(Correction::new("too-long", "said a lot", NOW + 30, 7).wanting("one line"));
    assert!(again.is_none(), "restating a complaint in the same breath became a rule");
}

#[test]
fn saying_it_on_a_second_occasion_earns_the_edit() {
    let mut m = Mending::default();
    m.heard(Correction::new("too-long", "said a lot", NOW, 1).wanting("one line"));
    let edit = m
        .heard(Correction::new("too-long", "said a lot", NOW + 86_400, 2).wanting("one line"))
        .expect("the second occasion did not earn an edit");
    assert_eq!(edit.said_times, REPEATS_NEEDED);
    assert_eq!(edit.becomes, "one line");
}

// ============ the scoreboard ============================================

#[test]
fn going_back_on_a_written_rule_is_the_number_that_matters() {
    let mut m = Mending::default();
    m.heard(Correction::new("too-long", "a lot", NOW, 1).wanting("one line"));
    let edit = m.heard(Correction::new("too-long", "a lot", NOW + 86_400, 2).wanting("one line")).unwrap();
    m.applied(edit);
    assert_eq!(m.repeat_rate(), 0.0);

    m.heard(Correction::new("too-long", "a lot", NOW + 172_800, 3).wanting("one line"));
    assert_eq!(m.repeat_rate(), 1.0, "Atlas was told, wrote it down, and did it again");
}

// ============ reached from the running program ==========================

#[test]
fn correcting_atlas_records_it_without_making_a_rule_yet() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "first");

    let said = d.turn("that was too long, keep it to one line", NOW);
    assert!(said.contains("Noted"), "got: {said}");
    assert_eq!(d.mending.heard.len(), 1);
    assert!(d.mending.applied.is_empty(), "one correction became a rule");
}

#[test]
fn a_bare_complaint_is_asked_about_rather_than_filed() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "bare");

    let said = d.turn("that's wrong", NOW);
    assert!(said.contains("What should I have done instead"), "got: {said}");
    assert!(d.mending.heard.is_empty(), "a rule with no fix in it was filed");
}

#[test]
fn the_answer_to_what_should_i_have_done_becomes_the_fix() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "answered");

    d.turn("that's wrong", NOW);
    let said = d.turn("open the newest file, not the first one", NOW + 5);

    assert!(said.contains("Noted"), "got: {said}");
    assert_eq!(d.mending.heard.len(), 1);
    assert_eq!(
        d.mending.heard[0].wanted.as_deref(),
        Some("open the newest file, not the first one"),
        "the answer was parsed as a fresh command instead of as the fix"
    );
}

#[test]
fn the_second_occasion_offers_the_edit_rather_than_writing_it() {
    // Atlas changing its own instructions without saying so is the one kind
    // of self-improvement you cannot audit.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "offered");

    d.turn("that was too long, keep it to one line", NOW);
    // A second sitting. `Session::started` is what separates the two.
    d.session.started += 1;
    let said = d.turn("that was too long, keep it to one line", NOW + 86_400);

    assert!(said.contains("Alright?"), "the edit was not put as a question: {said}");
    assert!(said.contains("one line"), "the proposal did not name the change: {said}");
    assert!(d.mending.applied.is_empty(), "it wrote the rule without asking");
}

#[test]
fn the_offer_can_be_refused_and_nothing_is_written() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "refused");

    d.turn("that was too long, keep it to one line", NOW);
    d.session.started += 1;
    d.turn("that was too long, keep it to one line", NOW + 86_400);
    d.turn("no", NOW + 86_405);

    assert!(d.mending.applied.is_empty(), "a refused edit was written anyway");
}

#[test]
fn saying_yes_writes_it_where_the_model_will_read_it() {
    // The end of the loop, and the only part that changes behaviour.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "accepted");

    d.turn("that was too long, keep it to one line", NOW);
    d.session.started += 1;
    d.turn("that was too long, keep it to one line", NOW + 86_400);
    let said = d.turn("yes", NOW + 86_405);

    assert!(said.contains("Written to"), "got: {said}");
    assert_eq!(d.mending.applied.len(), 1, "saying yes wrote nothing");
    assert!(
        d.context().contains("keep it to one line"),
        "the lesson was written somewhere the model never reads"
    );
}

#[test]
fn the_offers_own_command_parses_to_something_that_writes_the_lesson() {
    // The trap `nudge::drifted` fell into: an offer whose command parses to
    // `Unknown` is an offer to do nothing, and saying yes agrees to nothing.
    use atlas::intent::{Intent, Parser};
    let cfg = cfg();
    let parser = Parser::new(&cfg.commands);
    let edit = an_edit("keep it to one line", Home::HowTo);
    let offer = atlas::proactive::from_nudge(&atlas::nudge::offer_to_mend(&edit));

    assert_eq!(
        parser.parse(&offer.command),
        Intent::ApplyLesson,
        "the relief `{}` does not parse to anything that writes the lesson",
        offer.command
    );
}

#[test]
fn every_home_has_a_phrase_the_parser_recognises() {
    // `offer_to_mend`'s relief is built from `Home::plain()`, so all three
    // have to parse — not just the one that happened to be tested.
    use atlas::intent::{Intent, Parser};
    let cfg = cfg();
    let parser = Parser::new(&cfg.commands);
    for home in [Home::HowTo, Home::AboutYou, Home::Record] {
        let edit = an_edit("something", home);
        let offer = atlas::proactive::from_nudge(&atlas::nudge::offer_to_mend(&edit));
        assert_eq!(
            parser.parse(&offer.command),
            Intent::ApplyLesson,
            "{home:?} produces `{}`, which parses to nothing",
            offer.command
        );
    }
}

#[test]
fn a_correction_marks_the_model_call_that_caused_it() {
    // `trace::blame` and `grade_last` are the correction loop the flight
    // recorder was built to close, and neither had a caller.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "blame");
    let _ = std::fs::remove_file(d.trace_path());
    d.trace = Default::default();
    let mut call = atlas::trace::Call::new("brain", "llama3.1", NOW);
    call.took_ms = 100;
    d.trace.record(call);

    d.turn("that was too long, keep it to one line", NOW + 1);

    let c = d.trace.calls.last().unwrap();
    assert!(c.correction.is_some(), "the call that caused the correction was not marked");
    assert_eq!(c.graded, Some(false), "the call was not graded bad");
    assert_eq!(d.trace.caused_corrections().len(), 1);
}

#[test]
fn the_scoreboard_is_askable() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "score");

    assert!(d.turn("how am i doing", NOW).contains("haven't had to correct me"));

    d.turn("that was too long, keep it to one line", NOW);
    d.session.started += 1;
    d.turn("that was too long, keep it to one line", NOW + 86_400);
    d.turn("yes", NOW + 86_405);

    let said = d.turn("how am i doing", NOW + 86_410);
    assert!(said.contains("1 rule"), "got: {said}");
    assert!(said.contains("haven't gone back"), "got: {said}");
}

#[test]
fn what_has_been_learned_survives_a_restart() {
    // A lesson that dies with the process is the problem this module exists
    // to solve, arrived at from the other side.
    let store = tmp("restart");
    let (c, p) = (cfg(), plat());
    {
        let mut d = Daemon::new(
            &c,
            &p,
            None,
            Store::new(store.clone()),
            Proactive::new(ProactiveConfig::default()),
        );
        d.turn("that was too long, keep it to one line", NOW);
        d.session.started += 1;
        d.turn("that was too long, keep it to one line", NOW + 86_400);
        d.turn("yes", NOW + 86_405);
        assert_eq!(d.mending.applied.len(), 1);
    }
    let mut d2 = Daemon::new(
        &c,
        &p,
        None,
        Store::new(store),
        Proactive::new(ProactiveConfig::default()),
    );
    assert_eq!(d2.mending.applied.len(), 1, "the lesson died with the process");
    assert!(d2.context().contains("keep it to one line"));
}

#[test]
fn applying_with_nothing_proposed_writes_nothing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "nothing");
    assert!(d.apply_lesson().contains("Nothing waiting"));
    assert!(d.mending.applied.is_empty());
}
