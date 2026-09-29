//! NOTE (vault gate): these exercise the vault's *mechanics* — round-tripping,
//! locking, listing, staleness. They used credential kinds as convenient
//! sample data. Since `put` now refuses credential kinds while `REAL_CRYPTO`
//! is false, the samples are `Kind::Note`, which changes nothing about what is
//! being tested. The one place the *kind* was the subject is unchanged.
//! `tests/vault_crypto.rs` covers the refusal itself.

use atlas::vault::{
    Kind, State, Vault, VaultConfig, NOT_A_PASSWORD_MANAGER, WHAT_THIS_DOES_NOT_STOP,
    WHILE_YOU_SLEEP,
};
use atlas::walkthrough::{
    before_turning_off, to_prepare, to_turn_off, where_2fa_lives, WalkConfig, FAMILY_ACCESS,
};

fn cfg() -> VaultConfig {
    VaultConfig { enabled: true, ..Default::default() }
}

// ================= a stolen laptop is worth nothing =================

#[test]
fn a_sealed_vault_gives_up_nothing_even_to_atlas() {
    // If Atlas could read it whenever it liked, the key would be on the
    // machine — a locked box with the key taped to the lid.
    let mut v = Vault::default();
    assert_eq!(v.state(), State::Sealed);
    assert!(v.get("anything", 0).unwrap_err().contains("locked"));
    assert!(v.put("x", Kind::TotpSeed, "secret", 0).is_err());
}

#[test]
fn it_opens_with_a_passphrase_and_round_trips() {
    let mut v = Vault::default();
    v.open("correct horse battery staple", 0, &cfg()).unwrap();
    assert_eq!(v.state(), State::Open);
    v.put("gmail totp", Kind::Note, "JBSWY3DPEHPK3PXP", 0).unwrap();
    assert_eq!(v.get("gmail totp", 0).unwrap(), "JBSWY3DPEHPK3PXP");
}

#[test]
fn a_short_passphrase_is_refused_with_the_reason() {
    // Length beats complexity, and a short one makes the whole thing
    // decorative.
    let mut v = Vault::default();
    let e = v.open("hunter2", 0, &cfg()).unwrap_err();
    assert!(e.contains("use a sentence, not a word"));
    assert_eq!(v.state(), State::Sealed);
}

#[test]
fn locking_wipes_the_key_rather_than_dropping_it() {
    let mut v = Vault::default();
    v.open("correct horse battery staple", 0, &cfg()).unwrap();
    v.put("x", Kind::Note, "value", 0).unwrap();
    v.lock();
    assert_eq!(v.state(), State::Sealed);
    assert!(v.get("x", 0).is_err());
}

#[test]
fn it_re_locks_itself_after_a_while_and_when_the_screen_locks() {
    let mut v = Vault::default();
    v.open("correct horse battery staple", 0, &cfg()).unwrap();
    assert!(!v.should_lock(60, false, &cfg()));
    assert!(v.should_lock(60, true, &cfg()), "screen locked");
    assert!(v.should_lock(20 * 60, false, &cfg()), "15 minutes idle");
}

#[test]
fn nothing_in_it_is_reachable_while_you_are_asleep() {
    // If Atlas could open it unattended, so could whoever took the laptop
    // unattended.
    assert!(WHILE_YOU_SLEEP.contains("so could anyone who took the laptop"));
}

#[test]
fn the_key_never_being_written_down_is_not_configurable() {
    // `key_stays_in_memory` was a `#[serde(skip)]` bool that nothing read --
    // and its default helper was named `never` and returned `true`, which is
    // the kind of thing that stays harmless only while nobody reads it.
    // Deleted 19 Sep 2026.
    //
    // The guarantee is that the derived key is `#[serde(skip)]` on the vault
    // itself, so it is not in what gets written, and `lock` overwrites it
    // before dropping it.
    let src = std::fs::read_to_string("src/vault.rs").expect("src/vault.rs");
    let at = src.find("    key: Option<Vec<u8>>").expect("the vault's key field is gone");
    let before = &src[at.saturating_sub(200)..at];
    assert!(
        before.contains("#[serde(skip)]"),
        "the derived key is being serialised -- that is the vault writing its own key down"
    );
    let locking = src.split("pub fn lock").nth(1).expect("lock is gone");
    assert!(
        locking[..300].contains("*b = 0"),
        "`lock` no longer overwrites the key before dropping it"
    );

    // And an old config naming the removed key still loads.
    let parsed: VaultConfig =
        serde_yaml::from_str("enabled: true\nkey_stays_in_memory: false\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the section parsing");
}

#[test]
fn what_is_in_it_is_readable_while_sealed_because_that_gives_nothing_away() {
    // Knowing you have a recovery code for something is useful.
    let mut v = Vault::default();
    v.open("correct horse battery staple", 0, &cfg()).unwrap();
    v.put("chase recovery", Kind::Note, "1234 5678", 0).unwrap();
    v.lock();
    let list = v.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].0, "chase recovery");
    assert_eq!(list[0].1, Kind::Note);
}

#[test]
fn a_config_that_still_names_the_old_cost_knob_loads_and_is_told_about_it() {
    // `kdf_rounds` was deleted on 19 Sep 2026. It had been ignored since the
    // vault moved to Argon2id -- whose cost is memory and passes, not a round
    // count -- and the shipped file carried it with "a second to unlock,
    // years to attack" written beside a number nothing read. That comment was
    // the problem: a person reading their own config came away believing a
    // security property they did not have.
    //
    // Deleting a field is only safe if an existing file still loads, and only
    // honest if the person is told rather than left to assume.
    let old = "enabled: true\nlock_after_mins: 5\nkdf_rounds: 600000\n";
    let parsed: VaultConfig = serde_yaml::from_str(old).expect("an old file must still load");
    assert!(parsed.enabled, "a key with no field must not stop the section parsing");
    assert_eq!(parsed.lock_after_mins, 5, "and must not take the rest of it down");

    // And `atlas doctor` says so, by the nested key rather than by writing
    // off the whole `vault:` section, which is read and works.
    let said = atlas::config::settings_that_do_nothing(
        "vault:\n  enabled: true\n  kdf_rounds: 600000\n",
    );
    let named: Vec<&str> = said.iter().map(|(k, _)| *k).collect();
    assert!(named.contains(&"vault.kdf_rounds"), "{named:?}");
    assert!(!named.contains(&"vault"), "the section itself is alive: {named:?}");
    let why = said.iter().find(|(k, _)| *k == "vault.kdf_rounds").unwrap().1;
    assert!(why.contains("Argon2id"), "it doesn't say what replaced it: {why}");

    // A file that never set it hears nothing about it.
    let quiet = atlas::config::settings_that_do_nothing("vault:\n  enabled: true\n");
    assert!(!quiet.iter().any(|(k, _)| *k == "vault.kdf_rounds"));

    // The shipped file no longer carries the claim.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(
        !raw.contains("years to attack"),
        "the shipped config still tells you the number buys you something"
    );
}

#[test]
fn it_says_what_it_is_not() {
    assert!(NOT_A_PASSWORD_MANAGER.contains("not everything you own"));
    assert!(NOT_A_PASSWORD_MANAGER.contains("survives this laptop dying"));
}

#[test]
fn it_says_what_encryption_does_not_protect_you_from() {
    assert!(WHAT_THIS_DOES_NOT_STOP.contains("open and in someone else's hands"));
}

#[test]
fn something_untouched_for_a_long_time_can_be_noticed() {
    let mut v = Vault::default();
    v.open("correct horse battery staple", 0, &cfg()).unwrap();
    v.put("old key", Kind::Note, "x", 0).unwrap();
    assert_eq!(v.stale(400 * 86_400, 365), vec!["old key"]);
}

// ================= opening the page for you =================

#[test]
fn atlas_knows_exactly_where_the_setting_is_not_just_the_site() {
    // "Google security settings" is something you can find yourself. The
    // reason this is tedious is that every site buries it differently.
    let (url, where_to) = where_2fa_lives("Gmail").unwrap();
    assert!(url.contains("two-step-verification"), "the exact page, not the homepage");
    assert!(where_to.contains("2-Step Verification"));

    let (url, _) = where_2fa_lives("TikTok").unwrap();
    assert!(url.contains("tiktok.com/setting"));
}

#[test]
fn a_site_it_does_not_know_returns_nothing_rather_than_a_guess() {
    assert!(where_2fa_lives("some regional credit union").is_none());
}

#[test]
fn the_walk_says_which_stop_you_are_on_and_what_to_do() {
    let w = to_turn_off(&["Gmail".into(), "TikTok".into(), "GitHub".into()]);
    let said = w.say();
    assert!(said.starts_with("1 of 3: Gmail"));
    assert!(said.contains("Turn it off there"));
    assert!(said.contains("say next when you're done"));
}

#[test]
fn it_tells_you_what_to_have_in_hand_before_you_start() {
    let w = to_turn_off(&["Gmail".into()]);
    assert!(w.say().contains("You'll need your password"));
}

#[test]
fn the_warnings_are_specific_and_actually_differ() {
    let github = to_turn_off(&["GitHub".into()]);
    assert!(github.say().contains("drop your SSH keys"));

    let apple = to_turn_off(&["Apple".into()]);
    assert!(apple.say().contains("mostly doesn't allow this any more"));

    let bank = to_turn_off(&["Chase Bank".into()]);
    // Not in the known list, so no stop — but the warning logic is there for
    // ones that are.
    assert_eq!(bank.stops.len(), 0);
}

#[test]
fn gmail_gets_the_one_warning_that_matters_most() {
    let w = to_turn_off(&["Gmail".into()]);
    assert!(w.say().contains("the account everything else resets through"));
}

#[test]
fn you_can_skip_one_and_come_back_because_half_finished_is_normal() {
    let mut w = to_turn_off(&["Gmail".into(), "TikTok".into(), "GitHub".into()]);
    w.next();
    w.skip();
    assert_eq!(w.progress(), (1, 3));
    assert_eq!(w.remaining().len(), 2);
    assert!(w.remaining().iter().any(|s| s.site == "TikTok"));
}

#[test]
fn finishing_says_so() {
    let mut w = to_turn_off(&["Gmail".into()]);
    w.next();
    assert!(w.say().contains("all 1 done"));
}

#[test]
fn atlas_makes_its_case_once_then_offers_both_runs() {
    let said = before_turning_off(6);
    assert!(said.contains("6 accounts, and I'll open every one"));
    assert!(said.contains("then I'll stop saying it"), "once, not repeatedly");
    assert!(said.contains("having it off wouldn't have helped"));
    assert!(said.contains("say prepare and I'll do that run instead"));
}

#[test]
fn the_prepare_run_exists_and_does_the_other_thing() {
    let w = to_prepare(&["Gmail".into(), "TikTok".into()]);
    assert_eq!(w.stops.len(), 2);
    assert!(w.say().contains("authenticator app"));
    assert!(w.say().contains("print the recovery codes"));
}

#[test]
fn family_help_is_pointed_at_the_proper_mechanism_for_it() {
    // Which is what his situation actually points at.
    assert!(FAMILY_ACCESS.contains("Inactive Account Manager"));
    assert!(FAMILY_ACCESS.contains("Legacy Contact"));
    assert!(FAMILY_ACCESS.contains("without the account sitting open for months"));
}

#[test]
fn atlas_presses_the_control_only_when_you_have_let_it() {
    // Eric, 24 Sep 2026: yes, Atlas may make a security change itself after
    // the read-back and his yes. Off until he turns it on.
    assert!(!WalkConfig::default().atlas_clicks);
    let parsed: WalkConfig =
        serde_yaml::from_str("enabled: true\natlas_clicks: true\n").unwrap();
    assert!(parsed.atlas_clicks);
}
