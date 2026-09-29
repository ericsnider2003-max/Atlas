use atlas::fit::{plan_for, what_to_drop, Machine, Tier};
use atlas::improve::{automatic, automatic_cost_mb, mechanisms, progress, Gain, Vocabulary};
use atlas::learned::Learned;
use atlas::route::{plan_with, Kind, Plan, Record, RouteConfig};

/// Eric's laptop as measured — with Teams and Spotify idling.
fn laptop() -> Machine {
    Machine {
        total_ram_mb: 16070,
        free_ram_mb: 2900,
        cpu_cores: 8,
        vram_mb: 8192,
        has_npu: true,
        disk_free_mb: 120_000,
        disk_is_spinning: false,
        reclaimable_mb: 2400,
    }
}

// ================= not deciding a machine is small =================

#[test]
fn the_plan_counts_memory_atlas_could_free_not_just_what_is_free_now() {
    // Otherwise a laptop gets called small because Teams is idling.
    let m = laptop();
    assert!(m.budget_mb() > m.budget_now_mb());
    assert!(m.worth_reclaiming() > 1000, "over a gigabyte on the table");
}

#[test]
fn tidying_up_moves_eric_s_laptop_to_a_bigger_model() {
    let tidied = plan_for(&laptop());
    let untidied = plan_for(&Machine { reclaimable_mb: 0, ..laptop() });
    assert_eq!(untidied.model, Some("qwen2.5-1.5b-instruct-q4"), "cramped");
    assert_eq!(tidied.model, Some("qwen2.5-3b-instruct-q4"), "twice the model, same laptop");
    assert!(tidied.tier >= untidied.tier);
}

#[test]
fn atlas_says_that_the_bigger_plan_depends_on_closing_things() {
    // And that it will ask first.
    let p = plan_for(&laptop());
    assert!(p.because.contains("currently held by things you aren't using"));
    assert!(p.because.contains("ask before closing"), "got: {}", p.because);
}

#[test]
fn a_machine_with_nothing_to_reclaim_is_not_promised_anything() {
    let clean = Machine { reclaimable_mb: 0, ..laptop() };
    assert_eq!(clean.worth_reclaiming(), 0);
    assert!(!plan_for(&clean).because.contains("aren't using"));
}

#[test]
fn atlas_offers_to_drop_what_you_never_use() {
    // The other half of not limiting a machine.
    let installed = vec![
        ("en_GB-alan voice".to_string(), 63),
        ("en_US-ryan voice".to_string(), 63),
        ("spanish language model".to_string(), 142),
    ];
    let used = vec!["ryan".to_string()];
    let drop = what_to_drop(&installed, &used);
    assert_eq!(drop.len(), 2);
    assert!(drop.iter().all(|d| !d.what.contains("ryan")), "not the one you use");
    assert!(drop[0].costs_you.contains("re-download it in a minute"));
}

#[test]
fn eric_s_laptop_is_not_treated_as_a_small_machine() {
    let p = plan_for(&laptop());
    assert!(p.tier >= Tier::Thinking);
    assert!(p.speech.is_some() && p.embedding.is_some() && p.model.is_some());
    assert!(p.keep_model_warm);
}

// ================= getting better on the same hardware =================

#[test]
fn a_route_that_keeps_working_here_beats_one_i_guessed_was_better() {
    // The shipped numbers are estimates. After a dozen real attempts Atlas
    // knows better than I did.
    let mut r = Record::default();
    for _ in 0..15 {
        // The quick route keeps failing on this particular site.
        r.note("read the file that's already there", "awkward.com", false);
        r.note("read the page through the browser", "awkward.com", true);
    }
    let have: Vec<String> = vec![
        "a downloaded file".into(), "the app open".into(), "a logged-in session".into(),
        "the browser".into(), "OCR".into(),
    ];
    match plan_with(Kind::Extract, "awkward.com", &have, &Learned::default(), &r, &RouteConfig::default(), 0) {
        Plan::Try { route, why } => {
            assert_ne!(route.name, "read the file that's already there", "it learned");
            assert!(why.contains("here"), "and says it's from experience: {why}");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn one_success_does_not_mean_a_hundred_percent() {
    // Evidence is weighed by how much of it there is.
    let mut r = Record::default();
    r.note("read the window's text", "acme.com", true);
    let after_one = r.rate("read the window's text", "acme.com", 0.8);
    assert!(after_one < 0.9, "one win shouldn't overturn the estimate: {after_one}");

    for _ in 0..25 {
        r.note("read the window's text", "acme.com", true);
    }
    assert!(r.rate("read the window's text", "acme.com", 0.8) > 0.93, "twenty-five should");
}

#[test]
fn a_route_never_tried_here_uses_the_shipped_estimate() {
    assert_eq!(Record::default().rate("anything", "anywhere", 0.7), 0.7);
}

#[test]
fn learning_your_words_is_the_cheapest_large_gain_there_is() {
    let mut v = Vocabulary::default();
    for _ in 0..3 {
        v.learn("The QUIC v1 certification for Homelab is with the IETF");
    }
    let hints = v.hints(10);
    assert!(hints.contains(&"QUIC".to_string()));
    assert!(hints.contains(&"Homelab".to_string()));
    assert!(!hints.contains(&"certification".to_string()), "ordinary words are already known");
}

#[test]
fn the_hint_list_is_kept_short_because_a_long_one_makes_it_worse() {
    // Everything starts sounding like something on the list.
    let mut v = Vocabulary::default();
    for i in 0..300 {
        v.learn(&format!("Word{i} Word{i}"));
    }
    assert!(v.hints(20).len() <= 20);
    assert!(v.words.len() <= 500);
}

#[test]
fn a_word_said_once_is_not_learned_yet() {
    let mut v = Vocabulary::default();
    v.learn("Homelab");
    assert!(v.hints(10).is_empty(), "once is a typo, twice is a word");
    v.learn("Homelab");
    assert_eq!(v.hints(10), vec!["Homelab"]);
}

#[test]
fn most_of_how_atlas_improves_costs_no_memory_at_all() {
    let all = mechanisms();
    let free = all.iter().filter(|m| m.costs_mb == 0).count();
    let cheap = all.iter().filter(|m| m.costs_mb <= 60).count();
    assert!(free >= 3, "several gains are entirely free");
    assert!(cheap * 2 > all.len(), "and most are nearly free");
}

#[test]
fn the_ways_it_gets_better_are_mostly_automatic() {
    let all = mechanisms();
    let auto = automatic();
    assert!(auto.len() * 2 > all.len(), "most need nothing from you");
    assert!(all.iter().any(|m| m.gain == Gain::LearnedRoutes && m.automatic));
    assert!(all.iter().any(|m| m.gain == Gain::LearnedSnags && m.automatic));
}

#[test]
fn the_biggest_gains_are_free_or_nearly_so() {
    let all = mechanisms();
    for g in [Gain::LearnedRoutes, Gain::LearnedSnags, Gain::MovedOffThePath] {
        let m = all.iter().find(|m| m.gain == g).unwrap();
        assert_eq!(m.costs_mb, 0, "{g:?} should cost nothing");
        assert!(m.worth.contains("large"), "{g:?} should be worth having");
    }
}

#[test]
fn keeping_the_model_warm_is_named_as_the_biggest_latency_win() {
    let m = mechanisms().into_iter().find(|m| m.gain == Gain::WarmModel).unwrap();
    assert!(m.worth.contains("biggest latency win"));
    assert!(m.costs_mb > 1000, "and it's honest that it costs memory");
}

#[test]
fn the_automatic_gains_together_cost_less_than_a_model() {
    assert!(automatic_cost_mb() < 3000);
}

#[test]
fn you_can_ask_how_it_is_getting_on() {
    assert!(progress(0, 0, 0).contains("ask me to do things"));
    let said = progress(140, 3, 22);
    assert!(said.contains("140 attempts scored"));
    assert!(said.contains("3 new failures understood"));
    assert!(said.contains("22 of your words learned"));
    assert!(said.contains("None of it cost you anything"));
}
