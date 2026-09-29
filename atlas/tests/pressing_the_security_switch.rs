//! Atlas pressing a security switch itself, after the read-back and your yes.
//!
//! Eric, 24 Sep 2026, on security changes: "yes". The rules that make it
//! safe are in `confirmed::press_js`: only a visible control labelled for
//! the change, only when there's exactly one, never past a password box,
//! nothing typed. The first half runs everywhere; the second drives a real
//! headless Chrome when `ATLAS_CHROME` names one.

use atlas::browser::{Browser, BrowserConfig};
use atlas::confirmed::{labels_for, pressed_from, Asked, Change, Pressed};
use atlas::tools::ExternalTool;

#[test]
fn only_changes_with_known_words_can_be_pressed() {
    assert!(labels_for(&Change::TurnOffTwoFactor).contains(&"Turn off"));
    assert!(labels_for(&Change::GenerateRecoveryCodes).contains(&"Get new codes"));
    assert!(labels_for(&Change::SwitchMethod { from: "text".into(), to: "app".into() }).is_empty());
    assert!(labels_for(&Change::Other("anything".into())).is_empty());
}

#[test]
fn what_the_page_said_is_read_one_way() {
    assert_eq!(pressed_from("pressed"), Pressed::Done);
    assert_eq!(pressed_from("password"), Pressed::WantsYourPassword);
    assert_eq!(pressed_from("many:3"), Pressed::MoreThanOne(3));
    assert_eq!(pressed_from("none"), Pressed::NotFound);
    assert_eq!(pressed_from("something odd"), Pressed::NotFound);
}

#[test]
fn anything_but_done_hands_the_page_back() {
    let asked = Asked { site: "google".into(), account: String::new(), change: Change::TurnOffTwoFactor };
    for p in [Pressed::NotFound, Pressed::MoreThanOne(2), Pressed::WantsYourPassword, Pressed::NoWordsForIt] {
        let said = p.say(&asked, "https://example.test/security");
        assert!(said.ends_with("It's yours from here: https://example.test/security"), "{said}");
    }
    // Done still hands the page back: some sites ask you to confirm again.
    assert_eq!(
        Pressed::Done.say(&asked, "u"),
        "Done: I pressed it to turn two-factor off for google. Have a look at the page — some sites ask you to confirm again. u"
    );
    for p in [Pressed::WantsYouToSignIn, Pressed::SomewhereElse("evil.example".into())] {
        assert!(p.say(&asked, "u").ends_with("It's yours from here: u"), "{:?}", p);
    }
}

fn chrome(port: u16, tag: &str) -> Option<BrowserConfig> {
    let exe = std::env::var("ATLAS_CHROME").ok()?;
    let profile = std::env::temp_dir().join(format!("atlas-press-{tag}-{}", std::process::id()));
    Some(BrowserConfig {
        port,
        timeout_ms: 3000,
        startup_ms: 15000,
        launch: Some(ExternalTool {
            command: exe,
            args: vec![
                "--headless=new".into(), "--no-sandbox".into(), "--disable-gpu".into(),
                format!("--remote-debugging-port={port}"),
                format!("--user-data-dir={}", profile.display()), "about:blank".into(),
            ],
            ..serde_yaml::from_str("command: x").unwrap()
        }),
        ..BrowserConfig::default()
    })
}

fn page(body: &str) -> String {
    format!("data:text/html,{}", body.replace('#', "%23").replace(' ', "%20"))
}

#[test]
fn in_a_real_browser_it_presses_one_and_refuses_the_rest() {
    let Some(cfg) = chrome(9377, "real") else {
        eprintln!("skipped: ATLAS_CHROME isn't set");
        return;
    };
    let mut b = Browser::start(&cfg, &Default::default()).expect("chrome");
    let off = Change::TurnOffTwoFactor;

    // One switch: pressed, and the page shows it happened.
    b.open(&page("<p id=s>on</p><button onclick=\"document.getElementById('s').textContent='off'\">Turn off</button>")).unwrap();
    assert_eq!(b.press_the_one_at(&off, None).unwrap(), Pressed::Done);
    assert_eq!(b.cdp.eval("document.getElementById('s').textContent").unwrap(), "off");

    // Two that could be it: nothing pressed.
    b.open(&page("<p id=s>on</p><button onclick=\"s.textContent='A'\">Turn off</button><a href=# onclick=\"s.textContent='B'\">Disable</a>")).unwrap();
    assert_eq!(b.press_the_one_at(&off, None).unwrap(), Pressed::MoreThanOne(2));
    assert_eq!(b.cdp.eval("document.getElementById('s').textContent").unwrap(), "on");

    // A password box on the page: stops before anything.
    b.open(&page("<p id=s>on</p><input type=password><button onclick=\"s.textContent='off'\">Turn off</button>")).unwrap();
    assert_eq!(b.press_the_one_at(&off, None).unwrap(), Pressed::WantsYourPassword);
    assert_eq!(b.cdp.eval("document.getElementById('s').textContent").unwrap(), "on");

    // A hidden switch doesn't count; "Turn off notifications" isn't the words.
    b.open(&page("<button style=\"display:none\">Turn off</button><button>Turn off notifications</button>")).unwrap();
    assert_eq!(b.press_the_one_at(&off, None).unwrap(), Pressed::NotFound);

    // Nothing Atlas has words for.
    assert_eq!(b.press_the_one_at(&Change::Other("x".into()), None).unwrap(), Pressed::NoWordsForIt);
    let _ = b.cdp.call("Browser.close", serde_json::json!({}));
}

#[test]
fn a_reply_is_whole_when_its_length_says_so_even_if_the_server_keeps_the_line_open() {
    // Chrome's DevTools endpoint answers and keeps the connection open, so
    // reading to the end never ended: Atlas's own browser could not attach.
    use atlas::http::whole_reply as complete;
    assert!(!complete(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nabc"));
    assert!(complete(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nabcde"));
    assert!(complete(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n"));
    assert!(!complete(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n"));
    assert!(complete(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n"));
    // No length at all: only the end of the connection says it's finished.
    assert!(!complete(b"HTTP/1.1 200 OK\r\n\r\nstill coming"));
    assert!(!complete(b"HTTP/1.1 200 OK\r\nContent-Len"));
}
