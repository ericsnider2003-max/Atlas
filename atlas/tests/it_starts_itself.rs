//! Atlas starts when you log in, so there is nothing to click.
//!
//! **Eric's ruling, 17 Sep 2026: yes, in the background.**
//!
//! Nothing in this tree created a startup entry before today, while two
//! modules were written as though one existed — `onlyone.rs` opens with
//! "Start it from the shortcut", and `doctor.rs` explains what a shortcut
//! with the wrong `Start in` used to break. Neither could have been true,
//! because nothing made a shortcut.
//!
//! ## What is tested here, and what cannot be
//!
//! The **decision** — which command, which flag, how the path is quoted — is
//! tested exactly. So is the three-way contract of `startup::run`, against
//! real programs on this machine, because getting "it ran and said no" mixed
//! up with "it could not run" is how a status command starts lying.
//!
//! The **Windows registration has never been executed.** There is no Windows
//! host on the build side.
//! Running `atlas startup on` once on the VPS is the whole check, and it
//! should print the `schtasks` line before it runs it, so the command can be
//! read rather than trusted.

use atlas::startup::{self, Mode, Plan};
use std::fs;
use std::path::PathBuf;

fn main_rs() -> String {
    crate::common::source_of("main")
}

// ================= the quoting =================

#[test]
fn the_program_path_is_quoted_because_real_installs_have_spaces() {
    // `startup.rs` tests this in its own `#[cfg(test)]` module too, and that
    // is not enough: `dead_capabilities.rs` reads `tests/` only, so it counted
    // `task_command` as a helper nothing tests and the untested-helper count
    // went 27 -> 28. It was right to. In-module tests are written by whoever
    // wrote the function, at the same moment, with the same assumptions; the
    // integration suite is where a rule gets checked from outside.
    //
    // And this is the rule most worth checking from outside. `schtasks /TR`
    // takes the whole command as ONE string, so an unquoted
    // `C:\Program Files\Atlas\atlas.exe` is accepted at create time and fails
    // at logon with a file-not-found that appears only as a row in Task
    // Scheduler. Nobody would ever see it; Atlas would simply not be running.
    let exe = PathBuf::from(r"C:\Program Files\Atlas\atlas.exe");
    let cmd = startup::task_command(&exe, Mode::Background);

    assert!(cmd.starts_with('"'), "the program path is not quoted: {cmd}");
    assert_eq!(
        cmd, r#""C:\Program Files\Atlas\atlas.exe" --daemon"#,
        "the quoting moved; this is the exact string schtasks is handed"
    );

    // The closing quote goes after the path and before the flag. Getting that
    // backwards quotes the flag into the filename and is the same failure.
    let close = cmd.rfind('"').expect("a closing quote");
    assert!(
        cmd[close..].contains("--daemon"),
        "the flag is inside the quoted path, so the program name includes it: {cmd}"
    );

    // A path without spaces is quoted too, deliberately: one shape for both
    // means the one with spaces is never the untested branch.
    // `--wake` since 29 Sep 2026: `--voice` is the press-Enter-to-talk loop,
    // and at sign-in there is no keyboard, so it recorded clip after clip.
    let plain = startup::task_command(&PathBuf::from("/opt/atlas/atlas"), Mode::Listening);
    assert_eq!(plain, "\"/opt/atlas/atlas\" --wake");
}

// ================= the run contract =================

#[test]
fn a_command_that_succeeds_is_reported_as_success() {
    // `true` and `false` exist on every unix and are the cleanest possible
    // probe of the contract. Skipped on Windows rather than faked.
    if cfg!(windows) {
        return;
    }
    let ok = Plan { program: "true".into(), args: vec![], what: "succeed".into() };
    assert_eq!(startup::run(&ok), Ok(true));
}

#[test]
fn a_command_that_refuses_is_not_reported_as_a_failure_to_run() {
    // This is the distinction that matters. `schtasks /Query` on a task that
    // is not registered *fails*, and that is the normal answer to "does Atlas
    // start at logon?" -- no. Collapsing it into an error would make `atlas
    // startup status` say "I couldn't ask" for the commonest case.
    if cfg!(windows) {
        return;
    }
    let refused = Plan { program: "false".into(), args: vec![], what: "refuse".into() };
    assert_eq!(
        startup::run(&refused),
        Ok(false),
        "a command that ran and said no is being reported as a failure to run it"
    );
}

#[test]
fn a_program_that_is_not_there_is_an_error_rather_than_a_no() {
    let missing = Plan {
        program: "atlas-definitely-not-a-real-program-9f3a".into(),
        args: vec![],
        what: "not exist".into(),
    };
    match startup::run(&missing) {
        Err(e) => assert!(e.contains("couldn't run"), "unhelpful error: {e}"),
        Ok(v) => panic!("a missing program reported Ok({v}) -- status would be a fiction"),
    }
}

// ================= the unit file, written for real =================

#[test]
fn the_unit_file_is_something_systemd_would_accept() {
    // Written to disk and read back, rather than asserted as a string, so a
    // change that breaks the shape shows up here. Not installed -- enabling a
    // user unit needs a session bus this container does not have, and that is
    // named rather than worked around.
    let exe = PathBuf::from("/opt/atlas/atlas");
    let text = startup::unit_file(&exe, Mode::Background);
    let dir = std::env::temp_dir().join(format!("atlas-unit-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("atlas.service");
    fs::write(&path, &text).expect("write the unit");
    let back = fs::read_to_string(&path).expect("read it back");

    // The three lines that decide whether it does anything.
    let mut saw_exec = false;
    let mut saw_wanted = false;
    let mut section = String::new();
    for line in back.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            section = l.to_string();
        }
        if l.starts_with("ExecStart=") {
            assert_eq!(section, "[Service]", "ExecStart is in the wrong section");
            assert!(l.contains("--daemon"), "the unit does not start the background mode: {l}");
            assert!(l.contains("/opt/atlas/atlas"), "the unit starts the wrong program: {l}");
            saw_exec = true;
        }
        if l.starts_with("WantedBy=") {
            assert_eq!(section, "[Install]", "WantedBy is in the wrong section, so `enable` is a no-op");
            saw_wanted = true;
        }
    }
    assert!(saw_exec, "no ExecStart, so the unit runs nothing:\n{back}");
    assert!(saw_wanted, "no WantedBy, so enabling it does nothing at login:\n{back}");

    let _ = fs::remove_dir_all(&dir);
}

// ================= the wiring =================

#[test]
fn there_is_a_command_for_it_and_main_dispatches_it() {
    let src = main_rs();
    assert!(
        src.contains(r#"== Some("startup")"#),
        "`atlas startup` is not dispatched, so the module is unreachable"
    );
    assert!(src.contains("fn run_startup("), "the startup command dispatches to nothing");
}

#[test]
fn it_says_what_it_will_do_before_it_does_it() {
    // Registering a logon task means this machine starts something holding
    // the microphone every time you sign in. The person agreeing to that
    // should see the command, not a sentence claiming one was run.
    let src = main_rs();
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let body = src
        .split_once("fn run_startup(")
        .expect("run_startup")
        .1
        .split("\n}\n")
        .next()
        .unwrap_or_default()
        .to_string();
    assert!(
        body.contains("plan.as_typed()"),
        "the exact command is never shown, so `atlas startup on` asks you to trust it"
    );
    assert!(
        body.contains("mode.plainly()"),
        "it never says in plain words what will happen at logon"
    );
    assert!(
        body.contains("Nothing was changed"),
        "a failure does not say that nothing changed, which is the thing you need to know"
    );
}

#[test]
fn it_uses_its_own_absolute_path_rather_than_how_it_was_invoked() {
    // A logon task starts in `system32`. Anything relative resolves there,
    // and before `roots::decide()` read `current_exe()` that meant Atlas
    // would come up with an empty memory and write a `data/` tree into
    // `system32` without reporting anything wrong.
    let src = main_rs();
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let body = src
        .split_once("fn run_startup(")
        .expect("run_startup")
        .1
        .split("\n}\n")
        .next()
        .unwrap_or_default()
        .to_string();
    assert!(
        body.contains("current_exe()"),
        "the registered command is not built from `current_exe()`, so the task may \
         point at a different install -- or at nothing"
    );
    assert!(
        !body.contains("\"atlas.exe\""),
        "a bare program name is being registered, which resolves against system32"
    );
}

#[test]
fn turning_it_off_is_offered_wherever_it_is_turned_on() {
    // A thing that starts itself and does not say how to stop is the shape of
    // software people uninstall.
    let src = main_rs();
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let body = src
        .split_once("fn run_startup(")
        .expect("run_startup")
        .1
        .split("\n}\n")
        .next()
        .unwrap_or_default()
        .to_string();
    assert!(
        body.contains("atlas startup off"),
        "nothing tells you how to undo it"
    );
}

#[test]
fn the_windows_path_is_not_claimed_to_have_been_run() {
    // The honesty check. This module's own docs must keep saying the Windows
    // registration is untested, for as long as that is true -- the same
    // standing caveat the cross-compiled .exe carries.
    let src = fs::read_to_string("src/startup.rs").expect("src/startup.rs");
    assert!(
        src.contains("never been executed"),
        "the note saying the Windows path has never been run has been removed. If it \
         has actually been run on Windows now, say so with the date instead of \
         deleting the caveat"
    );
}
