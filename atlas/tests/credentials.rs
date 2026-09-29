use atlas::credentials::{
    all, misplaced, needs_you_awake, never_held, spoken, written, Kept, Opens, CAN_IT_LOG_IN,
    SESSION_VS_PASSWORD,
};
use atlas::mail::{credential_source, Account, MailConfig};

#[test]
fn atlas_has_no_code_that_signs_into_anything() {
    // The answer to the direct question, and the reason the rest holds.
    assert!(CAN_IT_LOG_IN.starts_with("No"));
    assert!(CAN_IT_LOG_IN.contains("no password store"));
    assert!(CAN_IT_LOG_IN.contains("the session you already made"));
}

#[test]
fn passwords_are_on_the_list_of_things_never_held() {
    let never = never_held();
    assert!(never.iter().any(|(w, _)| w.contains("account passwords")));
    assert!(never.iter().any(|(w, _)| w.contains("banking credentials")));
    assert!(never.iter().any(|(w, _)| w.contains("master password")));
    assert!(never.iter().any(|(w, _)| *w == "recovery codes"));
}

#[test]
fn recovery_codes_are_counted_never_stored() {
    let never = never_held();
    let (_, why) = never.iter().find(|(w, _)| *w == "recovery codes").unwrap();
    assert!(why.contains("never the codes themselves"));
}

#[test]
fn a_session_and_a_password_are_a_real_distinction_not_a_technicality() {
    assert!(SESSION_VS_PASSWORD.contains("one browser profile on one machine"));
    assert!(SESSION_VS_PASSWORD.contains("it expires"));
    assert!(SESSION_VS_PASSWORD.contains("A password is the account itself"));
}

#[test]
fn nothing_that_opens_anything_is_allowed_in_a_config_file() {
    // The rule that got broken quietly, which is why it's a test and not a
    // convention.
    assert!(misplaced().is_empty(), "found: {:?}", misplaced());
    assert!(!Kept::PlainConfig.safe_for(Opens::OneThing));
    assert!(!Kept::PlainConfig.safe_for(Opens::Everything));
    assert!(Kept::PlainConfig.safe_for(Opens::Nothing));
}

#[test]
fn the_mail_password_comes_from_the_vault_and_config_names_only_the_entry() {
    let account = Account { password_from_vault: "mail app password".into(), ..Default::default() };
    assert_eq!(credential_source(&account).unwrap(), "mail app password");
    // The password itself is never a field.
    let yaml = serde_yaml::to_string(&account.password_from_vault).unwrap();
    assert!(!yaml.contains("hunter2"));
    // Every account has its own entry, so `MailConfig` itself never
    // declares a password of its own -- there's nowhere on the type for
    // one to be.
    assert!(!MailConfig::default().accounts.iter().any(|a| !a.password_from_vault.is_empty()));
}

#[test]
fn a_config_with_no_vault_entry_is_refused_rather_than_falling_back() {
    let account = Account { password_from_vault: String::new(), ..Default::default() };
    let e = credential_source(&account).unwrap_err();
    assert!(e.contains("won't take a password in config"));
}

#[test]
fn every_credential_says_how_to_take_it_away() {
    for c in all() {
        assert!(!c.revoke.is_empty(), "{} has no revoke path", c.name);
    }
}

#[test]
fn the_mail_credential_is_narrow_and_says_so() {
    let mail = all().into_iter().find(|c| c.name == "mail app password").unwrap();
    assert_eq!(mail.opens, Opens::OneThing);
    assert!(mail.revoke.contains("nothing else changes"));
}

#[test]
fn the_browser_session_is_the_thing_that_replaces_a_password() {
    let s = all().into_iter().find(|c| c.name == "browser session").unwrap();
    assert_eq!(s.kept, Kept::YourBrowserSession);
    assert!(s.revoke.contains("it has its own"), "a separate profile");
    assert!(!s.needed_overnight);
}

#[test]
fn anything_needed_while_you_sleep_is_surfaced_as_a_trade() {
    // Rather than discovered at 3am when overnight work stops.
    let awake = needs_you_awake();
    assert!(awake.iter().any(|c| c.name == "hosted model key"));
}

#[test]
fn asking_what_it_holds_gets_a_short_honest_answer() {
    let said = spoken();
    assert!(said.contains("No passwords"));
    assert!(said.contains("no code that signs into anything"));
    assert!(said.contains("locked vault"));
}

#[test]
fn the_written_version_lists_both_what_it_has_and_what_it_does_not() {
    let w = written();
    assert!(w.contains("What I hold"));
    assert!(w.contains("What I don't"));
    assert!(w.contains("revoke:"));
}
