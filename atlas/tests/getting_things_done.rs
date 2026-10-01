//! The getting-things-done sweep (30 Sep 2026): notes kept and found again,
//! drafts sent, research write-ups reachable, documents written, messages
//! for you actually said, and settings never wiped by a file that won't read.

use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-gtd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn cfg() -> atlas::config::Config {
    atlas::config::Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}
const NOW: u64 = 1_790_776_800;

#[test]
fn a_note_is_kept_and_found_again() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("notes")), Proactive::new(ProactiveConfig::default()));
    let kept = d.turn("note that the broker fee is 25 dollars a month", NOW);
    assert!(!kept.to_lowercase().contains("couldn't"), "{kept}");
    let found = d.turn("where's that note about the broker fee", NOW + 60);
    assert!(found.contains("25 dollars"), "{found}");
    assert!(d.turn("find my note about the gym", NOW + 70).starts_with("I can't find a note like that"));
}

#[test]
fn a_capture_that_reads_as_a_whole_item_is_kept_too() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("made")), Proactive::new(ProactiveConfig::default()));
    let before = d.notebook.notes.len();
    let said = d.turn("note this: record a video called Tuesday tips, due Friday", NOW);
    assert!(d.notebook.notes.len() > before, "announced ({said}) and kept");
}

#[test]
fn drafts_are_listed_and_sending_says_what_it_needs() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("drafts")), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d.turn("what drafts are waiting", NOW), "No drafts waiting.");
    assert_eq!(d.turn("send the reply to Jane", NOW), "I don't have a draft waiting for jane.");
}

#[test]
fn duckduckgo_results_are_read_through_its_redirect() {
    let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fsolar%2Dpanels&amp;rut=abc">Solar</a>
<a href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fen.wikipedia.org%2Fwiki%2FSolar_panel&amp;rut=def">Wiki</a>"#;
    let urls = atlas::research::extract_urls(html, 5);
    assert_eq!(urls, vec!["https://example.org/solar-panels".to_string(), "https://en.wikipedia.org/wiki/Solar_panel".to_string()]);
}

#[test]
fn the_full_research_write_up_can_be_heard() {
    let (mut c, p) = (cfg(), plat());
    let notes = tmp("research-notes");
    c.tools.as_mut().unwrap().research.notes_dir = notes.display().to_string();
    std::fs::write(notes.join("1790000000-solar.md"), "# solar\n\nPanels lose about half a percent a year.\n\n## Sources\n- https://example.org\n").unwrap();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("research")), Proactive::new(ProactiveConfig::default()));
    let heard = d.turn("read me the full brief", NOW);
    assert_eq!(heard, "Panels lose about half a percent a year.", "the write-up, without its title or source list");
}

#[test]
fn a_letter_asked_for_is_written_not_built() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("writing")), Proactive::new(ProactiveConfig::default()));
    // No model here: it says what it needs, rather than building a program.
    let said = d.turn("write me a letter to my landlord about the heating", NOW);
    assert_eq!(said, "I need a language model to write that, and I haven't got one yet.");
    assert!(matches!(d.parser.parse("write me a letter to my landlord"), atlas::intent::Intent::Unknown(_)), "not a program to build");
}

#[test]
fn a_note_routed_to_speaking_is_said() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("speak")), Proactive::new(ProactiveConfig::default()));
    d.to_say_aloud.push("Your research on solar is ready.".into());
    let out = d.tick(NOW);
    assert_eq!(out.iter().filter(|l| l.as_str() == "Your research on solar is ready.").count(), 1, "said once: {out:?}");
    // 30 Sep 2026: was `is_empty()`; the same tick can queue something new
    // of its own for the next pass. What matters is this one was taken.
    assert!(!d.to_say_aloud.iter().any(|l| l.contains("research on solar")), "said and still waiting: {:?}", d.to_say_aloud);
}

#[test]
fn a_settings_file_that_wont_read_is_not_overwritten() {
    let dir = tmp("prefs");
    for f in std::fs::read_dir("config").unwrap().flatten() {
        if f.path().is_file() {
            std::fs::copy(f.path(), dir.join(f.file_name())).unwrap();
        }
    }
    let prefs = atlas::preferences::Preferences::file(&dir);
    std::fs::write(&prefs, "wake.enabled: [unclosed\n").unwrap();
    let said = atlas::settingswin::keep_setting(&dir, "wake.enabled", "off");
    let err = said.expect_err("refused");
    assert!(err.starts_with("I haven't changed anything"), "{err}");
    assert_eq!(std::fs::read_to_string(&prefs).unwrap(), "wake.enabled: [unclosed\n", "left as it was");
}

// ---- email, started by asking (30 Sep 2026) ----

#[test]
fn an_email_asked_for_is_read_as_who_and_what() {
    use atlas::outbox::email_asked;
    assert_eq!(email_asked("Email Sam saying I'll be late"), Some(("Sam".into(), "I'll be late".into())));
    assert_eq!(email_asked("send an email to jo@example.com that the deck is ready."), Some(("jo@example.com".into(), "the deck is ready".into())));
    assert_eq!(email_asked("shoot Priya an email saying thanks for today"), Some(("Priya".into(), "thanks for today".into())));
    assert_eq!(email_asked("email him saying hello there"), None, "nobody named");
    assert_eq!(email_asked("email is broken on my phone"), None, "not a request");
    assert_eq!(email_asked("check my email"), None);
    assert_eq!(atlas::outbox::body_from_spoken("i'll be there by 6 and i can bring the files"), "I'll be there by 6 and I can bring the files.");
    assert_eq!(atlas::outbox::subject_from_body("Running late."), "Running late");
    assert_eq!(atlas::outbox::subject_from_body("The deck for Thursday is ready to look at now."), "The deck for Thursday is ready to...");
}

#[test]
fn an_email_asked_for_is_drafted_and_held_until_you_say_send() {
    let (mut c, p) = (cfg(), plat());
    let mut d0 = Daemon::new(&c, &p, None, Store::new(tmp("email-none")), Proactive::new(ProactiveConfig::default()));
    assert_eq!(
        d0.turn("email Sam saying I'll be late", NOW),
        "I can't email Sam yet -- there's no mail account set up. Add one in Settings, under Mail, and ask me again."
    );
    drop(d0);
    c.tools.as_mut().unwrap().mail.accounts = vec![atlas::mail::Account {
        name: "personal".into(),
        address: "me@example.com".into(),
        password_from_vault: "mail personal".into(),
        ..Default::default()
    }];
    let store = Store::new(tmp("email"));
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
    assert!(matches!(d.parser.parse("email Sam saying I'll be late"), atlas::intent::Intent::Unknown(_)), "no command takes it first");
    assert_eq!(
        d.turn("email Sam saying I'll be late", NOW),
        "I don't have an email address for Sam. Tell me \"Sam's email is\" and the address, then ask again."
    );
    d.turn("Sam's email is sam@example.com", NOW + 10);
    let said = d.turn("email Sam saying i'll be late", NOW + 20);
    assert_eq!(said, "To Sam (sam@example.com): \"I'll be late.\" Say \"send it\" and it goes, or \"scrap the draft to Sam\".");
    let outbox = atlas::outbox::Outbox::load(&store);
    let held = outbox.waiting_for("sam@example.com").expect("held, not sent");
    assert_eq!((held.subject.as_str(), held.body.as_str()), ("I'll be late", "I'll be late."));
    assert_eq!(d.turn("what drafts are waiting", NOW + 30), "Waiting: a reply to Sam.");
}

/// 30 Sep 2026: a push to the phone that failed waited for the desk; it's
/// tried again while you're still away, and once it lands it isn't said a
/// second time when you're back.
#[test]
fn a_push_that_failed_is_tried_again_while_you_are_away() {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (mut c, p) = (cfg(), plat());
    c.tools.as_mut().unwrap().phone = atlas::phone::PhoneConfig {
        enabled: true,
        host: format!("127.0.0.1:{port}"),
        path: "/atlas".into(),
        timeout_secs: 3,
        ..Default::default()
    };
    *p.input_idle.borrow_mut() = Some(4 * 3600);
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("phone-retry")), Proactive::new(ProactiveConfig::default()));
    // Hours after the daemon last heard or said anything: away.
    let now = atlas::store::now() + 10 * 3600;
    let note = atlas::notify::Note::new("Build finished", "all green", atlas::notify::Urgency::Routine, NOW);
    d.outbox.held.push(note.clone());
    d.phone_to_retry.push(note);
    d.phone_retry_at = now + 300;
    d.retry_phone(now + 10);
    assert_eq!(d.phone_to_retry.len(), 1, "not before its time");
    l.set_nonblocking(true).unwrap();
    let got = std::thread::spawn(move || {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut s = loop {
            match l.accept() {
                Ok((s, _)) => break s,
                Err(_) if std::time::Instant::now() < until => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(_) => return String::new(),
            }
        };
        s.set_nonblocking(false).unwrap();
        let mut buf = [0u8; 2048];
        let n = s.read(&mut buf).unwrap();
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").unwrap();
        String::from_utf8_lossy(&buf[..n]).to_string()
    });
    d.retry_phone(now + 400);
    assert!(got.join().unwrap().contains("Build finished"), "the retry didn't reach the phone");
    assert!(d.phone_to_retry.is_empty(), "delivered, so no more tries");
    assert!(d.outbox.held.is_empty(), "delivered, so not said again at the desk");
}

/// 30 Sep 2026: the Status page said "Listening: Yes." whenever Atlas wasn't
/// paused, microphone or no microphone.
#[test]
fn the_status_page_says_what_is_actually_listening() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("listening")), Proactive::new(ProactiveConfig::default()));
    let r = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Status));
    assert_eq!(r.status, 200);
    assert_eq!(r.body.matches("the microphone isn").count(), 1, "the Listening line, once");
    assert!(r.body.contains("No — the microphone isn&#39;t running, so type to me here.") || r.body.contains("No — the microphone isn't running, so type to me here."), "{}", r.body);
    assert!(!r.body.contains(">Yes.<"), "claimed to be listening with no microphone");
}

/// 30 Sep 2026: on the phone the background's lines (a reminder going off)
/// were dropped; they're kept, numbered, in /hub/live.json for the app.
#[test]
fn what_the_background_says_reaches_the_phone_app() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("said-apps")), Proactive::new(ProactiveConfig::default()));
    d.keep_said_for_apps(vec!["Reminder: call the dentist".into(), " ".into()]);
    d.keep_said_for_apps((0..30).map(|i| format!("line {i}")).collect());
    assert_eq!(d.said_for_apps.len(), atlas::daemon::SAID_FOR_APPS_KEPT, "bounded");
    assert_eq!(d.said_for_apps.last().unwrap(), &(31, "line 29".to_string()), "numbered on from the last");
    let r = atlas::hublive::reply(&mut d, atlas::server::Action::LiveJson);
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    let said = v["said"].as_array().expect("live.json carries what was said");
    assert_eq!(said.last().unwrap()["text"], "line 29");
    assert_eq!(said.last().unwrap()["id"], 31);
}

/// 30 Sep 2026: the day's brief went off on the tick at the first hour it
/// was allowed -- you asleep, the room empty -- and was marked given. It
/// waits until you're here.
#[test]
fn the_days_brief_waits_until_you_are_here() {
    let (c, p) = (cfg(), plat());
    let store = Store::new(tmp("brief-here"));
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
    // Ten in the morning, hours after anything was said.
    let mut t = atlas::store::now() + 10 * 3600;
    while atlas::localclock::hour_here(t) != 10 {
        t += 3600;
    }
    *p.input_idle.borrow_mut() = Some(3 * 3600);
    d.tick(t);
    assert_eq!(store.load::<u64>("last_brief_at"), 0, "given to an empty room");
    // You sit down.
    *p.input_idle.borrow_mut() = Some(5);
    d.tick(t + 60);
    assert_eq!(store.load::<u64>("last_brief_at"), t + 60, "you're here and it's a new day");
}

/// 30 Sep 2026: "how's that going?" had no answer; the model guessed.
#[test]
fn hows_that_going_is_answered_from_what_is_actually_running() {
    use atlas::mind::asks_how_its_going;
    assert!(asks_how_its_going("How's that going?"));
    assert!(asks_how_its_going("is it done yet"));
    assert!(asks_how_its_going("how's the research coming along"));
    assert!(!asks_how_its_going("how's the weather"), "not about Atlas's work");
    assert!(!asks_how_its_going("how is your day"));
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("progress")), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d.turn("how's that going?", NOW), "Nothing's running, and nothing has run yet this session.");
    let t = atlas::store::now();
    let id = d.long_work.watch("research", "crew", t.saturating_sub(600));
    d.long_work.update(id, atlas::watching::Outcome::Failed, "every source failed to fetch", t.saturating_sub(120));
    assert_eq!(
        d.turn("is it done yet", NOW),
        "Nothing's running now. The last thing, research, failed 2 minutes ago: every source failed to fetch."
    );
}

/// 30 Sep 2026: failures were read aloud with their machinery in them.
/// The screen keeps it; the speaker says it plainly.
#[test]
fn technical_detail_stays_on_the_screen_not_in_the_ear() {
    use atlas::spoken_form::without_technical_detail as plain;
    assert_eq!(
        plain("The reply to Sam didn't go: connection refused (os error 10061). It's still waiting."),
        "The reply to Sam didn't go: connection refused. It's still waiting."
    );
    assert_eq!(plain("I couldn't open C:\\Users\\erics\\Atlas\\notes\\lease.pdf."), "I couldn't open lease.pdf.");
    assert_eq!(plain("Saved from https://www.example.org/solar/panels today."), "Saved from example.org today.");
    assert_eq!(
        plain("The build failed: E0599 at src/x.rs:12 ECONNREFUSED. Want me to try again?"),
        "The build failed -- the details are on the screen. Want me to try again?"
    );
    // Ordinary words, numbers and money are left alone.
    assert_eq!(plain("It came to $2,350 (about 12% more) in 2026."), "It came to $2,350 (about 12% more) in 2026.");
    assert_eq!(plain("Call Sam (he said after 5)."), "Call Sam (he said after 5).");
}

/// 30 Sep 2026: the unsubscribe report couldn't be acted on.
#[test]
fn the_unsubscribe_report_can_be_acted_on() {
    assert!(atlas::unsub::go_ahead("Unsubscribe from those."));
    assert!(atlas::unsub::go_ahead("go ahead and unsubscribe"));
    assert!(!atlas::unsub::go_ahead("unsubscribe from what I don't read"), "that's the look, not the go-ahead");
    let (mut c, p) = (cfg(), plat());
    c.tools.as_mut().unwrap().mail.enabled = true;
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("unsub")), Proactive::new(ProactiveConfig::default()));
    assert_eq!(
        d.turn("unsubscribe from those", NOW),
        "There's nothing lined up to unsubscribe from. Ask me to clear out your email and I'll look first."
    );
}

/// 30 Sep 2026: connecting Outlook ended with "add this under mail.accounts
/// in tools.yaml". An account Atlas connected is now one of your accounts.
#[test]
fn an_account_atlas_connected_is_used_without_editing_a_file() {
    let (c, p) = (cfg(), plat());
    let store = Store::new(tmp("connected-mail"));
    let acct = atlas::mail::Account {
        name: "me".into(),
        address: "me@outlook.com".into(),
        password_from_vault: "outlook me@outlook.com".into(),
        oauth: true,
        client_id: "abc-123".into(),
        ..Default::default()
    };
    store.save(atlas::daemon::CONNECTED_ACCOUNTS, &vec![acct.clone()]).unwrap();
    let d = Daemon::new(&c, &p, None, store, Proactive::new(ProactiveConfig::default()));
    let accounts = d.tools_cfg().mail.accounts.clone();
    assert_eq!(accounts.iter().filter(|a| a.address == "me@outlook.com").count(), 1);
    assert_eq!(accounts.iter().find(|a| a.address == "me@outlook.com"), Some(&acct));
}

/// 30 Sep 2026: an installed copy asked to work on itself ran cargo in
/// whatever folder it was started from.
#[test]
fn only_a_source_checkout_is_worked_on() {
    assert!(atlas::selfwork::is_a_source_checkout(Path::new(".")), "this checkout is Atlas's source");
    let elsewhere = tmp("not-source");
    assert!(!atlas::selfwork::is_a_source_checkout(&elsewhere));
    std::fs::write(elsewhere.join("Cargo.toml"), "[package]\nname = \"something-else\"\n").unwrap();
    assert!(!atlas::selfwork::is_a_source_checkout(&elsewhere), "another project isn't Atlas");
}

/// 30 Sep 2026: anticipation was handed midnight-on-a-Monday every tick.
#[test]
fn anticipation_reads_the_real_clock() {
    use atlas::daemon::moment_clock;
    // Wednesday 30 Sep 2026, 14:35 (as local seconds).
    let wed = 1_790_776_800 - (1_790_776_800 % 86_400) + 14 * 3600 + 35 * 60;
    assert_eq!(moment_clock(wed as i64), (14 * 60 + 35, 2));
    assert_eq!(moment_clock(0), (0, 3), "the epoch was a Thursday");
}

/// One message you were sent can be read, and answered (30 Sep 2026 sweep:
/// mail was summarised and drafted to, never read or replied to one by one).
#[test]
fn one_email_can_be_read_and_replied_to() {
    use atlas::daemon::{one_message_asked, OneMessage};
    assert_eq!(one_message_asked("read me the email from Sam"), Some(OneMessage::Read("Sam".into())));
    assert_eq!(one_message_asked("what did Sam's email say?"), Some(OneMessage::Read("Sam".into())));
    assert_eq!(
        one_message_asked("reply to Sam saying I'll be there at six"),
        Some(OneMessage::Reply("Sam".into(), "I'll be there at six".into()))
    );
    assert_eq!(one_message_asked("reply to him saying ok then"), None);
    assert_eq!(
        one_message_asked("send a reply to Jane saying Thursday works for me"),
        Some(OneMessage::Reply("Jane".into(), "Thursday works for me".into()))
    );
    assert_eq!(one_message_asked("read me a story"), None);

    let (mut c, p) = (cfg(), plat());
    c.tools.as_mut().unwrap().mail.accounts = vec![atlas::mail::Account {
        name: "personal".into(),
        address: "me@example.com".into(),
        password_from_vault: "mail personal".into(),
        ..Default::default()
    }];
    let store = Store::new(tmp("one-email"));
    let book = atlas::mailbook::MailBook {
        letters: vec![atlas::mailbook::Letter {
            id: "a1@example.com".into(),
            in_reply_to: None,
            refs: vec![],
            from_name: "Sam Ortiz".into(),
            from: "sam@example.com".into(),
            to: vec!["me@example.com".into()],
            subject: "Dinner Friday".into(),
            at: NOW - 3 * 3600,
            dated: true,
            mine: false,
            excerpt: "Are you still on for dinner Friday at seven?".into(),
        }],
        checked: Default::default(),
    };
    store.save(atlas::mailbook::MailBook::FILE, &book).unwrap();
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
    let read = d.turn("read me the email from Sam", NOW);
    assert!(read.contains("From Sam Ortiz") && read.contains("Dinner Friday") && read.contains("still on for dinner"), "{read}");
    let reply = d.turn("reply to Sam saying yes, see you at seven", NOW + 10);
    assert!(reply.contains("Re: Dinner Friday"), "{reply}");
    let outbox = atlas::outbox::Outbox::load(&store);
    let held = outbox.waiting_for("sam@example.com").expect("held as a draft, not sent");
    assert_eq!(held.subject, "Re: Dinner Friday");
    assert!(held.body.to_lowercase().contains("see you at seven"), "{}", held.body);
    // Nobody fetched from that sender: said, not guessed.
    let none = d.turn("read me the email from Priya", NOW + 20);
    assert!(none.contains("haven't got an email from Priya"), "{none}");
}

/// A sent message carries a date, an id and its character set, and a
/// subject that isn't plain ASCII is encoded (30 Sep 2026 sweep).
#[test]
fn a_sent_message_is_a_proper_message() {
    let m = atlas::smtp::message_text("me@example.com", "sam@example.com", "Café Friday", "See you there\nat six", 1_790_776_800);
    assert!(m.starts_with("Date: "), "{m}");
    assert!(m.contains("\r\nMessage-ID: <") && m.contains("@example.com>\r\n"), "{m}");
    assert!(m.contains("Content-Type: text/plain; charset=utf-8\r\n"), "{m}");
    assert!(m.contains("Subject: =?UTF-8?B?"), "{m}");
    assert!(m.ends_with("\r\n\r\nSee you there\r\nat six"), "{m}");
    assert_eq!(atlas::smtp::rfc2822_date(0), "Thu, 01 Jan 1970 00:00:00 +0000");
    assert_eq!(atlas::smtp::rfc2822_date(1_790_776_800), "Wed, 30 Sep 2026 14:00:00 +0000");
}

/// With `mail.backend: himalaya`, a draft goes out through Himalaya with its
/// own account, not over Atlas's SMTP with a vault password that isn't there.
#[cfg(unix)]
#[test]
fn with_himalaya_a_draft_is_sent_through_himalaya() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("himalaya-send");
    let got = dir.join("sent.eml");
    let args = dir.join("args.txt");
    let fake = dir.join("himalaya");
    std::fs::write(&fake, format!("#!/bin/sh\necho \"$@\" > '{}'\ncat > '{}'\n", args.display(), got.display())).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(atlas::himalaya::send_args("work"), vec!["--account", "work", "message", "send"]);

    let (mut c, p) = (cfg(), plat());
    let mail = &mut c.tools.as_mut().unwrap().mail;
    mail.backend = "himalaya".into();
    mail.himalaya = fake.display().to_string();
    mail.accounts = vec![atlas::mail::Account {
        name: "personal".into(),
        address: "me@example.com".into(),
        himalaya_account: "gmail".into(),
        ..Default::default()
    }];
    let store = Store::new(tmp("himalaya-send-store"));
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
    d.connectivity.set(atlas::connectivity::Reach::Online, 0);
    d.turn("Sam's email is sam@example.com", NOW);
    let drafted = d.turn("email Sam saying see you at six", NOW + 10);
    assert!(drafted.contains("send it"), "{drafted}");
    let sending = d.turn("send it", NOW + 20);
    assert!(sending.contains("Sending the reply to Sam"), "{sending}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut t = NOW + 21;
    while std::time::Instant::now() < deadline && atlas::outbox::Outbox::load(&store).waiting_for("sam@example.com").is_some() {
        d.tick(t);
        t += 1;
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(atlas::outbox::Outbox::load(&store).waiting_for("sam@example.com").is_none(), "still waiting: never sent");
    assert_eq!(std::fs::read_to_string(&args).unwrap().trim(), "--account gmail message send");
    let sent = std::fs::read_to_string(&got).unwrap();
    assert!(sent.contains("To: sam@example.com") && sent.contains("See you at six"), "{sent}");
}

/// With Himalaya, a mailto: unsubscribe goes through it too (1 Oct 2026:
/// it was tried over SMTP with the empty password Himalaya mode carries).
#[cfg(unix)]
#[test]
fn with_himalaya_an_unsubscribe_email_goes_through_himalaya() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("himalaya-unsub");
    let got = dir.join("sent.eml");
    let fake = dir.join("himalaya");
    std::fs::write(&fake, format!("#!/bin/sh\ncat > '{}'\n", got.display())).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

    let (mut c, p) = (cfg(), plat());
    let mail = &mut c.tools.as_mut().unwrap().mail;
    mail.enabled = true;
    mail.backend = "himalaya".into();
    mail.himalaya = fake.display().to_string();
    mail.accounts = vec![atlas::mail::Account {
        name: "personal".into(),
        address: "me@example.com".into(),
        himalaya_account: "gmail".into(),
        ..Default::default()
    }];
    let store = Store::new(tmp("himalaya-unsub-store"));
    let cleanup = atlas::unsub::Cleanup {
        unsubscribe: vec![("Deals Weekly".into(), "<mailto:leave@deals.example?subject=unsubscribe>".into())],
        ..Default::default()
    };
    store.save(atlas::unsub::PENDING, &vec![("personal".to_string(), cleanup)]).unwrap();
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
    d.connectivity.set(atlas::connectivity::Reach::Online, 0);
    let said = d.turn("unsubscribe from those", NOW);
    assert!(!said.contains("couldn't get at the mailbox"), "{said}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut t = NOW + 1;
    while std::time::Instant::now() < deadline && !got.exists() {
        d.tick(t);
        t += 1;
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let sent = std::fs::read_to_string(&got).expect("Himalaya was never asked to send");
    assert!(sent.contains("To: leave@deals.example") && sent.contains("Subject: unsubscribe"), "{sent}");
    assert_eq!(sent.matches("To: leave@deals.example\r\n").count(), 1, "the address, without the ?subject part: {sent}");
}

/// Texts (30 Sep 2026 ruling: messaging goes where the conversations are).
/// Atlas writes it; the phone sends it. Never said to be sent.
#[test]
fn a_text_is_written_for_your_phone_to_send() {
    use atlas::texting::{sms_link, text_asked};
    assert_eq!(text_asked("text Sam saying I'm running late"), Some(("Sam".into(), "I'm running late".into())));
    assert_eq!(text_asked("send Sam a text saying see you at six"), Some(("Sam".into(), "see you at six".into())));
    assert_eq!(text_asked("send a text to my wife saying love you lots"), Some(("my wife".into(), "love you lots".into())));
    assert_eq!(text_asked("send the report to Sam, please do"), None);
    assert_eq!(text_asked("text me when it's done"), None);
    assert_eq!(sms_link("+15551234567", "I'm late"), "sms:+15551234567?&body=I%27m%20late");
    assert_eq!(atlas::people::phone_number("(555) 123-4567"), Some("5551234567".into()));
    assert_eq!(atlas::people::phone_number("+44 7700 900123"), Some("+447700900123".into()));
    assert_eq!(atlas::people::phone_number("next tuesday"), None);

    let (c, p) = (cfg(), plat());
    let store = Store::new(tmp("texting"));
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
    let none = d.turn("text Sam saying I'm running late", NOW);
    assert!(none.contains("don't have a number for Sam"), "{none}");
    let kept = d.turn("Sam's number is (555) 123-4567", NOW + 5);
    assert!(kept.contains("Sam's number"), "{kept}");
    let wrote = d.turn("text Sam saying I'm running late", NOW + 10);
    assert!(wrote.starts_with("Text to Sam: \"I'm running late.\""), "{wrote}");
    assert!(!wrote.to_lowercase().contains("sent"), "never claims it was sent: {wrote}");
    let texts = atlas::texting::Texts::load(&store);
    assert_eq!(texts.waiting.len(), 1);
    assert_eq!(texts.waiting[0].number, "5551234567");
    let card = atlas::texting::card(&texts.current(NOW + 20));
    assert!(card.contains("href=\"sms:5551234567?&amp;body=I%27m%20running%20late.\""), "{card}");
    // "message Sam saying ..." with no paired Atlas called Sam: a text too.
    let ask = d.turn("message Sam saying on my way", NOW + 30);
    assert!(ask.contains("as you") && !ask.contains("terms"), "{ask}");
    let msg = d.turn("yes", NOW + 35);
    assert!(msg.starts_with("Text to Sam:"), "{msg}");
}

/// Bing's result links (`ck/a?...&u=a1<base64url>`) are read, and Bing's
/// own furniture isn't taken for a source (30 Sep 2026: DuckDuckGo answered
/// the laptop with a robot check, so research found "no sources").
#[test]
fn bing_results_are_read_through_its_redirect() {
    let enc = |u: &str| atlas::b64::encode(u.as_bytes()).trim_end_matches('=').replace('+', "-").replace('/', "_");
    let page = format!(
        "<link href=\"https://r.bing.com/rs/x.css\"><h2><a href=\"https://www.bing.com/ck/a?!&amp;&amp;p=abc&amp;u=a1{}&amp;ntb=1\">Canberra</a></h2>\
         <h2><a href=\"https://www.bing.com/ck/a?!&amp;&amp;p=def&amp;u=a1{}&amp;ntb=1\">B</a></h2><a href=\"https://www.bing.com/images\">x</a>",
        enc("https://en.wikipedia.org/wiki/Canberra"),
        enc("https://www.britannica.com/place/Canberra")
    );
    assert_eq!(
        atlas::research::extract_urls(&page, 5),
        vec!["https://en.wikipedia.org/wiki/Canberra".to_string(), "https://www.britannica.com/place/Canberra".to_string()]
    );
    let bing = atlas::research::bing_search();
    assert!(bing.args.iter().any(|a| a.ends_with("q={query_pct}&form=QBLH")));
}

/// Research report, Stage 1 item 10: a failed push waited a flat five
/// minutes and was forgotten on restart.
#[test]
fn a_failed_push_waits_longer_each_time_and_survives_a_restart() {
    use atlas::daemon::phone_retry_wait;
    assert_eq!(phone_retry_wait(0), 60);
    assert_eq!(phone_retry_wait(1), 120);
    assert_eq!(phone_retry_wait(3), 480);
    assert_eq!(phone_retry_wait(30), 1800, "capped at half an hour");
    let (mut c, p) = (cfg(), plat());
    c.tools.as_mut().unwrap().phone = atlas::phone::PhoneConfig {
        enabled: true,
        host: "127.0.0.1:9".into(),
        path: "/atlas".into(),
        timeout_secs: 1,
        ..Default::default()
    };
    let dir = tmp("phone-kept");
    {
        let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
        let note = atlas::notify::Note::new("Build finished", "all green", atlas::notify::Urgency::Routine, NOW);
        *p.input_idle.borrow_mut() = Some(4 * 3600);
        let now = atlas::store::now() + 10 * 3600;
        d.phone_to_retry.push(note);
        d.phone_retry_at = now;
        d.retry_phone(now);
        assert_eq!(d.phone_to_retry.len(), 1, "nothing listening, so it waits");
        assert!(d.phone_retry_at >= now + 120, "the second wait is longer than the first");
    }
    let d = Daemon::new(&c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d.phone_to_retry.len(), 1, "still waiting after a restart");
}

/// Research report, Stage 1 item 10: with no shared folder, syncing gave up
/// even when your other device could be reached directly by its address.
#[test]
fn with_no_folder_sync_goes_straight_to_your_named_devices() {
    if atlas::sync::best_folder().is_some() {
        return; // a cloud folder on this machine: the folder route is taken, not this one
    }
    // The "phone": something listening that takes the connection and hangs
    // up without a bundle. That it was dialled at all is the point.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let dialled = std::thread::spawn(move || {
        l.set_nonblocking(true).unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < until {
            if let Ok((s, _)) = l.accept() {
                drop(s);
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    });
    let (mut c, p) = (cfg(), plat());
    let tools = c.tools.as_mut().unwrap();
    tools.sync.enabled = true;
    tools.sync.folder = String::new();
    tools.elsewhere.known = vec![atlas::elsewhere::Elsewhere {
        name: "phone".into(),
        host: "127.0.0.1".into(),
        port: 1,
        sync_port: Some(port),
        ..Default::default()
    }];
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("sync-direct")), Proactive::new(ProactiveConfig::default()));
    let said = d.execute(&atlas::intent::Intent::Sync(String::new()));
    assert!(dialled.join().unwrap(), "the named device was never dialled: {said}");
    assert!(!said.contains("nowhere to put it"), "{said}");
}

/// A feed whose text has a multi-byte character just after an `&` must not
/// stop Atlas (30 Sep 2026: "end byte index 12 is not a char boundary",
/// a job feed with "&#x2F;CD.\n• Frontend").
#[test]
fn a_feed_with_a_bullet_after_an_ampersand_is_read() {
    let xml = "<rss><channel><title>Jobs</title><item><title>SRE</title><description>CI&#x2F;CD &amp;\n\u{2022} Frontend &\u{2022}\u{2022}\u{2022}\u{2022} ok</description><link>https://example.com/a</link></item></channel></rss>";
    let parsed = atlas::feeds::parse(xml).unwrap();
    assert_eq!(parsed.items.len(), 1);
    let _ = atlas::feeds::text_of("x &\u{2022}\u{2022}\u{2022}\u{2022}\u{2022} y");
}
