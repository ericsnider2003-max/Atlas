//! The hub, answered by the Atlas that is running.
//!
//! Before this, one serve loop existed and it had no daemon behind it, so ten
//! of the twelve pages answered "needs the full Atlas running" — in a build
//! where there was no other mode. This is the join, and these tests exist
//! because the failure it fixes was invisible: every page rendered, every page
//! was routed, every page was tested, and none of them ever had anything in
//! them.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::hub::Page;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::Action;
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-hublive-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
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
    Daemon::new(
        c,
        p,
        None,
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    )
}

const ALL: &[Page] = &[
    Page::Dashboard,
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
    Page::Status,
    Page::Updates,
    Page::Feedback,
    Page::Phone,
];

#[test]
fn every_page_is_answered_by_the_running_atlas() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "every");
    for page in ALL {
        let reply = atlas::hublive::reply(&mut d, Action::Hub(*page));
        let html = &reply.body;
        assert!(
            !html.contains("needs the full Atlas running"),
            "{} still answers with the settings-only excuse",
            page.label()
        );
        assert!(
            !html.contains("settings-only"),
            "{} still answers with the settings-only excuse",
            page.label()
        );
        assert!(html.contains("<main id=main"), "{} rendered nothing", page.label());
    }
}

#[test]
fn no_page_prints_a_variable_name_at_you() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "notcode");
    for page in ALL {
        let html = atlas::hublive::reply(&mut d, Action::Hub(*page)).body;
        for bad in ["LookingBack", "Kind::", "Some(", "Err("] {
            assert!(
                !html.contains(bad),
                "{} shows {bad} on screen",
                page.label()
            );
        }
    }
}

#[test]
fn the_navigation_is_on_every_page_so_you_are_never_stranded() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "nav");
    for page in ALL {
        let html = atlas::hublive::reply(&mut d, Action::Hub(*page)).body;
        assert!(
            html.contains("nav class=side"),
            "{} has no way out of it",
            page.label()
        );
        assert!(
            html.contains(Page::Settings.href()),
            "settings must be one click from {}, not buried",
            page.label()
        );
    }
}

#[test]
fn every_card_on_a_fresh_machine_says_something_rather_than_nothing() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "cards");
    let cards = d.dashboard_cards(1_000);
    assert_eq!(
        cards.len(),
        atlas::dash::Card::all().len(),
        "a card with no body renders an empty frame, which reads as broken"
    );
    for (card, body) in &cards {
        assert!(
            !body.trim().is_empty(),
            "{} is blank — a blank card and a card whose data failed to load \
             look identical",
            card.title()
        );
        assert!(
            body.contains('<'),
            "{} is not rendered markup",
            card.title()
        );
    }
}

#[test]
fn rearranging_through_the_daemon_persists() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "persist");
    let first = d.dashboard.cards[0].card;

    atlas::hublive::reply(
        &mut d,
        Action::DashMove(atlas::dash::Move::Down(first)),
    );

    assert_ne!(
        d.dashboard.cards[0].card, first,
        "the move did not take effect on the live daemon"
    );
}

#[test]
fn a_move_that_changes_nothing_still_returns_you_to_the_dashboard() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "noop");
    let top = d.dashboard.cards[0].card;
    let reply = atlas::hublive::reply(&mut d, Action::DashMove(atlas::dash::Move::Up(top)));
    assert_eq!(reply.status, 303, "a dead end after a button press is a bug");
}

#[test]
fn arranging_is_a_mode_the_daemon_remembers_between_requests() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "mode");
    assert!(!d.arranging, "reading is the default");

    atlas::hublive::reply(&mut d, Action::DashArrange(true));
    assert!(d.arranging);

    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Dashboard)).body;
    assert!(html.contains("value='up'"), "the move controls appear");

    atlas::hublive::reply(&mut d, Action::DashArrange(false));
    assert!(!d.arranging);
}

#[test]
fn the_header_reports_what_is_waiting_from_every_page() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "waiting");
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Dashboard)).body;
    assert!(
        html.contains("waiting") || html.contains("Nothing waiting"),
        "anything that needs you and is only findable by going looking for it \
         will be found late"
    );
}

#[test]
fn an_api_action_that_is_not_a_page_does_not_render_a_broken_shell() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "notapage");
    // 2 Oct 2026: /health is the plain API another Atlas reads, and answers
    // "ok" in plain text rather than the "isn't a page" shell it fell into.
    let reply = atlas::hublive::reply(&mut d, Action::Health);
    assert_eq!(reply.body, "ok");
    // Not "no mention of the word Health" — that is now the name of a page and
    // sits in the menu of every screen. What must never appear is the action
    // printed at you, which is what `{:?}` on the enum would produce.
    assert!(
        !reply.body.contains("Action::"),
        "the request was debug-formatted onto the page"
    );
    assert!(
        !reply.body.contains("Health}") && !reply.body.contains("(Health)"),
        "the variant name reached the screen"
    );
}

#[test]
fn the_daemon_holds_a_hub_server_slot_so_the_loop_can_serve_it() {
    let c = cfg();
    let p = plat();
    let d = daemon(&c, &p, "slot");
    assert!(
        d.hub_server.is_none(),
        "not opened until something opens it, so tests and the quick prompt \
         never bind a port they did not ask for"
    );
}


#[test]
fn the_accounts_page_shows_what_you_have_told_it_about() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "accounts");

    atlas::hublive::reply(
        &mut d,
        Action::Account(atlas::accounts::Change::Note("gmail".into())),
    );

    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Accounts)).body;
    assert!(html.contains("gmail"), "the account is on the page");
    assert!(
        html.contains("two-factor"),
        "and so is the thing worth doing about it — an inventory with no \
         advice is the version that listed nothing"
    );
    assert!(!html.contains("Keystone"), "no variable names on screen");
}

#[test]
fn an_account_change_persists_on_the_running_daemon() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "acctpersist");
    atlas::hublive::reply(
        &mut d,
        Action::Account(atlas::accounts::Change::Note("github".into())),
    );
    assert!(d.accounts.get("github").is_some());

    let reply = atlas::hublive::reply(
        &mut d,
        Action::Account(atlas::accounts::Change::Forget("github".into())),
    );
    assert_eq!(reply.status, 303, "a dead end after a button press is a bug");
    assert!(d.accounts.get("github").is_none());
}

#[test]
fn the_accounts_page_never_reads_a_secret_to_render_itself() {
    // Reading a value would touch its last-used date and defeat `stale()`,
    // quite apart from putting the secrets themselves through a page.
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "novaultread");
    let before = d.vault.state();
    let _ = atlas::hublive::reply(&mut d, Action::Hub(Page::Accounts));
    assert_eq!(d.vault.state(), before, "rendering must not open the vault");
}


#[test]
fn the_palette_is_on_every_page() {
    // A palette missing from one page is worse than no palette: you learn to
    // reach for it and then it is not there.
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "palette");
    for page in ALL {
        let html = atlas::hublive::reply(&mut d, Action::Hub(*page)).body;
        assert!(
            html.contains("id=palette"),
            "no palette on {}",
            page.label()
        );
    }
}

#[test]
fn searching_reaches_the_thing_rather_than_the_page_that_holds_it() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "find");
    let html = atlas::hublive::reply(&mut d, Action::Find("arrange".into())).body;
    assert!(html.contains("action='/hub/dash'"));
    assert!(html.contains("Arrange the dashboard"));
}

#[test]
fn what_you_searched_for_is_remembered_for_next_time() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "findmem");
    assert!(d.palette.picked.is_empty());
    atlas::hublive::reply(&mut d, Action::Find("settings".into()));
    assert_eq!(d.palette.picked.first().map(|s| s.as_str()), Some("Open settings"));
}

#[test]
fn opening_the_palette_without_typing_teaches_it_nothing() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "findempty");
    atlas::hublive::reply(&mut d, Action::Find(String::new()));
    assert!(
        d.palette.picked.is_empty(),
        "opening a palette is not a choice, and recording it as one would \
         make the memory a record of how often you pressed a key"
    );
}


#[test]
fn the_trust_card_says_what_would_change_each_verdict() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "trust");
    let cards = d.dashboard_cards(1_000);
    let (_, body) = cards
        .iter()
        .find(|(c, _)| *c == atlas::dash::Card::Trust)
        .expect("the trust card exists");
    for kind in atlas::earned::Kind::all() {
        assert!(
            body.contains(kind.title()),
            "{} is missing, so it is invisible rather than untrusted",
            kind.title()
        );
    }
    assert!(
        body.contains("coincidence") || body.contains("yours to decide"),
        "a verdict with no reason attached is what makes a system feel \
         arbitrary: {body}"
    );
    assert!(!body.contains('_'), "no variable names on a card");
}

#[test]
fn a_business_appears_as_its_own_record_not_as_more_of_yours() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "trustbiz");
    d.earned.note_in(
        &atlas::earned::Space::Business("Acme".into()),
        atlas::earned::Kind::Drafting,
        true,
        "a draft",
        1_000,
    );
    let cards = d.dashboard_cards(1_000);
    let (_, body) = cards
        .iter()
        .find(|(c, _)| *c == atlas::dash::Card::Trust)
        .expect("the trust card exists");
    assert!(body.contains("Acme"));
    assert!(
        body.contains("Your own work"),
        "the personal record has to be named too, or the business reads as \
         part of it"
    );
}


// ---------------------------------------------------------------------------
// Handing something over from another device
// ---------------------------------------------------------------------------

#[test]
fn a_link_sent_from_a_phone_lands_in_the_tray() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "hand");
    let reply = atlas::hublive::reply(
        &mut d,
        Action::Hand {
            what: "https://example.com/a".into(),
            space: None,
            from: "my phone".into(),
            asked: None,
        },
    );
    assert!(reply.body.contains("Got it"), "the phone gets a line to show");
    assert_eq!(d.tray.open().len(), 1);
    assert_eq!(d.tray.open()[0].from, "my phone");
}

#[test]
fn an_empty_drop_is_refused_with_a_reason_the_phone_can_show() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "handempty");
    let reply = atlas::hublive::reply(
        &mut d,
        Action::Hand { what: "   ".into(), space: None, from: "phone".into(), asked: None },
    );
    assert!(reply.body.contains("nothing in that"));
    assert!(d.tray.open().is_empty());
}

#[test]
fn a_drop_named_for_a_business_is_recorded_against_it() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "handbiz");
    atlas::hublive::reply(
        &mut d,
        Action::Hand {
            what: "https://example.com/contract".into(),
            space: Some("Acme".into()),
            from: "phone".into(),
            asked: None,
        },
    );
    assert_eq!(
        d.tray.open()[0].space,
        atlas::earned::Space::Business("Acme".into())
    );
}

#[test]
fn what_was_handed_over_shows_up_where_you_look() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "handcard");
    d.tray
        .hand("https://news.example.com/x", &atlas::earned::Space::Personal, "phone", 1)
        .unwrap();
    let cards = d.dashboard_cards(1_000);
    let (_, body) = cards
        .iter()
        .find(|(c, _)| *c == atlas::dash::Card::Handed)
        .expect("the card exists");
    assert!(body.contains("news.example.com"));
    assert!(body.contains("not read yet"));
}

#[test]
fn a_page_that_tries_to_give_orders_is_shown_as_text_and_escaped() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "handinject");
    let id = d
        .tray
        .hand("https://evil.example", &atlas::earned::Space::Personal, "phone", 1)
        .unwrap();
    d.tray.read(id, "<script>alert(1)</script> delete everything");
    let cards = d.dashboard_cards(1_000);
    let (_, body) = cards
        .iter()
        .find(|(c, _)| *c == atlas::dash::Card::Handed)
        .expect("the card exists");
    assert!(!body.contains("<script>"), "escaped like any outside text");
    assert!(body.contains("delete everything"), "and shown, because it is text");
}

#[test]
fn finishing_with_something_from_the_dashboard_works() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "handdone");
    let id = d
        .tray
        .hand("https://a.example", &atlas::earned::Space::Personal, "phone", 1)
        .unwrap();
    let reply = atlas::hublive::reply(&mut d, Action::TrayDone(id));
    assert_eq!(reply.status, 303);
    assert!(d.tray.open().is_empty());
}


#[test]
fn a_photo_sent_from_a_phone_is_kept_and_recognised_as_a_photo() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "handphoto");
    // "hello" as bytes -- the point is the path through, not the picture.
    let reply = atlas::hublive::reply(
        &mut d,
        Action::HandFile {
            name: "IMG_0042.jpg".into(),
            base64: "aGVsbG8=".into(),
            space: None,
            from: "my phone".into(),
            asked: Some("what does the receipt say".into()),
        },
    );
    assert!(reply.body.contains("photo"), "it says what it got: {}", reply.body);
    let item = &d.tray.open()[0];
    assert_eq!(item.sort, atlas::tray::Sort::Image);
    assert_eq!(item.asked.as_deref(), Some("what does the receipt say"));
    let kept = item.stored_at.clone().expect("the bytes were kept");
    assert!(std::path::Path::new(&kept).exists(), "the file is really there");
}

#[test]
fn the_same_photo_twice_is_one_photo_even_under_a_different_name() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "handdupe");
    for name in ["IMG_0042.jpg", "photo.jpg"] {
        atlas::hublive::reply(
            &mut d,
            Action::HandFile {
                name: name.into(),
                base64: "aGVsbG8=".into(),
                space: None,
                from: "phone".into(),
                asked: None,
            },
        );
    }
    assert_eq!(d.tray.open().len(), 1, "same bytes, one thing");
}

#[test]
fn two_different_photos_sharing_a_name_stay_separate() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "handsamename");
    for data in ["aGVsbG8=", "d29ybGQ="] {
        atlas::hublive::reply(
            &mut d,
            Action::HandFile {
                name: "IMG_0042.jpg".into(),
                base64: data.into(),
                space: None,
                from: "phone".into(),
                asked: None,
            },
        );
    }
    assert_eq!(
        d.tray.open().len(),
        2,
        "phones name things IMG_0042 with enthusiasm"
    );
}

#[test]
fn a_broken_upload_is_refused_rather_than_half_decoded() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "handbroken");
    let reply = atlas::hublive::reply(
        &mut d,
        Action::HandFile {
            name: "x.jpg".into(),
            base64: "not base64 !!!".into(),
            space: None,
            from: "phone".into(),
            asked: None,
        },
    );
    assert!(reply.body.contains("intact"), "{}", reply.body);
    assert!(
        d.tray.open().is_empty(),
        "a half-decoded photo is a corrupt file that looks like a real one"
    );
}

#[test]
fn what_you_asked_for_is_repeated_back_when_you_send_it() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "handasked");
    let reply = atlas::hublive::reply(
        &mut d,
        Action::Hand {
            what: "https://example.com/terms".into(),
            space: None,
            from: "phone".into(),
            asked: Some("did the payment terms change".into()),
        },
    );
    assert!(
        reply.body.contains("payment terms"),
        "so you can see it landed: {}",
        reply.body
    );
    // And it is actually kept against the item, not only echoed back — an
    // acknowledgement that agrees with nothing stored is the shape of a
    // feature that looks finished and does nothing.
    assert_eq!(
        d.tray.open()[0].asked.as_deref(),
        Some("did the payment terms change")
    );
}

// ============ the command deck (Eric's design, 23 Sep 2026) ============

/// Noon on a fixed day, as UTC seconds, and the clock pinned to UTC+12 so the
/// wall clock reads 18:00 — evening.
const NOON_UTC: u64 = 1_790_000_000 - (1_790_000_000 % 86_400) + 12 * 3600;
const OFF: i64 = 6 * 3600;

#[test]
fn the_deck_greets_you_by_the_time_on_your_wall_clock() {
    // The shipped config calls nobody by name (27 Sep 2026: Atlas goes to
    // other people too); a name set in the settings is used.
    let shipped = cfg();
    let p = plat();
    let plain = daemon(&shipped, &p, "deck-greet-plain").deck(NOON_UTC, OFF);
    assert_eq!(plain.greeting.trim_end_matches('.'), "Good evening", "{}", plain.greeting);
    let mut c = cfg();
    c.tools.as_mut().unwrap().persona.address = "Eric".into();
    let d = daemon(&c, &p, "deck-greet");
    let deck = d.deck(NOON_UTC, OFF); // 18:00 on the wall
    assert!(deck.greeting.starts_with("Good evening"), "{}", deck.greeting);
    assert!(deck.greeting.contains("Eric"), "{}", deck.greeting);
    let morning = d.deck(NOON_UTC, -4 * 3600); // 08:00 on the wall
    assert!(morning.greeting.starts_with("Good morning"), "{}", morning.greeting);
    // The small hours get a plain hello, the same rule the nudges follow.
    let late = d.deck(NOON_UTC, -10 * 3600); // 02:00
    assert!(late.greeting.starts_with("Hello"), "{}", late.greeting);
}

/// The phone widget's glance (`/hub/glance.json`): the next thing today and
/// the waiting count, with the subject kept off the lock screen unless you
/// turn it on.
#[test]
fn a_phone_widget_gets_the_next_thing_and_the_lock_screen_gets_only_its_time() {
    let p = plat();
    let now = atlas::store::now();
    let glance = |c: &Config, tag: &str| {
        let mut d = daemon(c, &p, tag);
        d.calendar.add(
            "Dentist on Elm Street",
            atlas::calendar::When { start: now + 90, end: now + 1800, all_day: false },
            None,
            now,
        );
        let r = atlas::hublive::reply(&mut d, Action::GlanceJson);
        serde_json::from_str::<serde_json::Value>(&r.body).unwrap()
    };
    let g = glance(&cfg(), "glance-private");
    assert_eq!(g["home"]["next"]["what"], "Dentist on Elm Street", "{g}");
    let at = g["home"]["next"]["at"].as_str().unwrap().to_string();
    assert_eq!(at.len(), 5, "a wall-clock time: {g}");
    assert_eq!(g["lock"]["next"]["at"], at.as_str());
    assert_eq!(g["lock"]["next"]["what"], "", "the lock screen named it: {g}");
    assert_eq!(g["home"]["waiting"], g["lock"]["waiting"]);
    assert_eq!(g["capture"], "atlas://hub/give");
    assert!(g["as_of"].as_u64().unwrap() >= now);
    let mut c = cfg();
    c.tools.as_mut().unwrap().phone.widget_titles_on_lock_screen = true;
    let g = glance(&c, "glance-titles");
    assert_eq!(g["lock"]["next"]["what"], "Dentist on Elm Street", "{g}");
}

#[test]
fn today_runs_down_the_spine_in_order_with_now_between_done_and_to_come() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "deck-spine");
    let wall_midnight = (NOON_UTC as i64 + OFF).div_euclid(86_400) * 86_400;
    // Real moments that read 09:00 and 20:30 on the wall.
    let at = |h: i64, m: i64| (wall_midnight + h * 3600 + m * 60 - OFF) as u64;
    d.calendar.add("Dentist", atlas::calendar::When { start: at(9, 0), end: at(9, 45), all_day: false }, None, NOON_UTC);
    d.calendar.add("Call with the broker", atlas::calendar::When { start: at(20, 30), end: at(21, 0), all_day: false }, None, NOON_UTC);
    // Atlas's own work: 13:40 on the wall.
    d.journal.record_at(atlas::activity::Kind::Scheduled, "Backed up the vault", true, at(13, 40));
    // Upkeep never reaches the spine.
    d.journal.record_at(atlas::activity::Kind::Upkeep, "rotated a log", true, at(14, 0));

    let deck = d.deck(NOON_UTC, OFF);
    let said: Vec<(&str, &str, atlas::hub::Mark)> =
        deck.spine.iter().map(|(t, w, m)| (t.as_str(), w.as_str(), *m)).collect();
    assert_eq!(said[0], ("09:00", "Dentist", atlas::hub::Mark::Done), "{said:?}");
    assert_eq!(said[1], ("13:40", "Backed up the vault", atlas::hub::Mark::Done), "{said:?}");
    assert_eq!(said[2].2, atlas::hub::Mark::Now, "{said:?}");
    assert_eq!(said[3], ("20:30", "Call with the broker", atlas::hub::Mark::Later), "{said:?}");
    assert_eq!(said.len(), 4, "upkeep leaked onto the spine: {said:?}");
    // With nothing underway, the next thing is named under Right now.
    assert_eq!(deck.now, "Nothing underway.");
    assert_eq!(deck.now_sub, "Next: Call with the broker at 20:30.");
}

#[test]
fn waiting_on_you_counts_the_same_things_the_header_does() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "deck-waiting");
    let now = NOON_UTC;
    let cards = d.dashboard_cards(now);
    let waiting = cards.iter().find(|(c, _)| *c == atlas::dash::Card::Outstanding).unwrap().1.clone();
    let header = d.waiting_count(now);
    if header == 0 {
        assert!(waiting.contains("Nothing waiting on you"), "{waiting}");
    } else {
        assert!(waiting.contains(&format!("<b>{header}</b><span>To act</span>")), "header says {header}: {waiting}");
    }
}

#[test]
fn every_card_the_deck_can_show_has_something_in_it_including_projects() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "deck-projects");
    let cards = d.dashboard_cards(NOON_UTC);
    assert_eq!(cards.len(), atlas::dash::Card::all().len(), "every card, Projects included, has a body");
    let projects = cards.iter().find(|(c, _)| *c == atlas::dash::Card::Projects).expect("the Projects card has a body");
    assert!(projects.1.contains("No projects yet"), "{}", projects.1);
    let html = String::from_utf8(atlas::server::render(&atlas::hublive::reply(&mut d, Action::Hub(Page::Dashboard))).into_bytes()).unwrap();
    // Home as the locked design draws it (26 Sep): the sidebar, the Brief
    // (which carries what's waiting on you), Today, Right now, and the cards.
    // A fresh Atlas knows nothing yet, so its brief is the first-run welcome.
    for part in ["class=sidebar", "<section class=brief aria-label='Welcome'>", "<h2>Today</h2>", "Right now", "Projects", "Health", "What I did without being asked"] {
        assert!(html.contains(part), "Home is missing {part}");
    }
}

// ---------------------------------------------------------------------------
// A change that couldn't be kept is not said as done (28 Sep 2026).
//
// The client list and the shared tasks are read from disk for each click and
// written back; `let _ = list.save(..)` threw the answer away and the page
// said "Added" whether or not anything was written. A failed save is that
// change gone at once. A folder standing where the save writes its temporary
// file makes the save fail the way a full disk or a locked file does.

fn saves_of(dir: &Path, record: &str) {
    std::fs::create_dir_all(dir.join(format!("{record}.{}.json.tmp", std::process::id()))).unwrap();
}

fn said_in(reply: &atlas::server::Reply) -> String {
    // A redirect carries the sentence in its address, encoded.
    reply.body.replace("%20", " ").replace('+', " ")
}

#[test]
fn a_client_that_could_not_be_saved_is_not_said_as_added() {
    let c = cfg();
    let p = plat();
    let dir = tmp("client-unsaved");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
    saves_of(&dir, "clients");
    let reply = atlas::hublive::reply(
        &mut d,
        Action::HubPost {
            path: "/hub/clients".into(),
            fields: vec![("address".into(), "sam@example.com".into()), ("name".into(), "Sam".into())],
        },
    );
    let said = said_in(&reply);
    assert!(!said.contains("Added"), "the page said it was added and nothing was saved: {said}");
    assert!(said.contains("stick"), "the failure wasn't said: {said}");
    assert!(!atlas::clients::ClientList::load(&Store::new(&dir)).is_client("sam@example.com"));
}

#[test]
fn a_client_that_was_saved_is_still_said_as_added() {
    // The control: the ordinary case is unchanged.
    let c = cfg();
    let p = plat();
    let dir = tmp("client-saved");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
    let reply = atlas::hublive::reply(
        &mut d,
        Action::HubPost {
            path: "/hub/clients".into(),
            fields: vec![("address".into(), "sam@example.com".into()), ("name".into(), "Sam".into())],
        },
    );
    assert!(said_in(&reply).contains("Added Sam"), "{}", said_in(&reply));
    assert!(atlas::clients::ClientList::load(&Store::new(&dir)).is_client("sam@example.com"));
}

#[test]
fn a_file_from_the_phone_that_could_not_be_kept_says_so() {
    let c = cfg();
    let p = plat();
    let dir = tmp("hand-unsaved");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
    saves_of(&dir, "tray");
    let reply = atlas::hublive::reply(
        &mut d,
        Action::Hand { what: "https://example.com/an-article".into(), space: None, from: "phone".into(), asked: None },
    );
    assert!(reply.body.contains("couldn't save"), "a share that wasn't kept was answered as kept: {}", reply.body);
}
