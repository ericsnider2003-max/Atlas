//! Eric, 25 Sep 2026: "If one of them says no it also needs to suggest what
//! would make it ok, then it gets retested and re-presented for should this
//! be built and is this safe. The ones that need my call — yes, but make them
//! better and more thought through."
//!
//! What these hold:
//!
//! - **A no names what would make it a yes**, and the changed proposal goes
//!   back to *both* rooms — should it be built, and is it safe — because
//!   fixing one worry can open another. A no with no way to yes is said as
//!   that. After `MAX_RETESTS` passes, what's left is Eric's call.
//! - **The activity log is sealed**: each entry carries the hash of the one
//!   before, so an edited, removed or reordered entry shows, and so does a
//!   log that doesn't match the heads written beside it.
//! - **Model calls are graded** from what already happens (a rewrite, a
//!   figure not in its sources, a seat that wouldn't commit, your
//!   correction), kept beside the call log, and summed up per kind with an
//!   honest range. Still no prompt or reply text is stored.
//! - **Search measures itself** against known questions, and vectors from a
//!   different meaning model are never mixed with this one's.
//! - **A queued code change knows when the code has moved on** under it, and
//!   isn't written over newer work.

use atlas::activity::{Journal, Kind, Sealed};
use atlas::brain::Llm;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::error::Result;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-waytoyes-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

// ---------------------------------------------------------------- a no with a way to yes

#[test]
fn the_rooms_ask_a_no_for_what_would_make_it_ok() {
    let build = atlas::council::build_room();
    let safe = atlas::council::security_room();
    assert!(build.retests && safe.retests);
    for (_, p) in build.blind_prompts("should I build it?").iter().chain(safe.blind_prompts("is it safe?").iter()) {
        assert!(p.contains("It would be OK if"), "a seat isn't asked what would win it over: {p}");
    }
    // The general room is unchanged.
    let general = atlas::council::Council::new(Vec::new());
    assert!(!general.retests);
    let rooms: Vec<&str> = atlas::council::retest_rooms().into_iter().map(|(n, _)| n).collect();
    assert_eq!(rooms, vec!["Should it be built", "Is it safe"]);
}

#[test]
fn a_condition_is_read_and_a_flat_no_is_named() {
    use atlas::council::{conditions, flat_noes, parse_opinion, Lean};
    let with = parse_opinion("money", "Against. Nobody has said they'd pay. It would be OK if three people pre-paid.");
    assert_eq!(with.lean, Lean::Against);
    assert_eq!(with.would_change_my_mind.as_deref(), Some("three people pre-paid."));
    let flat = parse_opinion("reach", "Against. It won't reach anyone.");
    let yes = parse_opinion("trust", "For. It keeps everything local.");
    let all = vec![with.clone(), flat, yes];
    assert_eq!(conditions(&all), vec![("money".to_string(), "three people pre-paid".to_string())]);
    assert_eq!(flat_noes(&all), vec!["reach"]);
    let changed = atlas::council::amended("Should I build it?", &conditions(&all));
    assert!(changed.starts_with("Should I build it?"), "{changed}");
    assert!(changed.contains("three people pre-paid") && changed.contains("asked for by money"), "{changed}");
    assert!(changed.contains("Judge it as changed"), "{changed}");
}

/// Every seat says no with a condition the first time; once the proposal
/// has been changed that way, they say yes.
struct WonOver(Mutex<Vec<String>>);
impl Llm for WonOver {
    fn complete(&self, _s: &str, user: &str) -> Result<String> {
        self.0.lock().unwrap().push(user.to_string());
        if user.contains("The proposal has since been changed") {
            Ok("For. Changed that way the risk I raised is handled.".into())
        } else {
            Ok("Against. Nobody has asked for it yet. It would be OK if two people said they'd use it weekly.".into())
        }
    }
}

#[test]
fn a_no_with_a_condition_is_changed_and_put_to_both_rooms_again() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let seats = Arc::new(WonOver(Mutex::new(Vec::new())));
    let mut d = Daemon::new(cfg, &p, Some(seats.clone()), Store::new(scratch("retest")), Proactive::new(ProactiveConfig::default()));
    let _ = d.ask_the_room("should I build the hollow CLI as a product?");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut t = atlas::store::now() + 50;
    let mut out = Vec::new();
    while Instant::now() < deadline && !out.iter().any(|l: &String| l.contains("Changed that way")) {
        out.extend(d.tick(t));
        t += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    let said = out.join(" ");
    assert!(said.contains("First look"), "{said}");
    assert!(said.contains("It would be OK if"), "the condition wasn't said: {said}");
    assert!(said.contains("two people said they'd use it weekly"), "{said}");
    assert!(said.contains("Should it be built?") && said.contains("Is it safe?"), "not put to both rooms: {said}");
    assert!(!said.contains("that one's yours to decide"), "the changed proposal won them over: {said}");
    let asked = seats.0.lock().unwrap().clone();
    let retested: Vec<&String> = asked.iter().filter(|u| u.contains("The proposal has since been changed")).collect();
    assert_eq!(retested.len(), 10, "five build seats and five safety seats should judge the changed proposal");
    assert!(retested.iter().any(|u| u.contains("who pays")), "the build room wasn't asked again");
    // Every seat's call was recorded, and graded.
    let council: Vec<_> = d.trace.calls.iter().filter(|c| c.asked_by == "council").collect();
    assert!(council.len() >= 15, "{} council calls recorded", council.len());
    assert!(council.iter().all(|c| c.graded == Some(true)), "every seat committed and gave a reason");
}

/// Never satisfied: each pass asks for something new.
struct NeverSatisfied(Mutex<u32>);
impl Llm for NeverSatisfied {
    fn complete(&self, _s: &str, _user: &str) -> Result<String> {
        let mut n = self.0.lock().unwrap();
        *n += 1;
        Ok(format!("Against. Still a risk. It would be OK if change number {n} were made."))
    }
}

#[test]
fn what_is_still_a_no_after_the_retests_is_yours_to_decide() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let seats = Arc::new(NeverSatisfied(Mutex::new(0)));
    let mut d = Daemon::new(cfg, &p, Some(seats.clone()), Store::new(scratch("never")), Proactive::new(ProactiveConfig::default()));
    let _ = d.ask_the_room("is it safe to open the phone link to the internet?");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut t = atlas::store::now() + 50;
    let mut out = Vec::new();
    while Instant::now() < deadline && !out.iter().any(|l: &String| l.contains("yours to decide")) {
        out.extend(d.tick(t));
        t += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    let said = out.join(" ");
    assert!(said.contains("After 2 rounds of changes"), "{said}");
    assert!(said.contains("yours to decide"), "{said}");
    // The first look, then two passes of both rooms, and no more.
    assert_eq!(*seats.0.lock().unwrap(), 5 + 2 * 10);
}

// ---------------------------------------------------------------- the sealed log

fn journal(n: usize) -> Journal {
    let mut j = Journal::default();
    for i in 0..n {
        j.record_at(Kind::Published, &format!("entry {i}"), true, 1_000 + i as u64);
    }
    j
}

#[test]
fn an_untouched_log_checks_out() {
    let j = journal(5);
    match j.check(&[]) {
        Sealed::Intact { sealed, head } => {
            assert_eq!(sealed, 5);
            assert_eq!(head, j.events[4].hash);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(j.events[3].prev, j.events[2].hash);
    assert_eq!(j.events[4].seq, 4);
}

#[test]
fn an_edited_removed_or_reordered_entry_shows() {
    let mut edited = journal(5);
    edited.events[2].what = "entry 2, but nicer".into();
    // `at` is when the broken entry was written, which is how it's found.
    match edited.check(&[]) {
        Sealed::Broken { at, why } => {
            assert_eq!(at, 1_002);
            assert!(why.contains("changed after it was written"), "{why}");
        }
        other => panic!("an edited entry went unnoticed: {other:?}"),
    }

    let mut removed = journal(5);
    removed.events.remove(2);
    assert!(matches!(removed.check(&[]), Sealed::Broken { .. }), "a missing entry went unnoticed");

    let mut swapped = journal(5);
    swapped.events.swap(1, 2);
    assert!(matches!(swapped.check(&[]), Sealed::Broken { .. }), "a reordered log went unnoticed");
}

#[test]
fn a_log_rewritten_seals_and_all_disagrees_with_its_heads() {
    let j = journal(5);
    let heads = vec![(4u64, j.events[4].hash.clone())];
    assert!(matches!(j.check(&heads), Sealed::Intact { .. }));
    // Rewritten from scratch: every seal recomputed, so the chain is fine —
    // but it isn't the log whose head was written down.
    let mut forged = Journal::default();
    for i in 0..5 {
        forged.record_at(Kind::Published, &format!("entry {i}"), i != 3, 1_000 + i as u64);
    }
    assert!(matches!(forged.check(&[]), Sealed::Intact { .. }));
    assert!(matches!(forged.check(&heads), Sealed::Broken { .. }), "a forged log matched the recorded head");
    // A head beyond the end means entries were cut off the end.
    let mut cut = journal(5);
    cut.events.truncate(3);
    assert!(matches!(cut.check(&heads), Sealed::Broken { .. }), "entries cut off the end went unnoticed");
}

#[test]
fn old_entries_rolling_off_still_leave_a_checkable_log() {
    let j = journal(450);
    assert_eq!(j.events.len(), 400);
    assert_eq!(j.events[0].seq, 50);
    assert_eq!(j.events[0].prev, j.base);
    assert!(matches!(j.check(&[]), Sealed::Intact { sealed: 400, .. }), "{:?}", j.check(&[]));
}

#[test]
fn entries_from_before_the_seal_are_skipped_not_failed() {
    let mut j = Journal::default();
    j.events.push(atlas::activity::Event {
        at: 1,
        kind: Kind::Published,
        what: "from before".into(),
        ok: true,
        seq: 0,
        prev: String::new(),
        hash: String::new(),
    });
    j.record_at(Kind::Published, "sealed", true, 2);
    assert!(matches!(j.check(&[]), Sealed::Intact { sealed: 1, .. }), "{:?}", j.check(&[]));
}

#[test]
fn saving_writes_the_head_beside_the_log_and_the_check_reads_it() {
    let root = scratch("anchors");
    let store = Store::new(root.clone());
    let mut j = journal(3);
    j.save(&store).unwrap();
    j.save(&store).unwrap(); // unchanged head: not written twice
    j.record_at(Kind::Published, "one more", true, 5_000);
    j.save(&store).unwrap();
    let heads = atlas::activity::anchors(store.root());
    assert_eq!(heads.len(), 2, "{heads:?}");
    assert_eq!(heads[1].1, j.events[3].hash);
    let back = Journal::load(&store);
    assert!(matches!(back.check(&heads), Sealed::Intact { sealed: 4, .. }));
    assert!(atlas::activity::said_with_backups(&back.check(&heads), 0).len() > 10);
}

// ---------------------------------------------------------------- graded calls

#[test]
fn a_grade_is_kept_beside_the_log_and_survives_a_reload() {
    let dir = scratch("grades");
    let log = atlas::trace::log_path(&dir);
    let mut t = atlas::trace::Trace::default();
    for i in 0..3 {
        let mut c = atlas::trace::Call::new("conversation-reply", "m", 10 + i).finished(5, "p", "r");
        c.id = t.next_id();
        assert!(atlas::trace::append(&log, &c));
        t.record(c);
    }
    assert!(atlas::trace::grade_and_keep(&log, &mut t, 2, false, Some("sounded like a chatbot")));
    assert!(atlas::trace::grade_and_keep(&log, &mut t, 3, true, None));
    let back = atlas::trace::load(&log);
    let two = back.calls.iter().find(|c| c.id == 2).unwrap();
    assert_eq!(two.graded, Some(false));
    assert_eq!(two.why.as_deref(), Some("sounded like a chatbot"));
    assert_eq!(back.calls.iter().find(|c| c.id == 3).unwrap().graded, Some(true));
    assert_eq!(back.calls.iter().find(|c| c.id == 1).unwrap().graded, None);
    // Still nothing of what was said.
    let raw = std::fs::read_to_string(&log).unwrap() + &std::fs::read_to_string(atlas::trace::grades_path(&log)).unwrap();
    assert!(!raw.contains("\"p\"") && !raw.contains("\"r\""), "{raw}");
}

#[test]
fn the_scorecard_says_too_few_as_too_few_and_gives_a_range() {
    let mut t = atlas::trace::Trace::default();
    for i in 0..40u64 {
        let mut c = atlas::trace::Call::new("research", "m", i).finished(5, "p", "r");
        c.id = t.next_id();
        t.record(c);
        t.grade(i + 1, i % 4 != 0, Some("stated a figure that isn't in its sources"));
    }
    let mut few = atlas::trace::Call::new("council", "m", 0).finished(5, "p", "r");
    few.id = t.next_id();
    let few_id = few.id;
    t.record(few);
    t.grade(few_id, false, Some("wouldn't commit"));
    let card = t.scorecard();
    let research = card.iter().find(|s| s.asked_by == "research").unwrap();
    assert_eq!((research.graded, research.good), (40, 30));
    assert!(research.low < 0.75 && research.high > 0.75 && research.low > 0.5, "{research:?}");
    assert!(research.said().contains("75% good"), "{}", research.said());
    assert!(research.said().contains("stated a figure that isn't in its sources (10)"), "{}", research.said());
    let council = card.iter().find(|s| s.asked_by == "council").unwrap();
    assert!(council.said().contains("too few to call a rate"), "{}", council.said());
    assert_eq!(atlas::trace::wilson(0, 0), (0.0, 1.0));
}

// ---------------------------------------------------------------- search

#[test]
fn vectors_from_another_meaning_model_are_forgotten() {
    let mut r = atlas::meaning::Remembered::default();
    assert!(!r.for_model("model-a"));
    r.put("Note", "text", vec![1.0, 0.0]);
    assert!(!r.for_model("model-a"), "the same model kept its vectors");
    assert_eq!(r.len(), 1);
    assert!(r.for_model("model-b"), "a new model should drop them");
    assert_eq!(r.len(), 0);
    assert_eq!(r.model(), "model-b");
}

#[test]
fn a_new_model_file_is_a_new_fingerprint() {
    let dir = scratch("fp");
    let model = dir.join("model.gguf");
    std::fs::write(&model, b"one").unwrap();
    let cfg: atlas::meaning::MeaningConfig =
        serde_yaml::from_str(&format!("encoder:\n  command: embed\n  args: ['{}']\n", model.display())).unwrap();
    let vars = atlas::tools::Vars::new();
    let a = atlas::meaning::fingerprint(&cfg, &vars);
    assert!(!a.is_empty());
    assert_eq!(a, atlas::meaning::fingerprint(&cfg, &vars));
    std::fs::write(&model, b"a different, larger model").unwrap();
    assert_ne!(a, atlas::meaning::fingerprint(&cfg, &vars), "a replaced model file kept its fingerprint");
    assert_eq!(atlas::meaning::fingerprint(&atlas::meaning::MeaningConfig::default(), &vars), "");
}

fn notes() -> atlas::recall::Library {
    let dir = scratch("notes");
    for (f, body) in [
        ("boiler.md", "# Boiler service\n\nThe boiler was serviced by Hartley Heating, who replaced the pressure valve and topped up the expansion vessel."),
        ("car.md", "# Car insurance\n\nThe car insurance renews in March with Admiral; the excess is three hundred and fifty pounds for any claim."),
        ("garden.md", "# Garden plan\n\nPlant the tomatoes along the south fence after the last frost and keep the courgettes well watered."),
    ] {
        std::fs::write(dir.join(f), body).unwrap();
    }
    std::fs::write(dir.join("ignore.txt"), "not a note").unwrap();
    atlas::recall::library_from_dir(&dir)
}

#[test]
fn search_is_measured_against_questions_it_can_answer() {
    let lib = notes();
    assert_eq!(lib.pieces.len(), 3);
    let made = atlas::recall::questions_from(&lib, 10);
    assert_eq!(made.len(), 3);
    for q in &made {
        assert!(!q.yours);
        assert!(!q.ask.contains("boiler") || q.answer != "Boiler service", "a title word was left in: {}", q.ask);
    }
    let yours = atlas::recall::questions_written("# comments are skipped => x\nwho fixed the valve => Boiler service\nnot a question\n");
    assert_eq!(yours.len(), 1);
    assert!(yours[0].yours);
    let mut all = yours;
    all.extend(made);
    let scores = atlas::recall::measure(&lib, &all, None, &Default::default(), atlas::store::now());
    assert_eq!(scores.asked, 4);
    assert!(scores.recall_at_5 >= 0.75, "{scores:?}");
    assert!(scores.mrr > 0.5 && scores.ndcg_at_5 > 0.5, "{scores:?}");
    // Asking for a note that isn't there scores zero rather than failing.
    let missing = vec![atlas::recall::KnownQuestion { ask: "passport renewal".into(), answer: "Passport".into(), yours: true }];
    assert_eq!(atlas::recall::measure(&lib, &missing, None, &Default::default(), 0).recall_at_5, 0.0);
}

#[test]
fn a_search_check_says_whether_meaning_helps() {
    use atlas::recall::{Scores, SearchCheck};
    let s = |r: f32| Scores { recall_at_5: r, mrr: r, ndcg_at_5: r, asked: 20 };
    let words_only = SearchCheck { at: 1, model: String::new(), words: s(0.6), meaning: None };
    assert!(words_only.said(None).contains("60%"), "{}", words_only.said(None));
    let better = SearchCheck { at: 2, model: "a".into(), words: s(0.6), meaning: Some(s(0.8)) };
    assert!(better.said(None).contains("meaning finds 20 points more"), "{}", better.said(None));
    let worse = SearchCheck { at: 3, model: "b".into(), words: s(0.6), meaning: Some(s(0.5)) };
    let said = worse.said(Some(&better));
    assert!(said.contains("worth a look"), "{said}");
    assert!(said.contains("Against the previous meaning model: down 30 points"), "{said}");
}

#[test]
fn the_search_check_runs_as_an_errand_and_is_kept() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let mut d = Daemon::new(cfg, &p, None, Store::new(scratch("check-errand")), Proactive::new(ProactiveConfig::default()));
    d.library = notes();
    let said = d.start_search_check();
    assert!(said.starts_with("Measuring"), "{said}");
    assert!(d.crew.errands().iter().any(|e| e.name == "search-check"));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut t = atlas::store::now() + 50;
    let mut out = Vec::new();
    while Instant::now() < deadline && !out.iter().any(|l: &String| l.contains("Search check")) {
        out.extend(d.tick(t));
        t += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(out.iter().any(|l| l.contains("Search check")), "{out:?}");
    let kept: Vec<atlas::recall::SearchCheck> = d.store.load("search_checks");
    assert_eq!(kept.len(), 1);
    use atlas::which_errand::{describe, Candidate};
    let c = Candidate { id: 1, label: "search-check".into(), topic: None, started: 0, paused: false, can_hold: true };
    assert_eq!(describe(&c), "the search check");
}

// ---------------------------------------------------------------- changes that go stale

fn project(tag: &str) -> (PathBuf, atlas::workshop::Workshop, u64) {
    let folder = scratch(tag);
    std::fs::write(folder.join("dates.rs"), "fn parse() {}\n").unwrap();
    let mut w = atlas::workshop::Workshop::default();
    w.register("diary", &folder.display().to_string(), 1);
    let files = vec![atlas::workshop::FileEdit { path: "dates.rs".into(), content: "fn parse() { /* better */ }\n".into() }];
    let bases = atlas::workshop::bases_in(&folder.display().to_string(), &files.iter().map(|f| f.path.clone()).collect::<Vec<_>>());
    assert_eq!(bases[0].1.len(), 64, "a sha256");
    let id = w.propose("diary", "date parser", "parse dates", files, true, "", 2).unwrap();
    assert!(w.note_bases("diary", id, bases));
    (folder, w, id)
}

#[test]
fn a_change_is_applied_while_the_code_is_as_it_was() {
    let (_folder, w, _) = project("stale-fresh");
    assert!(w.plan_implementation("date parser").is_ok());
}

#[test]
fn a_change_written_against_old_code_is_not_written_over_new_code() {
    let (folder, mut w, _) = project("stale-moved");
    std::fs::write(folder.join("dates.rs"), "fn parse() {}\nfn you_added_this() {}\n").unwrap();
    assert_eq!(
        w.plan_implementation("date parser"),
        Err(atlas::workshop::ImplementError::Outdated { title: "date parser".into(), files: vec!["dates.rs".into()] })
    );
    let newly = w.mark_outdated();
    assert_eq!(newly, vec![("diary".to_string(), "date parser".to_string(), vec!["dates.rs".to_string()])]);
    assert!(w.mark_outdated().is_empty(), "said again an hour later");
    assert_eq!(w.outdated().len(), 1);
    // Put back as it was, it's current again.
    std::fs::write(folder.join("dates.rs"), "fn parse() {}\n").unwrap();
    assert!(w.mark_outdated().is_empty());
    assert!(w.outdated().is_empty());
    assert!(w.plan_implementation("date parser").is_ok());
}

#[test]
fn a_file_that_appeared_since_counts_as_the_code_moving_on() {
    let folder = scratch("stale-new");
    let mut w = atlas::workshop::Workshop::default();
    w.register("diary", &folder.display().to_string(), 1);
    let files = vec![atlas::workshop::FileEdit { path: "new.rs".into(), content: "// new".into() }];
    let bases = atlas::workshop::bases_in(&folder.display().to_string(), &files.iter().map(|f| f.path.clone()).collect::<Vec<_>>());
    assert_eq!(bases[0].1, atlas::workshop::ABSENT);
    let id = w.propose("diary", "new module", "", files, false, "", 2).unwrap();
    w.note_bases("diary", id, bases);
    std::fs::write(folder.join("new.rs"), "// someone else wrote this").unwrap();
    assert!(matches!(w.plan_implementation("new module"), Err(atlas::workshop::ImplementError::Outdated { .. })));
}

#[test]
fn a_change_queued_before_this_is_never_called_out_of_date() {
    let folder = scratch("stale-legacy");
    std::fs::write(folder.join("x.rs"), "old").unwrap();
    let mut w = atlas::workshop::Workshop::default();
    w.register("p", &folder.display().to_string(), 1);
    w.propose("p", "legacy", "", vec![atlas::workshop::FileEdit { path: "x.rs".into(), content: "new".into() }], false, "", 2);
    std::fs::write(folder.join("x.rs"), "changed").unwrap();
    assert!(w.mark_outdated().is_empty());
    assert!(w.plan_implementation("legacy").is_ok());
}

#[test]
fn atlas_refuses_an_outdated_change_and_says_why_and_whats_queued_names_it() {
    let (folder, w, _) = project("stale-daemon");
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let mut d = Daemon::new(cfg, &p, None, Store::new(scratch("stale-daemon-store")), Proactive::new(ProactiveConfig::default()));
    d.workshop = w;
    let newer = "fn parse() {}\nfn you_added_this() {}\n";
    std::fs::write(folder.join("dates.rs"), newer).unwrap();
    let said = d.execute_timed(&Intent::Implement("date parser".into()), "implement the date parser");
    assert!(said.contains("didn't apply") && said.contains("dates.rs"), "{said}");
    assert!(said.contains("redo it"), "{said}");
    assert_eq!(std::fs::read_to_string(folder.join("dates.rs")).unwrap(), newer, "newer work was overwritten");
    d.workshop.mark_outdated();
    let queued = d.execute_timed(&Intent::Queued, "what's queued");
    assert!(queued.contains("date parser") && queued.contains("out of date"), "{queued}");
}
