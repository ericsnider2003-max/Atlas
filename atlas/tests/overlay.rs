use atlas::overlay::{
    window_style, Align, Element, MarkState, Overlay,
    OverlayConfig, Phase, Typing,
};

fn cfg() -> OverlayConfig {
    OverlayConfig::default()
}

// ================= it draws on the desktop, not in a window =================

#[test]
fn the_overlay_never_takes_focus_or_gets_in_your_way() {
    // Clicks pass through to whatever is underneath, and it's not something
    // you alt-tab to. It's paint on the glass, not an application.
    let s = window_style();
    assert!(s & atlas::overlay::WS_EX_TRANSPARENT != 0, "clicks go through it");
    assert!(
        s & atlas::overlay::WS_EX_TOOLWINDOW != 0 && s & atlas::overlay::WS_EX_NOACTIVATE != 0,
        "not in the taskbar, not in alt-tab"
    );
}

#[test]
fn it_is_a_layered_window_because_that_is_the_only_way_to_be_transparent() {
    assert!(window_style() & atlas::overlay::WS_EX_LAYERED != 0);
}

// ================= typing =================

#[test]
fn text_arrives_a_character_at_a_time() {
    let mut t = Typing::new("Morning — one thing needs you before you sit down");
    assert_eq!(t.visible(), "");
    t.at(100, &cfg());
    let early = t.visible();
    assert!(!early.is_empty() && early.len() < 12, "a few characters in: {early:?}");
    t.at(300, &cfg());
    assert!(t.visible().len() > early.len(), "it grows");
    assert!(!t.done());
}

#[test]
fn a_full_stop_gets_a_beat_before_the_next_sentence() {
    // So a sentence lands rather than running into the next one.
    let mut t = Typing::new("Morning. One thing needs you.");
    t.at(1000, &cfg());
    assert_eq!(t.visible(), "Morning.", "it stops on the full stop");
    t.at(1100, &cfg());
    assert_eq!(t.visible(), "Morning.", "and holds there for a moment");
    t.at(1400, &cfg());
    assert!(t.visible().len() > 8, "then carries on");
}

#[test]
fn typing_is_driven_by_time_so_a_dropped_frame_does_not_slow_it() {
    let mut fast = Typing::new("A steady line of text here.");
    let mut skipped = Typing::new("A steady line of text here.");
    for ms in (0..2000).step_by(16) {
        fast.at(ms, &cfg());
    }
    // The same elapsed time, far fewer updates.
    for ms in (0..2000).step_by(400) {
        skipped.at(ms, &cfg());
    }
    assert_eq!(fast.visible(), skipped.visible(), "the clock decides, not the frame count");
}

#[test]
fn it_finishes() {
    let mut t = Typing::new("Short.");
    t.at(60_000, &cfg());
    assert!(t.done());
    assert_eq!(t.visible(), "Short.");
}

// ================= the sequence =================

#[test]
fn the_mark_arrives_before_the_words() {
    let mut o = Overlay::begin("Morning.", 0);
    assert_eq!(o.phase, Phase::Arriving);
    assert!(o.opacity < 0.2, "fading in");
    assert!(o.frame(2560, 1392, &cfg()).iter().any(|e| matches!(e, Element::Mark { .. })));

    o.tick(200, &cfg());
    assert_eq!(o.phase, Phase::Arriving, "still arriving");
    o.tick(500, &cfg());
    assert_eq!(o.phase, Phase::Typing, "then the words start");
}

#[test]
fn nothing_is_typed_while_the_mark_is_still_arriving() {
    let o = Overlay::begin("Morning.", 0);
    assert!(!o.frame(2560, 1392, &cfg()).iter().any(|e| matches!(e, Element::Typed { .. })));
}

#[test]
fn it_holds_the_finished_line_then_fades_by_itself() {
    let mut o = Overlay::begin("Morning.", 0);
    for ms in (0..3000).step_by(16) {
        o.tick(ms, &cfg());
    }
    assert_eq!(o.phase, Phase::Holding);
    for ms in (3000..12_000).step_by(16) {
        o.tick(ms, &cfg());
    }
    assert_eq!(o.phase, Phase::Gone);
    assert!(o.frame(2560, 1392, &cfg()).is_empty(), "nothing left on screen");
}

#[test]
fn fading_is_gradual_rather_than_a_disappearance() {
    let mut o = Overlay::begin("x", 0);
    for ms in (0..3000).step_by(16) {
        o.tick(ms, &cfg());
    }
    o.dismiss(3000);
    o.tick(3300, &cfg());
    assert!(o.opacity > 0.0 && o.opacity < 1.0, "mid-fade: {}", o.opacity);
}

#[test]
fn speaking_to_it_takes_it_away_early() {
    let mut o = Overlay::begin("a long line that would take a while", 0);
    o.tick(600, &cfg());
    o.dismiss(600);
    assert_eq!(o.phase, Phase::Fading);
}

// ================= readable over anything =================

#[test]
fn text_sits_over_a_soft_shade_so_it_reads_on_a_white_document() {
    // Transparent text on a white spreadsheet is nothing at all.
    let mut o = Overlay::begin("Morning.", 0);
    for ms in (0..1500).step_by(16) {
        o.tick(ms, &cfg());
    }
    let frame = o.frame(2560, 1392, &cfg());
    let shade = frame.iter().find(|e| matches!(e, Element::Shade { .. })).expect("a shade");
    match shade {
        Element::Shade { width, strength, .. } => {
            assert!(*width > 1500, "much wider than the text, so it has no edge");
            assert!(*strength > 0.3 && *strength < 0.8, "darkens without becoming a box");
        }
        _ => unreachable!(),
    }
}

#[test]
fn the_shade_is_painted_before_the_text_not_over_it() {
    let mut o = Overlay::begin("Morning.", 0);
    for ms in (0..1500).step_by(16) {
        o.tick(ms, &cfg());
    }
    let frame = o.frame(2560, 1392, &cfg());
    let shade_at = frame.iter().position(|e| matches!(e, Element::Shade { .. })).unwrap();
    let text_at = frame.iter().position(|e| matches!(e, Element::Typed { .. })).unwrap();
    assert!(shade_at < text_at);
}

#[test]
fn everything_is_centred_on_the_screen_youre_looking_at() {
    let mut o = Overlay::begin("Morning.", 0);
    for ms in (0..1500).step_by(16) {
        o.tick(ms, &cfg());
    }
    for e in o.frame(2560, 1392, &cfg()) {
        match e {
            Element::Typed { x, align, .. } => {
                assert_eq!(x, 1280);
                assert_eq!(align, Align::Centre);
            }
            Element::Mark { x, size, .. } => assert_eq!(x + size / 2, 1280),
            _ => {}
        }
    }
}

#[test]
fn the_mark_keeps_moving_while_the_words_are_being_typed() {
    // It froze after two sweeps before, which read as a broken image.
    let mut o = Overlay::begin("Morning.", 0);
    o.tick(600, &cfg());
    match o.frame(2560, 1392, &cfg()).into_iter().next() {
        Some(Element::Mark { state, .. }) => assert_eq!(state, MarkState::Thinking),
        o => panic!("{o:?}"),
    }
}
