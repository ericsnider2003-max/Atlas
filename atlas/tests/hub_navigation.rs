//! The navigation and the routing table cannot drift apart.
//!
//! `Page::Connections` was routable, rendered, and linked from nowhere. You
//! could only reach it by typing the URL, which means in practice nobody ever
//! did. A page nothing links to is a page that does not exist, and no test
//! noticed because every part of it worked in isolation.
//!
//! The navigation is now data (`hub::NAV`) rather than a hardcoded row of
//! anchors, specifically so this file can check the join.

use atlas::hub::{self, Page};

/// Every page the hub will serve.
///
/// Written out by hand rather than derived, so adding a `Page` variant makes
/// this list fail to compile until someone decides where it belongs.
const ALL: &[Page] = &[
    Page::Dashboard,
    Page::Status,
    Page::Now,
    Page::Gestures,
    Page::Access,
    Page::Connections,
    Page::Workspace,
    Page::LookingBack,
    Page::Recommendations,
    Page::Settings,
    Page::Permissions,
    Page::Accounts,
    Page::Activity,
    Page::Outstanding,
];

#[test]
fn every_page_is_linked_from_the_navigation() {
    let linked: Vec<Page> = hub::NAV
        .iter()
        .flat_map(|(_, pages)| pages.iter().copied())
        .collect();

    let orphans: Vec<&str> = ALL
        .iter()
        .filter(|p| !linked.contains(p))
        .map(|p| p.label())
        .collect();

    assert!(
        orphans.is_empty(),
        "these pages are served but nothing links to them, so only someone \
         typing the URL will ever see them: {orphans:?}"
    );
}

#[test]
fn no_page_is_listed_in_two_places() {
    let linked: Vec<Page> = hub::NAV
        .iter()
        .flat_map(|(_, pages)| pages.iter().copied())
        .collect();
    for p in ALL {
        let n = linked.iter().filter(|q| *q == p).count();
        assert!(
            n <= 1,
            "{} appears in {n} groups; two homes means the breadcrumb has to \
             pick one and will sometimes pick the wrong one",
            p.label()
        );
    }
}

#[test]
fn every_navigation_link_actually_routes() {
    for (group, pages) in hub::NAV {
        for p in *pages {
            assert_eq!(
                hub::route(p.href()),
                Some(*p),
                "{} in group {group} links to {} and nothing serves it",
                p.label(),
                p.href()
            );
        }
    }
}

#[test]
fn no_group_is_long_enough_to_be_a_list_you_reread() {
    // The whole reason for grouping. A flat run of eleven was the bug; a
    // group of eleven would be the same bug indented.
    for (group, pages) in hub::NAV {
        assert!(
            pages.len() <= 6,
            "group {group} has {} items — past about six, a menu stops being \
             navigation and becomes a list you read every time",
            pages.len()
        );
    }
}

#[test]
fn the_page_you_are_on_is_marked() {
    let html = hub::shell_at(Some(Page::Settings), "Settings", "<p>x</p>");
    assert!(
        html.contains(&format!("class='nav on' href='{}'", Page::Settings.href())),
        "the current page must be marked in the navigation"
    );
    assert!(
        !html.contains(&format!("class='here' href='{}'", Page::Accounts.href()))
            && !html.contains(&format!("class='nav on' href='{}'", Page::Accounts.href())),
        "only the current page is marked"
    );
    // A page under More is marked there, and More opens itself to show it.
    let deep = hub::shell_at(Some(Page::Accounts), "Accounts", "<p>x</p>");
    assert!(deep.contains(&format!("class='here' href='{}'", Page::Accounts.href())));
    assert!(deep.contains("<details class=more open>"));
}

#[test]
fn a_deep_page_says_where_it_sits() {
    let html = hub::shell_at(Some(Page::Accounts), "Stored logins", "<p>x</p>");
    let group = Page::Accounts.group().expect("Accounts has a group");
    assert!(html.contains(group), "the breadcrumb names the parent group");
    // The way back up is the sidebar's Home, on every page: the design's
    // breadcrumb names where you are ("What it may do / Accounts") and its
    // groups are headings, not pages, so they are not links.
    assert!(html.contains(&format!("href='{}'>", Page::Dashboard.href())), "Home is one press away");
}

#[test]
fn the_breadcrumb_does_not_link_to_the_page_you_are_already_on() {
    let trail = hub::crumbs(Page::Accounts);
    assert!(
        trail.contains(&format!("<b>{}</b>", Page::Accounts.label())),
        "the page you are on is plain text, not a link to itself"
    );
    assert!(
        !trail.contains(&format!("href='{}'", Page::Accounts.href())),
        "a breadcrumb whose last item links to itself is a small lie about \
         where you are"
    );
}

#[test]
fn the_front_page_does_not_get_a_trail_back_to_itself() {
    // The design's top bar reads "Personal / Home": where you are, with
    // nothing in it linking back to the page you're on.
    let trail = hub::crumbs(Page::Dashboard);
    assert_eq!(trail, "<p class=crumbs>Personal<span>/</span><b>Home</b></p>");
    assert!(!trail.contains("href"), "nothing links to itself: {trail}");
}

#[test]
fn a_page_served_outside_the_routing_table_claims_no_place_in_the_tree() {
    // Errors and spoken replies are rendered through the same shell. They are
    // not pages, and pretending they sit somewhere would put a false trail on
    // screen.
    let html = hub::shell("Atlas", "<p>something went wrong</p>");
    assert!(!html.contains("class=crumbs"), "no breadcrumb without a page");
    assert!(
        html.contains("nav class=side"),
        "the navigation is still there — being lost is not a reason to strip \
         the way out"
    );
}

#[test]
fn an_index_of_nothing_says_so_rather_than_rendering_an_empty_box() {
    let html = hub::index_rows(&[]);
    assert!(
        html.contains("Nothing here yet"),
        "an empty frame reads as a broken page rather than an empty one"
    );
    assert!(!html.contains("class=entry"));
}

#[test]
fn an_index_row_opens_something() {
    let rows = vec![(
        "/hub/workspace".to_string(),
        "Homelab".to_string(),
        "3 accounts".to_string(),
    )];
    let html = hub::index_rows(&rows);
    assert!(html.contains("href='/hub/workspace'"));
    assert!(html.contains("Homelab"));
    assert!(html.contains("3 accounts"));
}

#[test]
fn a_name_from_outside_cannot_smuggle_markup_into_the_index() {
    let rows = vec![(
        "/hub/x".to_string(),
        "<script>alert(1)</script>".to_string(),
        "ok".to_string(),
    )];
    let html = hub::index_rows(&rows);
    assert!(!html.contains("<script>"), "escaped: {html}");
}

// ---------------------------------------------------------------------------
// The dashboard is what you open on purpose.
// ---------------------------------------------------------------------------

/// Asking Atlas out loud ("what's outstanding", "what are you thinking")
/// raises Atlas's own window. The hub is the other thing: somewhere you go to
/// look. So the front door of the hub is the dashboard, not a status readout.
#[test]
fn the_hubs_front_door_is_the_dashboard() {
    assert_eq!(hub::route("/hub"), Some(Page::Dashboard));
    assert_eq!(Page::Dashboard.href(), "/hub");
}

#[test]
fn the_dashboard_is_home_under_personal_as_the_design_files_it() {
    // It used to be its own group of one ("the front door is not an item
    // inside a category"). The design Eric locked on 20-21 Sep files Home
    // first under Personal, beside the business groups that appear once a
    // business exists, and its top bar reads "Personal / Home".
    assert_eq!(Page::Dashboard.group(), Some("Personal"));
    assert_eq!(hub::NAV[1].1[0], Page::Dashboard, "Home leads the Personal group");
}

#[test]
fn the_status_readout_kept_its_own_address_when_the_dashboard_took_the_root() {
    assert_eq!(hub::route("/hub/status"), Some(Page::Status));
}

// ---------------------------------------------------------------------------
// Arranging
// ---------------------------------------------------------------------------

use atlas::dash::{Card, Layout};

fn bodies() -> Vec<(Card, String)> {
    Card::all()
        .into_iter()
        .map(|c| (c, format!("<p>body of {}</p>", c.key())))
        .collect()
}

#[test]
fn reading_is_the_default_and_shows_no_move_controls() {
    let html = hub::dashboard_page(&Layout::default(), &bodies(), false);
    assert!(
        !html.contains("value='up'"),
        "a dashboard you rearrange by accident while reading it is worse than \
         one you cannot rearrange at all"
    );
    assert!(html.contains("body of projects"), "it shows the actual cards");
    assert!(html.contains(">Arrange<"), "and offers a way in");
}

#[test]
fn every_move_is_reachable_without_dragging() {
    // WCAG 2.2's dragging-movements criterion: a sortable dashboard is not one
    // of the cases where dragging counts as essential, so every drag needs a
    // single-pointer equivalent. Here the buttons are the mechanism and the
    // drag is the addition, which is the only ordering that cannot rot.
    let html = hub::dashboard_page(&Layout::default(), &bodies(), true);
    for what in ["up", "down", "widen", "hide"] {
        assert!(
            html.contains(&format!("value='{what}'")),
            "no button for {what} — that leaves dragging as the only way"
        );
    }
    assert!(
        html.contains("action=/hub/dash"),
        "the buttons post to the same place a drop posts to"
    );
}

#[test]
fn the_top_card_is_not_offered_a_move_up() {
    let html = hub::dashboard_page(&Layout::default(), &bodies(), true);
    let first = html.find("data-card=").unwrap();
    let second = html[first + 1..].find("data-card=").unwrap() + first + 1;
    let top_card = &html[first..second];
    assert!(
        !top_card.contains("value='up'"),
        "a button that cannot do anything is a button you press once and \
         distrust afterwards"
    );
}

#[test]
fn a_hidden_card_can_be_put_back() {
    let mut l = Layout::default();
    l.set_hidden(Card::Machine, true);
    let html = hub::dashboard_page(&l, &bodies(), true);
    assert!(html.contains("value=show"));
    assert!(html.contains(&format!("Show {}", Card::Machine.title())));
}

#[test]
fn a_card_with_nothing_to_show_says_so_rather_than_rendering_an_empty_box() {
    let html = hub::dashboard_page(&Layout::default(), &[], false);
    assert!(
        html.contains("Nothing to show"),
        "a blank card and a broken card look identical"
    );
}

#[test]
fn hiding_everything_explains_the_blank_page() {
    let mut l = Layout::default();
    for c in Card::all() {
        l.set_hidden(c, true);
    }
    let html = hub::dashboard_page(&l, &bodies(), true);
    assert!(html.contains("Every card is hidden"));
}

#[test]
fn putting_it_back_is_only_offered_once_there_is_something_to_put_back() {
    let plain = hub::dashboard_page(&Layout::default(), &bodies(), true);
    assert!(!plain.contains("value=reset"), "nothing has been arranged yet");

    let mut l = Layout::default();
    l.move_down(Card::Outstanding);
    let arranged = hub::dashboard_page(&l, &bodies(), true);
    assert!(arranged.contains("value=reset"));
}

#[test]
fn the_drag_script_posts_the_same_fields_the_buttons_post() {
    let html = hub::dashboard_page(&Layout::default(), &bodies(), true);
    assert!(html.contains("'/hub/dash'"), "same endpoint");
    for field in ["'what'", "'card'", "'to'"] {
        assert!(
            html.contains(field),
            "the drag path must send {field}, or it is a second mechanism \
             that can disagree with the first"
        );
    }
}

#[test]
fn nothing_is_draggable_while_you_are_only_reading() {
    let html = hub::dashboard_page(&Layout::default(), &bodies(), false);
    assert!(!html.contains("draggable=true"));
    // No *drag* script when you're only reading — the mechanism that reorders
    // cards should not ship unless you're arranging them. (The always-present
    // appearance/access menu script is a different, justified one; it drives
    // the header's Aa menu, not the cards, and posts nothing.)
    assert!(
        !html.contains("getElementById('cards')")
            && !html.contains("setPointerCapture")
            && !html.contains("pointerdown"),
        "the card-drag script must not ship while only reading"
    );
}
