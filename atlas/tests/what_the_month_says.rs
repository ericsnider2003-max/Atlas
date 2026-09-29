//! A month of statements, read.
//!
//! Every piece of this existed and none of it was reachable.
//! `finance::parse_csv` reads a bank export and had no caller anywhere.
//! `money::sort_one` puts a line in a bucket and had no caller.
//! `summarise`, `spoken`, `new_or_grown` and `work_spend` had none either.
//!
//! And the daemon's money branch called:
//!
//! ```ignore
//! money::spoken(&money::summarise(&[]), &[])
//! ```
//!
//! `spoken` opens with `if m.by_bucket.is_empty() { return "Nothing to go
//! on." }` — so with an empty slice that was the *only* reachable answer, and
//! the reason was that nothing had ever read a statement rather than that
//! there was nothing in it. The same defect as `goingaway::spoken(&[])` and
//! `messaging::spoken(&[], ..)` before it, and the third time this shape has
//! turned up in one tree.
//!
//! Two settings were underneath it. `money.work_words` decides which lines
//! are the business rather than the household. `finance.category_jump` says
//! "flag a category whose spend rises more than this fraction month on
//! month" — and **nothing did that at all**: `finance::review` never compares
//! two months, and `new_or_grown` compares one standing charge at a time with
//! 15% hardcoded against the 50% in the shipped file.

use atlas::finance::{self, Source};
use atlas::money::{self, Bucket, Entry};

const A_STATEMENT: &str = "\
Date,Description,Amount
2026-09-01,RENT PAYMENT,-1450.00
2026-09-02,TESCO SUPERSTORE,-84.20
2026-09-03,NETFLIX SUBSCRIPTION,-17.99
2026-09-04,ADOBE CREATIVE CLOUD,-59.99
2026-09-07,SALARY,3200.00
2026-09-08,TRANSFER TO SAVINGS,-500.00
2026-09-09,AWS HOSTING,-140.00
";

fn entry(description: &str, amount: f32, bucket: Bucket) -> Entry {
    Entry {
        description: description.into(),
        amount,
        at: 0,
        bucket,
        confirmed: false,
        account: String::new(),
    }
}

fn sorted(text: &str, work_words: &[String]) -> Vec<Entry> {
    finance::parse_csv(text, Source::LocalFile)
        .iter()
        .map(|t| entry(
            &t.description,
            t.amount as f32,
            money::sort_one(&t.description, t.amount as f32, work_words),
        ))
        .collect()
}

// ================= a statement becomes a month =================

#[test]
fn a_bank_export_turns_into_something_that_can_be_asked_about() {
    let entries = sorted(A_STATEMENT, &[]);
    assert_eq!(entries.len(), 7, "the export didn't parse");

    let month = money::summarise(&entries);
    assert!(month.in_total > 3100.0, "the salary is missing");

    // Moving your own money is not spending, and counting it as such is the
    // single commonest way a summary lies to you.
    assert!(
        !month.by_bucket.iter().any(|(b, _)| *b == Bucket::MovingYourOwnMoney),
        "a transfer to savings was counted: {:#?}",
        month.by_bucket
    );
    assert!(month.out_total < 1800.0, "the transfer went into the outgoings");

    // And what it says is the biggest thing you could change, not the
    // biggest thing. Telling you your rent is your largest expense is not
    // information.
    let said = money::spoken(&month, &[]);
    assert!(said.contains("food"), "{said}");
    assert!(!said.starts_with("1450"), "it led with the rent: {said}");
}

#[test]
fn nothing_read_is_said_differently_from_nothing_in_it() {
    // The bug this file exists for. "Nothing to go on." is the right answer
    // to an empty month and the wrong answer to a statement nobody read, and
    // the daemon could only ever give the second.
    assert_eq!(money::spoken(&money::summarise(&[]), &[]), "Nothing to go on.");

    let daemon = crate::common::source_of("daemon");
    assert!(
        !daemon.contains("money::spoken(&crate::money::summarise(&[]), &[])"),
        "the money branch still summarises an empty slice"
    );
    assert!(
        daemon.contains("self.store.load(crate::money::THIS_MONTH)"),
        "nothing reads the month that was kept"
    );
    assert!(
        daemon.contains("I haven't read a statement yet"),
        "it still says \"nothing to go on\" about a statement nobody gave it"
    );
}

// ================= work_words, which was the dead setting =================

#[test]
fn which_lines_are_the_business_is_a_list_you_write() {
    // Not because the tax rules differ — they do — but because mixing them
    // means you find out what the content cost you at the end of the year
    // rather than as you go.
    let plain = sorted(A_STATEMENT, &[]);
    let adobe_plain = plain
        .iter()
        .find(|e| e.description.contains("ADOBE"))
        .expect("parsed");
    assert_eq!(adobe_plain.bucket, Bucket::Standing, "a subscription, on the shipped list");

    // Your own word for it moves it, which is the whole point of the setting.
    let yours = sorted(A_STATEMENT, &["adobe".to_string()]);
    let adobe_yours = yours
        .iter()
        .find(|e| e.description.contains("ADOBE"))
        .expect("parsed");
    assert_eq!(adobe_yours.bucket, Bucket::Work);

    assert!(money::work_spend(&yours) > money::work_spend(&plain));
    // And it is case-insensitive, because nobody types their statement's
    // capitalisation back.
    assert_eq!(
        sorted(A_STATEMENT, &["ADOBE".to_string()])
            .iter()
            .find(|e| e.description.contains("ADOBE"))
            .unwrap()
            .bucket,
        Bucket::Work
    );
}

// ================= category_jump, which described a check nobody wrote =====

#[test]
fn a_whole_category_climbing_is_the_check_the_setting_names() {
    // `finance.category_jump` says "flag a category whose spend rises more
    // than this fraction month on month". `finance::review` never compares
    // two months, and `new_or_grown` works one standing charge at a time —
    // so nothing in the tree did what the setting described.
    let last = vec![
        entry("Tesco", -100.0, Bucket::Food),
        entry("Uber", -50.0, Bucket::Travel),
    ];
    let this = vec![
        entry("Tesco", -200.0, Bucket::Food),
        entry("Uber", -52.0, Bucket::Travel),
    ];

    let jumped = money::buckets_that_jumped(&this, &last, 0.5);
    assert_eq!(jumped.len(), 1, "{jumped:#?}");
    assert!(jumped[0].contains("food"), "{}", jumped[0]);
    assert!(jumped[0].contains("100") && jumped[0].contains("200"), "{}", jumped[0]);

    // The fraction is read, not a half that happens to equal it.
    assert!(money::buckets_that_jumped(&this, &last, 2.0).is_empty());
    assert_eq!(money::buckets_that_jumped(&this, &last, 0.01).len(), 2);

    // A bucket you did not have last month is not a rise of infinity. That is
    // a bucket you did not have, and `new_or_grown` says it better, one line
    // at a time.
    let brand_new = vec![entry("Gym", -40.0, Bucket::Standing)];
    assert!(money::buckets_that_jumped(&brand_new, &[], 0.5).is_empty());

    // Money in is not a category climbing.
    let more_pay = vec![entry("Salary", 4000.0, Bucket::Income)];
    let less_pay = vec![entry("Salary", 3000.0, Bucket::Income)];
    assert!(money::buckets_that_jumped(&more_pay, &less_pay, 0.1).is_empty());
}

#[test]
fn one_thing_that_climbed_is_said_once_however_often_it_billed() {
    // A subscription billed twice in a month is one subscription, and saying
    // "Netflix is new" twice is how a short, useful list turns into one you
    // skim.
    let twice = vec![
        entry("Netflix", -17.99, Bucket::Standing),
        entry("Netflix", -17.99, Bucket::Standing),
    ];
    assert_eq!(money::new_or_grown(&twice, &[], 0.15).len(), 1);
}

// ================= it is actually reachable =================

#[test]
fn the_command_is_what_reaches_it_rather_than_this_test() {
    let main = crate::common::source_of("main");
    assert!(main.contains("fn run_money("), "there is no way to read a statement");
    assert!(main.contains("finance::parse_csv(&text, source)"), "nothing parses one");
    assert!(
        main.contains("money::sort_one(&t.description, t.amount as f32, &mcfg.work_words)"),
        "nothing sorts one, or it sorts without your words"
    );
    assert!(main.contains("finance::review(&txns, &fcfg)"), "nothing flags anything");
    assert!(
        main.contains("money::buckets_that_jumped(this, last, jump)"),
        "category_jump still names a check nobody wrote"
    );
    // Keeping a month is deliberate, so a statement you were only looking at
    // does not become the one Atlas answers from.
    assert!(main.contains("Not kept -- add `--keep`"), "it keeps every statement it reads");

    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("work_words:"));
    assert!(raw.contains("category_jump:"));
    // Reading statements ships off.
    assert!(!atlas::money::MoneyConfig::default().enabled);
}

#[test]
fn a_file_that_is_not_a_statement_is_refused_rather_than_read_as_an_empty_month() {
    // The failure that would otherwise look exactly like a month with nothing
    // in it.
    assert!(finance::parse_csv("", Source::LocalFile).is_empty());
    assert!(finance::parse_csv("hello\nworld\n", Source::LocalFile).is_empty());

    // A header with no amount column at all is not a statement.
    assert!(
        finance::parse_csv("Date,Description\n2026-09-01,Rent\n", Source::LocalFile).is_empty()
    );

    // And the command says so rather than printing an empty summary.
    let main = crate::common::source_of("main");
    assert!(main.contains("Nothing I could read in {path}"), "{}", "it prints an empty month");
    assert!(main.contains("matched by meaning, not position"), "it doesn't say what was wrong");
}
