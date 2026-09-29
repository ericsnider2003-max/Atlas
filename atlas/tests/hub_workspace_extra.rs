use atlas::hub::{looking_back_page, route, Page};
use atlas::layout_prefs::{Block, Layout, LayoutConfig, Size, DRAG_SCRIPT};
use atlas::messaging::{
    interrupts, sort, spoken as msg_spoken, Message, MessagingConfig, Platform, Reach, Sort,
    WHY_NOT_WHATSAPP,
};
use atlas::workspace_view::{
    apply, day_of, day_spoken, shipped, why_this_took_so_long, Item, Kind, Origin, Status, Thinking,
    Thought,
};

const DAY: u64 = 86_400;

fn thought(at: u64, kind: Thinking, what: &str) -> Thought {
    Thought { at, kind, what: what.into() }
}

fn item(title: &str, status: Status, client: Option<&str>, at: u64, closed: Option<u64>, thinking: Vec<Thought>) -> Item {
    Item {
        id: title.into(),
        title: title.into(),
        kind: Kind::Task,
        status,
        due: None,
        project: Some("Homelab".into()),
        client: client.map(str::to_string),
        links: vec![],
        blocked_by: None,
        from: Origin::YouSaid,
        at,
        closed_at: closed,
        tags: vec![],
        thinking,
        estimate_mins: None,
        spent_mins: 0,
        atlas_could: atlas::workspace_view::Handoff::Unknown,
    }
}

// ================= looking back =================

fn history() -> Vec<Item> {
    vec![
        item("file the quarterly", Status::Done, None, 3 * DAY, Some(5 * DAY), vec![
            thought(5 * DAY + 100, Thinking::Tried, "the export, which timed out"),
            thought(5 * DAY + 200, Thinking::Chose, "the CSV route instead, since it needs no session"),
        ]),
        item("chase the certification", Status::Blocked, Some("Marta"), 2 * DAY, None, vec![
            thought(5 * DAY + 300, Thinking::Stuck, "no reply for six days"),
        ]),
        item("cut the fee video", Status::Doing, None, 5 * DAY + 400, None, vec![]),
    ]
}

#[test]
fn a_past_day_says_what_finished_not_what_was_on_the_list() {
    // Those differ. The list is what you meant to do.
    let d = day_of(&history(), 5 * DAY);
    assert_eq!(d.finished, vec!["file the quarterly"]);
    assert_eq!(d.started, vec!["cut the fee video"]);
}

#[test]
fn what_was_already_open_and_still_is_says_whether_it_was_a_good_day() {
    let d = day_of(&history(), 5 * DAY);
    assert!(d.carried_over.contains(&"chase the certification".to_string()));
    assert!(day_spoken(&d).contains("already open and still are"));
}

#[test]
fn a_day_where_nothing_moved_says_so() {
    assert_eq!(day_spoken(&day_of(&history(), 40 * DAY)), "Nothing moved that day.");
}

#[test]
fn atlas_working_out_is_kept_per_item_so_why_did_this_take_so_long_is_answerable() {
    // Hunting for it in a global stream is why nobody ever does.
    let items = history();
    let said = why_this_took_so_long(&items[0]);
    assert!(said.contains("tried the export, which timed out"));
    assert!(said.contains("chose the CSV route"));
}

#[test]
fn an_item_with_no_working_out_says_that_rather_than_inventing_some() {
    let items = history();
    assert!(why_this_took_so_long(&items[2]).contains("no working out"));
}

#[test]
fn the_day_page_shows_the_thinking_grouped_by_item() {
    let d = day_of(&history(), 5 * DAY);
    let html = looking_back_page(&d, "Five days ago");
    assert!(html.contains("What I was thinking"));
    assert!(html.contains("thought-item"), "grouped by item, not a flat stream");
    assert!(html.contains("timed out"));
}

#[test]
fn looking_back_is_reachable() {
    assert_eq!(route("/hub/back"), Some(Page::LookingBack));
}

// ================= clients =================

#[test]
fn the_same_work_for_two_clients_is_two_different_pieces_of_work() {
    // Mixing them is how you bill the wrong person.
    let items = history();
    let mut view = shipped().into_iter().find(|v| v.name == "Everything live").unwrap();
    view.only_client = Some("Marta".into());
    let showing = apply(&items, &view, 0);
    assert_eq!(showing.len(), 1);
    assert_eq!(showing[0].client.as_deref(), Some("Marta"));
}

#[test]
fn a_past_view_shows_the_state_as_it_was_not_todays_state_filtered_by_date() {
    // Different questions, and only the first is useful.
    let items = history();
    let mut view = shipped().into_iter().find(|v| v.name == "Everything live").unwrap();
    view.as_of = Some(3 * DAY);
    let showing = apply(&items, &view, 3 * DAY);
    assert!(!showing.iter().any(|i| i.title == "cut the fee video"), "it didn't exist yet");
}

// ================= layouts you arrange yourself =================

#[test]
fn the_order_is_the_layout_and_nothing_is_pinned_to_pixels() {
    // You have three screens; a layout pinned to coordinates breaks on the
    // second one.
    //
    // This asserted `stores_pixel_positions`, a `#[serde(skip)]` bool pinned
    // false that nothing read. Deleted 19 Sep 2026: the guarantee is the
    // type. `Placed` is a block, a `Size` of Small/Half/Full, and what it is
    // about — there is nowhere for a coordinate to go, and the order of
    // `Layout::blocks` is the whole layout.
    let src = std::fs::read_to_string("src/layout_prefs.rs").expect("src/layout_prefs.rs");
    let placed = src.split("pub struct Placed").nth(1).expect("Placed is gone");
    let placed = &placed[..placed.find("\n}").expect("unterminated Placed")];
    let fields: Vec<String> = placed
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("pub "))
        .map(|f| f.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect())
        .collect();
    assert_eq!(
        fields,
        vec!["block", "size", "about"],
        "`Placed` gained a field. If it can hold a coordinate, the layout stops surviving a \
         second screen."
    );

    // And an old config naming the removed key still loads.
    let parsed: LayoutConfig =
        serde_yaml::from_str("current: Home\nstores_pixel_positions: true\n").unwrap();
    assert_eq!(parsed.current, "Home", "an unknown key must not stop the section parsing");
}

#[test]
fn it_ships_short_because_nine_panels_means_nothing_stands_out() {
    assert!(Layout::default_layout().blocks.len() <= 5);
    assert_eq!(Layout::default_layout().blocks[0].block, Block::Now);
}

#[test]
fn you_can_get_back_to_how_it_shipped() {
    let mut l = Layout::default_layout();
    l.add(Block::Ledger, Size::Small, None);
    l.reset();
    assert_eq!(l.blocks.len(), Layout::default_layout().blocks.len());
}

#[test]
fn the_dragging_needs_no_framework() {
    // The hub has to work when everything else is broken, and a build step is
    // a thing that can be broken.
    assert!(DRAG_SCRIPT.contains("dragstart"));
    assert!(DRAG_SCRIPT.contains("/hub/layout/move"));
    for framework in ["React", "import ", "require(", "vue"] {
        assert!(!DRAG_SCRIPT.contains(framework), "pulls in {framework}");
    }
}

// ================= messages =================

fn msg(from: &str, group: Option<&str>, text: &str, mentions: bool) -> Message {
    Message {
        id: "1".into(),
        platform: Platform::Telegram,
        from: from.into(),
        group: group.map(str::to_string),
        text: text.into(),
        at: 0,
        mentions_you: mentions,
    }
}

#[test]
fn a_message_from_a_brand_is_told_apart_from_group_chat() {
    // The thing you actually wanted.
    let brand = msg("Nadia", None, "We'd love to work with you on a paid partnership", false);
    assert_eq!(sort(&brand, &[]), Sort::Business);

    let chat = msg("Dave", Some("Sunday football"), "anyone in for 6pm", false);
    assert_eq!(sort(&chat, &[]), Sort::Chatter);
}

#[test]
fn group_chatter_never_interrupts_and_that_is_not_configurable() {
    // It's the whole reason connecting these is bearable.
    assert!(!interrupts(Sort::Chatter));
    assert!(interrupts(Sort::Business));
    let parsed: MessagingConfig =
        serde_yaml::from_str("enabled: true\nchatter_interrupts: true\n").unwrap();
    assert!(!parsed.chatter_interrupts);
}

#[test]
fn a_group_message_that_names_you_is_for_you() {
    let named = msg("Dave", Some("Sunday football"), "Eric can you bring the ball?", false);
    assert_eq!(sort(&named, &["Eric".into()]), Sort::NeedsReply);
}

#[test]
fn a_direct_message_asking_something_needs_a_reply() {
    let dm = msg("Marta", None, "can you send the entity details?", false);
    assert_eq!(sort(&dm, &[]), Sort::NeedsReply);
}

#[test]
fn telegram_and_groupme_are_the_ones_worth_setting_up() {
    assert_eq!(Platform::Telegram.reach(), Reach::Full);
    assert_eq!(Platform::GroupMe.reach(), Reach::Full);
    assert!(Platform::Telegram.worth_setting_up());
    assert!(Platform::Telegram.to_connect().contains("two minutes"));
}

#[test]
fn whatsapp_is_refused_with_the_actual_reason_rather_than_a_shrug() {
    assert_eq!(Platform::WhatsApp.reach(), Reach::NotReally);
    assert!(!Platform::WhatsApp.worth_setting_up());
    assert!(WHY_NOT_WHATSAPP.contains("costs per message"));
    assert!(WHY_NOT_WHATSAPP.contains("gets accounts banned"));
    assert!(WHY_NOT_WHATSAPP.contains("Telegram does everything you'd want"));
}

#[test]
fn what_atlas_says_leads_with_the_one_that_is_work() {
    let said = msg_spoken(
        &[
            msg("Nadia", None, "sponsored campaign brief attached", false),
            msg("Dave", Some("football"), "6pm?", false),
        ],
        &[],
    );
    assert!(said.starts_with("Something from Nadia looks like work."));
    assert!(said.contains("1 group messages I've left alone"));
}

#[test]
fn nothing_but_chatter_says_exactly_that() {
    let said = msg_spoken(&[msg("Dave", Some("football"), "6pm?", false)], &[]);
    assert!(said.contains("all group chat"));
}

#[test]
fn atlas_drafts_replies_rather_than_sending_them() {
    assert!(MessagingConfig::default().draft_only);
}
