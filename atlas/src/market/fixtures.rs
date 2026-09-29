//! Deterministic synthetic series.
//!
//! Not test scaffolding, and not `#[cfg(test)]`. Atlas holds no real bars of
//! its own, so every measurement this module makes about itself — the
//! random-walk baseline the efficiency ratio is read against, the rate at which
//! arbitrary levels bounce, how often noise is mislabelled a trend — is made
//! against a synthetic series. Those are calibration facts the system relies on
//! at runtime, so the generator that produces them belongs in the build.
//!
//! Everything here is seeded and integer-based, so it reproduces bit for bit on
//! every machine. A calibration number that drifts between the developer's box
//! and the machine it runs on is not a calibration number.
//!
//! ## The bug this file remembers
//!
//! The first generator shifted `>> 33`, which yields a 31-bit value; divided by
//! `u32::MAX` it never exceeds 0.5, so subtracting 0.5 made **every step
//! negative**. It generated a monotone downtrend and called it a random walk.
//!
//! Nothing failed. A fixture that is not what it claims does not fail — it
//! passes for the wrong reason, and every test built on it quietly becomes
//! decoration. [`walk`] is checked by its own tests for the two properties it
//! is supposed to have: steps that go both ways, and pivots that actually form.

use super::bars::{Answer, Bars};

/// The spread a fixture needs for swings to form at all.
///
/// Not arbitrary. High and low hang off the bar's own close, so if the spread
/// is small relative to the bar-to-bar step the previous close dominates, the
/// highs of adjacent bars tie, the strict-fractal uniqueness test fails and
/// **no swing forms anywhere in the series**. Large enough, and structure
/// appears as it should. See `Bars::from_closes`.
pub const FIXTURE_SPREAD: f64 = 0.0006;

/// A seeded pseudo-random walk around 1.1000.
pub fn walk(n: usize, seed: u64) -> Bars {
    walk_with(n, seed, 0.0016)
}

/// A seeded walk with a stated step scale.
pub fn walk_with(n: usize, seed: u64, scale: f64) -> Bars {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut p = 1.1000;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // `>> 32`, then narrowed to u32. `>> 33` gives 31 bits and makes every
        // step negative -- see the module note.
        let u = (((s >> 32) as u32) as f64) / (u32::MAX as f64) - 0.5;
        p += u * scale;
        out.push(p);
    }
    Bars::from_closes(&out, FIXTURE_SPREAD).unwrap()
}

/// A straight ramp. No structure in it at all, which is the point: it is the
/// control for anything that claims to detect a trend.
pub fn ramp(n: usize, step: f64) -> Bars {
    let closes: Vec<f64> = (0..n).map(|i| 1.1 + i as f64 * step).collect();
    Bars::from_closes(&closes, FIXTURE_SPREAD).unwrap()
}

/// A zigzag that makes higher highs and higher lows.
///
/// A straight ramp has no swings at all, so it verifies almost nothing — which
/// is correct behaviour and makes it a useless fixture for anything structural.
pub fn zigzag(legs: usize, up: usize, back: usize, step: f64) -> Bars {
    let mut c = vec![1.0];
    let mut p = 1.0;
    for _ in 0..legs {
        for _ in 0..up {
            p += step;
            c.push(p);
        }
        for _ in 0..back {
            p -= step * 0.55;
            c.push(p);
        }
    }
    Bars::from_closes(&c, FIXTURE_SPREAD).unwrap()
}

/// An explicit oscillation between two prices.
///
/// A random walk is NOT a flat fixture: on a twenty-bar window one reads as a
/// trend often enough that using it to test "flat" would make the test measure
/// the seed.
pub fn box_range(n: usize, lo: f64, hi: f64, period: usize) -> Bars {
    let closes: Vec<f64> = (0..n)
        .map(|i| {
            let phase = (i % period) as f64 / period as f64;
            // A triangle wave. No trig, so no rounding difference between
            // platforms.
            let t = if phase < 0.5 { phase * 2.0 } else { 2.0 - phase * 2.0 };
            lo + (hi - lo) * t
        })
        .collect();
    Bars::from_closes(&closes, 0.0002).unwrap()
}

/// Walk between turning points so real swings form.
///
/// Fixtures are written as the turning points a person would draw and the walk
/// fills in the bars between. A fixture written as raw closes is unreadable,
/// and nobody notices when it stops testing what it says.
pub fn path(points: &[f64], step: f64) -> Vec<f64> {
    let mut c = vec![points[0]];
    for &t in &points[1..] {
        while (c[c.len() - 1] - t).abs() > step {
            let last = c[c.len() - 1];
            c.push(if t > last { last + step } else { last - step });
        }
        c.push(t);
    }
    c
}

/// Bars from a list of turning points.
pub fn from_path(points: &[f64]) -> Answer<Bars> {
    Bars::from_closes(&path(points, 0.0002), FIXTURE_SPREAD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_walk_is_actually_a_walk() {
        // A fixture that is not what it claims does not fail -- it passes for
        // the wrong reason, and every test built on it becomes decoration.
        for seed in [1u64, 7, 42, 1234] {
            let b = walk(400, seed);
            let v = b.latest().unwrap();
            let c = v.close();
            let ups = c.windows(2).filter(|w| w[1] > w[0]).count();
            let share = ups as f64 / (c.len() - 1) as f64;
            assert!(
                (0.40..=0.60).contains(&share),
                "seed {}: {:.2} of steps were up -- that is a trend, not a walk",
                seed,
                share
            );
        }
    }

    #[test]
    fn a_walk_actually_produces_swings() {
        // A fixture with no pivots makes every structural test vacuous without
        // failing any of them.
        for seed in [1u64, 7, 42] {
            let b = walk(300, seed);
            let (hi, lo) = b.latest().unwrap().swings(2);
            assert!(
                hi.len() > 5 && lo.len() > 5,
                "seed {}: {} highs, {} lows",
                seed,
                hi.len(),
                lo.len()
            );
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_series() {
        // A calibration number that drifts between machines is not one.
        let a = walk(100, 3);
        let b = walk(100, 3);
        assert_eq!(a.latest().unwrap().close(), b.latest().unwrap().close());
        assert_ne!(
            walk(100, 3).latest().unwrap().now(),
            walk(100, 4).latest().unwrap().now()
        );
    }

    #[test]
    fn a_ramp_has_no_structure_in_it() {
        let b = ramp(200, 0.0008);
        let (hi, lo) = b.latest().unwrap().swings(2);
        assert!(hi.is_empty() && lo.is_empty(), "a straight line has no pivots");
    }

    #[test]
    fn a_zigzag_makes_higher_highs_and_higher_lows() {
        let b = zigzag(9, 6, 3, 0.0015);
        let s = super::super::structure::recent(&b.latest().unwrap(), 5, 2);
        assert_eq!(s.higher_highs(), Some(true), "{}", s.say());
        assert_eq!(s.higher_lows(), Some(true));
    }

    #[test]
    fn a_box_stays_inside_its_box() {
        let b = box_range(200, 1.0980, 1.1020, 14);
        let v = b.latest().unwrap();
        assert!(v.close().iter().all(|&c| (1.0979..=1.1021).contains(&c)));
    }

    #[test]
    fn a_path_reaches_every_turning_point_it_was_given() {
        let p = path(&[1.0, 1.01, 1.005], 0.0002);
        assert!(p.iter().any(|&x| (x - 1.01).abs() < 1e-9));
        assert!((p[p.len() - 1] - 1.005).abs() < 1e-9);
        assert!(p.len() > 50, "the walk must fill in the bars between");
    }
}
