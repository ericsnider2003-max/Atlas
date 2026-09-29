//! The palette.
//!
//! A menu stops scaling somewhere around six items and Atlas has thirteen
//! pages, plus a growing number of things you can *do* that were only reachable
//! by first navigating to whichever page happens to hold the button.

use atlas::palette::{catalogue, find, Does, Entry, Recent, KEEP, SHOW};
use atlas::store::Store;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-palette-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn labels(hits: &[&Entry]) -> Vec<&'static str> {
    hits.iter().map(|e| e.label).collect()
}

// ---------------------------------------------------------------------------
// Finding things
// ---------------------------------------------------------------------------

#[test]
fn typing_what_you_would_call_it_finds_it() {
    let c = catalogue();
    let r = Recent::default();
    for (typed, expect) in [
        ("settings", "Open settings"),
        ("outstanding", "Open the board"),
        ("stuck", "See what's blocked"),
        ("2fa", "Track a new account"),
        ("offline", "Check the connections"),
        ("kanban", "Open the board"),
        ("yesterday", "Open the history"),
    ] {
        let hits = find(&c, typed, &r);
        assert!(
            hits.first().is_some_and(|e| e.label == expect),
            "typing {typed:?} should reach {expect:?}, got {:?}",
            labels(&hits).first()
        );
    }
}

#[test]
fn nobody_types_the_label_so_the_words_you_reach_for_are_indexed() {
    let c = catalogue();
    let r = Recent::default();
    // None of these words appear in the label they should find.
    for typed in ["password", "memory", "revoke", "customise", "log"] {
        assert!(
            !find(&c, typed, &r).is_empty(),
            "{typed:?} found nothing — the label is not what anyone types"
        );
    }
}

#[test]
fn a_query_that_matches_nothing_returns_nothing() {
    let c = catalogue();
    let r = Recent::default();
    assert!(
        find(&c, "banana", &r).is_empty(),
        "matching any letters anywhere sounds generous and means the palette \
         can never say it hasn't got something — so you keep typing at a list \
         of wrong answers instead of learning it isn't there"
    );
    assert!(find(&c, "open banana", &r).is_empty(), "every word has to land");
}

#[test]
fn an_untyped_palette_is_already_useful() {
    let c = catalogue();
    let hits = find(&c, "", &Recent::default());
    assert_eq!(
        hits.len(),
        c.len(),
        "the moment you open it is the moment you have not yet decided what to \
         call the thing you want"
    );
    assert_eq!(hits[0].label, c[0].label, "in the order it was written");
}

#[test]
fn doing_something_outranks_going_somewhere() {
    let c = catalogue();
    let hits = find(&c, "dashboard", &Recent::default());
    let arrange = labels(&hits)
        .iter()
        .position(|l| *l == "Arrange the dashboard")
        .expect("the action is a hit");
    let go = labels(&hits)
        .iter()
        .position(|l| *l == "Go to the dashboard")
        .expect("so is the destination");
    let reset = labels(&hits)
        .iter()
        .position(|l| *l == "Put the dashboard back how it was")
        .expect("the other action is a hit too");
    assert!(reset < go, "both actions come before the destination");
    assert!(
        arrange < go,
        "a palette full of destinations is the menu with extra steps"
    );
}

#[test]
fn the_same_query_always_gives_the_same_order() {
    let c = catalogue();
    let r = Recent::default();
    let once = labels(&find(&c, "see", &r));
    for _ in 0..5 {
        assert_eq!(
            labels(&find(&c, "see", &r)),
            once,
            "results that reshuffle between identical queries cannot be learned"
        );
    }
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

#[test]
fn what_you_reached_for_lately_is_offered_first() {
    let c = catalogue();
    let mut r = Recent::default();
    r.picked("Open the history");
    let first = r.first(&c);
    assert_eq!(first.first().map(|e| e.label), Some("Open the history"));
}

#[test]
fn picking_the_same_thing_twice_moves_it_rather_than_duplicating_it() {
    let mut r = Recent::default();
    r.picked("Open settings");
    r.picked("Go to the dashboard");
    r.picked("Open settings");
    assert_eq!(r.picked, vec!["Open settings", "Go to the dashboard"]);
}

#[test]
fn it_remembers_a_handful_not_a_history() {
    let mut r = Recent::default();
    for i in 0..40 {
        r.picked(&format!("thing {i}"));
    }
    assert_eq!(r.picked.len(), KEEP);
    assert_eq!(r.picked[0], "thing 39", "most recent first");
}

#[test]
fn memory_breaks_a_tie_and_never_floats_a_poor_match_over_a_good_one() {
    let c = catalogue();
    let mut r = Recent::default();
    // Something with nothing to do with "settings".
    r.picked("Open the history");
    let hits = find(&c, "settings", &r);
    assert_eq!(
        hits[0].label, "Open settings",
        "a palette that answers with what you did last time instead of what \
         you just typed is worse than one with no memory at all"
    );
}

#[test]
fn the_memory_survives_a_restart() {
    let store = Store::new(tmp("roundtrip"));
    let mut r = Recent::load(&store);
    r.picked("Open settings");
    r.save(&store).unwrap();
    assert_eq!(Recent::load(&store).picked, vec!["Open settings"]);
}

// ---------------------------------------------------------------------------
// The catalogue itself
// ---------------------------------------------------------------------------

#[test]
fn every_entry_is_phrased_as_something_you_do() {
    for e in catalogue() {
        assert!(!e.label.is_empty() && !e.hint.is_empty());
        assert!(
            !e.label.contains('_') && !e.label.contains("::"),
            "{} is an identifier, not a sentence",
            e.label
        );
        assert!(
            e.label.contains(' '),
            "{:?} is a noun — \"Accounts\" is navigation wearing an action's \
             clothes",
            e.label
        );
    }
}

#[test]
fn no_two_entries_share_a_label() {
    // The label is the id the memory keys on, so two of them would make
    // "what did you pick" ambiguous.
    let c = catalogue();
    let mut seen: Vec<&str> = c.iter().map(|e| e.label).collect();
    let n = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), n);
}

#[test]
fn every_destination_is_a_page_that_exists() {
    for e in catalogue() {
        if let Does::Go(href) = e.does {
            assert!(
                atlas::hub::route(href).is_some(),
                "{} points at {href}, which nothing serves",
                e.label
            );
        }
    }
}

#[test]
fn everything_reachable_from_the_menu_is_reachable_by_typing() {
    let c = catalogue();
    let r = Recent::default();
    for (_, pages) in atlas::hub::NAV {
        for p in *pages {
            let reached = find(&c, p.label(), &r)
                .iter()
                .any(|e| matches!(e.does, Does::Go(h) if h == p.href()));
            let listed = c
                .iter()
                .any(|e| matches!(e.does, Does::Go(h) if h == p.href()));
            assert!(
                listed && reached,
                "{} is in the menu and not in the palette, so it depends on \
                 which one you happened to reach for",
                p.label()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------

#[test]
fn a_result_does_the_thing_rather_than_taking_you_to_the_button() {
    let c = catalogue();
    let hits = find(&c, "arrange", &Recent::default());
    let html = atlas::hub::find_page("arrange", &hits, None);
    assert!(
        html.contains("action='/hub/dash'"),
        "an action must post, not navigate to the page that holds the button"
    );
}

#[test]
fn a_search_that_finds_nothing_says_so_in_a_sentence() {
    let html = atlas::hub::find_page("banana", &[], None);
    assert!(html.contains("banana"), "it repeats what you asked for");
    assert!(!html.contains("Nothing here."), "not the generic empty page");
}

#[test]
fn the_plain_page_shows_no_more_than_the_overlay_does() {
    let c = catalogue();
    let hits = find(&c, "", &Recent::default());
    let html = atlas::hub::find_page("", &hits, None);
    let shown = html.matches("<a class='hit").count() + html.matches("<button class='hit").count();
    assert!(
        shown <= SHOW,
        "{shown} results — a palette that lists everything is the menu again"
    );
}

#[test]
fn the_overlay_starts_closed_and_says_how_to_work_it() {
    let c = catalogue();
    let html = atlas::hub::palette_overlay(&c, &Recent::default());
    assert!(html.contains("id=palette hidden"), "not in the way until asked for");
    assert!(html.contains("Escape closes"));
    assert!(
        html.contains("action='/hub/find'"),
        "with no script at all, typing and pressing enter still lands somewhere"
    );
}

#[test]
fn the_overlay_carries_every_entry_so_typing_filters_all_of_them() {
    let c = catalogue();
    let html = atlas::hub::palette_overlay(&c, &Recent::default());
    for e in &c {
        assert!(
            html.contains(&atlas::hub::esc(e.label)),
            "{} is missing, so typing could never filter to it",
            e.label
        );
    }
}

#[test]
fn a_page_that_is_not_a_page_does_not_get_an_overlay_bolted_to_it() {
    let c = catalogue();
    let fragment = "<p>something went wrong</p>".to_string();
    let out = atlas::hub::with_palette(fragment.clone(), &c, &Recent::default());
    assert_eq!(
        out, fragment,
        "markup that looks right and is not is worse than markup that is \
         obviously a fragment"
    );
}

#[test]
fn the_way_in_is_visible_rather_than_only_a_shortcut() {
    // A palette nobody knows exists is dead weight, and the sidebar stays:
    // the palette is how someone who knows the product moves, the menu is how
    // anyone learns what is in it.
    let html = atlas::hub::shell_at(Some(atlas::hub::Page::Settings), "Settings", "<p>x</p>");
    assert!(html.contains("Find anything"));
    assert!(html.contains("Ctrl K"));
    assert!(html.contains("nav class=side"), "the menu is still there");
    // The way in has to reach the same place the shortcut does, or it is two
    // features that agree today.
    let opener = html.find("id=palopen").expect("the button is there");
    let href = &html[opener..opener + 60];
    assert!(
        href.contains("/hub/find"),
        "the visible way in must land where the palette lands: {href}"
    );
    // And the menu must still carry every page, so the palette is an addition
    // rather than the only route.
    let links = atlas::hub::NAV.iter().flat_map(|(_, p)| p.iter()).count();
    assert!(links >= 12, "only {links} pages in the menu");
}


#[test]
fn a_settings_category_can_be_reached_by_typing_what_is_in_it() {
    let c = catalogue();
    let r = Recent::default();
    for (typed, expect) in [
        ("camera", "Change what it can see"),
        ("overnight", "Change what it may touch"),
        ("interrupt", "Change when it speaks first"),
    ] {
        assert!(
            find(&c, typed, &r).first().is_some_and(|e| e.label == expect),
            "typing {typed:?} should land in the category that holds it, not \
             at the top of a page with forty settings on it"
        );
    }
}

#[test]
fn a_jump_into_a_settings_category_still_resolves_to_a_page() {
    for e in catalogue() {
        if let Does::Go(href) = e.does {
            assert!(
                atlas::hub::route(href).is_some(),
                "{href} is a dead end — an anchor must not stop a page resolving"
            );
        }
    }
}
