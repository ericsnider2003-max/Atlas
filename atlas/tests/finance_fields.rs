//! What counts as a sign-in form.
//!
//! `finance.rs` says the field check "is the whole safety of it", and it was
//! right about the stakes. The check was `field.to_lowercase().contains(s)`
//! over a list that included `"to"`.
//!
//! `"to"` is inside `total`, `token`, `stop_loss`, `custodian`, `routing_to`
//! and `account_to`. Measured against realistic forms, four out of five
//! transfer and order layouts came back as entirely safe fields and were
//! submitted on a banking domain:
//!
//! ```text
//! ["account_from", "account_to", "amount_total"]   -> SUBMITTED
//! ["from_account", "to_account", "total"]          -> SUBMITTED
//! ["stop_loss", "token", "total"]                  -> SUBMITTED
//! ["payee_code", "routing_to", "amount_total"]     -> SUBMITTED
//! ```
//!
//! Same shape as the `checks.rs` no-list: a guard whose intent was right and
//! whose matcher admitted the thing it existed to stop.

use atlas::finance::{allowed, is_safe_field, FinanceConfig, PageAction, Verdict};

const BANK: &str = "https://chase.com/transfer";
const BROKER: &str = "https://tastytrade.com/trade";

fn sign_in(fields: &[&str]) -> PageAction {
    PageAction::SubmitSignIn {
        fields: fields.iter().map(|s| s.to_string()).collect(),
    }
}

fn submitted(url: &str, fields: &[&str]) -> bool {
    matches!(
        allowed(url, &sign_in(fields), &FinanceConfig::default()),
        Verdict::Allowed
    )
}

// --- the forms that used to get through ------------------------------------

#[test]
fn a_wire_transfer_form_is_not_a_sign_in_form() {
    for fields in [
        &["account_from", "account_to", "amount_total"][..],
        &["from_account", "to_account", "total"][..],
        &["payee_code", "routing_to", "amount_total"][..],
        &["fromAccount", "toAccount", "transferAmount"][..],
        &["source_account", "destination_account", "amount"][..],
    ] {
        assert!(
            !submitted(BANK, fields),
            "this transfer form was submitted on a bank page: {fields:?}"
        );
    }
}

#[test]
fn an_order_ticket_is_not_a_sign_in_form() {
    for fields in [
        &["stop_loss", "token", "total"][..],
        &["symbol", "quantity", "order_type"][..],
        &["ticker", "side", "limit_price"][..],
        &["contracts", "strike", "expiry"][..],
    ] {
        assert!(
            !submitted(BROKER, fields),
            "this order ticket was submitted on a broker page: {fields:?}"
        );
    }
}

#[test]
fn one_bad_field_settles_the_whole_form() {
    // Nine credential fields and one amount is a payment form.
    assert!(!submitted(
        BANK,
        &["username", "password", "otp", "remember", "amount"]
    ));
}

// --- what must still work ---------------------------------------------------

#[test]
fn a_real_sign_in_form_still_submits() {
    for fields in [
        &["username", "password"][..],
        &["user", "pass"][..],
        &["email", "password", "remember"][..],
        &["login", "passcode", "otp"][..],
        &["userId", "password"][..],
        &["user_name", "pwd", "trust_device"][..],
    ] {
        assert!(
            submitted(BANK, fields),
            "a genuine sign-in form was refused: {fields:?}"
        );
    }
}

#[test]
fn date_filters_on_a_statement_still_work() {
    // Reading a statement means narrowing it. This was the reason from/to were
    // on the list in the first place, and it still holds.
    assert!(submitted(BANK, &["start_date", "end_date"]));
    assert!(submitted(BANK, &["from", "to"]));
    assert!(submitted(BANK, &["search", "month", "year"]));
}

#[test]
fn nothing_outside_a_money_domain_is_affected() {
    let anywhere = "https://example.com/anything";
    assert!(submitted(anywhere, &["amount", "payee", "account_to"]));
}

// --- the matcher itself -----------------------------------------------------

#[test]
fn fields_are_matched_as_words_not_substrings() {
    // The specific failure: "to" inside another word.
    assert!(!is_safe_field("total"));
    assert!(!is_safe_field("token"));
    assert!(!is_safe_field("stop_loss"));
    assert!(!is_safe_field("custodian"));
    assert!(is_safe_field("to"));
    assert!(is_safe_field("date_to"));
}

#[test]
fn separators_and_camel_case_split_the_same_way() {
    for spelling in ["account_from", "accountFrom", "account-from", "account.from"] {
        assert!(
            !is_safe_field(spelling),
            "{spelling} was read as a safe field"
        );
    }
    for spelling in ["user_name", "userName", "user-name"] {
        assert!(is_safe_field(spelling), "{spelling} was refused");
    }
}

#[test]
fn the_deny_list_beats_the_allow_list() {
    // Every word here is otherwise recognised; `amount` is not, and one is
    // enough.
    assert!(!is_safe_field("start_date_amount"));
    assert!(!is_safe_field("user_account"));
    assert!(!is_safe_field("confirm"));
}

#[test]
fn an_unrecognised_field_fails_closed() {
    // Neither allowed nor denied by name: refused, because an unknown field on
    // a money page could be anything.
    assert!(!is_safe_field("xyzzy"));
    assert!(!is_safe_field(""));
    assert!(!is_safe_field("   "));
    assert!(!is_safe_field("memo"));
}

#[test]
fn typing_uses_the_same_rule_as_submitting() {
    // The Type path had the identical substring bug, so it could type into
    // amount_total on the strength of "to".
    let c = FinanceConfig::default();
    let refused = |field: &str| {
        matches!(
            allowed(
                BANK,
                &PageAction::Type { field: field.into(), text: "500".into() },
                &c
            ),
            Verdict::Refused(_)
        )
    };
    assert!(refused("amount_total"));
    assert!(refused("account_to"));
    assert!(refused("quantity"));
    assert!(!refused("password"));
    assert!(!refused("start_date"));
}

// --- the same fix, failing the other way ------------------------------------

#[test]
fn reading_a_statement_is_not_blocked_by_a_word_inside_another_word() {
    // These were all refused: "pay" inside Payment, "buy" inside Buying,
    // "roll" inside Enrollment, "wire" inside Wireless. Every one is a
    // read-only view, and reading statements is what Atlas is for here.
    for label in [
        "Buying power",
        "Payment history",
        "Repayment schedule",
        "Enrollment",
        "Wireless statements",
        "Payee list",
        "Statements",
        "Next page",
        "Export CSV",
    ] {
        assert!(
            !atlas::finance::moves_money(label),
            "{label:?} is a read-only view and was refused"
        );
    }
}

#[test]
fn the_buttons_that_move_money_are_still_refused() {
    for label in [
        "Sell", "Buy", "Transfer", "Pay now", "Wire", "Withdraw", "Deposit",
        "Place order", "Review order", "Close position", "Liquidate",
        "Confirm", "Submit", "Add payee", "Fund account",
    ] {
        assert!(
            atlas::finance::moves_money(label),
            "{label:?} would have been clicked"
        );
    }
}

#[test]
fn multi_word_entries_still_match_as_phrases() {
    // "place order" cannot be judged one word at a time without "place"
    // catching "Marketplace".
    assert!(atlas::finance::moves_money("Place order"));
    assert!(!atlas::finance::moves_money("Marketplace"));
}

#[test]
fn position_decides_whether_a_qualifier_makes_it_a_view() {
    // The same two words in either order mean opposite things. Checking the
    // qualifier anywhere in the label let both of these through, and both were
    // caught by tests already in this repo rather than by me.
    assert!(!atlas::finance::moves_money("Payment schedule"));
    assert!(atlas::finance::moves_money("Schedule payment"));
    assert!(!atlas::finance::moves_money("Card limits"));
    assert!(atlas::finance::moves_money("Limit order"));
    assert!(!atlas::finance::moves_money("Transfer history"));
    assert!(atlas::finance::moves_money("Transfer"));
}

// --- statement parsing ------------------------------------------------------

#[test]
fn a_row_whose_amount_will_not_parse_is_skipped_not_booked_as_zero() {
    // `unwrap_or(0.0)` kept the row in the total as a transaction worth
    // nothing, so a statement that failed to parse looked like one that
    // balanced.
    let csv = "date,description,debit,credit\n\
               2026-01-01,coffee,4.20,\n\
               2026-01-02,mystery,NOT-A-NUMBER,\n\
               2026-01-03,wages,,2000.00\n";
    let txs = atlas::finance::parse_csv(csv, atlas::finance::Source::LocalFile);
    assert_eq!(txs.len(), 2, "the unparseable row was booked: {txs:?}");
    assert!(
        !txs.iter().any(|t| t.amount == 0.0),
        "a row came through worth exactly zero"
    );
}

#[test]
fn an_empty_cell_is_still_treated_as_nothing() {
    // Most rows fill one column and leave the other blank. That is genuinely
    // zero, and must keep parsing.
    let csv = "date,description,debit,credit\n2026-01-01,coffee,4.20,\n";
    assert_eq!(atlas::finance::parse_csv(csv, atlas::finance::Source::LocalFile).len(), 1);
}
