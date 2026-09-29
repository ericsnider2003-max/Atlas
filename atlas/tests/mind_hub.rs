use atlas::hub::{now_page, route, NowView, Page, Step, IDLE, THINKING};
use atlas::mind::{speak_brief, Item, Mind, Stage, Weight, Work};

// ================= what it's doing right now =================

fn working() -> Work {
    let mut w = Work::new(1, "tidy the downloads folder", false, 0);
    w.plan(&["list what's there".into(), "work out where each goes".into(), "move them".into()]);
    w.think(Stage::Gathering, "looking at what's in Downloads before deciding anything", 1);
    w.finish_step(0, None);
    w.think(Stage::Planning, "34 files, mostly PDFs and installers", 2);
    w
}

#[test]
fn atlas_can_say_what_it_is_doing_and_how_far_in() {
    // "Working on it" is not an answer.
    let w = working();
    let said = w.spoken();
    assert!(said.contains("tidy the downloads folder"));
    assert!(said.contains("work out where each goes"), "the actual step: {said}");
    assert!(said.contains("step 2 of 3"));
}

#[test]
fn the_reasoning_is_kept_so_you_can_ask_why() {
    let w = working();
    let recent = w.recent_thinking(5);
    assert_eq!(recent.len(), 2);
    assert!(recent[1].text.contains("34 files"));
    assert_eq!(recent[0].stage, Stage::Gathering);
}

#[test]
fn thinking_is_recorded_in_plain_language_not_as_log_lines() {
    for t in working().thoughts {
        assert!(!t.text.contains("::"), "not a log line: {}", t.text);
        assert!(t.text.chars().next().unwrap().is_lowercase() || t.text.starts_with('3'));
    }
}

#[test]
fn a_failed_step_is_shown_as_failed_rather_than_silently_skipped() {
    let mut w = working();
    w.finish_step(1, Some("no permission on that folder".into()));
    assert_eq!(w.steps[1].failed.as_deref(), Some("no permission on that folder"));
    assert!(!w.steps[1].done);
}

#[test]
fn being_stuck_says_what_it_is_stuck_on() {
    let mut w = working();
    w.stage = Stage::Stuck;
    w.blocked_on = Some("the folder is read-only".into());
    assert!(w.spoken().contains("read-only"));
}

#[test]
fn the_thinking_log_is_bounded_on_a_long_job() {
    let mut w = working();
    for i in 0..400 {
        w.think(Stage::Doing, &format!("step {i}"), i);
    }
    assert!(w.thoughts.len() <= 200);
    assert!(w.thoughts.last().unwrap().text.contains("399"), "the recent end is kept");
}

// ================= talking while it works =================

#[test]
fn background_work_is_tracked_separately_from_what_you_are_waiting_on() {
    // So a question from you doesn't disturb it.
    let mut m = Mind::default();
    let bg = m.begin("index the documents folder", true, 0);
    let fg = m.begin("open chrome", false, 1);
    assert_eq!(m.background().len(), 1);
    assert_eq!(m.background()[0].id, bg);
    assert_eq!(m.focus().unwrap().id, fg, "foreground is what you're waiting on");
}

#[test]
fn with_only_background_work_that_is_what_it_reports() {
    let mut m = Mind::default();
    m.begin("index the documents folder", true, 0);
    assert!(m.now().contains("index the documents folder"));
}

#[test]
fn several_things_at_once_are_summarised_rather_than_listed() {
    let mut m = Mind::default();
    m.begin("open chrome", false, 0);
    m.begin("index documents", true, 0);
    m.begin("research something", true, 0);
    let said = m.now();
    assert!(said.contains("open chrome"), "what you're waiting on leads");
    assert!(said.contains("2 others running"));
}

#[test]
fn nothing_running_says_so_plainly() {
    assert_eq!(Mind::default().now(), "Nothing at the moment.");
}

#[test]
fn finished_work_is_cleared_so_the_list_stays_about_now() {
    let mut m = Mind::default();
    for i in 0..10 {
        let id = m.begin(&format!("task {i}"), false, i);
        m.get_mut(id).unwrap().stage = Stage::Done;
    }
    m.tidy(3);
    assert_eq!(m.work.len(), 3);
}

// ================= briefs are spoken, in order =================

fn items() -> Vec<Item> {
    vec![
        Item { what: "the index is stale".into(), weight: Weight::Passing, because: "no rush".into() },
        Item {
            what: "a scheduled post can't go out".into(),
            weight: Weight::Urgent,
            because: "it's due at nine and it's over length".into(),
        },
        Item {
            what: "the wake word fix needs a decision".into(),
            weight: Weight::Blocking,
            because: "everything else is waiting on it".into(),
        },
    ]
}

#[test]
fn the_most_important_thing_is_said_first() {
    // A written brief is something you read later. Three sentences in
    // priority order is something you act on now.
    let said = speak_brief(&items());
    assert!(said.starts_with("First thing: a scheduled post can't go out"), "got: {said}");
    assert!(said.contains("due at nine"), "the top one gets its reasoning");
}

#[test]
fn the_rest_are_named_but_not_justified() {
    // A spoken list of six with justifications is unlistenable.
    let said = speak_brief(&items());
    assert!(said.contains("the wake word fix needs a decision"));
    assert!(!said.contains("everything else is waiting on it"), "no reasoning for the rest");
    assert!(said.len() < 220, "still speakable: {said}");
}

#[test]
fn a_blocking_item_is_phrased_as_holding_things_up() {
    let only_blocking = vec![Item {
        what: "the wake word fix".into(),
        weight: Weight::Blocking,
        because: "the rest is waiting".into(),
    }];
    assert!(speak_brief(&only_blocking).contains("is holding things up"));
}

#[test]
fn a_long_list_says_how_many_more_rather_than_reading_them() {
    let many: Vec<Item> = (0..9)
        .map(|i| Item { what: format!("thing {i}"), weight: Weight::Soon, because: "x".into() })
        .collect();
    let said = speak_brief(&many);
    assert!(said.contains("5 more after that"), "got: {said}");
}

#[test]
fn nothing_outstanding_says_nothing_needs_you() {
    assert_eq!(speak_brief(&[]), "Nothing needs you.");
}

// ================= the live view =================

/// The Now page for a title, its steps and what else is running.
fn now(title: &str, steps: Vec<(Step, String)>, background: Vec<String>, working: bool) -> String {
    now_page(&NowView {
        title: title.into(),
        since: "Started 10:00 · doing it".into(),
        steps,
        plain_from: 0,
        spent: None,
        fallback: "I stop and tell you what stopped it.".into(),
        paused: false,
        working,
        background,
    })
}

#[test]
fn the_live_page_shows_the_thinking_animation_while_it_works() {
    let page = now("tidy downloads", vec![], vec![], true);
    assert!(page.contains("class=think"));
    assert!(page.contains("animation:spin"), "it actually moves");
    let idle = now("Nothing at the moment", vec![], vec![], false);
    assert!(idle.contains("idle"), "and it settles when there's nothing to do");
}

#[test]
fn the_animation_needs_nothing_from_the_internet() {
    // It has to work offline because Atlas does. The xmlns is a namespace
    // identifier, not something fetched — what matters is that nothing is
    // loaded.
    for art in [THINKING, IDLE] {
        assert!(!art.contains("src="), "nothing is loaded");
        assert!(!art.contains("url("), "no external references");
        assert!(!art.contains("href=\"http"), "no links out");
        assert!(art.contains("<svg"), "drawn, not fetched");
    }
    // Its one script (26 Sep) is inline and fetches only this page again,
    // from Atlas itself.
    let page = now("x", vec![], vec![], true);
    assert!(!page.to_lowercase().contains("<script src"), "no script is fetched");
    assert!(!page.contains("fetch('http") && !page.contains("fetch(\"http"), "nothing from elsewhere");
}

#[test]
fn motion_is_dropped_for_anyone_who_asked_for_less_of_it() {
    let page = now("x", vec![], vec![], true);
    assert!(page.contains("prefers-reduced-motion"));
}

#[test]
fn the_live_page_refreshes_itself() {
    // A page you have to refresh to see live work isn't live. Since 26 Sep
    // it updates in place rather than with a meta refresh, which reloaded
    // the whole page every few seconds under a screen reader and a keyboard
    // (WCAG 2.2.1, technique F41) — and the updates can be paused.
    let page = now("x", vec![], vec![], true);
    assert!(!page.contains("http-equiv=refresh"), "no timed reload");
    assert!(page.contains(atlas::hub::LIVE_SCRIPT), "it updates itself in place");
    // One pause control, which the script finds by its id.
    assert_eq!(page.matches("id=livepause").count(), 1, "and the updates can be paused");
    assert_eq!(page.matches("getElementById('livepause')").count(), 1);
}

#[test]
fn steps_show_what_is_done_what_is_now_and_what_failed() {
    // The design's stream: what was checked, what it's on now, and a step
    // that failed shown as a reroute rather than hidden.
    let page = now(
        "tidy downloads",
        vec![
            (Step::Checked, "listed the files".into()),
            (Step::Rerouted, "moving them failed: disk full".into()),
            (Step::Now, "working out where each goes".into()),
        ],
        vec![],
        true,
    );
    assert!(page.contains("step checked"), "{page}");
    assert!(page.contains("<div class=txt>listed the files</div>"));
    assert!(page.contains("step now") && page.contains("working out where each goes"));
    assert!(page.contains("step rerouted") && page.contains("disk full"));
}

#[test]
fn the_thinking_is_shown_as_it_happens() {
    let page = now(
        "tidy downloads",
        vec![(Step::Plan, "looking at what's in Downloads".into()), (Step::Doing, "34 files, mostly PDFs".into())],
        vec![],
        true,
    );
    // In the order it happened: the plan comes before the doing.
    let plan = page.find("◇ Plan").expect("the plan step");
    let doing = page.find("▷ Doing").expect("the doing step");
    assert!(plan < doing, "oldest first");
    assert!(page[doing..].contains("34 files, mostly PDFs"));
}

#[test]
fn background_work_is_listed_without_taking_over_the_page() {
    let bg = vec!["indexing documents".to_string(), "researching the spec".to_string()];
    let page = now("open chrome", vec![], bg, true);
    assert!(page.contains("Also running") && page.contains("indexing documents · researching the spec"));
}

#[test]
fn the_live_view_is_reachable() {
    assert_eq!(route("/hub/now"), Some(Page::Now));
}

// ================= having an idea while it's busy =================

#[test]
fn a_new_request_pushes_the_running_one_out_of_sight_rather_than_queueing_you() {
    // You having an idea should never be limited by what Atlas happens to be
    // doing.
    let mut m = Mind::default();
    m.begin("index the documents folder", false, 0);
    match m.take_on("what's on my calendar", false, 10) {
        atlas::mind::Started::Demoted { moved, .. } => {
            assert!(moved.contains("index"), "the old one carried on out of sight");
        }
        o => panic!("{o:?}"),
    }
    assert_eq!(m.active().len(), 2, "both are still running");
    assert_eq!(m.background().len(), 1);
    assert_eq!(m.focus().unwrap().asked, "what's on my calendar", "the new one has the floor");
}

#[test]
fn something_that_needs_the_screen_runs_alongside_rather_than_being_demoted() {
    let mut m = Mind::default();
    let id = m.take_on("arrange my windows", true, 0).id();
    m.get_mut(id).unwrap();
    match m.take_on("open chrome", true, 10) {
        atlas::mind::Started::Alongside { with, .. } => assert_eq!(with, 1),
        o => panic!("{o:?}"),
    }
    assert_eq!(m.active().len(), 2);
}

#[test]
fn with_nothing_running_it_just_starts_and_says_nothing_about_it() {
    let mut m = Mind::default();
    let started = m.take_on("open chrome", true, 0);
    assert!(matches!(started, atlas::mind::Started::Straight { .. }));
    assert_eq!(started.spoken(), "", "no commentary when there's nothing to report");
}

#[test]
fn atlas_says_what_it_moved_so_you_know_it_didnt_drop_it() {
    let mut m = Mind::default();
    m.begin("research the spec", false, 0);
    let said = m.take_on("open chrome", true, 10).spoken();
    assert!(said.contains("Carrying on with research the spec in the background"), "got: {said}");
}

#[test]
fn you_can_bring_something_back_to_the_front() {
    let mut m = Mind::default();
    let id = m.begin("research the spec", false, 0);
    m.take_on("open chrome", false, 10);
    assert!(m.get(id).unwrap().background);
    assert!(m.promote(id));
    assert!(!m.get(id).unwrap().background);
}

#[test]
fn a_demoted_job_keeps_its_progress_and_its_thinking() {
    // Moving out of sight is not restarting.
    let mut m = Mind::default();
    let id = m.begin("index the documents folder", false, 0);
    m.get_mut(id).unwrap().plan(&["scan".into(), "read".into()]);
    m.get_mut(id).unwrap().finish_step(0, None);
    m.get_mut(id).unwrap().think(Stage::Doing, "12,000 files so far", 5);

    m.take_on("open chrome", true, 10);
    let w = m.get(id).unwrap();
    assert_eq!(w.progress(), (1, 2));
    assert_eq!(w.thoughts.len(), 1);
}
