//! Phrase -> Intent. Deterministic, config-driven, no model in the loop.
//! Longest phrase wins, so "open workspace" never gets eaten by "open".

use crate::config::CommandsConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    WorkspaceOn,
    WorkspaceOff,
    OpenApp(String),
    CloseApp(String),
    FocusApp(String),
    ViewDisplay,
    Research(String),
    /// Point the webcam at whatever you're holding up.
    CaptureWebcam,
    /// Nothing to do — just talk back.
    Say(String),
    /// Stop talking and suspend work until told otherwise.
    Pause,
    /// Watch my hands, or stop.
    ///
    /// Steering could never be entered before this existed: `steering_until`
    /// was only ever set inside the steering handler, which only ran once
    /// already steering. The mode was unreachable — the same hollow shape as
    /// everything else in this codebase, in code written two days earlier.
    Gestures(bool),
    Resume,
    /// "What couldn't you do?" — the outstanding list.
    Outstanding,
    /// "What's queued to go out?" — posts and scheduled sends.
    Queued,
    /// Write a post for a channel. Never sends; that needs approval.
    DraftPost(String),
    /// Put back the last thing Atlas moved or overwrote.
    Undo,
    /// Copy everything Atlas has learned somewhere safe.
    BackUp,
    /// Switch workspace mode: focus, call, research.
    SetMode(String),
    /// "How's the machine?"
    MachineHealth,
    /// "Check yourself" / "are you working" — a fast on-device self-check of the
    /// parts everything rests on (store, memory, screen, model) plus an honest
    /// count of what's ready here. Not the dev test suite; a setup-time "does
    /// this work on my machine" in seconds.
    SelfCheck,
    /// "Run a shakedown" / "commission yourself" — walk every capability that's
    /// never run on this machine and verify what can be verified without you:
    /// read-only checks now, visible ones on your go-ahead, data ones on first
    /// real use, and name the few that need your eyes. The setup-time answer to
    /// "how much do I have to sit and confirm by hand".
    Shakedown,
    /// Use whatever is on the clipboard. The argument is what to do with it.
    UseClipboard(String),
    /// Show what a command would do, without doing it.
    Rehearse(String),
    /// Put a panel on screen: outstanding, thinking, settings.
    Show(String),
    /// Take it off again.
    Dismiss,
    /// "I'm ready" — the waking moment and the brief.
    Ready,
    /// "What can you do?" / "what's new?" / "can you X?"
    Capabilities(String),
    /// "What did you do?" / "undo that" / "put it back"
    History(String),
    /// "Sign me up for X" — making an account, which is not signing in.
    CreateAccount(String),
    /// "Sign me into X" / "log me in"
    SignIn(String),
    /// Type a two-factor code: read out, or found in your email or texts.
    /// Carries the whole utterance, since where the code is ("in my email")
    /// is part of what was said.
    TypeCode(String),
    /// Turn two-factor on or off on a site. The whole utterance, since "on"
    /// or "off" is in the phrase that routed here.
    TwoFactor(String),
    /// Keep going on the last build that ran out of tries, as a long job.
    KeepAtIt,
    /// A goal of yours: set, worked on, dropped, or listed. The whole
    /// utterance, since which of those it is is in the phrase.
    Goals(String),
    /// The list for later: add what Atlas just said, read it, take one off.
    Later(String),
    /// Sort your mailbox, or delete a category you name (G1). The whole
    /// utterance, since "delete the noise" is in it.
    SortMail(String),
    /// When a post goes, or cancelling it (G2). The whole utterance.
    SchedulePost(String),
    /// Press a button by its name in an app (G4). The whole utterance.
    PressButton(String),
    /// Move Atlas's big folders to another drive, or say where they went (G5).
    MoveBigFiles(String),
    /// File the loose files on your desktop into folders, after showing the
    /// plan and hearing yes (Eric, 29 Sep 2026: "organize my desktop").
    TidyDesktop,
    /// Record from the microphone you name ("webcam", "headset"), and keep
    /// to it (Eric, 29 Sep 2026: "use my webcam mic"). The kind named.
    UseMic(String),
    /// Edit a video you name, on a copy (G8). The whole utterance.
    EditMedia(String),
    /// Edit a photo, or every photo in a folder, on a new copy (`photo`). The whole utterance.
    EditPhoto(String),
    /// Make a picture on this machine (`imagemake`): "draw me a lighthouse".
    MakePicture(String),
    /// What time and day it is, from this machine's clock.
    Clock,
    /// Change the push-to-talk key or the typing-box key by saying it (Eric,
    /// 26 Sep 2026: customizable, no Alt). The whole utterance.
    SetKey(String),
    /// Which languages Atlas can hear and whether it translates them (Eric, H5).
    Languages(String),
    /// Teach Atlas a new hand gesture by showing it (Eric, H6). The whole
    /// utterance, for the name and what it does.
    TeachGesture(String),
    /// General money questions: how long to keep records, how Atlas would read
    /// your bank (Eric, J: kept, general only). The whole utterance.
    MoneyAdvice(String),
    /// Video and content-creator advice: grading order, formats and export
    /// settings, brand deals, your profile (Eric, I). The whole utterance.
    CreatorAdvice(String),
    /// The long account of the night's work, on request (Eric, H13j).
    Overnight,
    /// Which notes point at things that don't exist yet (Eric, H13d).
    Dangling,
    /// Turn one of Atlas's own suggestions on or off by name, or list them
    /// (Eric, H13b). The whole utterance.
    Suggestions(String),
    /// Take something off your list, or bring back something you dropped
    /// (Eric, H8). The whole utterance.
    DropTask(String),
    /// Unzip a file, scanning it and everything it unpacks to (Eric, H3).
    /// The whole utterance, for the path in it.
    Unzip(String),
    /// Read a PDF, a Word file or a scan out loud (Eric, H3). The whole
    /// utterance, for the path in it.
    ReadDocument(String),
    /// "Fix that yourself" / "work on yourself"
    WorkOnYourself(String),
    /// Build code from a description: "build me a script that…", "write a
    /// function that…". Atlas writes it, checks it against the real
    /// toolchain, and hands over what passes.
    Build(String),
    /// Work on one of your projects: "on the Atlas project, add X",
    /// "improve the date parsing in Homelab". Atlas scopes it, builds it
    /// (itself or delegated), checks it, and files a proposed change in that
    /// project's queue for you to implement when ready.
    Improve(String),
    /// Apply a change you've reviewed: "implement the date parser". Finds the
    /// ready change by title and writes it into the project.
    Implement(String),
    /// Review a page's design against the house style: "review the design of
    /// index.html", "check this page's design". Atlas reads the markup and
    /// reports where it's off the spacing scale, using typed-in colours instead
    /// of tokens, or failing accessibility — the checkable part of taste, never
    /// a verdict on whether it's the right design.
    DesignReview(String),
    /// Make an animation: "animate a bouncing ball, 600x400, for 3 seconds".
    /// Atlas draws it as a self-contained SVG, checks it renders and matches
    /// the size/duration asked for, and saves it for you to open and judge —
    /// the checkable part, never a claim the motion looks good.
    Animate(String),
    /// Draw a 3-D scene: "draw a 3d scene of a red ball on a box". A model
    /// drafts the scene; Atlas draws it itself (a still and a turntable GIF),
    /// and in Blender too when Blender is installed.
    Scene(String),
    /// Explain code in plain English: "explain this: <code>", "explain
    /// src/foo.rs". Atlas writes a non-coder explanation, checks it reads
    /// plainly (no leaked code, right length, flags jargon), and hands it over
    /// — the checkable part, never a claim it's the right explanation.
    Explain(String),
    /// Explain the change Atlas has staged as *behaviour*, not code: "what
    /// will that change do?", "what's different?". It reads the tests the
    /// change adds and drops — sentences about behaviour in this tree — and
    /// tells you what will now happen and what it no longer promises, with no
    /// diff to read. The checkable half: it can't judge whether the change is
    /// right, only name what will be different.
    PlainChange(String),
    /// Someone proposed a time, or you're answering one. Atlas reads what was
    /// proposed, checks it against your calendar, and lays out what fits and
    /// what it could offer instead — then stops. You say accept, decline, or
    /// offer another time, and only an accept writes it to your calendar. Atlas
    /// never books or replies on its own; a time with someone else is a promise
    /// only you can make.
    Booking(String),
    /// Learn a whole body of knowledge at once: "learn this: <paste>", "learn
    /// from <a file>". Atlas breaks the text into many discrete facts, tags and
    /// indexes each, and folds them into its one knowledge book — the way its
    /// knowledge base grows vast without a hundred separate notes. Reference
    /// knowledge, kept and recalled like anything else it knows.
    Learn(String),
    /// Put something on your calendar: "schedule lunch tomorrow at 12",
    /// "add dentist on Monday". Atlas reads the time, notes any clash, and
    /// files it — offline, in your own calendar.
    Schedule(String),
    /// Ask what's on: "what's on today", "my calendar this week", "agenda".
    Agenda(String),
    /// "Unlock" / "the passphrase is..."
    Unlock(String),
    /// "Note that..." / "remember this" — capture, filed afterwards.
    Capture(String),
    /// "What's in my inbox" / "clear out my email"
    Mail(String),
    /// "Sync" / "send this to my phone"
    Sync(String),
    /// "Check this before I post it"
    ReviewPost(String),
    /// "Get me ready to travel"
    TravelPrep,
    /// "Convert this" / "join these"
    Files(String),
    /// "Why did you do that?"
    Why(String),
    /// "How's the homelab doing" / "brief me on the server"
    ///
    /// Asking one of *your other Atlases* how it is getting on. A server with
    /// nobody logged into it is the machine that most needs looking in on and
    /// the one nobody looks at.
    BriefOn(String),
    /// "What do you see?" — look at the room and say what's there.
    WhatsThere,
    /// "What's this?" — the one thing being held up or pointed at.
    WhatsThis,
    /// Call notes: "start", "everyone", "agreed", "declined" or "stop".
    CallNotes(String),
    /// "Finish this conversation until I'm back" / "draft a reply to this":
    /// work the window in front on your behalf (`delegate`). What was said,
    /// because the words decide drafting from carrying on.
    Delegate(String),
    /// "Where's my envelope?" — your after-me arrangement, read back. Only
    /// ever when asked (Eric, 24 Sep 2026: not announced "unless asked").
    AfterMe,
    /// "This is my mug" — put a name to what it can see, so it knows it next
    /// time. What makes the vocabulary open rather than a list somebody else
    /// chose.
    NameThis(String),
    /// "What could make you faster" / "what am I missing" — a self-report
    /// against this machine's real, measured capability, not a fixed
    /// laptop's numbers.
    Recommend,
    /// "Call me boss" / "stop calling me that" / "just talk" — the whole
    /// utterance, not a trimmed argument: `returning::address_change` does
    /// its own phrase detection on the full raw text.
    AddressAs(String),
    /// "Pair with Sarah" — generate an invite for someone by name. A
    /// commitment made as you, the same shape as `CreateAccount`, just
    /// aimed at another Atlas instead of a website.
    Pair(String),
    /// "Accept this pairing: ATLAS-KIN-1:..." — the other half. In practice
    /// this arrives typed or pasted, not spoken; a token isn't something
    /// anyone can reliably dictate.
    AcceptPairing(String),
    /// "Forget my pairing with Sarah" — undo one, both directions.
    ForgetPeer(String),
    /// "Start dictating" — the words go into the window you are looking at,
    /// not to Atlas.
    ///
    /// The argument is anything said after the trigger, kept raw: dictated
    /// text is the one place where normalising case and punctuation out of
    /// what you said is the opposite of the point. With nothing after it,
    /// dictation simply starts.
    ///
    /// Once on, this intent is not consulted again — `daemon::turn_from`
    /// routes every following utterance straight to `dictate::Dictation`
    /// before the parser sees it. That is what makes it a mode rather than a
    /// command: while dictating, "open chrome" is three words to type.
    Dictate(String),
    /// "Rebuild the index" — make the notes index agree with the folder again.
    ///
    /// The relief `nudge::drifted` offers. An offer whose acceptance runs
    /// nothing is the failure this codebase keeps finding, so the phrase it
    /// puts in front of you has to parse to something real.
    RebuildIndex,
    /// "What do you have on X" — which notes are worth opening, from the
    /// index alone. With no subject: how much Atlas knows it has.
    WhatIHave(String),
    /// "What have you been asking the model" — the flight recorder, read back.
    ModelTrace,
    /// You told Atlas it got something wrong, and ideally what right is.
    GotItWrong(String),
    /// Write the proposed lesson down. This is what the offer's own relief
    /// parses to — an offer whose command parses to nothing is an offer to do
    /// nothing.
    ApplyLesson,
    /// "Have you got any better?" — the correction scoreboard.
    HowAmIDoing,
    /// "Where did my time go today?" — the work log (`worklog`).
    TimeSpent(String),
    /// Clipboard history: "what did I copy", "paste 2", on/off. (round 11, `workday`)
    ClipHistory(String),
    /// Copy the text off the window in front (OCR, on this machine). (round 11, `workday`)
    ScreenText(String),
    /// The US market's calendar: holidays, early closes, CPI and the big releases. (round 11, `workday`)
    MarketDay(String),
    /// Who owes you a reply, and what you promised, from your mail. (round 11, `workday`)
    WaitingFor(String),
    /// A weekly look back over your captured notes. (round 11, `workday`)
    NoteReview(String),
    /// Open an app or Start-menu shortcut by part of its name. (round 11, `workday`)
    Launch(String),
    /// The trading day's check-in before the session and journal after it. (round 11, `workday`)
    TradeDay(String),
    /// A short brief on the next meeting: who, what you last wrote, what's open. (round 11, `workday`)
    MeetingPrep(String),
    /// Text you type often, kept and typed for you. (round 11, `workday`)
    Snippet(String),
    /// Find a file by part of its name, its type and when. (round 11, `workday`)
    FindFile(String),
    /// Merge, split and sign PDFs from the last list of files. (round 11, `workday`)
    Pdf(String),
    /// The people you deal with: notes, keeping in touch, birthdays. (round 11, `workday`)
    People(String),
    /// Sites you follow: what's new, read one, save one for later. (round 11, `workday`)
    Feeds(String),
    /// Your social accounts' numbers and the people you watch. (`social`)
    Social(String),
    /// Opportunities the hunter found (gigs, grants, niches): the list, more
    /// on one, not interested, save, and what to look for. (`hunting`)
    Opportunities(String),
    /// How much of a smart-ass Atlas may be: "tone it down". (`talkback`)
    Wit(String),
    /// Keep a receipt off the screen or the clipboard; what you spent where. (round 11, `workday`)
    Receipt(String),
    /// Habits, counted by strength rather than streaks. (round 11, `workday`)
    Habit(String),
    /// Flashcards on a spaced-repetition schedule (FSRS). (round 11, `workday`)
    Cards(String),
    /// Translate on this machine, with the numbers and names checked. (round 11, `workday`)
    Translate(String),
    /// "Which model are you using?" — what is installed, what fits here, why.
    WhichModel,
    /// Put a question to a room of seats that do not agree with each other.
    /// Hardware is what it is scoped to, because hardware is the subject
    /// Atlas has measured numbers for; anything else gets the general room.
    AskTheRoom(String),
    /// "Finish setting up" — come back to what first-run setup skipped.
    ///
    /// `firstrun.rs` tells you, in its own words, that there are things to
    /// come back to and to say this when you want to. `FirstRun::resume` has
    /// existed the whole time and nothing spoken reached it.
    FinishSetup,
    /// "Stop telling me about the backups" / "start telling me about them
    /// again" — mute a topic, or unmute it.
    ///
    /// The raw sentence, because `interrupt::mute_from` does its own phrase
    /// detection on the whole thing and the subject is whatever follows the
    /// lead-in.
    MuteTopic(String),
    /// "This is me" — the face in front of the camera is the owner's.
    ///
    /// Its own intent rather than a `NameThis` with an argument: `NameThis`
    /// requires one, and making it optional would mean a half-finished "this
    /// is my" naming your face.
    ThisIsMe,
    /// "Hand over" / "guest mode" — somebody else has the laptop.
    ///
    /// Deliberately takes an argument and deliberately never requires one:
    /// "hand over to Sam" records why, and "hand over" on its own is the
    /// whole command. A handover that insists on a reason is one you skip in
    /// the moment you need it.
    ///
    /// `handover.rs` has said from the day it was written that anyone may
    /// enter — your friend can say "guest mode" themselves — and for that
    /// whole time no such phrase parsed to anything. Entering was command
    /// line only, so the asymmetry the design rests on had no free half.
    HandOver(String),
    /// "Message Sam: the roof quote came back high" — say something to
    /// somebody you work with, in a room inside Atlas.
    ///
    /// The argument is raw on purpose: what you are sending is a sentence a
    /// person will read, and `normalize` strips exactly the punctuation and
    /// case that make it read like one.
    Message(String),
    /// "Any messages?" — what has come in, and what of yours has not landed.
    Messages,
    /// "Who's in the Northwind group?" — a group's members, and which of them
    /// you can actually reach.
    WhoIsIn(String),
    /// "Name the Jordan, Maya group Northwind project" — give a group a name so
    /// you can reach it by that name later.
    NameGroup(String),
    /// "Leave the Northwind group" — walk out of a group: drop your copy of
    /// it, tell the others you've gone, and remember not to be pulled back in
    /// by a message already in flight.
    LeaveGroup(String),
    /// "Add Sam to the Friends group", "take Sam out of the Friends group",
    /// "make Maya a reader in the Friends group", "let Maya post in the
    /// Friends group" -- changing a group you own. The whole sentence is kept;
    /// `groups::read_spoken` reads it.
    ChangeGroup(String),
    /// "Add a friend" (a one-time link to send), a friend link pasted in (add
    /// them), "accept friend request from Sam", "send Maya a friend request".
    /// The whole sentence is kept; `friends::read_spoken` reads it.
    Friend(String),
    /// Atlas's own updates, said out loud (OPEN_GAPS 8.2): "any updates"
    /// (`status`), "install the update" (`install`), "go back to the last
    /// version" (`undo`, which asks first; `undo-confirmed` is only ever
    /// built by the yes to that question).
    Updates(String),
    /// Feedback to whoever sends you Atlas, and on their side what came in
    /// (8.14): `send:<words>` (attaching an update failure written down here,
    /// if there is one), `bare:<words>` (just the words), `list`,
    /// `reply:<n> seen|fixing|fixed <version>|wont [note]`, and
    /// `confirmed-send`, only ever built by the yes to "send it?".
    Feedback(String),
    /// The phone's own language model (P.7): `get` (download it, on the
    /// phone) or `status`.
    PhoneModel(String),
    /// "I'm back" / "it's me again" — end a handover.
    ///
    /// The phrase is a *summons*, never the proof. It asks Atlas to put a
    /// passphrase prompt somewhere it can be typed; the vault decides the
    /// rest. See `handover.rs` for why the way out cannot be something
    /// anyone in the room can say, and `typed.rs` for why it cannot be
    /// something said at all.
    TakeItBack,
    /// "Recap our conversation" / "what have we been talking about" — read the
    /// recent turns of this session back to you.
    ///
    /// Distinct from `History`, which is the log of things Atlas *did* across
    /// files, mail and settings. This is the conversation itself — what you
    /// said and what Atlas answered — which `session::transcript` already
    /// assembles for the model and which nothing until now read back to you.
    Recap,
    /// "Why does the app keep closing?" / "troubleshoot the machine freezing"
    /// — a symptom you describe, matched against the snags in Atlas's shipped
    /// procedures so it names the likely cause and the fix.
    ///
    /// Distinct from `known_procedure` (which answers "how do I do X" from
    /// `knowhow::for_request`): this starts from what went wrong, not from a
    /// task, and reads `knowhow::for_symptom` — the one function that scores a
    /// symptom against every procedure's known snags and was reached by
    /// nothing. Offline by construction: the procedures ship with Atlas.
    Diagnose(String),
    /// "Walk me through freeing up memory" / "how do I open something that
    /// won't launch" — the step-by-step of a task Atlas has a shipped
    /// procedure for, read out in order.
    ///
    /// Distinct from `Diagnose` (which starts from a symptom and reads
    /// `knowhow::for_symptom`) and from the `Unknown` fallback (which reaches
    /// `knowhow::for_request` but only ever says `knowhow::announce` — "I know
    /// this one, 3 steps" — and never the steps themselves). This is the
    /// producer `knowhow::as_plan` never had: it turns the matched procedure
    /// into a numbered plan to actually follow. Offline by construction; the
    /// procedures ship compiled in.
    WalkThrough(String),
    /// "What can you do on your own?" / "where do you still ask me first?" —
    /// the autonomy ledger read back to you, per kind of work.
    ///
    /// `earned::Record` is written every turn (`note`, `taken_back`) and, until
    /// now, read by nothing a person could reach: `HowAmIDoing` reports
    /// corrections, not trust. This reads `earned::may_act_alone` and
    /// `earned::rope` — both reached only by their own tests — so the record
    /// that decides whether Atlas acts alone can finally be asked what it says.
    /// A local read of state Atlas already holds; nothing leaves the machine.
    ActAlone,
    /// "How much do you know?" / "how big is your memory?" / "does your
    /// knowledge keep growing?" — the size of the knowledge store, and the
    /// honest answer to the recurring fear that it grows without bound.
    ///
    /// `consolidate.rs` is built around one promise: the store's cost is
    /// value density, not count, and it does not grow while you are not
    /// asking. `consolidate::size_note` states that promise in numbers —
    /// how many things are known and roughly what they cost — and was
    /// reached by nothing a person could say. This is the question that
    /// asks it. A local read of a count Atlas already holds; nothing leaves
    /// the machine and nothing changes.
    KnowledgeSize,
    /// "File that under groceries" / "that's actually a task" — you told Atlas
    /// it filed a capture wrong, and this corrects it.
    ///
    /// Capture splits catching a thought from filing it: the note lands with a
    /// guessed kind and a set of handles, and the guess is sometimes wrong.
    /// `capture::Notebook::correct` is the one writer that records the fix —
    /// it changes the kind or adds a handle and marks the note confirmed, so
    /// the correction outlives the guess and `find` can reach it by the handle
    /// you actually used. It was written the day capture was and reached by
    /// nothing a person could say; every other path only ever *added* notes.
    /// The correction lands on the most recent note, which is the one you are
    /// almost always talking about the moment after it was filed.
    Refile(String),
    /// Atlas needs one thing clarified before it can act.
    Ask(String),
    /// One of another program's tools, chosen by the model (`mcp`): the
    /// tool's model-facing name and its arguments, as `mcp::payload` writes
    /// them. Never matched from a phrase -- only a tool call makes one.
    McpTool(String),
    Unknown(String),
}

impl Intent {
    /// What to call this out loud.
    ///
    /// Exists because `{other:?}` kept reaching the screen. Atlas said, in
    /// its own voice, *"I don't know how to rehearse ReviewPost."* — a Rust
    /// variant name arriving as English, which is the precise failure
    /// `tests/hub_is_not_code.rs` was written to prevent and which it missed
    /// for two reasons: it matched the literal `{:?}` and so never saw the
    /// inline-capture form `{other:?}`, and `daemon.rs` — the module that
    /// actually speaks to you — was not in its file list.
    ///
    /// Matched exhaustively, with no wildcard arm, on purpose. A wildcard
    /// here would be the same hole in a new shape: adding a 57th variant
    /// would compile, and the first thing anyone heard about it would be
    /// Atlas reading its name aloud. This way the build stops instead.
    pub fn plain(&self) -> String {
        match self {
            Intent::WorkspaceOn => "starting your workspace".into(),
            Intent::WorkspaceOff => "shutting the workspace down".into(),
            Intent::OpenApp(a) => format!("opening {a}"),
            Intent::CloseApp(a) => format!("closing {a}"),
            Intent::FocusApp(a) => format!("switching to {a}"),
            Intent::ViewDisplay => "looking at your screen".into(),
            Intent::Research(t) => format!("researching {t}"),
            Intent::McpTool(p) => crate::mcp::plain(p),
            Intent::CaptureWebcam => "looking through the camera".into(),
            Intent::Say(_) => "saying something back".into(),
            Intent::Pause => "pausing".into(),
            Intent::Gestures(true) => "watching your hands".into(),
            Intent::Gestures(false) => "stopping watching your hands".into(),
            Intent::Resume => "picking back up".into(),
            Intent::Outstanding => "the outstanding list".into(),
            Intent::Queued => "what is queued to go out".into(),
            Intent::DraftPost(c) => format!("drafting a post for {c}"),
            Intent::Undo => "putting back the last thing I moved".into(),
            Intent::BackUp => "backing everything up".into(),
            Intent::SetMode(m) => format!("switching to {m} mode"),
            Intent::MachineHealth => "checking how the machine is".into(),
            Intent::SelfCheck => "checking myself".into(),
            Intent::Shakedown => "running a shakedown".into(),
            Intent::UseClipboard(w) => format!("using the clipboard to {w}"),
            Intent::Rehearse(c) => format!("rehearsing \"{c}\""),
            Intent::Show(p) => format!("showing the {p} panel"),
            Intent::Dismiss => "taking the panel off screen".into(),
            Intent::Ready => "your waking brief".into(),
            Intent::Capabilities(_) => "what I can do".into(),
            Intent::History(_) => "what I did".into(),
            Intent::CreateAccount(s) => format!("making you an account with {s}"),
            Intent::SignIn(s) => format!("signing you in to {s}"),
            Intent::TypeCode(_) => "typing in your code".to_string(),
            Intent::TwoFactor(s) => format!("changing two-factor: {s}"),
            Intent::KeepAtIt => "keeping at the last build".to_string(),
            Intent::Goals(_) => "your goals".to_string(),
            Intent::Later(_) => "your later list".to_string(),
            Intent::SortMail(_) => "sorting your mailbox".to_string(),
            Intent::SchedulePost(_) => "scheduling your post".to_string(),
            Intent::PressButton(s) => format!("pressing a button: {s}"),
            Intent::MoveBigFiles(_) => "moving big files to another drive".to_string(),
            Intent::TidyDesktop => "tidying your desktop".to_string(),
            Intent::UseMic(m) => format!("listening with the {m} microphone"),
            Intent::EditMedia(_) => "editing your video on a copy".to_string(),
            Intent::EditPhoto(_) => "editing your photo on a copy".to_string(),
            Intent::MakePicture(_) => "making a picture on this machine".to_string(),
            Intent::Clock => "the time and date".to_string(),
            Intent::SetKey(_) => "changing your push-to-talk or typing-box key".to_string(),
            Intent::Languages(_) => "which languages Atlas can hear".to_string(),
            Intent::TeachGesture(_) => "learning a new hand gesture".to_string(),
            Intent::MoneyAdvice(_) => "general money questions".to_string(),
            Intent::CreatorAdvice(_) => "advice on your videos".to_string(),
            Intent::Overnight => "what Atlas did overnight".to_string(),
            Intent::Dangling => "notes that point at things that don't exist yet".to_string(),
            Intent::Suggestions(_) => "switching one of Atlas's suggestions on or off".to_string(),
            Intent::DropTask(_) => "taking something off your list, or bringing it back".to_string(),
            Intent::Unzip(_) => "unzipping".to_string(),
            Intent::ReadDocument(_) => "reading a document".to_string(),
            Intent::WorkOnYourself(_) => "working on myself".into(),
            Intent::Build(_) => "building what you described".into(),
            Intent::Improve(_) => "working on one of your projects".into(),
            Intent::Implement(_) => "implementing a change you approved".into(),
            Intent::DesignReview(_) => "reviewing a page's design against the house style".into(),
            Intent::Animate(_) => "making an animation".into(),
            Intent::Scene(_) => "drawing a 3-D scene".into(),
            Intent::Explain(_) => "explaining code in plain English".into(),
            Intent::PlainChange(_) => "explaining a staged change as behaviour, not code".into(),
            Intent::Booking(_) => "working through a time someone proposed".into(),
            Intent::Learn(_) => "learning a body of knowledge into its fact book".into(),
            Intent::Schedule(_) => "putting something on your calendar".into(),
            Intent::Agenda(_) => "reading your calendar".into(),
            Intent::Unlock(_) => "unlocking the vault".into(),
            Intent::Capture(_) => "making a note".into(),
            Intent::Mail(_) => "your mail".into(),
            Intent::Sync(_) => "syncing".into(),
            Intent::ReviewPost(_) => "checking something before you post it".into(),
            Intent::TravelPrep => "getting you ready to travel".into(),
            Intent::Files(_) => "working on your files".into(),
            Intent::Why(_) => "explaining why I did that".into(),
            Intent::BriefOn(who) => format!("asking {who} how it is getting on"),
            Intent::WhatsThere => "saying what I can see".into(),
            Intent::WhatsThis => "naming the thing you are holding up".into(),
            Intent::CallNotes(_) => "taking notes on your call".into(),
            Intent::Delegate(_) => "working that window for you".into(),
            Intent::AfterMe => "reading back your envelope arrangement".into(),
            Intent::NameThis(n) => format!("learning that this is {n}"),
            Intent::Recommend => "what would make me faster here".into(),
            Intent::AddressAs(_) => "what to call you".into(),
            Intent::Pair(n) => format!("pairing with {n}"),
            Intent::AcceptPairing(_) => "accepting a pairing".into(),
            Intent::ForgetPeer(n) => format!("forgetting the pairing with {n}"),
            Intent::Dictate(_) => "dictating into the window you are looking at".into(),
            Intent::RebuildIndex => "rebuilding the notes index".into(),
            Intent::WhatIHave(_) => "what I have written down".into(),
            Intent::ModelTrace => "what I have been asking the model".into(),
            Intent::GotItWrong(_) => "noting that I got it wrong".into(),
            Intent::ApplyLesson => "writing that lesson down".into(),
            Intent::HowAmIDoing => "the correction scoreboard".into(),
            Intent::TimeSpent(_) => "where your time went".into(),
            Intent::ClipHistory(_) => "your clipboard history".into(),
            Intent::ScreenText(_) => "copying the text off the screen".into(),
            Intent::MarketDay(_) => "the market's calendar".into(),
            Intent::WaitingFor(_) => "what you're waiting on".into(),
            Intent::NoteReview(_) => "reviewing your notes".into(),
            Intent::Launch(_) => "launching something".into(),
            Intent::TradeDay(_) => "your trading check-in".into(),
            Intent::MeetingPrep(_) => "prep for your next meeting".into(),
            Intent::Snippet(_) => "your snippets".into(),
            Intent::FindFile(_) => "finding a file".into(),
            Intent::Pdf(_) => "working on a PDF".into(),
            Intent::People(_) => "your people".into(),
            Intent::Feeds(_) => "your feeds".into(),
            Intent::Social(_) => "your social accounts".into(),
            Intent::Opportunities(_) => "the opportunities I found".into(),
            Intent::Wit(_) => "how much of a smart-ass I am".into(),
            Intent::Receipt(_) => "your receipts".into(),
            Intent::Habit(_) => "your habits".into(),
            Intent::Cards(_) => "your flashcards".into(),
            Intent::Translate(_) => "translating".into(),
            Intent::WhichModel => "which model I am using".into(),
            Intent::AskTheRoom(q) => format!("putting \"{q}\" to the room"),
            Intent::Message(m) => {
                let who = m.split_once(':').map(|(w, _)| w.trim()).unwrap_or(m.trim());
                if who.is_empty() {
                    "sending a message".into()
                } else {
                    format!("sending a message to {who}")
                }
            }
            Intent::FinishSetup => "finishing setting up".into(),
            Intent::MuteTopic(_) => "what I mention and what I don't".into(),
            Intent::ThisIsMe => "learning your face".into(),
            Intent::HandOver(note) if note.trim().is_empty() => "handing this over".into(),
            Intent::HandOver(note) => format!("handing this over ({})", note.trim()),
            Intent::Messages => "your messages".into(),
            Intent::WhoIsIn(_) => "who's in a group".into(),
            Intent::NameGroup(_) => "naming a group".into(),
            Intent::LeaveGroup(_) => "leaving a group".into(),
            Intent::ChangeGroup(_) => "changing who's in a group".into(),
            Intent::Friend(_) => "adding a friend".into(),
            Intent::Updates(w) if w.starts_with("undo") => "going back to the last version of Atlas".into(),
            Intent::Updates(w) if w == "install" => "installing the Atlas update".into(),
            Intent::Updates(_) => "Atlas's updates".into(),
            Intent::Feedback(w) if w == "list" => "feedback about Atlas".into(),
            Intent::Feedback(w) if w.starts_with("reply:") => "answering feedback".into(),
            Intent::Feedback(_) => "sending feedback about Atlas".into(),
            Intent::PhoneModel(_) => "the phone's own language model".into(),
            Intent::TakeItBack => "asking you for the passphrase, so this is yours again".into(),
            Intent::Recap => "reading our conversation back".into(),
            Intent::ActAlone => "what I'll do on my own and where I still ask".into(),
            Intent::KnowledgeSize => "how much I've got stored and what it costs".into(),
            Intent::Refile(_) => "filing that note where it belongs".into(),
            Intent::Diagnose(_) => "working out what's going wrong".into(),
            Intent::WalkThrough(_) => "walking you through how to do that".into(),
            Intent::Ask(_) => "asking you to clarify".into(),
            Intent::Unknown(said) => {
                if said.trim().is_empty() {
                    "something I didn't catch".into()
                } else {
                    format!("\"{}\"", said.trim())
                }
            }
        }
    }
}

/// One phrase in the parser's table.
#[derive(Debug, Clone)]
struct Row {
    phrase: String,
    intent: String,
    /// `takes_argument && !argument_optional`.
    ///
    /// `needs_arg` and `takes_arg` are both kept because they answer
    /// different questions and the first cannot answer the second: it is
    /// false both for a command that takes no argument and for one whose
    /// argument is optional. `understood_all_of_it` needs to tell those apart
    /// -- anything left over from a command that takes NO argument means the
    /// sentence was not understood.
    needs_arg: bool,
    takes_arg: bool,
    raw_argument: bool,
    /// The whole sentence must be the phrase (`CommandSpec::anchored`).
    anchored: bool,
    /// The argument must be a known name (`CommandSpec::names_only`).
    names_only: bool,
}

#[derive(Clone)]
pub struct Parser {
    /// Every phrase, sorted longest-first.
    table: Vec<Row>,
    /// Your registered projects' names, lower-cased: what lets a general
    /// phrase ("fix the", "change the") mean *project work* only when a
    /// project is named. Set by the daemon from its workshop.
    projects: Vec<String>,
    /// What round 11's tools need to read a sentence whole: the people and
    /// habits you have, and the list a number would refer to. Set by the
    /// daemon before each turn (`workday::Known`).
    workday: crate::workday::Known,
    /// The names a `names_only` command may take: your apps, your modes, your
    /// other Atlases. `None` until the daemon says -- a parser made from the
    /// command list alone takes any name, as it always did.
    names: Option<KnownNames>,
}

/// The names a `names_only` command's argument must be one of.
#[derive(Debug, Clone, Default)]
pub struct KnownNames {
    pub apps: Vec<String>,
    pub modes: Vec<String>,
    pub peers: Vec<String>,
}

impl Parser {
    pub fn new(cfg: &CommandsConfig) -> Self {
        let mut table: Vec<Row> = cfg
            .commands
            .iter()
            .flat_map(|c| {
                c.phrases.iter().map(move |p| Row {
                    // Normalised the same way the spoken input is.
                    // Lowercasing alone left every phrase containing an
                    // apostrophe unmatchable, because `normalize` strips
                    // them from what you say and nothing stripped them
                    // from the config. "what's new" was dead; "whats new"
                    // was listed beside it and hid that.
                    phrase: normalize(p),
                    intent: c.intent.clone(),
                    needs_arg: c.takes_argument && !c.argument_optional,
                    takes_arg: c.takes_argument,
                    raw_argument: c.raw_argument,
                    anchored: c.anchored,
                    names_only: c.names_only,
                })
            })
            .collect();
        // Longest phrase first. This is the whole trick.
        table.sort_by(|a, b| b.phrase.len().cmp(&a.phrase.len()).then(a.phrase.cmp(&b.phrase)));
        Parser { table, projects: Vec::new(), workday: Default::default(), names: None }
    }

    /// The projects a general phrase may reach. Without one named, "change
    /// the volume" or "add to my shopping list" is not a build job.
    pub fn know_workday(&mut self, k: crate::workday::Known) {
        self.workday = k;
    }

    pub fn know_projects(&mut self, names: impl IntoIterator<Item = String>) {
        self.projects = names.into_iter().map(|n| n.to_lowercase()).filter(|n| !n.is_empty()).collect();
    }

    /// The apps, modes and other Atlases a `names_only` command may name.
    pub fn know_names(&mut self, names: KnownNames) {
        let low = |v: Vec<String>| v.into_iter().map(|n| normalize(&n)).filter(|n| !n.is_empty()).collect();
        self.names = Some(KnownNames { apps: low(names.apps), modes: low(names.modes), peers: low(names.peers) });
    }

    /// Is `arg` a name this command may take? Always, until names are known.
    fn is_a_known_name(&self, intent: &str, arg: &str) -> bool {
        let Some(n) = &self.names else { return true };
        let a = normalize(arg);
        let a = a.strip_prefix("the ").or_else(|| a.strip_prefix("my ")).unwrap_or(&a).trim();
        let a = a.strip_suffix(" mode").unwrap_or(a).trim();
        // "close it", "switch to that one": the reference resolver says
        // which, or asks.
        if a.is_empty()
            || matches!(a, "it" | "that" | "this" | "them" | "these" | "those" | "that one" | "this one" | "the other one" | "other one")
        {
            return true;
        }
        let list = match intent {
            "open_app" | "close_app" | "focus_app" => &n.apps,
            // Leaving a mode is said with words that aren't a mode's name.
            "set_mode" if matches!(a, "off" | "leave" | "exit" | "normal" | "none" | "nothing" | "standard" | "default" | "out") => {
                return true
            }
            "set_mode" => &n.modes,
            "brief_on" => &n.peers,
            _ => return true,
        };
        list.iter().any(|k| k == a)
    }

    /// `improve` is reached by a clear coding verb, or by a general phrase
    /// with a project named ("the Atlas project", a registered name).
    fn improve_is_meant(&self, phrase: &str, rest: &str) -> bool {
        if matches!(phrase, "improve" | "refactor" | "work on the") {
            return true;
        }
        let low = rest.to_lowercase();
        low.split(|c: char| !c.is_alphanumeric()).any(|w| w == "project")
            || self.projects.iter().any(|p| low.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_').any(|w| w == p))
    }

    pub fn parse(&self, input: &str) -> Intent {
        self.parse_named(input).0
    }

    /// The same, with the name of the command it matched (`open_app`,
    /// `research`, ...), or `None` when nothing matched.
    ///
    /// The name is what add-on permissions are written in (`plugins`): a
    /// step an add-on runs is allowed or refused by *which command* it
    /// turned out to be, decided here by the one parser, rather than by a
    /// second reading of the text that could disagree with this one.
    pub fn parse_named(&self, input: &str) -> (Intent, Option<String>) {
        // "Can you check my email?" is "check my email", asked politely
        // (27 Sep 2026: the polite form matched nothing, so free-form asks
        // that Atlas can do fell through to "I can't answer that here").
        if let Some(rest) = polite_rest(input) {
            let (i, n) = self.parse_named(rest);
            if !matches!(i, Intent::Unknown(_)) {
                return (i, n);
            }
        }
        // A friend link pasted on its own, or inside a message: nothing a
        // phrase could start with, and it must reach `friends` exactly as
        // pasted -- the link is case-sensitive.
        // "send Maya a friend request" puts the name in the middle, where no
        // phrase can reach it; `friends::read_spoken` reads only its own shapes.
        if input.contains(crate::friends::PREFIX) || crate::friends::read_spoken(input).is_some() {
            return (Intent::Friend(input.trim().to_string()), Some("friend".into()));
        }
        // Asked for a report on itself, inside a longer sentence (29 Sep 2026:
        // "Can you do some work and generate a report on yourself?" went to
        // the model, which said it can't): the self-check.
        // Asked to be looked at, anywhere in the sentence (29 Sep 2026: "Are
        // you using my camera? Can you see me?" and "Please use my camera and
        // look at me" matched nothing, went to the model, and were told
        // "I don't have a camera"): the camera (`camera_ask`).
        if crate::camera_ask::asks_to_look(input) {
            return (Intent::CaptureWebcam, Some("capture_webcam".into()));
        }
        if asks_for_a_self_report(input) {
            return (Intent::SelfCheck, Some("self_check".into()));
        }
        // A follow-up to a list just shown ("open 2"), or a sentence one of
        // round 11's tools reads whole ("keep in touch with Priya every
        // month"). Strict shapes only; anything else carries on below.
        if let Some((i, name)) = crate::workday::read_first(input, &self.workday) {
            return (i, Some(name.into()));
        }
        let text = normalize(input);
        // The sentence with the words that don't change what it asks taken
        // off, for the anchored commands.
        let bare = without_fillers(&text);
        // For a raw-argument command, matching happens case- and
        // whitespace-insensitively (in normalized space), but the remainder
        // handed to `build` comes out of the *raw* words, not the fully
        // normalized text -- normalize() strips exactly the characters a
        // pairing code is made of. See the raw-argument branch below.
        let raw_trimmed = input.trim();
        for row in &self.table {
            let (phrase, intent) = (&row.phrase, &row.intent);
            // Anchored: the whole sentence is the phrase, or it isn't this.
            if row.anchored {
                if bare != *phrase && text != *phrase {
                    continue;
                }
                return (build(intent, String::new(), input), Some(intent.clone()));
            }
            let rest = if row.raw_argument {
                // Match the phrase word-for-word in *normalized* space, but cut
                // the remainder from the *raw* words so an argument's own
                // characters (a pairing code's dashes, punctuation) survive --
                // normalize() strips exactly those. Comparing the normalized
                // phrase against raw bytes was wrong twice over: normalize()
                // strips apostrophes from the phrase but not from what you say,
                // so a trigger like "check this page's design" could never
                // match, and `phrase.len()` is the normalized length, so slicing
                // it out of the raw input landed at the wrong byte.
                let phrase_words = phrase.split_whitespace().count();
                let raw_words: Vec<&str> = raw_trimmed.split_whitespace().collect();
                if raw_words.len() < phrase_words {
                    None
                } else {
                    let head = raw_words[..phrase_words].join(" ");
                    if normalize(&head) == *phrase {
                        Some(raw_words[phrase_words..].join(" "))
                    } else {
                        None
                    }
                }
            } else if text == *phrase {
                Some(String::new())
            } else if let Some(r) = text.strip_prefix(&format!("{} ", phrase)) {
                Some(r.trim().to_string())
            } else {
                None
            };

            let Some(rest) = rest else { continue };
            if row.needs_arg && rest.is_empty() {
                continue; // "open" with no target is not a match
            }
            if intent == "improve" && !self.improve_is_meant(phrase, &rest) {
                continue;
            }
            // "go to sleep" is not an app. The name is checked with the
            // politeness off the end ("close chrome for me").
            if row.names_only && !self.is_a_known_name(intent, &without_fillers(&rest)) {
                continue;
            }
            // The phrase matched. Did it account for the whole sentence?
            //
            // See `understood_all_of_it`. Matching a prefix and keeping the
            // remainder is what made Atlas feel scripted: it answered a
            // sentence it had only partly read, and threw the rest away.
            if !understood_all_of_it(intent, &rest, row.takes_arg, row.raw_argument, self) {
                continue;
            }
            let built = build(intent, rest, input);
            // A phrase that matched but whose sentence didn't read as that
            // command names no command -- so nothing downstream (an add-on's
            // permission check, say) mistakes it for one.
            if matches!(built, Intent::Unknown(_)) {
                return (built, None);
            }
            return (built, Some(intent.clone()));
        }
        // A sentence that asks for one of a few things Atlas does without
        // starting with its phrase -- "I guess I want you to organize my
        // desktop", "calm down with being our smart apps" (a smart-ass,
        // misheard) -- read as the plain command it means and parsed as
        // that (`doing::rescue`, Eric's evening, 29 Sep 2026). Only when the
        // reading is unambiguous; the command's own phrase is never read
        // again, so this cannot go round.
        if let Some(plain) = crate::doing::rescue(input) {
            if normalize(&plain) != text {
                let (i, n) = self.parse_named(&plain);
                if !matches!(i, Intent::Unknown(_)) {
                    return (i, n);
                }
            }
        }
        (Intent::Unknown(input.trim().to_string()), None)
    }
}

/// Words that don't change what a short command asks: the wake word,
/// "please", "now", "for me". Taken off both ends, for anchored commands.
pub fn without_fillers(normalized: &str) -> String {
    const LEAD: &[&str] = &["hey atlas", "ok atlas", "okay atlas", "atlas", "please", "ok", "okay", "just"];
    const TAIL: &[&str] = &["please", "now", "for me", "atlas", "thanks", "thank you", "right now"];
    let mut t = normalized.trim().to_string();
    loop {
        let before = t.clone();
        for l in LEAD {
            if let Some(r) = t.strip_prefix(&format!("{l} ")) {
                t = r.trim().to_string();
            }
        }
        for l in TAIL {
            if let Some(r) = t.strip_suffix(&format!(" {l}")) {
                t = r.trim().to_string();
            }
        }
        if t == before {
            return t;
        }
    }
}

impl Parser {
    /// Which intent, if any, this text starts with a phrase for.
    ///
    /// A plain prefix walk with **no** completeness check, deliberately:
    /// `understood_all_of_it` calls this, so checking completeness here would
    /// recurse. The question it answers is only "does this look like the
    /// start of a command", which is all rule 2 needs.
    fn first_phrase_match(&self, text: &str) -> Option<&str> {
        let t = normalize(text);
        for row in &self.table {
            let phrase = &row.phrase;
            let matched = if row.anchored {
                t == *phrase || without_fillers(&t) == *phrase
            } else {
                t == *phrase || t.starts_with(&format!("{phrase} "))
            };
            if !matched {
                continue;
            }
            if row.needs_arg && t.len() <= phrase.len() {
                continue;
            }
            return Some(&row.intent);
        }
        None
    }
}

/// Intents whose argument is a **name**, not a sentence.
///
/// Hard-coded here rather than in `commands.yaml` because `build` just below
/// already hard-codes exactly these intent names -- one list, one file, one
/// place to keep in step -- and because it is structural rather than a
/// preference: `open_app` takes the name of an app in every possible config.
///
/// `research`, `ask_the_room`, `got_it_wrong`, `what_i_have` and
/// `use_clipboard` are deliberately absent: a topic or a complaint is
/// *supposed* to be a whole clause, and rejecting those would break the
/// commands that work best. `unlock` is absent for a sharper reason -- a
/// passphrase may be a long phrase with commas in it, and refusing to parse
/// it would send the vault passphrase to the model instead, which is the one
/// thing `typed.rs` says a microphone must never carry.
fn argument_is_a_name(intent: &str) -> bool {
    matches!(intent, "open_app" | "close_app" | "focus_app" | "set_mode" | "draft_post")
}

/// Words that mean the sentence carried on past the bit that was understood.
const CARRIED_ON: [&str; 6] = [" and ", " then ", " but ", " so ", " because ", " while "];

/// Did the matched phrase account for the whole sentence?
///
/// The parser matches a phrase as a **prefix** and hands the remainder to
/// `build` as the argument. For "open chrome" that is exactly right and
/// instant. For anything conversational it was a disaster, and this is what
/// Eric meant by having to follow a script. Measured before fixing, on
/// fifteen ordinary sentences:
///
/// ```text
/// "open chrome and tell me what you think of the numbers"
///     -> OpenApp("chrome and tell me what you think of the numbers")
///        and said "Opening chrome and tell me what you think of the numbers."
/// "can you open chrome for me"
///     -> Capabilities("open chrome for me")     -- and did NOT open chrome
/// "open up a browser would you, I want to look at something"
///     -> OpenApp("up a browser would you i want to look at something")
/// "hang on, go back"
///     -> Pause                                  -- "go back" discarded
/// ```
///
/// Three rules, none of which needs a vocabulary to keep in sync:
///
/// 1. **A zero-argument intent with anything left over did not understand the
///    sentence.** "hang on, go back" is not "pause". This is the most
///    dangerous class, because the leftover was silently discarded and a mode
///    changed.
/// 2. **If the remainder itself parses as a command, the match is ambiguous.**
///    "can you open chrome for me" is a request in English, not a question
///    about Atlas's capabilities -- and which one it is, is exactly the sort
///    of thing a model is for.
/// 3. **A name-argument that looks like a clause is not a name.** An app is
///    one or two words with no conjunction in it.
///
/// Rejecting here does not lose the sentence: `parse` continues down the
/// table and ultimately returns `Intent::Unknown`, which is what reaches the
/// model. So the effect is "when in doubt, actually think about it".
fn understood_all_of_it(
    intent: &str,
    rest: &str,
    takes_argument: bool,
    raw_argument: bool,
    p: &Parser,
) -> bool {
    // Never second-guessed, whatever it looks like.
    //
    // `unlock`'s argument is the vault passphrase. It is NOT a
    // `raw_argument` command in the shipped config, so without this it falls
    // to the rules below -- and "the passphrase is open sesame" leaves "open
    // sesame" over, which reads as `open_app`, so the match would be rejected
    // and the sentence sent to the model instead. `typed.rs` opens by saying
    // there is "exactly one thing in Atlas that a microphone must never
    // carry: the vault passphrase". Refusing to parse it is how it gets
    // carried.
    //
    // Found by testing the rule rather than reading it: the first version of
    // this function excluded `unlock` from `argument_is_a_name` and left it
    // subject to everything else, which is not an exemption at all.
    if never_second_guessed(intent) {
        return true;
    }
    // An argument taken verbatim on purpose -- a pairing code.
    if raw_argument {
        return true;
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return true;
    }

    // Which checks apply depends on what the argument is FOR. Getting this
    // wrong in either direction breaks real commands, and both directions
    // were measured rather than reasoned about:
    //
    // Too wide (the first version applied the command-in-the-remainder check
    // to every intent):
    //
    // ```text
    // "research open source licensing"   -> Unknown   ("open" read as open_app)
    // "the passphrase is open sesame"    -> Unknown   (to the MODEL)
    // ```
    //
    // Too narrow is the original bug: "open chrome and tell me what you
    // think" answered from a partial read.
    let name_like = argument_is_a_name(intent);

    // The one check that applies to a zero-argument command too: if what is
    // left over is ITSELF a command, the phrase match was not the whole
    // sentence.
    //
    // This replaced a blunter rule -- "anything left over from a
    // zero-argument command means it was misread" -- which was right about
    // "hang on, go back" (leftover "go back", a `History` command, silently
    // discarded while the mode changed) and wrong about "that was too long,
    // keep it to one line". `got_it_wrong` declares no argument, so its
    // phrases are complaint openers and what follows is the correction
    // itself; refusing to parse it broke the whole correction path, and
    // `telling_it_twice.rs` caught it in six places.
    //
    // The difference between those two sentences is not whether something
    // was left over. It is whether what was left over was a command.
    if !takes_argument || name_like || ambiguous_with_a_request(intent) {
        // Same-intent remainders are not ambiguous, they are the sentence
        // carrying on in the same direction. `commands.yaml` lists both "why
        // did you" and "explain that" under `why`, so "why did you explain
        // that" leaves "explain that" over, which parses as `why` again --
        // one coherent question. An earlier version rejected it and turned a
        // specific question into a bare "Which one?", caught by `daemon.rs`'s
        // `a_bare_why_accounts_for_everything` test.
        if let Some(other) = p.first_phrase_match(rest) {
            if other != intent {
                return false;
            }
        }
    }

    // A name that is really a clause. An app is one or two words with no
    // conjunction in it.
    if name_like {
        let padded = format!(" {} ", rest.to_lowercase());
        if CARRIED_ON.iter().any(|w| padded.contains(w)) || rest.contains(',') {
            return false;
        }
        if rest.split_whitespace().count() > 3 {
            return false;
        }
    }
    true
}

/// Intents whose argument must reach `build` exactly as spoken, whatever it
/// looks like. See the note at the top of `understood_all_of_it`.
fn never_second_guessed(intent: &str) -> bool {
    matches!(intent, "unlock" | "pair" | "capture")
}

/// Phrases that are a question in one reading and a request in another.
///
/// `capabilities` carries the phrase "can you", so "can you open chrome for
/// me" parsed as `Capabilities("open chrome for me")` -- Atlas answered a
/// question about itself and did not open Chrome. In English that sentence is
/// a request. Which reading is meant is exactly the sort of thing a model is
/// for, so when the remainder is itself a command, this stops guessing.
///
/// Deliberately a list of one. Every member costs a fast path.
fn ambiguous_with_a_request(intent: &str) -> bool {
    matches!(intent, "capabilities")
}

/// Text the way the parser compares it: lowercase, letters, digits and
/// single spaces.
pub fn normalize(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn build(intent: &str, arg: String, raw: &str) -> Intent {
    match intent {
        "workspace_on" => Intent::WorkspaceOn,
        "workspace_off" => Intent::WorkspaceOff,
        "open_app" => Intent::OpenApp(arg),
        "close_app" => Intent::CloseApp(arg),
        "focus_app" => Intent::FocusApp(arg),
        "view_display" => Intent::ViewDisplay,
        "research" => Intent::Research(arg),
        "capture_webcam" => Intent::CaptureWebcam,
        "pause" => Intent::Pause,
        "gestures_on" => Intent::Gestures(true),
        "gestures_off" => Intent::Gestures(false),
        "resume" => Intent::Resume,
        "outstanding" => Intent::Outstanding,
        "queued" => Intent::Queued,
        "draft_post" => Intent::DraftPost(arg),
        "undo" => Intent::Undo,
        "back_up" => Intent::BackUp,
        "rebuild_index" => Intent::RebuildIndex,
        "what_i_have" => Intent::WhatIHave(arg),
        "model_trace" => Intent::ModelTrace,
        "ask_the_room" => Intent::AskTheRoom(arg),
        "got_it_wrong" => Intent::GotItWrong(arg),
        "apply_lesson" => Intent::ApplyLesson,
        "how_am_i_doing" => Intent::HowAmIDoing,
        "time_spent" => Intent::TimeSpent(raw.trim().to_string()),
        "clip_history" => Intent::ClipHistory(raw.trim().to_string()),
        "screen_text" => Intent::ScreenText(raw.trim().to_string()),
        "market_day" => Intent::MarketDay(raw.trim().to_string()),
        "waiting_for" => Intent::WaitingFor(raw.trim().to_string()),
        "note_review" => Intent::NoteReview(raw.trim().to_string()),
        "launch" => Intent::Launch(raw.trim().to_string()),
        "trade_day" => Intent::TradeDay(raw.trim().to_string()),
        "meeting_prep" => Intent::MeetingPrep(raw.trim().to_string()),
        "snippet" => Intent::Snippet(raw.trim().to_string()),
        "find_file" => Intent::FindFile(raw.trim().to_string()),
        "pdf" => Intent::Pdf(raw.trim().to_string()),
        "people" => Intent::People(raw.trim().to_string()),
        "feeds" => Intent::Feeds(raw.trim().to_string()),
        "social" => Intent::Social(raw.trim().to_string()),
        // The whole sentence: "look for video editing gigs" is read by
        // `hunt::understand`, which needs the verb as much as the words.
        "opportunities" => Intent::Opportunities(raw.trim().to_string()),
        "wit" => Intent::Wit(raw.trim().to_string()),
        "receipt" => Intent::Receipt(raw.trim().to_string()),
        "habit" => Intent::Habit(raw.trim().to_string()),
        "cards" => Intent::Cards(raw.trim().to_string()),
        "translate" => Intent::Translate(raw.trim().to_string()),
        "which_model" => Intent::WhichModel,
        "set_mode" => Intent::SetMode(arg),
        "machine_health" => Intent::MachineHealth,
        "self_check" => Intent::SelfCheck,
        "shakedown" => Intent::Shakedown,
        "use_clipboard" => Intent::UseClipboard(arg),
        "rehearse" => Intent::Rehearse(arg),
        "show_panel" => Intent::Show(arg),
        "dismiss_panel" => Intent::Dismiss,
        "ready" => Intent::Ready,
        "capabilities" => Intent::Capabilities(arg),
        "history" => Intent::History(arg),
        "create_account" => Intent::CreateAccount(arg),
        "sign_in" => Intent::SignIn(arg),
        "type_code" => Intent::TypeCode(raw.trim().to_string()),
        "two_factor" => Intent::TwoFactor(raw.trim().to_string()),
        "keep_at_it" => Intent::KeepAtIt,
        "goals" => Intent::Goals(raw.trim().to_string()),
        "later" => Intent::Later(raw.trim().to_string()),
        "sort_mail" => Intent::SortMail(raw.trim().to_string()),
        "schedule_post" => Intent::SchedulePost(raw.trim().to_string()),
        "press_button" => Intent::PressButton(raw.trim().to_string()),
        "move_big_files" => Intent::MoveBigFiles(raw.trim().to_string()),
        "tidy_desktop" => Intent::TidyDesktop,
        "use_mic" => Intent::UseMic(arg),
        "edit_media" => Intent::EditMedia(raw.trim().to_string()),
        "edit_photo" => Intent::EditPhoto(raw.trim().to_string()),
        "make_picture" => Intent::MakePicture(raw.trim().to_string()),
        "clock" => Intent::Clock,
        "set_key" => Intent::SetKey(raw.trim().to_string()),
        "languages" => Intent::Languages(raw.trim().to_string()),
        "teach_gesture" => Intent::TeachGesture(raw.trim().to_string()),
        "money_advice" => Intent::MoneyAdvice(raw.trim().to_string()),
        "creator_advice" => Intent::CreatorAdvice(raw.trim().to_string()),
        "overnight" => Intent::Overnight,
        "dangling" => Intent::Dangling,
        "suggestions" => Intent::Suggestions(raw.trim().to_string()),
        "drop_task" => Intent::DropTask(raw.trim().to_string()),
        "unzip" => Intent::Unzip(raw.trim().to_string()),
        "read_document" => Intent::ReadDocument(raw.trim().to_string()),
        "work_on_yourself" => Intent::WorkOnYourself(arg),
        "build_it" => Intent::Build(arg),
        "improve" => Intent::Improve(arg),
        "implement" => Intent::Implement(arg),
        "design_review" => Intent::DesignReview(arg),
        "animate" => Intent::Animate(arg),
        "scene3d" => Intent::Scene(arg),
        "explain_code" => Intent::Explain(arg),
        "plain_change" => Intent::PlainChange(arg),
        // The whole utterance, not the trimmed remainder: `booking` reads both
        // the verb ("accept", "decline", "offer another time") and the details
        // ("Sam proposed Tuesday at 2pm…"), and the phrase that routed here is
        // part of what it has to read.
        "booking" => Intent::Booking(raw.trim().to_string()),
        "learn_knowledge" => Intent::Learn(arg),
        "schedule" => Intent::Schedule(arg),
        // The reminder round-trip carrier: a fired reminder job's stored
        // command re-parses here and just gets spoken. See config/commands.yaml
        // and Daemon::remind_help (which creates the job).
        "reminder" => Intent::Say(arg),
        "agenda" => Intent::Agenda(arg),
        "unlock" => Intent::Unlock(arg),
        "capture" => Intent::Capture(arg),
        "mail" => Intent::Mail(arg),
        "sync" => Intent::Sync(arg),
        "review_post" => Intent::ReviewPost(arg),
        "travel_prep" => Intent::TravelPrep,
        "files" => Intent::Files(arg),
        "brief_on" => Intent::BriefOn(arg),
        "why" => Intent::Why(arg),
        "whats_there" => Intent::WhatsThere,
        "whats_this" => Intent::WhatsThis,
        "call_notes_on" => Intent::CallNotes("start".into()),
        "call_record_everyone" => Intent::CallNotes("everyone".into()),
        "call_they_agreed" => Intent::CallNotes("agreed".into()),
        "call_they_declined" => Intent::CallNotes("declined".into()),
        "call_notes_off" => Intent::CallNotes("stop".into()),
        "call_couldnt_ask" => Intent::CallNotes("couldnt_ask".into()),
        "call_no_answer" => Intent::CallNotes("no_answer".into()),
        "call_just_mine" => Intent::CallNotes("just_mine".into()),
        "call_status" => Intent::CallNotes("status".into()),
        "call_what_it_does" => Intent::CallNotes("what_it_does".into()),
        "delegate" => Intent::Delegate(raw.trim().to_string()),
        "after_me" => Intent::AfterMe,
        "name_this" => Intent::NameThis(arg),
        "recommend" => Intent::Recommend,
        "address_as" => Intent::AddressAs(raw.trim().to_string()),
        "pair" => Intent::Pair(arg),
        "accept_pairing" => Intent::AcceptPairing(arg),
        "forget_peer" => Intent::ForgetPeer(arg),
        "finish_setup" => Intent::FinishSetup,
        "mute_topic" => Intent::MuteTopic(raw.trim().to_string()),
        "this_is_me" => Intent::ThisIsMe,
        "hand_over" => Intent::HandOver(arg),
        "take_it_back" => Intent::TakeItBack,
        // Raw, like `dictate` and for the same reason: this argument is a
        // sentence somebody is going to read.
        "message" => Intent::Message(arg),
        "recap" => Intent::Recap,
        "act_alone" => Intent::ActAlone,
        "knowledge_size" => Intent::KnowledgeSize,
        "refile" => Intent::Refile(arg),
        "diagnose" => Intent::Diagnose(arg),
        "walk_through" => Intent::WalkThrough(arg),
        "messages" => Intent::Messages,
        "who_is_in" => Intent::WhoIsIn(arg),
        "name_group" => Intent::NameGroup(arg),
        "leave_group" => Intent::LeaveGroup(arg),
        // Only a sentence that actually reads as a group change becomes one;
        // "make a cake" is not about groups and falls through as not understood.
        "friend" => match crate::friends::read_spoken(raw) {
            Some(_) => Intent::Friend(raw.trim().to_string()),
            None => Intent::Unknown(raw.trim().to_string()),
        },
        "update_status" => Intent::Updates("status".into()),
        "update_install" => Intent::Updates("install".into()),
        "update_undo" => Intent::Updates("undo".into()),
        // Raw: these words are sent exactly as said.
        "feedback_send" => Intent::Feedback(format!("send:{}", arg.trim())),
        "feedback_send_bare" => Intent::Feedback(format!("bare:{}", arg.trim())),
        "feedback_list" => Intent::Feedback("list".into()),
        "phone_model_get" => Intent::PhoneModel("get".into()),
        "phone_model_status" => Intent::PhoneModel("status".into()),
        "feedback_reply" => Intent::Feedback(format!("reply:{}", arg.trim())),
        "change_group" => match crate::groups::read_spoken(raw) {
            Some(_) => Intent::ChangeGroup(raw.trim().to_string()),
            None => Intent::Unknown(raw.trim().to_string()),
        },
        // `arg` here is the RAW remainder, because `commands.yaml` marks this
        // command `raw_argument: true`. That is load-bearing rather than
        // tidy: `normalize` lowercases and strips punctuation, which is
        // precisely what dictated text exists to keep.
        "dictate" => Intent::Dictate(arg),
        _ => Intent::Unknown(raw.trim().to_string()),
    }
}

/// A request with its politeness taken off: "can you please check my email?"
/// is "check my email". `None` when there was nothing to take off.
fn polite_rest(input: &str) -> Option<&str> {
    let t = input.trim().trim_end_matches(['?', '!', '.']).trim_end();
    let lower = t.to_ascii_lowercase();
    for p in ["can you please ", "could you please ", "would you please ", "please can you ", "can you ", "could you ", "would you ", "will you ", "can u ", "please "] {
        if lower.starts_with(p) && t.len() > p.len() {
            let rest = t[p.len()..].trim_start();
            let rest = rest.strip_suffix(" please").unwrap_or(rest).trim();
            let rest = rest.strip_suffix(" for me").unwrap_or(rest).trim();
            return (!rest.is_empty()).then_some(rest);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Every command, offered to the model as a tool (27 Sep 2026).
//
// The model used to see thirteen actions in a hand-written schema, and a
// hand-written match accepted about 119 names back: of the 148 commands in
// `commands.yaml`, 32 could never be chosen by the model at all, and
// `brief_on` was advertised and then refused. The tools are now generated
// from `commands.yaml` itself -- one list, the parser's -- and a tool call
// comes back through the same `build` the phrases use, so anything the
// phrases can reach the model can reach, and nothing else.
// ---------------------------------------------------------------------------

/// Commands the model is never offered and never allowed to choose, whatever
/// `commands.yaml` says: typing into windows, the vault, keys, installing or
/// undoing updates, and feedback sent in your name. Their phrases still work.
///
/// Also what a model must not decide on its own: pausing and resuming (a
/// "wait" mid-sentence is not a pause), the consent answers on a recorded
/// call, the waking brief, and anything that changes what Atlas believes
/// about you or how it behaves. `commands.yaml` marks the same ones
/// `expose: never` (checked by `tests/talking_freely.rs`).
pub const NEVER_FOR_THE_MODEL: &[&str] = &[
    "dictate", "snippet", "type_code", "unlock", "set_key", "update_install", "update_undo",
    "feedback_send", "feedback_send_bare", "feedback_reply", "reminder",
    "pause", "resume", "ready",
    "call_record_everyone", "call_they_agreed", "call_they_declined", "call_couldnt_ask", "call_no_answer",
    "call_just_mine",
    "got_it_wrong", "apply_lesson", "this_is_me", "address_as",
];

/// How a command is offered to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exposure {
    /// Every turn, in a fixed order, so the prompt's prefix stays the same.
    Core,
    /// When the sentence reads like it.
    Retrieved,
    /// Never.
    Never,
}

/// One command as a tool.
#[derive(Debug, Clone)]
pub struct ToolEntry {
    pub name: String,
    pub describe: String,
    pub takes_arg: bool,
    pub arg_optional: bool,
    pub exposure: Exposure,
    pub phrases: Vec<String>,
}

impl ToolEntry {
    /// The OpenAI-shaped definition.
    pub fn spec(&self) -> serde_json::Value {
        use serde_json::json;
        let (what, arg) = match self.describe.split_once(" arg: ") {
            Some((w, a)) => (w.trim().to_string(), a.trim().to_string()),
            None => (self.describe.trim().to_string(), String::new()),
        };
        let parameters = if self.takes_arg {
            let arg_doc = if arg.is_empty() { "What it is about, in the user's words.".to_string() } else { arg };
            let mut p = json!({
                "type": "object",
                "properties": {"arg": {"type": "string", "description": arg_doc}},
            });
            if !self.arg_optional {
                p["required"] = json!(["arg"]);
            }
            p
        } else {
            json!({"type": "object", "properties": {}})
        };
        json!({"type": "function", "function": {"name": self.name, "description": what, "parameters": parameters}})
    }
}

/// Every command as a tool, and a way to pick the ones a sentence needs.
pub struct ToolBook {
    entries: Vec<ToolEntry>,
    index: crate::bm25::Index,
}

impl ToolBook {
    pub fn new(cfg: &CommandsConfig) -> ToolBook {
        let mut entries: Vec<ToolEntry> = Vec::new();
        for c in &cfg.commands {
            // An intent listed twice (`friend`) is one tool with all its
            // phrases.
            if let Some(e) = entries.iter_mut().find(|e| e.name == c.intent) {
                e.phrases.extend(c.phrases.iter().cloned());
                e.takes_arg |= c.takes_argument;
                continue;
            }
            let exposure = if NEVER_FOR_THE_MODEL.contains(&c.intent.as_str()) {
                Exposure::Never
            } else {
                match c.expose.as_deref().map(str::trim) {
                    Some("core") => Exposure::Core,
                    Some("never") => Exposure::Never,
                    _ => Exposure::Retrieved,
                }
            };
            entries.push(ToolEntry {
                name: c.intent.clone(),
                describe: c.describe.clone().unwrap_or_else(|| format!("The \"{}\" command.", c.phrases.first().cloned().unwrap_or_default())),
                takes_arg: c.takes_argument,
                arg_optional: c.argument_optional,
                exposure,
                phrases: c.phrases.clone(),
            });
        }
        let mut index = crate::bm25::Index::default();
        for (i, e) in entries.iter().enumerate() {
            if e.exposure == Exposure::Retrieved {
                index.add(i as u64, &e.phrases.join(" "), &e.describe);
            }
        }
        ToolBook { entries, index }
    }

    pub fn entries(&self) -> &[ToolEntry] {
        &self.entries
    }

    pub fn get(&self, name: &str) -> Option<&ToolEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// May the model choose this command?
    pub fn offered(&self, name: &str) -> bool {
        self.get(name).is_some_and(|e| e.exposure != Exposure::Never)
    }

    /// The tools for one sentence: every core tool, always in the same
    /// order, then up to `more` others the sentence reads like.
    pub fn for_sentence(&self, said: &str, more: usize) -> Vec<serde_json::Value> {
        let mut out: Vec<serde_json::Value> =
            self.entries.iter().filter(|e| e.exposure == Exposure::Core).map(|e| e.spec()).collect();
        for (id, _score) in self.index.search(said, more) {
            if let Some(e) = self.entries.get(id as usize) {
                out.push(e.spec());
            }
        }
        out
    }
}

/// A tool call as the command it names. `said` is the sentence it answers,
/// which the commands that read the whole utterance are given, exactly as
/// the phrase path gives it to them. `None` for a name that isn't a command
/// or that the model may not choose.
pub fn from_tool(name: &str, args: &serde_json::Value, said: &str) -> Option<Intent> {
    let name = name.trim();
    if NEVER_FOR_THE_MODEL.contains(&name) {
        return None;
    }
    // Another program's tool (`mcp`): carried as it came, to be placed,
    // asked about and run by the daemon. No command has this prefix.
    if name.starts_with(crate::mcp::PREFIX) {
        return Some(Intent::McpTool(crate::mcp::payload(name, args)));
    }
    let arg = match args {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(m) => m
            .get("arg")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            // A small model sometimes names the argument after what it is.
            .or_else(|| m.values().find_map(|v| v.as_str().map(str::to_string)))
            .unwrap_or_default(),
        _ => String::new(),
    };
    command_by_name(name, arg.trim().to_string(), said)
}

/// `build`, for a command named outright rather than matched by a phrase.
/// `None` when the name isn't a command.
fn command_by_name(name: &str, arg: String, raw: &str) -> Option<Intent> {
    let raw = if raw.trim().is_empty() { arg.clone() } else { raw.to_string() };
    match build(name, arg, &raw) {
        Intent::Unknown(_) => None,
        i => Some(i),
    }
}

/// Does this sentence ask for a report on Atlas itself, or what's left to
/// set up? Narrow on purpose: only these shapes, anywhere in the sentence.
fn asks_for_a_self_report(said: &str) -> bool {
    let t: String = said.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    [
        "report on yourself",
        "report on your self",
        "report about yourself",
        "status report",
        "self report",
        "diagnostic report",
        "what still needs to be set up",
        "what still needs setting up",
        "what still needs set up",
        "what needs to be set up",
        "whats left to set up",
    ]
    .iter()
    .any(|p| t.contains(p))
}
