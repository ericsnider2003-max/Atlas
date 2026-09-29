//! Approval gate. This is enforced in code, not described in a document.
//!
//! Design rule: the gate is the ONLY path to a side effect. A new intent that
//! is not classified defaults to RequireApproval — unclassified is not "safe".

use crate::error::{AtlasError, Result};
use crate::intent::Intent;
use serde::Deserialize;

/// Which actions may ever be relaxed by experience, and which may not.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PolicyConfig {
    /// Asked every single time. History is ignored for these.
    pub always_ask: Vec<String>,
    /// Consistent approval may relax these to ProceedAndReport.
    pub learnable: Vec<String>,
    pub min_samples: usize,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        PolicyConfig {
            always_ask: vec!["workspace_off".into(), "close_app".into()],
            learnable: vec!["focus_app".into()],
            min_samples: 5,
        }
    }
}

/// The spec's four-state model. Two states was not enough: "do it and tell me"
/// is genuinely different from "do it silently", and collapsing them either
/// makes Atlas chatty or makes it opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// High confidence, low risk, reversible. Just do it.
    AutoProceed,
    /// Low risk but not trivial — act, then say what was done.
    ProceedAndReport,
    /// Ambiguous target or destination. Ask before acting.
    AskClarification,
    /// Consequential. Explicit yes required.
    RequireApproval,
}

impl Decision {
    /// How strict, so two can be compared.
    pub fn severity(&self) -> u8 {
        match self {
            Decision::AutoProceed => 0,
            Decision::ProceedAndReport => 1,
            Decision::AskClarification => 2,
            Decision::RequireApproval => 3,
        }
    }

    /// The stricter of two, for callers outside this module.
    ///
    /// `understood::grade` needs it: its verdict and the voice-identity
    /// verdict are two independent escalations of the same action, and the
    /// answer is whichever asks for more, not whichever ran last.
    pub fn max(a: Decision, b: Decision) -> Decision {
        Decision::strictest(a, b)
    }

    /// The stricter of two. Used to apply a floor that relaxation can't dig
    /// under.
    fn strictest(a: Decision, b: Decision) -> Decision {
        if b.severity() > a.severity() {
            b
        } else {
            a
        }
    }

    pub fn needs_consent(&self) -> bool {
        matches!(self, Decision::RequireApproval | Decision::AskClarification)
    }
}

/// Baseline classification, before history is taken into account.
pub fn classify(intent: &Intent) -> Decision {
    match intent {
        // Another program's tool (`mcp`). The daemon asks before every call
        // (`Daemon::mcp_gate`, the last gate, which no history can relax)
        // unless that server's entry lets this one tool run unasked -- and a
        // tool you said may run unasked is not then questioned again here.
        Intent::McpTool(_) => Decision::AutoProceed,
        // Turning your own camera on for your own hands. Nothing leaves the
        // machine and nothing is acted on until a hand actually moves.
        Intent::Gestures(_) => Decision::AutoProceed,
        // Not `AutoProceed`, despite being local and asked for. Dictation is
        // the one capability that sends real keystrokes into a window Atlas
        // does not own, and the failure mode is a misheard sentence landing
        // somewhere public. `ProceedAndReport` is "act, then say what was
        // done" -- and what it reports is the window it is typing into, which
        // is exactly the fact you need to catch it being the wrong one.
        Intent::Dictate(_) => Decision::ProceedAndReport,
        // Call notes: the consent rules live in `consent::Recorder`, which
        // records nobody else without their yes. Reported, because you should
        // always hear that a recording started or stopped.
        Intent::CallNotes(_) => Decision::ProceedAndReport,
        // Typing into another app for you. The app itself is gated by
        // `grants` (an app Atlas doesn't know is asked about first), sends
        // are confirmed where the app says so, and you coming back ends it.
        Intent::Delegate(_) => Decision::ProceedAndReport,
        // Reading back what you arranged. It holds no passphrase — only the
        // kind of place and who was told — and nothing is sent anywhere.
        Intent::AfterMe => Decision::AutoProceed,
        // Rebuilding the index rewrites a file Atlas wrote from files you
        // wrote, and changes none of them. Reported rather than silent,
        // because an index that changes under you without a word is how a
        // trusted map goes wrong unnoticed.
        Intent::RebuildIndex => Decision::ProceedAndReport,
        // Reading the index back is the cheapest thing Atlas does.
        // Reading back a log Atlas keeps about itself. Nothing is sent, seen
        // or changed.
        Intent::WhatIHave(_) | Intent::ModelTrace => Decision::AutoProceed,
        // Reversible and read-only, but it spends one model call per seat and
        // takes a visible while on a local model. `ProceedAndReport` is the
        // grade for "act, then say what was done", which is exactly right for
        // something whose only cost is your patience.
        Intent::AskTheRoom(_) => Decision::ProceedAndReport,
        // Recording a correction and reading the scoreboard change nothing.
        Intent::GotItWrong(_) | Intent::HowAmIDoing | Intent::WhichModel => Decision::AutoProceed,
        // Reading the work log changes nothing.
        Intent::TimeSpent(_) => Decision::AutoProceed,
        // Round 11: reading and keeping your own things -- a note about a
        // person, a habit ticked, a card graded -- asked for and undoable.
        Intent::ClipHistory(_)
        | Intent::MarketDay(_)
        | Intent::WaitingFor(_)
        | Intent::NoteReview(_)
        | Intent::MeetingPrep(_)
        | Intent::FindFile(_)
        | Intent::People(_)
        | Intent::Feeds(_)
        | Intent::Social(_)
        | Intent::Receipt(_)
        | Intent::Habit(_)
        | Intent::Cards(_)
        | Intent::Translate(_)
        | Intent::TradeDay(_) => Decision::AutoProceed,
        // These act on the machine: type a snippet into the app in front,
        // open an app, replace the clipboard with a window's text, write a
        // new PDF beside the old (never over it). Done and said, like
        // dictation.
        Intent::ScreenText(_)
        | Intent::Launch(_)
        | Intent::Snippet(_)
        | Intent::Pdf(_) => Decision::ProceedAndReport,
        // Atlas rewriting its own standing instructions — the one thing here
        // that changes how it behaves next time.
        //
        // `ProceedAndReport`, not `RequireApproval`, and the distinction is
        // the whole mechanism rather than a softening of it. This intent is
        // only ever reached as the *answer* to `offer_to_mend`, which put the
        // exact new line in front of you and asked. Grading it
        // `RequireApproval` made the yes-path ask a second time for the same
        // edit, so saying yes wrote nothing — which is what the tests found.
        // The proposal is the approval; `apply_lesson` has nothing to write
        // unless one was made and shown.
        Intent::ApplyLesson => Decision::ProceedAndReport,
        // Signing you up is a commitment made as you. Never automatic.
        Intent::CreateAccount(_) => Decision::RequireApproval,
        // Signing in acts as you on someone else's system. The first time on
        // a site is a decision; after that the grant carries it.
        Intent::SignIn(_) => Decision::RequireApproval,
        // You asked for this code to be typed, into what's in front of you.
        Intent::TypeCode(_) => Decision::AutoProceed,
        // Its own read-back and yes (`confirmed`) is the approval; asking a
        // second time first would be two questions for one decision.
        Intent::TwoFactor(_) => Decision::AutoProceed,
        // You asked for it, and it works in a scratch copy.
        Intent::KeepAtIt => Decision::AutoProceed,
        // Your own list of what you're aiming at.
        Intent::Goals(_) => Decision::AutoProceed,
        Intent::Later(_) => Decision::AutoProceed,
        // Pairing hands someone standing access to reach you -- the same
        // "commitment made as you" shape as signing up, just aimed at a
        // person instead of a website. Never automatic, and the floor in
        // classify_with_policy means history can never relax it either.
        Intent::Pair(_) | Intent::AcceptPairing(_) => Decision::RequireApproval,
        // Changing its own code is never automatic, however many times you've
        // approved one before.
        Intent::WorkOnYourself(_) => Decision::RequireApproval,
        Intent::OpenApp(_)
        | Intent::FocusApp(_)
        | Intent::ViewDisplay
        | Intent::Research(_)
        // Building writes only into a throwaway sandbox and sends nothing on
        // its own, so it proceeds and reports rather than gating.
        | Intent::Build(_)
        // Improving a project files a *proposed* change into its queue; it
        // does not touch real files, so it proceeds and reports.
        | Intent::Improve(_)
        // Implementing applies a change you already reviewed and explicitly
        // named — the "implement it" is the approval, so a second gate would
        // be a nag. An undo backup is kept.
        | Intent::Implement(_)
        // Reviewing a design only reads markup and reports — it changes
        // nothing, so it proceeds like the other read-only capabilities.
        | Intent::DesignReview(_)
        // Drawing an animation writes only a file you open — it sends nothing,
        // so it proceeds and reports like building does.
        | Intent::Animate(_)
        | Intent::Scene(_)
        // Explaining code only reads and reports — it changes nothing.
        | Intent::Explain(_)
        // Explaining a staged change as behaviour only reads what's already
        // staged and reports it — it neither lands nor discards anything.
        | Intent::PlainChange(_)
        | Intent::Booking(_)
        | Intent::Learn(_)
        // The calendar is your own local record. Adding an event and reading
        // the agenda both just proceed — a clash is reported, not gated on.
        | Intent::Schedule(_)
        | Intent::Agenda(_)
        | Intent::CaptureWebcam
        | Intent::Say(_)
        | Intent::Pause
        | Intent::Resume
        | Intent::Outstanding
        | Intent::Queued
        // Drafting is safe by construction — a draft cannot be sent without
        // its own separate approval.
        | Intent::DraftPost(_)
        | Intent::SetMode(_)
        | Intent::MachineHealth
        | Intent::SelfCheck
        | Intent::Shakedown
        | Intent::UseClipboard(_)
        // A rehearsal is the opposite of consequential — it exists to show
        // you what would happen without any of it happening.
        | Intent::Rehearse(_)
        | Intent::Show(_)
        | Intent::Dismiss
        | Intent::Ready
        | Intent::Capabilities(_)
        | Intent::History(_)
        // Reading this session's own turns back to you. Nothing is sent, seen
        // or changed -- the same read-only shape as `History` above it.
        | Intent::Recap
        // Reading the autonomy ledger back to you: what Atlas will do on its
        // own and where it still asks. A read of state it already holds,
        // decides nothing, changes nothing -- same shape as `Recap`.
        | Intent::ActAlone
        // Reading the size of the knowledge store back to you: a count Atlas
        // already holds, decides nothing, changes nothing -- same read-only
        // shape as `ActAlone` above it.
        | Intent::KnowledgeSize
        | Intent::Unlock(_)
        // Deliberately not `RequireApproval`. This intent's whole body is a
        // stricter gate than approval -- it asks for the vault passphrase --
        // and asking "go ahead?" first would put a spoken yes in front of a
        // typed secret, which is the wrong way round. It is also the one
        // thing that must stay sayable while handed over: an approval
        // question nobody present can answer is a way out that is closed.
        | Intent::TakeItBack
        // Entering a handover only ever *narrows* what Atlas will do, so
        // there is nothing to approve and asking would be the thing that
        // makes it unusable: you say it with somebody standing there waiting
        // for the laptop. `handover.rs` has the long version.
        | Intent::HandOver(_)
        // Reading your own messages back to you.
        | Intent::Messages
        | Intent::WhoIsIn(_)
        | Intent::NameGroup(_)
        | Intent::LeaveGroup(_)
        // A group you own, changed because you said so out loud: it's done
        // and said, not asked about -- the same weight as leaving one.
        | Intent::ChangeGroup(_)
        // A friend: sending the link, or pasting theirs, is the decision. A
        // second "go ahead?" would be the whole process this replaces.
        | Intent::Friend(_)
        // Saying "install the update" is the decision, as typing it is. Going
        // back and sending feedback ask their own question, with exactly what
        // will happen in it, before anything changes or leaves.
        | Intent::Updates(_)
        | Intent::Feedback(_)
        // Asking for the phone's model is the decision; it says how big first.
        | Intent::PhoneModel(_)
        | Intent::Capture(_)
        | Intent::Mail(_)
        // It rehearses and waits for "go"; that is its approval (G1).
        | Intent::SortMail(_)
        // The post's own approval (each post, its exact words) is the gate;
        // setting its time is part of that answer.
        | Intent::SchedulePost(_)
        // Asks itself, for a button that can't be undone (G4).
        | Intent::PressButton(_)
        // Shows the plan and waits for a yes itself (G5).
        | Intent::MoveBigFiles(_)
        // Works on a copy; the original is only touched after two yeses.
        | Intent::EditMedia(_)
        // Writes a new file beside the original, never over it.
        | Intent::EditPhoto(_)
        | Intent::Clock
        | Intent::SetKey(_)
        | Intent::Languages(_)
        | Intent::TeachGesture(_)
        | Intent::MoneyAdvice(_)
        | Intent::CreatorAdvice(_)
        | Intent::Overnight
        | Intent::Dangling
        | Intent::Suggestions(_)
        | Intent::DropTask(_)
        | Intent::Unzip(_)
        | Intent::ReadDocument(_)
        | Intent::Sync(_)
        // Read-only, and you asked for it.
        | Intent::BriefOn(_)
        | Intent::ReviewPost(_)
        | Intent::TravelPrep
        | Intent::Files(_)
        | Intent::Why(_)
        // Looking is reading. Nothing leaves the machine, nothing is acted
        // on, and the camera's own switch is a separate decision Eric has
        // already made or not made.
        | Intent::WhatsThere
        | Intent::WhatsThis
        // A self-report never leaves the machine and changes nothing.
        | Intent::Recommend
        // Diagnosing a symptom only reads Atlas's shipped procedures and says
        // what the likely cause and fix are. Nothing leaves the machine and
        // nothing is acted on -- the same read-only shape as `Recommend`.
        | Intent::Diagnose(_)
        // Walking through a shipped procedure only reads Atlas's own how-to
        // knowledge and reads the steps back. Nothing leaves the machine and
        // nothing is acted on -- the same read-only shape as `Diagnose`.
        | Intent::WalkThrough(_)
        // Changing how Atlas addresses you writes to Atlas's own state and
        // nowhere else. Same shape as naming a thing, undone the same way.
        | Intent::AddressAs(_)
        // Naming a thing writes to Atlas's own album and nowhere else. It is
        // undone by forgetting it.
        | Intent::NameThis(_)
        // Correcting how a captured note was filed writes to Atlas's own
        // notebook and nowhere else -- the same shape as naming a thing. It
        // makes a note easier to find later, never sends or acts on anything.
        | Intent::Refile(_) => Decision::AutoProceed,

        // Said to a person, as you -- so it is announced rather than silent.
        // Deliberately not `RequireApproval`: you said the sentence, out
        // loud, naming who it was for. Asking "go ahead?" after that is the
        // confirmation-on-everything that gets turned off wholesale, and
        // then nothing is confirmed. What protects you here is that it is
        // announced and that the message sits visibly in the outbox.
        Intent::Message(_) => Decision::ProceedAndReport,

        // All three change something and all three are worth hearing about:
        // setup writes config, muting changes what Atlas will tell you
        // afterwards, and learning a face changes who Atlas thinks you are.
        // Announced rather than asked, because you said the sentence.
        Intent::FinishSetup => Decision::ProceedAndReport,
        Intent::MuteTopic(_) => Decision::ProceedAndReport,
        Intent::ThisIsMe => Decision::ProceedAndReport,

        // Both move files around, so both get announced.
        Intent::BackUp => Decision::ProceedAndReport,
        Intent::Undo => Decision::ProceedAndReport,

        // Not trivial: it moves windows around and takes a few seconds.
        Intent::WorkspaceOn => Decision::ProceedAndReport,

        // Closing apps can discard unsaved work.
        Intent::WorkspaceOff | Intent::CloseApp(_) => Decision::RequireApproval,

        // Revoking only removes standing that already existed -- it makes
        // things safer, not riskier, so it can announce rather than ask.
        Intent::ForgetPeer(_) => Decision::ProceedAndReport,

        Intent::Ask(_) => Decision::AskClarification,
        Intent::Unknown(_) => Decision::RequireApproval,
    }
}

pub fn classify_with_policy(
    intent: &Intent,
    mem: &crate::memory::Memory,
    cfg: &PolicyConfig,
) -> Decision {
    // The action categories are a floor, not a suggestion. Approval demanded
    // by what a thing *is* can't be learned away by having approved it before
    // — five hundred yeses to sending your content out should not teach Atlas
    // to stop asking.
    let floor = crate::categories::category_of(intent).default_decision();
    let base = Decision::strictest(classify(intent), floor);
    if base != Decision::RequireApproval {
        return base;
    }
    if floor == Decision::RequireApproval {
        return base;
    }
    let kind = crate::session::kind_of(intent);

    // Never learn to auto-run something we failed to understand.
    if kind == "unknown" {
        return base;
    }
    // The always-ask list wins over everything, permanently.
    if cfg.always_ask.iter().any(|k| k == kind) {
        return base;
    }
    // Only explicitly learnable actions can be relaxed at all.
    if !cfg.learnable.iter().any(|k| k == kind) {
        return base;
    }
    match (mem.times_seen(kind), mem.approval_rate(kind)) {
        (n, Some(rate)) if n >= cfg.min_samples && rate >= 0.9 => Decision::ProceedAndReport,
        _ => base,
    }
}

/// Supplies a yes/no when an intent is gated.
pub trait Approver {
    fn approve(&self, description: &str) -> bool;
}

pub struct DenyAll;
impl Approver for DenyAll {
    fn approve(&self, _: &str) -> bool {
        false
    }
}

pub struct AllowAll;
impl Approver for AllowAll {
    fn approve(&self, _: &str) -> bool {
        true
    }
}

pub fn gate(intent: &Intent, approver: &dyn Approver) -> Result<()> {
    // The category floor applies here too. `classify_with_policy` applies it
    // on the daemon path, and this one called bare `classify`, so the two
    // routes into the same action could have disagreed about whether it
    // needed your say-so.
    //
    // They do not disagree today — measured across every intent, `classify` is
    // already at least as strict as its category. That is a fact about the
    // current rules rather than about the code, and one new intent whose
    // `classify` arm falls through to AutoProceed would open the gap silently.
    // `the_two_consent_routes_cannot_disagree` fails the build if that
    // happens.
    let floor = crate::categories::category_of(intent).default_decision();
    gate_with(Decision::strictest(classify(intent), floor), intent, approver)
}

fn gate_with(decision: Decision, intent: &Intent, approver: &dyn Approver) -> Result<()> {
    match decision {
        Decision::AutoProceed | Decision::ProceedAndReport => Ok(()),
        Decision::AskClarification | Decision::RequireApproval => {
            // Never `{:?}` on an intent -- that reaches the screen as the
            // enum's Rust name, and this text is exactly what a person
            // reads when asked to approve something.
            let desc = crate::session::kind_of(intent).replace('_', " ");
            if approver.approve(&desc) {
                Ok(())
            } else {
                Err(AtlasError::ApprovalRequired(desc))
            }
        }
    }
}
