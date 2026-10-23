//! The correction log (23 Oct 2026, `corrlog`): real turns through the
//! daemon, each leaving the line it should in `data/learning/corrections.jsonl`,
//! and the model gate (item 7): a command a model invents is never run.

use atlas::config::Config;
use atlas::corrlog::{Kind, Log, Record};
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const T: u64 = 1_790_900_000;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-corr-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
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
fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(
        c,
        p,
        None,
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    )
}
fn lines(d: &Daemon) -> Vec<Record> {
    Log::at(&d.store.data_dir()).read()
}
fn file_text(d: &Daemon) -> String {
    std::fs::read_to_string(Log::at(&d.store.data_dir()).path()).unwrap_or_default()
}

/// A model that answers with the JSON action, or -- when the text starts
/// with `<tool_call>` -- with a real tool call, as a server's own would.
struct Says(String);
impl atlas::brain::Llm for Says {
    fn complete(&self, _s: &str, _u: &str) -> atlas::error::Result<String> {
        Ok(self.0.clone())
    }
    fn native_chat(&self) -> bool {
        self.0.starts_with("<tool_call>")
    }
    fn chat(
        &self,
        _req: &atlas::brain::ChatRequest,
        _on_text: &mut dyn FnMut(&str) -> bool,
    ) -> atlas::error::Result<atlas::brain::ChatReply> {
        let (text, tool_calls) = atlas::brain::inline_tool_calls(&self.0);
        Ok(atlas::brain::ChatReply { text, tool_calls })
    }
}

#[test]
fn a_correction_writes_what_was_done_and_what_was_meant() {
    let (c, p) = (cfg(), plat());
    p.focus_on(
        "explorer.exe",
        "C:\\Users\\eric\\secret plans - File Explorer",
    );
    let mut d = daemon(&c, &p, "correction");
    d.turn("open notepad", T);
    d.turn("no, I meant open spotify", T + 10);
    let l = lines(&d);
    assert_eq!(l.len(), 1, "{}", file_text(&d));
    let r = &l[0];
    assert_eq!(r.kind, Kind::Correction);
    assert_eq!(r.said, "no, I meant open spotify");
    assert_eq!(r.prev.said, "open notepad");
    assert_eq!(r.prev.intent.as_deref(), Some("open_app"));
    assert_eq!(r.did.intent.as_deref(), Some("open_app"));
    assert_eq!(r.did.detail.as_deref(), Some("notepad"));
    assert_eq!(r.right.intent.as_deref(), Some("open_app"));
    assert_eq!(r.right.detail.as_deref(), Some("spotify"));
    assert_eq!(r.right.from, atlas::corrlog::From::OwnerSaid);
    assert_eq!(r.context.source, "phrase");
    // The program, never the window's title.
    assert_eq!(r.context.app, "explorer.exe");
    assert!(!file_text(&d).contains("secret plans"));
    assert_eq!(r.ts, T + 10);
}

#[test]
fn that_is_wrong_is_written_once_what_was_right_is_known() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "wrong");
    d.turn("open notepad", T);
    d.turn("that's not what I asked", T + 10);
    assert!(lines(&d).is_empty(), "held until what was right is known");
    d.turn("open spotify", T + 20);
    let l = lines(&d);
    assert_eq!(l.len(), 1, "{}", file_text(&d));
    assert_eq!(l[0].kind, Kind::Correction);
    assert_eq!(l[0].did.detail.as_deref(), Some("notepad"));
    assert_eq!(l[0].right.detail.as_deref(), Some("spotify"));
    assert_eq!(l[0].right.from, atlas::corrlog::From::OwnerAnswer);
}

#[test]
fn that_is_wrong_with_no_answer_is_written_with_nothing_known() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "wrong-unanswered");
    d.turn("open notepad", T);
    d.turn("that's not what I asked", T + 10);
    d.turn("mumble mumble", T + 20);
    d.turn("hello", T + 30);
    let l = lines(&d);
    assert_eq!(l.len(), 1, "{}", file_text(&d));
    assert_eq!(l[0].right.intent, None);
    assert_eq!(l[0].right.from, atlas::corrlog::From::OwnerSaid);
}

#[test]
fn an_undo_is_written_with_what_was_undone() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "undo");
    d.turn("open notepad", T);
    d.turn("undo", T + 20);
    let l = lines(&d);
    assert_eq!(l.len(), 1, "{}", file_text(&d));
    assert_eq!(l[0].kind, Kind::Undo);
    assert_eq!(l[0].did.intent.as_deref(), Some("open_app"));
    assert_eq!(l[0].did.detail.as_deref(), Some("notepad"));
    assert_eq!(l[0].right.intent, None);
    assert_eq!(l[0].right.from, atlas::corrlog::From::Undo);
}

#[test]
fn a_yes_and_a_no_to_a_question_are_written() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "yes-no");
    d.turn("open notepad", T);
    d.turn("undo", T + 20);
    d.turn("yes", T + 30); // yes to "Undo ...?"
    d.turn("yes", T + 40); // yes to "Go ahead?" (closing is asked about)
    let l = lines(&d);
    let kinds: Vec<Kind> = l.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        vec![Kind::Undo, Kind::Approval, Kind::Approval],
        "{}",
        file_text(&d)
    );
    assert_eq!(l[1].did.intent.as_deref(), Some("undo"));
    assert_eq!(l[1].right.intent.as_deref(), Some("undo"));
    assert_eq!(l[1].right.from, atlas::corrlog::From::Approval);
    assert_eq!(l[2].did.intent.as_deref(), Some("close_app"));
    assert_eq!(l[2].did.detail.as_deref(), Some("notepad"));
    assert_eq!(l[2].right.intent.as_deref(), Some("close_app"));

    let mut d = daemon(&c, &p, "no");
    d.turn("open notepad", T);
    d.turn("undo", T + 20);
    d.turn("no", T + 30);
    let l = lines(&d);
    let last = l.last().unwrap();
    assert_eq!(last.kind, Kind::Rejection, "{}", file_text(&d));
    assert_eq!(last.right.intent, None);
    assert_eq!(last.right.from, atlas::corrlog::From::Rejection);
    assert_eq!(last.did.intent.as_deref(), Some("undo"));
}

#[test]
fn a_no_to_a_session_approval_is_a_rejection() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "reject");
    d.turn("open notepad", T);
    let q = d.turn("close notepad", T + 10);
    assert!(q.contains("Go ahead?"), "{q}");
    d.turn("no", T + 20);
    let l = lines(&d);
    assert_eq!(l.len(), 1, "{}", file_text(&d));
    assert_eq!(l[0].kind, Kind::Rejection);
    assert_eq!(l[0].did.intent.as_deref(), Some("close_app"));
    assert_eq!(l[0].right.intent, None);
}

#[test]
fn an_answer_to_a_question_asked_because_it_was_unsure_is_the_labelled_example() {
    let (c, p) = (cfg(), plat());
    p.focus_on("notepad.exe", "Draft - Notepad");
    let mut d = daemon(&c, &p, "clarified");
    let q = d.turn("close it", T);
    assert!(q.contains("Did you mean Notepad"), "{q}");
    d.turn("open spotify", T + 10);
    let l = lines(&d);
    assert_eq!(l.len(), 1, "{}", file_text(&d));
    let r = &l[0];
    assert_eq!(r.kind, Kind::Clarified);
    assert_eq!(r.said, "open spotify");
    assert_eq!(r.prev.said, "close it");
    let ask = r.ask.as_ref().expect("the question is kept");
    assert!(ask.question.contains("Did you mean Notepad"), "{ask:?}");
    assert!(ask.options.iter().any(|o| o.contains("Notepad")), "{ask:?}");
    assert_eq!(r.right.intent.as_deref(), Some("open_app"));
    assert_eq!(r.right.detail.as_deref(), Some("spotify"));
    assert_eq!(r.right.from, atlas::corrlog::From::OwnerAnswer);
    assert_eq!(r.context.source, "ask");
    assert_eq!(r.context.app, "notepad.exe");
}

#[test]
fn a_secret_said_or_handled_leaves_nothing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "secret");
    d.turn("open notepad", T);
    d.turn("no, my password is hunter2 -- I meant open spotify", T + 10);
    // Sign-in: the yes to its question is not written either.
    d.turn("sign me into github", T + 20);
    d.turn("yes", T + 30);
    assert!(lines(&d).is_empty(), "{}", file_text(&d));
    assert!(!file_text(&d).contains("hunter2"));
}

#[test]
fn what_an_email_said_is_not_kept_only_what_you_said() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "outside");
    d.turn("open notepad", T);
    d.session.tainted_by_outside = true;
    d.turn("undo", T + 20);
    let l = lines(&d);
    assert_eq!(l.len(), 1, "{}", file_text(&d));
    assert_eq!(l[0].prev.said, "open notepad");
    assert_eq!(l[0].prev.reply, "");
    assert!(l[0].did.detail.is_none());
}

#[test]
fn forgetting_by_voice_and_stopping_and_starting() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "forget");
    d.turn("open notepad", T);
    d.turn("undo", T + 20);
    d.turn("no", T + 30);
    assert_eq!(lines(&d).len(), 2, "{}", file_text(&d));
    // Later: a second chain.
    d.turn("close notepad", T + 1000);
    d.turn("no", T + 1010);
    assert_eq!(lines(&d).len(), 3, "{}", file_text(&d));
    let r = d.turn("forget that", T + 1020);
    assert!(r.contains("1 line"), "{r}");
    let l = lines(&d);
    assert_eq!(l.len(), 2);
    assert_eq!(l[0].kind, Kind::Undo);
    let r = d.turn("forget what I said about notepad", T + 1030);
    assert!(r.contains("2 lines"), "{r}");
    assert!(lines(&d).is_empty());

    // Off: nothing is written.
    let r = d.turn("stop learning from me", T + 2000);
    assert!(r.contains("stopped"), "{r}");
    d.turn("open notepad", T + 2010);
    d.turn("undo", T + 2020);
    d.turn("no", T + 2030);
    assert!(lines(&d).is_empty(), "written while off: {}", file_text(&d));
    let r = d.turn("start learning again", T + 3000);
    assert!(r.contains("again"), "{r}");
    d.turn("open notepad", T + 3010);
    d.turn("undo", T + 3020);
    assert_eq!(lines(&d).len(), 1, "{}", file_text(&d));
    let r = d.turn("clear what you've learned", T + 3030);
    assert!(r.contains("emptied"), "{r}");
    assert!(lines(&d).is_empty());
}

#[test]
fn forgetting_everything_about_how_you_talk_forgets_the_lines_too() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "phrase");
    d.turn("open notepad", T);
    d.turn("undo", T + 20);
    let r = d.turn("forget everything you learned about how I talk", T + 40);
    assert!(r.contains("corrections log"), "{r}");
    assert!(lines(&d).is_empty());
}

// ============ item 7: a command a model invents is not run ============

/// The two shapes a model's answer takes: a tool call, and the JSON action.
fn model_says(name: &str) -> [String; 2] {
    [
        format!("<tool_call>{{\"name\":\"{name}\",\"arguments\":{{\"arg\":\"notepad\"}}}}</tool_call>"),
        format!("{{\"action\":\"{name}\",\"arg\":\"notepad\",\"say\":\"on it\"}}"),
    ]
}

fn ask_the_model(c: &Config, p: &MockPlatform, tag: &str, reply: String) -> (String, Vec<Record>) {
    let llm: std::sync::Arc<dyn atlas::brain::Llm> = std::sync::Arc::new(Says(reply));
    let mut d = Daemon::new(
        c,
        p,
        Some(llm),
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    );
    let r = d.turn("I need to jot something down in notepad", T);
    (r, lines(&d))
}

#[test]
fn a_command_a_model_invents_is_not_run_and_is_handed_on() {
    let c = cfg();
    for invented in [
        "delete_everything",
        "openn_app",
        "open_app_now",
        "OPEN_APP",
        "open app",
        "../open_app",
    ] {
        for reply in model_says(invented) {
            let p = plat();
            let (r, log) = ask_the_model(&c, &p, "invented", reply.clone());
            assert!(p.actions().is_empty(), "{invented}: ran {:?}", p.actions());
            assert!(!r.contains("Notepad is up"), "{invented}: {r}");
            assert!(log.is_empty(), "a rejected name is not for the correction log");
        }
    }
    // A real name still works through the same two paths.
    for reply in model_says("open_app") {
        let p = plat();
        let (r, _) = ask_the_model(&c, &p, "real", reply.clone());
        assert!(
            p.actions().iter().any(|a| format!("{a:?}").contains("notepad")),
            "{reply}: {r} {:?}",
            p.actions()
        );
    }
}

#[test]
fn the_gate_turns_away_names_and_shapes_and_keeps_real_ones() {
    use atlas::intent::{gate_model_call, take_rejected_names, Intent, Rejected};
    use serde_json::json;
    let _ = take_rejected_names();
    let long = "x".repeat(80);
    for name in [
        "delete_everything",
        "openn_app",
        "open_apps",
        "Open_App",
        " ",
        "",
        "open app",
        "open_app;rm",
        long.as_str(),
    ] {
        assert_eq!(
            gate_model_call(name, &json!({"arg": "notepad"}), "x"),
            Err(Rejected::NotACommand),
            "{name:?}"
        );
    }
    // A real command the model may not choose.
    assert_eq!(
        gate_model_call("unlock", &json!("pw"), "x"),
        Err(Rejected::NotForTheModel)
    );
    // The wrong shape of arguments.
    for bad in [
        json!([1, 2]),
        json!(5),
        json!(true),
        json!({"arg": 5}),
        json!({"arg": ["a"]}),
        json!({"x": 5}),
    ] {
        assert_eq!(
            gate_model_call("open_app", &bad, "x"),
            Err(Rejected::BadArguments),
            "{bad}"
        );
    }
    // Real names, in every shape the models use.
    for ok in [
        json!("notepad"),
        json!({"arg": "notepad"}),
        json!({"app": "notepad"}),
    ] {
        assert_eq!(
            gate_model_call("open_app", &ok, "open notepad"),
            Ok(Intent::OpenApp("notepad".into())),
            "{ok}"
        );
    }
    assert_eq!(gate_model_call("clock", &json!({}), "x"), Ok(Intent::Clock));
    // Noted once each, for the log.
    let noted = take_rejected_names();
    assert!(
        noted.contains(&"delete_everything".to_string())
            && noted.contains(&"openn_app".to_string()),
        "{noted:?}"
    );
    assert_eq!(noted.iter().filter(|n| *n == "openn_app").count(), 1);
}

// ============ it never leaves the laptop ============

#[test]
fn the_log_is_not_in_a_backup_or_a_sync() {
    let root = Path::new("src");
    let safety = std::fs::read_to_string(root.join("safety.rs")).unwrap();
    assert!(
        !safety.contains("\"learning\""),
        "a backup folder list names learning"
    );
    let log = Log::at(Path::new("data"));
    assert!(
        !log.path().starts_with(Path::new("data").join("state")),
        "inside the state folder"
    );
    for f in [
        "sync.rs",
        "cloudsync.rs",
        "mesh.rs",
        "meshio.rs",
        "backed.rs",
        "elsewhere.rs",
    ] {
        if let Ok(t) = std::fs::read_to_string(root.join(f)) {
            assert!(
                !t.contains("corrections.jsonl") && !t.contains("corrlog"),
                "{f} reaches the log"
            );
        }
    }
}
