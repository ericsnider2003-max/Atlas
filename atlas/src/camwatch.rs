//! "Watch me for five minutes" -- looking for a while, not once (Eric,
//! 1 Oct 2026, item 79).
//!
//! Asked to watch for five minutes, Atlas said it couldn't: the camera tool
//! took one picture and closed. This is the watching, built from what Atlas
//! already has on disk -- the camera feed (`frames::Rolling`), the small face
//! and object detectors (`vision::Looking`), and, when the talking model can
//! see (`models::talking_model_sees`), that model for a sentence about what
//! changed.
//!
//! The rules, every time:
//!
//! 1. **Only when asked, only for a while.** A watch has an end: the minutes
//!    you named, five when you named none, never more than an hour. "Stop
//!    watching" ends it at once.
//! 2. **Never unannounced.** It needs the camera's grant (`camera_ask`), it
//!    says when it starts and when it stops, and the camera stays open the
//!    whole time -- its own light is on while Atlas watches, and off after.
//! 3. **Nothing kept.** Frames live in memory for one look and are dropped;
//!    nothing is written to disk. What's kept is what was said.
//! 4. **Speaks on change, not on every frame.** The detectors run about every
//!    two seconds; Atlas says something only when the picture changes in a
//!    way worth saying (someone arrives or leaves, something new is held up),
//!    and the same news is never repeated within half a minute.

use std::collections::BTreeSet;

/// The longest a watch runs, asked or not.
pub const MOST_SECS: u64 = 60 * 60;
/// A watch with no length named.
pub const DEFAULT_SECS: u64 = 5 * 60;
/// How often a frame is looked at.
pub const EVERY_SECS: u64 = 2;
/// The same news is not said twice within this.
pub const QUIET_SECS: u64 = 30;

/// What was asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// Start watching; `None` seconds: the default.
    Start { secs: Option<u64>, until_stopped: bool },
    Stop,
    /// "Are you watching me?"
    Status,
}

fn plain(s: &str) -> String {
    let s = s.to_lowercase().replace('\u{2019}', "'");
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn has(t: &str, phrase: &str) -> bool {
    format!(" {t} ").contains(&format!(" {phrase} "))
}

/// A number said as digits or words, up to sixty.
fn number(word: &str) -> Option<u64> {
    if let Ok(n) = word.parse::<u64>() {
        return Some(n);
    }
    const WORDS: &[(&str, u64)] = &[
        ("a", 1), ("an", 1), ("one", 1), ("two", 2), ("three", 3), ("four", 4), ("five", 5), ("six", 6), ("seven", 7),
        ("eight", 8), ("nine", 9), ("ten", 10), ("eleven", 11), ("twelve", 12), ("fifteen", 15), ("twenty", 20),
        ("thirty", 30), ("forty", 40), ("forty five", 45), ("fifty", 50), ("sixty", 60), ("couple", 2), ("few", 3),
    ];
    WORDS.iter().find(|(w, _)| *w == word).map(|(_, n)| *n)
}

/// How long, from "for five minutes", "for 10 mins", "for half an hour",
/// "for an hour", "for 90 seconds".
pub(crate) fn length_in(said: &str) -> Option<u64> {
    let t = plain(said);
    if t.contains("half an hour") || t.contains("half hour") {
        return Some(30 * 60);
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let unit = if w.starts_with("minute") || *w == "min" || *w == "mins" {
            60
        } else if w.starts_with("second") || *w == "sec" || *w == "secs" {
            1
        } else if w.starts_with("hour") || *w == "hr" || *w == "hrs" {
            3600
        } else {
            continue;
        };
        if i == 0 {
            continue;
        }
        // "a couple of minutes", "a few minutes"
        let before = if words[i - 1] == "of" && i >= 2 { words[i - 2] } else { words[i - 1] };
        if let Some(n) = number(before) {
            return Some(n * unit);
        }
    }
    None
}

/// Is this a request to watch, to stop watching, or about the watch?
pub fn asks(said: &str) -> Option<Ask> {
    let t = plain(said);
    if t.is_empty() {
        return None;
    }
    const STOP: &[&str] = &["stop watching", "stop watching me", "you can stop watching", "quit watching", "that's enough watching", "stop looking at me", "done watching"];
    if STOP.iter().any(|p| has(&t, p)) {
        return Some(Ask::Stop);
    }
    if ["are you watching me", "are you still watching", "are you watching", "how long are you watching"].iter().any(|p| has(&t, p)) {
        return Some(Ask::Status);
    }
    if t.starts_with("don't") || t.starts_with("do not") || t.contains("don't watch") || t.contains("never watch") {
        return None;
    }
    const START: &[&str] = &[
        "watch me", "keep watching me", "keep an eye on me", "keep looking at me", "look at me for", "watch for",
        "keep watching", "watch me for", "watch over me", "watch what i'm doing", "watch what i do", "watch me work",
        "continuously watch", "watch me continuously",
    ];
    if !START.iter().any(|p| has(&t, p)) {
        return None;
    }
    // "Can you watch me for a five minute period" and "how can we make it
    // so you can watch me for a long duration" are both asks; the second is
    // about the ability rather than a watch now, which `growth` handles once
    // the ability exists: it does now, so both start one.
    let until_stopped = t.contains("until i say stop") || t.contains("until i tell you") || t.contains("until i stop you");
    let secs = length_in(&t).map(|s| s.min(MOST_SECS));
    Some(Ask::Start { secs, until_stopped })
}

/// What one look found, in the terms watching cares about.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Glance {
    pub people: usize,
    /// Names of anyone recognised.
    pub known: BTreeSet<String>,
    /// Things named confidently.
    pub things: BTreeSet<String>,
}

impl Glance {
    pub fn of(scene: &crate::vision::Scene, sure_enough: f32) -> Glance {
        Glance {
            people: scene.faces.len(),
            known: scene.faces.iter().filter_map(|f| f.who.clone()).collect(),
            things: scene.things.iter().filter(|o| o.sure >= sure_enough).map(|o| o.name.clone()).collect(),
        }
    }
}

/// What changed between two looks that's worth saying, if anything.
pub fn news(before: &Glance, now: &Glance) -> Option<String> {
    if before.people > 0 && now.people == 0 {
        return Some("You've stepped out of view.".into());
    }
    if before.people == 0 && now.people > 0 {
        return Some(match now.known.iter().next() {
            Some(who) => format!("{who}'s in view."),
            None if now.people == 1 => "Someone's in view.".into(),
            None => format!("{} people are in view.", now.people),
        });
    }
    if now.people > before.people {
        return Some(match now.people - before.people {
            1 => "Someone else has come into view.".into(),
            n => format!("{n} more people have come into view."),
        });
    }
    if now.people < before.people && now.people > 0 {
        return Some("Someone's left the picture.".into());
    }
    let new: Vec<&String> = now.things.difference(&before.things).filter(|t| !["person", "chair", "bed", "couch"].contains(&t.as_str())).collect();
    if let Some(first) = new.first() {
        return Some(format!("I can see a {first} now."));
    }
    None
}

/// Said when a watch ends.
pub fn ended(secs: u64, said: usize, why: &str) -> String {
    let mins = secs / 60;
    let how_long = match mins {
        0 => format!("{secs} seconds"),
        1 => "a minute".into(),
        n => format!("{n} minutes"),
    };
    let what = match said {
        0 => "nothing changed worth mentioning".to_string(),
        1 => "I mentioned one change".into(),
        n => format!("I mentioned {n} changes"),
    };
    format!("Stopped watching{why} -- {how_long}, and {what}. The camera's off and nothing was kept.")
}

/// Said when a watch starts.
pub fn started(secs: u64, until_stopped: bool) -> String {
    let mins = secs.div_ceil(60);
    let how_long = if mins <= 1 { "a minute".to_string() } else { format!("{mins} minutes") };
    if until_stopped {
        format!(
            "Watching now, until you say \"stop watching\" -- for {how_long} at most. The camera light stays on while I do, \
             nothing is recorded, and I'll speak up when something changes."
        )
    } else {
        format!(
            "Watching now, for {how_long}. The camera light stays on while I do, nothing is recorded, and I'll speak up \
             when something changes. Say \"stop watching\" to end it sooner."
        )
    }
}

/// What the watching thread hands back.
#[derive(Debug, Clone, PartialEq)]
pub enum WatchNews {
    /// Something to say.
    Say(String),
    /// It ended by itself: the time ran out, or the camera stopped.
    Ended { secs: u64, why: String },
}

/// A watch in progress: the thread doing it, and how to stop it.
pub struct Watcher {
    pub started: u64,
    pub until: u64,
    pub until_stopped: bool,
    pub said: usize,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    news: std::sync::mpsc::Receiver<WatchNews>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Everything the watching thread needs, handed over whole.
pub struct Setup {
    pub feed: crate::frames::Feed,
    pub models_dir: std::path::PathBuf,
    pub vision: crate::vision::VisionConfig,
    pub album: crate::vision::Album,
    /// The talking model's chat address when it can see: asked for one
    /// sentence on each change. `None`: the detectors' words alone.
    pub eyes_url: Option<String>,
}

impl Watcher {
    /// Open the camera and start watching on a thread of its own.
    pub fn start(setup: Setup, secs: u64, until_stopped: bool, t: u64) -> Result<Watcher, String> {
        let secs = secs.clamp(10, MOST_SECS);
        // Opened here, so a camera that won't open is said now, not later.
        let rolling = crate::frames::Rolling::start(&setup.feed).map_err(|e| format!("the camera wouldn't open: {e}"))?;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tx, news) = std::sync::mpsc::channel();
        let stop2 = stop.clone();
        let thread = std::thread::Builder::new()
            .name("atlas-watching".into())
            .spawn(move || watch_loop(setup, rolling, secs, stop2, tx))
            .map_err(|e| format!("I couldn't start watching: {e}"))?;
        Ok(Watcher { started: t, until: t + secs, until_stopped, said: 0, stop, news, thread: Some(thread) })
    }

    /// Anything to say since last asked. Ended: the watch is over.
    pub fn drain(&mut self) -> Vec<WatchNews> {
        let got: Vec<WatchNews> = self.news.try_iter().collect();
        self.said += got.iter().filter(|n| matches!(n, WatchNews::Say(_))).count();
        got
    }

    /// Stop now. The camera closes as the thread ends.
    pub fn stop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }

    pub fn left_secs(&self, now: u64) -> u64 {
        self.until.saturating_sub(now)
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn watch_loop(
    setup: Setup,
    mut rolling: crate::frames::Rolling,
    secs: u64,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tx: std::sync::mpsc::Sender<WatchNews>,
) {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};
    let began = Instant::now();
    let end = began + Duration::from_secs(secs);
    let mut looking = crate::vision::Looking::open_with(&setup.models_dir, setup.models_dir.parent(), false);
    let mut vision = setup.vision.clone();
    vision.enabled = true;
    let (w, h) = rolling.size();
    let mut last_look = Instant::now() - Duration::from_secs(EVERY_SECS);
    let mut before: Option<Glance> = None;
    let mut last_said: Vec<(String, Instant)> = Vec::new();
    let mut why = String::new();
    loop {
        if stop.load(Ordering::SeqCst) {
            // Stopped by you: the daemon says so itself.
            return;
        }
        if Instant::now() >= end {
            break;
        }
        // Read every frame, so the one looked at is the newest, not one the
        // pipe has been holding.
        let Some(frame) = rolling.next().map(|f| f.to_vec()) else {
            if rolling.ended() {
                why = " because the camera stopped".into();
                break;
            }
            continue;
        };
        if last_look.elapsed() < Duration::from_secs(EVERY_SECS) {
            continue;
        }
        last_look = Instant::now();
        let sight = looking.look(&frame, w, h, &vision, &setup.album);
        let Some(scene) = sight.scene() else { continue };
        let now = Glance::of(scene, vision.sure_enough_to_say_plainly);
        let said = match &before {
            // The first look says what's there.
            None => Some(format!("I can see {}.", scene.spoken(&vision).trim_end_matches('.'))),
            Some(b) => news(b, &now),
        };
        before = Some(now);
        let Some(mut line) = said else { continue };
        last_said.retain(|(_, at)| at.elapsed() < Duration::from_secs(QUIET_SECS));
        if last_said.iter().any(|(s, _)| *s == line) {
            continue;
        }
        last_said.push((line.clone(), Instant::now()));
        // A sentence from the model that can see, about this frame, when it
        // can (never for the first look, which the detectors describe fine).
        if let (Some(url), true) = (&setup.eyes_url, last_said.len() > 1 || line.starts_with("I can see a")) {
            let rgba: Vec<u8> = frame.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
            let png = crate::pngcodec::write_png(&crate::pngcodec::Rgba { width: w as u32, height: h as u32, pixels: rgba });
            let q = format!(
                "This is a frame from the user's webcam while you watch them, at their request. What just changed: \"{line}\". \
                 In one short sentence, speaking to them as \"you\", say what they seem to be doing now. Don't guess at \
                 anything you can't see."
            );
            if let Ok(a) = crate::picture_talk::ask_server_png(url, &png, &q, 60) {
                line = format!("{line} {a}");
            }
        }
        drop(frame);
        if tx.send(WatchNews::Say(line)).is_err() {
            return;
        }
    }
    let _ = tx.send(WatchNews::Ended { secs: began.elapsed().as_secs(), why });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erics_sentences_start_a_watch() {
        assert_eq!(
            asks("Atlas, can you watch me for a five minute period?"),
            Some(Ask::Start { secs: Some(300), until_stopped: false })
        );
        assert_eq!(asks("watch me for 10 minutes"), Some(Ask::Start { secs: Some(600), until_stopped: false }));
        assert_eq!(asks("keep an eye on me for half an hour"), Some(Ask::Start { secs: Some(1800), until_stopped: false }));
        assert_eq!(asks("watch me until I say stop"), Some(Ask::Start { secs: None, until_stopped: true }));
        assert_eq!(asks("watch me for three hours"), Some(Ask::Start { secs: Some(MOST_SECS), until_stopped: false }));
        assert_eq!(asks("stop watching"), Some(Ask::Stop));
        assert_eq!(asks("are you watching me?"), Some(Ask::Status));
    }

    #[test]
    fn ordinary_sentences_dont() {
        for s in ["watch the news", "what's on my watch", "don't watch me", "look at me", "I watched a film"] {
            assert_eq!(asks(s), None, "{s}");
        }
    }

    #[test]
    fn news_is_about_people_first_then_new_things() {
        let empty = Glance::default();
        let you = Glance { people: 1, ..Default::default() };
        let two = Glance { people: 2, ..Default::default() };
        assert_eq!(news(&you, &empty).as_deref(), Some("You've stepped out of view."));
        assert_eq!(news(&empty, &you).as_deref(), Some("Someone's in view."));
        assert_eq!(news(&you, &two).as_deref(), Some("Someone else has come into view."));
        assert_eq!(news(&you, &you), None);
        let mut with_cup = you.clone();
        with_cup.things.insert("cup".into());
        assert_eq!(news(&you, &with_cup).as_deref(), Some("I can see a cup now."));
        let mut with_chair = you.clone();
        with_chair.things.insert("chair".into());
        assert_eq!(news(&you, &with_chair), None);
    }

    #[test]
    fn it_says_how_long_and_that_nothing_was_kept() {
        assert!(started(300, false).contains("5 minutes"));
        assert!(ended(125, 2, "").contains("2 minutes"));
        assert!(ended(125, 2, "").contains("nothing was kept"));
    }
}
