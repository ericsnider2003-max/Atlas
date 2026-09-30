//! Eric's rulings of 25 Sep 2026, H8 to H13: bringing back what he dropped,
//! working a decision over turns, remembering what was let go, his own words
//! as speech hints, summarising old conversation without losing what
//! matters, and the H13 set (lock awareness, suggestions by name, the
//! fact-conflict rule, notes that point nowhere, learning from his edits,
//! which project a request was about, the sync route, the overnight account).

use atlas::backlog::Blocker;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::facts::{Book, Fact, Kind};
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const DAY: u64 = 86_400;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-keeps-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cfg() -> &'static Config {
    Box::leak(Box::new(Config::load(Path::new("config")).unwrap()))
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(scratch(tag)), Proactive::new(ProactiveConfig::default()))
}

// ------------------------------------------------------------------ H8

#[test]
fn a_dropped_task_is_kept_and_can_be_brought_back() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "drop");
    let t = atlas::store::now();
    d.backlog.record("call the dentist about the crown", Blocker::Unsupported("do that one for you".into()), t - 3 * DAY);
    let said = d.execute(&Intent::DropTask("drop the task dentist".into()));
    assert!(said.starts_with("Dropped \"call the dentist about the crown\""), "{said}");
    assert!(d.backlog.outstanding().is_empty(), "off the list");
    assert_eq!(d.dropped.len(), 1);
    assert_eq!(d.dropped[0].carried_for, 3);

    let said = d.execute(&Intent::DropTask("bring back what i dropped".into()));
    assert!(said.ends_with("Put it back on your list?"), "asks rather than nags: {said}");
    let said = d.turn("yes", t + 1);
    assert!(said.contains("is back on your list"), "{said}");
    assert_eq!(d.backlog.outstanding().len(), 1);
    assert!(d.dropped.is_empty());
}

// ------------------------------------------------------------------ H9

#[test]
fn a_drafted_decision_leans_with_its_reason_and_shows_the_working_on_request() {
    use atlas::decide::{from_draft, said, Weight};
    let draft = "QUESTION: do I want steady income or more room to grow?\n\
        DEFAULT: keep the retainer | the contract pays at least a third more\n\
        OPTION: take the contract | six months, then nothing lined up | the client renews; I can find the next one\n\
        OPTION: keep the retainer | less money, less growth | the client stays\n\
        AGAINST: the contract could end with nothing next\n\
        IF WRONG: three months of lower income, found out in six months, can switch back\n\
        CONFIDENCE: 6 | don't know if they renew\n\
        OPEN: would the retainer client take a pause?";
    let d = from_draft("should I take the contract or keep the retainer", Weight::WorthWorking, draft);
    assert_eq!(d.options.len(), 2);
    let s = said(&d);
    assert!(s.starts_with("I'd lean toward keep the retainer"), "an opinion, with a reason: {s}");
    assert!(s.contains("less money, less growth") && s.ends_with("Want the whole working?"), "{s}");
    let working = d.lean().unwrap().written();
    assert!(working.contains("What would change it: the contract could end with nothing next"), "{working}");
}

#[test]
fn without_a_model_a_decision_is_worked_with_you_a_move_a_turn() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "decide");
    let t = 1_000_000;
    let mut q = d.turn("should I take the contract or keep the retainer", t);
    assert!(q.contains("question you should be asking"), "{q}");
    let answers = [
        ("question you should be asking", "whether I want steady income"),
        ("boring default", "keep the retainer"),
        ("what else could you do", "take the contract"),
        ("what are the options", "take the contract or keep the retainer"),
        ("\"keep the retainer\" resting on", "the client stays"),
        ("\"take the contract\" resting on", "they renew; I find the next one"),
        ("strongest case against", "the contract could end with nothing next"),
        ("what breaks", "three lean months, and I can switch back"),
    ];
    let mut turns = 0;
    while !q.starts_with("I'd lean toward") {
        let lq = q.to_lowercase();
        let a = answers.iter().find(|(k, _)| lq.contains(&k.to_lowercase())).map(|(_, a)| *a).unwrap_or_else(|| panic!("unexpected question: {q}"));
        turns += 1;
        q = d.turn(a, t + turns);
        assert!(turns < 12, "never got to a lean: {q}");
    }
    assert!(q.ends_with("Want the whole working?"), "{q}");
    let working = d.turn("yes", t + 20);
    assert!(working.contains("What would change it"), "{working}");
    assert!(d.deciding.is_some(), "kept, so it can be picked back up");
}

#[test]
fn a_decision_can_be_set_aside_and_picked_back_up() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "decide-aside");
    let _ = d.turn("should I move the studio or stay put", 2_000_000);
    let said = d.turn("not now", 2_000_001);
    assert!(said.contains("put the decision aside"), "{said}");
    let said = d.turn("back to the decision", 2_000_002);
    assert!(said.contains("question you should be asking"), "{said}");
}

// ------------------------------------------------------------------ H10

#[test]
fn what_was_let_go_to_make_room_is_remembered_as_having_been_known() {
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().consolidate.keep_at_most = 2;
    let c: &'static Config = Box::leak(Box::new(c));
    let p = plat();
    let mut d = daemon(c, &p, "stones");
    let t = 1_000;
    d.learned("the tier two fee on the broker is four dollars a month", "broker site", t);
    d.learned("the ferry to the island leaves at seven fifteen", "ferry timetable", t + 1);
    d.learned("the museum is closed on mondays in winter", "museum page", t + 2);
    d.learned("the library renews books twice online", "library site", t + 3);
    assert!(d.known.len() <= 2);
    assert!(!d.stones.is_empty(), "a line kept for what went");
    // Asked about whatever was let go, in words of its own.
    let gone = d.stones[0].clone();
    let said = d.turn(&format!("what was {}?", gone.about), t + 10);
    assert!(said.contains("I knew something about") && said.contains(&gone.source), "{said}");
}

// ------------------------------------------------------------------ H11

#[test]
fn your_own_words_become_the_speech_models_hints() {
    let mut v = atlas::improve::Vocabulary::default();
    v.learn("Ask Maya about Northwind. What did Maya say about Northwind?");
    assert_eq!(atlas::improve::hint_args(&v, &[]).0, "--prompt");
    let hints = v.hints(24);
    assert!(hints.contains(&"Maya".to_string()) && hints.contains(&"Northwind".to_string()), "{hints:?}");
    assert!(!hints.contains(&"What".to_string()) && !hints.contains(&"Ask".to_string()), "ordinary sentence starts aren't names: {hints:?}");
    assert_eq!(atlas::improve::hint_args(&atlas::improve::Vocabulary::default(), &[]), (String::new(), String::new()));

    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "vocab");
    let _ = d.turn("remind me to call Priya tomorrow", 1_000);
    let _ = d.turn("did Priya reply", 1_001);
    assert!(d.vocab.hints(24).contains(&"Priya".to_string()), "{:?}", d.vocab.words);
}

// ------------------------------------------------------------------ H12

#[test]
fn old_conversation_is_summarised_and_the_important_things_survive() {
    use atlas::thread::{with_the_important_kept, Exchange};
    let ex = |said: &str| Exchange { at: 0, said: said.into(), reply: "ok".into(), about: None };
    let old = vec![
        ex("how's the weather"),
        ex("remember the gate code is 4471"),
        ex("we decided to ship the tripod video on Friday"),
        ex("thanks"),
    ];
    let kept = with_the_important_kept("Chatted about the weather.", &old, "");
    assert!(kept.contains("4471") && kept.contains("tripod video"), "what the model dropped is put back: {kept}");
    let kept = with_the_important_kept("Gate code 4471; tripod video ships Friday, decided.", &old, "");
    assert!(!kept.contains("Said along the way"), "nothing added when it's all there: {kept}");

    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "fold");
    let _ = d.turn("remember the gate code is 4471", 5_000);
    for i in 0..30u64 {
        d.thread.append(&format!("small talk {i}"), "ok", None, 5_001 + i);
    }
    let _ = d.turn("what time is it", 5_100);
    assert!(d.thread.summary.contains("4471"), "folded without forgetting it: {}", d.thread.summary);
    assert!(!d.thread.summary.ends_with("earlier exchanges"), "not a bare count");
}

// ------------------------------------------------------------------ H13

#[test]
fn a_locked_machine_is_up_but_nobody_is_at_it() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "locked");
    let _ = d.tick(1_000);
    assert_eq!(d.running(), atlas::awake::Running::Awake);
    *p.locked.borrow_mut() = Some(true);
    let _ = d.tick(1_002);
    assert_eq!(d.running(), atlas::awake::Running::LockedButUp);
    assert!(d.running().work_continues(), "locked is not asleep");
    // Nothing that needs you at the machine happens while it's locked.
    let said = d.turn("type my code 123456", 1_003);
    assert!(!said.to_lowercase().contains("typed"), "{said}");
}

#[test]
fn a_suggestion_is_switched_off_and_on_by_its_name() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "suggest");
    d.anticipator.rules.push(
        serde_yaml::from_str(
            "name: morning backup\ntrigger: !daily {hour: 8, minute: 0, days: []}\ncommand: back up\nenabled: true\n",
        )
        .unwrap(),
    );
    let said = d.execute(&Intent::Suggestions("stop suggesting the morning backup".into()));
    assert!(said.starts_with("I'll stop suggesting morning backup"), "{said}");
    assert!(!d.anticipator.rules[0].enabled);
    let said = d.execute(&Intent::Suggestions("what do you suggest on your own".into()));
    assert!(said.contains("morning backup (off)"), "{said}");
    let said = d.execute(&Intent::Suggestions("start suggesting the morning backup".into()));
    assert!(said.contains("again") && d.anticipator.rules[0].enabled, "{said}");
}

#[test]
fn what_you_told_it_beats_what_it_noticed_and_newer_beats_older() {
    let mut b = Book::default();
    b.learn(Fact::new("car", "my car is a Honda", "my car is a Honda", Kind::You, 10), 10);
    // Atlas noticing something newer doesn't overwrite what you said.
    b.learn(Fact::new("car", "my car is a Ford", "my car is a Ford", Kind::Noticed, 20), 20);
    let kept = b.get("car").unwrap();
    assert_eq!((kept.summary.as_str(), kept.kind), ("my car is a Honda", Kind::You));
    assert_eq!(kept.confirmed, 1, "the observation still counted as seeing it again");
    // You saying something newer does.
    b.learn(Fact::new("car", "my car is a Toyota", "my car is a Toyota", Kind::Reference, 30), 30);
    assert_eq!(b.get("car").unwrap().summary, "my car is a Toyota");
}

#[test]
fn notes_that_point_at_nothing_yet_are_named() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "dangling");
    let mut f = Fact::new("launch plan", "the launch plan", "see the budget sheet", Kind::Project, 1);
    f.links.push("budget-sheet".into());
    d.facts.put(f);
    let said = d.execute(&Intent::Dangling);
    assert!(said.contains("launch-plan points to budget-sheet") || said.contains("points to budget-sheet"), "{said}");
}

#[test]
fn your_edit_of_the_words_is_asked_about_again_and_learned_from() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "edit");
    let _ = d.execute(&Intent::DraftPost("x".into()));
    let _ = d.execute(&Intent::ReviewPost(
        "new video goes up tonight at eight and it has the tripod and the night market and the lenses".into(),
    ));
    let t = atlas::store::now();
    let said = d.turn("change it to new video tonight", t);
    assert!(said.contains("new video tonight"), "asked about the new words: {said}");
    assert!(d.person.traits.iter().any(|tr| tr.what.contains("cuts what I write")), "{:?}", d.person.traits);
}

#[test]
fn the_project_a_request_was_about_is_kept() {
    use atlas::person::project_named;
    assert_eq!(project_named("where are we on the Northwind project", &[]).as_deref(), Some("Northwind"));
    assert_eq!(project_named("add a task to project atlas", &[]).as_deref(), Some("Atlas"));
    let known = vec![("Tripod Review".to_string(), 0u64)];
    assert_eq!(project_named("draft the intro for the tripod review", &known).as_deref(), Some("Tripod Review"));
    assert_eq!(project_named("what time is it", &known), None);

    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "project");
    let _ = d.turn("where are we on the Northwind project", 1_000);
    assert!(d.person.projects.iter().any(|(n, t)| n == "Northwind" && *t == 1_000), "{:?}", d.person.projects);
}

#[test]
fn the_sync_route_is_the_best_one_there_is_and_said() {
    use atlas::sync::{route_of, Carry};
    assert_eq!(route_of(Path::new("C:\\Users\\erics\\OneDrive\\Atlas sync")).0, Carry::CloudFolder);
    assert_eq!(route_of(Path::new("E:\\atlas")).0, Carry::Cable);
}

#[test]
fn the_overnight_account_is_there_when_asked() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "overnight");
    assert_eq!(d.execute(&Intent::Overnight), "I haven't worked overnight yet.");
    d.store.save("overnight_detail", &"Overnight session\n\n[needs you] the flaky test".to_string()).unwrap();
    assert!(d.execute(&Intent::Overnight).contains("the flaky test"));
}

#[test]
fn what_must_survive_a_summary_is_picked_out_by_what_it_is() {
    use atlas::thread::{must_keep, Exchange};
    let ex = |said: &str| Exchange { at: 0, said: said.into(), reply: String::new(), about: None };
    let kept = must_keep(&[ex("hello"), ex("don't forget the vet on the 14th"), ex("we decided on blue"), ex("nice")]);
    assert_eq!(kept, vec!["don't forget the vet on the 14th", "we decided on blue"]);
}

#[test]
fn the_ways_to_the_same_place_are_said_with_what_each_costs() {
    use atlas::decide::{Decision, Option_, Weight};
    let mut d = Decision::new("which camera", Weight::WorthWorking);
    assert_eq!(d.paths(), None, "one way isn't a choice");
    for (what, costs) in [("buy the a7", "£1,800"), ("rent one a month", "not said")] {
        d.options.push(Option_ { what: what.into(), costs: costs.into(), rests_on: vec![], set_aside: None });
    }
    assert_eq!(d.paths().as_deref(), Some("There are 2 ways to get there: buy the a7 (costs you £1,800); rent one a month."));
}
