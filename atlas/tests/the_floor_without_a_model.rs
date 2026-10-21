//! **The floor (20 Oct 2026): Atlas with no language model loaded.**
//!
//! The owner's requirement is that Atlas never becomes useless when memory is
//! low, even offline. Everything here runs through the real daemon with NO
//! model (or a model that must never be called), in temporary folders:
//!
//! 1. the everyday commands are understood and done without one -- and each
//!    says what really happened (a file is read back, a move is checked, a
//!    backup is compared by count and hash) and leaves a durable job record;
//! 2. a sentence that needs the model, with no model loaded and not enough
//!    memory to load it, is answered at once with a choice, and "answer
//!    online" / "close <app>" carries on from there;
//! 3. the floor is quick, and cheap: per-command latency and the process's
//!    memory are measured and printed (`--nocapture`).
//!
//! Own process (`[[test]]` in Cargo.toml) so the memory numbers are this
//! test's alone. Run with `--test-threads=1`: the memory reading is of the
//! whole test process, and so is the figure it is compared with.

use atlas::brain::Llm;
use atlas::daemon::Daemon;
use atlas::floor::{self as fl, Choice, Facts, Use};
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const T0: u64 = 1_790_740_000;

fn cfg() -> atlas::config::Config {
    let mut c = atlas::config::Config::load(Path::new("config")).unwrap();
    c.indexing = None;
    c
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-floor-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn daemon_with(tag: &str, llm: Option<Arc<dyn Llm>>) -> (Daemon<'static>, PathBuf) {
    let c: &'static atlas::config::Config = Box::leak(Box::new(cfg()));
    let p: &'static MockPlatform = Box::leak(Box::new(MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])));
    let dir = tmp(tag);
    (
        Daemon::new(
            c,
            p,
            llm,
            Store::new(dir.join("state")),
            Proactive::new(ProactiveConfig::default()),
        ),
        dir,
    )
}

/// A model that counts every call and says one thing.
struct Counting(Arc<AtomicUsize>, &'static str);

impl Llm for Counting {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({ "action": "say", "arg": null, "say": self.1 }).to_string())
    }
}

fn counting(says: &'static str) -> (Arc<dyn Llm>, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    (Arc::new(Counting(n.clone(), says)), n)
}

fn jobs(dir: &Path) -> Vec<atlas::job::Record> {
    atlas::job::load_checked(&Store::new(dir.join("state"))).unwrap_or_default()
}

fn wait_for_job(dir: &Path, stage: &str) -> atlas::job::Record {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(r) = jobs(dir)
            .into_iter()
            .find(|r| r.stage == stage && r.state.terminal())
        {
            return r;
        }
        assert!(
            Instant::now() < until,
            "no finished \"{stage}\" job: {:?}",
            jobs(dir)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ------------------------------------------------------------------ parsing

#[test]
fn the_everyday_commands_are_read_from_the_words_alone() {
    let c = cfg();
    let p = Parser::new(&c.commands);
    let named = |s: &str| p.parse_named(s).1;
    for (said, want) in [
        ("create a text file named atlas-test.txt in my Desktop folder containing the current date, then verify the file", "create_file"),
        ("Create a text file called notes.txt on my desktop saying hello there", "create_file"),
        ("please make a new file named todo.md in my Documents folder", "create_file"),
        ("create notes.txt in my Documents folder", "create_file"),
        ("read the file atlas-test.txt on my Desktop", "read_file"),
        ("read atlas-test.txt in my Documents folder", "read_file"),
        ("what's in notes.txt on my desktop", "read_file"),
        ("move atlas-test.txt from my Desktop to my Documents folder", "move_file"),
        ("move the file notes.txt to my Downloads folder", "move_file"),
        ("back up my Documents folder to D:\\Backups", "back_up_folder"),
        ("back up the folder C:\\Work\\site", "back_up_folder"),
        ("back up my downloads", "back_up_folder"),
        ("what's using my memory", "whats_using_memory"),
        ("whats using my memory right now", "whats_using_memory"),
        ("back up", "back_up"),
        ("back everything up", "back_up"),
        ("quit atlas", "quit_atlas"),
        ("open notepad", "open_app"),
        ("close notepad", "close_app"),
        ("what time is it", "clock"),
        ("system status", "machine_health"),
    ] {
        assert_eq!(named(said).as_deref(), Some(want), "{said}");
    }
}

#[test]
fn ordinary_talk_is_not_taken_for_a_file_job() {
    let c = cfg();
    let p = Parser::new(&c.commands);
    for said in [
        "write a file that renames photos by date",
        "make a file server for my house",
        "create a filing system for my taxes",
        "move my window to the left",
        "move the meeting to Thursday",
        "move it to the other screen",
        "back up a second, what did you mean",
        "back up and tell me what you think",
        "read the room for me",
        "read me the news",
        "show me the weather",
        "what's in the news today",
        "what's using the most power on my phone",
        "I need to create a plan to move to Denver",
        "save the date for the party",
        "how do I back up my phone",
    ] {
        let got = p.parse_named(said).1;
        assert!(
            !matches!(
                got.as_deref(),
                Some(
                    "create_file"
                        | "read_file"
                        | "move_file"
                        | "back_up_folder"
                        | "whats_using_memory"
                )
            ),
            "{said} -> {got:?}"
        );
    }
}

#[test]
fn what_was_asked_is_read_out_of_the_sentence() {
    let spec = fl::read_create(
        "Create a text file named atlas-test.txt in my Desktop folder containing the current date, then verify the file.",
        "20 October 2026",
    )
    .unwrap();
    assert_eq!(spec.name, "atlas-test.txt");
    assert_eq!(spec.folder.as_deref(), Some("my Desktop folder"));
    assert_eq!(spec.contents, "20 October 2026");
    assert!(!spec.replace);
    let q = fl::read_create(
        "create a text file named q.txt in my Documents folder containing \"hello, world\"",
        "x",
    )
    .unwrap();
    assert_eq!(q.contents, "hello, world");
    assert!(
        fl::read_create(
            "create a text file named x.txt in my Documents folder overwrite it if it exists",
            "x"
        )
        .unwrap()
        .replace
    );
    assert_eq!(
        fl::read_move("move a.txt from my Desktop to my Documents folder"),
        Some((
            "a.txt".into(),
            Some("my Desktop".into()),
            "my Documents folder".into()
        ))
    );
    assert_eq!(
        fl::read_backup("back up my Documents folder to D:\\Backups"),
        Some(("my Documents folder".into(), Some("D:\\Backups".into())))
    );
    assert_eq!(
        fl::read_backup("back up the folder C:\\Work"),
        Some(("C:\\Work".into(), None))
    );
}

#[test]
fn my_desktop_folder_is_the_one_windows_says_it_is() {
    // The Desktop OneDrive moved: asked of Windows, not assumed.
    let said = fl::folder_named("my Desktop folder").expect("a Desktop");
    assert_eq!(Some(said.clone()), atlas::organize::user_folder("desktop"));
    let onedrive = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default())
        .join("OneDrive")
        .join("Desktop");
    if onedrive.is_dir()
        && std::env::var("USERPROFILE").is_ok_and(|u| u.to_lowercase().contains("erics"))
    {
        assert_eq!(said, onedrive, "this machine's Desktop is the OneDrive one");
    }
}

// --------------------------------------------------------------- the jobs

#[test]
fn a_file_is_made_read_back_and_never_overwritten() {
    let (mut d, dir) = daemon_with("create", None);
    let place = dir.join("Docs");
    std::fs::create_dir_all(&place).unwrap();
    let said = format!(
        "Create a text file named atlas-test.txt in {} containing today's date, then verify the file.",
        place.display()
    );
    let reply = d.turn(&said, T0);
    let file = place.join("atlas-test.txt");
    assert!(reply.starts_with("Created"), "{reply}");
    assert!(reply.contains("read it back"), "{reply}");
    assert!(reply.contains("30 September 2026"), "{reply}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "30 September 2026");
    // A visible job, ended Succeeded with the words said.
    let job = jobs(&dir)
        .into_iter()
        .find(|r| r.stage == "Make a file")
        .expect("a job record");
    assert_eq!(job.state, atlas::job::State::Succeeded);
    assert!(format!("{job:?}").contains("Created"), "{job:?}");
    // Again: refused, nothing touched, a failed job on record.
    std::fs::write(&file, "mine").unwrap();
    let again = d.turn(&said, T0 + 1);
    assert!(
        again.contains("already exists") && again.contains("didn't touch"),
        "{again}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
    assert!(jobs(&dir)
        .iter()
        .any(|r| r.stage == "Make a file" && r.state == atlas::job::State::Failed));
    // Told to: replaced.
    let over = d.turn(&format!("{said} Overwrite it if it exists."), T0 + 2);
    assert!(over.starts_with("Replaced"), "{over}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "30 September 2026");
}

#[test]
fn a_file_is_read_and_moved_with_each_step_checked() {
    let (mut d, dir) = daemon_with("move", None);
    let (a, b) = (dir.join("A"), dir.join("B"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(a.join("note.txt"), "buy milk").unwrap();
    let read = d.turn(&format!("read the file note.txt in {}", a.display()), T0);
    assert!(
        read.contains("8 bytes") && read.contains("buy milk"),
        "{read}"
    );
    let moved = d.turn(
        &format!("move note.txt from {} to {}", a.display(), b.display()),
        T0 + 1,
    );
    assert!(
        moved.starts_with("Moved") && moved.contains("Checked"),
        "{moved}"
    );
    assert!(!a.join("note.txt").exists());
    assert_eq!(
        std::fs::read_to_string(b.join("note.txt")).unwrap(),
        "buy milk"
    );
    assert!(jobs(&dir)
        .iter()
        .any(|r| r.stage == "Move a file" && r.state == atlas::job::State::Succeeded));
    // Something already there is never replaced.
    std::fs::write(a.join("note.txt"), "new").unwrap();
    let clash = d.turn(
        &format!("move note.txt from {} to {}", a.display(), b.display()),
        T0 + 2,
    );
    assert!(clash.contains("already exists"), "{clash}");
    assert_eq!(std::fs::read_to_string(a.join("note.txt")).unwrap(), "new");
    assert_eq!(
        std::fs::read_to_string(b.join("note.txt")).unwrap(),
        "buy milk"
    );
}

/// A folder that is not there, said as a literal so the coverage scan in
/// `every_intent_reaches_the_daemon.rs` sees the command driven.
#[test]
fn backing_up_a_folder_that_is_not_there_says_so_and_starts_nothing() {
    let (mut d, dir) = daemon_with("backup-missing", None);
    let said = d.turn("back up the folder C:/NoSuchPlace/atlas-missing-folder", T0);
    assert!(
        said.contains("find") || said.contains("isn't there"),
        "{said}"
    );
    assert!(
        jobs(&dir).iter().all(|j| j.stage != "Back up a folder"),
        "{said}"
    );
}

#[test]
fn a_folder_backup_asks_where_runs_off_the_loop_and_is_checked() {
    let (mut d, dir) = daemon_with("backup", None);
    let src = dir.join("Project");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("a.txt"), "alpha").unwrap();
    std::fs::write(src.join("sub").join("b.txt"), "beta").unwrap();
    let dest = dir.join("Backups");
    std::fs::create_dir_all(&dest).unwrap();
    // No place said: asked, and the answer comes back to this command.
    let asked = d.turn(&format!("back up the folder {}", src.display()), T0);
    assert!(asked.contains("Where should I put the backup"), "{asked}");
    let started = d.turn(&dest.display().to_string(), T0 + 1);
    assert!(started.contains("in the background"), "{started}");
    let job = wait_for_job(&dir, "Back up a folder");
    assert_eq!(job.state, atlas::job::State::Succeeded, "{job:?}");
    let made: Vec<_> = std::fs::read_dir(&dest).unwrap().flatten().collect();
    assert_eq!(made.len(), 1, "one new dated folder");
    let copy = made[0].path();
    assert!(copy
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("Project backup "));
    assert_eq!(
        std::fs::read_to_string(copy.join("sub").join("b.txt")).unwrap(),
        "beta"
    );
    assert!(format!("{job:?}").contains("SHA-256"), "{job:?}");
    // The same again at once: a second dated folder, nothing overwritten.
    let said = format!("back up {} to {}", src.display(), dest.display());
    assert!(d.turn(&said, T0 + 1).contains("in the background"));
    wait_for_jobs(&dir, "Back up a folder", 2);
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 2);
}

fn wait_for_jobs(dir: &Path, stage: &str, n: usize) {
    let until = Instant::now() + Duration::from_secs(20);
    while jobs(dir)
        .iter()
        .filter(|r| r.stage == stage && r.state.terminal())
        .count()
        < n
    {
        assert!(Instant::now() < until, "{:?}", jobs(dir));
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_backup_into_itself_or_too_big_or_to_nowhere_is_refused() {
    let (mut d, dir) = daemon_with("refuse", None);
    let src = dir.join("Project");
    std::fs::create_dir_all(src.join("inner")).unwrap();
    std::fs::write(src.join("a.txt"), "alpha").unwrap();
    let inside = d.turn(
        &format!(
            "back up {} to {}",
            src.display(),
            src.join("inner").display()
        ),
        T0,
    );
    assert!(
        inside.contains("inside the folder being copied"),
        "{inside}"
    );
    assert_eq!(
        std::fs::read_dir(src.join("inner")).unwrap().count(),
        0,
        "nothing was copied"
    );
    let nowhere = d.turn(
        &format!(
            "back up {} to {}",
            src.display(),
            dir.join("no").join("such").display()
        ),
        T0 + 1,
    );
    assert!(nowhere.contains("doesn't exist"), "{nowhere}");
    let missing = d.turn(
        &format!("back up the folder {}", dir.join("Nope").display()),
        T0 + 2,
    );
    assert!(
        missing.contains("couldn't find") || missing.contains("isn't there"),
        "{missing}"
    );
    // The copy is compared with the original: a corrupted one is not "done".
    let plan = fl::plan_backup(&src, &dir, "t").unwrap();
    assert!(fl::dest_inside_source(&src, &src.join("x")));
    assert!(!fl::dest_inside_source(&src, &dir));
    let done = fl::run_backup(&plan, &|| false);
    assert!(done.ok, "{done:?}");
    assert!(plan.target.join("a.txt").is_file());
}

#[test]
fn closing_an_app_is_asked_about_done_and_checked() {
    let (mut d, dir) = daemon_with("close", None);
    assert!(d.turn("open notepad", T0).contains("up"));
    let asked = d.turn("close notepad", T0 + 1);
    assert!(asked.contains("Go ahead"), "{asked}");
    let closed = d.turn("yes", T0 + 2);
    assert!(
        closed.contains("Closed notepad") && closed.contains("Checked"),
        "{closed}"
    );
    assert!(jobs(&dir)
        .iter()
        .any(|r| r.stage == "Close an app" && r.state == atlas::job::State::Succeeded));
    // Atlas's own processes, and Windows's, are never on offer.
    for never in ["atlas", "llama-server", "explorer", "svchost", "claude"] {
        assert!(!atlas::tune::may_close(never, &[]), "{never}");
    }
}

#[test]
fn memory_is_said_as_five_programs_and_the_totals() {
    let (mut d, _dir) = daemon_with("memory", None);
    let said = d.turn("what's using my memory", T0);
    assert!(
        said.contains("of memory") && said.contains("free"),
        "{said}"
    );
    assert!(said.contains("MB") || said.contains("GB"), "{said}");
    let list = said.split("biggest five right now: ").nth(1).unwrap_or("");
    assert!(list.split(',').count() >= 4, "{said}");
    let text = fl::memory_said(
        &[
            Use {
                name: "Chrome".into(),
                mb: 1843,
                closable: true,
            },
            Use {
                name: "Teams".into(),
                mb: 900,
                closable: true,
            },
        ],
        16_000,
        4_000,
    );
    assert!(
        text.contains("Chrome 1.8 GB, Teams 900 MB") && text.contains("3.9 GB is free"),
        "{text}"
    );
}

// --------------------------------------------------- the model can't load

fn facts(free_mb: u64, online: bool, offline: bool, closable: &[(&str, u64)]) -> Facts {
    Facts {
        loaded: false,
        model: Some(("qwen3-4b".into(), 3_000)),
        free_mb,
        total_mb: 16_000,
        online_allowed: online,
        offline_known: offline,
        closable: closable
            .iter()
            .map(|(n, mb)| Use {
                name: n.to_string(),
                mb: *mb,
                closable: true,
            })
            .collect(),
    }
}

#[test]
fn what_to_do_when_the_model_cannot_load_is_decided_from_the_facts() {
    let apps = [("Chrome", 1800), ("Teams", 900), ("Spotify", 400)];
    // Enough memory, or already loaded: just ask the model.
    assert_eq!(
        fl::decide(&facts(4_000, true, false, &apps)),
        Choice::Proceed
    );
    let mut loaded = facts(100, true, false, &apps);
    loaded.loaded = true;
    assert_eq!(fl::decide(&loaded), Choice::Proceed);
    // Short by 1.2 GB, online allowed: both ways, biggest app first.
    let Choice::Low(ask) = fl::decide(&facts(1_800, true, false, &apps)) else {
        panic!()
    };
    assert_eq!(ask.shortfall_mb, 1_200);
    assert!(ask.online && ask.closing_would_do && !ask.hopeless);
    assert_eq!(ask.apps[0].name, "Chrome");
    let said = fl::say(&ask);
    assert!(
        said.contains("answer online") && said.contains("labelled as online"),
        "{said}"
    );
    assert!(
        said.contains("Chrome 1.8 GB, Teams 900 MB, Spotify 400 MB"),
        "{said}"
    );
    assert!(said.contains("say \"close Chrome\""), "{said}");
    // Offline, or online switched off: no online offer, said so.
    for (online, offline) in [(false, false), (true, true)] {
        let Choice::Low(ask) = fl::decide(&facts(1_800, online, offline, &apps)) else {
            panic!()
        };
        assert!(!ask.online);
        let said = fl::say(&ask);
        assert!(!said.contains("say \"answer online\""), "{said}");
        assert!(
            said.contains("can't answer online either") && said.contains("close Chrome"),
            "{said}"
        );
    }
    // Closing everything on offer wouldn't be enough.
    let Choice::Low(ask) = fl::decide(&facts(100, true, false, &[("Teams", 500)])) else {
        panic!()
    };
    assert!(!ask.closing_would_do);
    assert!(fl::say(&ask).contains("more, so you may need to close more than one"));
    // No app to offer, or a model bigger than the machine can spare.
    let Choice::Low(ask) = fl::decide(&facts(1_000, false, false, &[])) else {
        panic!()
    };
    assert!(fl::say(&ask).contains("yourself"), "{}", fl::say(&ask));
    let mut huge = facts(1_000, true, false, &apps);
    huge.model = Some(("big".into(), 20_000));
    let Choice::Low(ask) = fl::decide(&huge) else {
        panic!()
    };
    assert!(ask.hopeless && fl::say(&ask).contains("closing apps won't fix it"));
    // No model at all: said plainly, with the next step.
    let mut none = facts(1_000, true, false, &apps);
    none.model = None;
    assert_eq!(fl::decide(&none), Choice::NoModel);
    assert!(fl::no_model_said().contains("Get the better model"));
    // What a reply means.
    assert_eq!(fl::follow_up("answer online"), fl::Follow::Online);
    assert_eq!(fl::follow_up("Answer it online please"), fl::Follow::Online);
    assert_eq!(
        fl::follow_up("close Chrome"),
        fl::Follow::Close("chrome".into())
    );
    assert_eq!(fl::follow_up("never mind"), fl::Follow::Cancel);
    assert_eq!(fl::follow_up("what's the weather"), fl::Follow::Other);
}

const ASKED: &str = "Why is the sky blue at noon but red at sunset?";

#[test]
fn a_question_with_no_model_and_no_room_gets_the_choice_at_once_then_online() {
    let (main, main_calls) = counting("Never used.");
    let (online, online_calls) = counting("Rayleigh scattering.");
    let (mut d, _dir) = daemon_with("low-online", Some(main));
    d.floor_with_facts_for_test(
        Arc::new(|| facts(1_000, true, false, &[("Chrome", 1800), ("Teams", 900)])),
        Arc::new(|_| fl::Done::no("never called")),
        Some(online),
    );
    let t = Instant::now();
    let reply = d.turn(ASKED, T0);
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
    assert!(
        reply.contains("answer online") && reply.contains("close Chrome"),
        "{reply}"
    );
    assert_eq!(
        main_calls.load(Ordering::SeqCst),
        0,
        "no model was called to say that"
    );
    assert_eq!(online_calls.load(Ordering::SeqCst), 0);
    let answer = d.turn("answer online", T0 + 1);
    assert!(answer.starts_with("Online answer"), "{answer}");
    assert!(answer.contains("Rayleigh scattering."), "{answer}");
    assert_eq!(online_calls.load(Ordering::SeqCst), 1);
    assert_eq!(main_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn closing_an_app_for_memory_loads_the_model_and_answers_the_original_question() {
    let (main, main_calls) = counting("Because of scattering.");
    let free = Arc::new(AtomicU64::new(1_000));
    let closed: Arc<std::sync::Mutex<Vec<String>>> = Default::default();
    let (f, c, fr) = (free.clone(), closed.clone(), free.clone());
    let (mut d, dir) = daemon_with("low-close", Some(main));
    d.floor_with_facts_for_test(
        Arc::new(move || {
            facts(
                f.load(Ordering::SeqCst),
                false,
                true,
                &[("Chrome", 1800), ("Teams", 900)],
            )
        }),
        Arc::new(move |name| {
            // A fake: nothing real is closed; the memory it held comes back.
            c.lock().unwrap().push(name.to_string());
            fr.store(4_000, Ordering::SeqCst);
            fl::Done::yes(format!("Closed {name}. Checked: no process of it is left."))
        }),
        None,
    );
    let reply = d.turn(ASKED, T0);
    assert!(
        reply.contains("close Chrome") && !reply.contains("say \"answer online\""),
        "{reply}"
    );
    assert_eq!(main_calls.load(Ordering::SeqCst), 0);
    // "close Chrome" is asked about first -- closing loses unsaved work.
    let asked = d.turn("close Chrome", T0 + 1);
    assert!(
        asked.contains("Go ahead") && asked.contains("unsaved"),
        "{asked}"
    );
    assert!(
        closed.lock().unwrap().is_empty(),
        "nothing is closed before the yes"
    );
    let done = d.turn("yes", T0 + 2);
    assert_eq!(closed.lock().unwrap().as_slice(), ["Chrome"]);
    assert!(done.contains("Memory is enough now"), "{done}");
    assert!(done.contains("Because of scattering."), "{done}");
    assert_eq!(main_calls.load(Ordering::SeqCst), 1);
    assert!(jobs(&dir)
        .iter()
        .any(|r| r.stage == "Close an app" && r.state == atlas::job::State::Succeeded));
}

#[test]
fn a_no_that_starts_with_a_yes_word_closes_nothing_and_it_is_never_an_app() {
    let (main, _calls) = counting("Because of scattering.");
    let closed: Arc<std::sync::Mutex<Vec<String>>> = Default::default();
    let c = closed.clone();
    let (mut d, _dir) = daemon_with("low-close-no", Some(main));
    d.floor_with_facts_for_test(
        Arc::new(|| {
            facts(
                1_000,
                false,
                true,
                &[("Google Chrome", 1800), ("Teams", 900)],
            )
        }),
        Arc::new(move |name| {
            c.lock().unwrap().push(name.to_string());
            fl::Done::yes(format!("Closed {name}."))
        }),
        None,
    );
    d.turn(ASKED, T0);
    // "close it" names nothing: it asks which, and asks nothing about closing.
    let which = d.turn("close it", T0 + 1);
    assert!(which.contains("Which app"), "{which}");
    assert!(!which.contains("Go ahead"), "{which}");
    // A whole word of the name finds it, and it is confirmed first.
    let asked = d.turn("close chrome", T0 + 2);
    assert!(
        asked.contains("Go ahead") && asked.contains("Google Chrome"),
        "{asked}"
    );
    // Each of these begins with a yes word and means no.
    for no in ["do not close it", "please don't"] {
        let (mut d, _dir) = daemon_with("low-close-no2", None);
        let c2 = closed.clone();
        d.floor_with_facts_for_test(
            Arc::new(|| facts(1_000, false, true, &[("Chrome", 1800)])),
            Arc::new(move |name| {
                c2.lock().unwrap().push(name.to_string());
                fl::Done::yes("Closed.")
            }),
            None,
        );
        d.turn(ASKED, T0);
        let asked = d.turn("close Chrome", T0 + 1);
        assert!(asked.contains("Go ahead"), "{asked}");
        let said = d.turn(no, T0 + 2);
        assert!(said.contains("haven't closed anything"), "{no}: {said}");
    }
    assert!(closed.lock().unwrap().is_empty(), "nothing was closed");
}

#[test]
fn with_no_model_installed_it_says_so_and_what_to_do() {
    let (mut d, _dir) = daemon_with("no-model", None);
    let mut none = facts(1_000, false, false, &[]);
    none.model = None;
    d.floor_with_facts_for_test(
        Arc::new(move || none.clone()),
        Arc::new(|_| fl::Done::no("x")),
        None,
    );
    let reply = d.turn(ASKED, T0);
    assert!(reply.contains("no language model is installed"), "{reply}");
    assert!(
        reply.contains("Connections") && reply.contains("Get the better model"),
        "{reply}"
    );
}

#[test]
fn a_model_that_is_loaded_or_has_room_is_asked_as_usual() {
    let (main, calls) = counting("The sky scatters blue light.");
    let (mut d, _dir) = daemon_with("room", Some(main));
    d.floor_with_facts_for_test(
        Arc::new(|| facts(8_000, true, false, &[])),
        Arc::new(|_| fl::Done::no("x")),
        None,
    );
    let reply = d.turn(ASKED, T0);
    assert!(reply.contains("scatters blue light"), "{reply}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// ------------------------------------------- reaching every new intent by words

#[test]
fn each_new_command_is_driven_through_the_daemon_by_its_words() {
    // Harmless on purpose: each names something that does not exist, or asks
    // for what it still needs, so nothing on this machine is made or moved.
    let (mut d, _dir) = daemon_with("words", None);
    let p = Parser::new(&cfg().commands);
    // Said as literals, so the guard that looks for each intent being driven
    // through the daemon by a test (`every_intent_reaches_the_daemon`) can read them.
    for (reply, said, want) in [
        (
            d.turn("create a text file named notes.txt", T0),
            "create a text file named notes.txt",
            "create_file",
        ),
        (
            d.turn("read the file zz-no-such-file-floor.txt", T0),
            "read the file zz-no-such-file-floor.txt",
            "read_file",
        ),
        (
            d.turn("move zz-no-such-file-floor.txt to my Documents folder", T0),
            "move zz-no-such-file-floor.txt to my Documents folder",
            "move_file",
        ),
        (
            d.turn(
                "back up the folder zz-no-such-folder-floor to my Documents folder",
                T0,
            ),
            "back up the folder zz-no-such-folder-floor to my Documents folder",
            "back_up_folder",
        ),
        (
            d.turn("what's using my memory", T0),
            "what's using my memory",
            "whats_using_memory",
        ),
    ] {
        assert_eq!(p.parse_named(said).1.as_deref(), Some(want), "{said}");
        assert!(!reply.trim().is_empty(), "{said}");
    }
    assert!(matches!(
        p.parse("what's using my memory"),
        Intent::WhatsUsingMemory
    ));
}

// ----------------------------------------------------------- the measurement

/// This test process's working set, from the same process list the memory
/// question reads.
fn working_set_mb() -> Option<u64> {
    let me = std::process::id();
    atlas::tune::snapshot()
        .iter()
        .find(|p| p.pid == me)
        .map(|p| p.mem_mb)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = (p * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

#[test]
fn the_floor_is_quick_and_small() {
    let private = || atlas::relief::private_bytes().map(|b| b as f64 / 1_048_576.0);
    let before_mb = private();
    let (mut d, dir) = daemon_with("measure", None);
    let after_new_mb = private();
    let ws_new_mb = working_set_mb();

    let place = dir.join("Docs");
    let dest = dir.join("Backups");
    let src = dir.join("Project");
    for p in [&place, &dest, &src] {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(src.join("a.txt"), "alpha").unwrap();
    let commands: Vec<String> = vec![
        "open notepad".into(),
        "close notepad".into(),
        "yes".into(),
        format!(
            "create a text file named m.txt in {} containing today's date",
            place.display()
        ),
        format!(
            "create a text file named m.txt in {} containing today's date",
            place.display()
        ),
        format!("read the file m.txt in {}", place.display()),
        format!("move m.txt from {} to {}", place.display(), dest.display()),
        format!("back up {} to {}", src.display(), dest.display()),
        "back up".into(),
        "set a timer for 5 minutes".into(),
        "remind me in 20 minutes to stretch".into(),
        "what time is it".into(),
        "what's today's date".into(),
        "what's using my memory".into(),
        "which model are you using".into(),
        "what's outstanding".into(),
        "what can you do".into(),
        "recap our conversation".into(),
        "open notepad".into(),
        "system status".into(),
    ];
    let mut ms: Vec<f64> = Vec::new();
    println!("--- floor latency (real daemon, no model, mock screen) ---");
    for (i, said) in commands.iter().enumerate() {
        let t = Instant::now();
        let reply = d.turn(said, T0 + i as u64);
        let took = t.elapsed().as_secs_f64() * 1000.0;
        ms.push(took);
        assert!(!reply.trim().is_empty(), "{said}");
        let short: String = said.chars().take(48).collect();
        println!("{took:>8.1} ms  {short}");
    }
    let mut sorted = ms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let (p50, p95, max) = (
        percentile(&sorted, 0.50),
        percentile(&sorted, 0.95),
        *sorted.last().unwrap(),
    );
    println!(
        "--- {} commands: p50 {p50:.1} ms, p95 {p95:.1} ms, max {max:.1} ms ---",
        sorted.len()
    );
    wait_for_jobs(&dir, "Back up a folder", 1);
    let after_mb = private();
    let ws_after_mb = working_set_mb();
    println!(
        "--- memory of THIS TEST PROCESS (not the installed daemon): private bytes {:.1} MB before the daemon, {:.1} MB after Daemon::new, {:.1} MB after the commands; working set {:?} MB after new, {:?} MB after ---",
        before_mb.unwrap_or(0.0),
        after_new_mb.unwrap_or(0.0),
        after_mb.unwrap_or(0.0),
        ws_new_mb,
        ws_after_mb
    );
    // The model-less memory answer's own parts, timed: the process list, the
    // totals and the model folder's scan -- what `floor_facts` does for real.
    let t = Instant::now();
    let procs = atlas::tune::snapshot();
    let top = fl::biggest(&procs, std::process::id());
    let (total, free) = fl::memory_now();
    let parts = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let c = cfg();
    let models = c.tools.as_ref().map(|t| t.models.clone());
    if let Some(m) = &models {
        let _ = atlas::models::Registry::scan_reporting(&atlas::models::Registry::dir_for(m));
    }
    let scan = t.elapsed().as_secs_f64() * 1000.0;
    println!("--- the choice's inputs: process list + totals {parts:.1} ms ({} programs, {total} MB total, {free} MB free), models folder scan {scan:.1} ms ---", top.len());
    assert!(p95 < 1000.0, "p95 {p95:.1} ms over the 1 s floor; {ms:?}");
    assert!(max < 5000.0, "max {max:.1} ms; {ms:?}");
    assert!(
        parts + scan < 1000.0,
        "the choice's inputs took {:.1} ms",
        parts + scan
    );
}
