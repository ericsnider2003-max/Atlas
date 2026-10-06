//! Muse Spark for whoever connects it, and "think hard" for everyone
//! (6 Oct 2026).
//!
//! People on Eric's wait list asked for Muse. Meta's API has no free tier
//! and no spending cap, and no sign-in that bills the person signing in, so
//! each person brings their own key and Atlas keeps the monthly limit Meta
//! doesn't. What Muse does that's worth having without paying for it -- its
//! "Contemplating" mode, several tries combined -- runs on the model here.
//!
//! Its own process: the Muse key and the month's spending are process-wide.

use atlas::brain::{Llm, LongReply};
use atlas::muse::{self, MuseConfig, MuseFirst, MuseSpark};
use std::sync::{Arc, Mutex};

/// One test at a time touches the process-wide key and ledger.
static ONE: Mutex<()> = Mutex::new(());

fn answer(text: &str, tin: u64, tout: u64) -> String {
    serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": tin, "completion_tokens": tout},
    })
    .to_string()
}

fn now() -> u64 {
    atlas::store::now()
}

#[test]
fn the_request_is_what_meta_takes() {
    let b: serde_json::Value = serde_json::from_str(&muse::muse_request(&MuseConfig::default(), "sys", "hi", 500, "high")).unwrap();
    assert_eq!(b["model"], "muse-spark-1.3");
    assert_eq!(b["reasoning_effort"], "high");
    assert_eq!(b["max_completion_tokens"], 500);
    for refused in ["stop", "n", "logprobs", "logit_bias"] {
        assert!(b.get(refused).is_none(), "{refused} is refused by Meta");
    }
    let b: serde_json::Value = serde_json::from_str(&muse::muse_request(&MuseConfig::default(), "s", "u", 10, "none")).unwrap();
    assert_eq!(b["reasoning_effort"], "medium", "\"none\" is a 400 from Meta, so it's never sent");
    let (text, cut, tin, tout) = muse::answer_of(&answer("Paris.", 12, 3)).unwrap();
    assert_eq!((text.as_str(), cut, tin, tout), ("Paris.", false, 12, 3));
    let e = muse::answer_of(r#"{"error":{"message":"invalid api key"}}"#).unwrap_err();
    assert!(e.contains("invalid api key"));
}

#[test]
fn muse_is_asked_only_with_a_key_scrubbed_and_counted_until_the_limit() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let sent: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = sent.clone();
    let m = MuseSpark::with_sender(
        MuseConfig::default(),
        Box::new(move |key, body| {
            seen.lock().unwrap().push((key.to_string(), body.to_string()));
            // Writes the placeholder back, as it's told to.
            let v: serde_json::Value = serde_json::from_str(body).unwrap();
            let user = v["messages"][1]["content"].as_str().unwrap().to_string();
            Ok(answer(&format!("Mail {}", user.split_whitespace().last().unwrap()), 1_000_000, 0))
        }),
    );
    muse::set_key(None);
    let e = m.complete("s", "u").unwrap_err().to_string();
    assert!(e.contains("isn't connected"), "{e}");
    assert!(sent.lock().unwrap().is_empty(), "nothing goes out without a key");

    muse::restore(&muse::Ledger::default(), 2.0, now());
    muse::set_key(Some("mk-test-key-0123456789".into()));
    let r = m.complete("s", "write to sam@example.com").unwrap();
    assert_eq!(r, "Mail sam@example.com", "the address is put back in the answer");
    let (key, body) = sent.lock().unwrap()[0].clone();
    assert_eq!(key, "mk-test-key-0123456789");
    assert!(!body.contains("sam@example.com"), "the address never left: {body}");

    // A million tokens in at $1.25: under $2 still, then over.
    assert!(!muse::over_cap(now()));
    m.complete("s", "again").unwrap();
    assert!(muse::over_cap(now()), "{}", muse::spent_sentence(now()));
    let e = m.complete("s", "once more").unwrap_err().to_string();
    assert!(e.contains("limit"), "{e}");
    assert_eq!(sent.lock().unwrap().len(), 2, "nothing is sent past the limit");
    assert!(muse::spent_sentence(now()).contains("$2.50 of your $2.00 limit"), "{}", muse::spent_sentence(now()));

    // A new month starts from nothing; this month's record is kept.
    let l = muse::ledger(now());
    muse::restore(&muse::Ledger { month: "1999-01".into(), spent_usd: 99.0, calls: 9 }, 2.0, now());
    assert!(!muse::over_cap(now()), "last month's spending isn't this month's");
    muse::restore(&l, 2.0, now());
    assert!(muse::over_cap(now()));
    muse::set_key(None);
}

/// A model that says who it is and counts what it's asked.
struct Named(&'static str, Mutex<Vec<String>>, bool);
impl Llm for Named {
    fn complete(&self, _s: &str, u: &str) -> atlas::error::Result<String> {
        self.1.lock().unwrap().push(u.to_string());
        if self.2 {
            return Err(atlas::error::AtlasError::Platform("down".into()));
        }
        Ok(format!("{} answered", self.0))
    }
    fn complete_long(&self, s: &str, u: &str, _m: u32) -> atlas::error::Result<LongReply> {
        self.complete(s, u).map(|text| LongReply { text, cut_off: false })
    }
}

#[test]
fn connected_muse_takes_the_hard_work_and_talking_stays_here() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let rest = Arc::new(Named("local", Mutex::new(Vec::new()), false));
    let muse_down = Arc::new(Mutex::new(false));
    let down = muse_down.clone();
    let m = Arc::new(MuseSpark::with_sender(
        MuseConfig::default(),
        Box::new(move |_k, _b| if *down.lock().unwrap() { Err("Meta answered 500".into()) } else { Ok(answer("muse answered", 10, 10)) }),
    ));
    let both = MuseFirst { muse: m, rest: rest.clone() };

    muse::set_key(None);
    assert_eq!(both.complete_hard("s", "u").unwrap(), "local answered", "not connected: as before");

    muse::restore(&muse::Ledger::default(), 5.0, now());
    muse::set_key(Some("mk-test-key-0123456789".into()));
    assert_eq!(both.complete_hard("s", "u").unwrap(), "muse answered");
    assert_eq!(both.complete_long("s", "u", 100).unwrap().text, "muse answered");
    assert_eq!(both.complete("s", "u").unwrap(), "local answered", "talking stays on this machine");
    *muse_down.lock().unwrap() = true;
    assert_eq!(both.complete_hard("s", "u").unwrap(), "local answered", "Muse failing falls back, never fails the task");
    muse::set_key(None);
}

#[test]
fn thinking_hard_is_three_tries_and_the_best_of_them() {
    let model = Named("local", Mutex::new(Vec::new()), false);
    let said = atlas::contemplate::contemplate(&model, "Should I lease or buy a car?", &|| false).unwrap();
    assert_eq!(said, "local answered");
    let asked = model.1.lock().unwrap().clone();
    assert_eq!(asked.len(), atlas::contemplate::ANGLES.len() + 1, "a draft per angle, then the merge");
    let merge = asked.last().unwrap();
    assert!(merge.contains("Draft 1") && merge.contains("Draft 3") && merge.contains("lease or buy"), "{merge}");

    let down = Named("local", Mutex::new(Vec::new()), true);
    assert!(atlas::contemplate::contemplate(&down, "q", &|| false).unwrap_err().contains("no draft"));
    let stopped = Named("local", Mutex::new(Vec::new()), false);
    assert!(atlas::contemplate::contemplate(&stopped, "q", &|| true).is_err());
    assert!(stopped.1.lock().unwrap().is_empty(), "stopped before anything was asked");
}

#[test]
fn what_is_said_routes_to_muse_or_to_thinking_hard() {
    use atlas::connecting::{connect_asked, Connect};
    assert_eq!(muse::asked_of_muse("Ask Muse what the capital of Peru is").as_deref(), Some("what the capital of Peru is"));
    assert_eq!(muse::asked_of_muse("muse, plan my week").as_deref(), Some("plan my week"));
    assert_eq!(muse::asked_of_muse("ask muse"), None);
    assert_eq!(muse::asked_of_muse("I love the muse album"), None);
    assert_eq!(atlas::contemplate::asked_to_think_hard("Think hard about whether to move.").as_deref(), Some("whether to move"));
    assert_eq!(atlas::contemplate::asked_to_think_hard("take your time and think about my budget").as_deref(), Some("my budget"));
    assert_eq!(atlas::contemplate::asked_to_think_hard("I think about it a lot"), None);
    assert_eq!(connect_asked("connect Muse"), Some(Connect::Muse));
    assert_eq!(connect_asked("set up my muse spark"), Some(Connect::Muse));
}

#[test]
fn the_accounts_page_offers_muse_without_atlas_paying_for_it() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    muse::set_key(None);
    let dir = std::env::temp_dir().join(format!("atlas-muse-page-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let mut d = atlas::daemon::Daemon::new(&c, &p, None, atlas::store::Store::new(dir.clone()), atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()));
    let page = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Accounts)).body;
    assert!(page.contains("Muse Spark") && (page.contains("Open Meta&#39;s key page") || page.contains("Open Meta's key page")), "the Muse block");
    assert!(page.contains("value='5.00'"), "a $5 limit to start");
    let post = |d: &mut atlas::daemon::Daemon, f: &[(&str, &str)]| {
        let r = atlas::hublive::reply(d, atlas::server::Action::HubPost { path: "/hub/connect".into(), fields: f.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() });
        atlas::hub::urldecode(&r.body)
    };
    let back = post(&mut d, &[("what", "muse-key"), ("key", "short"), ("cap", "5")]);
    assert!(back.contains("doesn't look like a whole key"), "{back}");
    assert!(!muse::has_key(), "a bad paste isn't kept");
    let back = post(&mut d, &[("what", "muse-cap"), ("cap", "$12.50")]);
    assert!(back.contains("$12.50"), "{back}");
    let back = post(&mut d, &[("what", "muse-cap"), ("cap", "lots")]);
    assert!(back.contains("isn't an amount"), "{back}");
    // Said, not pressed: without a key Muse isn't asked, and says how.
    let r = d.turn("ask Muse what to cook tonight", now());
    assert!(r.contains("isn't connected") && r.contains("connect Muse"), "{r}");
    // "connect Muse" opens Meta's page in the browser.
    let r = d.turn("connect Muse", now());
    assert!(r.contains("Meta's page"), "{r}");
    let opened: Vec<String> = p.actions().iter().map(|a| format!("{a:?}")).collect();
    assert!(opened.iter().any(|a| a.contains("open:https://dev.meta.ai")), "{opened:?}");
    // Thinking hard needs no account at all.
    let r = d.turn("think hard about whether to lease or buy", now());
    assert!(r.contains("three tries") || r.contains("isn't one running"), "{r}");
    // Never a key of Atlas's own: nothing ships one.
    let src = std::fs::read_to_string("src/muse.rs").unwrap();
    assert!(!src.contains("mk-") && !src.contains("MODEL_API_KEY"), "no key is built in");
    let _ = std::fs::remove_dir_all(dir);
}
