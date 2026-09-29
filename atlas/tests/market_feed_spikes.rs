//! The spike count, after it stopped being the cap.
//!
//! `find_spikes` has always kept at most five notes, which is right — nobody
//! wants ninety notes about the same fact. What it did not do was say how many
//! there were, so `Report::say()` read "5 Spike" on every file it was ever run
//! against, including a twenty-year H1 series with ninety of them.
//!
//! It matters rather than being untidy: a spike drags the ATR up for the next
//! fourteen bars, and `levels` sizes its risk off the ATR. Five spikes is
//! nothing; ninety is about 1% of the series measured against an inflated
//! yardstick.

use atlas::market::bars::Bars;
use atlas::market::feed::{accept, spikes_in, NoteKind};

/// A quiet series with `spikes` deliberate outsized bars in it.
fn with_spikes(n: usize, spikes: usize) -> Bars {
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    for i in 0..n {
        let base = 1.0 + i as f64 * 0.0001;
        let wide = i >= 100 && i < 100 + spikes;
        o.push(base);
        c.push(base + 0.0001);
        h.push(base + if wide { 0.05 } else { 0.0002 });
        l.push(base - 0.0001);
        t.push(1_700_000_000_000 + i as i64 * 3_600_000);
    }
    Bars::new(o, h, l, c, t).expect("a series")
}

#[test]
fn the_count_is_the_number_of_spikes_not_the_number_of_notes() {
    let r = accept(&with_spikes(400, 9), None).expect("accepted");
    assert_eq!(
        r.of_kind(NoteKind::Spike).len(),
        5,
        "the note cap is deliberate and should stay"
    );
    assert_eq!(spikes_in(&r), 9, "the count must not be the cap");
    assert!(r.say().contains("9 Spike"), "the summary still quotes the cap: {}", r.say());
}

#[test]
fn a_series_with_fewer_spikes_than_the_cap_still_counts_right() {
    let r = accept(&with_spikes(400, 2), None).expect("accepted");
    assert_eq!(spikes_in(&r), 2);
    assert!(r.say().contains("2 Spike"), "{}", r.say());
}

#[test]
fn a_clean_series_has_no_spikes_and_says_nothing_about_them() {
    let r = accept(&with_spikes(400, 0), None).expect("accepted");
    assert_eq!(spikes_in(&r), 0);
    assert!(!r.say().contains("Spike"), "{}", r.say());
}

#[test]
fn every_spike_note_carries_the_total_so_one_note_is_enough() {
    // Read off any note, not only the first: a caller that filters or sorts
    // them should not end up with a different answer.
    let r = accept(&with_spikes(400, 9), None).expect("accepted");
    for n in r.of_kind(NoteKind::Spike) {
        assert!(n.detail.contains("9 bars like this"), "{}", n.detail);
    }
}
