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
    if std::env::var_os("ATLAS_UPDATE_PROBE").is_none() && !flag("--version") && !flag("-V") {
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
    for notice in atlas::yourchanges::keep_hand_edits(&dir).notices {
        println!("{notice}");
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
        run_enrol_voice(&cfg);
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
            if io::stdin().read_line(&mut line).is_err() {
                return;
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

fn run_craft() {
    use atlas::craft::{self, Next};

    let Some(dir) = first_bare_arg("craft") else {
        println!(
            "Which project? Try:\n\n  atlas craft path/to/the/project\n\n\
             I look for Cargo.toml or a Python project file in that folder to \
             know which ladder to run."
        );
        return;
    };
    let dir = std::path::PathBuf::from(&dir);
    if !dir.is_dir() {
        println!("{} isn't a folder I can see.", dir.display());
        return;
    }
    let Some(lang) = atlas::craft::lang_of_dir(&dir) else {
        println!(
            "I can't tell what this project is — no Cargo.toml, no pyproject.toml, \
             setup.py, setup.cfg, or requirements.txt in {}.",
            dir.display()
        );
        return;
    };

    println!("Running the {} ladder in {} — cheapest checks first.\n", lang.plain(), dir.display());

    let mut ran: Vec<craft::Ran> = Vec::new();
    loop {
        let todo = craft::still_worth_running(lang, &ran);
        let Some(gate) = todo.first() else { break };
        print!("  {} ... ", gate.command);
        let _ = io::stdout().flush();
        let r = run_gate(&dir, gate);
        println!("{}", if r.passed { "ok" } else { "failed" });
        ran.push(r);
        // Stop as soon as something blocking has failed -- running the rest
        // would only produce noise from code the earlier failure already
        // explains. `still_worth_running` encodes exactly this on the next
        // loop, but there is no point paying for a gate whose result cannot
        // change what happens next.
        if let Next::Fix { .. } = craft::read_ladder(lang, &ran) {
            if ran.last().map(|r| !r.passed && r.tells.blocks_later()).unwrap_or(false) {
                break;
            }
        }
    }

    match craft::read_ladder(lang, &ran) {
        Next::Good => println!("\nEverything passed."),
        Next::WorksWithNotes(notes) => {
            println!("\nIt works. Worth a look, not blocking:");
            for n in notes {
                println!("  - {n}");
            }
        }
        Next::Fix { gate, output } => {
            println!("\n{} — {}\n", gate.on_fail, gate.command);
            // The tool's own words, verbatim -- see craft.rs on why a
            // paraphrase loses the line number, which is the useful part.
            let head: String = output.lines().take(20).collect::<Vec<_>>().join("\n");
            println!("{head}");
            if output.lines().count() > 20 {
                println!("... ({} more lines)", output.lines().count() - 20);
            }
        }
    }
}

fn run_doctor(cfg: &Config, plat: &dyn Platform) {
    println!("atlas doctor\n");
    let mut findings = doctor::run(cfg, cfg.tools.as_ref(), plat);
    // The machine itself. Absent until today, which is how a readings stub
    // survived every clean doctor run there has ever been: a check that never
    // looks at a thing cannot fail on it.
    findings.extend(doctor::machine_findings());
    let mut bad = 0;
    for f in &findings {
        let mark = if f.ok { "  ok " } else { bad += 1; "FAIL " };
        println!("{mark}{:<18} {}", f.label, f.detail);
    }
    println!();
    // Source code for the developer's dry runs, never shown to the person
    // setting Atlas up: Eric's rule is no code in what he sees. Set
    // ATLAS_DEV to get it.
    if std::env::var_os("ATLAS_DEV").is_some() {
        if let Ok(m) = plat.monitors() {
            println!("Paste into src/main.rs so dry runs match this desk:\n");
            println!("{}", doctor::monitor_fixture(&m));
        }
    }
    if bad == 0 {
        println!("All checks passed.");
    } else {
        println!("{bad} problem(s). Fix these before expecting anything to work.");
    }
}

/// Suggest where loose files should go, and move them only if told to.
///
/// Two steps, like `reclaim`: plain shows what it would do, `--do-it` acts.
/// Every move goes through `system::judge` — the roots check, the master
/// switch, the reversibility rules — so filing cannot reach anywhere Atlas is
/// not already permitted to work.
fn run_file(cfg: &Config, go: bool) {
    let Some(tc) = cfg.tools.as_ref() else {
        eprintln!("no config/tools.yaml, so I don't know which folders I may work in.");
        return;
    };
    let sys = tc.system.clone();
    let Some(home) = atlas::doctor::lookup_env("USERPROFILE")
        .or_else(|| atlas::doctor::lookup_env("HOME"))
    else {
        eprintln!("I couldn't work out where your home folder is.");
        return;
    };
    let root = std::path::PathBuf::from(&home).join("Filed");
    let now = atlas::store::now();

    // Loose files in the folders things land in. Not a whole-disk sweep:
    // filing everywhere would move things you deliberately put somewhere.
    let mut moves = 0usize;
    let mut left = 0usize;
    let mut planned: Vec<(std::path::PathBuf, atlas::filing::Suggestion)> = Vec::new();
    for folder in ["Downloads", "Desktop", "Documents"] {
        let dir = std::path::PathBuf::from(&home).join(folder);
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let ext = path.extension().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            let age = now.saturating_sub(
                meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(now),
            ) / 86_400;
            let s = atlas::filing::suggest(&root, &name, &ext, age);
            match &s {
                atlas::filing::Suggestion::Move { .. } => moves += 1,
                atlas::filing::Suggestion::Leave { .. } => left += 1,
            }
            println!("  {}", s.line(&path));
            planned.push((path, s));
        }
    }
    println!();
    println!("{}", atlas::filing::spoken(moves, left));

    if !go {
        println!();
        println!("Nothing has been moved. `atlas file --do-it` files these.");
        return;
    }

    // No trash here, and that is the fix rather than an omission.
    //
    // ## What this did until 17 Sep 2026
    //
    // The `Verdict::Go` arm created the destination's parent directory and
    // then called `trash.take(from, "filed")`. **Nothing ever moved the file
    // to `to`.** Every file the walker picked from Downloads, Desktop **and
    // Documents** ended up in `data/trash/<id>-<name>`, and stdout said
    // `filed <path>`.
    //
    // Two things made that worse than a no-op. The trashed files then sat
    // inside `data/`, where the hourly retention pass surveys — and
    // `retention::classify` matched on extensions, so a filed `.png` was
    // `Captures` with a 24-hour limit and a filed `.wav` was `Scratch` with a
    // ten-minute one, deleted permanently with `remove_file` while the trash
    // ledger still listed them. So `atlas file --do-it` said "filed" and then
    // destroyed the person's documents within a day. Both halves are fixed;
    // this is the half that moves the file.
    //
    // The comment that used to be here said the trash made "a wrong home
    // recoverable rather than a hunt", which is a sound instinct about the
    // wrong operation: the trash is for removing something, and a misfiled
    // file is recoverable because the line below says where it went.
    for (from, s) in &planned {
        let Some(change) = atlas::filing::as_change(from, s) else { continue };
        // Judged every time. A refusal here names its own fix, so a run that
        // does nothing still tells you what to change.
        match atlas::system::judge(&change, &sys) {
            atlas::system::Verdict::Refuse(why) => {
                println!("  skipped {}: {why}", from.display());
            }
            atlas::system::Verdict::Go { .. } => {
                if let atlas::filing::Suggestion::Move { to, .. } = s {
                    if let Some(parent) = to.parent() {
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            println!("  couldn't make {}: {e}", parent.display());
                            continue;
                        }
                    }
                    // Never over the top of something already there.
                    //
                    // `fs::rename` replaces its destination silently on unix.
                    // Filing a `report.pdf` onto an existing `report.pdf`
                    // would destroy the one already filed -- and the one
                    // already filed is, by definition, the one the person
                    // meant to keep.
                    if to.exists() {
                        println!(
                            "  left {} alone: {} already exists, and I won't write over it",
                            from.display(),
                            to.display()
                        );
                        continue;
                    }
                    // Rename is atomic within a volume; Downloads and the
                    // Filed tree are usually the same one. Across volumes it
                    // fails with a cross-device error, so copy-then-remove is
                    // the fallback -- and the remove only happens once the
                    // copy has succeeded, so an interrupted move leaves the
                    // original where it was rather than nowhere.
                    let moved = match std::fs::rename(from, to) {
                        Ok(()) => Ok(()),
                        Err(_) => std::fs::copy(from, to)
                            .and_then(|_| std::fs::remove_file(from))
                            .map(|_| ()),
                    };
                    match moved {
                        // Says where it went, which is what makes a wrong
                        // home a correction rather than a hunt.
                        Ok(()) => println!("  filed {} -> {}", from.display(), to.display()),
                        Err(e) => {
                            // Clean up a half-finished cross-volume copy, so a
                            // failure does not leave two copies and no word
                            // about which is which.
                            if to.exists() && from.exists() {
                                let _ = std::fs::remove_file(to);
                            }
                            println!("  couldn't file {}: {e}", from.display());
                        }
                    }
                }
            }
        }
    }
}

/// Read the words in a picture, or off the screen, in Atlas's own code.
///
/// The one command in this round that can be run today with nothing installed
/// but the two model files — no tesseract, no account, no second program.
/// `atlas picture <file> [question]` or `atlas picture screen [question]`:
/// ask the picture reader about a picture, the same reader "look at my
/// screen" uses — for checking it on this machine once `atlas get pictures`
/// has fetched it. A screenshot taken here is deleted once it's been read.
fn run_picture(cfg: &Config, args: &[String]) {
    let tools = cfg.tools.clone().unwrap_or_default();
    let root = atlas::roots::store().install_root();
    if let Err(why) = atlas::picture_talk::ready(&tools.picture_talk, &root) {
        println!("I can't read pictures yet: {why}.");
        return;
    }
    let Some(what) = args.first() else {
        println!("atlas picture <file> [question]   or   atlas picture screen [question]");
        return;
    };
    let said = args[1..].join(" ");
    let question = atlas::picture_talk::question_for(&said);
    let (image, taken) = if what == "screen" {
        let Some(tool) = tools.capture_screen.clone() else {
            println!("There's no screen capture set up on this machine.");
            return;
        };
        let dir = std::path::PathBuf::from(&tools.work_dir);
        let _ = std::fs::create_dir_all(&dir);
        let shot = dir.join(format!("screen_{}.png", atlas::store::now()));
        let mut vars = tools.vars.clone();
        vars.insert("out_png".into(), shot.display().to_string());
        if let Err(e) = tool.run(&vars, None) {
            let _ = std::fs::remove_file(&shot);
            println!("I couldn't take the picture: {e}");
            return;
        }
        (shot, true)
    } else {
        (std::path::PathBuf::from(what), false)
    };
    let small = atlas::picture_talk::smaller(&image);
    let started = std::time::Instant::now();
    let answer = atlas::picture_talk::ask_until(
        &tools.picture_talk,
        &root,
        small.as_deref().unwrap_or(&image),
        &question,
        &|| false,
    );
    if let Some(p) = &small {
        let _ = std::fs::remove_file(p);
    }
    if taken {
        let _ = std::fs::remove_file(&image);
    }
    match answer {
        Ok(a) => println!("{a}"),
        Err(e) => println!("I couldn't read it: {e}"),
    }
    println!("({:.0} s, asked: {question})", started.elapsed().as_secs_f32());
}

fn run_screen(cfg: &Config, args: &[String]) {
    let tools = cfg.tools.clone().unwrap_or_default();
    let models = std::path::PathBuf::from(&tools.models.dir);
    if !atlas::words::Reader::installed(&models) {
        let missing = atlas::infer::whats_missing(&models, &atlas::infer::Kind::for_reading());
        println!("{}", atlas::infer::spoken(&missing));
        println!();
        println!("Menu item 8 in ATLAS.bat fetches them. About 36MB, once.");
        return;
    }
    let mut reader = match atlas::words::Reader::open(&models) {
        Ok(r) => r,
        Err(e) => {
            println!("{e}");
            return;
        }
    };
    let wcfg = tools.words;

    let read = match args.first().map(|s| s.as_str()) {
        Some("grab") => {
            // atlas screen grab <x> <y> <w> <h>
            let nums: Vec<i64> = args[1..].iter().filter_map(|a| a.parse().ok()).collect();
            if nums.len() != 4 || nums[2] <= 0 || nums[3] <= 0 {
                println!("atlas screen grab <x> <y> <width> <height>");
                println!("A rectangle, not the whole desktop: reading 2560x1392 finds a great");
                println!("deal you didn't mean and takes ten times as long.");
                return;
            }
            let (w, h) = (nums[2] as usize, nums[3] as usize);
            let grab =
                atlas::words::capture_args(nums[0] as i32, nums[1] as i32, w as u32, h as u32);
            match atlas::words::pixels_from(&tools.video.ffmpeg, &tools.vars, &grab)
                .and_then(|px| {
                    atlas::words::whole_picture(px.len(), w, h)?;
                    reader.look(&px, w, h, &wcfg)
                }) {
                Ok(r) => r,
                Err(e) => {
                    println!("{e}");
                    return;
                }
            }
        }
        Some(path) => {
            match atlas::words::read_file(&mut reader, &tools.video.ffmpeg, &tools.vars, path, &wcfg)
            {
                Ok(r) => r,
                Err(e) => {
                    println!("{e}");
                    return;
                }
            }
        }
        None => {
            println!("atlas screen <picture file>");
            println!("atlas screen grab <x> <y> <width> <height>");
            return;
        }
    };

    println!("{}", read.spoken());
    if read.words() > 0 {
        println!();
        println!("{}", read.text());
    }
    if !read.worth_acting_on() && read.words() > 0 {
        println!();
        println!("I'd not act on that. {}", atlas::words::NO_CAPITALS);
    }
}

/// The line between your own work and a business you share.
///
/// Four things, and `check` is the important one: it answers "would this be
/// allowed?" without anything actually crossing. A boundary nobody can
/// interrogate is a boundary nobody has reason to trust, and the first time
/// you find out what it does should not be the time it matters.
fn run_shared(args: &[String]) {
    // The same state folder everything else in Atlas uses.
    let store = atlas::roots::store();
    let mut wall = atlas::firewall::Firewall::load(&store);
    let now = atlas::store::now();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            println!("{}", wall.spoken());
            for h in wall.waiting() {
                println!("  {}  {}  -> {}  ({})", h.id, h.what, h.into, h.why);
            }
            println!();
            println!("atlas shared release <number>   let that one through");
            println!("atlas shared drop <number>      it should never have been going");
            println!("atlas shared check <business> <name>   would this be allowed?");
        }
        Some("release") => match args.get(1).and_then(|n| n.parse::<u64>().ok()) {
            Some(id) => match wall.release(id) {
                Ok(said) | Err(said) => {
                    println!("{said}");
                    keep(wall.save(&store), "the shared wall");
                }
            },
            None => println!("Which one? atlas shared release 3"),
        },
        Some("drop") => match args.get(1).and_then(|n| n.parse::<u64>().ok()) {
            Some(id) => match wall.forget(id) {
                Ok(said) | Err(said) => {
                    println!("{said}");
                    keep(wall.save(&store), "the shared wall");
                }
            },
            None => println!("Which one? atlas shared drop 3"),
        },
        Some("check") => {
            let (Some(into), Some(what)) = (args.get(1), args.get(2)) else {
                println!("atlas shared check <business> <name of the thing>");
                return;
            };
            // Asked as your own work, which is the case worth checking. A
            // business's own material was never going to be stopped.
            //
            // `would_stop`, not `check`. `check` is the enforcement path: it
            // takes `&mut self`, allocates an id and pushes a `Held`. This
            // subcommand called it, printed "Nothing has actually moved", and
            // then SAVED the wall — so every invocation of a command named
            // *check* created a real hold on disk, and asking the same
            // question three times left three entries in `atlas shared list`.
            match wall.would_stop(&atlas::earned::Space::Personal, into) {
                None => println!("That would go through."),
                Some(why) => {
                    println!("That would stop here — {why}.");
                    println!("Nothing has moved and nothing is held — this is just the answer.");
                    // The third leg, not just the first two: block and pause
                    // are the lines above, and this is what "notify" actually
                    // says. The text is real rather than a stub; what it does
                    // not have behind it is a hold, because nothing crossed.
                    let n = wall.would_say(into, what, &why, now);
                    println!("If it did cross, I'd say: \"{}\" — {}", n.title, n.body);
                }
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try list, release, drop or check."),
    }
}

/// One list, `earned::Space`-generic -- your own tasks and every business's
/// shelf are the same store. See `shared_task.rs`'s own doc for why this is
/// one type rather than two.
/// `atlas clients` — who your clients are, and bringing them in from a
/// phone's contacts or a business card (`.vcf`).
fn run_clients(args: &[String]) {
    let store = atlas::roots::store();
    let mut list = atlas::clients::ClientList::load(&store);
    let now = atlas::store::now();
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            println!("Your clients ({}):", list.len());
            for c in list.all() {
                println!("  {} <{}>{}", c.name_or_address(), c.address, if c.phone.is_empty() { String::new() } else { format!("  {}", c.phone) });
            }
            println!();
            println!("atlas clients add <address> [name]     add one");
            println!("atlas clients import <file.vcf>        bring them in from contacts");
            println!("atlas clients export <file.vcf>        write them out for your phone");
            println!("atlas clients dupes                    who looks like the same person twice");
        }
        Some("add") => match args.get(1) {
            Some(address) if address.contains('@') => {
                let name = args[2..].join(" ");
                list.add(address, &name, "", now);
                keep(list.save(&store), "your clients");
                println!("Added {address}.");
                for d in list.likely_duplicates().iter().filter(|d| d.contains(&address.to_lowercase())) {
                    println!("  Worth a look: {d}");
                }
            }
            _ => println!("atlas clients add <address> [name]"),
        },
        Some("import") => match args.get(1).map(|p| std::fs::read_to_string(p)) {
            Some(Ok(text)) => match list.import_vcf(&text, now) {
                Ok((added, skipped, notes)) => {
                    keep(list.save(&store), "your clients");
                    println!("Brought in {added} client{}.", if added == 1 { "" } else { "s" });
                    if skipped > 0 {
                        println!("Skipped {skipped} card{} with no email address — a client is recognised by address.", if skipped == 1 { "" } else { "s" });
                    }
                    for n in notes {
                        println!("  {n}");
                    }
                }
                Err(e) => println!("That file didn't read as contacts: {e}"),
            },
            Some(Err(e)) => println!("I couldn't open that file: {e}"),
            None => println!("atlas clients import <file.vcf>"),
        },
        Some("export") => match args.get(1) {
            Some(path) => match std::fs::write(path, list.to_vcf()) {
                Ok(()) => println!("Wrote {} client{} to {path}.", list.len(), if list.len() == 1 { "" } else { "s" }),
                Err(e) => println!("I couldn't write {path}: {e}"),
            },
            None => println!("atlas clients export <file.vcf>"),
        },
        Some("dupes") | Some("duplicates") => {
            let d = list.likely_duplicates();
            if d.is_empty() {
                println!("Nobody on the list looks like the same person twice.");
            } else {
                println!("These look like one person under two entries (nothing has been merged):");
                println!("({})", list.weights_note());
                for line in d {
                    println!("  {line}");
                }
            }
        }
        Some(other) => println!("I don't know 'clients {other}'. Try: atlas clients"),
    }
}

/// `atlas my-key` / `seal-file` / `open-file` — files only one person can
/// open (`agefile`, the age v1 format, which the real `age` tool reads too).
/// Your own key lives in the vault; the `age1…` line you give people is kept
/// in the clear beside it, because it is meant to be handed out.
fn run_agefile(words: &[String]) {
    let store = atlas::roots::store();
    let state = atlas::roots::install_state();
    let now = atlas::store::now();
    let vcfg = Config::load(&atlas::roots::config_dir()).ok().and_then(|c| c.tools).map(|t| t.vault).unwrap_or_default();
    const NAME: &str = "age identity";
    let unlock = |vault: &mut atlas::vault::Vault| -> bool {
        if !vault.has_a_passphrase() {
            println!("Your key is kept in the vault, and the vault has no passphrase yet — `atlas vault passphrase` first.");
            return false;
        }
        let Some(phrase) = ask_quietly("Vault passphrase: ") else { return false };
        match vault.open(&phrase, now, &vcfg) {
            Ok(()) => true,
            Err(why) => {
                println!("{why}");
                false
            }
        }
    };
    match words.first().map(|s| s.as_str()) {
        Some("my-key") => {
            let known: String = store.load("age-recipient");
            if !known.is_empty() {
                println!("{known}");
                println!("Give that line to anyone who should be able to send you files only you can open.");
                return;
            }
            let mut vault = atlas::vault::Vault::load(&state);
            if !unlock(&mut vault) {
                return;
            }
            // Already one in the vault (the public half was lost, say): read
            // it back rather than making a second key nobody has.
            if let Ok(identity) = vault.get(NAME, now) {
                vault.lock();
                return match atlas::agefile::recipient_of(&identity) {
                    Ok(r) => {
                        keep(store.save("age-recipient", &r), "your public key");
                        println!("{r}");
                    }
                    Err(why) => println!("The key in your vault didn't read: {why}"),
                };
            }
            let (identity, recipient) = atlas::agefile::new_identity();
            if let Err(why) = vault.put(NAME, atlas::vault::Kind::Note, &identity, now) {
                return println!("I couldn't keep the key: {why}");
            }
            if keep(vault.save(&state), "the vault") && keep(store.save("age-recipient", &recipient), "your public key") {
                println!("{recipient}");
                println!("Made you a key. That line is the public half — give it out freely. The secret half is in your vault.");
            }
            vault.lock();
        }
        Some("seal-file") => {
            // atlas seal-file <file> to <age1…> [age1…]
            let (Some(path), rest) = (words.get(1), &words[2.min(words.len())..]) else {
                return println!("atlas seal-file <file> to <age1… key> [more keys]");
            };
            let mut to: Vec<String> = rest.iter().filter(|w| *w != "to" && *w != "and").cloned().collect();
            let mine: String = store.load("age-recipient");
            if !mine.is_empty() && !to.contains(&mine) {
                to.push(mine); // so you can open what you sent
            }
            let plain = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => return println!("I couldn't read {path}: {e}"),
            };
            match atlas::agefile::seal(&plain, &to) {
                Ok(sealed) => {
                    let out = format!("{path}.age");
                    match std::fs::write(&out, sealed) {
                        Ok(()) => println!("Sealed to {} key{}: {out}. Only those keys open it — with Atlas, or with `age -d`.", to.len(), if to.len() == 1 { "" } else { "s" }),
                        Err(e) => println!("I couldn't write {out}: {e}"),
                    }
                }
                Err(why) => println!("{why}"),
            }
        }
        _ => {
            let Some(path) = words.get(1) else {
                return println!("atlas open-file <file.age>");
            };
            let sealed = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => return println!("I couldn't read {path}: {e}"),
            };
            let mut vault = atlas::vault::Vault::load(&state);
            if !unlock(&mut vault) {
                return;
            }
            let identity = match vault.get(NAME, now) {
                Ok(i) => i,
                Err(_) => return println!("There's no key of yours in the vault yet — `atlas my-key` makes one."),
            };
            vault.lock();
            match atlas::agefile::open(&sealed, &identity) {
                Ok(plain) => {
                    let out = path.strip_suffix(".age").map(String::from).unwrap_or_else(|| format!("{path}.opened"));
                    let out = if std::path::Path::new(&out).exists() { format!("{out}.opened") } else { out };
                    match std::fs::write(&out, plain) {
                        Ok(()) => println!("Opened: {out}"),
                        Err(e) => println!("I couldn't write {out}: {e}"),
                    }
                }
                Err(why) => println!("I couldn't open it: {why}."),
            }
        }
    }
}

/// `atlas notes <recording.wav>` — who said what (`vad` finds the speech,
/// `diarize` groups it by voice, your enrolled voiceprint names you). Uses
/// the same speaker encoder and speech-to-text tools as the voice loop; says
/// which one is missing rather than pretending.
fn run_notes(cfg: &Config, args: &[String], merge_voices: bool, people: Option<usize>) {
    let Some(path) = args.first() else {
        return println!("atlas notes <recording.wav> [--people N] [--merge-voices]");
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return println!("I couldn't open {path}: {e}"),
    };
    let (samples, rate) = match atlas::diarize::read_wav(&bytes) {
        Ok(x) => x,
        Err(e) => return println!("{path}: {e}"),
    };
    let tc = cfg.tools.clone().unwrap_or_default();
    let mut vars = tc.vars.clone();
    // Removed when this command ends (`RunScratch`).
    let scratch = atlas::roots::RunScratch::new("atlas-notes");
    let work = scratch.path().to_path_buf();
    let seg_wav = work.join("segment.wav");
    vars.insert("in_wav".into(), seg_wav.display().to_string());
    vars.insert("work_dir".into(), work.display().to_string());
    // An installed encoder is used when there is one. Otherwise Atlas's own
    // (`speaker::recording_embeddings`): supervectors against the machine's
    // background model, or the recording's own when there's none yet,
    // centred on the recording.
    let external = atlas::speaker::which(&tc.speaker, &vars) == atlas::speaker::Encoder::External;
    let own: Vec<Option<Vec<f32>>> = if external {
        Vec::new()
    } else {
        atlas::speaker::recording_embeddings(&samples, rate, &atlas::speaker::background(&atlas::roots::store()))
            .into_iter()
            .map(|(_, e)| e)
            .collect()
    };
    let can_embed = external || own.iter().flatten().count() >= 2;
    let can_hear = tc.stt.available(&vars);
    if !can_embed {
        println!("(Too little speech in this to tell voices apart — every line is \"Someone\".)");
    } else if !external {
        println!("(Voices told apart by Atlas's own encoder — a classical one, weaker than a trained model.)");
    }
    if !can_hear {
        println!("(Speech-to-text isn't installed, so these are the times people spoke, without the words.)");
    }
    let write = |s: &[i16]| std::fs::write(&seg_wav, atlas::audio::wav_bytes(s, rate)).is_ok();
    // Called once per stretch of speech, in order — the same order
    // `recording_embeddings` produced them in.
    let mut next = own.into_iter();
    let mut embed = |s: &[i16]| -> Option<Vec<f32>> {
        if external {
            (write(s)).then(|| atlas::speaker::embed(&tc.speaker, &vars).ok()).flatten()
        } else {
            next.next().flatten()
        }
    };
    let mut hear = |s: &[i16]| -> Option<String> {
        (can_hear && write(s)).then(|| tc.stt.run(&vars, None).ok().map(|raw| atlas::voice::clean_transcript(&raw))).flatten()
    };
    // Your print is only comparable when it came from the same encoder
    // against the same background; a per-recording background is not the
    // machine's, so with the built-in encoder "You" is not claimed here.
    let you = if external { atlas::voiceid::VoiceId::load(&atlas::roots::store()).print.map(|p| p.centroid) } else { None };
    let lines = if external {
        atlas::diarize::who_said_what(&samples, rate, &mut embed, &mut hear, you.as_deref(), tc.voice_id.accept)
    } else {
        atlas::diarize::who_said_what_grouped(&samples, rate, &mut embed, &mut hear, None, tc.voice_id.builtin_accept, atlas::speaker::GROUP_CENTRED_AT)
    };
    // A second look on each speaker's pooled speech (`diarize::
    // merge_same_voices`), only when asked for: measured on sixty calls it
    // puts a split voice back together, and on calls it wasn't tuned on it
    // also put two people under one name once in twenty. Splitting one
    // person is the safer mistake for notes, so it stays opt-in.
    let lines = if merge_voices {
        atlas::diarize::merge_same_voices(&samples, rate, lines, atlas::diarize::SAME_VOICE_LAMBDA)
    } else {
        lines
    };
    // Someone who spoke once, swallowed by the nearest voice, is given back a
    // name of their own (`diarize::split_strangers`) — on by default, because
    // on calls it wasn't tuned on it mixed fewer people and split no more.
    let lines = atlas::diarize::split_strangers(&samples, rate, lines, atlas::diarize::STRANGER_MARGIN);
    // Told how many people there were: that settles it (`diarize::to_count`).
    let lines = match people {
        Some(n) => atlas::diarize::to_count(&samples, rate, lines, n),
        None => lines,
    };
    let _ = std::fs::remove_dir_all(&work);
    if lines.is_empty() {
        return println!("I didn't find any speech in {path}.");
    }
    for l in &lines {
        println!("{}", l.say());
    }
}

/// `atlas fix <folder> <what you wanted> -- <test command>` — work a failing
/// test with the model until it passes (`fixloop`), in a copy of the folder.
/// Shows the tested diff; `--land` puts it in the folder, originals kept.
fn run_fix(cfg: &Config, args: &[String]) {
    let Some(split) = args.iter().position(|a| a == "--") else {
        return println!("atlas fix <folder> <what you wanted> -- <test command> [--land]");
    };
    let land = args.iter().any(|a| a == "--land");
    let head: Vec<&String> = args[..split].iter().filter(|a| *a != "--land").collect();
    let test: Vec<String> = args[split + 1..].iter().filter(|a| *a != "--land").cloned().collect();
    let Some(folder) = head.first() else { return println!("which folder?") };
    let goal = head[1..].iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
    let Some(tc) = cfg.tools.as_ref() else { return println!("no config/tools.yaml") };
    let Some(llm) = model_connection(tc) else {
        return println!("There's no model to work it with. Set one up (the models folder, or `llm:` in tools.yaml) and try again.");
    };
    let job = atlas::fixloop::Job { folder: std::path::PathBuf::from(folder.as_str()), test, goal };
    let mut counsel = atlas::fixloop::ModelCounsel::new(llm.as_ref());
    match atlas::fixloop::run(&job, &mut counsel, &atlas::roots::tmp_dir().join("fix"), &tc.strategy, &tc.handoff, &tc.consult) {
        Ok(o) => {
            for s in &o.steps {
                println!("  {s}");
            }
            if o.solved {
                println!("\nFixed and tested in a copy ({}), {} attempt(s):\n\n{}", o.copy.display(), o.attempts, o.diff);
                if land {
                    match atlas::fixloop::land(&o, &job.folder) {
                        Ok(f) => println!("Landed: {} (each original kept as .before).", f.join(", ")),
                        Err(e) => println!("{e}"),
                    }
                } else {
                    println!("Nothing in your folder has changed. Run it again with --land to put this in.");
                }
            } else if let Some(b) = o.brief {
                println!("\nDidn't get there. The write-up, for whoever picks it up:\n\n{b}");
            }
        }
        Err(e) => println!("I couldn't work on it: {e}. If that's the model, is its server running? Nothing in your folder changed."),
    }
}

/// `atlas wake-word`, `atlas hearing`, `atlas voices` — teaching Atlas your
/// phrase, your room and the voices that aren't you, from recordings. The
/// hub's calendar page does the same with buttons.
fn run_voice_lab(cfg: &Config, words: &[String]) {
    let store = atlas::roots::store();
    let read = |p: &String| -> Option<(Vec<i16>, u32)> {
        match std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| atlas::diarize::read_wav(&b)) {
            Ok(x) => Some(x),
            Err(e) => {
                println!("{p}: {e}");
                None
            }
        }
    };
    match (words[0].as_str(), words.get(1).map(|s| s.as_str())) {
        ("wake-word", Some("forget")) => match atlas::wakeword::forget(&store) {
            Ok(()) => println!("Forgotten. I'm back to listening for the phrase through speech-to-text."),
            Err(e) => println!("couldn't forget it: {e}"),
        },
        ("wake-word", Some("test")) => {
            let Some(m) = atlas::wakeword::load(&store) else { return println!("No phrase taught yet: atlas wake-word <take.wav> (three times).") };
            for p in &words[2..] {
                if let Some((s, r)) = read(p) {
                    let c = atlas::wakeword::closest(&s, r, &m);
                    println!("{p}: {} (closest {:.2}, heard at or under {:.2})", if atlas::wakeword::heard(&s, r, &m) { "heard" } else { "not heard" }, c.map(|x| x.0).unwrap_or(f32::INFINITY), m.threshold);
                }
            }
        }
        ("wake-word", Some(_)) => {
            for p in &words[1..] {
                if let Some((s, r)) = read(p) {
                    match atlas::wakeword::add_take(&store, &s, r) {
                        Ok(said) => println!("{p}: {said}"),
                        Err(e) => println!("{p}: {e}"),
                    }
                }
            }
        }
        ("hearing", Some(room)) if words.len() >= 3 => {
            let current = cfg.tools.as_ref().map(|t| t.endpoint.vad_params()).unwrap_or_default();
            let (Ok(a), Ok(b)) = (std::fs::read(room), std::fs::read(&words[2])) else { return println!("I couldn't open one of those files.") };
            match atlas::vadcal::from_recordings(&a, &b, current, &atlas::roots::config_dir()) {
                Ok(o) => println!("{}", atlas::vadcal::Outcome::say(&o)),
                Err(e) => println!("{e}"),
            }
        }
        ("voices", Some(_)) => {
            for p in &words[1..] {
                if let Some((s, r)) = read(p) {
                    match atlas::speaker::learn_background(&s, r, &store) {
                        Ok(n) => println!("{p}: learned from {n} stretch{} of speech. {}", if n == 1 { "" } else { "es" }, if atlas::speaker::background(&store).ready() { "That's enough to tell voices apart." } else { "" }),
                        Err(e) => println!("{p}: {e}"),
                    }
                }
            }
            if !atlas::speaker::background(&store).ready() {
                println!("{}", atlas::speaker::still_learning(&atlas::speaker::background(&store)));
            }
        }
        _ => {
            println!("atlas wake-word <take.wav> [...]     teach your phrase (three takes)");
            println!("atlas wake-word test <clip.wav>      would that have woken me?");
            println!("atlas wake-word forget               back to speech-to-text");
            println!("atlas hearing <room.wav> <you.wav>   tune the speech detector to your room");
            println!("atlas voices <recording.wav> [...]   learn what voices that aren't you sound like");
        }
    }
}

/// `atlas doc` — a page both of your machines can edit while apart, that
/// comes back together without a clash to settle (`yata`). The edits ride in
/// the sync log, so the next `atlas sync` carries them.
fn run_doc(args: &[String]) {
    let store = atlas::roots::store();
    // Read, never written from here: the running Atlas is the sync log's one
    // writer. Edits go to its inbox (`yata::queue`) and it takes them in on
    // its next tick.
    let log: atlas::sync::Log = store.load("synclog");
    let site = if log.device.trim().is_empty() {
        let named = Config::load(&atlas::roots::config_dir()).ok().and_then(|c| c.tools).map(|t| t.sync.name).unwrap_or_default();
        if named.trim().is_empty() { "this device".to_string() } else { named.trim().to_string() }
    } else {
        log.device.clone()
    };
    let inbox = store.data_dir().join("doc-inbox");
    match (args.first().map(|s| s.as_str()), args.get(1)) {
        (Some("show"), Some(name)) => {
            let d = atlas::yata::current(name, &site, &log.events, &inbox);
            print!("{}", d.text());
            if d.waiting() > 0 {
                println!("\n({} edits are waiting on others that haven't arrived yet.)", d.waiting());
            }
        }
        (Some("set"), Some(name)) | (Some("add"), Some(name)) => {
            let adding = args[0] == "add";
            let new_text = match args.get(2) {
                Some(path) if std::path::Path::new(path).is_file() => match std::fs::read_to_string(path) {
                    Ok(t) => t,
                    Err(e) => return println!("I couldn't read {path}: {e}"),
                },
                _ => args[2..].join(" "),
            };
            let mut d = atlas::yata::current(name, &site, &log.events, &inbox);
            let target = if adding {
                let now_text = d.text();
                let sep = if now_text.is_empty() || now_text.ends_with('\n') { "" } else { "\n" };
                format!("{now_text}{sep}{}\n", new_text.trim_end())
            } else {
                new_text
            };
            let before = d.text();
            let ops = d.set_text(&target);
            if ops.is_empty() {
                return println!("No change.");
            }
            match atlas::yata::queue(&inbox, name, &ops) {
                Ok(()) => println!(
                    "Saved \"{name}\" ({} character edits, {} lines changed). It goes to your other machines with the next sync.",
                    ops.len(),
                    atlas::diff::lines_changed(&before, &d.text())
                ),
                Err(e) => return println!("I couldn't save that: {e}"),
            }
            // No Atlas running to take the inbox in: take it in here, under
            // the same one-at-a-time lock the daemon holds, so there is still
            // only ever one writer of the sync log.
            let now = atlas::store::now();
            let lock = atlas::onlyone::OnlyOne::at(&store.data_dir());
            if lock.take(now).is_ok() {
                let mut log: atlas::sync::Log = store.load("synclog");
                if log.device.trim().is_empty() {
                    log = atlas::sync::Log::new(&site);
                }
                let taken = atlas::yata::take_queued(&inbox, &mut log, now);
                if !taken.is_empty() && store.save("synclog", &Some(log)).is_ok() {
                    for f in taken {
                        let _ = std::fs::remove_file(f);
                    }
                }
                lock.release();
            }
        }
        (Some("list"), _) | (None, _) => {
            let names = atlas::yata::all_names(&log.events, &inbox);
            if names.is_empty() {
                println!("No shared pages yet. atlas doc set <name> <text or file>");
            }
            for n in names {
                let t = atlas::yata::current(&n, &site, &log.events, &inbox).text();
                println!("{n} — {} lines", t.lines().count());
            }
        }
        _ => println!("atlas doc list | show <name> | set <name> <text or file> | add <name> <line>"),
    }
}

/// Your `time_zone` setting as a zone; UTC when unset.
fn your_zone() -> atlas::tz::Zone {
    let set = Config::load(&atlas::roots::config_dir()).ok().and_then(|c| c.tools).map(|t| t.time_zone).unwrap_or_default();
    atlas::tz::home(&set)
}

/// `atlas calendar import|export` — `.ics`, the file every other calendar
/// speaks, so an invite from anyone lands here and yours can go anywhere.
fn run_calendar(args: &[String]) {
    let store = atlas::roots::store();
    let mut cal = atlas::calendar::Calendar::load(&store);
    let now = atlas::store::now();
    let zone = your_zone();
    if zone.is_utc() {
        if let Some(z) = atlas::tz::suggest() {
            println!("(No time zone is set, so times are shown in UTC. This computer says {} — pick it under Settings → Time zone.)", z.name);
        }
    }
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("import") => match args.get(1).map(|p| std::fs::read_to_string(p)) {
            Some(Ok(text)) => match cal.import_ics(&text, now, &zone) {
                Ok((n, unknown)) => {
                    keep(cal.save(&store), "your calendar");
                    println!("{n} event{} added or updated.", if n == 1 { "" } else { "s" });
                    for id in &unknown {
                        println!("  (The file names a time zone I don't know, \"{id}\"; those times were read as {}.)", zone.name);
                    }
                    for e in cal.occurrences_between(now, now + 14 * 86_400).iter().take(5) {
                        println!("  {} — {}", e.title, e.say_when_in(&zone));
                    }
                }
                Err(e) => println!("That file didn't read as a calendar: {e}"),
            },
            Some(Err(e)) => println!("I couldn't open that file: {e}"),
            None => println!("atlas calendar import <file.ics>"),
        },
        Some("export") => match args.get(1) {
            Some(path) => match std::fs::write(path, cal.to_ics(now)) {
                Ok(()) => println!("Wrote {} event{} to {path}.", cal.len(), if cal.len() == 1 { "" } else { "s" }),
                Err(e) => println!("I couldn't write {path}: {e}"),
            },
            None => println!("atlas calendar export <file.ics>"),
        },
        _ => {
            println!("atlas calendar import <file.ics>   bring in an invite or another calendar");
            println!("atlas calendar export <file.ics>   write yours out for any other calendar");
        }
    }
}

fn run_tasks(args: &[String]) {
    let store = atlas::roots::store();
    let mut tasks = atlas::shared_task::Tasks::load(&store);
    let now = atlas::store::now();

    let print_task = |t: &atlas::shared_task::Task| {
        let mark = if t.done { "x" } else { " " };
        let shared = if t.shared_from_personal { "  (shared from personal)" } else { "" };
        println!("  [{mark}] {}  {}{shared}", t.id, t.description);
    };

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            println!("Your own tasks, most pressing first:");
            for (t, why) in tasks.in_order(&atlas::earned::Space::Personal, now) {
                print_task(t);
                if !why.is_empty() {
                    println!("        {why}");
                }
            }
            println!();
            println!("atlas tasks add <description>          add one of your own");
            println!("atlas tasks done <id>                   mark it done");
            println!("atlas tasks share <id> <business>       share one into a business's shelf");
            println!("atlas tasks for <business>               show that business's shelf");
            println!("atlas tasks roster <business> <peer>     let a paired Atlas see that business");
            println!("atlas tasks unroster <business> <peer>   take that access away");
        }
        Some("add") => {
            let description = args[1..].join(" ");
            if description.trim().is_empty() {
                println!("atlas tasks add <description>");
                return;
            }
            // "… by Friday", "… due tomorrow at 5": the part after the last
            // " by "/" due " is read with the calendar's own day reader; if
            // it names a day, that is the due date and it comes off the
            // description. Nothing it cannot read becomes a date.
            let low = description.to_ascii_lowercase();
            let cut = [" by ", " due "].iter().filter_map(|k| low.rfind(k).map(|i| (i, k.len()))).max();
            let (text, due) = match cut {
                // Read on your clock and kept as the real moment, like every
                // other time the calendar reads (`resolve_when_in`).
                Some((i, k)) => match atlas::calendar::resolve_when_in(&description[i + k..], now, &atlas::localclock::zone()) {
                    Some(w) => (description[..i].trim().to_string(), Some(w.start)),
                    None => (description.trim().to_string(), None),
                },
                None => (description.trim().to_string(), None),
            };
            let id = tasks.add(atlas::earned::Space::Personal, &text, due, now);
            keep(tasks.save(&store), "your tasks");
            match due {
                Some(d) => println!("Added as {id}, due {}.", atlas::digest::iso_utc(d)),
                None => println!("Added as {id}."),
            }
        }
        Some("done") => match args.get(1).and_then(|n| n.parse::<u64>().ok()) {
            Some(id) if tasks.complete(id) => {
                keep(tasks.save(&store), "your tasks");
                println!("Done.");
            }
            Some(_) => println!("That's already done, or there's no task with that id."),
            None => println!("Which one? atlas tasks done 3"),
        },
        Some("share") => {
            let (Some(id), Some(business)) =
                (args.get(1).and_then(|n| n.parse::<u64>().ok()), args.get(2))
            else {
                println!("atlas tasks share <id> <business>");
                return;
            };
            let mut wall = atlas::firewall::Firewall::load(&store);
            match tasks.share_into_business(id, business, &mut wall, now) {
                atlas::firewall::Crossing::Allowed => {
                    println!("Shared -- that was already {business}'s own.");
                }
                atlas::firewall::Crossing::Stopped { held, why } if held > 0 => {
                    println!("That stops at the line -- {why}.");
                    println!("Held as {held}. Run `atlas shared release {held}` to let it through,");
                    println!("then `atlas tasks release {held}` to actually finish the share.");
                    // The third leg, on the crossing that actually happened.
                    //
                    // `firewall::note` is the "notify" half of block/pause/
                    // notify, and its only caller was `atlas shared check` —
                    // the dry run. So Atlas built the notification for a
                    // crossing that never occurred and built nothing for the
                    // one that did. A real thing stopped at the boundary told
                    // Eric nothing beyond whatever scrollback he happened to
                    // be looking at.
                    if let Some(h) = wall.get(held) {
                        let n = atlas::firewall::note(h);
                        println!("{} — {}", n.title, n.body);
                    }
                }
                atlas::firewall::Crossing::Stopped { why, .. } => println!("Couldn't share that: {why}."),
            }
            keep(tasks.save(&store), "your tasks");
            keep(wall.save(&store), "the shared wall");
        }
        Some("release") => match args.get(1).and_then(|n| n.parse::<u64>().ok()) {
            Some(held_id) => {
                let wall = atlas::firewall::Firewall::load(&store);
                match tasks.complete_release(held_id, &wall, now) {
                    Some(new_id) => {
                        keep(tasks.save(&store), "your tasks");
                        println!("Finished -- now on the business's shelf as {new_id}.");
                    }
                    None => println!(
                        "Nothing to finish -- either that hold was never a task share, or it \
                         hasn't been released yet with `atlas shared release {held_id}`."
                    ),
                }
            }
            None => println!("Which held item? atlas tasks release 3"),
        },
        Some("for") => match args.get(1) {
            Some(business) => {
                println!("{business}'s shelf:");
                for (t, _) in tasks.in_order(&atlas::earned::Space::Business(business.clone()), now) {
                    print_task(t);
                }
            }
            None => println!("atlas tasks for <business>"),
        },
        Some("roster") => {
            let (Some(business), Some(peer)) = (args.get(1), args.get(2)) else {
                println!("atlas tasks roster <business> <peer>");
                return;
            };
            let pairings = atlas::kin::Pairings::load(store.root());
            let mut roster = atlas::roster::Roster::load(&store);
            match roster.add(business, peer, &pairings) {
                Ok(()) => {
                    keep(roster.save(&store), "the roster");
                    println!("{peer} can now see {business}'s shared shelf.");
                }
                Err(e) => println!("Couldn't add them: {}", e.plain()),
            }
        }
        Some("unroster") => {
            let (Some(business), Some(peer)) = (args.get(1), args.get(2)) else {
                println!("atlas tasks unroster <business> <peer>");
                return;
            };
            let mut roster = atlas::roster::Roster::load(&store);
            if roster.remove(business, peer) {
                keep(roster.save(&store), "the roster");
                println!("{peer} can no longer see {business}'s shared shelf.");
            } else {
                println!("{peer} wasn't on {business}'s roster.");
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try list, add, done, share, release, for, roster or unroster."),
    }
}

/// Which Atlas this is, and which of your own devices belong to it. See
/// `household.rs`'s own doc: two installs know nothing about each other
/// unless the same person paired them, device to device, with a code.
fn run_household(args: &[String]) {
    let store = atlas::roots::store();
    // The sync folder, for handing the household key to a device being
    // paired. Read here rather than passed in, like `trade_cfgs` and
    // `trading_cfg` do -- `run_household` is reached from a dispatch that
    // has no `&Config` to give it.
    let sync_folder = || -> String {
        Config::load(&atlas::roots::config_dir())
            .ok()
            .and_then(|c| c.tools.map(|t| t.sync.folder))
            .unwrap_or_default()
    };
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            let h = atlas::household::Household::load(&store);
            if !h.is_set() {
                println!("No household yet. atlas household init <name>");
                return;
            }
            println!("{} — {} device(s), made {}", h.name, h.devices.len(), h.made_at);
            for d in &h.devices {
                println!("  {d}");
            }
        }
        Some("proof") => {
            let id_cfg = Config::load(&atlas::roots::config_dir())
                .ok()
                .and_then(|c| c.tools.map(|t| t.identity))
                .unwrap_or_default();
            let identity = atlas::identity::Identity::load(&store);
            match atlas::identity::grace_remaining(&identity, &id_cfg, atlas::store::now()) {
                Some(secs) => println!("Proven for another {} minutes.", secs / 60),
                None => println!("Not currently proven — the next watched action will ask."),
            }
        }
        Some("init") => {
            let h = atlas::household::Household::load(&store);
            if h.is_set() {
                println!(
                    "Already set up as {}. There's no re-init — a second household id for the \
                     same devices is exactly the confusion this file exists to prevent.",
                    h.name
                );
                return;
            }
            let name = args[1..].join(" ");
            if name.trim().is_empty() {
                println!("atlas household init <name>");
                return;
            }
            match atlas::household::init(&store, &name, "", atlas::store::now()) {
                Ok(_) => {
                    println!("{}", atlas::household::THEIRS_IS_THEIRS);
                    println!("\n{}", atlas::household::SEPARATE_BY_DEFAULT);
                }
                Err(why) => println!("{why}"),
            }
        }
        Some("pair") => {
            let h = atlas::household::Household::load(&store);
            if !h.is_set() {
                println!("Set up a household first: atlas household init <name>");
                return;
            }
            let device = args[1..].join(" ");
            if device.trim().is_empty() {
                println!("atlas household pair <name for the new device>");
                return;
            }
            let now = atlas::store::now();
            let folder = sync_folder();

            // The short way, when both machines can see the same folder:
            // everything bulky goes in the folder sealed, and the person
            // types ten characters. The household key goes with it, so
            // joining and being able to read sealed bundles are one act.
            if !folder.trim().is_empty() {
                let short = atlas::household::new_invite_code();
                let kept: atlas::sync::KeptKey = store.load(atlas::sync::KEY_FILE);
                let phrase = kept.is_set().then(|| kept.phrase().ok()).flatten();
                match atlas::household::leave_invitation(
                    std::path::Path::new(folder.trim()),
                    &h.id,
                    &h.name,
                    &short,
                    phrase.as_deref(),
                    now,
                    180,
                ) {
                    Ok(_) => {
                        println!("On {}, within 3 minutes:", device.trim());
                        println!();
                        println!("    atlas household join {short} \"{}\"", device.trim());
                        println!();
                        println!("Or open the hub there, go to Your devices, and type:");
                        println!();
                        println!("    {short}");
                        println!();
                        if phrase.is_some() {
                            println!(
                                "The household key goes with it, so sealed bundles from here \
                                 will open there straight away."
                            );
                        }
                        println!(
                            "Nothing in {} says whose it is or what is in it -- the code is \
                             the only thing that opens it, and it clears itself either way.",
                            folder.trim()
                        );
                        return;
                    }
                    Err(why) => println!(
                        "(I couldn't leave an invitation in your sync folder: {why} -- \
                         falling back to the long code.)"
                    ),
                }
            }

            // No shared folder: the old way, which carries everything in the
            // code itself and is why it is long.
            let code = match atlas::server::new_token() {
                Ok(t) => t,
                Err(e) => {
                    println!("Couldn't generate a pairing code: {e}");
                    return;
                }
            };
            let pairing = atlas::household::new_pairing(&code, now);
            match atlas::household::encode_pairing(&h.id, &h.name, &pairing) {
                Some(block) => {
                    println!(
                        "On {}, within 3 minutes, run:\n  atlas household join \"{block}\" \"{}\"",
                        device.trim(),
                        device.trim()
                    );
                    // The household key goes with it, if there is one and a
                    // folder both can see. Sealed under the pairing code,
                    // which is a one-use token with real entropy, and gone
                    // when it is taken or when the three minutes are up.
                    //
                    // Without this the new device is paired and still cannot
                    // read a single sealed bundle until somebody copies a key
                    // file by hand -- which is the step this whole design is
                    // trying not to ask for.
                    let kept: atlas::sync::KeptKey = store.load(atlas::sync::KEY_FILE);
                    let folder = sync_folder();
                    if kept.is_set() && !folder.trim().is_empty() {
                        match kept.phrase().and_then(|phrase| {
                            atlas::sync::leave_handoff(
                                std::path::Path::new(folder.trim()),
                                &h.id,
                                &code,
                                &phrase,
                                now,
                                pairing.valid_secs,
                            )
                            .map_err(|e| e)
                        }) {
                            Ok(_) => println!(
                                "\nI've left the key for it in your sync folder, sealed under \
                                 that code. It'll pick it up when it joins, and the file goes \
                                 either way once the three minutes are up."
                            ),
                            Err(why) => println!("\n(I couldn't leave the key for it: {why})"),
                        }
                    } else if kept.is_set() {
                        println!(
                            "\nYour sync folder isn't set, so I can't hand the key over \
                             automatically. Set it on both machines and pair again, or copy \
                             {} across.",
                            atlas::sync::card_path().display()
                        );
                    }
                }
                None => println!("Couldn't build a pairing code -- the household name has a '|' in it."),
            }
        }
        Some("join") => {
            let Some(code) = args.get(1) else {
                println!("atlas household join <code> <this device's name>");
                return;
            };
            let device = args[2..].join(" ");
            let mine = atlas::household::Household::load(&store);

            // A short code first: ten characters, with everything else
            // waiting in the folder both machines can see. The long block
            // still works below, for a pairing started before this existed
            // and for two machines with no folder in common.
            let short = atlas::household::take_invitation(
                std::path::Path::new(sync_folder().trim()),
                code,
                atlas::store::now(),
            );
            if let Ok(inside) = short {
                if mine.is_set() && mine.id != inside.for_household {
                    println!(
                        "This device already belongs to {} -- joining {} would mean two \
                         households on one machine, which this file exists to prevent.",
                        mine.name, inside.name
                    );
                    return;
                }
                if device.trim().is_empty() {
                    println!("atlas household join <code> <this device's name>");
                    return;
                }
                let joined = atlas::household::Household {
                    id: inside.for_household.clone(),
                    name: inside.name.clone(),
                    made_at: atlas::store::now(),
                    devices: vec![device.trim().to_string()],
                };
                match joined.save(&store) {
                    Ok(()) => println!("Joined. This device now belongs to {}.", joined.name),
                    Err(e) => {
                        println!("Couldn't save that: {e}");
                        return;
                    }
                }
                match inside.key_phrase {
                    Some(phrase) => {
                        let keeping =
                            atlas::sync::KeptKey::keeping(&phrase, atlas::store::now());
                        match store.save(atlas::sync::KEY_FILE, &keeping) {
                            Ok(()) => {
                                println!(
                                    "The household key came with it, so sealed bundles from \
                                     your other machine open here."
                                );
                                if let Ok(card) = atlas::sync::write_card(&phrase) {
                                    println!("Written down in {}.", card.display());
                                }
                            }
                            Err(e) => println!("I got the key and couldn't keep it: {e}"),
                        }
                    }
                    None => println!(
                        "(No household key came with it -- the other machine isn't sealing \
                         what it carries.)"
                    ),
                }
                return;
            }

            let Some((their_id, their_name, pairing)) = atlas::household::decode_pairing(code) else {
                // The short path's reason is the useful one here: a mistyped
                // ten-character code is far more likely than a mangled block.
                println!("{}", short.unwrap_err());
                return;
            };
            let meeting = atlas::household::meets(
                &mine.id,
                &their_id,
                mine.is_set() && mine.id == their_id,
            );
            println!("{}", atlas::household::saw_another(&meeting));
            match meeting {
                atlas::household::Meeting::Mine => {
                    println!("Already paired -- nothing to do.");
                }
                atlas::household::Meeting::NotMine if mine.is_set() => {
                    println!(
                        "This device already belongs to {} -- joining {their_name} would mean \
                         two households on one machine, which this file exists to prevent.",
                        mine.name
                    );
                }
                _ => {
                    if !pairing.still_good(atlas::store::now()) {
                        println!("That code has expired -- ask for a fresh one with `atlas household pair`.");
                        return;
                    }
                    if device.trim().is_empty() {
                        println!("atlas household join <code> <this device's name>");
                        return;
                    }
                    let joined = atlas::household::Household {
                        id: their_id,
                        name: their_name,
                        made_at: atlas::store::now(),
                        devices: vec![device.trim().to_string()],
                    };
                    match joined.save(&store) {
                        Ok(()) => {
                            println!("Joined. This device now belongs to {}.", joined.name);
                            // And the key, if the other side left one. This
                            // is the whole point of doing it here: the code
                            // that proved the pairing is the same code that
                            // opens the key, so there is nothing further to
                            // type and nothing to copy.
                            let folder = sync_folder();
                            if folder.trim().is_empty() {
                                println!(
                                    "Your sync folder isn't set here yet. Set `sync.folder` to \
                                     the folder the other machine uses and pair again, and \
                                     I'll pick up the key with it."
                                );
                            } else {
                                match atlas::sync::take_handoff(
                                    std::path::Path::new(folder.trim()),
                                    &joined.id,
                                    &pairing.code,
                                    atlas::store::now(),
                                ) {
                                    Ok(phrase) => {
                                        let keeping = atlas::sync::KeptKey::keeping(
                                            &phrase,
                                            atlas::store::now(),
                                        );
                                        match store.save(atlas::sync::KEY_FILE, &keeping) {
                                            Ok(()) => {
                                                println!(
                                                    "I picked up the household key as well, so \
                                                     sealed bundles from your other machine will \
                                                     open here."
                                                );
                                                if let Ok(card) =
                                                    atlas::sync::write_card(&phrase)
                                                {
                                                    println!(
                                                        "Written down in {}.",
                                                        card.display()
                                                    );
                                                }
                                            }
                                            Err(e) => println!(
                                                "I got the key and couldn't keep it: {e}"
                                            ),
                                        }
                                    }
                                    // Not an error worth alarming anyone
                                    // with: most pairings have no key to
                                    // carry, because sealing is off.
                                    Err(why) => println!("(No key came across: {why})"),
                                }
                            }
                        }
                        Err(e) => println!("Couldn't save that: {e}"),
                    }
                }
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try status, proof, init, pair or join."),
    }
}

/// List and restore backups, checking whose household a backup actually
/// belongs to before anything is copied over your own.
/// `atlas trace` — every model call this install has made.
///
/// The flight recorder, read back. Whether the local model is good enough, or
/// a question needs the bigger one, is a measurement — and this is where the
/// measurement lives. Never the words of a prompt: see
/// `trace::STORES_NO_CONTENT`.
/// `atlas search check`: how well search finds the right note, measured the
/// same way Atlas measures it whenever its meaning model changes — known
/// questions (yours in `search-questions.txt`, plus some made from the notes),
/// scored by words alone and by meaning. Meaning uses only vectors already
/// made with the current model, so this never runs the encoder over the lot.
fn run_search_check() {
    let store = atlas::roots::store();
    let tools = Config::load(&atlas::roots::config_dir()).ok().and_then(|c| c.tools).unwrap_or_default();
    let notes = tools.research.clone().resolved(&store.install_root()).notes_dir;
    let mut lib = atlas::recall::library_from_dir(std::path::Path::new(&notes));
    if lib.is_empty() {
        println!("There are no notes in {notes} to search, so there's nothing to measure.");
        return;
    }
    let mut questions = atlas::recall::questions_written(
        &std::fs::read_to_string(store.root().join("search-questions.txt")).unwrap_or_default(),
    );
    questions.extend(atlas::recall::questions_from(&lib, 40));
    if questions.is_empty() {
        println!("The notes are too short to make questions from.");
        return;
    }
    let now = atlas::store::now();
    let words = atlas::recall::measure(&lib, &questions, None, &tools.recall, now);
    let fp = atlas::meaning::fingerprint(&tools.meaning, &tools.vars);
    let remembered = atlas::meaning::Remembered::load(&store);
    let same_model = !fp.is_empty() && remembered.model() == fp;
    let mut have = 0usize;
    if same_model {
        for p in lib.pieces.iter_mut() {
            p.embedding = remembered.get(&p.title, &p.text).cloned();
            have += p.embedding.is_some() as usize;
        }
    }
    let usable = tools.recall.semantic && have > 0 && atlas::meaning::available(&tools.meaning, &tools.vars);
    let meaning = usable.then(|| {
        let embed = |q: &str| atlas::meaning::embed(&tools.meaning, &tools.vars, q).ok();
        atlas::recall::measure(&lib, &questions, Some(&embed), &tools.recall, now)
    });
    let check = atlas::recall::SearchCheck { at: now, model: if usable { fp } else { String::new() }, words, meaning };
    let kept: Vec<atlas::recall::SearchCheck> = store.load("search_checks");
    println!("{}", check.said(kept.last()));
    if !usable {
        println!(
            "Meaning search wasn't measured: {}.",
            if !tools.recall.semantic {
                "it's switched off"
            } else if !same_model || have == 0 {
                "no note has a vector from the current model yet (Atlas makes them in the background)"
            } else {
                "the meaning model isn't installed"
            }
        );
    }
}

fn run_trace(args: &[String]) {
    // The store, not `data/logs`. `Daemon::trace_path` writes into the store
    // root, and the first version of this command read a different directory
    // — so `atlas trace` reported an empty log while the daemon was filling
    // one two folders away. `tests/flight_recorder.rs` holds the two together.
    let path = atlas::trace::log_path(atlas::roots::store().root());
    let t = atlas::trace::load(&path);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            println!("{}", t.spoken());
            if t.calls.is_empty() {
                println!("{}", path.display());
                return;
            }
            // Per module, because "what is actually using the model" is never
            // what you expect -- the module header says so and this is the
            // line that shows it.
            let mut by: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
            for c in &t.calls {
                let e = by.entry(c.asked_by.as_str()).or_default();
                e.0 += 1;
                if !c.ok() {
                    e.1 += 1;
                }
            }
            for (who, (n, bad)) in &by {
                let rate = t.failure_rate(who);
                println!(
                    "  {who}: {n} call{}, {bad} failed ({:.0}%)",
                    if *n == 1 { "" } else { "s" },
                    rate * 100.0
                );
            }
            let mut models: std::collections::BTreeSet<&str> =
                t.calls.iter().map(|c| c.model.as_str()).collect();
            models.remove("none");
            for m in models {
                println!("  {m}: typically {}ms", t.typical_ms(m));
            }
            let corrections = t.caused_corrections().len();
            if corrections > 0 {
                println!("  {corrections} call(s) you went on to correct");
            }
            println!("{}", path.display());
        }
        // `atlas trace grades`: how each kind of call is doing, graded
        // without a model — a read-back that found a chatbot's voice, a
        // figure missing from its sources, a seat that wouldn't commit, your
        // corrections.
        Some("grades") => {
            let card = t.scorecard();
            if card.is_empty() {
                println!("Nothing graded yet.");
            }
            for s in &card {
                println!("  {}", s.said());
            }
            let kept = atlas::trace::examples(&path).len();
            println!(
                "The call log keeps no words ({}). The words of graded calls are kept apart, scrubbed: \
                 {kept} so far (`atlas trace examples`). Below {} grades a kind is counted, not rated.",
                atlas::trace::STORES_NO_CONTENT,
                atlas::trace::ENOUGH_TO_MEASURE
            );
        }
        Some("examples") => {
            // The graded calls whose words were kept: what a new model can be
            // tested against, one line of JSON each, in the file named below.
            let all = atlas::trace::examples(&path);
            if all.is_empty() {
                println!("No graded examples kept yet.");
            }
            let mut by: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
            for e in &all {
                let n = by.entry(e.asked_by.as_str()).or_default();
                if e.good { n.0 += 1 } else { n.1 += 1 }
            }
            for (who, (good, bad)) in by {
                println!("  {who}: {good} good, {bad} bad");
            }
            println!("Kept in {}. Emails, phone numbers, long numbers, web-address queries and \
                      anything after \"password\" are taken out; names and addresses are not.",
                     atlas::trace::examples_path(&path).display());
        }
        Some("compact") => {
            let before = t.calls.len();
            match atlas::trace::compact(&path, atlas::trace::KEEP) {
                Ok(kept) if kept == before => println!("Nothing to drop — {before} calls."),
                Ok(kept) => println!("Kept the newest {kept} of {before}."),
                Err(e) => println!("Couldn't rewrite the log: {e}"),
            }
        }
        Some("failures") => {
            let bad: Vec<&atlas::trace::Call> = t.calls.iter().filter(|c| !c.ok()).collect();
            if bad.is_empty() {
                println!("No failed calls on record.");
                return;
            }
            for c in bad.iter().rev().take(20) {
                println!(
                    "  {} {} — {}",
                    c.at,
                    c.asked_by,
                    c.failed.as_deref().unwrap_or("(no reason recorded)")
                );
            }
        }
        Some(other) => println!("Don't know `atlas trace {other}`. Try status, failures or compact."),
    }
}

/// `atlas index` — the notes index, from outside the daemon.
///
/// The daemon notices drift hourly and offers to fix it. This is the same
/// thing on demand, and it is also the only way to look at the index without
/// waiting an hour to be told about it.
fn run_index(args: &[String]) {
    let cfg = Config::load(&atlas::roots::config_dir()).ok();
    let dir: std::path::PathBuf = cfg
        .as_ref()
        .and_then(|c| c.tools.as_ref())
        .map(|t| t.research.notes_dir.clone())
        .unwrap_or_else(|| atlas::roots::notes_dir().to_string_lossy().into_owned())
        .into();
    let folder = dir.to_string_lossy().to_string();
    let master = atlas::contents::master_path(&dir);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            let Some(c) = atlas::contents::load(&folder, &master) else {
                println!("No index yet. `atlas index rebuild` writes one from {folder}.");
                return;
            };
            println!("{}", atlas::nudge::what_i_know_of(&c));
            println!("Index: {}", master.display());
            let d = atlas::contents::drift(&c, &atlas::contents::names_on_disk(&dir));
            println!("{}", d.plain());
            // Named, not just counted. A count tells you something is wrong;
            // the names tell you whether it matters.
            for n in d.unlisted.iter().take(10) {
                println!("  not in the index: {n}");
            }
            for n in d.missing.iter().take(10) {
                println!("  promised, not there: {n}");
            }
            let blank = c.useless_lines().len();
            if blank > 0 {
                println!("{blank} line(s) say nothing the filename didn't.");
            }
            if c.needs_splitting() {
                println!(
                    "Past {} lines — worth splitting into folders.",
                    atlas::contents::MAX_LINES
                );
            }
            if !d.is_clean() {
                println!("\natlas index rebuild");
            }
        }
        Some("rebuild") => {
            if !dir.is_dir() {
                println!("There's no notes folder at {folder} yet, so there's nothing to index.");
                return;
            }
            let before = atlas::contents::load(&folder, &master)
                .map(|c| atlas::contents::drift(&c, &atlas::contents::names_on_disk(&dir)));
            match atlas::contents::rebuild(&dir) {
                Ok(c) => {
                    match before {
                        Some(d) if !d.is_clean() => println!(
                            "Rebuilt: {} added, {} dropped.",
                            d.unlisted.len(),
                            d.missing.len()
                        ),
                        Some(_) => println!("Rebuilt. It already matched."),
                        None => println!("Wrote the first index."),
                    }
                    println!("{}", atlas::nudge::what_i_know_of(&c));
                    println!("{}", master.display());
                }
                Err(e) => println!("Couldn't write the index: {e}"),
            }
        }
        Some("show") => match atlas::contents::load(&folder, &master) {
            Some(c) => print!("{}", c.as_markdown()),
            None => println!("No index yet. `atlas index rebuild` writes one."),
        },
        Some(other) => {
            println!("Don't know `atlas index {other}`. Try status, show or rebuild.");
        }
    }
}

fn run_backups(args: &[String]) {
    let store = atlas::roots::store();
    let cfg = Config::load(&atlas::roots::config_dir()).ok();
    let backup_cfg =
        cfg.as_ref().and_then(|c| c.tools.as_ref()).map(|t| t.backup.clone()).unwrap_or_default();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            // `backups`, not `list_backups`: an unreadable backup folder must
            // not read as "you have none". `list_backups` returns an empty
            // Vec whether the folder is empty or unreadable, so a permissions
            // error here printed "No backups yet." -- the answer most likely
            // to make someone stop worrying at exactly the wrong moment, which
            // is the distinction `backups` was written to keep.
            let backups = match atlas::safety::backups(&backup_cfg) {
                Ok(b) => b,
                Err(why) => {
                    println!("{why}");
                    return;
                }
            };
            if backups.is_empty() {
                println!("No backups yet.");
                return;
            }
            for (i, b) in backups.iter().enumerate() {
                let files = b.files.map(|f| f.to_string()).unwrap_or_else(|| "?".into());
                println!("  {i}: {} — {files} files, {} bytes", b.at, b.bytes);
            }
            println!("\natlas backups restore <n>");
        }
        Some("restore") => {
            let Some(n) = args.get(1).and_then(|s| s.parse::<usize>().ok()) else {
                println!("atlas backups restore <n> -- see `atlas backups list` for the number");
                return;
            };
            let backups = atlas::safety::list_backups(&backup_cfg);
            let Some(b) = backups.get(n) else {
                println!("There's no backup numbered {n}.");
                return;
            };
            let mine = atlas::household::Household::load(&store);
            let trash_cfg = cfg
                .as_ref()
                .and_then(|c| c.tools.as_ref())
                .map(|t| t.trash.clone())
                .unwrap_or_default();
            let trash =
                atlas::safety::Trash::new(trash_cfg.resolved(&atlas::roots::install_root()));
            match atlas::safety::restore(&b.path, store.root(), &trash, &mine) {
                Ok(count) => println!("Restored {count} file(s)."),
                Err(e) => println!("Couldn't restore that: {e}"),
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try list or restore."),
    }
}

/// `atlas mail` — what Atlas can see of your mailboxes, and how to add one.
///
/// Written because `config/tools.yaml` told people to run `atlas mail setup`
/// for the Azure app registration and there was no `mail` command at all. The
/// IMAP and SMTP sides have been built for days; the way in had never been.
fn run_mail(args: &[String]) {
    let cfg = Config::load(&atlas::roots::config_dir())
        .ok()
        .and_then(|c| c.tools)
        .map(|t| t.mail)
        .unwrap_or_default();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            if !cfg.enabled {
                println!("Mail is switched off. Turn it on with `mail.enabled: true` in config/tools.yaml.");
            }
            if cfg.accounts.is_empty() {
                println!("No mailboxes configured yet. `atlas mail setup` walks through adding one.");
                return;
            }
            println!("{} mailbox{}:", cfg.accounts.len(), if cfg.accounts.len() == 1 { "" } else { "es" });
            for a in &cfg.accounts {
                let provider = atlas::mail::Provider::from_address(&a.address);
                let how = match atlas::mail::credential_source(a) {
                    Ok(name) => format!("credential in the vault as \"{name}\""),
                    Err(why) => format!("cannot connect: {why}"),
                };
                println!("  {} ({})  {}, {how}", a.name, a.address, provider.plain());
            }
            println!();
            println!("Nothing here has ever connected to a real server from this machine.");
            println!("The first real run has to happen where it can reach the provider.");
        }
        Some("setup") => {
            println!("Adding a mailbox");
            println!("=================");
            println!();
            println!("Both kinds go in `mail.accounts` in config/tools.yaml, one entry each.");
            println!();
            println!("{}", atlas::mail::Provider::Gmail.how_to_connect());
            println!("Yahoo, Fastmail, or any ordinary IMAP server work the same way:");
            println!("  1. Make an app password with the provider (not your normal password).");
            println!("  2. Put it in the vault:  atlas vault");
            println!("  3. Add the account, naming that vault entry in `password_from_vault`.");
            println!();
            println!("{}", atlas::mail::Provider::Outlook.how_to_connect());
            println!("  1. Go to the Azure portal -> App registrations -> New registration.");
            println!("  2. Any name. Account types: personal + work/school.");
            println!("  3. Add a platform: Mobile and desktop, and tick the device-flow box.");
            println!("  4. Under API permissions add IMAP.AccessAsUser.All and SMTP.Send.");
            println!("  5. Copy the Application (client) ID into `client_id`, set `oauth: true`.");
            println!();
            println!("Then `atlas mail` shows what it can see, and the first connection");
            println!("happens on a machine that can actually reach the provider.");
        }
        Some(other) => {
            println!("I don't know `atlas mail {other}`.");
            println!("  atlas mail        — what mailboxes are configured");
            println!("  atlas mail setup  — how to add one");
        }
    }
}

fn run_watching(args: &[String]) {
    let store = atlas::roots::store();
    let mut w = atlas::watching::Watcher::load(&store);
    let cfg = Config::load(&atlas::roots::config_dir())
        .ok()
        .and_then(|c| c.tools.map(|t| t.watching))
        .unwrap_or_default();
    let now = atlas::store::now();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            let running = w.running();
            if running.is_empty() {
                println!("Nothing being watched.");
            }
            for j in &running {
                println!("  #{} {} — {} in", j.id, j.name, j.ran_for(now) / 60);
            }
        }
        Some("start") => {
            let name = args[1..].join(" ");
            if name.trim().is_empty() {
                println!("atlas watching start <name>");
                return;
            }
            let id = w.watch(name.trim(), name.trim(), now);
            match w.save(&store) {
                Ok(()) => println!("Watching #{id}: {}", name.trim()),
                Err(e) => println!("Couldn't save that: {e}"),
            }
        }
        Some("done") | Some("failed") => {
            let failed = args.first().map(|s| s.to_lowercase()).as_deref() == Some("failed");
            let Some(id) = args.get(1).and_then(|s| s.parse::<u64>().ok()) else {
                println!("atlas watching done <id> [last line of output]");
                return;
            };
            let last_line = args[2..].join(" ");
            let outcome =
                if failed { atlas::watching::Outcome::Failed } else { atlas::watching::Outcome::Finished };
            w.update(id, outcome, &last_line, now);
            if let Err(e) = w.save(&store) {
                println!("Couldn't save that: {e}");
                return;
            }
            for line in w.to_report(&cfg, now) {
                println!("{line}");
            }
            keep(w.save(&store), "what I am watching");
        }
        Some("report") => {
            let lines = w.to_report(&cfg, now);
            if lines.is_empty() {
                println!("Nothing new to report.");
            }
            for line in lines {
                println!("{line}");
            }
            if let Err(e) = w.save(&store) {
                println!("Couldn't save that: {e}");
            }
        }
        Some("prune") => {
            let n = w.prune(&cfg, now);
            match w.save(&store) {
                Ok(()) => println!("Stopped watching {n} job(s) that had run too long."),
                Err(e) => println!("Couldn't save that: {e}"),
            }
        }
        Some(other) => {
            println!("I don't know \"{other}\" — try list, start, done, failed, report or prune.")
        }
    }
}

/// What Atlas turned down, and what the shape of it says.
///
/// The half of the record that nothing kept until now. With only the trades it
/// found, "Atlas hasn't traded this week" has two readings — careful, or
/// broken — and no way to tell them apart.
fn run_refusals(args: &[String]) {
    let store = atlas::roots::store();
    let turned_down = atlas::refusals::Refusals::load(&store);
    let pairs: Vec<String> = match args.first() {
        Some(p) => vec![p.to_uppercase()],
        None => {
            let mut all: Vec<String> = turned_down.by_pair.iter().map(|(p, _)| p.clone()).collect();
            for (p, _) in &turned_down.proposed {
                if !all.contains(p) {
                    all.push(p.clone());
                }
            }
            all.sort();
            all
        }
    };
    if pairs.is_empty() {
        println!("I haven't been asked about a market yet, so there's nothing to show.");
        println!("Every `atlas market ...` adds to this, whether it finds a trade or not.");
        return;
    }
    for pair in &pairs {
        println!("{}", turned_down.spoken(pair));
        if let Some((_, causes)) = turned_down.by_pair.iter().find(|(p, _)| p == pair) {
            for c in causes {
                println!("  {:>5}  {}", c.times, c.label);
                for e in &c.examples {
                    println!("         e.g. {e}");
                }
            }
        }
        println!();
    }
}

fn run_reclaim(cfg: &Config, go: bool) {
    let now = atlas::store::now();
    // The places Atlas may look, passed in rather than discovered, so how far
    // it can reach is a decision written down here rather than a side effect.
    let roots = atlas::reclaim::roots_from_env();
    if roots.is_empty() {
        eprintln!("I couldn't work out where your home folder is, so I haven't looked anywhere.");
        return;
    }

    println!("Looking at the whole disk. Nothing is moved unless you ask.");
    println!();

    // Two lists, because they are two different decisions.
    let mut found = atlas::reclaim::survey(&roots, now);
    let looked = atlas::reclaim::whole_disk(&roots, now);
    let apps = atlas::reclaim::installed_apps(&roots, now);

    if found.is_empty() && looked.is_empty() && apps.is_empty() {
        println!("{}", atlas::reclaim::spoken(&found));
        // Still say what I'm costing you. "Nothing to reclaim" from a program
        // sitting on 900 MB of its own is the answer that makes you stop
        // trusting the other one.
        report_own_footprint(cfg);
        return;
    }
    if !found.is_empty() {
        println!("I can clear these myself -- they rebuild:");
        for c in found.iter().take(15) {
            println!("  {}", c.line());
        }
    }
    if !looked.is_empty() || !apps.is_empty() {
        println!();
        println!("Yours to judge. I won't touch any of these:");
        for c in looked.iter().chain(apps.iter()).take(25) {
            println!("  {}", c.line());
        }
    }
    let all: Vec<atlas::reclaim::Candidate> =
        found.iter().chain(looked.iter()).chain(apps.iter()).cloned().collect();
    println!();
    println!("{}", atlas::reclaim::report(&all));
    // Atlas's own folder, counted in the same breath as everybody else's.
    report_own_footprint(cfg);

    if !go {
        // The list on its own is the useful part. Acting is a separate
        // decision, made by typing something different.
        println!("
Nothing has been moved. `atlas reclaim --do-it` moves these to Atlas's");
        println!("trash, where they stay for 30 days and can be put back.");
        return;
    }

    let tc = cfg.tools.as_ref();
    let trash = atlas::safety::Trash::new(
        tc.map(|t| t.trash.clone()).unwrap_or_default().resolved(&atlas::roots::install_root()),
    );
    // Only ever the list Atlas may move. The reported half never reaches
    // here, and `reclaim` re-checks every entry regardless.
    found.retain(|c| c.kind.atlas_may_move());
    let (moved, refused) = atlas::reclaim::reclaim(&found, &trash);
    println!("
Moved {} to the trash. They can be put back for 30 days.", moved.len());
    // Never reported as a whole success: a partial reclaim announced as a
    // complete one sends you looking for space that was never freed.
    for (path, why) in &refused {
        println!("  couldn't move {}: {why}", path.display());
    }

    // And what is already stranded in there.
    //
    // Until 18 Sep, `Trash::expire` deleted a folder's ledger entry and then
    // failed to delete the folder — `remove_file` does not remove a
    // directory, and the error was discarded. Every folder ever reclaimed on
    // that code is still in `data/trash` with nothing pointing at it: `undo`
    // cannot find it, `expire` will never look at it again, and the space
    // `reclaim` reported freeing was never freed.
    //
    // Named and measured, never deleted. This is a holding pen for things
    // somebody decided to get rid of, and removing whatever Atlas did not
    // recognise in there is the wrong instinct in the one folder where being
    // wrong cannot be undone.
    let stray = trash.unaccounted();
    if !stray.is_empty() {
        let total: u64 = stray.iter().map(|(_, b)| *b).sum();
        println!(
            "\nThere's {} MB in my trash I have no record of — most likely folders \
             reclaimed before 18 Sep, whose records expired while the folders stayed. \
             Nothing can put these back, so they're yours to delete:",
            total / (1024 * 1024)
        );
        for (path, bytes) in stray.iter().take(10) {
            println!("  {} ({} MB)", path.display(), bytes / (1024 * 1024));
        }
        if stray.len() > 10 {
            println!("  ...and {} more", stray.len() - 10);
        }
    }
}

/// What Atlas's own data folder is costing you, and whether that is in budget.
///
/// `atlas reclaim` looked at your whole disk and named other people's caches
/// while saying nothing at all about Atlas's own folder. A tool that tells you
/// to clear 4 GB of npm cache and does not mention its own 900 MB is the one
/// shape of disk report nobody should ship.
///
/// The numbers existed. `retention::survey` walks the data folder,
/// `retention::usage` groups it by what the file is for, and
/// `Usage::total_mb` is the answer to "how much" — and that method had no
/// production caller at all. It *looked* like it had one until 19 Sep, because
/// `install::total_mb` shared its bare name and the deadness scan reads bare
/// names; renaming that one to `download_mb` is what made this visible.
///
/// Read-only on purpose. `atlas reclaim --do-it` moves things it found on your
/// disk; Atlas's own folder is pruned by the hourly housekeeping pass against
/// the budget in `retention:`, and having two things delete from the same
/// folder on two different rules is how a backup disappears.
fn report_own_footprint(cfg: &Config) {
    let root = atlas::roots::data_dir();
    if !root.exists() {
        return;
    }
    let items = atlas::retention::survey(&root);
    if items.is_empty() {
        return;
    }
    let usage = atlas::retention::usage(&items);
    let rcfg = cfg.tools.as_ref().map(|t| t.retention.clone()).unwrap_or_default();

    println!();
    println!(
        "And me: {} MB in my own folder, against a {} MB budget.",
        usage.total_mb(),
        rcfg.total_budget_mb
    );
    // The breakdown, because "900 MB" invites deleting the folder and the
    // split says which part of it is actually growing.
    //
    // The words are `Class`'s own, not guesses at them: `Scratch` is the wav
    // of the turn you are in, `Captures` is screenshots and webcam frames.
    // Calling `Captures` "recordings" — which the first draft of this did —
    // reads as audio and points at the wrong half of the folder.
    let mb = |b: u64| b / (1024 * 1024);
    println!(
        "  recordings and working files {} MB, screenshots {} MB, logs {} MB, \
         notes {} MB, what I've learned {} MB",
        mb(usage.scratch),
        mb(usage.captures),
        mb(usage.logs),
        mb(usage.notes),
        mb(usage.state)
    );
    if usage.unknown > 0 {
        // Its own line rather than folded into a total, because this is the
        // part Atlas will never delete: a file it cannot date is a file it
        // has no basis for evicting.
        println!("  and {} MB I can't date, so I leave it alone", mb(usage.unknown));
    }
    if usage.not_ours > 0 {
        println!("  plus {} MB in my trash and backups, which I don't prune here", mb(usage.not_ours));
    }
    if atlas::retention::irreducible(&usage, &rcfg) {
        println!(
            "  Notes and what I've learned alone are over the budget — clearing recordings \
             and screenshots won't bring me back under it. Raise `retention.total_budget_mb`."
        );
    } else if usage.total_mb() > rcfg.total_budget_mb {
        println!("  Over budget. The hourly housekeeping pass brings this down on its own.");
    }
}

/// Speak the same line in each shortlisted voice, so you can choose by ear.
///
/// The whole point is that you cannot pick a voice from a description. Atlas
/// says a real sentence — one of the things it actually says — in each
/// candidate, and you keep the one you want to hear every day.
fn run_audition(cfg: &Config) {
    let Some(tc) = cfg.tools.as_ref() else {
        eprintln!("no config/tools.yaml, so there's nothing to speak with.");
        return;
    };
    let voice = Voice::new(tc);
    println!("Auditioning {} voices.\n", atlas::tts::SHORTLIST.len());
    println!("{}\n", atlas::tts::KOKORO_HAS_NO_RANGE);

    for (id, why) in atlas::tts::SHORTLIST {
        println!("  {id} — {why}");
        // Spoken through the configured engine with the settings you already
        // have, so what you hear is what you would get.
        let line = format!(
            "This is {}. Your nine o'clock post is over length by twelve characters.",
            id.rsplit('_').next().unwrap_or(id)
        );
        match voice.say_as(id, &line) {
            Ok(()) => {}
            // Never silently skip: a voice you did not hear is one you cannot
            // choose, and a silent audition looks like a voice you disliked.
            Err(e) => eprintln!("      (couldn't speak that one: {e})"),
        }
    }
    println!("\nSet the one you want as `voice_settings.voice` in config/tools.yaml.");
}

/// Record a few samples and learn the voice in them.
///
/// Deliberately several: one recording captures one mood, one distance and one
/// microphone. `min_samples` in `tools.yaml` is what `voiceid` will trust.
fn run_enrol_voice(cfg: &Config) {
    let Some(tc) = cfg.tools.as_ref() else {
        eprintln!("no config/tools.yaml, so there's nothing to record with.");
        return;
    };
    let vars = tc.vars.clone();
    if !atlas::speaker::available(&tc.speaker, &vars) {
        // Said before anything is recorded, rather than after three samples.
        eprintln!("{}", atlas::speaker::NO_ENCODER);
        eprintln!("{}", atlas::speaker::still_learning(&atlas::speaker::background(&atlas::roots::store())));
        return;
    }
    let voice = Voice::new(tc);
    let store = atlas::roots::store();
    let mut id = atlas::voiceid::VoiceId::load(&store);
    let needed = tc.voice_id.min_samples.max(1);

    println!("Teaching Atlas your voice. {needed} short recordings.\n");
    while id.enrolled() < needed {
        println!("{}", atlas::voiceid::enrollment_prompt(id.enrolled(), needed));
        print!("[enter when ready] ");
        let _ = io::stdout().flush();
        let mut l = String::new();
        if io::stdin().read_line(&mut l).is_err() {
            return;
        }
        if voice.listen().is_err() {
            eprintln!("(didn't catch that — try again)");
            continue;
        }
        match voice.voiceprint() {
            Some(print) => match id.enroll(&print) {
                Ok(n) => println!("  sample {n} of {needed}."),
                Err(e) => eprintln!("(couldn't use that one: {e})"),
            },
            None => eprintln!("(the encoder didn't return anything usable — try again)"),
        }
    }
    match id.save(&store) {
        Ok(()) => println!("\n{}", atlas::voiceid::enrollment_prompt(id.enrolled(), needed)),
        // Never claim it learned something it did not keep.
        Err(e) => eprintln!("\nI learned it but couldn't save it ({e}) — it won't survive a restart."),
    }
}

fn run_hub(cfg: &Config) {
    use atlas::hub;
    use atlas::server::{Action, Reply, Server, ServerConfig};

    let tools = cfg.tools.clone().unwrap_or_default();
    // The one place that ignores the switch: you have typed `atlas settings`,
    // and a command that opens a page is not the same thing as a dashboard a
    // daemon leaves listening.
    let scfg = ServerConfig { enabled: true, ..ServerConfig::default() };
    // Loopback only, and a token even so — anything else on the machine can
    // reach a local port.
    // From the operating system, not the clock. If there's no entropy source
    // it stops rather than falling back — a token quietly generated from a
    // timestamp is worse than not starting, because everything downstream
    // assumes it's strong and you'd never know.
    // The install's own token, so this prints the same address the daemon
    // prints and the one you already bookmarked.
    let token = match atlas::server::token_for(&atlas::roots::store()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("couldn't make a secure token: {e}");
            leave(2);
        }
    };

    let server = match Server::bind(&scfg, &token) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("couldn't open the settings page: {e}");
            eprintln!("something else may already be using port {}.", scfg.port);
            leave(2);
        }
    };

    // WITH the token. These two lines printed addresses with no `?t=` on
    // them, and every request carries a token with no exemptions
    // (`token_matches` returns false for `None`), so the server answered
    // `Denied` to both. The token was generated three lines up and printed
    // nowhere at all.
    //
    // That made `atlas settings` unusable -- and it is menu item 3 in
    // `ATLAS.bat`, labelled "works even when Atlas won't", the recovery path
    // for when the voice or the daemon is broken. The one door that is
    // supposed to work when nothing else does answered Denied.
    println!(
        "Settings are at {}",
        atlas::server::hub_url(server.port(), &token, "/hub/settings")
    );
    println!("Access is at    {}", atlas::server::hub_url(server.port(), &token, "/hub/access"));
    println!("Leave this window open. Close it when you're done.");
    println!();
    println!("This is the fallback for when Atlas itself won't start. Normally: open");
    println!("Atlas and press Hub or Settings -- the same pages, in Atlas's own window.");

    let mut settings = atlas::settings::registry(&tools);
    let store = atlas::roots::store();
    let commands = cfg.commands.clone();
    let trash = atlas::safety::Trash::new(tools.trash.clone().resolved(&atlas::roots::install_root()));
    // The arrangement is yours and lives on disk. Read once at start rather
    // than per request, written back whenever a move actually changes it.
    let mut layout = atlas::dash::Layout::load(&store);
    // Reading is the default. Arranging is a mode you turn on, so a stray
    // drag while you are reading cannot rearrange anything.
    let mut arranging = false;

    let mut handle = |action: Action| -> Reply {
        match action {
            // Your switch, honoured.
            //
            // `hub.enabled` shipped `true` and was read by nothing, so there
            // was no way to turn the dashboard off from config -- the exact
            // sentence `config::PARSED_AND_NEVER_READ` recorded against it.
            // The pages are served here, so this is where the answer belongs.
            //
            // Plain HTML rather than a 404: you turned it off, and being told
            // so is more use than a browser error.
            Action::Hub(_) | Action::HubQ(..) if !tools.hub.enabled => Reply::html(
                "<h1>The hub is switched off</h1><p>Set <code>hub.enabled: true</code> \
                 in your tools.yaml to bring it back.</p>",
            ),
            Action::Hub(page) => match page {
                hub::Page::Dashboard => {
                    Reply::html(hub::dashboard_page(&layout, &dash_bodies(), arranging))
                }
                hub::Page::Settings => Reply::html(hub::settings_page(&settings)),
                hub::Page::Access => Reply::html(hub::access_page(&[])),
                // Both are files on disk, so they work with Atlas stopped --
                // which is when you would most want to take a permission away.
                hub::Page::AddOns => Reply::html(hub::addons_page_with(
                    &atlas::plugins::scan(
                        &atlas::plugins::plugins_dir(),
                        &commands,
                        &atlas::plugins::Approvals::load(&store),
                    ),
                    &atlas::plugins::Offers::load(&store).items,
                    &[],
                    &[],
                )),
                hub::Page::Edits => {
                    let (kept, problems) = atlas::yourchanges::all_kept(&atlas::roots::config_dir());
                    Reply::html(hub::edits_page(&kept, &problems))
                }
                hub::Page::Groups => {
                    let (views, addable) =
                        atlas::groups::views(&store, &atlas::kin::where_pairings_live());
                    Reply::html(hub::groups_page(&views, &addable))
                }
                // Never `{:?}` on an enum: that reaches the screen as a
                // variable name, which is a code leak in a product.
                other => Reply::html(hub::shell(
                    other.label(),
                    &format!(
                        "<p class=nothing>{} needs Atlas itself running. Start Atlas \
                         and open this page again — this window only knows about \
                         settings.</p>",
                        hub::esc(other.label())
                    ),
                )),
            },
            Action::DashArrange(on) => {
                arranging = on;
                Reply::redirect("/hub")
            }
            Action::DashMove(m) => {
                // Only write when something actually moved. A file rewritten
                // on every click is a file that gets corrupted on the one
                // click that happens during a power cut.
                if layout.apply(&m) {
                    if let Err(e) = layout.save(&store) {
                        eprintln!("couldn't save the dashboard layout: {e}");
                    }
                }
                Reply::redirect("/hub")
            }
            // Validated, then written, through the same path as the running
            // Atlas. Until 27 Sep 2026 this validated, said "is now on", and
            // wrote nothing -- in the window whose whole job is changing a
            // setting when Atlas won't start -- and put the sentence into the
            // page unescaped.
            Action::HubSet { key, value } => {
                let said = settings.set_and_keep(&key, &value, &atlas::roots::config_dir());
                hub::back_with(&format!("{}#set-{key}", hub::Page::Settings.href()), "", &said)
            }
            // What a button did, said on the page it came back to.
            Action::HubQ(page, q) => {
                let said = hub::form_fields(&q).into_iter().find(|(k, _)| k == "said").map(|(_, v)| v);
                let page = match page {
                    hub::Page::Settings => hub::settings_page(&settings),
                    hub::Page::Access => hub::access_page(&[]),
                    other => hub::shell(
                        other.label(),
                        &format!(
                            "<h1>{}</h1><p class=nothing>This page needs Atlas itself running. Start Atlas \
                             and open it again — this window only knows about settings.</p>",
                            hub::esc(other.label())
                        ),
                    ),
                };
                Reply::html(hub::with_said(page, said.as_deref()))
            }
            Action::HubBack(page, said) => hub::back_with(page.href(), "", &said),
            // Said rather than silently shown the settings page. This window
            // is the one you get when Atlas itself is not running, and it
            // does not hold the grants -- pressing revoke here and being
            // shown a different page would look exactly like it worked.
            Action::AddOn { what, id, key, sha } => hub::after_button(
                hub::Page::AddOns,
                atlas::plugins::hub_action(
                    &store,
                    &atlas::plugins::plugins_dir(),
                    &commands,
                    &trash,
                    &what,
                    &id,
                    &key,
                    &sha,
                ),
            ),
            Action::Friend { .. } => Reply::html(hub::shell(
                "Atlas",
                "<p class=note>Adding a friend needs Atlas itself running -- its door is what their \
                 Atlas knocks on. Start Atlas and open the Friends page.</p>",
            )),
            Action::GroupChange { what, group, who, role } => hub::after_button(
                hub::Page::Groups,
                atlas::groups::act(&store, &atlas::kin::where_pairings_live(), &what, &group, &who, &role),
            ),
            Action::ForgetEdit { file, path } => hub::after_button(
                hub::Page::Edits,
                atlas::yourchanges::forget(&atlas::roots::config_dir(), &file, &path).map(|_| String::new()),
            ),
            Action::RevokeAccess(_) | Action::RevokeAllAccess => Reply::html(hub::shell(
                "Atlas",
                "<p class=note>Taking access away needs Atlas itself running — this window \
                 only knows about settings. Start Atlas and open the Access page.</p>",
            )),
            Action::Appearance { what, to } => {
                let mut a: hub::Appearance = store.load(hub::APPEARANCE_KEY);
                if a.choose(&what, &to) {
                    if let Err(e) = store.save(hub::APPEARANCE_KEY, &a) {
                        eprintln!("couldn't keep that appearance choice: {e}");
                    }
                    Reply::redirect("/hub")
                } else if let Some(done) = atlas::appearance::choose(&what, &to) {
                    // From Settings → How it looks; a colourway chosen there
                    // clears the Aa menu's theme, which would otherwise win.
                    match done {
                        Ok(_) if what == "look.theme" && !a.theme.is_empty() => {
                            a.theme.clear();
                            if let Err(e) = store.save(hub::APPEARANCE_KEY, &a) {
                                eprintln!("couldn't keep that appearance choice: {e}");
                            }
                        }
                        Ok(_) => {}
                        Err(e) => eprintln!("couldn't keep that appearance choice: {e}"),
                    }
                    Reply::redirect(&format!("{}#how-it-looks", hub::Page::Settings.href()))
                } else {
                    Reply::redirect("/hub")
                }
            }
            _ => Reply::html(hub::settings_page(&settings)),
        }
    };

    // --- Answering other Atlases asking who is here ---
    //
    // Beside the hub rather than anywhere else, because the two are the same
    // fact: the announcement says "there is a door at this port", and this is
    // the process holding that door open. Started when the door opens and
    // gone when it closes, so the announcement cannot outlive the thing it
    // announces.
    //
    // Off unless you turned it on -- see `nearby.announce`, and the reason is
    // that the network you are on is not always your own. A failure here is
    // printed and not fatal: discovery is a convenience, and a machine that
    // cannot answer probes still serves every request it is sent.
    let ncfg = tools.nearby.clone();
    if ncfg.announce {
        let me = atlas::roots::install_root()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "atlas".into());
        let door = scfg.port;
        std::thread::spawn(move || {
            // Never stops: it lives as long as the hub does, and the hub is
            // the loop below. A stop that is always false is honest about
            // that rather than pretending to a lifecycle this has not got.
            if let Err(e) = atlas::nearby::answer_probes(&me, door, &ncfg, &|| false) {
                eprintln!("nearby: not answering probes ({e}). The hub is unaffected.");
            }
        });
    }

    // On threads of its own, as the running Atlas serves it (28 Sep 2026):
    // `serve_once` took one connection at a time, so a page, its icons and
    // its manifest queued behind each other and one silent connection held
    // every other one for its whole deadline. Every answer is still worked
    // out here, one at a time.
    let door = match server.threaded() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("hub: {e}");
            return;
        }
    };
    loop {
        door.wait_and_answer(1000, &mut handle);
    }
}


/// What's present, what isn't, and what to do about it.
///
/// Size, not checksums: pinning hashes breaks every time an upstream release
/// is rebuilt, and a truncated download is the failure that actually happens.
fn report_install(cfg: &Config) {
    use atlas::install::{
        after, before, what_to_fetch, state_of, wanted, where_it_lands, State, COSTS_NOTHING,
    };

    // `install.tools_dir` and `.models_dir` are for the machine where the
    // models live on another drive, and nothing read either -- so this
    // reported every piece missing on exactly the machine they existed for.
    // `include_optional` is why the list is `wanted` rather than `pieces`.
    let icfg = cfg.tools.as_ref().map(|t| t.install.clone()).unwrap_or_default();
    let want = wanted(&icfg);

    let found: Vec<(&'static str, Option<u64>)> = want
        .iter()
        .map(|p| {
            // Anchored: `Piece.path` is install-relative by declaration,
            // like `upgrade::YOURS`. Stat'd bare, `atlas install` reported
            // every piece missing from anywhere but the install folder.
            let bytes = std::fs::metadata(atlas::roots::under_install(where_it_lands(p, &icfg)))
                .ok()
                .map(|m| m.len());
            (p.name, bytes)
        })
        .collect();

    println!("{COSTS_NOTHING}\n");
    for p in &want {
        let bytes = found.iter().find(|(n, _)| *n == p.name).and_then(|(_, b)| *b);
        let mark = match state_of(p, bytes) {
            State::Present => "have",
            State::Missing if p.optional => "    ",
            State::Missing => "need",
            State::HalfDownloaded => "part",
        };
        println!("  [{mark}] {:<22} {}", p.name, if bytes.is_some() { "" } else { p.without_it });
    }
    // `what_to_fetch`, not `plan`: the list above and the number below it have to
    // be about the same pieces. `plan` walks every one, so with
    // `include_optional` off this printed "4801MB to fetch" beside a 341MB
    // download.
    println!("\n{}", before(&what_to_fetch(&want, &found)));
    println!(
        "{}MB altogether if none of it were here.",
        atlas::install::download_mb(icfg.include_optional)
    );
    let left_out = atlas::install::pieces().len() - want.len();
    if left_out > 0 {
        println!(
            "{left_out} optional piece{} left out -- `install.include_optional: true` adds them.",
            if left_out == 1 { "" } else { "s" }
        );
    }
    if icfg.tools_dir.trim() != "tools" || icfg.models_dir.trim() != "models" {
        println!(
            "Looking in {} and {}, which is where you said.",
            icfg.tools_dir.trim(),
            icfg.models_dir.trim()
        );
    }

    let results: Vec<(&'static str, bool)> = want
        .iter()
        .map(|p| {
            let bytes = found.iter().find(|(n, _)| *n == p.name).and_then(|(_, b)| *b);
            (p.name, state_of(p, bytes) == State::Present)
        })
        .collect();
    println!("{}", after(&results));
}

/// Regenerate `docs/METRICS.md`. Lost in two merges now; `tests/metrics.rs`
/// asserts this exists so the third time fails a build instead of quietly
/// leaving a stale file that still looks authoritative.
fn run_metrics() {
    let root = std::path::Path::new(".");
    let unwired = unwired_from_wiring_test(root);
    let m = atlas::metrics::gather(root, unwired);
    let rendered = m.render();
    let out = root.join("docs/METRICS.md");
    match std::fs::write(&out, &rendered) {
        Ok(()) => println!("{rendered}\nwritten to {}", out.display()),
        Err(e) => {
            eprintln!("could not write {}: {e}", out.display());
            println!("{rendered}");
        }
    }
}

fn unwired_from_wiring_test(root: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("tests/wiring.rs")) else {
        return Vec::new();
    };
    let Some(start) = text.find("UNWIRED_BASELINE") else { return Vec::new() };
    let rest = &text[start..];
    let Some(open) = rest.find("= &[").map(|i| i + 3) else { return Vec::new() };
    let Some(close) = rest.find("];") else { return Vec::new() };
    // Line by line, not `split(',')`.
    //
    // The comma version shipped and put its output straight into
    // `docs/METRICS.md`, where the "written but not reachable" section listed
    // things like "`tested (28`" and "`19`" and half a paragraph of a comment
    // about `afterme`. Every entry in that list is followed by a justifying
    // comment, and comments contain commas, so splitting on commas chops the
    // prose into pieces and each piece that is not empty becomes a module
    // name. The count was 18; the real one is 1.
    //
    // This is the third time this exact parser has been written and the
    // second time it has been wrong the same way -- `tests/capability_wiring.rs`
    // had it too, where a comment ending in a full stop swallowed `"mesh"`.
    // The rule that works: a baselined name is a quoted string alone on its
    // own line, and a line whose first non-space characters are `//` is prose.
    rest[open + 1..close]
        .lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.starts_with("//") {
                return None;
            }
            let name = t.strip_prefix('"')?.split('"').next()?;
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

/// `atlas adapt` — work out what this particular computer has.
///
/// The other half of the two-layer config split `adapt.rs` describes and
/// nothing implemented: `config/*.yaml` is the recipe that ships to anyone,
/// `config/machine.yaml` is what this kitchen actually has. Until this
/// existed, the second file was never written, so the first one carried a
/// hardcoded Chrome path and a Realtek microphone name and was wrong for
/// every machine but one.
fn run_adapt(cfg: &Config, plat: &dyn Platform, args: &[String]) {
    let dir = atlas::roots::config_dir();
    let dir = dir.as_path();

    if args.first().map(|s| s.as_str()) == Some("show") {
        match atlas::adapt::Machine::load(dir) {
            Some(m) => {
                println!("{}", m.summary());
                let wanted: Vec<String> = cfg.apps.apps.keys().cloned().collect();
                println!("Apps found: {:.0}% of the {} configured.", m.coverage(&wanted) * 100.0, wanted.len());
                match plat.monitors() {
                    Ok(mons) if !m.matches(&mons) => println!(
                        "The displays have changed since this was written — run `atlas adapt` again."
                    ),
                    _ => {}
                }
                for n in &m.notes {
                    println!("  - {n}");
                }
            }
            None => println!("Nothing worked out yet. Run `atlas adapt`."),
        }
        return;
    }

    if args.first().map(|s| s.as_str()) == Some("fixture") {
        match atlas::adapt::Machine::load(dir) {
            // The point of this is a dry run whose monitor geometry matches
            // the real desk, rather than the invented one in `fake_monitors`.
            Some(m) => print!("{}", atlas::adapt::monitor_fixture(&m)),
            None => println!("Nothing worked out yet. Run `atlas adapt`."),
        }
        return;
    }

    let app_names: Vec<String> = cfg.apps.apps.keys().cloned().collect();

    // How an app is found: take whatever the generic layer guessed, expand
    // its %VARS%, and see whether that is really a file here. If it is not,
    // fall back to looking for the bare executable name on PATH — which is
    // what makes a Linux or macOS machine work at all, since a Windows
    // "C:/Program Files/..." guess is never going to resolve on one.
    let find_app = |name: &str| -> Option<atlas::adapt::AppFound> {
        let spec = cfg.apps.apps.get(name)?;
        if spec.store {
            // A Store app id is not a path and cannot be checked by looking
            // for a file. Taken as given rather than reported missing.
            return Some(atlas::adapt::AppFound { launch: spec.launch.clone(), store: true });
        }
        let expanded = atlas::doctor::expand_env(&spec.launch);
        if std::path::Path::new(&expanded).is_file() {
            return Some(atlas::adapt::AppFound {
                launch: atlas::adapt::portable(&expanded),
                store: false,
            });
        }
        let bare = expanded
            .rsplit(['/', '\\'])
            .next()
            .map(|s| s.trim_end_matches(".exe").to_string())
            .unwrap_or_default();
        for candidate in [expanded.as_str(), bare.as_str(), name] {
            if candidate.is_empty() {
                continue;
            }
            if let Some(found) = atlas::tools::which(candidate) {
                return Some(atlas::adapt::AppFound {
                    launch: atlas::adapt::portable(&found),
                    store: false,
                });
            }
        }
        None
    };

    // The external tools, taken from the config rather than a list typed in
    // here — a hardcoded list is one more thing to forget to update.
    let mut tool_names: Vec<String> = vec!["ffmpeg".into(), "ffplay".into()];
    if let Some(t) = cfg.tools.as_ref() {
        for v in t.vars.values() {
            let looks_like_a_program = v.ends_with(".exe")
                || v.ends_with(".cmd")
                || v.ends_with(".bat")
                || (!v.contains('/') && !v.contains('\\') && !v.contains(' ') && !v.contains(':'));
            if looks_like_a_program {
                tool_names.push(v.clone());
            }
        }
    }
    tool_names.sort();
    tool_names.dedup();

    let find_tool = |t: &str| -> Option<String> {
        let expanded = atlas::doctor::expand_env(t);
        if std::path::Path::new(&expanded).is_file() {
            return Some(atlas::adapt::portable(&expanded));
        }
        atlas::tools::which(&expanded).map(|p| atlas::adapt::portable(&p))
    };

    // Microphones and speakers, asked of the machine rather than guessed.
    // An ffmpeg that isn't installed yields empty lists, and empty is the
    // honest answer — `first_run_message` says "no microphone, so we're
    // typing for now" rather than pretending.
    let inputs: Vec<String> = atlas::audio::probe("ffmpeg", true)
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.name)
        .collect();
    let outputs: Vec<String> = atlas::audio::probe("ffmpeg", false)
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.name)
        .collect();

    let m = atlas::adapt::detect(
        plat,
        &app_names,
        &find_app,
        inputs,
        outputs,
        &find_tool,
        &tool_names,
        atlas::store::now(),
    );

    println!("{}", atlas::adapt::first_run_message(&m, &app_names));
    match m.save(dir) {
        Ok(()) => println!("Written to config/machine.yaml. That file is yours — don't share it."),
        Err(e) => println!("Couldn't write config/machine.yaml: {e}"),
    }
}


/// `atlas update` — say what replacing the binary would do, before it does.
///
/// The whole command is a report. It moves nothing, downloads nothing and
/// asks for nothing, because the thing that was missing was never the
/// mechanics of copying a file — it was any way to know, in advance and
/// specifically, that your notes and your machine settings are not part of
/// what gets replaced.
/// `atlas update install`: your yes to the release that's here. Checked
/// again and staged; the running Atlas restarts itself into it, or the next
/// start installs it.
fn update_install(root: &std::path::Path) {
    let store = atlas::roots::store();
    let a = atlas::update_courier::Available::load(&store);
    let Some(platform) = atlas::release::this_platform() else {
        println!("Atlas doesn't ship updates for this kind of device.");
        return;
    };
    if a.notice.is_none() {
        println!("There's no update waiting. `atlas update` says what's been heard.");
        return;
    }
    atlas::update_apply::say_yes(&store, &a.version);
    match atlas::update_apply::stage_update(&store, root, platform) {
        Ok(v) => println!(
            "Atlas {v} is checked against your release key and ready. The background Atlas restarts itself into it \
             within a minute; if Atlas isn't running, the next start installs it. It has to pass its health check \
             first, and the version you're on now is kept."
        ),
        Err(why) => println!("{why}"),
    }
}

/// `atlas update undo`: back one version, with your yes typed here.
fn update_undo(root: &std::path::Path) {
    let Some((previous, _)) = atlas::update_apply::previous_build(root) else {
        println!("There's no previous version kept here to go back to.");
        return;
    };
    print!("Go back from Atlas {} to {previous}? {} won't be offered again. Type yes to go back: ", atlas::upgrade::version(), atlas::upgrade::version());
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() || !answer.trim().eq_ignore_ascii_case("yes") {
        println!("Left as it is.");
        return;
    }
    let Ok(running) = std::env::current_exe() else {
        println!("I couldn't tell where this program is, so nothing changed.");
        return;
    };
    let yes = atlas::release::LocalApproval::given_by_the_person_at_this_device();
    match atlas::update_apply::undo_update(&atlas::roots::store(), root, &running, yes) {
        Ok(v) => println!("Back on Atlas {v}. The background Atlas restarts itself onto it within a minute."),
        Err(why) => println!("{why}"),
    }
}

/// Who sends this Atlas its updates: (their key, your name for them), and
/// whether that's you. From your release channel's signed list.
fn update_sender() -> Option<(String, Option<String>, bool)> {
    atlas::feedback::release_sender(&atlas::roots::store(), &pairings_dir())
}

/// `atlas feedback`: tell whoever sends you Atlas that something's wrong --
/// when you decide it is -- and hear back what they did about it. On the
/// releaser's side, the feedback your friends sent, and your answers.
///
///   atlas feedback                      what you've sent and what's come in
///   atlas feedback send                 write it, see it, send it
///   atlas feedback reply <n> seen|fixing|fixed <version>|wont [note...]
fn run_feedback(args: &[String]) {
    use atlas::feedback::{self, FeedbackStatus};
    let store = atlas::roots::store();
    let now = atlas::store::now();
    match args.first().map(|s| s.as_str()) {
        Some("send") => {
            let Some((_, name, mine)) = update_sender() else {
                println!("This Atlas isn't in anyone's release channel, so there's no one to send feedback to.");
                return;
            };
            let words = ask_line("What's wrong? Say it in your own words: ");
            let attach = match atlas::update_apply::last_failure(&store) {
                Some(f) => {
                    println!("\nAn update failed here earlier. This is what was written down about it (your name and home folder are already taken out):\n");
                    println!("{}", feedback::feedback_preview(&feedback::Feedback { attached: Some(f.clone()), ..Default::default() }).lines().skip(2).collect::<Vec<_>>().join("\n"));
                    ask_line("\nAttach it? Type yes to attach: ").eq_ignore_ascii_case("yes").then_some(f)
                }
                None => None,
            };
            let f = match feedback::compose_feedback(&words, attach, now) {
                Ok(f) => f,
                Err(why) => {
                    println!("{why}");
                    return;
                }
            };
            println!("\nThis is exactly what will be sent:\n\n{}", feedback::feedback_preview(&f));
            if mine {
                // Your own Atlas: straight into your own list.
                let _ = feedback::heard_feedback(&store, "you", &serde_json::to_string(&f).unwrap_or_default());
                println!("This is your own Atlas, so it's in your own feedback list (`atlas feedback`).");
                return;
            }
            let Some(to) = name else {
                println!("I can't reach whoever sends you updates -- you aren't paired with them -- so nothing was sent.");
                return;
            };
            if !ask_line(&format!("Send this to {to}? Type yes to send: ")).eq_ignore_ascii_case("yes") {
                println!("Not sent.");
                return;
            }
            feedback::queue_feedback(&store, f, &to);
            println!("Sending it to {to}. The running Atlas delivers it when it can reach them, and tells you when they answer.");
        }
        Some("reply") => {
            let n = args.get(1).and_then(|n| n.parse::<usize>().ok());
            let word = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let version = args.get(3).map(|s| s.as_str());
            let (Some(n), Some(status)) = (n, FeedbackStatus::from_words(word, version)) else {
                println!("atlas feedback reply <n> seen | fixing | fixed <version> | wont [a note for them]");
                return;
            };
            let skip = if matches!(status, FeedbackStatus::Fixed(_)) { 4 } else { 3 };
            let note = args.iter().skip(skip).cloned().collect::<Vec<_>>().join(" ");
            match feedback::answer_feedback(&store, n, status.clone(), &note, now) {
                Ok(to) => println!("Marked {}; {to} will hear it the next time Atlas reaches them.", status.plain()),
                Err(why) => println!("{why}"),
            }
        }
        _ => {
            let sent = feedback::feedback_sent(&store);
            let got = feedback::feedback_inbox(&store);
            if sent.is_empty() && got.is_empty() {
                println!("No feedback sent or received yet. `atlas feedback send` tells whoever sends you Atlas that something's wrong.");
                return;
            }
            if !got.is_empty() {
                println!("Feedback from your friends:");
                for (i, f) in got.iter().enumerate() {
                    println!(
                        "  {}. {} (Atlas {}, {}): \"{}\"{} -- {}",
                        i + 1,
                        f.from,
                        f.version,
                        f.platform,
                        f.words,
                        if f.attached.is_some() { " [update failure attached]" } else { "" },
                        f.status.plain()
                    );
                }
                println!("  `atlas feedback reply <n> seen|fixing|fixed <version>|wont [note]` answers one; `atlas update failures brief <version>` makes a fix brief.\n");
            }
            if !sent.is_empty() {
                println!("What you've sent:");
                for f in &sent {
                    println!("  to {}: \"{}\" -- {}", f.to, f.words, f.status.plain());
                    for (_, note) in &f.replies {
                        println!("      they said: {note}");
                    }
                }
            }
        }
    }
}

/// `atlas update failures [brief <version>]`: every report of an update
/// that failed -- on your own devices and friends' -- grouped by version, and
/// a written brief for fixing one.
fn update_failures(flag: Option<&str>, version: Option<&str>) {
    let store = atlas::roots::store();
    let reports = atlas::update_apply::failure_reports(&store);
    if flag == Some("brief") {
        let Some(v) = version else {
            println!("atlas update failures brief <version>");
            return;
        };
        match atlas::update_apply::fix_brief(&reports, v) {
            Some(brief) => {
                let out = format!("atlas-{v}-fix-brief.md");
                match std::fs::write(&out, &brief) {
                    Ok(()) => println!("Wrote {out}: what failed, where, and how to close it. Hand it to a coding session, or say \"work on yourself\" to Atlas on the machine that holds the source."),
                    Err(e) => println!("couldn't write {out}: {e}\n\n{brief}"),
                }
            }
            None => println!("No reports about Atlas {v}."),
        }
        return;
    }
    if reports.is_empty() {
        println!("No update has failed anywhere that's told me.");
        return;
    }
    let mut versions: Vec<&str> = reports.iter().map(|r| r.version.as_str()).collect();
    versions.dedup();
    for v in versions {
        let these: Vec<_> = reports.iter().filter(|r| r.version == v).collect();
        println!("Atlas {v}: {} report(s)", these.len());
        for r in these {
            let why: Vec<&str> = r.reasons.iter().map(|x| x.as_str()).filter(|x| !x.starts_with("(passed)")).collect();
            println!(
                "  {} ({}), at its {}: {}{}",
                if r.from.is_empty() { "?" } else { &r.from },
                r.platform,
                r.stage,
                why.join("; "),
                match r.because {
                    atlas::update_apply::FailedBecause::ThisMachine => "  [that machine, not the build]",
                    atlas::update_apply::FailedBecause::TheBuild => "",
                }
            );
        }
    }
    println!("\n`atlas update failures brief <version>` writes a brief for fixing one. A failing build stays blocked and isn't handed out; the fixed release is offered as usual.");
}

/// `atlas update auto on|ask|off|default`.
fn update_auto(to: Option<&str>) {
    use atlas::update_apply::AutoUpdate;
    let (mode, said) = match to {
        Some("on" | "automatic") => (Some(AutoUpdate::Automatic), "Updates install by themselves at a quiet moment."),
        Some("ask") => (Some(AutoUpdate::Ask), "I'll ask before installing each update."),
        Some("off") => (Some(AutoUpdate::Off), "Updates are off. `atlas update` still says what's out."),
        Some("default") => (None, "Back to the default: automatic on your own devices, ask on friends'."),
        _ => {
            println!("atlas update auto on | ask | off | default");
            return;
        }
    };
    match atlas::update_apply::choose_mode(&atlas::roots::store(), mode) {
        Ok(()) => println!("{said}"),
        Err(e) => println!("I couldn't keep that: {e}"),
    }
}

fn run_update(args: &[String]) {
    // The install root, not the working directory.
    //
    // This was `Path::new(".")`. Run `atlas update` from a Start Menu
    // shortcut, from Task Scheduler (working directory `system32`), or from
    // any terminal that is not sitting in the install folder, and every entry
    // in `upgrade::YOURS` missed — so each was labelled
    // `Fate::Regenerated` ("Not there, and will be made from scratch on first
    // run. **Not a problem**"), `Report::safe()` returned true, and the output
    // read *"Safe to update. 0 things of yours stay exactly where they are."*
    //
    // This module exists for one job: answering "what survives" with
    // evidence. It could not tell "not there" apart from "I looked in the
    // wrong place", and the reassuring answer was the wrong one. `roots.rs`
    // names this exact scenario — `atlas update` run from anywhere — as the
    // bug it was written to kill, and `tests/one_install_root.rs` did not
    // catch it because `"."` is not a `data/…` literal.
    let root = atlas::roots::install_root();
    // Step 2: installing a release that's arrived, going back, and the
    // automatic setting (`update_apply`).
    match args.first().map(|s| s.as_str()) {
        Some("install") => return update_install(&root),
        Some("undo") => return update_undo(&root),
        Some("auto") => return update_auto(args.get(1).map(|s| s.as_str())),
        Some("failures") => return update_failures(args.get(1).map(|s| s.as_str()), args.get(2).map(|s| s.as_str())),
        _ => {}
    }
    let report = atlas::upgrade::check(&root);

    println!(
        "atlas {} (data format {})",
        atlas::upgrade::version(),
        atlas::upgrade::DATA_FORMAT
    );
    println!();
    println!("{}", report.spoken());
    println!("\n{}", atlas::update_courier::status(&atlas::roots::store(), atlas::store::now()));
    let avail = atlas::update_courier::Available::load(&atlas::roots::store());
    if avail.notice.is_some() {
        println!(
            "  For this device: {} ({} MB, fingerprint {}). Installing it from here is the next step \
             of the courier; until then it waits.",
            avail.file,
            avail.size / (1024 * 1024),
            avail.sha256.get(..16).unwrap_or("")
        );
    }
    // The trial of the last update, and what happened to updates here (O1).
    if let Some(t) = atlas::upgrade::current_trial(&root) {
        println!(
            "\n{} is on trial after replacing {}: {} of {} starts so far without getting through. \
             If it reaches {}, {} comes back by itself.",
            t.new, t.previous, t.starts, atlas::upgrade::TRIAL_STARTS, atlas::upgrade::TRIAL_STARTS, t.previous
        );
    }
    let history = atlas::upgrade::update_history(&root);
    if !history.is_empty() {
        println!("\nWhat happened to updates here (latest last):");
        for line in history.iter().rev().take(5).rev() {
            let (at, what) = line.split_once(' ').unwrap_or(("", line));
            let when = at.parse::<u64>().map(|t| atlas::freshness::ago(atlas::store::now().saturating_sub(t))).unwrap_or_default();
            println!("  {when}  {what}");
        }
    }
    {
        let store = atlas::roots::store();
        let mode = match atlas::update_apply::chosen_mode(&store) {
            Some(atlas::update_apply::AutoUpdate::Automatic) => "automatic (your choice)",
            Some(atlas::update_apply::AutoUpdate::Ask) => "ask first (your choice)",
            Some(atlas::update_apply::AutoUpdate::Off) => "off (your choice)",
            None => "the default: automatic on your own devices, ask on friends'",
        };
        println!("\nInstalling updates: {mode}. `atlas update auto on|ask|off|default` changes it.");
        if let Some(p) = atlas::update_apply::pending(&store) {
            println!("Atlas {} is checked and waiting to go in at the next start.", p.version);
        }
        if let Some((v, _)) = atlas::update_apply::previous_build(&root) {
            println!("Atlas {v} is kept; `atlas update undo` goes back to it.");
        }
    }
    let kept = atlas::yourchanges::kept_count(&atlas::roots::config_dir());
    println!(
        "\nYour own edits to the shipped config files: {kept} kept in config/{}, laid back over \
         whatever the next version ships. Hand edits made since the last start are moved there \
         the next time Atlas starts.",
        atlas::yourchanges::LOCAL_DIR
    );
    println!();

    if report.safe() {
        println!("To update: stop Atlas, replace the binary, start it again.");
        println!(
            "Keep the old one as {} first and you can go back without downloading anything.",
            atlas::upgrade::keep_old_at(&root, &atlas::upgrade::this_tag()).display()
        );
        println!("Nothing above marked \"kept\" is touched by any of that.");
        if args.first().map(|s| s.as_str()) == Some("--why") {
            println!();
            println!(
                "The reason this is a report rather than a downloader: Atlas is one \n\
                 binary plus two config layers. The generic layer ships; the machine \n\
                 layer and everything under data/ are written here and are not part of \n\
                 any download. `atlas adapt` rebuilds the machine layer if you ever do \n\
                 lose it, and it takes a few seconds."
            );
        }
    } else {
        println!("Move or copy the things marked AT RISK before updating.");
    }
}

// ===========================================================================
// Away from the laptop — `atlas carry`, `atlas remote`, `atlas mobile`,
// `atlas sync-setup <provider>`.
//
// Four modules (`workingset`, `remote`, `companion`/`ios`/`android`,
// `cloudsync`) sat in `UNWIRED_BASELINE` for the same reason: each is the
// *thinking* half of something whose other half is a phone app nobody has
// built. That is a real reason for the phone side to be missing and no reason
// at all for the laptop side to be unreachable. Packing a working set against
// real files on this disk, queueing a request and probing a real paired
// address to see whether the machine that must run it is up, and answering
// "what will this actually do on my phone" are all things the laptop can do
// today, alone.
//
// What is deliberately NOT faked here: nothing invents a phone. `carry back`
// takes the files you actually brought home and compares them against what
// was actually packed; `remote ask` reports reachability from a real TCP
// probe of a real paired contact, or says plainly that there is no contact to
// probe. Where the answer needs a device that isn't here, the command says so
// instead of printing a plausible sentence.
// ===========================================================================

/// Worth shrinking for the trip rather than leaving behind.
///
/// Extension-based on purpose: the alternative is opening every file to see
/// what it is, which costs more than the decision is worth and is wrong on
/// exactly the files (a `.dat` that is really a JPEG) nobody carries anyway.
fn carry_shrinkable(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some(
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "heic" | "mp4"
                | "mov" | "mkv" | "avi" | "webm" | "wav" | "flac" | "aiff"
        )
    )
}

/// A real file on this disk, measured rather than described.
fn carry_file(
    path: &std::path::Path,
    because: atlas::workingset::Why,
) -> Option<atlas::workingset::Carried> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    Some(atlas::workingset::Carried {
        path: path.display().to_string(),
        name: path.file_name()?.to_string_lossy().into_owned(),
        bytes: meta.len(),
        because,
        shrinkable: carry_shrinkable(path),
    })
}

/// Seconds since the epoch that a path was last written, or 0 if unknown.
fn mtime_secs(path: &std::path::Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `atlas carry` — what travels with the work, decided against the budget.
///
/// The packing rule is `workingset::pack`'s, unchanged: most valuable first,
/// shrink before dropping. What this adds is the part that was missing — real
/// paths, real byte counts, and a record on disk of what went, so that
/// `missing` and `back` have something true to answer from.
/// `atlas walkthrough` — Atlas finds the page, you make the change.
///
/// Wired 14 Sep 2026. This module sat built and tested and unreachable since
/// it landed, and it is the one in the `confirmed`/`consent`/`delegate`/
/// `afterme` family that needs no ruling from Eric, because **it is the half
/// that does not act** — until 24 Sep 2026, when Eric ruled that Atlas may make
/// the change itself after the read-back and his yes; `atlas_clicks` is now a
/// real setting, off until he turns it on. Before that it was `#[serde(skip)]` over a
/// function that returns false, so no config file can turn it into something
/// that touches his accounts; it opens a page and tells him where the switch
/// is. He proposed this shape himself, and it is also the answer to "one
/// instruction at a time, not multi-step reports" -- a walk is a queue of
/// single instructions that survives being put down halfway.
fn run_walkthrough(cfg: &Config, args: &[String]) {
    use atlas::confirmed::{self, Asked, Change, Run, Step};
    use atlas::walkthrough::{self, Walk};

    let store = atlas::roots::store();
    let wcfg = cfg.tools.as_ref().map(|t| t.walkthrough.clone()).unwrap_or_default();
    // `confirmed.rs` was a complete, tested module nothing could reach. It
    // was written for Atlas making a security change itself, which nothing
    // here does -- but the read-back is the half that does the work, and it
    // is worth exactly as much when you are the one about to click. This is
    // the one place in the tree where a security change actually gets
    // started, and until 19 Sep 2026 it started with no read-back and no yes.
    let ccfg = cfg.tools.as_ref().map(|t| t.confirmed.clone()).unwrap_or_default();
    if !wcfg.enabled {
        println!("Walkthroughs are switched off (walkthrough.enabled: false in tools.yaml).");
        return;
    }

    let mut walk: Option<Walk> = store.load("walkthrough");
    let sites: Vec<String> = args.iter().skip(1).cloned().collect();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("prepare") | Some("turn-off") | Some("turn-on") if sites.is_empty() => {
            println!("Which accounts? e.g. `atlas walkthrough prepare google apple`.");
        }
        Some(kind @ ("prepare" | "turn-off" | "turn-on")) => {
            let w = match kind {
                "prepare" => walkthrough::to_prepare(&sites),
                "turn-on" => walkthrough::to_turn_on(&sites),
                _ => walkthrough::to_turn_off(&sites),
            };
            if w.stops.is_empty() {
                println!(
                    "I don't know where the setting lives on any of those yet, so I'd be \
                     sending you to a homepage to hunt -- which is the thing this is for \
                     avoiding. Nothing started."
                );
                return;
            }
            let skipped = sites.len() - w.stops.len();
            if kind == "turn-off" {
                println!("{}", walkthrough::before_turning_off(w.stops.len()));
                println!();
                println!("{}", walkthrough::FAMILY_ACCESS);
                println!();
            }
            if skipped > 0 {
                println!("{skipped} I don't have an exact page for -- left out rather than guessed.");
            }
            // Read back first, one at a time, when you've asked for that.
            // Six accounts picked in one command is still six yeses -- a
            // single yes covering six security changes is how the wrong one
            // gets made, and you would have no way to tell which.
            let change = match kind {
                "turn-off" => Change::TurnOffTwoFactor,
                "turn-on" => Change::TurnOnTwoFactor,
                _ => Change::GenerateRecoveryCodes,
            };
            let asking: Vec<Asked> = w
                .stops
                .iter()
                .map(|stop| Asked {
                    site: stop.site.clone(),
                    account: String::new(),
                    change: change.clone(),
                })
                .filter(|a| confirmed::needs_reading_back(a, &ccfg))
                .collect();
            if !asking.is_empty() {
                let run = Run::new(asking);
                println!("{}", confirmed::before_a_run(&run.items));
                println!();
                if let Some(a) = run.current() {
                    println!("{}", confirmed::saying_it_back(a));
                }
                println!("  `atlas walkthrough yes` or `atlas walkthrough no`.");
                keep(store.save("walkthrough_pending", &Some(w)), "store");
                keep(store.save("walkthrough_confirming", &Some(run)), "store");
                return;
            }
            println!("{}", w.say());
            if wcfg.open_pages {
                if let Some(stop) = w.current() {
                    println!("  page: {}", stop.url);
                }
            }
            println!("  `atlas walkthrough next` when it's done, `skip` to come back to it.");
            keep(store.save("walkthrough", &Some(w)), "store");
        }
        Some(said @ ("yes" | "no")) => {
            let mut run: Option<Run> = store.load("walkthrough_confirming");
            let Some(r) = run.as_mut() else {
                println!("Nothing waiting on a yes or no.");
                return;
            };
            let Some(asked) = r.current().cloned() else {
                println!("Nothing waiting on a yes or no.");
                return;
            };
            match confirmed::answer(said, &asked) {
                Step::Go { .. } => {
                    // Your yes, and Atlas presses it if you've let it
                    // (Settings: "Atlas makes the change"). Exactly one
                    // labelled control, never past a password box, nothing
                    // typed. Anything else comes back to you with the page.
                    let mut worked = true;
                    if wcfg.atlas_clicks {
                        let url = atlas::walkthrough::where_2fa_lives(&asked.site).map(|(u, _)| u).unwrap_or("");
                        let bcfg = cfg.tools.as_ref().map(|t| t.browser.clone()).unwrap_or_default();
                        let vars = cfg.tools.as_ref().map(|t| t.vars.clone()).unwrap_or_default();
                        let outcome = if url.is_empty() {
                            Ok(atlas::confirmed::Pressed::NoWordsForIt)
                        } else {
                            atlas::browser::Browser::start(&bcfg, &vars).and_then(|mut b| {
                                b.open(url)?;
                                let p = b.press_the_one_at(&asked.change, Some(url));
                                b.close();
                                p
                            })
                        };
                        match outcome {
                            Ok(p) => {
                                worked = p == atlas::confirmed::Pressed::Done;
                                println!("{}", p.say(&asked, url));
                            }
                            Err(e) => {
                                worked = false;
                                println!("I couldn't open my browser to do it ({e}). It's yours from here: {url}");
                            }
                        }
                    }
                    let rec = confirmed::record(&asked, worked, said, atlas::store::now());
                    let mut trail: Vec<atlas::confirmed::Record> = store.load("security_changes");
                    trail.push(rec);
                    keep(store.save("security_changes", &trail), "store");
                    r.record(worked);
                    println!("Right. {}", confirmed::how_to_undo(&asked.change));
                }
                Step::Dropped => {
                    r.skip();
                    println!("Left alone.");
                }
                // Anything ambiguous is a no for now, not a yes.
                Step::Unclear { say } | Step::Cannot(say) => {
                    println!("{say}");
                    return;
                }
                Step::ReadBack { say } => {
                    println!("{say}");
                    return;
                }
            }
            if !r.finished() {
                if let Some(next) = r.current() {
                    println!();
                    println!("{}", confirmed::saying_it_back(next));
                    println!("  `atlas walkthrough yes` or `atlas walkthrough no`.");
                }
                keep(store.save("walkthrough_confirming", &run), "store");
                return;
            }
            println!();
            println!("{}", r.summary());
            let confirmed_sites: Vec<String> = r
                .done
                .iter()
                .filter(|(_, ok)| *ok)
                .filter_map(|(what, _)| what.split(" — ").next().map(|s| s.to_string()))
                .collect();
            keep(store.save("walkthrough_confirming", &None::<Run>), "store");
            let pending: Option<Walk> = store.load("walkthrough_pending");
            keep(store.save("walkthrough_pending", &None::<Walk>), "store");
            let Some(mut w) = pending else { return };
            // Only the ones you actually said yes to. A walk that still
            // visited the pages you declined would make the question
            // decorative.
            w.stops.retain(|stop| confirmed_sites.iter().any(|s| s == &stop.site));
            if w.stops.is_empty() {
                println!("Nothing left to walk through.");
                return;
            }
            println!();
            println!("{}", w.say());
            if wcfg.open_pages {
                if let Some(stop) = w.current() {
                    println!("  page: {}", stop.url);
                }
            }
            println!("  `atlas walkthrough next` when it's done, `skip` to come back to it.");
            keep(store.save("walkthrough", &Some(w)), "store");
        }
        Some("next") | Some("skip") => {
            let Some(w) = walk.as_mut() else {
                println!("No walkthrough going. `atlas walkthrough prepare <site>...` starts one.");
                return;
            };
            let skipping = args.first().map(|s| s.to_lowercase()).as_deref() == Some("skip");
            let more = if skipping { w.skip() } else { w.next() };
            let (done, total) = w.progress();
            if more {
                println!("{}", w.say());
                if wcfg.open_pages {
                    if let Some(stop) = w.current() {
                        println!("  page: {}", stop.url);
                    }
                }
                println!("  {done} of {total} done.");
                keep(store.save("walkthrough", &walk), "store");
            } else {
                let left = w.remaining().len();
                if left == 0 {
                    println!("{} — all {total} done.", w.what_for);
                } else {
                    println!(
                        "{} — end of the list, {left} still open. `atlas walkthrough` shows \
                         which.",
                        w.what_for
                    );
                }
                keep(store.save("walkthrough", &walk), "store");
            }
        }
        Some("stop") => {
            if walk.is_some() {
                let none: Option<Walk> = None;
                keep(store.save("walkthrough", &none), "store");
                println!("Dropped it. Nothing was changed on any account.");
            } else {
                println!("Nothing going.");
            }
        }
        None | Some("where") => match &walk {
            None => {
                println!("No walkthrough going.");
                println!("  `atlas walkthrough prepare <site>...`  — set up a way back in first");
                println!("  `atlas walkthrough turn-off <site>...` — turn two-factor off");
            }
            Some(w) => {
                let (done, total) = w.progress();
                println!("{}", w.say());
                if wcfg.open_pages {
                    if let Some(stop) = w.current() {
                        println!("  page: {}", stop.url);
                    }
                }
                println!("  {done} of {total} done.");
                for s in w.remaining().iter().skip(1).take(4) {
                    println!("  still to do: {}", s.site);
                }
            }
        },
        Some(other) => {
            println!("I don't know `{other}`. Try prepare, turn-off, next, skip, stop.");
        }
    }
}

/// `atlas type` — the place to type when voice isn't working.
///
/// Wired 14 Sep 2026. Its own doc names the gap exactly: voice fails, and
/// "opening Notepad to talk to your assistant is absurd." The module was
/// built, tested and unreachable, which meant the fallback for a broken mic
/// was itself unreachable -- the failure mode this codebase keeps finding,
/// landing on the one feature whose entire job is to work when something
/// else has stopped.
///
/// A real hotkey surface belongs to the panel process, not a CLI. What this
/// gives is the same state machine driven from the keyboard Eric already has
/// in front of him, so the capability is reachable today rather than waiting
/// on the window work.
fn run_quickinput(cfg: &Config, args: &[String]) {
    use atlas::quickinput::{Action, QuickInput, Surface};

    let qcfg = cfg.tools.as_ref().map(|t| t.quick_input.clone()).unwrap_or_default();
    if !qcfg.enabled {
        println!("The typing surface is switched off (quick_input.enabled: false in tools.yaml).");
        return;
    }
    // `surface` had no reader, so choosing the overlay silently got you the
    // console anyway. The borderless Win32 overlay isn't built (see the
    // module doc), so the honest thing is to say so rather than pretend the
    // setting did nothing — two values, two truthful outcomes.
    if qcfg.surface == Surface::Overlay {
        println!(
            "The overlay surface isn't built yet — set quick_input.surface: console in \
             tools.yaml to use the console one."
        );
        return;
    }

    let mut q = QuickInput::new(qcfg);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // A sentence on the command line goes straight through; that is the same
    // door as the panel's submit, without the typing.
    let typed: String = args.join(" ");
    if !typed.trim().is_empty() {
        q.hotkey(None, now);
        for c in typed.chars() {
            q.typed(c, now);
        }
        match q.submit() {
            Action::Submit(text) => println!("Sending: {text}"),
            Action::Hide => println!("Nothing to send."),
            other => println!("{}", other.plain()),
        }
        return;
    }

    match q.hotkey(None, now) {
        Action::Show | Action::ShowWithReason(_) => {
            println!("{}", q.placeholder());
            println!("  (type a line and press return, or Ctrl-C to close)");
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).is_ok() {
                for c in line.trim_end().chars() {
                    q.typed(c, now);
                }
                // An empty line means "go away", which is `escape`, not an
                // empty `submit`. The module draws that distinction itself
                // and it is worth keeping on this surface too.
                if line.trim().is_empty() {
                    q.escape();
                    println!("Nothing typed. Closed.");
                } else {
                    match q.submit() {
                        Action::Submit(text) => println!("Sending: {text}"),
                        _ => println!("Nothing typed. Closed."),
                    }
                }
                debug_assert!(!q.is_open(), "the box must not be left open behind a finished turn");
            }
        }
        other => println!("{}", other.plain()),
    }
}

fn run_carry(cfg: &Config, args: &[String]) {
    use atlas::workingset::{self, Carried, Changed, Why};

    let store = atlas::roots::store();
    let wcfg = cfg
        .tools
        .as_ref()
        .map(|t| t.working_set.clone())
        .unwrap_or_default();

    if !wcfg.enabled {
        println!("Carrying work is switched off (working_set.enabled: false in tools.yaml).");
        return;
    }

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            let taking: Vec<Carried> = store.load("working_set");
            let left: Vec<(String, String)> = store.load("working_set_left");
            if taking.is_empty() && left.is_empty() {
                println!("Nothing packed. `atlas carry pack <file>...` to work one out.");
                return;
            }
            let total: u64 = taking.iter().map(|c| c.bytes).sum();
            println!(
                "Carrying {} file{}, {}MB of a {}MB budget.",
                taking.len(),
                if taking.len() == 1 { "" } else { "s" },
                total / 1_048_576,
                wcfg.budget_mb
            );
            for c in &taking {
                println!(
                    "  {:>7}KB  {}  ({})",
                    c.bytes / 1024,
                    c.name,
                    match c.because {
                        Why::Needed => "needed",
                        Why::Made => "Atlas made it",
                        Why::YouHadItOpen => "you had it open",
                        Why::YouMentionedIt => "you mentioned it",
                        Why::NearSomethingNeeded => "near something needed",
                    }
                );
            }
            for (name, why) in &left {
                println!("  left behind: {name} — {why}");
            }
            println!();
            println!("atlas carry missing <name>   what to do about one that didn't come");
            println!("atlas carry back <folder>    bring changed copies home");
        }
        Some("pack") => {
            let named: Vec<std::path::PathBuf> = args[1..]
                .iter()
                .filter(|a| !a.starts_with("--"))
                .map(std::path::PathBuf::from)
                .collect();
            if named.is_empty() {
                println!("Which files? atlas carry pack notes.md draft.mp4");
                return;
            }

            let mut files: Vec<Carried> = Vec::new();
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

            for p in &named {
                match carry_file(p, Why::Needed) {
                    Some(c) => {
                        seen.insert(c.path.clone());
                        files.push(c);
                    }
                    None => println!("(skipping {} — not a readable file)", p.display()),
                }
            }
            if files.is_empty() {
                println!("None of those are files I can read. Nothing packed.");
                return;
            }

            // Everything sitting beside something needed. `pack` weights these
            // lowest, so they fill whatever space is left and are the first
            // thing dropped — which is the correct treatment for "it was in
            // the same folder".
            for p in &named {
                let Some(parent) = p.parent() else { continue };
                let Ok(entries) = std::fs::read_dir(if parent.as_os_str().is_empty() {
                    std::path::Path::new(".")
                } else {
                    parent
                }) else {
                    continue;
                };
                for e in entries.flatten() {
                    let path = e.path();
                    let key = path.display().to_string();
                    if seen.contains(&key) {
                        continue;
                    }
                    if let Some(c) = carry_file(&path, Why::NearSomethingNeeded) {
                        seen.insert(key);
                        files.push(c);
                    }
                }
            }

            let packed = workingset::pack(&files, &wcfg);
            println!("{}", workingset::spoken(&packed));
            println!();
            for c in &packed.taking {
                println!("  {:>7}KB  {}", c.bytes / 1024, c.name);
            }
            for (name, why) in &packed.leaving {
                println!("  left: {name} — {why}");
            }

            keep(store.save("working_set", &packed.taking), "store");
            keep(store.save("working_set_left", &packed.leaving), "store");
            keep(store.save("working_set_packed_at", &atlas::store::now()), "store");
            println!();
            println!("Recorded. `atlas carry back <folder>` when you get home.");
        }
        Some("missing") => {
            let Some(name) = args.get(1) else {
                println!("Which one? atlas carry missing draft.mp4");
                return;
            };
            let left: Vec<(String, String)> = store.load("working_set_left");
            match left.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                Some((n, why)) => println!("{}", workingset::not_here(n, why)),
                None => {
                    let taking: Vec<Carried> = store.load("working_set");
                    if taking.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
                        println!("{name} did come with us — it's in the carried set.");
                    } else {
                        println!("{name} wasn't in the last pack either way.");
                    }
                }
            }
        }
        Some("back") => {
            let Some(folder) = args.get(1) else {
                println!("Which folder did the changed copies come home in? atlas carry back ~/from-phone");
                return;
            };
            let taking: Vec<Carried> = store.load("working_set");
            if taking.is_empty() {
                println!("Nothing was packed, so nothing can come back.");
                return;
            }
            let packed_at: u64 = store.load("working_set_packed_at");

            // What actually came home: files in that folder whose names match
            // something that went out. Matched by name rather than path
            // because the phone will not have reproduced the laptop's folder
            // layout, and pretending otherwise would silently match nothing.
            let mut changed: Vec<Changed> = Vec::new();
            let Ok(entries) = std::fs::read_dir(folder) else {
                println!("Can't read {folder}.");
                return;
            };
            for e in entries.flatten() {
                let path = e.path();
                let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                    continue;
                };
                let Some(origin) = taking.iter().find(|c| c.name == name) else {
                    continue;
                };
                let Ok(meta) = std::fs::metadata(&path) else { continue };
                if !meta.is_file() {
                    continue;
                }
                changed.push(Changed {
                    path: origin.path.clone(),
                    at: mtime_secs(&path),
                    bytes: meta.len(),
                });
            }

            if changed.is_empty() {
                println!("Nothing in {folder} matches anything that went out.");
                return;
            }

            // The other half of the clash test, measured rather than assumed:
            // a carried file whose copy *here* has been written since the pack
            // moved on while you were away.
            let also_here: Vec<String> = taking
                .iter()
                .filter(|c| mtime_secs(std::path::Path::new(&c.path)) > packed_at)
                .map(|c| c.path.clone())
                .collect();

            let (clean, clashes) = workingset::returning(&changed, &also_here);
            println!(
                "{clean} of {} land without a question.",
                changed.len()
            );
            if clashes.is_empty() {
                println!("Nothing changed at both ends.");
            } else {
                println!("Changed in both places — you choose which wins:");
                for p in &clashes {
                    println!("  {p}");
                }
            }
            println!();
            println!("Nothing has been copied. This is the report; the copy is yours to make.");
        }
        Some(other) => {
            println!("I don't know \"{other}\" — try list, pack, missing or back.")
        }
    }
}

/// Read a `How` out of the string `remote.tell_me` holds.
///
/// The config field is a string because it is hand-edited YAML; this is the
/// one place that has to agree with it, so an unrecognised value falls back to
/// the same default the type does rather than failing the command.
fn remote_how(s: &str) -> atlas::remote::How {
    use atlas::remote::How;
    match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "quietly" => How::Quietly,
        "dont_tell_me" | "don't_tell_me" | "nothing" => How::DontTellMe,
        _ => How::InYourEar,
    }
}

/// Is the machine that has to run this actually up?
///
/// A real TCP probe of a real paired contact, not a guess. Returns `None` when
/// there is nobody paired to probe — which is a different answer from "not
/// reachable" and is printed as one.
fn laptop_reachable(dir: &std::path::Path, to: Option<&str>) -> Option<(String, bool)> {
    let pairings = atlas::kin::Pairings::load(dir);
    let contact = match to {
        Some(name) => pairings
            .contacts
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))?,
        None => pairings.contacts.first()?,
    };
    let address = format!("{}:{}", contact.host, contact.port);
    Some((contact.name.clone(), atlas::watch::reachable(&address, 800)))
}

/// `atlas remote` — the queue of things that need the other machine.
///
/// The phone half of this does not exist. The queue, the states, the
/// give-up rule and the "was that worth interrupting you for" gate all do, and
/// all of them are the laptop's side of the exchange — which is the side this
/// binary runs on.
fn run_remote(cfg: &Config, dir: &std::path::Path, args: &[String]) {
    use atlas::remote::{self, How, Needs, Queue, State};

    let store = atlas::roots::store();
    let rcfg = cfg.tools.as_ref().map(|t| t.remote.clone()).unwrap_or_default();
    let mut q: Queue = store.load("remote_queue");
    let now = atlas::store::now();

    let flagged = |f: &str| args.iter().any(|a| a == f);
    let id_arg = || args.get(1).and_then(|n| n.parse::<u64>().ok());

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            let waiting = q.waiting_count();
            println!(
                "{waiting} waiting, {} in the queue altogether.",
                q.requests.len()
            );
            for r in q.for_the_laptop() {
                println!(
                    "  {}  {}  ({})",
                    r.id,
                    r.what,
                    match r.needs {
                        Needs::TheLaptop => "needs the laptop",
                        Needs::Anything => "could run anywhere",
                    }
                );
            }
            for r in q.given_up(now, &rcfg) {
                let days = now.saturating_sub(r.asked_at) / 86_400;
                println!("  {}", remote::never_ran(r, days));
            }
            println!();
            println!("atlas remote ask <what>          queue it");
            println!("atlas remote start|done|failed|drop <number>");
        }
        Some("ask") => {
            let what = atlas::cli::plain_words(&args[1..], &["--to", "--secs"]);
            if what.trim().is_empty() {
                println!("Ask for what? atlas remote ask \"render the draft\"");
                return;
            }
            let needs = if flagged("--anywhere") { Needs::Anything } else { Needs::TheLaptop };
            let how = if flagged("--quietly") {
                How::Quietly
            } else if flagged("--dont-tell-me") {
                How::DontTellMe
            } else {
                remote_how(&rcfg.tell_me)
            };

            let to = atlas::cli::flag_value(args, "--to");

            let id = q.ask(&what, needs, how, now);
            match laptop_reachable(dir, to) {
                Some((name, up)) => {
                    println!("{}", remote::handing_off(&what, up));
                    println!("(probed {name} just now — {}.)", if up { "up" } else { "not answering" });
                }
                None => {
                    // Honest about the gap rather than picking a branch: with
                    // nothing paired there is no machine to probe, and saying
                    // "queued, it'll go when we're back in touch" would be a
                    // sentence about a relationship that doesn't exist.
                    println!("Queued as #{id}. Nothing is paired yet, so there's no other machine to reach —");
                    println!("`atlas invite` first, and this will go the moment one answers.");
                }
            }
            if remote::worth_waking_for(needs, flagged("--urgent")) {
                println!("Marked urgent and it needs the laptop — worth waking it if you have wake-on-LAN.");
            }
            keep(store.save("remote_queue", &q), "store");
            println!("#{id}.");
        }
        Some("start") => match id_arg() {
            Some(id) => {
                // The gate `confirm_side_effects` was written for. It ships
                // on, and until 19 Sep 2026 nothing read it: `atlas remote
                // start 3` marked a request running with no check of what it
                // was. A safety switch that is on by default and connected to
                // nothing is the worst of the three states.
                let asking = q
                    .requests
                    .iter()
                    .find(|r| r.id == id)
                    .filter(|r| r.state == State::Waiting)
                    .filter(|r| remote::needs_your_yes(&r.what, &rcfg) && !flagged("--yes"))
                    .map(|r| remote::asking_before_it_runs(id, &r.what));
                match asking {
                    Some(said) => println!("{said}"),
                    None if q.set(id, State::Running) => {
                        keep(store.save("remote_queue", &q), "store");
                        println!("#{id} running.");
                    }
                    None => println!("No request #{id}."),
                }
            }
            None => println!("Which one? atlas remote start 3"),
        },
        Some(verb @ ("done" | "failed")) => {
            let Some(id) = id_arg() else {
                println!("Which one? atlas remote {verb} 3 \"what happened\"");
                return;
            };
            let failed = verb == "failed";
            let result = atlas::cli::plain_words(&args[2..], &["--secs"]);
            let took = atlas::cli::flag_value(args, "--secs")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or_else(|| {
                    q.requests
                        .iter()
                        .find(|r| r.id == id)
                        .map(|r| now.saturating_sub(r.asked_at))
                        .unwrap_or(0)
                });

            if !q.set(id, if failed { State::Failed } else { State::Done }) {
                println!("No request #{id}.");
                return;
            }
            keep(store.save("remote_queue", &q), "store");
            let Some(r) = q.requests.iter().find(|r| r.id == id) else { return };
            match remote::finished(r, took, if result.is_empty() { "—" } else { &result }) {
                Some(said) => println!("{said}"),
                None => println!(
                    "#{id} {} quietly — not worth interrupting you for, by your own rule.",
                    if failed { "failed" } else { "finished" }
                ),
            }
        }
        Some("drop") => match id_arg() {
            Some(id) if q.set(id, State::Dropped) => {
                keep(store.save("remote_queue", &q), "store");
                println!("#{id} dropped.");
            }
            Some(id) => println!("No request #{id}."),
            None => println!("Which one? atlas remote drop 3"),
        },
        Some(other) => println!("I don't know \"{other}\" — try list, ask, start, done, failed or drop."),
    }
}

/// Going somewhere your texts will not arrive.
///
/// `going_away.remind_days_before` is "remind you this many days before a
/// trip you've told it about", and there was no way to tell it about a trip.
/// `periodic_nudge` takes `days_since_last` and nothing kept a last. Both
/// were waiting on the same small thing: a date, written down.
fn run_away(cfg: &Config, args: &[String]) {
    use atlas::goingaway::{self, Away};

    let store = atlas::roots::store();
    let acfg = cfg.tools.as_ref().map(|t| t.going_away.clone()).unwrap_or_default();
    let ccfg = cfg.tools.as_ref().map(|t| t.codes.clone()).unwrap_or_default();
    let mut away: Away = store.load(goingaway::AWAY_RECORD);
    let book: atlas::accounts::Book = store.load(atlas::accounts::FILE);
    let sets: Vec<atlas::codes::Set> = store.load("codes");
    let now = atlas::store::now();
    let flag = |name: &str| atlas::cli::flag_value(args, name);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("on") => {
            let Some(when) = args.get(1).and_then(|d| goingaway::leaving_on(d)) else {
                println!("When? A date, written the one way: atlas away on 2026-11-03");
                return;
            };
            if when < now {
                println!("That date has already gone. Nothing changed.");
                return;
            }
            away.leaving_at = when;
            away.going_to = flag("--to").map(|s| s.to_string()).unwrap_or_default();
            keep(store.save(goingaway::AWAY_RECORD, &away), "store");
            let days = away.days_until(now).unwrap_or(0);
            println!("Noted: {days} days from now.");
            if !acfg.enabled {
                println!("Getting your accounts ready is switched off, so I won't raise it");
                println!("on my own -- `going_away.enabled: true` in tools.yaml.");
            } else {
                println!("I'll start raising it {} days out.", acfg.remind_days_before);
            }
        }
        Some("clear") => {
            away.leaving_at = 0;
            away.going_to.clear();
            keep(store.save(goingaway::AWAY_RECORD, &away), "store");
            println!("No trip. I'll go back to the periodic check.");
        }
        None | Some("check") => {
            away.last_checked = now;
            keep(store.save(goingaway::AWAY_RECORD, &away), "store");

            match away.days_until(now) {
                Some(days) => println!(
                    "You're going{} in {days} day{}.",
                    if away.going_to.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" to {}", away.going_to.trim())
                    },
                    if days == 1 { "" } else { "s" }
                ),
                None if away.on_a_trip() => println!("The trip you told me about has passed."),
                None => println!("No trip set. `atlas away on 2026-11-03` when you know."),
            }
            println!();
            if book.accounts.is_empty() {
                println!("I don't know about any of your accounts yet, so I can't say which");
                println!("would lock you out. `atlas accounts` is where they go.");
            } else {
                println!("{}", goingaway::spoken(&book.accounts));
            }
            println!();
            println!("{}", atlas::codes::before_you_go(&sets, &account_names(&book), &ccfg));
            println!();
            println!("{}", goingaway::WHY_NOT_OFF);
        }
        Some(other) => println!("I don't know `atlas away {other}`. Try `atlas away check`."),
    }
}

/// The catalogue, out loud.
///
/// Atlas's own inventory of itself: what it does, what state each thing is
/// in, which files it lives in, and — the part that was missing — what each
/// of those does on a platform other than this one.
///
/// It reads from `capability::all()` and `portable::how` and nothing else, so
/// there is no second copy of the answer to keep in step. `atlas catalog
/// --module sync` is the direction Atlas needs when it is working on itself:
/// a file is open, and the question is what breaks if this goes wrong.
fn run_catalog(args: &[String]) {
    use atlas::capability;
    use atlas::portable::Platform;

    let arg = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(|s| s.to_lowercase())
    };

    if args.iter().any(|a| a == "--markdown") {
        // The generated form of docs/CAPABILITIES.md. `tests/catalogue.rs`
        // compares the file on disk with this, so regenerating is how that
        // document stays true rather than how it gets rewritten.
        print!("{}", capability::as_markdown());
        return;
    }

    if let Some(m) = arg("--module") {
        let uses = capability::what_uses(&m);
        if uses.is_empty() {
            println!(
                "Nothing in the catalogue claims src/{m}.rs. That is either plumbing — config, \
                 storage, the platform layer — or a capability nobody wrote down."
            );
            return;
        }
        println!("src/{m}.rs is part of:");
        for c in uses {
            println!("  {:<12} {:<52} {}", c.id, c.what, c.state.plain());
        }
        return;
    }

    let platform = match arg("--platform").as_deref() {
        Some("windows") => Some(Platform::Windows),
        Some("mac") | Some("macos") => Some(Platform::Mac),
        Some("linux") => Some(Platform::Linux),
        Some("ios") | Some("iphone") => Some(Platform::Ios),
        Some("android") => Some(Platform::Android),
        Some("web") | Some("browser") => Some(Platform::Web),
        Some(other) => {
            println!("I don't know a platform called \"{other}\". Try windows, mac, linux, ios, android or web.");
            return;
        }
        None => None,
    };

    match platform {
        Some(p) => print!("{}", capability::on_platform_full(p)),
        None => {
            // No platform asked for: answer for the one it is on, since
            // "here" is the question nine times out of ten.
            let here = atlas::platform::what_am_i();
            println!("{}", capability::summary());
            print!("{}", capability::on_platform_full(here));
            // The five-platform answer on one line, which is the question
            // Atlas is built to survive: it is meant to run on Windows, a
            // Mac, Linux, an iPhone and an Android phone, and until now
            // nothing said how much of it each of those would get.
            let tally: Vec<String> = [
                Platform::Windows, Platform::Mac, Platform::Linux,
                Platform::Ios, Platform::Android, Platform::Web,
            ]
            .iter()
            .map(|p| format!("{} {}", p.bare(), capability::on(*p).len()))
            .collect();
            println!("\nOf {} things, possible on: {}", capability::all().len(), tally.join(" · "));

            let (claimed, modules) = capability::module_coverage();
            println!(
                "{claimed} of {modules} source files are claimed by something in this list. \
                 The rest is plumbing, or not written down yet."
            );
        }
    }
}

fn run_mobile(cfg: &Config, args: &[String]) {
    use atlas::companion::{self, CompanionConfig, Phone, Piece};

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("android") => {
            println!("On Android:");
            for a in atlas::android::abilities() {
                println!(
                    "  [{}] {}{}",
                    a.can.plain(),
                    a.what,
                    if a.needs_deliberate_permission { "  (needs a permission you grant in Settings)" } else { "" }
                );
                println!("      {}", a.detail);
            }
            println!();
            println!("Permissions worth understanding before you grant them:");
            for (name, what, why) in atlas::android::serious_permissions() {
                println!("  {name} — {what}");
                println!("      {why}");
            }
            println!();
            println!(
                "Wake word running all day costs about {:.1}% of the battery an hour.",
                atlas::android::wake_word_battery_percent_per_hour()
            );
        }
        Some("mirror") => {
            let store = atlas::roots::store();
            let ccfg: CompanionConfig = cfg
                .tools
                .as_ref()
                .map(|t| t.companion.clone())
                .unwrap_or_default();
            let phone: Phone = store.load("phone_mirror");
            let now = atlas::store::now();

            println!("{}", phone.state(now, &ccfg));
            println!();

            // The config is a list of strings; this is the only place that
            // turns them into the type that knows what is safe to lose in a
            // taxi. An unrecognised name is named rather than skipped — a
            // typo in `mirror:` would otherwise silently mirror nothing.
            println!("What your config says to mirror:");
            for name in &ccfg.mirror {
                let piece = match name.trim().to_ascii_lowercase().as_str() {
                    "outstanding" => Some(Piece::Outstanding),
                    "projects" => Some(Piece::Projects),
                    "notes" => Some(Piece::Notes),
                    "last_brief" => Some(Piece::LastBrief),
                    "code_counts" => Some(Piece::CodeCounts),
                    "thread" => Some(Piece::Thread),
                    _ => None,
                };
                match piece {
                    Some(p) if !p.safe_on_a_phone() => println!(
                        "  {name} — NOT safe on a phone. Atlas will not mirror it whatever this says."
                    ),
                    Some(p) => println!(
                        "  {name} — travels, {}",
                        if p.writable() { "and you can change it there" } else { "read-only there" }
                    ),
                    None => println!("  {name} — not something Atlas knows how to mirror (typo?)"),
                }
            }

            println!();
            println!("Never leaves the laptop, whatever the config says:");
            for (what, why) in companion::never_travels() {
                println!("  {what} — {why}");
            }

            println!();
            println!("How the two would talk:");
            for (how, detail, works_offline) in companion::how_they_talk() {
                println!("  {how}{} — {detail}", if works_offline { "" } else { " (needs both online)" });
            }
        }
        Some("back") => {
            let store = atlas::roots::store();
            let phone: Phone = store.load("phone_mirror");
            let pending: Vec<companion::Pending> =
                phone.waiting().into_iter().cloned().collect();
            // What moved here while the phone was away, from the same record
            // the carry set keeps. Nothing is invented: with no phone paired
            // this is an empty list against an empty list, and `merge` says so.
            let changed_here: Vec<String> = {
                let taking: Vec<atlas::workingset::Carried> = store.load("working_set");
                let packed_at: u64 = store.load("working_set_packed_at");
                taking
                    .iter()
                    .filter(|c| mtime_secs(std::path::Path::new(&c.path)) > packed_at)
                    .map(|c| c.name.clone())
                    .collect()
            };
            let m = companion::merge(&pending, &changed_here);
            let away_days = phone.mirror_age_days(atlas::store::now()).unwrap_or(0);
            let said = companion::on_return(&m, away_days);
            if said.trim().is_empty() {
                println!("Nothing waiting from a phone.");
            } else {
                println!("{said}");
            }
        }
        Some("ios") | None => {
            println!("{}", atlas::ios::first_run());
            println!();
            println!("On an iPhone or iPad:");
            for a in atlas::ios::abilities() {
                println!("  [{}] {}", a.can.plain(), a.what);
                println!("      {}", a.detail);
            }
            println!();
            println!("Flatly not possible on iOS:");
            for a in atlas::ios::cannot() {
                println!("  {} — {}", a.what, a.detail);
            }
            println!();
            println!("Ways to start it:");
            for (how, detail) in atlas::ios::ways_to_start() {
                println!("  {how} — {detail}");
            }
            println!();
            println!("The phone is better at: {}", atlas::ios::phone_is_better_at().join(", "));
            println!("The laptop is better at: {}", atlas::ios::laptop_is_better_at().join(", "));
            println!();
            println!(
                "Offline, on the phone: transcription {}, the big model {}.",
                if atlas::ios::works_offline("transcription") { "works" } else { "doesn't" },
                if atlas::ios::works_offline("the big model") { "works" } else { "doesn't" }
            );
            println!();
            println!("atlas mobile android   the same list for Android");
            println!("atlas mobile mirror    what would actually travel");
        }
        Some(other) => println!("I don't know \"{other}\" — try ios, android, mirror or back."),
    }
}

/// `atlas sync-setup <provider>` — the folder two devices meet in.
///
/// With no provider named this still opens the hub, which is what it did
/// before. Naming one gets the actual steps, split into what Atlas does and
/// what only you can do, plus an honest answer about whether the free tier is
/// enough for the traffic your settings imply.
/// `atlas sync key ...` and `atlas sync read ...`.
///
/// Separate from `sync-setup`, which is about a cloud provider. This is about
/// the key, and it is deliberately usable when nothing else is: `read` needs
/// a file and a phrase, and touches no daemon, no vault and no config.
fn run_sync(cfg: &Config, args: &[String]) {
    use atlas::sync;
    let store = atlas::roots::store();
    let now = atlas::store::now();
    let value = |f: &str| atlas::cli::flag_value(args, f);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("key") => match args.get(1).map(|s| s.to_lowercase()).as_deref() {
            Some("new") => {
                let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                if kept.is_set() && !args.iter().any(|a| a == "--replace") {
                    println!(
                        "This device already has a household key. `atlas sync key show` \
                         prints it. Making a new one makes every bundle written under the \
                         old one unreadable -- `--replace` if that is what you mean."
                    );
                    return;
                }
                let phrase = sync::new_key_phrase();
                let keeping = sync::KeptKey::keeping(&phrase, now);
                match store.save(sync::KEY_FILE, &keeping) {
                    Ok(()) => {
                        println!("Your household key:");
                        println!();
                        println!("    {phrase}");
                        println!();
                        println!("Write it down now. It is the only thing that opens a sealed");
                        println!("bundle -- on your other devices, and on this one after a");
                        println!("reinstall. I can show it again while this device still works");
                        println!("(`atlas sync key show`), and not after that.");
                        println!();
                        println!("{}", keeping.at_rest_says());
                        println!();
                        println!("On your other devices:  atlas sync key set {phrase}");
                        println!("Then turn it on:        sync.encrypt_bundles: true in tools.yaml");
                    }
                    Err(e) => println!("I made a key and couldn't keep it: {e}"),
                }
            }
            Some("set") => {
                let typed: String = args[2..].join(" ");
                let typed = value("--key").map(|k| k.to_string()).unwrap_or(typed);
                if typed.trim().is_empty() {
                    println!("atlas sync key set <phrase>   -- the phrase from `atlas sync key new`");
                    return;
                }
                // Checked before it is kept, and an existing key kept unless
                // `--replace` -- in `sync::set_key`, shared with the Sync page.
                let replace = args.iter().any(|a| a == "--replace");
                let typed = typed.replace("--replace", "");
                let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                if kept.is_set() && !replace && kept.phrase().ok().as_deref().map(str::trim) != Some(typed.trim()) {
                    println!(
                        "This device already has a household key. Using another one makes every \
                         bundle sealed under the old one unreadable here -- `--replace` if that is \
                         what you mean."
                    );
                    return;
                }
                match sync::set_key(&store, &typed, replace, now) {
                    Ok(said) => println!("{said}"),
                    Err(why) => println!("{why}"),
                }
            }
            Some("show") => {
                let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                match kept.phrase() {
                    Ok(p) => {
                        println!("{p}");
                        println!();
                        println!("{}", kept.at_rest_says());
                    }
                    Err(why) => println!("{why}"),
                }
            }
            Some("forget") => {
                let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                if !kept.is_set() {
                    println!("There is no household key on this device.");
                    return;
                }
                if !args.iter().any(|a| a == "--yes") {
                    println!(
                        "That leaves this device unable to open any sealed bundle, and \
                         unable to write one. If the phrase is written down you can put it \
                         back with `atlas sync key set`. Add --yes if you mean it."
                    );
                    return;
                }
                match store.save(sync::KEY_FILE, &sync::KeptKey::default()) {
                    Ok(()) => println!("Forgotten. Sealed bundles here are now closed to me."),
                    Err(e) => println!("I couldn't forget it: {e}"),
                }
            }
            _ => {
                let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                let on = cfg
                    .tools
                    .as_ref()
                    .map(|t| t.sync.encrypt_bundles)
                    .unwrap_or(false);
                println!("Sealing is {}.", if on { "on" } else { "off" });
                println!(
                    "This device {}.",
                    if kept.is_set() { "has a household key" } else { "has no household key" }
                );
                if kept.is_set() {
                    println!("{}", kept.at_rest_says());
                }
                println!();
                println!("  atlas sync key new             make one, and print it once");
                println!("  atlas sync key set <phrase>    use the same key as another device");
                println!("  atlas sync key show            print the phrase this device holds");
                println!("  atlas sync key forget --yes    remove it from this device");
                println!("  atlas sync read <file>         open a bundle by hand");
            }
        },

        Some("read") => {
            let Some(path) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("atlas sync read <file.bundle> [--card <recovery card file>]");
                println!("                            [--key <phrase>]");
                return;
            };
            let raw = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    println!("I couldn't read {path}: {e}");
                    return;
                }
            };
            // Three ways to have the key, in the order that asks least of
            // you. A recovery card is a file you can copy anywhere, which is
            // the answer to "I will lose a piece of paper": `--card` takes
            // the file rather than making you read the phrase off it and
            // type it.
            let from_card = |path: &str| -> Option<String> {
                std::fs::read_to_string(path).ok().and_then(|t| sync::phrase_in_card(&t))
            };
            let phrase = value("--key")
                .map(|k| k.to_string())
                .or_else(|| value("--card").and_then(|p| from_card(p)))
                .or_else(|| {
                    let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                    kept.is_set().then(|| kept.phrase().unwrap_or_default())
                })
                // The card this machine wrote, if it is still where it was
                // put. Last, so an explicit key always wins.
                .or_else(|| from_card(&sync::card_path().display().to_string()));
            let key = match phrase.as_deref().map(sync::key_from_phrase) {
                Some(Ok(k)) => Some(k),
                Some(Err(why)) => {
                    println!("{why}");
                    return;
                }
                None => None,
            };
            if let Some(env) = sync::peek(&raw) {
                println!("Sealed bundle from {} ({}), written at {}.",
                    env.from_device, env.belongs_to, env.made_at);
                println!();
            }
            match sync::read_bundle_for(&raw, key.as_deref(), sync::Reader::Command) {
                Ok(b) => {
                    println!(
                        "{} thing{} from {} ({}).",
                        b.events.len(),
                        if b.events.len() == 1 { "" } else { "s" },
                        b.from_device,
                        b.from_name
                    );
                    println!();
                    match serde_json::to_string_pretty(&b) {
                        Ok(text) => println!("{text}"),
                        Err(e) => println!("(I opened it and couldn't print it: {e})"),
                    }
                }
                Err(why) => println!("{why}"),
            }
        }

        _ => {
            println!("atlas sync key    -- the household key sealed bundles use");
            println!("atlas sync read   -- open a bundle by hand");
            println!();
            println!("Carrying things between devices is `sync` spoken to Atlas, or the");
            println!("daemon doing it by itself. This command is the key and the reader.");
        }
    }
}

fn run_sync_setup(cfg: &Config, args: &[String]) -> bool {
    use atlas::cloudsync::{self, Provider};

    // `atlas sync-setup compare` — the providers side by side. `compare` is
    // not a provider name, so it is handled before the provider parse.
    if args.first().map(|a| a.eq_ignore_ascii_case("compare")).unwrap_or(false) {
        println!("{}", atlas::cloudsync::compare_providers());
        return true;
    }

    let ccfg = cfg.tools.as_ref().map(|t| t.cloud.clone()).unwrap_or_default();

    // `cloud.provider` ships `onedrive` and was read by nothing: this took
    // the provider from the command line or gave up, so `atlas sync-setup`
    // with nothing after it opened the hub and the line in your file decided
    // nothing at all. Named on the command line still wins -- setting up a
    // second provider once should not mean editing the file first.
    let name = match args.first() {
        Some(a) => a.clone(),
        None => ccfg.provider.clone(),
    };
    let p = match name.trim().to_ascii_lowercase().replace(['-', '_'], "").as_str() {
        "onedrive" => Provider::OneDrive,
        "icloud" | "iclouddrive" => Provider::ICloud,
        "dropbox" => Provider::Dropbox,
        "googledrive" | "gdrive" | "google" => Provider::GoogleDrive,
        "folder" | "whatever" | "any" => Provider::Whatever,
        _ => return false,
    };

    // Per-provider setup guidance, reading the real on_windows/on_ios/hint
    // notes: `atlas sync-setup <provider>` (laptop) or `... <provider> phone`.
    let device = if args.iter().any(|a| a.eq_ignore_ascii_case("phone")) {
        atlas::cloudsync::Setup::Phone
    } else {
        atlas::cloudsync::Setup::Laptop
    };
    println!("{}", atlas::cloudsync::setup_guidance(p, device));

    let already = args.iter().any(|a| a == "--installed");

    // Whether the folder is already known is a fact on disk, not a guess: an
    // empty `cloud.folder` means setup has never found one.
    let found_at = if ccfg.folder.trim().is_empty() {
        None
    } else if std::path::Path::new(&ccfg.folder).exists() {
        Some(ccfg.folder.clone())
    } else {
        None
    };
    println!("{}", cloudsync::setting_up(p, found_at.as_deref()));
    println!();
    // Said at set-up rather than buried, because this is the moment the
    // decision is being made. The sentence it replaces claimed the opposite.
    println!("{}", cloudsync::ABOUT_BUNDLES);
    println!();

    // The size question, answered from the settings that actually drive it.
    // `days_kept` comes from the working set's own retention because
    // `cloud.carry_files` is what routes those files here — the two settings
    // are the same decision seen from two sides, and taking the number from
    // anywhere else would make the estimate about a different system.
    let days_kept = cfg
        .tools
        .as_ref()
        .map(|t| t.working_set.keep_days)
        .unwrap_or(14);
    let events_per_day = 200;
    let needed = cloudsync::space_needed_mb(events_per_day, days_kept, ccfg.carry_files);
    let (fits, why) = cloudsync::free_tier_is_enough(p, needed);
    println!(
        "Space: about {needed}MB for {days_kept} days of history{}. {}{}",
        if ccfg.carry_files { ", carrying files" } else { "" },
        if fits { "" } else { "Doesn't fit: " },
        why
    );
    println!();

    println!("On the laptop:");
    for s in cloudsync::laptop_steps(p, already) {
        match &s.why_you {
            None => println!("  Atlas does it: {}", s.what),
            Some(why) => println!("  You do it: {} — {why}", s.what),
        }
    }
    println!();
    println!("On the phone:");
    for s in cloudsync::phone_steps(p) {
        match &s.why_you {
            None => println!("  Atlas does it: {}", s.what),
            Some(why) => println!("  You do it: {} — {why}", s.what),
        }
    }
    println!();
    println!("If Atlas can't find the folder, look in:");
    for place in cloudsync::where_to_look(p) {
        println!("  {place}");
    }
    true
}

// ===========================================================================
// `atlas video` — the production cluster, wired.
//
// Seven modules sat in `UNWIRED_BASELINE` together and they are one pipeline:
// `measure` reads the file, `grade` judges it, `plainly` lets you say what is
// wrong without knowing the words for it, `edit` cuts, `voiceover` lays the
// script over the cuts, `publishing` decides what the export has to be, and
// `editors` says which tool did it and whether a friend with nothing installed
// could have done the same.
//
// The reason none of it was reachable was one missing piece, not seven:
// nothing ever measured a real file, so `grade` had nothing to judge and the
// rest of the chain had nothing to hang off. `src/measure.rs` is that piece.
// ffmpeg and ffprobe are already declared in `tools.yaml` under `video:` and
// `edit::render` already knew how to call them.
// ===========================================================================

fn editor_named(s: &str) -> Option<atlas::editors::Editor> {
    use atlas::editors::Editor;
    match s.trim().to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
        "ffmpeg" => Some(Editor::Ffmpeg),
        "resolve" | "davinci" | "davinciresolve" => Some(Editor::Resolve),
        "premiere" | "premierepro" => Some(Editor::Premiere),
        "aftereffects" | "ae" => Some(Editor::AfterEffects),
        "photoshop" | "ps" => Some(Editor::Photoshop),
        "finalcut" | "finalcutpro" | "fcp" => Some(Editor::FinalCut),
        _ => None,
    }
}

fn platform_named(s: &str) -> Option<atlas::publishing::Platform> {
    use atlas::publishing::Platform;
    match s.trim().to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
        "tiktok" => Some(Platform::TikTok),
        "reels" | "instagram" | "ig" => Some(Platform::Reels),
        "shorts" => Some(Platform::Shorts),
        "youtube" | "yt" => Some(Platform::YouTube),
        "x" | "twitter" => Some(Platform::XTwitter),
        "linkedin" => Some(Platform::LinkedIn),
        _ => None,
    }
}

/// `3.5-9` — a range of seconds, as written on the command line.
fn span(s: &str) -> Option<(f64, f64)> {
    let (a, b) = s.split_once('-')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Run ffmpeg with a fixed argument list, reporting what it said if it failed.
fn run_ffmpeg(tool: &atlas::tools::ExternalTool, args: Vec<String>) -> Result<(), String> {
    let (cmd, mut full) = tool.resolved(&atlas::tools::Vars::new());
    full.extend(args);
    match atlas::tools::command(&cmd).args(&full).output() {
        Err(e) => Err(format!("could not start {cmd}: {e}")),
        Ok(out) if !out.status.success() => Err(format!(
            "{cmd} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Ok(_) => Ok(()),
    }
}

fn run_video(cfg: &Config, args: &[String]) {
    use atlas::edit::{EditPlan, Overlay, Segment};
    use atlas::editors::{self, Job};
    use atlas::grade;
    use atlas::publishing;
    use atlas::voiceover::{self, Beat};

    let Some(video) = cfg.tools.as_ref().map(|t| t.video.clone()) else {
        println!("No tools.yaml loaded, so ffmpeg isn't configured. `atlas doctor` says what's missing.");
        return;
    };
    let flag = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| atlas::cli::flag_value(args, f);
    let all_values = |f: &str| -> Vec<String> {
        args.iter()
            .enumerate()
            .filter(|(_, a)| a.as_str() == f)
            .filter_map(|(i, _)| args.get(i + 1))
            .filter(|v| !v.starts_with("--"))
            .cloned()
            .collect()
    };

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("check") => {
            let Some(path) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("Which file? atlas video check clip.mp4");
                return;
            };
            let audio = match atlas::measure::audio_of(&video.ffmpeg, path) {
                Ok(a) => Some(a),
                Err(e) => {
                    println!("Couldn't measure the sound: {e}");
                    None
                }
            };
            let picture = match atlas::measure::picture_of(&video.ffmpeg, &video.ffprobe, path) {
                Ok(p) => Some(p),
                Err(e) => {
                    println!("Couldn't measure the picture: {e}");
                    None
                }
            };
            if audio.is_none() && picture.is_none() {
                return;
            }

            // Your `grade:` section, which had nowhere to land until 18 Sep
            // 2026 -- the numbers in the file matched the constants in
            // `grade.rs` exactly, so the advice was right and the file had
            // nothing to do with it.
            let gcfg = cfg.tools.as_ref().map(|t| t.grade.clone()).unwrap_or_default();
            let an = audio.as_ref().map(|a| grade::check_audio(a, &gcfg)).unwrap_or_default();
            let pn = picture.as_ref().map(grade::check_picture).unwrap_or_default();

            println!("{}", grade::spoken(&an, &pn));
            println!();
            for n in an.iter().chain(pn.iter()) {
                println!("  {} — {}", n.what, n.because);
                println!("      fix: {}{}", n.fix, if n.fixable_now { "" } else { "  (needs re-recording)" });
            }
            if an.is_empty() && pn.is_empty() {
                println!("  (nothing above the thresholds in grade.rs)");
            }

            if let Some(a) = &audio {
                println!();
                println!("Measured: {:.1} LUFS, true peak {:.1}dB, range {:.1}dB, noise floor {:.1}dB{}{}.",
                    a.lufs, a.true_peak_db, a.range_db, a.noise_floor_db,
                    if a.rumble { ", rumble" } else { "" },
                    if a.harsh_s { ", harsh S" } else { "" });
                println!("The audio chain that would fix it:");
                println!("  -af \"{}\"", grade::audio_chain(a).join(","));
            }
            if let Some(p) = &picture {
                println!();
                println!(
                    "Measured: {}x{} at {:.0}fps, {:.0}% exposure, {:.1}% pure black, {:.1}% pure white.",
                    p.width, p.height, p.fps, p.brightness * 100.0,
                    p.clipped_black * 100.0, p.clipped_white * 100.0
                );
                let band = grade::SafeArea::typical().caption_band();
                println!("Captions belong between {:.0}% and {:.0}% down the frame.", band.0, band.1);
            }

            // The look you said you start from. `preset:` is the third line of
            // the section that had nowhere to land, and a name that isn't a
            // preset is said rather than quietly ignored -- a setting that
            // silently falls back to the default is a setting that does
            // nothing while looking like it worked.
            match grade::preset_named(&gcfg.preset) {
                Some(p) => {
                    println!();
                    println!("Your starting look, `{}` — {}:", p.name, p.what_it_is);
                    println!("  -vf \"{}\"", grade::preset_filter(&p));
                }
                None => {
                    println!();
                    println!(
                        "`grade.preset` is set to \"{}\", which isn't one of mine. \
                         `atlas video presets` lists them.",
                        gcfg.preset
                    );
                }
            }

            let advice = grade::recording_advice(&an.iter().chain(pn.iter()).cloned().collect::<Vec<_>>());
            if !advice.is_empty() {
                println!();
                println!("For the next recording rather than this one:");
                for a in advice {
                    println!("  {a}");
                }
            }

            println!();
            println!("Not measured, so nothing above is a judgement about either:");
            for (what, why) in atlas::measure::unmeasured() {
                println!("  {what} — {why}");
            }
        }

        Some("fix") => {
            let said = atlas::cli::plain_words(&args[1..], &["--file", "--out"]);
            if said.trim().is_empty() {
                println!("Say what's wrong: atlas video fix \"I'm too quiet\" --file clip.mp4");
                return;
            }
            match atlas::plainly::understand(&said) {
                None => println!("{}", atlas::plainly::didnt_understand(&said)),
                Some(reading) => {
                    println!("{}", atlas::plainly::confirm(&reading));
                    match value("--file") {
                        None => println!("\nGive me the file with --file and I'll measure it."),
                        Some(path) => match atlas::measure::audio_of(&video.ffmpeg, path) {
                            Err(e) => println!("\nCouldn't measure it: {e}"),
                            Ok(a) => {
                                // Your `grade.target_lufs` rather than a -14
                                // written into the sentence: the section had
                                // nowhere to land until 18 Sep 2026, and this
                                // line stated the number as a fact about the
                                // platforms while the file claimed to set it.
                                let gcfg =
                                    cfg.tools.as_ref().map(|t| t.grade.clone()).unwrap_or_default();
                                let measured = format!(
                                    "{:.1} LUFS against the {} platforms normalise to, range {:.1}dB",
                                    a.lufs, gcfg.target_lufs, a.range_db
                                );
                                let notes = grade::check_audio(&a, &gcfg);
                                let chain = grade::audio_chain(&a);

                                // "Fixed" is a claim about a file, so it is
                                // only said once a file has been written.
                                // Nothing found, or nothing written, both say
                                // so plainly instead.
                                if notes.is_empty() {
                                    println!("\n{}", atlas::plainly::result(&said, &measured, false));
                                    println!("  -af \"{}\"  (if you want it anyway)", chain.join(","));
                                    return;
                                }
                                let Some(out) = value("--out") else {
                                    println!("\nThat's real: {}", notes[0].because);
                                    println!("  -af \"{}\"", chain.join(","));
                                    println!("Add --out fixed.mp4 and I'll write it.");
                                    return;
                                };
                                let args = vec![
                                    "-y".to_string(), "-v".into(), "error".into(),
                                    "-i".into(), path.to_string(),
                                    "-af".into(), chain.join(","),
                                    "-c:v".into(), "copy".into(),
                                    out.to_string(),
                                ];
                                match run_ffmpeg(&video.ffmpeg, args) {
                                    Err(e) => println!("\nCouldn't write it: {e}"),
                                    Ok(()) => {
                                        println!("\n{}", atlas::plainly::result(&said, &measured, true));
                                        println!("Written: {out}");
                                    }
                                }
                            }
                        },
                    }
                }
            }
        }

        Some("cut") => {
            let Some(path) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("Which file? atlas video cut clip.mp4 --at 0-3 --out short.mp4");
                return;
            };
            let out = value("--out").unwrap_or("cut.mp4").to_string();
            let speed: f64 = value("--speed").and_then(|s| s.parse().ok()).unwrap_or(1.0);

            let spans: Vec<(f64, f64)> = all_values("--at").iter().filter_map(|s| span(s)).collect();
            if spans.is_empty() {
                println!("Which part? atlas video cut {path} --at 0-3 --at 8-12 --out short.mp4");
                return;
            }

            let overlays: Vec<Overlay> = all_values("--caption")
                .iter()
                .filter_map(|c| {
                    let (text, when) = c.rsplit_once('@')?;
                    let (start, end) = span(when)?;
                    Some(Overlay {
                        text: text.to_string(),
                        start,
                        end,
                        position: value("--caption-at").unwrap_or("bottom").to_string(),
                        size: 36,
                    })
                })
                .collect();

            let plan = EditPlan {
                sources: vec![path.clone()],
                segments: spans
                    .iter()
                    .map(|(a, b)| Segment { source: 0, start: *a, end: *b, speed })
                    .collect(),
                overlays,
                music: value("--music").map(|s| s.to_string()),
                music_gain_db: -18.0,
                output: out.clone(),
                resolution: None,
                fps: None,
                intent: atlas::cli::plain_words(
                    &args[1..],
                    &["--out", "--at", "--caption", "--speed", "--music", "--caption-at"],
                ),
            };

            // The source's real length, so `validate` can catch a cut that
            // runs off the end rather than ffmpeg failing halfway through.
            let probed = atlas::tools::command(&video.ffprobe.command)
                .args(atlas::edit::probe_args(path))
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            let source_len = atlas::edit::duration_from_probe(&probed).unwrap_or(0.0);

            if let Err(e) = plan.validate(&[source_len]) {
                println!("That plan won't do: {e}");
                return;
            }
            println!("{}", atlas::edit::describe(&plan, source_len));

            let (tool, why) = editors::best_for(Job::Trim, &installed_editors(cfg));
            println!("{} — {why}", tool.name());
            let ecfg = cfg.tools.as_ref().map(|t| t.editors.clone()).unwrap_or_default();
            let note = editors::used(tool, Job::Trim, &ecfg);
            if !note.is_empty() {
                println!("{note}");
            }

            if !flag("--render") {
                println!();
                println!("ffmpeg {}", atlas::edit::ffmpeg_args(&plan).join(" "));
                println!("Add --render to actually write {out}.");
                return;
            }
            match atlas::edit::render(&video.ffmpeg, &plan, &atlas::tools::Vars::new()) {
                Ok(written) => println!("Written: {written}"),
                Err(e) => println!("Render failed: {e}"),
            }
        }

        Some("render") => {
            let Some(plan_file) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("Which plan? atlas video render plan.json --source clip.mp4 --out out.mp4");
                return;
            };
            let sources = all_values("--source");
            if sources.is_empty() {
                println!("A plan refers to sources by number — give them with --source, in order.");
                return;
            }
            let out = value("--out").unwrap_or("out.mp4").to_string();
            let text = match std::fs::read_to_string(plan_file) {
                Ok(t) => t,
                Err(e) => {
                    println!("Can't read {plan_file}: {e}");
                    return;
                }
            };
            let plan = match atlas::edit::plan_from_model(&text, sources.clone(), &out) {
                Ok(p) => p,
                Err(e) => {
                    println!("{e}");
                    return;
                }
            };
            let lens: Vec<f64> = sources
                .iter()
                .map(|s| {
                    let probed = atlas::tools::command(&video.ffprobe.command)
                        .args(atlas::edit::probe_args(s))
                        .output()
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                        .unwrap_or_default();
                    atlas::edit::duration_from_probe(&probed).unwrap_or(0.0)
                })
                .collect();
            if let Err(e) = plan.validate(&lens) {
                println!("That plan won't do: {e}");
                return;
            }
            println!("{}", atlas::edit::describe(&plan, lens.first().copied().unwrap_or(0.0)));
            if !plan.intent.trim().is_empty() {
                println!("Intent: {}", plan.intent);
            }
            if !flag("--render") {
                println!("ffmpeg {}", atlas::edit::ffmpeg_args(&plan).join(" "));
                println!("Add --render to actually write {out}.");
                return;
            }
            match atlas::edit::render(&video.ffmpeg, &plan, &atlas::tools::Vars::new()) {
                Ok(written) => println!("Written: {written}"),
                Err(e) => println!("Render failed: {e}"),
            }
        }

        Some("export") => {
            let Some(path) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("Which file? atlas video export clip.mp4 --for tiktok --out post.mp4");
                return;
            };
            let Some(p) = value("--for").and_then(platform_named) else {
                println!("Where's it going? --for tiktok | reels | shorts | youtube | x | linkedin");
                return;
            };
            let e = publishing::export_for(p);
            let out = value("--out").unwrap_or("export.mp4").to_string();

            let probed = atlas::tools::command(&video.ffprobe.command)
                .args(atlas::edit::probe_args(path))
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            let secs = atlas::edit::duration_from_probe(&probed).unwrap_or(0.0) as f32;

            println!(
                "{}: {}x{} at {}fps, {}Mbps, audio {}kbps.",
                p.name(), e.width, e.height, e.fps, e.bitrate, e.audio_kbps
            );
            println!("{}", e.note);
            println!();

            let format = publishing::format_of(
                secs,
                flag("--talking-head"),
                flag("--screen"),
                flag("--product"),
            );
            println!("{}", publishing::ready_to_post(p, secs, format));

            if let (Some(tfile), Some(topic)) = (value("--transcript"), value("--topic")) {
                match std::fs::read_to_string(tfile) {
                    Err(err) => println!("Can't read {tfile}: {err}"),
                    Ok(transcript) => {
                        println!();
                        println!("Description:");
                        println!("{}", publishing::description_from(&transcript, p, topic));
                        println!("Tags: {}", publishing::tags(topic, p).join(" "));
                    }
                }
            }

            println!();
            // Your `opsec.always_strip_metadata`, which shipped `true` and was
            // read by nothing until 18 Sep 2026 -- so every export carried the
            // original's GPS coordinates and camera serial while
            // `opsec::Risk::Metadata` told you stripping happened by default.
            // Defaulted `true` here as well, because that is what the page
            // claimed and what an install with no tools.yaml should do.
            let strip = cfg
                .tools
                .as_ref()
                .map(|t| t.opsec.always_strip_metadata)
                .unwrap_or(true);
            if !flag("--render") {
                println!("ffmpeg {}", publishing::export_args(&e, path, &out, strip).join(" "));
                println!("Add --render to actually write {out}.");
                return;
            }
            match run_ffmpeg(&video.ffmpeg, publishing::export_args(&e, path, &out, strip)) {
                Ok(()) => println!("Written: {out}"),
                Err(err) => println!("Export failed: {err}"),
            }
        }

        Some("voiceover") => {
            let Some(script_file) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("Which script? atlas video voiceover script.txt --length 45");
                return;
            };
            let script = match std::fs::read_to_string(script_file) {
                Ok(t) => t,
                Err(e) => {
                    println!("Can't read {script_file}: {e}");
                    return;
                }
            };
            let vcfg = cfg.tools.as_ref().map(|t| t.voiceover.clone()).unwrap_or_default();
            let total: f32 = match value("--length").and_then(|s| s.parse().ok()) {
                Some(t) => t,
                None => match value("--over") {
                    Some(path) => {
                        let probed = atlas::tools::command(&video.ffprobe.command)
                            .args(atlas::edit::probe_args(path))
                            .output()
                            .ok()
                            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                            .unwrap_or_default();
                        atlas::edit::duration_from_probe(&probed).unwrap_or(0.0) as f32
                    }
                    None => {
                        println!("How long is the footage? --length 45, or --over clip.mp4");
                        return;
                    }
                },
            };

            let lines_in = voiceover::break_into_lines(&script);
            let beats: Vec<Beat> = all_values("--beat")
                .iter()
                .filter_map(|b| {
                    let (at, strong) = match b.strip_suffix('!') {
                        Some(rest) => (rest, true),
                        None => (b.as_str(), false),
                    };
                    Some(Beat { at: at.trim().parse().ok()?, strong })
                })
                .collect();

            let lines = voiceover::lay_out(&lines_in, &beats, total, &vcfg);
            let fit = voiceover::fits(&lines, total, &vcfg);
            let snapped = voiceover::snapped_count(&lines, &beats);

            println!("{}", voiceover::spoken(&lines, &fit, snapped));
            println!();
            for l in &lines {
                println!("  {:>6.1}s  {:>5.1}s  {}", l.at, l.lasts, l.text);
            }
            if !beats.is_empty() {
                println!();
                println!("Music ducking, as (start, end, gain dB):");
                for (a, b, g) in voiceover::music_ducking(&lines, &vcfg) {
                    println!("  {a:.1} .. {b:.1}  {g:.1}dB");
                }
                println!("  filter: {}", voiceover::duck_filter(&vcfg));
            }
        }

        Some("tools") => {
            let have = installed_editors(cfg);
            println!(
                "Installed, per tools.yaml: {}",
                if have.is_empty() {
                    "nothing beyond ffmpeg".to_string()
                } else {
                    have.iter().map(|e| e.name()).collect::<Vec<_>>().join(", ")
                }
            );
            println!();
            for job in [
                Job::Trim, Job::CutSilences, Job::Captions, Job::Loudness,
                Job::ColourCorrect, Job::ColourGrade, Job::Crop, Job::Thumbnail,
                Job::MotionGraphics, Job::HandOff,
            ] {
                let (e, why) = editors::best_for(job, &have);
                println!("  {:<22} -> {} ({}) — {why}", job.plain(), e.name(), e.how());
            }
            println!();
            println!(
                "With nothing installed at all you still get: {}",
                editors::without_anything()
                    .iter()
                    .map(|j| j.plain())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!();
            println!("Where these usually live, if you want to point Atlas at one:");
            for e in [
                atlas::editors::Editor::Resolve,
                atlas::editors::Editor::Premiere,
                atlas::editors::Editor::AfterEffects,
                atlas::editors::Editor::FinalCut,
            ] {
                println!(
                    "  {} — {}{}",
                    e.name(),
                    editors::where_to_look(e),
                    if e.drivable() { "" } else { "  (Atlas prepares, you finish)" }
                );
            }
        }

        Some("presets") => {
            for p in grade::presets() {
                println!("{} — {}", p.name, p.what_it_is);
                println!("  for: {}", p.for_what);
                println!("  -vf \"{}\"", grade::preset_filter(&p));
            }
        }

        Some("music") => {
            for (name, source, note) in publishing::where_to_get_music() {
                println!("{name} ({}) — {note}", source.plain());
            }
        }

        _ => {
            println!("atlas video check <file>          measure it and say what a viewer notices");
            println!("atlas video fix \"<what's wrong>\" --file <f>   say it in your own words");
            println!("atlas video cut <file> --at 0-3 --at 8-12 --out short.mp4 [--render]");
            println!("atlas video render <plan.json> --source <f> --out <f> [--render]");
            println!("atlas video export <file> --for tiktok --out post.mp4 [--render]");
            println!("atlas video voiceover <script.txt> --length 45 [--beat 3.2] [--beat 9!]");
            println!("atlas video tools                which editor does which job");
            println!("atlas video presets              the grading presets, as ffmpeg");
            println!("atlas video music                where to get music you won't be taken down for");
        }
    }
}

/// Which editors this machine actually has, per `editors.installed`.
///
/// ffmpeg is always in the list because `editors.rs` treats it as the floor
/// rather than as an option — and on this path it is genuinely present, since
/// nothing in `atlas video` runs without it.
fn installed_editors(cfg: &Config) -> Vec<atlas::editors::Editor> {
    let mut have = vec![atlas::editors::Editor::Ffmpeg];
    if let Some(t) = cfg.tools.as_ref() {
        if t.editors.use_what_you_have {
            for name in &t.editors.installed {
                if let Some(e) = editor_named(name) {
                    if !have.contains(&e) {
                        have.push(e);
                    }
                }
            }
        }
    }
    have
}

// ===========================================================================
// `atlas content` — what a piece is likely to do, and what actually happened.
//
// `content` judges a piece before it goes out; `reach` tells a signal from a
// fluke afterwards. They were both unwired for the same reason and it is not
// the reason the video cluster was: the code was fine, there was simply
// nowhere for the numbers to live. A post's performance comes off a platform's
// analytics page and there is no connector for that — so the honest wiring is
// a file Eric fills in, not an integration Atlas pretends to have.
//
// One stored list feeds both. `reach::Post` is the richer record (it carries
// comments and follows, which `content::Performance` does not), so that is
// what is kept and `content`'s view is derived from it.
// ===========================================================================

fn hook_named(s: &str) -> atlas::content::Hook {
    use atlas::content::Hook;
    match s.trim().to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
        "called" => Hook::Called,
        "contradiction" => Hook::Contradiction,
        "unfinished" => Hook::Unfinished,
        "outcome" => Hook::Outcome,
        "question" => Hook::Question,
        _ => Hook::None,
    }
}

fn as_performance(p: &atlas::reach::Post) -> atlas::content::Performance {
    atlas::content::Performance {
        id: p.id.clone(),
        views: p.views,
        completion: p.completion,
        held_at_three: p.held_at_three,
        saves: p.saves,
        shares: p.shares,
        hook: hook_named(&p.hook),
        topic: p.topic.clone(),
        seconds: p.seconds,
    }
}

fn run_content(cfg: &Config, args: &[String]) {
    use atlas::content;
    use atlas::reach;

    let store = atlas::roots::store();
    let ccfg = cfg.tools.as_ref().map(|t| t.content.clone()).unwrap_or_default();
    let value = |f: &str| atlas::cli::flag_value(args, f);
    let flag = |f: &str| args.iter().any(|a| a == f);
    let posts: Vec<reach::Post> = store.load("content_posts");

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("review") => {
            let Some(script_file) = value("--script") else {
                println!("atlas content review --script draft.txt --seconds 32 --value-at 6 [--lands]");
                return;
            };
            let script = match std::fs::read_to_string(script_file) {
                Ok(t) => t,
                Err(e) => {
                    println!("Can't read {script_file}: {e}");
                    return;
                }
            };
            let Some(seconds) = value("--seconds").and_then(|s| s.parse::<f32>().ok()) else {
                println!("How long is it? --seconds 32");
                return;
            };
            // Asked for rather than guessed. Where the substance starts is the
            // input two of the seven faults are judged on, and inferring it
            // from the text would decide the answer before measuring it.
            let Some(value_at) = value("--value-at").and_then(|s| s.parse::<f32>().ok()) else {
                println!("Where does the substance actually start? --value-at 6");
                println!("(Not guessed: two of the faults are entirely about that number.)");
                return;
            };

            // One thing IS read off the script, with the rule stated: a piece
            // with no numeral in it anywhere is not being specific.
            let has_specifics = flag("--specifics")
                || script.chars().any(|c| c.is_ascii_digit());

            let piece = content::Piece {
                first_line: script.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string(),
                script: script.clone(),
                seconds,
                value_at_secs: value_at,
                has_specifics,
                lands: flag("--lands"),
            };

            let hook = content::hook_of(&piece.first_line);
            println!("Opening: {} ({:.0}% hold, generally).", hook.plain(), hook.holds() * 100.0);
            println!("{}", content::before_posting(&piece));
            println!();
            for f in content::faults(&piece) {
                println!("  {} — {}", f.what(), f.fix());
            }
            println!();
            println!(
                "Specifics: {} ({}).",
                if has_specifics { "found" } else { "none found" },
                if flag("--specifics") { "you said so" } else { "read off the script — a numeral anywhere counts" }
            );
            if !ccfg.review_before_posting {
                println!("(content.review_before_posting is off, so this only happens when you ask.)");
            }
        }

        Some("record") => {
            let Some(file) = args.get(1).filter(|a| !a.starts_with("--")) else {
                println!("atlas content record post.json   (one post, or a list of them)");
                return;
            };
            let text = match std::fs::read_to_string(file) {
                Ok(t) => t,
                Err(e) => {
                    println!("Can't read {file}: {e}");
                    return;
                }
            };
            // One or many, because an export is a list and a single post typed
            // by hand is not.
            let incoming: Vec<reach::Post> = match serde_json::from_str::<Vec<reach::Post>>(&text) {
                Ok(v) => v,
                Err(_) => match serde_json::from_str::<reach::Post>(&text) {
                    Ok(one) => vec![one],
                    Err(e) => {
                        println!("That isn't a post or a list of posts: {e}");
                        return;
                    }
                },
            };
            let mut all = posts;
            let before = all.len();
            for p in incoming {
                all.retain(|x| x.id != p.id);
                all.push(p);
            }
            match store.save("content_posts", &all) {
                Ok(()) => println!(
                    "{} posts on file ({} new or replaced).",
                    all.len(),
                    all.len().saturating_sub(before).max(1)
                ),
                Err(e) => println!("Couldn't save: {e}"),
            }
        }

        Some("posts") => {
            if posts.is_empty() {
                println!("Nothing recorded. `atlas content record posts.json` to start.");
                return;
            }
            println!("{} posts.", posts.len());
            for p in &posts {
                println!(
                    "  {:<14} {:>7} views  held {:>3.0}%  done {:>3.0}%  kept {:.3}  {}  {}",
                    p.id, p.views, p.held_at_three * 100.0, p.completion * 100.0,
                    p.kept(), p.hook, p.topic
                );
            }
        }

        Some("learn") => {
            let history: Vec<content::Performance> = posts.iter().map(as_performance).collect();
            let learned = content::learn(&history, &ccfg);
            println!("{}", content::how_its_going(&learned));
            if !learned.confident {
                println!(
                    "({} posted, and I want {} before I'll claim a pattern -- \
                     `content.min_posts_for_patterns` in tools.yaml.)",
                    history.len(),
                    ccfg.min_posts_for_patterns
                );
            }
            println!();
            for (hook, held, n) in &learned.by_hook {
                println!("  {:<32} {:>3.0}% held, across {n}", hook.plain(), held * 100.0);
            }
            let repeatable = history.iter().filter(|p| p.worth_repeating()).count();
            println!();
            println!(
                "{repeatable} of {} are worth repeating — held past three seconds AND finished.",
                history.len()
            );
        }

        Some("reach") => {
            // `reach.min_posts_for_direction`, which had no type to parse
            // into until 18 Sep 2026 while `direction` hardcoded the same 6.
            let rcfg = cfg.tools.as_ref().map(|t| t.reach.clone()).unwrap_or_default();
            // How near an edge counts, and what too little behind a number
            // means. `reach` narrows the sample floor to its own domain's --
            // see `outlier` -- rather than trading's thirty.
            let jcfg = cfg.tools.as_ref().map(|t| t.judgment.clone()).unwrap_or_default();
            if posts.len() < 2 {
                println!("{}", reach::spoken(&posts, &rcfg, &jcfg));
                return;
            }
            println!("{}", reach::spoken(&posts, &rcfg, &jcfg));
            println!();
            for f in reach::findings(&posts, &jcfg) {
                println!(
                    "  [{}] {} — {:.2}x, across {} posts{}",
                    f.sure.plain(), f.what, f.lift, f.across,
                    if f.sure.worth_acting_on() { "" } else { "  (not yet worth acting on)" }
                );
            }
            let (dir, why) = reach::direction(&posts, &rcfg, &jcfg);
            println!();
            println!("Direction: {} — {why}", dir.plain());
            if let Some((post, why)) = reach::outlier(&posts, &jcfg) {
                println!("Outlier: {} — {why}", post.id);
            }
        }

        Some("edits") => {
            println!("What Atlas can do to a piece without a model or a service:");
            for (what, how) in content::edits_it_can_do() {
                println!("  {what} — {how}");
            }
        }

        _ => {
            println!("atlas content review --script draft.txt --seconds 32 --value-at 6");
            println!("atlas content record <posts.json>   what actually happened");
            println!("atlas content posts                 what's on file");
            println!("atlas content learn                 patterns across everything");
            println!("atlas content reach                 signal or fluke, and which way it's going");
            println!("atlas content edits                 the tedious half Atlas will take");
        }
    }
}

// ===========================================================================
// `atlas budget` — what a hosted model would cost, before it costs it.
//
// `budget.rs` is the gate every hosted call is supposed to go through, and no
// hosted call exists yet: `brain.rs` runs a local model. That made it look
// like a module waiting on something. It isn't — the question it answers
// ("what would this cost, and would it be refused?") is worth asking before
// the first hosted call rather than after, and it is answerable now.
// ===========================================================================

fn difficulty_named(s: &str) -> Option<atlas::budget::Difficulty> {
    use atlas::budget::Difficulty;
    match s.trim().to_ascii_lowercase().as_str() {
        "trivial" => Some(Difficulty::Trivial),
        "simple" => Some(Difficulty::Simple),
        "real" => Some(Difficulty::Real),
        "hard" => Some(Difficulty::Hard),
        _ => None,
    }
}

fn run_budget(cfg: &Config, args: &[String]) {
    use atlas::budget::{self, Approval, Difficulty, Job, Ledger, Tier};

    let store = atlas::roots::store();
    let bcfg = cfg.tools.as_ref().map(|t| t.budget.clone()).unwrap_or_default();
    let ledger: Ledger = store.load("budget_ledger");
    let now = atlas::store::now();
    let value = |f: &str| atlas::cli::flag_value(args, f);
    let num = |f: &str, d: u64| value(f).and_then(|s| s.parse::<u64>().ok()).unwrap_or(d);

    let job_from_args = |what: String| Job {
        what,
        cached_input: num("--cached", 120_000),
        fresh_input: num("--fresh", 2_000),
        expected_output: num("--out", 1_500),
        can_wait: args.iter().any(|a| a == "--can-wait"),
    };
    let difficulty = value("--difficulty")
        .and_then(difficulty_named)
        .unwrap_or(Difficulty::Real);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            println!("{}", budget::report(&ledger, &bcfg, now));
            println!();
            println!(
                "Caps: ${:.2} a day, ${:.2} a month. Ceiling: {}.",
                bcfg.daily_cap,
                bcfg.monthly_cap,
                bcfg.ceiling.name()
            );
            println!(
                "Spent: ${:.2} today, ${:.2} this month, across {} recorded calls.",
                ledger.today(now),
                ledger.this_month(now),
                ledger.spends.len()
            );
            if !bcfg.enabled {
                println!();
                println!("Hosted models are off (budget.enabled: false), so route() returns the local");
                println!("model whatever it's asked — which is why every estimate below is $0 until");
                println!("you turn them on deliberately.");
            }
            println!();
            println!("atlas budget would \"<the job>\" [--difficulty real] [--cached N --fresh N --out N]");
            println!("atlas budget night <count> [--difficulty real]");
            println!("atlas budget rates");
        }

        Some("would") => {
            let what = atlas::cli::plain_words(
                &args[1..],
                &["--difficulty", "--cached", "--fresh", "--out"],
            );
            if what.trim().is_empty() {
                println!("What job? atlas budget would \"rewrite the retry logic\"");
                return;
            }
            let job = job_from_args(what.clone());
            println!("{what}");
            println!(
                "  {} cached in, {} fresh in, {} out. {}",
                job.cached_input, job.fresh_input, job.expected_output,
                if job.can_wait { "can wait" } else { "wanted now" }
            );
            let routed = budget::route(difficulty, &bcfg);
            println!("  {} routes to {}.", difficulty.plain(), routed.name());
            println!();
            for tier in [Tier::Haiku, Tier::Sonnet, Tier::Opus] {
                println!(
                    "  {:<8} ${:>7.4} warm, ${:>7.4} on the first call (filling the cache)",
                    tier.name(),
                    budget::estimate(&job, tier, &bcfg),
                    budget::first_run_estimate(&job, tier, &bcfg)
                );
            }
            println!();
            match budget::allow(&job, difficulty, &ledger, &bcfg, now) {
                Approval::Go { tier, dollars, batched } => println!(
                    "Allowed on {} at ${dollars:.4}{}.",
                    tier.name(),
                    if batched { ", at the overnight rate" } else { "" }
                ),
                Approval::Local(why) => println!("Stays here — {why}."),
                Approval::Refused(why) => println!("Refused — {why}"),
            }
        }

        Some("night") => {
            let count = args
                .get(1)
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0);
            if count == 0 {
                println!("How many tasks? atlas budget night 12");
                return;
            }
            // Every task the same size, and said so: the point of this
            // estimate is the cache effect across a run, not a per-task
            // breakdown Atlas does not have.
            let jobs: Vec<Job> = (1..=count)
                .map(|i| Job { can_wait: true, ..job_from_args(format!("task {i}")) })
                .collect();
            let (total, note) = budget::overnight_estimate(&jobs, difficulty, &bcfg);
            println!("{note}");
            println!(
                "Every task sized the same ({} cached, {} fresh, {} out) — change it with --cached/--fresh/--out.",
                jobs[0].cached_input, jobs[0].fresh_input, jobs[0].expected_output
            );
            if total > bcfg.daily_cap {
                println!(
                    "That is past the ${:.2} daily cap, so it would be refused partway through.",
                    bcfg.daily_cap
                );
            }
        }

        Some("rates") => {
            println!("Dollars per million tokens, from tools.yaml:");
            for tier in [Tier::Haiku, Tier::Sonnet, Tier::Opus] {
                if let Some(r) = bcfg.rate(tier) {
                    println!("  {:<8} in ${:.2}  out ${:.2}", tier.name(), r.input, r.output);
                }
            }
            println!(
                "  a cache read costs {:.0}% of a fresh input token; writing the cache costs {:.2}x.",
                bcfg.cache_read_fraction * 100.0,
                bcfg.cache_write_multiplier
            );
            println!("  the batch (overnight) rate is {:.0}% of list.", bcfg.batch_multiplier * 100.0);
        }

        Some(other) => println!("I don't know \"{other}\" — try status, would, night or rates."),
    }
}

/// `atlas hub` — where Atlas can be reached from, said once.
///
/// **This does not open a browser, and that is Eric's ruling, 17 Sep 2026:**
/// *"the hub should not be a browser. You say that if Atlas has a dependency
/// on the internet."* The first version of this command launched Chrome, which
/// walked straight back into the design position the panels were rebuilt to
/// get away from. `tests/capability_wiring.rs` already records it against
/// `look`: rendering the panels as HTML "needs an external browser or a
/// bundled web engine, and the ruling for this system is in-house and
/// self-contained, so the design is painted natively by `look_paint`/`window`
/// instead."
///
/// So the server is for the PHONE, which is the job `server.rs` was written
/// for -- a phone cannot run the native window, and loopback plus a VPN is the
/// honest way to reach a desktop from one. On the desktop the surface is the
/// native window, and this command says so rather than papering over the gap.
///
/// The address still matters and still had a real defect: the token was
/// regenerated on every start, so the phone's saved URL broke on every desktop
/// reboot. `server::token_for` fixes that, which is why this prints one fixed
/// address instead of a different one each run.
fn run_hub_address(args: &[String]) {
    let store = atlas::roots::store();
    let token = match atlas::server::token_for(&store) {
        Ok(t) => t,
        Err(e) => {
            println!("I couldn't read or make the hub token: {e}");
            return;
        }
    };
    // The configured port, because `tools.yaml` can move it and a printed
    // address that ignores the setting is wrong in exactly the cases where
    // someone changed it on purpose.
    let configured = Config::load(&atlas::roots::config_dir())
        .ok()
        .and_then(|c| c.tools.map(|t| t.server.port))
        .unwrap_or(8787);
    // Where the running Atlas's hub really answers, when it does (it opens
    // beside a taken port rather than not at all: `server::open_hub`).
    let port = atlas::server::hub_port(&atlas::roots::state_dir(), configured);

    let url = atlas::server::hub_url(port, &token, "/hub");
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("address") | Some("url") => println!("{url}"),
        None => {
            println!("On this machine: open Atlas from the Start menu or the desktop and press");
            println!("Hub or Settings -- or say \"show me the hub\" or \"show me settings\". The hub");
            println!("shows inside Atlas's own window, no browser. `atlas home hub` opens it too.");
            println!();
            println!("From your phone:  {url}");
            println!();
            println!("  Loopback only, so that address is reachable from another device only");
            println!("  through a VPN that terminates on this machine. It is the same address");
            println!("  every time now -- save it once. It used to change on every restart,");
            println!("  which is why saving it never worked.");
            println!();
            println!("It only answers while Atlas is running.");
        }
        Some(other) => println!(
            "I don't know \"{other}\" -- try `atlas hub` or `atlas hub address`.\n\
             There is no `open`: opening a browser is not how Atlas shows you things."
        ),
    }
}

/// `atlas startup on|off|status` — Atlas starting itself when you log in.
///
/// Eric's ruling, 17 Sep 2026: yes, in the background. So `on` with no mode
/// means `--daemon`.
///
/// **It prints the exact command and what it will do before running it**, and
/// that is not decoration. Registering a logon task means this machine will
/// start something that holds the microphone every time you sign in, and the
/// person agreeing to that should be able to see what is being registered
/// rather than trusting a sentence. `startup show` prints it and runs nothing.
fn run_startup(args: &[String]) {
    use atlas::startup::{self, Mode};

    // `current_exe`, not argv[0] and not a relative path. A logon task starts
    // in `system32`, so anything relative resolves there -- see the module
    // note in `startup.rs` for why that used to be fatal and no longer is.
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            println!("I couldn't work out where my own program file is ({e}), so I can't");
            println!("register anything that would still find it at logon.");
            return;
        }
    };

    let rest = args.get(1..).map(|r| r.join(" ")).unwrap_or_default();
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("on") => {
            let mode = Mode::parse(&rest).unwrap_or(Mode::Background);
            let plan = startup::register(&exe, mode);
            println!("This will {}.\n", plan.what);
            println!("  {}\n", plan.as_typed());
            // On Windows the task is made from its XML definition (no time
            // limit, runs on battery): written first, printed so it can be
            // read before it's used.
            if cfg!(windows) {
                match startup::write_task_file(&exe, mode) {
                    Ok(path) => println!("  (the task's definition: {})\n", path.display()),
                    Err(e) => {
                        println!("{e}\nNothing was changed.");
                        return;
                    }
                }
            }
            match startup::run(&plan) {
                Ok(true) => {
                    let _ = startup::remember(&atlas::roots::state_dir(), true);
                    println!("Done. Atlas will start when you log in, and {}.", mode.plainly());
                    println!("Turn it off again with `atlas startup off`.");
                }
                Ok(false) => {
                    println!("That command ran and refused. Nothing was changed.");
                    println!("Run it yourself to see what it said -- I deliberately don't");
                    println!("swallow its output and guess.");
                }
                Err(e) => println!("{e}\nNothing was changed."),
            }
        }
        Some("off") => {
            let plan = startup::remove();
            println!("This will {}.\n", plan.what);
            println!("  {}\n", plan.as_typed());
            // The sign-in list entry the window falls back to when Task
            // Scheduler says no (`startup::turn_on`), gone too.
            if cfg!(windows) {
                let _ = startup::run(&startup::run_entry_remove());
            }
            let _ = startup::remember(&atlas::roots::state_dir(), false);
            match startup::run(&plan) {
                Ok(true) => println!("Done. Atlas will not start on its own any more."),
                // `/Delete` on a task that is not there reports failure, and
                // that is the same outcome the person asked for.
                Ok(false) => println!("It wasn't registered, so there was nothing to remove."),
                Err(e) => println!("{e}\nNothing was changed."),
            }
        }
        Some("status") | None => {
            let plan = startup::whether_registered();
            match startup::run(&plan) {
                Ok(true) => println!("Atlas starts when you log in."),
                Ok(false) => {
                    println!("Atlas does not start on its own.");
                    println!();
                    println!("  atlas startup on            in the background (wake word,");
                    println!("                              scheduled work, offers)");
                    println!("  atlas startup on listening  the wake word and nothing else");
                }
                Err(e) => println!("I couldn't ask: {e}"),
            }
        }
        Some("show") => {
            // Everything that would be run, and nothing run.
            for plan in [
                startup::register(&exe, Mode::Background),
                startup::register(&exe, Mode::Listening),
                startup::remove(),
                startup::whether_registered(),
            ] {
                println!("to {}:\n  {}\n", plan.what, plan.as_typed());
            }
            if !cfg!(windows) {
                println!("and this goes in {}:\n", startup::unit_path().display());
                println!("{}", startup::unit_file(&exe, Mode::Background));
            }
        }
        Some(other) => {
            println!("I don't know \"{other}\" -- try on, off, status or show.");
        }
    }
}

// acts on anything.
fn run_backends(args: &[String]) {
    // `roots::store()`, not `Store::new("data/state")`. That literal is a
    // relative path, so the Store it builds is rooted wherever the process
    // happened to be standing -- the bug that made Atlas start with an empty
    // memory when launched from a shortcut. It was mine: this function was
    // dropped by the 17 Sep merge and I restored it by hand with the literal
    // in it. `tests/one_install_root.rs` named the line and the fix.
    let store = atlas::roots::store();
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            let router = atlas::backends::Router::load(&store);
            let learned = router.what_ive_learned();
            if learned.is_empty() {
                println!(
                    "Nothing learned yet — I record which apps I can read as I read them, and it \
                     takes a few reads of an app before there's anything worth saying."
                );
                return;
            }
            println!("What I've learned about reading your apps:");
            for line in learned {
                println!("  {line}");
            }
        }
        Some("forget") => {
            let Some(app) = args.get(1) else {
                println!("Which app should I forget what I learned about? atlas backends forget <app>");
                return;
            };
            let mut router = atlas::backends::Router::load(&store);
            router.forget_app(app);
            match router.save(&store) {
                Ok(()) => println!("Forgotten what I'd learned about {app} — I'll learn it fresh."),
                Err(e) => println!("Couldn't save that: {e}"),
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try status or forget <app>."),
    }
}

/// Which Atlas this is, and which of your own devices belong to it. See
/// `household.rs`'s own doc: two installs know nothing about each other
/// unless the same person paired them, device to device, with a code.

/// `atlas wireguard` — your own server, reached over WireGuard, fenced so
/// your devices reach its model server and nothing else.
///
///   atlas wireguard            the layout, the fence, and what stays yours
///   atlas wireguard configs    make the keys and the three configs
///   atlas wireguard check      which devices have connected, and when
///   atlas wireguard tidy       delete the configs holding private keys
fn run_wireguard(cfg: &Config, args: &[String]) {
    use atlas::wireguard::Device;
    let tools = cfg.tools.clone().unwrap_or_default();
    let wcfg = tools.mesh.wireguard.clone();
    let plan = match atlas::wireguard::Plan::from_config(&wcfg) {
        Ok(p) => p,
        Err(e) => {
            println!("I can't lay out the tunnel: mesh.wireguard.subnet — {e}.");
            return;
        }
    };
    let dir = atlas::roots::data_sub("wireguard");
    let tool = atlas::wireguard::wg_tool(&wcfg);
    let vars = tools.vars.clone();
    match args.first().map(String::as_str) {
        None | Some("plan") => {
            println!("Your own server, over WireGuard:");
            for d in [Device::Server, Device::Laptop, Device::Phone] {
                println!("  the {} is {}", d.label(), plan.address(d));
            }
            println!();
            println!("What the server lets through from the tunnel:");
            for r in atlas::wireguard::fence(&plan) {
                println!("  - {}", r.what);
            }
            println!();
            println!("{}", atlas::wireguard::ONE_TUNNEL_ON_A_PHONE);
            println!();
            if plan.endpoint.is_empty() {
                println!("I don't have the server's outside address yet (mesh.wireguard.endpoint).");
            } else {
                println!("Your devices will find the server at {}.", plan.endpoint);
            }
            println!();
            println!("Yours to do:");
            for (i, step) in atlas::wireguard::YOURS.iter().enumerate() {
                println!("  {}. {step}", i + 1);
            }
            println!();
            println!("Mine: `atlas wireguard configs` makes the keys, the three configs and the");
            println!("server's fence; `atlas wireguard check` says who has connected.");
        }
        Some("configs") => {
            if plan.endpoint.is_empty() {
                println!("I need the server's outside address first — the name your home address");
                println!("answers to and the port — in mesh.wireguard.endpoint.");
                return;
            }
            let make = || atlas::wireguard::make_keys(&tool, &vars);
            let (server, laptop, phone) = match (make(), make(), make()) {
                (Ok(a), Ok(b), Ok(c)) => (a, b, c),
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                    println!("I couldn't make the keys: {e}");
                    println!("WireGuard's own tool makes them. Install WireGuard, then try again.");
                    return;
                }
            };
            let laptop_conf = atlas::wireguard::device_conf(&plan, Device::Laptop, &laptop, &server.public);
            let phone_conf = atlas::wireguard::device_conf(&plan, Device::Phone, &phone, &server.public);
            let (Ok(laptop_conf), Ok(phone_conf)) = (laptop_conf, phone_conf) else {
                println!("I couldn't write the device configs.");
                return;
            };
            let public = serde_json::json!({
                "server": server.public, "laptop": laptop.public, "phone": phone.public,
            });
            let files = [
                ("server.conf", atlas::wireguard::server_conf(&plan, &server, &laptop.public, &phone.public)),
                ("laptop.conf", laptop_conf),
                ("phone.conf", phone_conf),
                ("fence-linux.nft", atlas::wireguard::nftables(&plan, "wg0")),
                ("fence-windows.txt", atlas::wireguard::windows_rules(&plan).join("\n") + "\n"),
                ("public-keys.json", serde_json::to_string_pretty(&public).unwrap_or_default()),
            ];
            if let Err(e) = std::fs::create_dir_all(&dir) {
                println!("I couldn't make {}: {e}", dir.display());
                return;
            }
            for (name, body) in &files {
                if let Err(e) = std::fs::write(dir.join(name), body) {
                    println!("I couldn't write {name}: {e}");
                    return;
                }
            }
            println!("Made in {}:", dir.display());
            println!("  server.conf, laptop.conf, phone.conf — one for each WireGuard");
            println!("  the server's fence, for Linux and for Windows — your devices reach its model");
            println!("  server on port {} and nothing else", plan.model_port);
            println!();
            println!("The three .conf files hold private keys. Import each one, then run");
            println!("`atlas wireguard tidy` and I'll delete them. The phone's goes over by AirDrop or");
            println!("Files into the WireGuard app — not by email or a chat.");
        }
        Some("check") => {
            let names: std::collections::HashMap<String, String> =
                std::fs::read_to_string(dir.join("public-keys.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                    .and_then(|v| v.as_object().cloned())
                    .map(|m| {
                        m.into_iter()
                            .filter_map(|(k, v)| v.as_str().map(|p| (p.to_string(), k)))
                            .collect()
                    })
                    .unwrap_or_default();
            if names.is_empty() {
                println!("I haven't made the configs here, so I don't know whose key is whose.");
                return;
            }
            let show = atlas::tools::ExternalTool {
                args: vec!["show".into(), "all".into(), "latest-handshakes".into()],
                ..tool
            };
            let out = match show.run(&vars, None) {
                Ok(o) => o,
                Err(e) => {
                    println!("I couldn't ask WireGuard: {e}");
                    println!("On Windows that needs Atlas running as administrator.");
                    return;
                }
            };
            let seen = atlas::wireguard::handshakes(&out);
            let now = atlas::store::now();
            for name in ["laptop", "phone"] {
                let key = names.iter().find(|(_, n)| n.as_str() == name).map(|(k, _)| k.clone());
                let last = key.and_then(|k| seen.iter().find(|(p, _)| *p == k).map(|(_, t)| *t));
                println!("{}", atlas::wireguard::connection(name, last, now));
            }
        }
        Some("tidy") => {
            let mut gone = 0;
            for name in ["server.conf", "laptop.conf", "phone.conf"] {
                if std::fs::remove_file(dir.join(name)).is_ok() {
                    gone += 1;
                }
            }
            println!(
                "Deleted {gone} config file{} holding private keys. The public keys stay, so \
                 `atlas wireguard check` still knows who's who.",
                if gone == 1 { "" } else { "s" }
            );
        }
        Some(other) => {
            println!("I don't know `atlas wireguard {other}`. Try: atlas wireguard [configs|check|tidy]");
        }
    }
}

/// The window a double-click opens: set up the first time, "Atlas is
/// running" after that. Moves Atlas into its standard home first when it was
/// opened from somewhere that isn't an install.
fn run_home(double_clicked: bool, first: atlas::firstlaunch::First) {
    // Problems here are shown in a message box when Atlas was double-clicked:
    // it has no console, and until 28 Sep 2026 they went to one that wasn't
    // there (running Atlas Setup.exe again over a running Atlas did nothing,
    // silently).
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            atlas::firstlaunch::show_problem(&format!("I couldn't find myself on disk: {e}"));
            return;
        }
    };
    let exe_dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let told = std::env::var_os("ATLAS_HOME").map(|v| !v.is_empty()).unwrap_or(false);
    let home = atlas::firstlaunch::standard_home();
    match atlas::firstlaunch::where_to_live(
        &exe_dir,
        atlas::roots::looks_like_an_install(&exe_dir),
        told,
        home.as_deref(),
    ) {
        atlas::firstlaunch::Where::MoveTo(home) => {
            let words = first.words();
            let args: Vec<&str> = words.iter().map(|w| w.as_str()).collect();
            let installed = home.join(atlas::firstlaunch::INSTALLED_NAME);
            let open_installed = || match atlas::firstlaunch::spawn_quietly(&installed, &args) {
                Ok(child) => atlas::unwaited::dont_wait(child),
                Err(e) => atlas::firstlaunch::show_problem(&format!("I couldn't open the Atlas in {}: {e}", home.display())),
            };
            match atlas::firstlaunch::replacing_in(&exe, &home) {
                // Already there, byte for byte: just open it.
                atlas::firstlaunch::Replacing::Same => {
                    open_installed();
                    return;
                }
                // Never a silent downgrade.
                atlas::firstlaunch::Replacing::Older { installed: newer, mine } => {
                    let q = format!(
                        "The Atlas already on this computer is version {newer}, which is newer than this one ({mine}). \
                         Replace it with this older version? Your notes and settings are kept either way."
                    );
                    if !atlas::firstlaunch::ask_yes_no(&q) {
                        open_installed();
                        return;
                    }
                }
                atlas::firstlaunch::Replacing::Install => {}
            }
            match atlas::firstlaunch::move_in_over(&exe, &home, true, std::time::Duration::from_secs(20)) {
                // Carry on from the new home, in a fresh process, so every
                // path below is decided from there.
                Ok(moved) => match atlas::firstlaunch::spawn_quietly(&moved, &args) {
                    Ok(child) => atlas::unwaited::dont_wait(child),
                    Err(e) => atlas::firstlaunch::show_problem(&format!("I moved into {} but couldn't open from there: {e}", home.display())),
                },
                Err(e) => atlas::firstlaunch::show_problem(&format!(
                    "{e}\n\nIf Atlas is still running, close it (or restart the computer) and open this again."
                )),
            }
            return;
        }
        atlas::firstlaunch::Where::Here => {}
    }
    let dir = atlas::roots::config_dir();
    if atlas::firstlaunch::settings_missing(&dir) {
        let _ = atlas::firstlaunch::write_default_config(&dir);
    }
    let configured = Config::load(&dir).ok().and_then(|c| c.tools.map(|t| t.server.port)).unwrap_or(8787);
    // Opening Atlas once it's set up makes sure the background Atlas is
    // running (no console: it's started with no window of its own), then
    // opens the hub. Closing the window never stops it (Eric, 28 Sep 2026).
    // "Running" is Atlas's own lock and its hub's own answer, not whatever
    // holds the port (`firstlaunch::atlas_running`).
    let opening = atlas::firstlaunch::what_opening_does(
        atlas::firstlaunch::is_set_up(&atlas::roots::install_root()),
        atlas::firstlaunch::atlas_running(&atlas::roots::install_root()),
        first,
    );
    // Where the running Atlas's hub really is (it may have opened beside a
    // taken port: `server::open_hub`); the setting when it isn't answering.
    let port = atlas::firstlaunch::hub_port_at(&atlas::roots::install_root(), configured);
    if opening.start_background {
        match atlas::firstlaunch::spawn_quietly(&exe, &["--daemon"]) {
            Ok(child) => atlas::unwaited::dont_wait(child),
            Err(e) => atlas::firstlaunch::show_problem(&format!("I couldn't start Atlas in the background: {e}")),
        }
    }
    let first = opening.first;
    #[cfg(feature = "desktop-ui")]
    {
        let place = match atlas::setupwin::here(&atlas::roots::install_root(), &exe, port) {
            Ok(p) => p,
            Err(e) => {
                atlas::firstlaunch::show_problem(&format!("I couldn't get ready: {e}"));
                return;
            }
        };
        if double_clicked {
            atlas::firstlaunch::let_go_of_the_console();
        }
        if let Err(e) = atlas::setupwin::run(place, first) {
            atlas::firstlaunch::show_problem(&format!("I couldn't open my window: {e}"));
        }
    }
    // No desktop UI in this build: there is no setup window to open. The config
    // is written above; the hub (and, on a phone, the shell over it) is the
    // face. Say where things stand rather than opening nothing.
    #[cfg(not(feature = "desktop-ui"))]
    {
        let _ = (double_clicked, port, &exe, &first);
        println!("Atlas is set up. This build has no desktop window — open the hub in a browser.");
    }
}

/// `atlas kokoro-check [words]` — load Kokoro (`kokoro`), say the words (or
/// a sentence of its own) into `data/tmp/kokoro-check.wav`, and print how
/// long loading and speaking took against how long the speech lasts. Exit
/// status 0 only when it spoke. What the Windows build runs to prove the
/// library opens on real Windows (`.github/workflows/windows.yml`).
fn run_kokoro_check(words: &[String]) -> i32 {
    let root = atlas::roots::install_root();
    let ready = match atlas::kokoro::check(&root) {
        Ok(r) => r,
        Err(m) => {
            println!("Can't: {}. `atlas get kokoro` fetches it.", m.plain());
            return 1;
        }
    };
    let text = if words.is_empty() {
        "Your nine o'clock post is over length by twelve characters.".to_string()
    } else {
        words.join(" ")
    };
    let threads = atlas::kokoro::thread_count();
    let t = std::time::Instant::now();
    let k = match atlas::kokoro::Kokoro::load(&ready.runtime, &ready.model, threads) {
        Ok(k) => k,
        Err(why) => {
            println!("Can't: {why}.");
            return 1;
        }
    };
    let load_ms = t.elapsed().as_millis();
    let (voice, sid) = atlas::kokoro::voice_or_default(atlas::kokoro::DEFAULT_VOICE);
    let t = std::time::Instant::now();
    let samples = match k.synth(&text, sid, 1.0) {
        Ok(s) => s,
        Err(why) => {
            println!("Can't: {why}.");
            return 1;
        }
    };
    let synth_ms = t.elapsed().as_millis() as f64;
    let secs = samples.len() as f64 / k.sample_rate() as f64;
    if secs < 0.3 {
        println!("Kokoro loaded but said nothing ({secs:.2} s of audio).");
        return 1;
    }
    let out = atlas::roots::data_dir().join("tmp").join("kokoro-check.wav");
    let _ = std::fs::create_dir_all(out.parent().unwrap_or(&root));
    let _ = std::fs::write(&out, atlas::kokoro::to_wav(&samples, k.sample_rate()));
    println!(
        "Kokoro ({voice}, {threads} threads): loaded in {load_ms} ms; {secs:.2} s of speech made in {synth_ms:.0} ms \
         ({:.2}x its own length). Saved to {}.",
        synth_ms / 1000.0 / secs,
        out.display()
    );
    0
}

/// `atlas get` — fetch the voice pieces here, in the terminal. The same code
/// the setup window uses; what the launcher's first-run setup calls.
fn run_get(which: Option<&str>) {
    let root = atlas::roots::install_root();
    let tools = atlas::getpieces::Tools::default();
    let mut problems = 0;
    let Some((what, pieces)) = atlas::getpieces::set(which) else {
        println!("I don't know that set. `atlas get` (the voice), `atlas get seeing`, `atlas get pictures`, or `atlas get kokoro`.");
        return;
    };
    println!("Getting {what}. Safe to run again —");
    println!("it picks up where it stopped and skips what's already here.");
    println!();
    if let Err(why) = atlas::getpieces::room_for(&pieces, &root, atlas::getpieces::free_bytes(&root)) {
        println!("{why}");
        return;
    }
    for piece in pieces {
        if atlas::getpieces::have(&piece, &root) {
            println!("  [have] {}", piece.name);
            continue;
        }
        println!("  [get ] {} ({} MB)...", piece.name, piece.megabytes());
        let last = std::cell::Cell::new(0u64);
        let report = |done: u64, total: u64| {
            let pct = if total == 0 { 0 } else { done * 100 / total };
            if pct >= last.get() + 10 {
                last.set(pct - pct % 10);
                println!("         {pct}%");
            }
        };
        match atlas::getpieces::fetch(&piece, &root, &tools, &report) {
            Ok(()) => println!("  [ ok ] {}", piece.name),
            Err(e) => {
                problems += 1;
                println!("  [FAIL] {e}");
            }
        }
    }
    println!();
    if problems == 0 {
        println!("Everything's here.");
    } else {
        println!("{problems} piece(s) didn't arrive. Run this again to pick up where it stopped.");
    }
}

/// `atlas phone` — put Atlas on your phone over Tailscale and print the link
/// (the window shows the same thing as a code to scan); `atlas phone off`
/// takes it back off.
fn run_phone(cfg: &Config, args: &[String]) {
    let tool = atlas::phonelink::tailscale_tool();
    let vars = atlas::tools::Vars::new();
    if args.first().map(|s| s.as_str()) == Some("off") {
        match atlas::phonelink::unpublish(&tool, &vars) {
            Ok(()) => match atlas::roots::store().save(atlas::phonelink::LINK_KEY, &String::new()) {
                Ok(()) => println!("Atlas is off your phone. `atlas phone` puts it back."),
                Err(e) => println!(
                    "Atlas is off your phone, but I couldn't forget its old link ({e}), so the \
                     hub may still show it."
                ),
            },
            Err(e) => println!("That didn't take: {e}"),
        }
        return;
    }
    // The hub's real port, when the running Atlas's hub answers on another
    // than the configured one (`server::open_hub`).
    let port = atlas::server::hub_port(&atlas::roots::state_dir(), cfg.tools.as_ref().map(|t| t.server.port).unwrap_or(8787));
    let token = match atlas::server::token_for(&atlas::roots::store()) {
        Ok(t) => t,
        Err(e) => {
            println!("I couldn't read or make the hub token: {e}");
            return;
        }
    };
    let outcome = atlas::phonelink::publish(port, &token, &tool, &vars);
    println!("{}", atlas::phonelink::say(&outcome));
    if let atlas::phonelink::Serve::Published { url } = &outcome {
        if let Err(e) = atlas::roots::store().save(atlas::phonelink::LINK_KEY, url) {
            println!("(I couldn't keep the link for the hub's devices page: {e})");
        }
        println!();
        println!("{url}");
        println!();
        println!("On the phone: open it, then Share → Add to Home Screen, and Atlas is an app.");
    }
}

/// `atlas plugins` -- the add-ons on this install, and your decisions about them.
///
/// Approving happens here or on the hub's Add-ons page, and nowhere else:
/// there is deliberately no spoken command and no message that approves an
/// add-on, so nothing that can talk to Atlas can give one permissions.
fn run_plugins(cfg: &Config, args: &[String]) {
    use atlas::plugins;
    let dir = plugins::plugins_dir();
    let store = atlas::roots::store();
    let arg = |i: usize| args.get(i).map(|s| s.as_str()).unwrap_or("");
    let said = match arg(0) {
        "" | "list" => {
            let all = plugins::scan(&dir, &cfg.commands, &plugins::Approvals::load(&store));
            if all.is_empty() {
                println!("No add-ons. `atlas plugins add <file>` adds one; it does nothing until you approve it.");
                return;
            }
            for p in &all {
                describe_plugin(p);
            }
            return;
        }
        "approve" => {
            let id = arg(1);
            let all = plugins::scan(&dir, &cfg.commands, &plugins::Approvals::load(&store));
            let Some(p) = all.iter().find(|p| p.id == id) else {
                eprintln!("There's no add-on called \"{id}\". `atlas plugins` lists them.");
                leave(1);
            };
            describe_plugin(p);
            if p.manifest.is_none() {
                eprintln!("It can't be approved as it is.");
                leave(1);
            }
            println!("\nApprove {} to do the above? Type yes to approve.", p.name());
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            if line.trim().to_lowercase() != "yes" {
                println!("Not approved. It stays off.");
                return;
            }
            plugins::approve(&store, &dir, &cfg.commands, id, &p.sha256)
        }
        "revoke" => plugins::revoke(&store, arg(1), arg(2)),
        "trust" => plugins::trust_step(&store, &dir, &cfg.commands, arg(1), &args.get(2..).map(|a| a.join(" ")).unwrap_or_default()),
        "untrust" => plugins::untrust_step(&store, arg(1), &args.get(2..).map(|a| a.join(" ")).unwrap_or_default()),
        "remove" => {
            let tc = cfg.tools.as_ref();
            let trash = atlas::safety::Trash::new(
                tc.map(|t| t.trash.clone()).unwrap_or_default().resolved(&atlas::roots::install_root()),
            );
            plugins::remove(&store, &dir, arg(1), &trash)
        }
        "send" | "share" => send_plugin(&dir, arg(1), &args.get(2..).map(|a| a.join(" ")).unwrap_or_default()),
        "offers" => {
            let offers = plugins::Offers::load(&store).items;
            if offers.is_empty() {
                println!("Nobody has shared an add-on with you.");
            }
            for o in offers {
                println!(
                    "\n#{} {} -- {} (says it's by {})",
                    o.offer,
                    o.name,
                    match &o.in_group { Some(g) => format!("shared by {} in {g}", o.from), None => format!("sent by {}", o.from) },
                    o.author
                );
                for k in &o.permissions {
                    println!("  would be allowed to {}", plugins::permission(k).map(|p| p.plain).unwrap_or(k));
                }
            }
            return;
        }
        "take" => {
            let n: u64 = arg(1).trim_start_matches('#').parse().unwrap_or(0);
            let Some(o) = plugins::Offers::load(&store).items.into_iter().find(|o| o.offer == n) else {
                eprintln!("There's no offer #{n}. `atlas plugins offers` lists them.");
                leave(1);
            };
            println!("{} from {} would be allowed to:", o.name, o.from);
            for k in &o.permissions {
                println!("  {}", plugins::permission(k).map(|p| p.plain).unwrap_or(k));
            }
            println!("Add it and allow that? Type yes.");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            if line.trim().to_lowercase() != "yes" {
                println!("Left it where it is.");
                return;
            }
            plugins::take_offer(&store, &dir, &cfg.commands, n, &o.sha256)
        }
        "decline" => plugins::decline_offer(&store, arg(1).trim_start_matches('#').parse().unwrap_or(0)),
        "off" => plugins::set_off(&store, arg(1), true),
        "on" => plugins::set_off(&store, arg(1), false),
        "add" => plugins::add_from(std::path::Path::new(arg(1)), &dir, &cfg.commands),
        other => Err(format!(
            "I don't know `atlas plugins {other}`. Try: list, add <file>, approve <id>, revoke <id> <permission>, \
             trust <id> <step>, untrust <id> <step>, off <id>, on <id>, remove <id>, share <id> <person or group>, \
             offers, take <offer>, decline <offer>."
        )),
    };
    match said {
        Ok(s) => println!("{s}"),
        Err(e) => {
            eprintln!("{e}");
            leave(1);
        }
    }
}

/// Hand an add-on to a paired Atlas. It arrives there switched off, marked as
/// coming from you.
/// Hand an add-on to a paired person, or to everyone in a group you can
/// reach. It arrives on their shelf of things shared with them -- never
/// installed by being sent.
fn send_plugin(dir: &std::path::Path, id: &str, to: &str) -> Result<String, String> {
    let (file_name, bytes, name) = atlas::plugins::to_share(dir, id)?;
    let pairings = atlas::kin::Pairings::load(&atlas::kin::where_pairings_live());
    let chats = atlas::chat::Chats::load(&atlas::roots::store());
    let key = to.trim().strip_prefix("the ").unwrap_or(to.trim());
    let key = key.strip_suffix(" group").unwrap_or(key).trim();
    let (people, covering) = match chats.group_named(key) {
        Some(g) => (g.members.clone(), format!("{}{}", atlas::plugins::SHARED_IN, g.name)),
        None => (vec![key.to_string()], format!("the add-on {name}")),
    };
    let named = std::env::temp_dir().join(&file_name);
    std::fs::write(&named, &bytes).map_err(|e| format!("couldn't prepare it: {e}"))?;
    let mut reached = Vec::new();
    for who in &people {
        let Some(contact) = pairings.contacts.iter().find(|c| c.name.eq_ignore_ascii_case(who)).cloned() else { continue };
        let h = atlas::household::share_with_friend(&covering, "me");
        if atlas::kin::PeerLink::from_state(&pairings, &atlas::chat::Chats::default()).sealing_as(atlas::peerkey::Identity::load_or_create(&atlas::kin::where_pairings_live()).ok()).hand_note(&contact.name, &h, Some(&named)).is_ok() {
            reached.push(who.clone());
        }
    }
    let _ = std::fs::remove_file(&named);
    if reached.is_empty() {
        return Err(format!("couldn't reach anyone in {to} to share \"{name}\" with"));
    }
    Ok(format!(
        "Shared \"{name}\" with {}. It waits on their Add-ons page until they choose to add it.",
        reached.join(", ")
    ))
}

fn describe_plugin(p: &atlas::plugins::Plugin) {
    println!("\n{} ({}) -- {}", p.name(), p.id, p.status.plain());
    if let Some(m) = &p.manifest {
        println!("  by {}{}", m.author, if m.description.is_empty() { String::new() } else { format!(": {}", m.description) });
        if m.permissions.is_empty() {
            println!("  asks to do nothing beyond talking back");
        }
        for k in &m.permissions {
            let plain = atlas::plugins::permission(k).map(|x| x.plain).unwrap_or("");
            let mark = if p.granted.contains(k) { "allowed" } else { "asks" };
            println!("  [{mark}] {k}: {plain}");
        }
        for f in &p.flows {
            let starts = if f.triggers.is_empty() { "nothing starts it".to_string() } else { format!("say \"{}\"", f.triggers.join("\" or \"")) };
            println!("  \"{}\" -- {starts}", f.name);
            for s in &f.steps {
                println!("      {}", s.command);
            }
        }
    }
    if let Some(who) = &p.sent_by {
        println!("  sent to you by {who} (your paired Atlas)");
    }
    for q in &p.questions {
        match (&q.always_asks, q.trusted) {
            (Some(why), _) => println!("  asks before \"{}\" every time: {why}", q.command),
            (None, true) => println!("  won't ask before \"{}\" (you said) -- `atlas plugins untrust {} {}`", q.command, p.id, q.command),
            (None, false) => println!("  asks before \"{}\" -- `atlas plugins trust {} {}` to stop that", q.command, p.id, q.command),
        }
    }
    for t in &p.trouble {
        println!("  note: {t}");
    }
    println!("  file fingerprint {}", &p.sha256.get(..16).unwrap_or(""));
}

/// `atlas edits` -- your own edits to the shipped config files, which Atlas
/// keeps through every update, and giving one up.
fn run_edits(args: &[String]) {
    use atlas::yourchanges::{all_kept, forget, shown};
    let dir = atlas::roots::config_dir();
    match args.first().map(|s| s.as_str()).unwrap_or("list") {
        "list" => {
            let (kept, problems) = all_kept(&dir);
            for p in &problems {
                println!("! {p}");
            }
            if kept.is_empty() {
                println!("No edits of yours to the shipped config files. Hand edits are moved here the next time Atlas starts.");
                return;
            }
            for e in &kept {
                let yours = if e.change.removed { "(you removed it)".to_string() } else { shown(e.change.yours.as_ref()) };
                let mut line = format!("{} {}: {}", e.file, e.change.dotted(), yours);
                if e.change.unsure {
                    line.push_str("  [unsure it was you]");
                }
                if e.default_moved() {
                    line.push_str(&format!(
                        "  [the default changed: was {}, now {}]",
                        shown(e.change.was.as_ref()),
                        shown(e.shipped_now.as_ref())
                    ));
                }
                println!("{line}");
            }
            println!("\n`atlas edits revert <file> <setting>` goes back to the shipped default.");
        }
        "revert" => {
            let (file, path) = (args.get(1).cloned().unwrap_or_default(), args.get(2).cloned().unwrap_or_default());
            match forget(&dir, &file, &path) {
                Ok(true) => println!("{file} {path} is back to what Atlas ships, from the next start."),
                Ok(false) => println!("There's no edit of yours at {file} {path}. `atlas edits` lists them."),
                Err(e) => {
                    eprintln!("{e}");
                    leave(1);
                }
            }
        }
        other => {
            eprintln!("I don't know `atlas edits {other}`. Try: list, revert <file> <setting>.");
            leave(1);
        }
    }
}

/// `atlas install-page <Atlas.ipa|Atlas.apk> [--friends] [--minutes N] [--port N]`
/// -- put a phone build on a page the phone installs it from. For an iPhone,
/// this is the only way on without a Mac; for Android it saves emailing the APK.
///
/// The page, its manifest and the app are served on 127.0.0.1 only, under a
/// random path, for `--minutes` (default 20). Tailscale carries them to the
/// phone over HTTPS on port 8443: `serve` (only your own devices, the default)
/// or, with `--friends`, `funnel` (anyone with the link, until the time runs
/// out). Either way it is switched off again at the end.
fn run_install_page(_words: &[String]) {
    use atlas::ota;
    // Read the raw arguments: `main` strips every `--flag` out of the words,
    // which would leave `--minutes 5`'s "5" looking like the file.
    let raw: Vec<String> = std::env::args().skip_while(|a| a != "install-page").skip(1).collect();
    let mut file = None;
    let (mut minutes, mut port, mut public) = (20u64, 9741u16, false);
    let mut it = raw.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--friends" => public = true,
            "--minutes" => minutes = it.next().and_then(|v| v.parse().ok()).unwrap_or(minutes).clamp(1, 240),
            "--port" => port = it.next().and_then(|v| v.parse().ok()).unwrap_or(port),
            other if !other.starts_with("--") => file = Some(other.to_string()),
            other => println!("(ignoring {other})"),
        }
    }
    let Some(file) = file else {
        println!("atlas install-page <Atlas.ipa | Atlas.apk> [--friends] [--minutes 20]");
        println!("  Puts a phone build on a page the phone installs it from, over your Tailscale.");
        println!("  --friends  let a friend's iPhone (not on your tailnet) open it, until the time runs out");
        return;
    };
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => return println!("I can't read {file}: {e}"),
    };
    let package = match ota::Package::read(&bytes) {
        Ok(p) => p,
        Err(e) => return println!("{file} can't go on the page: {e}"),
    };
    let phone = match &package {
        ota::Package::Ios(ipa) => {
            println!("{} {} (build {}), {}", ipa.title, ipa.version, ipa.build, ipa.bundle_id);
            println!("Installs on {} device(s); works until {}.", ipa.devices.len(), ipa.expires.as_deref().unwrap_or("an unknown date"));
            if ipa.devices.is_empty() || ipa.expired(&ota::now_iso()) {
                return println!("This build can't install on anything: {}. Make a new build first.",
                    if ipa.devices.is_empty() { "it lists no devices" } else { "it has expired" });
            }
            "iPhone, in Safari"
        }
        ota::Package::Android(apk) => {
            println!("Atlas for Android, {:.1} MB, signed. SHA-256 {}", apk.bytes_len as f64 / 1048576.0, apk.sha256);
            "Android phone, in Chrome"
        }
    };

    let tool = atlas::phonelink::tailscale_tool();
    let vars = atlas::tools::Vars::new();
    let run = |a: &[String]| atlas::tools::ExternalTool { args: a.to_vec(), ..tool.clone() }.run(&vars, None).map_err(|e| e.to_string());
    let status = run(&["status".into(), "--json".into()]);
    let net = match status.as_ref().ok().and_then(|j| atlas::phonelink::read_status(j)) {
        Some(n) if n.running && !n.dns_name.is_empty() => n,
        Some(n) if n.running => return println!("{}", atlas::phonelink::say(&atlas::phonelink::Serve::NeedsHttps)),
        _ => {
            let why = atlas::phonelink::serve_outcome(status.map(|_| String::new()), "");
            let why = if why == atlas::phonelink::Serve::NeedsHttps { atlas::phonelink::Serve::NotRunning } else { why };
            return println!("{}", atlas::phonelink::say(&why));
        }
    };
    let token = ota::fresh_token();
    let (on, off) = ota::tailscale_args(port, public);
    let url = ota::page_url(&net.dns_name, &token);
    match atlas::phonelink::serve_outcome(run(&on), &url) {
        atlas::phonelink::Serve::Published { .. } => {}
        other => return println!("{}", atlas::phonelink::say(&other)),
    }
    println!();
    println!("Open this on the {phone}{}:", if public { "" } else { " (the phone needs Tailscale on)" });
    println!("  {url}");
    if let Some((n, dark)) = atlas::phonelink::qr_modules(&url) {
        // Two rows per line with half blocks, so the code fits a terminal.
        for y in (0..n).step_by(2) {
            let line: String = (0..n)
                .map(|x| match (dark[y * n + x], y + 1 < n && dark[(y + 1) * n + x]) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                })
                .collect();
            println!("  {line}");
        }
    }
    if public {
        println!("This link works from anywhere until the page closes, in {minutes} minutes. Only send it to people you mean to.");
    }
    println!("Waiting (Ctrl+C stops it early; the Tailscale switch then stays on until `tailscale {} --https=8443 off`).", on[0]);
    let base = format!("https://{}:{}", net.dns_name, ota::HTTPS_PORT);
    let served = ota::serve_install(&bytes, &package, port, &token, Some(&base), minutes, &mut |s: &str| println!("  {s}"));
    let _ = run(&off);
    match served {
        Ok(0) => println!("Closed. No phone downloaded the app."),
        Ok(n) => println!("Closed. The app was downloaded {n} time(s). Tailscale's switch is off again."),
        Err(e) => println!("The page couldn't run: {e}. Tailscale's switch is off again."),
    }
}

/// The name the release key is kept under in the vault.
use atlas::release::RELEASE_KEY_NAME;

/// `atlas release keygen | sign | announce | show` -- the build side of the
/// update courier: the key that signs every release, and the notice that
/// reaches every friend's Atlas through your release channel.
fn run_release(args: &[String]) {
    use atlas::release;
    let state = atlas::roots::install_state();
    let cfg = Config::load(&atlas::roots::config_dir()).ok().and_then(|c| c.tools).map(|t| t.vault).unwrap_or_default();
    let now = atlas::store::now();
    let open_vault = || -> Option<atlas::vault::Vault> {
        let mut vault = atlas::vault::Vault::load(&state);
        if !vault.has_a_passphrase() {
            println!("The release key lives in your vault, and the vault has no passphrase yet. `atlas vault passphrase` first.");
            return None;
        }
        let phrase = ask_quietly("Vault passphrase: ")?;
        if let Err(why) = vault.open(&phrase, now, &cfg) {
            println!("{why}");
            return None;
        }
        Some(vault)
    };
    match args.first().map(|s| s.as_str()).unwrap_or("show") {
        "show" => {
            println!(
                "This build trusts {}.",
                if release::anchor_configured() {
                    format!("the release key {}", release::seed_hex(&release::RELEASE_PUBLIC_KEY))
                } else {
                    "no release key yet -- so it accepts no updates. `atlas release keygen` makes one.".into()
                }
            );
            println!("{}", atlas::update_courier::status(&atlas::roots::store(), now));
        }
        "keygen" => {
            let Some(mut vault) = open_vault() else { return };
            let made = match release::make_release_key(&mut vault, now) {
                Ok(m) => m,
                Err(why) => {
                    println!("{why}");
                    vault.lock();
                    return;
                }
            };
            if !keep(vault.save(&state), "the vault") {
                return;
            }
            vault.lock();
            if let Err(e) = atlas::roots::store().save(release::KEY_CARD, &made.card) {
                println!("(Atlas couldn't keep the key card for the Updates page: {e}. It's printed below; that copy is the one to send.)\n");
            }
            println!("Made your release key. It's in your vault and never leaves it.\n");
            println!("Put these two lines in release-keys.txt before building the copies you hand out:\n");
            println!("{}\n", made.card);
            println!("  ================================================================");
            println!("    YOUR RELEASE RECOVERY KEY -- write this down now, keep it offline");
            println!("  ================================================================\n");
            println!("      {}\n", made.recovery);
            println!("  It is not stored anywhere and won't be shown again. It is the only way to");
            println!("  replace the release key if the vault is lost or the key is stolen -- so it");
            println!("  must never be on this computer.");
        }
        "sign" => {
            // atlas release sign 1.4.0 windows-x86_64=target/atlas.exe macos-aarch64=out/Atlas.dmg
            let Some(version) = args.get(1) else {
                println!("atlas release sign <version> <platform>=<file> [<platform>=<file> ...]");
                return;
            };
            let mut files = Vec::new();
            for a in &args[2..] {
                let Some((platform, path)) = a.split_once('=') else {
                    println!("{a}: write it as <platform>=<file>, e.g. windows-x86_64=atlas.exe");
                    return;
                };
                let bytes = match std::fs::read(path) {
                    Ok(b) => b,
                    Err(e) => {
                        println!("couldn't read {path}: {e}");
                        return;
                    }
                };
                let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                files.push((platform.to_string(), name, bytes));
            }
            let Some(mut vault) = open_vault() else { return };
            let seed = match vault.get(RELEASE_KEY_NAME, now) {
                Ok(s) => release::seed_from_hex(&s),
                Err(why) => {
                    println!("{why} -- `atlas release keygen` makes the key.");
                    return;
                }
            };
            vault.lock();
            let Some(seed) = seed else {
                println!("The release key in your vault is damaged.");
                return;
            };
            let key = release::signing_key_from_seed(&seed);
            if release::anchor_of(&key) != release::RELEASE_PUBLIC_KEY {
                println!(
                    "Warning: this build doesn't carry your release key, so copies built from it would \
                     refuse this release. Bake the key in (see `atlas release keygen`) before handing \
                     out copies."
                );
            }
            // The releaser's own store, where the hub's "Sign and send" keeps it too.
            let seq_store = atlas::roots::store();
            let last: u64 = seq_store.load(release::LAST_SEQUENCE);
            let sequence = last + 1;
            let manifest = match release::manifest_for(
                sequence,
                version,
                atlas::upgrade::DATA_FORMAT,
                now,
                now + 30 * 86_400,
                &files,
            ) {
                Ok(m) => m,
                Err(r) => {
                    println!("{}", r.plain());
                    return;
                }
            };
            let signed = release::seal_manifest(&key, &manifest);
            let out = format!("atlas-release-{version}.json");
            if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&signed).unwrap_or_default()) {
                println!("couldn't write {out}: {e}");
                return;
            }
            // The number is what makes "newer" mean something; if it can't be
            // kept, the next release would reuse it and be refused as already
            // installed. Said, not glossed.
            if let Err(e) = seq_store.save(release::LAST_SEQUENCE, &sequence) {
                println!("Signed {out}, but couldn't record release number {sequence} ({e}). Don't sign another until that's fixed -- it would reuse the number.");
                return;
            }
            // Put aside for friends' Atlases to fetch over their pairings, by
            // fingerprint, once the notice is announced.
            let state = atlas::roots::store().root().to_path_buf();
            for (_, name, bytes) in &files {
                if let Err(e) = atlas::update_courier::keep_for_friends(&state, bytes) {
                    println!("Signed, but {name} couldn't be put aside for friends to fetch: {e}");
                }
            }
            println!("Signed release {sequence} ({version}) for {} platform(s): {out}", files.len());
            println!("Next notice promised within 30 days. `atlas release announce {out}` posts it.");
        }
        "announce" => {
            let Some(path) = args.get(1) else {
                println!("atlas release announce <atlas-release-VERSION.json>");
                return;
            };
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    println!("couldn't read {path}: {e}");
                    return;
                }
            };
            if serde_json::from_str::<release::SignedManifest>(&text).is_err()
                && serde_json::from_str::<release::SignedRotation>(&text).is_err()
            {
                println!("{path} isn't a signed release notice or key change (`atlas release sign` or `rotate` makes one).");
                return;
            }
            let dir = atlas::roots::store().root().join(atlas::update_courier::OUTBOX);
            let dest = dir.join(std::path::Path::new(path).file_name().unwrap_or_default());
            match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&dest, &text)) {
                Ok(()) => println!("Queued. The running Atlas posts it in every release channel you own."),
                Err(e) => println!("couldn't queue it: {e}"),
            }
        }
        // Stop handing out a build, after reading feedback about it.
        "hold" => {
            let Some(v) = args.get(1) else {
                println!("atlas release hold <version>");
                return;
            };
            let store = atlas::roots::store();
            let builds: Vec<String> = atlas::update_apply::failure_reports(&store)
                .into_iter()
                .filter(|r| &r.version == v && !r.sha256.is_empty())
                .map(|r| r.sha256)
                .collect();
            if builds.is_empty() {
                println!("I don't know which build Atlas {v} is -- no failure report names it. Nothing held.");
                return;
            }
            for b in &builds {
                atlas::update_apply::hold_release(&store, b);
            }
            println!("Held: Atlas {v} isn't handed out to anyone until a fixed release replaces it.");
        }
        // A planned change of release key, signed by the key it replaces
        // (spec §20 gap G). Every device that trusts the old key moves to the
        // new one when it hears this in the release channel.
        "rotate" | "recover" => {
            let recovering = args.first().map(|s| s.as_str()) == Some("recover");
            let store = atlas::roots::store();
            let mut installed = release::Installed::load(&store);
            let signer = if recovering {
                println!("Recovery: for when the release key is lost or stolen. Type the recovery key you wrote down.");
                let Some(typed) = ask_quietly("Recovery key: ") else { return };
                let Some(seed) = release::seed_from_hex(&typed.replace([' ', '-'], "")) else {
                    println!("That isn't a recovery key (64 hex characters, spaces allowed).");
                    return;
                };
                let key = release::signing_key_from_seed(&seed);
                if release::anchor_of(&key) != release::RECOVERY_PUBLIC_KEY {
                    println!("That isn't the recovery key this build carries, so devices would refuse it. Nothing changed.");
                    return;
                }
                key
            } else {
                let Some(mut vault) = open_vault() else { return };
                let seed = vault.get(RELEASE_KEY_NAME, now).ok().and_then(|s| release::seed_from_hex(&s));
                vault.lock();
                let Some(seed) = seed else {
                    println!("There's no release key in your vault to change from. `atlas release keygen` makes one.");
                    return;
                };
                release::signing_key_from_seed(&seed)
            };
            // Recovery jumps well past anything a thief may have signed; a
            // planned change is exactly the next one.
            let number = installed.trust.rotations + if recovering { 100 } else { 1 };
            let new_seed = release::new_seed();
            let new_public = release::anchor_of(&release::signing_key_from_seed(&new_seed));
            let rotation = release::Rotation {
                number,
                new_anchor: release::seed_hex(&new_public),
                reason: if recovering { "recovery".into() } else { "planned".into() },
            };
            let signed = release::seal_rotation(&signer, &rotation);
            let Some(mut vault) = open_vault() else { return };
            if let Ok(old) = vault.get(RELEASE_KEY_NAME, now) {
                let _ = vault.put(&format!("{RELEASE_KEY_NAME} (before change {number})"), atlas::vault::Kind::ApiKey, &old, now);
            }
            if let Err(why) = vault.put(RELEASE_KEY_NAME, atlas::vault::Kind::ApiKey, &release::seed_hex(&new_seed), now) {
                println!("{why} Nothing changed.");
                return;
            }
            if !keep(vault.save(&state), "the vault") {
                return;
            }
            vault.lock();
            let out = format!("atlas-rotation-{number}.json");
            if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&signed).unwrap_or_default()) {
                println!("The new key is in your vault, but I couldn't write {out}: {e}");
                return;
            }
            // This device trusts the new key at once; the others when they hear it.
            match release::apply_rotation(&installed.trust, &signed) {
                Ok(t) => {
                    installed.trust = t;
                    if let Err(e) = installed.save(&store) {
                        println!("(This device couldn't record the new key ({e}); it takes it when it hears the announcement.)");
                    }
                }
                Err(r) => println!("(This device didn't take it: {})", r.plain()),
            }
            println!("Made a new release key (change {number}); the old one is kept in your vault, renamed.");
            println!("`atlas release announce {out}` posts the change in your release channels.");
            println!("Bake the new key into src/release.rs before building copies for new devices:\n");
            println!("pub const RELEASE_PUBLIC_KEY: [u8; 32] = {};", release::as_rust_array(&new_public));
        }
        other => println!("I don't know `atlas release {other}`. Try: show, keygen, sign <version> <platform>=<file>..., announce <file>, rotate, recover, hold <version>."),
    }
}

/// `atlas call check [seconds]`: record your microphone and what the laptop
/// plays for a few seconds, and say how loud each was — so you can tell,
/// before a real call, that both sides of call notes can hear. The files are
/// deleted afterwards.
fn run_call_check(args: &[String]) {
    if args.first().map(|s| s.as_str()) == Some("now") {
        match atlas::callwatch::call_now() {
            Some(app) => println!("You're on {app}."),
            None => println!("No call app is holding the microphone."),
        }
        return;
    }
    if args.first().map(|s| s.as_str()) != Some("check") {
        println!("`atlas call check` records a few seconds of both sides to check they work.");
        return;
    }
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5).clamp(1, 60);
    let scratch = atlas::roots::RunScratch::new("atlas-call-check");
    let dir = scratch.path().to_path_buf();
    let mine = dir.join("you.wav");
    let theirs = dir.join("them.wav");
    println!("Recording {secs} seconds of your microphone and of what this laptop plays...");
    let a = atlas::callrec::start(atlas::callrec::Side::Yours, &mine, false);
    let b = atlas::callrec::start(atlas::callrec::Side::Theirs, &theirs, false);
    std::thread::sleep(std::time::Duration::from_secs(secs));
    for (what, r, path) in [("Your microphone", a, &mine), ("What the laptop plays", b, &theirs)] {
        match r {
            Err(e) => println!("  {what}: couldn't record — {e}"),
            Ok(rec) => match rec.finish() {
                Err(e) => println!("  {what}: {e}"),
                Ok(got) => {
                    // Loudest moment, against a floor above hiss: about 3%.
                    let peak = std::fs::read(path)
                        .ok()
                        .map(|b| b.get(44..).unwrap_or(&[]).chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs()).max().unwrap_or(0))
                        .unwrap_or(0);
                    let peak = if peak > 1000 { 1 } else { 0 };
                    println!(
                        "  {what}: {got:.1} seconds recorded, {}",
                        if peak > 0 { "and there was sound in it." } else { "but it was silent." }
                    );
                }
            },
        }
    }
    drop(scratch);
}
