//! How long since *you* last used the keyboard or mouse, leaving out
//! Atlas's own typing.

/// Atlas's own keystrokes, on the clock Windows keeps for "last input": your
/// last input from before they started, and when they started and finished
/// (milliseconds since boot, wrapping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnInput {
    pub yours_before: u32,
    pub from: u32,
    pub to: u32,
}

/// How long Windows may take to stamp an injected key after it's sent.
const OWN_INPUT_SLACK_MS: u32 = 250;

/// Whether `tick` falls inside Atlas's own typing, allowing for the clock
/// wrapping every 49 days.
fn during(tick: u32, own: OwnInput) -> bool {
    tick.wrapping_sub(own.from) <= own.to.wrapping_add(OWN_INPUT_SLACK_MS).wrapping_sub(own.from)
}

/// Seconds since *you* last touched the keyboard or mouse.
///
/// Windows' last-input time counts Atlas's own keystrokes, so after Atlas
/// typed a reply it looked as if you had just typed — and its next reply
/// waited for a gap in "your" typing that was its own (found on 25 Sep 2026).
/// When the last input Windows saw is Atlas's, the answer is measured from
/// your last input before Atlas started instead.
pub fn idle_of_yours(last_input: u32, own: Option<OwnInput>, now: u32) -> u64 {
    let yours = match own {
        Some(o) if during(last_input, o) => o.yours_before,
        _ => last_input,
    };
    (now.wrapping_sub(yours) / 1000) as u64
}

/// Atlas is about to type: what to remember, given the last input Windows
/// saw and what Atlas remembered from its previous typing (so typing a
/// reply and then pressing Enter still measures from *your* last key).
pub fn own_input_starts(last_input: u32, before: Option<OwnInput>, now: u32) -> OwnInput {
    let yours_before = match before {
        Some(o) if during(last_input, o) => o.yours_before,
        _ => last_input,
    };
    OwnInput { yours_before, from: now, to: now }
}

