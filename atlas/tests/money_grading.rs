use atlas::grading::{
    check, recommended_setup, spoken as grade_spoken, tree, GradingConfig, Measured, Node,
    WHY_A_FIXED_TREE,
};
use atlas::money::{
    new_or_grown, sort_one, spoken as money_spoken, summarise, work_spend, Bucket, Entry,
    MoneyConfig, NOT_ADVICE,
};

// ================= colour, in a fixed order =================

#[test]
fn the_order_is_the_point_not_any_one_adjustment() {
    let t = tree();
    assert_eq!(t[0], Node::TransformIn, "log footage is normalised before anything");
    assert_eq!(t[t.len() - 1], Node::TransformOut);
    // Balance before contrast: adding contrast to a colour cast bakes it in.
    let bal = t.iter().position(|n| *n == Node::Balance).unwrap();
    let con = t.iter().position(|n| *n == Node::Contrast).unwrap();
    assert!(bal < con);
    assert!(WHY_A_FIXED_TREE.contains("every tool is available at once"));
}

#[test]
fn every_step_says_which_scope_to_watch_rather_than_the_picture() {
    // The habit that separates grading from fiddling.
    assert!(Node::Exposure.watch().contains("waveform, not the image"));
    assert!(Node::Balance.watch().contains("vectorscope"));
    assert!(Node::Contrast.watch().contains("crush the blacks"));
}

#[test]
fn balancing_to_skin_rather_than_to_white_is_stated_as_the_mistake() {
    let m = Node::Balance.the_mistake().unwrap();
    assert!(m.contains("skin is what the eye judges everything else against"));
}

#[test]
fn the_contrast_pivot_is_not_the_default_and_that_is_why_contrast_darkens_everything() {
    let s = recommended_setup();
    assert!((s.contrast_pivot - 0.336).abs() < 0.001);
    assert!(s.output_space.contains("Rec.709"));
    assert!(s.science.contains("Color Managed"));
}

#[test]
fn skin_off_the_line_is_reported_as_too_red_or_too_green_not_in_degrees_alone() {
    let m = Measured {
        crushed_black: 0.0, clipped_white: 0.0, skin_off_line: 14.0,
        saturation: 1.0, raised_globally: false,
    };
    let notes = check(&m);
    assert!(notes[0].1.contains("too red"));
    assert!(notes[0].1.contains("Move the global colour wheel"));
}

#[test]
fn raising_saturation_globally_is_caught_with_what_to_do_instead() {
    let m = Measured {
        crushed_black: 0.0, clipped_white: 0.0, skin_off_line: 0.0,
        saturation: 1.4, raised_globally: true,
    };
    let notes = check(&m);
    assert!(notes[0].1.contains("turns skin orange"));
    assert!(notes[0].1.contains("colour slice"));
}

#[test]
fn the_notes_come_back_in_tree_order_so_fixing_them_is_one_pass() {
    let m = Measured {
        crushed_black: 0.08, clipped_white: 0.0, skin_off_line: 12.0,
        saturation: 1.4, raised_globally: true,
    };
    let notes = check(&m);
    let order = tree();
    let positions: Vec<usize> = notes
        .iter()
        .map(|(n, _)| order.iter().position(|x| x == n).unwrap())
        .collect();
    assert!(positions.windows(2).all(|w| w[0] <= w[1]), "not in tree order: {positions:?}");
    assert_eq!(notes[0].0, Node::Balance, "balance comes before contrast");
}

#[test]
fn a_clean_grade_gets_one_line() {
    let m = Measured {
        crushed_black: 0.001, clipped_white: 0.0, skin_off_line: 2.0,
        saturation: 1.05, raised_globally: false,
    };
    assert_eq!(grade_spoken(&check(&m)), "Grade looks clean.");
}

#[test]
fn grading_is_off_until_you_turn_it_on() {
    assert!(!GradingConfig::default().enabled);
}

// ================= everyday money =================

fn e(desc: &str, amount: f32, bucket: Bucket) -> Entry {
    Entry {
        description: desc.into(),
        amount,
        at: 0,
        bucket,
        confirmed: false,
        account: "main".into(),
    }
}

#[test]
fn moving_your_own_money_is_never_counted_as_spending() {
    // The single commonest way a summary lies to you.
    assert_eq!(sort_one("Transfer to savings", -500.0, &[]), Bucket::MovingYourOwnMoney);
    assert_eq!(sort_one("Payment to card ending 4417", -300.0, &[]), Bucket::MovingYourOwnMoney);

    let entries = vec![
        e("Transfer to savings", -500.0, Bucket::MovingYourOwnMoney),
        e("Tesco", -60.0, Bucket::Food),
    ];
    let m = summarise(&entries);
    assert!((m.out_total - 60.0).abs() < 0.01, "the transfer isn't spending");
}

#[test]
fn a_transfer_that_mentions_a_shop_is_still_a_transfer() {
    assert_eq!(sort_one("Transfer - tesco savings pot", -50.0, &[]), Bucket::MovingYourOwnMoney);
}

#[test]
fn your_own_words_for_work_beat_the_guesses() {
    let mine = vec!["lens".to_string(), "homelab".to_string()];
    assert_eq!(sort_one("Homelab VPS renewal", -40.0, &mine), Bucket::Work);
}

#[test]
fn the_everyday_buckets_are_the_ones_people_actually_reason_about() {
    // Nobody thinks in "cost of goods sold".
    assert_eq!(sort_one("Rent April", -1200.0, &[]), Bucket::RoofAndBills);
    assert_eq!(sort_one("Sainsburys", -48.0, &[]), Bucket::Food);
    assert_eq!(sort_one("Shell petrol", -60.0, &[]), Bucket::Travel);
    assert_eq!(sort_one("Netflix", -15.0, &[]), Bucket::Standing);
    assert_eq!(sort_one("Salary", 3000.0, &[]), Bucket::Income);
}

#[test]
fn it_leads_with_the_biggest_thing_you_could_change_not_the_biggest_thing() {
    // Telling you your rent is your largest expense is not information.
    let entries = vec![
        e("Rent", -1200.0, Bucket::RoofAndBills),
        e("Food shopping", -420.0, Bucket::Food),
        e("Netflix", -15.0, Bucket::Standing),
    ];
    let said = money_spoken(&summarise(&entries), &[]);
    assert!(said.starts_with("420 on food"), "got: {said}");
    assert!(!said.contains("Rent"));
}

#[test]
fn things_that_go_out_without_you_thinking_are_totalled_separately() {
    let entries = vec![
        e("Netflix", -15.0, Bucket::Standing),
        e("Gym", -40.0, Bucket::Standing),
        e("Food", -300.0, Bucket::Food),
    ];
    assert!(money_spoken(&summarise(&entries), &[]).contains("55 went out on things you don't think about"));
}

#[test]
fn a_subscription_that_appeared_is_the_thing_nobody_looks_for() {
    let last = vec![e("Netflix", -15.0, Bucket::Standing)];
    let this = vec![
        e("Netflix", -15.0, Bucket::Standing),
        e("Adobe", -60.0, Bucket::Standing),
    ];
    // The third argument is `finance.category_jump` -- the fraction a thing
    // has to climb before it is worth saying. It was hardcoded at 15% here
    // and shipped as 50% in the config, so the file said one thing and the
    // code did another.
    let changes = new_or_grown(&this, &last, 0.15);
    assert!(changes.iter().any(|c| c.contains("Adobe is new")));

    // Something new is new whatever the threshold -- there is nothing for it
    // to have risen from.
    assert!(new_or_grown(&this, &last, 5.0).iter().any(|c| c.contains("Adobe is new")));

    // And a subscription billed twice in one month is one subscription.
    let twice = vec![
        e("Adobe", -60.0, Bucket::Standing),
        e("Adobe", -60.0, Bucket::Standing),
    ];
    assert_eq!(new_or_grown(&twice, &[], 0.15).len(), 1);
}

#[test]
fn a_price_rise_on_something_you_already_had_is_caught() {
    let last = vec![e("Spotify", -10.0, Bucket::Standing)];
    let this = vec![e("Spotify", -13.0, Bucket::Standing)];
    // A 30% rise clears a 15% bar and does not clear a 50% one. The number is
    // read rather than a fraction that happens to equal it.
    let changes = new_or_grown(&this, &last, 0.15);
    assert!(changes[0].contains("went from 10.00 to 13.00"));
    assert!(new_or_grown(&this, &last, 0.5).is_empty(), "the threshold is hardcoded");
}

#[test]
fn work_spending_is_kept_apart_so_you_know_as_you_go() {
    let entries = vec![
        e("Adobe", -60.0, Bucket::Work),
        e("Lens", -400.0, Bucket::Work),
        e("Food", -50.0, Bucket::Food),
    ];
    assert!((work_spend(&entries) - 460.0).abs() < 0.01);
}

#[test]
fn atlas_sorts_what_happened_and_does_not_tell_you_what_to_do() {
    assert!(!MoneyConfig::default().gives_advice);
    let parsed: MoneyConfig = serde_yaml::from_str("enabled: true\ngives_advice: true\n").unwrap();
    assert!(!parsed.gives_advice);
    assert!(NOT_ADVICE.contains("the last thing that should be telling you where to put your money"));
}

#[test]
fn things_it_could_not_place_are_admitted_rather_than_guessed_into_a_bucket() {
    let entries: Vec<Entry> = (0..5)
        .map(|i| e(&format!("SQ *SOMETHING {i}"), -20.0, Bucket::Unknown))
        .collect();
    // Behaviour, not just wording: an opaque line is genuinely sorted to
    // Unknown rather than guessed into a spending bucket.
    assert_eq!(sort_one("SQ *SOMETHING 0", -20.0, &[]), Bucket::Unknown);
    assert!(money_spoken(&summarise(&entries), &[]).contains("5 I couldn't place"));
}
