//! Going somewhere your texts will not arrive.
//!
//! Two modules were waiting on the same small thing, and neither could be
//! finished without it.
//!
//! `going_away.remind_days_before` is *"remind you this many days before a
//! trip you've told it about"* — and there was no way to tell it about a
//! trip. No date anywhere in the tree, so the setting was a threshold on a
//! number nothing computed. `periodic_nudge` had the same shape one step
//! along: it takes `days_since_last` and nothing kept a last.
//! `codes.check_days_before` was a third threshold on that same absent date.
//!
//! And `codes` had its own version of the problem: it could say what was
//! missing, and there was no way to tell it anything had changed.
//! `used_one`, `logins_available` and `Set::running_low` had no callers
//! anywhere.
//!
//! The date is what unlocks all of it.

use atlas::codes::{self, CodesConfig, Set};
use atlas::goingaway::{self, Away, AwayConfig};

const DAY: u64 = 86_400;

fn a_set(site: &str, issued: u32, used: u32, in_hand: bool) -> Set {
    Set { site: site.into(), issued, used, at: 0, kept_where: None, in_hand }
}

fn on() -> AwayConfig {
    AwayConfig { enabled: true, ..Default::default() }
}

// ================= the date =================

#[test]
fn a_trip_is_a_date_written_the_one_way() {
    // The standing ruling this follows is `one_way_to_write_a_date` in this
    // same suite: an ISO date is the one form that means the same thing to
    // everybody.
    assert_eq!(goingaway::leaving_on("1970-01-02"), Some(DAY));
    assert_eq!(goingaway::leaving_on("  2026-11-03  "), goingaway::leaving_on("2026-11-03"));

    // Anything else is a question rather than a guess. "03/11/2026" is the
    // third of November to half the world and the eleventh of March to the
    // other half, and a travel reminder is the worst place to find that out.
    for not_a_date in ["03/11/2026", "November 3rd", "2026-13-01", "2026-11-03-01", ""] {
        assert_eq!(goingaway::leaving_on(not_a_date), None, "{not_a_date}");
    }
}

#[test]
fn how_many_days_until_you_go_is_the_number_everything_else_reads() {
    let now = 1_000 * DAY;
    let away = Away { leaving_at: now + 10 * DAY, going_to: "Germany".into(), last_checked: 0 };
    assert_eq!(away.days_until(now), Some(10));
    assert!(away.on_a_trip());

    // A trip that has been and gone is not a trip in eleven months' time.
    let past = Away { leaving_at: now - DAY, ..away.clone() };
    assert_eq!(past.days_until(now), None);
    assert!(past.on_a_trip(), "it still happened — it just isn't ahead of you");

    // No trip is no answer rather than zero days, which would read as today.
    assert_eq!(Away::default().days_until(now), None);
    assert!(!Away::default().on_a_trip());
}

// ================= remind_days_before, which was the dead setting =========

#[test]
fn when_it_starts_reminding_you_is_the_number_you_set() {
    let now = 1_000 * DAY;
    let mut cfg = on();
    assert_eq!(cfg.remind_days_before, 14, "the shipped default");

    let fortnight_out = Away { leaving_at: now + 14 * DAY, ..Default::default() };
    let three_weeks_out = Away { leaving_at: now + 21 * DAY, ..Default::default() };
    assert!(fortnight_out.time_to_get_ready(&cfg, now));
    assert!(!three_weeks_out.time_to_get_ready(&cfg, now));

    // The number is read, not a fortnight that happens to equal it. The
    // window matters more than it looks: printing recovery codes and waiting
    // for a hardware key both take days, so being told the morning you leave
    // is the same as not being told.
    cfg.remind_days_before = 30;
    assert!(three_weeks_out.time_to_get_ready(&cfg, now), "the window is hardcoded");
    cfg.remind_days_before = 2;
    assert!(!fortnight_out.time_to_get_ready(&cfg, now));

    // Off means off, and no trip means nothing to be close to.
    cfg.remind_days_before = 14;
    cfg.enabled = false;
    assert!(!fortnight_out.time_to_get_ready(&cfg, now));
    cfg.enabled = true;
    assert!(!Away::default().time_to_get_ready(&cfg, now));
}

#[test]
fn it_says_where_you_are_going_and_what_would_stop_you_getting_in() {
    use atlas::accounts::{Account, SecondFactor, Stakes};
    let account = |site: &str, f: SecondFactor| Account {
        site: site.into(),
        second_factor: f,
        stakes: Stakes::High,
        reused_password: false,
        has_recovery_codes: false,
        settings_url: None,
    };
    let now = 1_000 * DAY;
    let away = Away { leaving_at: now + 5 * DAY, going_to: "Germany".into(), last_checked: 0 };
    // A code by text is the one that breaks when you travel: it needs your
    // number, your carrier and a signal.
    let stuck = vec![account("the bank", SecondFactor::Sms)];

    let said = goingaway::the_trip_is_close(&away, &stuck, &on(), now).expect("five days out");
    assert!(said.contains("to Germany"), "{said}");
    assert!(said.contains("5 days"), "{said}");
    assert!(said.contains("lock you out"), "{said}");

    // Nothing that would lock you out is nothing to say. A reminder that
    // fires with no problem in it is the one that teaches you to ignore it.
    let fine = vec![account("GitHub", SecondFactor::App)];
    assert!(goingaway::the_trip_is_close(&away, &fine, &on(), now).is_none());

    // And no trip at all says nothing, whatever the accounts look like.
    assert!(goingaway::the_trip_is_close(&Away::default(), &stuck, &on(), now).is_none());
}

#[test]
fn the_periodic_check_has_a_last_to_count_from() {
    // `periodic_nudge` takes `days_since_last` and nothing kept a last, so
    // the caller had no honest number to pass.
    let now = 1_000 * DAY;
    assert_eq!(
        Away { last_checked: now - 30 * DAY, ..Default::default() }.days_since_asked(now),
        30
    );
    // Never asked is a long time, not no time — otherwise the periodic check
    // would never come due on a fresh install.
    assert_eq!(Away::default().days_since_asked(now), u32::MAX);
    // A clock that moved backwards reads as overdue rather than as a
    // reminder from the future.
    assert_eq!(
        Away { last_checked: now + DAY, ..Default::default() }.days_since_asked(now),
        u32::MAX
    );
}

// ================= check_days_before, the codes half =================

#[test]
fn how_far_out_the_codes_question_gets_raised_is_yours() {
    let mut cfg = CodesConfig { enabled: true, ..Default::default() };
    assert_eq!(cfg.check_days_before, 21, "the shipped default");

    // Deliberately a longer window than the accounts one: printing a fresh
    // set and getting the paper into your bag is a weekend job.
    assert!(cfg.check_days_before > AwayConfig::default().remind_days_before);

    assert!(codes::worth_raising_now(&cfg, Some(21)));
    assert!(!codes::worth_raising_now(&cfg, Some(22)));
    cfg.check_days_before = 60;
    assert!(codes::worth_raising_now(&cfg, Some(40)), "the window is hardcoded");

    // Off means off, and no trip means nothing to be ahead of.
    cfg.enabled = false;
    assert!(!codes::worth_raising_now(&cfg, Some(1)));
    cfg.enabled = true;
    assert!(!codes::worth_raising_now(&cfg, None));
}

#[test]
fn running_low_is_one_rule_and_warn_at_is_it() {
    // `Set::running_low` hardcoded three and had no caller, while `gaps`
    // compared against `cfg.warn_at`. Two answers to the same question, and
    // the one a person could change was not the one the named predicate gave.
    let mut cfg = CodesConfig { enabled: true, warn_at: 3, ..Default::default() };
    let three_left = a_set("google", 10, 7, true);
    let four_left = a_set("google", 10, 6, true);
    let spent = a_set("google", 10, 10, true);

    assert!(three_left.running_low(&cfg));
    assert!(!four_left.running_low(&cfg));
    // None left is not "running low", it is gone — and `gaps` says so with a
    // different sentence and a higher urgency.
    assert!(!spent.running_low(&cfg));

    cfg.warn_at = 5;
    assert!(four_left.running_low(&cfg), "the threshold is still hardcoded");

    // And `gaps` gives the same answer, because it asks the same question.
    cfg.warn_at = 3;
    let g = codes::gaps(&[three_left.clone()], &[], &cfg);
    assert!(g.iter().any(|x| x.what.contains("3 left")), "{g:#?}");
    cfg.warn_at = 1;
    let g = codes::gaps(&[three_left], &[], &cfg);
    assert!(!g.iter().any(|x| x.what.contains("left —")), "{g:#?}");
}

#[test]
fn the_number_that_matters_counts_only_what_is_actually_on_you() {
    // A set you generated and left on the screen is not a login you have from
    // a government computer, and counting it would make the number
    // comforting and wrong.
    let printed = a_set("google", 10, 2, true);
    let on_the_screen = a_set("github", 10, 0, false);
    assert_eq!(codes::logins_available(&[printed.clone(), on_the_screen.clone()]), 8);
    assert_eq!(codes::logins_available(&[on_the_screen]), 0);
    assert_eq!(codes::logins_available(&[]), 0);
    assert_eq!(codes::logins_available(&[printed]), 8);
}

#[test]
fn spending_one_is_something_you_can_tell_it() {
    let mut sets = vec![a_set("google", 10, 0, true)];
    assert_eq!(codes::used_one(&mut sets, "google"), Some(9));
    assert_eq!(codes::used_one(&mut sets, "GOOGLE"), Some(8), "the site name is not case-sensitive");
    assert_eq!(codes::used_one(&mut sets, "nowhere"), None);
    assert_eq!(sets[0].used, 2);
    assert_eq!(codes::logins_available(&sets), 8);
}

// ================= it is actually reachable =================

#[test]
fn the_commands_and_the_daemon_are_what_reach_it_rather_than_these_tests() {
    let main = crate::common::source_of("main");
    assert!(main.contains("fn run_away("), "there is still no way to record a trip");
    assert!(main.contains("fn run_codes("), "there is still no way to say you printed them");
    assert!(main.contains("goingaway::leaving_on(d)"), "nothing reads a date");
    assert!(main.contains("codes::used_one(&mut sets, &site)"), "nothing spends one");
    assert!(main.contains("codes::logins_available(&sets)"), "nothing says the number");

    let daemon = crate::common::source_of("daemon");
    assert!(
        daemon.contains("crate::goingaway::the_trip_is_close("),
        "remind_days_before is still a threshold nothing checks"
    );
    assert!(
        daemon.contains("crate::codes::worth_raising_now(&ccfg, away.days_until(t))"),
        "check_days_before is still a threshold nothing checks"
    );
    assert!(
        daemon.contains("crate::goingaway::periodic_nudge("),
        "the periodic check has no caller"
    );
    assert!(
        daemon.contains("away.days_since_asked(t) >= 1"),
        "a trip reminder that fires every tick is one you stop reading"
    );

    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    for key in ["remind_days_before:", "check_days_before:", "warn_at:", "check_every_days:"] {
        assert!(raw.contains(key), "{key} is no longer in the shipped config");
    }
    // Both ship off: these speak unprompted, which is a thing you turn on.
    assert!(!AwayConfig::default().enabled);
    assert!(!CodesConfig::default().enabled);
}
