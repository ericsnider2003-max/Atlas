//! Atlas survives a bad turn, and remembers a fatal one.
//!
//! There was no `panic::set_hook`, no `catch_unwind`, no watchdog and no
//! restart anywhere in `src` — the only mention of `catch_unwind` was in
//! `mend.rs`, listing it as a *cheat to detect*. So a single `unwrap` on a
//! malformed file ended the process, the console window closed, nothing
//! brought it back, and the next start greeted you as though nothing had
//! happened. For something meant to be always-on and handed to friends, the
//! person in front of it is usually not the person who could read a stack
//! trace.
//!
//! Three things have to hold, and each is a test below: one bad turn costs a
//! turn and not the session; the crash is written down at the moment it
//! happens, not afterwards; and it is said once, in English, and then
//! cleared.

use atlas::crash::{self, Note};
use atlas::store::Store;
use std::fs;
use std::path::PathBuf;

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-crashtest-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn one_bad_turn_costs_a_turn_and_not_the_session() {
    // The whole point. Before this, the second line here never ran.
    let first = crash::caught("doing the first thing", || panic!("an assumption was wrong"));
    assert!(first.is_err());
    let second = crash::caught("doing the second thing", || "still here");
    assert_eq!(second, Ok("still here"), "the session ended with the first failure");
}

#[test]
fn the_sentence_a_person_hears_is_not_the_one_a_compiler_wrote() {
    let why = crash::caught("answering you", || panic!("called `Option::unwrap()` on a `None` value"))
        .unwrap_err();
    assert!(why.contains("answering you"), "it does not say what it was doing: {why}");
    for leak in ["unwrap", "Option", "panic", "None"] {
        assert!(!why.contains(leak), "{leak:?} reached the person: {why}");
    }
}

#[test]
fn a_crash_is_written_down_and_read_back_once() {
    let root = temp("roundtrip");
    let store = Store::new(&root);

    // No crash is not an error, and must not produce a report.
    assert!(crash::last(&store).is_none());

    let note = Note {
        at: 1_000,
        what: "index out of bounds".into(),
        where_: "src/somewhere.rs:42".into(),
        during: "reading your notes".into(),
    };
    fs::write(crash::note_path(store.root()), serde_json::to_string(&note).unwrap()).unwrap();

    // `last` may be asked twice and answer the same — that is what
    // `atlas crash` needs.
    assert_eq!(crash::last(&store).as_ref(), Some(&note));
    assert_eq!(crash::last(&store).as_ref(), Some(&note));

    // `take` is the spoken path: said once, then gone, or you are told about
    // the same crash every morning for the rest of the install's life.
    assert_eq!(crash::take(&store).as_ref(), Some(&note));
    assert!(crash::last(&store).is_none(), "the crash would be reported forever");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_note_lives_beside_the_state_and_not_in_it() {
    // A file of its own, because it is written from inside a panic hook —
    // when the program's invariants are already broken and anything clever
    // may itself panic. A panic inside a panic hook aborts the process.
    let root = temp("where");
    let store = Store::new(&root);
    let p = crash::note_path(store.root());
    assert!(p.starts_with(store.root()), "the note escapes the store: {p:?}");
    assert_eq!(p.extension().and_then(|e| e.to_str()), Some("json"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_daemon_catches_its_own_tick() {
    // Structural, because reaching it for real needs a panicking intent and
    // a running daemon. What must hold is that the tick is inside the
    // boundary and that a failure becomes a line rather than an exit.
    let daemon = crate::common::source_of("daemon");
    let flat: String = daemon.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("crash::caught(") && flat.contains("self.tick(t)"),
        "the tick is no longer inside a catch boundary — one bad intent ends \
         the session again"
    );
    assert!(
        flat.contains("crash::take(&self.store)"),
        "the daemon no longer reports a crash from last time"
    );
}

#[test]
fn the_hook_is_installed_before_anything_can_panic() {
    let main = crate::common::source_of("main");
    let at_hook = main.find("crash::watch(").expect("no panic hook is installed at all");
    // Anything that reads config or opens the store can panic, so the hook
    // has to be in front of them.
    let at_cfg = main.find("Config::load(&dir)").expect("config load moved");
    assert!(
        at_hook < at_cfg,
        "the hook is installed after config loading, so a crash while reading \
         a broken config leaves nothing behind"
    );
}

#[test]
fn there_is_a_way_to_read_the_crash_after_it_has_been_spoken() {
    // The spoken line clears itself, which is right for the daemon and
    // useless for working out what broke. `atlas crash` is the other half,
    // and it must not clear.
    let main = crate::common::source_of("main");
    assert!(main.contains("Some(\"crash\")"), "`atlas crash` is gone");
    let at = main.find("Some(\"crash\")").unwrap();
    let body = &main[at..at + 900];
    assert!(body.contains("crash::last("), "`atlas crash` should read without clearing");
    assert!(
        !body.contains("crash::take("),
        "`atlas crash` clears the note, so asking twice loses it"
    );
}

#[test]
fn a_price_series_with_a_hole_in_it_is_refused_rather_than_sorted() {
    // The unwrap that would actually have fired. `Bars::new` validated length
    // and raggedness and never finiteness — the finiteness check lived in
    // `feed::check_ohlc`, reachable only through `feed::accept`. A `Bars`
    // built any other way carried NaN into `partial_cmp().unwrap()` and ended
    // the process.
    use atlas::market::bars::Bars;
    let good = Bars::new(
        vec![1.0, 2.0],
        vec![1.5, 2.5],
        vec![0.5, 1.5],
        vec![1.2, 2.2],
        vec![1, 2],
    );
    assert!(good.is_ok(), "a sound series was refused");

    for (label, bad) in [
        ("NaN", f64::NAN),
        ("infinity", f64::INFINITY),
        ("negative infinity", f64::NEG_INFINITY),
    ] {
        let r = Bars::new(
            vec![1.0, 2.0],
            vec![1.5, 2.5],
            vec![0.5, 1.5],
            vec![1.2, bad],
            vec![1, 2],
        );
        assert!(r.is_err(), "a series containing {label} was accepted");
        let why = format!("{:?}", r.err().unwrap()).to_lowercase();
        assert!(
            why.contains("not a number") || why.contains("close"),
            "the refusal does not say which column was wrong: {why}"
        );
    }
}

#[test]
fn sorting_prices_can_no_longer_end_the_process() {
    // Belt as well as braces: even if something builds a series another way,
    // the comparisons themselves are now a total order.
    let src = fs::read_to_string("src/market/levels.rs").expect("src/market/levels.rs");
    assert!(
        !src.contains("partial_cmp") || !src.contains("unwrap()"),
        "a price sort can panic again"
    );
    assert!(src.contains("total_cmp"), "the total ordering is gone");
}

#[test]
fn reading_a_model_file_cannot_end_the_process() {
    // `.gguf` is a file someone downloaded. Two `try_into().unwrap()` calls
    // sat behind a length invariant held by a literal on the line above.
    let src = fs::read_to_string("src/gguf.rs").expect("src/gguf.rs");
    // Code only. The first version of this matched the whole file and failed
    // on the *doc comment explaining the fix* — a guard tripping over its own
    // explanation is the same fault as a guard passing because of one, and
    // this tree has now had both.
    let code: String = src
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with("///") && !t.starts_with('*')
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains("try_into().unwrap()"),
        "a truncated or hostile model file can end the process again"
    );
    assert!(src.contains("ended in the middle of a number"), "the honest error is gone");
}
