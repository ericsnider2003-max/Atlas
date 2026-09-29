use atlas::fit::{describe, limits, plan_for, worth_replanning, Machine, Tier};
use atlas::knowhow::{announce, as_plan, Knowhow};
use atlas::route::{coverage, needs_internet, known_routes, Kind};

// ================= fitting whatever machine it lands on =================

/// Eric's laptop, as measured.
fn laptop() -> Machine {
    Machine {
        total_ram_mb: 16070,
        free_ram_mb: 2900,
        cpu_cores: 8,
        vram_mb: 8192,
        has_npu: true,
        disk_free_mb: 120_000,
        disk_is_spinning: false,
        reclaimable_mb: 0,
    }
}

/// A friend with a real desktop.
fn desktop() -> Machine {
    Machine {
        total_ram_mb: 65536,
        free_ram_mb: 48000,
        cpu_cores: 16,
        vram_mb: 12288,
        has_npu: false,
        disk_free_mb: 900_000,
        disk_is_spinning: false,
        reclaimable_mb: 0,
    }
}

/// A friend with an old laptop.
fn old() -> Machine {
    Machine {
        total_ram_mb: 8192,
        free_ram_mb: 1800,
        cpu_cores: 4,
        vram_mb: 0,
        has_npu: false,
        disk_free_mb: 20_000,
        disk_is_spinning: true,
        reclaimable_mb: 0,
    }
}

/// A netbook, basically.
fn tiny() -> Machine {
    Machine {
        total_ram_mb: 4096,
        free_ram_mb: 700,
        cpu_cores: 2,
        vram_mb: 0,
        has_npu: false,
        disk_free_mb: 8000,
        disk_is_spinning: true,
        reclaimable_mb: 0,
    }
}

#[test]
fn what_is_free_matters_more_than_what_exists() {
    // Taking a share of the total is how you end up swapping.
    let idle = Machine { free_ram_mb: 14000, ..laptop() };
    let busy = Machine { free_ram_mb: 1200, ..laptop() };
    assert!(idle.budget_mb() > busy.budget_mb());
    assert!(
        idle.budget_mb() <= idle.total_ram_mb / 3,
        "and never more than a third of the machine, however idle it looks"
    );
}

#[test]
fn integrated_graphics_do_not_add_memory_they_share_it() {
    assert_eq!(laptop().usable_vram_mb(), 8192, "8GB is treated as real");
    let shared = Machine { vram_mb: 2048, ..laptop() };
    assert_eq!(shared.usable_vram_mb(), 0, "2GB shared is not a GPU budget");
}

#[test]
fn a_good_desktop_gets_everything() {
    let p = plan_for(&desktop());
    assert_eq!(p.tier, Tier::Full);
    assert!(p.model.unwrap().contains("14b") || p.model.unwrap().contains("7b"));
    assert!(p.vision, "there's a real GPU here");
    assert_eq!(p.concurrency, 4);
}

#[test]
fn eric_s_laptop_gets_speech_search_and_a_small_model() {
    let p = plan_for(&laptop());
    assert!(p.speech.is_some());
    assert!(p.embedding.is_some(), "search by meaning is 90MB and pays for itself");
    assert!(!p.vision, "integrated graphics can't do it usefully");
    assert!(p.because.contains("NPU"), "and it says where the NPU helps: {}", p.because);
}

#[test]
fn an_old_laptop_still_hears_you() {
    let p = plan_for(&old());
    assert!(p.speech.is_some(), "speech is the cheapest thing worth having");
    assert!(p.tier >= Tier::Voice);
}

#[test]
fn a_very_small_machine_still_hears_you_it_just_cannot_reason() {
    // Speech fits almost anywhere. A language model does not, and most of
    // Atlas doesn't need one.
    let p = plan_for(&tiny());
    assert_eq!(p.tier, Tier::Voice);
    assert!(p.speech.is_some());
    assert!(p.model.is_none());
    assert_eq!(p.concurrency, 1);
    let said = limits(&p);
    assert!(said.iter().any(|l| l.contains("Most of what I do doesn't need one")));
    assert!(said.iter().any(|l| l.contains("One thing at a time")));
}

#[test]
fn a_machine_with_nothing_spare_falls_back_to_text_and_says_so() {
    let nothing = Machine {
        total_ram_mb: 2048,
        free_ram_mb: 300,
        cpu_cores: 2,
        vram_mb: 0,
        has_npu: false,
        disk_free_mb: 4000,
        disk_is_spinning: true,
        reclaimable_mb: 0,
    };
    let p = plan_for(&nothing);
    assert_eq!(p.tier, Tier::Bare);
    assert!(limits(&p).iter().any(|l| l.contains("everything works typed")));
}

#[test]
fn what_it_cannot_do_here_is_said_up_front_not_discovered_later() {
    let l = limits(&plan_for(&old()));
    assert!(!l.is_empty());
    assert!(l.iter().any(|x| x.contains("read text off it") || x.contains("match words")));
}

#[test]
fn the_reasoning_is_given_in_gigabytes_not_jargon() {
    let p = plan_for(&laptop());
    assert!(p.because.contains("GB of memory"));
    assert!(p.because.contains("free"));
}

#[test]
fn a_bigger_model_is_never_chosen_than_actually_fits() {
    for m in [laptop(), desktop(), old(), tiny()] {
        let p = plan_for(&m);
        assert!(p.download_mb < m.disk_free_mb, "would not fit on disk");
    }
}

#[test]
fn the_model_is_kept_loaded_when_there_is_room_because_reloading_is_the_big_cost() {
    assert!(plan_for(&desktop()).keep_model_warm);
    // And on a spinning disk it's kept warm even when memory is tight, because
    // re-reading two gigabytes is worse.
    let slow = Machine { disk_is_spinning: true, ..old() };
    let p = plan_for(&slow);
    if p.model.is_some() {
        assert!(p.keep_model_warm);
    }
}

#[test]
fn a_machine_that_changes_gets_replanned() {
    // The plan made on a day you had forty tabs open isn't the plan you want
    // forever.
    let busy = Machine { free_ram_mb: 1200, ..laptop() };
    let free = Machine { free_ram_mb: 11000, ..laptop() };
    assert!(worth_replanning(&busy, &free));
    assert!(!worth_replanning(&laptop(), &laptop()));
    assert!(worth_replanning(&laptop(), &desktop()));
}

#[test]
fn atlas_can_describe_the_machine_in_words() {
    assert!(describe(&laptop()).contains("NPU"));
    assert!(describe(&desktop()).contains("graphics card"));
    assert!(describe(&tiny()).contains("modest"));
}

// ================= being useful with no internet =================

#[test]
fn atlas_ships_knowing_how_to_do_things() {
    let k = Knowhow::shipped();
    assert!(k.procedures.len() >= 6);
    for p in &k.procedures {
        assert!(!p.steps.is_empty(), "{} has no steps", p.id);
        assert!(!p.snags.is_empty(), "{} doesn't know what goes wrong", p.id);
    }
}

#[test]
fn nearly_everything_it_knows_works_offline() {
    let (offline, total) = Knowhow::shipped().offline_coverage();
    assert!(offline as f32 / total as f32 > 0.8, "{offline} of {total}");
}

#[test]
fn it_finds_the_procedure_from_how_you_would_actually_ask() {
    let k = Knowhow::shipped();
    assert_eq!(k.for_request("it's running slowly", true).unwrap().id, "free-up-memory");
    assert_eq!(k.for_request("you're not listening to me", true).unwrap().id, "cant-hear-you");
    assert_eq!(k.for_request("where did I put my notes on the VPS", true).unwrap().id, "find-a-file");
}

#[test]
fn offline_it_offers_only_what_works_offline() {
    let k = Knowhow::shipped();
    let online = k.for_request("look into the QUIC spec", true);
    assert_eq!(online.unwrap().id, "research-something");
    let offline = k.for_request("look into the QUIC spec", false);
    assert!(offline.map(|p| p.id != "research-something").unwrap_or(true));
}

#[test]
fn a_procedure_needing_the_internet_says_so_and_offers_the_local_alternative() {
    let research = Knowhow::shipped()
        .procedures
        .into_iter()
        .find(|p| p.id == "research-something")
        .unwrap();
    let said = announce(&research, false);
    assert!(said.contains("needs the internet, and there isn't any"));
    assert!(said.contains("offline instead"));
}

#[test]
fn it_knows_what_usually_goes_wrong_not_just_the_happy_path() {
    // The difference between a procedure and a list.
    let k = Knowhow::shipped();
    let (p, snag) = k.for_symptom("it launches and closes immediately").unwrap();
    assert_eq!(p.id, "app-wont-start");
    assert!(snag.cause.contains("updater stub"));
    assert!(snag.fix.contains("wait two seconds"));
}

#[test]
fn the_microphone_procedure_knows_about_the_thing_that_actually_happens() {
    let k = Knowhow::shipped();
    let (_, snag) = k.for_symptom("it worked yesterday and not today").unwrap();
    assert!(snag.cause.contains("device names shifted"));
    assert!(snag.fix.contains("re-measure"), "rather than trusting a saved name");
}

#[test]
fn a_new_failure_is_learned_so_the_surprise_happens_once() {
    let mut k = Knowhow::shipped();
    assert!(k.learn_snag("find-a-file", "finds the wrong file every time",
        "two files with the same name", "say which folder each is in"));
    assert!(!k.learn_snag("find-a-file", "finds the wrong file every time", "x", "y"),
        "not twice");
    assert!(k.for_symptom("finds the wrong file every time").is_some());
}

#[test]
fn a_procedure_becomes_something_you_can_follow() {
    let k = Knowhow::shipped();
    let p = k.for_request("it's running slowly", true).unwrap();
    let steps = as_plan(p);
    assert!(steps[0].contains("until"), "with how you know it worked: {}", steps[0]);
}

#[test]
fn asking_about_something_it_has_no_procedure_for_returns_nothing() {
    // Rather than the nearest thing, which would be worse than admitting it.
    assert!(Knowhow::shipped().for_request("book me a flight to Osaka", true).is_none());
}

// ================= online adds routes, it never enables them =================

#[test]
fn every_kind_of_problem_has_a_way_through_with_no_internet() {
    for kind in [Kind::Extract, Kind::Act, Kind::Learn, Kind::Fix] {
        let (offline, total) = coverage(kind);
        assert!(offline > 0, "{kind:?} has no offline route at all");
        assert!(offline < total || kind != Kind::Learn, "being online should add something");
    }
}

#[test]
fn being_online_is_strictly_better_never_differently_better() {
    // Nothing is possible only when offline.
    let all = known_routes();
    let online_only: Vec<&str> = all.iter().filter(|r| needs_internet(r)).map(|r| r.name.as_str()).collect();
    assert!(!online_only.is_empty(), "some things do need it");
    assert!(all.iter().filter(|r| !needs_internet(r)).count() > online_only.len(),
        "but most don't");
}
