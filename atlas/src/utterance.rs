//! The microphone's stream, cut into the things you said.
//!
//! **Why this exists (Eric, 29 Sep 2026: "I use the wake word and Atlas says
//! 'I heard my name but nothing after it'").** The wake word was heard by
//! recording a three-second clip, transcribing it and looking for "atlas" in
//! the words -- and then the words were thrown away and a *new* recording
//! was started for the request. People say "Atlas, what time is it" in one
//! breath: the request was in the clip that was thrown away, and the new
//! recording, started a second or more later (speech-to-text on the clip,
//! then the recorder starting up), heard the silence after it. Worse, the new
//! recording's speech detector learned "the room" from its first third of a
//! second, which -- when you were still talking -- was your own voice, so
//! what followed didn't count as speech either.
//!
//! Now the microphone is one stream, read a window at a time, and this
//! module cuts it where you pause:
//!
//! * `Segmenter` -- speech told from the room by Atlas's detector (`vad`,
//!   with your calibrated thresholds from `endpoint`), a third of a second
//!   kept from before the first word so it isn't clipped, and the end decided
//!   the way `endpoint` decides it: a longer pause after "I want you to" than
//!   after "what time is it".
//! * `wait_for_name` -- the wake word, looked for in what you said. The
//!   request is whatever came after the name in the same breath; if you were
//!   still talking when the first few seconds were checked, it keeps
//!   recording until you stop and hears the whole thing; if you said only the
//!   name, it waits a moment on the same stream for the rest. Only when
//!   nothing follows does the caller ask "Yes?".
//! * `next_utterance` -- the open floor after a reply: wait up to so many
//!   seconds for you to start, then take what you say until you stop.
//!   Nothing is transcribed unless you spoke, so a long hands-free wait
//!   costs a detector, not a speech engine.
//!
//! Speech-to-text runs only on stretches that are speech. The old wake word
//! transcribed every three seconds of the day, silence included.

use crate::endpoint::{EndpointConfig, Endpointer};
use crate::micthread::{MicStream, MicWork, NameCheck};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Samples per second: everything Atlas records is 16 kHz mono.
pub const RATE: u32 = 16_000;
/// One window, as `micthread` reads them: 32 ms.
pub const WINDOW: usize = 512;
const WINDOW_MS: u64 = 32;
/// Kept from before the voice was sure, so the first word isn't clipped.
const PRE_ROLL_MS: u64 = 320;
/// Windows of speech in a row before it counts as you starting.
const START_WINDOWS: u32 = 3;
/// Less speech than this in the whole stretch is a cough or a click.
const MIN_SPEECH_MS: u64 = 200;
/// Silence kept after the last word: enough for the speech engine to hear
/// the word end, not so much that it transcribes the pause.
const TAIL_MS: u64 = 300;
/// With only the name heard, how long to go on listening on the same stream
/// for the rest before handing back "just the name".
pub const AFTER_THE_NAME_MS: u64 = 1_500;
/// When the stretch being checked for the name is ruled out, this much of
/// its end is kept, in case the name was being said across the cut.
const OVERLAP_MS: u64 = 1_000;

fn ms_of(samples: usize) -> u64 {
    samples as u64 * 1000 / RATE as u64
}

fn samples_of(ms: u64) -> usize {
    (ms * RATE as u64 / 1000) as usize
}

/// The conversation after the wake word (`conversation` in settings).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct ConversationConfig {
    /// After the wake word the conversation stays open: Atlas listens for
    /// the next thing you say -- no key, no wake word again -- until you've
    /// said nothing for `quiet_secs`, or said "that's all" / "thanks Atlas".
    /// Then it goes back to listening for its name. Off, the floor stays
    /// open only briefly after each reply, as before.
    pub hands_free: bool,
    /// How long a silence ends the conversation.
    pub quiet_secs: u32,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        ConversationConfig { hands_free: true, quiet_secs: 25 }
    }
}

/// What one window did to the stretch being cut.
#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    /// Nobody talking.
    Quiet,
    /// You're talking (or pausing mid-sentence).
    Speaking,
    /// You stopped: what you said, from just before the first word to just
    /// after the last.
    Done(Vec<i16>),
}

/// Cuts a stream of windows into utterances. Fed one window at a time.
#[derive(Debug, Clone)]
pub struct Segmenter {
    cfg: EndpointConfig,
    vad: crate::vad::Vad,
    pre: VecDeque<Vec<i16>>,
    cur: Vec<i16>,
    in_utterance: bool,
    run: u32,
    /// When you've stopped: `endpoint`'s own decision, fed this stretch's
    /// windows as they come -- a longer pause after "I want you to" than
    /// after "what time is it", and the hard stop.
    ep: Endpointer,
    /// Audio time into this stretch.
    at_ms: u64,
    speech_ms: u64,
    /// Where in `cur` the last speech ended.
    last_speech_end: usize,
    /// The words so far, when known: sets how long a pause ends it.
    hint: String,
    /// The quietest window heard while the detector learns the room.
    quietest: Option<f32>,
}

impl Segmenter {
    pub fn new(cfg: &EndpointConfig) -> Segmenter {
        Segmenter {
            cfg: cfg.clone(),
            vad: crate::vad::Vad::tuned(RATE, cfg.vad_params()),
            pre: VecDeque::new(),
            cur: Vec::new(),
            in_utterance: false,
            run: 0,
            ep: Endpointer::start(0),
            at_ms: 0,
            speech_ms: 0,
            last_speech_end: 0,
            hint: String::new(),
            quietest: None,
        }
    }

    /// Is this window a voice? The detector's verdict, moved either side of
    /// the endpoint's line (`vad::level_for_endpoint`). While the detector is
    /// still learning the room (its first third of a second), a window
    /// counts when it stands `vad_loud_db` above the quietest window heard so
    /// far -- the room, measured the same way the detector will. It was the
    /// plain level against the endpoint's fixed -38 dB (30 Sep 2026): a
    /// normal voice on a microphone set low never reached it, and a loud fan
    /// always did.
    fn is_speech(&mut self, w: &[i16]) -> bool {
        let db = crate::audio::level_db(w);
        let share = self.vad.window(w);
        if share.is_none() {
            let above_room = self.quietest.is_some_and(|q| f64::from(db - q) >= self.cfg.vad_loud_db);
            self.quietest = Some(self.quietest.map_or(db, |q| q.min(db)));
            return above_room;
        }
        crate::vad::level_for_endpoint(db, share, self.cfg.silence_below_db) > self.cfg.silence_below_db
    }

    pub fn feed(&mut self, w: &[i16]) -> Seg {
        let speech = self.is_speech(w);
        if !self.in_utterance {
            self.pre.push_back(w.to_vec());
            let keep = (PRE_ROLL_MS / WINDOW_MS) as usize + START_WINDOWS as usize;
            while self.pre.len() > keep {
                self.pre.pop_front();
            }
            self.run = if speech { self.run + 1 } else { 0 };
            if self.run < START_WINDOWS {
                return Seg::Quiet;
            }
            self.in_utterance = true;
            self.cur = self.pre.drain(..).flatten().collect();
            self.speech_ms = u64::from(self.run) * WINDOW_MS;
            self.last_speech_end = self.cur.len();
            // The endpointer starts with you already talking: it has heard
            // the windows that started this.
            self.ep = Endpointer::start(0);
            self.at_ms = ms_of(self.cur.len()).max(self.cfg.warmup_ms);
            self.ep.feed(self.cfg.silence_below_db + 1.0, "", self.at_ms, &self.cfg);
            return Seg::Speaking;
        }
        self.cur.extend_from_slice(w);
        self.at_ms += WINDOW_MS;
        if speech {
            self.speech_ms += WINDOW_MS;
            self.last_speech_end = self.cur.len();
        }
        // The endpointer decides on a level against its line; the detector
        // has already said which side of it this window is (as
        // `vad::level_for_endpoint` puts it). With nothing known of the words
        // yet, it waits the mid-phrase pause, as `Voice::listen_until_you_stop`
        // always did: cutting you off is worse than half a second's wait.
        let line = self.cfg.silence_below_db;
        let level = if speech { line + 1.0 } else { line - 1.0 };
        self.ep.feed(level, &self.hint, self.at_ms, &self.cfg);
        if self.ep.finished() {
            return match self.finish() {
                Some(u) => Seg::Done(u),
                None => Seg::Quiet,
            };
        }
        Seg::Speaking
    }

    /// End the stretch now (the stream stopped): what was said, if it was
    /// enough to be speech.
    pub fn finish(&mut self) -> Option<Vec<i16>> {
        if !self.in_utterance {
            return None;
        }
        self.in_utterance = false;
        self.run = 0;
        self.pre.clear();
        self.hint.clear();
        let mut out = std::mem::take(&mut self.cur);
        out.truncate((self.last_speech_end + samples_of(TAIL_MS)).min(out.len()));
        (self.speech_ms >= MIN_SPEECH_MS).then_some(out)
    }

    /// What's been said so far in the stretch going on now.
    fn so_far(&self) -> &[i16] {
        if self.in_utterance {
            &self.cur
        } else {
            &[]
        }
    }

    /// The words so far, once they're known: "Atlas, remind me to" waits
    /// longer for the rest than "Atlas, what time is it".
    fn set_hint(&mut self, words: &str) {
        self.hint = words.to_string();
    }
}

/// Wait up to `wait_ms` (of audio) for you to start, then take what you say
/// until you stop. `None`: nobody spoke, `stop` said stop, or the stream
/// ended with nothing said.
pub fn next_utterance(stream: &mut dyn MicStream, cfg: &EndpointConfig, wait_ms: u64, stop: &dyn Fn() -> bool) -> Option<Vec<i16>> {
    let mut seg = Segmenter::new(cfg);
    let mut waited = 0u64;
    loop {
        if stop() {
            return None;
        }
        let Some(w) = stream.read(WINDOW) else {
            return seg.finish();
        };
        match seg.feed(&w) {
            Seg::Done(u) => return Some(u),
            Seg::Speaking => {}
            Seg::Quiet => {
                waited += WINDOW_MS;
                if waited >= wait_ms {
                    return None;
                }
            }
        }
    }
}

/// What listening for the name came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Woke {
    /// The name, and what you asked.
    Request(String),
    /// The name and nothing after it, even after a moment's wait.
    NameOnly,
}

/// The words after the name in what was heard; `None` when it wasn't the
/// name. Empty when the name came with nothing (or only punctuation) after it.
fn after_the_name(check: NameCheck) -> Option<String> {
    match check {
        NameCheck::Named(rest) => Some(rest.trim().to_string()),
        NameCheck::NotNamed(_) => None,
    }
}

/// Listen on `stream` until the wake word is heard, and return what came with
/// it. `Ok(None)` when `stop` said stop (paused, a turn started elsewhere,
/// Atlas closing) or the stream ended normally; `Err` when it ended having
/// given no sound at all (the microphone couldn't be opened), with why.
///
/// `work` hears the name in a stretch of audio (`MicWork::name_in`) and
/// transcribes what follows a bare name (`MicWork::transcribe`). Each
/// window read is counted in `heard` -- proof the microphone works.
pub fn wait_for_name(
    stream: &mut dyn MicStream,
    work: &mut dyn MicWork,
    cfg: &EndpointConfig,
    clip_secs: u32,
    stop: &dyn Fn() -> bool,
    heard: &dyn Fn(),
) -> std::result::Result<Option<Woke>, String> {
    let clip = samples_of(u64::from(clip_secs.max(1)) * 1000);
    let mut seg = Segmenter::new(cfg);
    let mut windows = 0u64;
    // The start of the part of this stretch not yet ruled out.
    let mut from = 0usize;
    // Where the name was found, and what followed it in that look.
    let mut found: Option<(usize, String)> = None;
    loop {
        if stop() {
            return Ok(None);
        }
        let Some(w) = stream.read(WINDOW) else {
            if windows == 0 {
                return Err(stream.why_stopped().unwrap_or_else(|| "the microphone gave no sound".into()));
            }
            return Ok(None);
        };
        windows += 1;
        heard();
        match seg.feed(&w) {
            Seg::Quiet => {}
            Seg::Speaking => {
                // Still talking a few seconds in: look for the name now, so a
                // long request is known to be one while it's being said.
                let len = seg.so_far().len();
                if found.is_none() && len.saturating_sub(from) >= clip {
                    let named = if work.name_by_sound(&seg.so_far()[from..]) == Some(false) {
                        Ok(NameCheck::NotNamed(String::new()))
                    } else {
                        work.name_in(&seg.so_far()[from..])
                    };
                    match named {
                        Ok(check) => match after_the_name(check) {
                            Some(rest) => {
                                seg.set_hint(&rest);
                                // The name, mid-sentence: on a call, the rest
                                // of it isn't heard there (`callmute`).
                                crate::callmute::addressed();
                                found = Some((from, rest));
                            }
                            None => from = len.saturating_sub(samples_of(OVERLAP_MS)),
                        },
                        Err(e) => return Err(e.to_string()),
                    }
                }
            }
            Seg::Done(u) => {
                let start = found.as_ref().map(|f| f.0).unwrap_or(from).min(u.len());
                let early = found.take().map(|f| f.1);
                from = 0;
                let rest = if ms_of(u.len() - start) < MIN_SPEECH_MS {
                    early
                } else if early.is_none() && work.name_by_sound(&u[start..]) == Some(false) {
                    None
                } else {
                    match work.name_in(&u[start..]) {
                        // The whole of it, heard at once: better than the
                        // first few seconds.
                        Ok(check) => after_the_name(check).or(early),
                        Err(e) => return Err(e.to_string()),
                    }
                };
                if rest.is_some() {
                    // Talking to Atlas now: Atlas's answer and what you say
                    // back aren't heard on the call either.
                    crate::callmute::addressed();
                }
                match rest {
                    None => continue,
                    Some(r) if r.chars().any(|c| c.is_alphanumeric()) => return Ok(Some(Woke::Request(r))),
                    Some(_) => {
                        // The name on its own. People pause after it: a
                        // moment more on the same stream, so "Atlas ...
                        // what's the time" is one request, not a "Yes?".
                        let Some(more) = next_utterance(stream, cfg, AFTER_THE_NAME_MS, stop) else {
                            return Ok(if stop() { None } else { Some(Woke::NameOnly) });
                        };
                        let words = match work.name_in(&more) {
                            Ok(NameCheck::Named(r)) => r,
                            Ok(NameCheck::NotNamed(all)) => all,
                            Err(e) => return Err(e.to_string()),
                        };
                        let words = words.trim().to_string();
                        return Ok(Some(if words.chars().any(|c| c.is_alphanumeric()) { Woke::Request(words) } else { Woke::NameOnly }));
                    }
                }
            }
        }
    }
}

/// A stream read on a thread of its own, so nothing is lost while the
/// thread that asked for it is busy -- transcribing the part already heard,
/// say, while you are still talking. Windows wait in a queue until read.
pub struct Pumped {
    rx: Receiver<Vec<i16>>,
    stop: Arc<AtomicBool>,
    why: Arc<Mutex<Option<String>>>,
    left: Vec<i16>,
}

/// How long with no sound at all before the stream counts as stopped.
const STALLED: Duration = Duration::from_secs(5);

pub fn pumped(mut inner: Box<dyn MicStream>) -> Pumped {
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let why = Arc::new(Mutex::new(None));
    let (stop2, why2) = (stop.clone(), why.clone());
    let spawned = std::thread::Builder::new().name("atlas-mic-pump".into()).spawn(move || {
        while !stop2.load(Ordering::SeqCst) {
            match inner.read(WINDOW) {
                Some(w) => {
                    if tx.send(w).is_err() {
                        break;
                    }
                }
                None => {
                    if let Ok(mut y) = why2.lock() {
                        *y = inner.why_stopped();
                    }
                    break;
                }
            }
        }
    });
    if spawned.is_err() {
        if let Ok(mut y) = why.lock() {
            *y = Some("couldn't start reading the microphone".into());
        }
    }
    Pumped { rx, stop, why, left: Vec::new() }
}

impl MicStream for Pumped {
    fn read(&mut self, n: usize) -> Option<Vec<i16>> {
        let mut waited = Duration::ZERO;
        while self.left.len() < n {
            match self.rx.recv_timeout(Duration::from_millis(250)) {
                Ok(w) => {
                    self.left.extend_from_slice(&w);
                    waited = Duration::ZERO;
                }
                Err(RecvTimeoutError::Timeout) => {
                    waited += Duration::from_millis(250);
                    if waited >= STALLED {
                        if let Ok(mut y) = self.why.lock() {
                            y.get_or_insert_with(|| "the microphone stopped sending sound".into());
                        }
                        return None;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
        Some(self.left.drain(..n).collect())
    }
    fn why_stopped(&mut self) -> Option<String> {
        self.why.lock().ok().and_then(|y| y.clone())
    }
}

impl Drop for Pumped {
    fn drop(&mut self) {
        // The reading thread ends within a window and drops the recorder.
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Is this a way of ending the conversation -- "that's all", "thanks
/// Atlas", "we're done" -- said on its own? Only the whole of what was said
/// counts: "thanks, and remind me to call Maya" is a request.
pub fn is_goodbye(said: &str) -> bool {
    let words: Vec<String> = said
        .split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric() || *c == '\'').collect::<String>().to_lowercase().replace('\u{2019}', "'"))
        .filter(|w| !w.is_empty())
        .collect();
    let s = words.join(" ");
    let s = s.trim_end_matches(" atlas").trim_start_matches("ok ").trim_start_matches("okay ");
    const ENDINGS: &[&str] = &[
        "that's all",
        "thats all",
        "that is all",
        "that's it",
        "thats it",
        "that's everything",
        "thanks",
        "thank you",
        "thanks that's all",
        "thank you that's all",
        "thanks that's it",
        "we're done",
        "were done",
        "i'm done",
        "im done",
        "all done",
        "stop listening",
        "goodbye",
        "bye",
        "never mind",
        "nevermind",
        "nothing",
        "nothing else",
        "no that's all",
        "no thanks",
        "no thank you",
    ];
    ENDINGS.contains(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: u64, amp: f32) -> Vec<i16> {
        (0..samples_of(ms)).map(|i| ((i as f32 * 0.07).sin() * amp * 32767.0 + ((i * 7919) % 97) as f32 - 48.0) as i16).collect()
    }
    fn room(ms: u64) -> Vec<i16> {
        (0..samples_of(ms)).map(|i| ((i * 7919) % 41) as i16 - 20).collect()
    }

    #[test]
    fn a_goodbye_is_only_a_goodbye_on_its_own() {
        assert!(is_goodbye("That's all."));
        assert!(is_goodbye("thanks Atlas"));
        assert!(is_goodbye("Okay, thank you."));
        assert!(is_goodbye("That\u{2019}s it"));
        assert!(!is_goodbye("thanks, and remind me to call Maya"));
        assert!(!is_goodbye("what time is it"));
    }

    #[test]
    fn two_stretches_of_sound_with_a_pause_between_are_two_utterances() {
        let mut s = room(600);
        s.extend(tone(900, 0.2));
        s.extend(room(1500));
        s.extend(tone(700, 0.2));
        s.extend(room(1500));
        let mut seg = Segmenter::new(&EndpointConfig::default());
        let mut done = Vec::new();
        for w in s.chunks_exact(WINDOW) {
            if let Seg::Done(u) = seg.feed(w) {
                done.push(u.len());
            }
        }
        assert_eq!(done.len(), 2, "{done:?}");
        assert!(ms_of(done[0]) >= 900 && ms_of(done[0]) <= 900 + PRE_ROLL_MS + TAIL_MS + 100, "{}", ms_of(done[0]));
    }
}
