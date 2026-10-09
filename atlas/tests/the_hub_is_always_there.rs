//! The hub is there whenever Eric opens it (round 4, 28 Sep 2026).
//!
//! Eric's rule: Atlas runs in the background on Windows with nothing open,
//! and the hub -- opened from the icon by the clock or a bookmark -- must
//! always answer. Round 3's scan found seven ways it didn't, or held up
//! everything else while it waited:
//!
//! 1. A hub port taken at start meant no hub for the whole session, said
//!    nowhere (`server::open_hub`).
//! 2. "Is Atlas running?" asked only whether something answered on the port
//!    (`firstlaunch::atlas_running`, `server::ping`).
//! 3. An update restart left the old icon by the clock beside the new one
//!    (`notifyicon::take_icon_away`).
//! 4. Hub saves that failed said nothing (`hublive`).
//! 5. The file index was read before the hub could answer (`index::Loading`).
//! 6. Reading and unpacking a file the model chose ran on the loop
//!    (`Daemon::file_work_off_the_loop`).
//! 7. Setup never fetched the voice cut-in model (`getpieces::catalogue`).

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::{Action, Door, Retry, ServerConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-hubthere-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn scfg(port: u16) -> ServerConfig {
    ServerConfig { enabled: true, port, ..ServerConfig::default() }
}

const TOKEN: &str = "abcdefghijklmnopqrstuvwx";

fn quick() -> Retry {
    Retry {
        first: Duration::from_millis(20),
        longest: Duration::from_millis(80),
        fall_back_after: Duration::from_millis(600),
        look_again_every: Duration::from_millis(100),
    }
}

/// A port nothing is on right now, held by a plain listener (another
/// program) until it's dropped.
fn taken_port() -> (std::net::TcpListener, u16) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let p = l.local_addr().unwrap().port();
    (l, p)
}

fn until(max: Duration, mut ok: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + max;
    while Instant::now() < end {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    ok()
}

fn opened_on() -> (Arc<Mutex<Vec<u16>>>, Box<dyn Fn(u16) + Send>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    (seen, Box::new(move |p| s.lock().unwrap().push(p)))
}

// ================= 1. a port that's taken is waited for =================

#[test]
fn a_hub_whose_port_is_free_opens_at_once_and_says_which_atlas_it_is() {
    let (l, port) = taken_port();
    drop(l);
    let (seen, on_open) = opened_on();
    let door = atlas::server::open_hub(&scfg(port), TOKEN, "atlas-one", Vec::new(), quick(), on_open).unwrap();
    assert!(door.is_open());
    assert_eq!(door.port(), port);
    assert_eq!(*seen.lock().unwrap(), vec![port]);
    assert_eq!(atlas::server::ping(port, Duration::from_secs(1)).as_deref(), Some("atlas-one"));
}

#[test]
fn a_taken_port_is_tried_again_until_it_frees() {
    let (held, port) = taken_port();
    let (seen, on_open) = opened_on();
    let door = atlas::server::open_hub(&scfg(port), TOKEN, "atlas-two", Vec::new(), quick(), on_open).unwrap();
    // Not open yet, and not an error: it said so, and it's trying.
    assert!(!door.is_open());
    assert_eq!(door.port(), 0);
    let news = door.take_news().join(" ");
    assert!(news.contains("taken") && news.contains("trying again"), "{news}");
    assert!(seen.lock().unwrap().is_empty());
    // The other program lets go (an old Atlas on its way out, say).
    std::thread::sleep(Duration::from_millis(100));
    drop(held);
    assert!(until(Duration::from_secs(3), || door.is_open()), "never opened once the port was free");
    assert_eq!(door.port(), port, "opened on its own port, so the bookmark works");
    assert_eq!(*seen.lock().unwrap(), vec![port]);
    assert_eq!(atlas::server::ping(port, Duration::from_secs(1)).as_deref(), Some("atlas-two"));
    assert!(door.take_news().join(" ").contains("usual port"));
}

#[test]
fn a_port_that_stays_taken_is_left_for_the_next_one_and_taken_back_when_it_frees() {
    let (held, port) = taken_port();
    let (seen, on_open) = opened_on();
    let door = atlas::server::open_hub(&scfg(port), TOKEN, "atlas-three", Vec::new(), quick(), on_open).unwrap();
    assert!(until(Duration::from_secs(5), || door.is_open()), "never fell back");
    let fell_to = door.port();
    assert_ne!(fell_to, port);
    assert_ne!(fell_to, 0);
    assert_eq!(*seen.lock().unwrap(), vec![fell_to], "the real port is handed on");
    assert_eq!(atlas::server::ping(fell_to, Duration::from_secs(1)).as_deref(), Some("atlas-three"));
    let news = door.take_news().join(" ");
    assert!(news.contains(&format!("port {fell_to}")) && news.contains("still taken"), "said plainly: {news}");

    // A request through the fallback reaches the daemon's side as usual.
    let asked = std::thread::spawn(move || {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", fell_to)).unwrap();
        use std::io::{Read, Write};
        s.write_all(format!("GET /hub HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {TOKEN}\r\n\r\n").as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    });
    let mut answered = 0;
    let end = Instant::now() + crate::common::allowed(Duration::from_secs(5));
    while answered == 0 && Instant::now() < end {
        answered += door.wait_and_answer(50, &mut |_a: Action| atlas::server::Reply::html("<h1>hub</h1>")).len();
    }
    assert!(answered > 0, "nothing reached the loop through the fallback port");
    let _ = asked.join();

    // The other program lets go: the usual address answers again too.
    drop(held);
    assert!(until(Duration::from_secs(3), || door.port() == port), "the usual port was never taken back");
    assert_eq!(atlas::server::ping(port, Duration::from_secs(1)).as_deref(), Some("atlas-three"));
    assert_eq!(atlas::server::ping(fell_to, Duration::from_secs(1)).as_deref(), Some("atlas-three"), "and the fallback still answers");
    assert_eq!(seen.lock().unwrap().last(), Some(&port));
    assert!(door.take_news().join(" ").contains("bookmark works again"));
}

#[test]
fn a_switched_off_hub_is_still_an_error_at_once_not_a_wait() {
    let (_, on_open) = opened_on();
    let off = ServerConfig { enabled: false, ..scfg(0) };
    let started = Instant::now();
    assert!(atlas::server::open_hub(&off, TOKEN, "x", Vec::new(), Retry::default(), on_open).is_err());
    crate::common::assert_prompt(started.elapsed(), Duration::from_secs(1), "took too long");
}

#[test]
fn the_daemon_writes_the_hubs_real_port_down_and_tells_the_icon() {
    let main = crate::common::source_of("main");
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let body = main.split_once("fn run_daemon(").unwrap().1.split("\n}\n").next().unwrap().to_string();
    assert!(body.contains("atlas::server::open_hub("), "the hub isn't opened by open_hub");
    assert!(body.contains("atlas::server::record_door("), "the real port isn't written down");
    assert!(body.contains("atlas::notifyicon::tray_hub_address("), "the icon isn't told");
    assert!(body.contains("atlas::notifyicon::tray_hub_note("), "the icon doesn't say it moved");
    // `atlas hub` and the phone link ask for the real port.
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let hub = main.split_once("fn run_hub_address(").unwrap().1.split("\n}\n").next().unwrap().to_string();
    assert!(hub.contains("atlas::server::hub_port("), "`atlas hub` prints the configured port only");
    // The loop writes the door's news in the log.
    let daemon = crate::common::source_of("daemon");
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let answer = daemon.split_once("fn answer_hub_saying(").unwrap().1.split("\n    }\n").next().unwrap().to_string();
    assert!(answer.contains("take_news()"), "the door's news never reaches the log");
}

// ================= 2. "is Atlas running?" =================

#[test]
fn the_hubs_port_is_believed_only_when_atlas_itself_answers_there() {
    let root = tmp("door");
    let state = root.join("data").join("state");
    assert_eq!(atlas::server::atlas_hub_port(&state), None, "nothing recorded");
    // Another program on the recorded port.
    let (other, port) = taken_port();
    atlas::server::record_door(&state, &Door { port, id: "mine".into() }).unwrap();
    assert_eq!(atlas::server::atlas_hub_port(&state), None, "a program that isn't Atlas");
    assert_eq!(atlas::server::hub_port(&state, 8787), 8787, "the setting when Atlas isn't answering");
    drop(other);
    // Atlas's hub, answering as someone else's Atlas.
    let (_, on_open) = opened_on();
    let door = atlas::server::open_hub(&scfg(0), TOKEN, "not-mine", Vec::new(), quick(), on_open).unwrap();
    atlas::server::record_door(&state, &Door { port: door.port(), id: "mine".into() }).unwrap();
    assert_eq!(atlas::server::atlas_hub_port(&state), None);
    // Ours.
    atlas::server::record_door(&state, &Door { port: door.port(), id: "not-mine".into() }).unwrap();
    assert_eq!(atlas::server::atlas_hub_port(&state), Some(door.port()));
    assert_eq!(atlas::server::hub_port(&state, 8787), door.port());
}

#[test]
fn running_is_the_lock_so_a_switched_off_hub_is_still_atlas_and_a_stranger_on_the_port_is_not() {
    let root = tmp("running");
    let lock = atlas::onlyone::OnlyOne::at(&root.join("data"));
    // Something else holds the hub's port, and no Atlas is running: "Open
    // Atlas" must start one.
    let (_other, port) = taken_port();
    atlas::server::record_door(&root.join("data").join("state"), &Door { port, id: "gone".into() }).unwrap();
    assert!(!atlas::firstlaunch::atlas_running(&root));
    let opening = atlas::firstlaunch::what_opening_does(true, atlas::firstlaunch::atlas_running(&root), atlas::firstlaunch::First::Home);
    assert!(opening.start_background, "another program on the port kept Atlas from starting");
    // An Atlas with its hub switched off: running.
    lock.take(atlas::store::now()).unwrap();
    assert!(atlas::firstlaunch::atlas_running(&root));
    let opening = atlas::firstlaunch::what_opening_does(true, atlas::firstlaunch::atlas_running(&root), atlas::firstlaunch::First::Home);
    assert!(!opening.start_background, "a second Atlas would be started");
    lock.release();
}

#[test]
fn setups_restart_waits_for_the_lock_to_be_let_go_not_for_the_port() {
    let root = tmp("restart");
    let data = root.join("data");
    let lock = atlas::onlyone::OnlyOne::at(&data);
    lock.take(atlas::store::now()).unwrap();
    // A stand-in Atlas: no hub at all (switched off). It sees the stop
    // request, saves for a moment, and lets go of its lock last.
    let stop = atlas::goodbye::stop_file(&data.join("state"));
    let watcher = {
        let (stop, lock) = (stop.clone(), lock.clone());
        std::thread::spawn(move || {
            let end = Instant::now() + crate::common::allowed(Duration::from_secs(5));
            while !stop.exists() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::thread::sleep(Duration::from_millis(300));
            let _ = std::fs::remove_file(&stop);
            lock.release();
        })
    };
    let started = Instant::now();
    assert!(atlas::firstlaunch::ask_atlas_to_stop(&root, Duration::from_secs(5)), "it stopped, and Setup didn't see it");
    assert!(started.elapsed() >= Duration::from_millis(250), "didn't wait for the saving to finish: {:?}", started.elapsed());
    watcher.join().unwrap();
    assert!(!atlas::firstlaunch::atlas_running(&root));
}

#[test]
fn setup_and_its_window_ask_the_install_not_the_port() {
    let setup = std::fs::read_to_string("src/setupwin.rs").unwrap();
    assert!(!setup.contains("atlas_running(place.port)") && !setup.contains("atlas_running(self.place.port)"));
    assert!(setup.contains("firstlaunch::hub_port_at("), "the window keeps the configured port when the hub moved");
    let main = crate::common::source_of("main");
    assert!(!main.contains("atlas_running(port)"));
}

// ================= 3. the icon by the clock =================

#[test]
fn the_icons_words_say_when_the_hub_moved_and_fit_windows() {
    assert_eq!(atlas::notifyicon::tray_tip(false, None), "Atlas — running");
    let t = atlas::notifyicon::tray_tip(false, Some("Hub on port 8788 (its usual 8787 is taken)"));
    assert!(t.starts_with("Atlas — running\n") && t.contains("8788"), "{t}");
    let long = "x".repeat(400);
    assert!(atlas::notifyicon::tray_tip(true, Some(&long)).encode_utf16().count() <= 127);
    atlas::notifyicon::tray_hub_address("http://127.0.0.1:8788/hub?t=abc");
    assert_eq!(atlas::notifyicon::tray_hub_now(), "http://127.0.0.1:8788/hub?t=abc");
}

#[test]
fn an_update_restart_takes_the_old_icon_away_before_the_new_one_starts() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("restart-icon")), Proactive::new(ProactiveConfig::default()));
    let before = atlas::notifyicon::icon_taken_away_count_for_test();
    let mut taken_when_started = None;
    d.restart_as(|| {
        taken_when_started = Some(atlas::notifyicon::icon_taken_away_count_for_test());
        Ok(())
    })
    .unwrap();
    assert!(taken_when_started.unwrap() > before, "the new copy started while the old icon was still up");
    // The Win32 half: removed by NIM_DELETE from whichever thread is leaving.
    let src = std::fs::read_to_string("src/notifyicon.rs").unwrap();
    let remove = src.split_once("pub(super) fn remove_now()").expect("remove_now").1.split("\n    }\n").next().unwrap();
    assert!(remove.contains("remove_icon(") && remove.contains("GONE.store(true"), "{remove}");
    assert!(src.contains("Shell_NotifyIconW(NIM_DELETE"));
    // And a restart that couldn't start the new copy puts it back.
    let daemon = crate::common::source_of("daemon");
    let restart = daemon.split_once("pub fn restart_as(").unwrap().1.split("\n    }\n").next().unwrap();
    assert!(restart.contains("take_icon_away()") && restart.contains("bring_icon_back()"), "{restart}");
}

// ================= 4. saves that fail say so =================

fn daemon_that_cannot_save<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let base = tmp(&format!("nosave-{tag}"));
    let daemon = Daemon::new(c, p, None, Store::new(&base), Proactive::new(ProactiveConfig::default()));
    // Startup must refuse invalid roots. These journeys instead exercise a
    // record becoming unwritable after a valid startup, before an owner edit.
    for name in [atlas::dash::FILE, atlas::palette::FILE, atlas::handover::FILE, atlas::tray::FILE] {
        let record = base.join(format!("{name}.json"));
        if record.is_file() { std::fs::remove_file(&record).unwrap(); }
        std::fs::create_dir(&record).unwrap();
    }
    daemon
}

#[test]
fn a_dashboard_move_that_cannot_be_kept_says_so() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = daemon_that_cannot_save(&c, &p, "dash");
    let first = d.dashboard.cards[0].card;
    let r = atlas::hublive::reply(&mut d, Action::DashMove(atlas::dash::Move::Down(first)));
    assert_eq!(r.status, 303);
    assert!(r.body.contains("said=") && r.body.contains("save"), "a plain redirect, as if it stuck: {}", r.body);
}

#[test]
fn what_you_picked_in_the_palette_that_cannot_be_kept_is_said() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = daemon_that_cannot_save(&c, &p, "palette");
    let r = atlas::hublive::reply(&mut d, Action::Find("settings".into()));
    assert!(r.body.contains("couldn&#39;t save what you picked") || r.body.contains("couldn't save what you picked"), "{}", r.body);
}

#[test]
fn something_handed_over_that_cannot_be_kept_is_said_to_the_phone() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = daemon_that_cannot_save(&c, &p, "hand");
    let r = atlas::hublive::reply(
        &mut d,
        Action::Hand { what: "https://example.com/a".into(), space: None, from: "phone".into(), asked: None },
    );
    assert!(r.body.contains("couldn't save it") && r.body.contains("until I next start"), "{}", r.body);
    // And from the hub's own Give page.
    let r = atlas::hublive::reply(&mut d, Action::HubPost { path: "/hub/give".into(), fields: vec![("text".into(), "remember the milk".into())] });
    assert!(r.body.contains("save"), "{}", r.body);
}

#[test]
fn no_hub_save_of_the_palette_dashboard_tray_or_chats_is_thrown_away() {
    let src = crate::common::source_of("hublive");
    for held in ["self.palette.save(", "self.dashboard.save(", "self.tray.save(", "self.chats.save(", "self.calendar.save(", "self.workshop.save("] {
        assert!(!src.contains(&format!("let _ = {held}")), "a failed {held} is still thrown away in hublive");
    }
}

// ================= 5. the file index doesn't hold up the start =================

fn entry(path: &str) -> atlas::index::Entry {
    atlas::index::Entry {
        name: path.rsplit('/').next().unwrap().into(),
        path: path.into(),
        ext: path.rsplit('.').next().unwrap().into(),
        size: 10,
        modified: 1_790_000_000,
        class: atlas::index::AssetClass::Document,
    }
}

#[test]
fn the_index_on_disk_is_read_beside_the_start_and_taken_in_when_it_arrives() {
    let dir = tmp("index-load");
    let store = Store::new(dir.clone());
    let mut idx = atlas::index::Index::default();
    idx.entries.insert("/docs/budget.xlsx".into(), entry("/docs/budget.xlsx"));
    idx.last_scan = 1_790_000_000;
    idx.save(&store).unwrap();
    // The reader, alone: an untouched index is replaced...
    let mut l = atlas::index::Loading::start(&store);
    let mut current = atlas::index::Index::default();
    assert!(matches!(l.wait(&mut current, Duration::from_secs(5)), atlas::index::Settled::Took((1, _))));
    assert!(current.entries.contains_key("/docs/budget.xlsx"));
    // ...one already filled another way (a scan; a test) is newer and stays.
    let mut l = atlas::index::Loading::start(&store);
    let mut current = atlas::index::Index::default();
    current.entries.insert("/new.txt".into(), entry("/new.txt"));
    assert_eq!(l.wait(&mut current, Duration::from_secs(5)), atlas::index::Settled::KeptNewer);
    assert!(current.entries.contains_key("/new.txt") && !current.entries.contains_key("/docs/budget.xlsx"));

    // Through the daemon: not read inside `new`, taken in after.
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
    assert!(d.wait_for_index(Duration::from_secs(5)));
    assert!(d.index.entries.contains_key("/docs/budget.xlsx"));
    let daemon = crate::common::source_of("daemon");
    let new = daemon.split_once("    pub fn new(").unwrap().1.split("\n    }\n").next().unwrap();
    assert!(!new.contains("Index::load("), "the index is read inside Daemon::new again");
}

#[test]
fn a_search_while_the_index_is_still_being_read_says_so_rather_than_nothing() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("index-honest")), Proactive::new(ProactiveConfig::default()));
    let (held, send) = atlas::index::Loading::held_for_test();
    d.index_loading_for_test(held);
    let said = d.execute(&atlas::intent::Intent::FindFile("find the file budget".into()));
    assert!(said.contains("still reading"), "{said}");
    assert!(d.index_still_loading());
    // A persist meanwhile doesn't write the empty stand-in over the real one.
    let file = d.store.root().join("index.json");
    std::fs::write(&file, "the real one").unwrap();
    d.persist();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "the real one");
    // It arrives.
    let mut idx = atlas::index::Index::default();
    idx.entries.insert("/docs/budget.xlsx".into(), entry("/docs/budget.xlsx"));
    idx.last_scan = 1_790_000_000;
    send.send(idx).unwrap();
    let said = d.execute(&atlas::intent::Intent::FindFile("find the file budget".into()));
    assert!(said.contains("budget.xlsx"), "{said}");
    assert!(!d.index_still_loading());
}

// ================= 6. a file the model chose, read off the loop =================

#[cfg(unix)]
#[test]
fn reading_a_file_leaves_the_loop_free_while_the_scan_runs() {
    let dir = tmp("slow-scan");
    let s = dir.join("slow-scan.sh");
    std::fs::write(&s, "#!/bin/sh\nsleep 2\necho clean\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().files.virus_scan =
        atlas::unpack::ScanConfig { command: s.display().to_string(), args: vec!["{path}".into()], threat_exit: 2 };
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("slow-scan-d")), Proactive::new(ProactiveConfig::default()));
    let path = Path::new("tests/fixtures/documents/letter.pdf").display().to_string();
    let started = Instant::now();
    let said = d.execute(&atlas::intent::Intent::ReadDocument(format!("read this pdf \"{path}\"")));
    let took = started.elapsed();
    crate::common::assert_prompt(took, Duration::from_millis(1500), "the turn waited for the scan");
    assert!(said.starts_with("Reading letter.pdf"), "{said}");
    // Meanwhile the hub is answered.
    let page = atlas::hublive::reply(&mut d, Action::Hub(atlas::hub::Page::Dashboard));
    assert_eq!(page.status, 200);
    let done = d.errands_done_for_test().join(" ");
    assert!(done.contains("Invoice for March"), "{done}");
}

// ================= 7. the voice cut-in model =================

#[test]
fn setup_fetches_the_silero_voice_cut_in_model_pinned_to_the_file_atlas_uses() {
    let pieces = atlas::getpieces::catalogue();
    let silero = pieces.iter().find(|p| p.key_path().ends_with(atlas::micthread::SILERO_FILE)).expect("not among what setup fetches");
    // Where the cut-in looks: the models folder, by the default name.
    assert_eq!(silero.key_path(), format!("models/{}", atlas::micthread::SILERO_FILE));
    assert_eq!(atlas::micthread::BargeInConfig::default().model, atlas::micthread::SILERO_FILE);
    // The official repository, at a tag rather than a moving branch.
    assert!(silero.url.starts_with("https://raw.githubusercontent.com/snakers4/silero-vad/v"), "{}", silero.url);
    assert!(silero.url.ends_with("/src/silero_vad/data/silero_vad_16k_op15.onnx"));
    // The same bytes as the copy the tests run the real model with.
    let fixture = std::fs::read("tests/fixtures/silero/silero_vad_16k_op15.onnx").unwrap();
    assert_eq!(fixture.len() as u64, silero.bytes);
    use sha2::Digest;
    let hash: String = sha2::Sha256::digest(&fixture).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hash, silero.sha256);
    // And `have` accepts the right file where it lands.
    let root = tmp("silero");
    std::fs::create_dir_all(root.join("models")).unwrap();
    std::fs::write(root.join(silero.key_path()), &fixture).unwrap();
    assert!(atlas::getpieces::have(silero, &root));
    // Part of setup, which walks `setupwin::setup_pieces`.
    #[cfg(feature = "desktop-ui")]
    assert!(atlas::setupwin::setup_pieces().iter().any(|p| p.name == silero.name));
}
