//! The swing scan, proved identical after it stopped being quadratic.
//!
//! `AsOf::swings` used to rescan every visible bar on every call, with a
//! 32-entry cache that was cleared wholesale when it overflowed. A
//! walk-forward replay asks once per bar with a different bound each time, so
//! it missed on essentially every call: O(n) per bar, O(n²) over a run. On a
//! real twenty-year H1 file that measured at about 27 minutes.
//!
//! The fix rests on one fact: **a pivot is decided once and never revised.**
//! Index `i` is a swing high exactly when its `±k` neighbours say so, and all
//! of those bars exist the moment `i + k` does. No later bar can change it.
//!
//! That is a claim about behaviour, so it is tested as one: the incremental
//! answer must equal a brute-force rescan at every bound, including bounds
//! visited out of order and bounds moved backwards.

use atlas::market::bars::Bars;
use atlas::market::fixtures;

/// The old algorithm, kept here as the thing to agree with.
fn brute(bars: &Bars, upto: usize, k: usize) -> (Vec<usize>, Vec<usize>) {
    let v = bars.as_of(upto).expect("a view");
    let (h, l) = (v.high(), v.low());
    let n = h.len();
    let (mut hi, mut lo) = (Vec::new(), Vec::new());
    if n > 2 * k {
        for i in k..n - k {
            let w = &h[i - k..=i + k];
            if h[i] >= *w.iter().fold(&f64::MIN, |a, b| if b > a { b } else { a })
                && w.iter().filter(|&&x| x == h[i]).count() == 1
            {
                hi.push(i);
            }
            let w = &l[i - k..=i + k];
            if l[i] <= *w.iter().fold(&f64::MAX, |a, b| if b < a { b } else { a })
                && w.iter().filter(|&&x| x == l[i]).count() == 1
            {
                lo.push(i);
            }
        }
    }
    (hi, lo)
}

#[test]
fn walking_forward_gives_the_same_answer_as_a_full_rescan() {
    let bars = fixtures::walk(400, 20_260_914);
    for k in [1usize, 2, 3, 5] {
        for upto in 0..400 {
            let got = bars.as_of(upto).expect("a view").swings(k);
            assert_eq!(got, brute(&bars, upto, k), "k={k}, upto={upto}");
        }
    }
}

#[test]
fn a_bound_moved_backwards_still_gives_that_bounds_answer() {
    // `AsOf::back_to` exists, so a caller really can ask for an earlier bound
    // after the scan has run past it. The incremental list has to be trimmed
    // to what that bound could have known, not handed out whole.
    let bars = fixtures::walk(300, 7);
    let k = 2;
    let _ = bars.as_of(299).expect("a view").swings(k); // run the scan to the end
    for upto in [250usize, 120, 61, 30, 5] {
        let got = bars.as_of(upto).expect("a view").swings(k);
        assert_eq!(got, brute(&bars, upto, k), "after a full scan, upto={upto}");
    }
}

#[test]
fn bounds_asked_out_of_order_do_not_contaminate_each_other() {
    let bars = fixtures::walk(300, 99);
    let k = 3;
    for upto in [200usize, 40, 275, 41, 12, 299, 13] {
        let got = bars.as_of(upto).expect("a view").swings(k);
        assert_eq!(got, brute(&bars, upto, k), "out of order, upto={upto}");
    }
}

#[test]
fn a_series_too_short_to_hold_a_pivot_has_none() {
    let bars = fixtures::walk(30, 3);
    for k in [5usize, 10, 20] {
        let (hi, lo) = bars.as_of(9).expect("a view").swings(k);
        assert!(hi.is_empty() && lo.is_empty(), "k={k}");
    }
}

#[test]
fn different_sensitivities_do_not_share_a_list() {
    // One growing list per `k`. Mixing them would be silent and wrong.
    let bars = fixtures::walk(300, 11);
    let a = bars.as_of(280).expect("a view").swings(2);
    let b = bars.as_of(280).expect("a view").swings(6);
    assert_ne!(a, b, "two sensitivities produced identical pivot sets");
    assert_eq!(a, brute(&bars, 280, 2));
    assert_eq!(b, brute(&bars, 280, 6));
}
