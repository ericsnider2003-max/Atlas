//! Round 11's Windows side, run on Windows itself: the clipboard's sequence
//! number and private-copy check, the window grab and Windows' own text
//! recognizer, a registered key chord, and the Start menu walk.
//!
//! Nothing here types, opens a window or shows anything: the clipboard is
//! never written; its native sequence diagnostic is opt-in. The chord is registered and let go.

#[cfg(windows)]
mod on_windows {
    use atlas::platform::win::WindowsPlatform;
    use atlas::platform::{ClipCopy, Grab, Platform};

    #[test]
    #[ignore = "explicit opt-in read-only native clipboard sequence check"]
    fn windows_clipboard_sequence_is_available_without_reading_or_writing_content() {
        assert_eq!(std::env::var("ATLAS_NATIVE_CLIPBOARD_READONLY").as_deref(), Ok("1"), "explicit native diagnostic opt-in required");
        assert!(WindowsPlatform.clipboard_change().is_some(), "Windows sequence API unavailable");
    }

    #[test]
    fn clipboard_sequence_and_classification_are_tested_with_disposable_mock_data() {
        use atlas::platform::mock::MockPlatform;
        use atlas::cliphist::{History, HistoryConfig, Skipped};
        let p = MockPlatform::new(vec![]);
        let mut history = History::default();
        let cfg = HistoryConfig { enabled: true, ..Default::default() };
        *p.clip_seq.borrow_mut() = Some(7);
        assert!(history.changed(p.clipboard_change()));
        assert!(!history.changed(p.clipboard_change()), "unchanged sequence must not trigger another read");
        *p.clip_seq.borrow_mut() = Some(8);
        *p.clip_copy.borrow_mut() = Some(ClipCopy::Text("Disposable round11 text".into()));
        assert!(history.changed(p.clipboard_change()));
        history.keep(&cfg, &p.clipboard_copy().unwrap(), "fixture", 100).unwrap();
        assert_eq!(history.clips.len(), 1);
        for (copy, expected) in [(ClipCopy::Private, Skipped::Private), (ClipCopy::NotText, Skipped::NotText)] {
            *p.clip_copy.borrow_mut() = Some(copy);
            assert_eq!(history.keep(&cfg, &p.clipboard_copy().unwrap(), "fixture", 101), Err(expected));
        }
        assert_eq!(history.clips.len(), 1, "private/nontext data must not enter history");
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
        crate::common::assert_prompt(t.elapsed(), std::time::Duration::from_secs(5), "took too long");
    }
}
