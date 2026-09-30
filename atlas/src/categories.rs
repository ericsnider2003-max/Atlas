//! The four action categories from the spec, enforced rather than documented.
//!
//! These existed only in prose across the six baselines. The distinction that
//! actually matters is the last one: work done locally is yours, and work sent
//! to a third-party AI service leaves your machine. That difference should be
//! visible in the type system, not remembered by whoever writes the next
//! feature.

use crate::intent::Intent;
use crate::policy::Decision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Opening apps, moving windows, searching files, taking notes.
    LocalOperational,
    /// Web research, browsing, public lookups. Leaves the machine, but only
    /// as ordinary internet use.
    StandardExternal,
    /// Image and video work done on your hardware.
    LocalCreative,
    /// Generation or editing by a third-party AI service. Your content is
    /// uploaded. Always requires approval.
    ExternalAiCreative,
    /// The four categories above all describe work being done. This one
    /// describes a commitment being made, which is a different thing and the
    /// reason it needed its own slot: nothing above would have stopped a
    /// signup, because a signup looks like ordinary internet use right up
    /// until it isn't.
    AgreementExternal,
    /// Atlas changing its own code. Its own category because nothing else
    /// carries the same risk of a mistake that then hides itself.
    SelfModification,
}

impl Category {
    pub fn default_decision(&self) -> Decision {
        match self {
            Category::LocalOperational => Decision::AutoProceed,
            Category::StandardExternal => Decision::AutoProceed,
            Category::LocalCreative => Decision::ProceedAndReport,
            Category::ExternalAiCreative => Decision::RequireApproval,
            Category::AgreementExternal => Decision::RequireApproval,
            Category::SelfModification => Decision::RequireApproval,
        }
    }

    pub fn describe(&self) -> &'static str {
        match self {
            Category::LocalOperational => "local, on your machine",
            Category::StandardExternal => "ordinary internet use",
            Category::LocalCreative => "created locally on your hardware",
            Category::ExternalAiCreative => "sends your content to an outside AI service",
            Category::AgreementExternal => "commits you to something, as you",
            Category::SelfModification => "changes Atlas itself",
        }
    }
}

pub fn category_of(intent: &Intent) -> Category {
    match intent {
        // Another program, started by Atlas on this machine. What it goes on
        // to touch is that program's business; it is asked about either way
        // (`Daemon::mcp_gate`).
        Intent::McpTool(_) => Category::LocalOperational,
        Intent::Gestures(_) => Category::LocalOperational,
        // Keystrokes into a window you are looking at. Nothing leaves the
        // machine, nothing commits you to anything, and the text is yours
        // either way -- you would have typed it.
        Intent::Dictate(_) => Category::LocalOperational,
        // Recorded and transcribed on this machine; the model that writes
        // the summary is whichever you set up, and `brain::Endpoint` already
        // says when that's somewhere else.
        Intent::CallNotes(_) => Category::LocalOperational,
        Intent::WhatsThere | Intent::WhatsThis | Intent::NameThis(_) => {
            Category::LocalOperational
        }
        // A self-report never leaves the machine and changes nothing.
        Intent::Recommend => Category::LocalOperational,
        // Reading Atlas's shipped snags for a symptom you describe. Nothing
        // leaves the machine and nothing is changed.
        Intent::Diagnose(_) => Category::LocalOperational,
        // Reading back the steps of a shipped procedure. Nothing leaves the
        // machine and nothing is changed -- the same shape as `Diagnose`.
        Intent::WalkThrough(_) => Category::LocalOperational,
        // Rebuilding rewrites a file Atlas wrote, about files you wrote. It
        // never touches the notes themselves — that is why it is operational
        // rather than a modification you have to weigh.
        Intent::RebuildIndex | Intent::WhatIHave(_) => Category::LocalOperational,
        Intent::ModelTrace => Category::LocalOperational,
        Intent::AskTheRoom(_) => Category::LocalOperational,
        Intent::GotItWrong(_) | Intent::ApplyLesson | Intent::HowAmIDoing => {
            Category::LocalOperational
        }
        // Reading your own work log back to you, on this machine.
        Intent::TimeSpent(_) => Category::LocalOperational,
        // Round 11's tools: your own things, on this machine. Following a
        // site reads the web, but only a feed you chose.
        Intent::ClipHistory(_)
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
        | Intent::Wit(_)
        | Intent::Receipt(_)
        | Intent::Habit(_)
        | Intent::Cards(_)
        | Intent::Translate(_) => Category::LocalOperational,
        Intent::WhichModel => Category::LocalOperational,
        // A preference about how Atlas talks to you, nothing else.
        Intent::AddressAs(_) => Category::LocalOperational,
        // A signup is a commitment made as you, not a task done for you.
        Intent::CreateAccount(_) => Category::AgreementExternal,
        // Signing in acts as you on someone else's system.
        Intent::SignIn(_) => Category::AgreementExternal,
        // A code you asked for, typed into the box in front of you.
        Intent::TypeCode(_) => Category::LocalOperational,
        // A setting on one of your accounts, changed only after the
        // read-back and your yes.
        Intent::TwoFactor(_) => Category::StandardExternal,
        // Building on this machine, in a scratch copy.
        Intent::KeepAtIt => Category::LocalCreative,
        Intent::Goals(_) => Category::LocalOperational,
        Intent::Later(_) => Category::LocalOperational,
        // A pairing is a commitment made as you, exactly like a signup --
        // just handed to a person instead of a website.
        Intent::Pair(_) | Intent::AcceptPairing(_) => Category::AgreementExternal,
        // Revoking only removes standing that already existed. It makes
        // things safer, not riskier, so it doesn't need the same gate.
        Intent::ForgetPeer(_) => Category::LocalOperational,
        Intent::WorkOnYourself(_) | Intent::FinishSetup => Category::SelfModification,
        // These stay on your machine.
        // Ending a handover is a local proof against a local vault. It is
        // the same shape as `Unlock` because it is the same act, with one
        // extra consequence that also never leaves this machine.
        Intent::TakeItBack
        | Intent::HandOver(_)
        | Intent::MuteTopic(_)
        | Intent::ThisIsMe
        | Intent::Messages
        | Intent::WhoIsIn(_)
        | Intent::NameGroup(_)
        | Intent::LeaveGroup(_)
        | Intent::ChangeGroup(_)
        | Intent::Friend(_)
        // Atlas's own updates and feedback about it: its own upkeep.
        | Intent::Updates(_)
        | Intent::Feedback(_)
        | Intent::PhoneModel(_)
        | Intent::Unlock(_)
        | Intent::Capture(_)
        // Correcting a note's filing stays in the local notebook, the same
        // local operation as making the note in the first place.
        | Intent::Refile(_)
        | Intent::Files(_)
        | Intent::TravelPrep => {
            Category::LocalOperational
        }
        // A message goes to another person, as you. That is the same
        // commitment `draft_post` makes, aimed at one person instead of a
        // channel.
        Intent::Message(_) => Category::AgreementExternal,
        Intent::Mail(_) | Intent::Sync(_) => Category::StandardExternal,
        // Your own mailbox, rehearsed first and only moved on your "go".
        Intent::SortMail(_) => Category::StandardExternal,
        // Setting or cancelling the time is local. The words were already
        // approved at review, which is where speaking as you is agreed to;
        // asking again here made "cancel the post" need a yes.
        Intent::SchedulePost(_) => Category::LocalOperational,
        // A control in one of your apps, by name. The ones that can't be
        // taken back ask first, in the handler.
        Intent::PressButton(_) => Category::LocalOperational,
        Intent::MoveBigFiles(_) => Category::LocalOperational,
        // Shows the plan and waits for a yes; every move through `system::judge`.
        Intent::TidyDesktop => Category::LocalOperational,
        Intent::UseMic(_) => Category::LocalOperational,
        Intent::EditMedia(_) => media_category(MediaOp::Edit, false),
        // A new file beside the original; the original is only read.
        Intent::EditPhoto(_) => media_category(MediaOp::Edit, false),
        Intent::Clock => Category::LocalOperational,
        Intent::SetKey(_) => Category::LocalOperational,
        Intent::Languages(_) => Category::LocalOperational,
        Intent::TeachGesture(_) => Category::LocalOperational,
        Intent::MoneyAdvice(_) => Category::LocalOperational,
        Intent::CreatorAdvice(_) => Category::LocalOperational,
        Intent::Overnight => Category::LocalOperational,
        Intent::Dangling => Category::LocalOperational,
        Intent::Suggestions(_) => Category::LocalOperational,
        Intent::DropTask(_) => Category::LocalOperational,
        Intent::Unzip(_) => Category::LocalOperational,
        Intent::ReadDocument(_) => Category::LocalOperational,
        // Reads one of your own machines over a token-checked door. Nothing
        // of yours goes out and nothing over there changes.
        Intent::BriefOn(_) => Category::StandardExternal,
        Intent::ReviewPost(_) => Category::LocalCreative,
        Intent::WorkspaceOn
        | Intent::WorkspaceOff
        | Intent::OpenApp(_)
        | Intent::CloseApp(_)
        | Intent::FocusApp(_)
        | Intent::ViewDisplay
        | Intent::CaptureWebcam
        | Intent::Say(_)
        | Intent::Ask(_)
        | Intent::Delegate(_)
        | Intent::AfterMe
        | Intent::Pause
        | Intent::Resume
        | Intent::Outstanding
        | Intent::Queued
        | Intent::DraftPost(_)
        | Intent::Undo
        | Intent::BackUp
        | Intent::SetMode(_)
        | Intent::MachineHealth
        | Intent::SelfCheck
        | Intent::Shakedown
        | Intent::UseClipboard(_)
        | Intent::Rehearse(_)
        | Intent::Show(_)
        | Intent::Dismiss
        | Intent::Ready
        | Intent::Capabilities(_)
        | Intent::History(_)
        // Reading this session's turns back to you is a local read of state
        // Atlas already holds -- nothing leaves the machine.
        | Intent::Recap
        // The autonomy ledger read back is the same shape: a local read of
        // the confidence record Atlas already holds.
        | Intent::ActAlone
        // The size of the knowledge store read back is the same shape: a
        // local read of a count Atlas already holds.
        | Intent::KnowledgeSize
        | Intent::Why(_)
        | Intent::Unknown(_) => Category::LocalOperational,

        Intent::Research(_) => Category::StandardExternal,
        // May hand the drafting to a worker online, so it can leave the machine.
        Intent::Build(_) => Category::StandardExternal,
        Intent::Improve(_) => Category::StandardExternal,
        Intent::Implement(_) => Category::LocalOperational,
        Intent::DesignReview(_) => Category::LocalOperational,
        Intent::Animate(_) => Category::StandardExternal,
        Intent::Scene(_) => Category::StandardExternal,
        Intent::Explain(_) => Category::StandardExternal,
        Intent::PlainChange(_) => Category::LocalOperational,
        Intent::Booking(_) => Category::LocalOperational,
        Intent::Learn(_) => Category::LocalOperational,
        // The calendar is your own local record: adding to it and reading it
        // back are both ordinary local operations.
        Intent::Schedule(_) | Intent::Agenda(_) => Category::LocalOperational,
    }
}

/// Media work, which does not yet have its own intents but has policy that
/// must be settled before it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaOp {
    Analyze,
    Edit,
    Generate,
    Export,
}

/// Where a media operation runs decides its category, not what it does.
/// The same edit is Category 3 locally and Category 4 in the cloud.
pub fn media_category(_op: MediaOp, external_service: bool) -> Category {
    if external_service {
        Category::ExternalAiCreative
    } else {
        Category::LocalCreative
    }
}

/// Export and overwrite are consequential whoever does them — that is about
/// destroying an original, not about where the compute happened.
pub fn media_decision(op: MediaOp, external_service: bool, overwrites_original: bool) -> Decision {
    if overwrites_original || op == MediaOp::Export {
        return Decision::RequireApproval;
    }
    media_category(op, external_service).default_decision()
}

/// A sentence naming the category, for spoken approval prompts. Approval is
/// only meaningful if you are told what you are approving.
pub fn consent_line(cat: Category, what: &str) -> String {
    match cat {
        Category::ExternalAiCreative => {
            format!("{what} would send your content to an outside AI service. Go ahead?")
        }
        Category::AgreementExternal => {
            format!("{what} would sign you up and agree to their terms as you. Go ahead?")
        }
        _ => format!("{what} — {}. Go ahead?", cat.describe()),
    }
}
