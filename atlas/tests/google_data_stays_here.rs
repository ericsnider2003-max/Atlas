//! What Atlas reads from Google stays on this computer (6 Oct 2026).
//!
//! Google's OAuth verification holds apps to its API Services User Data
//! Policy ("Limited Use"): data from Google may not be handed to others
//! except to provide what the user asked for, and never to train someone
//! else's model. Atlas's replies carry calendar titles and YouTube numbers
//! into the conversation the model reads, so the guarantee is at the doors
//! out: while anything from Google is held, no third party's model is asked.
//!
//! Its own process: the "Google data held" flag is process-wide.

use atlas::brain::{FallbackLlm, Llm, LongReply};
use std::sync::{Arc, Mutex};

static ONE: Mutex<()> = Mutex::new(());

struct Counting(&'static str, Mutex<u32>, bool);
impl Llm for Counting {
    fn complete(&self, _s: &str, _u: &str) -> atlas::error::Result<String> {
        *self.1.lock().unwrap() += 1;
        if self.2 {
            return Err(atlas::error::AtlasError::Platform("down".into()));
        }
        Ok(self.0.to_string())
    }
    fn complete_long(&self, s: &str, u: &str, _m: u32) -> atlas::error::Result<LongReply> {
        self.complete(s, u).map(|text| LongReply { text, cut_off: false })
    }
}

fn calls(c: &Arc<Counting>) -> u32 {
    *c.1.lock().unwrap()
}

#[test]
fn the_free_online_models_are_not_asked_while_google_data_is_held() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let sent = Arc::new(Mutex::new(0u32));
    let s2 = sent.clone();
    let free = atlas::freeonline::FreeOnline::with_sender(
        atlas::freeonline::PROVIDERS.to_vec(),
        Box::new(move |_url, _body| {
            *s2.lock().unwrap() += 1;
            Ok(r#"{"choices":[{"message":{"content":"hi"}}]}"#.to_string())
        }),
    );
    atlas::brain::set_google_data_held(true);
    let e = free.complete("s", "what's on my calendar").unwrap_err().to_string();
    assert!(e.contains("Google"), "{e}");
    assert_eq!(*sent.lock().unwrap(), 0, "nothing left the machine");
    atlas::brain::set_google_data_held(false);
    assert_eq!(free.complete("s", "u").unwrap(), "hi");
    assert_eq!(*sent.lock().unwrap(), 1);
}

#[test]
fn the_fallback_stays_local_unless_the_second_model_is_your_own() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let local = Arc::new(Counting("local", Mutex::new(0), true));
    let online = Arc::new(Counting("online", Mutex::new(0), false));
    let both = FallbackLlm::new(local.clone(), Some(online.clone()));

    atlas::brain::set_google_data_held(true);
    assert!(both.complete("s", "u").is_err(), "the local model's failure stands");
    assert!(both.complete_hard("s", "u").is_err());
    assert!(both.complete_long("s", "u", 100).is_err());
    assert_eq!(calls(&online), 0, "a third party was never asked");

    // Your own server is yours: it still takes the work.
    let own = Arc::new(Counting("own", Mutex::new(0), false));
    let yours = FallbackLlm::new(local.clone(), Some(own.clone())).secondary_is_your_own();
    assert_eq!(yours.complete_hard("s", "u").unwrap(), "own");

    atlas::brain::set_google_data_held(false);
    assert_eq!(both.complete("s", "u").unwrap(), "online", "as before when nothing from Google is held");
}

#[test]
fn muse_takes_no_background_work_while_google_data_is_held_but_answers_you() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let rest = Arc::new(Counting("local", Mutex::new(0), false));
    let asked = Arc::new(Mutex::new(0u32));
    let a2 = asked.clone();
    let muse = Arc::new(atlas::muse::MuseSpark::with_sender(
        atlas::muse::MuseConfig::default(),
        Box::new(move |_k, _b| {
            *a2.lock().unwrap() += 1;
            Ok(r#"{"choices":[{"message":{"content":"muse"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#.to_string())
        }),
    ));
    let first = atlas::muse::MuseFirst { muse: muse.clone(), rest: rest.clone() };
    atlas::muse::restore(&atlas::muse::Ledger::default(), 5.0, atlas::store::now());
    atlas::muse::set_key(Some("mk-test-key-0123456789".into()));

    atlas::brain::set_google_data_held(true);
    assert_eq!(first.complete_hard("s", "the week ahead").unwrap(), "local");
    assert_eq!(*asked.lock().unwrap(), 0, "the conversation never went to Muse");
    // "ask Muse ..." carries only your own question.
    assert_eq!(muse.ask("s", "what's a good pasta", 100, "low").unwrap().text, "muse");

    atlas::brain::set_google_data_held(false);
    assert_eq!(first.complete_hard("s", "u").unwrap(), "muse");
    atlas::muse::set_key(None);
}

#[test]
fn a_google_calendar_or_sign_in_marks_the_data_as_held() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("atlas-google-held-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = atlas::store::Store::new(dir.clone());
    let p = atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let mut d = atlas::daemon::Daemon::new(&c, &p, None, store.clone(), atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()));

    atlas::brain::set_google_data_held(true);
    atlas::connecting::note_google_data(&mut d);
    assert!(!atlas::brain::google_data_held(), "nothing from Google: the gate opens again");

    let link = atlas::connect::CalendarLink {
        name: "Google Calendar (me@gmail.com)".into(),
        url: atlas::oauthlink::calendar_key(atlas::oauthlink::Provider::Google, "me@gmail.com"),
        ..Default::default()
    };
    store.save(atlas::connect::CALENDAR_LINKS, &vec![link]).unwrap();
    atlas::connecting::note_google_data(&mut d);
    assert!(atlas::brain::google_data_held(), "a Google calendar is connected");

    // An Outlook calendar is Microsoft's, not Google's.
    let ms = atlas::connect::CalendarLink {
        name: "Outlook".into(),
        url: atlas::oauthlink::calendar_key(atlas::oauthlink::Provider::Microsoft, "me@outlook.com"),
        ..Default::default()
    };
    store.save(atlas::connect::CALENDAR_LINKS, &vec![ms]).unwrap();
    atlas::connecting::note_google_data(&mut d);
    assert!(!atlas::brain::google_data_held());
    atlas::brain::set_google_data_held(false);
    let _ = std::fs::remove_dir_all(dir);
}
