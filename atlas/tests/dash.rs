//! The dashboard arrangement.

use atlas::dash::{Card, Layout, Move, Span};
use atlas::store::Store;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-dash-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn order(l: &Layout) -> Vec<Card> {
    l.cards.iter().map(|p| p.card).collect()
}

// ============ arranging ============

#[test]
fn a_card_moves_up_and_the_rest_close_the_gap() {
    let mut l = Layout::default();
    let before = order(&l);
    let third = before[2];
    assert!(l.move_up(third));
    let after = order(&l);
    assert_eq!(after[1], third);
    assert_eq!(after[2], before[1], "the one it passed moved down, not away");
    assert_eq!(after.len(), before.len(), "nothing was lost in the move");
}

#[test]
fn the_top_card_cannot_move_up_and_says_so_rather_than_pretending() {
    let mut l = Layout::default();
    let top = order(&l)[0];
    assert!(
        !l.move_up(top),
        "returning true here would make the caller write an identical file \
         and show a 'saved' message for nothing"
    );
    assert_eq!(order(&l)[0], top);
}

#[test]
fn the_bottom_card_cannot_move_down() {
    let mut l = Layout::default();
    let last = *order(&l).last().unwrap();
    assert!(!l.move_down(last));
}

#[test]
fn dropping_past_the_end_lands_at_the_end_rather_than_being_refused() {
    let mut l = Layout::default();
    let first = order(&l)[0];
    assert!(l.place(first, 99));
    assert_eq!(
        *order(&l).last().unwrap(),
        first,
        "a drop below the last card means 'put it last'"
    );
}

#[test]
fn a_drag_and_a_button_press_go_through_the_same_call() {
    // The point of `place` taking an index: the two ways of moving a card
    // cannot drift apart, because there is only one of them.
    let mut dragged = Layout::default();
    let mut clicked = Layout::default();
    let card = order(&dragged)[3];

    dragged.place(card, 2);
    clicked.move_up(card);

    assert_eq!(order(&dragged), order(&clicked));
}

// ============ showing and hiding ============

#[test]
fn a_hidden_card_leaves_the_screen_but_not_the_layout() {
    let mut l = Layout::default();
    let card = order(&l)[1];
    assert!(l.set_hidden(card, true));
    assert!(!l.visible().iter().any(|p| p.card == card));
    assert!(
        l.cards.iter().any(|p| p.card == card),
        "kept, so putting it back is one click rather than remembering what \
         used to be there"
    );
}

#[test]
fn hiding_something_already_hidden_changes_nothing() {
    let mut l = Layout::default();
    let card = order(&l)[1];
    l.set_hidden(card, true);
    assert!(!l.set_hidden(card, true));
}

#[test]
fn hiding_everything_is_allowed_but_reports_itself() {
    let mut l = Layout::default();
    for c in Card::all() {
        l.set_hidden(c, true);
    }
    assert!(
        l.is_empty(),
        "a blank dashboard with no explanation reads as broken rather than \
         empty, so the renderer has to be told"
    );
}

#[test]
fn widening_toggles_and_does_not_stick() {
    let mut l = Layout::default();
    let card = order(&l)[2];
    let before = l.cards[2].span;
    l.widen(card);
    assert_eq!(l.cards[2].span, before.other());
    l.widen(card);
    assert_eq!(l.cards[2].span, before);
}

#[test]
fn what_needs_doing_is_the_first_thing_and_gets_the_width() {
    let l = Layout::default();
    assert_eq!(l.cards[0].card, Card::Outstanding);
    assert_eq!(
        l.cards[0].span,
        Span::Full,
        "the question you open a dashboard to answer"
    );
}

// ============ surviving a change to Atlas ============

#[test]
fn a_card_added_to_atlas_after_you_saved_still_appears() {
    // The silent one. A saved layout that names five cards must not mean a
    // sixth ships invisible to the people who use the dashboard most.
    let partial = Layout {
        cards: vec![atlas::dash::Placed {
            card: Card::Machine,
            span: Span::Half,
            hidden: false,
        }],
    };
    let fixed = partial.reconciled();
    for card in Card::all() {
        assert!(
            fixed.cards.iter().any(|p| p.card == card),
            "{card:?} exists in Atlas and is missing from the reconciled layout"
        );
    }
}

#[test]
fn a_card_you_arranged_keeps_its_place_when_others_are_added() {
    let partial = Layout {
        cards: vec![atlas::dash::Placed {
            card: Card::Machine,
            span: Span::Full,
            hidden: false,
        }],
    };
    let fixed = partial.reconciled();
    assert_eq!(fixed.cards[0].card, Card::Machine, "your choice comes first");
    assert_eq!(fixed.cards[0].span, Span::Full, "and keeps its width");
}

#[test]
fn the_same_card_listed_twice_is_collapsed_rather_than_shown_twice() {
    let doubled = Layout {
        cards: vec![
            atlas::dash::Placed { card: Card::Machine, span: Span::Half, hidden: false },
            atlas::dash::Placed { card: Card::Machine, span: Span::Full, hidden: true },
        ],
    };
    let fixed = doubled.reconciled();
    let n = fixed.cards.iter().filter(|p| p.card == Card::Machine).count();
    assert_eq!(n, 1, "two entries would make 'move it up' ambiguous");
    assert!(!fixed.cards[0].hidden, "the first one wins, not the last");
}

#[test]
fn a_saved_layout_survives_a_restart() {
    let store = Store::new(tmp("roundtrip"));
    let mut l = Layout::load(&store);
    let card = order(&l)[4];
    l.place(card, 0);
    l.set_hidden(Card::Ideas, true);
    l.save(&store).unwrap();

    let back = Layout::load(&store);
    assert_eq!(order(&back)[0], card);
    assert!(back.cards.iter().any(|p| p.card == Card::Ideas && p.hidden));
}

#[test]
fn no_saved_layout_gives_the_full_default_rather_than_an_empty_dashboard() {
    let store = Store::new(tmp("fresh"));
    let l = Layout::load(&store);
    assert_eq!(
        l.cards.len(),
        Card::all().len(),
        "an absent file must not read as 'you hid everything'"
    );
    assert!(l.is_default());
}

#[test]
fn an_arranged_layout_no_longer_claims_to_be_the_default() {
    let mut l = Layout::default();
    l.move_down(Card::Outstanding);
    assert!(!l.is_default());
}

// ============ reading a request ============

#[test]
fn a_move_is_read_from_its_fields() {
    assert_eq!(
        Move::parse("up", Some("machine"), None),
        Some(Move::Up(Card::Machine))
    );
    assert_eq!(
        Move::parse("to", Some("ideas"), Some("3")),
        Some(Move::To(Card::Ideas, 3))
    );
    assert_eq!(Move::parse("reset", None, None), Some(Move::Reset));
}

#[test]
fn an_unrecognised_request_does_nothing_rather_than_guessing() {
    assert_eq!(Move::parse("sideways", Some("machine"), None), None);
    assert_eq!(Move::parse("up", Some("not-a-card"), None), None);
    assert_eq!(
        Move::parse("to", Some("machine"), Some("over there")),
        None,
        "a dashboard that rearranges itself on a malformed field is worse \
         than one that does nothing"
    );
    assert_eq!(Move::parse("up", None, None), None);
}

#[test]
fn applying_a_move_reports_whether_anything_changed() {
    let mut l = Layout::default();
    let top = order(&l)[0];
    assert!(
        !l.apply(&Move::Up(top)),
        "no change means no write and no 'saved' message"
    );
    assert!(l.apply(&Move::Down(top)));
}

#[test]
fn reset_puts_everything_back_including_what_you_hid() {
    let mut l = Layout::default();
    l.apply(&Move::Hide(Card::Machine));
    l.apply(&Move::Down(Card::Outstanding));
    assert!(l.apply(&Move::Reset));
    assert!(l.is_default());
    assert!(!l.apply(&Move::Reset), "already default, nothing to write");
}

#[test]
fn every_card_can_be_written_into_a_form_and_read_back_out() {
    for card in Card::all() {
        assert_eq!(
            Card::from_key(card.key()),
            Some(card),
            "{card:?} cannot survive a round trip through a form field, so \
             any button carrying it would silently do nothing"
        );
    }
}

#[test]
fn every_card_has_a_distinct_key() {
    let keys: Vec<&str> = Card::all().iter().map(|c| c.key()).collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), keys.len(), "two cards sharing a key: {keys:?}");
}
