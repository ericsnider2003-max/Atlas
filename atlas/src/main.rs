// A windowed program on Windows, so opening Atlas never opens a command
// prompt (Eric, 27 Sep 2026). Typed in a terminal, it borrows that terminal
// for what it prints (`firstlaunch::started_without_a_terminal`).
#![cfg_attr(all(windows, feature = "desktop-ui"), windows_subsystem = "windows")]
use atlas::config::Config;
use atlas::brain::{self, Brain, Llm, ShellLlm};
use atlas::daemon::{Autonomy, Daemon};
use atlas::input::Keyboard;
use atlas::perf::Throttle;
use atlas::proactive::Proactive;
use atlas::doctor;
use atlas::intent::{Intent, Parser};
use atlas::platform::{mock::MockPlatform, Monitor, Platform};
use atlas::policy::{self, AllowAll, Approver, DenyAll};
use atlas::voice::Voice;
use atlas::workspace;
use std::io::{self, Write};

// --- the commands, split by topic (29 Sep 2026, docs/refactor-plan-daemon-split.md §7).
// `src/main/<x>.rs`, declared with a path because a crate root's `mod x;`
// looks in `src/x.rs`. Each child starts `use super::*;` and main reaches
// its commands through the `use x::*;` beside each `mod`.
#[path = "main/serving.rs"]
mod serving;
use serving::*;
#[path = "main/args.rs"]
mod args;
use args::*;
#[path = "main/peers.rs"]
mod peers;
use peers::*;
#[path = "main/trading.rs"]
mod trading;
use trading::*;
#[path = "main/secrets.rs"]
mod secrets;
use secrets::*;
#[path = "main/updating.rs"]
mod updating;
use updating::*;
#[path = "main/carrying.rs"]
mod carrying;
use carrying::*;
#[path = "main/media.rs"]
mod media;
use media::*;
#[path = "main/hubcmds.rs"]
mod hubcmds;
use hubcmds::*;
#[path = "main/everyday.rs"]
mod everyday;
use everyday::*;
#[path = "main/setup.rs"]
mod setup;
use setup::*;

const USAGE: &str = "\
atlas — local workspace assistant

  atlas doctor                    inspect this machine, print config to paste
  atlas \"boot workspace\"          run one command
  atlas                           interactive prompt
  atlas --voice                   voice loop, press Enter to talk
  atlas get kokoro                download the Kokoro voice (about 141 MB)
  atlas kokoro-check [words]      speak once in Kokoro into a file, and time it
  atlas --wake                    hands-free, waits for the wake phrase
  atlas --daemon                  always-on: wake word, conversation,
                                  scheduled work, proactive offers
  atlas --daemon --unattended     same, but nothing needing consent runs

  atlas friend [link|add <link>|requests|accept <name>|decline <name>|request <name>]
                                   add a friend in one step: make a link and
                                   send it, or paste theirs -- nothing to send
                                   back. Requests go through a group you share
  atlas invite <their-name> --as <your-name> --host <your-tailscale-name>
                                   pair with another Atlas -- prints a block
                                   to send them, no file-editing needed
  atlas accept <code>              paste what someone sent you to complete
                                   a pairing -- may print a block back
                                   (run with no arguments for a walkthrough)
  atlas index [status|show|rebuild]
                                   what Atlas knows it has written down, and
                                   whether that still matches the folder
  atlas trace [status|failures|compact]
                                   every model call: who asked, how long, what
                                   failed -- never the words of a prompt
  atlas craft <dir>                run the build ladder for the project in
                                   <dir> and say exactly what's wrong,
                                   cheapest signal first -- compile errors
                                   before test failures, never both at once

  atlas carry [list|pack <file>...|missing <name>|back <folder>]
                                   what travels with the work when you walk
                                   out, decided against a real byte budget
  atlas remote [list|ask <what>|start|done|failed|drop <number>]
                                   things that need the other machine, and
                                   whether that machine is actually answering
  atlas mobile [ios|android|mirror|back]
                                   what Atlas can and cannot do on a phone,
                                   and what the mirror would really carry
  atlas money [<export.csv>] [--from file|export|feed] [--keep]
                                   read a statement you exported yourself,
                                   sort it, and say the biggest thing you
                                   could change -- never the biggest thing
  atlas away [on <YYYY-MM-DD>|clear|check]
                                   a trip your texts won't reach you on, and
                                   what would lock you out from there
  atlas codes [list|have|in-hand|used|where]
                                   recovery codes: how many you have, how many
                                   are actually printed and on you
  atlas access [list|give|take|take-all|changed]
                                   which sites Atlas can sign you into, and
                                   taking that away. A grant points at a vault
                                   entry -- no password is stored here
  atlas walkthrough [prepare|turn-off <site>...|yes|no|next|skip]
                                   opens the exact settings page and says
                                   where the switch is. A change that leaves
                                   an account less protected is read back and
                                   waits for a yes, one at a time
  atlas afterme [where|told|confirmed|timer|reviewed|shape]
                                   the sealed envelope, who knows where it is,
                                   and what is still missing from the
                                   arrangement -- never the passphrase itself
  atlas catalog [--platform windows|mac|linux|ios|android|web]
                [--module <name>]
                                   everything Atlas does, what state each is
                                   in, and what each would do on another
                                   platform -- worked out from what it needs
                                   rather than written down twice
  atlas video [check|fix|cut|render|export|voiceover|tools|presets|music]
                                   measure a clip, judge it, cut it, and get
                                   it out at what the platform actually wants
  atlas content [review|record|posts|learn|reach|edits]
                                   what a piece is likely to do, and what
                                   actually happened once it was out
  atlas budget [status|would|night|rates]
                                   what a hosted model would cost, before it
                                   costs it
  atlas install-page <Atlas.ipa|Atlas.apk> [--friends] [--minutes N]
                                   a phone build on a page the phone
                                   installs it from, over your Tailscale --
                                   no Mac, switched off when it closes
  atlas sync-setup [onedrive|icloud|dropbox|google|folder]
                                   the folder two devices meet in -- what
                                   Atlas does, what only you can do

  crash       what happened the last time I stopped unexpectedly

  --health-check  check this build can run here (what an update asks a new
                  build before starting it; it goes back if the answer is no)

  --dry-run   use the fake OS, never touch real windows
  --yes       auto-approve gated actions
";

thread_local! {
    /// Set when this start is a build on trial, so a normal return ends the trial.
    static TRIAL_ROOT: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Dropped when `main` returns. A normal return is a start that got through;
/// unwinding from a panic is not.
struct TrialGuard;

/// Every deliberate way out of Atlas once it has started: the start got
/// through, whatever the exit code says. An exit code of 1 or 2 here means
/// "you asked for something I can't do" (a typo, a missing argument, a
/// refused command) -- the program working as intended, not a broken build.
/// `std::process::exit` skips `TrialGuard`, so before this existed three
/// mistyped commands in a row during a new build's trial rolled back a build
/// that was fine. What still counts against a trial: a crash, being killed,
/// and settings that won't load -- the one early exit that means the build
/// itself can't run here, which keeps calling `std::process::exit` directly.
/// A start of a build on trial got through: end the trial, and if the build
/// came through the update courier, record it as installed (`update_apply`).
fn settle_update(root: &std::path::Path) {
    if atlas::upgrade::trial_passed(root) {
        let _ = atlas::update_apply::finish_after_start(&atlas::roots::store(), root, &atlas::upgrade::this_tag());
    }
}

fn leave(code: i32) -> ! {
    if let Some(root) = TRIAL_ROOT.with(|t| t.borrow_mut().take()) {
        settle_update(&root);
    }
    std::process::exit(code)
}

impl Drop for TrialGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        if let Some(root) = TRIAL_ROOT.with(|t| t.borrow_mut().take()) {
            settle_update(&root);
        }
    }
}

fn main() {
    // First, before anything is printed: join the terminal Atlas was typed
    // in, if it was (it's a windowed program on Windows, with no console of
    // its own). The answer is kept for the rest of `main`.
    let _ = atlas::firstlaunch::started_without_a_terminal();
    // Atlas writes UTF-8 (curly quotes, dashes). A Windows console left on
    // its old code page shows each of those as two or three junk characters
    // (seen on the laptop, 26 Sep 2026). Tell the console it's UTF-8.
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::System::Console::SetConsoleOutputCP(65001);
    }
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| argv.iter().any(|a| a == f);
    let words: Vec<String> =
        argv.iter().filter(|a| !a.starts_with("--")).cloned().collect();

    if flag("--help") || flag("-h") {
        print!("{USAGE}");
        return;
    }

    // `setup/reference/RUN-TESTS.bat` has run `atlas.exe --version` since the
    // day it was written. Nothing handled it: `--version` was filtered out of
    // `words` as a flag, `words` came out empty, and Atlas dropped into the
    // interactive prompt and **blocked on stdin** — a script that looked like
    // it hung. The version has always been printable; only `atlas update`
    // ever printed it.
    // A new Atlas dropped into updates/ goes in now, before anything else
    // runs, and the new one is started in this one's place. Not while it is
    // itself being asked for its version, which is how the check works.
    // The overlay and the typing box are helpers the background Atlas starts;
    // they never swap in an update or count a trial start (29 Sep 2026: each
    // helper start counted as a start of the build on trial, so a build that
    // could not get through was never rolled back, and a helper could swap
    // the program file out from under the Atlas that started it).
    let helper = matches!(words.first().map(|s| s.as_str()), Some("overlay") | Some("typebox"));
    if !helper && std::env::var_os("ATLAS_UPDATE_PROBE").is_none() && !flag("--version") && !flag("-V") {
        let root = atlas::roots::install_root();
        if let Ok(running) = std::env::current_exe() {
            // The new build checks itself before it is started, and the
            // running one goes straight back if it can't (O1).
            let timeout = std::time::Duration::from_secs(atlas::upgrade::HEALTH_TIMEOUT_SECS);
            match atlas::upgrade::swap_checked(&root, &running, timeout) {
                Some(Ok(atlas::upgrade::Swapped::Started { path, version, previous, .. })) => {
                    eprintln!(
                        "Updated to {version}, the Atlas in updates/. It passed its check here; the old one is kept as {} \
                         and comes back by itself if {version} can't start {} times running.",
                        atlas::upgrade::keep_old_at(&root, &previous).display(),
                        atlas::upgrade::TRIAL_STARTS
                    );
                    let status = atlas::tools::command(&path).args(std::env::args().skip(1)).status();
                    std::process::exit(status.ok().and_then(|s| s.code()).unwrap_or(0));
                }
                Some(Ok(atlas::upgrade::Swapped::RolledBack { version, tag, why })) => eprintln!(
                    "(Not updating: {version} failed its check here -- {why}. Staying on {}; {version} is set aside as {} and won't be tried again.)",
                    atlas::upgrade::version(),
                    atlas::upgrade::failed_at(&root, &tag).display()
                ),
                Some(Err(why)) => eprintln!("(Not updating: {why}.)"),
                None => {}
            }
            // A build on trial counts this start; too many that never got
            // through and the previous one is put back and started instead.
            match atlas::upgrade::trial_on_start(&root, &running) {
                atlas::upgrade::TrialStep::RolledBack { failed, previous } => {
                    eprintln!(
                        "{failed} couldn't get through {} starts, so I went back to {previous}. {failed} is set aside as {}.",
                        atlas::upgrade::TRIAL_STARTS,
                        atlas::upgrade::failed_at(&root, &failed).display()
                    );
                    let status = atlas::tools::command(&running).args(std::env::args().skip(1)).status();
                    std::process::exit(status.ok().and_then(|s| s.code()).unwrap_or(0));
                }
                atlas::upgrade::TrialStep::Trying(_) => {
                    // Got through if it returns normally or stays up long enough.
                    let r = root.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(atlas::upgrade::HEALTHY_AFTER_SECS));
                        settle_update(&r);
                    });
                    TRIAL_ROOT.with(|t| *t.borrow_mut() = Some(root.clone()));
                }
                atlas::upgrade::TrialStep::Settled => {
                    // A build that isn't on trial: record how a staged update
                    // went (installed, or rolled back and never tried again).
                    // Only when something was staged: the running build's
                    // fingerprint is worked out from the whole program file.
                    let store = atlas::roots::store();
                    if atlas::update_apply::pending(&store).is_some() {
                        let _ = atlas::update_apply::finish_after_start(&store, &root, &atlas::upgrade::this_tag());
                    }
                }
            }
        }
    }
    // Ends a trial when `main` returns normally (not on a panic).
    let _trial = TrialGuard;

    // Asked by the build it is replacing: check yourself and say so.
    if flag("--health-check") {
        let root = atlas::roots::install_root();
        match atlas::upgrade::health_check(&root, &atlas::roots::config_dir(), &atlas::roots::state_dir()) {
            Ok(lines) => {
                for l in &lines {
                    println!("ok: {l}");
                }
                println!("healthy: atlas {}", atlas::upgrade::version());
                return;
            }
            Err(lines) => {
                for l in &lines {
                    println!("unhealthy: {l}");
                }
                std::process::exit(1);
            }
        }
    }

    if flag("--version") || flag("-V") {
        println!("atlas {}", atlas::upgrade::version());
        println!("install: {}", atlas::roots::where_and_why());
        return;
    }

    // Double-clicked, or opened from the Start menu or desktop shortcut:
    // Atlas's own window, never a terminal. From anywhere that isn't already
    // an install, Atlas first moves itself into its standard home. See
    // `firstlaunch`.
    let double_clicked = atlas::firstlaunch::started_without_a_terminal();
    if (argv.is_empty() && double_clicked) || words.first().map(|s| s.as_str()) == Some("home") {
        return run_home(double_clicked, atlas::firstlaunch::First::from_words(words.get(1..).unwrap_or(&[])));
    }
    // The words-on-the-desktop overlay, started by the background Atlas
    // (`overlaywin`). No console, no window of its own to speak of.
    if words.first().map(|s| s.as_str()) == Some("overlay") {
        #[cfg(feature = "desktop-ui")]
        if let Err(e) = atlas::overlaywin::run(atlas::overlaywin::Folders { data: atlas::roots::data_dir(), config: atlas::roots::config_dir() }) {
            eprintln!("The desktop overlay couldn't start: {e}");
        }
        #[cfg(not(feature = "desktop-ui"))]
        eprintln!("This build has no desktop overlay.");
        return;
    }
    // The typing box (H1), opened by the background Atlas when its key is
    // pressed. What's typed is printed for it to read.
    if words.first().map(|s| s.as_str()) == Some("typebox") {
        let qcfg = atlas::config::Config::load(&atlas::roots::config_dir())
            .ok()
            .and_then(|c| c.tools.map(|t| t.quick_input))
            .unwrap_or_default();
        if let Err(e) = atlas::typebox::run(qcfg, words.get(1).map(|w| w == "--standby").unwrap_or(false) || flag("--standby")) {
            eprintln!("The typing box couldn't open: {e}");
        }
        return;
    }
    // Trying the keys (H1): what Atlas sees when you press them, and nothing
    // else — no listening, no box. For checking a key you've just set.
    if words.first().map(|s| s.as_str()) == Some("keys") {
        let secs: u64 = words.get(1).and_then(|w| w.parse().ok()).unwrap_or(60);
        let tc = atlas::config::Config::load(&atlas::roots::config_dir())
            .ok()
            .and_then(|c| c.tools)
            .unwrap_or_default();
        println!("{}", atlas::hotkeys::try_them(&tc.push_to_talk, &tc.quick_input, secs, &mut |line| println!("{line}")));
        return;
    }
    // Started by Windows at sign-in (the start-with-Windows task): the
    // background Atlas needs no console window, so it lets go of the one it
    // was given.
    if double_clicked && flag("--daemon") {
        atlas::firstlaunch::let_go_of_the_console();
    }

    let dry = flag("--dry-run");
    // One place decides where the install is; `ATLAS_CONFIG` is honoured
    // inside it, and now by every other config read too rather than this
    // one alone.
    let dir = atlas::roots::config_dir();

    // Before anything else can panic. Atlas had no panic hook at all, so a
    // crash left nothing behind and the next start greeted you as though
    // nothing had happened -- which is also why "Atlas works on itself"
    // could not mean much: it could not report its own failures.
    atlas::crash::watch(atlas::roots::state_dir().as_path());

    // Said once, out loud, before anything else. The failure this replaces
    // was silent: Atlas came up with an empty `data/state` and no sign that
    // anything was wrong, and the natural reading of that is "Atlas lost
    // everything" rather than "Atlas is looking in the wrong folder".
    if atlas::roots::first_run_here() {
        println!(
            "Starting a new install in {}.\n\
             If you expected your notes and settings to be here, stop and check \
             the folder -- set ATLAS_HOME, or run me from the folder holding \
             atlas.exe.",
            atlas::roots::install_root().display()
        );
    }

    // The settings travel inside the program: a copy with none beside it
    // writes the shipped ones out rather than refusing to start. Never over a
    // file that's there.
    if atlas::firstlaunch::settings_missing(&dir) {
        match atlas::firstlaunch::write_default_config(&dir) {
            Ok(n) if n > 0 => println!("I wrote my settings into {}.", dir.display()),
            Ok(_) => {}
            Err(e) => eprintln!("I couldn't write my settings into {}: {e}", dir.display()),
        }
    }
    // Your hand edits to the shipped files, moved somewhere an update cannot
    // reach, and the shipped files brought up to this build. Before
    // `Config::load`, which lays those edits back over them. Cannot fail
    // startup: the worst case leaves a file as it was and says so.
    // Also in the log (29 Sep 2026): the background Atlas has no console,
    // so a notice only printed was a notice nobody saw.
    let kept = atlas::yourchanges::keep_hand_edits(&dir).notices;
    if !kept.is_empty() {
        let log = atlas::log::Log::new(atlas::roots::store().logs_dir(), 8 * 1024 * 1024);
        for notice in kept {
            println!("{notice}");
            log.info(&notice);
        }
    }
    // Atlas's own copies of its tools (the sound tools) are found where it
    // put them, with nothing installed system-wide.
    atlas::getpieces::use_own_tools(&atlas::roots::install_root());

    let cfg = match Config::load(&dir) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config failed to load: {e}");
            // Deliberately not `leave`: a build that can't load its settings
            // can't run here, and that counts against its trial. (Your own
            // edits can't cause this -- `keep_hand_edits` never lets one stop
            // a start.)
            std::process::exit(2);
        }
    };
    // Anchored once, here, so nothing downstream holds a second view of
    // where this install's files are. `Daemon::tools_cfg` did this for
    // `work_dir` and the two `Voice::new` call sites did not, which is how
    // the recorder came to write into a folder nothing else in the same run
    // used.
    let cfg = Config { tools: cfg.tools.map(|t| t.anchored()), ..cfg };

    let plat: Box<dyn Platform> = if dry || !cfg!(windows) {
        if !dry && !cfg!(windows) {
            eprintln!("note: not on Windows — using the mock platform.\n");
        }
        Box::new(MockPlatform::new(fake_monitors()))
    } else {
        #[cfg(windows)]
        { Box::new(atlas::platform::win::WindowsPlatform) }
        #[cfg(not(windows))]
        { unreachable!() }
    };

    // Teaching Atlas your voice.
    //
    // Without this every verdict is `NotEnrolled` forever, `handle` proceeds
    // on that by design, and the whole of voice-lock is a module that runs and
    // decides nothing. Enrollment is the difference between the feature
    // existing and the feature working.
    if words.first().map(|s| s.as_str()) == Some("backends") {
        run_backends(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("hub") {
        run_hub_address(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("startup") {
        run_startup(&words[1..]);
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("file") {
        run_file(&cfg, words.iter().any(|w| w.as_str() == "--do-it"));
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("reclaim") {
        run_reclaim(&cfg, words.iter().any(|w| w.as_str() == "--do-it"));
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("read") {
        run_read(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("mesh") {
        run_mesh(&cfg);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("nearby") {
        run_nearby(&cfg);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("telegram") {
        run_telegram(&cfg, &words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("fed") {
        run_fed();
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("trade") {
        run_trade(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("screen") {
        run_screen(&cfg, &words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("picture") {
        run_picture(&cfg, &words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("shared") {
        run_shared(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("tasks") {
        run_tasks(&words[1..]);
        return;
    }

    if matches!(words.first().map(|s| s.as_str()), Some("seal-file") | Some("open-file") | Some("my-key")) {
        run_agefile(&words);
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("notes") {
        // `--merge-voices` from the raw arguments: `words` drops `--` flags.
        let merge = argv.iter().any(|a| a == "--merge-voices");
        // `--people 3`: how many were on the call, when you know.
        let people = argv.iter().position(|a| a == "--people").and_then(|i| argv.get(i + 1)).and_then(|n| n.parse::<usize>().ok());
        run_notes(&cfg, &words[1..], merge, people);
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("cuts") {
        // Where each cut in a video sends the viewer's eye (`cutcheck` +
        // `editcraft`).
        let Some(video) = words.get(1) else { return println!("atlas cuts <video>") };
        let tc = cfg.tools.clone().unwrap_or_default();
        let ffmpeg = tc.vars.get("ffmpeg").cloned().unwrap_or_else(|| "ffmpeg".into());
        match atlas::cutcheck::cuts(&ffmpeg, video) {
            Ok(cuts) if cuts.is_empty() => println!("I didn't find any cuts in {video}."),
            Ok(cuts) => {
                let list: Vec<atlas::editcraft::Cut> = cuts.iter().map(|(_, c)| *c).collect();
                let notes = atlas::editcraft::check_cuts_within(&list, tc.editcraft.eye_jump_limit);
                println!("{} cuts.", cuts.len());
                for (i, (t, c)) in cuts.iter().enumerate() {
                    let note = notes.iter().find(|(j, _)| *j == i).map(|(_, n)| format!(" — {n}")).unwrap_or_else(|| " — the eye stays put".into());
                    println!("  {:>5.1}s  {:.0}% → {:.0}%{note}", t, c.leaving_at * 100.0, c.arriving_at * 100.0);
                }
            }
            Err(e) => println!("{video}: {e}"),
        }
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("film") {
        // Play an SVG animation and save it as a GIF (and an MP4 with
        // ffmpeg), next to the SVG (`filmstrip`).
        let Some(file) = words.get(1) else { return println!("atlas film <animation.svg> [frames per second]") };
        let fps: u32 = words.get(2).and_then(|w| w.parse().ok()).unwrap_or(12);
        let svg = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => return println!("{file}: {e}"),
        };
        let lower = svg.to_lowercase();
        let mut spec = atlas::motion::MotionSpec::new("");
        if let Some((w, h)) = atlas::motion::declared_size(&lower) {
            spec.width = w;
            spec.height = h;
        }
        let tc = cfg.tools.clone().unwrap_or_default();
        let Some(browser) = atlas::filmstrip::find_browser(tc.vars.get("browser").map(|s| s.as_str())) else {
            return println!("That needs Edge or Chrome to play the animation in, and I didn't find either.");
        };
        let path = std::path::Path::new(file);
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "animation".into());
        let plan = atlas::filmstrip::Plan::for_svg(&svg, &spec, fps);
        let ffmpeg = tc.vars.get("ffmpeg").cloned().unwrap_or_else(|| "ffmpeg".into());
        match atlas::filmstrip::film(&svg, &plan, &browser, Some(&ffmpeg), dir, &stem) {
            Ok(made) => println!("{}", made.say()),
            Err(e) => println!("{file}: {e}"),
        }
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("scene") {
        // Draw a 3-D scene from its JSON (`scene3d`): a still, a turntable
        // GIF, and Blender's render when Blender is installed.
        let Some(file) = words.get(1) else { return println!("atlas scene <scene.json>") };
        let scene = match atlas::scene3d::load_scene(std::path::Path::new(file)) {
            Ok(s) => s,
            Err(e) => return println!("{file}: {e}"),
        };
        let tc = cfg.tools.clone().unwrap_or_default();
        let path = std::path::Path::new(file);
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "scene".into());
        let blender = atlas::scene3d::find_blender(tc.vars.get("blender").map(|s| s.as_str()));
        match atlas::scene3d::make(&scene, dir, &stem, 24, blender.as_deref()) {
            Ok(made) => println!("{}", made.say()),
            Err(e) => println!("{file}: {e}"),
        }
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("fix") {
        // From the raw arguments: `words` drops everything starting with
        // `--`, and `atlas fix` needs both its `--` separator and `--land`.
        let at = argv.iter().position(|a| a == "fix").map(|i| i + 1).unwrap_or(argv.len());
        run_fix(&cfg, &argv[at..]);
        return;
    }
    if matches!(words.first().map(|s| s.as_str()), Some("wake-word") | Some("hearing") | Some("voices")) {
        run_voice_lab(&cfg, &words);
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("time") {
        // Where the time went (`worklog`), read from the record the running
        // Atlas keeps: `atlas time`, `atlas time yesterday`, `atlas time week`.
        let store = atlas::roots::store();
        let log: atlas::worklog::WorkLog = store.load("worklog");
        let tc = cfg.tools.clone().unwrap_or_default();
        let zone = atlas::tz::home(&tc.time_zone);
        let now = atlas::store::now();
        let lnow = zone.to_local(now as i64).max(0) as u64;
        let today = atlas::calendar::start_of_day(lnow);
        let which = words.get(1).map(|s| s.as_str()).unwrap_or("today");
        let (from, to, name) = match which {
            "yesterday" => (today - 86_400, today, "yesterday"),
            "week" => (today - 6 * 86_400, today + 86_400, "over the last seven days"),
            _ => (today, today + 86_400, "today"),
        };
        let back = |l: u64| zone.to_utc(l as i64).max(0) as u64;
        let summary = atlas::worklog::summarise(&log.between(back(from), back(to)));
        let clock = |u: u64| {
            let l = zone.to_local(u as i64).max(0) as u64 % 86_400;
            format!("{}:{:02}", l / 3600, (l % 3600) / 60)
        };
        println!("{}", atlas::worklog::say(&summary, name, !log.saw_input && log.blind_beats > 0, &clock));
        let p = atlas::platform::here();
        println!("(This machine: {} since the last keyboard or mouse input; {}.)",
            p.input_idle_secs().map(|s| format!("{s} s")).unwrap_or_else(|| "no way to tell".into()),
            p.quiet_state().map(|q| q.plain()).unwrap_or("the OS doesn't say whether it's a good moment to interrupt"));
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("logs") {
        // Atlas's own log as the few things that happened, with counts
        // (`drain`), rather than ten thousand lines nobody reads.
        let dir = atlas::roots::logs_dir();
        let mut text = std::fs::read_to_string(dir.join("atlas.log.1")).unwrap_or_default();
        text.push_str(&std::fs::read_to_string(dir.join("atlas.log")).unwrap_or_default());
        let (d, lines) = atlas::drain::read_log(&text);
        if lines == 0 {
            println!("The log at {} is empty.", dir.display());
            return;
        }
        let only_warn = flag("--warn");
        println!("{lines} lines, {} kinds of line:", d.templates.len());
        for t in d.by_count().into_iter().filter(|t| !only_warn || t.words.first().map(|w| w != "INFO").unwrap_or(true)).take(25) {
            println!("{:>6}  {}", t.count, t.text());
            if t.words.iter().filter(|w| *w == "<*>").count() * 2 > t.words.len() {
                println!("        e.g. {}", t.example);
            }
        }
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("doc") {
        run_doc(&words[1..]);
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("diff") {
        // What changed between two versions of a file — a draft and its
        // rewrite, a config and its backup — without needing git.
        match (words.get(1), words.get(2)) {
            (Some(a), Some(b)) => match (std::fs::read_to_string(a), std::fs::read_to_string(b)) {
                (Ok(x), Ok(y)) => {
                    let u = atlas::diff::unified(&x, &y, a, b, 3);
                    if u.is_empty() {
                        println!("They're the same.");
                    } else {
                        print!("{u}");
                        println!("({} lines changed)", atlas::diff::lines_changed(&x, &y));
                    }
                }
                (Err(e), _) => println!("I couldn't open {a}: {e}"),
                (_, Err(e)) => println!("I couldn't open {b}: {e}"),
            },
            _ => println!("atlas diff <old file> <new file>"),
        }
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("clients") {
        run_clients(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("calendar") {
        run_calendar(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("household") {
        run_household(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("backups") {
        run_backups(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("share") {
        run_share(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("handoffs") {
        run_handoffs(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("trust") {
        run_trust(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("hand") {
        run_hand(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("index") {
        run_index(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("journal") {
        // `atlas journal check`: is Atlas's record of what it did the one it
        // wrote?
        let store = atlas::roots::store();
        let j = atlas::activity::Journal::load(&store);
        let backup = Config::load(&atlas::roots::config_dir())
            .ok()
            .and_then(|c| c.tools)
            .map(|t| t.backup)
            .unwrap_or_default()
            .resolved(&store.install_root());
        let (sealed, from) = atlas::activity::check_with_backups(&j, store.root(), &backup);
        println!("{}", atlas::activity::said_with_backups(&sealed, from));
        println!(
            "Heads are kept in {} and copied into every backup in {}.",
            atlas::activity::anchor_path(store.root()).display(),
            backup.dir
        );
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("trace") {
        run_trace(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("search")
        && words.get(1).map(|s| s.as_str()) == Some("check")
    {
        run_search_check();
        return;
    }

    // What happened the last time Atlas stopped unexpectedly.
    //
    // The spoken line is one sentence and clears itself, which is right for
    // the daemon and useless for working out what actually broke. This is
    // the other half: the file, in full, on demand -- and it does not clear
    // it, so asking twice gives the same answer.
    if words.first().map(|s| s.as_str()) == Some("crash") {
        let store = atlas::roots::store();
        match atlas::crash::last(&store) {
            Some(n) => {
                println!("{}\n", n.plain());
                println!("{}", n.detail());
                println!(
                    "\nKept at {}. It is cleared once I've told you about it \
                     out loud, so this is the copy that stays.",
                    atlas::crash::note_path(store.root()).display()
                );
            }
            None => println!("No crash on record. Nothing has ended unexpectedly since I last said so."),
        }
        return;
    }

    // `setup` alongside `firstrun`, because `config/apps.yaml` and a doc
    // comment both told people to run `atlas setup` and nothing answered to
    // that name. Two spellings of one command is cheaper than a shipped
    // config file that lies, and `setup` is what a person types.
    if matches!(words.first().map(|s| s.as_str()), Some("firstrun") | Some("setup")) {
        run_firstrun(&cfg, plat.as_ref());
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("mail") {
        run_mail(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("accounts") {
        run_accounts(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("profiles") {
        run_profiles(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("handover") {
        run_handover(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("vault") {
        run_vault(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("update") {
        run_update(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("plugins") {
        run_plugins(&cfg, &words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("release") {
        run_release(&words[1..]);
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("install-page") {
        run_install_page(&words[1..]);
        return;
    }
    if words.first().map(|s| s.as_str()) == Some("feedback") {
        run_feedback(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("group") {
        run_group(&words[1..]);
        return;
    }

    // `atlas friend ...` is the same sentence said to Atlas, so it goes the
    // one way every friend action goes (`Intent::Friend`).
    let words = if words.first().map(|s| s.as_str()) == Some("friend") {
        let rest = words[1..].join(" ");
        let sentence = match words.get(1).map(|s| s.as_str()) {
            None | Some("link") => "add a friend".to_string(),
            Some("add") => words[2..].join(" "),
            Some("requests") => "friend requests".to_string(),
            Some("accept") => format!("accept friend request from {}", words[2..].join(" ")),
            Some("decline") => format!("decline friend request from {}", words[2..].join(" ")),
            Some("request") => format!("send a friend request to {}", words[2..].join(" ")),
            Some(_) => rest,
        };
        vec![sentence]
    } else {
        words
    };

    if words.first().map(|s| s.as_str()) == Some("edits") {
        run_edits(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("adapt") {
        run_adapt(&cfg, plat.as_ref(), &words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("watching") {
        run_watching(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("market") {
        run_market(&words[1..]);
        return;
    }

    // `atlas typing`: what correcting-as-you-type has learned (Eric, H4).
    if words.first().map(|s| s.as_str()) == Some("typing") {
        let store = atlas::roots::store();
        let mut lessons: atlas::astype::Lessons = store.load(atlas::astype::Lessons::RECORD);
        println!("{}", lessons.what_it_learned(atlas::store::now()));
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("window") && words.get(1).map(|s| s.as_str()) == Some("read") {
        // `atlas window read notepad.exe`: what Atlas can read of that app's
        // window, the same way it reads one it's working for you.
        let name = words.get(2).cloned().unwrap_or_default();
        let spec = atlas::config::AppSpec::for_process(&name);
        match plat.find_window(&spec) {
            Ok(Some(id)) => match plat.read_window(id) {
                Ok(Some(tree)) => println!("{}", tree.text()),
                Ok(None) => println!("That window shows nothing to other programs."),
                Err(e) => println!("I couldn't read it: {e}"),
            },
            _ => println!("There's no {name} window open."),
        }
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("window") && words.get(1).map(|s| s.as_str()) == Some("type") {
        // `atlas window type notepad.exe some words`: type into that app's
        // window exactly the way a window job types a reply — brought to the
        // front, checked it's a text box, typed, your window put back. Never
        // presses Enter, so nothing is sent: this is the live check of
        // typing, for a test window.
        let name = words.get(2).cloned().unwrap_or_default();
        let text = words.get(3..).map(|w| w.join(" ")).unwrap_or_default();
        if name.is_empty() || text.is_empty() {
            println!("Say which app and what to type: atlas window type notepad.exe hello there");
            return;
        }
        let spec = atlas::config::AppSpec::for_process(&name);
        match plat.find_window(&spec) {
            Ok(Some(id)) => match atlas::delegate::type_into_window(plat.as_ref(), id, &text, false) {
                Ok(()) => println!("Typed {} characters into {name}. Nothing was sent.", text.chars().count()),
                Err(e) => println!("I couldn't type into it: {e}."),
            },
            _ => println!("There's no {name} window open."),
        }
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("window") && words.get(1).map(|s| s.as_str()) == Some("idle") {
        // `atlas window idle`: how long since the keyboard or mouse was last
        // touched — what "wait for a gap in your typing" is measured by.
        match plat.input_idle_secs() {
            Some(s) => println!("Last keyboard or mouse input: {s} seconds ago."),
            None => println!("This machine doesn't say when it was last used."),
        }
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("call") {
        run_call_check(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("get") {
        run_get(words.get(1).map(|s| s.as_str()));
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("talk-bench") {
        std::process::exit(run_talk_bench(&cfg, &words[1..]));
    }

    if words.first().map(|s| s.as_str()) == Some("kokoro-check") {
        std::process::exit(run_kokoro_check(&words[1..]));
    }

    if words.first().map(|s| s.as_str()) == Some("phone") {
        run_phone(&cfg, &words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("wireguard") {
        run_wireguard(&cfg, &words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("multiframe") {
        run_multiframe(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("refusals") {
        run_refusals(&words[1..]);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("audition") {
        run_audition(&cfg);
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("enrol-voice")
        || words.first().map(|s| s.as_str()) == Some("enroll-voice")
    {
        run_enrol_voice(&cfg, plat.as_ref());
        return;
    }

    // The revoke half of enrolment. `voiceid::forget` existed from the day
    // the enrol command did, and nothing called it — so an enrolled print
    // could not be revoked from anywhere in production, which for a
    // credential is a gap, not a nicety.
    if words.first().map(|s| s.as_str()) == Some("forget-voice") {
        let store = atlas::roots::store();
        let mut id = atlas::voiceid::VoiceId::load(&store);
        if id.enrolled() == 0 {
            println!("I don't have your voice learned, so there's nothing to forget.");
            return;
        }
        id.forget();
        match id.save(&store) {
            Ok(()) => println!(
                "Forgotten — the voiceprint and its score history are gone. \
                 `atlas enrol-voice` teaches me again."
            ),
            // Never claim to have forgotten something still on disk.
            Err(e) => eprintln!("I couldn't forget it: {e}. The print is still stored."),
        }
        return;
    }

    // Atlas's own panel, run as its own process.
    //
    // Not a user-facing command so much as how the daemon opens a window:
    // macOS requires a window to be created on the process main thread, and
    // the daemon's main thread is the loop. A child process has a main thread
    // going spare, and the same path works on all three systems.
    if words.first().map(|s| s.as_str()) == Some("window") {
        let Some(path) = words.get(1) else {
            eprintln!("usage: atlas window <panel.json>");
            return;
        };
        #[cfg(feature = "desktop-ui")]
        match atlas::window::read_staged(std::path::Path::new(path)) {
            Ok(c) => {
                if let Err(e) = atlas::window::run(c) {
                    eprintln!("(couldn't open the window: {e})");
                }
            }
            Err(e) => eprintln!("(couldn't read that panel: {e})"),
        }
        // No desktop UI in this build (a mobile/headless core): there is no
        // native window to draw. `window::can_open` already returns false here,
        // so the daemon never spawns this — but if invoked by hand, say so
        // plainly rather than doing nothing.
        #[cfg(not(feature = "desktop-ui"))]
        {
            let _ = path;
            eprintln!("(this build has no desktop window; panels show in the hub)");
        }
        return;
    }

    if words.first().map(|s| s.as_str()) == Some("doctor") {
        return run_doctor(&cfg, plat.as_ref());
    }

    if words.first().map(|s| s.as_str()) == Some("invite") {
        return run_invite(&cfg);
    }
    if words.first().map(|s| s.as_str()) == Some("accept") {
        return run_accept(&cfg);
    }
    if words.first().map(|s| s.as_str()) == Some("craft") {
        return run_craft();
    }

    // Settings-only mode. The recovery path: no voice, no models, no memory,
    // nothing that can be broken — just the hub, so you can turn something
    // off or take away access when Atlas itself is misbehaving.
    //
    // The launcher has called this since it was written and there was no
    // such subcommand, so it fell through to the parser and said it didn't
    // understand.
    if words.first().map(|s| s.as_str()) == Some("settings") {
        return run_hub(&cfg);
    }

    // The household key, and reading a bundle by hand.
    //
    // `sync key` is how a sealed folder is set up and, more importantly, how
    // it is got back into: the phrase is the key, so typing it on a second
    // machine is the whole of pairing, and `sync read` opens a file with
    // nothing but the phrase on a machine where the rest of Atlas is broken.
    if words.first().map(|s| s.as_str()) == Some("sync") {
        // The raw tail, like `walkthrough` and `type` below: `words` has
        // every `--flag` stripped out by this point, and `--key <phrase>` is
        // the whole of what makes `sync read` work on a machine that holds no
        // key. Stripped, it silently became "read it with the key you don't
        // have" -- which the end-to-end test caught by running the binary.
        return run_sync(&cfg, atlas::cli::tail_after(&argv, "sync"));
    }

    // What the installer still has to fetch. INSTALL.bat does the downloading;
    // this is the part that knows what's needed and judges what's there.
    // Regenerates docs/METRICS.md from the code rather than from memory. The
    // numbers in the docs go stale silently, which is worse than having none.
    // Both open the hub; they differ only in which page they land on, and
    // having separate words for them means the launcher doesn't have to
    // explain the URL.
    if matches!(words.first().map(|s| s.as_str()), Some("access") | Some("sync-setup")) {
        // Naming a provider gets the real steps; naming nothing still opens
        // the hub, which is what this word did before.
        if words.first().map(|s| s.as_str()) == Some("sync-setup")
            && run_sync_setup(&cfg, &words[1..])
        {
            return;
        }
        // `atlas access` on its own still opens the hub. Anything after it is
        // the half that did not exist: until 19 Sep 2026 nothing in the tree
        // called `Access::grant`, so the list the page shows could only ever
        // be empty and `may_start` could only ever refuse.
        if words.first().map(|s| s.as_str()) == Some("access") && words.len() > 1 {
            return run_access(&cfg, atlas::cli::tail_after(&argv, "access"));
        }
        return run_hub(&cfg);
    }

    // These three read their own flags, so they get the raw tail rather than
    // `words` — which has every `--flag` stripped out before this point. That
    // stripping is right for the commands that take none and silently wrong
    // for any command whose meaning depends on one: `--secs 200` vanished
    // entirely and the request looked like it had finished instantly.
    if words.first().map(|s| s.as_str()) == Some("walkthrough") {
        return run_walkthrough(&cfg, atlas::cli::tail_after(&argv, "walkthrough"));
    }

    if words.first().map(|s| s.as_str()) == Some("type") {
        return run_quickinput(&cfg, atlas::cli::tail_after(&argv, "type"));
    }

    if words.first().map(|s| s.as_str()) == Some("carry") {
        return run_carry(&cfg, atlas::cli::tail_after(&argv, "carry"));
    }

    if words.first().map(|s| s.as_str()) == Some("remote") {
        return run_remote(&cfg, &dir, atlas::cli::tail_after(&argv, "remote"));
    }

    if words.first().map(|s| s.as_str()) == Some("mobile") {
        return run_mobile(&cfg, atlas::cli::tail_after(&argv, "mobile"));
    }

    if words.first().map(|s| s.as_str()) == Some("money") {
        return run_money(&cfg, atlas::cli::tail_after(&argv, "money"));
    }

    if words.first().map(|s| s.as_str()) == Some("away") {
        return run_away(&cfg, atlas::cli::tail_after(&argv, "away"));
    }

    if words.first().map(|s| s.as_str()) == Some("codes") {
        return run_codes(&cfg, atlas::cli::tail_after(&argv, "codes"));
    }

    if words.first().map(|s| s.as_str()) == Some("afterme") {
        return run_afterme(&cfg, atlas::cli::tail_after(&argv, "afterme"));
    }

    if words.first().map(|s| s.as_str()) == Some("catalog") {
        return run_catalog(atlas::cli::tail_after(&argv, "catalog"));
    }

    if words.first().map(|s| s.as_str()) == Some("video") {
        return run_video(&cfg, atlas::cli::tail_after(&argv, "video"));
    }

    if words.first().map(|s| s.as_str()) == Some("content") {
        return run_content(&cfg, atlas::cli::tail_after(&argv, "content"));
    }

    if words.first().map(|s| s.as_str()) == Some("budget") {
        return run_budget(&cfg, atlas::cli::tail_after(&argv, "budget"));
    }


    if words.first().map(|s| s.as_str()) == Some("metrics") {
        return run_metrics();
    }

    if words.first().map(|s| s.as_str()) == Some("install") {
        return report_install(&cfg);
    }

    let approver: Box<dyn Approver> = if flag("--yes") {
        Box::new(AllowAll)
    } else {
        Box::new(DenyAll)
    };
    let parser = Parser::new(&cfg.commands);

    if flag("--daemon") {
        return run_daemon(&cfg, plat.as_ref(), flag("--unattended"));
    }

    if flag("--voice") || flag("--wake") {
        return voice_loop(&cfg, plat.as_ref(), &parser, approver.as_ref(), flag("--wake"));
    }

    // The prompt used to answer six intents itself and say "not wired to an
    // action yet" for everything else — which was true of the prompt and not
    // of Atlas. Every one of those actions already existed in the daemon; the
    // typed path simply never reached it.
    //
    // This is the same Daemon the --daemon mode builds, minus voice and the
    // wake word. Where it cannot be built (no config/tools.yaml) the prompt
    // falls back to the six it always had, so a machine with no voice setup
    // still gets a working prompt rather than an error.
    // Not beside a running Atlas (30 Sep 2026): this built a second daemon
    // on the same store, and its first save wrote every file -- the
    // schedule, the calendar, what you've told it -- back from the copy it
    // loaded at start, over whatever the running one had done since.
    if let atlas::onlyone::Found::Running { .. } =
        atlas::onlyone::OnlyOne::at(&atlas::roots::data_dir()).look(atlas::store::now())
    {
        println!(
            "Atlas is already running. Talk to it, use its typing box, or the hub's Talk page -- \
             a second one here would write over what it keeps."
        );
        return;
    }
    let store = atlas::roots::store();
    let mut shell: Option<Daemon> = cfg.tools.as_ref().map(|tc| {
        // With the model, like every other door. `None` here meant a
        // question typed at `atlas` was never put to a model at all.
        let mut d = Daemon::new(&cfg, plat.as_ref(), model_connection(tc), store, Proactive::new(tc.proactive.clone()))
            .starting_the_model_server()
            .with_typed_prompt(Box::new(atlas::typed::Console))
            .watch_settings(atlas::roots::config_dir());
        d.autonomy = Autonomy::Supervised;
        d
    });

    if words.is_empty() {
        println!("atlas ready. type a command, 'help', or 'quit'.");
        loop {
            print!("> ");
            let _ = io::stdout().flush();
            let mut line = String::new();
            // Nothing more to read (no keyboard: started by another program,
            // or its input closed) is the end, not an empty line: read as an
            // empty line it spun here for ever with no window (29 Sep 2026).
            match io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let line = line.trim().to_string();
            match line.as_str() {
                "" => continue,
                "quit" | "exit" => return,
                "help" => { print!("{USAGE}"); continue; }
                _ => {
                    println!(
                        "{}",
                        prompt_line(&cfg, plat.as_ref(), &parser, approver.as_ref(), shell.as_mut(), &line)
                    );
                }
            }
        }
    } else {
        let line = words.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
        println!(
            "{}",
            prompt_line(&cfg, plat.as_ref(), &parser, approver.as_ref(), shell.as_mut(), &line)
        );
    }
}

/// `atlas talk-bench <model.gguf> [port]`: Atlas's own conversation, timed,
/// against a server already serving that model (`talkbench`).
fn run_talk_bench(cfg: &atlas::config::Config, words: &[String]) -> i32 {
    let Some(path) = words.first().map(std::path::PathBuf::from) else {
        println!("atlas talk-bench <model.gguf> [port]   (a llama.cpp server must already serve it on that port)");
        return 2;
    };
    let port: u16 = words.get(1).and_then(|p| p.parse().ok()).unwrap_or(8091);
    let Some(tc) = cfg.tools.as_ref() else {
        println!("No tools section in the config.");
        return 2;
    };
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let reg = atlas::models::Registry::scan(&dir);
    let Some(model) = reg.models.iter().find(|m| m.path == path || m.path.file_name() == path.file_name()) else {
        println!("Couldn't read a model at {}.", path.display());
        return 2;
    };
    let mut mcfg = tc.models.clone();
    mcfg.port = port;
    let lc = atlas::models::llm_config_for(model, &mcfg, &atlas::models::server_post());
    let llm = std::sync::Arc::new(atlas::brain::ShellLlm { cfg: lc, vars: tc.vars.clone() }) as std::sync::Arc<dyn atlas::brain::Llm>;
    let store = std::env::temp_dir().join(format!("atlas-talk-bench-{}", std::process::id()));
    let answers = atlas::talkbench::run(cfg, llm, &store);
    println!("{}", atlas::talkbench::report(&model.id, &answers));
    let _ = std::fs::remove_dir_all(&store);
    0
}
