//! UI Automation — reading your open windows without a screenshot.
//!
//! Windows publishes an accessibility tree for screen readers: every control
//! with a role, a name, and a value. Reading that is far better than
//! screenshotting and asking a vision model what it sees — it is exact, it is
//! fast, it costs no GPU, and it works on a window that isn't in front.
//!
//! The catch is app cooperation. Native Windows apps expose everything.
//! Electron apps often expose one undifferentiated blob. Custom-drawn UIs
//! expose nothing. So a large part of this module is **detecting that the tree
//! is useless**, so Atlas can fall back to a screenshot instead of confidently
//! reporting nonsense.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Window,
    Pane,
    Document,
    Text,
    Button,
    Edit,
    CheckBox,
    RadioButton,
    ComboBox,
    List,
    ListItem,
    Link,
    MenuItem,
    Tab,
    Image,
    Other,
}

impl Role {
    /// Roles you can act on rather than just read.
    fn interactive(&self) -> bool {
        matches!(
            self,
            Role::Button | Role::Edit | Role::CheckBox | Role::RadioButton
                | Role::ComboBox | Role::Link | Role::MenuItem | Role::Tab | Role::ListItem
        )
    }

    /// Roles that carry readable content.
    fn textual(&self) -> bool {
        matches!(self, Role::Text | Role::Document | Role::Edit | Role::ListItem)
    }

    /// Map a Windows control type id. Values from UIA_ControlTypeIds.
    pub fn from_control_type(id: i32) -> Role {
        match id {
            50032 => Role::Window,
            50033 => Role::Pane,
            50030 => Role::Document,
            50020 => Role::Text,
            50000 => Role::Button,
            50004 => Role::Edit,
            50002 => Role::CheckBox,
            50013 => Role::RadioButton,
            50003 => Role::ComboBox,
            50008 => Role::List,
            50007 => Role::ListItem,
            50005 => Role::Link,
            50011 => Role::MenuItem,
            50019 => Role::Tab,
            50006 => Role::Image,
            _ => Role::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub role: Role,
    /// The accessible name — a button's label, a field's caption.
    pub name: String,
    /// Current content, for editable or valued controls.
    #[serde(default)]
    pub value: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub children: Vec<Node>,
    /// Where it is on the screen, in pixels (x, y, width, height), when
    /// Windows says: what a click falls back to when the control has no
    /// way of being pressed through UI Automation (`operate`).
    #[serde(default)]
    pub rect: Option<[i32; 4]>,
}
fn yes() -> bool {
    true
}

impl Node {
    pub fn new(role: Role, name: &str) -> Node {
        Node { role, name: name.into(), value: String::new(), enabled: true, children: Vec::new(), rect: None }
    }

    pub fn with(mut self, children: Vec<Node>) -> Node {
        self.children = children;
        self
    }

    pub fn valued(mut self, value: &str) -> Node {
        self.value = value.into();
        self
    }

    pub fn at(mut self, x: i32, y: i32, w: i32, h: i32) -> Node {
        self.rect = Some([x, y, w, h]);
        self
    }

    pub fn disabled(mut self) -> Node {
        self.enabled = false;
        self
    }

    pub fn count(&self) -> usize {
        1 + self.children.iter().map(Node::count).sum::<usize>()
    }

    pub fn depth(&self) -> usize {
        1 + self.children.iter().map(Node::depth).max().unwrap_or(0)
    }

    /// Depth-first walk.
    fn visit<'a>(&'a self, f: &mut dyn FnMut(&'a Node)) {
        f(self);
        for c in &self.children {
            c.visit(f);
        }
    }

    pub fn find<'a>(&'a self, pred: &dyn Fn(&Node) -> bool) -> Option<&'a Node> {
        if pred(self) {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(pred))
    }

    /// Find a control by its label. Exact match wins over a partial one, so
    /// "Save" does not click "Save As" when both are present.
    pub fn by_name(&self, name: &str) -> Option<&Node> {
        let want = name.trim().to_lowercase();
        let mut exact = None;
        let mut partial = None;
        self.visit(&mut |n| {
            let got = n.name.trim().to_lowercase();
            if got == want && exact.is_none() {
                exact = Some(n);
            } else if partial.is_none() && !got.is_empty() && got.contains(&want) {
                partial = Some(n);
            }
        });
        exact.or(partial)
    }

    /// Everything you could actually click or type into.
    pub fn actionable(&self) -> Vec<&Node> {
        let mut out = Vec::new();
        self.visit(&mut |n| {
            if n.role.interactive() && n.enabled && !n.name.trim().is_empty() {
                out.push(n);
            }
        });
        out
    }

    /// Readable text of the window, in document order.
    pub fn text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        self.visit(&mut |n| {
            if !n.role.textual() {
                return;
            }
            let s = if n.value.trim().is_empty() { &n.name } else { &n.value };
            let s = s.trim();
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        });
        dedupe_adjacent(parts).join(" ")
    }
}

/// Why a tree can't be used, so Atlas falls back instead of guessing.
#[derive(Debug, Clone, PartialEq)]
pub enum Quality {
    /// Rich enough to read and act on.
    Usable,
    /// The app publishes almost nothing — typical of Electron and custom UIs.
    TooShallow { nodes: usize },
    /// One giant unlabelled blob. Common in Chrome without accessibility on.
    Unlabelled { named: usize, total: usize },
    /// Nothing at all.
    Empty,
}

impl Quality {
    /// Why this window could not be read, in words that point at the fix:
    /// "too shallow" is the app, "unlabelled" is usually a setting.
    pub fn plain(&self) -> String {
        match self {
            Quality::Usable => "readable".into(),
            Quality::TooShallow { nodes } => {
                format!("it publishes almost nothing — {nodes} elements in the whole window")
            }
            Quality::Unlabelled { named, total } => {
                format!("only {named} of its {total} elements have names")
            }
            Quality::Empty => "it publishes nothing at all".into(),
        }
    }

    pub fn usable(&self) -> bool {
        *self == Quality::Usable
    }

    pub fn explain(&self) -> String {
        match self {
            Quality::Usable => "accessibility tree looks good".into(),
            Quality::TooShallow { nodes } => {
                format!("app exposes only {nodes} controls — falling back to a screenshot")
            }
            Quality::Unlabelled { named, total } => {
                format!("only {named} of {total} controls are labelled — falling back to a screenshot")
            }
            Quality::Empty => "app publishes no accessibility tree".into(),
        }
    }
}

/// Judge a tree before trusting it.
///
/// This is the check that makes UIA safe to try first everywhere: when an app
/// does not cooperate, Atlas finds out immediately and cheaply rather than
/// reporting an empty window as an empty document.
pub fn assess(root: &Node, min_nodes: usize, min_named_ratio: f32) -> Quality {
    let total = root.count();
    if total <= 1 {
        return Quality::Empty;
    }
    if total < min_nodes {
        return Quality::TooShallow { nodes: total };
    }
    let mut named = 0;
    root.visit(&mut |n| {
        if !n.name.trim().is_empty() || !n.value.trim().is_empty() {
            named += 1;
        }
    });
    if (named as f32 / total as f32) < min_named_ratio {
        return Quality::Unlabelled { named, total };
    }
    Quality::Usable
}

/// Collapse runs of the same string. Accessibility trees frequently repeat a
/// label on a container and again on its only child.
fn dedupe_adjacent(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(v.len());
    for s in v {
        if out.last().map(|l| *l == s).unwrap_or(false) {
            continue;
        }
        out.push(s);
    }
    out
}

/// A compact outline for handing to the model — roles and labels only, no
/// geometry, capped so a big window does not blow the context.
pub fn outline(root: &Node, max_lines: usize) -> String {
    let mut lines = Vec::new();
    write_outline(root, 0, &mut lines, max_lines);
    lines.join("\n")
}

fn write_outline(n: &Node, depth: usize, out: &mut Vec<String>, max: usize) {
    if out.len() >= max {
        return;
    }
    let label = if n.value.trim().is_empty() { &n.name } else { &n.value };
    if !label.trim().is_empty() || n.role.interactive() {
        out.push(format!(
            "{}{:?}: {}",
            "  ".repeat(depth.min(6)),
            n.role,
            label.trim()
        ));
    }
    for c in &n.children {
        write_outline(c, depth + 1, out, max);
    }
}

/// What can be done to one control through UI Automation (`operate`).
#[derive(Debug, Clone, PartialEq)]
pub enum UiAct {
    /// Press a button, a menu item, a link (Invoke).
    Invoke,
    /// Replace an edit box's text (Value).
    SetValue(String),
    /// Tick or untick (Toggle).
    Toggle,
    /// Pick a list item or a tab (SelectionItem).
    Select,
    /// Open a drop-down or a tree branch (ExpandCollapse).
    Expand,
    /// Give it the keyboard.
    Focus,
}

/// Buttons whose press can't be taken back: asked about first, every time.
const CANT_BE_UNDONE: &[&str] = &["send", "delete", "remove", "pay", "buy", "purchase", "submit", "post",
    "publish", "confirm", "transfer", "order", "sign", "accept", "erase", "format", "uninstall"];

pub fn cannot_be_undone(button: &str) -> bool {
    let b = button.to_lowercase();
    CANT_BE_UNDONE.iter().any(|w| b.split(|c: char| !c.is_alphanumeric()).any(|x| x == *w))
}

/// "click Export in Excel" / "press the Save button" → ("Export", Some("Excel")).
pub fn button_request(said: &str) -> Option<(String, Option<String>)> {
    let t = said.trim();
    let lower = t.to_lowercase();
    let start = ["click the", "press the", "click", "press", "hit the", "hit", "tap the", "tap"]
        .iter()
        .find_map(|p| lower.find(p).map(|i| i + p.len()))?;
    let rest = t[start..].trim();
    let (button, app) = match rest.to_lowercase().rfind(" in ") {
        Some(i) => (rest[..i].trim().to_string(), Some(rest[i + 4..].trim().to_string())),
        None => (rest.to_string(), None),
    };
    let button = button
        .trim_end_matches(['.', '!'])
        .trim_end_matches(" button")
        .trim()
        .trim_matches('"')
        .to_string();
    (!button.is_empty()).then_some((button, app.filter(|a| !a.is_empty())))
}
