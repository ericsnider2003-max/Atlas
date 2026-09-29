use atlas::backlog::{Backlog, BacklogConfig, Blocker, Conditions};
use atlas::uia::{assess, outline, Node, Quality, Role};

fn online() -> Conditions {
    Conditions { online: true, screen_free: true, you_are_here: true, tools: vec![] }
}
fn offline() -> Conditions {
    Conditions { online: false, ..online() }
}
fn cfg() -> BacklogConfig {
    BacklogConfig::default()
}

// ================= nothing gets forgotten =================

#[test]
fn a_task_atlas_could_not_do_is_kept_not_dropped() {
    let mut b = Backlog::default();
    b.record("research the IETF QUIC v1 spec", Blocker::Offline, 100);
    assert_eq!(b.outstanding().len(), 1);
}

#[test]
fn it_stays_quiet_while_the_blocker_is_still_in_the_way() {
    let mut b = Backlog::default();
    b.record("research something", Blocker::Offline, 100);
    assert!(b.ready(&offline(), &cfg(), 100_000).is_empty(), "still offline, still quiet");
}

#[test]
fn it_offers_the_task_back_once_the_blocker_clears() {
    let mut b = Backlog::default();
    b.record("research something", Blocker::Offline, 100);
    let item = b.next_offer(&online(), &cfg(), 1000).expect("should offer");
    assert_eq!(item.request, "research something");
    let said = Backlog::phrase(&item);
    assert!(said.contains("research something"));
    assert!(said.contains("no connection"), "says why it didn't happen: {said}");
    assert!(said.ends_with("Want me to do it now?"), "asks, never just does it");
}

#[test]
fn it_asks_rather_than_silently_running_something_from_last_week() {
    let mut b = Backlog::default();
    b.record("close everything", Blocker::NeedsApproval, 100);
    let item = b.next_offer(&online(), &cfg(), 1000).unwrap();
    assert!(Backlog::phrase(&item).contains("Want me to"));
}

#[test]
fn asking_twice_for_the_same_thing_does_not_create_two_entries() {
    // A flaky connection would otherwise turn one task into twenty.
    let mut b = Backlog::default();
    for t in 0..10 {
        b.record("Research the QUIC V1 spec!", Blocker::Offline, t);
        b.record("research the quic v1 spec", Blocker::Offline, t);
    }
    assert_eq!(b.outstanding().len(), 1);
}

#[test]
fn reminders_get_further_apart_rather_than_repeating_forever() {
    let mut b = Backlog::default();
    b.record("research something", Blocker::Offline, 0);
    let c = cfg();

    let mut times = Vec::new();
    let mut t = 0u64;
    for _ in 0..4 {
        // advance until it is willing to speak again
        while b.ready(&online(), &c, t).is_empty() {
            t += 30;
            if t > 10_000_000 {
                break;
            }
        }
        times.push(t);
        b.next_offer(&online(), &c, t);
    }
    let gaps: Vec<u64> = times.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps[1] > gaps[0], "gaps must grow: {gaps:?}");
    assert!(gaps[2] > gaps[1], "gaps must keep growing: {gaps:?}");
}

#[test]
fn after_enough_unanswered_offers_it_stops_bringing_it_up() {
    let mut b = Backlog::default();
    b.record("research something", Blocker::Offline, 0);
    let c = cfg();
    let mut t = 0u64;
    for _ in 0..c.give_up_after {
        t += 10_000_000;
        assert!(b.next_offer(&online(), &c, t).is_some());
    }
    t += 10_000_000;
    assert!(b.next_offer(&online(), &c, t).is_none(), "four reminders is enough");
}

#[test]
fn saying_no_retires_it_permanently() {
    let mut b = Backlog::default();
    let id = b.record("research something", Blocker::Offline, 100);
    b.dismiss(id);
    assert!(b.next_offer(&online(), &cfg(), 10_000).is_none());
    assert!(b.outstanding().is_empty());
}

#[test]
fn asking_again_revives_something_you_previously_dismissed() {
    let mut b = Backlog::default();
    let id = b.record("research something", Blocker::Offline, 100);
    b.dismiss(id);
    b.record("research something", Blocker::Offline, 200);
    assert_eq!(b.outstanding().len(), 1, "you changed your mind, that's allowed");
}

#[test]
fn different_blockers_clear_on_different_conditions() {
    let mut b = Backlog::default();
    b.record("research a thing", Blocker::Offline, 100);
    b.record("open chrome", Blocker::NoScreenGap, 100);
    b.record("run whisper", Blocker::MissingTool("whisper-cli".into()), 100);

    let busy_offline = Conditions { online: false, screen_free: false, you_are_here: true, tools: vec![] };
    assert!(b.ready(&busy_offline, &cfg(), 10_000).is_empty());

    let mut c = online();
    c.screen_free = false;
    let ready: Vec<String> = b.ready(&c, &cfg(), 10_000).iter().map(|i| i.request.clone()).collect();
    assert_eq!(ready, vec!["research a thing"], "only the network one cleared");

    c.tools = vec!["whisper-cli".into()];
    c.screen_free = true;
    assert_eq!(b.ready(&c, &cfg(), 10_000).len(), 3);
}

#[test]
fn something_atlas_simply_cannot_do_is_never_offered_as_if_it_could() {
    let mut b = Backlog::default();
    b.record("edit my video", Blocker::Unsupported("edit video".into()), 100);
    assert!(b.next_offer(&online(), &cfg(), 10_000).is_none());
    assert_eq!(b.outstanding().len(), 1, "still on the list, just not offered");
}

#[test]
fn one_task_is_raised_at_a_time_oldest_first() {
    let mut b = Backlog::default();
    b.record("older thing", Blocker::Offline, 100);
    b.record("newer thing", Blocker::Offline, 500);
    let item = b.next_offer(&online(), &cfg(), 10_000).unwrap();
    assert_eq!(item.request, "older thing", "the thing that has waited longest");
}

#[test]
fn completed_and_stale_items_are_tidied_away() {
    let mut b = Backlog::default();
    let id = b.record("done thing", Blocker::Offline, 100);
    b.record("ancient thing", Blocker::Offline, 100);
    b.complete(id);
    b.tidy(&cfg(), 100 + 40 * 86_400);
    assert!(b.items.is_empty(), "done and expired both go");
}

#[test]
fn you_can_ask_what_is_outstanding() {
    let mut b = Backlog::default();
    assert_eq!(b.summary(), "Nothing outstanding.");
    b.record("research something", Blocker::Offline, 100);
    assert!(b.summary().contains("One thing"));
    b.record("open chrome", Blocker::NoScreenGap, 200);
    assert!(b.summary().contains("2 things"));
}

#[test]
fn the_backlog_survives_a_restart() {
    let d = std::env::temp_dir().join("atlas-backlog-test");
    let _ = std::fs::remove_dir_all(&d);
    let store = atlas::store::Store::new(&d);
    let mut b = Backlog::default();
    b.record("research something", Blocker::Offline, 100);
    b.save(&store).unwrap();
    assert_eq!(Backlog::load(&store).outstanding().len(), 1);
}

// ================= reading windows without screenshots =================

fn notepad_tree() -> Node {
    Node::new(Role::Window, "Untitled - Notepad").with(vec![
        Node::new(Role::MenuItem, "File"),
        Node::new(Role::MenuItem, "Edit"),
        Node::new(Role::Edit, "Text Editor").valued("meeting notes for Thursday"),
        Node::new(Role::Text, "Ln 1, Col 1"),
    ])
}

fn electron_tree() -> Node {
    // What a badly-behaved Electron app actually publishes.
    Node::new(Role::Window, "Discord").with(vec![
        Node::new(Role::Pane, "").with(vec![
            Node::new(Role::Pane, ""),
            Node::new(Role::Pane, ""),
            Node::new(Role::Pane, ""),
            Node::new(Role::Pane, ""),
            Node::new(Role::Pane, ""),
        ]),
    ])
}

#[test]
fn window_text_is_read_without_a_screenshot_or_a_vision_model() {
    let t = notepad_tree().text();
    assert!(t.contains("meeting notes for Thursday"), "got: {t}");
}

#[test]
fn a_field_reports_its_contents_rather_than_its_label() {
    let n = notepad_tree();
    let edit = n.find(&|x| x.role == Role::Edit).unwrap();
    assert_eq!(edit.value, "meeting notes for Thursday");
}

#[test]
fn repeated_labels_are_collapsed() {
    let n = Node::new(Role::Window, "w").with(vec![
        Node::new(Role::Text, "Save"),
        Node::new(Role::Text, "Save"),
        Node::new(Role::Text, "Cancel"),
    ]);
    assert_eq!(n.text(), "Save Cancel");
}

#[test]
fn controls_are_found_by_their_label() {
    let n = Node::new(Role::Window, "w").with(vec![
        Node::new(Role::Button, "Cancel"),
        Node::new(Role::Button, "Save"),
    ]);
    assert_eq!(n.by_name("save").unwrap().role, Role::Button);
}

#[test]
fn an_exact_label_beats_a_partial_one() {
    // "Save" must not click "Save As...".
    let n = Node::new(Role::Window, "w").with(vec![
        Node::new(Role::Button, "Save As..."),
        Node::new(Role::Button, "Save"),
    ]);
    assert_eq!(n.by_name("Save").unwrap().name, "Save");
}

#[test]
fn only_enabled_labelled_controls_are_offered_as_actionable() {
    let n = Node::new(Role::Window, "w").with(vec![
        Node::new(Role::Button, "Submit"),
        Node::new(Role::Button, "Greyed Out").disabled(),
        Node::new(Role::Button, ""),
        Node::new(Role::Text, "not clickable"),
    ]);
    let a = n.actionable();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].name, "Submit");
}

#[test]
fn a_cooperative_app_is_judged_usable() {
    assert_eq!(assess(&notepad_tree(), 3, 0.5), Quality::Usable);
}

#[test]
fn an_electron_blob_is_detected_so_atlas_falls_back_instead_of_lying() {
    // This is the whole point: reporting an unreadable window as an empty
    // document would be worse than admitting UIA can't see it.
    let q = assess(&electron_tree(), 3, 0.5);
    assert!(!q.usable(), "got {q:?}");
    assert!(q.explain().contains("screenshot"), "says what happens instead: {}", q.explain());
}

#[test]
fn an_app_publishing_nothing_is_detected() {
    assert_eq!(assess(&Node::new(Role::Window, "Game"), 3, 0.5), Quality::Empty);
}

#[test]
fn a_tiny_tree_is_treated_as_too_shallow_to_trust() {
    let n = Node::new(Role::Window, "w").with(vec![Node::new(Role::Pane, "p")]);
    assert!(matches!(assess(&n, 5, 0.5), Quality::TooShallow { .. }));
}

#[test]
fn control_types_map_from_the_windows_ids() {
    assert_eq!(Role::from_control_type(50000), Role::Button);
    assert_eq!(Role::from_control_type(50004), Role::Edit);
    assert_eq!(Role::from_control_type(99999), Role::Other);
}

#[test]
fn the_outline_is_capped_so_a_big_window_cannot_blow_the_context() {
    let mut kids = Vec::new();
    for i in 0..500 {
        kids.push(Node::new(Role::Button, &format!("Button {i}")));
    }
    let n = Node::new(Role::Window, "big").with(kids);
    let o = outline(&n, 20);
    assert_eq!(o.lines().count(), 20);
}

#[test]
fn the_outline_shows_roles_and_labels_for_the_model() {
    let o = outline(&notepad_tree(), 50);
    assert!(o.contains("Edit:"), "got:\n{o}");
    assert!(o.contains("meeting notes"), "field contents included");
}

#[test]
fn the_shipped_config_wires_the_backlog_and_uia_thresholds() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();
    assert!(t.backlog.backoff > 1.0, "reminders must get further apart");
    assert!(t.backlog.give_up_after > 0, "must eventually stop asking");
    assert!(t.backlog.expire_days > 0);
    assert!(t.uia.min_named_ratio > 0.0 && t.uia.min_named_ratio < 1.0);
}
