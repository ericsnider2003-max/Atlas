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

    pub fn await_approval(&mut self, intent: Intent, description: &str) {
        self.pending = Pending::Approval(intent, description.to_string());
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

/// Deliberately strict. An ambiguous grunt must not count as consent for a
/// gated action, so anything not clearly affirmative reads as "no".
pub fn is_yes(s: &str) -> bool {
    let t: String = s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
    let t = t.trim();
    matches!(
        t,
        "y" | "yes" | "yeah" | "yep" | "yup" | "sure" | "ok" | "okay" | "do it"
            | "go ahead" | "confirm" | "confirmed" | "please do" | "affirmative"
    )
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
pub fn is_no(s: &str) -> bool {
    let t: String = s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
    matches!(
        t.trim(),
        "n" | "no" | "nope" | "nah" | "dont" | "do not" | "cancel" | "stop"
            | "not now" | "no thanks" | "leave it" | "never mind" | "negative"
    )
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
        Intent::Goals(_) => "goals",
        Intent::Later(_) => "later",
        Intent::SortMail(_) => "sort_mail",
        Intent::SchedulePost(_) => "schedule_post",
        Intent::PressButton(_) => "press_button",
        Intent::MoveBigFiles(_) => "move_big_files",
        Intent::EditMedia(_) => "edit_media",
        Intent::EditPhoto(_) => "edit_photo",
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
