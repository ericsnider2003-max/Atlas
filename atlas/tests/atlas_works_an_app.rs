//! **Atlas works an app, step by step (30 Sep 2026, `operate`).**
//!
//! A pretend order form -- a name box, a gift-wrap tick box, Save draft and
//! Send order -- behind the same `Platform` calls Windows answers (the
//! window's tree, acting on a control by its place in it), and a model that
//! chooses one step at a time from what it's shown. The whole loop runs
//! through the daemon's front door and its tick, as on the laptop: look,
//! choose, check, act, look again.

use atlas::brain::{ChatReply, ChatRequest, Llm, ToolCall};
use atlas::config::{AppSpec, Config};
use atlas::daemon::Daemon;
use atlas::platform::{ActiveWindow, Monitor, PixelRect, Platform, WindowId};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::uia::{Node, Role, UiAct};
use std::cell::RefCell;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
struct Form {
    name: String,
    gift: bool,
    status: String,
    sent: bool,
    acts: Vec<String>,
}

struct FormApp {
    s: RefCell<Form>,
}

impl FormApp {
    fn tree(&self) -> Node {
        let s = self.s.borrow();
        Node::new(Role::Window, "Order form").with(vec![
            Node::new(Role::Text, "Your name"),
            Node::new(Role::Edit, "Name").valued(&s.name).at(10, 10, 200, 20),
            Node::new(Role::CheckBox, "Gift wrap").valued(if s.gift { "on" } else { "off" }),
            Node::new(Role::Button, "Save draft").at(10, 50, 80, 20),
            Node::new(Role::Button, "Send order").at(100, 50, 80, 20),
            Node::new(Role::Text, if s.status.is_empty() { "Not saved" } else { &s.status }),
        ])
    }
}

impl Platform for FormApp {
    fn active_window(&self) -> atlas::error::Result<Option<ActiveWindow>> {
        Ok(Some(ActiveWindow { process: "orders".into(), title: "Order form".into() }))
    }
    fn active_window_id(&self) -> atlas::error::Result<Option<WindowId>> {
        Ok(Some(WindowId(7)))
    }
    fn input_idle_secs(&self) -> Option<u64> {
        Some(100)
    }
    fn monitors(&self) -> atlas::error::Result<Vec<Monitor>> {
        Ok(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
    }
    fn launch(&self, _spec: &AppSpec) -> atlas::error::Result<()> {
        Ok(())
    }
    fn find_window(&self, _spec: &AppSpec) -> atlas::error::Result<Option<WindowId>> {
        Ok(None)
    }
    fn place(&self, _win: WindowId, _rect: PixelRect) -> atlas::error::Result<()> {
        Ok(())
    }
    fn focus(&self, _win: WindowId) -> atlas::error::Result<()> {
        Ok(())
    }
    fn close(&self, _spec: &AppSpec) -> atlas::error::Result<()> {
        Ok(())
    }
    fn sleep_ms(&self, _ms: u64) {}
    fn read_window(&self, _win: WindowId) -> atlas::error::Result<Option<Node>> {
        Ok(Some(self.tree()))
    }
    fn act_on(&self, _win: WindowId, path: &[usize], act: &UiAct) -> atlas::error::Result<bool> {
        let mut s = self.s.borrow_mut();
        s.acts.push(format!("{path:?} {act:?}"));
        Ok(match (path, act) {
            ([1], UiAct::SetValue(v)) => {
                s.name = v.clone();
                true
            }
            ([2], UiAct::Toggle) => {
                s.gift = !s.gift;
                true
            }
            ([3], UiAct::Invoke) => {
                s.status = "Draft saved".into();
                true
            }
            ([4], UiAct::Invoke) => {
                s.status = "Order sent".into();
                s.sent = true;
                true
            }
            _ => false,
        })
    }
}

/// A model that reads the list it's shown and picks the next step the way a
/// careful person would. What it was shown is kept.
struct Chooser {
    shown: Mutex<Vec<String>>,
}

fn call(name: &str, arg: &str) -> ChatReply {
    ChatReply { text: String::new(), tool_calls: vec![ToolCall { name: name.into(), arguments: serde_json::json!({ "arg": arg }) }] }
}

impl Llm for Chooser {
    fn complete(&self, _s: &str, _u: &str) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, _on: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        let u = req.messages.last().map(|m| m.content.clone()).unwrap_or_default();
        self.shown.lock().unwrap().push(u.clone());
        assert!(req.force_tool, "a step is always a tool call");
        let send = u.contains("Goal: send the order");
        Ok(if u.contains("[1] text box \"Name\"\n") && !send {
            call("type", "1: Eric")
        } else if u.contains("\"Gift wrap\" = \"off\"") && !send {
            call("click", "2")
        } else if u.contains("Not saved") && !send {
            call("click", "3")
        } else if send && !u.contains("Order sent") {
            call("click", "4")
        } else {
            call("done", if send { "The order is sent." } else { "Filled in and saved as a draft." })
        })
    }
}

fn run(goal: &str, answers: &[&str]) -> (Form, Vec<String>, Vec<String>) {
    let c = Config::load(Path::new("config")).unwrap();
    let app = FormApp { s: RefCell::new(Form::default()) };
    let llm = Arc::new(Chooser { shown: Mutex::new(Vec::new()) });
    let dir = std::env::temp_dir().join(format!("atlas-operate-{}-{}", goal.len(), std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut d = Daemon::new(&c, &app, Some(llm.clone() as Arc<dyn Llm>), Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    let mut said = vec![d.turn(goal, 1_790_760_000)];
    let mut answers = answers.iter();
    let mut t = 1_790_760_001;
    for _ in 0..400 {
        if d.operating.is_none() {
            break;
        }
        if d.operating.as_ref().is_some_and(|j| j.waiting_on_you) {
            match answers.next() {
                Some(a) => said.push(d.turn(a, t)),
                None => break,
            }
        }
        said.extend(d.tick(t));
        t += 1;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let _ = std::fs::remove_dir_all(&dir);
    let shown = llm.shown.lock().unwrap().clone();
    let form = app.s.borrow().clone();
    (form, said, shown)
}

#[test]
fn a_form_is_filled_in_and_saved_a_step_at_a_time() {
    let (form, said, shown) = run("fill in this form: name Eric, gift wrap on, then save a draft", &[]);
    assert_eq!(form.name, "Eric", "{said:?}");
    assert!(form.gift, "{said:?}");
    assert_eq!(form.status, "Draft saved", "{said:?}");
    assert!(!form.sent, "nothing was sent");
    // Through the controls' own patterns, not clicks on the screen.
    assert_eq!(form.acts, vec!["[1] SetValue(\"Eric\")", "[2] Toggle", "[3] Invoke"]);
    // Each look after a step showed the step's result.
    assert!(shown.iter().any(|s| s.contains("= \"Eric\"")), "{shown:?}");
    assert!(said.iter().any(|s| s.contains("Filled in and saved as a draft")), "{said:?}");
}

#[test]
fn a_press_that_cant_be_taken_back_waits_for_a_yes() {
    let (form, said, _) = run("fill in this form. Goal: send the order", &[]);
    assert!(!form.sent, "sent without asking: {said:?}");
    assert!(said.iter().any(|s| s.contains("\"Send order\"") && s.contains("can't be taken back")), "{said:?}");

    let (form, said, _) = run("fill in this form. Goal: send the order", &["yes"]);
    assert!(form.sent, "the yes didn't send it: {said:?}");
    assert!(said.iter().any(|s| s.contains("The order is sent")), "{said:?}");

    let (form, said, _) = run("fill in this form. Goal: send the order", &["no"]);
    assert!(!form.sent, "{said:?}");
    assert!(said.iter().any(|s| s.starts_with("Stopped")), "{said:?}");
}

#[test]
fn stop_ends_the_job_where_it_is() {
    let c = Config::load(Path::new("config")).unwrap();
    let app = FormApp { s: RefCell::new(Form::default()) };
    let llm = Arc::new(Chooser { shown: Mutex::new(Vec::new()) });
    let dir = std::env::temp_dir().join(format!("atlas-operate-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut d = Daemon::new(&c, &app, Some(llm as Arc<dyn Llm>), Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    d.turn("fill in this form: name Eric", 1_790_760_000);
    assert!(d.operating.is_some());
    let r = d.turn("stop", 1_790_760_002);
    assert!(r.starts_with("Stopped"), "{r}");
    assert!(d.operating.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same form, worked by a real model (`ATLAS_REAL_MODEL_URL`): what a
/// small model does with the numbered list and the step tools.
#[test]
fn a_real_model_fills_in_the_form() {
    let Ok(url) = std::env::var("ATLAS_REAL_MODEL_URL") else { return };
    struct Real(String, Mutex<Vec<String>>);
    impl Llm for Real {
        fn complete(&self, _s: &str, _u: &str) -> atlas::error::Result<String> {
            Ok(String::new())
        }
        fn native_chat(&self) -> bool {
            true
        }
        fn chat(&self, req: &ChatRequest, on: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
            let started = std::time::Instant::now();
            let r = atlas::models::chat_call(&self.0, req, on);
            let shown = req.messages.last().map(|m| m.content.clone()).unwrap_or_default();
            self.1.lock().unwrap().push(format!("{:?} in {:?} after:\n{}", r.as_ref().map(|r| (r.text.clone(), r.tool_calls.clone())), started.elapsed(), shown));
            r
        }
    }
    let c = Config::load(Path::new("config")).unwrap();
    let app = FormApp { s: RefCell::new(Form::default()) };
    let llm = Arc::new(Real(url, Mutex::new(Vec::new())));
    let dir = std::env::temp_dir().join(format!("atlas-operate-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut d = Daemon::new(&c, &app, Some(llm.clone() as Arc<dyn Llm>), Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    let mut said = vec![d.turn("fill in this form: name Eric, tick gift wrap, then save a draft", 1_790_760_000)];
    let mut t = 1_790_760_001;
    let started = std::time::Instant::now();
    while d.operating.is_some() && started.elapsed() < std::time::Duration::from_secs(900) {
        said.extend(d.tick(t));
        t += 1;
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    for s in llm.1.lock().unwrap().iter() {
        println!("STEP {s}\n");
    }
    println!("SAID {said:?}");
    let form = app.s.borrow().clone();
    println!("FORM name={:?} gift={} status={:?} acts={:?}", form.name, form.gift, form.status, form.acts);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(form.name, "Eric");
    assert!(form.gift);
    assert_eq!(form.status, "Draft saved");
    assert!(!form.sent);
}
