//! Where the self-audit signals come from.
//!
//! `selfaudit` knew how to read signals and recommend from them. Nothing built
//! any. `Daemon.signals` was declared, passed to `recommend()`, and never
//! pushed to — so asking Atlas what it should fix about itself returned
//! nothing, every time, not because nothing was wrong but because the vector
//! was empty.
//!
//! That is a different failure from an unwired module. `selfaudit` *is*
//! reachable; `tests/wiring.rs` is satisfied and always would be. A wired
//! consumer with no producer looks identical to a working one from every
//! angle except the answer it gives.
//!
//! So this derives signals from records the daemon already keeps. Nothing new
//! is measured here — that is deliberate. A producer that needs new
//! instrumentation is a producer that ships later, and an empty audit shipping
//! now is what caused the problem.

use crate::selfaudit::{Kind, Signal};
use crate::undo::History;

/// An action taken back within this many seconds was a wrong answer, not a
/// change of mind.
///
/// Chosen long enough to cover reading what happened and saying so, short
/// enough that deciding later doesn't count. Deliberately not tuneable: a
/// threshold you can widen is one that gets widened until nothing crosses it.
pub const REGRET_WINDOW_SECS: u64 = 120;

/// Things Atlas did that you immediately took back.
///
/// This is the outcome signal the system has been missing. `record_approval`
/// captures consent at the moment of asking, which is not evidence the action
/// was right — you approve at 1am and find out at 9. An undo inside two
/// minutes is the cheapest honest evidence available, and it is already being
/// written down for other reasons.
pub fn from_undo(history: &History) -> Vec<Signal> {
    let mut by_area: std::collections::BTreeMap<String, (u32, u32, String)> = Default::default();

    for did in &history.done {
        // Only things Atlas chose. Undoing something you asked for is you
        // changing your mind, which says nothing about Atlas.
        if did.you_asked {
            continue;
        }
        let e = by_area
            .entry(did.area.clone())
            .or_insert((0, 0, String::new()));
        e.1 += 1;
        if did.undone {
            e.0 += 1;
            e.2 = did.what.clone();
        }
    }

    by_area
        .into_iter()
        .filter(|(_, (undone, _, _))| *undone > 0)
        .map(|(area, (undone, total, example))| Signal {
            kind: Kind::YouKeepCorrecting,
            subject: area,
            seen: undone,
            of: total,
            example,
        })
        .collect()
}

/// Intents that keep landing on `Unknown`.
///
/// Takes counts rather than reading a log, so the caller decides what a
/// "chance" was. Otherwise this module would need its own opinion about what
/// counts as an utterance, and there would be two.
pub fn from_misunderstandings(
    unknown_count: u32,
    total_utterances: u32,
    last_example: &str,
) -> Option<Signal> {
    if unknown_count == 0 || total_utterances == 0 {
        return None;
    }
    Some(Signal {
        kind: Kind::NotUnderstood,
        subject: "what you said".into(),
        seen: unknown_count,
        of: total_utterances,
        example: last_example.to_string(),
    })
}

/// Capabilities that claim to work and have never once been used.
///
/// Not a fault on its own — you may simply not need it. It becomes one when
/// the list is long, because a system advertising forty things you never touch
/// is a system whose inventory you have stopped reading.
/// What the "never used" signal is about (`used`). Its "Have a go" shows
/// how to ask for each one rather than starting work on Atlas's code.
pub const UNUSED: &str = "what I can do that you haven't asked for yet";

pub fn from_unused(never_used: &[String], total_capabilities: u32) -> Option<Signal> {
    if never_used.is_empty() || total_capabilities == 0 {
        return None;
    }
    Some(Signal {
        kind: Kind::NeverUsed,
        subject: UNUSED.into(),
        seen: never_used.len() as u32,
        of: total_capabilities,
        example: never_used.join(", "),
    })
}

/// Everything derivable right now, in one call.
///
/// `GotSlower` is absent on purpose and it is the gap worth naming: nothing in
/// Atlas times a turn, so that signal cannot be produced by anything. It stays
/// in `selfaudit::Kind` because it is the right taxonomy, not because it works.
pub fn gather(
    history: &History,
    unknown_count: u32,
    total_utterances: u32,
    last_unknown: &str,
    never_used: &[String],
    total_capabilities: u32,
) -> Vec<Signal> {
    let mut out = from_undo(history);
    out.extend(from_misunderstandings(
        unknown_count,
        total_utterances,
        last_unknown,
    ));
    out.extend(from_unused(never_used, total_capabilities));
    out
}
