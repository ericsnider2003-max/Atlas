//! Doing things in apps (30 Sep 2026).
//!
//! "In Excel, make a new sheet called Budget." "Fill in this form with my
//! address." "Turn on dark mode in Settings." Atlas works the app the way a
//! person would, one step at a time, and checks each step took:
//!
//! 1. **Look.** The window's controls, read through UI Automation -- what a
//!    screen reader hears -- as a numbered list: role, name, value, whether
//!    it can be used (`view_of_tree`). An app that shows Windows nothing
//!    (some Electron and custom-drawn apps) is read off its picture instead,
//!    line by line, by Windows' own text recognition (`view_of_text`).
//! 2. **Choose one step.** The model is given the goal, the list and the
//!    steps so far, and must call exactly one of a few actions (`tools`):
//!    click, type, choose, key, scroll, open an app, done, ask you, or give
//!    up. The call is held to that shape (`models::chat_body`'s schema).
//! 3. **Check it before doing it** (`guard`): a button whose press can't be
//!    taken back (Send, Delete, Pay, Buy...) stops and asks you, every time;
//!    a step aimed at something not on the list is refused and the model is
//!    told why.
//! 4. **Do it** through the control's own pattern (press, set the text,
//!    tick, select, open), or, where the control has none, by clicking its
//!    middle on the screen.
//! 5. **Look again.** A step that changed nothing three times running ends
//!    the job, said plainly with how far it got -- never "done" when it
//!    isn't.
//!
//! It works in the foreground lane: it waits for your hands to be off the
//! keyboard unless you asked just now, Pause holds it, and "stop" ends it.
//! At most `MOST_STEPS` steps.

use crate::uia::{Node, Role, UiAct};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The most steps one job takes before it stops and says how far it got.
pub const MOST_STEPS: usize = 30;
/// A step that changes nothing this many times running ends the job.
pub const MOST_UNCHANGED: u32 = 3;
/// The longest control list the model is shown.
pub const MOST_TARGETS: usize = 120;

/// Something on the screen a step can be aimed at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Target {
    /// A control in the window's tree: its path, role, name and bounds.
    Control { path: Vec<usize>, role: Role, name: String, rect: Option<[i32; 4]> },
    /// A line of words read off the window's picture, and where (screen pixels).
    Words { text: String, rect: [i32; 4] },
}

impl Target {
    pub fn name(&self) -> &str {
        match self {
            Target::Control { name, .. } => name,
            Target::Words { text, .. } => text,
        }
    }
    /// The middle of it on the screen, to click when nothing better works.
    pub fn middle(&self) -> Option<(i32, i32)> {
        let r = match self {
            Target::Control { rect, .. } => (*rect)?,
            Target::Words { rect, .. } => *rect,
        };
        Some((r[0] + r[2] / 2, r[1] + r[3] / 2))
    }
}

/// How the window was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Seen {
    /// Its controls, through UI Automation.
    Controls,
    /// Words read off its picture: the app doesn't show Windows its controls.
    Picture,
}

/// One look at the window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub title: String,
    pub seen: Seen,
    /// What the model reads: numbered targets and, unnumbered, the text around them.
    pub lines: Vec<String>,
    pub targets: Vec<Target>,
}

impl View {
    /// Something that changes when the window does: a step that leaves this
    /// the same did nothing.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.title.hash(&mut h);
        self.lines.hash(&mut h);
        h.finish()
    }
}

fn role_word(r: Role) -> &'static str {
    match r {
        Role::Button => "button",
        Role::Edit => "text box",
        Role::CheckBox => "tick box",
        Role::RadioButton => "option",
        Role::ComboBox => "drop-down",
        Role::List => "list",
        Role::ListItem => "list item",
        Role::Link => "link",
        Role::MenuItem => "menu item",
        Role::Tab => "tab",
        Role::Document => "document",
        Role::Text => "text",
        _ => "control",
    }
}

fn can_aim_at(r: Role) -> bool {
    matches!(
        r,
        Role::Button | Role::Edit | Role::CheckBox | Role::RadioButton | Role::ComboBox | Role::ListItem | Role::Link | Role::MenuItem | Role::Tab | Role::Document
    )
}

/// The window's controls as a numbered list (`MOST_TARGETS` at most), with
/// the text around them unnumbered so the model knows where it is.
pub fn view_of_tree(root: &Node, title: &str) -> View {
    let mut lines = Vec::new();
    let mut targets = Vec::new();
    fn walk(n: &Node, path: &mut Vec<usize>, lines: &mut Vec<String>, targets: &mut Vec<Target>) {
        if targets.len() >= MOST_TARGETS {
            return;
        }
        let name = n.name.trim();
        if can_aim_at(n.role) && (!name.is_empty() || n.role == Role::Edit || n.role == Role::Document) {
            let num = targets.len() + 1;
            let mut line = format!("[{num}] {} \"{}\"", role_word(n.role), name.chars().take(80).collect::<String>());
            let v = n.value.trim();
            if !v.is_empty() {
                line.push_str(&format!(" = \"{}\"", v.chars().take(80).collect::<String>()));
            }
            if !n.enabled {
                line.push_str(" (greyed out)");
            }
            lines.push(line);
            targets.push(Target::Control { path: path.clone(), role: n.role, name: name.to_string(), rect: n.rect });
        } else if n.role == Role::Text && !name.is_empty() && lines.len() < MOST_TARGETS * 2 {
            lines.push(format!("    {}", name.chars().take(100).collect::<String>()));
        }
        for (i, c) in n.children.iter().enumerate() {
            path.push(i);
            walk(c, path, lines, targets);
            path.pop();
        }
    }
    walk(root, &mut Vec::new(), &mut lines, &mut targets);
    View { title: title.to_string(), seen: Seen::Controls, lines, targets }
}

/// The window's words read off its picture, each line a target. `origin` is
/// where the picture's top-left is on the screen.
pub fn view_of_text(found: &[(String, crate::platform::PixelRect)], origin: (i32, i32), title: &str) -> View {
    let mut lines = Vec::new();
    let mut targets = Vec::new();
    for (text, r) in found.iter().take(MOST_TARGETS) {
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        let num = targets.len() + 1;
        lines.push(format!("[{num}] \"{}\"", t.chars().take(100).collect::<String>()));
        targets.push(Target::Words { text: t.to_string(), rect: [origin.0 + r.x, origin.1 + r.y, r.width, r.height] });
    }
    View { title: title.to_string(), seen: Seen::Picture, lines, targets }
}

/// Is the tree worth working from? Too few named controls, and the picture
/// is read instead (`uia::assess`'s idea: an Electron app's tree is often
/// one unnamed blob).
pub fn tree_is_usable(view: &View) -> bool {
    view.targets.len() >= 2
}

/// One step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Click(usize),
    Type(usize, String),
    Choose(usize),
    Key(String),
    Scroll(i32),
    Open(String),
    Done(String),
    Ask(String),
    GiveUp(String),
}

impl Action {
    pub fn plain(&self, view: &View) -> String {
        let name = |n: &usize| view.targets.get(n.wrapping_sub(1)).map(|t| format!("\"{}\"", t.name())).unwrap_or_else(|| format!("#{n}"));
        match self {
            Action::Click(n) => format!("clicked {}", name(n)),
            Action::Type(n, t) => format!("typed \"{}\" into {}", t.chars().take(40).collect::<String>(), name(n)),
            Action::Choose(n) => format!("chose {}", name(n)),
            Action::Key(k) => format!("pressed {k}"),
            Action::Scroll(d) => format!("scrolled {}", if *d < 0 { "up" } else { "down" }),
            Action::Open(a) => format!("opened {a}"),
            Action::Done(s) => format!("finished: {s}"),
            Action::Ask(q) => format!("asked: {q}"),
            Action::GiveUp(w) => format!("gave up: {w}"),
        }
    }
}

/// The steps the model may take, as tools. Each takes one `arg`, so the
/// call can be held to `{name, arg}` (`models::forced_call`).
pub fn tools() -> Vec<Value> {
    let t = |name: &str, what: &str, arg: &str| {
        json!({ "type": "function", "function": { "name": name, "description": what,
            "parameters": { "type": "object", "properties": { "arg": { "type": "string", "description": arg } }, "required": ["arg"] } } })
    };
    vec![
        t("click", "Click a numbered control or line.", "its number, e.g. 7"),
        t("type", "Put text into a numbered text box (replacing what's there).", "its number, a colon, then the text, e.g. 3: hello"),
        t("choose", "Pick a numbered list item, tab, option or drop-down.", "its number"),
        t("key", "Press keys in the window, e.g. ctrl+s, enter, tab, alt+f.", "the keys"),
        t("scroll", "Scroll the window to see more.", "up or down"),
        t("open_app", "Open or switch to another app.", "the app's name"),
        t("done", "The goal is reached and you can see that it is.", "what was done, in one sentence for the user"),
        t("ask", "Something only the user can say (which file, what to write).", "the question"),
        t("give_up", "It can't be done here; say why.", "why, in one sentence"),
    ]
}

/// A step from the model's call, or why it isn't one.
pub fn read_call(name: &str, arg: &str) -> Result<Action, String> {
    let arg = arg.trim();
    let num = |s: &str| s.trim().trim_start_matches('#').trim_start_matches('[').trim_end_matches(']').parse::<usize>().map_err(|_| format!("\"{s}\" isn't one of the numbers"));
    Ok(match name {
        "click" => Action::Click(num(arg)?),
        "choose" => Action::Choose(num(arg)?),
        "type" => {
            let (n, text) = arg.split_once(':').ok_or_else(|| "type needs a number, a colon and the text".to_string())?;
            Action::Type(num(n)?, text.trim_start().to_string())
        }
        "key" => Action::Key(arg.to_lowercase()),
        "scroll" => Action::Scroll(if arg.to_lowercase().contains("up") { -5 } else { 5 }),
        "open_app" => Action::Open(arg.to_string()),
        "done" => Action::Done(arg.to_string()),
        "ask" => Action::Ask(arg.to_string()),
        "give_up" => Action::GiveUp(arg.to_string()),
        other => return Err(format!("{other} isn't a step")),
    })
}

/// What `guard` says about a step before it's taken.
#[derive(Debug, Clone, PartialEq)]
pub enum Guard {
    Go,
    /// It can't be taken back: ask first.
    AskFirst(String),
    /// Not a step that can be taken here: told to the model as the result.
    Refuse(String),
}

/// Look at a step before taking it. `allowed` is a button you've already
/// said yes to pressing.
pub fn guard(action: &Action, view: &View, allowed: Option<&str>) -> Guard {
    let target = |n: usize| view.targets.get(n.wrapping_sub(1));
    match action {
        Action::Click(n) | Action::Choose(n) | Action::Type(n, _) => {
            let Some(t) = target(*n) else {
                return Guard::Refuse(format!("there's no [{n}] -- pick one of the numbers shown"));
            };
            let name = t.name();
            // A click and a choice alike: "choose Send" from a list sends.
            if matches!(action, Action::Click(_) | Action::Choose(_)) {
                match crate::policy::press(name) {
                    crate::policy::Press::Never => {
                        return Guard::Refuse(format!("\"{name}\" pays for something -- I don't buy things"));
                    }
                    crate::policy::Press::AskFirst if allowed.map(|a| !a.eq_ignore_ascii_case(name)).unwrap_or(true) => {
                        return Guard::AskFirst(name.to_string());
                    }
                    _ => {}
                }
            }
            Guard::Go
        }
        Action::Key(k) => {
            // Enter beside a Send button sends: asked about the same. Any
            // target counts -- a window read as a picture has words, not
            // buttons, and "Send" there is still a Send.
            if k == "enter" || k == "ctrl+enter" {
                if let Some(t) = view.targets.iter().find(|t| crate::policy::press(t.name()) == crate::policy::Press::Never) {
                    return Guard::Refuse(format!("Enter here could press \"{}\", which pays for something -- I don't buy things", t.name()));
                }
                let what = view.targets.iter().find(|t| crate::policy::press(t.name()) == crate::policy::Press::AskFirst);
                if let (Some(t), None) = (what, allowed) {
                    return Guard::AskFirst(t.name().to_string());
                }
            }
            if k.contains("alt+f4") || k.contains("ctrl+w") {
                return Guard::Refuse("closing the window isn't a step here -- say done or give up".into());
            }
            Guard::Go
        }
        _ => Guard::Go,
    }
}

/// The UI Automation act for a click or a choice on a control of this role.
pub fn acts_for(action: &Action, role: Role) -> Vec<UiAct> {
    match (action, role) {
        (Action::Type(_, t), _) => vec![UiAct::SetValue(t.clone())],
        (_, Role::CheckBox) => vec![UiAct::Toggle, UiAct::Invoke],
        (_, Role::ComboBox) => vec![UiAct::Expand, UiAct::Invoke],
        (_, Role::ListItem | Role::Tab | Role::RadioButton) => vec![UiAct::Select, UiAct::Invoke],
        (_, Role::Edit | Role::Document) => vec![UiAct::Focus],
        _ => vec![UiAct::Invoke, UiAct::Select, UiAct::Toggle],
    }
}

/// One job: a goal in an app, worked a step at a time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub goal: String,
    /// The app, as you named it (empty: the window in front when asked).
    pub app: String,
    pub win: Option<u64>,
    /// What has been done, in words, for the model and for you.
    pub steps: Vec<String>,
    /// The last step's result, told to the model with the next look.
    pub last_result: String,
    pub unchanged: u32,
    pub last_fingerprint: Option<u64>,
    /// A button you've said yes to pressing, once.
    pub allowed: Option<String>,
    /// Waiting on the model for the next step (the crew's id).
    pub thinking: Option<u64>,
    /// The step the model chose, to take on the next pass.
    pub next: Option<(String, String)>,
    /// Waiting on you (a question asked, or a yes to an irreversible press).
    pub waiting_on_you: bool,
    /// The view the model was shown, for reading its numbers back.
    pub view: Option<View>,
    pub started: u64,
    /// When the app was asked to open, to give up on a window that never comes.
    pub opened_at: Option<u64>,
    /// A button waiting on your yes, and the step that presses it.
    pub pending_press: Option<String>,
    pub held: Option<(String, String)>,
    /// The model failing to choose a step, running.
    pub model_failures: u32,
}

impl Job {
    pub fn new(goal: &str, app: &str, t: u64) -> Job {
        Job {
            goal: goal.trim().to_string(),
            app: app.trim().to_string(),
            win: None,
            steps: Vec::new(),
            last_result: String::new(),
            unchanged: 0,
            last_fingerprint: None,
            allowed: None,
            thinking: None,
            next: None,
            waiting_on_you: false,
            view: None,
            started: t,
            opened_at: None,
            pending_press: None,
            held: None,
            model_failures: 0,
        }
    }
}

/// The system and user messages for the next step.
pub fn prompt(job: &Job, view: &View) -> (String, String) {
    let system = "You operate an app on the user's Windows computer for them, one step at a time. \
Each turn you see the window's numbered controls and must call exactly one tool. \
Use the numbers shown. Prefer the app's own controls; use keys for shortcuts. \
Call done only when what you see shows the goal is reached. Call ask when only the user can decide. \
Call give_up if it can't be done in this app. Never guess at personal details."
        .to_string();
    let mut u = format!("Goal: {}\nWindow: {}\n", job.goal, view.title);
    if view.seen == Seen::Picture {
        u.push_str("(This app doesn't show its controls, so these are the words read off it; clicking one clicks where it is.)\n");
    }
    if !job.steps.is_empty() {
        u.push_str("Done so far:\n");
        for (i, s) in job.steps.iter().enumerate().rev().take(10).collect::<Vec<_>>().into_iter().rev() {
            u.push_str(&format!("  {}. {s}\n", i + 1));
        }
    }
    if !job.last_result.is_empty() {
        u.push_str(&format!("Last step: {}\n", job.last_result));
    }
    u.push_str("Now on screen:\n");
    for l in &view.lines {
        u.push_str(l);
        u.push('\n');
    }
    u.push_str("One tool call: the next step.");
    (system, u)
}

/// "In Excel, make a sheet called Budget" -> ("Excel", "make a sheet called
/// Budget"); "fill in this form" -> ("", the whole). `apps` are the names
/// Atlas knows.
pub fn app_and_goal(said: &str, apps: &[String]) -> (String, String) {
    let s = said.trim().trim_end_matches(['.', '!']);
    let l = s.to_lowercase();
    for a in apps {
        let al = a.to_lowercase();
        for lead in [format!("in {al}, "), format!("in {al} "), format!("use {al} to "), format!("open {al} and "), format!("go into {al} and ")] {
            if let Some(i) = l.find(&lead) {
                let goal = format!("{}{}", &s[..i], &s[i + lead.len()..]).trim().trim_start_matches(',').trim().to_string();
                return (a.clone(), goal);
            }
        }
        for tail in [format!(" in {al}"), format!(" on {al}")] {
            if l.ends_with(&tail) {
                return (a.clone(), s[..s.len() - tail.len()].trim().to_string());
            }
        }
    }
    (String::new(), s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Node {
        Node::new(Role::Window, "Order form").with(vec![
            Node::new(Role::Text, "Your name"),
            Node::new(Role::Edit, "Name").at(10, 10, 200, 20),
            Node::new(Role::CheckBox, "Gift wrap"),
            Node::new(Role::Button, "Save draft").at(10, 50, 80, 20),
            Node::new(Role::Button, "Send order").at(100, 50, 80, 20),
        ])
    }

    #[test]
    fn the_view_numbers_what_can_be_used() {
        let v = view_of_tree(&form(), "Order form");
        assert_eq!(v.targets.len(), 4);
        assert!(v.lines.iter().any(|l| l == "[1] text box \"Name\""), "{:?}", v.lines);
        assert!(v.lines.iter().any(|l| l.contains("Your name")));
        assert_eq!(v.targets[0], Target::Control { path: vec![1], role: Role::Edit, name: "Name".into(), rect: Some([10, 10, 200, 20]) });
        assert_eq!(v.targets[3].middle(), Some((140, 60)));
    }

    #[test]
    fn a_button_that_cant_be_undone_is_asked_about() {
        let v = view_of_tree(&form(), "Order form");
        assert_eq!(guard(&Action::Click(4), &v, None), Guard::AskFirst("Send order".into()));
        assert_eq!(guard(&Action::Click(4), &v, Some("send order")), Guard::Go);
        assert_eq!(guard(&Action::Click(3), &v, None), Guard::Go);
        assert!(matches!(guard(&Action::Click(9), &v, None), Guard::Refuse(_)));
        assert_eq!(guard(&Action::Key("enter".into()), &v, None), Guard::AskFirst("Send order".into()));
    }

    #[test]
    fn calls_are_read_into_steps() {
        assert_eq!(read_call("type", "1: Eric Snider"), Ok(Action::Type(1, "Eric Snider".into())));
        assert_eq!(read_call("click", "[4]"), Ok(Action::Click(4)));
        assert!(read_call("click", "the save button").is_err());
        assert_eq!(read_call("scroll", "down"), Ok(Action::Scroll(5)));
    }

    #[test]
    fn the_app_is_taken_from_the_sentence() {
        let apps = vec!["Excel".to_string(), "Settings".to_string()];
        assert_eq!(app_and_goal("In Excel, make a new sheet called Budget", &apps), ("Excel".into(), "make a new sheet called Budget".into()));
        assert_eq!(app_and_goal("turn on dark mode in settings", &apps), ("Settings".into(), "turn on dark mode".into()));
        assert_eq!(app_and_goal("fill in this form", &apps), (String::new(), "fill in this form".into()));
    }
}

#[cfg(test)]
mod presses {
    use super::*;

    fn view(names: &[&str], picture: bool) -> View {
        let targets = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                if picture {
                    Target::Words { text: n.to_string(), rect: [0, (i as i32) * 20, 100, 18] }
                } else {
                    Target::Control { path: vec![i], role: Role::ListItem, name: n.to_string(), rect: None }
                }
            })
            .collect();
        View { title: "w".into(), seen: if picture { Seen::Picture } else { Seen::Controls }, lines: Vec::new(), targets }
    }

    #[test]
    fn choosing_send_from_a_list_asks_first() {
        let v = view(&["Draft", "Send"], false);
        assert_eq!(guard(&Action::Choose(2), &v, None), Guard::AskFirst("Send".into()));
        assert_eq!(guard(&Action::Choose(1), &v, None), Guard::Go);
    }

    #[test]
    fn enter_beside_send_asks_first_even_on_a_picture() {
        let v = view(&["Message", "Send"], true);
        assert_eq!(guard(&Action::Key("enter".into()), &v, None), Guard::AskFirst("Send".into()));
    }

    #[test]
    fn paying_is_never_pressed() {
        let v = view(&["Place order"], false);
        assert!(matches!(guard(&Action::Click(1), &v, Some("Place order")), Guard::Refuse(_)));
        assert_eq!(crate::policy::press("Buy now"), crate::policy::Press::Never);
        assert_eq!(crate::policy::press("Delete"), crate::policy::Press::AskFirst);
        assert_eq!(crate::policy::press("Open"), crate::policy::Press::Go);
    }
}
