//! A bar knows its open and its close.
//!
//! **Ruled 17 September 2026.**
//!
//! ## Why it needed ruling on
//!
//! The code and the documentation stated opposite conventions for the same
//! number:
//!
//! * `market/bars.rs`, on the accessor every reader went through:
//!   *"`now_time()` — When this bar closed, if the series is timestamped."*
//! * The data itself: broker bars are **open-stamped**. An H4 bar stamped
//!   18:00 has exactly the open, high, low and close of the H1 bars at 18:00,
//!   19:00, 20:00 and 21:00, so it does not finish until 22:00.
//!
//! So the stamp is an open, and the accessor's doc was wrong.
//!
//! The cost was real: `standdown::spanning` fed the stamp to
//! `Tf::bar_window(close_ms)`, which returned `(close - span, close)` — **the
//! previous bar's window**. The news blackout was therefore a full bar early
//! in both directions. The bar containing a payrolls print read clean; the bar
//! after it, hours past the release, was refused.
//!
//! Nothing caught it because `tests/standdown.rs` built its fixtures
//! close-stamped as well, so fixture and code agreed and only reality
//! disagreed. That is this tree's signature failure mode — green for a reason
//! that has nothing to do with being right — and a doc comment is where it
//! hid.
//!
//! ## What this file is for
//!
//! The answer to a convention nobody wrote down is not a better comment, it is
//! a function that returns it. `opened_at()` and `closes_at()` cannot be
//! misread the way one ambiguous `now_time()` could. These tests pin that,
//! pin the relationship between the two, and pin the one thing a comment
//! cannot: that no module goes back to guessing.

use atlas::market::bars::Bars;
use atlas::market::timeframe::Tf;

const H1: i64 = 60 * 60 * 1000;
const H4: i64 = 4 * H1;

/// `n` bars, `step` apart, the last one opening at `last_open`.
fn series(last_open: i64, step: i64, n: usize) -> Bars {
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    for i in 0..n {
        let px = 1.1000 + (i % 7) as f64 * 1e-4;
        o.push(px);
        h.push(px + 5e-4);
        l.push(px - 5e-4);
        c.push(px);
        t.push(last_open - ((n - 1 - i) as i64) * step);
    }
    Bars::new(o, h, l, c, t).expect("bars")
}

#[test]
fn the_stamp_is_the_open() {
    let last_open = 1_780_675_200_000;
    let bars = series(last_open, H4, 40);
    let view = bars.latest().unwrap();
    assert_eq!(
        view.opened_at(),
        Some(last_open),
        "the stamp is not being reported as the open"
    );
}

#[test]
fn the_close_is_one_bar_after_the_open() {
    for (step, name) in [(H1, "H1"), (H4, "H4"), (15 * 60 * 1000, "M15")] {
        let last_open = 1_780_675_200_000;
        let bars = series(last_open, step, 40);
        let view = bars.latest().unwrap();
        assert_eq!(view.bar_span_ms(), Some(step), "{name}: the bar length is wrong");
        assert_eq!(
            view.closes_at(),
            Some(last_open + step),
            "{name}: the close is not one bar after the open"
        );
    }
}

#[test]
fn a_weekend_gap_does_not_change_how_long_a_bar_is() {
    // The reason `bar_span_ms` is a median and not a mean or the last gap.
    // FX shuts about 22:00 Friday and reopens about 22:00 Sunday, so a real
    // H1 series carries a 65-hour gap every week. An average over that is not
    // an hour, and a series whose bar length reads as several hours would put
    // every close in the wrong place.
    let last_open = 1_780_675_200_000;
    let mut t: Vec<i64> = (0..40).map(|i| last_open - (39 - i) * H1).collect();
    // Push a weekend into the middle.
    for x in t.iter_mut().skip(20) {
        *x += 65 * H1;
    }
    let n = t.len();
    let px: Vec<f64> = (0..n).map(|i| 1.1 + (i % 7) as f64 * 1e-4).collect();
    let bars = Bars::new(
        px.clone(),
        px.iter().map(|p| p + 5e-4).collect(),
        px.iter().map(|p| p - 5e-4).collect(),
        px.clone(),
        t.clone(),
    )
    .expect("bars");
    let view = bars.latest().unwrap();
    assert_eq!(
        view.bar_span_ms(),
        Some(H1),
        "a weekend gap moved the bar length, so every close would be wrong"
    );
}

#[test]
fn a_series_with_no_clock_does_not_invent_one() {
    let px: Vec<f64> = (0..40).map(|i| 1.1 + (i % 7) as f64 * 1e-4).collect();
    let bars = Bars::new(
        px.clone(),
        px.iter().map(|p| p + 5e-4).collect(),
        px.iter().map(|p| p - 5e-4).collect(),
        px.clone(),
        Vec::new(),
    )
    .expect("bars");
    let view = bars.latest().unwrap();
    assert_eq!(view.opened_at(), None);
    assert_eq!(view.bar_span_ms(), None);
    assert_eq!(
        view.closes_at(),
        None,
        "with no clock the close must be unknown, not equal to the open"
    );
}

#[test]
fn one_bar_has_no_knowable_length_so_no_knowable_close() {
    // Honest rather than convenient: one stamp says when it opened and
    // nothing about how long it lasts.
    let bars = series(1_780_675_200_000, H4, 1);
    let view = bars.latest().unwrap();
    assert_eq!(view.opened_at(), Some(1_780_675_200_000));
    assert_eq!(view.bar_span_ms(), None);
    assert_eq!(view.closes_at(), None);
}

#[test]
fn the_timeframe_window_agrees_with_the_bar_itself() {
    // Two ways of asking the same question, which is how they came to
    // disagree. `Tf::bar_window` takes an open and derives the close from the
    // timeframe; `AsOf` takes the open from the stamp and derives the close
    // from the spacing. They must land on the same instant.
    let last_open = 1_780_675_200_000;
    let bars = series(last_open, H4, 40);
    let view = bars.latest().unwrap();

    let (from_tf, to_tf) = Tf::H4.bar_window(view.opened_at().unwrap());
    assert_eq!(from_tf, view.opened_at().unwrap(), "the opens disagree");
    assert_eq!(to_tf, view.closes_at().unwrap(), "the closes disagree");
}

#[test]
fn nothing_in_the_tree_still_asks_the_ambiguous_question() {
    // The structural half, and the point of the rename. `now_time()` could be
    // read as either end, and was read as both — by two modules, in two
    // different files, for weeks. Deleting it is what makes that impossible;
    // this fails if it comes back.
    //
    // The mentions allowed are the comments that explain the history, such as
    // the doc comment on `opened_at`.
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir("src").expect("src") {
        let mut stack = vec![entry.expect("entry").path()];
        while let Some(p) = stack.pop() {
            if p.is_dir() {
                for e in std::fs::read_dir(&p).expect("dir").flatten() {
                    stack.push(e.path());
                }
                continue;
            }
            if p.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            for (i, line) in text.lines().enumerate() {
                let t = line.trim_start();
                let is_comment = t.starts_with("//");
                if line.contains("now_time") && !is_comment {
                    offenders.push(format!("{}:{}  {}", p.display(), i + 1, t));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "`now_time` is back, and it is the accessor whose meaning two modules \
         disagreed about. Ask for `opened_at()` or `closes_at()`:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn the_convention_is_written_down_where_a_reader_will_find_it() {
    // A guard against the other half of how this happened: the data was
    // open-stamped and the *documentation* in `bars.rs` said otherwise, and the
    // documentation is what the next person reads.
    let bars_rs = std::fs::read_to_string("src/market/bars.rs").expect("bars.rs");
    let at = bars_rs.find("pub fn opened_at").expect("opened_at is gone");
    let doc_before = &bars_rs[..at];
    assert!(
        doc_before.contains("open-stamped"),
        "the accessor no longer says which end of the bar its answer is, which is \
         the exact gap that let two modules disagree"
    );
    assert!(
        bars_rs.contains("When the bar being decided at OPENED")
            || bars_rs.contains("OPENED"),
        "the doc comment stopped being explicit about the open"
    );
}
