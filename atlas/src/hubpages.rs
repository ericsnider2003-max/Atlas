//! The hub pages the locked design drew and the first port didn't build.
//!
//! The design (`design/hub/`, locked with Eric 20–21 Sep 2026, with its phone
//! screens redrawn on 24 Sep) has thirty desktop artboards and a phone for
//! each. The first port (26 Sep) built the frame, Home, Outstanding and Now.
//! This module is the rest: Messages, Documents, the business section
//! (Overview, Shared tasks as a table, a board or a calendar, Clients with the
//! firewall on the record, Partners), Sound & voice, Trusted recipients, Give
//! Atlas something, Offline, Talk, Start a project, and Help & accessibility.
//!
//! Each page is a pure function of a view the running Atlas fills in
//! (`hublive`), so every one renders — and is tested — without a daemon. The
//! phone is not a second set of pages: the same pages reflow to one column
//! and the frame swaps the sidebar for the design's bottom tab bar
//! (`hub::STYLE`'s phone rules), because on a phone Atlas serves this hub to
//! its own WebView from the core running on the phone (`mobile`).
//!
//! Accessibility is in the markup, not bolted on (EN 301 549 / WCAG 2.2 AA,
//! see `Help`): every control is a real `<a>`, `<button>` or labelled input,
//! every status is an icon *and* a word, tables have header cells, and live
//! parts announce themselves politely.

use crate::hub::{esc, shell_at, Page};

/// A status as an icon and a word — the design's rule: never colour alone.
fn word_tag(kind: &str, word: &str) -> String {
    let (class, paths) = match kind {
        "done" | "ok" => ("ok", "<path d='M5 12l5 5L20 6'/>"),
        "wait" => ("wait", "<circle cx=12 cy=12 r=9 /><path d='M12 8v4l2.5 1.5'/>"),
        "info" => ("info", "<circle cx=12 cy=12 r=9 /><path d='M12 11v5M12 8h.01'/>"),
        "stop" => ("stop", "<path d='M12 8v4M12 16h.01'/><circle cx=12 cy=12 r=9 />"),
        _ => ("", "<circle cx=12 cy=12 r=9 />"),
    };
    format!(
        "<span class='pill {class}'><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2.4 aria-hidden=true>{paths}</svg>{}</span>",
        esc(word)
    )
}

fn initials(name: &str) -> String {
    let s: String = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_')
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .collect();
    let s = s.to_uppercase();
    if s.is_empty() {
        "?".into()
    } else {
        s
    }
}

fn empty(text: &str) -> String {
    format!("<p class=nothing>{}</p>", esc(text))
}

// ---------------------------------------------------------------- Messages

/// One conversation in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomRow {
    pub id: String,
    pub name: String,
    /// "Personal" or the business's name — the firewall, shown as a word.
    pub area: String,
    pub last: String,
    pub when: String,
    pub unread: usize,
    pub group: bool,
}

/// One message, as the page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Said {
    pub mine: bool,
    pub from: String,
    pub body: String,
    /// When it was written, on the writer's clock.
    pub when: String,
    /// For yours: "Delivered", "Read", "Sent · held for Sam".
    pub state: String,
    pub held: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MessagesView {
    pub rooms: Vec<RoomRow>,
    /// The conversation open on the right: id, name, area, members, messages.
    pub open: Option<(String, String, String, Vec<String>, Vec<Said>)>,
    /// People you've paired with, who a new conversation can be started with.
    pub people: Vec<String>,
    /// What happened last time you sent, said once.
    pub notice: Option<String>,
}

pub fn messages_page(v: &MessagesView) -> String {
    let mut list = String::from("<nav class=roomlist aria-label='Conversations'><ul>");
    if v.rooms.is_empty() {
        list.push_str("<li>");
        list.push_str(&empty("No conversations yet. Start one with someone you've paired with."));
        list.push_str("</li>");
    }
    for r in &v.rooms {
        let here = v.open.as_ref().map(|o| o.0 == r.id).unwrap_or(false);
        list.push_str(&format!(
            "<li><a class='room{on}' href='{href}?room={id}'{cur}><span class=av aria-hidden=true>{ini}</span>\
             <span class=rn><b>{name}</b><span class=last>{last}</span></span>\
             <span class=rm><span class=when>{when}</span>{unread}<span class=area>{area}</span></span></a></li>",
            on = if here { " on" } else { "" },
            cur = if here { " aria-current=page" } else { "" },
            href = Page::Messages.href(),
            id = esc(&r.id),
            ini = esc(&initials(&r.name)),
            name = esc(&r.name),
            last = esc(&r.last),
            when = esc(&r.when),
            unread = if r.unread > 0 {
                format!("<span class=count aria-label='{n} unread'>{n}</span>", n = r.unread)
            } else {
                String::new()
            },
            area = esc(&r.area),
        ));
    }
    list.push_str("</ul>");
    // Start a conversation: a real form, labelled, no script.
    if !v.people.is_empty() {
        list.push_str("<form class=start method=post action='/hub/messages'><input type=hidden name=what value=start>\
                       <label for=startwho>Start a conversation with</label><select id=startwho name=who>");
        for p in &v.people {
            list.push_str(&format!("<option>{}</option>", esc(p)));
        }
        list.push_str("</select><button>Open</button></form>");
    }
    list.push_str("</nav>");

    let convo = match &v.open {
        None => format!(
            "<section class=convo aria-label='Conversation'>{}</section>",
            empty("Pick a conversation to read it here.")
        ),
        Some((id, name, area, members, said)) => {
            let mut s = format!(
                "<section class=convo aria-labelledby=convotitle><div class=convohead>\
                 <h2 id=convotitle>{name}</h2><span class=area>{area}</span>\
                 <span class=who>{members}</span></div><ol class=said data-live aria-live=polite>",
                name = esc(name),
                area = esc(area),
                members = esc(&members.join(", ")),
            );
            if said.is_empty() {
                s.push_str("<li>");
                s.push_str(&empty("Nothing said yet."));
                s.push_str("</li>");
            }
            for m in said {
                s.push_str(&format!(
                    "<li class='msg{mine}'><div class=bubble><span class=from>{from}</span>{body}</div>\
                     <div class=meta><time>{when}</time>{state}</div></li>",
                    mine = if m.mine { " mine" } else { "" },
                    from = if m.mine { "<span class=sr>You: </span>".to_string() } else { format!("{}: ", esc(&m.from)) },
                    body = esc(&m.body),
                    when = esc(&m.when),
                    state = if m.mine {
                        word_tag(if m.held { "wait" } else { "done" }, &m.state)
                    } else {
                        String::new()
                    },
                ));
            }
            s.push_str(&format!(
                "</ol><form class=compose method=post action='/hub/messages'>\
                 <input type=hidden name=what value=send><input type=hidden name=room value='{id}'>\
                 <label for=body class=sr>Message to {name}</label>\
                 <textarea autocomplete=off id=body name=body rows=2 required placeholder='Write a message…'></textarea>\
                 <button class=primary>Send</button></form>\
                 <p class=note>Sent now and stamped with your time. If they're offline, Atlas holds it and delivers it \
                 when they're back. A business room only ever reaches people on that business's roster.</p>\
                 <p class=liveline><span id=liveword role=status>Updating live.</span> <button type=button id=livepause aria-pressed=false>Pause live updates</button></p>{live}</section>",
                live = crate::hub::LIVE_SCRIPT,
                id = esc(id),
                name = esc(name),
            ));
            s
        }
    };
    let notice = v
        .notice
        .as_deref()
        .map(|n| format!("<p class=notice role=status>{}</p>", esc(n)))
        .unwrap_or_default();
    shell_at(Some(Page::Messages), "Messages", &format!("{notice}<div class=msgs>{list}{convo}</div>"))
}

// ---------------------------------------------------------------- Documents

#[derive(Debug, Clone, PartialEq)]
pub struct DocRow {
    /// The tray item's id, for "Send to…".
    pub id: u64,
    pub name: String,
    pub kind: String,
    /// "Personal" or the business's name.
    pub area: String,
    /// "Private", or who it has gone to.
    pub shared: String,
    pub private: bool,
    pub when: String,
    /// "Read", "Waiting to be read", "Couldn't read".
    pub state: String,
    /// Where a photo is kept on this machine, so it can be edited from here
    /// (`photo`). `None` for anything that isn't a photo file.
    pub photo: Option<String>,
}

/// The photo edits offered on the Documents page: the words sent, as if
/// typed on Talk, after "edit this photo" and the photo's path.
pub const PHOTO_EDITS: &[(&str, &str)] = &[
    ("fix the colours and exposure", "Fix colours and light"),
    ("brighter", "Brighter"),
    ("straighten", "Check it's level"),
    ("crop it for instagram", "Instagram 4:5"),
    ("crop it for an instagram story", "Instagram story"),
    ("resize it for a youtube thumbnail", "YouTube thumbnail"),
    ("blur the background", "Blur the background"),
    ("remove the background", "Remove the background"),
    ("undo the photo edit", "Take the last edit back"),
];

/// A photo's "Edit a copy" form: one choice, sent to Talk as a sentence, so
/// the page and your voice are the same path.
fn photo_form(id: u64, name: &str, path: &str) -> String {
    let opts: String = PHOTO_EDITS
        .iter()
        .map(|(said, label)| format!("<option value=\"edit this photo &quot;{}&quot; {}\">{}</option>", esc(path), esc(said), esc(label)))
        .collect();
    format!(
        "<form class=inline method=post action='/hub/talk'><label class=sr for=ped{id}>Edit a copy of {name}</label>\
         <select id=ped{id} name=text>{opts}</select><button aria-label='Edit a copy of {name}'>Edit a copy</button></form>",
        name = esc(name),
    )
}

/// `people`: who a document can be sent to (paired, by name).
pub fn documents_page(docs: &[DocRow], people: &[String], notice: Option<&str>) -> String {
    let mut body = notice.map(|n| format!("<p class=notice role=status>{}</p>", esc(n))).unwrap_or_default();
    body.push_str(
        "<p class=lead>Everything Atlas holds for you, on this machine. The area keeps the firewall legible; \
         the Shared column says for each one whether it's private or has gone out, and to whom.</p>",
    );
    if docs.is_empty() {
        body.push_str(&empty("Nothing here yet. Hand Atlas a file, a link or a photo and it shows up here."));
        body.push_str(&format!("<p><a class='btn primary' href='{}'>Give Atlas something</a></p>", Page::Give.href()));
        return shell_at(Some(Page::Documents), "Documents", &body);
    }
    body.push_str(
        "<div class=tablewrap tabindex=0 role=region aria-label='Documents, scrolls sideways'><table class=data><caption class=sr>Documents</caption><thead><tr>\
         <th scope=col>Name</th><th scope=col>Kind</th><th scope=col>Area</th><th scope=col>Shared</th>\
         <th scope=col>State</th><th scope=col>Added</th><th scope=col>Send</th><th scope=col>Edit</th></tr></thead><tbody>",
    );
    for d in docs {
        let send = if people.is_empty() {
            "<span class=meta>Pair with someone to send</span>".to_string()
        } else {
            let opts: String = people.iter().map(|p| format!("<option>{}</option>", esc(p))).collect();
            format!(
                "<form class=inline method=post action='/hub/documents'><input type=hidden name=what value=send>\
                 <input type=hidden name=id value={id}><label class=sr for=dto{id}>Send {name} to</label>\
                 <select id=dto{id} name=who>{opts}</select><button aria-label='Send {name}'>Send</button></form>",
                id = d.id,
                name = esc(&d.name),
            )
        };
        // Photos can be edited from here; the copy lands beside the file.
        let edit = d.photo.as_deref().map(|p| photo_form(d.id, &d.name, p)).unwrap_or_default();
        body.push_str(&format!(
            "<tr><th scope=row>{}</th><td>{}</td><td><span class=area>{}</span></td><td>{}</td><td>{}</td><td>{}</td><td>{send}</td><td>{edit}</td></tr>",
            esc(&d.name),
            esc(&d.kind),
            esc(&d.area),
            if d.private { word_tag("", "Private") } else { word_tag("info", &d.shared) },
            esc(&d.state),
            esc(&d.when)
        ));
    }
    body.push_str("</tbody></table></div>");
    shell_at(Some(Page::Documents), "Documents", &body)
}

// ---------------------------------------------------------------- Business

/// One business, for its Overview.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BusinessView {
    pub name: String,
    /// (task, due words, overdue)
    pub open_tasks: Vec<(String, String, bool)>,
    pub clients: usize,
    /// (partner, paired with you, trusted)
    pub partners: Vec<(String, bool, bool)>,
    /// Things held at the firewall, waiting on your yes.
    pub held: Vec<String>,
    /// Atlas's plain-language read of the business.
    pub read: String,
}

/// The business section's front page — or, with no business yet, how to add one.
pub fn business_page(all: &[BusinessView], chosen: Option<&str>) -> String {
    if all.is_empty() {
        let body = format!(
            "{}<p class=note>A business opens its own section — shared tasks, clients, partners — walled off from \
             personal. Add one by saying <q>add Jordan to the Northwind business</q>, or by typing it into \
             the bar at the top of any page. Anyone on its roster must be paired first.</p>",
            empty("No business yet.")
        );
        return shell_at(Some(Page::Business), "Overview", &body);
    }
    let b = chosen
        .and_then(|c| all.iter().find(|b| b.name.eq_ignore_ascii_case(c)))
        .unwrap_or(&all[0]);
    let mut body = String::new();
    if all.len() > 1 {
        body.push_str("<nav class=views aria-label='Businesses'>");
        for x in all {
            let here = x.name == b.name;
            body.push_str(&format!(
                "<a class='view{}' href='{}?b={}'{}>{}</a>",
                if here { " here" } else { "" },
                Page::Business.href(),
                esc(&x.name),
                if here { " aria-current=page" } else { "" },
                esc(&x.name)
            ));
        }
        body.push_str("</nav>");
    }
    let overdue = b.open_tasks.iter().filter(|t| t.2).count();
    body.push_str(&format!(
        "<h2 class=bizname>{name} <small>· business</small></h2>\
         <div class=tiles><div class=tile><b>{t}</b><span>open tasks</span></div>\
         <div class=tile><b>{o}</b><span>overdue</span></div>\
         <div class=tile><b>{c}</b><span>clients</span></div>\
         <div class=tile><b>{p}</b><span>partners</span></div></div>\
         <section class=brief aria-label='Atlas on {name}'>{mark}<div><p>{read}</p></div></section>",
        name = esc(&b.name),
        t = b.open_tasks.len(),
        o = overdue,
        c = b.clients,
        p = b.partners.len(),
        mark = crate::hub::MARK,
        read = esc(&b.read),
    ));
    body.push_str("<div class=living><section><h3>Open tasks</h3>");
    if b.open_tasks.is_empty() {
        body.push_str(&empty("Nothing open."));
    } else {
        body.push_str("<ul class=plainlist>");
        for (t, due, late) in &b.open_tasks {
            body.push_str(&format!(
                "<li><span>{}</span>{}</li>",
                esc(t),
                if *late { word_tag("stop", &format!("Overdue · {due}")) } else { word_tag("", due) }
            ));
        }
        body.push_str("</ul>");
    }
    body.push_str(&format!(
        "<a class=more href='{}?b={}'>All shared tasks</a></section><section><h3>Partners</h3>",
        Page::SharedTasks.href(),
        esc(&b.name)
    ));
    if b.partners.is_empty() {
        body.push_str(&empty("Nobody on the roster yet."));
    }
    for (p, up, trusted) in &b.partners {
        body.push_str(&format!(
            "<div class=who><span class=av aria-hidden=true>{}</span><span>{}</span><span class=meta>{}{}</span></div>",
            esc(&initials(p)),
            esc(p),
            if *up { word_tag("ok", "Paired") } else { word_tag("stop", "Not paired — nothing reaches them") },
            if *trusted { word_tag("ok", "Trusted") } else { String::new() }
        ));
    }
    if !b.held.is_empty() {
        body.push_str("<h3>Waiting at the firewall</h3><ul class=plainlist>");
        for h in &b.held {
            body.push_str(&format!("<li>{}</li>", esc(h)));
        }
        body.push_str("</ul>");
    }
    body.push_str("</section></div>");
    shell_at(Some(Page::Business), "Overview", &body)
}

/// A shared task, for the three database views.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    pub id: u64,
    pub what: String,
    pub business: String,
    /// "Sep 28", or "" for no date.
    pub due: String,
    /// Day index since the epoch in your zone, for the calendar; None: no date.
    pub due_day: Option<i64>,
    pub done: bool,
    pub overdue: bool,
    /// Came across from personal (the firewall let it through on your yes).
    pub from_personal: bool,
}

/// Which view of the shared tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskView {
    Table,
    Board,
    Calendar,
}

impl TaskView {
    pub fn from_query(q: &str) -> TaskView {
        match q {
            "board" => TaskView::Board,
            "calendar" => TaskView::Calendar,
            _ => TaskView::Table,
        }
    }
    fn key(self) -> &'static str {
        match self {
            TaskView::Table => "table",
            TaskView::Board => "board",
            TaskView::Calendar => "calendar",
        }
    }
}

fn task_status(t: &TaskRow) -> String {
    if t.done {
        word_tag("done", "Done")
    } else if t.overdue {
        word_tag("stop", "Overdue")
    } else if t.due.is_empty() {
        word_tag("", "To do")
    } else {
        word_tag("wait", "Due")
    }
}

fn task_done_button(t: &TaskRow) -> String {
    if t.done {
        return String::new();
    }
    format!(
        "<form class=inline method=post action='/hub/tasks'><input type=hidden name=what value=done>\
         <input type=hidden name=id value={}><button class=go aria-label='Mark done: {}'>Done</button></form>",
        t.id,
        esc(&t.what)
    )
}

/// Shared tasks as the design's database view: Table, Board or Calendar,
/// over the same rows. `today` is today's day index in your zone.
pub fn shared_tasks_page(tasks: &[TaskRow], businesses: &[String], chosen: Option<&str>, view: TaskView, today: i64) -> String {
    let biz = chosen.map(|s| s.to_string()).or_else(|| businesses.first().cloned());
    let rows: Vec<&TaskRow> = tasks
        .iter()
        .filter(|t| biz.as_deref().map(|b| t.business.eq_ignore_ascii_case(b)).unwrap_or(true))
        .collect();
    let q = biz.as_deref().map(|b| format!("&amp;b={}", esc(b))).unwrap_or_default();
    let mut body = String::from("<nav class=views aria-label='How to show the tasks'>");
    for v in [TaskView::Table, TaskView::Board, TaskView::Calendar] {
        let here = v == view;
        body.push_str(&format!(
            "<a class='view{}' href='{}?view={}{q}'{}>{}</a>",
            if here { " here" } else { "" },
            Page::SharedTasks.href(),
            v.key(),
            if here { " aria-current=page" } else { "" },
            match v {
                TaskView::Table => "Table",
                TaskView::Board => "Board",
                TaskView::Calendar => "Calendar",
            }
        ));
    }
    body.push_str("</nav>");
    if businesses.is_empty() {
        body.push_str(&empty("Shared tasks belong to a business, and there isn't one yet."));
        body.push_str(&format!("<p><a class=more href='{}'>How to add a business</a></p>", Page::Business.href()));
        return shell_at(Some(Page::SharedTasks), "Shared tasks", &body);
    }
    match view {
        TaskView::Table => {
            if rows.is_empty() {
                body.push_str(&empty("No shared tasks yet."));
            } else {
                body.push_str(
                    "<div class=tablewrap tabindex=0 role=region aria-label='Shared tasks, scrolls sideways'><table class=data><caption class=sr>Shared tasks</caption><thead><tr>\
                     <th scope=col>Task</th><th scope=col>Status</th><th scope=col>Due</th><th scope=col>How it got here</th>\
                     <th scope=col><span class=sr>Actions</span></th></tr></thead><tbody>",
                );
                for t in &rows {
                    body.push_str(&format!(
                        "<tr><th scope=row>{}</th><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                        esc(&t.what),
                        task_status(t),
                        if t.due.is_empty() { "—".to_string() } else { esc(&t.due) },
                        if t.from_personal { "Shared from personal, on your yes" } else { "Made in the business" },
                        task_done_button(t)
                    ));
                }
                body.push_str("</tbody></table></div>");
            }
        }
        TaskView::Board => {
            body.push_str("<div class=columns>");
            for (name, pick) in [
                ("Overdue", Box::new(|t: &&TaskRow| !t.done && t.overdue) as Box<dyn Fn(&&TaskRow) -> bool>),
                ("To do", Box::new(|t: &&TaskRow| !t.done && !t.overdue)),
                ("Done", Box::new(|t: &&TaskRow| t.done)),
            ] {
                let col: Vec<&&TaskRow> = rows.iter().filter(|t| pick(t)).collect();
                body.push_str(&format!("<section class=col aria-label='{name}'><h2>{name} <small>{}</small></h2>", col.len()));
                if col.is_empty() {
                    body.push_str(&empty("None."));
                }
                for t in col {
                    body.push_str(&format!(
                        "<div class=card><span class=what>{}</span>{}{}{}</div>",
                        esc(&t.what),
                        task_status(t),
                        if t.due.is_empty() { String::new() } else { format!("<span class=chip>{}</span>", esc(&t.due)) },
                        task_done_button(t)
                    ));
                }
                body.push_str("</section>");
            }
            body.push_str("</div>");
        }
        TaskView::Calendar => {
            // The next two weeks, a row per week; undated tasks listed after.
            let start = today - (today + 3).rem_euclid(7);
            body.push_str(
                "<div class=tablewrap tabindex=0 role=region aria-label='Two weeks, scrolls sideways'><table class='data cal'><caption class=sr>Shared tasks by due date, two weeks</caption><thead><tr>",
            );
            for d in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
                body.push_str(&format!("<th scope=col>{d}</th>"));
            }
            body.push_str("</tr></thead><tbody>");
            for w in 0..2 {
                body.push_str("<tr>");
                for d in 0..7 {
                    let day = start + w * 7 + d;
                    let (_, m, dd) = crate::hubpages::ymd(day);
                    let here: Vec<&&TaskRow> = rows.iter().filter(|t| t.due_day == Some(day)).collect();
                    body.push_str(&format!(
                        "<td{}><span class=daynum>{} {}</span>",
                        if day == today { " class=today aria-current=date" } else { "" },
                        MONTHS[(m - 1) as usize],
                        dd
                    ));
                    for t in here {
                        body.push_str(&format!("<span class=calitem>{}{}</span>", task_status(t), esc(&t.what)));
                    }
                    body.push_str("</td>");
                }
                body.push_str("</tr>");
            }
            body.push_str("</tbody></table></div>");
            let undated: Vec<&&TaskRow> = rows.iter().filter(|t| t.due_day.is_none() && !t.done).collect();
            if !undated.is_empty() {
                body.push_str("<h2>No date</h2><ul class=plainlist>");
                for t in undated {
                    body.push_str(&format!("<li>{}</li>", esc(&t.what)));
                }
                body.push_str("</ul>");
            }
        }
    }
    // Add one: labelled, with a date field the browser helps with.
    body.push_str(&format!(
        "<form class=addtask method=post action='/hub/tasks'><input type=hidden name=what value=add>\
         <input type=hidden name=b value='{b}'>\
         <label for=tasktext>New shared task for {b}</label><input autocomplete=off id=tasktext name=text required>\
         <label for=taskdue>Due (optional)</label><input id=taskdue name=due type=date>\
         <button class=primary>Add</button></form>",
        b = esc(biz.as_deref().unwrap_or_default())
    ));
    shell_at(Some(Page::SharedTasks), "Shared tasks", &body)
}

pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// (year, month 1-12, day) of a day count since 1970-01-01. Howard Hinnant's
/// civil-from-days, the arithmetic every calendar library uses.
pub fn ymd(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Day count since 1970-01-01 of a date written "YYYY-MM-DD" (what a date
/// field sends). `None` for anything else.
pub fn days_of(date: &str) -> Option<i64> {
    let mut it = date.trim().split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || it.next().is_some() {
        return None;
    }
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// A client, with the firewall made visible on the record.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientRow {
    pub address: String,
    pub name: String,
    pub phone: String,
    pub notes: String,
    pub added: String,
    /// Replies Atlas has sent them for you, and when the last went.
    pub sent: usize,
    pub last_sent: String,
}

pub fn clients_page(clients: &[ClientRow], open: Option<&str>, notice: Option<&str>) -> String {
    let mut body = notice
        .map(|n| format!("<p class=notice role=status>{}</p>", esc(n)))
        .unwrap_or_default();
    body.push_str("<div class=msgs><nav class=roomlist aria-label='Clients'><ul>");
    if clients.is_empty() {
        body.push_str("<li>");
        body.push_str(&empty("No clients yet."));
        body.push_str("</li>");
    }
    let chosen = open.and_then(|a| clients.iter().find(|c| c.address.eq_ignore_ascii_case(a))).or(clients.first());
    for c in clients {
        let here = chosen.map(|x| x.address == c.address).unwrap_or(false);
        body.push_str(&format!(
            "<li><a class='room{on}' href='{href}?c={addr}'{cur}><span class=av aria-hidden=true>{ini}</span>\
             <span class=rn><b>{name}</b><span class=last>{addr}</span></span></a></li>",
            on = if here { " on" } else { "" },
            cur = if here { " aria-current=page" } else { "" },
            href = Page::Clients.href(),
            addr = esc(&c.address),
            ini = esc(&initials(&c.name)),
            name = esc(&c.name),
        ));
    }
    body.push_str(
        "</ul><form class=start method=post action='/hub/clients'><input type=hidden name=what value=add>\
         <label for=caddr>Email address</label><input id=caddr name=address type=email autocomplete=off required>\
         <label for=cname>Name</label><input id=cname name=name autocomplete=off>\
         <button>Add client</button></form></nav>",
    );
    match chosen {
        None => body.push_str(&format!("<section class=convo>{}</section>", empty("Add a client to see their record."))),
        Some(c) => body.push_str(&format!(
            "<section class=convo aria-labelledby=ctitle><div class=convohead><h2 id=ctitle>{name}</h2></div>\
             <dl class=facts><dt>Email</dt><dd>{addr}</dd><dt>Phone</dt><dd>{phone}</dd><dt>Added</dt><dd>{added}</dd>\
             <dt>Notes</dt><dd>{notes}</dd></dl>\
             <div class=wall><h3>What {name} can see</h3><ul class=plainlist>\
             <li>{sent}</li></ul>\
             <h3>What {name} can't see</h3><ul class=plainlist>\
             <li>{no} Nothing personal — the firewall stops it, and anything crossing waits for your yes.</li>\
             <li>{no} No other client, and nothing from any other business.</li>\
             <li>{no} Nothing else on this machine.</li></ul></div>\
             <p><a class=btn href='/hub/clients.vcf'>Export clients as a contact card file</a></p></section>",
            name = esc(&c.name),
            addr = esc(&c.address),
            phone = if c.phone.is_empty() { "—".to_string() } else { esc(&c.phone) },
            added = esc(&c.added),
            notes = if c.notes.is_empty() { "—".to_string() } else { esc(&c.notes) },
            sent = if c.sent == 0 {
                "Nothing has been sent to them through Atlas.".to_string()
            } else {
                format!("{} {} sent through Atlas, the last {}.", c.sent, if c.sent == 1 { "reply" } else { "replies" }, esc(&c.last_sent))
            },
            no = word_tag("stop", "Not shared"),
        )),
    }
    body.push_str("</div>");
    shell_at(Some(Page::Clients), "Clients", &body)
}

/// Whether a partner's Atlas is up, in a word: paired or not, and when it was
/// last heard from (`kin::Reached`).
#[derive(Debug, Clone, PartialEq)]
pub enum Reach {
    NotPaired,
    /// Paired, never heard from.
    Unheard,
    /// Heard from within `kin::ONLINE_SECS`.
    Online,
    /// Last heard from this long ago, in words.
    LastHeard(String),
}

/// Partners across your businesses: (name, businesses, reach, trusted).
pub fn partners_page(rows: &[(String, Vec<String>, Reach, bool)]) -> String {
    let mut body = String::from(
        "<p class=lead>The people you run a business with. Each has their own Atlas; what you share with them \
         goes Atlas-to-Atlas, encrypted, and only what the business's roster allows.</p>",
    );
    if rows.is_empty() {
        body.push_str(&empty("No partners yet. A partner is someone you've paired with and added to a business's roster."));
    } else {
        body.push_str(
            "<div class=tablewrap tabindex=0 role=region aria-label='Partners, scrolls sideways'><table class=data><caption class=sr>Partners</caption><thead><tr>\
             <th scope=col>Partner</th><th scope=col>Businesses</th><th scope=col>Their Atlas</th><th scope=col>Sending</th></tr></thead><tbody>",
        );
        for (n, b, up, trusted) in rows {
            body.push_str(&format!(
                "<tr><th scope=row>{}</th><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(n),
                esc(&b.join(", ")),
                match up {
                    Reach::Online => word_tag("ok", "Online"),
                    Reach::LastHeard(ago) => word_tag("wait", &format!("Offline · last heard {ago}")),
                    Reach::Unheard => word_tag("", "Paired · not heard from yet"),
                    Reach::NotPaired => word_tag("stop", "Not paired"),
                },
                if *trusted { word_tag("ok", "Trusted — no prompt") } else { word_tag("", "Asks first") }
            ));
        }
        body.push_str("</tbody></table></div>");
    }
    shell_at(Some(Page::Partners), "Partners", &body)
}

// ---------------------------------------------------------------- Sound & voice

/// One voice on the Sound page.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceRow {
    pub id: String,
    pub name: String,
    /// "British · dry, low".
    pub what: String,
    pub installed: bool,
    pub chosen: bool,
    /// Its sample can be played (`voicepick`).
    pub hear: bool,
    /// "63 MB", for the Get button.
    pub size: String,
    /// "Getting it: 34%" or why it couldn't, while there's something to say.
    pub getting: Option<String>,
}

/// Which engine speaks, and whether Kokoro is here (28 Sep 2026).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EngineView {
    pub kokoro_chosen: bool,
    /// Kokoro's library and model are both downloaded.
    pub kokoro_ready: bool,
    /// Kokoro can be had on this kind of computer at all.
    pub kokoro_here: bool,
    pub kokoro_mb: u64,
    /// "Getting it: 34%" or why it couldn't, while there's something to say.
    pub getting: Option<String>,
    /// Why Atlas last spoke in piper with Kokoro chosen.
    pub fell_back: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SoundView {
    pub engine: EngineView,
    pub voices: Vec<VoiceRow>,
    pub speed: f32,
    /// "hands_free", "always" or "never".
    pub speak_replies: String,
    pub volume: u8,
    pub muted: bool,
    pub wake_on: bool,
    pub wake_phrase: String,
    pub ptt_on: bool,
    pub ptt_key: String,
    pub typing_key: String,
    pub quiet_on: bool,
    pub quiet_from: String,
    pub quiet_to: String,
    /// Whether this device can listen and speak at all (a phone shell may).
    pub mics: Vec<String>,
}

fn switch(name: &str, label: &str, on: bool) -> String {
    format!(
        "<form class=switchrow method=post action='/hub/sound'><input type=hidden name=key value='{name}'>\
         <input type=hidden name=value value='{next}'><span id=l-{name}>{label}</span>\
         <button role=switch aria-checked='{on}' aria-labelledby=l-{name} class='sw{cls}'><span class=knob></span>\
         <span class=sr>{word}</span></button></form>",
        next = if on { "off" } else { "on" },
        on = on,
        cls = if on { " on" } else { "" },
        word = if on { "On" } else { "Off" },
        label = esc(label),
    )
}

/// The engine choice: piper or Kokoro, and getting Kokoro.
fn engine_choice(e: &EngineView) -> String {
    let mut out = String::from(
        "<form method=post action='/hub/sound'><input type=hidden name=key value=engine>\
         <fieldset class=seg3><legend>Voice engine</legend>",
    );
    for (val, label, on) in [("piper", "piper — small and plain", !e.kokoro_chosen), ("kokoro", "Kokoro — much more natural", e.kokoro_chosen)] {
        out.push_str(&format!(
            "<label><input type=radio name=value value={val}{}> {label}</label>",
            if on { " checked" } else { "" }
        ));
    }
    out.push_str("</fieldset><button>Use this engine</button></form>");
    let status = if !e.kokoro_here {
        "Kokoro isn't available on this kind of computer yet.".to_string()
    } else if e.kokoro_ready {
        "Kokoro is downloaded and runs on this computer's processor; nothing is sent away.".to_string()
    } else {
        match &e.getting {
            Some(said) if said.starts_with("Getting") => format!("<span role=status>{}</span>", esc(said)),
            said => format!(
                "{why}Kokoro isn't downloaded yet. \
                 <form class=inline method=post action='/hub/sound'><input type=hidden name=key value=get-kokoro>\
                 <button>Get Kokoro · {mb} MB</button></form>",
                why = said.as_ref().map(|s| format!("<span role=status>{}</span> ", esc(s))).unwrap_or_default(),
                mb = e.kokoro_mb
            ),
        }
    };
    out.push_str(&format!("<p class=note>{status} A change of engine is used from Atlas's next start.</p>"));
    if e.kokoro_chosen && !e.kokoro_ready {
        out.push_str("<p class=note>Until Kokoro is here, Atlas speaks in piper.</p>");
    } else if let Some(why) = &e.fell_back {
        if e.kokoro_chosen {
            out.push_str(&format!("<p class=note role=status>{}</p>", esc(why)));
        }
    }
    out
}

pub fn sound_page(v: &SoundView, notice: Option<&str>) -> String {
    let mut body = notice
        .map(|n| format!("<p class=notice role=status>{}</p>", esc(n)))
        .unwrap_or_default();
    body.push_str(&format!(
        "<div class=soundtop><p class=lead>Atlas hears and speaks on your machine. The wake word and the voice run \
         offline; nothing is sent away to listen or talk.</p>\
         <form method=post action='/hub/sound'><input type=hidden name=key value=muted>\
         <input type=hidden name=value value='{mv}'><button class='{mc}' aria-pressed='{m}'>{ml}</button></form></div>",
        mv = if v.muted { "off" } else { "on" },
        mc = if v.muted { "primary" } else { "" },
        m = v.muted,
        ml = if v.muted { "Unmute Atlas" } else { "Mute Atlas" },
    ));
    body.push_str("<div class=two><section><h2>Atlas's voice</h2>");
    body.push_str(&engine_choice(&v.engine));
    body.push_str("<form method=post action='/hub/sound'>\
                   <input type=hidden name=key value=voice><fieldset><legend class=sr>Voice</legend>");
    for r in &v.voices {
        // Hear it (its own sample, through Atlas) and, until it's here, get
        // it. The Get button belongs to the form after this one (`form=`),
        // since a form can't sit inside the voice list's form.
        let hear = if r.hear {
            format!(
                "<audio class=hear controls preload=none src='/hub/voice-sample?id={id}' aria-label='Hear {name}'></audio>",
                id = esc(&r.id),
                name = esc(&r.name)
            )
        } else {
            String::new()
        };
        let get = match (&r.getting, r.installed) {
            (Some(said), _) if said.starts_with("Getting") => format!("<span role=status>{}</span>", esc(said)),
            (_, true) => String::new(),
            // Kokoro's voices come with Kokoro, not one by one.
            (_, false) if r.size.is_empty() => String::new(),
            (said, false) => format!(
                "{why}<button form=getvoice name=value value='{id}'>Get {name} · {size}</button>",
                why = said.as_ref().map(|s| format!("<span role=status>{}</span> ", esc(s))).unwrap_or_default(),
                id = esc(&r.id),
                name = esc(&r.name),
                size = esc(&r.size)
            ),
        };
        body.push_str(&format!(
            "<div class='voice{on}'><label><input type=radio name=value value='{id}'{ck}{dis}> <b>{name}</b> \
             <span>{what}{inst}</span></label>{hear}{get}</div>",
            on = if r.chosen { " on" } else { "" },
            ck = if r.chosen { " checked" } else { "" },
            dis = if r.installed { "" } else { " disabled" },
            id = esc(&r.id),
            name = esc(&r.name),
            what = esc(&r.what),
            inst = if r.installed { "" } else { " — not downloaded yet" },
        ));
    }
    body.push_str(&format!(
        "</fieldset><button>Use this voice</button></form>\
         <form id=getvoice method=post action='/hub/sound'><input type=hidden name=key value=get-voice></form>\
         <form method=post action='/hub/sound'><input type=hidden name=key value=speed>\
         <label for=spd>Speaking pace (higher is slower)</label><input id=spd name=value type=range min=0.7 max=1.4 step=0.05 value='{speed}'>\
         <output for=spd>{speed:.2}</output><button>Save</button></form>\
         <form method=post action='/hub/sound'><input type=hidden name=key value=volume>\
         <label for=vol>Speaking volume</label><input id=vol name=value type=range min=0 max=100 step=5 value='{vol}'>\
         <output for=vol data-unit=%>{vol}%</output><button>Save</button></form>\
         <form method=post action='/hub/sound'><input type=hidden name=key value=speak_replies>\
         <fieldset class=seg3><legend>Speak replies aloud</legend>",
        speed = v.speed,
        vol = v.volume,
    ));
    for (val, label) in [("always", "Always"), ("hands_free", "Hands-free only"), ("never", "Never")] {
        body.push_str(&format!(
            "<label><input type=radio name=value value={val}{}> {label}</label>",
            if v.speak_replies == val { " checked" } else { "" }
        ));
    }
    body.push_str("</fieldset><button>Save</button></form></section><section><h2>Listening</h2>");
    body.push_str(&switch("wake", "Wake word", v.wake_on));
    body.push_str(&format!(
        "<form method=post action='/hub/sound'><input type=hidden name=key value=wake_phrase>\
         <label for=wp>Wake word</label><input autocomplete=off id=wp name=value value='{}'><button>Save</button></form>\
         <p class=note>Heard on-device. Nothing leaves the machine to listen.</p>",
        esc(&v.wake_phrase)
    ));
    body.push_str(&switch("ptt", "Hold a key to talk", v.ptt_on));
    body.push_str(&format!(
        "<p class=note>Push-to-talk key: <kbd>{}</kbd> · typing box: <kbd>{}</kbd>. Change them in \
         <a href='{}#keys'>Settings</a> or by saying “set the talk key to F9”.</p>",
        esc(&v.ptt_key),
        esc(&v.typing_key),
        Page::Settings.href()
    ));
    body.push_str("<h3>Microphones</h3>");
    if v.mics.is_empty() {
        body.push_str(&empty("No microphone found on this device. You can still type everything."));
    } else {
        body.push_str("<ul class=plainlist>");
        for m in &v.mics {
            body.push_str(&format!("<li>{}</li>", esc(m)));
        }
        body.push_str("</ul>");
    }
    body.push_str("<h3>Quiet hours</h3>");
    body.push_str(&switch("quiet", "Quiet hours", v.quiet_on));
    body.push_str(&format!(
        "<form method=post action='/hub/sound'><input type=hidden name=key value=quiet_hours>\
         <label for=qf>From</label><input id=qf name=from type=time value='{}'>\
         <label for=qt>To</label><input id=qt name=to type=time value='{}'><button>Save</button></form>\
         <p class=note>Atlas won't speak or chime in this window. It still works, silently, and anything \
         it wanted to say waits in your brief.</p></section></div>",
        esc(&v.quiet_from),
        esc(&v.quiet_to)
    ));
    shell_at(Some(Page::Sound), "Sound & voice", &body)
}

// ---------------------------------------------------------------- Trusted

/// (contact, trusted)
pub fn trusted_page(people: &[(String, bool)], notice: Option<&str>) -> String {
    let mut body = notice
        .map(|n| format!("<p class=notice role=status>{}</p>", esc(n)))
        .unwrap_or_default();
    body.push_str(
        "<p class=lead>Pre-approve the people you share with often, so routine things flow instead of stopping you. \
         A trusted send goes with no prompt — just a heads-up and an Undo. <b>Personal files always ask first</b>, \
         trusted or not.</p>",
    );
    if people.is_empty() {
        body.push_str(&empty("Nobody paired yet. Pair with someone first, then you can trust them here."));
        return shell_at(Some(Page::Trusted), "Trusted", &body);
    }
    body.push_str("<ul class=trustlist>");
    for (p, t) in people {
        body.push_str(&format!(
            "<li><span class=av aria-hidden=true>{ini}</span><span class=tn>{name}</span>{state}\
             <form class=inline method=post action='/hub/trusted'><input type=hidden name=who value='{name}'>\
             <input type=hidden name=trust value='{next}'><button{cls} aria-label='{act} {name}'>{act}</button></form></li>",
            ini = esc(&initials(p)),
            name = esc(p),
            state = if *t { word_tag("ok", "Trusted — sends without asking") } else { word_tag("", "Asks first") },
            next = if *t { "no" } else { "yes" },
            cls = if *t { "" } else { " class=primary" },
            act = if *t { "Stop trusting" } else { "Trust" },
        ));
    }
    body.push_str("</ul>");
    shell_at(Some(Page::Trusted), "Trusted", &body)
}

// ---------------------------------------------------------------- Give

/// (what, kind, when, state)
///
/// `draft`: words already in the box, for you to read and send yourself
/// (a phone app's share or link opens the page this way; nothing is handed
/// over until the button is pressed).
pub fn give_page(recent: &[(String, String, String, String)], notice: Option<&str>, draft: Option<&str>) -> String {
    let mut body = notice
        .map(|n| format!("<p class=notice role=status>{}</p>", esc(n)))
        .unwrap_or_default();
    body.push_str(&format!(
        "<form class=givebar method=post action='/hub/give'><input type=hidden name=what value=hand>\
         <label for=givetext>Paste a link, or write what you want Atlas to look at</label>\
         <textarea autocomplete=off id=givetext name=text rows=3 required>{}</textarea>",
        esc(draft.unwrap_or(""))
    ));
    body.push_str(
        "\
         <label for=giveask>What should Atlas do with it? (optional)</label><input autocomplete=off id=giveask name=asked>\
         <button class=primary>Give it to Atlas</button></form>\
         <form class=givefile method=post action='/hub/give' enctype='multipart/form-data' id=giveform>\
         <label for=givefile>Or choose a file — any type, up to 20 MB</label>\
         <input id=givefile type=file name=file>\
         <button type=button id=givesend>Send the file</button>\
         <p id=givestatus class=note role=status aria-live=polite></p></form>\
         <script>(function(){var b=document.getElementById('givesend'),f=document.getElementById('givefile'),\
         s=document.getElementById('givestatus');if(!b)return;b.addEventListener('click',function(){\
         var x=f.files&&f.files[0];if(!x){s.textContent='Choose a file first.';f.focus();return;}\
         s.textContent='Reading '+x.name+'…';var r=new FileReader();r.onload=function(){\
         var d=String(r.result),c=d.indexOf(',');\
         fetch('/hand/file',{method:'POST',headers:{'Content-Type':'application/json'},credentials:'same-origin',\
         body:JSON.stringify({name:x.name,data:d.slice(c+1),from:'the hub'})})\
         .then(function(q){return q.ok?q.json():Promise.reject();}).then(function(j){s.textContent=j&&j.said?j.said:'Sent to Atlas: '+x.name+'.';})\
         .catch(function(){s.textContent='That didn\\u2019t go through — try again.';});};r.readAsDataURL(x);});})();</script>\
         <p class=note>It stays on this machine. From your phone, use Share and pick Atlas.</p>",
    );
    body.push_str("<h2>Handed to Atlas lately</h2>");
    if recent.is_empty() {
        body.push_str(&empty("Nothing yet."));
    } else {
        body.push_str("<ul class=plainlist>");
        for (what, kind, when, state) in recent {
            body.push_str(&format!(
                "<li><span>{}</span><span class=meta>{} · {} · {}</span></li>",
                esc(what),
                esc(kind),
                esc(when),
                esc(state)
            ));
        }
        body.push_str("</ul>");
    }
    shell_at(Some(Page::Give), "Give Atlas something", &body)
}

// ---------------------------------------------------------------- Offline

#[derive(Debug, Clone, PartialEq, Default)]
pub struct OfflineView {
    pub online: bool,
    /// What works right now, in words.
    pub live: Vec<String>,
    /// (what's waiting, since)
    pub waiting: Vec<(String, String)>,
}

pub fn offline_page(v: &OfflineView) -> String {
    let banner = if v.online {
        format!("<div class='banner ok' role=status>{} You're online. Anything queued is going out.</div>", word_tag("ok", "Online"))
    } else {
        format!(
            "<div class='banner wait' role=status>{} You're offline, and Atlas is still working. What needs the \
             internet is queued, and goes by itself, in order, with its original time, when you reconnect.</div>",
            word_tag("wait", "Offline")
        )
    };
    let mut live = String::from("<section><h2>Working now</h2><ul class=plainlist>");
    for l in &v.live {
        live.push_str(&format!("<li>{}{}</li>", word_tag("ok", "Live"), esc(l)));
    }
    live.push_str("</ul></section>");
    let mut wait = String::from("<section><h2>Waiting for a connection</h2>");
    if v.waiting.is_empty() {
        wait.push_str(&empty("Nothing is waiting."));
    } else {
        wait.push_str("<ul class=plainlist>");
        for (w, since) in &v.waiting {
            wait.push_str(&format!("<li>{}<span>{}</span><span class=meta>{}</span></li>", word_tag("wait", "Queued"), esc(w), esc(since)));
        }
        wait.push_str("</ul>");
    }
    wait.push_str("</section>");
    shell_at(Some(Page::Offline), "Offline", &format!("{banner}<div class=two>{live}{wait}</div>"))
}

// ---------------------------------------------------------------- Talk

/// (you said, Atlas said) — most recent last.
/// The Talk page's wait for a reply: see `talk_page`.
///
/// One question out at a time, and an answer that isn't the page's own (a
/// "busy" reply) is waited through, not read as "done" (29 Sep 2026: while
/// Atlas was stuck on a turn, a new fetch every 1.5 s piled up to the
/// hub's limit of open connections, every page was then refused, and the
/// "busy" reply -- with no `busy` in it -- reloaded into a page that never
/// came back).
pub const TALK_WAIT_SCRIPT: &str = "<script>(function(){var seen=null,out=false;var t=setInterval(function(){if(document.hidden||out)return;out=true;\
fetch('/hub/changed.json?p=talk',{credentials:'same-origin'}).then(function(r){out=false;return r.ok?r.json():null;}).then(function(j){\
if(!j||j.busy===undefined)return;if(!j.busy||(seen!==null&&j.v!==seen)){clearInterval(t);location.reload();return;}seen=j.v;}).catch(function(){out=false;});},1500);})();</script>";

pub fn talk_page(exchanges: &[(String, String)], pending: &[String], listening_here: bool) -> String {
    let mut body = String::from("<ol class=said aria-live=polite aria-label='What we said'>");
    if exchanges.is_empty() {
        body.push_str("<li>");
        body.push_str(&empty("Say or type anything — a whole errand in one breath is fine."));
        body.push_str("</li>");
    }
    for (you, atlas) in exchanges {
        body.push_str(&format!(
            "<li class='msg mine'><div class=bubble><span class=sr>You: </span>{}</div></li>\
             <li class=msg><div class=bubble><span class=from>Atlas: </span>{}</div></li>",
            esc(you),
            esc(atlas)
        ));
    }
    // Sent, and Atlas is still on it: shown straight away, and the page looks
    // again every two seconds until the reply is there.
    for you in pending {
        body.push_str(&format!(
            "<li class='msg mine'><div class=bubble><span class=sr>You: </span>{}</div></li>\
             <li class=msg><div class=bubble><span class=from>Atlas: </span><span class=note>thinking…</span></div></li>",
            esc(you)
        ));
    }
    body.push_str("</ol>");
    // Still thinking: the page asks every second and a half whether the
    // conversation has moved -- a few bytes (`/hub/changed.json`) -- and
    // reloads only when it has, or when Atlas is done. It reloaded itself
    // whole every two seconds (28 Sep 2026). Offline, the ask fails and the
    // page stays as it is.
    if !pending.is_empty() {
        body.push_str(TALK_WAIT_SCRIPT);
    }
    body.push_str(&format!(
        "<form class=compose method=post action='/hub/talk'>\
         <label for=talktext class=sr>Say something to Atlas</label>\
         <textarea autocomplete=off id=talktext name=text rows=2 required placeholder='Type to Atlas…'></textarea>\
         <input type=hidden id=talkspoken name=spoken value=0>\
         <button class=primary>Send</button></form>\
         <div class=holdtalk{hide}><button type=button id=holdtalk class=bigtalk aria-describedby=holdnote>\
         <svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true><rect x=9 y=3 width=6 height=12 rx=3 />\
         <path d='M6 11a6 6 0 0 0 12 0M12 17v4'/></svg>Hold to talk</button>\
         <p id=holdnote class=note>Press and hold, speak, let go. Or press it once to start and again to stop. \
         Listening happens on this device.</p></div>\
         <script>(function(){{var s=window.AtlasShell;var b=document.getElementById('holdtalk');\
         if(!b)return;if(!s||!s.listen){{b.parentNode.hidden=true;return;}}b.parentNode.hidden=false;var on=false;\
         function go(){{if(on)return;on=true;b.setAttribute('aria-pressed','true');s.listen();}}\
         function stop(){{if(!on)return;on=false;b.setAttribute('aria-pressed','false');s.stop();}}\
         b.addEventListener('pointerdown',go);b.addEventListener('pointerup',stop);b.addEventListener('pointercancel',stop);\
         b.addEventListener('keydown',function(e){{if(e.key===' '||e.key==='Enter'){{e.preventDefault();on?stop():go();}}}});\
         window.atlasHeard=function(t){{if(!t)return;var f=document.getElementById('talktext');f.value=t;\
         document.getElementById('talkspoken').value='1';f.form.submit();}};\
         if(/[?&]say=1/.test(location.search)&&s.speak&&{pending_none}){{var r=document.querySelectorAll('.msg:not(.mine) .bubble');\
         if(r.length)s.speak(r[r.length-1].textContent.replace(/^Atlas: /,''));}}}})();</script>",
        hide = if listening_here { "" } else { " hidden" },
        pending_none = if pending.is_empty() { "true" } else { "false" },
    ));
    shell_at(Some(Page::Talk), "Talk", &body)
}

// ---------------------------------------------------------------- Start a project

pub fn new_project_form() -> String {
    format!(
        "<form class=addtask method=post action='/hub/workshop'><input type=hidden name=what value=new>\
         <label for=pfirst>Start a project — what should Atlas take on?</label>\
         <textarea autocomplete=off id=pfirst name=task rows=2 required placeholder='Speed up the report export and prove it against the guard suite'></textarea>\
         <label for=pname>Call it (optional)</label><input autocomplete=off id=pname name=name>\
         <label for=pfolder>Folder on this machine (optional)</label><input autocomplete=off id=pfolder name=folder>\
         <button class=primary>Start — Atlas will plan it</button></form><p class=note>Say it in your words. Atlas plans it \
         first, builds any change in a copy, and nothing lands until you approve it here on <a href='{}'>Projects</a>. \
         Offline, it starts planning and syncs when you reconnect.</p>",
        Page::Workshop.href()
    )
}

// ---------------------------------------------------------------- Help & accessibility

/// Help, and the accessibility statement (EN 301 549 clause 12; the EAA's
/// Annex V asks for the same things a UK public-sector statement does).
pub fn help_page(reviewed: &str, notice: Option<&str>) -> String {
    let mut body = notice
        .map(|n| format!("<p class=notice role=status>{}</p>", esc(n)))
        .unwrap_or_default();
    body.push_str(&format!(
        "<nav class=jump aria-label='On this page'><a href='#use'>Using Atlas your way</a><a href='#keys'>Keyboard</a>\
         <a href='#statement'>Accessibility statement</a><a href='#report'>Report a barrier</a></nav>\
         <h2 id=use>Using Atlas your way</h2>\
         <ul class=plainlist>\
         <li><b>Screen readers.</b> Every page is real headings, landmarks, lists, tables and labelled controls, so \
         NVDA, JAWS, Narrator, VoiceOver and TalkBack read it. Atlas's own windows expose themselves to the \
         screen reader through the system's accessibility service (UI Automation on Windows).</li>\
         <li><b>Text size.</b> Aa → Text size, or your browser's or phone's own text size: everything reflows, \
         down to a 320-pixel-wide screen, with nothing cut off.</li>\
         <li><b>Contrast and colour.</b> Warm Paper, Ember Dark and a colour-blind-safe set (Settings → How it \
         looks). High contrast follows your system's setting, and Windows' high-contrast themes are used as they are. \
         Status is always an icon and a word, never colour alone.</li>\
         <li><b>Motion.</b> Aa → Reduce motion, or your system's setting. Live pages update quietly, and you can \
         pause the updates.</li>\
         <li><b>Voice or no voice.</b> Everything you can say, you can type or click. Gestures are optional and \
         can never approve anything that can't be undone.</li>\
         <li><b>Any screen, either way up.</b> A phone, a folding phone folded or open, a tablet upright or on \
         its side, a laptop: the pages rearrange for the room there is and never lock the orientation. On a \
         touch screen every control is at least 44 points.</li>\
         <li><b>Atlas's own windows</b> follow Windows: its dark mode when you choose “Follow this computer”, its \
         high-contrast colours, its text size and its animation setting.</li>\
         <li><b>Sign-in.</b> Nothing asks you to remember or solve anything to get in; pairing is a link you open, \
         and you can paste anything Atlas asks for.</li></ul>\
         <h2 id=keys>Keyboard</h2><dl class=facts>\
         <dt><kbd>Tab</kbd> / <kbd>Shift</kbd>+<kbd>Tab</kbd></dt><dd>Move between links and controls. The first \
         Tab offers “Skip to the page”.</dd>\
         <dt><kbd>Ctrl</kbd>+<kbd>K</kbd> (<kbd>⌘</kbd>+<kbd>K</kbd> on a Mac or an iPad keyboard)</dt><dd>Find anything, from any page.</dd>\
         <dt><kbd>Enter</kbd> / <kbd>Space</kbd></dt><dd>Press the control that has focus.</dd>\
         <dt><kbd>Esc</kbd></dt><dd>Close the finder or a menu.</dd>\
         <dt>The talk and typing keys</dt><dd>Yours to change (Sound &amp; voice, or Settings); no single-letter \
         shortcut is ever active while you type.</dd></dl>\
         <h2 id=statement>Accessibility statement</h2>\
         <p>Atlas aims to meet <b>WCAG 2.2 level AA</b> and the software requirements of <b>EN 301 549</b> (the \
         European standard the European Accessibility Act points to), on the laptop, in its own windows, and on \
         phones.</p>\
         <h3>How it's checked</h3><ul class=plainlist>\
         <li>Every hub page is rendered and checked with axe-core's WCAG 2.2 AA rules at ten screen sizes — a \
         320-pixel phone, a phone upright and on its side, a folding phone folded and open both ways, two iPads \
         both ways up, and a laptop — in each colourway, with nothing scrolling sideways at any of them. Atlas's \
         own tests hold every page to a language, a skip link, one main landmark, labelled controls, help in \
         the same place and no timed refresh.</li>\
         <li>Keyboard-only use of every page.</li></ul>\
         <h3>What isn't there yet</h3><ul class=plainlist>\
         <li>Hands-on testing with each screen reader on real devices (NVDA/JAWS/Narrator on Windows, VoiceOver on \
         iPhone, TalkBack on Android) has not been done yet.</li>\
         <li>The phone apps' native parts (the share sheet, the lock-screen card) are written but have not been \
         built and tested on a phone yet.</li>\
         <li>Speaking volume, captions and audio description don't apply yet: Atlas plays no video, and it has no \
         voice or video calls between people. If calls are added, real-time text comes with them.</li></ul>\
         <p>This statement was last reviewed on {reviewed}.</p>\
         <h2 id=report>Report a barrier</h2>\
         <form class=addtask method=post action='/hub/help'><input type=hidden name=what value=report>\
         <label for=barrier>What got in your way, and where?</label><textarea autocomplete=off id=barrier name=text rows=3 required></textarea>\
         <button class=primary>Send the report</button></form>\
         <p class=note>On your own Atlas this is kept on your list to fix. On a friend's Atlas it goes to the person \
         who gave it to them, with nothing else attached. You can also say “that was hard to use”.</p>",
        reviewed = esc(reviewed)
    ));
    shell_at(Some(Page::Help), "Help & accessibility", &body)
}

// ---------------------------------------------------------------- Updates

/// Everything the Updates page shows (OPEN_GAPS 8.2), from `update_courier`,
/// `update_apply` and `upgrade`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UpdatesView {
    pub version: String,
    /// What has been heard from the release channel, in a sentence.
    pub heard: String,
    /// A newer release this device can take: (version, arrived and checked, MB).
    pub available: Option<(String, bool, u64)>,
    /// A build checked and waiting for the next start.
    pub pending: Option<String>,
    /// A new build on trial, in a sentence.
    pub trial: Option<String>,
    /// The version kept to go back to.
    pub previous: Option<String>,
    /// "default", "on", "ask" or "off".
    pub mode: String,
    /// (when, what), latest first.
    pub history: Vec<(String, String)>,
    /// On the releaser's own Atlas: (version, one line per report, held).
    pub failures: Vec<(String, Vec<String>, bool, String)>,
    /// Showing the "are you sure" for going back.
    pub confirming_undo: bool,
    /// The release key, when this build doesn't carry one yet.
    pub key: ReleaseKey,
    /// Sending an update: only on the releaser's own Atlas, once a build
    /// carries their key.
    pub send: Option<SendBuild>,
}

/// Sending an update to friends from this page, with no terminal (27 Sep
/// 2026): Atlas finds the build in Downloads or on the Desktop itself.
#[derive(Debug, Clone, PartialEq)]
pub enum SendBuild {
    /// Nothing there that could be a build of Atlas.
    NothingFound,
    /// The newest download, and why it can't go out.
    Unusable { name: String, why: String },
    /// Ready to sign: the download, how long ago, what's in it, and the
    /// program's fingerprint (so the button signs exactly what was shown).
    Ready { name: String, ago: String, version: String, platform: String, sha: String },
    /// This very build already went out.
    AlreadySent { version: String, sequence: u64 },
}

/// Where the release key stands, for the Updates page. Made here, with a
/// button, so nobody has to open a terminal to become the person who sends
/// Atlas out (27 Sep 2026).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ReleaseKey {
    /// This build carries a key: nothing to do.
    #[default]
    InTheBuild,
    /// No key anywhere yet. `vault_set`: the vault already has a passphrase
    /// (otherwise what's typed becomes it, and is asked twice).
    NotMade { vault_set: bool },
    /// Made; the card still has to reach the build.
    Made { card: String },
    /// Just made, this page only: the card and the recovery key, shown once.
    JustMade { card: String, recovery: String },
}

pub fn updates_page(v: &UpdatesView, notice: Option<&str>) -> String {
    let mut body = notice.map(|n| format!("<p class=notice role=status>{}</p>", esc(n))).unwrap_or_default();
    body.push_str(&format!("<dl class=facts><dt>This is</dt><dd>Atlas {}</dd><dt>Heard</dt><dd>{}</dd></dl>", esc(&v.version), esc(&v.heard)));
    match (&v.pending, &v.available) {
        (Some(p), _) => body.push_str(&format!(
            "<div class='banner ok' role=status>{} Atlas {} is checked and goes in at the next start. Your version now is kept.</div>",
            word_tag("ok", "Ready"),
            esc(p)
        )),
        (None, Some((ver, true, mb))) => body.push_str(&format!(
            "<div class='banner wait'>{} Atlas {ver} ({mb} MB) has arrived and matches what the release key signed.\
             <form class=inline method=post action='/hub/updates'><input type=hidden name=what value=install>\
             <button class=primary>Install Atlas {ver} now</button></form>\
             <span class=note>Atlas restarts into it. It has to pass its health check first, and the version you're on now is kept.</span></div>",
            word_tag("wait", "Ready to install"),
            ver = esc(ver),
        )),
        (None, Some((ver, false, mb))) => body.push_str(&format!(
            "<div class='banner wait' role=status>{} Atlas {} ({mb} MB) is on its way — it arrives in checked pieces from your \
             friends' Atlases and is kept only if it matches the release key's signature.</div>",
            word_tag("wait", "Arriving"),
            esc(ver)
        )),
        (None, None) => body.push_str(&empty("There's no newer Atlas waiting.")),
    }
    if let Some(t) = &v.trial {
        body.push_str(&format!("<p>{} {}</p>", word_tag("info", "On trial"), esc(t)));
    }
    body.push_str("<h2>Going back</h2>");
    match (&v.previous, v.confirming_undo) {
        (None, _) => body.push_str(&empty("No earlier version is kept here to go back to.")),
        (Some(prev), false) => body.push_str(&format!(
            "<p>Atlas {prev} is kept. <a class=btn href='/hub/updates?confirm=undo'>Go back to Atlas {prev}…</a></p>",
            prev = esc(prev)
        )),
        (Some(prev), true) => body.push_str(&format!(
            "<div class='banner stop' role=alert>{} Go back from Atlas {now} to {prev}? Atlas {now} won't be offered again; \
             a newer release will be.\
             <form class=inline method=post action='/hub/updates'><input type=hidden name=what value=undo-confirmed>\
             <button class=primary>Yes, go back to Atlas {prev}</button></form> <a href='/hub/updates'>Keep Atlas {now}</a></div>",
            word_tag("stop", "Check"),
            now = esc(&v.version),
            prev = esc(prev),
        )),
    }
    body.push_str("<h2>How updates go in here</h2><form class=choices method=post action='/hub/updates'>\
                   <input type=hidden name=what value=mode><fieldset><legend>Installing updates</legend>");
    for (value, label) in [
        ("default", "The usual: by itself at a quiet moment on your own devices, asking first on friends'"),
        ("on", "By itself, at a quiet moment"),
        ("ask", "Ask me first, every time"),
        ("off", "Off — only tell me what's out"),
    ] {
        body.push_str(&format!(
            "<label><input type=radio name=mode value={value}{}> {}</label>",
            if v.mode == value { " checked" } else { "" },
            esc(label)
        ));
    }
    body.push_str("</fieldset><button>Keep this</button></form>");
    if !v.failures.is_empty() {
        body.push_str("<h2>Updates that failed</h2><p class=note>From your own devices, and friends who chose to tell you. \
                       A build stays yours to hold: nothing here stops a release by itself.</p><ul class=plainlist>");
        for (ver, lines, held, sha) in &v.failures {
            body.push_str(&format!("<li><b>Atlas {}</b>{}<ul>", esc(ver), if *held { format!(" {}", word_tag("stop", "Held")) } else { String::new() }));
            for l in lines {
                body.push_str(&format!("<li>{}</li>", esc(l)));
            }
            body.push_str("</ul>");
            if !*held && !sha.is_empty() {
                body.push_str(&format!(
                    "<form class=inline method=post action='/hub/updates'><input type=hidden name=what value=hold>\
                     <input type=hidden name=sha value='{}'><button aria-label='Stop handing out Atlas {v}'>Stop handing out Atlas {v}</button></form>",
                    esc(sha),
                    v = esc(ver)
                ));
            }
            body.push_str("</li>");
        }
        body.push_str("</ul>");
    }
    if !v.history.is_empty() {
        body.push_str("<h2>What happened to updates here</h2><ul class=plainlist>");
        for (when, what) in &v.history {
            body.push_str(&format!("<li><span class=meta>{}</span> {}</li>", esc(when), esc(what)));
        }
        body.push_str("</ul>");
    }
    if let Some(send) = &v.send {
        body.push_str(&send_block(send));
    }
    body.push_str(&release_key_block(&v.key));
    body.push_str("<p class=note>You can also say “any updates”, “install the update” or “go back to the last version”.</p>");
    shell_at(Some(Page::Updates), "Updates", &body)
}

/// The card, in a box to copy from.
fn key_card(card: &str) -> String {
    format!(
        "<label for=keycard>Your key card (public: safe to send)</label>\
         <textarea id=keycard readonly rows=2 autocomplete=off spellcheck=false>{}</textarea>\
         <button type=button onclick=\"navigator.clipboard.writeText(document.getElementById('keycard').value);this.textContent='Copied'\">Copy</button>",
        esc(card)
    )
}

fn send_block(s: &SendBuild) -> String {
    let body = match s {
        SendBuild::NothingFound => "<p>No new build of Atlas in your Downloads or on your Desktop. To send one: on GitHub, \
             open <b>Actions</b>, then <b>Windows app</b>, then the newest run, and download <b>Atlas-Windows</b>. \
             Then come back to this page. It finds the download by itself.</p>"
            .to_string(),
        SendBuild::Unusable { name, why } => format!(
            "<p>The newest download, <b>{}</b>, can't be sent: {}.</p>",
            esc(name),
            esc(why)
        ),
        SendBuild::AlreadySent { version, sequence } => format!(
            "<p>Atlas {} went out as release {sequence}. The next build needs a new version number before it can go.</p>",
            esc(version)
        ),
        SendBuild::Ready { name, ago, version, platform, sha } => format!(
            "<p>Found <b>{name}</b>, downloaded {ago}: Atlas {v} for {platform}.</p>\
             <p class=note>Signing it with your release key is how friends' Atlases know it came from you. \
             They fetch it from yours, check it, and try it before keeping it. If it fails, they go back by themselves.</p>\
             <form method=post action='/hub/updates'><input type=hidden name=what value=sign-send>\
             <input type=hidden name=sha value='{sha}'>\
             <label for=sp>Your vault passphrase</label><input id=sp name=passphrase type=password autocomplete=current-password required>\
             <button class=primary>Sign and send Atlas {v}</button></form>",
            name = esc(name),
            ago = esc(ago),
            v = esc(version),
            platform = esc(platform),
            sha = esc(sha),
        ),
    };
    format!("<section aria-labelledby=su><h2 id=su>Send an update to friends</h2>{body}</section>")
}

fn release_key_block(k: &ReleaseKey) -> String {
    match k {
        ReleaseKey::InTheBuild => String::new(),
        ReleaseKey::NotMade { vault_set } => format!(
            "<section aria-labelledby=rk><h2 id=rk>Your release key</h2>\
             <p>If you're the one who sends Atlas out to friends, this is the key that signs every update, so their \
             Atlases install only what you signed. It's made once, here, and kept in your vault. You'll get a recovery \
             key to write on paper.</p>\
             <form method=post action='/hub/updates'><input type=hidden name=what value=make-key>\
             <label for=vp>{label}</label><input id=vp name=passphrase type=password autocomplete={ac} required>{again}\
             <button class=primary>Make my release key</button></form></section>",
            label = if *vault_set { "Your vault passphrase" } else { "Choose a vault passphrase (the vault keeps the key)" },
            ac = if *vault_set { "current-password" } else { "new-password" },
            again = if *vault_set {
                String::new()
            } else {
                "<label for=vp2>The same again</label><input id=vp2 name=again type=password autocomplete=new-password required>".into()
            },
        ),
        ReleaseKey::Made { card } => format!(
            "<section aria-labelledby=rk><h2 id=rk>Your release key</h2>\
             <p>Made, and in your vault. One step left: send this card to Claude, so the next build of Atlas carries it. \
             Until then, copies don't accept updates from anyone.</p>{}</section>",
            key_card(card)
        ),
        ReleaseKey::JustMade { card, recovery } => format!(
            "<section aria-labelledby=rk class='banner stop'><h2 id=rk>Write this down now</h2>\
             <p><b>Your recovery key.</b> It's shown this once and kept nowhere, not even in your vault. \
             If your laptop is lost or the release key is stolen, it's the only way to take the key back. \
             Write it on paper and keep it away from this computer.</p>\
             <pre class=preview aria-label='Recovery key'>{}</pre>\
             <p>Then send the card below to Claude, so the next build carries your key.</p>{}</section>",
            esc(recovery),
            key_card(card)
        ),
    }
}

// ---------------------------------------------------------------- Feedback

/// Everything the Feedback page shows (OPEN_GAPS 8.14).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FeedbackView {
    /// Who it goes to: `None` when this Atlas isn't in anyone's release
    /// channel; "your own list" on the releaser's own Atlas.
    pub to: Option<String>,
    /// A failed update written down here, in a line, offered to attach.
    pub failure_here: Option<String>,
    /// Shown and waiting for Send: exactly what will go.
    pub draft: Option<String>,
    /// What came in: (number, from, version, platform, words, has an attachment, where it stands).
    pub inbox: Vec<(usize, String, String, String, String, bool, String)>,
    /// What you sent: (to, words, where it stands, their notes).
    pub sent: Vec<(String, String, String, Vec<String>)>,
}

pub fn feedback_page(v: &FeedbackView, notice: Option<&str>) -> String {
    let mut body = notice.map(|n| format!("<p class=notice role=status>{}</p>", esc(n))).unwrap_or_default();
    body.push_str("<h2 id=tell>Something's wrong with Atlas</h2>");
    match (&v.to, &v.draft) {
        (None, _) => body.push_str(&empty(
            "This Atlas isn't in anyone's release channel, so there's no one to send feedback to. \
             Help & accessibility has Report a barrier, which is kept here.",
        )),
        (Some(to), Some(draft)) => body.push_str(&format!(
            "<p>This is exactly what will go to {to} — nothing else:</p><pre class=preview>{}</pre>\
             <form class=inline method=post action='/hub/feedback'><input type=hidden name=what value=send>\
             <button class=primary>Send it to {to}</button></form>\
             <form class=inline method=post action='/hub/feedback'><input type=hidden name=what value=discard>\
             <button>Change it</button></form>",
            esc(draft),
            to = esc(to),
        )),
        (Some(to), None) => {
            body.push_str(&format!(
                "<form class=addtask method=post action='/hub/feedback'><input type=hidden name=what value=preview>\
                 <label for=fbwords>What's wrong? In your own words.</label>\
                 <textarea autocomplete=off id=fbwords name=words rows=4 required></textarea>{}\
                 <button class=primary>Show me what will go to {}</button></form>\
                 <p class=note>Nothing is sent until you've seen it and pressed Send.</p>",
                match &v.failure_here {
                    Some(f) => format!(
                        "<label><input type=checkbox name=attach value=yes> Attach what was written down about the update \
                         that failed here ({}) — your name and home folder are already taken out</label>",
                        esc(f)
                    ),
                    None => String::new(),
                },
                esc(to)
            ));
        }
    }
    if !v.inbox.is_empty() {
        body.push_str("<h2 id=inbox>From your friends</h2><ul class=plainlist>");
        for (n, from, ver, platform, words, attached, status) in &v.inbox {
            body.push_str(&format!(
                "<li><b>{n}. {from}</b> <span class=meta>Atlas {ver} · {platform}{att} · {status}</span><p>“{words}”</p>\
                 <form class=inline method=post action='/hub/feedback'><input type=hidden name=what value=answer>\
                 <input type=hidden name=n value={n}>\
                 <label for=fbs{n}>Answer</label><select id=fbs{n} name=status>\
                 <option value=seen>Seen</option><option value=fixing>Being fixed</option><option value=fixed>Fixed in…</option>\
                 <option value=wont>Won't change</option></select>\
                 <label for=fbv{n}>Version (for fixed)</label><input autocomplete=off id=fbv{n} name=version size=8>\
                 <label for=fbn{n}>A note for {from} (optional)</label><input autocomplete=off id=fbn{n} name=note>\
                 <button>Answer {from}</button></form></li>",
                from = esc(from),
                ver = esc(ver),
                platform = esc(platform),
                att = if *attached { " · update failure attached" } else { "" },
                status = esc(status),
                words = esc(words),
            ));
        }
        body.push_str("</ul>");
    }
    if !v.sent.is_empty() {
        body.push_str("<h2 id=sent>What you've sent</h2><ul class=plainlist>");
        for (to, words, status, notes) in &v.sent {
            body.push_str(&format!("<li>To <b>{}</b>: “{}” <span class=meta>{}</span>", esc(to), esc(words), esc(status)));
            for n in notes {
                body.push_str(&format!("<p class=note>They said: {}</p>", esc(n)));
            }
            body.push_str("</li>");
        }
        body.push_str("</ul>");
    }
    body.push_str("<p class=note>You can also say “report a bug” and what's wrong, or “any feedback”.</p>");
    shell_at(Some(Page::Feedback), "Feedback", &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_through_the_day_count() {
        for d in ["1970-01-01", "2026-09-26", "2000-02-29", "2031-12-31"] {
            let n = days_of(d).unwrap();
            let (y, m, dd) = ymd(n);
            assert_eq!(format!("{y:04}-{m:02}-{dd:02}"), d);
        }
        assert_eq!(days_of("2026-13-01"), None);
        assert_eq!(days_of("soon"), None);
        assert_eq!(days_of("1970-01-02"), Some(1));
    }

    #[test]
    fn initials_are_two_letters_and_never_empty() {
        assert_eq!(initials("Jo-2"), "J2");
        assert_eq!(initials("Maya Riverstone"), "MR");
        assert_eq!(initials("  "), "?");
    }
}

// ---------------------------------------------------------------- Your phone

/// A code on screen for a phone to scan: what it does, its address, and the
/// QR code itself (already SVG).
#[derive(Debug, Clone, PartialEq)]
pub struct PhoneCode {
    /// "add" (an iPhone telling Atlas its ID) or "install" (the app).
    pub what: String,
    pub url: String,
    pub qr: String,
    pub minutes_left: u64,
}

/// Everything the Your phone page shows (D5).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhoneView {
    pub kind: Option<crate::phoneadd::Kind>,
    pub code: Option<PhoneCode>,
    /// This Atlas's own iPhones and iPads that told it their ID.
    pub mine: Vec<crate::phoneadd::Device>,
    /// The iPhone app on this computer: (version, whether it installs on one of `mine`).
    pub ipa: Option<(String, bool)>,
    /// An iPhone app is here and still being read, off the loop
    /// (`hublive::ipa_facts`): said, rather than "no app yet".
    pub ipa_reading: bool,
    /// The Android app on this computer, by size in MB.
    pub apk: Option<u64>,
    /// On the releaser's Atlas: phones waiting for the next iPhone build.
    pub waiting: Vec<crate::phoneadd::Device>,
}

fn show_code(kind: crate::phoneadd::Kind, what: &str, label: &str) -> String {
    format!(
        "<form method=post action='/hub/phone'><input type=hidden name=what value=start>\
         <input type=hidden name=kind value={}><input type=hidden name=for value={what}>\
         <button class=primary>{}</button></form>",
        kind.slug(),
        esc(label)
    )
}

fn code_block(c: &PhoneCode, kind: crate::phoneadd::Kind) -> String {
    let how = match (c.what.as_str(), kind) {
        ("add", _) => "Open the Camera on the iPhone or iPad, point it at this code and tap the link. Then follow the three steps on its screen.",
        (_, crate::phoneadd::Kind::Apple) => "Open the Camera on the iPhone or iPad, point it at this code, tap the link, then tap Install.",
        _ => "Open the Camera on the phone, point it at this code and tap the link. When Android asks, allow installs from there, then tap Install.",
    };
    format!(
        "<div class='banner wait' role=status><div class=qr aria-hidden=true>{qr}</div><p>{how}</p>\
         <p class=note>The phone needs Tailscale switched on, signed in as you. This code works for {m} more minutes. \
         Or open this on the phone: <a href='{url}'>{url_t}</a></p>\
         <form method=post action='/hub/phone'><input type=hidden name=what value=stop><button>Stop showing it</button></form></div>",
        qr = c.qr,
        m = c.minutes_left,
        url = esc(&c.url),
        url_t = esc(&c.url),
    )
}

pub fn phone_page(v: &PhoneView, notice: Option<&str>) -> String {
    use crate::phoneadd::Kind;
    let mut body = notice.map(|n| format!("<p class=notice role=status>{}</p>", esc(n))).unwrap_or_default();
    body.push_str("<p class=lead>Atlas on your phone or iPad is the same Atlas as here. Pick which you have, then scan a code with it.</p>");
    body.push_str("<nav aria-label='Which phone' class=seg3>");
    for k in [Kind::Apple, Kind::Android] {
        let on = v.kind == Some(k);
        body.push_str(&format!(
            "<a class='button{}' href='/hub/phone?kind={}' aria-current='{}'>{}</a>",
            if on { " primary" } else { "" },
            k.slug(),
            if on { "page" } else { "false" },
            k.said()
        ));
    }
    body.push_str("</nav>");
    match v.kind {
        None => {}
        Some(Kind::Apple) => {
            body.push_str("<h2>1. Tell Atlas about it</h2>");
            if !v.mine.is_empty() {
                body.push_str("<ul class=plainlist>");
                for d in &v.mine {
                    body.push_str(&format!("<li>{} {}</li>", word_tag("ok", "Added"), esc(&d.name)));
                }
                body.push_str("</ul>");
                let unsent: Vec<crate::phoneadd::Device> = v.mine.iter().filter(|d| !d.sent).cloned().collect();
                if !unsent.is_empty() {
                    body.push_str(&format!(
                        "<p>Not sent on yet; Atlas keeps trying. Or send this to whoever sends you Atlas yourself:</p>\
                         <label for=udidcard>Your device's ID (safe to send)</label>\
                         <textarea id=udidcard readonly rows={} autocomplete=off spellcheck=false>{}</textarea>\
                         <button type=button onclick=\"navigator.clipboard.writeText(document.getElementById('udidcard').value);this.textContent='Copied'\">Copy</button>",
                        unsent.len(),
                        esc(&crate::phoneadd::devices_card(&unsent))
                    ));
                }
            }
            match &v.code {
                Some(c) if c.what == "add" => body.push_str(&code_block(c, Kind::Apple)),
                _ => {
                    body.push_str(
                        "<p>Apple only lets the app open on devices listed in it, by an ID the device reads out itself. \
                         This code lets the iPhone or iPad tell Atlas its ID; it changes nothing on the device.</p>",
                    );
                    body.push_str(&show_code(Kind::Apple, "add", if v.mine.is_empty() { "Show the code" } else { "Add another" }));
                }
            }
            body.push_str("<h2>2. Install the app</h2>");
            match (&v.ipa, &v.code) {
                (Some(_), Some(c)) if c.what == "install" => body.push_str(&code_block(c, Kind::Apple)),
                (Some((ver, true)), _) => {
                    body.push_str(&format!("<p>Atlas {} for iPhone and iPad is here, built for your device.</p>", esc(ver)));
                    body.push_str(&show_code(Kind::Apple, "install", "Show the install code"));
                }
                (Some((ver, false)), _) if !v.mine.is_empty() => body.push_str(&format!(
                    "<p>The app here (Atlas {}) was built before your device was added. The next iPhone build includes it. \
                     Once whoever sends you Atlas gives you that build, save it to Downloads and this page shows its code.</p>",
                    esc(ver)
                )),
                _ if v.mine.is_empty() => body.push_str("<p class=note>After step 1.</p>"),
                _ if v.ipa_reading => body.push_str(
                    "<p class=note>Reading the iPhone app that's on this computer — open this page again in a moment.</p>",
                ),
                _ => body.push_str(
                    "<p>Your device is on the list for the next iPhone build. Once whoever sends you Atlas gives you that build, \
                     save it to Downloads and this page shows its code.</p>",
                ),
            }
        }
        Some(Kind::Android) => match (&v.apk, &v.code) {
            (Some(_), Some(c)) => body.push_str(&code_block(c, Kind::Android)),
            (Some(mb), None) => {
                body.push_str(&format!("<p>The Android app is here ({mb} MB).</p>"));
                body.push_str(&show_code(Kind::Android, "install", "Show the install code"));
            }
            (None, _) => body.push_str(
                "<p>The Android app isn't on this computer yet. Save <b>Atlas.apk</b> (from whoever sends you Atlas) to \
                 Downloads, and this page finds it and shows its code.</p>",
            ),
        },
    }
    if v.kind.is_some() {
        body.push_str(
            "<p class=note>Until the app is on it, the phone can use Atlas in its browser: the code on the \
             <a href='/hub/sync'>Sync</a> page opens it.</p>",
        );
    }
    if !v.waiting.is_empty() {
        body.push_str("<h2>Waiting for the next iPhone build</h2><ul class=plainlist>");
        for d in &v.waiting {
            body.push_str(&format!(
                "<li>{}{}</li>",
                esc(&d.name),
                if d.from.is_empty() { String::new() } else { format!(" <span class=meta>from {}</span>", esc(&d.from)) }
            ));
        }
        body.push_str(&format!(
            "</ul><p>Send this list to Claude, and the next iPhone build includes them all.</p>\
             <label for=devcard>The list</label><textarea id=devcard readonly rows={} autocomplete=off spellcheck=false>{}</textarea>\
             <button type=button onclick=\"navigator.clipboard.writeText(document.getElementById('devcard').value);this.textContent='Copied'\">Copy</button>",
            v.waiting.len().clamp(2, 8),
            esc(&crate::phoneadd::devices_card(&v.waiting))
        ));
    }
    shell_at(Some(Page::Phone), "Your phone", &body)
}
