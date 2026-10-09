//! `overnight.rs` was a complete night with nobody to run it.
//!
//! 373 lines: the window, the give-up-after-N-separate-failures rule, the
//! per-night problem cap, the budget check, the delegated-turn budget, and
//! the morning brief. All of it tested. **None of it called.** The settings
//! sat in `tools.yaml` — `start_hour`, `stop_hour`, `max_problems` — read by
//! nothing, which is the config-that-lies shape this tree keeps turning up:
//! worse than dead code, because a setting in a file tells its reader the
//! behaviour exists.
//!
//! What runs now is the `ask_you_later` brain, which is the shipped default
//! and the only one that is free and needs no network. The `local`, `hosted`
//! and `delegate` brains drive a solver, spend money, or work another
//! application on your behalf; those stay unwired on purpose.
//!
//! # The window is gone (19 Sep 2026)
//!
//! `start_hour: 23` and `stop_hour: 6` were the gate, and a clock cannot tell
//! you asleep from you at a desk at two in the morning, nor know you left for
//! work at eight. So the night ran on a timetable: **it ran while you were up
//! working, and it did not run on the Saturday you were out all day.**
//!
//! `daily::whereabouts` answers what they were standing in for -- are you
//! gone, and gone long enough that an hour of work will not be interrupted --
//! from when you were last here and the hours `Rhythm` has watched you keep.
//! Nothing asks you to announce that you are stepping away, because that is a
//! thing people do not do.
//!
//! The two settings were deleted rather than pinned. They are not a promise
//! about something unbuilt; the capability exists and works better without
//! them.

use atlas::backlog::Blocker;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-night-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

/// A timestamp at a given hour of the day, on a day far enough in that the
/// hourly gates have no history to trip over.
fn at_hour(day: u64, hour: u64) -> u64 {
    day * 86_400 + hour * 3600
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

// ---------- the settings are real ----------

#[test]
fn the_night_ships_on() {
    let c = cfg();
    let o = c.tools.as_ref().expect("tools.yaml loads").overnight.clone();
    assert!(o.enabled, "overnight still ships disabled");
    assert_eq!(
        o.brain,
        atlas::overnight::Brain::AskYouLater,
        "the shipped brain should be the one that is free and offline"
    );
}

#[test]
fn the_clock_window_is_gone_rather_than_left_settable_and_unread() {
    // A setting in a file tells its reader the behaviour exists. These two
    // described a night that no longer happens on a clock, so they are out
    // of the struct and out of `tools.yaml` -- not pinned, because
    // `PROMISES_ABOUT_WHAT_IS_NOT_BUILT` is for decisions recorded ahead of
    // a capability that does not exist, and this capability exists.
    // Comment lines stripped first. The note in `tools.yaml` explaining that
    // these two are gone names them, so a plain `contains` fails on the
    // comment describing the removal -- the same self-reference trap
    // `dead_capabilities.rs` documents for its own ceiling constant, hit for
    // the fourth time in two days.
    let yaml = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    let settings: String = yaml
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!settings.contains("start_hour:"), "the clock window is settable again");
    assert!(!settings.contains("stop_hour:"));
    // And what replaced it is settable, because it is about you.
    assert!(yaml.contains("away_a_while_minutes:"), "nowhere to say what counts as gone");

    let src = std::fs::read_to_string("src/overnight.rs").expect("overnight.rs");
    assert!(!src.contains("pub fn in_window"), "the clock gate is back");
    assert!(src.contains("where_you_are.free_to_work()"), "the night doesn't ask where you are");
}

// ---------- the guarantee ----------

#[test]
fn the_night_never_applies_anything() {
    // Not a setting. `apply_while_asleep` is `#[serde(skip)]` with a `never`
    // default, so no config file can turn it on, and this holds that shut
    // from the outside as well.
    let c = cfg();
    let o = c.tools.as_ref().unwrap().overnight.clone();
    assert!(
        !o.apply_while_asleep,
        "something can now apply changes while you are asleep"
    );

    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(
        !raw.contains("apply_while_asleep"),
        "apply_while_asleep appears in the shipped config, which invites someone to set it"
    );
}

#[test]
fn a_decision_only_you_can_make_is_left_alone() {
    // The one thing a night must not do is guess at an answer. The filter
    // reads the request the way you said it.
    use atlas::handoff::Problem;
    let mine = |g: &str| Problem { goal: g.into(), ..Default::default() };

    for needs_you in [
        "which of these should I cancel",
        "should I reply to that",
        "send the invoice",
        "pay the hosting bill",
        "delete the old exports",
    ] {
        assert!(
            !atlas::overnight::worth_doing_overnight(&mine(needs_you)),
            "the night would have taken this on: {needs_you}"
        );
    }

    for fine in ["retry the backup", "reindex the notes folder", "look up the tide times"] {
        assert!(
            atlas::overnight::worth_doing_overnight(&mine(fine)),
            "the night skipped something harmless: {fine}"
        );
    }
}

// ---------- it actually runs ----------

#[test]
fn work_happens_while_you_are_gone_and_is_reported_when_you_are_back() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "starts");

    // You were here. Without this the daemon has never seen you, and
    // `whereabouts` answers `Here` -- which starts nothing, deliberately: a
    // fresh install with no idea when you sleep should not begin unattended
    // work on its first hour.
    d.turn("hello", at_hour(100, 22));

    // Something Atlas could not do. Seeded directly, the way the other
    // daemon tests do it -- the first draft of this test spoke a request and
    // assumed it was parked, it never was, and the whole file then passed
    // with the wiring torn out because nothing ever happened.
    d.backlog.record("retry the backup", Blocker::NeedsApproval, at_hour(100, 14));

    // 1am and 2am: the night advances a step an hour.
    d.tick(at_hour(100, 25));
    d.tick(at_hour(100, 26));

    // You come back. That is what ends the stretch of work -- not the clock
    // reaching six, which is what used to end it and is why the night ran
    // while you were up and refused to run while you were out.
    //
    // Keyed on the write-up's own wording. The backlog separately offers to
    // retry a parked item ("Earlier you asked me to..."), so matching the
    // problem name alone would pass on that instead -- which is what the
    // first version of this assertion did.
    d.turn("morning", at_hour(101, 8));
    let now = at_hour(101, 8) + 60;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut lines = Vec::new();
    loop {
        lines.extend(d.tick(now));
        if lines.iter().any(|line| line.contains("While you were out:")) || std::time::Instant::now() >= deadline { break; }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let morning = lines.join(" | ");

    // "While you were out", not "Overnight" -- and that is the point rather
    // than an inconvenience. This daemon has no fortnight of watching behind
    // it, so `Rhythm::quiet_at` cannot say one in the morning is your quiet
    // stretch, and `whereabouts` falls back to `Out`. That is the cautious
    // direction: `Out` assumes you may be back at any moment.
    //
    // The heading used to be the fixed word "Overnight:" whatever had
    // happened, which on a Tuesday afternoon you spent at the office was a
    // small lie that makes the rest of the report harder to trust.
    assert!(
        morning.contains("While you were out:") && morning.contains("retry the backup"),
        "the night's work was never reported: {morning}"
    );

    // And only once. A write-up repeated every tick is worse than a late one.
    let mut later = Vec::new();
    for _ in 0..30 { later.extend(d.tick(at_hour(101, 8) + 120)); std::thread::sleep(std::time::Duration::from_millis(5)); }
    let later = later.join(" | ");
    assert!(!later.contains("While you were out:"), "it was said twice: {later}");
}

#[test]
fn a_night_is_called_a_night_once_atlas_knows_your_hours() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    // The other half of the heading. With a fortnight of watching behind it,
    // `quiet_at` can say one in the morning is your quiet stretch, and the
    // same work is reported as having happened overnight.
    use atlas::daily::{whereabouts, DailyConfig, Rhythm, Whereabouts};
    use atlas::judgment::JudgmentConfig;

    let mut r = Rhythm::default();
    for _ in 0..20 {
        r.note_day();
        // Awake 9am to 11pm, asleep midnight to 8am.
        //
        // Every waking hour, not every other one. The first version of this
        // fixture logged 9, 11, 13, 15... and two in the afternoon then had
        // no activity at all, so `quiet_at` called it part of the quiet
        // stretch and Atlas decided you were asleep at 2pm. The fixture was
        // wrong and the code was right, which is worth the four lines it
        // takes to say.
        for h in 9..=23 {
            for _ in 0..6 {
                r.saw(h, true);
            }
        }
    }
    let cfg = DailyConfig::default();
    let last_turn = at_hour(100, 22);

    // One in the morning, four hours after you stopped: asleep.
    assert_eq!(whereabouts(last_turn, at_hour(100, 25), &r, &cfg, &JudgmentConfig::default()), Whereabouts::Asleep);
    // Two in the afternoon, having been gone since ten: out, not asleep.
    assert_eq!(
        whereabouts(at_hour(101, 10), at_hour(101, 14), &r, &cfg, &JudgmentConfig::default()),
        Whereabouts::Out
    );
    // And both mean Atlas may get on with something.
    assert!(Whereabouts::Asleep.free_to_work());
    assert!(Whereabouts::Out.free_to_work());
    assert!(!Whereabouts::Here.free_to_work());
}

#[test]
fn being_quiet_for_ten_minutes_is_not_being_gone() {
    use atlas::daily::{whereabouts, DailyConfig, Rhythm, Whereabouts};
    use atlas::judgment::JudgmentConfig;
    let cfg = DailyConfig::default();
    let r = Rhythm::default();
    assert_eq!(
        whereabouts(at_hour(100, 14), at_hour(100, 14) + 600, &r, &cfg, &JudgmentConfig::default()),
        Whereabouts::Here,
        "ten minutes of not typing started an hour of work"
    );
}

#[test]
fn a_machine_that_has_never_seen_you_starts_nothing() {
    // A fresh install with no idea when you sleep should not begin
    // unattended work on its first hour. `Here` is the answer that starts
    // nothing, and it is the honest one: Atlas does not know where you are.
    use atlas::daily::{whereabouts, DailyConfig, Rhythm, Whereabouts};
    use atlas::judgment::JudgmentConfig;
    assert_eq!(
        whereabouts(0, at_hour(100, 3), &Rhythm::default(), &DailyConfig::default(), &JudgmentConfig::default()),
        Whereabouts::Here
    );
}

#[test]
fn nothing_happens_while_you_are_sitting_there() {
    // This used to be `nothing_happens_at_midday`, and midday was never the
    // point -- being *there* was. A night that runs while you are working is
    // not a night, and one that refuses to run at two in the afternoon while
    // you are out is a timetable rather than an assistant.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "present");

    d.backlog.record("retry the backup", Blocker::NeedsApproval, at_hour(200, 13));
    // A turn of yours, then a tick a minute later. You are plainly here.
    d.turn("hello", at_hour(200, 14));
    let said = d.tick(at_hour(200, 14) + 60).join(" | ");

    assert!(
        !said.contains("Overnight:"),
        "it started work with you sitting at the machine: {said}"
    );
}

#[test]
fn switching_it_off_stops_the_night() {
    let mut c = cfg();
    c.tools.as_mut().unwrap().overnight.enabled = false;
    let p = plat();
    let mut d = daemon(&c, &p, "off");

    d.backlog.record("retry the backup", Blocker::NeedsApproval, at_hour(300, 14));
    d.tick(at_hour(300, 25));
    d.tick(at_hour(300, 26));
    let said = d.tick(at_hour(301, 8)).join(" | ");

    assert!(!said.contains("Overnight:"), "the night ran with the setting off: {said}");
}

// ---------- the brief tells the truth ----------

#[test]
fn the_note_does_not_claim_an_attempt_that_never_happened() {
    // `ask_you_later` writes things up. It does not try them. A brief that
    // implied otherwise would be the night lying about itself, which is the
    // failure this whole tree keeps correcting.
    let c = cfg();
    let o = c.tools.as_ref().unwrap().overnight.clone();
    let note = atlas::overnight::note_for("retry the backup", &o);

    assert!(note.contains("retry the backup"), "the note lost the problem: {note}");
    for claim in ["fixed", "solved", "tests pass", "attempted", "tried"] {
        assert!(
            !note.to_lowercase().contains(claim),
            "the note claims '{claim}' for work that was only written up: {note}"
        );
    }
}

// ---------- what else is Atlas holding ----------

#[test]
fn asking_what_is_queued_covers_errands_and_not_only_posts() {
    // `Intent::Queued` answered from the publisher alone, so "what's queued"
    // meant "what posts are waiting to be sent" and nothing else -- while
    // `crew::queued`, `crew::in_hand` and `crew::why_waiting`, written for
    // exactly this question, had no caller anywhere.
    //
    // With nothing running there is nothing extra to say, and that is the
    // case worth pinning: the answer must not grow a dangling "and 0 errands
    // waiting" just because a second source was added to it.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "queued");

    let said = d.turn("what's queued", at_hour(400, 10));

    assert!(!said.is_empty(), "nothing came back at all");
    assert!(
        !said.contains("0 errand"),
        "the answer counted nothing out loud: {said}"
    );
    assert!(
        !said.to_lowercase().contains("not sure which you meant"),
        "the phrase list no longer recognises this: {said}"
    );
}
