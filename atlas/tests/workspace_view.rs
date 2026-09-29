use atlas::hub::{route, workspace_page, Page};
use atlas::workspace_view::{
    apply, grouped, overview, shipped, spoken, Group, Item, Kind, Origin, Shape, Status,
    WorkspaceConfig,
};

const DAY: u64 = 86_400;

fn item(id: &str, title: &str, kind: Kind, status: Status, due: Option<u64>, project: Option<&str>, blocked_by: Option<&str>, at: u64) -> Item {
    Item {
        id: id.into(),
        title: title.into(),
        kind,
        status,
        due,
        project: project.map(str::to_string),
        blocked_by: blocked_by.map(str::to_string),
        from: Origin::YouSaid,
        client: None,
        links: vec![],
        at,
        closed_at: if status == Status::Done { Some(at) } else { None },
        tags: vec![],
        thinking: vec![],
        estimate_mins: None,
        spent_mins: 0,
        atlas_could: atlas::workspace_view::Handoff::Unknown,
    }
}

fn workspace() -> Vec<Item> {
    vec![
        item("1", "decide on the VPS", Kind::Decision, Status::NeedsYou, None, Some("Homelab"), None, 0),
        item("2", "chase the certification", Kind::Task, Status::Blocked, Some(5 * DAY), Some("Homelab"), Some("the broker"), 0),
        item("3", "file the quarterly", Kind::Task, Status::Blocked, Some(2 * DAY), None, Some("the broker"), 0),
        item("4", "cut the fee video", Kind::Draft, Status::Doing, None, Some("Content"), None, 0),
        item("5", "what if we cached it", Kind::Idea, Status::Waiting, None, None, None, DAY),
        item("6", "old thing", Kind::Task, Status::Done, None, None, None, 0),
    ]
}

// ================= views are questions, not folders =================

#[test]
fn the_same_item_appears_in_every_view_it_answers() {
    // Nothing is *in* a view. That's the whole difference from folders.
    let items = workspace();
    let live = shipped().into_iter().find(|v| v.name == "Everything live").unwrap();
    let stuck = shipped().into_iter().find(|v| v.name == "Stuck").unwrap();

    let in_live = apply(&items, &live, 0);
    let in_stuck = apply(&items, &stuck, 0);
    let cert = "chase the certification";
    assert!(in_live.iter().any(|i| i.title == cert));
    assert!(in_stuck.iter().any(|i| i.title == cert), "same item, two views");
}

#[test]
fn the_default_view_opens_on_what_to_do_next_not_on_everything() {
    assert_eq!(WorkspaceConfig::default().default_view, "Now");
    let now = shipped().into_iter().find(|v| v.name == "Now").unwrap();
    let items = workspace();
    let showing = apply(&items, &now, 0);
    assert!(showing.len() <= 2, "Now is not a list of everything");
}

#[test]
fn something_waiting_on_you_outranks_a_date() {
    // A date you can't act on isn't urgent, it's just soon.
    let now = shipped().into_iter().find(|v| v.name == "Now").unwrap();
    let items = workspace();
    let showing = apply(&items, &now, 0);
    assert_eq!(showing[0].title, "decide on the VPS");
}

#[test]
fn finished_and_dropped_things_are_out_of_the_live_views() {
    let live = shipped().into_iter().find(|v| v.name == "Everything live").unwrap();
    let items = workspace();
    assert!(!apply(&items, &live, 0).iter().any(|i| i.status == Status::Done));
}

#[test]
fn a_view_can_narrow_to_one_kind_without_moving_anything() {
    let content = shipped().into_iter().find(|v| v.name == "Content").unwrap();
    let items = workspace();
    let showing = apply(&items, &content, 0);
    assert_eq!(showing.len(), 1);
    assert_eq!(showing[0].kind, Kind::Draft);
}

#[test]
fn there_are_few_views_because_thirty_means_arranging_views_instead_of_working() {
    assert!(shipped().len() <= 6);
    assert!(shipped().iter().any(|v| v.shape == Shape::Board));
    assert!(shipped().iter().any(|v| v.shape == Shape::List));
}

// ================= what it tells you =================

#[test]
fn one_thing_holding_several_others_up_is_the_most_useful_line_there_is() {
    let o = overview(&workspace(), 0);
    let (what, n) = o.biggest_blocker.clone().unwrap();
    assert_eq!(what, "the broker");
    assert_eq!(n, 2);
    assert!(spoken(&workspace(), 0).contains("2 things are waiting on the broker"));
}

#[test]
fn one_thing_blocked_on_something_is_not_reported_as_a_pattern() {
    let single = vec![item("1", "x", Kind::Task, Status::Blocked, None, None, Some("Priya"), 0)];
    assert!(overview(&single, 0).biggest_blocker.is_none());
}

#[test]
fn it_leads_with_what_to_do_next_rather_than_a_count() {
    // A count makes a long list sound like an accusation.
    let said = spoken(&workspace(), 0);
    assert!(said.starts_with("First: decide on the VPS."));
}

#[test]
fn things_past_their_date_are_counted_but_finished_ones_are_not() {
    let o = overview(&workspace(), 10 * DAY);
    assert_eq!(o.overdue, 2, "both blocked tasks are past their dates");
    assert!(spoken(&workspace(), 10 * DAY).contains("past their date"));
}

#[test]
fn an_empty_workspace_says_so_in_two_words() {
    assert_eq!(spoken(&[], 0), "Nothing outstanding.");
}

// ================= the board =================

#[test]
fn a_board_groups_by_status_so_stuck_is_a_shape_not_a_word() {
    let live = shipped().into_iter().find(|v| v.name == "Everything live").unwrap();
    let items = workspace();
    let showing = apply(&items, &live, 0);
    let groups = grouped(&showing, Group::Status);
    assert!(groups.iter().any(|(k, _)| k == "Blocked"));
    assert!(groups.iter().any(|(k, _)| k == "NeedsYou"));
}

#[test]
fn the_page_shows_only_numbers_you_would_act_on() {
    let items = workspace();
    let live = shipped().into_iter().find(|v| v.name == "Everything live").unwrap();
    let showing = apply(&items, &live, 0);
    let groups = grouped(&showing, Group::Status);
    let names: Vec<String> = shipped().iter().map(|v| v.name.clone()).collect();
    let html = workspace_page(&live, &groups, &overview(&items, 0), &names);

    assert!(html.contains("need you"));
    assert!(html.contains("stuck"));
    // A total count of everything is not one of them.
    assert!(!html.contains("6 items"));
}

#[test]
fn the_page_names_what_is_holding_things_up() {
    let items = workspace();
    let live = shipped().into_iter().find(|v| v.name == "Everything live").unwrap();
    let showing = apply(&items, &live, 0);
    let html = workspace_page(&live, &grouped(&showing, Group::Status), &overview(&items, 0), &[]);
    assert!(html.contains("waiting on the broker"));
    assert!(html.contains("worth more than anything else here"));
}

#[test]
fn views_are_links_across_the_top_rather_than_a_tree_down_the_side() {
    let live = shipped().into_iter().find(|v| v.name == "Everything live").unwrap();
    let names: Vec<String> = shipped().iter().map(|v| v.name.clone()).collect();
    let html = workspace_page(&live, &[], &overview(&[], 0), &names);
    assert!(html.contains("nav class=views"));
    assert!(html.contains("view here"), "the one you're on is marked");
}

#[test]
fn columns_scroll_sideways_rather_than_squashing() {
    // A squashed column is unreadable.
    let live = shipped().into_iter().find(|v| v.name == "Now").unwrap();
    let html = workspace_page(&live, &[], &overview(&[], 0), &[]);
    assert!(html.contains("overflow-x:auto"));
    assert!(html.contains("min-width:260px"));
}

#[test]
fn the_workspace_page_is_reachable() {
    assert_eq!(route("/hub/workspace"), Some(Page::Workspace));
}
