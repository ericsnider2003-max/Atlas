//! What somebody else gets, while they are holding your laptop.
//!
//! # The hole this file was written for
//!
//! The handover restriction lived in one place: `gate_with_identity`, in
//! `src/main.rs`. That function guards the paths where a person **types** a
//! line — the interactive prompt, and the press-Enter voice loop.
//!
//! The daemon's own listening loop does not go through it. It calls
//! `Daemon::turn_from` directly, and `turn_from` had no such check. So the
//! restriction held for the path a stranger is least likely to use and not
//! for the one they are most likely to use: the always-listening Atlas
//! sitting on the desk, which is the entire situation a handover is for.
//!
//! Everything below is that hole, in the shapes it actually takes. Three of
//! them are not the obvious one:
//!
//! * A question you were already asked. Atlas says "go ahead?", you hand the
//!   laptop over, and the next person says "yes" -- and that branch calls
//!   `execute` several hundred lines above where `turn_from` now checks.
//! * The same yes recorded as a standing grant, which is how the *next* one
//!   gets waved through without being asked at all.
//! * An unrecognised sentence, answered out of your research notes by a
//!   lookup that sits deliberately above the policy gate.

mod common; // `common::source_of`: a module's source wherever its files live

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::handover::Handover;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, Once};

fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// This file's own install, set before `roots` is first asked. See the note
/// in `tests/saying_youre_back.rs` -- without it these tests would be
/// writing a handed-over flag into the repository's own state directory.
fn home() -> PathBuf {
    static ONCE: Once = Once::new();
    let p = std::env::temp_dir().join("atlas-what-a-stranger-gets");
    ONCE.call_once(|| {
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("data").join("state")).unwrap();
        std::env::set_var("ATLAS_HOME", &p);
    });
    p
}

fn install() -> Store {
    home();
    atlas::roots::install_state()
}

fn hand_it_over() {
    let mut h = Handover::default();
    h.hand_over("Sam has it", 1_000);
    h.save(&install()).unwrap();
}

fn it_is_yours() {
    Handover::default().save(&install()).unwrap();
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

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let dir = home().join(format!("person-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Daemon::new(c, p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()))
}

fn refused(said: &str) -> bool {
    said.contains("Not while this is handed over")
}

// --- the spoken path, which had no gate at all -----------------------------

#[test]
fn the_daemon_refuses_what_the_command_line_refuses() {
    let _lock = alone();
    hand_it_over();
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "spoken");

    // One from each list: acting as you, and reading what Atlas holds for
    // you. The second is the one that was not refused anywhere at all.
    let posting = d.turn("draft a post for twitter", 2_000);
    assert!(refused(&posting), "a stranger could post as you by saying so: {posting}");

    let yours = d.turn("what's outstanding", 2_010);
    assert!(refused(&yours), "a stranger could read your day out loud: {yours}");
    assert!(yours.contains("vault"), "the refusal does not name the way back: {yours}");
}

#[test]
fn the_same_daemon_does_all_of_it_when_the_machine_is_yours() {
    // The half that keeps this honest. A gate that refuses everything is not
    // a gate, and the way this change would do real damage is by leaving the
    // refusal on when the handover is over.
    let _lock = alone();
    it_is_yours();
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "yours");
    for said in ["draft a post for twitter", "what's outstanding", "what did you do"] {
        let reply = d.turn(said, 2_000);
        assert!(!refused(&reply), "still refusing after the handover ended: {said} -> {reply}");
    }
}

#[test]
fn ordinary_help_still_works_for_whoever_is_holding_it() {
    // The other half of the point. A handed-over Atlas that refuses
    // everything is one your friend hands straight back, and then nobody
    // uses the feature and everybody uses the owner's live assistant
    // instead -- which is the situation this was built to improve on.
    let _lock = alone();
    hand_it_over();
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "ordinary");
    for said in ["boot workspace", "how's the machine", "what can you do"] {
        let reply = d.turn(said, 2_000);
        assert!(!refused(&reply), "an ordinary thing was refused: {said} -> {reply}");
        assert!(!reply.is_empty(), "{said} produced nothing at all");
    }
}

// --- the ways round it -----------------------------------------------------

#[test]
fn a_question_you_were_asked_is_not_answered_by_whoever_is_there_next() {
    // Atlas asks "go ahead?", the laptop changes hands, the next person says
    // "yes". The approval branch calls `execute` directly, so a check placed
    // only where a new command is parsed would never see this.
    let _lock = alone();
    it_is_yours();
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "parked");

    let asked = d.turn("shutdown workspace", 2_000);
    assert!(asked.contains("Go ahead?"), "this test needs a parked approval: {asked}");

    hand_it_over();
    let answered = d.turn("yes", 2_010);
    assert!(refused(&answered), "a stranger answered a question you were asked: {answered}");

    // And the yes did not become a standing grant. This is the quieter half:
    // `policy::classify_with_policy` reads approval history, so a recorded
    // yes here would make the *next* shutdown automatic -- long after the
    // laptop came back, with nothing to show where the permission came from.
    assert_eq!(
        d.memory.approval_rate("workspace_off"),
        None,
        "a stranger's yes was recorded as something you approve of"
    );
}

#[test]
fn an_unrecognised_sentence_is_not_answered_out_of_your_notes() {
    // `from_notes` sits above the policy gate on purpose -- reading back
    // something Atlas already wrote down risks nothing, when it is you
    // asking. It is a different act when somebody else is holding the
    // laptop, and `unknown` is not on either refusal list because it must
    // not be: ordinary conversation lands there too.
    let src = crate::common::source_of("daemon");
    // `run_command`, not `turn_from`: `turn_from` handles the modes and the
    // parked answers and then hands the rest here, which is where a new
    // command is actually parsed and run.
    let body = function_body(&src, "fn run_command(");
    // Since 27 Sep 2026 the notes are asked twice: before the model (the real
    // answers Atlas holds) and after it (the whole chain, for what the model
    // left unknown). Each call must sit behind the handed-over check, and
    // each helper must still be the one that reads the notes.
    for call in ["self.answer_before_the_model(said, _t)", "self.answer_locally(raw, _t)"] {
        let lookup = body.find(call).unwrap_or_else(|| panic!("the notes lookup is gone or renamed: {call}"));
        let before: String = body[..lookup].chars().rev().take(600).collect::<String>().chars().rev().collect();
        assert!(
            before.contains("handed_over()"),
            "{call} no longer asks whether the machine is handed over, so \
             an unrecognised sentence is answered out of the owner's research notes"
        );
    }
    // Since 27 Sep 2026 only the no-model fallback answers FROM the notes;
    // with a model, the notes go in front of it as hints (`notes_as_hints`),
    // which carries the same check itself -- the conversation path reaches
    // it without going through `run_command`'s handed-over guard above.
    assert!(function_body(&src, "fn answer_locally(").contains("self.from_notes(raw, t)"), "answer_locally no longer reads the notes");
    assert!(
        !function_body(&src, "fn answer_before_the_model(").contains("from_notes"),
        "the notes answer on their own before the model again, instead of being a hint to it"
    );
    let hints = code_only(function_body(&src, "fn notes_as_hints("));
    let guard = hints.find("handed_over()").expect("the notes go to the model while somebody else has the machine");
    let first_read = hints.find("self.facts").expect("notes_as_hints no longer reads the fact book");
    assert!(guard < first_read, "the handover is checked after the notes are read");
}

// --- the checks are where they have to be ----------------------------------

#[test]
fn the_daemons_own_turn_checks_before_anything_can_say_yes_for_it() {
    // Position, not presence. Below `classify_with_policy`, a standing grant
    // has already turned the action into `AutoProceed`; below the notes
    // lookup, the answer has already been read out of your notes.
    let src = crate::common::source_of("daemon");
    let body = function_body(&src, "fn run_command(");

    let check = body
        .find("handed_over_refusal(&intent)")
        .expect("the daemon's own turn no longer asks about the handover");
    let policy = body
        .find("classify_with_policy")
        .expect("run_command no longer classifies -- this guard needs rewriting");
    assert!(
        check < policy,
        "the handover check sits below the policy classification, where a standing \
         grant recorded when the machine was yours has already had its say"
    );

    // And the parked answer, which is a different function and a different
    // hole: `turn_from` resolves "yes" into a direct `execute` call without
    // ever reaching `run_command`.
    // Comments stripped first. The check in that branch is introduced by a
    // comment that says *why* it is ahead of `record_approval` -- and that
    // comment is itself ahead of the check, so searching the raw text finds
    // `record_approval` first and reports the opposite of the truth. This
    // tree has a standing caution about guards that match their own
    // explanation; here is one, caught by watching it fail.
    let parked = code_only(function_body(&src, "pub fn turn_from("));
    let parked = parked.as_str();
    let asked = parked
        .find("handed_over_refusal(&intent)")
        .expect("a parked approval no longer asks about the handover");
    let recorded = parked
        .find("record_approval")
        .expect("turn_from no longer records approvals -- this guard needs rewriting");
    assert!(
        asked < recorded,
        "a stranger's yes is recorded as a standing grant before the handover is \
         checked, so it outlives the handover it was given during"
    );
}

#[test]
fn execute_is_the_last_line_and_not_the_only_one() {
    // Every caller arrives here: the parked approval, the work queue, a
    // scheduled job coming due, and `turn_from` itself. The check has to be
    // ahead of the work, which for this function means ahead of
    // `resolve_subject` -- the first thing that can return a reply.
    let src = crate::common::source_of("daemon");
    let body = function_body(&src, "pub fn execute(&mut self, intent: &Intent)");
    let check = body
        .find("handed_over_refusal(intent)")
        .expect("execute no longer asks about the handover");
    let first_work = body.find("resolve_subject").expect("execute has changed shape");
    assert!(check < first_work, "the handover check is not the first thing execute does");

    // And it reads the install's own state rather than the active person's,
    // where switching profiles -- which passes through no gate at all --
    // would clear it.
    let helper = function_body(&src, "fn handover(&self)");
    assert!(
        helper.contains("install_state()"),
        "the daemon reads the handover from somewhere other than the install's own state"
    );
}

/// The same text with every `//` comment removed.
fn code_only(text: &str) -> String {
    text.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text between a function's opening brace and its matching close.
///
/// The same helper `tests/handed_over.rs` uses, and for the same reason:
/// `daemon.rs` is 7,000 lines, and a guard that searched the whole file
/// would pass on a mention in a comment about something else entirely. That
/// exact mistake has been made repeatedly in this tree.
fn function_body<'a>(text: &'a str, signature: &str) -> &'a str {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} is gone or renamed"));
    let open = start + text[start..].find('{').expect("no body");
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    for i in open..bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open..=i];
                }
            }
            _ => {}
        }
    }
    panic!("{signature} has no closing brace");
}

// --- the two-step way out of a handover ------------------------------------

#[test]
fn a_stranger_cannot_set_the_first_passphrase_and_let_themselves_out() {
    // `handover::take_back` refuses a vault with no passphrase, because a
    // first unlock *chooses* one rather than checking it. That refusal was
    // reversible in one command: whoever is holding the laptop runs
    // `atlas vault passphrase`, sets one, and takes the handover back with
    // the thing they just invented. Two steps, both allowed, and the
    // protection is gone.
    use atlas::handover::would_hand_out_the_way_back as would;

    // The case itself.
    for setting in ["passphrase", "set", "change", "recovery", "recovery-key", "newkey"] {
        assert!(would(true, false, Some(setting)), "`atlas vault {setting}` is allowed");
    }

    // And the three ways of being too strict, each of which breaks something
    // real. Blocking it when the machine is yours makes the vault unusable;
    // blocking it when a passphrase exists takes away the thing you most want
    // right after somebody watched you type it; blocking the read-only
    // commands leaves a guest unable to see why they are being refused.
    assert!(!would(false, false, Some("passphrase")), "blocked while the machine is yours");
    assert!(!would(true, true, Some("passphrase")), "a passphrase you have cannot be changed");
    assert!(!would(true, false, None), "reading the status is blocked");
    assert!(!would(true, false, Some("status")), "reading the status is blocked");
    assert!(!would(true, false, Some("recover")), "using a recovery key is blocked -- which is \
                                                   the one way back a stranger cannot fake");
}

#[test]
fn the_refusal_says_why_without_assuming_who_is_reading_it() {
    // Read by whoever is holding the laptop, who may not know there is an
    // owner in the picture at all -- so it says what it will not do and who
    // can, and does not scold anybody.
    let said = atlas::handover::not_yours_to_set();
    assert!(said.contains("handed over"), "it does not say why: {said}");
    assert!(said.contains("owner"), "it does not say who can: {said}");
    assert!(!said.contains('_'), "an identifier reached the sentence: {said}");
    assert!(!said.to_lowercase().contains("you can't"), "it reads as a telling-off: {said}");
}

#[test]
fn the_refusal_to_set_a_first_passphrase_is_actually_wired_to_the_command() {
    // The behaviour above lives in the library so it can be tested at all.
    // This is the other half: that `run_vault` calls it, and calls it before
    // it asks for anything.
    //
    // Both halves are needed and neither is enough. The first version of this
    // guard was text-only -- it asserted the condition appeared in `main.rs`
    // above the first prompt -- and turning the condition off with `if false
    // &&` left it green, because every string it looked for was still there.
    let src = crate::common::source_of("main");
    let body = code_only(function_body(&src, "fn run_vault("));
    let body = body.as_str();
    let check = body
        .find("would_hand_out_the_way_back")
        .expect("run_vault no longer asks whether this hands out the way back");
    let asks = body
        .find("ask_quietly")
        .expect("run_vault no longer asks for anything -- this guard needs rewriting");
    assert!(check < asks, "the check sits below the point where a passphrase is taken");
}

#[test]
fn a_recovery_key_is_a_way_back_that_is_not_in_the_room() {
    // Why the recovery key is allowed to end a handover at all, stated as a
    // test so the reasoning survives: `take_back` asks for proof, and this is
    // proof that lives on paper in a drawer rather than in the room where the
    // laptop was handed over. It is the one credential the person holding it
    // cannot have got hold of by being there.
    let mut v = atlas::vault::Vault::default();
    v.open("the one thing you actually know", 100, &atlas::vault::VaultConfig::default())
        .unwrap();
    let code = v.issue_recovery_key(100, &atlas::vault::VaultConfig::default()).unwrap();
    v.lock();

    let mut h = Handover::default();
    h.hand_over("Sam has it", 100);
    v.open_with_recovery_key(&code, 200, &atlas::vault::VaultConfig::default()).unwrap();
    assert!(h.take_back(&v, 200).is_ok(), "a recovery key could not end a handover");
}
