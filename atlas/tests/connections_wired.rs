//! The connections board, wired to something real.
//!
//! Every piece of this module existed and passed its own tests before this
//! file did, and none of it ran. `Daemon::new` built an empty `Board`, nothing
//! ever registered an integration into it, `mark()` was never called from
//! anywhere, and the `Health` a hub page rendered was computed over nothing.
//! Each part was individually correct and the feature did not exist.
//!
//! These tests are deliberately end-to-end through `Daemon::turn` rather than
//! unit tests on `mark`, because unit tests on `mark` are exactly what already
//! existed while the feature was dead. The question worth asking is not "does
//! `mark` format a string correctly" — it is "does a person asking Atlas a
//! question ever see the result".

use atlas::brain::{Llm, Reached};
use atlas::config::Config;
use atlas::connectivity::Need;
use atlas::daemon::Daemon;
use atlas::error::{AtlasError, Result};
use atlas::integrations::{dependencies, sources_for, Health, INTERNET, MODEL};
use std::sync::Arc;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-cw-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

/// A model that is down. The point of the whole exercise.
struct DownLlm;
impl Llm for DownLlm {
    fn complete(&self, _: &str, _: &str) -> Result<String> {
        Err(AtlasError::Platform("connection refused".into()))
    }
}

/// A model that answers.
struct UpLlm;
impl Llm for UpLlm {
    fn complete(&self, _: &str, _: &str) -> Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"Sure."}"#.into())
    }
}

// ---------------------------------------------------------------------------
// The end-to-end question: does a person ever see it?
// ---------------------------------------------------------------------------

#[test]
fn an_answer_produced_without_the_model_admits_it_to_your_face() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(
        &c,
        &p,
        Some(Arc::new(DownLlm)),
        Store::new(tmp("down")),
        Proactive::new(ProactiveConfig::default()),
    );
    // Something the phrase parser cannot recognise, so the model is genuinely
    // consulted rather than bypassed.
    let reply = d.turn("what do you make of the situation with the roof", 100);
    assert!(
        reply.contains(MODEL),
        "the answer never mentions the dependency that failed to produce it: {reply}"
    );
    assert!(
        reply.contains("missing"),
        "naming the source without saying what it cost is half an answer: {reply}"
    );
    // Behaviour, not wording: the failure was recorded, and the board agrees.
    let i = d.connections.get_mut(MODEL).unwrap();
    assert_eq!(i.health(100), Health::Failing);
    assert_eq!(i.failures_running, 1);
}

#[test]
fn the_admission_survives_the_sentence_cap() {
    // `persona.spoken` shapes and truncates. Marking the answer before that
    // step put the caveat first in line to be cut, which would have deleted
    // the warning from precisely the answers that needed it. This asserts the
    // ordering, not the wording.
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(
        &c,
        &p,
        Some(Arc::new(DownLlm)),
        Store::new(tmp("cap")),
        Proactive::new(ProactiveConfig::default()),
    );
    d.persona.max_spoken_sentences = 1;
    let reply = d.turn("what do you make of the situation with the roof", 100);
    assert!(reply.contains(MODEL), "a one-sentence cap swallowed the caveat: {reply}");
    // The cap still did its job on the answer itself; the caveat is additional
    // rather than exempting the reply from shaping.
    assert!(reply.len() > 20, "the reply collapsed entirely: {reply}");
    assert_eq!(d.persona.max_spoken_sentences, 1, "the cap was silently raised");
}

#[test]
fn an_answer_the_parser_handled_alone_is_not_marked() {
    // The model being down does not make "boot workspace" a worse answer. A
    // caveat attached to a complete answer trains you to ignore caveats.
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(
        &c,
        &p,
        Some(Arc::new(DownLlm)),
        Store::new(tmp("local")),
        Proactive::new(ProactiveConfig::default()),
    );
    let reply = d.turn("boot workspace", 100);
    assert!(!reply.contains(MODEL), "a complete local answer got a caveat: {reply}");
}

#[test]
fn a_working_model_leaves_the_answer_clean() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(
        &c,
        &p,
        Some(Arc::new(UpLlm)),
        Store::new(tmp("up")),
        Proactive::new(ProactiveConfig::default()),
    );
    let reply = d.turn("what do you make of the situation with the roof", 100);
    assert!(!reply.contains("missing whatever"), "a healthy answer was marked: {reply}");
}

#[test]
fn the_daemon_records_the_model_going_down_rather_than_only_saying_so_once() {
    // The board is what the hub page and the nudge both read. An answer that
    // mentions the failure but never writes it down means the failure is
    // invisible everywhere except that one reply.
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(
        &c,
        &p,
        Some(Arc::new(DownLlm)),
        Store::new(tmp("record")),
        Proactive::new(ProactiveConfig::default()),
    );
    d.turn("what do you make of the situation with the roof", 100);
    let i = d.connections.get_mut(MODEL).expect("the model is a registered dependency");
    assert_eq!(i.failures_running, 1, "the failure was never written to the board");
    assert_eq!(i.health(100), Health::Failing);
}

#[test]
fn a_parser_only_turn_does_not_invent_a_success_for_the_model() {
    // Recording "worked" whenever a turn goes well would show a healthy model
    // on a machine whose model has never once been reached.
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(
        &c,
        &p,
        Some(Arc::new(DownLlm)),
        Store::new(tmp("noinvent")),
        Proactive::new(ProactiveConfig::default()),
    );
    d.turn("boot workspace", 100);
    let i = d.connections.get_mut(MODEL).unwrap();
    assert_eq!(i.health(100), Health::Unknown, "a local turn claimed the model works");
}

#[test]
fn the_daemon_starts_with_a_board_that_can_actually_report() {
    let (c, p) = (cfg(), plat());
    let d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(tmp("board")),
        Proactive::new(ProactiveConfig::default()),
    );
    assert!(
        !d.connections.integrations.is_empty(),
        "back to an empty board — every reader of it is reading nothing"
    );
}

// ---------------------------------------------------------------------------
// The mapping from a request to the things it leaned on.
// ---------------------------------------------------------------------------

#[test]
fn a_local_request_that_never_asked_the_model_leaned_on_nothing() {
    assert_eq!(sources_for(Need::Local, Reached::NotNeeded), Vec::<&str>::new());
}

#[test]
fn research_leans_on_the_internet_and_nothing_else() {
    // Asserting the whole set, not membership. Membership would still pass if
    // the mapping quietly started listing something it has no business
    // listing, which is the failure that put "I've never seen the internet
    // work" on every ordinary reply.
    assert_eq!(sources_for(Need::Internet, Reached::NotNeeded), vec![INTERNET]);
}

#[test]
fn a_request_that_merely_prefers_the_network_is_not_marked_as_missing_it() {
    // `PrefersInternet` still answers, from the local model. Treating that as
    // a missing source is what put a caveat on healthy answers, and a caveat
    // that appears on healthy answers is one you learn to skip.
    assert_eq!(sources_for(Need::PrefersInternet, Reached::NotNeeded), Vec::<&str>::new());
}

#[test]
fn a_model_that_was_asked_counts_whether_or_not_it_answered() {
    assert_eq!(sources_for(Need::Local, Reached::Yes), vec![MODEL]);
    assert_eq!(sources_for(Need::Local, Reached::No), vec![MODEL]);
}

#[test]
fn a_model_that_was_never_asked_is_not_listed() {
    let used = sources_for(Need::Local, Reached::NotNeeded);
    assert!(used.is_empty(), "got {used:?}");
}

#[test]
fn a_research_request_that_also_needed_the_model_lists_both() {
    assert_eq!(sources_for(Need::Internet, Reached::Yes), vec![INTERNET, MODEL]);
}

// ---------------------------------------------------------------------------
// Reachability is structural, not a phrase in a sentence.
// ---------------------------------------------------------------------------

#[test]
fn reaching_the_model_is_recorded_as_a_value_not_inferred_from_wording() {
    // This was recoverable only by matching the string "Model unreachable",
    // which survives exactly until someone rewords the message.
    use atlas::brain::Brain;
    use atlas::intent::Parser;
    let c = cfg();
    let parser = Parser::new(&c.commands);
    let down = DownLlm;
    let d = Brain { llm: &down, fallback: &parser, voice: None }.decide("ruminate on the roof", "");
    assert_eq!(d.model, Reached::No);

    let up = UpLlm;
    let d = Brain { llm: &up, fallback: &parser, voice: None }.decide("ruminate on the roof", "");
    assert_eq!(d.model, Reached::Yes);

    let d = Brain { llm: &down, fallback: &parser, voice: None }.decide("boot workspace", "");
    assert_eq!(d.model, Reached::NotNeeded, "the parser handled it; the model was never asked");
}

#[test]
fn a_model_that_answers_with_rubbish_is_reached_not_down() {
    // Two different problems. Recording an unusable reply as "unreachable"
    // sends you to check the network when the model is sitting right there
    // returning nonsense.
    use atlas::brain::Brain;
    use atlas::intent::Parser;
    struct Rubbish;
    impl Llm for Rubbish {
        fn complete(&self, _: &str, _: &str) -> Result<String> {
            Ok("no json here at all".into())
        }
    }
    let c = cfg();
    let parser = Parser::new(&c.commands);
    let r = Rubbish;
    let d = Brain { llm: &r, fallback: &parser, voice: None }.decide("ruminate on the roof", "");
    assert_eq!(d.model, Reached::Yes);
}

#[test]
fn the_registry_names_a_consequence_for_every_dependency() {
    for i in dependencies().integrations {
        assert!(!i.if_it_breaks.trim().is_empty(), "{} has no stated consequence", i.name);
    }
}

// ---------------------------------------------------------------------------
// A broken connection reaches the person, and obeys the nudge rules.
// ---------------------------------------------------------------------------

#[test]
fn a_failing_connection_is_said_out_loud_not_only_logged() {
    use atlas::integrations::Integration;
    use atlas::nudge::{link_broke, Nudger};
    let mut i = Integration::new("stripe", "payment alerts stop");
    i.failed(100, "401");
    let n = link_broke(&i);
    assert_eq!(n.subject.as_deref(), Some("integration:stripe"));
    assert!(n.confidence > 0.9, "a measurement should not be hedged like a judgement");
    assert!(n.message.contains("stripe"), "{}", n.message);
    assert!(
        n.message.contains("payment alerts stop"),
        "a failure has to read as a consequence: {}",
        n.message
    );

    // And it goes through the same gate as every other nudge.
    let mut ng = Nudger::default();
    let subject = n.subject.clone().unwrap();
    assert!(ng.may_raise(&subject, 100), "a first mention should be allowed");
    ng.raised(&subject, &n.message, 100);
    assert!(!ng.may_raise(&subject, 100), "it re-announced itself immediately");
}

#[test]
fn saying_no_to_a_broken_connection_is_believed_the_first_time() {
    use atlas::integrations::Integration;
    use atlas::nudge::{link_broke, Nudger, Response};
    let mut i = Integration::new("stripe", "payment alerts stop");
    i.failed(100, "401");
    let n = link_broke(&i);
    let subject = n.subject.clone().unwrap();
    let mut ng = Nudger::default();
    ng.raised(&subject, &n.message, 100);
    ng.record(Some(&subject), Response::Declined, 100);
    assert!(
        !ng.may_raise(&subject, 10_000_000),
        "a declined nudge came back — the rule is that no is believed the first time"
    );
}

#[test]
fn silence_widens_the_gap_rather_than_repeating_at_the_same_rate() {
    use atlas::nudge::Nudger;
    let mut ng = Nudger::default();
    ng.raised("integration:stripe", "stripe is down", 0);
    let first = ng.goals.iter().find(|g| g.id == "integration:stripe").unwrap().ignored;
    ng.raised("integration:stripe", "stripe is down", 1_000_000);
    let second = ng.goals.iter().find(|g| g.id == "integration:stripe").unwrap().ignored;
    assert!(second > first, "unanswered nudges must accumulate, or back-off never kicks in");
}

// ---------------------------------------------------------------------------
// A machine nothing could read must never report as fine.
//
// This is the readings stub again, and the reason it came back is that the
// fix last time was "make the Windows readings real" rather than "stop an
// unread machine looking healthy". `assess()` still only speaks when a value
// is above zero, and `summary()`'s unread check sat in the branch that only
// runs when there *is* a finding — the one branch an unread machine can never
// reach. Once Atlas targeted more than Windows this went live again: off
// Windows, `read_disk` was an empty stub and `read_memory` needed /proc.
// ---------------------------------------------------------------------------

#[test]
fn a_machine_that_could_not_be_read_does_not_say_all_fine() {
    use atlas::health::{assess, summary, HealthConfig, Readings};
    let unread = Readings::default();
    let findings = assess(&unread, &HealthConfig::default());
    let said = summary(&unread, &findings);
    assert!(!said.contains("All fine"), "an unread machine reported healthy: {said}");
    assert!(said.contains("could not read"), "it did not say what it was missing: {said}");
}

#[test]
fn a_machine_that_was_read_and_is_healthy_still_says_so() {
    // The fix must not turn every healthy answer into a warning.
    use atlas::health::{assess, summary, HealthConfig, Readings};
    let r = Readings {
        disk_total_gb: 500.0,
        disk_free_gb: 300.0,
        ram_total_gb: 16.0,
        ram_used_gb: 8.0,
        ..Default::default()
    };
    let said = summary(&r, &assess(&r, &HealthConfig::default()));
    // A read machine leads with its numbers. It may still carry a finding
    // (an un-backed-up state folder is a real one), but it must never claim
    // an instrument was unreadable when it was read.
    assert!(!said.contains("could not read"), "a read machine claimed it was unreadable: {said}");
    assert!(said.contains("300 gigabytes free"), "the real numbers were dropped: {said}");
    assert!(said.contains("50 percent"), "the memory reading was dropped: {said}");
}

#[test]
fn one_unread_instrument_is_named_rather_than_lumped_in() {
    use atlas::health::{summary, Readings};
    let half = Readings { ram_total_gb: 16.0, ram_used_gb: 8.0, ..Default::default() };
    let said = summary(&half, &[]);
    assert!(said.contains("disk"), "the unread instrument was not named: {said}");
    assert!(!said.contains("memory or"), "it blamed an instrument that worked: {said}");
}

#[test]
fn this_machine_reports_real_numbers_rather_than_zero() {
    // Runs wherever the suite runs. Before the non-Windows readings existed
    // this asserted zero on every platform but Windows and nobody noticed,
    // because nothing asserted it at all.
    let r = atlas::health::read_machine();
    assert!(r.disk_total_gb > 0.0, "disk read as zero on this platform");
    assert!(r.ram_total_gb > 0.0, "memory read as zero on this platform");
    assert!(r.disk_free_gb <= r.disk_total_gb, "free disk exceeded total");
    assert!(r.ram_used_gb <= r.ram_total_gb, "used memory exceeded total");
}
