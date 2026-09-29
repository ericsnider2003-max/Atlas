//! Reading the messages also builds the contact book.
//!
//! `messaging::note_on` was written, tested and unreachable: it turns a
//! sender's messages into a durable note (name, platform, the folder they
//! belong in, what they first wanted), and nothing in the tree ever called
//! it. The daemon's "messages" reading spoke a summary and threw the senders
//! away, so a brand's first approach vanished into a count of "3 messages"
//! and Atlas remembered no one between reads.
//!
//! Now the same reading updates the notes: each sender becomes a `note_on`
//! entry, merged into the stored book so it accumulates, and a *new* work or
//! prospect contact is said out loud. This drives that end to end through the
//! daemon and checks the note is both surfaced and kept.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::messaging::{self, Folder, Message, Person, Platform};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-note-on-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg_with_messaging() -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let tools = c.tools.as_mut().expect("the shipped config has a tools section");
    tools.messaging.enabled = true;
    tools.messaging.your_names = vec!["you".into()];
    c
}

fn msg(id: &str, from: &str, text: &str, at: u64) -> Message {
    Message {
        id: id.into(),
        platform: Platform::Telegram,
        from: from.into(),
        group: None,
        text: text.into(),
        at,
        mentions_you: false,
    }
}

#[test]
fn a_new_work_contact_is_named_and_kept_when_the_messages_are_read() {
    let dir = tmp("named-and-kept");
    // Two business messages from one sender -- a working relationship, so
    // `folder_for` files them under Work rather than a one-off Prospect.
    let kept = vec![
        msg("1", "Dana", "we'd love to work with you -- sending a brief and a rate card", 1_700_000_000),
        msg("2", "Dana", "here's the contract and the invoice for the campaign", 1_700_000_100),
        msg("3", "five a side", "anyone about saturday", 1_700_000_200),
    ];
    // Seed into the very store the daemon will load from.
    Store::new(dir.clone()).save(atlas::telegram::KEPT, &kept).unwrap();

    let c = cfg_with_messaging();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));

    let said = d.execute(&Intent::Mail("any new messages".into()));

    // The line that only the note-building path produces: a fresh work
    // contact, said rather than buried in the summary count.
    // Eric, 25 Sep 2026 (F3): said as what they want, with an offer.
    assert!(said.contains("It looks like Dana wants to know about"), "the new contact was not surfaced: {said}");
    assert!(said.contains("Would you like me to look for the answer and respond?"), "{said}");
    let offer = d.pending_offer().expect("the offer is waiting on a yes");
    assert!(offer.command.starts_with("research "), "{}", offer.command);

    // And it was kept -- the book grows from the read, it isn't spoken and
    // discarded.
    let book: Vec<Person> = Store::new(dir).load(messaging::PEOPLE);
    let dana = book.iter().find(|pp| pp.name == "Dana").expect("Dana was not kept");
    assert_eq!(dana.folder, Folder::Work);
    assert_eq!(dana.platform, Platform::Telegram);
    assert!(dana.messages >= 2, "the note undercounted her messages: {}", dana.messages);
}

#[test]
fn a_contact_already_known_as_work_is_not_announced_again() {
    let dir = tmp("not-twice");
    let kept = vec![msg("9", "Priya", "following up on the partnership and the rate card", 1_700_000_000)];
    Store::new(dir.clone()).save(atlas::telegram::KEPT, &kept).unwrap();
    // Pre-file Priya as a prospect already on the books.
    let known = vec![Person {
        name: "Priya".into(),
        platform: Platform::Telegram,
        folder: Folder::Prospect,
        first_about: "hello".into(),
        first_at: 1_699_000_000,
        last_at: 1_699_000_000,
        messages: 1,
        about: None,
    }];
    Store::new(dir.clone()).save(messaging::PEOPLE, &known).unwrap();

    let c = cfg_with_messaging();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));

    let said = d.execute(&Intent::Mail("check my messages".into()));
    // Already known as work, so no fresh-contact announcement -- only news is
    // said, not a re-read of the same person.
    assert!(!said.contains("It looks like Dana"), "an already-known contact was announced as new: {said}");

    // But the note was still updated, not left behind.
    let book: Vec<Person> = Store::new(dir).load(messaging::PEOPLE);
    let priya = book.iter().find(|pp| pp.name == "Priya").expect("Priya was dropped");
    assert!(priya.messages >= 2, "her note wasn't updated: {}", priya.messages);
}
