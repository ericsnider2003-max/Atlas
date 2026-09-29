//! 29 Sep 2026: the desktop overlay was a window over the whole screen,
//! above everything, all the time, invisible only if transparency worked.
//! On Eric's laptop it didn't: a black screen from the moment Atlas started
//! that clicks passed through and only Task Manager removed.

use atlas::overlay::{Element, Overlay, OverlayConfig};

#[test]
fn everything_the_overlay_draws_fits_in_its_band() {
    let cfg = OverlayConfig::default();
    for (w, h) in [(2560, 1392), (1920, 1080), (1366, 768)] {
        let (bx, by, bw, bh) = atlas::overlaywin::overlay_band(w, h, &cfg);
        assert!(bw < w && bh < h / 2, "the band is most of the screen: {bw}x{bh} on {w}x{h}");
        let mut o = Overlay::begin("A reply long enough to wrap onto more than one line of the caption.", 0);
        // Far enough in for the words to be typing.
        for t in (0..20_000).step_by(100) {
            let _ = o.tick(t, &cfg);
        }
        for e in o.frame(w, h, &cfg) {
            let (x, y, ew, eh) = match e {
                Element::Mark { x, y, size, .. } => (x, y, size, size),
                Element::Shade { x, y, width, height, .. } => (x, y, width, height),
                Element::Typed { x, y, size, .. } => (x, y, 0, size),
                Element::Outline { .. } => continue,
            };
            assert!(x >= bx && x + ew <= bx + bw, "{e:?} outside {bx}..{} on {w}x{h}", bx + bw);
            assert!(y >= by && y + eh <= by + bh, "{e:?} outside {by}..{} on {w}x{h}", by + bh);
        }
    }
}

#[test]
fn the_overlay_window_starts_hidden_and_hides_when_quiet() {
    let src = std::fs::read_to_string("src/overlaywin.rs").unwrap();
    assert!(src.contains(".with_visible(false)"), "the overlay window starts on screen");
    assert!(!src.contains("InnerSize(size)"), "the overlay is sized to the whole monitor again");
    assert!(src.contains("ViewportCommand::Visible(false)"), "nothing hides it when Atlas is quiet");
}
