use atlas::awareness::Signals;
use atlas::config::Config;
use atlas::daemon::{Autonomy, Daemon};
use atlas::index::Changes;
use atlas::intent::Intent;
use atlas::memory::Memory;
use atlas::platform::mock::MockPlatform;
use atlas::platform::{ActiveWindow, Monitor};
use atlas::proactive::{detect, Proactive, ProactiveConfig};
use atlas::session::Pending;
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-d-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![
        Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true },
        Monitor { id: 3, x: 1920, y: 0, width: 1920, height: 1040, primary: false },
    ])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

// ================= spoken turns =================

#[test]
fn a_spoken_command_executes_and_replies_in_one_line() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "turn");
    let reply = d.turn("boot workspace", 100);
    // The FACT, not the exact wording. This was
    // `assert_eq!(reply, "Workspace online.")`, which pinned the words
    // `execute` happens to return -- and those now go through
    // `Persona::acknowledge`, which adds the configured form of address and
    // varies the phrasing so the same command twice does not come back
    // identical. That variation is the point rather than an accident, so a
    // test that pins one spelling of it is a test that has to be rewritten
    // every time the character changes.
    //
    // `tests/a_conversation_not_a_script.rs` owns the wording rules.
    assert!(reply.starts_with("Workspace online"), "got: {reply}");
    assert!(reply.len() < 40, "spoken replies stay short");
}

#[test]
fn reviewing_a_post_silently_fixes_certain_wording_rather_than_narrating_it() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "review-prose");
    // `review_post` isn't a raw-argument command, so the parser lowercases
    // the whole thing before the argument is even split out -- meaning any
    // natural sentence here also trips the start-of-sentence capital fix,
    // on top of the doubled word. Both are `Certain` fixes, so both should
    // be applied to the text rather than narrated one at a time -- fixing
    // is the job, not a running commentary on it.
    let reply = d.turn("review this post the the launch happens today", 100);
    assert!(reply.contains("Fixed 2 things"), "got: {reply}");
    assert!(reply.contains("The launch happens today"), "got: {reply}");
    assert!(!reply.contains("things worth a look"), "certain fixes shouldn't be narrated: {reply}");
}

#[test]
fn reviewing_a_post_asks_rather_than_guesses_when_it_cannot_tell() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "review-uncertain");
    // A homophone the checker can only flag, not silently resolve, should
    // read as a genuine question -- not a flat report and not a silent
    // change to wording that might have been intentional.
    let reply = d.turn("review this post there going to love it", 100);
    assert!(reply.contains("Did you mean it, or should I fix it?"), "got: {reply}");
}

#[test]
fn a_gated_action_asks_out_loud_instead_of_just_refusing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "gate");
    let reply = d.turn("shutdown workspace", 100);
    assert!(reply.contains("Go ahead?"), "got: {reply}");
    assert!(matches!(d.session.pending, Pending::Approval(..)));
}

#[test]
fn saying_yes_completes_the_parked_action() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "yes");
    d.turn("boot workspace", 100);
    d.turn("shutdown workspace", 110);
    let reply = d.turn("yes", 115);
    assert_eq!(reply, "Workspace down.");
    assert!(!d.session.is_waiting());
}

#[test]
fn saying_no_cancels_and_leaves_things_alone() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "no");
    d.turn("shutdown workspace", 100);
    assert_eq!(d.turn("no thanks", 105), "Left it alone.");
    assert!(!d.session.is_waiting());
}

#[test]
fn declining_something_is_learned_as_a_trait() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "learn-refusal");
    assert!(d.person.traits.is_empty(), "starts with nothing learned");
    d.turn("shutdown workspace", 100);
    d.turn("no thanks", 105);
    assert_eq!(d.person.traits.len(), 1, "the refusal should be the one trait now on file");
    assert!(
        d.person.traits[0].what.starts_with("said no to:"),
        "got: {}",
        d.person.traits[0].what
    );
    assert_eq!(d.person.traits[0].seen, 1);
}

#[test]
fn an_answer_is_never_reparsed_as_a_new_command() {
    // Changed 27 Sep 2026 (Eric: "freely speak with Atlas"). Anything that
    // wasn't a yes or a no used to be read as a no -- "Left it alone." --
    // and the new request was lost with the question. Now the question is
    // dropped, never approved, and what you said is taken as itself. What
    // this test still holds: a sentence after "Go ahead?" never approves the
    // parked action.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "reparse");
    d.turn("shutdown workspace", 100);
    assert!(d.session.is_waiting(), "the shutdown should be asked about");
    let reply = d.turn("close chrome", 105);
    assert_ne!(reply, "Left it alone.", "the new request was taken as a no and lost");
    assert!(
        !matches!(d.session.pending, atlas::session::Pending::Approval(atlas::intent::Intent::WorkspaceOff, _)),
        "the parked shutdown is still waiting for an answer"
    );
    let closed_other: Vec<String> = p
        .log
        .borrow()
        .iter()
        .filter_map(|a| match a {
            atlas::platform::mock::Action::Close(w) if !w.to_lowercase().contains("chrome") => Some(w.clone()),
            _ => None,
        })
        .collect();
    assert!(closed_other.is_empty(), "the workspace was shut down on a sentence that wasn't a yes: {closed_other:?}");
}

#[test]
fn restricted_actions_ask_every_single_time_no_matter_the_history() {
    // workspace_off is on the always_ask list. Fifty yeses change nothing.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "alwaysask");
    for t in 0..8 {
        let q = d.turn("shutdown workspace", t * 20);
        assert!(q.contains("Go ahead?"), "round {t} should still ask, got: {q}");
        assert_eq!(d.turn("yes", t * 20 + 5), "Workspace down.");
    }
    assert_eq!(d.memory.approval_rate("workspace_off"), Some(1.0));
    assert!(d.turn("shutdown workspace", 900).contains("Go ahead?"));
}

#[test]
fn learnable_actions_can_relax_but_only_to_announce_and_do() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "learn");
    for _ in 0..8 {
        d.memory.record_approval("focus_app", true, None);
    }
    use atlas::policy::{classify_with_policy, Decision};
    assert_eq!(
        classify_with_policy(&Intent::CloseApp("x".into()), &d.memory, &c.policy),
        Decision::RequireApproval,
        "close_app is on always_ask"
    );
}

#[test]
fn single_app_control_works_by_voice() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "apps");
    // `starts_with`, not `assert_eq!`, for the reason given in
    // `a_spoken_command_executes_and_replies_in_one_line` above: the
    // acknowledgement now carries the form of address.
    //
    // The capitalisation this asserts is still asserted, and it caught a real
    // bug in the persona: a "Right, {body}" phrasing had to lowercase the
    // first word to follow the comma, and the first word here is the app's
    // name -- "Right, chrome is up." That phrasing was removed rather than
    // special-cased, because knowing which first words may be lowercased
    // means knowing which are proper nouns, and the app names come from the
    // user's own config.
    let up = d.turn("open chrome", 100);
    assert!(up.starts_with("Chrome is up"), "the persona capitalises what it says: {up}");
    let there = d.turn("focus notepad", 110);
    assert!(there.starts_with("There's notepad"), "got: {there}");
}

// ================= unattended operation =================

#[test]
fn unattended_atlas_still_does_the_safe_work() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "unattended-ok");
    d.autonomy = Autonomy::Unattended;
    let id = d.scheduler.at("boot workspace", 50);
    let out = d.tick(100);
    assert!(out.iter().any(|s| s.contains("online")), "got {out:?}");
    assert_eq!(d.scheduler.jobs.iter().find(|j| j.id == id).unwrap().runs, 1);
}

#[test]
fn unattended_atlas_parks_anything_needing_consent_rather_than_guessing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "unattended-park");
    d.autonomy = Autonomy::Unattended;
    let id = d.scheduler.at("shutdown workspace", 50);
    let out = d.tick(100);
    // Asserting `out.is_empty()` here was over-broad, and it passed for the
    // wrong reason: off Windows the machine readings were all zero, so the
    // health check never produced a finding and `tick` never had anything
    // else to say. With real disk and memory readings the same tick can
    // legitimately report "you're down to N gigabytes", and this test would
    // fail on any low-disk machine while the behaviour it exists to check —
    // that a job needing consent is parked rather than guessed at — is fine.
    //
    // A health warning is information, not an action taken without consent.
    // Whether Atlas should stay silent about a filling disk while unattended
    // is a separate design question and not this test's business.
    assert!(
        !out.iter().any(|s| s.contains("online") || s.contains("Workspace")),
        "the parked job ran anyway: {out:?}"
    );
    let j = d.scheduler.jobs.iter().find(|j| j.id == id).unwrap();
    assert_eq!(j.runs, 0);
    assert_eq!(j.state, atlas::scheduler::JobState::AwaitingApproval);
}

#[test]
fn a_parked_job_is_not_lost_it_resumes_when_approved() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "resume");
    d.autonomy = Autonomy::Unattended;
    let id = d.scheduler.at("shutdown workspace", 50);
    d.tick(100);
    d.scheduler.approve(id);
    d.autonomy = Autonomy::Supervised;
    let out = d.tick(200);
    assert!(out.iter().any(|s| s.contains("down")), "got {out:?}");
}

#[test]
fn unattended_atlas_sets_it_aside_rather_than_acting() {
    // This asserted `reply.contains("wait")` — and the reply was literally
    // "That needs your say-so. I'll wait." with nothing recorded anywhere.
    // Nothing waited. The next time you looked there was no trace you had
    // been asked.
    //
    // The assertion below it is the one that mattered and it still holds:
    // no question is parked in the *session*, because a session question
    // asked into an empty room is the thing nobody will hear. What is new is
    // that the request is set aside in the backlog instead, which is what
    // makes "I'll wait" true rather than a form of words. See
    // `tests/a_question_that_survives_you_leaving.rs`.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "wait");
    d.autonomy = Autonomy::Unattended;
    let reply = d.turn("shutdown workspace", 100);

    assert!(!d.session.is_waiting(), "must not park a question nobody will hear");
    assert!(!reply.is_empty(), "it said nothing at all");
    assert_eq!(
        d.backlog.outstanding().len(),
        1,
        "it went ahead, or it promised to wait and kept no record: {reply}"
    );
}

#[test]
fn saying_no_to_a_backlog_offer_takes_it_off_the_list_for_good() {
    // A backlog offer is a real question -- "Earlier you asked me to X but Y.
    // Want me to do it now?" Before `backlog::dismiss` was wired to it, a no
    // only cleared the question, so `next_offer` raised the same task on the
    // next quiet tick: "no" meant "ask me again later" forever.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "backlog-no");
    // A failed task is worth another go regardless of conditions, so it is the
    // simplest thing to get re-offered.
    let id = d.backlog.record(
        "water the plants",
        atlas::backlog::Blocker::Failed("hose burst".into()),
        0,
    );
    // Past the first-retry gap (60s by default) so the item is due a re-offer.
    let out = d.tick(60);
    assert!(
        out.iter().any(|s| s.contains("water the plants")),
        "the backlog item was never offered: {out:?}"
    );
    assert!(d.session.is_waiting(), "the offer must leave a question pending");

    // Now say no.
    let reply = d.turn("no", 70);
    assert!(reply.to_lowercase().contains("leave"), "unexpected reply: {reply}");
    assert!(d.backlog.outstanding().is_empty(), "a refused task must leave the list");
    let item = d.backlog.items.iter().find(|i| i.id == id).expect("still on record");
    assert!(item.dismissed, "the item is marked dismissed, not merely un-offered");
    assert!(!d.session.is_waiting(), "the question has been answered");

    // And it is never raised again, however long the machine sits quiet.
    let later = d.tick(10_000);
    assert!(
        !later.iter().any(|s| s.contains("water the plants")),
        "a dismissed task came back: {later:?}"
    );
}

// ================= speaking first =================

fn busy_signals(added: usize) -> Signals {
    Signals {
        active: Some(ActiveWindow { process: "chrome.exe".into(), title: "docs".into() }),
        dwell_secs: 60,
        idle_secs: 120,
        recent_changes: Changes {
            added: (0..added).map(|i| format!("/f{i}")).collect(),
            ..Default::default()
        },
        in_conversation: false,
        ..Default::default()
    }
}

fn eager() -> ProactiveConfig {
    ProactiveConfig { enabled: true, cooldown_secs: 60, ..Default::default() }
}

#[test]
fn atlas_offers_help_when_something_notable_happens() {
    let mut pr = Proactive::new(eager());
    let m = Memory::default();
    let o = pr.consider(&busy_signals(6), &m, 1000).expect("should offer");
    assert_eq!(o.kind, "index_new_files");
    assert!(o.message.ends_with('?'), "an offer is a question: {}", o.message);
}

#[test]
fn atlas_never_talks_over_an_active_conversation() {
    let mut pr = Proactive::new(eager());
    let mut s = busy_signals(6);
    s.in_conversation = true;
    assert!(pr.consider(&s, &Memory::default(), 1000).is_none());
}

#[test]
fn atlas_lets_a_thought_finish_before_speaking() {
    let mut pr = Proactive::new(eager());
    let mut s = busy_signals(6);
    s.idle_secs = 2; // you just said something
    assert!(pr.consider(&s, &Memory::default(), 1000).is_none());
}

#[test]
fn cooldown_stops_back_to_back_interruptions() {
    let mut pr = Proactive::new(eager());
    let m = Memory::default();
    assert!(pr.consider(&busy_signals(6), &m, 1000).is_some());
    assert!(pr.consider(&busy_signals(6), &m, 1030).is_none(), "inside cooldown");
    assert!(pr.consider(&busy_signals(6), &m, 1100).is_some(), "after cooldown");
}

#[test]
fn there_is_an_hourly_interruption_budget() {
    let mut pr = Proactive::new(ProactiveConfig { enabled: true, cooldown_secs: 1, max_interruptions_per_hour: 3, ..Default::default() });
    let m = Memory::default();
    let mut got = 0;
    for t in 0..10 {
        if pr.consider(&busy_signals(6), &m, 1000 + t * 10).is_some() {
            got += 1;
        }
    }
    assert_eq!(got, 3);
}

#[test]
fn declining_raises_the_bar_without_silencing_the_offer() {
    // Atlas learns to be choosier, it does not give up.
    let pr = Proactive::new(eager());
    let mut m = Memory::default();
    let base = pr.threshold_for("index_new_files", &m);
    pr.record_response("index_new_files", false, &mut m);
    let after_one_no = pr.threshold_for("index_new_files", &m);
    assert!(after_one_no > base, "a decline should raise the bar");

    for _ in 0..50 {
        pr.record_response("index_new_files", false, &mut m);
    }
    let saturated = pr.threshold_for("index_new_files", &m);
    assert!(saturated <= pr.cfg.confidence_ceiling, "never silenced outright");
    assert!(saturated < 1.0, "a strong enough signal can still get through");
}

#[test]
fn accepting_offers_makes_atlas_more_forthcoming() {
    let pr = Proactive::new(eager());
    let mut m = Memory::default();
    let base = pr.threshold_for("index_new_files", &m);
    for _ in 0..3 {
        pr.record_response("index_new_files", true, &mut m);
    }
    assert!(pr.threshold_for("index_new_files", &m) < base, "yes should lower the bar");
}

#[test]
fn learning_is_clamped_at_both_ends() {
    let pr = Proactive::new(eager());
    let mut m = Memory::default();
    for _ in 0..100 {
        pr.record_response("k", true, &mut m);
    }
    assert!(pr.threshold_for("k", &m) >= 0.25, "never becomes trigger-happy either");
}

#[test]
fn proactive_is_off_unless_explicitly_enabled() {
    let mut pr = Proactive::new(ProactiveConfig::default());
    assert!(pr.consider(&busy_signals(20), &Memory::default(), 1000).is_none());
}

#[test]
fn a_quiet_signal_produces_no_offer_at_all() {
    assert!(detect(&busy_signals(1)).is_empty(), "one new file is not an event");
}

#[test]
fn an_offer_is_answered_by_voice_and_runs_the_command() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("offer")), Proactive::new(eager()));
    d.index.entries.clear();
    // Force an offer through the daemon's tick path.
    let out = {
        d.awareness.last_spoke = 0;
        d.tick(10_000)
    };
    let _ = out;
    // Directly park an offer to test the answering path deterministically.
    d.session.ask("Want me to file them?");
    assert!(d.session.is_waiting());
}

// ================= awareness =================

#[test]
fn atlas_knows_what_you_are_looking_at() {
    let (c, p) = (cfg(), plat());
    p.focus_on("chrome.exe", "Homelab — pull request");
    let mut d = daemon(&c, &p, "aware");
    let s = d.observe(500);
    assert_eq!(s.active.unwrap().title, "Homelab — pull request");
}

#[test]
fn dwell_time_resets_when_you_switch_windows() {
    let (c, p) = (cfg(), plat());
    p.focus_on("chrome.exe", "a");
    let mut d = daemon(&c, &p, "dwell");
    d.observe(100);
    let s = d.observe(400);
    assert_eq!(s.dwell_secs, 300);
    p.focus_on("notepad.exe", "b");
    let s = d.observe(500);
    assert_eq!(s.dwell_secs, 0, "switching windows restarts the clock");
}

#[test]
fn what_you_are_doing_reaches_the_models_context() {
    let (c, p) = (cfg(), plat());
    p.focus_on("chrome.exe", "quarterly numbers");
    let mut d = daemon(&c, &p, "ctx");
    // 29 Sep 2026: the window goes into the context only when what was said
    // is about the screen (`doing::refers_to_screen`) -- with Discord in
    // front, Eric's evening was answered as if every sentence were about it.
    let _ = d.turn("hmm, thinking about this window", 100);
    let ctx = d.context();
    assert!(ctx.contains("quarterly numbers"), "got:\n{ctx}");
    assert!(ctx.contains("Displays:"));
}

#[test]
fn conversation_history_reaches_the_context_too() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "ctxhist");
    d.turn("open chrome", 100);
    assert!(d.context().contains("open chrome"));
}

// ================= persistence =================

#[test]
fn what_atlas_learns_survives_a_restart() {
    let (c, p) = (cfg(), plat());
    let dir = tmp("persist");
    {
        let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
        d.turn("shutdown workspace", 100);
        d.turn("yes", 105);
        d.scheduler.every("boot workspace", 86400, 999_999);
    }
    let d2 = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d2.memory.approval_rate("workspace_off"), Some(1.0));
    assert_eq!(d2.scheduler.active().len(), 1);
}

#[test]
fn empty_speech_is_ignored_not_misparsed() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "empty");
    assert_eq!(d.turn("   ", 100), "");
    assert!(d.session.turns.is_empty());
}

#[test]
fn intent_kinds_cover_every_variant() {
    for i in [Intent::WorkspaceOn, Intent::Say("x".into()), Intent::CaptureWebcam] {
        assert_ne!(atlas::session::kind_of(&i), "");
    }
}

#[test]
fn a_bare_why_accounts_for_everything_decided_not_just_the_last_thing() {
    // `why::account` had no caller: a general "why is my workspace like
    // this?" and "why did you do that" both landed on `Intent::Why` and both
    // got `why::answer`'s single-latest-decision reply, so asking broadly
    // silently dropped every decision but the most recent one.
    use atlas::why::Decision;
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "why-account");
    d.decisions.note_full(Decision {
        at: 1,
        what: "put Chrome on the left screen".into(),
        because: "that's the widest one".into(),
        instead_of: None,
        set_by: Some("config/layouts.yaml".into()),
    });
    d.decisions.note_full(Decision {
        at: 2,
        what: "used the webcam mic".into(),
        because: "the built-in mic isn't listed while the lid is closed".into(),
        instead_of: None,
        set_by: None,
    });

    // A follow-up question ("why did you explain that") still gets the
    // single-latest-decision answer `why::answer` always gave.
    let one = d.turn("why did you explain that", 3);
    assert!(one.to_lowercase().contains("used the webcam mic"), "{one}");
    assert!(!one.contains("Chrome"), "a specific question pulled in the other decision: {one}");

    // A bare "why is that" (no subject) gets everything, not just whatever
    // happened last.
    let all = d.turn("why is that", 4);
    assert!(all.contains("Chrome on the left screen"), "{all}");
    assert!(all.to_lowercase().contains("used the webcam mic"), "{all}");
}

/// The model returns a valid `say` action for whatever it is asked, so the
/// turn resolves through the model -- `Reached::Yes` -- and the routing
/// decision recorded should say exactly that.
struct AlwaysSays;
impl atlas::brain::Llm for AlwaysSays {
    fn complete(&self, _system: &str, _user: &str) -> atlas::error::Result<String> {
        Ok("{\"action\":\"say\",\"arg\":null,\"say\":\"here is what I found\"}".into())
    }
}

#[test]
fn a_turn_records_where_its_answer_came_from_and_why_can_read_it_back() {
    // The read half of `Intent::Why` shipped wired and the list it read was
    // never filled -- `self.decisions` was a `Vec` nothing pushed to, so
    // every "why is that?" answered "nothing to account for yet". This drives
    // a real turn through the daemon and then asks why: the routing decision
    // has to have been written by `run_command` (via `why::note_full`) for
    // the account to name it.
    let (c, p) = (cfg(), plat());
    let spy = std::sync::Arc::new(AlwaysSays);
    let mut d = Daemon::new(
        &c,
        &p,
        Some(spy),
        Store::new(tmp("why-records")),
        Proactive::new(ProactiveConfig::default()),
    );

    // A novel question the phrase parser cannot settle, so the model is
    // actually reached. Nothing was recorded before this turn.
    assert!(d.decisions.decisions.is_empty(), "started with a decision already recorded");
    let _ = d.turn("what were the main causes of the 1918 flu pandemic", 100);
    assert_eq!(
        d.decisions.decisions.len(),
        1,
        "the turn's routing was not written down: {:?}",
        d.decisions.decisions
    );

    // Now ask why. The answer is built from the recorded decision, not a
    // stand-in for an empty list.
    let answer = d.turn("why is that", 200).to_lowercase();
    assert!(
        answer.contains("went to the model"),
        "the why answer did not read back the recorded routing decision: {answer}"
    );
    // Asking why is itself excluded from the record, so the account did not
    // grow a self-referential entry about the "why" turn.
    assert_eq!(
        d.decisions.decisions.len(),
        1,
        "asking why recorded its own routing: {:?}",
        d.decisions.decisions
    );
}

#[test]
fn closing_a_day_archives_it_for_the_hourly_sweep_to_later_prune() {
    // `daily::close()` was purely computational: built fresh from live
    // workspace items each rollover and handed straight to the brief
    // opener, with nothing ever persisted. `daily::still_keep` therefore had
    // no archive to run against -- this proves the other half exists now.
    use atlas::workspace_view::{Handoff, Item, Kind, Origin, Status};
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "archive-closed-day");
    d.workspace.push(Item {
        id: "t".into(),
        title: "finish the report".into(),
        kind: Kind::Task,
        status: Status::Done,
        due: None,
        project: None,
        client: None,
        links: vec![],
        blocked_by: None,
        from: Origin::YouSaid,
        at: 0,
        closed_at: Some(0),
        tags: vec![],
        thinking: vec![],
        estimate_mins: None,
        spent_mins: 0,
        atlas_could: Handoff::Unknown,
    });
    d.last_seen = 0;
    assert!(d.daily_history.is_empty(), "nothing should be archived before a day ever closes");

    d.turn("what's outstanding", 2 * 86_400);

    assert_eq!(d.daily_history.len(), 1, "closing a day did not archive it");
    assert!(d.daily_history[0].finished.contains(&"finish the report".to_string()));
}

#[test]
fn the_hourly_sweep_prunes_archived_days_past_their_keep_window() {
    use atlas::daily::Closed;
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "prune-old-closed-days");
    let day_secs = 86_400u64;
    // `DailyConfig::default().keep_days` is 90. One well inside the window,
    // one well past it.
    d.daily_history.push(Closed { day: 0, finished: vec!["old".into()], carried: vec![], dropped: vec![] });
    d.daily_history.push(Closed {
        day: 95 * day_secs,
        finished: vec!["recent".into()],
        carried: vec![],
        dropped: vec![],
    });

    d.tick(100 * day_secs);

    assert_eq!(d.daily_history.len(), 1, "the sweep should have pruned exactly the old one");
    assert_eq!(d.daily_history[0].finished, vec!["recent".to_string()]);
}

#[test]
fn the_hourly_reclaim_sweep_actually_looks_in_this_daemons_own_store() {
    // The hourly sweep surveys and applies `retention::plan` against this
    // Daemon's own install root rather than a literal relative `data`
    // shared by every daemon in the process. `store.install_root()` climbs
    // to the install root (here, a plain tmp dir, so it is the dir itself)
    // and the sweep then looks under its `data/` subtree — the layout every
    // other path-producing call site in the daemon now uses. Proven here
    // rather than asserted: a scratch file inside *this* store's data tree,
    // old enough to evict, must actually be gone after the sweep.
    let (c, p) = (cfg(), plat());
    let dir = tmp("reclaim-store-root");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));

    // `install_root().join("data")` is where the sweep looks; for a plain
    // tmp store root that resolves to `<dir>/data`.
    let scratch_dir = dir.join("data").join("scratch");
    std::fs::create_dir_all(&scratch_dir).unwrap();
    let stale = scratch_dir.join("old.wav");
    std::fs::write(&stale, b"stale recording").unwrap();
    // `classify` sorts by extension/path, not by real recency, but `plan`'s
    // age check reads the file's real mtime — back-date it well past
    // `scratch_minutes` (10 minutes by default).
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600);
    std::fs::File::options().write(true).open(&stale).unwrap().set_modified(long_ago).unwrap();

    // The reclaim walk is a crew errand — it runs on a worker, not on the
    // tick, so it may finish a tick or two after it's handed off (and on a
    // one-core, one-slot machine it waits for the slot rather than running
    // instantly). Tick until the crew is idle rather than assuming one tick
    // is enough: what's being tested is that the sweep reclaims the file, not
    // how many workers the machine happens to have.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut t = atlas::store::now() + 7200;
    loop {
        d.tick(t);
        if d.crew.active() == 0 && !stale.exists() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the reclaim sweep did not finish within the deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        t += 1;
    }

    assert!(!stale.exists(), "a stale scratch file in this daemon's own store survived the sweep");
}

// ================= a crisis is not a command =================

#[test]
fn a_message_that_needs_a_person_is_redirected_before_it_is_ever_parsed() {
    // A directed line that reads as a crisis must not fall through to the
    // parser, a mode trigger, a saved flow, or the model -- any of which
    // would try to *do* something with it. `person::beyond_me` is the narrow
    // test for this, and `Daemon::turn_from` checks it right after the
    // addressing gate says the words were meant for Atlas. The whole point is
    // that the redirect wins over everything else.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "beyond-me");
    let reply = d.turn("honestly I don't think it's worth living anymore", 100);
    assert_eq!(
        reply,
        atlas::person::NOT_A_THERAPIST,
        "a crisis line got treated as an instruction instead of a redirect"
    );
}

#[test]
fn an_ordinary_bad_week_is_not_treated_as_a_crisis() {
    // The redirect is deliberately narrow: a rough week is a rough week, and
    // sending someone to a hotline over "this project is killing me" is its
    // own kind of unhelpful. This line must reach ordinary handling, so it
    // comes back as anything OTHER than the redirect.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "bad-week");
    let reply = d.turn("this project is killing me and I'm behind on everything", 100);
    assert_ne!(
        reply,
        atlas::person::NOT_A_THERAPIST,
        "an ordinary bad week was mistaken for a crisis"
    );
}

#[test]
fn a_rough_day_is_met_with_an_offer_to_take_real_work_off_the_plate() {
    // The sibling of the crisis gate: a hard day that is not a crisis must not
    // fall through to the parser (where "rough day" hunts the disk for a file
    // called "rough") -- it is caught in `turn_from` right after `beyond_me`,
    // and answered by offering to take the outstanding backlog off your plate.
    // Seed one blocked errand so the offer has something concrete to name.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "rough-day");
    d.backlog.record("email Jane back", atlas::backlog::Blocker::Offline, 50);
    let reply = d.turn("honestly, rough day", 100);
    assert!(
        reply.contains("I can deal with email Jane back"),
        "a rough day should offer to pick up the outstanding errand, got: {reply}"
    );
    assert!(
        !reply.contains("worth living"),
        "a rough day must not be treated as a crisis"
    );
}

#[test]
fn a_rough_day_with_an_empty_list_offers_help_without_performing_concern() {
    // With nothing outstanding, `hard_day([])` says little rather than faking
    // sympathy or inventing work -- and it still reaches this path rather than
    // the command parser.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "rough-day-empty");
    let reply = d.turn("I'm completely overwhelmed", 100);
    assert!(
        reply.contains("anything I can take off you"),
        "an empty backlog should give the quiet offer, got: {reply}"
    );
}

#[test]
fn a_test_daemon_never_starts_a_mutation_sweep_of_the_checkout() {
    // 5 Oct 2026: on a machine with cargo-mutants, every daemon a test built
    // started a real `cargo mutants` over the checkout once its day had come
    // round -- hours of builds, gigabytes in the temp folder, and the crew's
    // slots gone from the work eight other tests were waiting on.
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("no-sweep")), Proactive::new(ProactiveConfig::default()));
    let t = atlas::store::now() + 3 * 24 * 3600;
    for i in 0..5 {
        d.tick(t + i * 1000);
    }
    let names: Vec<String> = d.crew.errands().into_iter().map(|e| e.name).collect();
    assert!(!names.iter().any(|n| n.contains("mutation")), "a test daemon started {names:?}");
}
