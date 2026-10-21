//! Never "I'm on it" for work that isn't running (30 Sep 2026).
//!
//! Eric's evening: "And you start that research." -- "Alright -- I'm on it."
//! Nothing ran. "When you start that research, tell me what you find." --
//! "I'm already on it -- no need to wait." Nothing ran then either. The
//! small model says what an assistant would say, and nothing checked the
//! words against what happened.
//!
//! So a sentence that says work has started, is under way or is done is
//! held back from speech (`brain::SpeechGate`) until the reply is over. If a
//! tool or a job really started in that reply, the tool's own words say so;
//! if none did, the model is asked once more with a tool call required, and
//! if that still starts nothing the reply says plainly that nothing has
//! started (`NOT_STARTED`). The daemon applies the same rule to a reply
//! that came by the one-prompt path (`without_unbacked_claims`).

/// Said in place of a claim no tool or job backs.
pub const NOT_STARTED: &str = "I haven't actually started anything on that yet.";

/// Does this sentence say that work has started, is under way, or is done?
pub fn claims_work_started(sentence: &str) -> bool {
    let t = format!(" {} ", norm(sentence));
    const CLAIMS: &[&str] = &[
        " i'm on it ",
        " consider it done ",
        " consider that done ",
        " im on it ",
        " i am on it ",
        " already on it ",
        " on it now ",
        " on it already ",
        " i've started ",
        " ive started ",
        " i have started ",
        " i've begun ",
        " i have begun ",
        " started on it ",
        " starting on it ",
        " starting it now ",
        " starting now ",
        " i'm working on it ",
        " im working on it ",
        " i'm working on that ",
        " working on it now ",
        " i'm researching ",
        " im researching ",
        " i'm looking into it ",
        " im looking into it ",
        " i'm looking into that ",
        " i'm doing it ",
        " i'm doing that now ",
        " i'll get started ",
        " ill get started ",
        " getting started now ",
        " i'll start on it ",
        " i'll start on that ",
        " i'll start now ",
        " i'll start the ",
        " consider it done ",
        " all done ",
        " i've done it ",
        " i have done it ",
        " done and done ",
        " i'll report back ",
        " i will report back ",
        " i'll let you know what i find ",
        " i'll tell you what i find ",
        " i'll get back to you ",
        " i'm already doing ",
        " im already doing ",
        // An announced look with nothing looked at (30 Sep 2026: "I'll check
        // your calendar." was a whole reply when the forced ask still called
        // nothing -- a promise nothing followed).
        " i'll check ",
        " ill check ",
        " let me check ",
        " i'll look it up ",
        " let me look it up ",
        " let me look that up ",
        " i'll pull up ",
        " let me pull up ",
        " i'll find out ",
        // A result claimed with nothing run to get it (30 Sep 2026, a real
        // 2B model: "The machine health check found that the RAM usage has
        // spiked to 85%", no tool called).
        " check found ",
        " health check found ",
        " the check shows ",
        " the model trace shows ",
        " i checked ",
        " i've checked ",
        " i have checked ",
        " i am checking ",
        " i'm checking ",
        " here is what i found ",
        " here's what i found ",
        " i looked it up ",
        " i've looked it up ",
        " i just looked ",
        // And a switch claimed flipped, or a camera claimed on (the same 2B:
        // "I'm looking through your camera and can definitely see you right
        // now ... I've turned on Recognising things in settings").
        " i've turned on ",
        " i have turned on ",
        " i turned on ",
        " i've switched on ",
        " i've enabled ",
        " i'm looking through your camera ",
        " looking through your camera ",
        " can definitely see you ",
        " i can see you right now ",
        // A sight described with no picture taken (Eric, 1 Oct 2026: "I see
        // you -- standing there, holding the camera like it's a microphone",
        // and the log shows no look at all). A real look comes from the
        // camera's own tool, which counts as work started.
        " i see you ",
        " i can see you ",
        " i can see someone ",
        " i can see a person ",
        " you're holding ",
        " you are holding ",
        " holding the camera ",
        " i see you're standing ",
        " i see you're sitting ",
        // Doing it, said by a model that did nothing (30 Sep 2026, Atlas's
        // own self-test on the laptop: "I'm focusing on the quarterly budget
        // now", "I'll add the quarterly budget to your calendar" -- no tool).
        " i'm focusing ",
        " i've focused ",
        " i'm opening ",
        " i've opened ",
        " i'll open ",
        " i'm adding ",
        " i'll add ",
        " i've added ",
        " i'm creating ",
        " i'll create ",
        " i've created ",
        " i'll schedule ",
        " i've scheduled ",
        " i'm scheduling ",
        " i'll send ",
        " i've sent ",
        " i'm sending ",
        " i'll save ",
        " i've saved ",
        " i'm saving ",
        " i'll move ",
        " i've moved ",
        " i'm moving ",
        " i'm switching ",
        " i've switched ",
        " i'll set up ",
        " i've set up ",
        " i'll remind you ",
        " i've set a reminder ",
        // Eric's evening, 30 Sep 2026, a 4B model with no tool called:
        // "TradingView's open -- I've got it ready", "Camera's on -- you're
        // good to go" (before the camera was allowed), "I've got the call
        // notes ... ready for you", "Chrome's self-improvement list is active
        // -- we're tracking progress", and a made-up list of what's pending.
        "'s open ",
        " is open now ",
        " got it ready ",
        " got them ready ",
        " got those ready ",
        " ready for you ",
        "camera's on ",
        " camera is on ",
        " working through it now ",
        " i'm working on tweaks ",
        " im working on tweaks ",
        " tracking progress ",
        " still pending on the ",
        " here's what's still pending ",
        " got your list ready ",
        " got the call notes ",
        " got those call notes ",
        " got your call notes ",
        " notes are ready ",
        " got those notes ready ",
        " i've got your screen ",
        // 1 Oct 2026 model ranking (Qwen3-4B-2507, no tool called): "I've
        // noted that your car insurance renews in March", and a made-up job
        // "check out this one: [link]".
        " i've noted ",
        " i have noted ",
        " i've made a note ",
        " noted that ",
        " i've written that down ",
        " i've jotted ",
        " i found a few ",
        " check out this one ",
        // Qwen3.5-4B, same run, its tool calls failing: "I've pulled your
        // Friday schedule. You have a team sync at ten and a client call
        // with Sarah" (none of it real), "I've just added it to your calendar".
        " i've pulled ",
        " i've just added ",
        " i've just set ",
        " i've just saved ",
        " i'll set that reminder ",
        " i've put that ",
        " i've put it ",
        // Round two (the current Qwen3-VL, Qwen3-4B-2507, Gemma 4): "I'll
        // make a note that your passport expires", "I made a picture of a
        // cozy cabin", "I found your file named 2025 Tax Return", "I'll look
        // for your tax return ... Let me search your files" -- no tool.
        " i'll make a note ",
        " i'll make sure to ",
        " i made a picture ",
        " i found your file ",
        " i'll look for your ",
        " let me search your ",
        " i've got your friday ",
        " let me pull them up ",
    ];
    CLAIMS.iter().any(|c| t.contains(c)) || claims_by_structure(sentence)
}

/// Action verbs, in the progressive, that open a subjectless claim of work
/// ("Making atlas-test.txt on your Desktop"). Verbs that need a particle
/// are in `PROGRESSIVE_PHRASES`.
const PROGRESSIVE: &[&str] = &[
    "making",
    "creating",
    "writing",
    "saving",
    "copying",
    "moving",
    "deleting",
    "sending",
    "downloading",
    "installing",
    "opening",
    "running",
    "starting",
    "booking",
    "scheduling",
    "adding",
    "fetching",
    "checking",
    "searching",
    "researching",
    "generating",
    "building",
    "preparing",
    "drafting",
    "recording",
    "restoring",
    "uploading",
    "renaming",
    "removing",
    "updating",
    "fixing",
    "pulling",
    "verifying",
    "confirming",
    "transcribing",
    "syncing",
    "launching",
    "compiling",
    "converting",
    "zipping",
    "setting",
    "backing",
];

/// Progressive claims that are only claims with their particle.
const PROGRESSIVE_PHRASES: &[&str] = &[
    "backing up",
    "setting up",
    "working on",
    "looking into",
    "looking at it",
    "taking a look",
    "taking care of",
    "taking a break",
    "getting that",
    "getting it",
];

/// What follows "I'll" / "I will" in a promise of work.
const FUTURE_ACTIONS: &[&str] = &[
    "make",
    "create",
    "write",
    "save",
    "back up",
    "copy",
    "move",
    "delete",
    "send",
    "download",
    "install",
    "open",
    "run",
    "start",
    "set up",
    "book",
    "schedule",
    "add",
    "fetch",
    "check",
    "search",
    "research",
    "generate",
    "build",
    "prepare",
    "draft",
    "record",
    "restore",
    "upload",
    "rename",
    "remove",
    "update",
    "fix",
    "confirm",
    "verify",
    "take a look",
    "take care",
    "let you know",
    "get that",
    "get it",
    "get this",
    "do that",
    "do it",
    "look into",
    "look at it",
];

/// Said before the real sentence ("Sure, taking a break"). Longest first.
const LEAD_INS: &[&str] = &[
    "sure thing",
    "of course",
    "no problem",
    "all right",
    "got it",
    "thank you",
    "alright",
    "absolutely",
    "certainly",
    "perfect",
    "okay",
    "sure",
    "yes",
    "yep",
    "yeah",
    "great",
    "right",
    "thanks",
    "well",
    "ok",
    "nice",
];

/// Gerunds that open ordinary sentences as often as claims.
const WEAK_GERUND: &[&str] = &["running", "starting", "checking"];

/// Words that make a sentence a general statement rather than a report.
const GENERAL: &[&str] = &[
    "usually",
    "typically",
    "often",
    "generally",
    "always",
    "normally",
    "sometimes",
    "never",
    "rarely",
    "tends",
    "tend",
    "fixes",
    "last",
    "lasts",
    "best",
    "mostly",
];

/// Verbs that promise work only when the task is named or "now" is said.
const WEAK_FUTURE: &[&str] = &[
    "start", "run", "check", "open", "add", "update", "fix", "search", "research", "build",
    "prepare", "record", "draft", "generate", "set up",
];

/// Whether `after` (the words after a weak verb) names the task: "it",
/// "that", "the backup", a file or path, or the sentence says "now".
fn task_object(after: &[&str], now: bool, sentence: &str) -> bool {
    if now {
        return true;
    }
    if matches!(
        after.first().copied(),
        // Not "this": "I'll make this simple" is a manner of speaking.
        Some("it" | "that" | "them" | "those")
    ) {
        return true;
    }
    if after.first() == Some(&"the")
        && matches!(
            after.get(1).copied(),
            Some(
                "backup"
                    | "folder"
                    | "file"
                    | "download"
                    | "update"
                    | "install"
                    | "copy"
                    | "script"
                    | "test"
            )
        )
    {
        return true;
    }
    sentence.split_whitespace().any(|w| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric());
        w.contains('\\')
            || w.contains('/')
            // A file name has a lettered extension; "version 2.0" is not one.
            || w.split_once('.').is_some_and(|(a, b)| {
                !a.is_empty() && b.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            })
    })
}

/// A finite verb right after the gerund means it is the sentence's subject
/// ("Making a backup copies Atlas's own state"), not a claim.
const FINITE: &[&str] = &[
    "is", "are", "was", "were", "means", "takes", "helps", "requires", "can", "could", "will",
    "would", "should", "may", "might", "does", "keeps", "gives", "lets", "makes", "creates",
    "copies", "saves", "writes", "moves", "deletes", "sends", "runs", "protects", "ensures",
    "causes", "uses", "needs", "involves", "includes", "allows", "has", "have", "had", "depends",
    "adds", "opens", "starts", "sets", "checks", "builds", "records", "restores", "isn't",
    "doesn't", "won't", "sounds", "feels", "seems", "looks", "becomes", "remains", "gets", "goes",
    "works", "matters", "beats", "wins",
];

/// The sentence says work started, is running or is done, by how it is
/// built rather than by a phrase (20 Oct 2026). The installed build's free
/// online model answered "Create a text file ... then verify it" with "On it.
/// Making atlas-test.txt on your Desktop with today's date. I'll take a look
/// at it to confirm once it's there." -- no list of phrases had any of it,
/// and nothing was made. Questions, offers on a condition ("I'll create it
/// once you pick a folder") and a gerund that is the sentence's subject are
/// left alone.
fn claims_by_structure(sentence: &str) -> bool {
    if sentence.trim_end().ends_with('?') {
        return false;
    }
    let n = norm(sentence);
    if n.is_empty() {
        return false;
    }
    let t = format!(" {n} ");
    // (d) work reported as in hand or done.
    const STATUS: &[&str] = &[
        " still running ",
        " still working on ",
        " still in progress ",
        " is in progress ",
        " it's in progress ",
        " currently in progress ",
        " is underway ",
        " it's underway ",
        " is under way ",
        " now underway ",
        " still underway ",
        " it should be done ",
        " it should be finished ",
        " has been created ",
        " has been saved ",
        " has been backed up ",
        " has been copied ",
        " has been moved ",
        " has been deleted ",
        " has been sent ",
        " has been written ",
        " have been created ",
        " have been saved ",
        " have been backed up ",
        " have been copied ",
        " has finished ",
        " has completed ",
        " have finished ",
        " finished successfully ",
        " completed successfully ",
    ];
    // Said about the work in hand: by the speaker, by nobody ("Still
    // running"), or by "it" / "the <task>". "Windows will let you know" and
    // "That should be done in a minute" are about something else.
    let status_subject_ok = |p: &str| {
        if !t.contains(p) {
            return false;
        }
        let lead: Vec<&str> = n.split_whitespace().collect();
        let first = lead.first().copied().unwrap_or("");
        if matches!(
            first,
            "it" | "it's" | "still" | "currently" | "now" | "i'm" | "im" | "we're" | "i" | "we"
        ) {
            return true;
        }
        // "Backing up C:/x is still in progress": the activity is the subject.
        if PROGRESSIVE.contains(&first) && !WEAK_GERUND.contains(&first) {
            return true;
        }
        // "The backup of C:/x/y is still in progress": a path can be long.
        first == "the"
    };
    if STATUS.iter().any(|p| t.contains(p) && status_subject_ok(p)) {
        return true;
    }
    if (n.ends_with(" it's done") || n.ends_with(" it's done now") || n.ends_with(" it is done"))
        || matches!(n.as_str(), "done" | "finished" | "all set" | "it's done")
    {
        return true;
    }
    // "I'll let you know" says work is going on, whatever it is.
    // Said by Atlas: first person, or with no subject at all ("Will let you
    // know if any issues"); "Windows will let you know" is Windows.
    if t.contains(" i'll let you know ") || t.contains(" i will let you know ") {
        return true;
    }
    // Strip "Sure, " / "Alright, " from the front.
    let mut rest = n.as_str();
    'strip: loop {
        for lead in LEAD_INS {
            if let Some(r) = rest.strip_prefix(lead) {
                if r.is_empty() || r.starts_with(' ') {
                    rest = r.trim_start();
                    continue 'strip;
                }
            }
        }
        break;
    }
    let words: Vec<&str> = rest.split_whitespace().collect();
    let Some(first) = words.first().copied() else {
        return false;
    };
    if rest.starts_with("will let you know") || rest.starts_with("we'll let you know") {
        return true;
    }
    // (a) "On it", "Doing it", "Will do".
    if rest == "on it"
        || rest.starts_with("on it ")
        || rest.starts_with("doing it")
        || rest.starts_with("doing that")
        || rest == "will do"
        || rest.starts_with("will do ")
    {
        return true;
    }
    if rest == "on that"
        || rest.starts_with("on that now")
        || (rest.starts_with("on that ")
            && words
                .get(2)
                .is_some_and(|w| PROGRESSIVE.contains(w) || *w == "now"))
    {
        return true;
    }
    // (b) A progressive verb with no subject.
    let progressive = PROGRESSIVE_PHRASES.iter().find(|p| rest.starts_with(*p));
    // "Running shoes ...", "Starting with a small dose ...", "Checking in
    // ...": these open ordinary sentences as often as a claim, so they are a
    // claim only with "now" in it.
    let says_now = words.contains(&"now");
    let single = PROGRESSIVE.contains(&first)
        && !(first == "starting" && words.get(1) == Some(&"to"))
        && !matches!(first, "backing" | "setting")
        && (!WEAK_GERUND.contains(&first) || says_now);
    if progressive.is_some() || single {
        let skip = progressive
            .map(|p| p.split_whitespace().count())
            .unwrap_or(1);
        // A verb anywhere makes it a sentence about the activity ("Starting
        // with a small dose usually works best"), not a report of doing it;
        // so does a word that generalises.
        let sentence_words = words.iter().skip(skip);
        let general = sentence_words
            .clone()
            .any(|w| FINITE.contains(w) || GENERAL.contains(w));
        // A gerund clause set off by a comma with no task named is an
        // instruction's lead-in ("Adding the eggs one at a time, beat until
        // smooth"), not a report (21 Oct 2026 review).
        // Only commas after the "Sure, " that opens it count.
        let own = sentence.to_lowercase();
        let from = own.find(first).unwrap_or(0);
        let lead_in = own[from..].contains(',') && !task_object(&[], false, sentence);
        if says_now || (!general && !lead_in) {
            return true;
        }
    }
    // (c) First person, going on or promised.
    let conditional = [
        " if ",
        " once you ",
        " when you ",
        " after you ",
        " unless ",
        " want me to ",
        " would you like ",
    ]
    .iter()
    .any(|c| t.contains(c));
    for (i, w) in words.iter().enumerate() {
        let after = &words[i + 1..];
        match *w {
            "i'm" | "im" | "i" if *w != "i" || after.first() == Some(&"am") => {
                let after = if *w == "i" { &after[1..] } else { after };
                let Some(v) = after.first() else { continue };
                let particle = after.get(1).copied();
                let ok = PROGRESSIVE.contains(v)
                    && !(matches!(*v, "backing" | "setting") && particle != Some("up"))
                    && !(*v == "starting" && particle == Some("to"))
                    && (!WEAK_GERUND.contains(v)
                        || task_object(&after[1..], words.contains(&"now"), sentence));
                if ok
                    || PROGRESSIVE_PHRASES
                        .iter()
                        .any(|p| after.join(" ").starts_with(p))
                {
                    return true;
                }
            }
            "i'll" | "ill" | "i" | "we'll" if !conditional => {
                let after = if *w == "i" {
                    if after.first() != Some(&"will") {
                        continue;
                    }
                    &after[1..]
                } else {
                    after
                };
                let said = after.join(" ");
                if let Some(a) = FUTURE_ACTIONS
                    .iter()
                    .find(|a| said == **a || said.starts_with(&format!("{a} ")))
                {
                    // "I'll start with the basics", "I'll run through the
                    // steps": these verbs promise work only with the task
                    // named, or "now".
                    let object: Vec<&str> = said
                        .split_whitespace()
                        .skip(a.split_whitespace().count())
                        .collect();
                    // "I'll make this simple": a manner of speaking.
                    if *a == "make" && matches!(object.first(), Some(&("this" | "things" | "sure")))
                    {
                        continue;
                    }
                    // "We'll create the sauce first" is a plan said to a
                    // reader, so "we'll" needs the task named like a weak verb.
                    if (!WEAK_FUTURE.contains(a) && *w != "we'll")
                        || task_object(&object, words.contains(&"now"), sentence)
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Lowercase words with the apostrophes straightened and the punctuation
/// that isn't part of a word turned to spaces.
fn norm(s: &str) -> String {
    let s = s.to_lowercase().replace(['\u{2019}', '\u{2018}'], "'");
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A reply with every unbacked claim taken out and the truth said instead.
/// `started`: a tool or job really did start in this turn (then the text is
/// left as it is).
pub fn without_unbacked_claims(text: &str, started: bool) -> String {
    without_unbacked_claims_for(text, started, "", true)
}

/// Said when no language model that can act is installed (20 Oct 2026): the
/// free online model answers but has no tools, so "make a file" got "On it"
/// and nothing. The reason, and the one thing that fixes it -- there is no
/// spoken phrase that fetches a model, only the Connections page's button.
pub const NO_ACTING_MODEL: &str = "I can't do that yet: no language model that can act is installed on this computer, so nothing was started. To fix that, open the Connections page in the Atlas hub and press \"Get the better model\", then ask again.";

/// Said when "quit Atlas" really did ask the daemon to stop.
pub const STOPPING: &str = "Stopping Atlas now -- everything's written down.";

/// Said when the question was about work's progress and no such job exists.
pub const NO_SUCH_JOB: &str =
    "There's no job like that in Outstanding, so nothing is running on it.";

/// What replaces a claim nothing backs: says why nothing started and what
/// to do next. `can_act`: the model in use takes tools (`Llm::native_chat`),
/// so a claim without a call is its slip, not a missing ability.
fn not_started_because(can_act: bool) -> &'static str {
    if can_act {
        NOT_STARTED
    } else {
        NO_ACTING_MODEL
    }
}

/// Is `said` a question about how work is going -- "did the backup finish?"
pub fn asks_for_status(said: &str) -> bool {
    let n = norm(said);
    let opens = [
        "did ",
        "is ",
        "are ",
        "has ",
        "have ",
        "was ",
        "how is ",
        "how's ",
        "hows ",
        "any news ",
        "any update ",
        "what's the status ",
        "whats the status ",
        "status of ",
    ]
    .iter()
    .any(|o| n.starts_with(o));
    let t = format!(" {n} ");
    let about_work = [
        " finish",
        " done",
        " complete",
        " still ",
        " progress",
        " going",
        " status",
        " ready",
        " yet ",
        " work",
    ]
    .iter()
    .any(|w| t.contains(w));
    opens && about_work
}

/// `without_unbacked_claims`, told what was asked and whether the model can
/// act, so the replacement says the true reason. A status question ("did the
/// backup finish?") answered with a claim is replaced whole: the model's
/// guess at a job that doesn't exist is not kept in part.
pub fn without_unbacked_claims_for(text: &str, started: bool, said: &str, can_act: bool) -> String {
    if started {
        return text.to_string();
    }
    let sentences = crate::repeating::sentences(text);
    if !sentences.iter().any(|s| claims_work_started(s)) {
        return text.to_string();
    }
    if asks_for_status(said) {
        return format!("{NO_SUCH_JOB} {}", not_started_because(can_act));
    }
    let saw = sentences.iter().any(|s| claims_sight(s));
    let truth = if saw {
        NOT_LOOKED
    } else {
        not_started_because(can_act)
    };
    let kept: Vec<String> = sentences
        .into_iter()
        .filter(|s| !claims_work_started(s))
        .collect();
    let kept = kept.join(" ");
    if kept.trim().is_empty() {
        truth.to_string()
    } else {
        format!("{} {truth}", kept.trim())
    }
}

/// Said in place of a sight nothing was looked at for.
pub const NOT_LOOKED: &str = "I haven't actually looked -- say \"look at me\" and I will.";

/// Does this sentence describe something seen through the camera?
fn claims_sight(sentence: &str) -> bool {
    let t = format!(" {} ", norm(sentence));
    [
        " i see you ",
        " i can see you ",
        " i can see someone ",
        " i can see a person ",
        " you're holding ",
        " you are holding ",
        " holding the camera ",
        " looking through your camera ",
        " can definitely see you ",
    ]
    .iter()
    .any(|c| t.contains(c))
}

/// Does this sentence say Atlas lacks, or may not use, an ability it has?
/// The ability, in a word the catalogue can be searched with (`""`: a
/// refusal that names none -- "I'm not supposed to" -- so the question
/// itself is searched instead).
///
/// Eric's evening, 30 Sep 2026: "I don't have a camera -- so no selfies,
/// sorry", then "I can't -- I'm not supposed to", while the catalogue lists
/// looking through the camera; the night before, "I don't have a research
/// mode". The model is told the truth before it answers
/// (`capability::abilities_for_prompt`); this is the backstop when it says
/// it anyway.
pub fn denies_an_ability(sentence: &str) -> Option<&'static str> {
    let t = format!(
        " {} ",
        norm(sentence)
            .replace("do not", "don't")
            .replace("cannot", "can't")
            .replace("dont", "don't")
            .replace("cant", "can't")
    );
    const DENIALS: &[(&str, &str)] = &[
        (" don't have a camera", "camera"),
        (" don't have access to your camera", "camera"),
        (" don't have access to the camera", "camera"),
        (" no camera", "camera"),
        (" can't see you", "camera"),
        (" can't use your camera", "camera"),
        (" can't use the camera", "camera"),
        (" can't look at you", "camera"),
        (" can't access your camera", "camera"),
        (" don't have a research mode", "research"),
        (" can't do research", "research"),
        (" can't research", "research"),
        (" can't browse", "research"),
        (" can't access the internet", "research"),
        (" don't have internet", "research"),
        (" don't have access to the internet", "research"),
        (" can't look things up", "research"),
        (" can't see your screen", "screen"),
        (" can't look at your screen", "screen"),
        (" can't see what's on your screen", "screen"),
        // "I can't add capabilities to myself -- that's a system architecture
        // thing" (Eric, 1 Oct 2026): a request for a new ability is always
        // taken down for his yes (`growth`).
        (" can't add capabilities", "grow"),
        (" can't add capability", "grow"),
        (" can't add that capability", "grow"),
        (" can't add new capabilities", "grow"),
        (" can't add abilities", "grow"),
        (" can't add features", "grow"),
        (" can't add new features", "grow"),
        (" can't give myself", "grow"),
        (" can't build new capabilities", "grow"),
        (" can't build new abilities", "grow"),
        (" can't extend myself", "grow"),
        (" can't add to myself", "grow"),
        (" can't change my own", "grow"),
        (" can't improve myself", "grow"),
        (" system architecture thing", "grow"),
        (" i'm not supposed to", ""),
        (" i am not supposed to", ""),
        (" i'm not allowed to", ""),
        (" i am not allowed to", ""),
    ];
    if let Some((_, topic)) = DENIALS.iter().find(|(p, _)| t.contains(p)) {
        return Some(topic);
    }
    // Said another way: "I don't have personal cameras", "I do not have
    // access to external files" (30 Sep 2026, a real 0.8B model) -- a
    // "can't" or "don't have" with the ability named in the same sentence.
    let negated = [
        " don't have ",
        " can't ",
        " not able to ",
        " unable to ",
        " no access ",
        " don't have access",
        " without access",
    ]
    .iter()
    .any(|n| t.contains(n));
    if !negated {
        return None;
    }
    const TOPICS: &[(&[&str], &str)] = &[
        (&[" camera", " cameras", " webcam", " your face"], "camera"),
        (
            &[
                " internet",
                " the web",
                " browse",
                " research",
                " look things up",
                " online",
            ],
            "research",
        ),
        (
            &[
                " your files",
                " external files",
                " files",
                " documents",
                " your computer's files",
            ],
            "files",
        ),
        (&[" your screen", " the screen"], "screen"),
    ];
    TOPICS
        .iter()
        .find(|(words, _)| words.iter().any(|w| t.contains(w)))
        .map(|(_, topic)| *topic)
}

/// Does this sentence bring in a person or a story nobody mentioned --
/// "the freaky man", "a figment of your imagination"? `known` is everything
/// the model was given (what was said, the conversation, the notes).
///
/// Narrow on purpose: a stranger named by a role ("the X man", "a woman
/// who ...") that none of the words it was given contain. Eric's evening
/// had the model bring "the freaky man" into answers about his camera and
/// his research, turn after turn; a conversation about a man he did
/// mention is left alone.
pub fn invents_someone(sentence: &str, known: &str) -> bool {
    let s = norm(sentence);
    let known = norm(known);
    if s.contains("figment of your imagination") && !known.contains("imagination") {
        return true;
    }
    const WHO: &[&str] = &[
        "man", "woman", "guy", "girl", "lady", "dude", "stranger", "figure", "ghost",
    ];
    let words: Vec<&str> = s.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        if !WHO.contains(w) || i == 0 {
            continue;
        }
        // "the freaky man": the word before the role says who.
        let who = format!("{} {w}", words[i - 1]);
        let article = i >= 2 && matches!(words[i - 2], "the" | "a" | "this" | "that");
        if article && !known.contains(&who) {
            return true;
        }
    }
    false
}

/// Said for "I'm not supposed to" when the question names no ability: no
/// rule stops Atlas doing what its owner asks.
pub const NOTHING_STOPS_ME: &str =
    "Nothing stops me doing what you ask -- say what you'd like and I'll do it.";

/// The ability a question is about, in the words `denies_an_ability`
/// returns: "camera", "research", "screen", or `""` for none of them.
pub fn ability_asked_about(said: &str) -> &'static str {
    let t = format!(" {} ", norm(said));
    if [
        " camera",
        " webcam",
        " see me",
        " look at me",
        " watching me",
    ]
    .iter()
    .any(|w| t.contains(w))
    {
        "camera"
    } else if [
        " research",
        " internet",
        " the web",
        " online",
        " look it up",
        " look up",
    ]
    .iter()
    .any(|w| t.contains(w))
    {
        "research"
    } else if t.contains(" screen") || t.contains(" monitor") {
        "screen"
    } else if [" file", " files", " pdf", " document", " folder"]
        .iter()
        .any(|w| t.contains(w))
    {
        "files"
    } else {
        ""
    }
}

/// Sentences that say work started or was done. Measured, not used for the
/// meaning check: with this small encoder a claim ("I've kicked that off",
/// 0.41) sits no closer to these than an ordinary offer does ("I can help
/// with that", 0.51), so claims stay with the phrase list alone and the
/// tool-call check behind it (30 Sep 2026,
/// `tests/meaning_checks_the_reply.rs`).
pub const CLAIM_EXAMPLES: &[&str] = &[
    "I'm on it.",
    "I've started working on that.",
    "I'm already working on it right now.",
    "I checked, and here is what I found.",
    "Done, I've taken care of it.",
    "I'll report back when I'm finished.",
    "I turned that setting on for you.",
    "I'm looking at you through your camera.",
];

/// Sentences that deny an ability, with the ability, for the meaning check.
pub const DENIAL_EXAMPLES: &[(&str, &str)] = &[
    ("I don't have a camera, so I can't see you.", "camera"),
    ("I'm unable to see you.", "camera"),
    ("I can't browse the internet.", "research"),
    ("I don't have the ability to search the web.", "research"),
    ("I can't see your screen.", "screen"),
    ("I can't get at the files on your computer.", "files"),
    ("I'm not allowed to do that.", ""),
];

/// Over this likeness to a denial example, a sentence is held as a denial.
/// Set with the real encoder (`tests/meaning_checks_the_reply.rs`): the
/// denials no list has scored 0.73 to 0.77; the closest ordinary reply ("I
/// can look through your camera if you'd like") 0.66.
pub const DENIAL_LIKE: f32 = 0.70;

/// What the meaning check found in a sentence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Meant {
    Denial(&'static str),
    Neither,
}

/// Each example's vector, made once the encoder runs.
pub struct Examples {
    pub denials: Vec<(Vec<f32>, &'static str)>,
}

/// The denial list's second net (30 Sep 2026: "model-checked replies instead
/// of phrase lists" was on the open list). A model call per sentence would
/// add seconds to every reply; the meaning encoder adds about fifteen
/// milliseconds, and catches the ways of saying it no list has.
pub fn meant(v: &[f32], ex: &Examples) -> Meant {
    let (deny, topic) = ex
        .denials
        .iter()
        .map(|(d, t)| (crate::router::cosine(v, d), *t))
        .fold((0.0f32, ""), |a, b| if b.0 > a.0 { b } else { a });
    if deny >= DENIAL_LIKE {
        Meant::Denial(topic)
    } else {
        Meant::Neither
    }
}

/// Something that can say what a sentence means (`meaningroute::Route`),
/// installed once the encoder runs. Process-wide because the speech gate
/// sits deep in `brain`, far from the daemon that owns the encoder.
pub trait MeaningCheck: Send + Sync {
    fn check(&self, sentence: &str) -> Meant;
}

static CHECKER: std::sync::OnceLock<Box<dyn MeaningCheck>> = std::sync::OnceLock::new();

/// Install the meaning check. The first one stays.
pub fn install(c: Box<dyn MeaningCheck>) {
    // unheard-ok: a OnceLock already set keeps its first value, which is the one wanted
    let _ = CHECKER.set(c);
}

/// What the meaning check says of a sentence; `Neither` with none installed.
pub fn check_meaning(sentence: &str) -> Meant {
    CHECKER
        .get()
        .map(|c| c.check(sentence))
        .unwrap_or(Meant::Neither)
}

/// Said for "record 5 seconds from my microphone" (20 Oct 2026: the model
/// said "I can't record audio from your microphone" with no reason and no
/// way forward). Atlas listens for its wake word and answers what follows;
/// dictation types what you say. There is no ability that records a clip,
/// and none is claimed.
pub const NO_CLIP_RECORDING: &str = "I can't record a clip from the microphone: I listen for my wake word and answer what you say after it, but I have no record-a-clip ability, so nothing was recorded. The closest things I can do: say \"atlas\" and then speak, and I'll answer what I heard, or say \"start dictating\" and I'll type what you say into the window in front.";

/// Is this a request to record sound from the microphone for a while?
/// Calls (`call_notes_on`, "record everyone") are a real ability with their
/// own phrases and are left to them.
pub fn asks_to_record_a_clip(said: &str) -> bool {
    let t = format!(" {} ", norm(said));
    let record = t.contains(" record ") || t.contains(" capture ") || t.contains(" tape ");
    let mic = [
        " microphone",
        " mic ",
        " my voice ",
        " audio ",
        " from my mic",
        " the mic ",
    ]
    .iter()
    .any(|m| t.contains(m));
    let call = t.contains(" call ") || t.contains(" meeting ") || t.contains(" everyone ");
    record && mic && !call
}

/// What to do next when a file isn't where it was said to be: the closest
/// names in its folder (at most three, from a bounded listing), or that the
/// folder isn't there either -- and, either way, the phrase that searches
/// for it (20 Oct 2026: "I can't read that file" with nothing after it).
fn missing_file_next_step(path: &str) -> String {
    let p = std::path::Path::new(path);
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let search = if name.is_empty() {
        "Say \"find the file\" and what it was called, and I'll search for it.".to_string()
    } else {
        format!("Say \"find the file {name}\" and I'll search for it.")
    };
    let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return search;
    };
    if !dir.is_dir() {
        return format!(
            "The folder {} doesn't exist either. {search}",
            dir.display()
        );
    }
    let want = name.to_lowercase();
    let mut near: Vec<(f64, String)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .take(2000)
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    let score = crate::linkage::jaro_winkler(&want, &n.to_lowercase());
                    (score >= 0.6).then_some((score, n))
                })
                .collect()
        })
        .unwrap_or_default();
    near.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    near.truncate(3);
    if near.is_empty() {
        format!(
            "The folder {} is there, but nothing in it has a name like that. {search}",
            dir.display()
        )
    } else {
        let names: Vec<String> = near.into_iter().map(|(_, n)| n).collect();
        format!(
            "The folder {} is there; the closest names in it are {}. Say which one you meant, or: {}",
            dir.display(),
            names.join(", "),
            search.replacen("Say ", "say ", 1)
        )
    }
}

/// "I can't find <path>." with what to do next (`missing_file_next_step`).
pub fn cant_find(path: &str) -> String {
    format!("I can't find {path}. {}", missing_file_next_step(path))
}

#[cfg(test)]
mod growth_tests {
    use super::*;

    #[test]
    fn a_sight_with_no_look_is_taken_back() {
        let said = without_unbacked_claims(
            "I see you — standing there, holding the camera like it's a microphone. What's up?",
            false,
        );
        assert!(said.contains(NOT_LOOKED), "{said}");
        assert!(!said.contains("standing there"), "{said}");
        let real = "I can see you at your desk.";
        assert_eq!(without_unbacked_claims(real, true), real);
        assert!(!claims_work_started(
            "I see you're asking about the weather."
        ));
    }

    #[test]
    fn saying_it_cant_gain_abilities_is_a_denial() {
        assert_eq!(
            denies_an_ability(
                "I can't add capabilities to myself — that's a system architecture thing."
            ),
            Some("grow")
        );
        assert_eq!(
            denies_an_ability("I can't add that capability with the current setup."),
            Some("grow")
        );
    }
}

#[cfg(test)]
mod structure_tests {
    use super::*;

    /// The exact sentences of the 20 Oct 2026 desktop test, and ordinary
    /// ways a model says the same.
    #[test]
    fn the_desktop_test_sentences_are_claims() {
        for s in [
            "On it.",
            "Making atlas-test.txt on your Desktop with today's date, 20 October 2026.",
            "I'll take a look at it to confirm once it's there.",
            "Backing up C:\\Users\\erics\\atlas-e2e-backup-test now.",
            "Will let you know if any issues.",
            "The backup is still in progress; once it completes you'll find the copied folder there.",
            "Sure, taking a break",
            "Sure thing, creating the folder now.",
            "Doing it now.",
            "Will do.",
            "Writing the file now.",
            "I'm saving it to your Desktop.",
            "I will copy the folder across.",
            "I'll confirm when it's finished.",
            "I'll let you know when it's finished.",
            "The file has been created.",
            "It's underway.",
            "It should be done in a minute.",
            "All of it is done.",
            "Done.",
        ] {
            assert!(claims_work_started(s), "{s}");
        }
    }

    #[test]
    fn advice_and_description_are_not_claims_but_real_claims_still_are() {
        for s in [
            "Starting with a small dose usually works best.",
            "Updating drivers usually fixes it.",
            "Running shoes typically last 500 miles.",
            "Opening a savings account online typically requires ID.",
            "I'm running locally on your computer.",
            "I am running a 4B model.",
            "I'll start with the basics.",
            "I'll run through the steps.",
            "Once it finishes downloading, restart.",
            "That should be done in a minute.",
            "Windows will let you know when it's ready.",
        ] {
            assert!(!claims_work_started(s), "false positive: {s}");
        }
        for s in [
            "On it.",
            "Making atlas-test.txt on your Desktop with today's date, 20 October 2026.",
            "I'll take a look at it to confirm once it's there.",
            "Backing up C:\\Users\\erics\\atlas-e2e-backup-test now.",
            "Will let you know if any issues.",
            "The backup is still in progress; once it completes you'll find the copied folder there.",
            "I'm on it",
            "Consider it done",
            "The backup of C:\\Users\\erics\\atlas-e2e-backup-test is still in progress; once it completes you'll find the copied folder there.",
        ] {
            assert!(claims_work_started(s), "missed claim: {s}");
        }
    }

    #[test]
    fn ordinary_talk_is_not_a_claim() {
        for s in [
            "Making a backup copies Atlas's own state.",
            "I'll need a destination folder.",
            "Where would you like it stored?",
            "You asked me to create a file.",
            "Is the backup still in progress?",
            "Backing up is a good habit.",
            "Saving files takes a moment on a slow disk.",
            "I'll back up the folder once you give me a destination.",
            "I'm starting to think you're right.",
            "It's done wonders for my mood.",
            "The capital of Australia is Canberra.",
            "Which folder should I put it in?",
            "Nice choice.",
            "Running is good exercise.",
            "That sounds like a plan.",
            // Second review, 21 Oct 2026: recipes, manner of speaking, versions.
            "Adding the eggs one at a time, beat until smooth.",
            "Removing the lid, let it simmer for ten minutes.",
            "I'll make this simple.",
            "We'll create the sauce first.",
            "I'm running version 2.0 of the app.",
        ] {
            assert!(!claims_work_started(s), "{s}");
        }
    }

    #[test]
    fn what_atlas_says_instead_is_not_itself_a_claim() {
        for s in [
            NOT_STARTED,
            NO_ACTING_MODEL,
            NO_SUCH_JOB,
            STOPPING,
            NO_CLIP_RECORDING,
            NOT_LOOKED,
        ] {
            for sentence in crate::repeating::sentences(s) {
                assert!(!claims_work_started(&sentence), "{sentence}");
            }
        }
    }

    #[test]
    fn the_free_online_reply_is_taken_back_with_the_reason() {
        let said = "It's been a while. We were on Practical ways to extend this Atlas assistant's capabilities. On it. Making atlas-test.txt on your Desktop with today's date, 20 October 2026. I'll take a look at it to confirm once it's there.";
        let out = without_unbacked_claims_for(
            said,
            false,
            "Create a text file named atlas-test.txt",
            false,
        );
        assert!(!out.contains("On it"), "{out}");
        assert!(!out.contains("Making atlas-test"), "{out}");
        assert!(!out.contains("take a look"), "{out}");
        assert!(out.contains("no language model that can act"), "{out}");
        assert!(out.contains("nothing was started"), "{out}");
        assert!(out.contains("Connections"), "{out}");
        // A model that can act: the plain line, no install advice.
        let acting = without_unbacked_claims_for(said, false, "make a file", true);
        assert!(acting.contains(NOT_STARTED), "{acting}");
        assert!(!acting.contains("Connections"), "{acting}");
        // Something really started: left as said.
        assert_eq!(
            without_unbacked_claims_for(said, true, "make a file", false),
            said
        );
    }

    #[test]
    fn a_status_question_about_no_job_says_there_is_none() {
        let guess = "Backing up C:\\Users\\erics\\atlas-e2e-backup-test is still in progress; once it completes you'll find the copied folder there.";
        for q in [
            "did the backup finish?",
            "Is the backup done yet",
            "how's the backup going",
        ] {
            assert!(asks_for_status(q), "{q}");
            let out = without_unbacked_claims_for(guess, false, q, false);
            assert!(out.starts_with(NO_SUCH_JOB), "{out}");
            assert!(out.contains("Outstanding"), "{out}");
            assert!(!out.contains("still in progress"), "{out}");
        }
        assert!(!asks_for_status("back up the folder"));
    }

    #[test]
    fn recording_a_clip_gets_the_reason_and_the_closest_real_thing() {
        assert!(asks_to_record_a_clip(
            "record 5 seconds from my microphone and tell me what you heard"
        ));
        assert!(asks_to_record_a_clip("can you record some audio"));
        assert!(!asks_to_record_a_clip("record everyone on the call"));
        assert!(!asks_to_record_a_clip("what's the record for the 100m"));
        assert!(NO_CLIP_RECORDING.contains("start dictating"));
        assert!(NO_CLIP_RECORDING.contains("nothing was recorded"));
    }

    #[test]
    fn a_missing_file_says_what_to_do_next() {
        let dir = std::env::temp_dir().join(format!("atlas-missing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["notes-2025.txt", "notes-2024.txt", "photo.png"] {
            std::fs::write(dir.join(n), "x").unwrap();
        }
        let missing = dir.join("notes-2026.txt");
        let said = cant_find(&missing.to_string_lossy());
        assert!(said.starts_with("I can't find"), "{said}");
        assert!(said.contains("closest names"), "{said}");
        assert!(said.contains("notes-2025.txt"), "{said}");
        assert!(said.contains("find the file notes-2026.txt"), "{said}");
        // A folder that isn't there either.
        let gone = dir.join("no-such-folder").join("a.txt");
        let said = cant_find(&gone.to_string_lossy());
        assert!(said.contains("doesn't exist either"), "{said}");
        assert!(said.contains("find the file a.txt"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
