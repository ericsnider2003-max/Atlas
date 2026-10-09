//! Read-only brief preparation. Only the daemon applies accepted effects.
use super::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::result::Result;

const LIMIT: usize = 2 * 1024 * 1024;
const RECORDS: usize = 4096;

struct Limited(Vec<u8>);
impl std::io::Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > LIMIT {
            return Err(std::io::Error::other("Brief input exceeds its memory budget"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

pub(super) fn snapshot_value<T: Serialize>(value: &T) -> Result<serde_json::Value, String> {
    let mut bytes = Limited(Vec::new());
    serde_json::to_writer(&mut bytes, value).map_err(|_| "The brief exceeds its local memory budget. Full coverage is unavailable; no complete brief was produced.".to_string())?;
    serde_json::from_slice(&bytes.0).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp { path: PathBuf, len: u64, modified: Option<std::time::SystemTime> }
fn stamp(path: &Path) -> Result<Option<Stamp>, String> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => Ok(Some(Stamp { path: path.into(), len: m.len(), modified: m.modified().ok() })),
        Ok(_) => Err("A brief record is not a regular file; nothing was changed.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("A brief record could not be checked; nothing was changed.".into()),
    }
}

fn introduction_stamp_current(root: &Path, path: &Path, expected: &Option<Stamp>) -> bool {
    if stamp(path).ok().as_ref() == Some(expected) { return true; }
    // Persist can create an empty person record or update activity and traits.
    // Those changes do not make "I don't know what you're working on" stale;
    // projects do. Other knowledge and the once-only acknowledgment stay exact.
    if path != root.join("person.json")
        || std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return false;
    }
    Reader::default().record::<crate::person::Person>(root, "person")
        .is_ok_and(|person| person.projects.is_empty())
}

#[derive(Default)]
struct Reader { versions: Vec<(PathBuf, Option<Stamp>)> }
impl Reader {
    fn text(&mut self, path: &Path) -> Result<Option<String>, String> {
        let before = stamp(path)?;
        self.versions.push((path.into(), before.clone()));
        let Some(metadata) = before.as_ref() else { return Ok(None) };
        if metadata.len > LIMIT as u64 { return Err("A brief record exceeds the read budget; the file was left untouched.".into()); }
        // Read at most LIMIT+1 even if a file grows after its metadata check.
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path).map_err(|_| "A brief record could not be opened; the file was left untouched.")?
            .take((LIMIT + 1) as u64).read_to_end(&mut bytes).map_err(|_| "A brief record could not be read; the file was left untouched.")?;
        if bytes.len() > LIMIT || stamp(path)? != before { return Err("Local records changed while the brief was being read; please ask again.".into()); }
        String::from_utf8(bytes).map(Some).map_err(|_| "A brief record is not readable text; the file was left untouched.".into())
    }
    fn record<T: serde::de::DeserializeOwned + Default>(&mut self, root: &Path, key: &str) -> Result<T, String> {
        let Some(text) = self.text(&root.join(format!("{key}.json")))? else { return Ok(T::default()) };
        let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| format!("The {key} record could not be read; its file was left untouched."))?;
        let data = if let Some(schema) = value.get("schema") {
            if schema.as_u64() != Some(u64::from(crate::store::SCHEMA)) { return Err(format!("The {key} record uses an unsupported format; its file was left untouched.")); }
            value.get("data").cloned().ok_or_else(|| format!("The {key} record has no data; its file was left untouched."))?
        } else { value };
        serde_json::from_value(data).map_err(|_| format!("The {key} record could not be read; its file was left untouched."))
    }
}

#[derive(Serialize, Deserialize)]
struct Memory {
    backlog: crate::backlog::Backlog,
    scheduler: crate::scheduler::Scheduler,
    proposals: Vec<crate::booking::Proposal>,
    publisher: crate::publish::Publisher,
    notebook: crate::capture::Notebook,
    day: serde_json::Value,
    hunt: Option<crate::hunt::HuntState>,
    upkeep: Vec<String>,
    push: Option<String>,
    quiet_login: Option<String>,
    no_interests: bool,
    knows_you: bool,
    noticed: Option<String>,
    social_watch: Option<crate::social::watchlist::Watch>,
}

struct Input {
    root: PathBuf,
    owner: PathBuf,
    memory: serde_json::Value,
    configuration: String,
    cfg: crate::brief::BriefConfig,
    workday: crate::workday::WorkdayConfig,
    hunt: crate::hunt::HuntConfig,
    zone: crate::tz::Zone,
    online: bool,
    now: u64,
    since: u64,
    epoch: u64,
    automatic: bool,
    daypart: Option<crate::nudge::Part>,
    refreshes: u8,
    refresh_until: std::time::Instant,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Effects {
    later: Option<(String, u64, Vec<crate::later::Item>)>,
    ideas: Option<(String, Vec<(u64, u64, String)>)>,
    quiet_login: Option<String>,
    introduction: Option<String>,
    applications: Vec<(String, crate::applied::Application)>,
    asked: Option<String>,
    offers: Vec<(String, String)>,
    noticed: Option<(String, u64)>,
}

struct Prepared {
    brief: crate::brief::Brief,
    effects: Effects,
    input: Input,
    versions: Vec<(PathBuf, Option<Stamp>)>,
    hunt_base: Option<crate::hunt::HuntState>,
    announcement: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct DeliveryPlan {
    root: PathBuf,
    configuration: String,
    owner_versions: Vec<(PathBuf, Option<Stamp>)>,
    pending: Option<(String, u64)>,
    effects: Effects,
    last_source_at: u64,
    last_delivered_at: u64,
    #[serde(default)]
    automatic_offer_at: u64,
    #[serde(default)]
    introduction_versions: Vec<(PathBuf, Option<Stamp>)>,
    #[serde(default)]
    daypart: Option<crate::nudge::Part>,
}

// Existing plans store the full five-part preparation signature. Project
// only validated settings; reachability describes a moment, not permission.
fn brief_settings_signature(configuration: &str) -> Result<Vec<serde_json::Value>, String> {
    if configuration.len() > LIMIT { return Err("Saved brief settings exceed the validation budget.".into()); }
    let mut values: Vec<serde_json::Value> = serde_json::from_str(configuration).map_err(|_| "Saved brief settings could not be read.".to_string())?;
    if values.len() != 5 || !values[..3].iter().all(serde_json::Value::is_object)
        || !values[3].as_str().is_some_and(|zone| !zone.is_empty())
        || !matches!(values[4].as_str(), Some("unknown" | "offline" | "online")) {
        return Err("Saved brief settings use an unknown format; its pending delivery was canceled.".into());
    }
    values.truncate(4);
    Ok(values)
}

#[derive(Default)]
pub(super) struct Live {
    receiver: Option<Receiver<Result<Prepared, String>>>,
    ready: Option<Prepared>,
    effects: Option<Effects>,
    epoch: u64,
    plan: Option<DeliveryPlan>,
    notice: Option<String>,
    automatic_retry_after: Option<std::time::Instant>,
    automatic_canceled_day: Option<u64>,
    daypart: Option<crate::nudge::Part>,
    announcement: Option<String>,
}

fn item(id: String, from: &str, subject: String, weight: crate::brief::Weight) -> crate::brief::Item {
    crate::brief::Item { id, source: crate::brief::Source::Day, from: from.into(), subject, weight,
        outcome: crate::brief::Outcome::Yours, draft: None, conflicts_with: None }
}

fn build(input: Input) -> Result<Prepared, String> {
    let mut memory: Memory = serde_json::from_value(input.memory.clone()).map_err(|e| e.to_string())?;
    let mut read = Reader::default();
    let root = &input.root;
    let now = input.now;
    let handover: crate::handover::Handover = read.record(&input.owner, "handover")?;
    let profiles: crate::profiles::Profiles = read.record(&input.owner, "profiles")?;
    if handover.stance.handed_over() || profiles.active_state_dir(&input.owner).unwrap_or_else(|| input.owner.clone()) != *root {
        return Err("The active person changed; the brief was canceled.".into());
    }
    let inbox: crate::household::Inbox = read.record(root, "handoffs")?;
    let mut later: crate::later::Later = read.record(root, crate::later::RECORD)?;
    let named: Vec<u64> = read.record(root, "ideas_named")?;
    let quiet: u64 = read.record(root, "quiet_logins_said")?;
    let apps: crate::applied::Applications = read.record(root, crate::applied::FILE)?;
    if apps.all.len() > RECORDS {
        return Err("The application history exceeds this brief's record budget. Application coverage is unavailable; no full brief was produced.".into());
    }
    let mut upkeep = std::mem::take(&mut memory.upkeep);
    let mut effects = Effects::default();
    let mentioned = later.mentioned;
    if let Some(line) = later.weekly_line(now) { effects.later = Some((line.clone(), mentioned, later.items.clone())); upkeep.push(line); }
    let ideas = crate::capture::ideas_to_name(&memory.notebook, now, &named);
    if let Some(line) = crate::capture::ideas_line(&ideas, now) {
        effects.ideas = Some((line.clone(), ideas.iter().map(|n| (n.id, n.at, n.text.clone())).collect())); upkeep.push(line);
    }
    if now.saturating_sub(quiet) >= crate::signin::QUIET_EVERY_DAYS * 86_400 {
        if let Some(line) = memory.quiet_login.take() { effects.quiet_login = Some(line.clone()); upkeep.push(line); }
    }
    let sources = crate::brief::Sources { inbox: &inbox, backlog: &memory.backlog, scheduler: &memory.scheduler,
        proposals: &memory.proposals, publisher: &memory.publisher, upkeep: &upkeep, mail: &[], online: input.online };
    let mut brief = crate::brief::from_here(&sources, &input.cfg, now, input.since);
    let (day, hunt_base) = day_items(&input, &mut memory, &mut read, &apps, &mut effects)?;
    if brief.start_with.is_none() { brief.start_with = day.iter().find(|i| i.weight == crate::brief::Weight::Urgent).map(|i| i.subject.clone()); }
    brief.yours.extend(day);
    if input.cfg.enabled { brief.push = memory.push; }
    if input.automatic {
        let today = crate::localclock::midnight(now, crate::localclock::offset_secs());
        let noticed_on: u64 = read.record(root, "noticed_on")?;
        if noticed_on != today {
            if let Some(line) = memory.noticed { effects.noticed = Some((line.clone(), today)); brief.noticed = Some(line); }
        }
    }
    // Hidden/below-floor opportunities were prepared but not offered.
    effects.offers.retain(|(id, _)| brief.yours.iter().any(|i| i.id == format!("opportunity:{id}")));
    let announcement = if let Some(part) = input.daypart {
        if !brief.is_empty() {
            Some(crate::nudge::daypart_with_brief(part, &brief).message)
        } else {
            let offered: bool = read.record(root, "offered_get_to_know")?;
            let person: crate::person::Person = read.record(root, "person")?;
            let legacy: crate::facts::Book = read.record(root, "facts")?;
            let mut knows_you = memory.knows_you || !person.projects.is_empty()
                || legacy.get("push them on").is_some() || !legacy.of_kind(crate::facts::Kind::Project).is_empty();
            let mut count = legacy.facts.len();
            if count > RECORDS { return Err("The saved knowledge exceeds the greeting's record budget; no introduction was offered.".into()); }
            for shard in 0..16 {
                let facts: Vec<crate::facts::Fact> = read.record(root, &format!("facts-{shard}"))?;
                count = count.saturating_add(facts.len());
                if count > RECORDS { return Err("The saved knowledge exceeds the greeting's record budget; no introduction was offered.".into()); }
                knows_you |= facts.iter().any(|fact| fact.name == crate::facts::slug("push them on") || fact.kind == crate::facts::Kind::Project);
            }
            let line = crate::returning::empty_hello(part.greeting(), knows_you, offered);
            effects.introduction = line.clone();
            line
        }
    } else { None };
    Ok(Prepared { brief, effects, input, versions: read.versions, hunt_base, announcement })
}

// The historical workday collector now runs only on the immutable brief worker.
fn day_items(input: &Input, memory: &mut Memory, read: &mut Reader, apps: &crate::applied::Applications, effects: &mut Effects) -> Result<(Vec<crate::brief::Item>, Option<crate::hunt::HuntState>), String> {
    let root = &input.root;
    let now = input.now;
    let (people, habits, deck, journal, taught): (Option<crate::people::People>, Option<crate::habits::Habits>, Option<crate::srs::Deck>, Option<crate::tradeday::Journal>, Option<crate::waitingfor::Taught>) =
        serde_json::from_value(std::mem::take(&mut memory.day)).map_err(|e| e.to_string())?;
    let people = match people { Some(v) => v, None => read.record(root, "people")? };
    let habits = match habits { Some(v) => v, None => read.record(root, "habits")? };
    let deck = match deck { Some(v) => v, None => read.record(root, "cards")? };
    let journal = match journal { Some(v) => v, None => read.record(root, "trade_journal")? };
    let taught = match taught { Some(v) => v, None => read.record(root, "waiting_taught")? };
    let mail: crate::mailbook::MailBook = read.record(root, crate::mailbook::MailBook::FILE)?;
    let local = input.zone.to_local(now as i64).max(0) as u64;
    let today = (local / 86_400) as i64;
    let mut day = Vec::new();
    if input.workday.market_in_brief || !journal.entries.is_empty() {
        for line in crate::marketdays::today_and_tomorrow(now as i64, &input.zone) { day.push(item(format!("market:{line}"), "Markets", line, crate::brief::Weight::Info)); }
    }
    if input.workday.waiting_for.enabled {
        let waiting = crate::waitingfor::open(&mail, &taught, &input.workday.waiting_for, now, local as i64 - now as i64);
        for w in crate::waitingfor::due_now(&waiting, now).into_iter().take(5) {
            let (from, line, weight) = match w.side {
                crate::waitingfor::Side::Promised => ("You promised", format!("\"{}\" -- {}", w.subject, w.said), crate::brief::Weight::Urgent),
                crate::waitingfor::Side::Owed => ("Waiting on", format!("{} -- \"{}\"", w.with, w.subject), crate::brief::Weight::Info),
            };
            day.push(item(format!("waiting:{}", w.letter), from, line, weight));
        }
    }
    let day0 = local - local % 86_400;
    for n in memory.notebook.due_between(day0, day0 + 86_400) {
        day.push(item(format!("note:{}", n.id), "Your note", n.text.chars().take(90).collect(), crate::brief::Weight::Info));
    }
    for (name, days, every) in people.due(&mail, now).into_iter().take(3) {
        let line = days.map(|d| format!("{name} -- {d} days (every {every})")).unwrap_or_else(|| format!("{name} -- no contact on record"));
        day.push(item(format!("person:{name}"), "Catch up with", line, crate::brief::Weight::Info));
    }
    for (name, away) in people.birthdays(local, 1) { day.push(item(format!("birthday:{name}"), "Birthday", if away == 0 { format!("{name}, today") } else { format!("{name}, tomorrow") }, crate::brief::Weight::Info)); }
    let due: Vec<String> = habits.due_today(today).iter().map(|h| h.name.clone()).collect();
    if !due.is_empty() { day.push(item("habits".into(), "Habits", due.join(", "), crate::brief::Weight::Info)); }
    let cards = deck.due(today, usize::MAX).len();
    if cards > 0 { day.push(item("cards".into(), "Cards", format!("{cards} cards due"), crate::brief::Weight::Info)); }
    if input.workday.social.enabled && input.workday.social.in_brief {
        if let Some(text) = read.text(&root.join(crate::social::snapshots::FILE))? {
            let book = crate::social::snapshots::Book::parse_brief(&text)?;
            if let Some(line) = crate::social::analysis::own_brief_line(&book, now) { day.push(item("social:own".into(), "Social", line, crate::brief::Weight::Info)); }
        }
        let watch: crate::social::watchlist::Watch = match memory.social_watch.take() { Some(w) => w, None => read.record(root, crate::social::watchlist::FILE)? };
        if watch.list.iter().any(|w| w.last_ok.is_some_and(|ok| now.saturating_sub(ok) < 36 * 3600)) {
            if let Some(line) = crate::social::analysis::brief_digest(&watch, now) { day.push(item("social:watch".into(), "Social", line, crate::brief::Weight::Info)); }
        }
    }
    for a in apps.all.iter().filter(|a| a.stage == crate::applied::Stage::Applied && !a.nudged && now.saturating_sub(a.moved_at) >= crate::applied::FOLLOW_UP_AFTER_SECS) {
        let line = format!("No word from {} in a week since you applied. A short follow-up note is normal now.", a.to);
        effects.applications.push((line.clone(), a.clone()));
        day.push(item(format!("application:{}:{}", a.to, a.applied_at), "Application", line, crate::brief::Weight::Info));
    }
    let mut hunt_base = None;
    if input.hunt.enabled {
        let mut hunt = match memory.hunt.take() { Some(v) => v, None => read.record(root, crate::hunt::FILE)? };
        hunt_base = Some(hunt.clone());
        if memory.no_interests && !hunt.asked { effects.asked = Some(crate::hunt::ASK.into()); day.push(item("opportunity:ask".into(), "Opportunity", crate::hunt::ASK.into(), crate::brief::Weight::Info)); }
        let mut offers = hunt.take_unbriefed(input.hunt.top_n.max(1) as usize, now);
        for pending in hunt.pending_offers(now) {
            if !offers.iter().any(|r| r.found.id == pending.found.id) { offers.push(pending); }
        }
        for (i, ranked) in offers.iter().enumerate() {
            let line = crate::hunt::line(i + 1, ranked);
            effects.offers.push((ranked.found.id.clone(), line.clone()));
            day.push(item(format!("opportunity:{}", ranked.found.id), "Opportunity", line, crate::brief::Weight::Info));
        }
    }
    Ok((day, hunt_base))
}

#[derive(Serialize)]
struct MemoryView<'b> {
    backlog: &'b crate::backlog::Backlog,
    scheduler: &'b crate::scheduler::Scheduler,
    proposals: &'b [crate::booking::Proposal],
    publisher: &'b crate::publish::Publisher,
    notebook: &'b crate::capture::Notebook,
    day: serde_json::Value,
    hunt: &'b Option<crate::hunt::HuntState>,
    upkeep: Vec<String>,
    push: Option<String>,
    quiet_login: Option<String>,
    no_interests: bool,
    knows_you: bool,
    noticed: Option<String>,
    social_watch: &'b Option<crate::social::watchlist::Watch>,
}

impl Daemon<'_> {
    pub(super) fn automatic_brief_available(&self, now: u64) -> bool {
        self.brief_preparation.automatic_canceled_day != Some(crate::localclock::day_here(now) as u64)
            && self.brief_preparation.automatic_retry_after.is_none_or(|until| std::time::Instant::now() >= until)
    }

    pub(super) fn automatic_brief_failed(&mut self) {
        self.last_brief_attempt_at = self.last_brief_at;
        self.brief_preparation.automatic_retry_after = Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
    }

    pub(super) fn explicitly_cancel_brief_preparation(&mut self, now: u64) -> bool {
        let queued_introduction = self.brief_preparation.plan.as_ref().is_some_and(|plan| plan.pending.is_some() && plan.effects.introduction.is_some());
        let pending = self.brief_preparation.receiver.is_some() || self.brief_preparation.ready.is_some() || queued_introduction;
        if pending {
            self.cancel_brief_preparation();
            self.brief_preparation.automatic_canceled_day = Some(crate::localclock::day_here(now) as u64);
            if queued_introduction {
                self.morning_brief = None;
                self.brief_preparation.announcement = None;
                self.pending_brief_delivery = None;
                self.brief_preparation.effects = None;
                self.brief_preparation.plan = None;
                if let Err(error) = self.store.save("brief_delivery_plan", &None::<DeliveryPlan>) {
                    self.brief_preparation.notice = Some(format!("The introduction was stopped for this session, but its cancellation could not be saved. Its restart status is unconfirmed: {error}"));
                }
            }
        }
        pending
    }
    /// An unsolicited brief waits behind requested work and its result.
    /// Explicit brief requests keep their separate response path in tick.
    pub(super) fn offer_ready_brief(&mut self, out: &mut Vec<String>) {
        if !self.ensure_brief_plan_current() { return; }
        if self.attention.is_paused() || self.brief_preparation.notice.is_some() { return; }
        if !out.is_empty() || self.task_loop.is_some() || self.pending_turn.is_some()
            || self.operating.is_some() || self.active_file_move_id().is_some()
            || self.queue.tasks.iter().any(|task| task.state == crate::lanes::TaskState::Running)
            || self.scheduler.jobs.iter().any(|job| job.in_flight)
        { return; }
        if let Some(brief) = self.morning_brief.take() { out.push(brief); }
    }
    fn brief_memory(&self, now: u64) -> Result<serde_json::Value, String> {
        if self.backlog.items.len() > RECORDS || self.scheduler.jobs.len() > RECORDS || self.proposals.len() > RECORDS
            || self.publisher.posts.len() > RECORDS || self.notebook.notes.len() > RECORDS {
            return Err("The brief exceeds its local record budget. Full coverage is unavailable; no complete brief was produced.".into());
        }
        snapshot_value(&MemoryView { backlog: &self.backlog, scheduler: &self.scheduler, proposals: &self.proposals,
            publisher: &self.publisher, notebook: &self.notebook, day: self.workday.brief_records()?, hunt: self.workday.hunt.brief_state(),
            upkeep: self.upkeep_questions(now),
            push: self.facts.get("push them on").and_then(|f| crate::brief::push_for_day(&f.summary, crate::localclock::day_here(now) as u64)),
            quiet_login: self.access.quiet_line(now), knows_you: self.brief_knows_you(), no_interests: crate::hunt::Interests::from_facts(&self.facts).is_empty(),
            noticed: crate::daily::one_thing_noticed(&self.noticed_days(now)), social_watch: &self.workday.social.watch })
    }

    fn brief_knows_you(&self) -> bool {
        self.facts.get("push them on").is_some()
            || !self.facts.of_kind(crate::facts::Kind::Project).is_empty()
            || !self.person.projects.is_empty()
    }

    fn brief_configuration(&self) -> Result<String, String> {
        serde_json::to_string(&(&self.tools_cfg().brief, self.workday_cfg(), &self.tools_cfg().hunt, self.home_zone().id(), match self.connectivity.cached() { crate::connectivity::Reach::Online => "online", crate::connectivity::Reach::Offline => "offline", crate::connectivity::Reach::Unknown => "unknown" })).map_err(|error| format!("Brief configuration could not be captured: {error}"))
    }

    pub(super) fn owner_state_root(&self) -> PathBuf {
        let install = crate::roots::state_dir();
        if self.store.root().starts_with(&install) { return install; }
        // Custom profile stores use the same owner/profiles/<id> layout as an install.
        if let Some(profiles)=self.store.root().parent().filter(|p|p.file_name().is_some_and(|n|n=="profiles")) {
            if let Some(owner)=profiles.parent() {return owner.into();}
        }
        self.store.root().into()
    }

    fn brief_input(&self, now: u64, owner: PathBuf) -> Result<Input, String> {
        let configuration = self.brief_configuration()?;
        if configuration.len() > LIMIT { return Err("The brief settings exceed the preparation budget.".into()); }
        Ok(Input { root: self.store.root().into(), owner, memory: self.brief_memory(now)?, configuration,
            cfg: self.tools_cfg().brief.clone(), workday: self.workday_cfg(), hunt: self.tools_cfg().hunt.clone(), zone: self.home_zone(),
            online: self.connectivity.cached() == crate::connectivity::Reach::Online, now, since: self.last_brief, epoch: self.brief_preparation.epoch,
            automatic: !self.brief_requested_explicitly && !self.brief_requested_panel, daypart: self.brief_preparation.daypart, refreshes: 0, refresh_until: std::time::Instant::now() + std::time::Duration::from_secs(5) })
    }

    fn brief_staleness(&self, ready: &Prepared) -> Result<Option<(bool, String)>, String> {
        if self.attention.is_paused() || self.brief_preparation.epoch != ready.input.epoch || self.store.root() != ready.input.root || self.last_brief != ready.input.since {
            return Ok(Some((false, "The brief was canceled or its delivery interval changed.".into())));
        }
        let changed: Vec<_> = ready.versions.iter().filter(|(path, expected)| stamp(path).ok().as_ref() != Some(expected)).map(|(path, _)| path).collect();
        if changed.iter().any(|path| path.parent() == Some(ready.input.owner.as_path()) && matches!(path.file_name().and_then(|name| name.to_str()), Some("profiles.json" | "handover.json"))) {
            return Ok(Some((false, "The active person changed; the brief was canceled.".into())));
        }
        let configuration = self.brief_configuration()?;
        if configuration != ready.input.configuration {
            if brief_settings_signature(&configuration)? != brief_settings_signature(&ready.input.configuration)? {
                return Ok(Some((false, "Brief settings changed; the prepared brief was canceled.".into())));
            }
            return Ok(Some((true, "Brief refresh: background connectivity changed.".into())));
        }
        let current = self.brief_memory(ready.input.now)?;
        if current != ready.input.memory {
            let names: Vec<_> = current.as_object().into_iter().flat_map(|map| map.keys()).filter(|key| current.get(*key) != ready.input.memory.get(*key)).cloned().collect();
            return Ok(Some((true, format!("Brief refresh: local domains changed: {}.", names.join(", ")))));
        }
        if !changed.is_empty() {
            let names: Vec<_> = changed.iter().filter_map(|path| path.file_stem().and_then(|name| name.to_str())).collect();
            return Ok(Some((true, format!("Brief refresh: local records changed: {}.", names.join(", ")))));
        }
        Ok(None)
    }

    fn accept_brief(&mut self, ready: Prepared) -> Result<crate::brief::Brief, String> {
        if let Some((_, reason)) = self.brief_staleness(&ready)? {
            return Err(format!("{reason} Ask again for a current brief; nothing was marked delivered."));
        }
        let pending = ready.announcement.clone().or_else(|| (!ready.brief.is_empty()).then(|| crate::brief::spoken(&ready.brief))).map(|line| (line, ready.input.now));
        let plan = DeliveryPlan {
            root: ready.input.root.clone(), configuration: ready.input.configuration.clone(),
            owner_versions: ready.versions.iter().filter(|(p, _)| p.parent() == Some(ready.input.owner.as_path())
                && matches!(p.file_name().and_then(|s| s.to_str()), Some("profiles.json" | "handover.json"))).cloned().collect(),
            daypart: ready.input.daypart,
            introduction_versions: if ready.effects.introduction.is_some() { ready.versions.iter().filter(|(path, _)| {
                let name = path.file_stem().and_then(|name| name.to_str()).unwrap_or("");
                matches!(name, "person" | "facts" | "offered_get_to_know") || name.starts_with("facts-")
            }).cloned().collect() } else { Vec::new() },
            pending: pending.clone(), effects: ready.effects.clone(), last_source_at: self.last_brief, last_delivered_at: self.last_brief_at,
            automatic_offer_at: if ready.input.automatic { ready.input.now } else { self.last_brief_attempt_at },
        };
        snapshot_value(&plan)?;
        let receipt_store = self.store.clone();
        let _transaction = receipt_store.transaction().map_err(|e| format!("The brief plan is waiting for local storage; nothing was marked delivered: {e}"))?;
        // Mirrors are staged first. Only the final atomic record authorizes
        // restart consumption; a partial pair can never revive a failed plan.
        self.store.save("brief_effects", &Some(&ready.effects)).map_err(|e| format!("The brief could not save its reminder plan; nothing was marked delivered: {e}"))?;
        self.store.save("brief_prepared", &pending).map_err(|e| format!("The brief could not save its prepared text; nothing was marked delivered: {e}"))?;
        crate::hunting::install_brief_offers(self, ready.hunt_base, &ready.effects.offers, ready.input.now)?;
        if ready.input.automatic {
            self.store.save("last_brief_attempt_at", &ready.input.now).map_err(|e| format!("The prepared brief could not checkpoint its daily offer; nothing was marked delivered: {e}"))?;
        }
        self.store.save("brief_delivery_plan", &Some(&plan)).map_err(|e| format!("The brief's delivery plan could not be committed; nothing was marked delivered: {e}"))?;
        if ready.input.automatic { self.last_brief_attempt_at = ready.input.now; }

        self.brief_preparation.effects = Some(ready.effects);
        self.brief_preparation.plan = Some(plan);
        self.pending_brief_delivery = pending;
        self.brief_preparation.announcement = ready.announcement;
        self.brief_preparation.daypart = None;
        Ok(ready.brief)
    }

    /// Start one bounded read-only preparation. The caller must keep polling;
    /// waiting here would put slow disk access back on the control loop.
    pub(super) fn request_daypart_brief(&mut self, now: u64, part: crate::nudge::Part) -> Result<(), String> {
        if self.brief_preparation.receiver.is_some() || self.brief_preparation.ready.is_some() { return Ok(()); }
        self.brief_preparation.daypart = Some(part);
        self.start_brief(now)
    }

    pub(super) fn take_brief_announcement(&mut self) -> Option<String> {
        self.brief_preparation.announcement.take()
    }

    pub(crate) fn request_brief(&mut self, now: u64) -> Result<(), String> {
        if self.brief_preparation.receiver.is_some() || self.brief_preparation.ready.is_some() { return Ok(()); }
        self.brief_preparation.daypart = None;
        self.start_brief(now)
    }

    fn start_brief(&mut self, now: u64) -> Result<(), String> {
        if !self.ensure_brief_plan_current() {
            return Err("The previous brief is waiting for local storage. Nothing was marked delivered.".into());
        }
        if let Some(why) = self.brief_preparation.notice.take() { return Err(why); }
        if self.brief_preparation.receiver.is_some() || self.brief_preparation.ready.is_some() { return Ok(()) }
        let owner = self.owner_state_root();
        let input = self.brief_input(now, owner)?;
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::Builder::new().name("atlas-brief".into()).spawn(move || { let _ = tx.send(build(input)); })
            .map_err(|e| format!("The brief worker could not start: {e}"))?;
        self.brief_preparation.receiver = Some(rx);
        Ok(())
    }

    pub(super) fn cancel_brief_preparation(&mut self) {
        if self.brief_preparation.receiver.is_some() || self.brief_preparation.ready.is_some() { self.brief_preparation.epoch = self.brief_preparation.epoch.wrapping_add(1); }
    }

    pub fn poll_brief(&mut self) -> Option<Result<crate::brief::Brief, String>> {
        if !self.ensure_brief_plan_current() { return None; }
        if let Some(why) = self.brief_preparation.notice.take() { return Some(Err(why)) }
        if self.attention.is_paused() { self.cancel_brief_preparation(); }
        if self.brief_preparation.ready.is_none() {
            let receiver = self.brief_preparation.receiver.as_ref()?;
            match receiver.try_recv() {
                Ok(result) => {
                    self.brief_preparation.receiver = None;
                    match result { Ok(prepared) => self.brief_preparation.ready = Some(prepared), Err(why) => return Some(Err(why)) }
                }
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.brief_preparation.receiver = None;
                    return Some(Err("Brief preparation stopped before it produced a result. Nothing was marked delivered.".into()));
                }
            }
        }
        // Hold one bounded result across a snapshot's temporary ownership.
        // The control loop never waits, and no effects or receipt are staged.
        let store = self.store.clone();
        let _transaction = match store.transaction() {
            Ok(guard) => guard,
            Err(crate::error::AtlasError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => return None,
            Err(error) => {
                self.brief_preparation.ready = None;
                return Some(Err(format!("The brief could not access local storage; nothing was marked delivered: {error}")));
            }
        };
        let prepared = self.brief_preparation.ready.take()?;
        match self.brief_staleness(&prepared) {
            Ok(Some((true, reason))) if prepared.input.refreshes < 3 && std::time::Instant::now() < prepared.input.refresh_until => {
                self.log.info(&reason);
                let mut input = match self.brief_input(prepared.input.now, prepared.input.owner.clone()) { Ok(input) => input, Err(why) => return Some(Err(why)) };
                input.daypart = prepared.input.daypart;
                input.refreshes = prepared.input.refreshes + 1;
                input.refresh_until = prepared.input.refresh_until;
                let (send, receive) = mpsc::sync_channel(1);
                if let Err(error) = std::thread::Builder::new().name("atlas-brief-refresh".into()).spawn(move || { let _ = send.send(build(input)); }) {
                    return Some(Err(format!("The fresh brief could not start: {error}. Nothing was marked delivered.")));
                }
                self.brief_preparation.receiver = Some(receive);
                return None;
            }
            Err(why) => return Some(Err(why)),
            _ => {}
        }
        // Re-check owner, settings, records, and cancellation on every retry.
        Some(self.accept_brief(prepared))
    }

    pub(super) fn prepare_brief_inline(&mut self, now: u64) -> Result<crate::brief::Brief, String> {
        let input = self.brief_input(now, self.owner_state_root())?;
        self.accept_brief(build(input)?)
    }

    pub(super) fn acknowledge_prepared_effects(&mut self, delivered: &str, now: u64) -> bool {
        let receipt_store = self.store.clone();
        let _transaction = match receipt_store.transaction() {
            Ok(guard) => guard,
            Err(why) => { self.log.warn(&format!("Brief reminder receipts remain pending while local storage is unavailable: {why}")); return false; }
        };
        if !self.ensure_brief_plan_current() { return false; }
        let Some(mut effects) = self.brief_preparation.effects.take() else { return self.brief_preparation.notice.is_none() };
        let compact = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        let delivered = compact(delivered);
        let contains = |text: &str| delivered.contains(&compact(text));
        // Apply to current records by stable IDs/preconditions. No worker
        // record is ever saved over the daemon's newer state.
        let mut reader = Reader::default();
        if effects.later.as_ref().is_some_and(|(line, _, _)| contains(line)) {
            if let Some((_, expected, expected_items)) = effects.later.as_ref() {
                match reader.record::<crate::later::Later>(self.store.root(), crate::later::RECORD) {
                    Ok(mut later) if later.mentioned == *expected && later.items == *expected_items => {
                        later.mentioned = now;
                        match self.store.save(crate::later::RECORD, &later) { Ok(()) => effects.later = None, Err(e) => self.log.warn(&format!("The delivered later-list reminder could not be saved: {e}")) }
                    }
                    Ok(_) => effects.later = None,
                    Err(e) => self.log.warn(&e),
                }
            }
        }
        if effects.ideas.as_ref().is_some_and(|(line, _)| contains(line)) {
            match reader.record::<Vec<u64>>(self.store.root(), "ideas_named") {
                Ok(mut named) => {
                    if let Some((_, ids)) = &effects.ideas {
                        for (id, at, text) in ids {
                            if self.notebook.notes.iter().any(|n| n.id == *id && n.at == *at && &n.text == text) && !named.contains(id) { named.push(*id); }
                        }
                    }
                    match self.store.save("ideas_named", &named) { Ok(()) => effects.ideas = None, Err(e) => self.log.warn(&format!("The delivered idea reminder could not be saved: {e}")) }
                }
                Err(e) => self.log.warn(&e),
            }
        }
        if effects.introduction.as_ref().is_some_and(|line| contains(line)) {
            if self.brief_knows_you() { effects.introduction = None; }
            else {
                match self.store.save("offered_get_to_know", &true) {
                    Ok(()) => effects.introduction = None,
                    Err(error) => self.log.warn(&format!("The delivered introduction remains pending: {error}")),
                }
            }
        }
        if effects.quiet_login.as_ref().is_some_and(|line| contains(line)) {
            if self.access.quiet_line(now) == effects.quiet_login {
                match self.store.save("quiet_logins_said", &now) { Ok(()) => effects.quiet_login = None, Err(e) => self.log.warn(&format!("The delivered login reminder could not be saved: {e}")) }
            } else { effects.quiet_login = None; }
        }
        if effects.applications.iter().any(|(line, _)| contains(line)) {
            match reader.record::<crate::applied::Applications>(self.store.root(), crate::applied::FILE) {
                Ok(mut apps) => {
                    let mut acknowledged = Vec::new();
                    for (i, (line, expected)) in effects.applications.iter().enumerate() {
                        if !contains(line) { continue; }
                        if let Some(current) = apps.all.iter_mut().find(|a| a.to == expected.to && a.role == expected.role && a.applied_at == expected.applied_at) {
                            if current.stage == expected.stage && current.moved_at == expected.moved_at { current.nudged = true; }
                        }
                        acknowledged.push(i);
                    }
                    match self.store.save(crate::applied::FILE, &apps) {
                        Ok(()) => { effects.applications = effects.applications.into_iter().enumerate().filter(|(i, _)| !acknowledged.contains(i)).map(|(_, effect)| effect).collect(); }
                        Err(e) => self.log.warn(&format!("The delivered application reminder could not be saved: {e}")),
                    }
                }
                Err(e) => self.log.warn(&e),
            }
        }
        if effects.asked.as_ref().is_some_and(|line| contains(line)) {
            if crate::hunting::acknowledge_brief_question(self) { effects.asked = None; }
        }
        if effects.noticed.as_ref().is_some_and(|(line, _)| contains(line)) {
            let day = effects.noticed.as_ref().map(|(_, day)| *day).unwrap_or_default();
            match self.store.save("noticed_on", &day) { Ok(()) => effects.noticed = None, Err(e) => self.log.warn(&format!("The delivered observation could not be saved: {e}")) }
        }
        let delivered_effect_pending = effects.introduction.as_ref().is_some_and(|line| contains(line))
            || effects.later.as_ref().is_some_and(|(line, _, _)| contains(line))
            || effects.ideas.as_ref().is_some_and(|(line, _)| contains(line))
            || effects.quiet_login.as_ref().is_some_and(|line| contains(line))
            || effects.applications.iter().any(|(line, _)| contains(line))
            || effects.asked.as_ref().is_some_and(|line| contains(line))
            || effects.noticed.as_ref().is_some_and(|(line, _)| contains(line));
        self.brief_preparation.effects = Some(effects);
        let checkpoint = (|| {
            self.store.save("brief_effects", &self.brief_preparation.effects).map_err(|e| e.to_string())?;
            if let Some(mut plan) = self.brief_preparation.plan.clone() {
                plan.effects = self.brief_preparation.effects.clone().unwrap_or_default();
                if plan.effects.introduction.is_none() { plan.introduction_versions.clear(); }
                self.store.save("brief_delivery_plan", &Some(&plan)).map_err(|e| e.to_string())?;
                self.brief_preparation.plan = Some(plan);
            }
            Ok::<(), String>(())
        })();
        match checkpoint {
            Ok(()) if !delivered_effect_pending => true,
            Ok(()) => {
                let notice = "Some displayed or spoken brief reminders could not be recorded. Its full delivery receipt remains pending.".to_string();
                self.log.warn(&notice);
                self.keep_said_for_apps(vec![notice]);
                false
            }
            Err(why) => {
                let notice = format!("The brief's reminder receipts could not be saved. The delivery receipt is still pending: {why}");
                self.log.warn(&notice);
                self.keep_said_for_apps(vec![notice]);
                false
            }
        }
    }

    pub(super) fn ensure_brief_plan_current(&mut self) -> bool {
        if self.brief_preparation.plan.is_none() { return true; }
        let store = self.store.clone();
        let _transaction = match store.transaction() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        let invalid = self.brief_preparation.plan.as_ref().is_some_and(|p| p.root != self.store.root()
            || (p.effects.introduction.is_some() && (self.brief_knows_you()
                || p.introduction_versions.iter().any(|(path, expected)| !introduction_stamp_current(self.store.root(), path, expected))))
            || match (self.brief_configuration().and_then(|current| brief_settings_signature(&current)), brief_settings_signature(&p.configuration)) {
                (Ok(current), Ok(saved)) => current != saved,
                _ => true,
            }
            || p.owner_versions.iter().any(|(path, expected)| stamp(path).ok().as_ref() != Some(expected)));
        if invalid {
            self.brief_preparation.plan = None;
            self.brief_preparation.effects = None;
            self.pending_brief_delivery = None;
            self.brief_preparation.announcement = None;
            self.morning_brief = None;
            self.brief_preparation.notice = Some("The previous brief's person or settings changed. Its pending delivery was canceled.".into());
            crate::heard!(self.store.save("brief_delivery_plan", &None::<DeliveryPlan>));
        }
        true
    }

    pub(super) fn commit_brief_receipt(&mut self, now: u64) -> bool {
        let store = self.store.clone();
        let _transaction = match store.transaction() { Ok(guard) => guard, Err(_) => return false };
        if !self.ensure_brief_plan_current() { return false; }
        if self.brief_preparation.notice.is_some() { return false; }
        let Some(mut plan) = self.brief_preparation.plan.clone() else { return true };
        let Some((_, prepared_at)) = &plan.pending else { return false };
        plan.last_source_at = *prepared_at;
        plan.last_delivered_at = now;
        plan.pending = None;
        match self.store.save("brief_delivery_plan", &Some(&plan)) {
            Ok(()) => { self.brief_preparation.plan = Some(plan); true }
            Err(why) => {
                let notice = format!("The brief's delivery receipt could not be saved. It remains pending: {why}");
                self.log.warn(&notice);
                self.keep_said_for_apps(vec![notice]);
                false
            }
        }
    }
}

impl Live {
    pub(super) fn load(store: &crate::store::Store) -> Self {
        match restored_plan(store) {
            Ok(plan) => Self { effects: plan.as_ref().map(|p| p.effects.clone()), plan, ..Self::default() },
            Err(why) => Self { notice: Some(why), ..Self::default() },
        }
    }
}

fn restored_plan(store: &crate::store::Store) -> Result<Option<DeliveryPlan>, String> {
    let mut reader = Reader::default();
    let plan: Option<DeliveryPlan> = reader.record(store.root(), "brief_delivery_plan")?;
    if let Some(p) = &plan {
        brief_settings_signature(&p.configuration)?;
        if p.root != store.root() || p.owner_versions.iter().any(|(path, expected)| stamp(path).ok().as_ref() != Some(expected)) {
            return Err("A previous brief belongs to an earlier person; its delivery plan was left untouched and canceled.".into());
        }
    }
    Ok(plan)
}

pub(super) fn restored_pending(store: &crate::store::Store) -> Option<(String, u64)> {
    if store.exists("brief_delivery_plan") || store.root().join("brief_delivery_plan.json").exists() {
        return restored_plan(store).ok().flatten().and_then(|p| p.pending);
    }
    // Old prepared-text-only records can be delivered. New staged mirrors
    // without their canonical commit are deliberately never revived.
    if store.root().join("brief_effects.json").exists() { return None }
    store.load("brief_prepared")
}

pub(super) fn restored_receipt(store: &crate::store::Store) -> Option<(u64, u64)> {
    restored_plan(store).ok().flatten().map(|p| (p.last_source_at, p.last_delivered_at))
}

pub(super) fn restored_attempt(store: &crate::store::Store) -> u64 {
    // New mirrors authorize no retry suppression without their canonical commit.
    if store.root().join("brief_delivery_plan.json").exists() || store.root().join("brief_effects.json").exists() {
        return restored_plan(store).ok().flatten().map_or(0, |p| p.automatic_offer_at);
    }
    store.load("last_brief_attempt_at")
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOW: u64 = 1_900_000_000;
    fn root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("atlas-brief-worker-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    fn platform() -> crate::platform::mock::MockPlatform {
        crate::platform::mock::MockPlatform::new(vec![crate::platform::Monitor { id: 1, x: 0, y: 0, width: 1280, height: 800, primary: true }])
    }
    fn daemon<'a>(cfg: &'a crate::config::Config, platform: &'a crate::platform::mock::MockPlatform, root: &Path) -> Daemon<'a> {
        Daemon::new(cfg, platform, None, crate::store::Store::new(root), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()))
    }

    #[test]
    fn malformed_and_oversized_reads_leave_the_record_untouched() {
        let root = root("strict");
        let path = root.join("ideas_named.json");
        std::fs::write(&path, "{broken").unwrap();
        assert!(Reader::default().record::<Vec<u64>>(&root, "ideas_named").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        assert!(!root.join("ideas_named.json.bak").exists());
        std::fs::write(&path, vec![b' '; LIMIT + 1]).unwrap();
        assert!(Reader::default().record::<Vec<u64>>(&root, "ideas_named").is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), (LIMIT + 1) as u64);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changed_owner_settings_or_domain_discard_prepared_text_and_effects() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let p = platform();
        for kind in ["owner", "settings", "domain"] {
            let root = root(kind);
            let mut d = daemon(&cfg, &p, &root);
            d.backlog.record("renew certificate", crate::backlog::Blocker::NeedsApproval, NOW);
            let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
            match kind {
                "owner" => {
                    let mut profiles = crate::profiles::Profiles::default();
                    profiles.add("Owner", crate::profiles::Role::Owner).unwrap();
                    let guest = profiles.add("Guest", crate::profiles::Role::Guest).unwrap();
                    profiles.active = Some(guest.id);
                    d.store.save("profiles", &profiles).unwrap();
                }
                "settings" => {
                    let tools = std::sync::Arc::make_mut(&mut d.tools_resolved);
                    tools.brief.enabled = !tools.brief.enabled;
                }
                _ => { d.backlog.record("new request", crate::backlog::Blocker::NeedsApproval, NOW + 1); }
            }
            assert!(d.accept_brief(prepared).is_err(), "{kind} accepted stale content");
            assert!(!d.store.exists("brief_prepared"));
            assert!(!d.store.exists("brief_effects"));
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn blocked_worker_does_not_hold_pause_and_resume_discards_its_result() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let p = platform();
        let root = root("blocked");
        let mut d = daemon(&cfg, &p, &root);
        d.backlog.record("renew certificate", crate::backlog::Blocker::NeedsApproval, NOW);
        let input = d.brief_input(NOW, root.clone()).unwrap();
        let epoch = input.epoch;
        let (release, wait) = mpsc::sync_channel::<()>(0);
        let (send, receive) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || { wait.recv().unwrap(); send.send(build(input)).unwrap(); });
        d.brief_preparation.receiver = Some(receive);
        let started = std::time::Instant::now();
        assert!(d.poll_brief().is_none());
        let paused = d.turn("pause", NOW + 1);
        assert!(d.attention.is_paused(), "{paused}");
        d.steer_mic();
        assert_ne!(d.brief_preparation.epoch, epoch, "pause did not invalidate the blocked preparation");
        let _ = d.turn("carry on", NOW + 2);
        assert!(!d.attention.is_paused());
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "a blocked brief held the control turn");
        release.send(()).unwrap();
        worker.join().unwrap();
        assert!(d.poll_brief().unwrap().is_err(), "resuming revived a canceled preparation");
        assert!(!d.store.exists("brief_prepared"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preparation_does_not_consume_reminders_and_delivery_rebases_by_precondition() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let p = platform();
        let root = root("effects");
        let mut d = daemon(&cfg, &p, &root);
        let mut later = crate::later::Later::default();
        later.items.push(crate::later::Item { what: "review saved design".into(), added: NOW - 1 });
        d.store.save(crate::later::RECORD, &later).unwrap();
        let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
        let line = prepared.effects.later.as_ref().unwrap().0.clone();
        d.accept_brief(prepared).unwrap();
        assert_eq!(d.store.load::<crate::later::Later>(crate::later::RECORD).mentioned, 0);
        d.acknowledge_prepared_effects("review saved", NOW + 1);
        assert_eq!(d.store.load::<crate::later::Later>(crate::later::RECORD).mentioned, 0);
        // A newer list survives; the old spoken line cannot consume its reminder.
        later.items.push(crate::later::Item { what: "new saved design".into(), added: NOW + 2 });
        d.store.save(crate::later::RECORD, &later).unwrap();
        d.acknowledge_prepared_effects(&line, NOW + 3);
        let current: crate::later::Later = d.store.load(crate::later::RECORD);
        assert_eq!(current.items.len(), 2);
        assert_eq!(current.mentioned, 0);
        // A fresh preparation, completely delivered, gets the receipt.
        let prepared = build(d.brief_input(NOW + 4, root.clone()).unwrap()).unwrap();
        let line = prepared.effects.later.as_ref().unwrap().0.clone();
        d.accept_brief(prepared).unwrap();
        d.acknowledge_prepared_effects(&line, NOW + 5);
        assert_eq!(d.store.load::<crate::later::Later>(crate::later::RECORD).mentioned, NOW + 5);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_preparation_mirrors_or_commit_do_not_accept_or_revive_a_plan() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let p = platform();
        for key in ["brief_effects", "brief_prepared", "brief_delivery_plan"] {
            let root = root(key);
            let mut d = daemon(&cfg, &p, &root);
            d.backlog.record("renew certificate", crate::backlog::Blocker::NeedsApproval, NOW);
            let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
            std::fs::create_dir(root.join(format!("{key}.json"))).unwrap();
            let why = d.accept_brief(prepared).unwrap_err();
            assert!(why.contains("nothing was marked delivered"), "{key}: {why}");
            assert!(d.pending_brief_delivery.is_none());
            assert!(d.brief_preparation.plan.is_none());
            assert!(d.brief_preparation.effects.is_none());
            assert_eq!(d.last_brief_at, 0);
            drop(d);
            let restarted = daemon(&cfg, &p, &root);
            assert!(restarted.pending_brief_delivery.is_none(), "{key} revived a staged mirror");
            assert!(restarted.brief_preparation.effects.is_none());
            assert_eq!(restarted.last_brief_at, 0);
            drop(restarted);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn failed_effect_checkpoint_keeps_durable_retry_without_full_delivery_receipt() {
        struct Speaker;
        impl Mouth for Speaker {
            fn speak(&self, _: &str) -> crate::error::Result<()> { Ok(()) }
        }
        let mut cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let tools = cfg.tools.get_or_insert_with(Default::default);
        tools.sound.muted = false;
        tools.sound.speak_replies = "always".into();
        tools.sound.quiet_hours = false;
        let p = platform();
        let root = root("checkpoint-fault");
        let mut d = daemon(&cfg, &p, &root);
        let mut later = crate::later::Later::default();
        later.items.push(crate::later::Item { what: "review saved design".into(), added: NOW - 1 });
        d.store.save(crate::later::RECORD, &later).unwrap();
        let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
        let line = crate::brief::spoken(&prepared.brief);
        d.accept_brief(prepared).unwrap();
        std::fs::remove_file(root.join("brief_effects.json")).unwrap();
        std::fs::create_dir(root.join("brief_effects.json")).unwrap();
        d.tiers.tier = crate::input::Tier::Voice;
        d.say_volunteered_with(&Speaker, &line, &mut || None);
        assert!(d.said_for_apps.iter().any(|(_, line)| line.contains("receipt") && line.contains("pending")), "the failed checkpoint was not visible to apps");
        let saved: Option<DeliveryPlan> = Reader::default().record(&root, "brief_delivery_plan").unwrap();
        let saved = saved.unwrap();
        assert!(saved.pending.is_some());
        assert!(saved.effects.later.is_some(), "the failed checkpoint lost its retry intent");
        assert_eq!(saved.last_delivered_at, 0);
        assert_eq!(d.last_brief_at, 0);
        drop(d);
        let restarted = daemon(&cfg, &p, &root);
        assert!(restarted.pending_brief_delivery.is_some());
        assert!(restarted.brief_preparation.effects.as_ref().unwrap().later.is_some());
        assert_eq!(restarted.last_brief_at, 0);
        drop(restarted);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn oversized_application_history_reports_unavailable_coverage() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let p = platform();
        let root = root("applications-cap");
        let d = daemon(&cfg, &p, &root);
        let application = crate::applied::Application { to: "Fixture company".into(), role: "Designer".into(), applied_at: NOW - 10,
            stage: crate::applied::Stage::Applied, moved_at: NOW - 10, nudged: false };
        d.store.save(crate::applied::FILE, &crate::applied::Applications { all: vec![application; RECORDS + 1] }).unwrap();
        let why = match build(d.brief_input(NOW, root.clone()).unwrap()) { Ok(_) => panic!("oversized history yielded a full brief"), Err(why) => why };
        assert!(why.contains("coverage is unavailable"), "{why}");
        assert!(!d.store.exists("brief_prepared"));
        drop(d);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restart_rejects_stale_owner_and_settings_without_reviving_mirrors() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let p = platform();
        for kind in ["restart-owner", "restart-settings"] {
            let root = root(kind);
            let mut d = daemon(&cfg, &p, &root);
            d.backlog.record("renew certificate", crate::backlog::Blocker::NeedsApproval, NOW);
            let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
            d.accept_brief(prepared).unwrap();
            if kind == "restart-owner" {
                let mut profiles = crate::profiles::Profiles::default();
                profiles.add("Owner", crate::profiles::Role::Owner).unwrap();
                let guest = profiles.add("Guest", crate::profiles::Role::Guest).unwrap();
                profiles.active = Some(guest.id);
                d.store.save("profiles", &profiles).unwrap();
            }
            drop(d);
            let mut next_cfg = cfg.clone();
            if kind == "restart-settings" {
                let tools = next_cfg.tools.get_or_insert_with(Default::default);
                tools.brief.enabled = !tools.brief.enabled;
            }
            let mut restarted = daemon(&next_cfg, &p, &root);
            assert!(restarted.poll_brief().unwrap().is_err(), "{kind} did not report the canceled plan");
            assert!(restarted.pending_brief_delivery.is_none());
            assert!(restarted.brief_preparation.effects.is_none());
            assert_eq!(restarted.last_brief_at, 0);
            drop(restarted);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn prepared_brief_waits_for_snapshot_then_commits_once_and_cancel_stays_canceled() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let p = platform();
        for canceled in [false, true] {
            let root = root(if canceled { "held-canceled" } else { "held-retry" });
            let mut d = daemon(&cfg, &p, &root);
            d.backlog.record("renew certificate", crate::backlog::Blocker::NeedsApproval, NOW);
            d.brief_preparation.ready = Some(build(d.brief_input(NOW, root.clone()).unwrap()).unwrap());
            let store = d.store.clone();
            let (held, acquired) = mpsc::sync_channel(0);
            let (release, waiting) = mpsc::sync_channel(0);
            let holder = std::thread::spawn(move || { let _guard = store.transaction().unwrap(); held.send(()).unwrap(); waiting.recv().unwrap(); });
            acquired.recv().unwrap();
            let started = std::time::Instant::now();
            for _ in 0..3 { assert!(d.poll_brief().is_none()); }
            assert!(started.elapsed() < std::time::Duration::from_millis(200));
            assert!(d.brief_preparation.ready.is_some());
            assert!(d.pending_brief_delivery.is_none());
            assert!(!d.store.exists("brief_delivery_plan"));
            if canceled { d.cancel_brief_preparation(); }
            release.send(()).unwrap(); holder.join().unwrap();
            let result = d.poll_brief().expect("released result must settle");
            if canceled { assert!(result.is_err()); assert!(d.pending_brief_delivery.is_none()); }
            else { assert!(result.unwrap().yours.iter().any(|item| item.subject.contains("certificate"))); assert!(d.pending_brief_delivery.is_some()); }
            assert!(d.poll_brief().is_none(), "one preparation cannot be accepted twice");
            assert_eq!(d.last_brief_at, 0, "acceptance is not a delivery receipt");
            drop(d); std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn relocated_day_items_reads_local_domains_without_consuming_them() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let p = platform(); let root = root("day-domains");
        let mut d = daemon(&cfg, &p, &root);
        let mut input = d.brief_input(NOW, root.clone()).unwrap();
        input.workday.market_in_brief = true;
        input.hunt.enabled = true;
        let local = input.zone.to_local(NOW as i64).max(0) as u64;
        let today = (local / 86_400) as i64;
        let civil = crate::civil::Civil::from_local(local as i64);
        let mut people = crate::people::People::default();
        people.every("Disposable contact", Some(7)).unwrap();
        people.birthday("Disposable contact", civil.month, civil.day).unwrap();
        let mut habits = crate::habits::Habits::default(); habits.add("Practice sketching", 1, 1, today).unwrap();
        let mut cards = crate::srs::Deck::default(); cards.add("Disposable question", "Disposable answer", "proof", today).unwrap();
        d.store.save("people", &people).unwrap(); d.store.save("habits", &habits).unwrap(); d.store.save("cards", &cards).unwrap();
        let mut mail = crate::mailbook::MailBook::default();
        mail.add(vec![crate::mailbook::Letter { id: "disposable-waiting".into(), in_reply_to: None, refs: vec![], from_name: "Owner".into(), from: "owner@example.test".into(), to: vec!["contact@example.test".into()], subject: "Disposable numbers".into(), at: NOW - 7 * 86400, dated: true, mine: true, excerpt: "Could you send the quarterly numbers?".into() }], 30, NOW);
        d.store.save(crate::mailbook::MailBook::FILE, &mail).unwrap();
        let mut watch = crate::social::watchlist::Watch::default();
        watch.list.push(crate::social::watchlist::Watched { target: crate::social::watchlist::Target::HackerNews { query: "disposable proof".into() }, next_due: 0, failures: 0, last_ok: Some(NOW), last_error: String::new(), last_modified: None });
        watch.summary = Some((NOW, "Disposable saved social summary".into()));
        d.store.save(crate::social::watchlist::FILE, &watch).unwrap();
        input.workday.social.enabled = true; input.workday.social.in_brief = true;
        input.workday.waiting_for.enabled = true;
        d.notebook.notes.push(crate::capture::Note { id: 42, text: "Disposable due design note".into(), at: NOW, while_in: None, about: vec![], kind: crate::capture::Kind::Task, confirmed: true, due: Some(local), reviewed: false });
        input.memory = d.brief_memory(NOW).unwrap();

        let prepared = build(input).unwrap();
        for prefix in ["waiting:", "note:", "person:", "birthday:", "habits", "cards", "social:watch", "market:", "opportunity:ask"] {
            assert!(prepared.brief.yours.iter().any(|item| item.id.starts_with(prefix)), "day collector omitted {prefix}");
        }
        assert_eq!(serde_json::to_value(d.store.load::<crate::people::People>("people")).unwrap(), serde_json::to_value(&people).unwrap());
        assert_eq!(serde_json::to_value(d.store.load::<crate::habits::Habits>("habits")).unwrap(), serde_json::to_value(&habits).unwrap());
        assert_eq!(serde_json::to_value(d.store.load::<crate::srs::Deck>("cards")).unwrap(), serde_json::to_value(&cards).unwrap());
        assert!(d.pending_brief_delivery.is_none());
        drop(d); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn automatic_preparation_errors_cool_down_but_explicit_cancel_never_revives_today() {
        let mut cfg = crate::config::Config::load(Path::new("config")).unwrap();
        cfg.tools.as_mut().unwrap().brief.enabled = true;
        cfg.tools.as_mut().unwrap().brief.not_before_hour = 0;
        let p = platform(); *p.input_idle.borrow_mut() = Some(5);
        let root = root("automatic-retry-cooldown"); let mut d = daemon(&cfg, &p, &root);
        std::fs::write(root.join("ideas_named.json"), b"malformed disposable record").unwrap();
        d.request_brief(NOW).unwrap(); d.last_brief_attempt_at = NOW;
        // Await the actual read-only worker independently of unrelated tick work.
        // Preserve its unchanged result for the real tick's presentation/cooldown path.
        let receiver = d.brief_preparation.receiver.take().expect("actual preparation worker");
        let result = receiver.recv_timeout(std::time::Duration::from_secs(3)).expect("actual worker completed within its bounded fixture wait");
        let (send, receive) = mpsc::sync_channel(1);
        assert!(send.send(result).is_ok(), "retain the actual worker result");
        d.brief_preparation.receiver = Some(receive);
        let started = std::time::Instant::now();
        let notices = d.tick(NOW);
        eprintln!("automatic brief error presentation tick: {}ms; {}", started.elapsed().as_millis(), d.tick_laps.plain(10));
        assert_eq!(notices.iter().filter(|line| line.contains("ideas_named")).count(), 1, "the real preparation error must be presented once: {}", notices.join("; "));
        for _ in 0..20 {
            let out = d.tick(NOW);
            assert!(!out.iter().any(|line| line.contains("ideas_named")), "repeated tick repeated a terminal notice: {}", out.join("; "));
            assert!(d.brief_preparation.receiver.is_none(), "cooldown started another automatic worker");
        }
        assert!(!d.automatic_brief_available(NOW));
        d.brief_preparation.automatic_retry_after = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert!(d.automatic_brief_available(NOW), "retry eligibility should return after wall cooldown");
        std::fs::remove_file(root.join("ideas_named.json")).unwrap();
        d.request_brief(NOW).unwrap();
        assert!(d.explicitly_cancel_brief_preparation(NOW));
        assert!(!d.automatic_brief_available(NOW));
        assert!(d.automatic_brief_available(NOW + 86_400), "an explicit cancellation must not block another day");
        assert_eq!(d.last_brief_at, 0);
        drop(d); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uncommitted_daily_attempt_does_not_suppress_restart_and_acceptance_checkpoints_it() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap(); let p = platform();
        let root = root("attempt-checkpoint"); let mut d = daemon(&cfg, &p, &root);
        d.backlog.record("renew certificate", crate::backlog::Blocker::NeedsApproval, NOW);
        d.store.save("last_brief_attempt_at", &NOW).unwrap();
        d.store.save("brief_effects", &Some(Effects::default())).unwrap();
        assert_eq!(restored_attempt(&d.store), 0, "a staged attempt cannot suppress a restarted offer");
        let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
        d.accept_brief(prepared).unwrap();
        assert_eq!(d.store.load::<u64>("last_brief_attempt_at"), NOW);
        assert_eq!(restored_attempt(&d.store), NOW);
        assert_eq!(d.last_brief_at, 0, "attempt checkpoint is not a delivered receipt");
        drop(d); std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn authorized_domain_changes_rebuild_once_but_owner_changes_cancel() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap(); let p = platform();
        for case in ["automatic", "manual", "owner"] {
            let root = root(case); let mut d = daemon(&cfg, &p, &root);
            d.backlog.record("original certificate", crate::backlog::Blocker::NeedsApproval, NOW);
            let mut input = d.brief_input(NOW, root.clone()).unwrap();
            input.automatic = case != "manual";
            d.brief_preparation.ready = Some(build(input).unwrap());
            if case == "owner" {
                let mut profiles = crate::profiles::Profiles::default();
                profiles.add("Changed owner", crate::profiles::Role::Owner).unwrap();
                d.store.save("profiles", &profiles).unwrap();
            } else { d.backlog.record("new printer request", crate::backlog::Blocker::NeedsApproval, NOW); }
            let mut result = d.poll_brief();
            if case != "owner" {
                assert!(result.is_none(), "authorized stale content must rebuild, not be delivered or discarded");
                assert!(d.pending_brief_delivery.is_none());
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                while result.is_none() && std::time::Instant::now() < deadline { std::thread::sleep(std::time::Duration::from_millis(5)); result = d.poll_brief(); }
                let brief = result.expect("fresh brief never completed").unwrap();
                assert!(brief.yours.iter().any(|item| item.subject.contains("new printer request")));
                assert!(d.poll_brief().is_none(), "a fresh preparation must settle only once");
                assert_eq!(d.last_brief_at, 0, "fresh preparation must not claim actual delivery");
            } else {
                assert!(result.unwrap().is_err(), "{case} revived stale work");
                assert!(d.pending_brief_delivery.is_none());
                assert!(d.brief_preparation.receiver.is_none());
            }
            drop(d); std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn accepted_plan_survives_connectivity_resolution_and_records_actual_delivery_once() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap(); let p = platform(); let root = root("plan-connectivity");
        let mut d = daemon(&cfg, &p, &root);
        d.backlog.record("renew certificate", crate::backlog::Blocker::NeedsApproval, NOW);
        let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
        d.accept_brief(prepared).unwrap();
        let line = d.pending_brief_delivery.as_ref().unwrap().0.clone();
        let saved_signature = d.brief_preparation.plan.as_ref().unwrap().configuration.clone();
        for reach in [crate::connectivity::Reach::Offline, crate::connectivity::Reach::Online] {
            d.connectivity.set(reach, NOW + 1);
            d.ensure_brief_plan_current();
            assert!(d.pending_brief_delivery.is_some(), "reachability canceled accepted text");
            assert_eq!(d.brief_preparation.plan.as_ref().unwrap().configuration, saved_signature, "legacy full signature remains readable");
        }
        assert!(d.acknowledge_prepared_effects(&line, NOW + 2));
        d.acknowledge_brief_delivery(&line, NOW + 2);
        assert_eq!(d.last_brief_at, NOW + 2);
        assert!(d.pending_brief_delivery.is_none());
        d.acknowledge_brief_delivery(&line, NOW + 3);
        assert_eq!(d.last_brief_at, NOW + 2, "same delivered text was recorded twice");
        drop(d); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_or_unknown_saved_settings_cannot_revive_pending_delivery() {
        for configuration in ["not-json", "[]", "[{}, {}, {}, \"UTC\", \"future-reach\"]", "[{}, {}, {}, 42, \"offline\"]"] {
            assert!(brief_settings_signature(configuration).is_err());
        }
        let cfg = crate::config::Config::load(Path::new("config")).unwrap(); let p = platform(); let root = root("plan-settings-format");
        let mut d = daemon(&cfg, &p, &root);
        d.backlog.record("renew certificate", crate::backlog::Blocker::NeedsApproval, NOW);
        let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap(); d.accept_brief(prepared).unwrap();
        let mut plan = d.brief_preparation.plan.clone().unwrap(); plan.configuration = "[]".into();
        d.store.save("brief_delivery_plan", &Some(plan)).unwrap();
        drop(d);
        let mut restarted = daemon(&cfg, &p, &root);
        assert!(restarted.pending_brief_delivery.is_none());
        assert!(restarted.poll_brief().unwrap().is_err());
        assert_eq!(restarted.last_brief_at, 0);
        drop(restarted); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn daypart_introduction_is_pending_until_actual_delivery_and_survives_restart() {
        let mut cfg = crate::config::Config::load(Path::new("config")).unwrap();
        cfg.tools.as_mut().unwrap().hunt.enabled = false;
        let p = platform(); let root = root("daypart-introduction");
        let mut d = daemon(&cfg, &p, &root);
        d.request_daypart_brief(NOW, crate::nudge::Part::Morning).unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let brief = loop {
            if let Some(result) = d.poll_brief() { break result.unwrap(); }
            assert!(std::time::Instant::now() < until); std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(brief.is_empty(), "fixture must exercise the original empty greeting");
        let line = d.take_brief_announcement().unwrap();
        assert!(line.starts_with("Good morning.")); assert!(line.contains("get to know me"));
        assert!(!d.store.load::<bool>("offered_get_to_know"));
        assert!(d.acknowledge_prepared_effects("Good morning.", NOW));
        assert!(!d.store.load::<bool>("offered_get_to_know"), "partial audio cannot spend the introduction");
        drop(d);
        let mut d = daemon(&cfg, &p, &root);
        assert_eq!(d.pending_brief_delivery.as_ref().unwrap().0, line);
        assert!(!d.store.load::<bool>("offered_get_to_know"));
        assert!(d.acknowledge_prepared_effects(&line, NOW + 1));
        assert!(d.store.load::<bool>("offered_get_to_know"));
        d.acknowledge_brief_delivery(&line, NOW + 1);
        d.brief_preparation.daypart = Some(crate::nudge::Part::Evening);
        let prepared = build(d.brief_input(NOW + 86_400, root.clone()).unwrap()).unwrap();
        assert!(prepared.announcement.is_none(), "the once-only offer must not repeat");
        drop(d); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_empty_brief_never_offers_onboarding_and_canceled_daypart_never_spends_it() {
        let mut cfg = crate::config::Config::load(Path::new("config")).unwrap(); cfg.tools.as_mut().unwrap().hunt.enabled = false;
        let p = platform(); let root = root("daypart-cancel"); let mut d = daemon(&cfg, &p, &root);
        let manual = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
        assert!(manual.brief.is_empty()); assert!(manual.announcement.is_none()); assert!(manual.effects.introduction.is_none());
        d.request_daypart_brief(NOW, crate::nudge::Part::Morning).unwrap();
        assert!(d.explicitly_cancel_brief_preparation(NOW));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = d.poll_brief() { assert!(result.is_err()); break; }
            assert!(std::time::Instant::now() < until); std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(d.pending_brief_delivery.is_none()); assert!(!d.store.load::<bool>("offered_get_to_know"));
        assert!(!d.automatic_brief_available(NOW));
        drop(d); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn introduction_survives_activity_updates_but_not_new_projects() {
        let mut cfg = crate::config::Config::load(Path::new("config")).unwrap();
        cfg.tools.as_mut().unwrap().hunt.enabled = false;
        let p = platform();
        for project in [false, true] {
            let root = root(if project { "introduction-new-project" } else { "introduction-activity" });
            let mut d = daemon(&cfg, &p, &root);
            d.person.save(&d.store).unwrap();
            d.brief_preparation.daypart = Some(crate::nudge::Part::Morning);
            let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
            d.accept_brief(prepared).unwrap();
            let line = d.pending_brief_delivery.as_ref().unwrap().0.clone();
            let mut person = crate::person::Person::default();
            person.worked_at(12);
            if project { person.projects.push(("actual new project".into(), NOW)); }
            person.save(&crate::store::Store::new(&root)).unwrap();
            assert_eq!(d.acknowledge_prepared_effects(&line, NOW + 1), !project);
            assert_eq!(d.store.load::<bool>("offered_get_to_know"), !project);
            if project { assert!(d.pending_brief_delivery.is_none()); }
            // The normal turn/tick reload also preserves the outside writer's
            // profile before the fixture shuts down.
            d.take_outside_changes();
            drop(d); std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn accepted_introduction_waits_for_storage_without_spending_delivery() {
        let mut cfg = crate::config::Config::load(Path::new("config")).unwrap();
        cfg.tools.as_mut().unwrap().hunt.enabled = false;
        let p = platform(); let root = root("introduction-storage-busy");
        let mut d = daemon(&cfg, &p, &root);
        d.brief_preparation.daypart = Some(crate::nudge::Part::Morning);
        let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
        d.accept_brief(prepared).unwrap();
        let line = d.pending_brief_delivery.as_ref().unwrap().0.clone();
        let store = crate::store::Store::new(&root);
        let (locked, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _guard = store.transaction().unwrap();
            locked.send(()).unwrap(); wait.recv().unwrap();
        });
        ready.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let started = std::time::Instant::now();
        assert!(!d.acknowledge_prepared_effects(&line, NOW + 1));
        let elapsed = started.elapsed();
        release.send(()).unwrap(); holder.join().unwrap();
        assert!(elapsed < std::time::Duration::from_millis(500));
        assert_eq!(d.pending_brief_delivery.as_ref().unwrap().0, line);
        assert!(!d.store.load::<bool>("offered_get_to_know"));
        assert!(d.acknowledge_prepared_effects(&line, NOW + 2));
        assert!(d.store.load::<bool>("offered_get_to_know"));
        drop(d); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn daypart_keeps_real_brief_greeting_and_rejects_changed_knowledge() {
        let cfg = crate::config::Config::load(Path::new("config")).unwrap(); let p = platform(); let root = root("daypart-knowledge");
        let mut d = daemon(&cfg, &p, &root);
        d.brief_preparation.daypart = Some(crate::nudge::Part::Morning);
        d.backlog.record("renew certificate", crate::backlog::Blocker::NeedsApproval, NOW);
        let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
        assert!(prepared.announcement.as_ref().unwrap().starts_with("Good morning."));
        assert!(prepared.announcement.as_ref().unwrap().contains("certificate"));
        assert!(prepared.effects.introduction.is_none());
        d.person.projects.push(("new project".into(), NOW));
        assert!(d.brief_staleness(&prepared).unwrap().is_some());
        assert!(d.accept_brief(prepared).is_err()); assert!(d.pending_brief_delivery.is_none());
        drop(d); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accepted_introduction_waits_while_paused_and_cancel_does_not_revive_on_restart() {
        let mut cfg = crate::config::Config::load(Path::new("config")).unwrap(); cfg.tools.as_mut().unwrap().hunt.enabled = false;
        let p = platform(); let root = root("daypart-accepted-cancel"); let mut d = daemon(&cfg, &p, &root);
        d.brief_preparation.daypart = Some(crate::nudge::Part::Evening);
        let prepared = build(d.brief_input(NOW, root.clone()).unwrap()).unwrap();
        assert!(prepared.effects.introduction.is_some()); d.accept_brief(prepared).unwrap();
        d.morning_brief = d.take_brief_announcement();
        d.attention.pause(None, NOW); let mut out = Vec::new(); d.offer_ready_brief(&mut out);
        assert!(out.is_empty()); assert!(!d.store.load::<bool>("offered_get_to_know"));
        assert!(d.explicitly_cancel_brief_preparation(NOW));
        assert!(d.morning_brief.is_none()); assert!(d.pending_brief_delivery.is_none());
        drop(d); let d = daemon(&cfg, &p, &root);
        assert!(d.pending_brief_delivery.is_none()); assert!(!d.store.load::<bool>("offered_get_to_know"));
        drop(d); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn actual_arrival_selects_greeting_before_attempt_timestamp_spends_it() {
        let mut cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let tools = cfg.tools.as_mut().unwrap(); tools.brief.enabled = true; tools.brief.not_before_hour = 0; tools.hunt.enabled = false;
        let p = platform(); *p.input_idle.borrow_mut() = Some(5);
        let root = root("arrival-daypart-origin"); let mut d = daemon(&cfg, &p, &root);
        // The keyboard shows that we're here now; the prior interaction was
        // yesterday, so this exercises an arrival rather than StillGoing.
        d.last_turn_of_yours = NOW - 86_400;
        d.tick(NOW);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = d.poll_brief() { result.unwrap(); break; }
            assert!(std::time::Instant::now() < until); std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let part = crate::nudge::Part::from_hour(crate::localclock::hour_here(NOW)).unwrap();
        assert_eq!(d.brief_preparation.plan.as_ref().unwrap().daypart, Some(part));
        assert!(d.pending_brief_delivery.as_ref().unwrap().0.starts_with(part.greeting()));
        assert!(!d.store.load::<bool>("offered_get_to_know")); assert_eq!(d.last_greeted_at, 0);
        drop(d); std::fs::remove_dir_all(root).unwrap();
    }

}
