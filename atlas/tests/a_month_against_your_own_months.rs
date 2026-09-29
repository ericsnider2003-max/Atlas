//! Per-bucket money baselines, against retained months.
//!
//! `--keep` used to shift this month into last and drop what was there, so
//! the two-month window was the entire memory and "is this normal FOR ME?"
//! was unanswerable — `judgment::ordinary_for` sat ready the whole time with
//! nothing to feed it. `money.category_jump`'s single fraction was the wrong
//! shape for the same reason, in `judgment`'s own words: groceries moving
//! 50% is an emergency, "things you chose" moving 50% is a Tuesday, because
//! the two buckets have different ordinary variation.

use atlas::money::{
    remember_month, summarise, unusual_buckets, Bucket, Entry, KeptMonth, MONTHS_FLOOR, MONTHS_KEPT,
};

fn spend(desc: &str, amount: f32, bucket: Bucket) -> Entry {
    Entry {
        description: desc.into(),
        amount: -amount,
        at: 0,
        bucket,
        confirmed: false,
        account: "test".into(),
    }
}

/// A month of groceries at `food`, chosen-things at `fun`.
fn month(food: f32, fun: f32) -> Vec<Entry> {
    vec![spend("tesco", food, Bucket::Food), spend("hobby shop", fun, Bucket::Spending)]
}

fn kept(months: &[(f32, f32)]) -> Vec<KeptMonth> {
    let mut out = Vec::new();
    for (i, (food, fun)) in months.iter().enumerate() {
        remember_month(&mut out, &month(*food, *fun), i as u64);
    }
    out
}

#[test]
fn below_the_floor_the_baseline_stays_quiet() {
    // Five months of history is a fact about which months happened.
    let past = kept(&[(400.0, 100.0); 5]);
    assert!(past.len() < MONTHS_FLOOR);
    let now = month(4_000.0, 100.0);
    assert!(
        unusual_buckets(&past, &now).is_empty(),
        "a baseline with too little behind it must say nothing, not something"
    );
}

#[test]
fn a_steady_bucket_that_jumps_is_said_and_a_wobbly_one_is_not() {
    // Food is steady around 400; fun swings 50 to 450 as a matter of course.
    let past = kept(&[
        (390.0, 60.0),
        (410.0, 420.0),
        (400.0, 90.0),
        (395.0, 380.0),
        (405.0, 50.0),
        (400.0, 440.0),
        (398.0, 70.0),
        (402.0, 400.0),
    ]);
    // This month: food doubles, fun lands inside its usual swing.
    let now = month(800.0, 430.0);
    let said = unusual_buckets(&past, &now);
    assert!(
        said.iter().any(|l| l.contains("food")),
        "a steady bucket doubling is exactly what the baseline exists to say: {said:?}"
    );
    assert!(
        said.iter().all(|l| !l.contains("things you chose")),
        "a swing inside a bucket's own ordinary variation is a Tuesday: {said:?}"
    );
}

#[test]
fn the_same_fraction_reads_differently_per_bucket() {
    // The category_jump critique, held as a test: +50% on the steady bucket
    // speaks, +50% on the wobbly one stays quiet.
    let past = kept(&[
        (390.0, 150.0),
        (410.0, 450.0),
        (395.0, 200.0),
        (405.0, 400.0),
        (400.0, 250.0),
        (398.0, 350.0),
        (402.0, 300.0),
    ]);
    let now = month(600.0, 450.0); // both +50% of their middles (400, 300)
    let said = unusual_buckets(&past, &now);
    assert!(said.iter().any(|l| l.contains("food")), "{said:?}");
    assert!(said.iter().all(|l| !l.contains("things you chose")), "{said:?}");
}

#[test]
fn a_month_with_nothing_in_a_bucket_counts_as_zero_not_missing() {
    // Sometimes-bought things: most months zero, occasionally a chunk. A
    // baseline that skipped the zeros would learn you buy it monthly.
    let past = kept(&[
        (400.0, 0.0),
        (400.0, 0.0),
        (400.0, 300.0),
        (400.0, 0.0),
        (400.0, 0.0),
        (400.0, 0.0),
        (400.0, 0.0),
    ]);
    // The median of {0,0,0,0,0,0,300} is 0 and the spread is small, so a
    // fresh 300 is genuinely unusual — but it should be SAID from a real
    // baseline, not skipped for lack of rows.
    let now = month(400.0, 300.0);
    let said = unusual_buckets(&past, &now);
    assert!(
        said.iter().any(|l| l.contains("things you chose")),
        "zeros are observations; without them this month would look ordinary: {said:?}"
    );
}

#[test]
fn the_series_is_bounded_oldest_out_first() {
    let mut months = Vec::new();
    for i in 0..(MONTHS_KEPT + 10) {
        remember_month(&mut months, &month(400.0 + i as f32, 100.0), i as u64);
    }
    assert_eq!(months.len(), MONTHS_KEPT);
    assert_eq!(months.first().unwrap().at, 10, "the oldest months fell off, not the newest");
}

#[test]
fn what_is_remembered_is_the_summary_not_the_statement() {
    let mut months = Vec::new();
    remember_month(&mut months, &month(400.0, 100.0), 7);
    let m = &months[0];
    assert_eq!(m.at, 7);
    let s = summarise(&month(400.0, 100.0));
    assert_eq!(m.by_bucket, s.by_bucket, "the kept month is the same summary money answers from");
}
