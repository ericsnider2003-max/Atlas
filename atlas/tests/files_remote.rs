use atlas::files::{
    after_scan, convert, join, pdf_is_really_a_scan, safe_to_unpack, scan_steps, Convert,
    FilesConfig, Join, Scanned, Sort,
};
use atlas::remote::{
    finished, handing_off, never_ran, worth_waking_for, How, Needs, Queue, RemoteConfig, State,
};
use atlas::workingset::{not_here, pack, returning, spoken, Carried, Changed, WorkingSetConfig, Why};

const MB: u64 = 1_048_576;

// ================= taking the work with you =================

fn cfg() -> WorkingSetConfig {
    WorkingSetConfig::default()
}

fn file(name: &str, mb: u64, why: Why, shrinkable: bool) -> Carried {
    Carried {
        path: format!("/docs/{name}"),
        name: name.into(),
        bytes: mb * MB,
        because: why,
        shrinkable,
    }
}

#[test]
fn what_the_task_needs_comes_first_when_space_is_short() {
    let files = vec![
        file("holiday.mp4", 300, Why::NearSomethingNeeded, true),
        file("contract.pdf", 2, Why::Needed, false),
        file("notes.md", 1, Why::YouHadItOpen, false),
    ];
    let p = pack(&files, &cfg());
    assert!(p.taking.iter().any(|f| f.name == "contract.pdf"));
    assert!(p.taking.iter().any(|f| f.name == "notes.md"));
}

#[test]
fn something_too_big_is_shrunk_before_it_is_dropped() {
    // A smaller version of the document you need beats the absence of it.
    let files = vec![file("scan.png", 400, Why::Needed, true)];
    let p = pack(&files, &cfg());
    assert_eq!(p.shrunk, vec!["scan.png"]);
    assert_eq!(p.taking.len(), 1);
}

#[test]
fn something_that_cannot_shrink_and_does_not_fit_is_named() {
    let files = vec![file("archive.zip", 900, Why::NearSomethingNeeded, false)];
    let p = pack(&files, &cfg());
    assert!(p.taking.is_empty());
    assert_eq!(p.leaving[0].0, "archive.zip");
}

#[test]
fn atlas_says_what_did_not_come_because_finding_out_later_is_the_problem() {
    let files = vec![
        file("contract.pdf", 2, Why::Needed, false),
        file("raw-footage.mov", 900, Why::NearSomethingNeeded, false),
    ];
    let said = spoken(&pack(&files, &cfg()));
    assert!(said.contains("Left behind: raw-footage.mov"));
}

#[test]
fn reaching_for_something_that_did_not_come_gets_a_useful_answer() {
    let said = not_here("raw-footage.mov", "900MB, no room");
    assert!(said.contains("noted that you wanted it"));
    assert!(said.contains("next time the laptop's in reach"));
}

#[test]
fn a_file_changed_only_on_the_phone_just_lands() {
    let changed = vec![Changed { path: "/docs/notes.md".into(), at: 100, bytes: 900 }];
    let (clean, clashes) = returning(&changed, &[]);
    assert_eq!(clean, 1);
    assert!(clashes.is_empty());
}

#[test]
fn a_file_changed_on_both_sides_needs_you() {
    let changed = vec![Changed { path: "/docs/notes.md".into(), at: 100, bytes: 900 }];
    let (clean, clashes) = returning(&changed, &["/docs/notes.md".into()]);
    assert_eq!(clean, 0);
    assert_eq!(clashes.len(), 1);
}

// ================= asking the laptop from your phone =================

#[test]
fn something_that_needs_the_laptop_is_sent_and_you_are_told_it_will_be() {
    let said = handing_off("Rendering the video", true);
    assert!(said.contains("Rendering the video needs the laptop"));
    assert!(said.contains("I'll tell you when it's done"));
}

#[test]
fn a_laptop_that_is_off_queues_rather_than_failing() {
    // A request that quietly disappears is worse than one that's slow.
    let said = handing_off("Rendering the video", false);
    assert!(said.contains("queued"));
    assert!(said.contains("the moment we're back in touch"));
}

#[test]
fn something_that_finished_in_twenty_seconds_is_not_news() {
    // You were still holding the phone.
    let mut q = Queue::default();
    let id = q.ask("pull the figures", Needs::TheLaptop, How::InYourEar, 0);
    q.set(id, State::Done);
    let r = q.requests.iter().find(|r| r.id == id).unwrap();
    assert!(finished(r, 20, "done").is_none());
    assert!(finished(r, 300, "done").is_some());
}

#[test]
fn a_failure_is_worth_hearing_however_quick_it_was() {
    let mut q = Queue::default();
    let id = q.ask("render it", Needs::TheLaptop, How::InYourEar, 0);
    q.set(id, State::Failed);
    let r = q.requests.iter().find(|r| r.id == id).unwrap();
    // Behaviour, not just wording: a failure surfaces even at 3 seconds, where
    // a success that quick would be swallowed as not-news.
    assert!(finished(r, 3, "the file was locked").is_some(), "a quick failure was swallowed");
    let said = finished(r, 3, "the file was locked").unwrap();
    assert!(said.contains("didn't work"));
    assert!(said.contains("the file was locked"));
}

#[test]
fn saying_do_not_tell_me_is_respected_even_for_failures() {
    let mut q = Queue::default();
    let id = q.ask("tidy the folder", Needs::TheLaptop, How::DontTellMe, 0);
    q.set(id, State::Failed);
    let r = q.requests.iter().find(|r| r.id == id).unwrap();
    assert!(finished(r, 600, "nope").is_none());
}

#[test]
fn finishing_says_how_long_it_took_in_units_you_would_use() {
    let mut q = Queue::default();
    let id = q.ask("render the video", Needs::TheLaptop, How::InYourEar, 0);
    q.set(id, State::Done);
    let r = q.requests.iter().find(|r| r.id == id).unwrap();
    let said = finished(r, 420, "it's in Outputs").unwrap();
    assert!(said.contains("took 7 minutes"));
    assert!(said.contains("it's in Outputs"));
}

#[test]
fn something_that_waited_too_long_is_raised_rather_than_assumed_done() {
    let mut q = Queue::default();
    q.ask("render the video", Needs::TheLaptop, How::Quietly, 0);
    let cfg = RemoteConfig { enabled: true, ..Default::default() };
    let gone = q.given_up(60 * 86_400, &cfg);
    assert_eq!(gone.len(), 1);
    assert!(never_ran(gone[0], 60).contains("Still want it?"));
}

#[test]
fn waking_the_laptop_is_only_for_something_urgent_that_needs_it() {
    assert!(worth_waking_for(Needs::TheLaptop, true));
    assert!(!worth_waking_for(Needs::TheLaptop, false));
    assert!(!worth_waking_for(Needs::Anything, true));
}

// ================= whatever you point at =================

#[test]
fn the_common_types_are_recognised_from_the_name() {
    assert_eq!(Sort::of("contract.pdf"), Sort::Pdf);
    assert_eq!(Sort::of("photo.HEIC"), Sort::Picture);
    assert_eq!(Sort::of("backup.tar.gz"), Sort::Archive);
    assert_eq!(Sort::of("sheet.xlsx"), Sort::Sheet);
    assert_eq!(Sort::of("mail.eml"), Sort::Email);
}

#[test]
fn a_pdf_that_is_really_photographs_of_pages_is_the_common_trap() {
    // It looks like a document and behaves like a picture.
    assert!(pdf_is_really_a_scan(120, 10));
    assert!(!pdf_is_really_a_scan(40_000, 10));
}

#[test]
fn converting_says_what_you_lose_rather_than_just_doing_it() {
    match convert(Sort::Sheet, Sort::Text) {
        Convert::CanWithLoss { loses, .. } => {
            assert!(loses.contains("formulas"));
            assert!(loses.contains("every sheet but the first"));
        }
        o => panic!("{o:?}"),
    }
    match convert(Sort::Pdf, Sort::Text) {
        Convert::CanWithLoss { loses, .. } => assert!(loses.contains("tables")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_lossless_conversion_says_nothing_about_loss() {
    assert!(matches!(convert(Sort::Video, Sort::Audio), Convert::Can { loses: None, .. }));
}

#[test]
fn something_that_does_not_convert_says_so_plainly() {
    assert!(matches!(convert(Sort::Archive, Sort::Pdf), Convert::Cannot(_)));
    assert!(matches!(convert(Sort::Pdf, Sort::Pdf), Convert::Cannot(_)));
}

#[test]
fn joining_the_same_kind_of_thing_says_what_it_will_produce() {
    match join(&["a.pdf".into(), "b.pdf".into()]) {
        Join::Can(how) => assert!(how.contains("in the order you gave them")),
        o => panic!("{o:?}"),
    }
    match join(&["1.jpg".into(), "2.jpg".into()]) {
        Join::Can(how) => assert!(how.contains("a page each")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn photos_and_pdfs_together_is_how_documents_actually_arrive() {
    // Worth handling rather than refusing.
    match join(&["scan1.jpg".into(), "contract.pdf".into()]) {
        Join::Mixed { suggestion, .. } => {
            assert!(suggestion.contains("all pages of one PDF"));
            assert!(suggestion.contains("usually what's wanted"));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn genuinely_unrelated_things_get_a_question_not_a_guess() {
    match join(&["song.mp3".into(), "sheet.xlsx".into()]) {
        Join::Mixed { suggestion, .. } => assert!(suggestion.contains("Which did you want")),
        o => panic!("{o:?}"),
    }
}

// ================= a photo of a page =================

#[test]
fn straightening_happens_before_reading_because_that_is_most_of_the_accuracy() {
    // People skip it because the photo looks fine to a human eye.
    let steps = scan_steps(Scanned::Page, &FilesConfig::default());
    let straighten = steps.iter().position(|s| s.contains("straighten")).unwrap();
    let read = steps.iter().position(|s| s.contains("read the words")).unwrap();
    assert!(straighten < read);
    assert!(steps.iter().any(|s| s.contains("flatten the lighting")));
}

#[test]
fn what_gets_pulled_out_depends_on_what_it_is() {
    assert!(scan_steps(Scanned::Receipt, &FilesConfig::default())
        .iter()
        .any(|s| s.contains("the total, the date")));
    assert!(scan_steps(Scanned::Card, &FilesConfig::default())
        .iter()
        .any(|s| s.contains("name, number and email")));
}

#[test]
fn a_receipt_goes_somewhere_more_useful_than_a_text_file() {
    assert!(Scanned::Receipt.probably_wanted().contains("into your ledger"));
    assert!(Scanned::Card.probably_wanted().contains("as a contact"));
}

#[test]
fn atlas_says_what_it_thinks_it_is_so_a_wrong_guess_is_caught_early() {
    let said = after_scan(Scanned::Receipt, 40);
    assert!(said.contains("Looks like a receipt"));
    assert!(said.contains("document, a PDF, or just the text"));
}

#[test]
fn a_bad_photo_says_what_to_do_differently() {
    let said = after_scan(Scanned::Unclear, 2);
    assert!(said.contains("more light, or flatter"));
}

#[test]
fn a_zip_that_becomes_a_full_disk_is_listed_rather_than_unpacked() {
    let cfg = FilesConfig::default();
    let e = safe_to_unpack(2, 90_000, 1, &cfg).unwrap_err();
    assert!(e.contains("list what's in it rather than unpacking"));
    assert!(safe_to_unpack(50, 300, 1, &cfg).is_ok());
}

#[test]
fn archives_nested_too_deep_are_refused() {
    let e = safe_to_unpack(1, 10, 9, &FilesConfig::default()).unwrap_err();
    assert!(e.contains("either a mistake or something trying to be clever"));
}

#[test]
fn everything_can_be_read_without_the_internet() {
    for s in [Sort::Pdf, Sort::Document, Sort::Sheet, Sort::Audio, Sort::Video, Sort::Archive] {
        assert!(s.readable_offline(), "{s:?}");
    }
}
