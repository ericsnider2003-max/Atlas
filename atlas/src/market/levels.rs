//! Support and resistance, built on what the evidence actually supports.
//!
//! ## Why this file is shaped the way it is
//!
//! Most of the retail support-and-resistance canon is unevidenced convention,
//! and some of it has been directly measured and found worthless. The research
//! is written up with citations in `TRADING_KNOWLEDGE.md`; the short version,
//! because it determines every design choice here:
//!
//! **Best evidenced — round numbers.** Osler (2003, *J. Finance*) examined
//! 9,667 real conditional orders from a major FX dealing bank. 8.7% sat at
//! rates ending in 00 against 1% expected by chance. Take-profit orders cluster
//! AT the figure (9.3% vs 4.4% for stops); stop-loss orders cluster **1–10 pips
//! BEYOND** it. That asymmetry has a measured consequence: price reverses at
//! round numbers 3.4pp more often than at arbitrary ones, and ACCELERATES after
//! crossing them. Confirmed independently on the EBS order book a decade later.
//!
//! So round numbers are the spine of this module, not a garnish — and they are
//! **bidirectional**: "at the figure" and "ten pips past the figure" are
//! opposite states, not one level with one behaviour.
//!
//! **Measured and found worthless — level "strength" ratings.** The six bank
//! desks in Osler (2000) published strength estimates alongside their levels.
//! Those estimates had no meaningful correlation with actual bounce frequency,
//! and the desks agreed with each other on level PLACEMENT only 30% of the
//! time. So [`Level`] has no strength field and this module refuses to compute
//! one. It scores on the three things with measured effects — touch count,
//! recency, round-number status — and nothing else.
//!
//! **No evidence at all — polarity.** Broken resistance becoming support. Every
//! practitioner source asserts it; not one quantifies it, and no academic test
//! exists. Implemented, and it says UNTESTED in its own verdict text so the
//! scoreboard settles it rather than this file assuming it.
//!
//! **The size of all of it.** The measured edges are 3–5 percentage points on a
//! ~56% base rate, before spread. Real, and economically marginal. Levels
//! belong here as context that earns its weight from its own track record, not
//! as a signal anything may trade on alone.
//!
//! ## The tolerance, and the null
//!
//! The hardest number in the subject is "how close is *at*". Practitioner
//! sources insist levels are zones and then give no width at all. The one
//! principled answer in the literature is the mean absolute bar-to-bar change,
//! chosen because a random walk then bounces off its own levels at close to
//! chance — which gives every measurement a built-in null. Measured here at
//! **56–58%** on synthetic walks, against Osler's **56.2%** for artificial
//! levels on real FX data. Two roads, same number. See [`null_bounce_rate`].

use super::bars::{pip_size, refuse, Answer, AsOf};

/// Bars of separation before a return counts as a new touch.
///
/// Without it a level price hugged for twenty bars reports twenty touches and
/// outranks one tested on three separate occasions, which is backwards.
pub const TOUCH_SEPARATION: usize = 3;

/// How far back a pivot may be and still count as structure.
///
/// **This is a stated bound, not a finding, and it is labelled as one.**
/// Nothing in Osler or anywhere else in `TRADING_KNOWLEDGE.md` gives a
/// horizon for how long a swing high stays a level, and this module's rule is
/// that unevidenced numbers are named as unevidenced rather than dressed up.
///
/// It is here because the alternative was not an evidence-based choice
/// either. `levels` used **all history**, which is a horizon too — an
/// unbounded one, chosen by nobody — and it had two costs:
///
/// * A swing high from four years ago came back as a level, and `nearest`
///   sorts purely by distance, so a stale one-touch pivot three pips away
///   outranked a five-touch round number ten pips away.
/// * The pivot count grows with the file, so the work per bar grew with the
///   file: a walk-forward reader calls `nearest` twice per bar, and on a
///   twenty-year H1 file that was the same quadratic shape `bars.rs`
///   measured at 27 minutes for the swing scan.
///
/// A thousand bars is about six weeks of H1 or four years of Daily, which on
/// both is "the structure people are still looking at". Bars, not days,
/// because `levels` is given a series and not a timeframe.
pub const STRUCTURE_LOOKBACK_BARS: usize = 1_000;

/// How often the window's start is allowed to move.
///
/// The window has to be re-anchored in steps rather than slid one bar at a
/// time, and the reason is the cache. `AsOf::touches` keys an entry on the
/// first bar counted; a start that moves every bar is a new key every bar, so
/// every level would be rescanned from scratch on every bar and the O(n²)
/// this replaces would be back with a smaller constant.
///
/// So the start sits on a 250-bar grid: the window is between 1,000 and 1,250
/// bars, and each level's running count is extended one bar at a time for 250
/// bars before being rebuilt. The wobble is well inside the precision
/// `STRUCTURE_LOOKBACK_BARS` itself has -- it is a stated bound, not a
/// measurement, and "about a thousand bars" is exactly as true of 1,250.
pub const WINDOW_STEP_BARS: usize = 250;

/// The first bar of the structure window, for a view of `visible` bars.
///
/// On the grid, so it is the same answer for 250 consecutive bars. A view
/// shorter than the lookback starts at nothing, which is the whole of it.
pub fn window_start(visible: usize) -> usize {
    let want = visible.saturating_sub(STRUCTURE_LOOKBACK_BARS);
    (want / WINDOW_STEP_BARS) * WINDOW_STEP_BARS
}

/// Osler (2003): stop orders cluster at rates ending 01–10 past the figure —
/// 14.4% in that band against 7.4% just below it. The only sweep-depth figure
/// in the subject with order data behind it.
pub const STOP_BAND_PIPS: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Support,
    Resistance,
    /// A round number. Either, depending where price is.
    Figure,
}

/// One price level, and only what has been measured about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    pub price: f64,
    pub kind: Kind,
    pub touches: usize,
    /// Bar of the most recent touch; `None` when never touched.
    pub last_touch: Option<usize>,
    pub on_figure: bool,
}

impl Level {
    fn is_support(&self) -> bool {
        matches!(self.kind, Kind::Support | Kind::Figure)
    }
    fn is_resistance(&self) -> bool {
        matches!(self.kind, Kind::Resistance | Kind::Figure)
    }
    pub fn say(&self) -> String {
        let k = match self.kind {
            Kind::Support => "support",
            Kind::Resistance => "resistance",
            Kind::Figure => "figure",
        };
        let fig = if self.on_figure { ", on the figure" } else { "" };
        format!("{:.5} {}{}, {} touch(es)", self.price, k, fig, self.touches)
    }
}

/// The 00 and 50 levels within `span_pips` of a price, nearest first.
///
/// **Nothing else.** Osler's order data found clustering at final digits 0 and
/// 5 and specifically **none at 25 or 75**. Quarter levels are a retail
/// convention the order book does not support, and generating them would fill
/// every list with lines nobody's orders are at.
pub fn figures_near(price: f64, span_pips: f64, pip: f64) -> Vec<f64> {
    let half = pip * 50.0;
    let span = span_pips * pip;
    let (lo, hi) = (price - span, price + span);
    let mut out = Vec::new();
    // `ceil`, not `floor`.
    //
    // `floor` starts at the multiple at or BELOW `lo`, so the first figure
    // emitted could sit up to a whole step -- fifty pips -- outside the span
    // asked for. Asked for the figures within thirty pips of 1.05000 it
    // returned 1.04500, fifty pips away, and this function's own first line
    // says "within `span_pips` of a price". Found by a test asserting that
    // `levels` returns nothing outside its band, which is the same claim one
    // layer up.
    let mut x = (lo / half).ceil() * half;
    while x <= hi + 1e-12 {
        if x > 0.0 {
            out.push((x / half).round() * half);
        }
        x += half;
    }
    // `total_cmp`, deliberately. All three sorts in this file used the
    // partial comparison and unwrapped it, which panics on a NaN and so
    // could end the whole process on a price series nothing had validated --
    // and `Bars::new` validated length but not finiteness until 17 Sep. Both
    // ends are fixed: the constructor now refuses a non-finite series, and
    // these comparisons are a total order regardless, so a series built some
    // other way sorts rather than crashes.
    //
    // Named in prose rather than spelled out, because
    // `tests/it_knows_it_crashed.rs` searches this file for the old call and
    // a comment explaining the fix would trip it. The test right after that
    // one warns about exactly this: "a guard tripping over its own
    // explanation is the same fault as a guard passing because of one, and
    // this tree has now had both." Three, now.
    out.sort_by(|a, b| (a - price).abs().total_cmp(&(b - price).abs()));
    out
}

fn is_figure(price: f64, pip: f64) -> bool {
    let half = pip * 50.0;
    (price / half - (price / half).round()).abs() < 1e-6
}

/// Every level worth naming near current price.
///
/// Sources in the order the evidence supports, not the order tradition uses:
/// round numbers first (direct order-flow evidence, two datasets a decade
/// apart, an identified mechanism), then confirmed swing pivots.
///
/// Prior-day and prior-week extremes are the obvious third source and are NOT
/// built, because they need a decision about where the FX day ends — 17:00 New
/// York by convention, not midnight UTC — and a module that picked the wrong
/// one would disagree with every broker chart while looking right. That is a
/// real gap, named rather than papered over with a guess.
pub fn levels(view: &AsOf<'_>, k: usize, span_pips: f64) -> Answer<Vec<Level>> {
    view.need(25)?;
    let (h, l) = (view.high(), view.low());
    let now = view.now();
    let tol = view.gamma();
    let pip = pip_size(now);

    // `span_pips` applied to pivots as well as to round numbers.
    //
    // It always governed `figures_near` and never the swings, so this
    // function's own first line -- "every level worth naming NEAR CURRENT
    // PRICE" -- was true of one of its two sources. Every confirmed pivot in
    // the whole history came back, including a swing high four hundred pips
    // and two years away, which is not a level anybody is trading against
    // and is not what the caller asked for. `nearest` then filtered most of
    // them out by distance after paying for all of them.
    //
    // It is also the term that made the cost grow: the pivot count grows
    // with the history, so a walk-forward run paid more per bar the further
    // it got. Inside the band the count is bounded by the band, not by the
    // file.
    //
    // Same band as the figures, because it is the same question. A level is
    // near price or it is not, and having two answers to that in one
    // function is how the two sources came to disagree.
    let reach = span_pips * pip;
    let near = |p: f64| (p - now).abs() <= reach;

    // Only the pivots inside the structure window, and only the touches
    // inside it either. See `STRUCTURE_LOOKBACK_BARS`, which says plainly
    // that the number is a stated bound rather than a measured one -- and
    // that "all history", the thing it replaces, was a bound too, chosen by
    // nobody.
    //
    // Counting touches over the window rather than over everything is a
    // correctness point as well as a cost one: the old count depended on how
    // long the file happened to be, so the same level in a ten-year file and
    // a one-year file reported different evidence for itself.
    let since = window_start(view.len());
    let (hi, lo) = view.swings_since(k, since);
    let mut out: Vec<Level> = Vec::new();

    for &i in &hi {
        let p = h[i];
        if !near(p) {
            continue;
        }
        let (n, last) = view.touches(p, tol, TOUCH_SEPARATION, since);
        out.push(Level {
            price: p,
            kind: if p > now { Kind::Resistance } else { Kind::Support },
            touches: n,
            last_touch: last,
            on_figure: is_figure(p, pip),
        });
    }
    for &i in &lo {
        let p = l[i];
        if !near(p) {
            continue;
        }
        let (n, last) = view.touches(p, tol, TOUCH_SEPARATION, since);
        out.push(Level {
            price: p,
            kind: if p < now { Kind::Support } else { Kind::Resistance },
            touches: n,
            last_touch: last,
            on_figure: is_figure(p, pip),
        });
    }
    for p in figures_near(now, span_pips, pip) {
        let (n, last) = view.touches(p, tol, TOUCH_SEPARATION, since);
        out.push(Level {
            price: p,
            kind: Kind::Figure,
            touches: n,
            last_touch: last,
            on_figure: true,
        });
    }
    Ok(merge(out, tol))
}

/// Fold levels within one tolerance of each other into one.
///
/// Where two merge, the one on a figure keeps its price: the round number is
/// the only price in the pair with order-flow evidence behind it, and averaging
/// it a fraction away moves the level off the orders.
fn merge(mut found: Vec<Level>, tol: f64) -> Vec<Level> {
    if found.is_empty() {
        return found;
    }
    found.sort_by(|a, b| a.price.total_cmp(&b.price));
    let mut out: Vec<Level> = Vec::new();
    let mut group: Vec<Level> = vec![found[0]];
    for lv in found.into_iter().skip(1) {
        if lv.price - group[group.len() - 1].price <= tol {
            group.push(lv);
        } else {
            out.push(fold(&group));
            group = vec![lv];
        }
    }
    out.push(fold(&group));
    out
}

fn fold(group: &[Level]) -> Level {
    let fig = group.iter().find(|g| g.on_figure);
    let anchor = fig.unwrap_or(&group[0]);
    let all_same = group.iter().all(|g| g.kind == group[0].kind);
    Level {
        price: anchor.price,
        kind: if fig.is_some() || !all_same { Kind::Figure } else { group[0].kind },
        touches: group.iter().map(|g| g.touches).max().unwrap_or(0),
        last_touch: group.iter().filter_map(|g| g.last_touch).max(),
        on_figure: fig.is_some(),
    }
}

/// The level closest to current price that helps a side.
///
/// A support below price helps a long; a resistance above helps a short.
pub fn nearest(view: &AsOf<'_>, long: bool, k: usize) -> Answer<Option<Level>> {
    let all = levels(view, k, 120.0)?;
    let now = view.now();
    let mut candidates: Vec<Level> = all
        .into_iter()
        .filter(|x| {
            if long {
                x.price <= now + 1e-12 && x.is_support()
            } else {
                x.price >= now - 1e-12 && x.is_resistance()
            }
        })
        .collect();
    candidates.sort_by(|a, b| {
        (a.price - now).abs().total_cmp(&(b.price - now).abs())
    });
    Ok(candidates.into_iter().next())
}

/// (bounced, broke) for every completed visit to a level's band.
///
/// A bounce is an ENTRY and an EXIT, not a snapshot: price enters the band from
/// one side and later leaves it, and it bounced if it left the side it came in
/// on. Getting this definition right mattered more than anything else in the
/// file — two earlier definitions both scored 90% on random data, because they
/// measured the definition rather than the market.
fn visits(closes: &[f64], level: f64, half: f64) -> (usize, usize) {
    let (mut same, mut diff) = (0usize, 0usize);
    let mut state: Option<bool> = None; // Some(true) = above, Some(false) = below
    let mut entered: Option<bool> = None;
    let mut inside = false;
    for &x in closes {
        let above = x > level + half;
        let below = x < level - half;
        if !above && !below {
            if !inside {
                if let Some(s) = state {
                    entered = Some(s);
                }
            }
            inside = true;
        } else {
            let side = above;
            if inside {
                if let Some(e) = entered {
                    if e == side {
                        same += 1;
                    } else {
                        diff += 1;
                    }
                }
                entered = None;
            }
            inside = false;
            state = Some(side);
        }
    }
    (same, diff)
}

/// What bounce rate ARBITRARY levels get on these same bars.
///
/// Osler tested published levels against artificially generated ones rather
/// than against nothing, and that is the only reason her 60.8% means anything —
/// arbitrary levels scored 56.2% on identical data. **A bounce rate reported
/// without its null is a measurement of the tolerance.**
pub fn null_bounce_rate(view: &AsOf<'_>, trials: usize, seed: u64) -> Answer<f64> {
    view.need(25)?;
    let c = view.close();
    let g = view.gamma();
    let (lo, hi) = c.iter().fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let (mut same, mut total) = (0usize, 0usize);
    for _ in 0..trials {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let u = (((s >> 32) as u32) as f64) / (u32::MAX as f64);
        let p = lo + (hi - lo) * u;
        let (a, b) = visits(c, p, g);
        same += a;
        total += a + b;
    }
    if total == 0 {
        return refuse("no completed visits to measure a null against");
    }
    Ok(same as f64 / total as f64)
}

/// (rate, visits) for one real level, on the same definition as the null.
///
/// Always read against [`null_bounce_rate`] on the same bars. A rate quoted on
/// its own is uninterpretable.
pub fn bounce_rate(view: &AsOf<'_>, level: f64) -> (f64, usize) {
    let (s, d) = visits(view.close(), level, view.gamma());
    let n = s + d;
    (if n == 0 { 0.0 } else { s as f64 / n as f64 }, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::fixtures::{from_path, walk, walk_with};

    #[test]
    fn figures_are_the_hundreds_and_the_fifties() {
        // Both families, which is what this test is named for and is here to
        // show: the 00 above and the 50 below, from one call.
        let f = figures_near(1.1023, 60.0, 0.0001);
        let r: Vec<f64> = f.iter().map(|x| (x * 10000.0).round() / 10000.0).collect();
        assert!(r.contains(&1.1000), "{:?}", r);
        assert!(r.contains(&1.1050), "{:?}", r);

        // It used to assert `1.0950` as well, and that assertion was pinning a
        // defect. 1.0950 is **73 pips** from 1.1023 and the span asked for is
        // 60 -- it came back only because the loop started at the multiple at
        // or below the bottom of the band, so the first figure emitted could
        // be a whole fifty-pip step outside it. This function's own first line
        // says "within `span_pips` of a price", and `levels` now has a test
        // one layer up asserting that no level it returns is outside the band
        // it was given; the two could not both hold.
        assert!(
            !r.contains(&1.0950),
            "a figure 73 pips away came back from a 60-pip span: {r:?}"
        );
        for x in &r {
            assert!(
                (x - 1.1023).abs() <= 60.0 * 0.0001 + 1e-9,
                "{x:.5} is outside the span asked for: {r:?}"
            );
        }
    }

    #[test]
    fn quarter_levels_are_not_generated() {
        // Osler's order data found clustering at digits 0 and 5 and
        // specifically NONE at 25 or 75. Generating them would fill the list
        // with lines nobody's orders are at.
        let f = figures_near(1.1023, 120.0, 0.0001);
        let r: Vec<f64> = f.iter().map(|x| (x * 10000.0).round() / 10000.0).collect();
        assert!(!r.contains(&1.1025), "{:?}", r);
        assert!(!r.contains(&1.0975));
    }

    #[test]
    fn a_jpy_cross_gets_figures_a_hundred_times_wider() {
        let f = figures_near(151.23, 80.0, 0.01);
        let r: Vec<f64> = f.iter().map(|x| (x * 100.0).round() / 100.0).collect();
        assert!(r.contains(&151.00) && r.contains(&151.50), "{:?}", r);
    }

    #[test]
    fn a_price_visited_more_than_once_becomes_a_level() {
        let b = from_path(&[1.1000, 1.1100, 1.1000, 1.1100, 1.1000, 1.1080]).unwrap();
        let lv = levels(&b.latest().unwrap(), 2, 120.0).unwrap();
        assert!(!lv.is_empty());
        let near: Vec<&Level> = lv.iter().filter(|x| (x.price - 1.1100).abs() < 0.0015).collect();
        assert!(!near.is_empty(), "{:?}", lv.iter().map(|x| x.say()).collect::<Vec<_>>());
        assert!(near.iter().map(|x| x.touches).max().unwrap() >= 2);
    }

    #[test]
    fn a_level_hugged_for_twenty_bars_was_touched_once() {
        // Through `AsOf::touches`, which is now the only implementation of
        // the fold. It was `count_touches` here, a second copy of the same
        // rule that `levels` no longer called -- and a rule with two homes is
        // a rule with two definitions waiting to happen. See `Cache::touched`
        // in `bars.rs`.
        let px = vec![1.1000f64; 20];
        let bars = super::super::bars::Bars::new(
            px.clone(),
            vec![1.10005; 20],
            vec![1.09995; 20],
            px,
            Vec::new(),
        )
        .expect("bars");
        let view = bars.latest().expect("a view");
        let (n, _) = view.touches(1.1000, 1e-4, TOUCH_SEPARATION, 0);
        assert_eq!(n, 1, "counted {n} touches for one continuous visit");
    }

    #[test]
    fn nearby_levels_merge_into_one() {
        let raw = vec![
            Level { price: 1.10000, kind: Kind::Support, touches: 2, last_touch: Some(5), on_figure: false },
            Level { price: 1.10002, kind: Kind::Support, touches: 1, last_touch: Some(9), on_figure: false },
            Level { price: 1.10500, kind: Kind::Resistance, touches: 3, last_touch: Some(7), on_figure: false },
        ];
        let out = merge(raw, 0.0001);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].touches, 2, "the merged level keeps the best evidence");
    }

    #[test]
    fn where_a_pivot_and_a_figure_merge_the_figure_keeps_the_price() {
        // The round number is the only price in the pair with order-flow
        // evidence behind it. Averaging it away moves the level off the orders.
        let raw = vec![
            Level { price: 1.09994, kind: Kind::Support, touches: 4, last_touch: Some(5), on_figure: false },
            Level { price: 1.10000, kind: Kind::Figure, touches: 1, last_touch: Some(9), on_figure: true },
        ];
        let out = merge(raw, 0.0002);
        assert_eq!(out.len(), 1);
        assert!((out[0].price - 1.10000).abs() < 1e-9, "{}", out[0].say());
        assert!(out[0].on_figure && out[0].touches == 4);
    }

    #[test]
    fn a_long_is_only_offered_support_and_a_short_only_resistance() {
        let b = from_path(&[1.1000, 1.1100, 1.1000, 1.1100, 1.1050]).unwrap();
        let v = b.latest().unwrap();
        let now = v.now();
        if let Some(up) = nearest(&v, false, 2).unwrap() {
            assert!(up.price >= now - 1e-9, "{}", up.say());
        }
        if let Some(dn) = nearest(&v, true, 2).unwrap() {
            assert!(dn.price <= now + 1e-9, "{}", dn.say());
        }
    }

    #[test]
    fn arbitrary_levels_bounce_about_half_the_time() {
        // The whole point of the tolerance. If a random level on a random walk
        // bounced 90% of the time, every measurement here would be measuring
        // the tolerance rather than the market -- which is exactly what two
        // earlier definitions of "bounce" did.
        let b = walk_with(600, 7, 0.0008);
        let r = null_bounce_rate(&b.latest().unwrap(), 300, 3).unwrap();
        assert!(
            (0.42..=0.68).contains(&r),
            "arbitrary levels bounced {:.0}% -- the tolerance is doing the work",
            r * 100.0
        );
    }

    #[test]
    fn the_null_does_not_move_with_volatility() {
        // If it did, gamma would not be self-scaling and every threshold in
        // this file would need per-pair tuning.
        let a = null_bounce_rate(&walk_with(600, 9, 0.0004).latest().unwrap(), 200, 4).unwrap();
        let b = null_bounce_rate(&walk_with(600, 9, 0.0020).latest().unwrap(), 200, 4).unwrap();
        assert!((a - b).abs() < 0.10, "{} vs {}", a, b);
    }

    #[test]
    fn a_real_level_is_measurable_on_the_same_definition() {
        let b = from_path(&[1.1000, 1.1100, 1.1000, 1.1100, 1.1000, 1.1080]).unwrap();
        let (rate, visits) = bounce_rate(&b.latest().unwrap(), 1.1100);
        assert!(visits >= 1, "a level price kept returning to must record visits");
        assert!((0.0..=1.0).contains(&rate));
    }

    #[test]
    fn too_little_history_is_refused_rather_than_guessed_at() {
        let b = super::super::bars::Bars::from_closes(&[1.1, 1.1001, 1.1002], 0.0006).unwrap();
        assert!(levels(&b.latest().unwrap(), 2, 120.0).is_err());
    }

    #[test]
    fn levels_never_include_a_price_from_after_the_view() {
        // The type makes this structural, and the test states it anyway
        // because it is the property everything else rests on.
        let b = walk(400, 11);
        let early = b.as_of(150).unwrap();
        let seen_max = early.high().iter().cloned().fold(f64::MIN, f64::max);
        let seen_min = early.low().iter().cloned().fold(f64::MAX, f64::min);
        for lv in levels(&early, 2, 120.0).unwrap() {
            if !lv.on_figure {
                assert!(lv.price <= seen_max + 1e-9 && lv.price >= seen_min - 1e-9);
            }
        }
    }
}
