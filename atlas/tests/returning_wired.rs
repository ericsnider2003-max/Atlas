//! `returning.rs`, actually reached from the running program.
//!
//! `tests/returning_otherside.rs` tests the module's own reasoning against
//! hand-built `Happened` lists. This file tests the other half: that a real
//! `Daemon`, after a real absence, builds that list from what actually
//! accumulated and says what `welcome()` decided — including the part that
//! only exists once something calls it, which is what "yes" does to an
//! offered brief.
//!
//! Before this, `welcome()` and `full_brief()` were complete, tested and
//! unreachable: the away path built a pre-formatted string from
//! `Journal::brief` and `notify::spoken`, so nothing in the program ever had
//! a `[Happened]` to hand them.

use atlas::activity::Kind;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::notify::{Note, NotifyConfig, Urgency};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const OVERNIGHT: u64 = 40_000;
/// Long enough to count as away, short enough to be `Gone::HalfDay`.
const HALF_DAY: u64 = 20_000;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-rw-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

/// A real command rather than a greeting: a bare "hello" is answered before
/// `run_command` and returns an empty string, so the brief would be prepared
/// and simply not spoken on that turn. Same trap `tests/notify.rs` documents.
const A_REAL_COMMAND: &str = "what's outstanding";

// ================= the list is built from what actually happened =========

#[test]
fn a_quiet_overnight_still_gets_its_one_line() {
    // `welcome()`'s own floor: a quiet five minutes deserves nothing, but a
    // quiet overnight deserves a line, or a brief that is always silent
    // stops meaning anything. This used to be reimplemented inline in
    // `daemon.rs`; now it is the module's decision and this proves it still
    // arrives.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "quiet");
    let now = atlas::store::now();
    let said = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    assert!(said.contains("Nothing needed you"), "got: {said}");
}

#[test]
fn a_quiet_half_day_says_nothing_at_all() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "quiet-half");
    let now = atlas::store::now();
    let said = d.turn(A_REAL_COMMAND, now + HALF_DAY);
    assert!(
        !said.contains("Nothing needed you") && !said.contains("While you were away"),
        "a quiet half-day should pass without comment: {said}"
    );
}

#[test]
fn something_that_needs_you_is_said_straight_away_not_offered() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "blocked");
    let now = atlas::store::now();
    // `ok: true` on purpose. A `Blocked` entry is Atlas filing something it
    // could not do, which is not the same as the attempt failing — and with
    // `ok: false` the `failed` flag would carry this test on its own, so
    // `needs_you` would never be the thing under test.
    d.journal.record_at(Kind::Blocked, "the certificate renewal needs a card", true, now + 10);

    let said = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    assert!(said.contains("certificate renewal"), "not named: {said}");
    assert!(
        !said.contains("when you're ready") && !said.contains("when you want"),
        "something waiting on you was offered rather than said: {said}"
    );
}

#[test]
fn ordinary_updates_are_offered_rather_than_recited() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "offer");
    let now = atlas::store::now();
    d.journal.record_at(Kind::Scheduled, "the index rebuild finished", true, now + 10);
    d.journal.record_at(Kind::Scheduled, "the nightly backup finished", true, now + 20);

    let said = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    assert!(
        said.contains("when you want") || said.contains("when you're ready"),
        "two ordinary finishes should be offered, not recited: {said}"
    );
}

#[test]
fn upkeep_never_reaches_the_brief() {
    // You do not need to hear about log rotation.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "upkeep");
    let now = atlas::store::now();
    d.journal.record_at(Kind::Upkeep, "rotated the logs", true, now + 10);

    let said = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    assert!(!said.contains("rotated the logs"), "upkeep was read out: {said}");
    // And with nothing else, the absence reads as quiet rather than busy.
    assert!(said.contains("Nothing needed you"), "got: {said}");
}

#[test]
fn your_own_instructions_are_never_read_back_to_you_as_news() {
    // Every turn records what you said as `Kind::Asked`. `Journal::brief`
    // never surfaced those, because it picked its three kinds out one at a
    // time — so when the brief was rebuilt around a list that excluded only
    // `Upkeep`, `Asked` walked in, and coming back produced "I've got an
    // update on call me boss when you're ready". You were there. You said it.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "asked");
    let now = atlas::store::now();
    // A command the offline parser handles cleanly. An unparseable one puts
    // `wanted`'s "solutions or just listening?" question in front of the next
    // turn, which is a different mechanism entirely and would make this test
    // about that instead.
    d.turn(A_REAL_COMMAND, now);

    let said = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    // Asserted as "the absence reads as quiet", not as "that exact sentence
    // is absent". An offer names only the first few words of a subject
    // ("I've got an update on remind me to when you're ready"), so looking
    // for the whole instruction back would have missed the bug it is here
    // to catch — it did, on the first attempt.
    assert!(
        said.contains("Nothing needed you"),
        "an absence in which you only ever spoke should read as quiet: {said}"
    );
}

#[test]
fn an_offer_that_did_not_reach_you_is_reported_once_not_twice() {
    // An undeliverable offer is written to the journal as `Offered` *and*
    // held in the outbox. Only the outbox is read, or the same thing is said
    // twice in one breath.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "twice");
    let now = atlas::store::now();
    d.journal.record_at(Kind::Offered, "shall I renew the certificate", false, now + 5);
    d.outbox.hold(
        Note::new("shall I renew the certificate", "", Urgency::Urgent, now + 5),
        &NotifyConfig::default(),
    );

    let said = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    assert_eq!(
        said.matches("renew the certificate").count(),
        1,
        "said more than once: {said}"
    );
}

#[test]
fn a_held_notification_and_a_journal_entry_arrive_as_one_absence() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "both");
    let now = atlas::store::now();
    d.outbox.hold(
        Note::new("the disk is nearly full", "", Urgency::Urgent, now + 5),
        &NotifyConfig::default(),
    );
    d.journal.record_at(Kind::Blocked, "the invoice needs approving", false, now + 10);

    let said = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    assert!(said.contains("disk is nearly full"), "held note missing: {said}");
    assert!(said.contains("invoice needs approving"), "journal entry missing: {said}");
    assert_eq!(d.outbox.waiting(), 0, "handed over and also kept");
}

#[test]
fn a_held_note_is_still_said_when_the_absence_is_too_short_to_count() {
    // `away_after` is configurable below fifteen minutes, at which point the
    // daemon calls it an absence and `returning` calls it `Gone::Moment` and
    // says nothing. The journal can wait; something held back *because* you
    // were not there cannot, or "held" is just a nicer word for dropped.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "short");
    d.away_after = 60;
    let now = atlas::store::now();
    d.outbox.hold(
        Note::new("the build failed", "", Urgency::Urgent, now + 5),
        &NotifyConfig::default(),
    );

    let said = d.turn(A_REAL_COMMAND, now + 120);
    assert!(said.contains("build failed"), "a held note was dropped: {said}");
}

// ================= saying yes to an offer =================

#[test]
fn saying_yes_to_an_offered_brief_gets_the_whole_list() {
    // `full_brief()`'s first real caller. Without this the offer was a
    // promise nothing kept: "updates when you want them", and then no way to
    // want them.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "yes");
    let now = atlas::store::now();
    d.journal.record_at(Kind::Scheduled, "the index rebuild finished", true, now + 10);
    d.journal.record_at(Kind::Scheduled, "the nightly backup finished", true, now + 20);

    let offer = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    assert!(offer.contains("when you want") || offer.contains("when you're ready"), "{offer}");

    let full = d.turn("yes", now + OVERNIGHT + 5);
    assert!(full.contains("index rebuild finished"), "{full}");
    assert!(full.contains("nightly backup finished"), "{full}");
}

#[test]
fn saying_no_to_an_offered_brief_leaves_it() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "no");
    let now = atlas::store::now();
    d.journal.record_at(Kind::Scheduled, "the index rebuild finished", true, now + 10);
    d.journal.record_at(Kind::Scheduled, "the nightly backup finished", true, now + 20);

    d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    let said = d.turn("no", now + OVERNIGHT + 5);
    assert!(!said.contains("index rebuild"), "declined and read out anyway: {said}");
}

#[test]
fn the_offer_does_not_hijack_your_next_instruction() {
    // The reason this is not routed through `Pending::Clarification` like
    // every other outstanding question: that machinery treats your next
    // utterance as the answer, and "when you're ready" promised the
    // opposite. You may well have come back to do something specific.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "hijack");
    let now = atlas::store::now();
    d.journal.record_at(Kind::Scheduled, "the index rebuild finished", true, now + 10);
    d.journal.record_at(Kind::Scheduled, "the nightly backup finished", true, now + 20);

    d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    let next = d.turn("what's outstanding", now + OVERNIGHT + 5);
    assert!(!next.contains("index rebuild finished"), "the offer ate the instruction: {next}");

    // And having moved on, the offer is gone rather than lying in wait for
    // an unrelated yes later.
    let later = d.turn("yes", now + OVERNIGHT + 10);
    assert_eq!(later, "Nothing to confirm.", "a stale brief answered a later yes: {later}");
}

#[test]
fn a_yes_on_the_very_turn_of_the_offer_does_not_answer_it() {
    // Both halves are set on the same turn. Without the check that the offer
    // was actually delivered, saying "yes" as the first thing after an
    // absence would answer an offer you had not heard, and leave the offer
    // itself queued to surface later against nothing.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "sameturn");
    let now = atlas::store::now();
    d.journal.record_at(Kind::Scheduled, "the index rebuild finished", true, now + 10);
    d.journal.record_at(Kind::Scheduled, "the nightly backup finished", true, now + 20);

    let said = d.turn("yes", now + OVERNIGHT);
    assert!(
        !said.contains("index rebuild finished") || said.contains("when you"),
        "a yes answered an offer that had not been made yet: {said}"
    );
}

// ================= how it addresses you =================

#[test]
fn a_spoken_address_change_is_used_by_the_next_brief() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "address");
    let now = atlas::store::now();
    d.turn("call me boss", now);
    let said = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    // The greeting specifically, not the word anywhere in the reply. The
    // first version of this asserted `contains("boss")` and passed with the
    // address wiring deliberately disabled — because the journal was reciting
    // the words "call me boss" straight back as an update, so the substring
    // was there for entirely the wrong reason. That bug is fixed; this
    // assertion is narrowed so it could not have hidden it.
    assert!(
        said.contains(", boss."),
        "the address it confirmed was not used in the greeting: {said}"
    );
    assert!(!said.contains("call me boss"), "your own instruction was read back: {said}");
}

#[test]
fn with_no_address_set_the_brief_still_says_it_was_while_you_were_away() {
    // The default address is nothing at all, on purpose. Without a greeting
    // to carry it, "Disk is nearly full." is indistinguishable from
    // something happening right now -- and that distinction is the entire
    // point of a returning brief.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "unaddressed");
    let now = atlas::store::now();
    d.journal.record_at(Kind::Blocked, "the invoice needs approving", false, now + 10);

    let said = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    assert!(said.contains("While you were away"), "no framing at all: {said}");
}

#[test]
fn stopping_it_calling_you_that_beats_the_config_file() {
    // `Address::None` is both "never asked" and what "stop calling me that"
    // deliberately writes. Reading the value alone cannot tell them apart,
    // which is why the daemon checks whether the file exists instead.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "stopit");
    let now = atlas::store::now();
    d.turn("call me boss", now);
    d.turn("stop calling me that", now + 1);
    let said = d.turn(A_REAL_COMMAND, now + OVERNIGHT);
    assert!(!said.contains("boss"), "an address it was told to drop came back: {said}");
}
