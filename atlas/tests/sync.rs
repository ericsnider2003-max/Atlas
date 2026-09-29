use atlas::sync::{
    can_open, drifting, how_to_carry, make_bundle, merge, spoken, Bundle, Carry, Kind, Log,
    SyncConfig, What, BUNDLE_VERSION, WHAT_THE_PHONE_MISSES, WHY_THIS_WORKS,
};

const DAY: u64 = 86_400;

// ================= the phone is a real Atlas =================

#[test]
fn what_the_phone_cannot_do_is_about_hardware_not_permission() {
    let missing = Kind::Standalone.cannot();
    assert!(missing.contains(&"arrange your windows"));
    assert!(missing.contains(&"reach files on the laptop"));
    assert!(!missing.contains(&"hold a conversation"));
    assert!(WHAT_THE_PHONE_MISSES.contains("because they aren't there"));
    assert!(WHAT_THE_PHONE_MISSES.contains("is the same Atlas"));
}

// ================= appending can't conflict with appending =================

#[test]
fn two_devices_apart_for_months_merge_cleanly() {
    // Neither is overwriting the other — both are saying what happened.
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");

    for i in 0..5 {
        laptop.append(What::Captured { id: format!("l{i}"), text: "a note".into() }, i * DAY);
    }
    for i in 0..40 {
        phone.append(What::Captured { id: format!("p{i}"), text: "a thought".into() }, 100 * DAY + i * DAY);
    }

    let m = merge(&laptop.events, &phone.events, 200 * DAY);
    assert_eq!(m.clean, 45);
    assert!(m.clashes.is_empty());
    assert!(m.gap_days > 190);
}

#[test]
fn only_the_same_field_changed_on_both_sides_needs_you() {
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");
    laptop.append(What::Changed { id: "vps-audit".into(), field: "status".into(), to: "done".into() }, 100);
    phone.append(What::Changed { id: "vps-audit".into(), field: "status".into(), to: "parked".into() }, 200);

    let m = merge(&laptop.events, &phone.events, 300);
    assert_eq!(m.clashes.len(), 1);
    let c = &m.clashes[0];
    assert_eq!(c.subject, "vps-audit");
    assert_eq!(c.here, "done");
    assert_eq!(c.there, "parked");
    assert_eq!(c.later, "there", "the phone's was later");
}

#[test]
fn different_fields_on_the_same_thing_do_not_clash() {
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");
    laptop.append(What::Changed { id: "vps".into(), field: "status".into(), to: "done".into() }, 100);
    phone.append(What::Changed { id: "vps".into(), field: "note".into(), to: "ask Priya".into() }, 200);
    assert!(merge(&laptop.events, &phone.events, 300).clashes.is_empty());
}

#[test]
fn the_same_change_on_both_sides_is_not_a_clash() {
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");
    laptop.append(What::Changed { id: "vps".into(), field: "status".into(), to: "done".into() }, 100);
    phone.append(What::Changed { id: "vps".into(), field: "status".into(), to: "done".into() }, 200);
    assert!(merge(&laptop.events, &phone.events, 300).clashes.is_empty());
}

#[test]
fn captures_and_conversation_can_never_clash() {
    assert!(!What::Captured { id: "x".into(), text: "y".into() }.can_clash());
    assert!(!What::Said { text: "hello".into(), from_you: true }.can_clash());
    assert!(What::Changed { id: "x".into(), field: "y".into(), to: "z".into() }.can_clash());
}

#[test]
fn both_sides_merging_independently_reach_the_same_answer() {
    // Otherwise they drift again the moment they part.
    let mut a = Log::new("aaa");
    let mut b = Log::new("bbb");
    a.append(What::Captured { id: "1".into(), text: "x".into() }, 100);
    b.append(What::Captured { id: "2".into(), text: "y".into() }, 100);

    let from_a = merge(&a.events, &b.events, 200);
    let from_b = merge(&b.events, &a.events, 200);
    assert_eq!(from_a.clean, from_b.clean);
    assert_eq!(from_a.clashes.len(), from_b.clashes.len());
}

// ================= one file, anything can carry it =================

#[test]
fn a_bundle_is_one_self_describing_file() {
    let mut log = Log::new("phone");
    log.append(What::Captured { id: "1".into(), text: "a note".into() }, 100);
    log.append(What::Said { text: "what's outstanding".into(), from_you: true }, 200);

    let b = make_bundle(&log, "Eric's iPhone", 0, 300);
    assert_eq!(b.events.len(), 2);
    assert_eq!(b.from_name, "Eric's iPhone");
    assert_eq!(b.up_to_seq, 2);
    assert_eq!(b.version, BUNDLE_VERSION);
    assert!(can_open(&b).is_ok());
}

#[test]
fn only_what_the_other_side_has_not_seen_goes_in() {
    let mut log = Log::new("phone");
    for i in 0..5 {
        log.append(What::Captured { id: i.to_string(), text: "x".into() }, i);
    }
    assert_eq!(make_bundle(&log, "phone", 3, 100).events.len(), 2);
}

#[test]
fn a_bundle_from_a_newer_atlas_says_so_rather_than_half_reading_it() {
    let b = Bundle {
        from_device: "phone".into(),
        from_name: "iPhone".into(),
        made_at: 0,
        up_to_seq: 1,
        events: vec![],
        version: BUNDLE_VERSION + 1,
        belongs_to: "personal".into(),
    };
    assert!(can_open(&b).unwrap_err().contains("newer Atlas"));
}

#[test]
fn a_cable_needs_no_network_of_any_kind() {
    assert!(!Carry::Cable.needs_internet());
    assert!(Carry::Cable.plain().contains("no wifi, no service, no account"));
}

#[test]
fn airdrop_is_one_tap_with_nothing_online() {
    assert!(!Carry::AirDrop.needs_internet());
    assert!(Carry::AirDrop.plain().contains("phone to iPad, nothing online"));
}

#[test]
fn a_connection_is_preferred_over_a_cable_because_it_needs_nothing_from_you() {
    let (c, why) = how_to_carry(true, true, true, true);
    assert_eq!(c, Carry::SameNetwork);
    assert!(why.contains("nothing for you to do"));
    assert!(c.automatic());
}

#[test]
fn the_cable_is_used_when_there_is_no_network_at_all() {
    let (c, why) = how_to_carry(false, true, false, true);
    assert_eq!(c, Carry::Cable);
    assert!(why.contains("no network, but you're plugged in"));
}

#[test]
fn with_no_network_at_all_airdrop_is_offered_between_apple_devices() {
    let (c, why) = how_to_carry(false, false, false, true);
    assert_eq!(c, Carry::AirDrop);
    assert!(why.contains("the other side will take it in"));
}

#[test]
fn with_nothing_available_it_says_plug_it_in_rather_than_failing() {
    let (c, why) = how_to_carry(false, false, false, false);
    assert_eq!(c, Carry::Cable);
    assert!(why.contains("plug it in when you can"));
}

#[test]
fn the_cloud_route_is_the_one_for_devices_never_on_together() {
    assert!(Carry::CloudFolder.plain().contains("never on together"));
    assert!(Carry::CloudFolder.automatic());
}

#[test]
fn no_bundle_ever_carries_anything_that_opens_something_else() {
    // `bundles_carry_secrets` was a `#[serde(skip)]` bool pinned false that
    // nothing read, and this asserted it was false. Deleted 19 Sep 2026.
    //
    // What keeps the promise is the shape of `What`: five variants, none of
    // which can hold a credential. That matters more than usual here because
    // a bundle is written into a folder a cloud provider copies, in the clear
    // unless sealing is on -- `tests/settings_that_do_something_now.rs` pins
    // the variant list for that reason, and this is the same guarantee seen
    // from the module's own side.
    let src = std::fs::read_to_string("src/sync.rs").expect("src/sync.rs");
    let body = src.split("pub enum What").nth(1).expect("What is gone");
    let body = &body[..body.find("\n}").expect("unterminated What")];
    let carries: Vec<String> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//") && !l.is_empty() && *l != "{")
        .map(|l| l.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect())
        .filter(|v: &String| !v.is_empty())
        .collect();
    assert_eq!(carries, vec!["Captured", "Said", "Finished", "Changed", "Removed"]);

    // And an old config naming the removed key still loads.
    let parsed: SyncConfig =
        serde_yaml::from_str("enabled: true\nbundles_carry_secrets: true\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the section parsing");
}

// ================= what it says =================

#[test]
fn a_merge_says_how_much_and_how_far_back() {
    let m = merge(
        &[],
        &Log::new("phone").events.clone(),
        0,
    );
    assert!(spoken(&m, "your phone").contains("Nothing new"));
}

#[test]
fn a_clash_is_put_to_you_with_both_versions_and_which_is_newer() {
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");
    laptop.append(What::Changed { id: "the VPS audit".into(), field: "status".into(), to: "done".into() }, 100);
    phone.append(What::Changed { id: "the VPS audit".into(), field: "status".into(), to: "parked".into() }, 200);

    let said = spoken(&merge(&laptop.events, &phone.events, 300), "your phone");
    assert!(said.contains("\"done\" here"));
    assert!(said.contains("\"parked\" there"));
    assert!(said.contains("newer"));
    assert!(said.ends_with("Which?"));
}

#[test]
fn a_long_gap_is_mentioned_but_not_treated_as_a_problem() {
    let mut phone = Log::new("phone");
    phone.append(What::Captured { id: "1".into(), text: "x".into() }, 0);
    let said = spoken(&merge(&[], &phone.events, 200 * DAY), "your phone");
    assert!(said.contains("going back 200 days"));
}

#[test]
fn devices_apart_a_long_time_are_mentioned_without_alarm() {
    let cfg = SyncConfig { enabled: true, ..Default::default() };
    assert!(drifting(5, &cfg).is_none());
    let said = drifting(90, &cfg).unwrap();
    assert!(said.contains("Nothing's lost — it all merges"));
}

#[test]
fn the_reason_two_real_copies_is_fine_is_stated() {
    assert!(WHY_THIS_WORKS.contains("Appending can't conflict with appending"));
    assert!(WHY_THIS_WORKS.contains("there's only one of you"));
}


#[test]
fn a_bundle_is_not_a_file_you_have_to_manage() {
    // Tap share, pick Atlas, done.
    use atlas::sync::NOT_A_FILE_YOU_MANAGE;
    assert!(NOT_A_FILE_YOU_MANAGE.contains("never lands in Files or your Downloads folder"));
    assert!(NOT_A_FILE_YOU_MANAGE.contains("Sending twice is harmless"));
}

#[test]
fn sending_the_same_bundle_twice_costs_nothing() {
    // Sending again is the normal response to not being sure it landed.
    use atlas::sync::already_seen;
    let mut log = Log::new("phone");
    for i in 0..5 {
        log.append(What::Captured { id: i.to_string(), text: "x".into() }, i);
    }
    let (new, skipped) = already_seen(&log.events, &[("phone".into(), 3)]);
    assert_eq!(skipped, 3);
    assert_eq!(new, 2);
}

// ================= a delete is a claim about all of it =================
//
// `can_clash` has said since it was written that `Removed` can clash, and
// `merge` sent every removal to "additive, simply lands" — so a delete on
// one device racing an edit on the other merged clean and one side's work
// was silently lost. These hold the fix from both directions.

#[test]
fn removing_here_while_they_edited_it_is_a_clash_not_a_clean_merge() {
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");
    phone.append(What::Changed { id: "n1".into(), field: "text".into(), to: "keep this".into() }, 100);
    laptop.append(What::Removed { id: "n1".into() }, 200);

    let m = merge(&laptop.events, &phone.events, 300);
    assert_eq!(m.clashes.len(), 1, "a racing delete needs a decision, not a shrug");
    let c = &m.clashes[0];
    assert_eq!(c.subject, "n1");
    assert_eq!(c.here, "removed");
    assert_eq!(c.there, "keep this");
    assert_eq!(c.later, "here", "the delete came second");
}

#[test]
fn editing_after_the_other_side_removed_it_is_the_same_race_from_the_other_end() {
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");
    laptop.append(What::Removed { id: "n1".into() }, 100);
    phone.append(What::Changed { id: "n1".into(), field: "text".into(), to: "still here".into() }, 200);

    let m = merge(&laptop.events, &phone.events, 300);
    assert_eq!(m.clashes.len(), 1);
    let c = &m.clashes[0];
    assert_eq!(c.here, "removed");
    assert_eq!(c.there, "still here");
    assert_eq!(c.later, "there", "the edit came second");
}

#[test]
fn removing_something_nobody_else_touched_still_lands_clean() {
    // The fix must not make every delete a ceremony.
    let mut laptop = Log::new("laptop");
    let phone = Log::new("phone");
    laptop.append(What::Captured { id: "n1".into(), text: "a note".into() }, 50);
    laptop.append(What::Removed { id: "n1".into() }, 100);
    let m = merge(&laptop.events, &phone.events, 300);
    assert!(m.clashes.is_empty(), "one-sided removals are ordinary: {:?}", m.clashes);
}

#[test]
fn both_sides_removing_the_same_thing_agree_and_do_not_clash() {
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");
    laptop.append(What::Removed { id: "n1".into() }, 100);
    phone.append(What::Removed { id: "n1".into() }, 150);
    let m = merge(&laptop.events, &phone.events, 300);
    assert!(m.clashes.is_empty(), "two deletes are one answer: {:?}", m.clashes);
}

// ================= the clock survives a skewed phone =================
//
// The reason `hlc` exists. The phone edits a field *after* taking in the
// laptop's edit of the same field, but the phone's wall clock reads earlier
// (it woke from a drawer a few seconds behind). Ordered by raw wall time the
// merge would call the laptop's edit "later" and settle on it, silently losing
// the phone's newer decision. Stamped by the clock, the phone's edit sorts
// after the one it just learned — which is what "syncs, then continues" means.

#[test]
fn a_phone_edit_made_after_reconnect_wins_even_with_a_behind_clock() {
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");

    // Laptop makes a decision at wall-second 2000.
    laptop.append(
        What::Changed { id: "vps".into(), field: "status".into(), to: "done".into() },
        2_000,
    );

    // Phone reconnects, takes in the laptop's event (this advances its clock),
    // then — its own wall clock 8 seconds behind — changes its mind.
    let skew = phone.note_seen(&laptop.events, 1_992);
    assert!(skew.is_none(), "8 seconds is ordinary skew, not a wrong clock");
    phone.append(
        What::Changed { id: "vps".into(), field: "status".into(), to: "parked".into() },
        1_992,
    );

    let m = merge(&laptop.events, &phone.events, 3_000);
    assert_eq!(m.clashes.len(), 1);
    let c = &m.clashes[0];
    assert_eq!(
        c.later, "there",
        "the phone's edit was causally later and must win, despite the behind clock: {c:?}"
    );
    assert_eq!(c.there, "parked");
    // Ordered by raw `at` (1992 < 2000) this assertion would be "here"/"done".
}

#[test]
fn a_device_with_a_wildly_wrong_clock_is_flagged_but_loses_nothing() {
    // The spare laptop's clock is set years ahead. Taking its bundle in must
    // warn about the clock — and still keep every event it sent.
    let mut here = Log::new("here");
    let mut spare = Log::new("spare");
    here.append(What::Captured { id: "a".into(), text: "mine".into() }, 1_000);
    spare.append(What::Captured { id: "b".into(), text: "theirs".into() }, 1_000 + 8 * 365 * 86_400);

    let skew = here.note_seen(&spare.events, 1_000);
    assert!(skew.is_some(), "a clock years ahead should be flagged");
    assert!(skew.unwrap().plain("the spare").contains("nothing's lost"));

    let m = merge(&here.events, &spare.events, 1_000);
    assert_eq!(m.clean, 2, "both events are kept — the wrong clock loses nothing");
    assert!(m.clashes.is_empty());
}

#[test]
fn without_a_reconnect_the_clock_falls_back_to_wall_time_unchanged() {
    // Two independent devices that never met: no causal link, so plain time
    // order (via the stamp, which equals wall time for a first event) still
    // decides — the fix must not change the ordinary case.
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");
    laptop.append(What::Changed { id: "vps".into(), field: "status".into(), to: "done".into() }, 100);
    phone.append(What::Changed { id: "vps".into(), field: "status".into(), to: "parked".into() }, 200);
    let c = &merge(&laptop.events, &phone.events, 300).clashes[0];
    assert_eq!(c.later, "there", "later wall time still wins when there's no causal link");
}
