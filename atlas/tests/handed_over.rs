//! Handing the machine to someone else, and getting it back.
//!
//! The design is one sentence: **entering narrows, leaving grants, so they
//! are not guarded the same way.** Anyone may say "this isn't mine right
//! now". Only the vault passphrase says "it's mine again".
//!
//! What these tests are actually defending is the asymmetry. Every plausible
//! wrong turn here collapses it:
//!
//! * guarding the way *in* — which locks you out of the one control you need
//!   at the exact moment a stranger has your laptop;
//! * letting a sensor take it back out — a face is a photograph and a voice
//!   is a recording, and both are cheaper to fake than a passphrase;
//! * accepting "the vault is open" as proof, when on a vault that has never
//!   had a passphrase the first unlock *sets* one and opens for anybody;
//! * building the restriction out of action names nothing produces, which is
//!   the failure `tests/profiles.rs` was already caught committing.

mod common; // `common::source_of`: a module's source wherever its files live

use atlas::handover::{refusal, refuses, Handover, Hint, Stance};
use atlas::store::Store;
use atlas::vault::{Kind, Vault, VaultConfig};
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-handover-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

const PHRASE: &str = "a long enough passphrase";

/// A vault with the passphrase genuinely set, written to disk, and then read
/// back — which is the only arrangement under which an unlock proves
/// anything.
fn a_real_vault(state: &Store) -> Vault {
    let mut v = Vault::default();
    v.open(PHRASE, 100, &VaultConfig::default()).expect("the first unlock sets the passphrase");
    v.put("something", Kind::ApiKey, "sk-test", 100).unwrap();
    v.save(state).expect("the vault should write");
    Vault::load(state)
}

// --- the asymmetry ----------------------------------------------------------

#[test]
fn anyone_can_hand_it_over_and_it_survives_a_restart() {
    let state = Store::new(tmp("enter"));
    let mut h = Handover::load(&state);
    assert!(!h.stance.handed_over(), "it started handed over");

    // No passphrase, no profile switch, no gate of any kind. Your friend can
    // do this themselves, which is the point of it.
    h.hand_over("Sam is borrowing it", 1000);
    h.save(&state).unwrap();

    let again = Handover::load(&state);
    assert_eq!(again.stance, Stance::HandedOver, "it forgot on the way to disk");
    assert_eq!(again.note, "Sam is borrowing it");
}

#[test]
fn getting_it_back_costs_the_passphrase_and_nothing_else_will_do() {
    let state = Store::new(tmp("leave"));
    let vault = a_real_vault(&state);

    let mut h = Handover::default();
    h.hand_over("", 1000);

    // Locked: refused, and told how.
    let mut locked = vault.clone();
    locked.lock();
    let why = h.take_back(&locked, 1100).unwrap_err();
    assert!(why.contains("Unlock the vault"), "{why}");
    assert!(h.stance.handed_over(), "it came back without the passphrase");

    // Wrong passphrase: the vault refuses to open at all, so there is nothing
    // to take it back with. This is the case that was live until the check
    // value existed -- `open` returned `Ok` for any twelve characters.
    let mut wrong = vault.clone();
    assert!(
        wrong.open("the wrong passphrase entirely", 1100, &VaultConfig::default()).is_err(),
        "a wrong passphrase opened the vault"
    );
    assert!(h.take_back(&wrong, 1100).is_err(), "a wrong passphrase took the machine back");
    assert!(h.stance.handed_over());

    // Right passphrase: back.
    let mut right = vault.clone();
    right.open(PHRASE, 1200, &VaultConfig::default()).unwrap();
    let said = h.take_back(&right, 1200).unwrap();
    assert!(said.contains("Yours again"), "{said}");
    assert!(!h.stance.handed_over());
}

#[test]
fn a_vault_that_has_never_had_a_passphrase_proves_nothing() {
    // The hole that would have made the whole thing decoration, and it is not
    // theoretical: a fresh install has no vault file, so a stranger holding
    // the laptop types twelve characters, the first unlock *sets* that as the
    // passphrase, and `state() == Open` is true. Openness is not proof;
    // having passed a check is.
    let state = Store::new(tmp("virgin"));
    let mut fresh = Vault::load(&state);
    fresh.open("twelve characters at least", 100, &VaultConfig::default()).unwrap();
    assert_eq!(fresh.state(), atlas::vault::State::Open, "it did not open");
    assert!(!fresh.proved_it(), "setting a passphrase was reported as passing a check");

    let mut h = Handover::default();
    h.hand_over("", 100);
    let why = h.take_back(&fresh, 200).unwrap_err();
    assert!(why.contains("no passphrase"), "{why}");
    assert!(h.stance.handed_over(), "an unverifiable unlock took the machine back");
}

// --- sensors may speak, and may not act -------------------------------------

#[test]
fn nothing_a_sensor_produces_can_change_the_stance() {
    // `Hint` is the only thing an unfamiliar voice or face is allowed to
    // reach, and the only thing it can produce is a sentence. Asserted here
    // as well as in the module's own tests, because this is the rule the
    // design rests on and it is one careless method away from being false.
    let mut h = Handover::default();
    let before = h.clone();
    for hint in [Hint::VoiceUnfamiliar, Hint::FaceUnfamiliar, Hint::None] {
        let _ = hint.offer();
    }
    assert_eq!(h, before, "a hint changed the handover");

    h.hand_over("", 100);
    let handed = h.clone();
    for hint in [Hint::VoiceUnfamiliar, Hint::FaceUnfamiliar, Hint::None] {
        let _ = hint.offer();
    }
    assert_eq!(h, handed, "a hint lifted a handover");
}

#[test]
fn no_file_outside_the_handover_module_may_set_the_stance() {
    // The rule stated as a grep, because the way this gets broken is not by
    // someone arguing for it -- it is by one convenient line somewhere that
    // sets `Stance::Yours` from a face match, in a file nobody associates
    // with security.
    //
    // `main.rs` and `daemon.rs` are allowed to *read* a stance. Neither may
    // name a variant, which is what constructing one takes.
    let mut offenders = Vec::new();
    // Every file under src/, subfolders included (27 Sep 2026: was the top
    // level only, which would have stopped seeing daemon.rs's code the day it
    // is split into src/daemon/*.rs; no subfolder file names a stance today).
    for (module, text) in crate::common::source_file_set() {
        if module == "handover" {
            continue;
        }
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if code.contains("Stance::HandedOver") || code.contains("Stance::Yours") {
                offenders.push(format!("src/{module}.rs:{}", n + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a stance is being set outside handover.rs: {}",
        offenders.join(", ")
    );
}

// --- the gate actually consults it ------------------------------------------

#[test]
fn the_one_gate_every_action_passes_through_asks_about_the_handover() {
    // `gate_with_identity` lives in `src/main.rs`, which is a binary and
    // cannot be called from here. So this reads the function's own body --
    // not the whole file, which would pass on a mention in a comment
    // somewhere else in 5,000 lines, and that exact mistake has already been
    // made seven times in this tree.
    //
    // Position matters as much as presence. The check has to come *before*
    // `policy::gate`, because `policy::gate` can return `Ok` on a standing
    // grant -- a grant recorded when the machine was yours, which is the
    // thing in doubt.
    let text = crate::common::source_of("main");
    let body = function_body(&text, "fn gate_with_identity(");

    let refuses = body
        .find("handover::refuses")
        .unwrap_or_else(|| panic!("the gate no longer asks whether a handover refuses this"));
    let handed = body
        .find("stance.handed_over()")
        .expect("the gate no longer checks whether it is handed over");
    let gate = body.find("policy::gate").expect("gate_with_identity no longer calls policy::gate");

    assert!(refuses < gate, "the handover check sits below policy::gate, where a standing grant has already returned Ok");
    assert!(handed < gate, "the stance is read below policy::gate");

    // From the install's own state. Reading it from `roots::store()` would
    // mean a guest profile carries its own handover file, and switching
    // profiles -- which passes through no gate at all -- would clear it.
    assert!(
        body.contains("install_state()"),
        "the gate reads the handover from somewhere other than the install's own state"
    );
}

/// The text between a function's opening brace and its matching close.
fn function_body<'a>(text: &'a str, signature: &str) -> &'a str {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} is gone or renamed"));
    let open = start + text[start..].find('{').expect("no body");
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    for i in open..bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open..=i];
                }
            }
            _ => {}
        }
    }
    panic!("{signature} has no closing brace");
}

// --- the restriction is made of real action names ---------------------------

#[test]
fn everything_a_handover_refuses_is_something_atlas_can_actually_be_asked() {
    // The exact failure `tests/profiles.rs` was caught committing: it
    // asserted a guest could not "publish" or "send_email", names
    // `session::kind_of` never produces, so the test passed green while
    // nothing at all was protected.
    //
    // Checked through the public API rather than against a copy of the list,
    // so a name added to `NEVER_AS_A_GUEST` is covered the day it is added.
    let every_kind = every_kind_atlas_can_produce();
    assert!(every_kind.len() > 50, "the parse found {} kinds -- it broke", every_kind.len());
    for name in atlas::profiles::NEVER_AS_A_GUEST {
        assert!(
            every_kind.contains(&name.to_string()),
            "a handover refuses `{name}`, which no intent ever produces -- so it refuses nothing"
        );
        assert!(refuses(name), "the list and the check disagree about {name}");
    }
    assert!(!refuses("say"), "answering a question is refused while handed over");
    assert!(!refuses("whats_there"), "looking at the screen is refused while handed over");

    // And what gets said when one of them arrives. Folded in here rather
    // than kept as its own test, because on its own it asserted nothing but
    // wording -- `tests/retrospective.rs` catches exactly that shape, and it
    // caught this one.
    for name in atlas::profiles::NEVER_AS_A_GUEST {
        let said = refusal(name);
        assert!(!said.contains('_'), "an identifier reached the sentence: {said}");
        assert!(said.contains("vault"), "the refusal does not name the way back: {said}");
        assert!(
            said.contains(&name.replace('_', " ")),
            "the refusal does not say what it refused: {said}"
        );
    }
}


/// Every name `session::kind_of` can return, read from its match arms.
///
/// Deliberately not "does the file contain this string anywhere". That is
/// what `tests/profiles.rs` does, and it would pass for a name that appears
/// only in a comment or in some unrelated string in the same file -- the
/// tree's own standing caution about guards that match substrings. This
/// parses the arms of the one function whose output `Role::may` and
/// `handover::refuses` are called with.
fn every_kind_atlas_can_produce() -> Vec<String> {
    let text = std::fs::read_to_string("src/session.rs").expect("src/session.rs");
    let body = text
        .split_once("pub fn kind_of(")
        .expect("kind_of is gone or renamed")
        .1;
    let mut out = Vec::new();
    for line in body.lines() {
        let code = line.split("//").next().unwrap_or("");
        let Some((_, rest)) = code.split_once("=> \"") else { continue };
        let Some((name, _)) = rest.split_once('"') else { continue };
        out.push(name.to_string());
        if code.contains("Intent::Unknown") {
            break;
        }
    }
    out
}

// --- two lists, and every action on exactly one of them ---------------------

/// What a stranger holding your laptop is welcome to do.
///
/// Written out by hand, and that is the point of it. Everything
/// `session::kind_of` can produce has to appear here or on one of the two
/// refusal lists, so a new intent cannot land on the permissive side by
/// default -- which is how `draft_post` would have arrived, silently allowed,
/// the day somebody added posting.
const ORDINARY: &[&str] = &[
    // Round 11: the market's calendar is public, and translating what a
    // guest hands you uses nothing of yours but the model.
    "market_day",
    "translate",
    // Running the machine in front of you.
    "workspace_on",
    "open_app",
    "close_app",
    "focus_app",
    "set_mode",
    "machine_health",
    // A self-check reports whether the core works on this machine and a count
    // of what's ready -- a health check, the same shape as "machine_health"
    // beside it, welcome to whoever is holding the laptop.
    "self_check",
    // A shakedown walks the never-run capabilities and reports how each would
    // be verified -- a commissioning report about the machine, not a read of
    // your data, so the same as "self_check" and "machine_health" beside it.
    "shakedown",
    "which_model",
    "recommend",
    // Troubleshooting a symptom reads only Atlas's shipped procedures -- the
    // same generic how-to knowledge behind "how do I do X", none of the
    // owner's own state -- so a stranger holding the laptop is welcome to it,
    // beside "machine_health" and "recommend".
    "diagnose",
    // Walking through a shipped how-to reads only Atlas's generic procedures
    // -- the same knowledge behind "how do I do X", none of the owner's own
    // state -- so a stranger holding the laptop is welcome to it, beside
    // "diagnose".
    "walk_through",
    // Looking, listening, talking.
    "view_display",
    "capture_webcam",
    "whats_there",
    "whats_this",
    "say",
    "ask",
    "capabilities",
    "gestures",
    "dictate",
    "use_clipboard",
    "unknown",
    // Looking something up. It reaches the internet rather than into you.
    "research",
    "ask_the_room",
    // Showing what would happen, without it happening.
    "rehearse",
    "dismiss_panel",
    // Tools that work on something the person in front of the laptop hands
    // over -- a design to critique, a scene to animate, a snippet to explain, a
    // passage to reword. None of them read your state or act as you, so they
    // sit beside "research" and "rehearse": anyone holding the laptop may ask.
    "design_review",
    "animate",
    "scene3d",
    "explain_code",
    "plain_change",
    // Stopping and starting.
    "pause",
    "resume",
    // The way out. Refusing this one would close the door -- see
    // `tests/saying_youre_back.rs`.
    "take_it_back",
    // And the way *in*, which is the same argument from the other side:
    // anyone may say it, including the person holding the laptop, and saying
    // it again while already handed over is answered rather than refused.
    // Refusing it would mean a guest who wants Atlas narrowed cannot ask for
    // that, which is backwards.
    "hand_over",
    // The time and the date: anyone at the laptop may ask what time it is.
    "clock",
];

#[test]
fn every_action_atlas_has_is_decided_one_way_or_the_other() {
    // The guard that makes the two lists maintainable rather than a pair of
    // things to remember. It does not compare them to each other -- that
    // would only prove they are different -- it compares their union to the
    // complete set of actions, and requires the leftovers to have been
    // written down as deliberately allowed.
    let every = every_kind_atlas_can_produce();
    assert!(every.len() > 50, "the parse found {} kinds -- it broke", every.len());

    let mut undecided = Vec::new();
    for kind in &every {
        let blocked = atlas::profiles::NEVER_AS_A_GUEST.contains(&kind.as_str());
        let owners = atlas::profiles::THE_OWNERS_OWN.contains(&kind.as_str());
        let ordinary = ORDINARY.contains(&kind.as_str());
        match (blocked, owners, ordinary) {
            (false, false, false) => undecided.push(kind.clone()),
            (true, true, _) | (true, _, true) | (_, true, true) => {
                panic!("{kind} is on more than one list, so which one applies is a coin toss")
            }
            _ => {}
        }
    }
    assert!(
        undecided.is_empty(),
        "these actions are neither refused nor deliberately allowed while handed \
         over, so they are allowed by omission: {}",
        undecided.join(", ")
    );

    // And the lists are made of real names, checked the same way the first
    // one already was.
    for name in atlas::profiles::THE_OWNERS_OWN {
        assert!(
            every.contains(&name.to_string()),
            "a handover refuses `{name}`, which no intent ever produces -- so it refuses nothing"
        );
        assert!(refuses(name), "the list and the check disagree about {name}");
    }
}

#[test]
fn a_stranger_cannot_read_back_what_atlas_holds_for_you() {
    // The hole this list closed. `NEVER_AS_A_GUEST` is entirely about acting
    // *as* you -- posting, mailing, signing in -- and a handover checked
    // against it alone stopped your friend sending an email as you and let
    // them ask what was on your mind today. Your outstanding list, your
    // brief, your notes index and your history were all one sentence away.
    for reading in ["outstanding", "ready", "what_i_have", "history", "why", "queued"] {
        assert!(refuses(reading), "somebody else holding your laptop can ask for {reading}");
    }
    // And writing into what Atlas remembers about you.
    for writing in ["capture", "got_it_wrong", "apply_lesson", "address_as"] {
        assert!(refuses(writing), "a stranger can teach your Atlas by saying {writing}");
    }
}

#[test]
fn a_guest_profile_is_not_held_to_the_second_list() {
    // The reason it is two lists rather than one longer one. A guest profile
    // has its own state directory, so their outstanding list is theirs and
    // starts empty -- refusing to read it back would leave them with an
    // assistant that will not tell them what they themselves said a minute
    // ago. The handover is the case with no separate directory, and that
    // difference is the whole of it.
    use atlas::profiles::Role;
    // Except what only the owner may even ask: Eric, 25 Sep 2026, on the
    // envelope — "no one else can ask Atlas", not even from a profile of
    // their own.
    for theirs in atlas::profiles::THE_OWNERS_OWN.iter().filter(|a| !atlas::profiles::ONLY_YOU_MAY_ASK.contains(a)) {
        assert!(
            Role::Guest.may(theirs),
            "a guest profile cannot {theirs} in their own state directory"
        );
    }
    // Acting as you is refused in both situations, which is the half that
    // was always right.
    for acting in atlas::profiles::NEVER_AS_A_GUEST {
        assert!(!Role::Guest.may(acting), "a guest profile could {acting}");
        assert!(refuses(acting), "a handover allows {acting}");
    }
}
