//! Conversation / session manager.
//!
//! Holds enough context for follow-ups ("move it to the other screen") to
//! resolve, and parks a pending question or approval so the next thing you
//! say is read as an answer rather than a new command.

use crate::intent::Intent;
use crate::store::now;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub said: String,
    pub action: String,
    pub reply: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pending {
    Nothing,
    /// Atlas asked something and is waiting on the answer.
    Clarification(String),
    /// Atlas wants a yes/no before doing this.
    Approval(Intent, String),
}

#[derive(Debug, Clone)]
pub struct Session {
    pub turns: Vec<Turn>,
    pub pending: Pending,
    /// Approvals asked for while another was waiting (30 Sep 2026): kept in
    /// order and asked one at a time after it, rather than each new one
    /// replacing the last -- two parts of a request that each needed an OK
    /// left only the second to answer.
    pub queued: std::collections::VecDeque<(Intent, String)>,
    /// Last app acted on, so "move it" and "close that" resolve.
    pub last_app: Option<String>,
    pub current_project: Option<String>,
    pub started: u64,
    max_turns: usize,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            turns: Vec::new(),
            pending: Pending::Nothing,
            queued: std::collections::VecDeque::new(),
            last_app: None,
            current_project: None,
            started: now(),
            max_turns: 40,
        }
    }
}

impl Session {
    /// A session that keeps `max_turns` turns before folding the oldest away.
    ///
    /// `Default` hardcoded 40, which is the shipped `retention.session_turns`
    /// — so the setting and the behaviour agreed by coincidence and a person
    /// changing the file changed nothing. `Daemon::new` uses this now;
    /// `Default` stays for callers with no config to hand, and keeps the same
    /// number so nothing moves for them.
    pub fn holding(max_turns: usize) -> Session {
        Session { max_turns: max_turns.max(1), ..Session::default() }
    }

    pub fn record(&mut self, said: &str, intent: &Intent, reply: &str) {
        if let Some(app) = app_of(intent) {
            self.last_app = Some(app);
        }
        self.turns.push(Turn {
            said: said.to_string(),
            action: kind_of(intent).to_string(),
            reply: reply.to_string(),
            at: now(),
        });
        if self.turns.len() > self.max_turns {
            let drop = self.turns.len() - self.max_turns;
            self.turns.drain(0..drop);
        }
    }

    pub fn ask(&mut self, question: &str) {
        self.pending = Pending::Clarification(question.to_string());
    }

    /// Wait for a yes or no before doing `intent`. One already waiting keeps
    /// its place and this one queues behind it (`queued`); the same one twice
    /// is asked once.
    pub fn await_approval(&mut self, intent: Intent, description: &str) {
        if let Pending::Approval(waiting, _) = &self.pending {
            if *waiting != intent && !self.queued.iter().any(|(q, _)| *q == intent) {
                self.queued.push_back((intent, description.to_string()));
            }
            return;
        }
        self.pending = Pending::Approval(intent, description.to_string());
    }

    /// How many approvals are waiting: the one being asked and the queue.
    pub fn approvals_waiting(&self) -> usize {
        usize::from(matches!(self.pending, Pending::Approval(..))) + self.queued.len()
    }

    /// Every approval waiting, the one being asked first.
    pub fn all_approvals(&self) -> Vec<(Intent, String)> {
        let mut out = Vec::new();
        if let Pending::Approval(i, d) = &self.pending {
            out.push((i.clone(), d.clone()));
        }
        out.extend(self.queued.iter().cloned());
        out
    }

    /// Nothing waits any more: the question and everything queued behind it.
    pub fn drop_approvals(&mut self) {
        if matches!(self.pending, Pending::Approval(..)) {
            self.pending = Pending::Nothing;
        }
        self.queued.clear();
    }

    /// The question as it is put now: the one approval, or -- with more
    /// waiting -- how many, and the first of them.
    pub fn asking_line(&self) -> Option<String> {
        let Pending::Approval(_, first) = &self.pending else { return None };
        Some(match self.queued.len() {
            0 => first.clone(),
            n => format!("{} things need your OK. First: {}", count_word(n + 1), yes_or_no(first)),
        })
    }

    /// The one asked has been answered: the next in the queue is asked now,
    /// and the line that asks it. `None` when nothing else waits.
    pub fn ask_the_next(&mut self) -> Option<String> {
        let (i, d) = self.queued.pop_front()?;
        self.pending = Pending::Approval(i, d.clone());
        Some(match self.queued.len() {
            0 => format!("Next: {}", yes_or_no(&d)),
            n => format!("{} more need your OK. Next: {}", count_word(n + 1), yes_or_no(&d)),
        })
    }

    pub fn is_waiting(&self) -> bool {
        !matches!(self.pending, Pending::Nothing)
    }

    /// Recent turns, oldest first, for handing to the model.
    pub fn transcript(&self, n: usize) -> String {
        self.turns
            .iter()
            .rev()
            .take(n)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|t| format!("you: {}\natlas: {}", t.said, t.reply))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Steps taken this session, for storing as a workflow memory.
    pub fn steps(&self) -> Vec<String> {
        self.turns.iter().map(|t| t.action.clone()).collect()
    }
}

/// "Two", "Three" ... for saying how many.
fn count_word(n: usize) -> String {
    match n {
        2 => "Two".into(),
        3 => "Three".into(),
        4 => "Four".into(),
        5 => "Five".into(),
        n => n.to_string(),
    }
}

/// A "... Go ahead?" question as one of several: "... -- yes or no?".
fn yes_or_no(q: &str) -> String {
    let t = q.trim();
    let t = t.strip_suffix("Go ahead?").unwrap_or(t).trim_end();
    let t = t.trim_end_matches(['?', '.']).trim_end();
    format!("{t} -- yes or no?")
}

/// An answer to several approvals at once, one entry each in the order they
/// were asked: `Some(true)` yes, `Some(false)` no, `None` not answered (still
/// asked). "Yes to both", "no to all of them", "neither", "yes to the first
/// and no to the second", "no to the second". `None` when it isn't such an
/// answer -- a bare "yes" or "no" answers only the one being asked, as
/// always, and is left to `is_yes`/`is_no`. Strict like them: a clause that
/// isn't clearly a yes or a no to a named one makes the whole thing not an
/// answer.
pub fn answers_for_several(said: &str, n: usize) -> Option<Vec<Option<bool>>> {
    if n < 2 {
        return None;
    }
    let t = said.to_lowercase().replace('\u{2019}', "'").replace("don't", "dont").replace("do not", "dont");
    let t: String = t.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == ',' || c == ';' { c } else { ' ' }).collect();
    let flat = t.replace([',', ';'], " ");
    let flat: String = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    const ALL_YES: &[&str] = &[
        "yes to both", "yes both", "both", "both of them", "do both", "yes do both", "go ahead with both", "ok both",
        "okay both", "yes to all", "yes all", "all of them", "yes to all of them", "yes to everything", "do all of them",
        "yes to all three", "all three", "yes please both", "yes to both please", "both please", "sure both",
    ];
    const ALL_NO: &[&str] = &[
        "no to both", "no both", "neither", "neither of them", "no to either", "no to all", "no to all of them",
        "none", "none of them", "dont do either", "no to everything", "not either", "no neither", "no to all three",
    ];
    if ALL_YES.contains(&flat.as_str()) {
        return Some(vec![Some(true); n]);
    }
    if ALL_NO.contains(&flat.as_str()) {
        return Some(vec![Some(false); n]);
    }
    // Clause by clause: "yes to the first, no to the second".
    let mut out: Vec<Option<bool>> = vec![None; n];
    let mut any = false;
    for clause in t.split([',', ';']).flat_map(|c| c.split(" and ")).flat_map(|c| c.split(" but ")) {
        let words: Vec<&str> = clause.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        let no = words.iter().any(|w| matches!(*w, "no" | "nope" | "dont" | "not" | "skip" | "leave" | "cancel"));
        // "Do" is a yes only when nothing in the clause says no.
        let yes = !no && words.iter().any(|w| matches!(*w, "yes" | "yeah" | "yep" | "ok" | "okay" | "sure" | "do" | "approve"));
        let which = words.iter().find_map(|w| match *w {
            "first" | "1st" | "one" => Some(0),
            "second" | "2nd" | "two" => Some(1),
            "third" | "3rd" | "three" => Some(2),
            "fourth" | "4th" | "four" => Some(3),
            "last" => Some(n - 1),
            _ => None,
        });
        match (yes, no, which) {
            (true, false, Some(k)) | (false, true, Some(k)) if k < n => {
                out[k] = Some(yes);
                any = true;
            }
            // "yes to the first and the second": the ordinal alone takes the
            // last answer given.
            (false, false, Some(k)) if k < n && any => {
                let last = out.iter().rev().flatten().next().copied();
                out[k] = last;
            }
            _ => return None,
        }
    }
    any.then_some(out)
}

/// Deliberately strict. An ambiguous grunt must not count as consent for a
/// gated action, so anything not clearly affirmative reads as "no".
pub fn is_yes(s: &str) -> bool {
    let t: String = s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
    let t = t.trim();
    if matches!(
        t,
        "y" | "yes" | "yeah" | "yep" | "yup" | "sure" | "ok" | "okay" | "do it"
            | "go ahead" | "confirm" | "confirmed" | "please do" | "affirmative"
    ) {
        return true;
    }
    // "Yes please", "yeah go for it", "sure, do it", "sounds good" (30 Sep
    // 2026: only the bare words counted, so "yes please" threw the approval
    // away and went on as a new request). Opening with a yes, short, and
    // nothing in it that takes the yes back.
    let words: Vec<&str> = t.split_whitespace().collect();
    let opens = ["yes", "yeah", "yep", "yup", "sure", "ok", "okay", "absolutely", "definitely", "please"];
    let phrases = ["go ahead", "do it", "go for it", "sounds good", "of course", "why not", "please do", "that works", "lets do it", "yes please"];
    let opened = words.first().is_some_and(|w| opens.contains(w)) || phrases.iter().any(|p| t.starts_with(p));
    let takes_it_back = words.iter().any(|w| matches!(*w, "no" | "not" | "dont" | "wait" | "but" | "actually" | "hold" | "stop" | "cancel" | "never"))
        && !t.starts_with("why not");
    // Nothing but more yes: "sure, what's the weather" is a new question,
    // not an answer to "go ahead?".
    const MORE_YES: &[&str] = &[
        "yes", "yeah", "yep", "yup", "sure", "ok", "okay", "please", "go", "ahead", "do", "it", "for", "sounds", "good", "great",
        "thanks", "thank", "you", "lets", "that", "works", "fine", "perfect", "atlas", "absolutely", "definitely", "of", "course",
        "why", "not", "right", "correct", "exactly", "cool", "alright", "all", "send", "save", "book", "go", "on", "then",
    ];
    let only_yes = words.iter().all(|w| MORE_YES.contains(w));
    opened && !takes_it_back && only_yes && words.len() <= 6
}

/// "Yes, and don't ask me that again." Strict the same way `is_yes` is: it
/// is a yes *and* a standing decision, so only unmistakable wording counts.
pub fn is_always(s: &str) -> bool {
    let t: String = s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
    let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    matches!(
        t.as_str(),
        "always" | "yes always" | "yeah always" | "always do that" | "yes always do that"
            | "dont ask again" | "dont ask me again" | "yes dont ask again" | "yes dont ask me again"
            | "yes and dont ask again" | "yes and dont ask me again" | "you dont need to ask"
    )
}

/// Also strict. "Not now" is a no; a mumble is neither.
/// "That's all", "bye", "thanks, that's it": the conversation is over, and
/// the open floor after a reply closes (30 Sep 2026: "bye" was dropped as
/// a speech-to-text ghost, and nothing ended the floor but silence).
pub fn ends_the_conversation(s: &str) -> bool {
    let t: String = s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_start_matches("ok ").trim_start_matches("okay ").trim_start_matches("thanks ").trim_start_matches("thank you ").trim();
    matches!(
        t,
        "bye" | "goodbye" | "bye bye" | "see you" | "later" | "thats all" | "thats it" | "thats everything" | "nothing"
            | "nothing else" | "no thats it" | "no thats all" | "all done" | "im done" | "done" | "were done"
            | "thanks" | "thank you" | "thanks atlas" | "thank you atlas" | "cheers" | "good night" | "goodnight"
    )
}

pub fn is_no(s: &str) -> bool {
    let t: String = s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
    let t = t.trim();
    if matches!(
        t,
        "n" | "no" | "nope" | "nah" | "dont" | "do not" | "cancel" | "stop"
            | "not now" | "no thanks" | "leave it" | "never mind" | "negative"
    ) {
        return true;
    }
    // "No thanks, I'm good", "nah leave it", "not right now".
    let words: Vec<&str> = t.split_whitespace().collect();
    let opened = words.first().is_some_and(|w| matches!(*w, "no" | "nope" | "nah")) || ["not now", "not right now", "never mind", "leave it", "dont bother", "no need"].iter().any(|p| t.starts_with(p));
    let turns_to_yes = words.iter().skip(1).any(|w| matches!(*w, "yes" | "yeah" | "go" | "do" | "sure"));
    // Nothing but more no: "no more jokes" is a request, not an answer.
    const MORE_NO: &[&str] = &[
        "no", "nope", "nah", "thanks", "thank", "you", "im", "good", "fine", "leave", "it", "dont", "bother", "need", "not",
        "now", "right", "atlas", "its", "ok", "okay", "all", "that", "never", "mind", "thats", "alright", "maybe", "later",
    ];
    let only_no = words.iter().all(|w| MORE_NO.contains(w));
    opened && !turns_to_yes && only_no && words.len() <= 6
}

pub fn kind_of(i: &Intent) -> &'static str {
    match i {
        Intent::BriefOn(_) => "brief_on",
        Intent::WorkspaceOn => "workspace_on",
        Intent::WorkspaceOff => "workspace_off",
        Intent::OpenApp(_) => "open_app",
        Intent::CloseApp(_) => "close_app",
        Intent::FocusApp(_) => "focus_app",
        Intent::ViewDisplay => "view_display",
        Intent::CaptureWebcam => "capture_webcam",
        Intent::Research(_) => "research",
        Intent::McpTool(_) => "mcp_tool",
        Intent::Build(_) => "build_it",
        Intent::Improve(_) => "improve",
        Intent::Implement(_) => "implement",
        Intent::DesignReview(_) => "design_review",
        Intent::Animate(_) => "animate",
        Intent::Scene(_) => "scene3d",
        Intent::Explain(_) => "explain_code",
        Intent::PlainChange(_) => "plain_change",
        Intent::Booking(_) => "booking",
        Intent::Learn(_) => "learn_knowledge",
        Intent::Schedule(_) => "schedule",
        Intent::Agenda(_) => "agenda",
        Intent::Say(_) => "say",
        Intent::Gestures(_) => "gestures",
        Intent::Dictate(_) => "dictate",
        Intent::WhatsThere => "whats_there",
        Intent::WhatsThis => "whats_this",
        Intent::CallNotes(_) => "call_notes",
        Intent::Delegate(_) => "delegate",
        Intent::AfterMe => "after_me",
        Intent::NameThis(_) => "name_this",
        Intent::Recommend => "recommend",
        Intent::AddressAs(_) => "address_as",
        Intent::Pause => "pause",
        Intent::Resume => "resume",
        Intent::Outstanding => "outstanding",
        Intent::Queued => "queued",
        Intent::DraftPost(_) => "draft_post",
        Intent::Undo => "undo",
        Intent::BackUp => "back_up",
        Intent::RebuildIndex => "rebuild_index",
        Intent::WhatIHave(_) => "what_i_have",
        Intent::ModelTrace => "model_trace",
        Intent::AskTheRoom(_) => "ask_the_room",
        Intent::GotItWrong(_) => "got_it_wrong",
        Intent::ApplyLesson => "apply_lesson",
        Intent::HowAmIDoing => "how_am_i_doing",
        Intent::TimeSpent(_) => "time_spent",
        Intent::ClipHistory(_) => "clip_history",
        Intent::ScreenText(_) => "screen_text",
        Intent::MarketDay(_) => "market_day",
        Intent::WaitingFor(_) => "waiting_for",
        Intent::NoteReview(_) => "note_review",
        Intent::Launch(_) => "launch",
        Intent::TradeDay(_) => "trade_day",
        Intent::MeetingPrep(_) => "meeting_prep",
        Intent::Snippet(_) => "snippet",
        Intent::FindFile(_) => "find_file",
        Intent::Pdf(_) => "pdf",
        Intent::People(_) => "people",
        Intent::Feeds(_) => "feeds",
        Intent::Social(_) => "social",
        Intent::Opportunities(_) => "opportunities",
        Intent::Wit(_) => "wit",
        Intent::Receipt(_) => "receipt",
        Intent::Habit(_) => "habit",
        Intent::Cards(_) => "cards",
        Intent::Translate(_) => "translate",
        Intent::WhichModel => "which_model",
        Intent::SetMode(_) => "set_mode",
        Intent::MachineHealth => "machine_health",
        Intent::SelfCheck => "self_check",
        Intent::Shakedown => "shakedown",
        Intent::UseClipboard(_) => "use_clipboard",
        Intent::Rehearse(_) => "rehearse",
        Intent::Show(_) => "show_panel",
        Intent::Dismiss => "dismiss_panel",
        Intent::Ready => "ready",
        Intent::Capabilities(_) => "capabilities",
        Intent::History(_) => "history",
        Intent::CreateAccount(_) => "create_account",
        Intent::SignIn(_) => "sign_in",
        Intent::TypeCode(_) => "type_code",
        Intent::TwoFactor(_) => "two_factor",
        Intent::KeepAtIt => "keep_at_it",
        Intent::RunBuild(_) => "run_build",
        Intent::Goals(_) => "goals",
        Intent::Later(_) => "later",
        Intent::SortMail(_) => "sort_mail",
        Intent::SchedulePost(_) => "schedule_post",
        Intent::PressButton(_) => "press_button",
        Intent::MoveBigFiles(_) => "move_big_files",
        Intent::PcTune(_) => "pc_tune",
        Intent::TidyDesktop => "tidy_desktop",
        Intent::UseMic(_) => "use_mic",
        Intent::EditMedia(_) => "edit_media",
        Intent::EditPhoto(_) => "edit_photo",
        Intent::MakePicture(_) => "make_picture",
        Intent::SelfTest => "self_test",
        Intent::Operate(_) => "operate",
        Intent::Clock => "clock",
        Intent::SetKey(_) => "set_key",
        Intent::Languages(_) => "languages",
        Intent::TeachGesture(_) => "teach_gesture",
        Intent::MoneyAdvice(_) => "money_advice",
        Intent::CreatorAdvice(_) => "creator_advice",
        Intent::Overnight => "overnight",
        Intent::Dangling => "dangling",
        Intent::Suggestions(_) => "suggestions",
        Intent::DropTask(_) => "drop_task",
        Intent::Unzip(_) => "unzip",
        Intent::ReadDocument(_) => "read_document",
        Intent::WorkOnYourself(_) => "work_on_yourself",
        Intent::Unlock(_) => "unlock",
        Intent::Capture(_) => "capture",
        Intent::Mail(_) => "mail",
        Intent::Sync(_) => "sync",
        Intent::ReviewPost(_) => "review_post",
        Intent::TravelPrep => "travel_prep",
        Intent::Files(_) => "files",
        Intent::Why(_) => "why",
        Intent::Ask(_) => "ask",
        Intent::Pair(_) => "pair",
        Intent::AcceptPairing(_) => "accept_pairing",
        Intent::ForgetPeer(_) => "forget_peer",
        Intent::FinishSetup => "finish_setup",
        Intent::MuteTopic(_) => "mute_topic",
        Intent::ThisIsMe => "this_is_me",
        Intent::HandOver(_) => "hand_over",
        Intent::TakeItBack => "take_it_back",
        Intent::Recap => "recap",
        Intent::ActAlone => "act_alone",
        Intent::KnowledgeSize => "knowledge_size",
        Intent::Refile(_) => "refile",
        Intent::Diagnose(_) => "diagnose",
        Intent::WalkThrough(_) => "walk_through",
        Intent::Message(_) => "message",
        Intent::Messages => "messages",
        Intent::WhoIsIn(_) => "who_is_in",
        Intent::NameGroup(_) => "name_group",
        Intent::LeaveGroup(_) => "leave_group",
        Intent::ChangeGroup(_) => "change_group",
        Intent::Friend(_) => "friend",
        Intent::Updates(_) => "updates",
        Intent::Feedback(_) => "feedback",
        Intent::PhoneModel(_) => "phone_model",
        Intent::Unknown(_) => "unknown",
    }
}

pub fn app_of(i: &Intent) -> Option<String> {
    match i {
        Intent::OpenApp(a) | Intent::CloseApp(a) | Intent::FocusApp(a) => Some(a.clone()),
        _ => None,
    }
}
