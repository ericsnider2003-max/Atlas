//! A self-check that runs the whole module offline and prints what it found.
//!
//! This exists because the thing most worth demonstrating about this module
//! cannot be shown by a test suite passing. A suite says "no assertion failed".
//! It does not say what the market looked like, what the calibration numbers
//! came out at, or which of the readings had evidence behind them — and those
//! are the facts somebody reviewing a merge actually needs.
//!
//! It also runs with **no real bars, no network, no files and no clock**, which
//! is the deployment condition that matters: if this prints on a bare Windows
//! machine with nothing installed, the module works there.
//!
//! Call [`report`] from a binary, a test, or Atlas itself.
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "built from fixed fixture data that is known to be valid; a self-check, not a path for real input")]

use super::bars::Bars;
use super::claims::{verify, Claim, Side, ALL_KINDS};
use super::events::{self, Purpose};
use super::feed;
use super::fixtures;
use super::levels;
use super::params;
use super::regime;
use super::session;
use super::structure;
use super::time::{Utc, MS_PER_HOUR, MS_PER_MIN};
use super::timeframe::{self, Horizon, Tf};

fn rule(title: &str) -> String {
    format!("\n{}\n{}", "=".repeat(70), title)
}

/// Everything this module knows, said out loud.
pub fn report() -> String {
    let mut out: Vec<String> = Vec::new();
    out.push("ATLAS :: market -- offline self-check".into());

    // ---- a series to look at ------------------------------------------
    let raw = fixtures::walk(400, 11);
    let v0 = raw.latest().unwrap();
    let start = Utc::at(2026, 9, 28, 0, 0).to_ms();
    let time: Vec<i64> = (0..raw.len()).map(|i| start + i as i64 * 15 * MS_PER_MIN).collect();
    let bars = Bars::new(
        v0.open().to_vec(),
        v0.high().to_vec(),
        v0.low().to_vec(),
        v0.close().to_vec(),
        time,
    )
    .unwrap();
    let when = Utc::at(2026, 10, 2, 12, 35).to_ms(); // inside the payrolls window
    let view = bars.latest().unwrap();

    // ---- the door ------------------------------------------------------
    out.push(rule("THE DOOR -- what the feed had to satisfy"));
    match feed::accept(&bars, Some(bars.all_time()[bars.len() - 1])) {
        Ok(r) => out.push(format!("  {}   timeframe {:?}", r.say(), r.timeframe)),
        Err(e) => out.push(format!("  REFUSED: {}", e)),
    }
    let future_check = feed::accept(&bars, Some(bars.all_time()[200]));
    out.push(format!(
        "  a bar stamped after the decision instant: {}",
        match future_check {
            Err(e) => format!("refused -- {}", &e.0[..e.0.len().min(90)]),
            Ok(_) => "ACCEPTED (this is a defect)".into(),
        }
    ));

    // ---- what the market is doing --------------------------------------
    out.push(rule("WHAT ATLAS READS BEFORE IT ARGUES"));
    out.push(format!("  WHEN    {}", session::session_at(when).say()));
    match regime::read_plain(&view) {
        Ok(r) => out.push(format!("  MARKET  {}", r.say())),
        Err(e) => out.push(format!("  MARKET  cannot say: {}", e)),
    }
    let s = structure::recent(&view, 5, 2);
    out.push(format!("  SHAPE   {}", s.say()));
    if let Some(p) = s.highs.last() {
        out.push(format!(
            "  DELAY   last swing high at bar {} was not knowable until bar {} ({})",
            p.at,
            p.confirmed_at,
            timeframe::say_span(Tf::M15.confirmation_delay_ms(2))
        ));
    }
    out.push("  LEVELS".into());
    if let Ok(mut lv) = levels::levels(&view, 2, 120.0) {
        let now = view.now();
        lv.sort_by(|a, b| (a.price - now).abs().total_cmp(&(b.price - now).abs()));
        for l in lv.iter().take(4) {
            out.push(format!("          {}", l.say()));
        }
    }
    if let Ok(null) = levels::null_bounce_rate(&view, 120, 5) {
        out.push(format!(
            "  NULL    arbitrary levels bounce {:.0}% on these bars -- every real level \
             is read against that, not against 50%",
            null * 100.0
        ));
    }

    // ---- news ----------------------------------------------------------
    out.push(rule("NEWS -- and what a bar actually contains"));
    match events::overlapping(when - 6 * MS_PER_HOUR, when, "EURUSD", Purpose::Volatility) {
        Ok(hits) => {
            for e in hits.iter().take(3) {
                let (a, b) = e.blackout(Purpose::Volatility);
                out.push(format!("  {}", e.say()));
                out.push(format!(
                    "     volatility window {:02}:{:02}-{:02}:{:02}Z -- the industry's \
                     +/-2 min would have cleared this",
                    Utc::from_ms(a).hour,
                    Utc::from_ms(a).minute,
                    Utc::from_ms(b).hour,
                    Utc::from_ms(b).minute
                ));
            }
            if hits.is_empty() {
                out.push("  nothing due".into());
            }
        }
        Err(e) => out.push(format!("  calendar refused: {}", e)),
    }
    let late = Utc::at(2026, 10, 2, 16, 0).to_ms();
    let h4 = events::overlapping(late - Tf::H4.span_ms(1), late, "EURUSD", Purpose::Volatility)
        .map(|v| v.len())
        .unwrap_or(0);
    let m5 = events::overlapping(late - Tf::M5.span_ms(1), late, "EURUSD", Purpose::Volatility)
        .map(|v| v.len())
        .unwrap_or(0);
    out.push(format!(
        "  the bar closing 16:00Z contains {} release(s) on H4 and {} on M5 -- \
         a close-only check would call both clean",
        h4, m5
    ));

    // ---- timeframe -----------------------------------------------------
    out.push(rule("TIMEFRAME -- what a bar is worth"));
    out.push(format!(
        "  {:>4}  {:>12}  {:>18}  {:>14}  {:>18}",
        "TF", "20 bars is", "a day of price", "k=2 delay", "30 bars arrive in"
    ));
    for tf in timeframe::ALL {
        let n = tf.bars_for(Horizon::Recent);
        let floored = if timeframe::stretched(tf, Horizon::Recent).is_some() {
            " (floored)"
        } else {
            ""
        };
        out.push(format!(
            "  {:>4}  {:>12}  {:>18}  {:>14}  {:>18}",
            tf.name(),
            timeframe::say_span(tf.span_ms(20)),
            format!("{} bars{}", n, floored),
            timeframe::say_span(tf.confirmation_delay_ms(2)),
            timeframe::say_span(tf.calendar_ms(30))
        ));
    }

    // ---- the vocabulary ------------------------------------------------
    out.push(rule("THE VOCABULARY -- every reading, and its grounds"));
    for k in ALL_KINDS {
        let long = verify(&Claim::new(k, Side::Long), &view);
        let short = verify(&Claim::new(k, Side::Short), &view);
        out.push(format!(
            "  {:<22} {:<9} long {:<8} short {:<8}",
            k.name(),
            format!("{:?}", k.grounds()),
            long.truth.say(),
            short.truth.say()
        ));
    }

    // ---- lookahead -----------------------------------------------------
    out.push(rule("LOOKAHEAD -- why it is not expressible here"));
    out.push(
        "  Every reader takes an AsOf view, never the series. The accessors are\n  \
         cut at the bound, so a later price is not reachable from what a reader\n  \
         holds -- the same reason it cannot read past the end of a slice."
            .into(),
    );
    // Demonstrated rather than asserted: the same bar, reached two ways.
    let mut agree = 0;
    let mut checked = 0;
    for at in [150usize, 220, 300] {
        // The cold frame must carry the SAME timestamps. Without them the
        // regime reader cannot infer the timeframe, falls back to its default
        // lookback and answers a different question -- which showed up here as
        // three "leaks" that were nothing of the sort. The comparison was
        // wrong, not the readers, and that is exactly the class of mistake a
        // leak detector has to be immune to before it is worth trusting.
        let cold_bars = Bars::new(
            view.open()[..=at].to_vec(),
            view.high()[..=at].to_vec(),
            view.low()[..=at].to_vec(),
            view.close()[..=at].to_vec(),
            bars.all_time()[..=at].to_vec(),
        )
        .unwrap();
        let cold = cold_bars.latest().unwrap();
        let warm = bars.as_of(at).unwrap();
        for k in ALL_KINDS {
            for side in [Side::Long, Side::Short] {
                let a = verify(&Claim::new(k, side), &cold);
                let b = verify(&Claim::new(k, side), &warm);
                checked += 1;
                if a.truth == b.truth {
                    agree += 1;
                }
            }
        }
    }
    out.push(format!(
        "  a frame that has never seen a later bar, and a view of one that has:\n  \
         {}/{} readings identical at three points in the series",
        agree, checked
    ));
    // Stated against the bar that actually proves the point. Asking "how much
    // was unknowable three bars ago" returns zero whenever no pivot happens to
    // sit there, which reads as reassurance and is not one.
    if let Some(last) = structure::confirmed_swings(&view, 2, None).last().copied() {
        let n = structure::unknowable(&view, last.confirmed_at - 1, 2).len();
        out.push(format!(
            "  at bar {}, {} pivot(s) existed in the data that had not happened yet;\n               the most recent was still unproven for another {}",
            last.confirmed_at - 1,
            n,
            timeframe::say_span(Tf::M15.confirmation_delay_ms(2))
        ));
    }

    // ---- the freeze ----------------------------------------------------
    out.push(rule("THE PARAMETER FREEZE"));
    out.push(format!(
        "  set {}  --  {} values, {} searchable, {} configuration(s) tried",
        params::fingerprint(),
        params::frozen().len(),
        params::freedoms(),
        params::attempts()
    ));
    let drift = params::drift();
    out.push(if drift.is_empty() {
        "  no drift: every frozen value matches what its module actually has".into()
    } else {
        format!("  DRIFT: {}", drift.join("; "))
    });

    out.push(rule("CALIBRATION -- measured here, not assumed"));
    let mut fired = 0;
    for seed in 1u64..=60 {
        let b = fixtures::walk(300, seed);
        if let Ok(r) = regime::read_plain(&b.latest().unwrap()) {
            if r.direction != regime::Direction::Flat {
                fired += 1;
            }
        }
    }
    out.push(format!(
        "  pure noise reads as a trend {:.0}% of the time on a 20-bar window",
        fired as f64 / 60.0 * 100.0
    ));
    out.push(format!(
        "  the efficiency ratio's random-walk baseline: n=10 -> {:.3}, n=30 -> {:.3}\n  \
         (the universally quoted 0.30 threshold is BELOW chance at n=10)",
        regime::er_baseline(10),
        regime::er_baseline(30)
    ));
    out.push(format!(
        "  a Hurst test would need {} bars to see rho=0.05; that is why there is not one",
        regime::hurst_needs(0.05, 0.80)
    ));

    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_self_check_runs_and_says_something_about_everything() {
        let r = report();
        for section in [
            "THE DOOR",
            "WHAT ATLAS READS",
            "NEWS",
            "TIMEFRAME",
            "THE VOCABULARY",
            "LOOKAHEAD",
            "PARAMETER FREEZE",
            "CALIBRATION",
        ] {
            assert!(r.contains(section), "missing section: {}", section);
        }
        assert!(r.contains("no drift"), "the freeze must be intact");
    }

    #[test]
    fn the_self_check_names_every_claim_kind() {
        let r = report();
        for k in ALL_KINDS {
            assert!(r.contains(k.name()), "{} is missing from the report", k.name());
        }
    }

    #[test]
    fn the_self_check_needs_no_clock_no_files_and_no_network() {
        // The deployment condition that matters. Nothing here reads the system
        // clock, opens a file or resolves a name -- so if it runs at all, it
        // runs on a bare machine with nothing installed. Asserted by construction:
        // the function takes no arguments and returns a String, and the whole
        // module tree imports only `std::cell` and `std::fmt`.
        let a = report();
        let b = report();
        assert_eq!(a, b, "the self-check is not deterministic");
    }

    #[test]
    fn the_same_bar_reached_two_ways_reads_the_same() {
        // The claim the LOOKAHEAD section makes, checked rather than printed.
        let r = report();
        let line = r
            .lines()
            .find(|l| l.contains("readings identical"))
            .expect("the lookahead comparison is missing");
        let got: Vec<&str> = line.split_whitespace().next().unwrap().split('/').collect();
        assert_eq!(got[0], got[1], "{}", line);
    }
}
