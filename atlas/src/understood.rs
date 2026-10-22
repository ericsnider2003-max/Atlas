//! Acting on a guess.
//!
//! Atlas arrives at what you meant two different ways. A phrase in its own
//! list is matched outright — it cannot have misread that. Anything else is
//! handed to a small local model, which returns an intent in exactly the same
//! confident shape whether it recognised your sentence or invented a reading
//! of it.
//!
//! The policy gate grades *what Atlas is about to do*. Nothing has ever told
//! it *how sure Atlas is that you asked for it*. So a dictation matched from
//! the phrase list and a dictation the model guessed out of a mumble are
//! graded identically, and both simply happen. `brain.rs` asks the model to
//! "use ask rather than guessing" — but that is an instruction to the very
//! model whose confident wrongness `certainty.rs` exists to catch, and
//! nothing checks whether it obeyed.
//!
//! This is the missing half, and it is deliberately the smallest thing that
//! closes the gap: an inferred reading of something that *changes the world*
//! is graded one rung stricter, so Atlas says what it took you to mean and
//! waits for an answer instead of acting in hope. Being told what Atlas
//! assumed costs you a sentence. Undoing what it did on a guess costs
//! whatever it did.
//!
//! Three properties, on purpose:
//!
//! - **It only ever escalates.** Like [`crate::voiceid::handle`], this
//!   function can return a stricter grade and never a looser one, so the
//!   worst case is a question you did not need to be asked.
//! - **Reading is left alone.** An inferred `AutoProceed` stays automatic.
//!   Ordinary conversation reaches the model constantly, and asking "did you
//!   mean that?" before answering a question would make Atlas unusable while
//!   protecting nothing — nothing was going to change.
//! - **Already-asking stays as it is.** `AskClarification` and
//!   `RequireApproval` are the two rungs that already stop and ask. There is
//!   nothing above them to escalate to.

use crate::policy::Decision;
use serde::Deserialize;

/// How Atlas came to believe this is what you asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Understanding {
    /// Matched against Atlas's own phrase list. Deterministic: the same words
    /// always give the same intent, and no model was consulted.
    Matched,
    /// A small local model's reading of a sentence the phrase list did not
    /// know. It may be right. Nothing has checked.
    Inferred,
}

impl Understanding {
    /// Read from what the brain reported about how it got there.
    ///
    /// `Reached::No` — the model was asked and did not answer — counts as
    /// matched rather than inferred, because in that case no model reading
    /// exists to doubt: the reply was produced without one.
    pub fn from_reached(r: crate::brain::Reached) -> Self {
        match r {
            crate::brain::Reached::Yes => Understanding::Inferred,
            _ => Understanding::Matched,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UnderstoodConfig {
    /// On by default, unlike most of `tools.yaml`.
    ///
    /// The usual default is off, so that a missing config means Atlas does
    /// less rather than more. This one inverts that for the same reason:
    /// switching it off does not remove a behaviour, it removes a question —
    /// and what is left is Atlas acting on the model's guess without saying
    /// so. "Less" here means fewer things done unasked.
    pub enabled: bool,
}

impl Default for UnderstoodConfig {
    fn default() -> Self {
        UnderstoodConfig { enabled: true }
    }
}

/// Grade an action by how Atlas came to understand the request.
///
/// `base` is the grade the action already earned on its own merits — pass
/// the intrinsic [`crate::policy::classify`] rather than the
/// history-relaxed `classify_with_policy`, for the same reason `voiceid`
/// does: a standing grant records that *you* approved something like this
/// before, and what is in doubt here is whether you asked for it at all.
pub fn grade(base: Decision, u: Understanding, cfg: &UnderstoodConfig) -> Decision {
    if !cfg.enabled || u == Understanding::Matched {
        return base;
    }
    match base {
        // Reading, listing, answering. Nothing changes, so a wrong reading
        // costs you one irrelevant answer and no more.
        Decision::AutoProceed => Decision::AutoProceed,
        // The one that matters. `ProceedAndReport` is "act, then say what was
        // done" — the world has already changed by the time you hear about
        // it. `Intent::Dictate` is graded here, and its own note in
        // `policy.rs` names the exact failure this prevents: "a misheard
        // sentence landing somewhere public".
        Decision::ProceedAndReport => Decision::AskClarification,
        // Already stops and asks.
        other => other,
    }
}

/// What Atlas says when it stops to check a reading.
///
/// It states the assumption rather than pleading ignorance, then asks one
/// concrete yes/no (21 Oct 2026). "I didn't catch that" throws away work Atlas
/// actually did and makes you start over; naming the reading lets you correct
/// one word, and the closing question tells you what a plain "yes" will do.
pub fn checking(described: &str) -> String {
    let d = described.trim().trim_end_matches('.');
    format!("I took that as {d}. Shall I go ahead?")
}

// ---- Context before asking (Eric, 21 Oct 2026) ----------------------------
//
// "Atlas should ask and think with context. Atlas shouldn't ask and be
// guessing." Before a question is put to you, the request is read against what
// Atlas already knows: what was just said, the window you have open, what you
// did lately and what you usually pick. Clear -> act and say what was
// understood. Ambiguous -> ask one question that offers the candidates.
// Nothing at all -> say exactly what is missing, with an example. Never a bare
// "Which one?". Anything hard to undo is still confirmed by the policy gate;
// this only decides what to say and whether a slot can be filled.

/// What Atlas can read a request against. Built by
/// `Daemon::resolution_context`; plain data so it can be tested.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// What was said lately, oldest first.
    pub recent_said: Vec<String>,
    /// The focused window's title. For apps, the app is the part after the
    /// last " - " (Windows titles read "Document - Notepad").
    pub active_window: Option<String>,
    /// (kind, value, when): kinds are "folder", "app", "person", "file".
    pub recent_targets: Vec<(String, String, u64)>,
    /// (kind, value, times) for things picked again and again.
    pub habits: Vec<(String, String, u32)>,
}

/// What reading a request against the context came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// One answer, and why. Act on it and say so (see [`stated`]).
    Clear { value: String, because: String },
    /// Two or three live candidates, each with its reason, in one question.
    Choose {
        question: String,
        options: Vec<String>,
    },
    /// Nothing to go on. The question names what is missing, with an example.
    Missing { question: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    Mentioned,
    Open,
    Recent(bool),
    Habit,
}

impl Why {
    fn because(self) -> &'static str {
        match self {
            Why::Mentioned => "you mentioned it",
            Why::Open => "the one you have open",
            Why::Recent(_) => "the one you used last time",
            Why::Habit => "the one you usually pick",
        }
    }
    fn tag(self) -> &'static str {
        match self {
            Why::Mentioned => "you mentioned it",
            Why::Open => "open now",
            Why::Recent(true) => "last time",
            Why::Recent(false) => "used recently",
            Why::Habit => "you usually pick it",
        }
    }
}

fn last_part(v: &str) -> &str {
    v.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(v)
}

fn mentions(said: &str, value: &str) -> bool {
    let (s, v) = (said.to_lowercase(), value.to_lowercase());
    if v.len() >= 2 && s.contains(&v) {
        return true;
    }
    let tail = last_part(&v);
    tail.len() >= 3 && s.contains(tail)
}

fn example_for(kind: &str) -> &'static str {
    match kind {
        "folder" => "D:\\Backups",
        "app" => "Notepad",
        "person" => "Sam",
        "file" => "report.docx",
        _ => "its full name",
    }
}

/// Read a request for `kind` against the context.
///
/// Candidates, in order: the window you have open (if it is that kind), what
/// you did lately (newest first), what you usually pick (most first). Anything
/// the conversation names comes to the front.
pub(crate) fn resolve(kind: &str, what_for: &str, ctx: &Context) -> Resolution {
    let mut found: Vec<(String, Why)> = Vec::new();
    let known: Vec<&str> = ctx
        .recent_targets
        .iter()
        .filter(|t| t.0 == kind)
        .map(|t| t.1.as_str())
        .chain(
            ctx.habits
                .iter()
                .filter(|h| h.0 == kind)
                .map(|h| h.1.as_str()),
        )
        .collect();

    if let Some(w) = ctx
        .active_window
        .as_deref()
        .map(str::trim)
        .filter(|w| !w.is_empty())
    {
        let wl = w.to_lowercase();
        if kind == "app" {
            let app = w.rsplit(" - ").next().unwrap_or(w).trim();
            if !app.is_empty() {
                found.push((app.to_string(), Why::Open));
            }
        } else if let Some(v) = known.iter().find(|v| {
            let t = last_part(v).to_lowercase();
            t.len() >= 3 && wl.contains(&t)
        }) {
            found.push((v.to_string(), Why::Open));
        } else if kind == "folder" && (w.contains(":\\") || w.starts_with('/')) {
            found.push((w.to_string(), Why::Open));
        }
    }
    let mut recents: Vec<&(String, String, u64)> =
        ctx.recent_targets.iter().filter(|t| t.0 == kind).collect();
    recents.sort_by(|a, b| b.2.cmp(&a.2));
    for (i, t) in recents.iter().enumerate() {
        found.push((t.1.clone(), Why::Recent(i == 0)));
    }
    let mut habits: Vec<&(String, String, u32)> =
        ctx.habits.iter().filter(|h| h.0 == kind).collect();
    habits.sort_by(|a, b| b.2.cmp(&a.2));
    for h in habits {
        found.push((h.1.clone(), Why::Habit));
    }

    // Whatever was just said comes first.
    for f in found.iter_mut() {
        if ctx.recent_said.iter().any(|s| mentions(s, &f.0)) {
            f.1 = Why::Mentioned;
        }
    }
    found.sort_by_key(|f| f.1 != Why::Mentioned);
    let mut seen: Vec<String> = Vec::new();
    found.retain(|f| {
        let k = f.0.to_lowercase();
        let new = !seen.contains(&k);
        if new {
            seen.push(k);
        }
        new
    });

    let said_it: Vec<&(String, Why)> = found.iter().filter(|f| f.1 == Why::Mentioned).collect();
    let pick = if said_it.len() == 1 {
        Some(said_it[0])
    } else if said_it.is_empty() && found.len() == 1 {
        Some(&found[0])
    } else {
        None
    };
    if let Some((value, why)) = pick {
        return Resolution::Clear {
            value: value.clone(),
            because: why.because().to_string(),
        };
    }
    if found.is_empty() {
        return Resolution::Missing {
            question: format!(
                "I need {what_for}, and nothing I know of fits. Say it, for example {}.",
                example_for(kind)
            ),
        };
    }
    let options: Vec<String> = found
        .iter()
        .take(3)
        .map(|(v, w)| format!("{v} ({})", w.tag()))
        .collect();
    Resolution::Choose {
        question: ask_with_options(what_for, &options),
        options,
    }
}

/// The one line of "what I understood", for a Clear reading: "Backing up to
/// D:\Backups -- the one you used last time." `None` for the others, which ask.
pub(crate) fn stated(action: &str, r: &Resolution) -> Option<String> {
    match r {
        Resolution::Clear { because, .. } => Some(format!(
            "{} \u{2014} {because}.",
            action.trim().trim_end_matches('.')
        )),
        _ => None,
    }
}

/// A question that offers the candidates. Never a bare "Which one?": with no
/// options it says what is missing.
pub(crate) fn ask_with_options(what_for: &str, options: &[String]) -> String {
    match options {
        [] => format!(
            "I need {what_for}, and I have nothing to go on. Name it in full and I'll use it."
        ),
        [one] => format!("Did you mean {one} for {what_for}? Say yes, or name another."),
        [rest @ .., last] => format!(
            "For {what_for}: {} or {last}? Say which, or name another.",
            rest.join(", ")
        ),
    }
}

/// Folder paths, files, apps and people an utterance or journal line clearly
/// names, as `(kind, value)`. Best effort and deliberately conservative: a
/// wrong target here would later be offered as a candidate.
pub(crate) fn targets_in(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    for (i, raw) in words.iter().enumerate() {
        let w = raw.trim_matches(|c: char| "\"'`,;()[]".contains(c));
        let w = w.trim_end_matches(['.', '?', '!']);
        let b = w.as_bytes();
        let drive = b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
        if drive || (w.starts_with('/') && w.len() > 3 && w[1..].contains('/')) {
            let tail = last_part(w);
            let ext = tail.rsplit_once('.').map(|e| e.1).unwrap_or("");
            if (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                out.push(("file".into(), w.to_string()));
            } else {
                out.push(("folder".into(), w.to_string()));
            }
            continue;
        }
        let lw = w.to_lowercase();
        let next = words
            .get(i + 1)
            .map(|n| n.trim_matches(|c: char| !c.is_alphanumeric()).to_string());
        let Some(n) = next.filter(|n| n.len() >= 2) else {
            continue;
        };
        match lw.as_str() {
            "open" | "opened" | "opening" | "launch" | "launched" | "close" | "closed"
            | "closing" | "focus" | "focused" => {
                if !matches!(
                    n.to_lowercase().as_str(),
                    "the" | "my" | "it" | "that" | "this" | "a"
                ) {
                    out.push(("app".into(), n));
                }
            }
            "message" | "messaged" | "email" | "emailed" | "text" | "texted" | "call"
            | "called"
                if n.chars().next().is_some_and(|c| c.is_uppercase()) =>
            {
                out.push(("person".into(), n));
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matched_phrase_is_graded_exactly_as_before() {
        for base in [
            Decision::AutoProceed,
            Decision::ProceedAndReport,
            Decision::AskClarification,
            Decision::RequireApproval,
        ] {
            assert_eq!(
                grade(base, Understanding::Matched, &UnderstoodConfig::default()),
                base
            );
        }
    }

    #[test]
    fn an_inferred_change_to_the_world_becomes_a_question() {
        assert_eq!(
            grade(
                Decision::ProceedAndReport,
                Understanding::Inferred,
                &UnderstoodConfig::default()
            ),
            Decision::AskClarification
        );
    }

    #[test]
    fn an_inferred_reading_is_still_just_answered() {
        // The noise guard. Ordinary conversation is inferred almost every
        // time; if this escalated, Atlas would ask before every answer.
        assert_eq!(
            grade(
                Decision::AutoProceed,
                Understanding::Inferred,
                &UnderstoodConfig::default()
            ),
            Decision::AutoProceed
        );
    }

    #[test]
    fn it_never_relaxes_anything() {
        for base in [
            Decision::AutoProceed,
            Decision::ProceedAndReport,
            Decision::AskClarification,
            Decision::RequireApproval,
        ] {
            for u in [Understanding::Matched, Understanding::Inferred] {
                assert!(
                    grade(base, u, &UnderstoodConfig::default()).severity() >= base.severity(),
                    "{base:?}/{u:?} came back looser"
                );
            }
        }
    }

    #[test]
    fn switched_off_it_changes_nothing() {
        let off = UnderstoodConfig { enabled: false };
        assert_eq!(
            grade(Decision::ProceedAndReport, Understanding::Inferred, &off),
            Decision::ProceedAndReport
        );
    }

    #[test]
    fn the_model_being_unreachable_is_not_a_guess() {
        // There is no model reading to doubt when the model never answered.
        assert_eq!(
            Understanding::from_reached(crate::brain::Reached::No),
            Understanding::Matched
        );
        assert_eq!(
            Understanding::from_reached(crate::brain::Reached::NotNeeded),
            Understanding::Matched
        );
        assert_eq!(
            Understanding::from_reached(crate::brain::Reached::Yes),
            Understanding::Inferred
        );
    }

    #[test]
    fn what_it_says_names_the_reading_rather_than_the_failure() {
        let s = checking("dictating into the window you are looking at");
        assert!(s.contains("dictating into the window"), "{s}");
        assert!(!s.to_lowercase().contains("didn't catch"), "{s}");
    }

    #[test]
    fn checking_states_the_reading_and_asks_a_yes_or_no() {
        let s = checking("posting a note to the team.");
        assert!(
            s.starts_with("I took that as posting a note to the team."),
            "{s}"
        );
        assert!(s.ends_with("Shall I go ahead?"), "{s}");
    }

    #[test]
    fn one_candidate_from_the_conversation_is_clear() {
        let mut c = Context::default();
        c.recent_said = vec!["put it in Backups please".into()];
        c.recent_targets = vec![
            ("folder".into(), "D:\\Backups".into(), 5),
            ("folder".into(), "C:\\Projects".into(), 9),
        ];
        let r = resolve("folder", "where to back up to", &c);
        assert_eq!(
            r,
            Resolution::Clear {
                value: "D:\\Backups".into(),
                because: "you mentioned it".into()
            }
        );
        let line = stated("Backing up to D:\\Backups", &r).unwrap();
        assert_eq!(line, "Backing up to D:\\Backups \u{2014} you mentioned it.");
    }

    #[test]
    fn one_candidate_from_the_active_window_is_clear() {
        let mut c = Context::default();
        c.active_window = Some("Draft - Notepad".into());
        let r = resolve("app", "which app to close", &c);
        assert_eq!(
            r,
            Resolution::Clear {
                value: "Notepad".into(),
                because: "the one you have open".into()
            }
        );
        // A folder window is matched to a known folder by its name.
        let mut c = Context::default();
        c.active_window = Some("Projects".into());
        c.recent_targets = vec![("folder".into(), "C:\\Work\\Projects".into(), 1)];
        c.habits = vec![("folder".into(), "D:\\Backups".into(), 4)];
        match resolve("folder", "where to back up to", &c) {
            Resolution::Choose { options, .. } => {
                assert!(options[0].contains("Projects") && options[0].contains("open now"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_only_recent_one_is_clear_as_last_time() {
        let mut c = Context::default();
        c.recent_targets = vec![("folder".into(), "D:\\Backups".into(), 3)];
        match resolve("folder", "where to back up to", &c) {
            Resolution::Clear { because, .. } => {
                assert_eq!(because, "the one you used last time")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_habit_alone_is_clear_as_what_you_usually_pick() {
        let mut c = Context::default();
        c.habits = vec![("person".into(), "Sam".into(), 7)];
        match resolve("person", "who to message", &c) {
            Resolution::Clear { value, because } => {
                assert_eq!(value, "Sam");
                assert_eq!(because, "the one you usually pick");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn two_candidates_are_one_question_naming_both() {
        let mut c = Context::default();
        c.recent_targets = vec![
            ("folder".into(), "D:\\Backups".into(), 3),
            ("folder".into(), "C:\\Projects".into(), 9),
        ];
        match resolve("folder", "where to back up to", &c) {
            Resolution::Choose { question, options } => {
                assert_eq!(options.len(), 2);
                assert!(
                    question.contains("C:\\Projects") && question.contains("D:\\Backups"),
                    "{question}"
                );
                assert!(question.contains("last time"), "{question}");
                assert!(question.contains("where to back up to"), "{question}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn at_most_three_are_offered_and_repeats_collapse() {
        let mut c = Context::default();
        c.recent_targets = (0..5)
            .map(|i| ("file".to_string(), format!("C:\\f{i}.txt"), i))
            .collect();
        c.habits = vec![("file".into(), "c:\\F4.TXT".into(), 9)];
        match resolve("file", "which file to open", &c) {
            Resolution::Choose { options, .. } => assert_eq!(options.len(), 3),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nothing_known_names_the_slot_with_an_example() {
        let r = resolve("folder", "where the backup should go", &Context::default());
        match r {
            Resolution::Missing { question } => {
                assert!(
                    question.contains("where the backup should go"),
                    "{question}"
                );
                assert!(question.contains("D:\\Backups"), "{question}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            stated("x", &resolve("app", "an app", &Context::default())),
            None
        );
    }

    #[test]
    fn no_question_is_ever_a_bare_which_one() {
        for opts in [
            vec![],
            vec!["a".to_string()],
            vec!["a".to_string(), "b".to_string()],
        ] {
            let q = ask_with_options("the thing to open", &opts);
            let l = q.to_lowercase();
            assert!(q.contains("the thing to open"), "{q}");
            assert!(l.split_whitespace().count() > 3, "{q}");
            assert!(!l.contains("didn't catch"), "{q}");
        }
    }

    #[test]
    fn targets_are_read_only_where_a_line_clearly_names_one() {
        let t = targets_in("Backed up to D:\\Backups, then opened Notepad and messaged Sam.");
        assert!(
            t.contains(&("folder".into(), "D:\\Backups".into())),
            "{t:?}"
        );
        assert!(t.contains(&("app".into(), "Notepad".into())), "{t:?}");
        assert!(t.contains(&("person".into(), "Sam".into())), "{t:?}");
        let t = targets_in("read C:\\docs\\report.docx");
        assert_eq!(
            t,
            vec![("file".to_string(), "C:\\docs\\report.docx".to_string())]
        );
        assert!(targets_in("open the door and call it a day").is_empty());
    }
}
