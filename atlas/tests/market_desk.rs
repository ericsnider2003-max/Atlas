//! The general market desk (why-stale idea 7, 1 Oct 2026): today's
//! scheduled releases before the open, and "why did X move today" asked of
//! the web with today's date on it. General trading knowledge only.

use atlas::tradeday::{releases_in, why_it_moved};

#[test]
fn why_did_it_move_is_heard_and_names_the_thing() {
    assert_eq!(why_it_moved("Why did Nvidia move today?").as_deref(), Some("nvidia"));
    assert_eq!(why_it_moved("why is the S&P down today").as_deref(), Some("s&p"));
    assert_eq!(why_it_moved("atlas, why did gold drop").as_deref(), Some("gold"));
    assert_eq!(why_it_moved("why did tesla stock tank").as_deref(), Some("tesla"));
    assert_eq!(why_it_moved("why is the sky blue"), None);
    assert_eq!(why_it_moved("why did you move my file"), None, "\"move my file\" doesn't end in a move word");
}

#[test]
fn the_pre_market_names_todays_big_releases_at_your_clock() {
    // September 2026 from the checked tables: whatever lands on one day.
    let ev = atlas::market::events::month(2026, 9).expect("the tables cover 2026");
    let big: Vec<&atlas::market::events::Event> = ev.iter().filter(|e| e.impact >= 2).collect();
    let first = big.first().expect("a month has releases");
    let day = first.at - first.at.rem_euclid(86_400_000);
    let said = releases_in(&ev, day, day + 86_400_000, 0).expect("the day has one");
    assert!(said.starts_with("Scheduled today: ") && said.contains(&first.name), "{said}");
    assert_eq!(releases_in(&ev, 0, 1000, 0), None);
}
