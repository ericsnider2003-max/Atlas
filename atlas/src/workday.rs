//! Round 11's seventeen tools, joined to the daemon: what you say reaches
//! them, what they keep is saved, the tick feeds the few that run on their
//! own, and the brief reads your day from them.
//!
//! The tools themselves live in their own modules and know nothing of the
//! daemon; this is the one place that does, so the wiring can be read in one
//! sitting. Three rules it keeps for all of them:
//!
//! - **Lazy.** Nothing is loaded until it is first used, and nothing runs on
//!   the tick unless it's switched on and has something to do -- a daemon
//!   that never uses a card deck never reads one off the disk.
//! - **Follow-ups are short-lived.** "Open 2" means the list you were just
//!   shown, for ten minutes; after that "open 2" is an ordinary sentence
//!   again.
//! - **Your data stays yours.** Every one of these is on the owner's list
//!   (`profiles::THE_OWNERS_OWN`) or harmless to a guest, and none of them is
//!   open to an add-on (`plugins::NEVER`).

use crate::daemon::Daemon;
use crate::intent::Intent;
use serde::{Deserialize, Serialize};

/// How long a numbered list can be referred to by number.
pub const FOLLOW_FOR: u64 = 600;

// ---------------------------------------------------------------- settings

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkdayConfig {
    pub clipboard_history: crate::cliphist::HistoryConfig,
    pub waiting_for: crate::waitingfor::WaitingConfig,
    pub trade_day: crate::tradeday::TradeDayConfig,
    pub feeds: crate::feeds::FeedsConfig,
    pub cards: crate::srs::SrsConfig,
    pub translate: crate::translation::TranslateConfig,
    pub chords: crate::chords::ChordsConfig,
    /// Your social accounts and the people you watch (`social`).
    pub social: crate::social::SocialConfig,
    /// Minutes before a meeting that its prep is offered; 0 is off.
    pub meeting_prep_minutes: u64,
    /// Days of mail kept in the local cache.
    pub mail_keep_days: u64,
    /// A PNG of your signature, for "sign the pdf". Empty: not set.
    pub signature_png: String,
    /// The market's calendar in the brief. It's there anyway once you've
    /// done a trading check-in.
    pub market_in_brief: bool,
}

impl Default for WorkdayConfig {
    fn default() -> Self {
        WorkdayConfig {
            clipboard_history: Default::default(),
            waiting_for: Default::default(),
            trade_day: Default::default(),
            feeds: Default::default(),
            cards: Default::default(),
            translate: Default::default(),
            chords: Default::default(),
            social: Default::default(),
            meeting_prep_minutes: 15,
            mail_keep_days: 60,
            signature_png: String::new(),
            market_in_brief: false,
        }
    }
}

// ---------------------------------------------------------------- follow-ups

/// What a number or a short answer refers to right now.
#[derive(Debug, Clone, PartialEq)]
pub enum Follow {
    Files(usize),
    Launch(usize),
    Waiting(usize),
    Feeds(usize),
    /// The opportunities just listed, in the brief or asked for (`hunting`).
    Opportunities(usize),
    Quiz,
    /// The notes review just listed this many.
    Review(usize),
    Trade(Vec<crate::tradeday::Question>),
    Receipt,
}

/// What the parser is told before each turn: the names that make a
/// sentence about a person or a habit unambiguous, and the live follow-up.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Known {
    pub people: Vec<String>,
    pub habits: Vec<String>,
    pub follow: Option<Follow>,
}

fn first_word(s: &str) -> String {
    s.split_whitespace().next().unwrap_or("").trim_matches(|c: char| !c.is_alphanumeric()).to_ascii_lowercase()
}

/// A number after a verb: "open 2", "done 3", "read the second".
pub fn numbered_reply(said: &str, verbs: &[&str], count: usize) -> Option<(String, usize)> {
    let low = said.trim().trim_end_matches(['.', '!']).to_ascii_lowercase();
    let words: Vec<&str> = low.split_whitespace().collect();
    let (verb, rest) = verbs.iter().find_map(|v| {
        let vw: Vec<&str> = v.split_whitespace().collect();
        (words.len() > vw.len() && words[..vw.len()] == vw[..]).then(|| (v.to_string(), &words[vw.len()..]))
    })?;
    const ORD: &[(&str, usize)] = &[("first", 1), ("second", 2), ("third", 3), ("fourth", 4), ("fifth", 5), ("sixth", 6), ("seventh", 7), ("eighth", 8), ("ninth", 9), ("tenth", 10)];
    for w in rest {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric());
        if matches!(w, "the" | "one" | "number" | "no") {
            continue;
        }
        let n = w.parse::<usize>().ok().or_else(|| ORD.iter().find(|(o, _)| *o == w).map(|(_, n)| *n)).or_else(|| (w == "last").then_some(count))?;
        return (1..=count).contains(&n).then(|| (verb, n - 1));
    }
    None
}

/// The sentences a tool reads whole, before the phrase table: a follow-up
/// to a list just shown, or a sentence whose shape is unmistakable ("keep
/// in touch with Priya every month"). `None` sends the sentence on to the
/// ordinary parser.
pub fn read_first(input: &str, k: &Known) -> Option<(Intent, &'static str)> {
    let said = input.trim();
    if said.is_empty() {
        return None;
    }
    let own = || said.to_string();
    match &k.follow {
        Some(Follow::Files(n)) if numbered_reply(said, &["open", "show", "merge", "extract", "sign"], *n).is_some() || crate::findfile::which(said, *n).is_some() => {
            let v = first_word(said);
            return Some(if v == "open" || v == "show" { (Intent::FindFile(own()), "find_file") } else { (Intent::Pdf(own()), "pdf") });
        }
        Some(Follow::Review(n)) if numbered_reply(said, &["keep", "drop", "done with", "bin"], *n).is_some() || said.trim().trim_end_matches('.').eq_ignore_ascii_case("keep all") => {
            return Some((Intent::NoteReview(own()), "note_review"))
        }
        Some(Follow::Launch(n)) if numbered_reply(said, &["open", "launch", "start"], *n).is_some() => return Some((Intent::Launch(own()), "launch")),
        Some(Follow::Waiting(n)) if numbered_reply(said, &["done", "not a promise", "not a request", "wrong", "close"], *n).is_some() => {
            return Some((Intent::WaitingFor(own()), "waiting_for"))
        }
        Some(Follow::Feeds(n)) if numbered_reply(said, &["read", "open", "save", "skip", "keep"], *n).is_some() => return Some((Intent::Feeds(own()), "feeds")),
        Some(Follow::Opportunities(n)) if crate::hunt::understand(said, *n).is_some() => return Some((Intent::Opportunities(own()), "opportunities")),
        Some(Follow::Quiz) if crate::srs::Grade::read(said).is_some() || matches!(said.to_ascii_lowercase().trim_end_matches(['.', '!']), "show" | "show me" | "flip" | "answer" | "stop" | "that's enough" | "done") => {
            return Some((Intent::Cards(own()), "cards"))
        }
        Some(Follow::Trade(qs)) if crate::tradeday::read(qs, said).is_some() || said.eq_ignore_ascii_case("skip") => return Some((Intent::TradeDay(own()), "trade_day")),
        Some(Follow::Receipt) => {
            let low = said.to_ascii_lowercase();
            let low = low.trim_end_matches(['.', '!']);
            if matches!(low, "yes" | "yes keep it" | "keep it" | "no" | "don't keep it") || (crate::receipts::amounts(said).len() == 1 && said.split_whitespace().count() <= 3) {
                return Some((Intent::Receipt(own()), "receipt"));
            }
        }
        _ => {}
    }
    if let Some(a) = crate::people::read(said) {
        use crate::people::Asked;
        let knows = |who: &str| {
            let w = who.to_lowercase();
            k.people.iter().any(|p| *p == w || p.split(' ').next() == Some(w.as_str()))
        };
        let take = match &a {
            Asked::Talked { who } | Asked::About { who } => knows(who),
            _ => true,
        };
        if take {
            return Some((Intent::People(own()), "people"));
        }
    }
    if let Some(a) = crate::habits::read(said) {
        use crate::habits::Asked;
        let take = match &a {
            Asked::Did { name } | Asked::Undo { name } => {
                let n = name.to_lowercase();
                k.habits.iter().any(|h| h.contains(&n) || n.contains(h.as_str()))
            }
            _ => true,
        };
        if take {
            return Some((Intent::Habit(own()), "habit"));
        }
    }
    // "Follow theverge.com": a site, by its address.
    if let Some(rest) = said.strip_prefix("follow ").or_else(|| said.strip_prefix("Follow ")) {
        let r = rest.trim().trim_end_matches('.');
        if !r.contains(' ') && r.contains('.') && r.len() >= 4 {
            return Some((Intent::Feeds(own()), "feeds"));
        }
    }
    // "Watch @name on YouTube", "follow the #rust hashtag on Mastodon":
    // the watch list (`social`), only when a source is named by its mark.
    if crate::social::spoken_watch(said) {
        return Some((Intent::Social(own()), "social"));
    }
    // The opportunity hunter's own sentences ("look for video editing gigs",
    // "not interested in crypto") and the wit ("tone it down"), whole
    // sentences only, so nothing longer is swallowed.
    if crate::hunt::understand(said, 0).is_some() {
        return Some((Intent::Opportunities(own()), "opportunities"));
    }
    if crate::wit::level_asked(said).is_some() {
        return Some((Intent::Wit(own()), "wit"));
    }
    if crate::snippets::read_save(said).is_some() {
        return Some((Intent::Snippet(own()), "snippet"));
    }
    if crate::srs::read_card(said).is_some() {
        return Some((Intent::Cards(own()), "cards"));
    }
    if crate::translation::read(said).is_some() {
        return Some((Intent::Translate(own()), "translate"));
    }
    None
}

/// Replies from these tools are lists and records, read whole rather than
/// cut to a spoken sentence count (`daemon`'s shaping step).
pub fn reads_whole(i: &Intent) -> bool {
    matches!(
        i,
        Intent::ClipHistory(_)
            // "This is exactly what will go" is only exactly that if nothing
            // is cut off it, and the feedback list is numbered for "answer
            // feedback 2". The update question carries what going back means.
            | Intent::Feedback(_)
            | Intent::Updates(_)
            | Intent::ScreenText(_)
            | Intent::MarketDay(_)
            | Intent::WaitingFor(_)
            | Intent::NoteReview(_)
            | Intent::Launch(_)
            | Intent::TradeDay(_)
            | Intent::MeetingPrep(_)
            | Intent::Snippet(_)
            | Intent::FindFile(_)
            | Intent::Pdf(_)
            | Intent::People(_)
            | Intent::Feeds(_)
            | Intent::Social(_)
            | Intent::Opportunities(_)
            | Intent::Receipt(_)
            | Intent::Habit(_)
            | Intent::Cards(_)
            | Intent::Translate(_)
    )
}

// ---------------------------------------------------------------- state

/// What's kept, loaded on first use.
#[derive(Default)]
pub struct Kit {
    clips: crate::cliphist::History,
    clips_on: Option<bool>,
    uses: Option<crate::launcher::Uses>,
    snippets: Option<crate::snippets::Snippets>,
    people: Option<crate::people::People>,
    feeds: Option<crate::feeds::Feeds>,
    receipts: Option<crate::receipts::Receipts>,
    habits: Option<crate::habits::Habits>,
    deck: Option<crate::srs::Deck>,
    journal: Option<crate::tradeday::Journal>,
    taught: Option<crate::waitingfor::Taught>,
    follow: Option<(Follow, u64)>,
    last_files: Vec<String>,
    last_launch: Vec<crate::launcher::Candidate>,
    last_waiting: Vec<crate::waitingfor::Waiting>,
    last_feeds: Vec<crate::feeds::Unread>,
    pending_receipt: Option<(crate::receipts::Reading, String)>,
    shortcuts: Option<(u64, Vec<crate::launcher::Candidate>)>,
    prepped: std::collections::HashSet<(u64, u64)>,
    trade_asked: Option<(i64, crate::tradeday::When)>,
    last_look: u64,
    feed_in_flight: Option<std::sync::mpsc::Receiver<(String, Result<crate::feeds::Parsed, String>)>>,
    chords: Option<std::sync::mpsc::Receiver<crate::chords::Does>>,
    chords_failed: Vec<crate::chords::Does>,
    /// What `social` keeps between turns (`social::glue`).
    pub(crate) social: crate::social::Live,
    /// The opportunity hunter's running state (`hunting`).
    pub(crate) hunt: crate::hunting::Live,
}

macro_rules! loaded {
    ($kit:expr, $store:expr, $field:ident, $file:expr) => {
        $kit.$field.get_or_insert_with(|| $store.load($file))
    };
}

const USES: &str = "launcher_uses";
const SNIPPETS: &str = "snippets";
const PEOPLE: &str = "people";
const FEEDS: &str = "feeds";
const RECEIPTS: &str = "receipts";
const HABITS: &str = "habits";
const DECK: &str = "cards";
const JOURNAL: &str = "trade_journal";
const TAUGHT: &str = "waiting_taught";
const CLIPS_ON: &str = "clipboard_history_on";

impl Kit {
    pub fn follow(&self, now: u64) -> Option<Follow> {
        self.follow.as_ref().filter(|(_, at)| now.saturating_sub(*at) <= FOLLOW_FOR).map(|(f, _)| f.clone())
    }

    fn set_follow(&mut self, f: Option<Follow>, now: u64) {
        self.follow = f.map(|f| (f, now));
    }

    /// The opportunities just listed (`hunting`): "save 2" means the second
    /// of them for the next ten minutes, like every other numbered list.
    pub(crate) fn follow_opportunities(&mut self, n: usize, now: u64) {
        self.set_follow((n > 0).then_some(Follow::Opportunities(n)), now);
    }

    pub fn known(&mut self, store: &crate::store::Store, now: u64) -> Known {
        // Names only, and only for what's already loaded or small: the
        // people and habits files are read once, then kept.
        let people = loaded!(self, store, people, PEOPLE)
            .by_key
            .keys()
            .cloned()
            .collect();
        let habits = loaded!(self, store, habits, HABITS).habits.iter().map(|h| h.name.to_lowercase()).collect();
        Known { people, habits, follow: self.follow(now) }
    }
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// A file name that isn't taken yet: "report (merged).pdf", then
/// "report (merged 2).pdf". Nothing is ever written over.
pub fn unused_name(dir: &std::path::Path, stem: &str, what: &str, ext: &str) -> std::path::PathBuf {
    let mut p = dir.join(format!("{stem} ({what}).{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({what} {n}).{ext}"));
        n += 1;
    }
    p
}

// ---------------------------------------------------------------- handlers

impl Daemon<'_> {
    pub(crate) fn workday_cfg(&self) -> WorkdayConfig {
        self.tools_ref().map(|t| t.workday.clone()).unwrap_or_default()
    }

    fn local(&self, t: u64) -> u64 {
        self.home_zone().to_local(t as i64).max(0) as u64
    }

    fn today(&self, t: u64) -> i64 {
        (self.local(t) / 86_400) as i64
    }

    /// Before a turn: what the parser needs to read a follow-up or a name.
    pub(crate) fn workday_known(&mut self, t: u64) {
        let k = self.workday.known(&self.store, t);
        self.parser.know_workday(k);
    }

    fn keep<T: Serialize>(&self, name: &str, v: &T) -> Option<String> {
        self.store.save(name, v).err().map(|e| format!(" (I couldn't save that: {e})"))
    }

    // -- 1. clipboard history ------------------------------------------------

    fn clips_on(&mut self) -> bool {
        let cfg = self.workday_cfg().clipboard_history;
        let store = &self.store;
        *self.workday.clips_on.get_or_insert_with(|| store.load::<Option<bool>>(CLIPS_ON).unwrap_or(cfg.enabled))
    }

    pub(crate) fn wd_clip_history(&mut self, said: &str, t: u64) -> String {
        let low = said.to_ascii_lowercase();
        let cfg = self.workday_cfg().clipboard_history;
        if low.contains("turn on") || low.contains("start keeping") || low.contains("switch on") {
            self.workday.clips_on = Some(true);
            let _ = self.store.save(CLIPS_ON, &Some(true));
            return format!(
                "Clipboard history is on. I'll keep what you copy for {} hours, in memory only -- never on disk -- and never anything a password manager marks private or that looks like a key. \"Turn off clipboard history\" stops it and forgets it all.",
                cfg.keep_hours
            );
        }
        if low.contains("turn off") || low.contains("stop keeping") || low.contains("switch off") {
            self.workday.clips_on = Some(false);
            let _ = self.store.save(CLIPS_ON, &Some(false));
            self.workday.clips = Default::default();
            return "Clipboard history is off, and what it held is gone.".into();
        }
        if !self.clips_on() {
            return "Clipboard history is off: I never read your clipboard unless you ask. \"Turn on clipboard history\" starts it, kept in memory for a day.".into();
        }
        if low.contains("clear") || low.contains("forget") {
            let n = self.workday.clips.clips.len();
            self.workday.clips.clips.clear();
            return format!("Forgot {}.", plural(n, "copy"));
        }
        if let Some((_, i)) = numbered_reply(said, &["paste", "copy", "put back", "give me", "restore", "use"], self.workday.clips.clips.len()) {
            let recent = self.workday.clips.recent(usize::MAX);
            let Some(c) = recent.get(i) else { return "There's no copy with that number.".into() };
            let text = c.text.clone();
            return match self.plat.write_clipboard(&text) {
                Ok(()) => format!("It's back on your clipboard: {}", crate::cliphist::line(c, t)),
                Err(e) => format!("I couldn't put it back: {e}"),
            };
        }
        self.workday.clips.forget(&cfg, t);
        let q = ["find ", "search for ", "with ", "about ", "containing "].iter().find_map(|m| low.find(m).map(|i| said[i + m.len()..].trim().to_string()));
        let list: Vec<&crate::cliphist::Clip> = match &q {
            Some(q) if !q.is_empty() => self.workday.clips.find(q),
            _ => self.workday.clips.recent(10),
        };
        if list.is_empty() {
            return match q {
                Some(q) => format!("Nothing you copied matches \"{q}\"."),
                None => self.workday.clips.describe(&crate::cliphist::HistoryConfig { enabled: true, ..cfg.clone() }),
            };
        }
        let mut out: Vec<String> = list.iter().take(10).enumerate().map(|(i, c)| format!("{}. {}", i + 1, crate::cliphist::line(c, t))).collect();
        out.push("\"Paste 2\" puts one back on your clipboard.".into());
        out.join("\n")
    }

    fn clip_tick(&mut self, t: u64) {
        if !self.clips_on() {
            return;
        }
        // One cheap call: the clipboard's sequence number. The copy is read
        // only when it moved.
        if !self.workday.clips.changed(self.plat.clipboard_change()) {
            return;
        }
        let Some(copy) = self.plat.clipboard_copy() else { return };
        let from = self.plat.active_window().ok().flatten().map(|w| w.process).unwrap_or_default();
        let cfg = self.workday_cfg().clipboard_history;
        let _ = self.workday.clips.keep(&cfg, &copy, &from, t);
        self.workday.clips.forget(&cfg, t);
    }

    // -- 2. copy text off the screen ------------------------------------------

    pub(crate) fn wd_screen_text(&mut self, said: &str, _t: u64) -> String {
        match self.screen_words() {
            Ok((text, engine, title)) => {
                let text = crate::screentext::pick(&text, said);
                match self.plat.write_clipboard(&text) {
                    Ok(()) => crate::screentext::said(&text, engine, &title),
                    Err(e) => format!("I read it but couldn't put it on your clipboard: {e}"),
                }
            }
            Err(why) => why,
        }
    }

    /// The front window's words: Windows' own recognizer, on this machine.
    fn screen_words(&self) -> Result<(String, crate::screentext::Engine, String), String> {
        let grab = match self.plat.grab_window() {
            Ok(Some(g)) => g,
            Ok(None) => return Err("I can't capture a window on this machine.".into()),
            Err(e) => return Err(format!("I couldn't capture the window: {e}")),
        };
        let raw = match self.plat.recognise_text(&grab) {
            Ok(Some(t)) => t,
            Ok(None) => return Err("There's no text recognizer here -- on Windows it's built in (Settings > Time & language > Language, with a language pack installed).".into()),
            Err(e) => return Err(format!("The text recognizer failed: {e}")),
        };
        let text = crate::screentext::tidy_lines(&raw);
        if !crate::screentext::plausible(&text) {
            return Err("I couldn't read any real text in that window -- if it's an image, zooming in usually helps.".into());
        }
        Ok((text, crate::screentext::Engine::Windows, grab.title))
    }

    // -- 3. market days ---------------------------------------------------------

    pub(crate) fn wd_market_day(&mut self, said: &str, t: u64) -> String {
        let low = said.to_lowercase();
        if low.contains("next") && (low.contains("holiday") || low.contains("closed") || low.contains("closure")) {
            return match crate::marketdays::next_closure(t as i64) {
                Some(((y, m, d), day)) => {
                    let what = match day {
                        crate::marketdays::Day::Closed(n) => format!("closed for {n}"),
                        crate::marketdays::Day::EarlyClose(n) => format!("closes early, 1:00 pm New York ({n})"),
                        _ => "open".into(),
                    };
                    format!("Next: {y}-{m:02}-{d:02}, US markets {what}.")
                }
                None => "I don't have a closure in the next year on file.".into(),
            };
        }
        // "Is the market open?" answered first, then the day's calendar
        // (the capability sweep, 30 Sep 2026: it gave only tomorrow's data).
        if low.contains("open") || low.contains("closed") {
            let now = crate::marketdays::open_now(t as i64);
            let lines = crate::marketdays::today_and_tomorrow(t as i64, &self.home_zone());
            return if lines.is_empty() { now } else { format!("{now} {}", lines.join(" ")) };
        }
        let lines = crate::marketdays::today_and_tomorrow(t as i64, &self.home_zone());
        if lines.is_empty() {
            "Nothing on the market's calendar today or tomorrow: ordinary sessions.".into()
        } else {
            lines.join("\n")
        }
    }

    // -- 4. waiting for ---------------------------------------------------------

    fn waiting_now(&mut self, t: u64) -> Vec<crate::waitingfor::Waiting> {
        let book: crate::mailbook::MailBook = self.store.load(crate::mailbook::MailBook::FILE);
        self.waiting_in(&book, t)
    }

    fn waiting_in(&mut self, book: &crate::mailbook::MailBook, t: u64) -> Vec<crate::waitingfor::Waiting> {
        let cfg = self.workday_cfg().waiting_for;
        let offset = self.home_zone().to_local(t as i64) - t as i64;
        let taught = loaded!(self.workday, self.store, taught, TAUGHT);
        crate::waitingfor::open(book, taught, &cfg, t, offset)
    }

    pub(crate) fn wd_waiting_for(&mut self, said: &str, t: u64) -> String {
        let cfg = self.workday_cfg().waiting_for;
        if !cfg.enabled {
            return "The waiting-for list is switched off (workday.waiting_for.enabled).".into();
        }
        let n = self.workday.last_waiting.len();
        if let Some((verb, i)) = numbered_reply(said, &["done", "not a promise", "not a request", "wrong", "close"], n) {
            let w = self.workday.last_waiting[i].clone();
            let taught = loaded!(self.workday, self.store, taught, TAUGHT);
            let right = matches!(verb.as_str(), "done" | "close");
            if right {
                taught.done(&w);
            } else {
                taught.wrong(&w);
            }
            let saved = self.keep(TAUGHT, self.workday.taught.as_ref().expect("loaded"));
            return if right {
                format!("Closed \"{}\".{}", w.subject, saved.unwrap_or_default())
            } else {
                format!("Noted: \"{}\" wasn't one. I'll trust \"{}\" less.{}", w.subject, w.cue, saved.unwrap_or_default())
            };
        }
        let items = self.waiting_now(t);
        if items.is_empty() {
            let book: crate::mailbook::MailBook = self.store.load(crate::mailbook::MailBook::FILE);
            if book.letters.iter().all(|l| !l.mine) {
                return "I haven't read your sent mail yet -- it's read on the next mail check (\"check my mail\"), and only what you sent in the last month.".into();
            }
        }
        let reply = crate::waitingfor::say(&items, t);
        self.workday.last_waiting = items;
        let n = self.workday.last_waiting.len();
        self.workday.set_follow((n > 0).then_some(Follow::Waiting(n)), t);
        reply
    }

    // -- 5. capture, dated and reviewed -------------------------------------------

    /// After a capture: a note that said a sure time is dated.
    pub(crate) fn wd_date_note(&mut self, id: u64, t: u64) {
        let local = self.local(t);
        self.notebook.date_it(id, local);
    }

    pub(crate) fn wd_note_review(&mut self, said: &str, _t: u64) -> String {
        let low = said.to_lowercase();
        let pending: Vec<u64> = self.notebook.to_review().iter().map(|n| n.id).collect();
        if let Some((verb, i)) = numbered_reply(said, &["keep", "drop", "done with", "bin"], pending.len()) {
            let keep = verb == "keep";
            self.notebook.settle(pending[i], keep);
            let _ = self.store.save(crate::capture::Notebook::FILE, &self.notebook);
            return if keep { "Kept.".into() } else { "Dropped.".into() };
        }
        if low.contains("keep all") || low.contains("keep them all") {
            for id in &pending {
                self.notebook.settle(*id, true);
            }
            let _ = self.store.save(crate::capture::Notebook::FILE, &self.notebook);
            return format!("Kept all {}.", pending.len());
        }
        let listed = self.notebook.to_review().len().min(20);
        self.workday.set_follow((listed > 0).then_some(Follow::Review(listed)), _t);
        crate::capture::review_said(&self.notebook.to_review())
    }

    // -- 6. launcher --------------------------------------------------------------

    fn launch_candidates(&mut self, t: u64) -> Vec<crate::launcher::Candidate> {
        use crate::launcher::{Candidate, Kind};
        let mut all: Vec<Candidate> = self.cfg.apps.apps.keys().map(|name| Candidate { kind: Kind::App, label: name.clone(), target: name.clone() }).collect();
        // The Start menu is walked at most once an hour.
        let fresh = self.workday.shortcuts.as_ref().map(|(at, _)| t.saturating_sub(*at) < 3600).unwrap_or(false);
        if !fresh {
            let found = crate::launcher::shortcuts(&crate::launcher::start_menu_dirs());
            self.workday.shortcuts = Some((t, found));
        }
        all.extend(self.workday.shortcuts.as_ref().map(|(_, v)| v.clone()).unwrap_or_default());
        all
    }

    fn launch_one(&mut self, c: crate::launcher::Candidate, t: u64) -> String {
        let store = &self.store;
        loaded!(self.workday, store, uses, USES).picked(&c, t);
        let _ = self.store.save(USES, self.workday.uses.as_ref().expect("loaded"));
        match c.kind {
            crate::launcher::Kind::App => self.execute(&Intent::OpenApp(c.target.clone())),
            _ => match self.plat.open_path(&c.target) {
                Ok(()) => format!("Opening {}.", c.label),
                Err(e) => format!("I couldn't open {}: {e}", c.label),
            },
        }
    }

    pub(crate) fn wd_launch(&mut self, said: &str, t: u64) -> String {
        let n = self.workday.last_launch.len();
        if let Some((_, i)) = numbered_reply(said, &["open", "launch", "start"], n) {
            let c = self.workday.last_launch[i].clone();
            self.workday.set_follow(None, t);
            return self.launch_one(c, t);
        }
        let low = said.to_lowercase();
        let q = ["launch ", "start up ", "run the app ", "fire up ", "open "].iter().find_map(|p| low.strip_prefix(p)).unwrap_or(&low).trim().to_string();
        if q.is_empty() {
            return "Launch what?".into();
        }
        let all = self.launch_candidates(t);
        let uses = loaded!(self.workday, self.store, uses, USES).clone();
        match crate::launcher::pick(&q, &all, &uses, t) {
            crate::launcher::Pick::Open(c) => self.launch_one(c, t),
            crate::launcher::Pick::Choose(list) => {
                let said = crate::launcher::say_choices(&list);
                self.workday.set_follow(Some(Follow::Launch(list.len())), t);
                self.workday.last_launch = list;
                said
            }
            crate::launcher::Pick::Nothing => format!("I can't find an app or shortcut called \"{q}\"."),
        }
    }

    // -- 7. trading-session prompts -------------------------------------------------

    pub(crate) fn wd_trade_day(&mut self, said: &str, t: u64) -> String {
        use crate::tradeday::*;
        let cfg = self.workday_cfg().trade_day;
        if !cfg.enabled {
            return format!("Trading check-ins are switched off (workday.trade_day.enabled).\n{THE_LINE}");
        }
        let today = self.today(t);
        // An answer to the questions just asked.
        if let Some(Follow::Trade(qs)) = self.workday.follow(t) {
            let when = if qs == cfg.before { When::Before } else { When::After };
            let given = if said.trim().eq_ignore_ascii_case("skip") { Some(qs.iter().map(|_| Given::Skipped).collect()) } else { read(&qs, said) };
            if let Some(given) = given {
                let journal = loaded!(self.workday, self.store, journal, JOURNAL);
                journal.record(today, when, t, &qs, given);
                let saved = self.keep(JOURNAL, self.workday.journal.as_ref().expect("loaded"));
                self.workday.set_follow(None, t);
                return format!("Noted.{}\n{THE_LINE}", saved.unwrap_or_default());
            }
        }
        let low = said.to_lowercase();
        if low.contains("how has") || low.contains("summary") || low.contains("how have") || low.contains("my process") {
            let days = if low.contains("month") { 30 } else { 7 };
            let journal = loaded!(self.workday, self.store, journal, JOURNAL);
            return journal.summary(&cfg, today, days);
        }
        let when = if low.contains("after") || low.contains("journal") || low.contains("post") || low.contains("close") { When::After } else { When::Before };
        self.trade_ask(when, t)
    }

    fn trade_ask(&mut self, when: crate::tradeday::When, t: u64) -> String {
        let cfg = self.workday_cfg().trade_day;
        let qs = match when {
            crate::tradeday::When::Before => cfg.before.clone(),
            crate::tradeday::When::After => cfg.after.clone(),
        };
        let schedule = if when == crate::tradeday::When::Before { crate::marketdays::today_and_tomorrow(t as i64, &self.home_zone()) } else { vec![] };
        // Before the open: today's scheduled releases, from the checked
        // event tables (the general market desk -- general trading knowledge only).
        let releases = if when == crate::tradeday::When::Before {
            let off = crate::localclock::offset_secs();
            let from = crate::localclock::midnight(t, off) as i64 * 1000;
            let (y, m, _) = crate::civil::civil_from_days((from + off * 1000) / 86_400_000);
            crate::market::events::month(y as i32, m)
                .ok()
                .and_then(|ev| crate::tradeday::releases_in(&ev, from, from + 86_400_000, off))
                .map(|s| format!("\n{s}"))
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.workday.set_follow(Some(Follow::Trade(qs.clone())), t);
        format!("{}{releases}\n{}", crate::tradeday::ask(&qs, when, &schedule), crate::tradeday::THE_LINE)
    }

    /// Before the open and after the close, on a trading day, once each.
    fn trade_tick(&mut self, t: u64) -> Option<String> {
        use crate::tradeday::When;
        let cfg = self.workday_cfg().trade_day;
        if !cfg.enabled || !cfg.prompt {
            return None;
        }
        let ny = crate::tz::Zone::named("America/New_York").unwrap_or_else(crate::tz::Zone::utc);
        let local = ny.to_local(t as i64);
        let (y, m, d) = crate::market::time::civil_from_days(local.div_euclid(86_400));
        let close = match crate::marketdays::day(y, m, d) {
            crate::marketdays::Day::Open => 16 * 60,
            crate::marketdays::Day::EarlyClose(_) => 13 * 60,
            _ => return None,
        };
        let mins = (local.rem_euclid(86_400) / 60) as u64;
        let open = 9 * 60 + 30;
        let when = if mins + cfg.before_minutes >= open && mins < open {
            When::Before
        } else if mins >= close + cfg.after_minutes && mins < close + cfg.after_minutes + 60 {
            When::After
        } else {
            return None;
        };
        let today = self.today(t);
        if self.workday.trade_asked == Some((today, when)) {
            return None;
        }
        self.workday.trade_asked = Some((today, when));
        if loaded!(self.workday, self.store, journal, JOURNAL).has(today, when) {
            return None;
        }
        Some(self.trade_ask(when, t))
    }

    // -- 8. meeting prep -----------------------------------------------------------

    fn prep_for(&mut self, ev: &crate::calendar::Event, t: u64) -> (Vec<String>, bool) {
        let cfg = self.workday_cfg();
        let book: crate::mailbook::MailBook = self.store.load(crate::mailbook::MailBook::FILE);
        let waiting = if cfg.waiting_for.enabled { self.waiting_in(&book, t) } else { vec![] };
        let note = ev.note.clone().unwrap_or_default();
        let named = !crate::meetprep::people_in(&ev.title, &note).is_empty();
        (crate::meetprep::prepare(&ev.title, &note, &book, &self.notebook.notes, &waiting, cfg.waiting_for.look_back_days, t), named)
    }

    pub(crate) fn wd_meeting_prep(&mut self, _said: &str, t: u64) -> String {
        let next = self.calendar.occurrences_between(t, t + 12 * 3600).into_iter().find(|e| !e.all_day);
        let Some(ev) = next else { return "Nothing on your calendar in the next twelve hours.".into() };
        let (lines, _) = self.prep_for(&ev, t);
        crate::meetprep::said(&ev.title, ev.start.saturating_sub(t) / 60, &lines)
    }

    fn prep_tick(&mut self, t: u64) -> Option<String> {
        let mins = self.workday_cfg().meeting_prep_minutes;
        if mins == 0 {
            return None;
        }
        self.workday.prepped.retain(|(_, start)| *start > t);
        let soon = self.calendar.occurrences_between(t, t + mins * 60).into_iter().find(|e| !e.all_day && !self.workday.prepped.contains(&(e.id, e.start)))?;
        self.workday.prepped.insert((soon.id, soon.start));
        let (lines, named) = self.prep_for(&soon, t);
        // Nobody named, nothing to prepare: not worth an interruption.
        named.then(|| crate::meetprep::said(&soon.title, soon.start.saturating_sub(t) / 60, &lines))
    }

    // -- 9. snippets -----------------------------------------------------------------

    pub(crate) fn wd_snippet(&mut self, said: &str, t: u64) -> String {
        let low = said.to_lowercase();
        if let Some((trigger, text)) = crate::snippets::read_save(said) {
            let s = loaded!(self.workday, self.store, snippets, SNIPPETS);
            return match s.save(&trigger, &text) {
                Ok(()) => {
                    let saved = self.keep(SNIPPETS, self.workday.snippets.as_ref().expect("loaded"));
                    format!("Saved \"{trigger}\".{}", saved.unwrap_or_default())
                }
                Err(crate::snippets::Refused::Secret(kinds)) => format!("That holds what looks like a {} -- keep it in the vault, not a snippet.", kinds.join(" and a ")),
                Err(crate::snippets::Refused::TooLong) => format!("That's over {} characters; a snippet is for text you type often.", crate::snippets::MAX_CHARS),
                Err(crate::snippets::Refused::Empty) => "Say it as \"save snippet ;sig as Best, Sam\".".into(),
            };
        }
        for lead in ["delete snippet ", "forget snippet ", "remove snippet "] {
            if let Some(rest) = low.strip_prefix(lead) {
                let s = loaded!(self.workday, self.store, snippets, SNIPPETS);
                return if s.remove(rest) {
                    let _ = self.store.save(SNIPPETS, self.workday.snippets.as_ref().expect("loaded"));
                    format!("Forgot \"{rest}\".")
                } else {
                    format!("There's no snippet \"{rest}\".")
                };
            }
        }
        let s = loaded!(self.workday, self.store, snippets, SNIPPETS).clone();
        let asked = ["type my ", "insert my ", "type snippet ", "insert snippet ", "paste my ", "type "].iter().find_map(|p| low.strip_prefix(p));
        match asked {
            Some(name) if !name.trim().is_empty() && !matches!(name.trim(), "snippets" | "snippet") => {
                let Some((trigger, text)) = s.get(name) else { return format!("There's no snippet \"{}\".", name.trim()) };
                let app = self.plat.active_window().ok().flatten().map(|w| w.process).unwrap_or_default();
                if !crate::dictate::may_type_into(&app, &self.tools_ref().map(|t| t.dictate.clone()).unwrap_or_default()) {
                    return format!("I won't type into {app} -- it's on your never-type list.");
                }
                match self.plat.type_text(&crate::snippets::fill(text, self.local(t))) {
                    Ok(()) => format!("Typed \"{trigger}\"."),
                    Err(e) => format!("I couldn't type it: {e}"),
                }
            }
            _ => {
                if s.by_trigger.is_empty() {
                    "No snippets yet. \"Save snippet ;sig as Best, Sam\" makes one.".into()
                } else {
                    format!("Your snippets: {}.", s.by_trigger.keys().cloned().collect::<Vec<_>>().join(", "))
                }
            }
        }
    }

    // -- 10. find any file -------------------------------------------------------------

    /// The files the last search listed, in the order they were said.
    pub(crate) fn files_last_listed(&self) -> &[String] {
        &self.workday.last_files
    }

    pub(crate) fn wd_find_file(&mut self, said: &str, t: u64) -> String {
        // A moment for the list on disk, if it's still being read at start.
        self.wait_for_index(std::time::Duration::from_secs(2));
        let n = self.workday.last_files.len();
        if let Some(i) = numbered_reply(said, &["open", "show"], n).map(|x| x.1).or_else(|| crate::findfile::which(said, n)) {
            let path = self.workday.last_files[i].clone();
            return match self.plat.open_path(&path) {
                Ok(()) => format!("Opening {}.", crate::findfile::short_path(&path)),
                Err(e) => format!("I couldn't open it: {e}"),
            };
        }
        let local = self.local(t);
        let day_start = self.home_zone().to_utc((local - local % 86_400) as i64).max(0) as u64;
        let q = ["find the file ", "find my file ", "find file ", "find files ", "where is my file ", "where's my file ", "find "].iter().find_map(|p| said.to_lowercase().strip_prefix(p).map(|s| s.to_string())).unwrap_or_else(|| said.to_string());
        let asked = crate::findfile::read(&q, t, day_start);
        let mut found: Vec<&crate::index::Entry> = if asked.words.is_empty() {
            crate::findfile::by_filter(self.index.entries.values(), &asked, 8)
        } else {
            // Every word must be in the name; the index ranks the first.
            let mut hits: Vec<&crate::index::Entry> = self.index.search(&asked.words[0]).into_iter().filter(|e| crate::findfile::passes(e, &asked)).collect();
            hits.retain(|e| {
                let n = e.name.to_lowercase();
                asked.words.iter().all(|w| n.contains(w.as_str()))
            });
            hits.truncate(8);
            hits
        };
        let mut closest = false;
        if found.is_empty() && !asked.words.is_empty() {
            found = crate::findfile::near(self.index.entries.values(), &asked, 5);
            closest = true;
        }
        if found.is_empty() {
            return if self.index_still_loading() {
                crate::index::STILL_READING.into()
            } else if self.index.entries.is_empty() {
                "Your file index is empty -- \"rebuild the index\" reads the folders you've listed in settings.".into()
            } else {
                "Nothing in your indexed folders matches that.".into()
            };
        }
        let paths: Vec<String> = found.iter().map(|e| e.path.clone()).collect();
        let head = if closest { "Nothing exact; closest:" } else { "Found:" };
        let reply = format!("{head} {}\n\"Open 1\" opens one.", crate::findfile::numbered(&paths));
        self.workday.set_follow(Some(Follow::Files(paths.len())), t);
        self.workday.last_files = paths;
        reply
    }

    // -- 11. PDF tools --------------------------------------------------------------------

    pub(crate) fn wd_pdf(&mut self, said: &str, t: u64) -> String {
        let low = said.to_lowercase();
        let listed: Vec<String> = self.workday.last_files.iter().filter(|p| p.to_lowercase().ends_with(".pdf")).cloned().collect();
        if self.workday.last_files.is_empty() {
            return "Find the PDFs first (\"find the pdf from yesterday\"), then \"merge 1 and 2\", \"extract pages 2-4 from 1\" or \"sign 1\".".into();
        }
        // Which files: the numbers said, in order, from the last list.
        let nums: Vec<usize> = low
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter_map(|w| w.parse::<usize>().ok())
            .collect();
        let pick = |k: usize| self.workday.last_files.get(k.wrapping_sub(1)).filter(|p| p.to_lowercase().ends_with(".pdf")).cloned();
        let read = |p: &str| -> Result<crate::pdfkit::Doc, String> {
            let meta = std::fs::metadata(p).map_err(|e| format!("{p}: {e}"))?;
            if meta.len() as usize > crate::pdfkit::MAX_BYTES {
                return Err(format!("{} is too big for me to rewrite", crate::findfile::short_path(p)));
            }
            let bytes = std::fs::read(p).map_err(|e| format!("{p}: {e}"))?;
            crate::pdfkit::Doc::parse(&bytes).map_err(|e| format!("{}: {e}", crate::findfile::short_path(p)))
        };
        let write_out = |first: &str, what: &str, bytes: Vec<u8>, pages: usize| -> String {
            if let Err(e) = crate::pdfkit::check_written(&bytes, pages) {
                return format!("I built it but it didn't check out ({e}), so I didn't save it.");
            }
            let p = std::path::Path::new(first);
            let dir = p.parent().unwrap_or(std::path::Path::new("."));
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("document");
            let out = unused_name(dir, stem, what, "pdf");
            match std::fs::write(&out, &bytes) {
                Ok(()) => format!("Saved {} ({}).", crate::findfile::short_path(&out.to_string_lossy()), plural(pages, "page")),
                Err(e) => format!("I couldn't save it: {e}"),
            }
        };
        if low.starts_with("merge") || low.starts_with("combine") || low.starts_with("join") {
            let files: Vec<String> = if nums.is_empty() { listed.clone() } else { nums.iter().filter_map(|n| pick(*n)).collect() };
            if files.len() < 2 {
                return "Merging needs two PDFs or more from the list.".into();
            }
            let docs: Result<Vec<crate::pdfkit::Doc>, String> = files.iter().map(|f| read(f)).collect();
            let docs = match docs { Ok(d) => d, Err(e) => return e };
            let refs: Vec<&crate::pdfkit::Doc> = docs.iter().collect();
            let pages: Vec<crate::pdfkit::PageRef> = docs.iter().enumerate().flat_map(|(di, d)| (0..d.pages.len()).map(move |p| crate::pdfkit::PageRef { doc: di, page: p })).collect();
            let n = pages.len();
            return match crate::pdfkit::write(&refs, &pages, None) {
                Ok(b) => write_out(&files[0], "merged", b, n),
                Err(e) => format!("I couldn't merge them: {e}"),
            };
        }
        // One file: "extract pages 2-5 from 1", "sign 1", "how many pages in 1".
        let (file_no, spec) = match low.find(" from ").or_else(|| low.find(" of ")) {
            Some(i) => (low[i..].split(|c: char| !c.is_ascii_digit()).find_map(|w| w.parse::<usize>().ok()), low[..i].to_string()),
            None => (nums.last().copied(), low.clone()),
        };
        let file = match file_no.and_then(pick).or_else(|| (listed.len() == 1).then(|| listed[0].clone())) {
            Some(f) => f,
            None => return "Which PDF? Say its number from the list.".into(),
        };
        let d = match read(&file) { Ok(d) => d, Err(e) => return e };
        let count = d.pages.len();
        if low.starts_with("extract") || low.starts_with("split") || low.contains("pages ") && !low.starts_with("sign") && !low.starts_with("how many") {
            let range = spec.split("pages").nth(1).or_else(|| spec.split("page").nth(1)).unwrap_or("").trim().to_string();
            let idx = match crate::pdfkit::ranges(&range, count) { Ok(i) if !i.is_empty() => i, Ok(_) => return "Which pages?".into(), Err(e) => return e };
            let pages: Vec<crate::pdfkit::PageRef> = idx.iter().map(|p| crate::pdfkit::PageRef { doc: 0, page: *p }).collect();
            let n = pages.len();
            return match crate::pdfkit::write(&[&d], &pages, None) {
                Ok(b) => write_out(&file, &format!("pages {}", range.replace(' ', "")), b, n),
                Err(e) => format!("I couldn't do that: {e}"),
            };
        }
        if low.starts_with("sign") || low.starts_with("stamp") {
            let sig = self.workday_cfg().signature_png;
            if sig.is_empty() {
                return "Set workday.signature_png to a PNG of your signature (a transparent background looks best), and I'll put it on.".into();
            }
            let img = match std::fs::read(&sig).map_err(|e| e.to_string()).and_then(|b| crate::pngcodec::read_png(&b)) { Ok(i) => i, Err(e) => return format!("Your signature image: {e}") };
            let page = spec.split("page ").nth(1).and_then(|w| w.split_whitespace().next()).and_then(|w| w.parse::<usize>().ok()).map(|p| p.saturating_sub(1)).unwrap_or(count - 1).min(count - 1);
            let (w, _) = crate::pdfkit::page_size(&d, page).unwrap_or((612.0, 792.0));
            let width = 150.0f64.min(w / 3.0);
            let stamp = match crate::pdfkit::stamp(0, page, &img.pixels, img.width, img.height, w - width - 54.0, 54.0, width) { Ok(s) => s, Err(e) => return e };
            let pages: Vec<crate::pdfkit::PageRef> = (0..count).map(|p| crate::pdfkit::PageRef { doc: 0, page: p }).collect();
            return match crate::pdfkit::write(&[&d], &pages, Some(stamp)) {
                Ok(b) => write_out(&file, "signed", b, count),
                Err(e) => format!("I couldn't sign it: {e}"),
            };
        }
        let _ = t;
        format!("{} has {}.", crate::findfile::short_path(&file), plural(count, "page"))
    }

    // -- 12. people -----------------------------------------------------------------------

    /// Who you've told Atlas about, as kept here (the same copy "Sam's
    /// email is ..." changes): for addressing an email by name.
    pub(crate) fn people_known(&mut self) -> &crate::people::People {
        loaded!(self.workday, self.store, people, PEOPLE)
    }

    pub(crate) fn wd_people(&mut self, said: &str, t: u64) -> String {
        use crate::people::{Asked, Refused};
        let book: crate::mailbook::MailBook = self.store.load(crate::mailbook::MailBook::FILE);
        let local = self.local(t);
        let Some(asked) = crate::people::read(said) else {
            let p = loaded!(self.workday, self.store, people, PEOPLE);
            return crate::people::due_said(&p.due(&book, t), &p.birthdays(local, 7));
        };
        let p = loaded!(self.workday, self.store, people, PEOPLE);
        let done = match asked {
            Asked::Due => return crate::people::due_said(&p.due(&book, t), &p.birthdays(local, 7)),
            Asked::About { who } => return p.what_i_know(&who, &book, t),
            Asked::Note { who, text } => p.note(&who, &text, t).map(|_| format!("Noted about {who}.")),
            Asked::Every { who, days: Some(d) } => p.every(&who, Some(d)).map(|_| format!("I'll mention {who} when it's been {d} days.")),
            Asked::Every { who, days: None } => p.every(&who, None).map(|_| format!("I'll stop mentioning {who}.")),
            Asked::Talked { who } => p.talked(&who, t).map(|_| format!("Noted -- in touch with {who} today.")),
            Asked::Birthday { who, month, day } => p.birthday(&who, month, day).map(|_| format!("{who}'s birthday noted.")),
            Asked::Email { who, address } => p.email(&who, &address).map(|_| format!("Noted -- mail with {address} counts as being in touch with {who}.")),
            Asked::Phone { who, number } => p.phone(&who, &number).map(|_| format!("Noted -- {who}'s number. \"Text {who} saying\" and what to say, and I'll write it.")),
        };
        match done {
            Ok(said) => {
                let saved = self.keep(PEOPLE, self.workday.people.as_ref().expect("loaded"));
                format!("{said}{}", saved.unwrap_or_default())
            }
            Err(Refused::Which(names)) => format!("Which one -- {}? Say the full name.", names.join(" or ")),
            Err(Refused::Secret(kinds)) => format!("That holds what looks like a {} -- I won't keep it in a note about someone.", kinds.join(" and a ")),
            Err(Refused::Full) => "Your people list is full.".into(),
            Err(Refused::Empty) => "I didn't catch who, or what.".into(),
        }
    }

    // -- 13. feeds -------------------------------------------------------------------------

    fn fetch_url(url: &str) -> Result<(String, String), String> {
        let get = |https: bool, host: &str, path: &str| {
            let timeout = std::time::Duration::from_secs(10);
            if https { crate::http::https_get(host, path, timeout) } else { crate::http::get(host, path, timeout) }
        };
        crate::feeds::fetch(url, &get)
    }

    pub(crate) fn wd_feeds(&mut self, said: &str, t: u64) -> String {
        let cfg = self.workday_cfg().feeds;
        if !cfg.enabled {
            return "Following sites is switched off (workday.feeds.enabled).".into();
        }
        let low = said.to_lowercase();
        let n = self.workday.last_feeds.len();
        if let Some((verb, i)) = numbered_reply(said, &["read", "open", "save", "skip", "keep"], n) {
            let item = self.workday.last_feeds[i].clone();
            let feeds = loaded!(self.workday, self.store, feeds, FEEDS);
            feeds.unread.retain(|u| u.link != item.link);
            let _ = self.store.save(FEEDS, self.workday.feeds.as_ref().expect("loaded"));
            return match verb.as_str() {
                "skip" => "Skipped.".into(),
                "save" | "keep" => match self.tray.hand(&item.link, &crate::earned::Space::Personal, "feeds", t) {
                    Ok(_) => {
                        let _ = self.tray.save(&self.store);
                        format!("Saved for later: {}", item.title)
                    }
                    Err(e) => e,
                },
                "open" => match self.plat.open_path(&item.link) {
                    Ok(()) => format!("Opening {}.", item.title),
                    Err(e) => format!("I couldn't open it: {e}"),
                },
                _ => match Self::fetch_url(&item.link) {
                    Ok((_, html)) => {
                        let a = crate::readable::extract(&html);
                        let text: String = a.text.chars().take(2400).collect();
                        format!("{}\n\n{}{}", if a.title.is_empty() { &item.title } else { &a.title }, text, if a.text.chars().count() > 2400 { "…" } else { "" })
                    }
                    Err(e) => format!("I couldn't fetch it: {e}"),
                },
            };
        }
        for lead in ["unfollow ", "stop following "] {
            if let Some(rest) = low.strip_prefix(lead) {
                let feeds = loaded!(self.workday, self.store, feeds, FEEDS);
                return match feeds.unfollow(rest) {
                    Some(name) => {
                        let _ = self.store.save(FEEDS, self.workday.feeds.as_ref().expect("loaded"));
                        format!("Stopped following {name}.")
                    }
                    None => format!("I'm not following anything matching \"{}\" -- or more than one thing does.", rest.trim()),
                };
            }
        }
        let follow = ["follow the site ", "follow the blog ", "subscribe to ", "follow "].iter().find_map(|p| low.strip_prefix(p));
        if let Some(rest) = follow {
            let mut url = rest.trim().trim_end_matches('.').to_string();
            if !url.starts_with("http") {
                url = format!("https://{url}");
            }
            let (final_url, body) = match Self::fetch_url(&url) { Ok(x) => x, Err(e) => return format!("I couldn't reach it: {e}") };
            let (feed_url, parsed) = match crate::feeds::parse(&body) {
                Ok(p) => (final_url, p),
                Err(_) => {
                    let Some(found) = crate::feeds::discover(&body, &final_url).into_iter().next() else {
                        return "That page doesn't say where its feed is. If you have the feed's own address, give me that.".into();
                    };
                    match Self::fetch_url(&found).and_then(|(u, b)| crate::feeds::parse(&b).map(|p| (u, p))) {
                        Ok(x) => x,
                        Err(e) => return format!("Its feed didn't read: {e}"),
                    }
                }
            };
            let title = parsed.title.clone();
            let feeds = loaded!(self.workday, self.store, feeds, FEEDS);
            return match feeds.follow(&feed_url, &title) {
                Ok(true) => {
                    let i = feeds.feeds.len() - 1;
                    let got = feeds.took(i, parsed, &cfg, t);
                    let _ = self.store.save(FEEDS, self.workday.feeds.as_ref().expect("loaded"));
                    format!("Following {}. {} to start with; after that, only what's new.", if title.is_empty() { &feed_url } else { &title }, plural(got, "item"))
                }
                Ok(false) => "You already follow that.".into(),
                Err(e) => e,
            };
        }
        let feeds = loaded!(self.workday, self.store, feeds, FEEDS);
        if low.contains("what am i following") || low.contains("my feeds list") || low.contains("list my feeds") {
            if feeds.feeds.is_empty() {
                return "You're not following anything yet.".into();
            }
            return feeds.feeds.iter().enumerate().map(|(i, f)| format!("{}. {}", i + 1, if f.title.is_empty() { &f.url } else { &f.title })).collect::<Vec<_>>().join("\n");
        }
        let reply = feeds.said(10);
        self.workday.last_feeds = feeds.unread.iter().take(10).cloned().collect();
        let n = self.workday.last_feeds.len();
        self.workday.set_follow((n > 0).then_some(Follow::Feeds(n)), t);
        if n > 0 {
            format!("{reply}\n\"Read 1\", \"save 2\" for later, or \"skip 3\".")
        } else {
            reply
        }
    }

    /// One feed at a time, off the tick thread; its result is taken on a
    /// later tick.
    fn feeds_tick(&mut self, t: u64, online: bool) {
        let cfg = self.workday_cfg().feeds;
        if !cfg.enabled {
            return;
        }
        if let Some(rx) = &self.workday.feed_in_flight {
            match rx.try_recv() {
                Ok((url, result)) => {
                    self.workday.feed_in_flight = None;
                    let feeds = loaded!(self.workday, self.store, feeds, FEEDS);
                    if let Some(i) = feeds.feeds.iter().position(|f| f.url == url) {
                        match result {
                            Ok(p) => {
                                feeds.took(i, p, &cfg, t);
                            }
                            Err(e) => feeds.failed(i, &e, &cfg, t),
                        }
                        let _ = self.store.save(FEEDS, self.workday.feeds.as_ref().expect("loaded"));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.workday.feed_in_flight = None,
            }
        }
        if !online || self.workday.feed_in_flight.is_some() {
            return;
        }
        // Loaded only if there's a feeds file at all: a daemon that follows
        // nothing never reads one.
        let feeds = loaded!(self.workday, self.store, feeds, FEEDS);
        let Some(i) = feeds.due(t).into_iter().next() else { return };
        let url = feeds.feeds[i].url.clone();
        // Marked due later now, so a slow read isn't started twice.
        feeds.feeds[i].next_due = t + cfg.every_minutes.max(15) * 60;
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("atlas-feed".into()).spawn(move || {
            let r = Self::fetch_url(&url).and_then(|(_, body)| crate::feeds::parse(&body));
            let _ = tx.send((url, r));
        });
        if spawned.is_ok() {
            self.workday.feed_in_flight = Some(rx);
        }
    }

    // -- 14. receipts -------------------------------------------------------------------------

    pub(crate) fn wd_receipt(&mut self, said: &str, t: u64) -> String {
        use crate::receipts::Total;
        let low = said.to_lowercase();
        // The answer to "probably $7.75 -- keep it?".
        if let Some((reading, text)) = self.workday.pending_receipt.clone() {
            let amount = crate::receipts::amounts(said);
            let yes = low.starts_with("yes") || low.starts_with("keep");
            if low.starts_with("no") || low.starts_with("don't") {
                self.workday.pending_receipt = None;
                self.workday.set_follow(None, t);
                return "Not kept.".into();
            }
            let total = match (&reading.total, amount.first()) {
                (_, Some(a)) => Some(*a),
                (Total::Probably(p), None) if yes => Some(*p),
                (Total::Disagrees { total, .. }, None) if yes => Some(*total),
                _ => None,
            };
            if let Some(total) = total {
                self.workday.pending_receipt = None;
                self.workday.set_follow(None, t);
                let r = loaded!(self.workday, self.store, receipts, RECEIPTS);
                let kept = r.keep(reading.clone(), total, true, &text, t);
                let _ = self.store.save(RECEIPTS, self.workday.receipts.as_ref().expect("loaded"));
                return if kept { format!("Kept: {} {}.", reading.merchant, crate::receipts::money(total, &reading.currency)) } else { "I already have that one.".into() };
            }
        }
        // Questions about what's kept.
        if low.contains("spend") || low.contains("spent") || low.starts_with("receipts for") || low.starts_with("my receipts") || low.starts_with("show my receipts") {
            let today = self.today(t);
            let c = crate::civil::Civil::from_local(today * 86_400);
            let month_start = crate::civil::days_from_civil(c.year, c.month, 1);
            let (from, to) = if low.contains("last month") {
                let (y, m) = if c.month == 1 { (c.year - 1, 12) } else { (c.year, c.month - 1) };
                (Some(crate::civil::days_from_civil(y, m, 1)), Some(month_start - 1))
            } else if low.contains("this month") {
                (Some(month_start), Some(today))
            } else if low.contains("this year") {
                (Some(crate::civil::days_from_civil(c.year, 1, 1)), Some(today))
            } else {
                (None, None)
            };
            let words: String = low
                .split_whitespace()
                .filter(|w| !["what", "did", "i", "spend", "spent", "at", "on", "this", "last", "month", "year", "my", "receipts", "for", "show", "how", "much", "have", "in", "total"].contains(w))
                .collect::<Vec<_>>()
                .join(" ");
            let r = loaded!(self.workday, self.store, receipts, RECEIPTS);
            let found = r.find(&words, from, to);
            return crate::receipts::Receipts::summed(&found);
        }
        // Keep one: from the clipboard if you copied a receipt's text, else
        // off the window in front.
        let clip = self.plat.read_clipboard().ok().flatten().filter(|c| c.to_lowercase().contains("total") && !crate::receipts::amounts(c).is_empty() && !low.contains("screen"));
        let text = match clip {
            Some(c) => c,
            None => match self.screen_words() {
                Ok((text, _, _)) => text,
                Err(e) => return e,
            },
        };
        let reading = crate::receipts::read(&text);
        let said_back = reading.said();
        match reading.total {
            Total::Labelled(total) => {
                let r = loaded!(self.workday, self.store, receipts, RECEIPTS);
                if !r.keep(reading, total, false, &text, t) {
                    return "I already have that one.".into();
                }
                let saved = self.keep(RECEIPTS, self.workday.receipts.as_ref().expect("loaded"));
                format!("{said_back}{}", saved.unwrap_or_default())
            }
            Total::None => said_back,
            _ => {
                self.workday.pending_receipt = Some((reading, text));
                self.workday.set_follow(Some(Follow::Receipt), t);
                said_back
            }
        }
    }

    // -- 15. habits --------------------------------------------------------------------------

    pub(crate) fn wd_habit(&mut self, said: &str, t: u64) -> String {
        use crate::habits::{Asked, Refused};
        let today = self.today(t);
        let low = said.to_lowercase();
        if low.starts_with("pause my habits") || low.starts_with("pause habits") {
            let local = self.local(t);
            let until = crate::when::parse(said, local).filter(|p| p.day_said).map(|p| (p.start / 86_400) as i64);
            let days = low.split_whitespace().find_map(|w| w.parse::<i64>().ok()).filter(|_| low.contains("day"));
            let to = until.or(days.map(|d| today + d - 1)).unwrap_or(today);
            let h = loaded!(self.workday, self.store, habits, HABITS);
            let _ = h.pause(None, today, to);
            let _ = self.store.save(HABITS, self.workday.habits.as_ref().expect("loaded"));
            return format!("Habits paused through {} -- those days won't count against them.", if to == today { "today".to_string() } else { format!("{} days from now", to - today) });
        }
        let h = loaded!(self.workday, self.store, habits, HABITS);
        let r = match crate::habits::read(said) {
            Some(Asked::Add { name, times, days }) => h.add(&name, times, days, today).map(|_| {
                let quiet = if crate::nudge::is_medical(&name) { " I'll track it, but I won't bring it up -- body numbers are yours to raise." } else { "" };
                format!("Tracking \"{name}\", {}.{quiet}", if days == 1 { "daily".to_string() } else { format!("{times} in {days} days") })
            }),
            Some(Asked::Did { name }) => h.did(&name, today).map(|n| {
                let habit = h.habits.iter().find(|x| x.name == n).expect("just done");
                let streak = habit.streak(today);
                format!("{n}: done.{}", if habit.days == 1 && streak >= 2 { format!(" {streak} days running.") } else { String::new() })
            }),
            Some(Asked::Undo { name }) => h.undo(&name, today).map(|n| format!("{n}: unmarked for today.")),
            Some(Asked::Remove { name }) => h.remove(&name).map(|n| format!("Stopped tracking {n}.")),
            Some(Asked::Show) | None => return h.said(today),
        };
        match r {
            Ok(s) => {
                let saved = self.keep(HABITS, self.workday.habits.as_ref().expect("loaded"));
                format!("{s}{}", saved.unwrap_or_default())
            }
            Err(Refused::NotFound) => "I'm not tracking a habit by that name. \"How are my habits\" lists them.".into(),
            Err(Refused::Which(names)) => format!("Which one -- {}?", names.join(" or ")),
            Err(Refused::Exists) => "You're already tracking that.".into(),
            Err(Refused::Full) => format!("That's {} habits already.", crate::habits::MAX_HABITS),
            Err(Refused::Empty) => "A habit needs a name.".into(),
        }
    }

    // -- 16. cards -----------------------------------------------------------------------------

    pub(crate) fn wd_cards(&mut self, said: &str, t: u64) -> String {
        let cfg = self.workday_cfg().cards;
        let today = self.today(t);
        let low = said.trim().trim_end_matches(['.', '!']).to_lowercase();
        let quizzing = self.workday.follow(t) == Some(Follow::Quiz);
        let deck = loaded!(self.workday, self.store, deck, DECK);
        if let Some((front, back)) = crate::srs::read_card(said) {
            return match deck.add(&front, &back, "", today) {
                Ok(()) => {
                    let saved = self.keep(DECK, self.workday.deck.as_ref().expect("loaded"));
                    format!("Card made. It comes up in your next \"quiz me\".{}", saved.unwrap_or_default())
                }
                Err(crate::srs::Refused::Secret(k)) => format!("That holds what looks like a {} -- not something to put on a flashcard.", k.join(" and a ")),
                Err(crate::srs::Refused::Exists) => "You already have a card with that front.".into(),
                Err(crate::srs::Refused::Full) => "Your deck is full.".into(),
                Err(crate::srs::Refused::Empty) => "Say it as \"make a card: front | back\".".into(),
            };
        }
        if quizzing && matches!(low.as_str(), "stop" | "that's enough" | "done") {
            deck.asking = None;
            self.workday.follow = None;
            return format!("Stopped. {} still due today.", plural(deck.due(today, usize::MAX).len(), "card"));
        }
        if quizzing && matches!(low.as_str(), "show" | "show me" | "flip" | "answer") {
            return match deck.show() {
                Some(b) => format!("{b}\nHow well did you know it -- again, hard, good or easy?"),
                None => "There's no card up.".into(),
            };
        }
        if let (true, Some(g)) = (quizzing, crate::srs::Grade::read(said)) {
            let graded = deck.grade(g, today, &cfg);
            let next = deck.next(today, &cfg);
            let saved = self.keep(DECK, self.workday.deck.as_ref().expect("loaded"));
            let head = match graded {
                Some((back, days)) => format!("({back}) Back in {}.", plural(days as usize, "day")),
                None => String::new(),
            };
            return match next {
                Some(q) => format!("{head}\n{q}{}", saved.unwrap_or_default()),
                None => {
                    self.workday.follow = None;
                    format!("{head}\nThat's all that's due.{}", saved.unwrap_or_default())
                }
            };
        }
        if low.contains("how many") {
            return format!("{} due today, of {}.", plural(deck.due(today, usize::MAX).len(), "card"), deck.cards.len());
        }
        // "Quiz me".
        match deck.next(today, &cfg) {
            Some(q) => {
                self.workday.follow = Some((Follow::Quiz, t));
                q
            }
            None if deck.cards.is_empty() => "No cards yet. \"Make a card: front | back\" starts a deck.".into(),
            None => "Nothing's due -- you're up to date.".into(),
        }
    }

    // -- 17. translation -----------------------------------------------------------------------

    pub(crate) fn wd_translate(&mut self, said: &str, _t: u64) -> String {
        let cfg = self.workday_cfg().translate;
        if !cfg.enabled {
            return "Translation is switched off (workday.translate.enabled).".into();
        }
        let Some((to, inline)) = crate::translation::read(said) else { return "Translate into which language?".into() };
        let text = match inline {
            Some(t) => t,
            None => match self.plat.read_clipboard() {
                Ok(Some(c)) if !c.trim().is_empty() => c,
                _ => return "Copy the text first, then say \"translate this into …\".".into(),
            },
        };
        let Some(llm) = self.llm.clone() else { return "Translation runs on your local model, and none is set up.".into() };
        // The source is assumed to be English when the target isn't and the
        // text is plain Latin letters; otherwise the back-check is skipped
        // rather than done against a guessed language.
        let latin = text.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_ascii());
        let from = if to != "English" && latin { "English" } else { "" };
        match crate::translation::translate(llm.as_ref(), &text, to, from, &cfg) {
            Ok(done) => {
                let reply_to_clip = self.tools_ref().map(|t| t.clipboard.reply_to_clipboard).unwrap_or(false);
                let mut out = done.said(to);
                if reply_to_clip && done.issues.is_empty() && self.plat.write_clipboard(&done.text).is_ok() {
                    out.push_str("\n(On your clipboard.)");
                }
                out
            }
            Err(e) => format!("The model couldn't translate it: {e}"),
        }
    }

    // ---------------------------------------------------------------- the tick

    /// What these tools do on their own. `may_speak` is whether Atlas may
    /// interrupt now; `paused` whether you've asked it to stand down.
    pub(crate) fn workday_tick(&mut self, t: u64, may_speak: bool, paused: bool, online: bool) -> Vec<String> {
        let mut out = Vec::new();
        // Chords are you pressing a key: answered even when Atlas may not
        // speak up on its own.
        out.extend(self.chords_tick(t));
        if paused {
            return out;
        }
        self.clip_tick(t);
        self.feeds_tick(t, online);
        self.social_tick(t, online);
        crate::hunting::tick(self, t, online);
        // Calendars connected by their link, read again every 15 minutes.
        if online {
            crate::connecting::tick(self, t);
        }
        // The calendar and the market clock are looked at once a minute.
        if t.saturating_sub(self.workday.last_look) < 60 {
            return out;
        }
        self.workday.last_look = t;
        if may_speak {
            if let Some(s) = self.prep_tick(t) {
                out.push(s);
            }
            if let Some(s) = self.trade_tick(t) {
                out.push(s);
            }
        }
        out
    }

    fn chords_tick(&mut self, t: u64) -> Vec<String> {
        let cfg = self.workday_cfg().chords;
        if !cfg.enabled {
            return vec![];
        }
        if self.workday.chords.is_none() {
            let (ok, bad) = cfg.chords();
            let (tx, rx) = std::sync::mpsc::channel();
            self.workday.chords_failed = crate::chords::start_chords(ok, tx);
            self.workday.chords = Some(rx);
            let mut said: Vec<String> = bad;
            if !self.workday.chords_failed.is_empty() {
                said.push(format!("{} of your key chords couldn't be registered -- another program has them.", self.workday.chords_failed.len()));
            }
            return said;
        }
        let pressed: Vec<crate::chords::Does> = self.workday.chords.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        let mut out = Vec::new();
        for does in pressed {
            match does {
                crate::chords::Does::Expand => {
                    let s = loaded!(self.workday, self.store, snippets, SNIPPETS).clone();
                    if let Err(e) = crate::chords::expand(self.plat, &s, self.local(t)) {
                        out.push(format!("Snippet expansion failed: {e}"));
                    }
                }
                crate::chords::Does::CopyText => out.push(self.wd_screen_text("", t)),
                crate::chords::Does::Capture => out.push(self.capture_selection(t)),
            }
        }
        out
    }

    /// The capture chord: whatever's selected becomes a note, in one press.
    fn capture_selection(&mut self, t: u64) -> String {
        let before = self.plat.read_clipboard().ok().flatten();
        let _ = self.plat.write_clipboard("\u{2063}");
        let _ = self.plat.press("ctrl+c");
        self.plat.sleep_ms(80);
        let got = self.plat.read_clipboard().ok().flatten().filter(|g| g != "\u{2063}" && !g.trim().is_empty());
        let _ = self.plat.write_clipboard(before.as_deref().unwrap_or(""));
        match got {
            Some(text) => self.execute(&Intent::Capture(text)),
            None => {
                let _ = t;
                "Nothing was selected to capture.".into()
            }
        }
    }

    // ---------------------------------------------------------------- the brief

    /// Your day, as brief items: what these tools know needs you today.
    pub(crate) fn day_items(&mut self, t: u64) -> Vec<crate::brief::Item> {
        use crate::brief::{Item, Outcome, Source, Weight};
        let item = |id: String, from: &str, subject: String, weight: Weight| Item {
            id,
            source: Source::Day,
            from: from.to_string(),
            subject,
            weight,
            outcome: Outcome::Yours,
            draft: None,
            conflicts_with: None,
        };
        let mut out = Vec::new();
        let cfg = self.workday_cfg();
        let today = self.today(t);
        // Read once, for the waiting-for list and the people both.
        let book: crate::mailbook::MailBook = self.store.load(crate::mailbook::MailBook::FILE);
        // The market's calendar is for someone who trades: said once you've
        // done a trading check-in, or asked for in settings -- otherwise a
        // brief on a quiet day would lead with an exchange holiday.
        let trades = cfg.market_in_brief || !loaded!(self.workday, self.store, journal, JOURNAL).entries.is_empty();
        if trades {
            for line in crate::marketdays::today_and_tomorrow(t as i64, &self.home_zone()) {
                out.push(item(format!("market:{line}"), "Markets", line, Weight::Info));
            }
        }
        if cfg.waiting_for.enabled {
            let items = self.waiting_in(&book, t);
            for w in crate::waitingfor::due_now(&items, t).into_iter().take(5) {
                let (who, what, weight) = match w.side {
                    crate::waitingfor::Side::Promised => ("You promised", format!("\"{}\" -- {}", w.subject, w.said), Weight::Urgent),
                    crate::waitingfor::Side::Owed => ("Waiting on", format!("{} -- \"{}\"", w.with, w.subject), Weight::Info),
                };
                out.push(item(format!("waiting:{}", w.letter), who, what, weight));
            }
        }
        let local = self.local(t);
        let day0 = local - local % 86_400;
        for n in self.notebook.due_between(day0, day0 + 86_400) {
            out.push(item(format!("note:{}", n.id), "Your note", n.text.chars().take(90).collect(), Weight::Info));
        }
        // The files below are read only if they exist -- a missing one loads
        // as empty, which costs a stat.
        let p = loaded!(self.workday, self.store, people, PEOPLE);
        for (name, days, every) in p.due(&book, t).into_iter().take(3) {
            let s = match days {
                Some(d) => format!("{name} -- {d} days (every {every})"),
                None => format!("{name} -- no contact on record"),
            };
            out.push(item(format!("person:{name}"), "Catch up with", s, Weight::Info));
        }
        for (name, away) in p.birthdays(local, 1) {
            out.push(item(format!("birthday:{name}"), "Birthday", if away == 0 { format!("{name}, today") } else { format!("{name}, tomorrow") }, Weight::Info));
        }
        let h = loaded!(self.workday, self.store, habits, HABITS);
        let due: Vec<String> = h.due_today(today).iter().map(|x| x.name.clone()).collect();
        if !due.is_empty() {
            out.push(item("habits".into(), "Habits", due.join(", "), Weight::Info));
        }
        let d = loaded!(self.workday, self.store, deck, DECK);
        let cards = d.due(today, usize::MAX).len();
        if cards > 0 {
            out.push(item("cards".into(), "Cards", format!("{} due", plural(cards, "card")), Weight::Info));
        }
        out.extend(self.social_brief_items(t));
        // The best opportunities found, with why (`hunting`); nothing when
        // hunting is off.
        out.extend(crate::hunting::brief_items(self, t));
        out
    }
}
