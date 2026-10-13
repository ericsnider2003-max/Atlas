//! Every button, link and control in the hub does something (UX7, 12 Oct 2026).
//!
//! Eric clicked buttons in the hub and nothing happened. A page that renders
//! proves nothing about its buttons, so this one walks the real hub: it starts
//! at Home, follows every link it finds, and on every page it reaches it
//! pulls out each actionable element and asks the question that matters:
//!
//!   * a link goes to a page the server routes and that answers;
//!   * a `#anchor` goes to an id that is on that page;
//!   * a form posts to a route the server routes, and pressing it (with the
//!     form's own hidden/default fields, in a scratch store) is answered with
//!     a page or a redirect -- never an error, never "that button isn't wired
//!     to anything";
//!   * a `<button>` that is not in a form is wired by a script, and the id
//!     that script looks for is on the page.
//!
//! Everything is the real hub code against a real (scratch) daemon; nothing
//! is mocked but the machine's windows.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::{route, Action, Reply, Request};
use atlas::store::Store;
use atlas::workspace_view::{Handoff, Item, Kind, Origin, Status};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-every-button-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
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

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(
        c,
        p,
        None,
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    )
}

fn an_item(id: &str, title: &str, status: Status) -> Item {
    Item {
        id: id.into(),
        title: title.into(),
        kind: Kind::Task,
        status,
        due: None,
        project: Some("Homelab".into()),
        client: None,
        links: vec![],
        blocked_by: if status == Status::Blocked {
            Some("the vendor".into())
        } else {
            None
        },
        from: Origin::YouSaid,
        at: 1,
        closed_at: None,
        tags: vec![],
        thinking: vec![],
        estimate_mins: None,
        spent_mins: 0,
        atlas_could: Handoff::Unknown,
    }
}

// ---------- a small, honest HTML reader ----------

#[derive(Debug, Clone, Default)]
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    closing: bool,
}

impl Tag {
    fn get(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }
    fn has(&self, k: &str) -> bool {
        self.get(k).is_some()
    }
}

/// Every tag in the page, outside `<script>` and `<style>` bodies, plus the
/// text of each script (they are read separately).
fn read_html(html: &str) -> (Vec<Tag>, Vec<String>) {
    let b = html.as_bytes();
    let (mut tags, mut scripts) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i..].find("-->").map(|e| i + e + 3).unwrap_or(b.len());
            continue;
        }
        let mut j = i + 1;
        let mut quote = 0u8;
        while j < b.len() {
            let c = b[j];
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' {
                quote = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        let inner = &html[i + 1..j.min(b.len())];
        i = j + 1;
        let closing = inner.starts_with('/');
        let inner = inner.trim_start_matches('/').trim_end_matches('/');
        let name_end = inner
            .find(|c: char| c.is_whitespace())
            .unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        if name.is_empty() || name.starts_with('!') {
            continue;
        }
        let mut attrs = Vec::new();
        let rest = &inner[name_end..];
        let r = rest.as_bytes();
        let mut k = 0;
        while k < r.len() {
            while k < r.len() && (r[k] as char).is_whitespace() {
                k += 1;
            }
            let s = k;
            while k < r.len() && r[k] != b'=' && !(r[k] as char).is_whitespace() {
                k += 1;
            }
            let key = rest[s..k].to_ascii_lowercase();
            if key.is_empty() {
                break;
            }
            while k < r.len() && (r[k] as char).is_whitespace() {
                k += 1;
            }
            let mut val = String::new();
            if k < r.len() && r[k] == b'=' {
                k += 1;
                while k < r.len() && (r[k] as char).is_whitespace() {
                    k += 1;
                }
                if k < r.len() && (r[k] == b'"' || r[k] == b'\'') {
                    let q = r[k];
                    k += 1;
                    let s = k;
                    while k < r.len() && r[k] != q {
                        k += 1;
                    }
                    val = rest[s..k].to_string();
                    k += 1;
                } else {
                    let s = k;
                    while k < r.len() && !(r[k] as char).is_whitespace() {
                        k += 1;
                    }
                    val = rest[s..k].to_string();
                }
            }
            attrs.push((key, unescape(&val)));
        }
        if (name == "script" || name == "style") && !closing {
            let close = format!("</{name}");
            let end = html[i..].find(&close).map(|e| i + e).unwrap_or(b.len());
            if name == "script" {
                scripts.push(html[i..end].to_string());
            }
            i = end;
            continue;
        }
        tags.push(Tag {
            name,
            attrs,
            closing,
        });
    }
    (tags, scripts)
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[derive(Debug, Default)]
struct Form {
    id: Option<String>,
    /// Text boxes: a form with exactly one is sent by Enter, no button needed.
    text_inputs: usize,
    /// `type=button` inside it, by id: sent by a script, which must name it.
    scripted: Vec<Option<String>>,
    action: String,
    method: String,
    /// What pressing it would send: hidden inputs, checked/default values,
    /// the first option of each select, the submitter's own name/value.
    fields: Vec<(String, String)>,
    /// The `<button>`s inside it, as (name, value, formaction).
    submitters: Vec<(Option<String>, Option<String>, Option<String>)>,
}

#[derive(Default)]
struct Page {
    links: Vec<String>,
    ids: BTreeSet<String>,
    forms: Vec<Form>,
    /// `<button form=ID>` anywhere: (form id, name, value, formaction).
    remote_submitters: Vec<(String, Option<String>, Option<String>, Option<String>)>,
    /// Buttons outside any form, as (id, type).
    loose_buttons: Vec<(Option<String>, String)>,
    /// Elements that claim to be clickable but carry no handler markup.
    scripts: Vec<String>,
    /// `form=ID` naming a form that is not on the page.
    orphans: Vec<String>,
}

fn read_page(html: &str) -> Page {
    let (tags, scripts) = read_html(html);
    let mut page = Page {
        scripts,
        ..Page::default()
    };
    let mut open: Option<Form> = None;
    let mut select_name: Option<String> = None;
    let mut select_set = false;
    for t in tags {
        if let Some(id) = t.get("id") {
            page.ids.insert(id.to_string());
        }
        if let Some(n) = t.get("name") {
            if t.name == "a" {
                page.ids.insert(n.to_string());
            }
        }
        if t.closing {
            if t.name == "form" {
                if let Some(f) = open.take() {
                    page.forms.push(f);
                }
            }
            if t.name == "select" {
                select_name = None;
            }
            continue;
        }
        match t.name.as_str() {
            "a" => {
                if let Some(h) = t.get("href") {
                    page.links.push(h.to_string());
                }
            }
            "form" => {
                if let Some(f) = open.take() {
                    page.forms.push(f);
                }
                open = Some(Form {
                    id: t.get("id").map(str::to_string),
                    action: t.get("action").unwrap_or("").to_string(),
                    method: t.get("method").unwrap_or("get").to_ascii_lowercase(),
                    ..Form::default()
                });
            }
            "select" => {
                select_name = t.get("name").map(str::to_string);
                select_set = false;
            }
            "option" => {
                if let (Some(n), Some(f)) = (&select_name, open.as_mut()) {
                    if !select_set || t.has("selected") {
                        f.fields.retain(|(k, _)| k != n);
                        f.fields
                            .push((n.clone(), t.get("value").unwrap_or("").to_string()));
                        select_set = true;
                    }
                }
            }
            "input" => {
                if let Some(f) = open.as_mut() {
                    let ty = t.get("type").unwrap_or("text");
                    if let Some(n) = t.get("name") {
                        match ty {
                            "checkbox" | "radio" => {
                                if t.has("checked") {
                                    f.fields
                                        .push((n.into(), t.get("value").unwrap_or("on").into()));
                                }
                            }
                            "submit" => f.submitters.push((
                                Some(n.into()),
                                t.get("value").map(str::to_string),
                                t.get("formaction").map(str::to_string),
                            )),
                            "file" | "button" | "reset" | "image" => {}
                            _ => {
                                if matches!(ty, "text" | "search" | "email" | "url" | "tel") {
                                    f.text_inputs += 1;
                                }
                                f.fields
                                    .push((n.into(), t.get("value").unwrap_or("").to_string()))
                            }
                        }
                    } else if ty == "submit" {
                        f.submitters
                            .push((None, None, t.get("formaction").map(str::to_string)));
                    }
                }
            }
            "textarea" => {
                if let (Some(f), Some(n)) = (open.as_mut(), t.get("name")) {
                    f.fields.push((n.into(), String::new()));
                }
            }
            "button" => {
                let ty = t.get("type").unwrap_or("submit").to_string();
                if let Some(owner) = t.get("form") {
                    if ty == "submit" {
                        page.remote_submitters.push((
                            owner.to_string(),
                            t.get("name").map(str::to_string),
                            t.get("value").map(str::to_string),
                            t.get("formaction").map(str::to_string),
                        ));
                    }
                    continue;
                }
                match open.as_mut() {
                    Some(f) if ty == "submit" => f.submitters.push((
                        t.get("name").map(str::to_string),
                        t.get("value").map(str::to_string),
                        t.get("formaction").map(str::to_string),
                    )),
                    Some(f) => f.scripted.push(t.get("id").map(str::to_string)),
                    None => page
                        .loose_buttons
                        .push((t.get("id").map(str::to_string), ty)),
                }
            }
            _ => {}
        }
    }
    if let Some(f) = open.take() {
        page.forms.push(f);
    }
    let remote = std::mem::take(&mut page.remote_submitters);
    for (owner, n, v, fa) in remote {
        if let Some(f) = page
            .forms
            .iter_mut()
            .find(|f| f.id.as_deref() == Some(owner.as_str()))
        {
            f.submitters.push((n, v, fa));
        } else {
            page.orphans.push(owner);
        }
    }
    page
}

// ---------- the walk ----------

fn encode(s: &str) -> String {
    atlas::research::urlencode(s)
}

fn request(method: &str, target: &str, body: &str) -> Request {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Request {
        method: method.into(),
        path: path.into(),
        query: query.into(),
        token: None,
        token_from_url: false,
        body: body.into(),
    }
}

/// Phrases the hub uses to say a control reached nothing.
const DEAD: &[&str] = &[
    "isn't wired",
    "is not wired",
    "needs the full Atlas",
    "isn't something the",
    "No such page",
    "That button didn't",
    "didn't say which",
];

fn ask(d: &mut Daemon, method: &str, target: &str, body: &str) -> Result<Reply, String> {
    let req = request(method, target, body);
    let Some(action) = route(&req) else {
        return Err(format!("{method} {target}: the server has no route for it"));
    };
    let reply = atlas::hublive::reply(d, action);
    if reply.status >= 400 {
        return Err(format!(
            "{method} {target}: answered {} ({})",
            reply.status,
            reply.body.chars().take(120).collect::<String>()
        ));
    }
    for dead in DEAD {
        if reply.body.contains(dead) {
            return Err(format!("{method} {target}: answered \"{dead}\""));
        }
    }
    Ok(reply)
}

/// Where a redirect goes (the body of a 303 is the address).
fn followed(d: &mut Daemon, r: Reply, trail: &str, problems: &mut Vec<String>) {
    if r.status == 303 {
        let to = r.body.clone();
        if let Err(e) = ask(d, "GET", &to, "") {
            problems.push(format!("{trail} -> redirect {e}"));
        }
    }
}

struct Walk {
    pages: BTreeSet<String>,
    forms_pressed: BTreeSet<String>,
    links_seen: usize,
    statically_checked: usize,
    controls_seen: usize,
    problems: Vec<String>,
}

fn walk(d: &mut Daemon, starts: &[&str]) -> Walk {
    let mut w = Walk {
        pages: BTreeSet::new(),
        forms_pressed: BTreeSet::new(),
        links_seen: 0,
        statically_checked: 0,
        controls_seen: 0,
        problems: Vec::new(),
    };
    let mut todo: Vec<String> = starts.iter().map(|s| s.to_string()).collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    while let Some(target) = todo.pop() {
        if !seen.insert(target.clone()) || seen.len() > 300 {
            continue;
        }
        let reply = match ask(d, "GET", &target, "") {
            Ok(r) => r,
            Err(e) => {
                w.problems.push(e);
                continue;
            }
        };
        if reply.status == 303 {
            todo.push(reply.body.clone());
            continue;
        }
        if reply.kind != atlas::server::Body::Html || reply.download.is_some() {
            continue;
        }
        w.pages
            .insert(target.split('?').next().unwrap_or(&target).to_string());
        let page = read_page(&reply.body);
        let script_text = page.scripts.join("\n");
        for link in &page.links {
            w.links_seen += 1;
            w.controls_seen += 1;
            if let Some(anchor) = link.strip_prefix('#') {
                if !anchor.is_empty() && !page.ids.contains(anchor) {
                    w.problems
                        .push(format!("{target}: link #{anchor} points at nothing"));
                }
                continue;
            }
            if link.starts_with("http") || link.contains(':') && !link.starts_with('/') {
                // The hub is offline: an outside address is a different kind of
                // problem, caught by `the_hub_works_offline`. mailto/tel are
                // the person's own apps.
                if link.starts_with("http") {
                    w.problems
                        .push(format!("{target}: link {link} leaves the machine"));
                }
                continue;
            }
            if !link.starts_with('/') {
                w.problems
                    .push(format!("{target}: link \"{link}\" is not an address"));
                continue;
            }
            let bare = link.split('#').next().unwrap_or(link);
            if safe_to_open(bare) {
                todo.push(bare.to_string());
            } else if route(&request("GET", bare, "")).is_none() {
                // Not opened (it may write a setting, start a job or reach
                // out); it must still be a route the server answers.
                w.problems
                    .push(format!("{target}: link {bare} has no route"));
            }
        }
        for form in &page.forms {
            w.controls_seen += 1;
            if form.action.is_empty() {
                w.problems
                    .push(format!("{target}: a form with no action posts nowhere"));
                continue;
            }
            if form.submitters.is_empty() {
                let by_enter = form.text_inputs == 1;
                let by_script = form
                    .scripted
                    .iter()
                    .flatten()
                    .any(|id| script_text.contains(id.as_str()));
                // A named form of only hidden fields is a helper that other
                // buttons name with `form=`; it may have none right now.
                let helper = form.id.is_some() && form.text_inputs == 0 && form.fields.len() <= 1;
                if helper {
                    continue;
                }
                if !by_enter && !by_script {
                    w.problems
                        .push(format!("{target}: form {} has no button", form.action));
                }
                if !by_enter && by_script {
                    // Sent by a script (the file picker): not pressed here.
                    continue;
                }
            }
            let method = if form.method == "post" { "POST" } else { "GET" };
            // Each button of a form is pressed on its own: two buttons of
            // one form can mean two different things (name/value).
            let presses: Vec<Option<(String, String)>> = if form.submitters.is_empty() {
                vec![None]
            } else {
                form.submitters
                    .iter()
                    .map(|(n, v, _)| {
                        n.as_ref()
                            .map(|n| (n.clone(), v.clone().unwrap_or_default()))
                    })
                    .collect()
            };
            for (i, press) in presses.iter().enumerate() {
                let action = form
                    .submitters
                    .get(i)
                    .and_then(|s| s.2.clone())
                    .unwrap_or_else(|| form.action.clone());
                let mut fields = form.fields.clone();
                if let Some(p) = press {
                    fields.push(p.clone());
                }
                let body = fields
                    .iter()
                    .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
                    .collect::<Vec<_>>()
                    .join("&");
                let key = format!("{method} {action} {body}");
                if !w.forms_pressed.insert(key.clone()) {
                    continue;
                }
                let (t, b) = if method == "GET" {
                    (
                        format!(
                            "{action}{}{body}",
                            if action.contains('?') { "&" } else { "?" }
                        ),
                        String::new(),
                    )
                } else {
                    (action.clone(), body)
                };
                if !safe_to_press(method, &action) {
                    // Never pressed: it may sign in, download, send, install
                    // or write a setting, and a test must touch nothing real.
                    // It is checked by its wiring instead.
                    if let Err(e) = wired(method, &action, &t, &b) {
                        w.problems.push(format!("{target}: {e}"));
                    }
                    w.statically_checked += 1;
                    continue;
                }
                match ask(d, method, &t, &b) {
                    Ok(r) => followed(
                        d,
                        r,
                        &format!("{target}: {method} {action}"),
                        &mut w.problems,
                    ),
                    Err(e) => w.problems.push(format!("{target}: {e}")),
                }
            }
        }
        for orphan in &page.orphans {
            w.problems.push(format!(
                "{target}: a button points at form #{orphan}, which is not there"
            ));
        }
        for (id, ty) in &page.loose_buttons {
            w.controls_seen += 1;
            let wired = match id {
                Some(id) => {
                    let page_script = script_text.contains(id.as_str());
                    page_script || hub_scripts_mention(id)
                }
                None => false,
            };
            if ty == "submit" || !wired {
                w.problems.push(format!(
                    "{target}: a button outside any form ({}) that no script reaches",
                    id.clone().unwrap_or_else(|| "no id".into())
                ));
            }
        }
    }
    w
}

/// The only controls this test ever presses. Everything else in the hub can
/// reach outside the test (open a browser to sign in, start a download,
/// launch an app, send, install, write the real settings) and is only checked
/// for wiring. Adding to this list means the press has no effect outside the
/// scratch store.
fn safe_to_press(method: &str, action: &str) -> bool {
    matches!(
        (method, action),
        ("GET", "/hub/find") | ("POST", "/hub/workspace")
    )
}

/// Pages that are only read when opened, by query keys that only choose what
/// is shown. Anything else (appearance links, job followers, downloads) is
/// not opened.
fn safe_to_open(link: &str) -> bool {
    let (path, query) = link.split_once('?').unwrap_or((link, ""));
    atlas::hub::route(path).is_some()
        && !matches!(path, "/hub/phone" | "/hub/updates" | "/hub/connections")
        && query
            .split('&')
            .filter(|kv| !kv.is_empty())
            .all(|kv| matches!(kv.split('=').next(), Some("view" | "b" | "room")))
}

/// A control that is not pressed is wired when the server routes it and, for
/// a hub form, the hub has an arm that answers that path.
fn wired(method: &str, action: &str, target: &str, body: &str) -> Result<(), String> {
    let Some(routed) = route(&request(method, target, body)) else {
        return Err(format!("{method} {action}: the server has no route for it"));
    };
    if let Action::HubPost { path, .. } = routed {
        let src = crate::common::source_of("hublive");
        let named = format!("\"{path}\"");
        if !crate::common::contains_code(&src, &named) {
            return Err(format!("{method} {action}: no handler answers {path}"));
        }
    }
    Ok(())
}

fn hub_scripts_mention(id: &str) -> bool {
    ["live.js", "palette.js", "drag.js"].iter().any(|f| {
        std::fs::read_to_string(Path::new("assets/hub").join(f))
            .map(|s| s.contains(&format!("'{id}'")))
            .unwrap_or(false)
    })
}

fn seeded<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let mut d = daemon(c, p, tag);
    d.workspace = vec![
        an_item("w1", "Reply to the landlord", Status::Doing),
        an_item("w2", "Choose a venue", Status::NeedsYou),
        an_item("w3", "Renew the licence", Status::Blocked),
        an_item("w4", "Order the parts", Status::Waiting),
    ];
    d
}

const STARTS: &[&str] = &[
    "/hub",
    "/hub/status",
    "/hub/now",
    "/hub/gestures",
    "/hub/access",
    "/hub/connections",
    "/hub/workspace",
    "/hub/workshop",
    "/hub/calendar",
    "/hub/back",
    "/hub/recommendations",
    "/hub/settings",
    "/hub/permissions",
    "/hub/addons",
    "/hub/edits",
    "/hub/friends",
    "/hub/groups",
    "/hub/accounts",
    "/hub/activity",
    "/hub/outstanding",
    "/hub/sync",
    "/hub/messages",
    "/hub/documents",
    "/hub/business",
    "/hub/tasks",
    "/hub/clients",
    "/hub/partners",
    "/hub/sound",
    "/hub/trusted",
    "/hub/give",
    "/hub/social",
    "/hub/offline",
    "/hub/talk",
    "/hub/help",
    "/hub/updates",
    "/hub/phone",
    "/hub/feedback",
    "/hub/opportunities",
];

#[test]
fn every_link_form_and_button_in_a_seeded_hub_reaches_something() {
    let (c, p) = (cfg(), plat());
    let mut d = seeded(&c, &p, "seeded");
    let w = walk(&mut d, STARTS);
    eprintln!(
        "walked {} pages, {} controls ({} links), pressed {} safe forms, checked {} by wiring only",
        w.pages.len(),
        w.controls_seen,
        w.links_seen,
        w.forms_pressed.len(),
        w.statically_checked
    );
    assert!(
        w.pages.len() >= 30,
        "the walk only reached {} pages: {:?}",
        w.pages.len(),
        w.pages
    );
    assert!(
        w.problems.is_empty(),
        "dead or broken controls:\n{}",
        w.problems.join("\n")
    );
}

#[test]
fn every_link_form_and_button_in_an_empty_hub_reaches_something() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "empty");
    let w = walk(&mut d, STARTS);
    assert!(
        w.problems.is_empty(),
        "dead or broken controls on a fresh hub:\n{}",
        w.problems.join("\n")
    );
}

#[test]
fn the_scripts_find_the_ids_they_look_for() {
    // The palette script (on every page) wires #palopen/#palette/#palq; a
    // page missing them has a "Find anything" that is only a link.
    let (c, p) = (cfg(), plat());
    let mut d = seeded(&c, &p, "script-ids");
    for target in [
        "/hub",
        "/hub/workspace",
        "/hub/settings",
        "/hub/outstanding",
    ] {
        let r = ask(&mut d, "GET", target, "").unwrap();
        let page = read_page(&r.body);
        for id in ["palopen", "palette", "palq", "palhits", "palall"] {
            assert!(page.ids.contains(id), "{target} has no #{id}");
        }
    }
    let r = ask(&mut d, "GET", "/hub/now", "").unwrap();
    let page = read_page(&r.body);
    assert!(
        page.ids.contains("livepause") && page.ids.contains("liveword"),
        "the live page has no pause button for live.js to wire"
    );
}

#[test]
fn the_reader_sees_what_is_there() {
    // The walk is only as good as its eyes.
    let p = read_page(
        "<a href='/hub'>x</a><form method=post action='/hub/set'>\
         <input type=hidden name=key value=a><select name=value><option value=1>1</option>\
         <option value=2>2</option></select><button>Go</button></form><button id=b>Y</button>\
         <script>var x = '<a href=\"/no\">'</script>",
    );
    assert_eq!(p.links, vec!["/hub"]);
    assert_eq!(p.forms.len(), 1);
    assert_eq!(p.forms[0].action, "/hub/set");
    assert_eq!(
        p.forms[0].fields,
        vec![("key".into(), "a".into()), ("value".into(), "1".into())]
    );
    assert_eq!(p.forms[0].submitters.len(), 1);
    assert_eq!(p.loose_buttons.len(), 1);
    let _ = Action::Status;
}

// ---------- the Workspace (UX8) ----------

fn post_workspace(d: &mut Daemon, fields: &str) -> Reply {
    ask(d, "POST", "/hub/workspace", fields).unwrap()
}

#[test]
fn an_empty_workspace_says_what_it_is_for_and_offers_two_working_buttons() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "ws-empty");
    let html = ask(&mut d, "GET", "/hub/workspace", "").unwrap().body;
    assert!(
        !html.contains(">Nothing here.<"),
        "the empty Workspace is a dead end again"
    );
    assert!(html.contains("Your workspace is empty"));
    assert!(html.contains("one board for everything you have open"));
    let page = read_page(&html);
    let mine = page
        .forms
        .iter()
        .filter(|f| f.action == "/hub/workspace")
        .count();
    assert_eq!(mine, 2, "expected the add form and the open button");
    // Labelled for a screen reader, and reachable by keyboard like any form.
    assert!(html.contains("<label for=ws-new>"), "the box has no label");
    assert!(page.ids.contains("ws-new"));
    assert!(html.contains("aria-labelledby=ws-empty-h") && page.ids.contains("ws-empty-h"));
}

#[test]
fn adding_the_first_thing_puts_it_on_the_board_and_keeps_it() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "ws-add");
    let r = post_workspace(&mut d, "what=add&title=Reply+to+the+landlord");
    assert_eq!(r.status, 303);
    assert!(r.body.starts_with("/hub/workspace?said="), "{}", r.body);
    assert!(d
        .workspace
        .iter()
        .any(|i| i.title == "Reply to the landlord"));
    // On the board (the "Everything live" view lists waiting things) ...
    let html = ask(&mut d, "GET", "/hub/workspace?view=Everything+live", "")
        .unwrap()
        .body;
    assert!(html.contains("Reply to the landlord"));
    assert!(!html.contains("Your workspace is empty"));
    // ... and kept on disk, not only in memory.
    let kept: Vec<Item> = d.store.load("workspace");
    assert_eq!(kept.len(), 1);
    // An empty box changes nothing and says so.
    let r = post_workspace(&mut d, "what=add&title=+++");
    assert!(r.body.contains("Type+what") || r.body.contains("Type%20what"));
    assert_eq!(d.workspace.len(), 1);
}

#[test]
fn the_default_view_being_empty_points_at_what_is_open() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "ws-other-view");
    d.workspace = vec![an_item("w1", "Order the parts", Status::Waiting)];
    // "Now" shows only what is in hand or needs you.
    let html = ask(&mut d, "GET", "/hub/workspace", "").unwrap().body;
    assert!(html.contains("Nothing in \u{201c}Now\u{201d} right now"));
    assert!(html.contains("1 other thing is open"));
    assert!(html.contains("/hub/workspace?view=Everything%20live"));
}

#[test]
fn each_view_link_shows_its_own_view() {
    // The links across the top used to send a query nothing read: all five
    // showed the same page.
    let (c, p) = (cfg(), plat());
    let mut d = seeded(&c, &p, "ws-views");
    let home = ask(&mut d, "GET", "/hub/workspace", "").unwrap().body;
    let views: Vec<String> = read_page(&home)
        .links
        .into_iter()
        .filter(|l| l.starts_with("/hub/workspace?view="))
        .collect();
    assert!(views.len() >= 4, "{views:?}");
    for link in views {
        let html = ask(&mut d, "GET", &link, "").unwrap().body;
        assert!(
            html.contains(&format!("class='view here' href='{link}'")),
            "{link} did not open its own view"
        );
    }
}

#[test]
fn open_my_usual_apps_asks_for_exactly_what_saying_it_does() {
    // The button asks Atlas "open workspace"; that must be the phrase that
    // really boots it (config/commands.yaml), model or no model.
    use atlas::brain::{Brain, Llm};
    use atlas::intent::{Intent, Parser};
    struct Down;
    impl Llm for Down {
        fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
            Err(atlas::error::AtlasError::Platform("down".into()))
        }
    }
    let commands = cfg().commands;
    let parser = Parser::new(&commands);
    let said = Brain {
        llm: &Down,
        fallback: &parser,
        voice: None,
    }
    .decide("open workspace", "");
    assert_eq!(said.intent, Intent::WorkspaceOn);

    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "ws-open");
    let r = post_workspace(&mut d, "what=open");
    assert_eq!(r.status, 303);
    assert!(r.body.starts_with("/hub/workspace?said="), "{}", r.body);
    assert_eq!(
        d.talk_queue_for_test(),
        vec![("open workspace".to_string(), false)]
    );
    // A button the page does not draw changes nothing and says so.
    let r = post_workspace(&mut d, "what=bogus");
    assert!(r.body.contains("nothing+changed") || r.body.contains("nothing%20changed"));
}
