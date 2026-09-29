//! The signup flow.
//!
//! Two rules carry the weight here, and both are tested from the outside
//! rather than trusted: a run that meets payment ends, and a run that meets a
//! human check waits. Everything else is detail.

use atlas::enrol::*;
use atlas::intent::Intent;
use atlas::policy::{self, Decision};
use atlas::categories::{self, Category};
use atlas::finance::FinanceConfig;

fn page(text: &str) -> PageSignals {
    PageSignals {
        domain: "example.com".into(),
        text: text.into(),
        ..Default::default()
    }
}

fn on() -> EnrolConfig {
    EnrolConfig { enabled: true, ..Default::default() }
}

// --- the payment rule -------------------------------------------------------

#[test]
fn a_page_wanting_a_card_ends_the_run() {
    for t in [
        "Enter your card number to continue",
        "Billing address",
        "CVV",
        "Start free trial — $9.99 per month",
        "Add a card to finish signing up",
        "Continue with PayPal",
    ] {
        match read(&page(t), "example.com", &on()) {
            Verdict::Abandon(Stopped::WantsPayment(_)) => {}
            other => panic!("{t:?} should have ended the run, got {other:?}"),
        }
    }
}

#[test]
fn an_abandoned_run_cannot_be_resumed() {
    // If you can talk it back to life, the payment rule is a suggestion.
    let mut e = Enrolment::new("example.com", "eric", 0);
    e.step(&page("enter your card number"), &on());
    assert!(matches!(e.phase, Phase::Abandoned(_)));
    assert!(e.resume().is_err(), "an abandoned run resumed");
}

#[test]
fn payment_beats_a_human_check_on_the_same_page() {
    // A page with both must stop, not wait — waiting implies it continues.
    let p = PageSignals {
        domain: "example.com".into(),
        text: "verify you are human. then enter your card number.".into(),
        ..Default::default()
    };
    assert!(matches!(
        read(&p, "example.com", &on()),
        Verdict::Abandon(Stopped::WantsPayment(_))
    ));
}

// --- the human check rule ---------------------------------------------------

#[test]
fn a_human_check_hands_over_and_the_run_survives() {
    let mut e = Enrolment::new("example.com", "eric", 0);
    let v = e.step(&page("Please confirm you are not a robot"), &on());
    assert_eq!(v, Verdict::HandOver(Stopped::CheckingYouAreHuman));
    assert!(e.is_waiting());

    e.resume().expect("you cleared it, so it carries on");
    assert_eq!(e.phase, Phase::Filling);
}

#[test]
fn every_shape_of_human_check_stops_it() {
    for t in [
        "Are you a robot?",
        "I'm not a robot",
        "Verify you're human",
        "Human verification required",
        "Select all images with a bus",
        "Press and hold to continue",
        "Security check",
    ] {
        assert!(
            matches!(read(&page(t), "example.com", &on()), Verdict::HandOver(Stopped::CheckingYouAreHuman)),
            "{t:?} did not hand over"
        );
    }
}

#[test]
fn a_challenge_frame_hands_over_even_with_no_matching_text() {
    // The wording changes constantly; the frame is the reliable signal.
    let p = PageSignals {
        domain: "example.com".into(),
        text: "Create your account".into(),
        third_party_challenge_frame: true,
        ..Default::default()
    };
    assert!(matches!(
        read(&p, "example.com", &on()),
        Verdict::HandOver(Stopped::CheckingYouAreHuman)
    ));
}

#[test]
fn a_human_check_is_never_final() {
    assert!(!Stopped::CheckingYouAreHuman.is_final());
    assert!(Stopped::WantsPayment("cvv".into()).is_final());
}

// --- the rest of the guards -------------------------------------------------

#[test]
fn the_domain_changing_partway_stops_it() {
    let mut e = Enrolment::new("example.com", "eric", 0);
    let p = PageSignals { domain: "example-secure.net".into(), text: "nearly done".into(), ..Default::default() };
    assert!(matches!(e.step(&p, &on()), Verdict::Abandon(Stopped::DomainChanged { .. })));
}

#[test]
fn a_code_sent_elsewhere_waits_for_you() {
    assert!(matches!(
        read(&page("We sent a verification code to your email"), "example.com", &on()),
        Verdict::HandOver(Stopped::NeedsACodeFromElsewhere(_))
    ));
}

#[test]
fn identity_documents_end_the_run_by_default() {
    assert!(matches!(
        read(&page("Upload your passport"), "example.com", &on()),
        Verdict::Abandon(Stopped::WantsIdentityDocuments(_))
    ));
}

#[test]
fn the_never_list_holds_before_anything_is_touched() {
    let cfg = on();
    assert!(Enrolment::permitted("coinbase.com", &cfg, &FinanceConfig::default()).is_err());
    assert!(Enrolment::permitted("interactivebrokers.com", &cfg, &FinanceConfig::default()).is_err());
    assert!(Enrolment::permitted("example.com", &cfg, &FinanceConfig::default()).is_ok());
}

#[test]
fn it_is_off_until_you_turn_it_on() {
    assert!(!EnrolConfig::default().enabled);
    assert!(Enrolment::permitted("example.com", &EnrolConfig::default(), &FinanceConfig::default()).is_err());
}

#[test]
fn an_ordinary_signup_page_just_carries_on() {
    let p = PageSignals {
        domain: "example.com".into(),
        text: "Create your account. Choose a username.".into(),
        fields: vec!["username".into(), "email".into(), "password".into()],
        buttons: vec!["Sign up".into()],
        ..Default::default()
    };
    assert_eq!(read(&p, "example.com", &on()), Verdict::Carry);
}

// --- the password -----------------------------------------------------------

#[test]
fn the_password_follows_the_stated_ruleset() {
    let policy = PasswordPolicy::default();
    let entropy: Vec<u8> = (0u8..=255).collect();
    let pw = policy.make(&entropy).unwrap();
    assert_eq!(pw.len(), 20);
    for bad in ['l', 'I', 'O', '0', '1', 'o'] {
        assert!(!pw.contains(bad), "ambiguous character {bad} in {pw}");
    }
}

#[test]
fn a_short_password_is_refused_rather_than_padded() {
    let p = PasswordPolicy { length: 8, ..Default::default() };
    assert!(p.make(&[7u8; 64]).is_err());
}

#[test]
fn generation_refuses_rather_than_reusing_thin_entropy() {
    // Wrapping a short buffer would silently make passwords repeat.
    let p = PasswordPolicy::default();
    assert!(p.make(&[1, 2, 3]).is_err());
}

#[test]
fn both_password_styles_exist_because_you_chose_one() {
    assert_eq!(PasswordPolicy::default().style, PasswordStyle::PerSite);
    let shared = PasswordPolicy { style: PasswordStyle::Shared, ..Default::default() };
    assert_eq!(shared.style, PasswordStyle::Shared);
}

// --- what it tells you ------------------------------------------------------

#[test]
fn it_says_the_username_and_never_the_password() {
    let mut e = Enrolment::new("example.com", "eric_s", 0);
    e.finish();
    let said = e.spoken_result(&on());
    assert!(said.contains("eric_s"), "you were not told the username: {said}");
    assert!(said.contains("vault"));
    let pw = PasswordPolicy::default().make(&(0u8..=255).collect::<Vec<u8>>()).unwrap();
    assert!(!said.contains(&pw), "the password was said out loud");
}

#[test]
fn the_record_never_carries_the_password_itself() {
    // This struct reaches the journal. The journal is not the vault.
    let e = Enrolment::new("example.com", "eric_s", 0);
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("vault_entry"));
    assert!(!json.to_lowercase().contains("\"password\""));
}

#[test]
fn every_stop_is_kept_so_it_can_account_for_itself() {
    let mut e = Enrolment::new("example.com", "eric", 0);
    e.step(&page("are you a robot"), &on());
    e.resume().unwrap();
    e.step(&page("we sent you a code"), &on());
    assert_eq!(e.stops.len(), 2);
    assert!(!e.stops[0].spoken().is_empty());
}

// --- the domain, recovered from speech -------------------------------------

#[test]
fn the_domain_survives_the_phrase_matcher() {
    assert_eq!(domain_from("sign me up for example.com").as_deref(), Some("example.com"));
    assert_eq!(domain_from("make an account on www.news.ycombinator.com").as_deref(), Some("news.ycombinator.com"));
    assert_eq!(domain_from("sign me up"), None);
}

// --- the fifth category -----------------------------------------------------

#[test]
fn account_creation_is_its_own_category() {
    let i = Intent::CreateAccount("example.com".into());
    assert_eq!(categories::category_of(&i), Category::AgreementExternal);
    assert_eq!(
        Category::AgreementExternal.default_decision(),
        Decision::RequireApproval
    );
}

#[test]
fn no_amount_of_history_makes_a_signup_automatic() {
    let mut mem = atlas::memory::Memory::default();
    let i = Intent::CreateAccount("example.com".into());
    let kind = atlas::session::kind_of(&i);
    for _ in 0..500 {
        mem.record_approval(kind, true, None);
    }
    assert_eq!(
        policy::classify_with_policy(&i, &mem, &Default::default()),
        Decision::RequireApproval,
        "five hundred approvals taught it to sign you up unasked"
    );
}

#[test]
fn the_consent_line_says_what_you_are_agreeing_to() {
    let line = categories::consent_line(Category::AgreementExternal, "Signing up on example.com");
    assert!(line.to_lowercase().contains("terms"), "got: {line}");
    assert!(line.to_lowercase().contains("as you"), "got: {line}");
}

// ===========================================================================
// Brokers: never created, freely signed into, never touched
// ===========================================================================

use atlas::finance::{self, PageAction, Verdict as FinVerdict};
use atlas::vault::Kind;

fn money() -> FinanceConfig {
    FinanceConfig::default()
}

#[test]
fn no_account_is_ever_made_on_a_broker() {
    let cfg = on();
    for d in [
        "interactivebrokers.com", "ibkr.com", "tastytrade.com", "tradestation.com",
        "thinkorswim.com", "etrade.com", "webull.com", "robinhood.com",
        "vanguard.com", "ninjatrader.com", "tradovate.com", "schwab.com",
        "fidelity.com", "kraken.com", "binance.com", "gemini.com", "coinbase.com",
    ] {
        assert!(
            Enrolment::permitted(d, &cfg, &money()).is_err(),
            "{d} would have been signed up for"
        );
    }
}

#[test]
fn no_account_is_made_on_a_bank_either() {
    let cfg = on();
    for d in ["chase.com", "bankofamerica.com", "amex.com", "paypal.com", "venmo.com"] {
        assert!(Enrolment::permitted(d, &cfg, &money()).is_err(), "{d}");
    }
}

#[test]
fn the_financial_block_beats_the_enabled_flag() {
    // Turning enrolment on must not turn this off.
    let mut cfg = on();
    cfg.enabled = true;
    cfg.never_on.clear();
    assert!(Enrolment::permitted("schwab.com", &cfg, &money()).is_err());
}

#[test]
fn one_list_governs_both_modules() {
    // Two lists would drift, and the day they drifted is the day it opens a
    // brokerage account.
    let cfg = on();
    for d in money().financial_domains.iter() {
        assert!(
            Enrolment::permitted(d, &cfg, &money()).is_err(),
            "{d} is financial to finance.rs but not to enrol.rs"
        );
    }
}

#[test]
fn a_broker_can_be_signed_into_and_read() {
    let m = money();
    for a in [
        PageAction::Navigate("https://tastytrade.com/positions".into()),
        PageAction::Read,
        PageAction::Scroll,
        PageAction::Download,
    ] {
        assert!(
            finance::allowed("https://tastytrade.com/x", &a, &m).ok(),
            "{a:?} should be fine on a broker"
        );
    }
}

#[test]
fn scrolling_a_statement_is_always_allowed() {
    // Reading a statement means reaching the bottom of it.
    let m = money();
    for d in ["https://chase.com/activity", "https://ibkr.com/portfolio"] {
        assert!(finance::allowed(d, &PageAction::Scroll, &m).ok(), "{d}");
    }
}

#[test]
fn it_never_clicks_anything_that_could_move_money() {
    let m = money();
    for label in [
        "Transfer", "Send money", "Pay now", "Wire", "Withdraw", "Deposit",
        "Buy", "Sell", "Place order", "Review order", "Market order",
        "Limit order", "Close position", "Liquidate", "Flatten", "Roll",
        "Exercise", "Swap", "Stake", "Enable trading", "Fund account",
        "Link bank", "Add payee", "Confirm", "Submit", "Authorize",
    ] {
        let v = finance::allowed(
            "https://tastytrade.com/x",
            &PageAction::Click(label.into()),
            &m,
        );
        assert!(!v.ok(), "it would have clicked {label:?}");
    }
}

#[test]
fn a_refusal_says_which_button_it_refused() {
    let m = money();
    match finance::allowed("https://schwab.com/x", &PageAction::Click("Sell All".into()), &m) {
        FinVerdict::Refused(why) => assert!(why.contains("Sell All"), "got: {why}"),
        FinVerdict::Allowed => panic!("it would have sold"),
    }
    // Behaviour, not just wording: the named button is actually blocked.
    assert!(
        !finance::allowed("https://schwab.com/x", &PageAction::Click("Sell All".into()), &m).ok(),
        "Sell All was named in the refusal but still allowed through"
    );
}

#[test]
fn ordinary_reading_clicks_still_work() {
    let m = money();
    for label in ["Statements", "Next page", "Show more", "March 2026", "Export CSV"] {
        assert!(
            finance::allowed("https://chase.com/x", &PageAction::Click(label.into()), &m).ok(),
            "{label} should be clickable"
        );
    }
}

#[test]
fn submitting_a_form_on_a_money_site_is_always_refused() {
    let m = money();
    assert!(!finance::allowed("https://ibkr.com/x", &PageAction::Submit, &m).ok());
}

#[test]
fn typing_fails_closed_on_a_money_site() {
    let m = money();
    assert!(finance::allowed(
        "https://schwab.com/x",
        &PageAction::Type { field: "password".into(), text: "x".into() },
        &m
    ).ok());
    assert!(!finance::allowed(
        "https://schwab.com/x",
        &PageAction::Type { field: "quantity".into(), text: "500".into() },
        &m
    ).ok(), "it would have typed an order quantity");
}

// ===========================================================================
// The vault gets the pair
// ===========================================================================

#[test]
fn both_halves_go_to_the_vault_together() {
    let e = Enrolment::new("example.com", "eric_s", 0);
    let (name, kind, value) = e.vault_write("Correct-Horse-99");
    assert_eq!(name, "signup/example.com/eric_s");
    assert_eq!(kind, Kind::Login);
    let (u, p) = Enrolment::split_login(&value).expect("a pair goes in and comes back");
    assert_eq!(u, "eric_s");
    assert_eq!(p, "Correct-Horse-99");
}

#[test]
fn half_a_credential_is_not_a_credential() {
    assert!(Enrolment::split_login("no-newline-here").is_none());
    assert!(Enrolment::split_login("eric_s\n").is_none());
    assert!(Enrolment::split_login("\npassword").is_none());
}

// ===========================================================================
// The expanded keyword sets
// ===========================================================================

#[test]
fn the_new_payment_keywords_all_stop_it() {
    for t in [
        "Choose a plan", "Billed annually", "MM / YY", "Klarna", "Apple Pay",
        "Add a payment method", "Order summary", "Promo code", "Total due",
        "Confirm and pay", "Go premium", "Trial ends in 14 days", "SWIFT",
        "Direct debit", "Card on file", "$12 a month",
    ] {
        assert!(
            matches!(read(&page(t), "example.com", &on()), Verdict::Abandon(Stopped::WantsPayment(_))),
            "{t:?} did not stop it"
        );
    }
}

#[test]
fn the_new_human_check_keywords_all_hand_over() {
    for t in [
        "hCaptcha", "Cloudflare Turnstile", "Checking your browser",
        "One more step", "Slide to verify", "Rotate the image",
        "Type the characters you see", "Additional verification required",
        "Hold to confirm",
    ] {
        assert!(
            matches!(read(&page(t), "example.com", &on()), Verdict::HandOver(Stopped::CheckingYouAreHuman)),
            "{t:?} did not hand over"
        );
    }
}

#[test]
fn kyc_language_ends_the_run() {
    for t in [
        "Know your customer", "Accredited investor", "Source of funds",
        "Annual income", "Liveness check", "Upload a utility bill",
    ] {
        assert!(
            matches!(read(&page(t), "example.com", &on()), Verdict::Abandon(Stopped::WantsIdentityDocuments(_))),
            "{t:?} did not stop it"
        );
    }
}

#[test]
fn the_expanded_lists_did_not_swallow_ordinary_pages() {
    // Over-broad matching is the risk with lists this long. These must pass.
    for t in [
        "Create your account",
        "Choose a username and a password",
        "Welcome. Tell us your name.",
        "Pick a display name",
        "What should we call you?",
    ] {
        assert_eq!(read(&page(t), "example.com", &on()), Verdict::Carry, "{t:?} was wrongly stopped");
    }
}

#[test]
fn a_broker_is_refused_outright_not_offered_for_approval() {
    // "requires explicit approval" would imply that approving works.
    let m = FinanceConfig::default();
    let why = never_enrols_on("tastytrade.com", &m).expect("should refuse");
    assert!(why.contains("wouldn't change that"), "got: {why}");
    assert!(why.contains("sign you in"), "it should still offer the thing it will do: {why}");
    assert!(never_enrols_on("example.com", &m).is_none());
}
