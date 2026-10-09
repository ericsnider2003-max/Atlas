//! Eric's rulings of 25 Sep 2026 on the security group:
//!
//! - B1: Atlas types a two-factor code you read out, or finds it in your
//!   email or texts; and turns two-factor on or off for you, read back first.
//! - B2: wrong tokens at the hub are answered more and more slowly.
//! - B3: grants Atlas has don't lapse while you're away (tests/selfgrant.rs).
//! - B4: autofill for real, and a gentle monthly line about unused logins.
//! - B5: each way back into the vault says its weakness.
//! - B6: Atlas makes accounts, and stops at payment, ID or a robot check.
//! - C: the agreed call wording, used.
//!
//! The browser tests run Atlas's page scripts in a real headless Chromium
//! against local test pages when one is installed (ATLAS_CHROMIUM, or the
//! Playwright one at /opt/pw-browsers/chromium); without one they say so and
//! pass, since the decisions are tested without a browser above them.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::twofactor::{self, Found, Source};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-2fa-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

// ------------------------------------------------------------ finding the code

#[test]
fn a_code_read_out_is_heard_however_it_is_said() {
    assert_eq!(twofactor::code_in_words("type my code four eight two nine one seven").as_deref(), Some("482917"));
    assert_eq!(twofactor::code_in_words("the code is 482 917").as_deref(), Some("482917"));
    assert_eq!(twofactor::code_in_words("enter 4-8-2-9-1-7").as_deref(), Some("482917"));
    // A leading zero read as "oh" is kept.
    assert_eq!(twofactor::code_in_words("it's oh one double seven three").as_deref(), Some("01773"));
    assert_eq!(twofactor::code_in_words("zero one double seven three").as_deref(), Some("01773"));
    // "for" and "to" are words before a digit has been heard.
    assert_eq!(twofactor::code_in_words("type my code for google").as_deref(), None);
    assert_eq!(twofactor::code_in_words("type my code").as_deref(), None);
    assert_eq!(twofactor::source_of("it's in my email"), Source::Email);
    assert_eq!(twofactor::source_of("the code's in my texts"), Source::Texts);
    assert_eq!(twofactor::source_of("type 482917"), Source::Spoken);
}

#[test]
fn the_code_in_a_message_is_the_number_beside_the_code_words() {
    let google = "G-614207 is your Google verification code. © 2026 Google LLC, 1600 Amphitheatre Parkway";
    assert_eq!(twofactor::code_in_text("", google).as_deref(), Some("614207"));
    let microsoft = "Security code\nPlease use the following security code for the Microsoft account er***@gmail.com.\nSecurity code: 3981724\nThanks, The Microsoft account team";
    assert_eq!(twofactor::code_in_text("Microsoft account security code", microsoft).as_deref(), Some("3981724"));
    let spaced = "Your Discord login code is 123 456. It expires in 10 minutes.";
    assert_eq!(twofactor::code_in_text("", spaced).as_deref(), Some("123456"));
    // An order confirmation has numbers and no code words: nothing.
    assert_eq!(twofactor::code_in_text("Your order has shipped", "Order #55512345, total $1,299.00, 2026").as_deref(), None);
    // A bank alert with an amount and a code: the code, not the amount.
    let bank = "Chase: we'll never call you for this code. Your one-time code is 55120. Purchase of $4,210.00 pending.";
    assert_eq!(twofactor::code_in_text("", bank).as_deref(), Some("55120"));
}

#[test]
fn a_fresh_code_from_the_site_wins_over_a_newer_one_from_elsewhere() {
    let now = 10_000;
    let found = vec![
        Found { code: "111111".into(), from: "noreply@github.com".into(), at: now - 60 },
        Found { code: "222222".into(), from: "security@discord.com".into(), at: now - 10 },
        Found { code: "333333".into(), from: "noreply@github.com".into(), at: now - 3_600 },
    ];
    assert_eq!(twofactor::newest(&found, Some("github.com"), now).unwrap().code, "111111");
    assert_eq!(twofactor::newest(&found, None, now).unwrap().code, "222222");
    // An hour old is spent.
    let old = vec![found[2].clone()];
    assert!(twofactor::newest(&old, Some("github"), now).is_none());
}

#[test]
fn codes_are_found_in_the_phone_link_window() {
    let window = "Messages\nMom\nsee you sunday\n22395\nYour Instagram code is 845 221. Don't share it.\nType a message";
    let found = twofactor::from_phone_link(window, 500);
    assert_eq!(found.first().map(|f| f.code.as_str()), Some("845221"), "{found:?}");
}

// ------------------------------------------------------------ typing it in

fn daemon_with<'a>(cfg: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(cfg, p, None, Store::new(scratch(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn a_code_you_read_out_is_typed_into_the_window_in_front() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    p.focus_on("chrome.exe", "Sign in - Google Accounts");
    *p.front.borrow_mut() = Some(atlas::platform::WindowId(9));
    p.set_window_text(9, "Enter the code");
    let mut d = daemon_with(cfg, &p, "typed");
    let said = d.turn("type my code four eight two nine one seven", 1_000);
    assert!(said.contains("Typed 4 8 2, 9 1 7"), "{said}");
    use atlas::platform::mock::Action;
    assert!(p.actions().iter().any(|a| matches!(a, Action::Type(t) if t == "482917")), "{:?}", p.actions());
    assert!(p.actions().iter().any(|a| matches!(a, Action::Press(k) if k == "enter")));
}

#[test]
fn with_no_code_it_asks_for_one_and_says_where_it_can_look() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    p.focus_on("chrome.exe", "GitHub");
    *p.front.borrow_mut() = Some(atlas::platform::WindowId(3));
    let mut d = daemon_with(cfg, &p, "ask");
    let said = d.turn("type the code", 1_000);
    assert!(said.contains("in my email") && said.contains("in my texts"), "{said}");
    use atlas::platform::mock::Action;
    assert!(!p.actions().iter().any(|a| matches!(a, Action::Type(_) | Action::Press(_))), "nothing typed without a code");
    // Texts, with Phone Link not open: says so rather than guessing.
    let said = d.turn("it's in my texts", 1_001);
    assert!(said.contains("Phone Link"), "{said}");
}

// ------------------------------------------------------------ on and off

#[test]
fn turning_two_factor_off_is_read_back_and_waits_for_a_yes() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let mut d = daemon_with(cfg, &p, "off");
    // The vault is locked: Atlas would need to sign in, so it says so.
    let said = d.turn("turn off two factor on github", 1_000);
    assert!(said.contains("vault"), "{said}");

    d.vault.open("a genuinely long passphrase, not a word", 0, &atlas::vault::VaultConfig::default()).unwrap();
    let said = d.turn("turn off two factor on github", 1_010);
    assert!(said.contains("So: turn two-factor off for github"), "{said}");
    assert!(said.contains("SSH keys"), "the consequence is said: {said}");
    // A mumble is not a yes.
    let said = d.turn("hmm maybe", 1_020);
    assert!(said.contains("left it alone"), "{said}");

    // Asking for "on" reads back "on".
    let said = d.turn("turn on 2fa for my google account", 1_030);
    assert!(said.contains("turn two-factor on for google"), "{said}");
    let said = d.turn("no", 1_040);
    assert_eq!(said, "Left alone.");

    // No site named: asked, not guessed.
    let said = d.turn("turn off two factor", 1_050);
    assert!(said.starts_with("Which site"), "{said}");
}

#[test]
fn the_walkthrough_can_turn_it_on_as_well_as_off() {
    let w = atlas::walkthrough::to_turn_on(&["github".to_string()]);
    assert_eq!(w.stops.len(), 1);
    assert!(w.stops[0].do_this.contains("Turn it on"));
}

// ------------------------------------------------------------ B2: wrong tokens

#[test]
fn wrong_tokens_at_the_hub_are_answered_more_and_more_slowly() {
    let cfg: atlas::server::ServerConfig = serde_yaml::from_str("enabled: true\nport: 0\n").unwrap();
    let server = atlas::server::Server::bind(&cfg, "a-long-enough-token-for-this-test").unwrap();
    let port = server.port();
    let client = std::thread::spawn(move || {
        let mut took = Vec::new();
        for _ in 0..6 {
            let started = std::time::Instant::now();
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /hub HTTP/1.1\r\nHost: x\r\nX-Atlas-Token: not-the-token-at-all\r\n\r\n").unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            took.push(started.elapsed().as_millis());
            assert!(out.contains("401") || out.contains("403"), "{out}");
        }
        took
    });
    for _ in 0..6 {
        let _ = server.serve_once(&mut |_| unreachable!("a wrong token never reaches a handler"));
    }
    let took = client.join().unwrap();
    assert!(took[0] >= 90, "the first wrong one is already slowed: {took:?}");
    assert!(took[5] >= 1_900, "six in a row wait the cap: {took:?}");
    assert!(server.guesses_worth_mentioning().unwrap().contains("6 failed attempts"));
}

// ------------------------------------------------------------ B4, B5, C

#[test]
fn unused_logins_are_mentioned_gently_and_never_removed() {
    let mut a = atlas::signin::Access::default();
    let day = 86_400;
    a.grant("github.com", "eric", "GitHub", atlas::signin::Allowed::SignIn, "github", 0);
    a.grant("reddit.com", "eric", "Reddit", atlas::signin::Allowed::SignIn, "reddit", 0);
    assert!(a.quiet_line(30 * day).is_none(), "a month isn't unused");
    let line = a.quiet_line(100 * day).unwrap();
    assert!(line.contains("GitHub and Reddit") && line.contains("I'm keeping"), "{line}");
    assert_eq!(a.grants.len(), 2);
}

#[test]
fn each_way_back_into_the_vault_says_its_weakness() {
    let s = atlas::recovery::Setup {
        route: atlas::recovery::Route::OnePerson,
        with: "my brother".into(),
        set_up_at: 0,
        last_checked: None,
        used_at: None,
    };
    let said = atlas::recovery::described(&s);
    assert!(said.contains("my brother") && said.contains("any time") && said.contains("wouldn't know"), "{said}");
    let split = atlas::recovery::route_from(&["split".into(), "3".into(), "2".into()]).unwrap();
    assert!(atlas::recovery::described(&atlas::recovery::Setup { route: split, ..s }).contains("be able to tell"));
}

#[test]
fn the_agreed_call_wording_is_the_one_used() {
    let mut cfg = atlas::consent::ConsentConfig::default();
    cfg.announcement = "brief".into();
    assert_eq!(atlas::consent::the_announcement(&cfg), atlas::consent::announcement_named("brief").unwrap());
    assert!(atlas::consent::script_line(&cfg, "if someone objects").unwrap().contains("deleted"));
    let notes = atlas::callnotes::Notes::new(cfg, scratch("call"));
    assert!(notes.what_it_does().contains("Nothing is uploaded"));
}

// ------------------------------------------------------------ in a real browser

/// A tiny site: a sign-in page, a code page, a welcome page, a sign-up page
/// and a sign-up page that wants a card.
fn test_site() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            // One thread each: a browser opens connections ahead of time and
            // may leave one idle, which would hold up a one-at-a-time server.
            std::thread::spawn(move || serve_one(s));
        }
    });
    port
}

fn serve_one(s: std::net::TcpStream) {
    {
        {
            let mut s = s;
            let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let mut buf = vec![0u8; 16384];
            let mut got = Vec::new();
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(h) = text.find("\r\n\r\n") {
                    let len = text
                        .lines()
                        .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    if got.len() >= h + 4 + len {
                        break;
                    }
                }
            }
            let req = String::from_utf8_lossy(&got).to_string();
            let first = req.lines().next().unwrap_or("").to_string();
            let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            let page = |inner: &str| format!("<!doctype html><html><body>{inner}</body></html>");
            let html = if first.starts_with("GET /login") {
                page("<h1>Sign in</h1><form method=post action=/login><input name=username placeholder=Email><input type=password name=password><button>Sign in</button></form>")
            } else if first.starts_with("POST /login") {
                if body.contains("password=right-pass") && body.contains("username=eric") {
                    page("<h1>Two-step verification</h1><p>Enter the code we sent to your phone.</p><form method=post action=/code><input name=otp autocomplete=one-time-code><button>Verify</button></form>")
                } else {
                    page("<p>Incorrect password.</p><form method=post action=/login><input name=username><input type=password name=password><button>Sign in</button></form>")
                }
            } else if first.starts_with("POST /code") {
                if body.contains("otp=482917") {
                    page("<h1>Welcome back, eric</h1>")
                } else {
                    page("<p>Wrong code. Enter the code we sent.</p><form method=post action=/code><input name=otp autocomplete=one-time-code><button>Verify</button></form>")
                }
            } else if first.starts_with("GET /signup-paid") {
                page("<h1>Create account</h1><form method=post action=/made><input type=email name=email><input type=password name=password><p>Card number</p><input name=cc><button>Create account</button></form>")
            } else if first.starts_with("GET /signup") {
                page("<h1>Create account</h1><form method=post action=/made><input type=email name=email><input type=password name=password><input type=password name=confirm_password><label><input type=checkbox name=terms> I agree to the terms</label><label><input type=checkbox name=news> Send me marketing offers</label><button>Create account</button></form>")
            } else if first.starts_with("POST /made") {
                if body.contains("terms=on") && !body.contains("news=on") && body.contains("email=eric%40example.com") {
                    page("<h1>You're all set</h1>")
                } else {
                    page(&format!("<p>Form wrong: {body}</p>"))
                }
            } else {
                page("<p>nothing here</p>")
            };
            let _ = s.write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len())
                    .as_bytes(),
            );
        }
    }
}

/// The test's Chrome, shut down when the test ends however it ends (6 Oct
/// 2026): `close()` only dropped the connection, so every run left a headless
/// Chrome behind, phoning Google's push service for as long as the machine
/// stayed up.
struct Quits(Option<atlas::browser::Browser>);

impl std::ops::Deref for Quits {
    type Target = atlas::browser::Browser;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("browser")
    }
}

impl std::ops::DerefMut for Quits {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("browser")
    }
}

impl Drop for Quits {
    fn drop(&mut self) {
        if let Some(b) = self.0.take() {
            b.quit();
        }
    }
}

fn a_browser(tag: &str) -> Option<Quits> {
    let chrome = std::env::var("ATLAS_CHROMIUM").unwrap_or_else(|_| "/opt/pw-browsers/chromium".into());
    if !Path::new(&chrome).exists() {
        eprintln!("no headless Chromium here ({chrome}); the page scripts weren't run in a browser");
        return None;
    }
    // A port per test: two tests sharing one Chrome would drive the same page.
    let which: u16 = match tag { "signin" => 1, "signup" => 2, _ => 3 };
    let debug_port = 9400 + (std::process::id() % 300) as u16 * 3 + which;
    let profile = scratch(&format!("chrome-{tag}"));
    let yaml = format!(
        "port: {debug_port}\ntimeout_ms: 8000\nstartup_ms: 15000\nlaunch:\n  command: \"{chrome}\"\n  args: [\"--headless=new\", \"--no-sandbox\", \"--disable-gpu\", \"--remote-debugging-port={debug_port}\", \"--user-data-dir={}\", \"--no-first-run\", \"--disable-background-networking\", \"--disable-component-update\", \"--disable-sync\", \"--no-pings\"]\n",
        profile.display()
    );
    let bcfg: atlas::browser::BrowserConfig = serde_yaml::from_str(&yaml).unwrap();
    atlas::browser::Browser::start(&bcfg, &Default::default()).ok().map(|b| Quits(Some(b)))
}

#[test]
fn signing_in_fills_the_login_then_the_code_in_a_real_browser() {
    let site = test_site();
    let Some(mut b) = a_browser("signin") else { return };
    let start = format!("http://localhost:{site}/login");
    let got = atlas::webrun::sign_in_at(&mut b, "localhost", &start, "eric", "right-pass");
    assert_eq!(got, atlas::webrun::SignedIn::WantsCode);
    let wrong = atlas::webrun::enter_code(&mut b, "111111").unwrap();
    assert!(matches!(wrong, atlas::webrun::SignedIn::Failed(ref w) if w.contains("wrong")), "{wrong:?}");
    let right = atlas::webrun::enter_code(&mut b, "482917").unwrap();
    assert!(matches!(right, atlas::webrun::SignedIn::Unconfirmed(_)), "a vanished code prompt is not a verified session receipt: {right:?}");
    assert!(b.cdp.text().unwrap().contains("Welcome back"));

    // A changed password is said as that, not as "it worked".
    let got = atlas::webrun::sign_in_at(&mut b, "localhost", &start, "eric", "old-pass");
    assert_eq!(got, atlas::webrun::SignedIn::Rejected);

    // A page that isn't the site asked for: nothing filled.
    let got = atlas::webrun::sign_in_at(&mut b, "github.com", &start, "eric", "right-pass");
    assert_eq!(got, atlas::webrun::SignedIn::SomewhereElse("localhost".into()));
}

#[test]
fn signing_up_fills_the_form_agrees_to_the_terms_and_stops_at_a_card() {
    let site = test_site();
    let Some(mut b) = a_browser("signup") else { return };
    let cfg = atlas::enrol::EnrolConfig::default();
    let mut e = atlas::enrol::Enrolment::new("localhost", "eric", 0);
    let got = atlas::webrun::sign_up_unless(&mut b, &mut e, "eric@example.com", "Xk7-long-pass", &cfg, Some(&format!("http://localhost:{site}/signup")), &|| false);
    assert_eq!(got, atlas::webrun::SignedUp::Made);
    let text = b.cdp.text().unwrap();
    assert!(text.contains("all set"), "terms ticked, marketing left alone, email in: {text}");

    let mut e = atlas::enrol::Enrolment::new("localhost", "eric", 0);
    let got = atlas::webrun::sign_up_unless(&mut b, &mut e, "eric@example.com", "Xk7-long-pass", &cfg, Some(&format!("http://localhost:{site}/signup-paid")), &|| false);
    assert!(matches!(got, atlas::webrun::SignedUp::Stopped(atlas::enrol::Stopped::WantsPayment(_))), "{got:?}");
}

