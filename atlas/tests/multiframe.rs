use atlas::market::bars::Bars;
use atlas::market::multiframe::{agreement, read_all, read_frame, Agreement, FrameRead};
use atlas::market::structure::Trend;

const H1_MS: i64 = 3_600_000;
const H4_MS: i64 = 4 * H1_MS;

/// A zigzagging series that nonetheless trends, so the pivot detector has
/// actual highs and lows to find rather than a straight line with none.
/// Matches `structure.rs`'s own private `clean_uptrend` fixture in shape
/// (six-steps-up, three-steps-back, repeated) and spread (0.0006 -- a
/// narrower one puts adjacent highs and lows too close together for the
/// pivot detector to tell a real reversal from noise, confirmed directly
/// against `recent()` before trusting this fixture for real).
fn zigzag_trend(n_legs: usize, start: f64, up: bool, spacing_ms: i64) -> Bars {
    let sign = if up { 1.0 } else { -1.0 };
    let mut close = vec![start];
    let mut p = start;
    for _ in 0..n_legs {
        for _ in 0..6 {
            p += sign * 0.0015;
            close.push(p);
        }
        for _ in 0..3 {
            p -= sign * 0.0008;
            close.push(p);
        }
    }
    let spread = 0.0006;
    let n = close.len();
    let mut open = Vec::with_capacity(n);
    let mut high = Vec::with_capacity(n);
    let mut low = Vec::with_capacity(n);
    let mut time = Vec::with_capacity(n);
    let mut t: i64 = 1_700_000_000_000;
    for (i, &c) in close.iter().enumerate() {
        let o = if i == 0 { close[0] } else { close[i - 1] };
        open.push(o);
        high.push((c + spread).max(o).max(c));
        low.push((c - spread).min(o).min(c));
        time.push(t);
        t += spacing_ms;
    }
    Bars::new(open, high, low, close, time).unwrap()
}

#[test]
fn a_frame_read_names_its_own_inferred_timeframe() {
    let h1 = zigzag_trend(10, 1.0, true, H1_MS);
    let view = h1.latest().unwrap();
    let read = read_frame(&view, 2);
    assert_eq!(read.timeframe, "H1", "{:?}", read);
    assert_eq!(read.trend, Trend::Up, "{:?}", read);
}

#[test]
fn two_timeframes_that_both_trend_up_agree() {
    let h1 = zigzag_trend(10, 1.0, true, H1_MS);
    let h4 = zigzag_trend(10, 1.0, true, H4_MS);
    let v1 = h1.latest().unwrap();
    let v4 = h4.latest().unwrap();
    let reads = read_all(&[(&v1, 2), (&v4, 2)]);
    assert_eq!(reads.len(), 2);
    assert_eq!(agreement(&reads), Agreement::Agrees(Trend::Up), "{reads:?}");
}

#[test]
fn a_conflicting_higher_timeframe_is_reported_as_conflict_not_agreement() {
    let h1 = zigzag_trend(10, 1.0, true, H1_MS);
    let h4 = zigzag_trend(10, 1.0, false, H4_MS);
    let v1 = h1.latest().unwrap();
    let v4 = h4.latest().unwrap();
    let reads = read_all(&[(&v1, 2), (&v4, 2)]);
    assert_eq!(agreement(&reads), Agreement::Conflicts, "{reads:?}");
}

#[test]
fn nothing_directional_reports_inconclusive_not_silent_agreement() {
    // Not enough bars for either read to find real structure -- both
    // should come back Unknown, which must not be reported as agreement
    // just because the two Unknowns are technically equal to each other.
    let h1 = zigzag_trend(1, 1.0, true, H1_MS);
    let h4 = zigzag_trend(1, 1.0, true, H4_MS);
    let v1 = h1.latest().unwrap();
    let v4 = h4.latest().unwrap();
    let reads = read_all(&[(&v1, 2), (&v4, 2)]);
    assert_eq!(agreement(&reads), Agreement::Inconclusive, "{reads:?}");
}

#[test]
fn one_directional_and_one_range_read_is_mixed_not_full_agreement() {
    // The real bug this guards: H1 read DOWN and H4 read RANGE on the same
    // actual bars, and the first version of `agreement` reported "every
    // timeframe agrees: DOWN" -- true only of the reads that had an
    // opinion, and a real, live-caught overstatement.
    let reads = vec![
        FrameRead { timeframe: "H1".into(), trend: Trend::Down },
        FrameRead { timeframe: "H4".into(), trend: Trend::Range },
    ];
    assert_eq!(agreement(&reads), Agreement::Mixed(Trend::Down), "{reads:?}");
    let said = atlas::market::multiframe::spoken(&reads);
    assert!(!said.contains("Every timeframe checked agrees"), "{said}");
    assert!(said.contains("not the same as every timeframe agreeing"), "{said}");
}

#[test]
fn spoken_names_every_timeframe_and_the_overall_verdict() {
    let h1 = zigzag_trend(10, 1.0, true, H1_MS);
    let h4 = zigzag_trend(10, 1.0, true, H4_MS);
    let v1 = h1.latest().unwrap();
    let v4 = h4.latest().unwrap();
    let reads = read_all(&[(&v1, 2), (&v4, 2)]);
    let said = atlas::market::multiframe::spoken(&reads);
    assert!(said.contains("H1"), "{said}");
    assert!(said.contains("H4"), "{said}");
    assert!(said.contains("agrees"), "{said}");
}
