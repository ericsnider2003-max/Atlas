//! Something that can actually supply a message.
//!
//! # What `messaging.rs` was
//!
//! A complete, correct, tested module with nothing at either end. It had
//! triage by what a message asks of you, a folder, a note on a person, and a
//! spoken form -- `sort`, `folder_for`, `note_on`, `spoken`, `work_spend` --
//! and **nothing in the tree could supply it a message.** So the daemon
//! called:
//!
//! ```ignore
//! messaging::spoken(&[], ..)
//! ```
//!
//! whose empty case returns literally `"0 messages, all group chat."` -- a
//! count of an inbox nothing had read, stated as a fact. The third instance
//! of that exact shape in this tree, after `goingaway::spoken(&[])` and
//! `money::spoken(&money::summarise(&[]), &[])`.
//!
//! # Why Telegram and not the others
//!
//! `Platform::what_it_permits` has said since the day it was written that two
//! of the six can never work for a personal account: WhatsApp's interface is
//! for businesses and costs per message, Signal is deliberately closed. The
//! tools claiming otherwise drive the desktop app while pretending to be you,
//! which gets accounts banned. Atlas does not do that, and saying so is the
//! module's own words.
//!
//! # And it is honest about being online
//!
//! Everything primary in Atlas works unplugged. This cannot. It is off by
//! default, catalogued `offline: false`, and nothing depends on it.

use atlas::messaging::{self, Platform, Sort};
use atlas::telegram::{self, TelegramConfig};

const A_BATCH: &str = r#"{
  "ok": true,
  "result": [
    {"update_id": 11,
     "message": {"date": 1700000000,
                 "from": {"first_name": "Priya", "username": "priya_k"},
                 "chat": {"id": 5, "type": "private"},
                 "text": "can you send the rate card? we'd love to work with you"}},
    {"update_id": 12,
     "message": {"date": 1700000100,
                 "from": {"first_name": "Sam"},
                 "chat": {"id": 9, "type": "group", "title": "five a side"},
                 "text": "anyone about on saturday"}},
    {"update_id": 13,
     "message": {"date": 1700000200,
                 "from": {"username": "onlyahandle"},
                 "chat": {"id": 5, "type": "private"},
                 "text": "did you get a chance to look at that?"}}
  ]
}"#;

// ===================== a message exists now ===========================

#[test]
fn a_batch_from_telegram_becomes_messages_the_sorting_can_read() {
    let msgs = telegram::into_messages(A_BATCH).expect("parses");
    assert_eq!(msgs.len(), 3);
    assert!(msgs.iter().all(|m| m.platform == Platform::Telegram));

    // A display name when there is one, the handle when there is not, and
    // never an empty string -- "  said:" is how a list stops being readable.
    assert_eq!(msgs[0].from, "Priya");
    assert_eq!(msgs[2].from, "onlyahandle");
    assert!(msgs.iter().all(|m| !m.from.trim().is_empty()));

    // A group is a group and a direct message is not.
    assert_eq!(msgs[1].group.as_deref(), Some("five a side"));
    assert_eq!(msgs[0].group, None);
}

#[test]
fn the_sorting_that_could_never_be_reached_now_runs_on_something_real() {
    // The point of the whole module. These four functions were written,
    // tested and unreachable.
    let msgs = telegram::into_messages(A_BATCH).expect("parses");

    // A brand asking about a rate card is work, not chatter.
    assert_eq!(messaging::sort(&msgs[0], &[]), Sort::Business);
    // A group carrying on is not for you.
    assert_eq!(messaging::sort(&msgs[1], &[]), Sort::Chatter);

    let said = messaging::spoken(&msgs, &[]);
    assert!(said.contains("Priya"), "{said}");
    assert!(!said.contains("all group chat"), "the empty-slice answer is back: {said}");
}

#[test]
fn nothing_read_is_still_said_differently_from_nothing_in_it() {
    // The defect this file exists for, kept as an assertion. An empty slice
    // must never produce a count.
    assert_eq!(messaging::spoken(&[], &[]), "0 messages, all group chat.");

    let raw = crate::common::source_of("daemon");
    let code: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!code.contains("messaging::spoken(&[]"), "the empty-slice count is back");
    assert!(
        code.contains("self.store.load(crate::telegram::KEPT)"),
        "nothing reads the messages that were kept"
    );
    // And with nothing read, it still says what it can honestly say about the
    // platforms rather than counting.
    assert!(code.contains("if kept.is_empty() {"), "it counts an inbox it hasn't read");
}

// ===================== what Telegram sends that is not a message =======

#[test]
fn telegram_refusing_is_read_as_a_refusal_rather_than_an_empty_inbox() {
    // A wrong token comes back as HTTP 200 with `ok: false`, so the status
    // code is not the answer and "no messages" would be the wrong one.
    let refused = r#"{"ok": false, "error_code": 401, "description": "Unauthorized"}"#;
    let e = telegram::into_messages(refused).expect_err("should refuse");
    assert!(e.contains("Unauthorized"), "{e}");

    let no_reason = r#"{"ok": false}"#;
    assert!(telegram::into_messages(no_reason).is_err());
}

#[test]
fn an_ok_with_no_result_list_is_a_shape_change_rather_than_an_empty_inbox() {
    // Reporting it as "no messages" would be the same defect this module was
    // built to end.
    let odd = r#"{"ok": true}"#;
    assert!(telegram::into_messages(odd).is_err());
    // An actually-empty result is an actually-empty inbox.
    assert_eq!(telegram::into_messages(r#"{"ok": true, "result": []}"#).unwrap().len(), 0);
}

#[test]
fn updates_that_are_not_messages_are_skipped_rather_than_fatal() {
    // Somebody edited a message, a button was pressed. An unknown update type
    // must not stop the ones after it being read.
    let mixed = r#"{"ok": true, "result": [
        {"update_id": 1, "callback_query": {"id": "x"}},
        {"update_id": 2, "message": {"date": 1, "from": {"first_name": "Sam"},
                                     "chat": {"id": 1}, "text": "hello"}},
        {"update_id": 3, "message": {"date": 1, "from": {"first_name": "Sam"},
                                     "chat": {"id": 1}, "sticker": {"id": "y"}}}
    ]}"#;
    let msgs = telegram::into_messages(mixed).expect("parses");
    assert_eq!(msgs.len(), 1, "{msgs:#?}");
    assert_eq!(msgs[0].text, "hello");
}

#[test]
fn a_reply_that_is_not_json_says_so() {
    for junk in ["", "<html>502 Bad Gateway</html>", "ok"] {
        assert!(telegram::into_messages(junk).is_err(), "accepted {junk:?}");
    }
}

// ===================== not reading the same thing twice ===============

#[test]
fn how_far_it_got_is_one_past_the_last_one() {
    // Telegram hands the same messages back until told. `+1` because the
    // offset means "start here", not "I had this one" -- off by one here is
    // an inbox that repeats its last message forever.
    let msgs = telegram::into_messages(A_BATCH).expect("parses");
    assert_eq!(telegram::read_up_to(&msgs), Some(14));
    assert_eq!(telegram::read_up_to(&[]), None);
}

#[test]
fn the_offset_goes_into_the_request_and_so_does_the_limit() {
    let p = telegram::updates_path("123:AAA", Some(14), 20);
    assert!(p.starts_with("/bot123:AAA/getUpdates"), "{p}");
    assert!(p.contains("offset=14"), "{p}");
    assert!(p.contains("limit=20"), "{p}");
    // First run: no offset, because there is no "got to" yet and sending
    // `offset=0` means something different.
    assert!(!telegram::updates_path("123:AAA", None, 20).contains("offset"));
}

#[test]
fn how_far_it_got_is_written_before_anything_that_could_fail() {
    // A crash after printing is a crash that repeats itself.
    let raw = crate::common::source_of("main");
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let body = raw
        .split("fn run_telegram(")
        .nth(1)
        .and_then(|r| r.split("\n}\n").next())
        .expect("run_telegram");
    let saved = body.find("telegram::READ_UP_TO, &Some(n)").expect("it never records progress");
    let printed = body.find("messaging::spoken(&fresh").expect("it never says anything");
    assert!(saved < printed, "it prints before recording how far it got");
}

// ===================== the token =====================================

#[test]
fn the_token_never_goes_in_a_settings_file() {
    // A token in `tools.yaml` is a token in your backups, your sync folder,
    // and any screenshot of your settings.
    let yaml = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    let block = yaml.split("\ntelegram:").nth(1).expect("no telegram block").split("\n\n").next().unwrap();
    assert!(!block.contains("token"), "there is a token field in the config: {block}");

    let raw = crate::common::source_of("main");
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let body = raw
        .split("fn run_telegram(")
        .nth(1)
        .and_then(|r| r.split("\n}\n").next())
        .expect("run_telegram");
    assert!(body.contains("vault.put(atlas::telegram::TOKEN"), "the token isn't kept in the vault");
    assert!(body.contains("vault.get(atlas::telegram::TOKEN"), "it isn't read from the vault");

    // And the behaviour rather than the wording: the token goes into the
    // request path, so anything that quotes a failed request quotes the
    // token. An empty one is refused before a socket is opened.
    let e = telegram::fetch("", None, &TelegramConfig::default()).expect_err("no token");
    assert_eq!(e, "no token — run `atlas telegram token`");
    assert!(telegram::updates_path("123:AAA", None, 5).contains("123:AAA"));
}

#[test]
fn a_paste_of_the_wrong_half_is_caught_before_it_becomes_a_network_error() {
    assert!(telegram::looks_like_a_token("123456789:AAEhBOweik6ad9r_QXbKZ5cP1FZ0Zx9abcd"));
    for wrong in [
        "",
        "AAEhBOweik6ad9r_QXbKZ5cP1FZ0Zx9abcd", // the half after the colon
        "123456789",                            // the half before it
        "123456789:short",
        "notdigits:AAEhBOweik6ad9r_QXbKZ5cP1FZ0Zx9abcd",
        "123456789:has spaces in it and is long enough otherwise",
    ] {
        assert!(!telegram::looks_like_a_token(wrong), "accepted {wrong:?}");
    }
}

#[test]
fn a_transport_error_does_not_carry_the_token_into_the_log() {
    // The token is in the path, so an error quoting the request would put it
    // in the log. An empty token short-circuits before any of that.
    let e = telegram::fetch("   ", None, &TelegramConfig::default()).expect_err("no token");
    assert!(e.contains("atlas telegram token"), "{e}");
}

// ===================== online, and said to be ========================

#[test]
fn it_is_off_until_you_turn_it_on_and_the_catalogue_says_it_needs_the_network() {
    assert!(!TelegramConfig::default().enabled);

    let cap = atlas::capability::all()
        .into_iter()
        .find(|c| c.id == "telegram")
        .expect("telegram is not in the catalogue");
    assert!(!cap.offline, "an online capability is catalogued as working unplugged");

    let setup = telegram::HOW_TO_SET_UP;
    assert!(setup.contains("BotFather"), "{setup}");
    // The thing worth knowing before you spend the two minutes.
    assert!(setup.contains("cannot see your existing conversations"), "{setup}");
    assert!(setup.contains("logging in as you, which I won't do"), "{setup}");
}

#[test]
fn the_ones_that_cannot_work_are_still_said_to_be_unable() {
    // Building a reader for one platform must not quietly imply the others.
    let said = messaging::what_you_asked_for(&["whatsapp".into(), "signal".into()]);
    assert!(said.contains("no way to do this for a personal account"), "{said}");
    assert!(said.contains("that's the point of Signal"), "{said}");
}
