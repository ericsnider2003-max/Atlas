//! A panic while holding a lock no longer switches that part of Atlas off for
//! the rest of the run (5 Oct 2026 audit, Q16).
//!
//! `lock()` fails for good once a thread panicked holding it. About 170
//! places answered that with `if let Ok(..)` or `.ok()` and skipped their
//! work, every time, silently. They now take the lock anyway
//! (`crash::unpoison`): the panic is caught and said where it happened, and
//! the data it left is still the data.

use std::sync::{Arc, Mutex};

#[test]
fn a_lock_a_panic_poisoned_is_still_taken_with_its_data() {
    let m = Arc::new(Mutex::new(vec![1, 2]));
    let m2 = m.clone();
    let _ = std::thread::spawn(move || {
        let mut g = m2.lock().unwrap();
        g.push(3);
        panic!("a bug elsewhere, holding the lock");
    })
    .join();
    assert!(m.is_poisoned());
    let got = m.lock().or_else(atlas::crash::unpoison);
    assert_eq!(got.map(|g| g.clone()).ok(), Some(vec![1, 2, 3]), "the work behind the lock was skipped");
}

#[test]
fn no_lock_failure_is_skipped_silently_any_more() {
    // The shapes that skipped: `if let Ok(..) = x.lock()`, `let Ok(..) =
    // x.lock() else`, and `x.lock().ok()`, outside tests.
    let mut skipping = Vec::new();
    for (f, text) in crate::common::source_file_set() {
        for (n, line) in text.lines().enumerate() {
            if line.contains("#[cfg(test)]") {
                break;
            }
            let l = line.trim_start();
            if l.starts_with("//") || !l.contains(".lock()") || l.contains("unpoison") || l.contains("stdin") {
                continue;
            }
            let skips = l.contains(".lock().ok()")
                || ((l.contains("if let Ok(") || l.contains("let Ok(")) && l.contains(".lock()"));
            if skips {
                skipping.push(format!("src/{f}.rs:{}: {l}", n + 1));
            }
        }
    }
    assert!(skipping.is_empty(), "a poisoned lock skips work silently here -- use `.or_else(crate::crash::unpoison)`:\n{}", skipping.join("\n"));
}
