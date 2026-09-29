use atlas::hub::{access_page, route, works_without_voice, Page};
use atlas::signin::{
    hub_rows, registered_domain, spoken, Access, Allowed, Refused, SignInConfig,
    IF_YOU_HAVE_ONE, SIGNIN_IS_NOT_SETTINGS, WHY_SAFER,
};

const DAY: u64 = 86_400;

fn cfg() -> SignInConfig {
    SignInConfig { enabled: true, ..Default::default() }
}

fn granted() -> Access {
    let mut a = Access::default();
    a.grant("linkedin.com", "eric", "LinkedIn", Allowed::SignInAndUse, "linkedin login", 0);
    a.grant("accounts.google.com", "personal", "Google", Allowed::SignIn, "google login", 0);
    a
}

// ================= the domain is the whole thing =================

#[test]
fn subdomains_of_the_same_site_are_the_same_account() {
    assert_eq!(registered_domain("accounts.google.com"), "google.com");
    assert_eq!(registered_domain("mail.google.com"), "google.com");
    assert_eq!(registered_domain("www.linkedin.com"), "linkedin.com");
}

#[test]
fn a_lookalike_domain_is_not_the_site_and_this_is_where_it_lives_or_dies() {
    // google.com.evil.co is evil.co.
    assert_eq!(registered_domain("google.com.evil.co"), "evil.co");
    assert_eq!(registered_domain("paypa1-secure.com"), "paypa1-secure.com");
    assert_ne!(registered_domain("linked1n.com"), "linkedin.com");
}

#[test]
fn two_part_country_domains_are_not_split_wrongly() {
    assert_eq!(registered_domain("accounts.hsbc.co.uk"), "hsbc.co.uk");
    assert_eq!(registered_domain("www.gov.uk"), "www.gov.uk");
}

#[test]
fn it_will_not_fill_into_a_lookalike_and_says_why() {
    let a = granted();
    let e = a.may_fill("linked1n.com", true, true, true, true, &cfg()).unwrap_err();
    match &e {
        Refused::NotGranted(d) => assert_eq!(d, "linked1n.com"),
        o => panic!("{o:?}"),
    }
    let said = e.say();
    assert!(said.contains("linked1n.com"));
}

#[test]
fn the_wrong_domain_message_names_what_a_phishing_page_counts_on() {
    let r = Refused::WrongDomain {
        expected: "paypal.com".into(),
        got: "paypa1-secure.com".into(),
    };
    assert!(r.say().contains("exactly what a phishing page counts on you not noticing"));
}

#[test]
fn filling_is_safer_than_typing_and_the_reason_is_stated() {
    // The part that surprises people.
    assert!(WHY_SAFER.contains("won't put your password into a lookalike"));
    assert!(WHY_SAFER.contains("the domain is the one thing a phishing page can't fake"));
}

// ================= what it will and won't do =================

#[test]
fn a_granted_site_on_a_real_login_page_fills() {
    let a = granted();
    let g = a.may_fill("www.linkedin.com", true, true, true, true, &cfg()).unwrap();
    assert_eq!(g.name, "LinkedIn");
    assert_eq!(g.vault_entry, "linkedin login");
}

#[test]
fn a_page_that_is_not_a_login_form_gets_nothing() {
    let a = granted();
    assert!(matches!(
        a.may_fill("linkedin.com", false, true, true, true, &cfg()),
        Err(Refused::NotALoginPage)
    ));
}

#[test]
fn a_page_reached_by_following_a_link_gets_nothing() {
    // Which is how you land on a fake login page in the first place.
    let a = granted();
    let e = a.may_fill("linkedin.com", true, false, true, true, &cfg()).unwrap_err();
    assert!(e.say().contains("following a link rather than typing the address"));
}

#[test]
fn a_locked_vault_means_no_sign_in() {
    let a = granted();
    assert!(matches!(
        a.may_fill("linkedin.com", true, true, false, true, &cfg()),
        Err(Refused::Locked)
    ));
}

#[test]
fn nothing_signs_in_while_you_are_away_unless_you_allowed_that_site() {
    // The risk of a stored credential concentrates in what can use it while
    // nobody's watching.
    let a = granted();
    assert!(matches!(
        a.may_fill("linkedin.com", true, true, true, false, &cfg()),
        Err(Refused::YouAreNotHere)
    ));
    assert!(!SignInConfig::default().allow_while_away);
}

#[test]
fn signing_in_never_means_changing_security_settings() {
    // Otherwise "log me into my bank" quietly means "and you can move money".
    // `may_change_security` was a `#[serde(skip)]` bool pinned false that
    // nothing read; deleted 19 Sep 2026. The guarantee is `Allowed`, which is
    // the only authority a `Grant` carries and has no variant that can
    // express changing a security setting.
    let src = std::fs::read_to_string("src/signin.rs").expect("src/signin.rs");
    let body = src.split("pub enum Allowed").nth(1).expect("Allowed is gone");
    let body = &body[..body.find("\n}").expect("unterminated Allowed")];
    let variants: Vec<String> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//") && !l.is_empty() && *l != "{")
        .map(|l| l.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect())
        .filter(|v: &String| !v.is_empty())
        .collect();
    assert_eq!(
        variants,
        vec!["Nothing", "SignIn", "SignInAndUse"],
        "`Allowed` gained a variant -- if it can say \"and change the settings\", \"log me \
         into my bank\" quietly means \"and you can move money\""
    );

    let parsed: SignInConfig =
        serde_yaml::from_str("enabled: true\nmay_change_security: true\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the section parsing");
    assert!(SIGNIN_IS_NOT_SETTINGS.contains("holding one never implies the other"));
}

#[test]
fn filling_only_on_pages_it_navigated_to_is_not_configurable() {
    let parsed: SignInConfig =
        serde_yaml::from_str("enabled: true\nonly_on_pages_it_navigated_to: false\n").unwrap();
    assert!(parsed.only_on_pages_it_navigated_to);
}

// ================= taking it back =================

#[test]
fn access_to_one_site_can_be_taken_away_without_touching_the_rest() {
    let mut a = granted();
    assert!(a.revoke("linkedin.com"));
    assert!(a.find("linkedin.com").is_none());
    assert!(a.find("google.com").is_some(), "the others are untouched");
}

#[test]
fn revoking_works_from_any_form_of_the_address() {
    let mut a = granted();
    assert!(a.revoke("https://www.linkedin.com/feed".replace("https://", "").split('/').next().unwrap()));
}

#[test]
fn everything_can_be_taken_at_once() {
    let mut a = granted();
    assert_eq!(a.revoke_all(), 2);
    assert!(a.grants.is_empty());
}

#[test]
fn the_access_page_is_reachable_and_works_when_atlas_is_broken() {
    // If something's wrong, taking access away is exactly what you'd want to
    // do — needing a working assistant to do it would be the wrong way round.
    assert_eq!(route("/hub/access"), Some(Page::Access));
    assert!(works_without_voice(Page::Access));
}

#[test]
fn the_page_has_a_button_per_site_and_one_for_all_of_it() {
    let rows = hub_rows(&granted(), DAY);
    let html = access_page(&rows);
    assert!(html.contains("LinkedIn"));
    assert!(html.contains("Take it away"));
    assert!(html.contains("Take all of it away"));
    assert!(html.contains("doesn't change your password"), "and says what it does");
}

#[test]
fn the_page_says_what_each_site_is_for_and_when_it_was_last_used() {
    let mut a = granted();
    a.note_use("linkedin.com", "eric", "https://linkedin.com/login", true, true, DAY);
    let rows = hub_rows(&a, DAY);
    let linkedin = rows.iter().find(|(n, _, _, _)| n.starts_with("LinkedIn")).unwrap();
    assert!(linkedin.0.contains("eric"), "the account is named: {}", linkedin.0);
    assert!(linkedin.1.contains("sign in and act as you"));
    assert!(linkedin.1.contains("used today"));
}

#[test]
fn a_credential_you_have_not_used_lately_is_not_dropped_or_flagged() {
    // Not using a site for four months is normal, and being away is normal.
    // Dropping it is exactly the wrong thing to do to someone about to need
    // it after a long absence.
    let rows = hub_rows(&granted(), 200 * DAY);
    assert!(rows.iter().all(|(_, _, _, needs_attention)| !*needs_attention));
    assert_eq!(granted().grants.len(), 2, "still there");
}

#[test]
fn time_you_were_away_does_not_count_against_a_credential() {
    let a = granted();
    // 200 days quiet, but 150 of them you were gone.
    assert!(a.quiet(200 * DAY, 90, 150).is_empty());
    assert_eq!(a.quiet(200 * DAY, 90, 0).len(), 2, "without that, it would show");
}

// ================= every use is recorded =================

#[test]
fn every_sign_in_is_logged_whether_it_worked_or_not() {
    let mut a = granted();
    a.note_use("linkedin.com", "eric", "https://linkedin.com/login", true, true, 100);
    a.note_use("linkedin.com", "eric", "https://linkedin.com/login", false, true, 200);
    assert_eq!(a.log.len(), 2);
    assert_eq!(a.find("linkedin.com").unwrap().times_used, 2);
}

#[test]
fn the_page_it_filled_on_is_recorded_so_a_wrong_one_is_visible_afterwards() {
    let mut a = granted();
    a.note_use("linkedin.com", "eric", "https://linkedin.com/uas/login", true, true, 100);
    assert!(a.log[0].page.contains("uas/login"));
    assert!(a.log[0].you_were_here);
}

#[test]
fn failures_this_week_are_surfaced_because_that_is_how_you_notice() {
    let mut a = granted();
    a.note_use("linkedin.com", "eric", "x", false, true, 100);
    let said = spoken(&a, 200);
    assert!(said.contains("1 sign-ins failed this week"));
}

#[test]
fn what_atlas_mentions_is_what_stopped_working_not_what_is_old() {
    let mut a = granted();
    a.note_use("linkedin.com", "eric", "x", true, true, 0);
    a.note_use("linkedin.com", "eric", "x", false, true, 100);
    a.note_use("linkedin.com", "eric", "x", false, true, 200);
    let said = spoken(&a, 300);
    assert!(said.contains("password looks like it changed"));
    assert!(said.contains("LinkedIn"));
}

#[test]
fn nothing_granted_says_how_to_grant_something() {
    assert!(spoken(&Access::default(), 0).contains("give me access to"));
}

#[test]
fn a_real_password_manager_is_pointed_at_if_you_have_one() {
    assert!(IF_YOU_HAVE_ONE.contains("keep doing that"));
    assert!(IF_YOU_HAVE_ONE.contains("survive this laptop dying"));
}

#[test]
fn signing_in_is_off_until_you_turn_it_on() {
    assert!(!SignInConfig::default().enabled);
    assert!(matches!(
        granted().may_fill("linkedin.com", true, true, true, true, &SignInConfig::default()),
        Err(Refused::Disabled)
    ));
}

// ================= more than one account on a site =================

use atlas::signin::{probably_changed, Which};

fn two_googles() -> Access {
    let mut a = Access::default();
    a.grant("google.com", "personal", "Google", Allowed::SignIn, "google personal", 0);
    a.grant("google.com", "work", "Google", Allowed::SignIn, "google work", 0);
    a
}

#[test]
fn two_accounts_on_the_same_site_are_two_grants_not_one_overwriting_the_other() {
    let a = two_googles();
    assert_eq!(a.accounts_on("mail.google.com").len(), 2);
    assert_eq!(a.accounts_on_all_sites(), 1, "one site, two logins");
}

#[test]
fn with_two_accounts_atlas_asks_rather_than_picking() {
    // Signing into the wrong account is hard to see and hard to undo.
    let a = two_googles();
    match a.which_account("google.com", None) {
        Which::Several(accounts) => {
            assert_eq!(accounts.len(), 2);
            let q = Which::Several(accounts).ask("Google").unwrap();
            assert!(q.contains("Which one?"));
            assert!(q.contains("personal"));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_hint_in_what_you_said_settles_it_without_asking() {
    let a = two_googles();
    assert_eq!(a.which_account("google.com", Some("work")), Which::One("work".into()));
    assert_eq!(a.which_account("google.com", Some("personal")), Which::One("personal".into()));
}

#[test]
fn with_one_account_there_is_nothing_to_ask() {
    let a = granted();
    assert_eq!(a.which_account("linkedin.com", None), Which::One("eric".into()));
    assert!(a.which_account("linkedin.com", None).ask("LinkedIn").is_none());
}

#[test]
fn each_account_is_found_and_revoked_separately() {
    let mut a = two_googles();
    assert!(a.find_account("google.com", "work").is_some());
    a.grants.retain(|g| g.account != "work");
    assert!(a.find_account("google.com", "personal").is_some());
}

// ================= a password changed somewhere else =================

#[test]
fn two_failures_on_something_that_worked_means_the_password_changed() {
    // Not that the site is down.
    let mut a = granted();
    a.note_use("linkedin.com", "eric", "x", true, true, 0);
    a.note_use("linkedin.com", "eric", "x", true, true, 10);
    a.note_use("linkedin.com", "eric", "x", false, true, 100);
    assert!(a.superseded().is_empty(), "one failure is just a failure");
    a.note_use("linkedin.com", "eric", "x", false, true, 200);
    assert_eq!(a.superseded().len(), 1);
}

#[test]
fn atlas_names_what_probably_happened_rather_than_saying_sign_in_failed() {
    let mut a = granted();
    for i in 0..4 {
        a.note_use("linkedin.com", "eric", "x", i < 2, true, i * 100);
    }
    let g = a.superseded()[0];
    let said = probably_changed(g);
    assert!(said.contains("usually means the password was changed somewhere else"));
    assert!(said.contains("Want to give me the new one?"));
}

#[test]
fn pointing_it_at_the_new_password_clears_the_flag() {
    let mut a = granted();
    for i in 0..4 {
        a.note_use("linkedin.com", "eric", "x", i < 2, true, i * 100);
    }
    assert!(a.superseded_by("linkedin.com", "eric", "linkedin login v2"));
    assert!(a.superseded().is_empty());
    assert_eq!(a.find_account("linkedin.com", "eric").unwrap().vault_entry, "linkedin login v2");
}

#[test]
fn a_working_sign_in_clears_a_run_of_failures() {
    let mut a = granted();
    a.note_use("linkedin.com", "eric", "x", false, true, 0);
    a.note_use("linkedin.com", "eric", "x", true, true, 100);
    assert_eq!(a.find_account("linkedin.com", "eric").unwrap().failures_in_a_row, 0);
}

// ================= your bank, and only as far as reading =================

#[test]
fn atlas_can_sign_you_into_your_bank() {
    // The old rule refused every form submit on a financial site, so Atlas
    // could fill your password and then not press the button. That isn't
    // safety, it's a broken feature.
    use atlas::finance::{allowed, FinanceConfig, PageAction};
    let money = FinanceConfig::default();
    let login = PageAction::SubmitSignIn {
        fields: vec!["username".into(), "password".into()],
    };
    assert!(allowed("https://chase.com/login", &login, &money).ok());
}

#[test]
fn a_form_with_an_amount_in_it_is_not_a_sign_in_form_whatever_the_page_calls_it() {
    use atlas::finance::{allowed, FinanceConfig, PageAction};
    let money = FinanceConfig::default();
    let sneaky = PageAction::SubmitSignIn {
        fields: vec!["username".into(), "amount".into()],
    };
    let v = allowed("https://chase.com/x", &sneaky, &money);
    assert!(!v.ok());
    assert!(format!("{v:?}").contains("isn't a sign-in form"));
}

#[test]
fn everything_else_on_a_banking_page_is_still_refused() {
    use atlas::finance::{allowed, FinanceConfig, PageAction};
    let money = FinanceConfig::default();
    assert!(!allowed("https://chase.com/x", &PageAction::Submit, &money).ok());
    assert!(!allowed("https://chase.com/x", &PageAction::Click("Transfer".into()), &money).ok());
    // A broker is a bank with faster ways to lose money.
    assert!(!allowed("https://ibkr.com/x", &PageAction::Click("Place order".into()), &money).ok());
    assert!(!allowed("https://ibkr.com/x", &PageAction::Click("Close position".into()), &money).ok());
}

#[test]
fn reading_your_statements_still_works() {
    use atlas::finance::{allowed, FinanceConfig, PageAction};
    let money = FinanceConfig::default();
    for a in [PageAction::Read, PageAction::Scroll, PageAction::Download] {
        assert!(allowed("https://chase.com/statements", &a, &money).ok(), "{a:?}");
    }
}

#[test]
fn what_atlas_will_and_will_not_do_on_a_bank_is_said_in_one_place() {
    use atlas::signin::BANKS_ARE_SITES_TOO;
    assert!(BANKS_ARE_SITES_TOO.contains("sign you into your bank and your broker"));
    assert!(BANKS_ARE_SITES_TOO.contains("that's where it stops"));
    assert!(BANKS_ARE_SITES_TOO.contains("on a bank or a broker"));
}
