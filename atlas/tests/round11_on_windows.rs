//! Round 11's Windows side, run on Windows itself: the clipboard's sequence
//! number and private-copy check, the window grab and Windows' own text
//! recognizer, a registered key chord, and the Start menu walk.
//!
//! Nothing here types, opens a window or shows anything: the clipboard is
//! put back as it was, and the chord is registered and let go.

#[cfg(windows)]
mod on_windows {
    use atlas::platform::win::WindowsPlatform;
    use atlas::platform::{ClipCopy, Grab, Platform};

    #[test]
    fn windows_the_clipboard_is_read_only_when_it_moves_and_put_back() {
        let p = WindowsPlatform;
        let before = p.read_clipboard().unwrap();
        let seq0 = p.clipboard_change();
        println!("LIVE [clipboard] sequence number: {seq0:?}");
        assert!(seq0.is_some(), "Windows always has a sequence number");
        p.write_clipboard("round 11 clipboard check").unwrap();
        let seq1 = p.clipboard_change();
        assert_ne!(seq0, seq1, "a write moves the sequence");
        let copy = p.clipboard_copy();
        println!("LIVE [clipboard] read back: {copy:?}");
        assert_eq!(copy, Some(ClipCopy::Text("round 11 clipboard check".into())));
        // Put back what was there.
        p.write_clipboard(before.as_deref().unwrap_or("")).unwrap();
        assert_eq!(p.read_clipboard().unwrap().unwrap_or_default(), before.unwrap_or_default());
    }

    #[test]
    fn windows_reads_text_off_an_image_on_this_machine() {
        let png = std::fs::read("tests/fixtures/round11/ocr_invoice.png".to_string()).unwrap();
        let img = atlas::pngcodec::read_png(&png).unwrap();
        let rgb: Vec<u8> = img.pixels.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        let grab = Grab { width: img.width, height: img.height, rgb, title: "fixture".into() };
        let t = std::time::Instant::now();
        let read = WindowsPlatform.recognise_text(&grab).unwrap();
        println!("LIVE [ocr] Windows.Media.Ocr read {read:?} in {:?}", t.elapsed());
        let read = read.expect("a language with text recognition is installed");
        assert!(read.contains("4471") && read.contains("43.20"), "{read}");
        let r = atlas::receipts::read(&read);
        assert_eq!(r.total, atlas::receipts::Total::Labelled(4320));
    }

    #[test]
    fn windows_the_front_window_can_be_grabbed() {
        let g = WindowsPlatform.grab_window().unwrap();
        println!("LIVE [grab] {:?}", g.as_ref().map(|g| (g.width, g.height, g.title.clone())));
        if let Some(g) = g {
            assert_eq!(g.rgb.len(), (g.width * g.height * 3) as usize);
        }
    }

    #[test]
    fn windows_a_chord_registers_and_one_windows_keeps_is_refused() {
        let c = atlas::chords::read_chord("ctrl+alt+shift+f12").unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let failed = atlas::chords::start_chords(vec![(atlas::chords::Does::Capture, c)], tx);
        println!("LIVE [chords] not registered: {failed:?}");
        assert!(failed.is_empty(), "ctrl+alt+shift+F12 is free on a stock machine");
        assert!(atlas::chords::read_chord("win+l").is_err());
    }

    #[test]
    fn windows_the_start_menu_is_walked() {
        let dirs = atlas::launcher::start_menu_dirs();
        let t = std::time::Instant::now();
        let found = atlas::launcher::shortcuts(&dirs);
        println!("LIVE [launcher] {} shortcuts in {:?} from {dirs:?}", found.len(), t.elapsed());
        assert!(!found.is_empty());
        assert!(t.elapsed().as_secs() < 5);
    }
}
