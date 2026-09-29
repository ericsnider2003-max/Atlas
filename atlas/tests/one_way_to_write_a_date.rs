//! `say` and `say_date` write the same date, because there is one of them.
//!
//! `say_date` was on the orphan list — public, called by nothing, tested by
//! nothing — with an entry describing it as "a date said the way a person
//! says it", for a spoken side that "does not discuss dates". It returns
//! `2026-09-17`: the fixed form, on the market side, which is what the entry
//! gave as the reason it wasn't needed. Nobody had opened it.
//!
//! What was actually there: `Timestamp::say` carried the same
//! `{:04}-{:02}-{:02}` inline, so one fact — how this program writes a date —
//! lived in two format strings, and only one of them had a caller. `say` is
//! built on `say_date` now.
//!
//! This test lives in `tests/` rather than a `#[cfg(test)]` block beside the
//! code, and that is not a style choice: `dead_capabilities.rs` reads `tests/`
//! to decide what is tested, so a unit test inside `src` leaves a function
//! counted as untested. Thirty-six files have inline tests and are miscounted
//! this way — named in the handover as its own finding rather than quietly
//! worked around here.

use atlas::market::time::Utc;

#[test]
fn a_timestamp_and_its_date_agree_on_the_format() {
    let t = Utc::at(2026, 9, 17, 14, 5);
    assert_eq!(t.say_date(), "2026-09-17");
    // The relationship, not the literal: changing how this program writes
    // dates should stay a one-line change that cannot drift apart.
    assert!(
        t.say().starts_with(&t.say_date()),
        "`say` no longer begins with `say_date`: {} vs {}",
        t.say(),
        t.say_date()
    );
    assert_eq!(t.say(), "2026-09-17 14:05Z");
}
