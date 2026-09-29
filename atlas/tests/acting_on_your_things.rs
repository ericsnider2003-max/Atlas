//! Eric's rulings of 25 Sep 2026 on Atlas acting on his things (G1–G8):
//!
//! - G1: sort the mailbox; delete only when told.
//! - G2: schedule posts.
//! - G3: move windows to answer a question, never while he's mid-something.
//! - G4: click buttons in other apps by name.
//! - G5: move big files, as long as things stay findable and organised.
//! - G6: actually carry out an undo.
//! - G7: say which steps can't be undone.
//! - G8: make a copy, do the work, and once it's approved ask about the original.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::imap::{Message, Session};
use atlas::intent::Intent;
use atlas::mail::Action;
use atlas::platform::mock::{Action as Did, MockPlatform};
use atlas::platform::{ActiveWindow, Monitor, WindowId};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-yours-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> &'static Config {
    Box::leak(Box::new(Config::load(Path::new("config")).unwrap()))
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(scratch(tag)), Proactive::new(ProactiveConfig::default()))
}

// ------------------------------------------------------------------ G1

/// An IMAP server that says OK to everything and remembers what it was asked.
struct Server {
    sent: Vec<u8>,
    replies: std::io::Cursor<Vec<u8>>,
}

impl Server {
    fn new(n: usize) -> Server {
        let mut r = String::new();
        for i in 1..=n {
            r.push_str(&format!("A{i:04} OK done\r\n"));
        }
        Server { sent: Vec::new(), replies: std::io::Cursor::new(r.into_bytes()) }
    }
}
impl Read for Server {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.replies.read(b)
    }
}
impl Write for Server {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.sent.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn sorting_moves_labels_and_archives_and_never_deletes() {
    let mut s = Session::new(Server::new(10));
    s.apply(7, &Action::MoveTo("Newsletters".into()), false).unwrap();
    s.apply(8, &Action::Label("Receipts".into()), true).unwrap();
    s.apply(9, &Action::Archive, true).unwrap();
    s.apply(10, &Action::Archive, false).unwrap();
    let sent = String::from_utf8(s.into_inner().sent).unwrap();
    assert!(sent.contains("CREATE \"Newsletters\""), "{sent}");
    assert!(sent.contains("UID MOVE 7 \"Newsletters\""), "{sent}");
    assert!(sent.contains("UID STORE 8 +X-GM-LABELS (\"Receipts\")"), "{sent}");
    assert!(sent.contains("UID STORE 9 -X-GM-LABELS (\\Inbox)"), "gmail archive is taking the inbox label off: {sent}");
    assert!(sent.contains("UID MOVE 10 \"Archive\""), "{sent}");
    assert!(!sent.to_uppercase().contains("EXPUNGE") && !sent.contains("\\Deleted"), "nothing deleted: {sent}");
}

#[test]
fn mail_is_sorted_into_the_categories_you_can_see() {
    let receipt = Message { subject: "Your order has shipped".into(), from: "orders@shop.com".into(), ..Default::default() };
    assert_eq!(atlas::mail::category_of(&receipt), "Receipts");
    let news = Message {
        subject: "This week in cameras".into(),
        from: "news@lens.com".into(),
        list_unsubscribe: "<mailto:x@lens.com>".into(),
        ..Default::default()
    };
    assert_eq!(atlas::mail::category_of(&news), "Newsletters");
    assert_eq!(atlas::mail::category_to_delete("delete the noise"), Some("Noise"));
    assert_eq!(atlas::mail::category_to_delete("sort my inbox"), None, "deleting only when told");
    let plan = atlas::mail::SortPlan {
        account: "personal".into(),
        gmail: true,
        moves: vec![(1, "Noise".into()), (2, "Noise".into()), (3, "Receipts".into())],
    };
    assert_eq!(plan.counts(), vec![("Noise".to_string(), 2), ("Receipts".to_string(), 1)]);
}

// ------------------------------------------------------------------ G2

#[test]
fn a_post_is_approved_on_its_words_then_given_a_time_and_only_then_scheduled() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "post");
    let _ = d.execute(&Intent::DraftPost("x".into()));
    // Noon today, not the real clock: between midnight and four "tomorrow"
    // is honestly ambiguous and Atlas asks which (when::parse), so on the
    // real clock this test failed every night (found 26 Sep, 01:39 UTC).
    let t = atlas::store::now() / 86_400 * 86_400 + 12 * 3600;
    // The review is the step that asks for approval of these exact words.
    let said = d.execute(&Intent::ReviewPost("new video goes up tonight at eight".into()));
    assert_eq!(d.publisher.posts.last().map(|p| p.state), Some(atlas::publish::PostState::AwaitingApproval), "{said}");
    let said = d.turn("yes", t + 1);
    assert!(said.contains("When should it go?"), "{said}");
    // Not approved, and not due, until there's a time.
    assert!(d.publisher.due(t + 2, true).is_empty());
    let said = d.turn("tomorrow at 6pm", t + 2);
    assert!(said.starts_with("Scheduled"), "{said}");
    let post = d.publisher.posts.last().unwrap();
    assert_eq!(post.state, atlas::publish::PostState::Scheduled);
    assert!(post.send_at.unwrap() > t);
    // Cancelling works right up to the time.
    let said = d.turn("cancel the post", t + 3);
    assert!(said.contains("Cancelled"), "{said}");
}

// ------------------------------------------------------------------ G3

#[test]
fn a_question_about_another_app_looks_at_that_app() {
    let c = cfg();
    assert_eq!(atlas::probe::target_for("what does discord say", c), atlas::probe::Target::App("discord".into()));
    assert_eq!(atlas::probe::target_for("what's this", c), atlas::probe::Target::Active);
}

// ------------------------------------------------------------------ G4

fn window_with(p: &MockPlatform, win: u64, buttons: &[&str]) {
    use atlas::uia::{Node, Role};
    let tree = Node::new(Role::Window, "Exporter").with(buttons.iter().map(|b| Node::new(Role::Button, b)).collect());
    p.screens.borrow_mut().insert(win, tree);
    *p.front.borrow_mut() = Some(WindowId(win));
    *p.active.borrow_mut() = Some(ActiveWindow { process: "exporter.exe".into(), title: "Exporter".into() });
}

#[test]
fn a_button_is_pressed_by_name_and_one_that_cannot_be_undone_asks_first() {
    assert_eq!(atlas::uia::button_request("click the Export button"), Some(("Export".into(), None)));
    assert_eq!(
        atlas::uia::button_request("press the save button in notepad"),
        Some(("save".into(), Some("notepad".into())))
    );
    assert!(atlas::uia::cannot_be_undone("Send"));
    assert!(!atlas::uia::cannot_be_undone("Export"));

    let c = cfg();
    let p = plat();
    window_with(&p, 4, &["Export", "Send"]);
    let mut d = daemon(c, &p, "button");
    let said = d.turn("click the export button", 1_000);
    assert!(said.contains("Pressed \"export\""), "{said}");
    assert!(p.actions().iter().any(|a| matches!(a, Did::Press(k) if k == "button:export")));

    let said = d.turn("click the send button", 1_001);
    assert!(said.contains("can't be taken back"), "{said}");
    assert!(!p.actions().iter().any(|a| matches!(a, Did::Press(k) if k == "button:send")), "not before the yes");
    let said = d.turn("yes", 1_002);
    assert!(said.contains("Pressed \"send\""), "{said}");

    let said = d.turn("click the frobnicate button", 1_003);
    assert!(said.contains("can't see"), "{said}");
}

// ------------------------------------------------------------------ G5

#[test]
fn a_moved_folder_arrives_whole_and_leaves_a_note_where_it_was() {
    let root = scratch("move");
    let from = root.join("atlas").join("models");
    std::fs::create_dir_all(from.join("sub")).unwrap();
    std::fs::write(from.join("a.gguf"), vec![1u8; 2_000_000]).unwrap();
    std::fs::write(from.join("sub").join("b.onnx"), vec![2u8; 1_000_000]).unwrap();
    let to = root.join("D").join("atlas").join("models");
    let m = atlas::tune::move_folder(&from, &to, 5).unwrap();
    assert_eq!(m.mb, 3);
    assert!(to.join("sub").join("b.onnx").is_file(), "everything arrived");
    assert!(!from.exists(), "and the old one is cleared");
    let note = std::fs::read_to_string(from.with_extension("MOVED.txt")).unwrap();
    assert!(note.contains(&to.display().to_string()), "findable: {note}");
    assert!(atlas::tune::move_folder(&from, &to, 6).is_err(), "nothing left to move");
}

// ------------------------------------------------------------------ G6

#[test]
fn undo_asks_and_then_actually_takes_it_back() {
    assert_eq!(atlas::daemon::undo_intent("opened notepad"), Some(Intent::CloseApp("notepad".into())));
    assert_eq!(atlas::daemon::undo_intent("brought the workspace up"), Some(Intent::WorkspaceOff));
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "undo");
    d.history.note("opened notepad", "windows", atlas::undo::Undo::Atlas("close notepad".into()), true, 10);
    let said = d.turn("undo", 1_000);
    assert!(said.starts_with("Undo \"opened notepad\"?"), "{said}");
    let said = d.turn("yes", 1_001);
    assert!(said.starts_with("Undone: opened notepad."), "{said}");
    assert!(d.history.done.iter().any(|x| x.what == "opened notepad" && x.undone), "marked undone");
}

// ------------------------------------------------------------------ G7

#[test]
fn what_already_happened_and_cannot_be_undone_is_named() {
    use atlas::chain::{Chain, Step};
    let step = |what: &str, reversible: bool| Step {
        what: what.into(),
        app: String::new(),
        reversible,
        goes_out: !reversible,
        needs: None,
        produces: None,
    };
    let mut c = Chain::new("send the numbers", vec![step("copy the numbers", true), step("email Marta", false), step("file it", true)]);
    c.done(None);
    c.done(None);
    assert_eq!(atlas::chain::what_stands(&c), vec!["email Marta"]);
}

// ------------------------------------------------------------------ G8

#[test]
fn the_edit_works_on_a_copy_and_never_writes_over_anything() {
    let (path, wish) = atlas::edit::path_and_wish("edit \"C:\\clips\\my trip.mp4\" to cut the dead air").unwrap();
    assert_eq!(path, "C:\\clips\\my trip.mp4");
    assert_eq!(wish, "cut the dead air");
    let (path, wish) = atlas::edit::path_and_wish("edit this video clip.mov: make it a minute long").unwrap();
    assert_eq!((path.as_str(), wish.as_str()), ("clip.mov", "make it a minute long"));

    let dir = scratch("media");
    let original = dir.join("trip.mp4");
    std::fs::write(&original, b"video").unwrap();
    std::fs::write(dir.join("trip.edited.mp4"), b"an earlier edit").unwrap();
    let (copy, result) = atlas::edit::copy_and_result_paths(&original, &dir.join("work"));
    assert!(copy.starts_with(dir.join("work")), "the copy is Atlas's");
    assert_eq!(result, dir.join("trip.edited-2.mp4"), "never over an earlier edit, never over the original");
}

// ------------------------------------------------------------------ through the daemon
//
// Each of these branches does something to your things, so each is driven
// through the daemon at least on the path that must not act: no mailbox, no
// file, no second drive, nothing to keep at.

#[test]
fn sorting_with_no_mailbox_says_so_and_touches_nothing() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "sort-none");
    let said = d.turn("sort my mailbox", 1_000);
    assert!(said.contains("no mailbox") || said.contains("couldn't get into"), "{said}");
}

#[test]
fn editing_a_file_that_is_not_there_does_nothing() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "edit-none");
    let said = d.execute(&Intent::EditMedia("edit \"/no/such/clip.mp4\" to cut the dead air".into()));
    assert!(said.contains("can't find") || said.contains("model"), "{said}");
}

#[test]
fn moving_big_files_asks_first_or_says_why_not() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "move-none");
    let said = d.execute(&Intent::MoveBigFiles("move my big files".into()));
    assert!(said.ends_with("Go ahead?") || said.contains("no other drive") || said.contains("nothing big"), "{said}");
    assert!(!said.starts_with("Moved"), "never moves before the yes: {said}");
}

#[test]
fn keeping_at_it_with_nothing_given_up_on_says_so() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "keep-none");
    let said = d.execute(&Intent::KeepAtIt);
    assert!(!said.is_empty());
    assert!(!said.contains("checks out"), "it can't have built anything from nothing: {said}");
}
