//! Correcting as you type, in your other apps, and learning from it.
//!
//! Eric, 25 Sep 2026 (H4): option 1. Atlas fixes a mistake in place as you
//! type it, the way phone autocorrect does. "If I go back and correct it then
//! it doesn't fix it for that text or email again." And then: "I would still
//! like Atlas to learn so it gets better at these kinds of tasks. But if it
//! learns too well then it will stop working in general so it needs to make
//! learning adaptive but still smart."
//!
//! ## What happens as you type
//!
//! Atlas reads the text box you're typing in (only its own text, never a
//! password box, never an app on the `prose.never_in` list). When you pause
//! just after finishing a word, and that word is one of the fixes with only
//! one possible correction (`prose`'s certain kind: "dont", "teh",
//! "recieve"), Atlas deletes the word and types the correction. It reads the
//! box back afterwards; if the text isn't what it expected, it says so and
//! leaves it.
//!
//! ## You changing it back
//!
//! If a word Atlas fixed turns back into what you typed, you changed it back.
//! Two things follow:
//!
//! - **In that text or email:** that word is left alone for the rest of it.
//!   That's the ruling, and it holds on the first change-back.
//! - **In general:** one change-back is one piece of evidence, not a lesson.
//!
//! ## Learning, adaptive and still smart
//!
//! Each correction ("dont" → "don't") keeps two tallies: times you kept it,
//! times you changed it back, each counted once per text. They decide:
//!
//! - **One miss never teaches it.** A correction stops being made on its own
//!   only after you've changed it back in at least `STOP_AFTER` different
//!   texts *and* more often than you've kept it. Until then it keeps fixing.
//! - **Where you changed it back matters.** Change-backs in one app (a game
//!   chat's slang) stop it in that app first; it keeps working everywhere
//!   else unless the change-backs are spread across apps.
//! - **Stopped isn't deleted.** A correction you keep changing back is
//!   offered rather than made ("did you mean don't?" is `prose`'s flag).
//! - **Evidence fades.** Every tally halves every `HALF_LIFE_DAYS`, so a
//!   correction stopped months ago comes back once the change-backs are old,
//!   and a habit you've changed stops counting against it. Nothing it
//!   learned is permanent, which is what keeps it from over-learning.
//! - **It picks up your own fixes.** When you correct a word yourself (type
//!   "recieve", then change it to "receive") in `LEARN_AFTER` different texts,
//!   that becomes a correction Atlas makes, provided the two are close
//!   spellings of each other. One of your fixes is never enough, and learned
//!   corrections fade like everything else.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Change-backs, in different texts, before a correction stops on its own.
pub const STOP_AFTER: f32 = 3.0;
/// Your own fixes of the same word, in different texts, before Atlas makes it.
pub const LEARN_AFTER: f32 = 3.0;
/// Every tally halves this often.
pub const HALF_LIFE_DAYS: f32 = 45.0;
/// Tallies fade continuously, so three events a few seconds apart add to
/// 2.99999: counted as three.
const NEARLY: f32 = 0.05;
/// Most corrections learned from you; past this, the weakest go first.
pub const MOST_LEARNED: usize = 300;

/// The evidence about one correction.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tally {
    pub kept: f32,
    pub changed_back: f32,
    /// Change-backs by app (process name, lowercased).
    #[serde(default)]
    pub by_app: HashMap<String, f32>,
    /// When the tallies were last faded.
    pub as_of: u64,
}

impl Tally {
    fn fade(&mut self, now: u64) {
        if self.as_of == 0 {
            self.as_of = now;
            return;
        }
        let days = now.saturating_sub(self.as_of) as f32 / 86_400.0;
        if days <= 0.0 {
            return;
        }
        let k = 0.5f32.powf(days / HALF_LIFE_DAYS);
        self.kept *= k;
        self.changed_back *= k;
        for v in self.by_app.values_mut() {
            *v *= k;
        }
        self.by_app.retain(|_, v| *v > 0.05);
        self.as_of = now;
    }

    /// Stopped in this app? Spread-out change-backs stop it everywhere;
    /// change-backs in one app stop it there.
    fn stopped_in(&self, app: &str) -> bool {
        let everywhere = self.changed_back >= STOP_AFTER - NEARLY && self.changed_back > self.kept;
        let here = self.by_app.get(app).copied().unwrap_or(0.0);
        let only_here = here >= STOP_AFTER - NEARLY && here > self.kept * 0.5;
        everywhere || only_here
    }
}

/// A fix you made yourself, seen in some texts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    pub times: f32,
    pub as_of: u64,
}

/// Everything learned about correcting as you type. Kept in the store.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lessons {
    /// "was→becomes" to its tally.
    pub tallies: HashMap<String, Tally>,
    /// Your own fixes, "was→becomes", not yet made corrections.
    pub yours_seen: HashMap<String, Seen>,
    /// Your own fixes that are now corrections Atlas makes.
    pub learned: HashMap<String, String>,
}

fn key(was: &str, becomes: &str) -> String {
    format!("{}→{}", was.to_lowercase(), becomes.to_lowercase())
}

impl Lessons {
    pub const RECORD: &'static str = "typing_lessons";

    /// May Atlas make this correction on its own, in this app?
    pub fn may_fix(&mut self, was: &str, becomes: &str, app: &str, now: u64) -> bool {
        match self.tallies.get_mut(&key(was, becomes)) {
            None => true,
            Some(t) => {
                t.fade(now);
                !t.stopped_in(&app.to_lowercase())
            }
        }
    }

    /// You left a correction in place. Counted once per text by the caller.
    pub fn kept(&mut self, was: &str, becomes: &str, now: u64) {
        let t = self.tallies.entry(key(was, becomes)).or_default();
        t.fade(now);
        t.kept += 1.0;
    }

    /// You changed a correction back. Counted once per text by the caller.
    pub fn changed_back(&mut self, was: &str, becomes: &str, app: &str, now: u64) {
        let t = self.tallies.entry(key(was, becomes)).or_default();
        t.fade(now);
        t.changed_back += 1.0;
        *t.by_app.entry(app.to_lowercase()).or_default() += 1.0;
        // A learned correction you keep changing back is unlearned rather
        // than just stopped: it came from you in the first place.
        if t.changed_back >= STOP_AFTER - NEARLY && t.changed_back > t.kept {
            self.learned.remove(&was.to_lowercase());
        }
    }

    /// You fixed a word yourself: `was` became `becomes` in this text.
    /// Returns true when that's now a correction Atlas makes.
    pub fn you_fixed(&mut self, was: &str, becomes: &str, now: u64) -> bool {
        if !close_spellings(was, becomes) {
            return false;
        }
        let k = key(was, becomes);
        let s = self.yours_seen.entry(k.clone()).or_default();
        if s.as_of != 0 {
            let days = now.saturating_sub(s.as_of) as f32 / 86_400.0;
            s.times *= 0.5f32.powf(days / HALF_LIFE_DAYS);
        }
        s.as_of = now;
        s.times += 1.0;
        if s.times >= LEARN_AFTER - NEARLY && !self.learned.contains_key(&was.to_lowercase()) {
            self.learned.insert(was.to_lowercase(), becomes.to_lowercase());
            self.yours_seen.remove(&k);
            // Held to a size: the least-evidenced go first. A tally is the
            // evidence, so a learned word with none is the weakest.
            while self.learned.len() > MOST_LEARNED {
                let weakest = self
                    .learned
                    .iter()
                    .map(|(w, b)| (w.clone(), self.tallies.get(&key(w, b)).map(|t| t.kept).unwrap_or(0.0)))
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(w, _)| w);
                match weakest {
                    Some(w) => {
                        self.learned.remove(&w);
                    }
                    None => break,
                }
            }
            return true;
        }
        false
    }

    /// Learned corrections fade too: one that hasn't been kept in a long
    /// while, with no fresh evidence, is dropped.
    pub fn tidy(&mut self, now: u64) {
        for t in self.tallies.values_mut() {
            t.fade(now);
        }
        let tallies = &self.tallies;
        let seen_recently = |w: &String, b: &String| {
            tallies.get(&key(w, b)).map(|t| t.kept + t.changed_back > 0.25).unwrap_or(false)
        };
        let stale: Vec<String> = self
            .learned
            .iter()
            .filter(|(w, b)| !seen_recently(w, b))
            .map(|(w, _)| w.clone())
            .collect();
        // Only drop learned ones that have had time to be used: a lesson
        // learned today has no tally yet and isn't stale.
        for w in stale {
            let b = self.learned[&w].clone();
            if self.tallies.contains_key(&key(&w, &b)) {
                self.learned.remove(&w);
            }
        }
        self.yours_seen.retain(|_, s| {
            let days = now.saturating_sub(s.as_of) as f32 / 86_400.0;
            s.times * 0.5f32.powf(days / HALF_LIFE_DAYS) > 0.2
        });
    }

    /// What it has learned, in plain words: corrections picked up from you,
    /// and ones stopped because you kept changing them back (and where).
    pub fn what_it_learned(&mut self, now: u64) -> String {
        let mut out = Vec::new();
        let mut learned: Vec<String> = self.learned.iter().map(|(w, b)| format!("{w} → {b}")).collect();
        learned.sort();
        if !learned.is_empty() {
            out.push(format!("Learned from your own fixes: {}.", learned.join(", ")));
        }
        let mut stopped = Vec::new();
        for (k, t) in self.tallies.iter_mut() {
            t.fade(now);
            let everywhere = t.changed_back >= STOP_AFTER - NEARLY && t.changed_back > t.kept;
            if everywhere {
                stopped.push(format!("{k} (everywhere)"));
                continue;
            }
            let mut apps: Vec<&String> = t
                .by_app
                .iter()
                .filter(|(_, n)| **n >= STOP_AFTER - NEARLY && **n > t.kept * 0.5)
                .map(|(a, _)| a)
                .collect();
            apps.sort();
            if !apps.is_empty() {
                stopped.push(format!("{k} (in {})", apps.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(", ")));
            }
        }
        stopped.sort();
        if !stopped.is_empty() {
            out.push(format!(
                "Not making these on my own, because you kept changing them back: {}. They come back as that fades.",
                stopped.join(", ")
            ));
        }
        if out.is_empty() {
            "Nothing learned about your typing yet — every correction is still the built-in one.".into()
        } else {
            out.join(" ")
        }
    }

    /// The correction for a word, from what's been learned from you.
    pub fn learned_fix(&self, word: &str) -> Option<String> {
        self.learned.get(&word.to_lowercase()).cloned()
    }
}

/// Two spellings of the same word: a small edit apart, and long enough that
/// the edit isn't a different word ("form" → "from" is two words).
pub fn close_spellings(a: &str, b: &str) -> bool {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    if a == b || a.chars().count() < 5 || b.chars().count() < 5 {
        return false;
    }
    if !a.chars().all(|c| c.is_alphabetic() || c == '\'') || !b.chars().all(|c| c.is_alphabetic() || c == '\'') {
        return false;
    }
    edit_distance(&a, &b) <= 2
}

fn edit_distance(a: &str, b: &str) -> usize {
    // Optimal string alignment: insertions, deletions, substitutions, and a
    // swap of two neighbouring letters ("teh" → "the") as one edit.
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

// ---------------------------------------------------------------------------
// One text being typed
// ---------------------------------------------------------------------------

/// A correction Atlas made in a text, remembered so a change-back is seen.
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    pub was: String,
    pub becomes: String,
    /// Which word of the text it was (0-based), counted from the start.
    pub word_no: usize,
    pub settled: bool,
}

/// What Atlas knows about one text: an email, a chat's message box. Kept in
/// memory while you're on it; a text is a window and its title.
#[derive(Debug, Clone, Default)]
pub struct Text {
    pub last: String,
    pub made: Vec<Made>,
    /// Words you changed back in this text: never corrected again here.
    pub leave_alone: Vec<String>,
    /// Your own fixes already counted in this text.
    pub counted: Vec<String>,
}

/// The words of a text, in order.
fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// What to do now, from the text as it is.
#[derive(Debug, Clone, PartialEq)]
pub enum Now {
    Nothing,
    /// Replace the last word (and the one character after it) with this.
    Fix {
        was: String,
        becomes: String,
        /// Characters to delete from the end: the word and what followed it.
        delete: usize,
        /// What to type in its place: the correction and what followed it.
        type_in: String,
        word_no: usize,
    },
}

/// Look at the text as it is now, against how it was.
///
/// Records change-backs, keeps and your own fixes into `lessons`, and says
/// whether the word just finished needs correcting.
fn look(
    text: &mut Text,
    now_text: &str,
    app: &str,
    lessons: &mut Lessons,
    cfg: &crate::prose::ProseConfig,
    t: u64,
) -> Now {
    let before = std::mem::replace(&mut text.last, now_text.to_string());
    let words_now = words_of(now_text);
    let words_before = words_of(&before);

    // Change-backs and keeps, for corrections made in this text.
    for m in text.made.iter_mut().filter(|m| !m.settled) {
        match words_now.get(m.word_no) {
            Some(w) if w.eq_ignore_ascii_case(&m.was) => {
                lessons.changed_back(&m.was, &m.becomes, app, t);
                text.leave_alone.push(m.was.to_lowercase());
                m.settled = true;
            }
            // Kept once the text has moved on three words past it.
            Some(w) if w.eq_ignore_ascii_case(&m.becomes) && words_now.len() > m.word_no + 3 => {
                lessons.kept(&m.was, &m.becomes, t);
                m.settled = true;
            }
            _ => {}
        }
    }

    // Your own fixes: same place, same neighbours, a close spelling.
    if words_now.len() == words_before.len() {
        for (i, (a, b)) in words_before.iter().zip(&words_now).enumerate() {
            if a != b
                && !text.made.iter().any(|m| m.word_no == i)
                && close_spellings(a, b)
                && !text.counted.contains(&a.to_lowercase())
            {
                text.counted.push(a.to_lowercase());
                lessons.you_fixed(a, b, t);
            }
        }
    }

    // Only just after a word has been finished at the end: the text grew,
    // and its last character ends a word.
    if !now_text.starts_with(&before) || now_text.len() <= before.len() {
        return Now::Nothing;
    }
    let Some(boundary) = now_text.chars().last() else { return Now::Nothing };
    if boundary.is_alphanumeric() || boundary == '\'' {
        return Now::Nothing;
    }
    let body = &now_text[..now_text.len() - boundary.len_utf8()];
    let word_start = body
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '\''))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let word = &body[word_start..];
    if word.is_empty() {
        return Now::Nothing;
    }
    let word_no = words_of(body).len().saturating_sub(1);
    if text.leave_alone.contains(&word.to_lowercase()) || cfg.my_words.iter().any(|w| w.eq_ignore_ascii_case(word)) {
        return Now::Nothing;
    }
    // The certain fixes in this word, read with the sentence it's in so
    // the checker sees its neighbours.
    let sentence_start = body[..word_start]
        .rfind(['.', '!', '?', '\n'])
        .map(|i| i + 1)
        .unwrap_or(0);
    let sentence = &body[sentence_start..];
    let fixes = crate::prose::check(sentence, cfg);
    let offset = word_start - sentence_start;
    let fix = fixes
        .into_iter()
        .find(|f| f.kind == crate::prose::Kind::Certain && f.at == offset && f.len == word.len() && !f.becomes.is_empty())
        .map(|f| f.becomes)
        .or_else(|| {
            lessons.learned_fix(word).map(|b| {
                // Keep a capital the way `prose` does.
                if word.chars().next().map_or(false, char::is_uppercase) {
                    let mut c = b.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or(b)
                } else {
                    b
                }
            })
        });
    let Some(becomes) = fix else { return Now::Nothing };
    if !lessons.may_fix(word, &becomes, app, t) {
        return Now::Nothing;
    }
    Now::Fix {
        was: word.to_string(),
        becomes: becomes.clone(),
        delete: word.chars().count() + 1,
        type_in: format!("{becomes}{boundary}"),
        word_no,
    }
}

/// After Atlas typed a fix: did the box end up as expected? Updates the
/// text's memory either way.
fn after_fixing(text: &mut Text, expected_end: &str, now_text: &str, was: &str, becomes: &str, word_no: usize) -> bool {
    text.last = now_text.to_string();
    let ok = now_text.ends_with(expected_end);
    if ok {
        text.made.push(Made { was: was.to_string(), becomes: becomes.to_string(), word_no, settled: false });
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_spellings_are_one_word_not_two() {
        assert!(close_spellings("recieve", "receive"));
        assert!(!close_spellings("form", "from"), "too short to tell");
        assert!(!close_spellings("house", "horse and"));
        assert!(!close_spellings("definately", "maybe"));
    }
}

// ---------------------------------------------------------------------------
// The watcher: its own thread, polling the box you're typing in
// ---------------------------------------------------------------------------

/// How often the box is read. Fast enough to catch the pause after a word.
pub const POLL_MS: u64 = 120;
/// How long the text must sit unchanged after a finished word before Atlas
/// touches it: a pause, so its keys don't land in the middle of yours.
pub const STILL_FOR_MS: u64 = 250;

/// A correction found, waiting for you to pause.
#[derive(Debug, Clone, PartialEq)]
struct Waiting {
    fix: Now,
    text_then: String,
    since_ms: u64,
}

/// The watcher's memory across polls.
#[derive(Debug, Default)]
pub struct Watch {
    texts: HashMap<(u64, String), Text>,
    waiting: Option<Waiting>,
    /// After a fix that didn't land as expected, a rest before trying again.
    resting_until_ms: u64,
    /// Something to say, once.
    pub said: Vec<String>,
}

/// What one poll did.
#[derive(Debug, Clone, PartialEq)]
pub enum Polled {
    Nothing,
    Fixed { was: String, becomes: String },
    /// A fix went in and the box didn't read back as expected.
    DidNotLand,
}

/// One poll: read the box, keep the books, and fix the word just finished
/// once you've paused. `busy` are windows Atlas is typing in itself.
pub fn look_at_the_box(
    plat: &dyn crate::platform::Platform,
    w: &mut Watch,
    lessons: &mut Lessons,
    cfg: &crate::prose::ProseConfig,
    busy: &[u64],
    now_ms: u64,
) -> Polled {
    let t = now_ms / 1000;
    let Some(win) = plat.active_window_id().ok().flatten() else { return Polled::Nothing };
    if busy.contains(&win.0) {
        w.waiting = None;
        return Polled::Nothing;
    }
    let Some(aw) = plat.active_window().ok().flatten() else { return Polled::Nothing };
    if !crate::prose::may_correct_in(&format!("{} {}", aw.process, aw.title), cfg) {
        return Polled::Nothing;
    }
    if plat.focused_is_editable().ok().flatten() != Some(true) {
        return Polled::Nothing;
    }
    // `focused_text` never reads a password box.
    let Some(now_text) = plat.focused_text().ok().flatten() else { return Polled::Nothing };
    // Texts are remembered by window and title; a handful at a time.
    if w.texts.len() > 40 {
        w.texts.clear();
    }
    let text = w.texts.entry((win.0, aw.title.clone())).or_default();
    if text.last.is_empty() && text.made.is_empty() && !now_text.is_empty() {
        // First sight of this box: remember it, correct nothing already there.
        text.last = now_text;
        return Polled::Nothing;
    }

    if now_text != text.last {
        let found = look(text, &now_text, &aw.process, lessons, cfg, t);
        w.waiting = match found {
            Now::Nothing => None,
            fix => Some(Waiting { fix, text_then: now_text, since_ms: now_ms }),
        };
        return Polled::Nothing;
    }

    // Unchanged. Is a fix waiting, and have you paused long enough?
    let Some(waiting) = w.waiting.clone() else { return Polled::Nothing };
    if waiting.text_then != now_text || now_ms.saturating_sub(waiting.since_ms) < STILL_FOR_MS || now_ms < w.resting_until_ms {
        return Polled::Nothing;
    }
    w.waiting = None;
    let Now::Fix { was, becomes, delete, type_in, word_no } = waiting.fix else { return Polled::Nothing };
    for _ in 0..delete {
        if plat.press("backspace").is_err() {
            return Polled::Nothing;
        }
    }
    if plat.type_text(&type_in).is_err() {
        return Polled::Nothing;
    }
    let after = plat.focused_text().ok().flatten().unwrap_or_default();
    if after_fixing(text, &type_in, &after, &was, &becomes, word_no) {
        Polled::Fixed { was, becomes }
    } else {
        // Your keys and Atlas's crossed, or the box didn't take it. Stop for
        // a minute and say so, once.
        w.resting_until_ms = now_ms + 60_000;
        w.said.push(format!(
            "I tried to change \"{was}\" to \"{becomes}\" and the box didn't come out as I expected — have a look at it. I'll leave your typing alone for a minute."
        ));
        Polled::DidNotLand
    }
}

/// Start the watcher on its own thread. It stops when `stop` is set.
///
/// Its own platform handle: the daemon's isn't shared across threads. The
/// lessons are saved whenever a fix is kept or changed back, at most once a
/// minute.
pub fn start(
    cfg: crate::prose::ProseConfig,
    store: crate::store::Store,
    busy: std::sync::Arc<std::sync::Mutex<Vec<u64>>>,
    said: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let plat = crate::platform::here();
        let mut lessons: Lessons = store.load(Lessons::RECORD);
        lessons.tidy(crate::store::now());
        let mut w = Watch::default();
        let mut saved_at = 0u64;
        let mut dirty = false;
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let busy_now = busy.lock().map(|b| b.clone()).unwrap_or_default();
            let before = lessons.clone();
            let _ = crate::astype::look_at_the_box(plat.as_ref(), &mut w, &mut lessons, &cfg, &busy_now, now_ms);
            if lessons != before {
                dirty = true;
            }
            if dirty && now_ms.saturating_sub(saved_at) > 60_000 {
                let _ = store.save(Lessons::RECORD, &lessons);
                saved_at = now_ms;
                dirty = false;
            }
            if !w.said.is_empty() {
                if let Ok(mut s) = said.lock() {
                    s.append(&mut w.said);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
        }
        if dirty {
            let _ = store.save(Lessons::RECORD, &lessons);
        }
    })
}
