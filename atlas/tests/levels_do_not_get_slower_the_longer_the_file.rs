//! Counting touches must not rescan the file for every level, every bar.
//!
//! ## The defect, which already had a documented twin
//!
//! `bars.rs` carries this note on its swing cache:
//!
//! > A walk-forward replay asks once per bar with a different `upto` every
//! > time, so it missed on essentially every call and rescanned the whole
//! > visible series each time: O(n) per bar, O(n²) over a run. Measured on a
//! > real twenty-year H1 file, that was about 27 minutes.
//!
//! `levels::count_touches` was the same shape and was left behind. `levels`
//! asked it once per confirmed pivot and once per round number, and it walked
//! the whole visible series every time — so one call was O(levels × bars),
//! and a walk-forward reader calls `levels::nearest` **twice per bar**.
//!
//! It was worse than the swing version in the term that matters, because the
//! number of pivots grows with the history too. At bar 100,000 of an H1 file
//! there are thousands of confirmed pivots, each counted over 100,000 bars,
//! twice, for that one row of the table.
//!
//! ## Two fixes, and they are separate claims
//!
//! **Correctness.** `levels` takes `span_pips` and applied it only to round
//! numbers, so its own first line — "every level worth naming *near current
//! price*" — was true of one of its two sources. A swing high four hundred
//! pips and two years away came back as a level. It now governs both.
//!
//! **Cost.** Touch counting moved to `AsOf::touches`, cached forward-only on
//! the bars, for the same reason the swing scan is: whether bar `i` begins a
//! new visit depends on bar `i` and the previous visit alone, and no later
//! bar revises it. That makes the cached answer *identical* to the rescan
//! rather than an approximation of it — which is the claim the first test
//! here exists to check, because a cache that is merely close would quietly
//! change every level's evidence.
//!
//! ## Four terms, not one
//!
//! Fixing the touch scan left the cost still growing per bar, and chasing the
//! remainder found three more of the same shape in the same call path:
//!
//! * `AsOf::gamma` — the level tolerance — re-summed every visible close on
//!   every call. It is a prefix mean, so it is now a running total.
//! * `AsOf::atr(n)` built a true range for **every** visible bar and then
//!   averaged the last `n`. Nothing outside the window was ever read; the
//!   values were computed and thrown away.
//! * `AsOf::swings` copied every pivot in the file into two fresh `Vec`s on
//!   every call. The swing *scan* had been fixed and the swing *copy* had
//!   not.
//!
//! Measured on this machine, release profile, `nearest` both ways per bar
//! over the whole file:
//!
//! | bars   | before    | after     |
//! |--------|-----------|-----------|
//! | 2,000  | 11.6 µs/bar | 11.8 µs/bar |
//! | 8,000  | 37.9 µs/bar | 13.6 µs/bar |
//! | 32,000 | 145.8 µs/bar | 13.9 µs/bar |
//!
//! Per-bar cost was rising with the file — which is the definition of the
//! quadratic — and is now flat. Extrapolated to a twenty-year H1 file the
//! swing note measures against, that is the difference between well over a
//! minute and about two seconds.

use atlas::market::bars::Bars;
use atlas::market::levels::{levels, nearest, TOUCH_SEPARATION};

/// The fold, written out independently.
///
/// Deliberately not a shared helper. A test that calls the same function the
/// code calls proves the function equals itself; this is the definition in
/// `TOUCH_SEPARATION`'s own words — *"bars of separation before a return
/// counts as a new touch"* — written fresh, so agreement means something.
fn touches_the_long_way(
    high: &[f64],
    low: &[f64],
    level: f64,
    tol: f64,
    separation: usize,
) -> (usize, Option<usize>) {
    let mut visits: Vec<usize> = Vec::new();
    for i in 0..high.len() {
        let inside = low[i] - tol <= level && level <= high[i] + tol;
        if !inside {
            continue;
        }
        let continues = visits.last().map(|prev| i - prev <= separation).unwrap_or(false);
        if !continues {
            visits.push(i);
        } else {
            *visits.last_mut().expect("just checked") = i;
        }
    }
    // `visits` now holds one entry per separate visit, updated to its last
    // bar -- so the count is its length and the last touch is its tail.
    let last = if visits.is_empty() { None } else { Some(*visits.last().unwrap()) };
    (visits.len(), last)
}

/// A series that wanders and revisits, so levels genuinely get touched more
/// than once. Deterministic, so a failure is reproducible.
fn wandering(n: usize) -> Bars {
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    let mut px = 1.1000f64;
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    for i in 0..n {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let step = ((seed >> 33) as f64 / (1u64 << 31) as f64 - 1.0) * 4e-4;
        px = (px + step).max(1.0500).min(1.1500);
        o.push(px);
        h.push(px + 6e-4);
        l.push(px - 6e-4);
        c.push(px);
        t.push(1_780_000_000_000 + (i as i64) * 3_600_000);
    }
    Bars::new(o, h, l, c, t).expect("bars")
}

#[test]
fn the_cached_count_is_the_same_number_as_the_rescan() {
    let bars = wandering(2_000);
    let view = bars.latest().expect("a view");
    let (h, l) = (view.high(), view.low());
    let tol = view.gamma();

    // Levels spread across the range, including ones price never reaches, so
    // the empty case is covered too.
    let mut checked = 0;
    for step in 0..40 {
        let level = 1.0400 + (step as f64) * 0.0030;
        let fast = view.touches(level, tol, TOUCH_SEPARATION, 0);
        let slow = touches_the_long_way(h, l, level, tol, TOUCH_SEPARATION);
        assert_eq!(
            fast, slow,
            "the cached count disagrees with the plain fold at {level:.5}: \
             {fast:?} against {slow:?}"
        );
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn asking_again_gives_the_same_answer_as_asking_once() {
    // The cache is mutable state behind a `&self`, so the second call must be
    // indistinguishable from the first. This is the shape of bug a
    // forward-only scan can have: an off-by-one in `from` that double-counts
    // the boundary bar.
    let bars = wandering(1_500);
    let view = bars.latest().expect("a view");
    let tol = view.gamma();
    let first = view.touches(1.1000, tol, TOUCH_SEPARATION, 0);
    for _ in 0..5 {
        assert_eq!(view.touches(1.1000, tol, TOUCH_SEPARATION, 0), first, "the count moved");
    }
}

#[test]
fn a_walk_forward_agrees_with_a_view_built_fresh_at_each_bar() {
    // The contamination path the swing cache had to be hunted for: a result
    // computed over more bars leaking into a view bounded at fewer. Here the
    // same series is walked forward -- warming the cache -- and then each
    // bar's answer is compared against a view of its own.
    let bars = wandering(600);
    let tol = bars.latest().expect("view").gamma();

    let mut walking = Vec::new();
    for i in 300..600 {
        let v = bars.as_of(i).expect("a bounded view");
        walking.push(v.touches(1.1000, tol, TOUCH_SEPARATION, 0));
    }

    // A second `Bars` over identical data, asked only about one bar each, so
    // nothing has been scanned past it.
    for (k, i) in (300..600).enumerate() {
        let fresh = wandering(600);
        let v = fresh.as_of(i).expect("a bounded view");
        if k % 37 != 0 {
            continue; // every 37th, because each of these builds a series
        }
        assert_eq!(
            walking[k],
            v.touches(1.1000, tol, TOUCH_SEPARATION, 0),
            "bar {i}: the walk-forward answer differs from a view that has seen \
             nothing later, which is the cache leaking the future backwards"
        );
    }
}

#[test]
fn a_view_narrower_than_a_scan_already_taken_further_is_still_right() {
    // `back_to` makes this a real case rather than a defensive one. A touch
    // count is a running total and not a list of indices, so unlike `swings`
    // there is no prefix to take -- it has to be recomputed over this view's
    // bars only, and getting that wrong hands back a count that includes
    // bars the view must not see.
    let bars = wandering(1_000);
    let tol = bars.latest().expect("view").gamma();

    // Warm the scan to the end.
    let wide = bars.latest().expect("view");
    let all = wide.touches(1.1000, tol, TOUCH_SEPARATION, 0);

    // Now ask a much narrower view.
    let narrow = bars.as_of(200).expect("a bounded view");
    let (n, last) = narrow.touches(1.1000, tol, TOUCH_SEPARATION, 0);
    let expected = touches_the_long_way(narrow.high(), narrow.low(), 1.1000, tol, TOUCH_SEPARATION);
    assert_eq!(
        (n, last),
        expected,
        "the narrow view got the wide view's count back"
    );
    assert!(
        last.map(|i| i <= 200).unwrap_or(true),
        "the last touch is at bar {last:?}, past the view's own end"
    );
    assert!(all.0 >= n, "the wider view somehow saw fewer visits");
}

#[test]
fn a_level_far_from_price_is_not_returned_as_a_nearby_level() {
    // The correctness half. `levels(view, k, span_pips)` says "near current
    // price"; `span_pips` governed the round numbers and not the pivots, so
    // every confirmed pivot in the file came back however far away it was.
    let bars = wandering(3_000);
    let view = bars.latest().expect("a view");
    let now = view.now();
    let pip = atlas::market::bars::pip_size(now);

    let span = 30.0;
    let out = levels(&view, 2, span).expect("levels");
    let reach = span * pip;
    for lv in &out {
        assert!(
            (lv.price - now).abs() <= reach + 1e-9,
            "{:.5} is {:.1} pips from {now:.5}, outside the {span} pips asked for",
            lv.price,
            (lv.price - now).abs() / pip
        );
    }
    assert!(!out.is_empty(), "the band filtered out everything, which proves nothing");
}

#[test]
fn a_wider_band_still_returns_more() {
    // So the filter cannot be satisfied by returning nothing.
    let bars = wandering(3_000);
    let view = bars.latest().expect("a view");
    let narrow = levels(&view, 2, 20.0).expect("levels");
    let wide = levels(&view, 2, 200.0).expect("levels");
    assert!(
        wide.len() > narrow.len(),
        "a ten-times-wider band returned {} levels against {}",
        wide.len(),
        narrow.len()
    );
}

#[test]
fn the_cost_does_not_grow_with_the_length_of_the_file() {
    // The defect, as the person experiences it. Not an absolute timing --
    // those belong to the machine, not to the code -- but a comparison of
    // the same work over two file lengths.
    //
    // Under the old shape, doubling the file more than quadrupled the work:
    // twice the bars to scan, and more pivots to scan them for. Under a
    // forward-only scan over a bounded band it is close to linear. The
    // threshold is deliberately loose -- 6x for 4x the bars -- because this
    // has to hold on a shared machine under load, and the failure it is
    // catching is a factor of twenty, not of two.
    fn walk(n: usize) -> std::time::Duration {
        let bars = wandering(n);
        let started = std::time::Instant::now();
        for i in (n / 2..n).step_by(7) {
            let v = bars.as_of(i).expect("a view");
            let _ = nearest(&v, true, 2);
            let _ = nearest(&v, false, 2);
        }
        started.elapsed()
    }

    // Warm anything one-off (allocator, first-touch pages) before measuring.
    let _ = walk(500);
    // The fastest of five, interleaved, for each length. One sample each was
    // what this test used to take, and on a two-core machine running the
    // whole suite a single preemption during the long walk read as 6.5x
    // (failed once, 23 Sep, in code nobody had touched). Interference only
    // ever adds time, so the minimum is the estimate it corrupts least; a
    // real return of the quadratic shape (~16x here) still fails.
    let (mut small, mut large) = (std::time::Duration::MAX, std::time::Duration::MAX);
    for _ in 0..5 {
        small = small.min(walk(2_000));
        large = large.min(walk(8_000));
    }

    let ratio = large.as_secs_f64() / small.as_secs_f64().max(1e-6);
    assert!(
        ratio < 6.0,
        "four times the bars cost {ratio:.1} times the work ({small:?} then {large:?}). \
         That is the quadratic shape again: on a twenty-year H1 file it is the \
         difference between seconds and half an hour"
    );
}

// ---------------------------------------------------------------------------
// The other two terms, each of which had to stay bar-for-bar identical.
// ---------------------------------------------------------------------------

/// The tolerance, the long way: mean absolute bar-to-bar close change over
/// everything visible. Written out rather than shared, for the same reason as
/// `touches_the_long_way`.
fn gamma_the_long_way(close: &[f64]) -> f64 {
    if close.len() < 2 {
        return f64::NAN; // the fallback is the code's business, not this fold's
    }
    let mut total = 0.0;
    for i in 1..close.len() {
        total += (close[i] - close[i - 1]).abs();
    }
    total / (close.len() - 1) as f64
}

/// True range averaged over the last `n`, built the way it used to be: every
/// bar computed, the tail averaged.
fn atr_the_long_way(high: &[f64], low: &[f64], close: &[f64], n: usize) -> f64 {
    let len = close.len();
    let mut trs = Vec::with_capacity(len);
    for i in 0..len {
        let prev = if i == 0 { close[0] } else { close[i - 1] };
        trs.push((high[i] - low[i]).max((high[i] - prev).abs()).max((low[i] - prev).abs()));
    }
    let take = if trs.len() < n { trs.len() } else { n };
    if take == 0 {
        return 0.0;
    }
    trs[trs.len() - take..].iter().sum::<f64>() / take as f64
}

#[test]
fn the_running_tolerance_is_the_same_number_as_the_full_sum() {
    // `gamma` is the width at which "at a level" is decided, so a cache that
    // was a shade off would move every level in every reading by a shade,
    // silently and everywhere.
    let bars = wandering(2_000);
    for i in [30usize, 200, 999, 1_000, 1_001, 1_999] {
        let v = bars.as_of(i).expect("a view");
        let mine = v.gamma();
        let theirs = gamma_the_long_way(v.close());
        assert!(
            (mine - theirs).abs() < 1e-15,
            "bar {i}: the running tolerance is {mine:.17} against {theirs:.17}"
        );
    }
}

#[test]
fn the_tolerance_is_right_however_the_views_are_asked_for() {
    // A running total extended forward has one failure mode a full re-sum
    // does not: being asked out of order. Backwards, then forwards, then the
    // middle again.
    let bars = wandering(1_200);
    let mut order: Vec<usize> = (40..1_200).step_by(53).collect();
    order.reverse();
    order.extend((40..1_200).step_by(97));
    order.push(600);
    for i in order {
        let v = bars.as_of(i).expect("a view");
        assert!(
            (v.gamma() - gamma_the_long_way(v.close())).abs() < 1e-15,
            "bar {i}: asking out of order changed the tolerance"
        );
    }
}

#[test]
fn the_shortened_atr_is_the_same_number_as_the_old_one() {
    // `atr` is what every distance in the feature table is measured in, so
    // "only look at the last n" has to be a saving and not a change. The
    // short-series case is included because that is where bar 0's missing
    // previous close still matters.
    let bars = wandering(400);
    for n in [2usize, 14, 60, 399, 400, 500] {
        for at in [5usize, 20, 100, 399] {
            let v = bars.as_of(at).expect("a view");
            let mine = v.atr(n);
            let theirs = atr_the_long_way(v.high(), v.low(), v.close(), n);
            assert!(
                (mine - theirs).abs() < 1e-15,
                "atr({n}) at bar {at}: {mine:.17} against {theirs:.17}"
            );
        }
    }
}

#[test]
fn the_bounded_pivot_query_is_the_tail_of_the_unbounded_one() {
    // `swings_since(k, 0)` must be `swings(k)`, and any other `since` must be
    // exactly its tail -- no dropped boundary pivot, no extra one.
    let bars = wandering(1_500);
    let v = bars.latest().expect("a view");
    let (all_hi, all_lo) = v.swings(2);

    for since in [0usize, 1, 500, 1_000, 1_499, 5_000] {
        let (hi, lo) = v.swings_since(2, since);
        let want_hi: Vec<usize> = all_hi.iter().copied().filter(|i| *i >= since).collect();
        let want_lo: Vec<usize> = all_lo.iter().copied().filter(|i| *i >= since).collect();
        assert_eq!(hi, want_hi, "swings_since(2, {since}) highs are not the tail");
        assert_eq!(lo, want_lo, "swings_since(2, {since}) lows are not the tail");
    }
}

#[test]
fn the_structure_window_start_moves_in_steps_and_never_backwards() {
    // The property the cache depends on. A start that moved every bar would
    // be a new cache key every bar, and the rescan-per-bar would be back --
    // see `WINDOW_STEP_BARS`.
    use atlas::market::levels::{window_start, STRUCTURE_LOOKBACK_BARS, WINDOW_STEP_BARS};

    let mut distinct = 0usize;
    let mut last = window_start(0);
    for visible in 0..6_000usize {
        let s = window_start(visible);
        assert!(s <= visible, "the window starts after the bars it covers");
        assert!(s >= last, "the window start went backwards at {visible} bars");
        if s != last {
            distinct += 1;
            last = s;
        }
        assert!(
            visible - s >= STRUCTURE_LOOKBACK_BARS || s == 0,
            "at {visible} bars the window is only {} long, under the {STRUCTURE_LOOKBACK_BARS} \
             it is supposed to cover",
            visible - s
        );
        assert!(
            visible - s <= STRUCTURE_LOOKBACK_BARS + WINDOW_STEP_BARS,
            "at {visible} bars the window is {} long, more than the lookback plus one step",
            visible - s
        );
    }
    // Once per step over five thousand bars past the lookback, not once per bar.
    assert!(
        distinct <= 6_000 / WINDOW_STEP_BARS + 1,
        "the window start moved {distinct} times, which is more often than the step"
    );
}
