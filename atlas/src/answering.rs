//! Getting an answer to a question, however you want to give it.
//!
//! Atlas asks things — "go ahead?", "which one?", "record this call?" — and
//! you shouldn't have to be able to speak to answer. You might be on a call,
//! in a room with someone, or it might simply have misheard you twice.
//!
//! So a question stays open across three channels at once: your voice, a
//! gesture on camera, and the typing box. Whichever arrives first wins.
//!
//! One rule holds throughout: **a nod is not a signature.** A thumbs-up from a
//! webcam is two fingers of confidence from a camera that has been wrong
//! before. It can decline anything, and it can approve anything ordinary, but
//! it cannot approve something irreversible — that still takes a word.

use crate::presence::{interpret, may_answer, Gesture, Signal};
use crate::session::{is_no, is_yes};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Voice,
    Gesture,
    Typed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Yes(Channel),
    No(Channel),
    /// Heard something, but it wasn't an answer.
    Unclear(String),
    /// Nothing yet.
    Waiting,
    /// Long enough. Treated as no.
    TimedOut,
}

impl Answer {
    pub fn settled(&self) -> bool {
        matches!(self, Answer::Yes(_) | Answer::No(_) | Answer::TimedOut)
    }
    /// Did this actually approve?
    pub fn approved(&self) -> bool {
        matches!(self, Answer::Yes(_))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AnsweringConfig {
    /// Watch the camera for a thumb when a question is open.
    pub accept_gestures: bool,
    /// Give up waiting after this long. Treated as no.
    pub timeout_secs: u64,
    /// Repeat the question once if nothing has arrived by then.
    pub repeat_after_secs: u64,
    /// A gesture may approve consequential things too. Off, deliberately.
    pub gestures_may_approve_anything: bool,
}

impl Default for AnsweringConfig {
    fn default() -> Self {
        AnsweringConfig {
            accept_gestures: true,
            timeout_secs: 45,
            repeat_after_secs: 15,
            gestures_may_approve_anything: false,
        }
    }
}

/// What Atlas should do while a question is open.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Wait,
    /// Say it again, once.
    Repeat(String),
    /// The gesture said yes but isn't allowed to for this. Ask for a word.
    NeedsAWord(String),
    Settled(Answer),
}

#[derive(Debug, Clone)]
pub struct Question {
    pub asked: String,
    /// Does saying yes to this change something irreversible?
    pub consequential: bool,
    pub asked_at: u64,
    repeated: bool,
    pub answer: Answer,
}

impl Question {
    pub fn new(asked: &str, consequential: bool, t: u64) -> Question {
        Question {
            asked: asked.to_string(),
            consequential,
            asked_at: t,
            repeated: false,
            answer: Answer::Waiting,
        }
    }

    /// Something was said.
    pub fn heard(&mut self, said: &str, cfg: &AnsweringConfig) -> Step {
        let _ = cfg;
        if self.answer.settled() {
            return Step::Settled(self.answer.clone());
        }
        if is_yes(said) {
            self.answer = Answer::Yes(Channel::Voice);
        } else if is_no(said) {
            self.answer = Answer::No(Channel::Voice);
        } else {
            // Not an answer. The question stays open rather than being
            // resolved by an unrelated sentence.
            self.answer = Answer::Unclear(said.to_string());
            return Step::Wait;
        }
        Step::Settled(self.answer.clone())
    }

    /// Something was typed. Same rules as speech.
    pub fn typed(&mut self, text: &str, cfg: &AnsweringConfig) -> Step {
        let step = self.heard(text, cfg);
        if let Answer::Yes(c) | Answer::No(c) = &mut self.answer {
            *c = Channel::Typed;
        }
        match step {
            Step::Settled(_) => Step::Settled(self.answer.clone()),
            other => other,
        }
    }

    /// A gesture was seen on camera.
    ///
    /// This is the fallback you asked for: when speech isn't working or you
    /// can't speak, a thumb answers.
    pub fn saw(&mut self, g: Gesture, cfg: &AnsweringConfig) -> Step {
        if !cfg.accept_gestures || self.answer.settled() {
            return Step::Wait;
        }
        let signal = interpret(g, true, false);
        let permitted = if cfg.gestures_may_approve_anything {
            true
        } else {
            may_answer(signal, self.consequential)
        };

        match (signal, permitted) {
            (Signal::No, _) => {
                // Declining is always safe, whatever the stakes.
                self.answer = Answer::No(Channel::Gesture);
                Step::Settled(self.answer.clone())
            }
            (Signal::Yes, true) => {
                self.answer = Answer::Yes(Channel::Gesture);
                Step::Settled(self.answer.clone())
            }
            (Signal::Yes, false) => Step::NeedsAWord(
                "I saw that, but for something this consequential I need to hear it.".into(),
            ),
            _ => Step::Wait,
        }
    }

    /// Time passing.
    pub fn tick(&mut self, cfg: &AnsweringConfig, t: u64) -> Step {
        if self.answer.settled() {
            return Step::Settled(self.answer.clone());
        }
        let waited = t.saturating_sub(self.asked_at);
        if waited >= cfg.timeout_secs {
            // Silence is not a yes. It never becomes one by waiting.
            self.answer = Answer::TimedOut;
            return Step::Settled(Answer::TimedOut);
        }
        if !self.repeated && waited >= cfg.repeat_after_secs {
            self.repeated = true;
            return Step::Repeat(self.asked.clone());
        }
        Step::Wait
    }

    /// How the answer arrived, for the record.
    pub fn how(&self) -> Option<Channel> {
        match self.answer {
            Answer::Yes(c) | Answer::No(c) => Some(c),
            _ => None,
        }
    }
}

/// A line describing how an answer was given, for the journal.
pub fn describe(q: &Question) -> String {
    match (&q.answer, q.how()) {
        (Answer::Yes(_), Some(Channel::Gesture)) => "you gave a thumbs up".into(),
        (Answer::No(_), Some(Channel::Gesture)) => "you gave a thumbs down".into(),
        (Answer::Yes(_), Some(Channel::Typed)) => "you typed yes".into(),
        (Answer::No(_), Some(Channel::Typed)) => "you typed no".into(),
        (Answer::Yes(_), _) => "you said yes".into(),
        (Answer::No(_), _) => "you said no".into(),
        (Answer::TimedOut, _) => "you didn't answer, so I left it".into(),
        _ => "still waiting".into(),
    }
}
