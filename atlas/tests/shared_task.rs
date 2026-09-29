//! One task type, `earned::Space`-generic. `for_space(Personal)` is Eric's
//! own to-do list; `for_space(Business(name))` is that business's shelf --
//! the same list, the same code, no second module.

use atlas::earned::Space;
use atlas::firewall::Firewall;
use atlas::shared_task::Tasks;

fn business(name: &str) -> Space {
    Space::Business(name.to_string())
}

#[test]
fn a_personal_task_shows_up_on_the_personal_shelf_and_nowhere_else() {
    let mut tasks = Tasks::default();
    tasks.add(Space::Personal, "renew the car registration", None, 100);
    assert_eq!(tasks.for_space(&Space::Personal).len(), 1);
    assert!(tasks.for_space(&business("Acme")).is_empty());
}

/// The whole point of building this as one Space-generic type: a business
/// task never has to go anywhere near the firewall, because it was never
/// personal material in the first place.
#[test]
fn a_business_native_task_never_touches_the_firewall_at_all() {
    let mut tasks = Tasks::default();
    tasks.add(business("Acme"), "send the Q3 invoice", Some(200), 100);
    assert_eq!(tasks.for_space(&business("Acme")).len(), 1);
    assert!(tasks.for_space(&Space::Personal).is_empty());
}

#[test]
fn completing_marks_done_without_moving_it_to_another_shelf() {
    let mut tasks = Tasks::default();
    let id = tasks.add(Space::Personal, "call the accountant", None, 100);
    assert!(tasks.complete(id));
    assert!(tasks.get(id).unwrap().done);
    assert_eq!(tasks.for_space(&Space::Personal).len(), 1, "still on the same shelf, just done");
}

#[test]
fn completing_something_already_done_or_nonexistent_does_nothing() {
    let mut tasks = Tasks::default();
    let id = tasks.add(Space::Personal, "x", None, 100);
    assert!(tasks.complete(id));
    assert!(!tasks.complete(id), "already done -- nothing changed");
    assert!(!tasks.complete(9999), "no such task");
}

// ================= the real crossing =================

/// The core of the whole design: sharing a personal task into a business
/// is *never* immediate. It is always held on the first attempt, exactly
/// like every other personal-sourced crossing in `firewall.rs`.
#[test]
fn sharing_a_personal_task_into_a_business_is_always_held_first() {
    let mut tasks = Tasks::default();
    let mut wall = Firewall::default();
    let id = tasks.add(Space::Personal, "draft the vendor contract", None, 100);

    let verdict = tasks.share_into_business(id, "Acme", &mut wall, 100);
    assert!(!verdict.allowed(), "personal never crosses on the first attempt");
    assert!(wall.waiting().len() == 1, "the firewall's own held record exists");

    // And nothing was actually copied yet -- Acme's shelf is still empty.
    assert!(tasks.for_space(&business("Acme")).is_empty());
    // The personal original is completely untouched.
    assert_eq!(tasks.for_space(&Space::Personal).len(), 1);
}

#[test]
fn releasing_the_hold_actually_completes_the_share() {
    let mut tasks = Tasks::default();
    let mut wall = Firewall::default();
    let id = tasks.add(Space::Personal, "draft the vendor contract", None, 100);

    let verdict = tasks.share_into_business(id, "Acme", &mut wall, 100);
    let held_id = match verdict {
        atlas::firewall::Crossing::Stopped { held, .. } => held,
        atlas::firewall::Crossing::Allowed => panic!("should have been held"),
    };

    wall.release(held_id).unwrap();
    let new_id = tasks.complete_release(held_id, &wall, 200).expect("release should complete the share");

    let on_shelf = tasks.for_space(&business("Acme"));
    assert_eq!(on_shelf.len(), 1);
    assert_eq!(on_shelf[0].id, new_id);
    assert_eq!(on_shelf[0].description, "draft the vendor contract");
    assert!(on_shelf[0].shared_from_personal);
    // And the personal original is still there too -- a copy, not a move.
    assert_eq!(tasks.for_space(&Space::Personal).len(), 1);
}

#[test]
fn completing_release_before_anything_is_actually_released_does_nothing() {
    let mut tasks = Tasks::default();
    let mut wall = Firewall::default();
    let id = tasks.add(Space::Personal, "x", None, 100);
    let held_id = match tasks.share_into_business(id, "Acme", &mut wall, 100) {
        atlas::firewall::Crossing::Stopped { held, .. } => held,
        _ => panic!(),
    };

    // Not released yet.
    assert!(tasks.complete_release(held_id, &wall, 200).is_none());
    assert!(tasks.for_space(&business("Acme")).is_empty());
}

#[test]
fn dropping_a_hold_instead_of_releasing_it_leaves_the_share_permanently_incomplete() {
    let mut tasks = Tasks::default();
    let mut wall = Firewall::default();
    let id = tasks.add(Space::Personal, "x", None, 100);
    let held_id = match tasks.share_into_business(id, "Acme", &mut wall, 100) {
        atlas::firewall::Crossing::Stopped { held, .. } => held,
        _ => panic!(),
    };

    wall.forget(held_id).unwrap();
    assert!(tasks.complete_release(held_id, &wall, 200).is_none(), "there's nothing left to release");
    assert!(tasks.for_space(&business("Acme")).is_empty());
}

#[test]
fn releasing_the_same_hold_twice_only_delivers_once() {
    let mut tasks = Tasks::default();
    let mut wall = Firewall::default();
    let id = tasks.add(Space::Personal, "x", None, 100);
    let held_id = match tasks.share_into_business(id, "Acme", &mut wall, 100) {
        atlas::firewall::Crossing::Stopped { held, .. } => held,
        _ => panic!(),
    };
    wall.release(held_id).unwrap();
    assert!(tasks.complete_release(held_id, &wall, 200).is_some());
    // The pending share was consumed the first time.
    assert!(tasks.complete_release(held_id, &wall, 200).is_none());
    assert_eq!(tasks.for_space(&business("Acme")).len(), 1, "only one copy, not two");
}

#[test]
fn sharing_something_that_is_already_a_business_s_own_task_is_refused() {
    let mut tasks = Tasks::default();
    let mut wall = Firewall::default();
    let id = tasks.add(business("Acme"), "already theirs", None, 100);
    let verdict = tasks.share_into_business(id, "Widgets Inc", &mut wall, 100);
    assert!(!verdict.allowed());
    assert!(wall.waiting().is_empty(), "this was refused before it ever reached the firewall");
}

#[test]
fn sharing_a_task_that_does_not_exist_is_refused_without_touching_the_firewall() {
    let mut tasks = Tasks::default();
    let mut wall = Firewall::default();
    let verdict = tasks.share_into_business(9999, "Acme", &mut wall, 100);
    assert!(!verdict.allowed());
    assert!(wall.waiting().is_empty());
}

#[test]
fn shared_tasks_survive_save_and_load() {
    let dir = std::env::temp_dir().join("atlas-shared-task-roundtrip");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = atlas::store::Store::new(&dir);

    let mut tasks = Tasks::default();
    tasks.add(Space::Personal, "x", None, 100);
    tasks.add(business("Acme"), "y", None, 100);
    tasks.save(&store).unwrap();

    let loaded = Tasks::load(&store);
    assert_eq!(loaded.for_space(&Space::Personal).len(), 1);
    assert_eq!(loaded.for_space(&business("Acme")).len(), 1);
}
