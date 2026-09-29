//! A council, convened — about the machine it is actually running on.
//!
//! `council.rs` shipped complete: five dispositions, blind and open rounds, a
//! tally that names the split rather than averaging it, a `strongest_dissent`
//! that can never be empty when there was one, and a `suspiciously_unanimous`
//! flag on the theory that four seats agreeing on the first pass means the
//! question leaked the answer.
//!
//! **Nothing ever built an `Opinion`.** `tally`, `Verdict`, `spoken`,
//! `strongest_dissent` and `empty chairs` all computed over a `&[Opinion]`
//! that only a test had ever filled, and `nudge::convene` produced five
//! prompts that nothing sent and nothing read back. Third module this session
//! with that exact shape, after `contents` and `trace`.
//!
//! Two things were missing and only one of them was code. The code half: a way
//! to turn what a seat says back into an `Opinion`. The other half was a
//! decision — *when is a council worth five model calls* — and the answer Eric
//! gave is hardware: it is the subject Atlas has measured numbers for, so the
//! seats can disagree about 15.7GB instead of about the word "upgrade".

use atlas::config::Config;
use atlas::council::{
    empty_chairs, hardware_room, is_hardware_question, parse_opinion, Council, Disposition, Lean,
    Opinion, Round, Seat, MAX_SEATS, MIN_SEATS,
};
use atlas::daemon::Daemon;
use atlas::brain::{Llm, LlmConfig};
use std::sync::Arc;
use atlas::error::{AtlasError, Result};
use atlas::fit::{Machine, Plan};
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// `ask_the_room` now hands the real work to the crew and returns an
/// immediate acknowledgment; this polls `tick` until the crew has actually
/// settled and returns everything said along the way, since an unrelated
/// proactive nudge can legitimately speak on an earlier tick than the
/// council's own report does.
fn wait_for_report<'a>(d: &mut Daemon<'a>, start_t: u64) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut t = start_t;
    let mut said: Vec<String> = Vec::new();
    loop {
        said.extend(d.tick(t));
        if d.crew.active() == 0 && !said.is_empty() {
            return said.join(" ");
        }
        assert!(Instant::now() < deadline, "no report arrived within the deadline");
        std::thread::sleep(Duration::from_millis(10));
        t += 1;
    }
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-room-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn a_laptop() -> Machine {
    Machine {
        total_ram_mb: 16_064,
        free_ram_mb: 6_400,
        cpu_cores: 8,
        vram_mb: 0,
        has_npu: false,
        disk_free_mb: 200_000,
        disk_is_spinning: false,
        reclaimable_mb: 0,
    }
}

fn a_plan() -> Plan {
    atlas::fit::plan_for(&a_laptop())
}

// ===================== what the council is scoped to ====================

#[test]
fn hardware_questions_are_recognised_as_the_rooms_subject() {
    for q in [
        "should I upgrade the RAM",
        "is a GPU worth it",
        "can this machine run the 14B",
        "would an NVMe help",
        "what about the mini pc for the rack",
    ] {
        assert!(is_hardware_question(q), "not recognised as hardware: {q}");
    }
}

#[test]
fn an_ordinary_question_is_not_mistaken_for_a_hardware_one() {
    // It still gets a room — the general one. What it must not get is five
    // seats briefed on RAM figures that have nothing to do with the question.
    for q in ["should I take the contract", "is this worth writing up", "who should I ask"] {
        assert!(!is_hardware_question(q), "wrongly treated as hardware: {q}");
    }
}

// ===================== the room has seen the machine ====================

#[test]
fn every_seat_is_handed_the_same_measured_machine() {
    // The whole difference between this and `default_room`. Five seats
    // arguing about "should I upgrade" is five opinions; five seats arguing
    // about 15GB and no discrete graphics is a decision.
    let room = hardware_room(&a_laptop(), &a_plan());
    for seat in &room.seats {
        assert!(
            seat.brief.contains("6400MB free"),
            "seat `{}` was briefed without the measured numbers: {}",
            seat.id,
            seat.brief
        );
    }
}

#[test]
fn integrated_graphics_are_not_reported_as_spare_video_memory() {
    // `fit`'s own hard-won note: integrated graphics share system memory, so
    // VRAM is not extra. A seat told it has video memory it does not have will
    // argue for a model that will not load.
    let room = hardware_room(&a_laptop(), &a_plan());
    let brief = &room.seats[0].brief;
    assert!(brief.contains("integrated graphics share system memory"), "got: {brief}");
}

#[test]
fn the_hardware_room_is_quorate_and_covers_different_ground() {
    let room = hardware_room(&a_laptop(), &a_plan());
    assert!(room.is_quorate());
    assert!(room.covers_different_ground(), "the seats all want the same thing");
    assert!((MIN_SEATS..=MAX_SEATS).contains(&room.seats.len()));
}

#[test]
fn exactly_one_seat_can_break_a_tie() {
    // A weighting that can outvote the room is one seat wearing five hats.
    let room = hardware_room(&a_laptop(), &a_plan());
    assert_eq!(room.seats.iter().filter(|s| s.tiebreak).count(), 1);
}

// ===================== reading a seat's answer back =====================

#[test]
fn a_seat_that_argues_against_is_recorded_against() {
    let o = parse_opinion("sceptic", "Against. You measured 6GB free and the 14B needs 9.");
    assert_eq!(o.lean, Lean::Against);
    assert!(o.because.contains("Against"));
}

#[test]
fn the_word_for_inside_another_word_does_not_decide_an_opinion() {
    // "for" is a substring of "before", "form" and "therefore", and an
    // opinion decided by the word "therefore" is noise dressed as a verdict.
    let o = parse_opinion("mover", "Therefore the performance before the upgrade is the issue.");
    assert_eq!(o.lean, Lean::Depends, "a stray substring was read as a position");
}

#[test]
fn a_seat_naming_the_argument_on_the_other_side_still_keeps_its_own() {
    // "For, though the argument against is the heat" — the seat's own lean is
    // the one it states first; the second is nearly always a concession.
    let o = parse_opinion("mover", "For — ship it. The case against is thermals, which are fine.");
    assert_eq!(o.lean, Lean::For);
}

#[test]
fn an_explicit_it_depends_outranks_a_stray_for() {
    let o = parse_opinion("operator", "It depends on whether you are paying for the power.");
    assert_eq!(o.lean, Lean::Depends);
}

#[test]
fn what_would_change_a_seats_mind_is_kept_when_it_says() {
    let o = parse_opinion(
        "sceptic",
        "Against for now. What would change my mind: a week of headroom under real load.",
    );
    assert_eq!(o.would_change_my_mind.as_deref(), Some("a week of headroom under real load."));
}

#[test]
fn a_seat_that_will_not_commit_is_named_as_an_empty_chair() {
    // `Seat::prompt` tells every seat exactly this. When one turns up anyway
    // it is worth saying, rather than quietly counting as "it depends".
    let opinions = vec![
        parse_opinion("mover", "For. Ship it."),
        parse_opinion("sceptic", "Hard to say either way, lots to consider."),
        parse_opinion("operator", "Against. It costs more than it returns."),
    ];
    assert_eq!(empty_chairs(&opinions), vec!["sceptic"]);
}

#[test]
fn a_lean_is_never_invented_where_none_was_given() {
    // The important half of being lenient about shape: a reply that commits
    // to nothing must not become a vote. `tally` counts Depends separately
    // and can return no call at all, which is a real finding.
    let o = parse_opinion("user", "");
    assert_eq!(o.lean, Lean::Depends);
    assert_eq!(o.because, "gave no reason");
    assert!(o.would_change_my_mind.is_none());
}

// ===================== what the room produces ==========================

fn op(seat: &str, lean: Lean, because: &str) -> Opinion {
    Opinion { seat: seat.into(), lean, because: because.into(), would_change_my_mind: None }
}

#[test]
fn the_verdict_names_the_split_rather_than_averaging_it() {
    let room = hardware_room(&a_laptop(), &a_plan());
    let v = room.tally(
        &[
            op("mover", Lean::For, "the machine will cope"),
            op("sceptic", Lean::Against, "6GB free is not 9GB"),
            op("operator", Lean::Against, "it costs more than it returns"),
        ],
        Round::Blind,
    );
    assert!(v.split.contains("1 for"), "the split was not stated: {}", v.split);
    assert!(v.strongest_dissent.is_some(), "the losing argument was buried");
}

#[test]
fn a_room_that_agreed_on_the_first_pass_says_so_rather_than_sounding_confident() {
    let room = hardware_room(&a_laptop(), &a_plan());
    let v = room.tally(
        &[
            op("mover", Lean::For, "yes"),
            op("sceptic", Lean::For, "yes"),
            op("operator", Lean::For, "yes"),
        ],
        Round::Blind,
    );
    assert!(v.suspiciously_unanimous, "unanimity on the blind round was taken at face value");
    assert!(room.spoken(&v).contains("usually means the question told them the answer"));
}

#[test]
fn the_same_agreement_after_they_have_read_each_other_is_not_suspicious() {
    let room = hardware_room(&a_laptop(), &a_plan());
    let v = room.tally(&[op("mover", Lean::For, "yes"), op("sceptic", Lean::For, "yes")], Round::Open);
    assert!(!v.suspiciously_unanimous);
}

// ===================== reached from the running program =================

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// A model that gives each seat a different answer, so the room can split.
struct Room;
impl Llm for Room {
    fn complete(&self, _: &str, user: &str) -> Result<String> {
        if user.contains("wants it proved") {
            Ok("Against. You have 6GB free and it needs 9.".into())
        } else if user.contains("counts the cost") {
            Ok("Against. It costs more than it returns this year.".into())
        } else {
            Ok("For. It will cope and you get the time back.".into())
        }
    }
}

struct DownLlm;
impl Llm for DownLlm {
    fn complete(&self, _: &str, _: &str) -> Result<String> {
        Err(AtlasError::Platform("connection refused".into()))
    }
}

fn cfg_with_model() -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let tools = c.tools.as_mut().expect("tools section");
    tools.llm = Some(LlmConfig {
        tool: Default::default(),
        request: r#"{"model":"llama3.1","prompt":"{user}"}"#.into(),
        response_path: "response".into(),
        vision_request: None,
    });
    c
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, llm: Option<Arc<dyn Llm>>, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, llm, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn the_phrases_for_asking_reach_the_room() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&cfg.commands);
    for said in ["ask the room", "what does the room think", "should I upgrade"] {
        assert!(
            matches!(parser.parse(said), Intent::AskTheRoom(_)),
            "did not reach the room: {said} -> {:?}",
            parser.parse(said)
        );
    }
}

#[test]
fn asking_the_room_produces_a_verdict_with_the_split_in_it() {
    let (c, p, llm): (_, _, Arc<dyn Llm>) = (cfg_with_model(), plat(), Arc::new(Room));
    let mut d = daemon(&c, &p, Some(llm), "verdict");

    let ack = d.ask_the_room("should I upgrade the RAM");
    assert!(ack.contains("Asking the room"), "expected an immediate acknowledgment, got: {ack}");
    let said = wait_for_report(&mut d, 100);

    assert!(said.contains("for"), "no split in the answer: {said}");
    assert!(said.contains("against"), "no split in the answer: {said}");
}

/// The same verdict, but driven through the front door.
///
/// Every other test here calls `d.ask_the_room(...)` directly, which proves
/// the handler and the parser separately -- `the_phrases_for_asking_reach_the_room`
/// checks that "ask the room ..." parses to `Intent::AskTheRoom`, and the test
/// above checks that the handler splits the room. What neither exercises is
/// the join: the daemon's dispatch arm that turns the parsed intent into the
/// handler call. `tests/every_intent_reaches_the_daemon.rs` exists precisely
/// because a wrong dispatch arm is invisible to both halves tested apart. This
/// says the spoken sentence reaches the room end to end -- parser, dispatch and
/// handler -- so the entry `ask_the_room` no longer needs its `NO_DAEMON_TEST`
/// exemption.
#[test]
fn a_spoken_ask_reaches_the_room_through_the_daemon() {
    let (c, p, llm): (_, _, Arc<dyn Llm>) = (cfg_with_model(), plat(), Arc::new(Room));
    let mut d = daemon(&c, &p, Some(llm), "spoken-ask");

    let ack = d.turn("ask the room should I upgrade the RAM", 1_000);
    assert!(
        ack.contains("Asking the room"),
        "the spoken phrase did not reach ask_the_room through the daemon: {ack}"
    );
    let said = wait_for_report(&mut d, 1_001);
    assert!(said.contains("for"), "no split in the answer: {said}");
    assert!(said.contains("against"), "no split in the answer: {said}");
}

#[test]
fn a_hardware_verdict_says_what_the_room_was_looking_at() {
    // Otherwise you cannot tell whether the seats were arguing about your
    // machine or about machines in general.
    let (c, p, llm): (_, _, Arc<dyn Llm>) = (cfg_with_model(), plat(), Arc::new(Room));
    let mut d = daemon(&c, &p, Some(llm), "looking-at");

    d.ask_the_room("is a GPU worth it");
    let said = wait_for_report(&mut d, 100);
    assert!(
        said.contains("They were all looking at:"),
        "the verdict did not say what the room had seen: {said}"
    );
}

#[test]
fn every_seat_is_one_recorded_model_call() {
    // This is what made the council worth wiring rather than worth avoiding:
    // it costs five calls and `atlas trace` says so, instead of the cost
    // being invisible until the model budget goes.
    let (c, p, llm): (_, _, Arc<dyn Llm>) = (cfg_with_model(), plat(), Arc::new(Room));
    let mut d = daemon(&c, &p, Some(llm), "recorded");
    let _ = std::fs::remove_file(d.trace_path());
    d.trace = Default::default();

    d.ask_the_room("should I upgrade the RAM");
    wait_for_report(&mut d, 100);

    let council_calls = d.trace.calls.iter().filter(|c| c.asked_by == "council").count();
    assert_eq!(council_calls, 5, "the room's cost was not recorded call by call");
}

#[test]
fn with_no_model_it_says_why_rather_than_convening_an_empty_room() {
    let (c, p) = (cfg_with_model(), plat());
    let mut d = daemon(&c, &p, None, "nomodel");
    let said = d.ask_the_room("should I upgrade the RAM");
    assert!(said.contains("need a model"), "got: {said}");
}

#[test]
fn a_model_that_is_down_does_not_produce_a_verdict_out_of_nothing() {
    // Four real opinions and a stated absence beats five where one is
    // invented. With none at all there is no room, and it says so.
    let (c, p) = (cfg_with_model(), plat());
    let mut d = daemon(&c, &p, Some(Arc::new(DownLlm)), "down");

    d.ask_the_room("should I upgrade the RAM");
    let said = wait_for_report(&mut d, 100);
    assert!(said.contains("seats answered"), "it invented a verdict: {said}");
    assert!(said.contains("model may be struggling") && !said.contains("`atlas"), "it didn't say why, or sent you to a terminal: {said}");
}

#[test]
fn asking_the_room_nothing_asks_nothing() {
    let (c, p, llm): (_, _, Arc<dyn Llm>) = (cfg_with_model(), plat(), Arc::new(Room));
    let mut d = daemon(&c, &p, Some(llm), "empty-q");
    let _ = std::fs::remove_file(d.trace_path());
    d.trace = Default::default();

    let said = d.ask_the_room("   ");
    assert!(said.contains("Ask the room what"), "got: {said}");
    assert!(d.trace.calls.is_empty(), "an empty question still spent five model calls");
}

#[test]
fn a_room_that_cannot_disagree_is_refused_before_it_costs_anything() {
    // Five seats that all want it proved is one seat five times, and
    // `covers_different_ground` already knew that — it just had no caller.
    let same = Council::new(vec![
        Seat::new("a", "one", Disposition::Sceptic),
        Seat::new("b", "two", Disposition::Sceptic),
        Seat::new("c", "three", Disposition::Sceptic),
    ]);
    assert!(same.is_quorate());
    assert!(!same.covers_different_ground());
}

#[test]
fn a_decisive_blind_round_never_pays_for_a_second_one() {
    // Stopping after Blind when it already settled something is council's
    // own rule working, not half a feature — the default `Room` fake
    // resolves 3-2, so no open-round wording should reach the model at all.
    let (c, p, llm): (_, _, Arc<dyn Llm>) = (cfg_with_model(), plat(), Arc::new(Room));
    let mut d = daemon(&c, &p, Some(llm), "no-open-when-decisive");
    let _ = std::fs::remove_file(d.trace_path());
    d.trace = Default::default();

    d.ask_the_room("should I upgrade the RAM");
    let said = wait_for_report(&mut d, 100);

    assert_eq!(d.trace.calls.iter().filter(|c| c.asked_by == "council").count(), 5);
    assert!(!said.contains("read each other"), "a settled room still ran a second round: {said}");
}

#[test]
fn a_genuine_tie_earns_the_second_round_and_can_be_settled_by_it() {
    // `Round::Open` was defined in the type and never driven for a while:
    // `blind_prompts` was the only prompt-builder anything called, so the
    // room always stopped after the blind pass no matter how it came out.
    // This exercises the real thing: the tiebreak seat is down for the
    // blind round, leaving a genuine 2-2 tie with nothing to break it, and
    // comes back for the open round with the room's blind opinions in
    // front of it.
    struct SplitThenSettles;
    impl Llm for SplitThenSettles {
        fn complete(&self, _: &str, user: &str) -> Result<String> {
            let open = user.contains("Here is what the rest of the room said");
            if user.contains("wants it proved") {
                Ok("Against. You have 6GB free and it needs 9.".into())
            } else if user.contains("counts the cost") {
                Ok("Against. It costs more than it returns this year.".into())
            } else if user.contains("thinks about afterwards") {
                // The tiebreak seat: down on the blind pass, answers on the
                // open one. A real tie (2 for, 2 against, this seat missing)
                // with no tiebreak opinion to fall back on is the one case
                // `Round::Open` is worth its five extra calls for.
                if open {
                    Ok("For. Having seen the split, it still comes out ahead.".into())
                } else {
                    Err(AtlasError::Platform("timeout".into()))
                }
            } else {
                Ok("For. It will cope and you get the time back.".into())
            }
        }
    }

    let (c, p): (_, _) = (cfg_with_model(), plat());
    let llm: Arc<dyn Llm> = Arc::new(SplitThenSettles);
    let mut d = daemon(&c, &p, Some(llm), "open-round-settles-a-tie");
    let _ = std::fs::remove_file(d.trace_path());
    d.trace = Default::default();

    d.ask_the_room("should I upgrade the RAM");
    let said = wait_for_report(&mut d, 100);

    assert!(said.contains("read each other"), "the tie did not earn a second round: {said}");
    assert!(said.contains("3 for"), "the settled count is missing: {said}");
    let council_calls = d.trace.calls.iter().filter(|c| c.asked_by == "council").count();
    assert_eq!(council_calls, 10, "a tie should cost the blind round plus the open one: {council_calls}");
}

#[test]
fn a_genuine_deadlock_asks_for_guidance_and_files_a_backlog_entry() {
    // Four seats answer, split evenly with the one seat that could break
    // the tie unreachable -- a real, complete outcome, not a technical
    // failure. Eric's call on the wording: this is not "the room is split
    // and there is no seat to break it", it's "we're asking for guidance".
    struct DeadlockLlm;
    impl Llm for DeadlockLlm {
        fn complete(&self, _system: &str, user: &str) -> Result<String> {
            if user.contains("thinks about afterwards") {
                // The steward -- the only seat with `breaks_ties()` in the
                // default room -- never answers.
                Err(AtlasError::Platform("connection refused".into()))
            } else if user.contains("wants it moving") || user.contains("wants it proved") {
                Ok("For. Good reasons either way.".into())
            } else {
                Ok("Against. Good reasons either way.".into())
            }
        }
    }

    let (c, p): (_, _) = (cfg_with_model(), plat());
    let llm: Arc<dyn Llm> = Arc::new(DeadlockLlm);
    let mut d = daemon(&c, &p, Some(llm), "deadlock");
    let before = d.backlog.outstanding().len();

    let ack = d.ask_the_room("should I upgrade the RAM");
    assert!(ack.contains("Asking the room"));
    let said = wait_for_report(&mut d, 100);

    assert!(
        said.contains("couldn't come to an agreement") && said.contains("asking for guidance"),
        "expected the deadlock wording, got: {said}"
    );
    assert!(
        d.backlog.outstanding().len() > before,
        "a genuine deadlock must be filed for guidance, not just spoken and forgotten"
    );
}
