use atlas::checks::{
    by_id, first_pass, is_refused, needs_approval, read_only, reversible, Kind, Undo, CHECKS, NEVER,
};

// ================= the catalogue is honest =================

#[test]
fn every_check_names_a_real_mechanism_rather_than_a_vibe() {
    for c in CHECKS {
        assert!(!c.how.trim().is_empty(), "{} has no mechanism", c.id);
        assert!(
            c.how.len() > 8,
            "{} says '{}', which is not something anyone can act on",
            c.id,
            c.how
        );
        assert!(c.what.len() > 30, "{} does not explain itself", c.id);
    }
}

#[test]
fn ids_are_unique_because_a_duplicate_silently_shadows_the_other() {
    let mut seen = std::collections::BTreeSet::new();
    for c in CHECKS {
        assert!(seen.insert(c.id), "duplicate check id: {}", c.id);
    }
}

#[test]
fn anything_that_cannot_be_undone_says_what_it_costs() {
    // The one exception is a pure deletion of things Windows itself calls
    // temporary — there is nothing to give up.
    for c in CHECKS {
        if matches!(c.undo, Undo::OneWay | Undo::Yours) && c.cost.is_none() {
            assert_eq!(
                c.id, "temp-files",
                "{} is not reversible and does not say what it costs",
                c.id
            );
        }
    }
}

#[test]
fn read_only_checks_change_nothing_and_so_need_no_cost_but_may_declare_one() {
    for c in read_only() {
        assert_eq!(c.undo, Undo::ReadOnly);
        assert_eq!(c.kind, Kind::Diagnose, "{} reads but is not a diagnostic", c.id);
    }
}

// ================= the gate is reversibility, not impressiveness =================

#[test]
fn one_way_and_judgement_calls_both_wait_for_you() {
    let gated = needs_approval();
    for id in ["component-cleanup", "winget-upgrade", "fast-startup", "dns-override"] {
        assert!(gated.iter().any(|c| c.id == id), "{id} should be gated");
    }
    for id in ["keyboard-repeat", "startup-items", "netstat"] {
        assert!(!gated.iter().any(|c| c.id == id), "{id} should not be gated");
    }
}

#[test]
fn a_security_downgrade_is_never_something_atlas_decides() {
    let mi = by_id("memory-integrity").expect("the trade-off should be listed, not hidden");
    assert_eq!(mi.undo, Undo::Yours);
    assert!(mi.cost.unwrap().contains("security"));
}

#[test]
fn firmware_is_outside_the_machine_atlas_can_reach() {
    let xmp = by_id("xmp").unwrap();
    assert_eq!(xmp.undo, Undo::Yours);
    assert!(xmp.how.to_lowercase().contains("bios"));
}

// ================= the no-list =================

#[test]
fn credentials_are_not_an_optimisation() {
    assert!(is_refused("netsh wlan show profile key=clear"));
    assert!(is_refused("NETSH WLAN SHOW PROFILE KEY=CLEAR"));
    // And it is not quietly in the catalogue under another name.
    for c in CHECKS {
        let h = c.how.to_lowercase();
        assert!(!h.contains("key=clear"), "{} would dump saved passwords", c.id);
        assert!(!h.contains("show profiles"), "{} enumerates saved networks", c.id);
    }
}

#[test]
fn the_myths_are_on_the_no_list_rather_than_in_the_catalogue() {
    // Deleting Prefetch is the most repeated bad advice in the whole corpus.
    assert!(is_refused("delete prefetch"));
    assert!(is_refused("registry cleaners"));
    for c in CHECKS {
        assert!(
            !c.how.to_lowercase().contains("prefetch"),
            "{} tells you to touch Prefetch",
            c.id
        );
    }
}

#[test]
fn every_refusal_gives_a_reason_so_it_can_be_argued_with() {
    assert!(NEVER.len() >= 4);
    for (what, why) in NEVER {
        assert!(!what.is_empty());
        assert!(why.len() > 40, "'{what}' is refused without a real reason");
    }
}

#[test]
fn nothing_on_the_no_list_survives_into_a_pass() {
    for c in first_pass() {
        assert!(!is_refused(c.how), "{} is on the no-list and still ran", c.id);
    }
}

// ================= the order of a first pass =================

#[test]
fn the_way_back_is_established_before_anything_is_changed() {
    let pass = first_pass();
    assert_eq!(
        pass[0].id, "system-restore",
        "restore points come first — they are what makes the rest safe to try"
    );
}

#[test]
fn a_first_pass_diagnoses_before_it_acts() {
    let pass = first_pass();
    let last_readonly = pass.iter().rposition(|c| c.undo == Undo::ReadOnly).unwrap();
    let first_change = pass
        .iter()
        .position(|c| c.undo == Undo::Reversible && c.id != "system-restore")
        .unwrap();
    assert!(last_readonly < first_change, "it changes things before it looks");
}

#[test]
fn a_first_pass_never_includes_anything_irreversible() {
    for c in first_pass() {
        assert!(
            matches!(c.undo, Undo::ReadOnly | Undo::Reversible),
            "{} is not reversible and has no business in an unattended pass",
            c.id
        );
    }
}

#[test]
fn restore_points_appear_once_despite_being_both_first_and_reversible() {
    let pass = first_pass();
    let n = pass.iter().filter(|c| c.id == "system-restore").count();
    assert_eq!(n, 1);
}

// ================= it hangs off the findings tune already makes =================

#[test]
fn a_finding_can_name_the_mechanism_that_fixes_it() {
    use atlas::tune::{mechanism_for, Finding, Fix};
    let f = Finding {
        id: "startup:something".into(),
        what: "costs 4s at boot".into(),
        frees_mb: 0,
        saves_boot_secs: 4.0,
        fix: Fix::Reversible { action: "disable".into() },
    };
    let m = mechanism_for(&f).expect("a startup finding should point at the startup mechanism");
    assert_eq!(m.id, "startup-items");
    assert_eq!(m.undo, Undo::Reversible);
}

#[test]
fn admin_is_declared_rather_than_discovered_when_it_fails() {
    for id in ["sfc", "dism-restore", "component-cleanup", "system-restore"] {
        assert!(by_id(id).unwrap().needs_admin, "{id} needs elevation and should say so");
    }
    for id in ["keyboard-repeat", "netstat", "clipboard-history"] {
        assert!(!by_id(id).unwrap().needs_admin, "{id} does not need elevation");
    }
}

#[test]
fn reversible_and_read_only_do_not_overlap() {
    for c in reversible() {
        assert_ne!(c.undo, Undo::ReadOnly);
    }
}
