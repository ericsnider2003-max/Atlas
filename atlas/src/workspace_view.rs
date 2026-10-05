//! What you see when you ask what's outstanding.
//!
//! A flat list of tasks is what every app gives you and it's why nobody looks
//! at them twice. What makes a Notion-style workspace worth opening is not the
//! prettiness — it's three structural things:
//!
//! 1. **Everything is one kind of thing with properties**, so the same items
//!    can be a list today, a board tomorrow and a calendar next week without
//!    being re-entered.
//! 2. **Views are saved questions, not folders.** "What's blocked" isn't a
//!    place things live, it's a filter over everything.
//! 3. **Items link to each other**, so a task carries its project, its notes
//!    and the thing it's waiting on rather than referring to them.
//!
//! Atlas already has the items — outstanding work, captures, projects, mail
//! that needs you, content in progress. What it lacked was a way to look at
//! them together.

use serde::{Deserialize, Serialize};

/// One thing in the workspace. Everything is one of these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub kind: Kind,
    pub status: Status,
    /// When it matters, if it does.
    pub due: Option<u64>,
    /// What it belongs to.
    pub project: Option<String>,
    /// Who it's for. A project can be yours or a client's, and the same task
    /// under two clients is two different pieces of work — mixing them is how
    /// you bill the wrong person.
    pub client: Option<String>,
    /// Other items this one relates to. Not a hierarchy: a task can inform
    /// three things and belong to one.
    pub links: Vec<String>,
    /// What it's waiting on — another item, or a person.
    pub blocked_by: Option<String>,
    /// Where it came from, so you can get back to the original.
    pub from: Origin,
    pub at: u64,
    /// When it stopped being live. Kept rather than deleted — "what did I get
    /// done last Tuesday" has no answer if finished things vanish.
    pub closed_at: Option<u64>,
    /// Free-text tags you or Atlas attached.
    pub tags: Vec<String>,
    /// Roughly how long you think it'll take, in minutes.
    pub estimate_mins: Option<u32>,
    /// How long it has actually taken so far.
    ///
    /// Kept because the gap between this and the estimate is the only thing
    /// that ever makes the estimates better.
    pub spent_mins: u32,
    /// Whether Atlas could do this instead of you.
    pub atlas_could: Handoff,
    /// What Atlas was thinking about this, in order.
    pub thinking: Vec<Thought>,
}

/// Could Atlas take this off you?
///
/// The mark that saves the most time isn't a priority — it's noticing that
/// something on your list didn't need to be on your list. Being honest about
/// the middle case matters: "I could try" is different from "I can do this",
/// and pretending otherwise costs you more time than doing it yourself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Handoff {
    /// Atlas can do the whole thing.
    Yes,
    /// Atlas can get it most of the way and needs you at one point.
    MostOfIt,
    /// Atlas can prepare, you do it.
    PrepOnly,
    /// It needs you — a decision, a relationship, your hands.
    NeedsYou,
    /// Not looked at yet.
    Unknown,
}

impl Handoff {
    pub fn mark(&self) -> &'static str {
        match self {
            Handoff::Yes => "I can do this",
            Handoff::MostOfIt => "I can get most of the way",
            Handoff::PrepOnly => "I can set it up for you",
            Handoff::NeedsYou => "",
            Handoff::Unknown => "",
        }
    }

    /// Worth Atlas picking up in the background without being asked?
    ///
    /// Only the unambiguous case. Starting something it can only half do
    /// leaves you with a half-done thing and no idea where it stopped.
    pub fn safe_to_start_alone(&self) -> bool {
        *self == Handoff::Yes
    }
}

/// One step of Atlas's reasoning on an item.
///
/// Kept per item rather than in one log, because "why did this take three
/// days" is a question about the item and hunting for it in a global stream
/// is why nobody ever does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thought {
    pub at: u64,
    pub kind: Thinking,
    /// In your words.
    pub what: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Thinking {
    /// Why it thinks this matters now.
    Ranked,
    /// It tried something.
    Tried,
    /// It hit something.
    Stuck,
    /// It decided between options.
    Chose,
    /// It asked you.
    Asked,
    /// It noticed something about the item.
    Noticed,
}

impl Thinking {
    pub fn plain(&self) -> &'static str {
        match self {
            Thinking::Ranked => "put it here because",
            Thinking::Tried => "tried",
            Thinking::Stuck => "got stuck on",
            Thinking::Chose => "chose",
            Thinking::Asked => "asked you",
            Thinking::Noticed => "noticed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Task,
    Idea,
    Question,
    Decision,
    /// A piece of content in progress.
    Draft,
    /// Something Atlas is doing or has queued.
    Job,
    /// A message that needs you.
    Message,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Nothing has happened to it.
    Waiting,
    /// You're on it.
    Doing,
    /// Waiting on someone or something else.
    Blocked,
    /// Needs you to decide before anything can move.
    NeedsYou,
    Done,
    /// Deliberately not doing it.
    Dropped,
}

impl Status {
    /// Does it want your attention right now?
    pub fn live(&self) -> bool {
        matches!(self, Status::Waiting | Status::Doing | Status::Blocked | Status::NeedsYou)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// You said it.
    YouSaid,
    /// Captured from something you were looking at.
    Captured,
    /// Came out of your inbox.
    Mail,
    /// Atlas noticed it.
    Noticed,
    /// A step of something bigger.
    PartOfAJob,
}

/// A saved question over everything.
///
/// The whole point: a view is a filter, not a folder. Nothing is *in* a view,
/// so the same item appears in as many as it answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub name: String,
    /// How to show it.
    pub shape: Shape,
    pub only_kinds: Vec<Kind>,
    pub only_status: Vec<Status>,
    pub only_project: Option<String>,
    pub only_client: Option<String>,
    /// Look at a past day rather than now.
    pub as_of: Option<u64>,
    /// Group the results by this.
    pub grouped_by: Group,
    pub sort: Sort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// One column, most pressing first.
    List,
    /// Columns by status. Good for seeing what's stuck.
    Board,
    /// By date. Only useful when things have dates.
    Calendar,
    /// Counts and headlines, nothing you can act on directly.
    Dashboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Nothing,
    Status,
    Project,
    Kind,
    Day,
    Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// Soonest due, then oldest.
    Pressing,
    Newest,
    Oldest,
}

/// The views that ship.
///
/// Deliberately few. A workspace with thirty views is one where you spend your
/// time arranging views.
pub fn shipped() -> Vec<View> {
    vec![
        View {
            name: "Now".into(),
            shape: Shape::List,
            only_kinds: vec![],
            only_status: vec![Status::Doing, Status::NeedsYou],
            only_project: None,
            only_client: None,
            as_of: None,
            grouped_by: Group::Nothing,
            sort: Sort::Pressing,
        },
        View {
            name: "Everything live".into(),
            shape: Shape::Board,
            only_kinds: vec![],
            only_status: vec![Status::Waiting, Status::Doing, Status::Blocked, Status::NeedsYou],
            only_project: None,
            only_client: None,
            as_of: None,
            grouped_by: Group::Status,
            sort: Sort::Pressing,
        },
        View {
            name: "Stuck".into(),
            shape: Shape::List,
            only_kinds: vec![],
            only_status: vec![Status::Blocked],
            only_project: None,
            only_client: None,
            as_of: None,
            grouped_by: Group::Project,
            sort: Sort::Oldest,
        },
        View {
            name: "Content".into(),
            shape: Shape::Board,
            only_kinds: vec![Kind::Draft],
            only_status: vec![],
            only_project: None,
            only_client: None,
            as_of: None,
            grouped_by: Group::Status,
            sort: Sort::Pressing,
        },
        View {
            name: "Ideas".into(),
            shape: Shape::List,
            only_kinds: vec![Kind::Idea],
            only_status: vec![],
            only_project: None,
            only_client: None,
            as_of: None,
            grouped_by: Group::Nothing,
            sort: Sort::Newest,
        },
    ]
}

/// Apply a view to everything.
pub fn apply<'a>(items: &'a [Item], view: &View, now: u64) -> Vec<&'a Item> {
    let mut out: Vec<&Item> = items
        .iter()
        .filter(|i| view.only_kinds.is_empty() || view.only_kinds.contains(&i.kind))
        .filter(|i| view.only_status.is_empty() || view.only_status.contains(&i.status))
        .filter(|i| match &view.only_project {
            None => true,
            Some(p) => i.project.as_deref() == Some(p.as_str()),
        })
        .filter(|i| match &view.only_client {
            None => true,
            Some(c) => i.client.as_deref() == Some(c.as_str()),
        })
        // Looking at a past day means the state as it was then, not today's
        // state filtered by date — those are different questions and only the
        // first one is useful.
        .filter(|i| match view.as_of {
            None => true,
            Some(day) => {
                // Your day, on your clock.
                let start = crate::localclock::midnight_here(day);
                let end = crate::localclock::next_midnight_here(day);
                i.at < end && i.closed_at.map(|c| c >= start).unwrap_or(true)
            }
        })
        .collect();

    match view.sort {
        Sort::Pressing => out.sort_by_key(|i| (pressure(i, now), i.at)),
        Sort::Newest => out.sort_by_key(|i| std::cmp::Reverse(i.at)),
        Sort::Oldest => out.sort_by_key(|i| i.at),
    }
    out
}

/// Lower is more pressing. Used for sorting, never shown.
fn pressure(i: &Item, now: u64) -> i64 {
    // Something waiting on you outranks a date, because a date you can't act
    // on is not urgent, it's just soon.
    let base: i64 = match i.status {
        Status::NeedsYou => 0,
        Status::Doing => 100,
        Status::Blocked => 200,
        Status::Waiting => 300,
        _ => 900,
    };
    match i.due {
        Some(d) if d <= now => base - 50,
        Some(d) => base + ((d - now) / 86_400).min(60) as i64,
        None => base + 30,
    }
}

/// Grouped, for a board.
pub fn grouped<'a>(items: &[&'a Item], by: Group) -> Vec<(String, Vec<&'a Item>)> {
    let mut groups: Vec<(String, Vec<&Item>)> = Vec::new();
    for i in items {
        let key = match by {
            Group::Nothing => String::new(),
            Group::Status => format!("{:?}", i.status),
            Group::Project => i.project.clone().unwrap_or_else(|| "No project".into()),
            Group::Kind => format!("{:?}", i.kind),
            Group::Day => crate::localclock::day_here(i.at).to_string(),
            Group::Client => i.client.clone().unwrap_or_else(|| "Yours".into()),
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    groups
}

/// What a day looked like, after the fact.
///
/// The question people actually ask is "what did I get done" rather than
/// "what was on the list", and those differ — the list is what you meant to
/// do.
#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    pub day: u64,
    pub finished: Vec<String>,
    pub started: Vec<String>,
    /// Was live all day and still is.
    pub carried_over: Vec<String>,
    /// What Atlas was thinking that day, across everything.
    pub thoughts: Vec<(String, Thought)>,
}

pub fn day_of(items: &[Item], day_secs: u64) -> Day {
    let start = crate::localclock::midnight_here(day_secs);
    let end = crate::localclock::next_midnight_here(day_secs);
    let mut thoughts: Vec<(String, Thought)> = Vec::new();
    for i in items {
        for t in &i.thinking {
            if t.at >= start && t.at < end {
                thoughts.push((i.title.clone(), t.clone()));
            }
        }
    }
    thoughts.sort_by_key(|(_, t)| t.at);

    Day {
        day: start,
        finished: items
            .iter()
            .filter(|i| i.closed_at.map(|c| c >= start && c < end).unwrap_or(false))
            .map(|i| i.title.clone())
            .collect(),
        started: items
            .iter()
            .filter(|i| i.at >= start && i.at < end)
            .map(|i| i.title.clone())
            .collect(),
        carried_over: items
            .iter()
            .filter(|i| i.at < start && i.status.live())
            .map(|i| i.title.clone())
            .collect(),
        thoughts,
    }
}

/// What Atlas says about a past day.
pub fn day_spoken(d: &Day) -> String {
    if d.finished.is_empty() && d.started.is_empty() {
        return "Nothing moved that day.".into();
    }
    let mut s = String::new();
    if !d.finished.is_empty() {
        s.push_str(&format!(
            "{} finished: {}.",
            d.finished.len(),
            d.finished.join(", ")
        ));
    }
    if !d.started.is_empty() {
        s.push_str(&format!(" {} started.", d.started.len()));
    }
    // The number that says whether it was a good day, which neither of the
    // other two does.
    if !d.carried_over.is_empty() {
        s.push_str(&format!(
            " {} were already open and still are.",
            d.carried_over.len()
        ));
    }
    s
}

/// Everything Atlas thought about one item, in order.
pub fn why_this_took_so_long(i: &Item) -> String {
    if i.thinking.is_empty() {
        return format!("I've got no working out for {}.", i.title);
    }
    let mut s = format!("{}:", i.title);
    for t in &i.thinking {
        s.push_str(&format!("\n  {} {}", t.kind.plain(), t.what));
    }
    s
}

/// Things Atlas could just do.
///
/// Sorted by how long they'd cost you, because the win is proportional to
/// what you'd have spent.
pub fn could_hand_over(items: &[Item]) -> Vec<&Item> {
    let mut out: Vec<&Item> = items
        .iter()
        .filter(|i| i.status.live() && i.atlas_could.safe_to_start_alone())
        .collect();
    out.sort_by_key(|i| std::cmp::Reverse(i.estimate_mins.unwrap_or(0)));
    out
}

/// What's true right now, in numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Overview {
    pub needs_you: usize,
    pub doing: usize,
    pub blocked: usize,
    pub waiting: usize,
    /// Past their date.
    pub overdue: usize,
    /// Finished in the last week.
    pub done_lately: usize,
    /// Waiting on the same thing.
    pub biggest_blocker: Option<(String, usize)>,
}

pub fn overview(items: &[Item], now: u64) -> Overview {
    let mut blockers: Vec<(String, usize)> = Vec::new();
    for i in items.iter().filter(|i| i.status == Status::Blocked) {
        if let Some(b) = &i.blocked_by {
            match blockers.iter_mut().find(|(k, _)| k == b) {
                Some((_, n)) => *n += 1,
                None => blockers.push((b.clone(), 1)),
            }
        }
    }
    blockers.sort_by_key(|(_, n)| std::cmp::Reverse(*n));

    Overview {
        needs_you: items.iter().filter(|i| i.status == Status::NeedsYou).count(),
        doing: items.iter().filter(|i| i.status == Status::Doing).count(),
        blocked: items.iter().filter(|i| i.status == Status::Blocked).count(),
        waiting: items.iter().filter(|i| i.status == Status::Waiting).count(),
        overdue: items
            .iter()
            .filter(|i| i.status.live() && i.due.map(|d| d < now).unwrap_or(false))
            .count(),
        done_lately: items
            .iter()
            .filter(|i| i.status == Status::Done && now.saturating_sub(i.at) < 7 * 86_400)
            .count(),
        biggest_blocker: blockers.into_iter().find(|(_, n)| *n > 1),
    }
}

/// What Atlas says when you ask what's outstanding.
///
/// Not the count. The count is what every app already tells you and it makes
/// a long list sound like an accusation. What's useful is the one thing to do
/// next and the one thing holding several others up.
pub fn spoken(items: &[Item], now: u64) -> String {
    let o = overview(items, now);
    let live: Vec<&Item> = items.iter().filter(|i| i.status.live()).collect();

    if live.is_empty() {
        return "Nothing outstanding.".into();
    }

    let now_view = shipped().into_iter().find(|v| v.name == "Now").unwrap();
    let first = apply(items, &now_view, now).first().copied();

    let mut s = match first {
        Some(i) => format!("First: {}.", i.title),
        None => format!("{} things, none of them waiting on you.", live.len()),
    };

    if o.overdue > 0 {
        s.push_str(&format!(" {} past their date.", o.overdue));
    }
    // The most useful single sentence in a task list: one thing is holding up
    // several others, so unsticking it is worth more than anything else.
    if let Some((what, n)) = &o.biggest_blocker {
        s.push_str(&format!(" {n} things are waiting on {what}."));
    }
    // The most useful thing Atlas can say about a task list is that some of
    // it isn't yours to do.
    let mine = could_hand_over(items);
    if !mine.is_empty() {
        let mins: u32 = mine.iter().filter_map(|i| i.estimate_mins).sum();
        s.push_str(&format!(
            " {} of these I can just do{}.",
            mine.len(),
            if mins > 0 {
                format!(" — about {mins} minutes of your time")
            } else {
                String::new()
            }
        ));
    }
    s
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    pub enabled: bool,
    /// The view you get by default.
    pub default_view: String,
    /// Hide anything finished more than this many days ago.
    pub keep_done_days: u32,
    /// Roll up items into their project on the dashboard.
    pub group_by_project: bool,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        WorkspaceConfig {
            enabled: true,
            // Not "everything" — the point is to open on what to do next.
            default_view: "Now".into(),
            keep_done_days: 14,
            group_by_project: true,
        }
    }
}

// ============ the three settings that reached nothing ============
//
// `WorkspaceConfig` had no field anywhere in `ToolsConfig` and no block in
// `tools.yaml`, so none of its three settings could be set at all — and
// `workspace_page_live` took `views.first()`, which happened to be "Now" and
// so looked like it was honouring `default_view`. That is the worst version
// of a dead setting: the shipped default and the hardcoded behaviour agreed,
// so nothing looked wrong until somebody changed the setting.

/// The view you asked for, by name.
///
/// Falls back to the first shipped view rather than to nothing — a dashboard
/// that refuses to draw because a name was mistyped is worse than one that
/// draws the usual thing. `picked_what_you_asked_for` says which happened, so
/// the page can tell you rather than quietly ignoring you.
pub fn pick(views: &[View], wanted: &str) -> Option<View> {
    let want = wanted.trim();
    if want.is_empty() {
        return views.first().cloned();
    }
    views
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case(want))
        .or_else(|| views.first())
        .cloned()
}

/// Did the name you set actually match a view?
pub fn is_a_view(views: &[View], wanted: &str) -> bool {
    views.iter().any(|v| v.name.eq_ignore_ascii_case(wanted.trim()))
}

/// Drop what finished longer ago than you keep it.
///
/// Finished items are kept rather than deleted — "what did I get done last
/// Tuesday" has no answer if they vanish — so this hides them from a view
/// without touching them. `keep_done_days: 0` hides them the moment they
/// close, which is a real answer and not the same as "keep forever".
pub fn still_worth_showing<'a>(
    items: &[&'a Item],
    keep_done_days: u32,
    now: u64,
) -> Vec<&'a Item> {
    let window = keep_done_days as u64 * 86_400;
    items
        .iter()
        .copied()
        .filter(|i| match i.closed_at {
            None => true,
            // A close time in the future is a clock that moved, not an item
            // from tomorrow. Shown rather than hidden: the failure of this
            // filter should always be that you see too much.
            Some(at) => at > now || now.saturating_sub(at) <= window,
        })
        .collect()
}

/// How to group, given what you asked for and what the view wanted.
///
/// `group_by_project` is a preference about the dashboard rather than about
/// one view, so it wins — except over a view that has already chosen to group
/// by project, where there is nothing to win.
pub fn grouping(view_wants: Group, group_by_project: bool) -> Group {
    if group_by_project {
        Group::Project
    } else {
        view_wants
    }
}
