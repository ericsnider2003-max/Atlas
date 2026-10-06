//! Every command, end to end (5 Oct 2026, audit Q8).
//!
//! On 5 Oct, 47 of the 162 commands in `config/commands.yaml` had no test
//! that said their words to Atlas and looked at what came back -- among them
//! all ten call-recording consent commands, pairing, sign-in and the self-test.
//! A unit test of the part behind a command says that part works; it doesn't
//! say the words reach it. This does, for every command, read from the same
//! file Atlas reads, so a command added tomorrow is covered the day it lands:
//!
//! 1. its first phrase is understood as that command and not a neighbour
//!    ("longest phrase wins" is easy to break by adding a phrase elsewhere);
//! 2. said to a real daemon (a mock screen, an empty store, no model), it
//!    answers in words, without panicking, and within a deadline.
//!
//! The answer itself isn't judged here: a command that needs an account or a
//! model says so, which is a correct answer on a machine with neither. Each
//! command's own tests judge its answer.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

#[derive(serde::Deserialize)]
struct Commands {
    commands: Vec<Command>,
}

#[derive(serde::Deserialize)]
struct Command {
    intent: String,
    #[serde(default)]
    phrases: Vec<String>,
    #[serde(default)]
    takes_argument: bool,
    #[serde(default)]
    argument_optional: bool,
}

fn commands() -> Vec<Command> {
    let text = std::fs::read_to_string("config/commands.yaml").expect("config/commands.yaml");
    serde_yaml::from_str::<Commands>(&text).expect("commands.yaml parses").commands
}

/// What to say for a command: its first phrase, with something to act on
/// when it needs one -- of the kind it acts on, since "open the weather in
/// paris" is rightly not understood. Said bare when the argument is optional.
fn said(c: &Command) -> Option<String> {
    let p = c.phrases.first()?;
    if !c.takes_argument || c.argument_optional {
        return Some(p.clone());
    }
    let arg = match c.intent.as_str() {
        "open_app" | "close_app" | "focus_app" => "notepad",
        "draft_post" => "twitter",
        "set_mode" => "focus",
        "change_group" => "Sam to the Friends group",
        "name_this" => "mug",
        _ => "the weather in paris",
    };
    Some(format!("{p} {arg}"))
}

fn daemon(tag: &str) -> (atlas::daemon::Daemon<'static>, std::path::PathBuf) {
    let cfg: &'static atlas::config::Config = Box::leak(Box::new(atlas::config::Config::load(Path::new("config")).expect("config")));
    let plat = Box::leak(Box::new(atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])));
    let dir = std::env::temp_dir().join(format!("atlas-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let d = atlas::daemon::Daemon::new(
        cfg,
        plat,
        None,
        atlas::store::Store::new(dir.clone()),
        atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()),
    );
    (d, dir)
}

#[test]
fn every_command_is_understood_as_itself() {
    let cfg = atlas::config::Config::load(Path::new("config")).expect("config");
    let parser = atlas::intent::Parser::new(&cfg.commands);
    let mut wrong = Vec::new();
    for c in commands() {
        let Some(s) = said(&c) else { continue };
        let got = parser.parse(&s);
        if matches!(got, atlas::intent::Intent::Unknown(_)) {
            wrong.push(format!("{}: \"{s}\" -> not understood", c.intent));
            continue;
        }
        // What the command's own name builds, to compare the kind of intent
        // (commands the model may never choose have no tool form; the parse
        // not being Unknown is the check for those).
        if let Some(want) = atlas::intent::from_tool(&c.intent, &serde_json::Value::String("the weather in paris".into()), &s) {
            if std::mem::discriminant(&want) != std::mem::discriminant(&got) {
                wrong.push(format!("{}: \"{s}\" -> {got:?}", c.intent));
            }
        }
    }
    assert!(wrong.is_empty(), "{} command(s) not understood as themselves:\n{}", wrong.len(), wrong.join("\n"));
}

/// The install-wide state (handed over or not, pairing, the lock), cleared
/// so each command starts from a fresh install: "hand over" said for real
/// would otherwise leave every command after it talking to a guest's Atlas.
///
/// Only ever this test process's own folder. On 5 Oct this state was the
/// checkout's real `data/state` whenever the build folder was outside the
/// checkout (`roots::is_a_test_binary`), and this test left it handed over.
/// So it checks rather than trusts, and refuses to touch anything else.
fn fresh_install_state() {
    let dir = atlas::roots::state_dir();
    let ours = dir.components().any(|c| c.as_os_str().to_string_lossy().starts_with(&format!("atlas-test-{}", std::process::id())));
    assert!(ours, "install state is {} -- not this test's own folder, so a command said here would change a real Atlas", dir.display());
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clear this test's install state");
    }
}

#[test]
fn every_command_answers_when_said() {
    let t = 1_790_500_000;
    let mut wrong = Vec::new();
    for c in commands() {
        let Some(s) = said(&c) else { continue };
        fresh_install_state();
        let intent = c.intent.clone();
        let (tx, rx) = mpsc::channel();
        let words = s.clone();
        std::thread::Builder::new()
            .name(format!("e2e-{intent}"))
            .spawn(move || {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let (mut d, dir) = daemon(&intent);
                    let reply = d.turn(&words, t);
                    drop(d);
                    let _ = std::fs::remove_dir_all(&dir);
                    reply
                }));
                let _ = tx.send(r.map_err(|p| p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default()));
            })
            .expect("thread");
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(Ok(reply)) if !reply.trim().is_empty() => {}
            Ok(Ok(_)) => wrong.push(format!("{}: \"{s}\" -> an empty reply", c.intent)),
            Ok(Err(why)) => wrong.push(format!("{}: \"{s}\" -> panicked: {why}", c.intent)),
            Err(_) => wrong.push(format!("{}: \"{s}\" -> no answer in 60 s", c.intent)),
        }
    }
    assert!(wrong.is_empty(), "{} command(s) failed end to end:\n{}", wrong.len(), wrong.join("\n"));
}
