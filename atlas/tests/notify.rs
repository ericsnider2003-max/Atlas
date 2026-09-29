//! Notifications: getting something to you when you are not at the desk.
//!
//! Written alongside the module rather than after it, and deliberately
//! testing behaviour rather than wording — the recurring failure in this
//! codebase is a component that is complete, tested, and never reached, so
//! the wiring tests at the bottom matter more than the unit tests at the top.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::notify::{
    can_notify, route, spoken, Note, NotifyConfig, Outbox, Route, Sent, Urgency, NO_NOTIFIER,
};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::presence::Presence;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const HOUR: u64 = 3600;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-nt-{tag}"));
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
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

fn routine(title: &str, at: u64) -> Note {
    Note::new(title, "body", Urgency::Routine, at)
}

fn urgent(title: &str, at: u64) -> Note {
    Note::new(title, "body", Urgency::Urgent, at)
}

// --- choosing a route -------------------------------------------------------

#[test]
fn at_the_desk_it_speaks_rather_than_popping_something_up() {
    assert_eq!(route(Presence::AtDesk, true, true, false, false), Route::Speak);
}

#[test]
fn away_it_puts_it_on_the_screen() {
    assert_eq!(route(Presence::Away, true, true, false, false), Route::Notify);
}

#[test]
fn at_the_desk_with_no_working_speakers_still_gets_through() {
    // Being at the desk with the volume off is not the same as being away,
    // and it must not mean the message is skipped.
    assert_eq!(route(Presence::AtDesk, false, true, false, false), Route::Notify);
}

#[test]
fn with_no_way_to_reach_you_it_is_held_rather_than_dropped() {
    assert_eq!(route(Presence::Away, false, false, false, false), Route::Hold);
}

#[test]
fn something_private_knocks_rather_than_discloses() {
    // Replaces a test that asserted a private note was *held* when presence
    // reported other people nearby. That was wrong twice: `presence::Sensor`
    // is wired to nothing, so `NotAlone` and `Stranger` can never occur and
    // the branch was dead; and "someone is nearby" is not "someone can read my
    // screen" — in a coffee shop it is always true, so the rule would hold
    // everything forever and Atlas would go quiet exactly where it is needed.
    let n = routine("your bank", 0).private();
    let (title, body) = n.shown();
    assert_eq!(title, "your bank", "you still get told there is something");
    assert!(!body.contains("body"), "the private content went on screen anyway: {body}");
    assert!(body.contains("Ask me"), "it doesn't say how to get the rest: {body}");
}

#[test]
fn an_ordinary_note_is_shown_in_full() {
    let n = routine("disk", 0);
    let (_, body) = n.shown();
    assert_eq!(body, "body", "an ordinary note was redacted for no reason");
}

#[test]
fn a_private_note_is_still_delivered_rather_than_held() {
    // The coffee-shop case. Delivery must not depend on knowing the room.
    let _n = routine("your bank", 0).private();
    assert_eq!(route(Presence::NotAlone, true, true, false, false), Route::Speak);
    assert_eq!(route(Presence::Away, false, true, false, false), Route::Notify);
}

#[test]
fn a_machine_with_no_camera_is_not_treated_as_an_empty_room() {
    // `Unknown` presence is the normal case — most machines have no sensor.
    // Treating it as "away" would mean Atlas never speaks to anyone.
    assert_eq!(route(Presence::Unknown, true, true, false, false), Route::Speak);
}

// --- what actually happened -------------------------------------------------

#[test]
fn whether_it_reached_you_is_not_the_same_as_whether_it_was_attempted() {
    // The distinction the whole module exists to keep. `Sent` is not a bool
    // for exactly this reason.
    assert!(Sent::Spoken.reached_you());
    assert!(Sent::Notified.reached_you());
    assert!(!Sent::Held.reached_you());
    assert!(!Sent::Failed("no notifier".into()).reached_you());
}

#[test]
fn a_failure_keeps_the_reason_rather_than_flattening_it() {
    // "Couldn't notify" throws away the one detail that says which thing to
    // fix: not installed, not configured, or blocked by policy.
    let s = Sent::Failed("no notification command is configured".into());
    match s {
        Sent::Failed(why) => assert!(why.contains("configured"), "{why}"),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn a_machine_that_cannot_notify_says_so_rather_than_claiming_it_can() {
    let none = NotifyConfig { tool: None, ..Default::default() };
    assert!(!can_notify(&none));
    let off = NotifyConfig { enabled: false, ..Default::default() };
    assert!(!can_notify(&off));
    assert!(NO_NOTIFIER.contains("held"), "it doesn't say what happens instead");
}

#[test]
fn showing_something_with_no_display_fails_rather_than_quietly_succeeding() {
    // The failure this codebase keeps producing: a component that returns
    // success because nothing objected.
    //
    // With no external notifier configured — the shipped default now that
    // PowerShell is gone — Atlas draws its own panel. On a machine with no
    // display, which is what a test run is, that must be reported rather than
    // returning Ok for a window nobody could see.
    let cfg = NotifyConfig { tool: None, ..Default::default() };
    let got = atlas::notify::show(&routine("x", 0), &cfg, &Default::default());
    if atlas::window::can_open() {
        // A real desktop: the panel is the delivery, and it opened.
        assert!(got.is_ok(), "a machine with a display could not open a panel: {got:?}");
    } else {
        let e = got.unwrap_err();
        assert!(e.contains("display"), "it didn't say why it couldn't show anything: {e}");
    }
}

#[test]
fn a_private_note_shown_in_a_window_only_knocks() {
    // The panel is on a screen other people may be able to see, so the
    // private version carries the title and nothing else.
    let c = atlas::window::Contents::knock("your bank");
    assert_eq!(c.title, "your bank");
    assert!(
        c.lines.iter().all(|l| !l.contains("balance") && l.contains("Ask me")),
        "the private content reached the screen: {:?}",
        c.lines
    );
}

#[test]
fn only_an_urgent_panel_steals_focus() {
    // A brief that jumps in front of what you are typing is a brief you come
    // to resent, and the window exists because it is *less* intrusive than
    // talking.
    use atlas::window::Panel;
    assert!(Panel::Urgent.insists());
    assert!(!Panel::Brief.insists());
    assert!(!Panel::Outstanding.insists());
    assert!(!Panel::Thinking.insists());
}

#[test]
fn a_second_urgent_panel_replaces_the_first_rather_than_stacking() {
    // Otherwise a bad morning leaves a wall of windows to dismiss one by one.
    use atlas::window::{stage, Contents, Panel};
    let a = stage(&Contents::new(Panel::Urgent, "first", vec!["one".into()])).unwrap();
    let b = stage(&Contents::new(Panel::Urgent, "second", vec!["two".into()])).unwrap();
    assert_eq!(a, b, "each urgent item staged its own window");
    let back = atlas::window::read_staged(&a).unwrap();
    assert_eq!(back.title, "second", "the newer item did not win");
}

#[test]
fn a_panel_survives_the_trip_to_the_child_process() {
    // The window runs as a separate process, so everything it needs travels
    // through a file. A field lost in that trip is a field the window silently
    // renders empty.
    use atlas::window::{read_staged, stage, Contents, Panel};
    let c = Contents::new(Panel::Brief, "While you were away", vec!["a".into(), "b".into()])
        .because("shown rather than said");
    let back = read_staged(&stage(&c).unwrap()).unwrap();
    assert_eq!(back.title, c.title);
    assert_eq!(back.lines, c.lines);
    assert_eq!(back.footer, c.footer);
    assert_eq!(back.panel, Panel::Brief);
}

// --- holding and returning --------------------------------------------------

#[test]
fn something_held_is_given_back_when_you_return() {
    let cfg = NotifyConfig::default();
    let mut o = Outbox::default();
    o.hold(urgent("disk is nearly full", 0), &cfg);
    assert_eq!(o.waiting(), 1);
    let back = o.collect(HOUR, &cfg);
    assert_eq!(back.len(), 1);
    assert_eq!(o.waiting(), 0, "collecting must not leave a duplicate behind");
}

#[test]
fn stale_routine_news_is_not_read_out_but_stale_urgent_news_is() {
    // "Your disk filled up four hours ago" is still true. "A backup finished
    // four hours ago" is not worth hearing when you sit down.
    let cfg = NotifyConfig { stale_after_secs: HOUR, ..Default::default() };
    let mut o = Outbox::default();
    o.hold(routine("a backup finished", 0), &cfg);
    o.hold(urgent("disk is nearly full", 0), &cfg);
    let back = o.collect(5 * HOUR, &cfg);
    assert_eq!(back.len(), 1, "got: {back:?}");
    assert_eq!(back[0].urgency, Urgency::Urgent);
}

#[test]
fn the_held_queue_does_not_grow_without_limit() {
    // A wall of held messages is the same as losing them.
    let cfg = NotifyConfig { max_held: 3, ..Default::default() };
    let mut o = Outbox::default();
    for i in 0..10 {
        o.hold(routine(&format!("note {i}"), i), &cfg);
    }
    assert_eq!(o.waiting(), 3);
}

#[test]
fn a_routine_note_is_dropped_before_an_urgent_one_when_full() {
    let cfg = NotifyConfig { max_held: 2, ..Default::default() };
    let mut o = Outbox::default();
    o.hold(urgent("disk is nearly full", 0), &cfg);
    o.hold(routine("a backup finished", 1), &cfg);
    o.hold(routine("another backup finished", 2), &cfg);
    assert_eq!(o.waiting(), 2);
    assert!(
        o.held.iter().any(|n| n.urgency == Urgency::Urgent),
        "the urgent one was dropped to make room for a routine one: {:?}",
        o.held
    );
}

#[test]
fn what_you_missed_leads_with_the_urgent_ones_by_name() {
    // Twenty titles is not something anyone reads on returning to a desk.
    let notes = vec![
        routine("a backup finished", 0),
        urgent("disk is nearly full", 0),
        routine("a note was saved", 0),
    ];
    let s = spoken(&notes);
    assert!(s.contains("disk is nearly full"), "the urgent one wasn't named: {s}");
    assert!(s.contains("2 other"), "the rest weren't counted: {s}");
}

#[test]
fn nothing_missed_says_nothing_at_all() {
    // An empty summary must be empty, not "0 things happened" — that is a
    // greeting nobody asked for.
    assert_eq!(spoken(&[]), "");
}

#[test]
fn only_routine_news_is_said_without_alarming_you() {
    let notes = vec![routine("a backup finished", 0), routine("a note was saved", 0)];
    let s = spoken(&notes);
    assert!(s.contains("nothing urgent"), "{s}");
}

// --- the wiring -------------------------------------------------------------

#[test]
fn the_daemon_can_actually_reach_you() {
    // The module being complete and tested is not the thing that matters.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "reach");
    let sent = d.reach_you(urgent("something", 100), 100);
    assert!(
        !matches!(sent, Sent::Failed(_)),
        "the daemon could not deliver by any route at all: {sent:?}"
    );
}

#[test]
fn a_note_that_could_not_be_delivered_is_waiting_rather_than_gone() {
    // Timestamps here are anchored to the real clock on purpose. `Daemon::new`
    // seeds `last_present` and `last_spoke` from `store::now()`, so a test
    // using small `t` values saturates to zero elapsed and can never look
    // away — the away path would silently never be exercised, and the test
    // would pass while proving nothing.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "hold");
    let away = atlas::store::now() + 100_000;
    let sent = d.reach_you(urgent("disk is nearly full", away), away);
    assert!(
        !sent.reached_you(),
        "the away route did not even try the notifier: {sent:?}"
    );
    assert_eq!(d.outbox.waiting(), 1, "an undeliverable note vanished: {sent:?}");
}

#[test]
fn with_no_notifier_configured_nothing_is_lost() {
    // The shipped config no longer sets a notification command: PowerShell was
    // removed because raising a toast through it flashes a console window and
    // puts a shell in the path of every alert. Until a replacement is chosen,
    // Atlas holds anything it cannot say and hands it over on return — which
    // must actually happen rather than being claimed.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "why");
    let away = atlas::store::now() + 100_000;
    let sent = d.reach_you(urgent("disk is nearly full", away), away);
    assert_eq!(sent, Sent::Held, "an undeliverable note did something else: {sent:?}");
    assert_eq!(d.outbox.waiting(), 1, "it was neither delivered nor kept");
}

#[test]
fn coming_back_hands_over_what_was_held() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "return");
    let cfgn = NotifyConfig::default();
    let now = atlas::store::now();
    d.outbox.hold(urgent("disk is nearly full", now), &cfgn);
    // A turn after a long absence is the moment of return.
    //
    // Deliberately a real command rather than "hello": a bare greeting is
    // handled before `run_command` and returns an empty string, so the brief
    // is prepared and simply not spoken on that turn. Worth knowing, and not
    // this test's subject.
    let reply = d.turn("what's outstanding", now + 100_000);
    assert!(
        reply.contains("disk is nearly full") || reply.contains("While you were away"),
        "held notes were not handed over on return: {reply}"
    );
    assert_eq!(d.outbox.waiting(), 0, "they were handed over and also kept");
}

#[test]
fn being_at_the_desk_speaks_rather_than_holding() {
    // The other half of the pair above. If this ever became a Hold, every
    // message would silently queue up instead of being said.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "present");
    let now = atlas::store::now();
    let sent = d.reach_you(urgent("something", now), now);
    assert_eq!(sent, Sent::Spoken, "a present user was not spoken to");
    assert_eq!(d.outbox.waiting(), 0, "it was said and also held");
}

#[test]
fn the_shipped_config_is_enabled_and_holds_rather_than_drops() {
    let c = cfg();
    let n = c.tools.as_ref().expect("tools.yaml loads").notify.clone();
    assert!(n.enabled, "notifications are switched off in the shipped config");
    assert!(n.max_held > 0, "a hold limit of zero drops everything immediately");
    assert!(n.stale_after_secs > 0, "everything would count as stale immediately");
}

#[test]
fn the_shipped_config_does_not_route_alerts_through_powershell() {
    // Asserted rather than left to a comment. A toast raised through
    // PowerShell flashes a console window on every alert.
    let c = cfg();
    let n = c.tools.as_ref().unwrap().notify.clone();
    if let Some(t) = n.tool {
        assert!(
            !t.command.to_lowercase().contains("powershell")
                && !t.command.to_lowercase().contains("pwsh"),
            "alerts are being raised through PowerShell again: {}",
            t.command
        );
    }
}

#[test]
fn doctor_reports_the_notifier_by_looking_for_the_binary_not_the_config() {
    // A configured command that is not installed is the same as no notifier.
    // Reporting it as fine because the config mentions it would be a check
    // that cannot fail — which is the failure this codebase keeps producing.
    let c = cfg();
    let p = plat();
    let findings = atlas::doctor::run(&c, c.tools.as_ref(), &p);
    let n = findings
        .iter()
        .find(|f| f.label == "notify")
        .expect("doctor never mentions whether Atlas can reach you off the desk");

    let installed = c
        .tools
        .as_ref()
        .and_then(|t| t.notify.tool.as_ref())
        .map(|t| t.available(&Default::default()))
        .unwrap_or(false);
    assert_eq!(
        n.ok, installed,
        "doctor's verdict disagrees with whether the command actually exists: {}",
        n.detail
    );
    if !n.ok {
        assert!(
            n.detail.contains("held") || n.detail.contains("switched off"),
            "it says it can't notify without saying what happens instead: {}",
            n.detail
        );
    }
}

// --- durability: "nothing is lost" has to actually be true -------------------
//
// `NO_NOTIFIER` and doctor both tell you that anything held while you are away
// is kept and said when you return. That claim was false twice over when this
// module was first written: the outbox was never saved, so a restart emptied
// it; and the summary was built by draining the queue at the start of a turn
// that could still end without saying anything.

#[test]
fn held_notes_survive_a_restart() {
    let c = cfg();
    let p = plat();
    let dir = tmp("restart");
    let cfgn = NotifyConfig::default();
    let now = atlas::store::now();
    {
        let mut d = Daemon::new(
            &c,
            &p,
            None,
            Store::new(dir.clone()),
            Proactive::new(ProactiveConfig::default()),
        );
        d.outbox.hold(urgent("disk is nearly full", now), &cfgn);
        d.persist();
    }
    // A second Daemon over the same store is what a restart looks like.
    let d2 = Daemon::new(&c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
    assert_eq!(
        d2.outbox.waiting(),
        1,
        "a restart emptied the outbox while doctor still claims nothing is lost"
    );
}

#[test]
fn a_turn_that_says_nothing_does_not_consume_what_was_waiting() {
    // `addressing` can decide the words were not meant for Atlas and end the
    // turn with an empty reply. Anything waiting must still be waiting.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "notconsumed");
    let cfgn = NotifyConfig::default();
    let now = atlas::store::now();
    d.outbox.hold(urgent("disk is nearly full", now), &cfgn);

    let reply = d.turn("hello", now + 100_000);
    if reply.is_empty() {
        assert_eq!(
            d.outbox.waiting(),
            1,
            "a turn that said nothing still emptied the outbox"
        );
    }
}

#[test]
fn peeking_does_not_remove_anything() {
    let cfg = NotifyConfig::default();
    let mut o = Outbox::default();
    o.hold(urgent("disk is nearly full", 0), &cfg);
    let seen = o.ready(HOUR, &cfg);
    assert_eq!(seen.len(), 1);
    assert_eq!(o.waiting(), 1, "peeking consumed it");
}

#[test]
fn peek_and_collect_agree_on_what_is_worth_saying() {
    // If these ever disagreed, you would be told about one set of things and
    // a different set would be cleared.
    let cfg = NotifyConfig { stale_after_secs: HOUR, ..Default::default() };
    let mut o = Outbox::default();
    o.hold(routine("old routine", 0), &cfg);
    o.hold(urgent("old urgent", 0), &cfg);
    o.hold(routine("fresh routine", 5 * HOUR), &cfg);
    let peeked = o.ready(5 * HOUR, &cfg);
    let collected = o.collect(5 * HOUR, &cfg);
    assert_eq!(peeked, collected);
}

// --- speaking out loud, or not ----------------------------------------------
//
// Eric's rules, which solve what presence could not. Atlas cannot tell who is
// in the room and never will reliably. What it *can* answer is whether this
// can be overheard by anyone but you — and headphones settle that anywhere.

use atlas::audio::{Device, Kind};
use atlas::notify::{how_to_say, on_a_call, Quiet, Say};

fn out(name: &str) -> Device {
    Device::new(name, Kind::Output)
}

#[test]
fn headphones_mean_a_busy_room_stops_mattering() {
    // The coffee-shop case, answered without knowing anything about the room.
    let devs = vec![out("AirPods Pro")];
    assert_eq!(
        how_to_say(Some(&devs), false, true),
        Say::InYourEar("AirPods Pro".into()),
        "in public with headphones on, Atlas should still speak"
    );
}

#[test]
fn at_the_desk_with_no_headphones_it_speaks_aloud() {
    let devs = vec![out("Speakers (Realtek High Definition Audio)")];
    match how_to_say(Some(&devs), false, false) {
        Say::Aloud(d) => assert!(d.contains("Speakers"), "{d}"),
        other => panic!("expected out loud at the desk, got {other:?}"),
    }
}

#[test]
fn in_public_with_no_headphones_it_shows_rather_than_says() {
    let devs = vec![out("Speakers (Realtek High Definition Audio)")];
    assert_eq!(how_to_say(Some(&devs), false, true), Say::Silent(Quiet::InPublic));
}

#[test]
fn a_call_beats_everything_including_headphones() {
    // Talking over a call is the one unforgivable interruption, and headphones
    // make it worse rather than better — it lands in the middle of what you
    // are listening to.
    let devs = vec![out("AirPods Pro")];
    assert_eq!(how_to_say(Some(&devs), true, false), Say::Silent(Quiet::OnACall));
}

#[test]
fn never_having_looked_at_the_audio_is_not_the_same_as_finding_none() {
    // `None` means Atlas has not probed yet. Treating that as "nothing can
    // play sound" would silence it on every machine that has not run voice
    // mode — an absence of a finding read as a finding.
    match how_to_say(None, false, false) {
        Say::Aloud(_) => {}
        other => panic!("an unprobed machine was silenced: {other:?}"),
    }
    // Probed and genuinely empty is a real finding, and stays one.
    assert_eq!(how_to_say(Some(&[]), false, false), Say::Silent(Quiet::NoOutput));
}

#[test]
fn every_reason_for_going_quiet_can_be_said_out_loud() {
    // "It went quiet" with no explanation is what makes an assistant feel
    // broken.
    for q in [Quiet::OnACall, Quiet::InPublic, Quiet::NoOutput] {
        assert!(!q.plain().is_empty(), "{q:?} has no explanation");
    }
}

#[test]
fn a_meeting_in_a_browser_tab_counts_as_a_call() {
    // The gap a process-name list alone would leave: the meeting is a tab, so
    // the process is just the browser. The title is what gives it away.
    use atlas::platform::ActiveWindow;
    let w = ActiveWindow {
        process: "chrome.exe".into(),
        title: "Weekly sync - Google Meet".into(),
    };
    assert!(on_a_call(Some(&w)), "a meeting in a tab was missed");
}

#[test]
fn ordinary_work_is_not_mistaken_for_a_call() {
    use atlas::platform::ActiveWindow;
    let w = ActiveWindow { process: "code.exe".into(), title: "notify.rs - atlas".into() };
    assert!(!on_a_call(Some(&w)), "editing a file was read as being on a call");
    assert!(!on_a_call(None), "no foreground window was read as being on a call");
}

#[test]
fn a_listing_of_microphones_only_is_not_a_machine_without_speakers() {
    // 29 Sep 2026, Eric's laptop: Windows' device listing (dshow) names
    // microphones and never speakers, so every Windows machine read as
    // "nothing here can play sound" and no notification was ever said.
    use atlas::audio::{Device, Kind};
    let devs = vec![
        Device::new("Microphone Array (Intel\u{ae} Smart Sound Technology for Digital Microphones)", Kind::Input),
        Device::new("Microphone (HD Pro Webcam C920)", Kind::Input),
    ];
    match how_to_say(Some(&devs), false, false) {
        Say::Aloud(_) => {}
        other => panic!("a microphones-only listing silenced Atlas: {other:?}"),
    }
}
