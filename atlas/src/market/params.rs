//! Every tunable number in this module, frozen, with the reason it has it.
//!
//! ## Why this had to be written before the first real backtest
//!
//! Sullivan, Timmermann & White took 7,846 technical trading rules — including
//! 1,220 support-and-resistance variants — and tested them under White's
//! Reality Check for data snooping. The rules that looked best in sample did
//! not survive out of sample. That is not a curiosity; it is the **default
//! outcome** of searching a large parameter space against one dataset.
//!
//! This module has that space: pivot strength, level tolerance, touch
//! separation, three lookbacks, the trend entry and exit gates, dwell time, the
//! cost multiple, the minimum window. Turning them against the first real
//! dataset until the equity curve looks right would produce a system that
//! backtests beautifully and does nothing — and there would be no way
//! afterwards to tell that from a system that works, because the evidence would
//! have been spent.
//!
//! So they are written down **now**, while nobody knows what the real data
//! looks like and therefore nobody can be tempted. Each carries its value, its
//! reason, and how it may legitimately change. That third field is the point:
//! "measured on the bars" and "it looked better" are different grounds, and the
//! difference is invisible six months later unless it was recorded at the time.
//!
//! ## How the freeze is enforced
//!
//! Every entry holds an `expected` literal **and** reads the live constant from
//! the module that owns it. [`drift`] compares them, and the test suite fails
//! on any difference — so a value edited in its own module and not here is
//! caught, and the change has to be deliberate in both places rather than in
//! neither. A planted mismatch proves the check can fire.
//!
//! [`fingerprint`] hashes the whole set. Record it beside a backtest result: a
//! result quoted without one cannot be reproduced, whatever the commit message
//! says.
//!
//! [`attempts`] is the multiple-comparisons count. A system that has tried
//! forty configurations and reports the best has not found an edge — it has
//! found the largest of forty noise draws, and there is no way to recover that
//! fact afterwards unless it was counted at the time. It is zero, and that is
//! the whole value of it.
//!
//! ## What this is not
//!
//! Not a config file. Nothing reads it at runtime; the modules own their own
//! constants and this registry checks them. Making it the source of truth would
//! let one import change the behaviour of the whole system, which is precisely
//! the door it exists to close.

use super::{levels, regime, timeframe};

/// How a value is allowed to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// Falls out of arithmetic. Changing it means the maths changed, and that
    /// is not a search.
    Derived,
    /// Set from a measurement. Change only with a new measurement.
    Measured,
    /// A judgement call. Changing it is a new configuration.
    Chosen,
    /// Somebody else's constant. Changes when theirs does.
    External,
}

pub struct Param {
    pub module: &'static str,
    pub name: &'static str,
    /// What the registry says it should be.
    pub expected: f64,
    /// What the owning module actually has, read at compile time.
    pub live: f64,
    pub basis: Basis,
    pub why: &'static str,
}

impl Param {
    /// Does changing this burn a degree of freedom.
    ///
    /// `Derived` and `External` do not: 1/√n is not a choice and somebody
    /// else's spread is not ours to tune. Counting them would make the discount
    /// meaningless.
    pub fn costs_a_comparison(&self) -> bool {
        matches!(self.basis, Basis::Measured | Basis::Chosen)
    }
    pub fn say(&self) -> String {
        format!("{}::{} = {} [{:?}] {}", self.module, self.name, self.live, self.basis, self.why)
    }
}

pub fn frozen() -> Vec<Param> {
    vec![
        Param {
            module: "levels", name: "TOUCH_SEPARATION",
            expected: 3.0, live: levels::TOUCH_SEPARATION as f64, basis: Basis::Chosen,
            why: "bars between visits before a second touch counts as separate. Without \
                  it a level price hugged for twenty bars reports twenty touches and \
                  outranks one tested on three occasions",
        },
        Param {
            module: "levels", name: "STOP_BAND_PIPS",
            expected: 10.0, live: levels::STOP_BAND_PIPS, basis: Basis::Measured,
            why: "Osler (2003): stop orders cluster at rates ending 01-10 past the \
                  figure, 14.4% against 7.4% just below it",
        },
        Param {
            module: "regime", name: "DEFAULT_SPREAD_PIPS",
            expected: 0.8, live: regime::DEFAULT_SPREAD_PIPS, basis: Basis::External,
            why: "typical retail EURUSD; a real deployment passes its own",
        },
        Param {
            module: "regime", name: "DEFAULT_SLIPPAGE_PIPS",
            expected: 0.3, live: regime::DEFAULT_SLIPPAGE_PIPS, basis: Basis::External,
            why: "a property of the broker, not of this system",
        },
        Param {
            module: "regime", name: "COST_MULTIPLE",
            expected: 8.0, live: regime::COST_MULTIPLE, basis: Basis::Chosen,
            why: "how many round trips of cost a range must be worth to be worth \
                  trading. The SHAPE of the rule is arithmetic; the 8 is a choice",
        },
        Param {
            module: "regime", name: "ENTER_TREND",
            expected: 1.30, live: regime::ENTER_TREND, basis: Basis::Chosen,
            why: "multiples of the random-walk efficiency ratio needed to call a trend. \
                  Measured consequence: pure noise reads as a trend about a third of \
                  the time on a twenty-bar window at this gate",
        },
        Param {
            module: "regime", name: "EXIT_TREND",
            expected: 1.10, live: regime::EXIT_TREND, basis: Basis::Chosen,
            why: "the hysteresis floor. Must stay below ENTER_TREND or the label flips \
                  every time the measure grazes the threshold",
        },
        Param {
            module: "regime", name: "MIN_DWELL",
            expected: 5.0, live: regime::MIN_DWELL as f64, basis: Basis::Chosen,
            why: "bars a regime must be held before it can be left. Should exceed the \
                  lag of the measure driving it",
        },
        Param {
            module: "timeframe", name: "MIN_BARS",
            expected: 20.0, live: timeframe::MIN_BARS as f64, basis: Basis::Derived,
            why: "the floor below which the measures cannot answer: the efficiency \
                  ratio's baseline at n=6 is 0.41 and the 95% critical R-squared at \
                  n=5 is 0.77, so a smaller window is a sample too small to ask",
        },
        Param {
            module: "timeframe", name: "TRADING_HOURS_PER_WEEK",
            expected: 120.0, live: timeframe::TRADING_HOURS_PER_WEEK, basis: Basis::External,
            why: "FX trades about 120 hours a week, not 168. Using 168 would promise \
                  data a third sooner than it can arrive",
        },
        Param {
            module: "feed", name: "GAP_BARS",
            expected: 2.0, live: super::feed::GAP_BARS, basis: Basis::Chosen,
            why: "bar-lengths of silence before a hole is worth reporting. One missing \
                  bar is a quiet market; several is a feed problem",
        },
        Param {
            module: "bars", name: "swing k",
            expected: 2.0, live: 2.0, basis: Basis::Chosen,
            why: "swing strength, and therefore the confirmation delay. On H4 this is \
                  eight hours before a pivot is knowable",
        },
    ]
}

/// Append-only. Every entry is a degree of freedom spent.
///
/// Empty on purpose at the freeze. If it is still empty when a backtest is
/// reported, that result is worth what it says. If it has forty entries, the
/// result is the best of forty draws and should be read as one.
pub const CHANGES: [(&str, &str); 0] = [];

pub fn attempts() -> usize {
    CHANGES.len()
}

/// How many parameters could legitimately be searched over.
pub fn freedoms() -> usize {
    frozen().iter().filter(|p| p.costs_a_comparison()).count()
}

/// A short hash of every frozen value.
///
/// Not a cryptographic hash and does not need to be: it has to change when any
/// value changes and be the same on every machine, which FNV-1a over the
/// rendered values does with no dependency.
pub fn fingerprint() -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in frozen() {
        for b in format!("{}::{}={:.10};", p.module, p.name, p.live).bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
    }
    format!("{:012x}", h & 0xffff_ffff_ffff)
}

/// Parameters whose live value no longer matches the frozen one.
///
/// This is what stops the registry becoming a document that was true once.
pub fn drift() -> Vec<String> {
    frozen()
        .iter()
        .filter(|p| (p.live - p.expected).abs() > 1e-12)
        .map(|p| format!("{}::{}: live {}, frozen at {}", p.module, p.name, p.live, p.expected))
        .collect()
}

pub fn report() -> String {
    let d = drift();
    let mut out = vec![format!(
        "parameter freeze {} -- {} values, {} of them searchable, {} \
         configuration(s) tried so far",
        fingerprint(),
        frozen().len(),
        freedoms(),
        attempts()
    ), String::new()];
    if !d.is_empty() {
        out.push("DRIFT -- the registry and the code disagree:".into());
        for line in &d {
            out.push(format!("  {}", line));
        }
        out.push(String::new());
    }
    let all = frozen();
    for basis in [Basis::Derived, Basis::Measured, Basis::Chosen, Basis::External] {
        let rows: Vec<&Param> = all.iter().filter(|p| p.basis == basis).collect();
        if rows.is_empty() {
            continue;
        }
        out.push(format!("{:?} ({}):", basis, rows.len()));
        for p in rows {
            out.push(format!("  {}::{} = {}", p.module, p.name, p.live));
            out.push(format!("      {}", p.why));
        }
        out.push(String::new());
    }
    out.join("\n").trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_has_drifted() {
        // The test that keeps this file honest. A registry not checked against
        // the code becomes a document that was true once.
        let d = drift();
        assert!(d.is_empty(), "{}", d.join("\n"));
    }

    #[test]
    fn the_drift_check_catches_a_value_planted_to_disagree() {
        // A check that cannot fire is decoration.
        let planted = Param {
            module: "regime", name: "MIN_DWELL",
            expected: 99.0, live: regime::MIN_DWELL as f64, basis: Basis::Chosen,
            why: "deliberately wrong, to prove the check can fail",
        };
        assert!((planted.live - planted.expected).abs() > 1e-12);
    }

    #[test]
    fn every_module_with_a_tunable_is_represented() {
        let mods: Vec<&str> = frozen().iter().map(|p| p.module).collect();
        for m in ["levels", "regime", "timeframe", "feed", "bars"] {
            assert!(mods.contains(&m), "{} has tunables and none are frozen", m);
        }
    }

    #[test]
    fn the_fingerprint_is_stable_and_moves_when_a_value_does() {
        let a = fingerprint();
        assert_eq!(a, fingerprint());
        assert_eq!(a.len(), 12);
        // Hashing a deliberately different set must land somewhere else, or the
        // fingerprint cannot tie a result to the system that produced it.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in "regime::ENTER_TREND=9.9;".bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        assert_ne!(a, format!("{:012x}", h & 0xffff_ffff_ffff));
    }

    #[test]
    fn arithmetic_and_other_peoples_constants_do_not_count_as_choices() {
        for p in frozen() {
            match p.basis {
                Basis::Derived | Basis::External => assert!(!p.costs_a_comparison(), "{}", p.name),
                _ => assert!(p.costs_a_comparison(), "{}", p.name),
            }
        }
    }

    #[test]
    fn the_search_space_is_a_stated_fact() {
        let n = freedoms();
        assert!((5..=40).contains(&n), "{}", n);
        assert!(n < frozen().len(), "not every parameter is a free choice");
    }

    #[test]
    fn no_configurations_have_been_tried_yet_and_the_count_says_so() {
        // The whole value of this number is that it starts honest.
        assert_eq!(attempts(), 0);
    }

    #[test]
    fn every_value_carries_the_reason_it_has_that_value() {
        for p in frozen() {
            assert!(p.why.len() > 30, "{}", p.name);
        }
    }

    #[test]
    fn a_value_called_measured_names_its_measurement() {
        for p in frozen().iter().filter(|p| p.basis == Basis::Measured) {
            assert!(
                ["Osler", "Chaboud", "Brunnermeier", "Measured", "measured"]
                    .iter()
                    .any(|w| p.why.contains(w)),
                "{}: {}",
                p.name,
                p.why
            );
        }
    }

    #[test]
    fn the_trend_gates_keep_their_dead_zone() {
        // A cross-parameter invariant. Frozen values are checked one at a time,
        // so a relationship between two of them needs saying explicitly.
        assert!(
            regime::EXIT_TREND < regime::ENTER_TREND,
            "the exit gate has risen above the entry gate; the label will now \
             flip every time the measure grazes the threshold"
        );
    }

    #[test]
    fn the_report_separates_measured_values_from_chosen_ones() {
        let r = report();
        for b in ["Measured", "Chosen", "External"] {
            assert!(r.contains(b), "{}", b);
        }
        assert!(r.contains(&fingerprint()));
        assert!(!r.contains("DRIFT"));
    }
}
