//! Speaking on its own thread.
//!
//! **Why.** The run loop answers the hub, the typing box and the icon by the
//! clock. Until 28 Sep 2026 it also played every sentence of a reply itself
//! (`Mouth::speak` returns when the sentence has been heard), so a long
//! sentence held all of that for as long as it took to say -- the hub was
//! answered between sentences, never during one.
//!
//! Now a reply is handed, sentence by sentence, to a player thread
//! (`SpeakWork`, the voice's owned twin), and the loop waits on it in slices
//! of a few milliseconds: answering the hub, watching the talk key and your
//! voice (`micthread`), and printing each sentence as it starts. Everything
//! the reply did before still happens, in the same place:
//!
//! * **cutting in** -- the talk key or your voice stops the player at once
//!   (`micthread::cut_playback`), not at the end of the sentence;
//! * **"carry on"** -- what wasn't said is kept, *starting with the sentence
//!   that was cut*: a sentence you heard half of was not said (until 28 Sep
//!   2026 it was counted as said, and "carry on" skipped it);
//! * **one watch per reply** -- the microphone is watched from the reply's
//!   first sentence to its last, including the gaps while the model is still
//!   writing, rather than opened again for each sentence;
//! * **the next sentence made while this one plays** (Kokoro, `Mouth::prepare`
//!   and `prepare_more`) -- and now it can be, even for a reply the model is
//!   still writing, because the loop is free to hand the next one over;
//! * **typed and quiet** -- printed, not spoken, a sentence at a time, as
//!   before.
//!
//! A `Mouth` that can't give an owned voice for a thread (`speak_work` is
//! `None`: most test stand-ins) is spoken on the loop as it always was, a
//! sentence at a time with the hub answered between.

use crate::daemon::Mouth;
use crate::error::Result;
use crate::micthread::{CutIn, MicLink};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// What the player thread does with a sentence: the voice, owned, so it can
/// be sent there. `Voice`'s is `voice::VoiceSpeaker`.
pub trait SpeakWork: Send {
    /// Say one sentence (already in its spoken form), returning when it has
    /// been heard -- or at once when `micthread::playback_cut()` turns true.
    fn speak(&mut self, text: &str) -> Result<()>;
}

/// How long the loop waits on the player between looks at everything else.
const SLICE: Duration = Duration::from_millis(15);

/// How long a reply waits, once your voice has stopped it, for the words you
/// said to be made out. The hub is answered meanwhile.
pub const CUT_IN_WAIT: Duration = Duration::from_secs(20);

/// Sentences handed to a player and not yet played (or skipped):
/// `Daemon::say` waits for none rather than talk over a reply.
static QUEUED: AtomicUsize = AtomicUsize::new(0);

/// Is a reply playing on its own thread?
fn playing() -> bool {
    QUEUED.load(Ordering::SeqCst) > 0
}

/// Wait (up to `max`) until no reply is playing.
pub fn wait_quiet(max: Duration) {
    let until = Instant::now() + max;
    while playing() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Outcome {
    Whole,
    Cut,
    Failed,
}

#[derive(Debug)]
enum Event {
    Started(usize),
    Done(usize, Outcome),
    Skipped(usize),
}

/// The thread playing one reply.
struct Player {
    tx: Option<Sender<(usize, String)>>,
    rx: Receiver<Event>,
    handle: Option<std::thread::JoinHandle<()>>,
    /// This reply was stopped (by the loop). The player is also stopped by
    /// `micthread::playback_cut`, which your voice sets from the
    /// microphone's thread before the loop has heard about it.
    cut: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Player {
    fn start(mut work: Box<dyn SpeakWork>) -> Option<Player> {
        let (tx, jobs) = channel::<(usize, String)>();
        let (ev, rx) = channel::<Event>();
        let cut = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cut2 = cut.clone();
        let is_cut = move || cut2.load(Ordering::SeqCst) || crate::micthread::playback_cut();
        let handle = std::thread::Builder::new()
            .name("atlas-speaking".into())
            .spawn(move || {
                let mut failed = false;
                for (i, text) in jobs {
                    // Counted off however it ends.
                    struct Off;
                    impl Drop for Off {
                        fn drop(&mut self) {
                            QUEUED.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    let _off = Off;
                    if failed || is_cut() {
                        let _ = ev.send(Event::Skipped(i));
                        continue;
                    }
                    let _ = ev.send(Event::Started(i));
                    // Caught: a panic in the voice costs the reply, not Atlas.
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work.speak(&text)));
                    let o = if is_cut() {
                        Outcome::Cut
                    } else if matches!(r, Ok(Ok(()))) {
                        Outcome::Whole
                    } else {
                        failed = true;
                        Outcome::Failed
                    };
                    let _ = ev.send(Event::Done(i, o));
                }
            });
        match handle {
            Ok(h) => Some(Player { tx: Some(tx), rx, handle: Some(h), cut }),
            Err(_) => None,
        }
    }

    fn play(&self, i: usize, text: String) {
        if let Some(tx) = &self.tx {
            QUEUED.fetch_add(1, Ordering::SeqCst);
            if tx.send((i, text)).is_err() {
                QUEUED.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }

    /// No more sentences: the thread ends once it has played (or skipped)
    /// what it has.
    fn close(&mut self) {
        self.tx = None;
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// What the loop does while a reply is being said.
pub trait Host {
    /// A sentence is starting: shown and logged, as written.
    fn line(&mut self, chunk: &str);
    /// Everything else the loop answers while it waits (the hub).
    fn between(&mut self);
    /// Should the reply stop now, and in what words (Atlas paused from the
    /// hub mid-reply: "pause")? Asked as often as the talk key.
    fn hush(&mut self) -> Option<String> {
        None
    }
}

/// Why a reply stopped before its end.
#[derive(Debug, Clone, PartialEq)]
enum Stop {
    /// Cut off: by the words given (a stop, a pause, or "hold on" for words
    /// that are answered next).
    Interrupted(String),
    /// Playback failed.
    Failed,
}

/// A reply being said, from its first sentence to its last.
pub struct Saying<'m> {
    mouth: &'m dyn Mouth,
    typed: bool,
    player: Option<Player>,
    link: Option<MicLink>,
    /// Every chunk handed over, as written.
    chunks: Vec<String>,
    /// `chunks[..said]` reached you whole.
    said: usize,
    /// Chunks the player has finished with (played, cut or skipped), or,
    /// inline, chunks gone through.
    through: usize,
    stop: Option<Stop>,
    /// What you said over the reply, to be answered next.
    words: Option<String>,
    /// When your voice stopped the reply, while its words are made out.
    cut_since: Option<Instant>,
    /// `Host::hush` said nothing when the reply began: only a change counts
    /// (a reply said while already paused -- "I'm paused" -- is said).
    hush_armed: Option<bool>,
}

/// How a reply went.
pub struct Said {
    pub delivery: crate::speech::Delivery,
    /// What you said over it by voice (not a stop or a pause), to answer next.
    pub words: Option<String>,
}

impl<'m> Saying<'m> {
    /// A reply starts. `typed`: printed, not spoken. `link`: the microphone,
    /// watched for your voice from now until `finish`.
    pub fn start(mouth: &'m dyn Mouth, typed: bool, link: Option<MicLink>) -> Saying<'m> {
        let player = if typed { None } else { mouth.speak_work().and_then(Player::start) };
        // The stop switch is the real player's (and the microphone's, which
        // throws it): a new reply played or watched starts with it off.
        if player.is_some() || link.is_some() {
            crate::micthread::clear_cut();
        }
        if let Some(l) = &link {
            l.watch(true);
        }
        Saying { mouth, typed, player, link, chunks: Vec::new(), said: 0, through: 0, stop: None, words: None, cut_since: None, hush_armed: None }
    }

    /// Is some of what was handed over still to be said?
    pub fn busy(&self) -> bool {
        self.stop.is_none() && self.through < self.chunks.len()
    }

    /// Stop now, in these words ("stop", "pause"): the player is cut and the
    /// rest kept.
    pub fn cut(&mut self, why: &str) {
        if self.stop.is_none() {
            self.halt(Stop::Interrupted(why.to_string()));
        }
    }

    /// Nothing more is said: the player (and the real one it drives) is
    /// stopped now.
    fn halt(&mut self, why: Stop) {
        if let Some(p) = &self.player {
            p.cut.store(true, Ordering::SeqCst);
        }
        // The real player (on the thread) listens to the stop switch; on
        // the loop, nothing is playing by the time this is decided.
        if self.player.is_some() && why != Stop::Failed {
            crate::micthread::cut_playback();
        }
        self.stop = Some(why);
    }

    /// Has the reply been cut off (or failed)? Nothing more is said.
    pub fn stopped(&self) -> bool {
        self.stop.is_some()
    }

    /// More of the reply: queued to be said after what's already there.
    pub fn add(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if self.stop.is_some() {
            // Cut off already: kept, unsaid, for "carry on".
            self.chunks.extend(crate::speech::split(text));
            return;
        }
        if !self.typed {
            // Each next sentence made while the one before plays (Kokoro).
            if self.chunks.is_empty() {
                self.mouth.prepare(text);
            } else {
                self.mouth.prepare_more(text);
            }
        }
        for chunk in crate::speech::split(text) {
            let i = self.chunks.len();
            if let Some(p) = &self.player {
                // The speaker gets the spoken form; the screen and the log
                // the chunk as written.
                p.play(i, crate::spoken_form::for_speech(&chunk));
            }
            self.chunks.push(chunk);
        }
    }

    /// Look once, without waiting: sentences starting, your voice, the talk
    /// key (`listen`). True once the reply has stopped short.
    pub fn poll(&mut self, host: &mut dyn Host, listen: &mut dyn FnMut() -> Option<String>) -> bool {
        self.step(host, listen, Duration::ZERO);
        self.stop.is_some()
    }

    /// Say everything handed over so far (or until cut off), answering the
    /// hub meanwhile.
    pub fn wait(&mut self, host: &mut dyn Host, listen: &mut dyn FnMut() -> Option<String>) {
        loop {
            self.step(host, listen, SLICE);
            if self.stop.is_some() && self.cut_since.is_none() {
                return;
            }
            if self.player.is_none() && self.cut_since.is_some() {
                // Your words being made out, on the loop: no player to wait
                // on, so a slice's rest here.
                std::thread::sleep(SLICE);
            }
            if self.stop.is_none() && self.through >= self.chunks.len() {
                // One last look, so a stop at the very end still counts.
                self.check(host, listen);
                if self.cut_since.is_none() {
                    return;
                }
            }
            host.between();
        }
    }

    /// One step: at most `slice` waiting on the player (or, inline, one
    /// sentence said).
    fn step(&mut self, host: &mut dyn Host, listen: &mut dyn FnMut() -> Option<String>, slice: Duration) {
        if self.player.is_some() {
            self.take_events(host, slice);
            self.check(host, listen);
        } else {
            self.check(host, listen);
            if self.stop.is_none() && self.through < self.chunks.len() {
                self.inline_one(host);
            }
        }
    }

    /// The player's news.
    fn take_events(&mut self, host: &mut dyn Host, slice: Duration) {
        let Some(p) = &self.player else { return };
        let mut news = Vec::new();
        if !slice.is_zero() {
            match p.rx.recv_timeout(slice) {
                Ok(e) => news.push(e),
                Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        news.extend(p.rx.try_iter());
        for ev in news {
            match ev {
                Event::Started(i) => {
                    if let Some(c) = self.chunks.get(i) {
                        host.line(c);
                    }
                }
                Event::Done(i, o) => {
                    self.through = self.through.max(i + 1);
                    match o {
                        Outcome::Whole if i == self.said => self.said += 1,
                        Outcome::Whole => {}
                        Outcome::Cut => {
                            // Cut without a reason given here (the stop
                            // switch, thrown elsewhere): taken as a stop, so
                            // the rest is kept.
                            if self.stop.is_none() && !self.voice_pending() {
                                self.halt(Stop::Interrupted("stop".into()));
                            }
                        }
                        Outcome::Failed => {
                            if self.stop.is_none() {
                                self.halt(Stop::Failed);
                            }
                        }
                    }
                }
                Event::Skipped(i) => self.through = self.through.max(i + 1),
            }
        }
    }

    fn voice_pending(&self) -> bool {
        self.cut_since.is_some() || self.link.as_ref().is_some_and(|l| l.cut_pending())
    }

    /// Your voice, then Atlas being paused or closing, then the talk key.
    fn check(&mut self, host: &mut dyn Host, listen: &mut dyn FnMut() -> Option<String>) {
        if let Some(l) = self.link.clone() {
            match l.try_cut_in() {
                CutIn::No => {}
                CutIn::Waiting => {
                    // The player has already been stopped; nothing more is
                    // handed on while the words are made out.
                    let since = *self.cut_since.get_or_insert_with(Instant::now);
                    if self.stop.is_none() {
                        self.halt(Stop::Interrupted("hold on".into()));
                    }
                    if since.elapsed() >= CUT_IN_WAIT {
                        l.give_up_cut_in();
                        self.cut_since = None;
                    }
                    return;
                }
                CutIn::Said(words) => {
                    self.cut_since = None;
                    let why = if crate::speech::is_interruption(&words) {
                        words
                    } else {
                        self.words = Some(words);
                        "hold on".into()
                    };
                    if self.stop.is_none() {
                        self.halt(Stop::Interrupted(why));
                    } else {
                        self.stop = Some(Stop::Interrupted(why));
                    }
                    return;
                }
            }
        }
        if self.stop.is_some() {
            return;
        }
        let hush = host.hush();
        let armed = *self.hush_armed.get_or_insert(hush.is_none());
        if let Some(why) = hush.filter(|_| armed) {
            self.halt(Stop::Interrupted(why));
            return;
        }
        if let Some(heard) = listen() {
            if crate::speech::is_interruption(&heard) {
                self.halt(Stop::Interrupted(heard));
            }
        }
    }

    /// Inline (no player thread): one sentence, said here.
    fn inline_one(&mut self, host: &mut dyn Host) {
        let i = self.through;
        let chunk = self.chunks[i].clone();
        host.line(&chunk);
        self.through += 1;
        if self.typed {
            self.said += 1;
            host.between();
            return;
        }
        let ok = self.mouth.speak(&crate::spoken_form::for_speech(&chunk)).is_ok();
        // Stopped by your voice while it played (the microphone's thread
        // stopped the player): not said. Asked of this Atlas's microphone,
        // not the process-wide stop switch.
        if self.voice_pending() {
        } else if ok {
            self.said += 1;
        } else {
            self.halt(Stop::Failed);
        }
        host.between();
    }

    /// The reply is over (said, or cut off): the microphone stops watching,
    /// the player is let go, and what reached you is told apart from what
    /// didn't.
    pub fn finish(mut self) -> Said {
        if let Some(l) = &self.link {
            l.watch(false);
        }
        let switch = self.player.is_some() || self.link.is_some();
        if let Some(mut p) = self.player.take() {
            p.close();
            // Whatever is still queued is skipped at once when cut; otherwise
            // `wait` has already let it all play.
            while let Ok(ev) = p.rx.recv() {
                if let Event::Done(i, Outcome::Whole) = ev {
                    if i == self.said {
                        self.said += 1;
                    }
                }
            }
            drop(p);
        }
        if switch {
            crate::micthread::clear_cut();
        }
        let said = self.said.min(self.chunks.len());
        let (interrupted_by, unspoken) = match self.stop.take() {
            Some(Stop::Interrupted(w)) => (Some(w), self.chunks[said..].to_vec()),
            Some(Stop::Failed) => (None, self.chunks[said..].to_vec()),
            None => (None, self.chunks[said..].to_vec()),
        };
        Said {
            delivery: crate::speech::Delivery { spoken: self.chunks[..said].to_vec(), unspoken, interrupted_by },
            words: self.words.take(),
        }
    }
}
