//! Q4 residuals: replies on user-visible success paths must not claim a
//! success whose record could not be written.

use atlas::feedback::{compose_feedback, queue_feedback};
use atlas::store::Store;

#[test]
fn feedback_is_not_called_queued_when_the_outbox_cannot_be_written() {
    let base = std::env::temp_dir().join(format!("atlas-q4-fb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_file(&base);
    std::fs::write(&base, b"a file, where the state folder would go").unwrap();
    let store = Store::new(base.join("state"));
    let f = compose_feedback("the hub goes blank", None, 100).unwrap();
    let got = queue_feedback(&store, f, "Eric");
    assert!(got.is_err(), "claimed to queue feedback it could not write");
    assert!(got.unwrap_err().contains("nothing was sent"));
}

#[test]
fn feedback_is_queued_when_the_disk_works() {
    let d = std::env::temp_dir().join(format!("atlas-q4-fb-ok-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let store = Store::new(&d);
    let f = compose_feedback("the hub goes blank", None, 100).unwrap();
    queue_feedback(&store, f, "Eric").unwrap();
    assert_eq!(atlas::feedback::feedback_outbox(&store).len(), 1);
}
