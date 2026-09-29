//! The access page, and settings you can find something in.
//!
//! Access was a page that took a list of sites and was never given one, so it
//! rendered "Nothing yet." on a machine where Atlas had a browser session and a
//! vault. That is worse than empty: an access page saying nothing reads as
//! "nothing to worry about", which is the one thing an access page must never
//! do by accident. `credentials.rs` had the honest answer written down the
//! whole time and nothing ever called it.
//!
//! Settings had forty-odd entries in four groups, twenty of them in one called
//! "Acting" — which is not a category, it is where things go when nobody
//! decides.

use atlas::credentials::{self, Kept, Opens};
use atlas::hub;
use atlas::settings::{registry, Settings};
use atlas::voice::ToolsConfig;

fn s() -> Settings {
    registry(&ToolsConfig::default())
}

// ---------------------------------------------------------------------------
// Access
// ---------------------------------------------------------------------------

#[test]
fn an_empty_access_page_says_atlas_can_reach_nothing_rather_than_going_quiet() {
    let html = hub::access_page_full(&[], &[], &[]);
    assert!(
        html.contains("Atlas is holding no credentials"),
        "a page that says nothing on a security screen reads as nothing to \
         worry about"
    );
}

#[test]
fn it_always_says_what_atlas_never_holds() {
    // "What can it reach" is only reassuring next to "and what can it never".
    // Checked by counting, so adding a fifth thing to the list and forgetting
    // to render it fails here rather than passing because the other four are
    // still on the page.
    let never = credentials::never_held();
    assert!(!never.is_empty(), "the list itself must not be empty");

    let html = hub::access_page_full(&[], &[], &[]);
    let rendered = never
        .iter()
        .filter(|(what, _)| html.contains(&hub::esc(what)))
        .count();
    assert_eq!(
        rendered,
        never.len(),
        "{} of {} things Atlas refuses to hold reached the page",
        rendered,
        never.len()
    );

    // And the reason, not only the name — "your account passwords" on its own
    // is a claim, the sentence after it is why you should believe it.
    for (what, why) in &never {
        assert!(
            html.contains(&hub::esc(why)),
            "{what} is listed with no reason given"
        );
    }
}

#[test]
fn each_thing_it_holds_comes_with_the_words_for_taking_it_back() {
    let all = credentials::all();
    let held: Vec<&credentials::Credential> = all.iter().collect();
    let html = hub::access_page_full(&held, &[], &[]);
    for c in &held {
        assert!(html.contains(c.name), "{} missing", c.name);
        assert!(
            html.contains(&hub::esc(c.revoke)),
            "{} is listed with no way to take it away, which is the half that \
             matters",
            c.name
        );
    }
}

#[test]
fn a_credential_kept_somewhere_weaker_than_what_it_opens_is_named_first() {
    let bad = credentials::Credential {
        name: "a test key",
        used_for: "nothing real",
        opens: Opens::Everything,
        kept: Kept::PlainConfig,
        revoke: "delete it",
        needed_overnight: false,
    };
    let html = hub::access_page_full(&[], &[&bad], &[]);
    let problem = html.find("Kept somewhere it shouldn't be").expect("named");
    let inventory = html.find("What Atlas can reach").expect("also there");
    assert!(
        problem < inventory,
        "the only thing on this page that is a problem rather than a fact has \
         to come before the facts"
    );
}

#[test]
fn nothing_on_the_access_page_reads_like_a_type_name() {
    let all = credentials::all();
    let held: Vec<&credentials::Credential> = all.iter().collect();
    let html = hub::access_page_full(&held, &credentials::misplaced(), &[]);
    for name in ["PlainConfig", "YourBrowserSession", "AService", "Opens::", "Kept::"] {
        assert!(!html.contains(name), "{name} is a variable name");
    }
    // Every credential's storage and reach must actually be described, not
    // just avoid being described in Rust.
    let described = held
        .iter()
        .filter(|c| html.contains(&hub::esc(c.used_for)))
        .count();
    assert_eq!(
        described,
        held.len(),
        "{described} of {} credentials say what they are for",
        held.len()
    );
}

#[test]
fn a_signed_in_site_can_be_taken_away_from_the_page() {
    let sites = vec![(
        "Example".to_string(),
        "signed in three weeks ago".to_string(),
        "example.com".to_string(),
        true,
    )];
    let html = hub::access_page_full(&[], &[], &sites);
    assert!(html.contains("Signed in right now"));
    assert!(html.contains("action=/hub/access/revoke"));
    assert!(html.contains("Take all of it away"));
}

#[test]
fn the_signed_in_section_is_absent_rather_than_empty_when_there_is_nothing() {
    let html = hub::access_page_full(&[], &[], &[]);
    assert!(
        !html.contains("Signed in right now"),
        "an empty heading makes you wonder whether it failed to load"
    );
}

#[test]
fn the_honesty_check_still_holds() {
    // Anything opening more than nothing must not sit in a config file.
    assert!(
        credentials::misplaced().is_empty(),
        "the inventory itself now breaks the rule it exists to enforce: {:?}",
        credentials::misplaced().iter().map(|c| c.name).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[test]
fn no_category_is_a_bucket() {
    let s = s();
    for g in s.groups() {
        let n = s.in_group(&g).len();
        assert!(
            n <= 12,
            "{g} holds {n} — past a dozen you are scanning, not choosing"
        );
        assert!(n > 0, "{g} is a heading with nothing under it");
    }
}

#[test]
fn there_is_no_category_called_acting() {
    let s = s();
    for g in s.groups() {
        assert_ne!(
            g, "Acting",
            "that was where twenty of the forty-odd settings went when nobody \
             decided what they were"
        );
    }
}

#[test]
fn the_categories_are_in_a_deliberate_order_not_an_alphabetical_one() {
    let s = s();
    let groups = s.groups();
    let mut sorted = groups.clone();
    sorted.sort();
    assert_ne!(
        groups, sorted,
        "sorting put \"Acting\" first because of its A, which is not a reason"
    );
    assert_eq!(
        groups.first().map(|g| g.as_str()),
        Some("Talking to it"),
        "the first category should be the one you change most"
    );
}

#[test]
fn every_category_says_what_it_covers() {
    let s = s();
    for g in s.groups() {
        assert!(
            Settings::group_note(&g).is_some(),
            "{g} is a bare heading, so you have to open every section to find \
             out which one holds the thing you came for"
        );
    }
}

#[test]
fn every_setting_has_a_home() {
    let s = s();
    let filed: usize = s.groups().iter().map(|g| s.in_group(g).len()).sum();
    assert_eq!(
        filed,
        s.items.len(),
        "a setting in no category is a setting nobody finds"
    );
}

#[test]
fn the_settings_page_can_be_jumped_around_rather_than_scrolled() {
    let s = s();
    let html = hub::settings_page(&s);
    for g in s.groups() {
        assert!(
            html.contains(&format!(">{g}<")),
            "{g} is not in the index at the top"
        );
    }
    assert!(
        html.contains("<div class=jump>"),
        "forty settings down one page is a scroll, not a choice"
    );
}

#[test]
fn the_page_says_how_much_you_have_actually_changed() {
    // Find something sitting at its default, so the count genuinely moves.
    let mut s = s();
    let untouched = s
        .items
        .iter()
        .find(|i| !i.changed() && matches!(i.value, atlas::settings::Value::Toggle(_)))
        .map(|i| i.key.clone())
        .expect("something is at its default");
    let before = s.changed().len();
    let now = match s.get(&untouched).map(|i| i.value.clone()) {
        Some(atlas::settings::Value::Toggle(true)) => "off",
        _ => "on",
    };
    s.set(&untouched, now).expect("set it");
    let after = s.changed().len();
    assert_eq!(after, before + 1, "the change was recorded");
    assert!(
        hub::settings_page(&s).contains(&format!("You've changed {after}")),
        "the page has to say how much of this is yours rather than the default"
    );
}


// ---------------------------------------------------------------------------
// The naming rule
// ---------------------------------------------------------------------------

/// A setting is named, then explained.
///
/// These used to read "Stop listening when you stop talking" and "Offer the
/// brief rather than reading it" — accurate, and doing the description's job,
/// so a page of them scanned like prose rather than a list you can find
/// something in. The sentence still exists: it is `what`.
#[test]
fn every_setting_is_named_rather_than_described() {
    for item in &s().items {
        let n = &item.name;
        assert!(
            n.split_whitespace().count() <= 3,
            "{n:?} is a sentence — that is what the description is for"
        );
        assert!(!n.ends_with('.') && !n.contains(','), "{n:?} is punctuated like prose");
        for opener in ["Know ", "Tell ", "Ask ", "Let ", "Stop ", "Use ", "Look ", "Get ", "Admit "] {
            assert!(
                !n.starts_with(opener),
                "{n:?} starts by describing an action rather than naming a thing"
            );
        }
    }
}

#[test]
fn every_setting_still_explains_itself_underneath() {
    for item in &s().items {
        assert!(
            item.what.split_whitespace().count() >= 4,
            "{} has no real description, so the short name costs you the meaning",
            item.name
        );
        assert_ne!(item.what, item.name, "{} explains itself with its own name", item.name);
    }
}

#[test]
fn no_two_settings_share_a_name() {
    // Short names collide far more easily than sentences did: two settings
    // were both called "Voice" the moment they were shortened.
    let s = s();
    let mut names: Vec<&str> = s.items.iter().map(|i| i.name.as_str()).collect();
    let n = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(n, names.len(), "two settings share a name");
}

#[test]
fn a_setting_does_not_just_repeat_its_category() {
    let s = s();
    for g in s.groups() {
        for item in s.in_group(&g) {
            assert_ne!(
                item.name.to_lowercase(),
                g.to_lowercase(),
                "{} says nothing its heading did not already say",
                item.name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Gestures page — teaches the shapes AND says whether hand control is on
// ---------------------------------------------------------------------------
//
// The page used to render the vocabulary and a "turn it on or off" link, but
// never said which it currently was. "How do I make this shape" and "is this
// even watching right now" are different questions; someone opening the page
// can be asking either, so the page must answer both. Enablement is read from
// the setting and rendered, so it cannot say "on" while the toggle is off.

#[test]
fn the_gestures_page_says_when_hand_control_is_on() {
    let known = atlas::handshape::as_demonstrated();
    let html = hub::gestures_page(&known, &atlas::handshape::needs_deciding(), true, false);
    assert!(
        html.contains("Hand control is on"),
        "an enabled gestures page must say so, not just teach the shapes"
    );
    // Still teaches: a demonstrated action is present.
    assert!(
        html.contains("undo that"),
        "the how-to must still be there — the shapes and what they do"
    );
}

#[test]
fn the_gestures_page_says_when_hand_control_is_off() {
    let known = atlas::handshape::as_demonstrated();
    let html = hub::gestures_page(&known, &atlas::handshape::needs_deciding(), false, false);
    assert!(
        html.contains("Hand control is off"),
        "a disabled gestures page must say so plainly, not read as if it were on"
    );
    // The reference is still shown while off, so you can learn before enabling.
    assert!(
        html.contains("undo that"),
        "the gestures stay visible as a reference even when hand control is off"
    );
}

#[test]
fn the_gestures_page_states_the_irreversible_safety_rule_from_the_setting() {
    let known = atlas::handshape::as_demonstrated();

    // Default: a gesture cannot approve something irreversible.
    let safe = hub::gestures_page(&known, &[], true, false);
    assert!(
        safe.contains("never approves something irreversible"),
        "by default the page must say a gesture can't approve irreversible actions"
    );

    // Loosened deliberately: the page says that instead, rather than the safe line.
    let loose = hub::gestures_page(&known, &[], true, true);
    assert!(
        loose.contains("approve anything"),
        "if gestures may approve anything, the page must say so, not the safe line"
    );
    assert!(
        !loose.contains("never approves something irreversible"),
        "the page must not claim the safe rule while it is switched off"
    );
}
