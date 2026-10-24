//! "What can this computer run?" (Plan C, 24 Oct 2026, as a proof slice).
//!
//! `fit::plan_for` already sizes Atlas to the machine at start-up, quietly.
//! This says it out loud: the memory free right now (measured), what each
//! quality tier needs (Atlas's own size estimates), which tiers fit, and
//! what the next one up would take -- before anything is downloaded or
//! loaded, rather than discovered as a machine that has gone slow.
//!
//! The tier needs are not a second table: they are found by asking
//! `plan_for` itself at rising budgets, so they can't drift from what Atlas
//! actually does. Reading only.

use crate::fit::{plan_for, Machine, Tier};

/// The least memory budget at which `plan_for` reaches a tier. No model
/// names: the one actually loaded is chosen by `models::pick`, not here.
#[derive(Debug, Clone, PartialEq)]
pub struct Rung {
    pub tier: Tier,
    pub needs_mb: u64,
}

/// Every tier, cheapest first, from `plan_for` probed in 50 MB steps on a
/// machine with no graphics card (a card only adds the larger models).
pub fn ladder() -> Vec<Rung> {
    let mut out: Vec<Rung> = Vec::new();
    for b in (0..=24_000u64).step_by(50) {
        let m = Machine {
            total_ram_mb: 1 << 30,
            free_ram_mb: (b as f64 / 0.55).ceil() as u64,
            cpu_cores: 8,
            vram_mb: 0,
            has_npu: false,
            disk_free_mb: 1 << 20,
            disk_is_spinning: false,
            reclaimable_mb: 0,
        };
        let p = plan_for(&m);
        if out.iter().all(|r| r.tier != p.tier) {
            out.push(Rung {
                tier: p.tier,
                needs_mb: m.budget_mb(),
            });
        }
    }
    out.sort_by_key(|r| r.tier);
    out
}

/// Asked what this machine can run, or about the quality tiers.
pub fn asks(said: &str) -> bool {
    let s: String = said
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    [
        "what can this computer run",
        "what can this machine run",
        "what can this laptop run",
        "what can my computer run",
        "what can my machine run",
        "what can my laptop run",
        "what can you run here",
        "what can you run on this",
        "quality tiers",
        "which quality tier",
        "what quality tier",
    ]
    .iter()
    .any(|p| s.contains(p))
}

fn gb(mb: u64) -> String {
    format!("{:.1}GB", mb as f64 / 1024.0)
}

/// The answer for this machine: measured memory, each tier against it, and
/// the honest way to the next one -- or that there isn't one here.
pub fn said(m: &Machine, rungs: &[Rung]) -> String {
    let budget = m.budget_mb();
    let ceiling = m.total_ram_mb / 3;
    let now = plan_for(m);
    let mut s = format!(
        "This computer: {}. {} free right now, so about {} for me to work with (measured just now; the sizes below are my estimates).",
        crate::fit::describe(m),
        gb(m.free_ram_mb),
        gb(budget)
    );
    for r in rungs {
        let verdict = if r.needs_mb == 0 {
            "runs anywhere".to_string()
        } else if r.needs_mb <= budget {
            format!("about {} -- fits", gb(r.needs_mb))
        } else if r.needs_mb > ceiling {
            format!(
                "about {} -- out of reach here: I never take more than a third of this machine's {}",
                gb(r.needs_mb),
                gb(m.total_ram_mb)
            )
        } else {
            format!(
                "about {} -- needs {} more free; closing apps you aren't using would get there",
                gb(r.needs_mb),
                gb(r.needs_mb - budget)
            )
        };
        s.push_str(&format!("\n- {}: {verdict}.", r.tier.name()));
    }
    s.push_str(&format!("\nRunning now: {}.", now.tier.name()));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laptop(total_mb: u64, free_mb: u64) -> Machine {
        Machine {
            total_ram_mb: total_mb,
            free_ram_mb: free_mb,
            cpu_cores: 8,
            vram_mb: 0,
            has_npu: false,
            disk_free_mb: 100_000,
            disk_is_spinning: false,
            reclaimable_mb: 0,
        }
    }

    #[test]
    fn the_ladder_has_every_tier_in_order_and_agrees_with_plan_for() {
        let l = ladder();
        assert_eq!(
            l.iter().map(|r| r.tier).collect::<Vec<_>>(),
            [Tier::Bare, Tier::Voice, Tier::Thinking, Tier::Full]
        );
        assert_eq!(l[0].needs_mb, 0);
        assert!(l.windows(2).all(|w| w[0].needs_mb < w[1].needs_mb), "{l:?}");
        // Each need, given as a budget, really reaches that tier -- and 50 MB
        // less does not.
        for r in &l[1..] {
            let at = |b: u64| plan_for(&laptop(1 << 30, (b as f64 / 0.55).ceil() as u64)).tier;
            assert_eq!(at(r.needs_mb), r.tier, "{r:?}");
            assert!(at(r.needs_mb - 50) < r.tier, "{r:?}");
        }
    }

    #[test]
    fn a_tier_out_of_reach_says_so_and_one_within_reach_says_what_it_takes() {
        // 8GB, 2GB free: the top tier needs more than a third of 8GB.
        let small = said(&laptop(8192, 2048), &ladder());
        assert!(small.contains("everything"), "{small}");
        assert!(small.contains("out of reach here"), "{small}");
        assert!(small.contains("text only: runs anywhere"), "{small}");
        // 32GB with little free: the top tier is reachable by closing things.
        let busy = said(&laptop(32_768, 3000), &ladder());
        assert!(busy.contains("more free; closing apps"), "{busy}");
        assert!(!busy.contains("out of reach"), "{busy}");
        // Plenty free: everything fits, and that's what runs.
        let idle = said(&laptop(32_768, 20_000), &ladder());
        assert!(idle.ends_with("Running now: everything."), "{idle}");
        assert!(idle.contains("measured just now"), "{idle}");
    }

    #[test]
    fn it_is_asked_for_in_plain_words_and_not_otherwise() {
        assert!(asks("What can this computer run?"));
        assert!(asks("what quality tiers can you run here"));
        assert!(asks("what can my laptop run"));
        assert!(!asks("run the backup"));
        assert!(!asks("what can you do"));
    }
}
