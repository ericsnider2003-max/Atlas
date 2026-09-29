//! A month the news calendar cannot answer for is not a quiet month.
//!
//! ## The gap
//!
//! `market::events::month` had a ceiling and no floor. Above 2027-12 it
//! refused, with the right reasoning written out in the source: *"a calendar
//! that keeps answering after its data runs out is worse than one that
//! stops."* Below 2026-01 it answered.
//!
//! What it answered with is the whole problem. Half the calendar is
//! algorithmic — payrolls is "the first Friday of the month, with four
//! exceptions", and that rule is as true of 2019 as of 2026. The other half
//! is hand-entered tables of central-bank decision dates, and those start in
//! January 2026. So March 2019 came back with payrolls and CPI in it and no
//! Fed meeting, no ECB, no BoE, no Canadian LFS.
//!
//! Nothing in that answer says it is half an answer. An empty list of Fed
//! meetings reads as "there were none", and a reader that trusts it treats a
//! Fed day as an ordinary Wednesday.

use atlas::market::events;

#[test]
fn a_month_before_the_tables_start_is_refused_rather_than_answered() {
    let e = events::month(2019, 3);
    assert!(
        e.is_err(),
        "March 2019 was answered. Everything in that answer is algorithmic -- \
         there is no Fed meeting in it, and nothing says so"
    );
}
