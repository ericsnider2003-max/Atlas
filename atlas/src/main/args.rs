//! Reading the command line and asking at the terminal: flags, the first bare
//! argument, the listening port, prompts and quiet (passphrase) prompts.
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

/// `data/state` is already where everything else Atlas manages for itself
/// lives -- memory, backups, the trash ledger. Pairings belong next to them,
/// not in `config/`, which stays the folder a person hand-edits.
pub(super) fn pairings_dir() -> std::path::PathBuf {
    atlas::roots::state_dir()
}

/// Pull `--flag value` out of the raw command line. Deliberately not built
/// on the `words`/`flag` split `main` already uses -- that split throws away
/// anything starting with `--` and everything positional gets mixed
/// together, which is fine for a boolean switch like `--dry-run` and not
/// enough for a value like `--host eric-laptop`.
pub(super) fn flag_value(name: &str) -> Option<String> {
    let argv: Vec<String> = std::env::args().collect();
    argv.iter().position(|a| a == name).and_then(|i| argv.get(i + 1)).cloned()
}

/// A bare flag with no value, e.g. `--yes`. Separate from `flag_value` because
/// a boolean flag has nothing after it, and treating it as if it did would
/// swallow the next argument.
pub(super) fn has_flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

/// The first argument after the subcommand name that does not start with
/// `--` and is not the value belonging to a `--flag` immediately before it.
pub(super) fn first_bare_arg(after: &str) -> Option<String> {
    let argv: Vec<String> = std::env::args().collect();
    let start = argv.iter().position(|a| a == after)? + 1;
    let mut i = start;
    while i < argv.len() {
        if argv[i].starts_with("--") {
            i += 2; // skip the flag and its value
            continue;
        }
        return Some(argv[i].clone());
    }
    None
}

/// The port a running daemon binds for peers.
///
/// One function, because the answer was written in three places and two of
/// them were wrong. `run_daemon` had it right; `atlas invite` and
/// `atlas accept` each had their own copy that ignored the config.
pub(super) fn listening_port(cfg: &Config) -> u16 {
    let configured = cfg.tools.as_ref().map(|t| t.kin.port).unwrap_or(0);
    if configured != 0 {
        configured
    } else {
        atlas::kin::DEFAULT_PORT
    }
}

/// A save whose failure you would want to know about.
///
/// `Store::save` writes a temp file and renames it, and it is fallible: a
/// full disk, a read-only folder, a file someone else has open. Thirty-six
/// call sites in this file wrote `keep(x.save(&store), "x");` and then printed a
/// confirmation -- "Added as 3.", "Removed.", "Recorded." -- so a failed
/// write was reported as a success and the change was gone at the next start
/// with nothing said. This file already did it correctly in exactly one
/// place, for the dashboard layout.
///
/// Returns whether it stuck, so a caller can decline to confirm something
/// that did not happen.
pub(super) fn keep<T>(r: atlas::error::Result<T>, what: &str) -> bool {
    match r {
        Ok(_) => true,
        Err(e) => {
            eprintln!("I couldn't save {what}: {e}");
            eprintln!("That change will be gone when I next start.");
            false
        }
    }
}

/// Read a passphrase without echoing it.
///
/// One line, because there is now exactly one implementation of this in the
/// tree: `typed::ask_quietly`. It moved into the library because the daemon
/// needs it too -- "I'm back" has to reach the same prompt `atlas handover
/// back` reaches -- and two copies of the code that reads a secret is one
/// copy too many to keep honest.
pub(super) fn ask_quietly(prompt: &str) -> Option<String> {
    atlas::typed::ask_quietly(prompt)
}

/// One typed line, answered by the daemon where there is one.
///
/// `handle` remains the fallback for a machine with no `config/tools.yaml`,
/// because a prompt that refuses to start is worse than a prompt that can
/// only do six things.
pub(super) fn prompt_line(
    cfg: &Config,
    plat: &dyn Platform,
    parser: &Parser,
    approver: &dyn Approver,
    shell: Option<&mut Daemon>,
    line: &str,
) -> String {
    let Some(d) = shell else {
        return handle(cfg, plat, parser, approver, line);
    };
    let intent = parser.parse(line);
    // The same gate the command-line path uses. The daemon has its own
    // consent path; running both means a gated action is refused by whichever
    // is stricter, which is the direction an error should go.
    //
    // Not for a line the phrase list doesn't know. That is exactly what
    // `turn` exists to answer (through the model), and gating it here
    // printed "blocked: 'unknown' requires explicit approval" for every
    // ordinary question typed at Atlas (found on the laptop, 26 Sep 2026).
    // Nothing unrecognised is ever run: `turn` answers it or says it can't.
    if !matches!(intent, Intent::Unknown(_)) {
        if let Err(e) = gate_with_identity(cfg, &intent, approver) {
            return format!("{e}");
        }
    }
    // `turn`, not `execute_timed`.
    //
    // This used to parse the line against the phrase list and run whatever
    // came back, which meant **typing to Atlas reached the phrase list and
    // nothing else**. `execute_timed`'s own note says it: the one-shot path
    // "parses and executes without going through a turn at all". A sentence
    // the phrase list did not know came back "I didn't catch that", with no
    // model consulted, no context assembled, and nothing written to the
    // conversation -- so the next line you typed could not refer to the last
    // one either.
    //
    // Everything needed was already built and reachable only by voice:
    // `Brain::decide` for a sentence the list doesn't know, `Daemon::context`
    // (focused window, recent files, standing corrections, the conversation
    // so far), and `thread` to record the exchange. `turn` is the door to
    // all of it, and typing was the one way in that didn't use it.
    //
    // `Arrival::Directed`, which `turn` supplies, is also the right reading:
    // a line you typed at the prompt is addressed to Atlas by construction,
    // and the addressing check never drops a directed arrival.
    d.turn(line, atlas::store::now())
}

/// Read one line of an answer from whoever's at the keyboard.
pub(super) fn ask_line(prompt: &str) -> String {
    print!("{prompt}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    atlas::heard!(std::io::stdin().read_line(&mut line));
    line.trim().to_string()
}
