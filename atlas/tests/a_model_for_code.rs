//! "It still can't code" (2 Oct 2026), the second half: a model trained for
//! code, swapped in for a build; the project read the way a person reads it
//! before changing it; and the write-check-fix loop shown what failed, not a
//! whole log.
//!
//! Nothing here needs a real model, a toolchain or the internet: the coding
//! model's server is a stand-in started and stopped by the test, the talking
//! model's helpers are a stand-in that counts what was asked of them, and
//! the projects are folders written into the temp directory.

use atlas::brain::{Llm, LongReply, MockLlm};
use atlas::build_it::{self, Check, Outcome, Writer};
use atlas::coder::{self, ChatRoom, Coder, Size};
use atlas::craft::Lang;
use atlas::deepbrain::{DeepBrain, Engine, State};
use atlas::models::{Model, ModelsConfig, Registry};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-model-for-code-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

// ---------------------------------------------------------------------------
// Which coding model, by this device's memory.
// ---------------------------------------------------------------------------

#[test]
fn a_sixteen_gigabyte_laptop_gets_the_seven_b() {
    // Eric's: 16 GB soldered, integrated graphics (no memory of its own).
    assert_eq!(coder::size_for(16_000, 0, 0, 16384), Some(Size::Seven));
    // A 12 GB machine still has room for it once the talking model steps aside.
    assert_eq!(coder::size_for(12_288, 0, 0, 16384), Some(Size::Seven));
}

#[test]
fn a_small_machine_gets_the_small_one_and_a_tiny_one_none() {
    assert_eq!(coder::size_for(8_192, 0, 0, 16384), Some(Size::OneAndAHalf));
    assert_eq!(coder::size_for(4_096, 0, 0, 16384), Some(Size::OneAndAHalf));
    assert_eq!(coder::size_for(2_048, 0, 0, 16384), None, "a coding model that pages the machine helps nobody");
}

#[test]
fn a_graphics_card_and_a_memory_limit_both_count() {
    // An 8 GB machine with an 8 GB graphics card holds the 7B on the card.
    assert_eq!(coder::size_for(8_192, 8_192, 0, 16384), Some(Size::Seven));
    // A limit you set (4, meaning gigabytes) is never exceeded.
    assert_eq!(coder::size_for(32_768, 0, 4, 16384), Some(Size::OneAndAHalf));
    assert_eq!(coder::size_for(32_768, 0, 1, 16384), None);
}

#[test]
fn what_each_needs_is_the_weights_and_the_cache() {
    // 7B: 4.68 GB of weights and 57 KB of cache a token; at 16k about 5.9 GB.
    let seven = Size::Seven.need_mb(16384);
    assert!((5_700..6_100).contains(&seven), "{seven} MB");
    // A longer context costs more.
    assert!(Size::Seven.need_mb(32768) > seven);
    let small = Size::OneAndAHalf.need_mb(16384);
    assert!((1_500..1_900).contains(&small), "{small} MB");
}

#[test]
fn the_coding_models_are_pinned_and_checkable() {
    for (p, id) in [(atlas::getpieces::coder_model(), Size::Seven.id()), (atlas::getpieces::small_coder_model(), Size::OneAndAHalf.id())] {
        assert!(p.url.starts_with("https://huggingface.co/Qwen/"), "Qwen's own repository: {}", p.url);
        assert!(p.url.contains("/resolve/") && !p.url.contains("/resolve/main/"), "pinned to a commit: {}", p.url);
        assert_eq!(p.sha256.len(), 64);
        assert!(p.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(p.key_path(), format!("models/{id}.gguf"), "lands where the registry finds it by its id");
        assert!(!p.url.contains("3b"), "the 3B is under a non-commercial licence");
    }
    assert_eq!(Size::Seven.piece(), atlas::getpieces::coder_model());
    assert_eq!(Size::OneAndAHalf.piece(), atlas::getpieces::small_coder_model());
    // `atlas get coder` names a set (what's in it depends on the machine).
    assert!(atlas::getpieces::set(Some("coder")).is_some());
}

fn model(id: &str, params: u64, bytes: u64) -> Model {
    Model {
        path: PathBuf::from(format!("models/{id}.gguf")),
        id: id.into(),
        architecture: "qwen2".into(),
        quant: "Q4_K".into(),
        parameters: params,
        weight_bytes: bytes,
        max_context: 32768,
        chat_template: None,
    }
}

#[test]
fn a_coding_model_never_takes_over_talking() {
    let r = Registry { models: vec![model(Size::OneAndAHalf.id(), 1_500_000_000, 1_117_320_768), model("small-talker-1b", 1_000_000_000, 900_000_000)] };
    let cfg = ModelsConfig { talk_ceiling_b: 5, ..Default::default() };
    let chosen = r.choose_for(&cfg, 64 * 1024 * 1024 * 1024).map(|m| m.id.clone());
    assert_eq!(chosen.as_deref(), Some("small-talker-1b"), "the 1.5B coder is bigger, and still doesn't talk");
}

#[test]
fn the_coding_model_is_the_largest_here_that_fits_and_can_be_switched_off() {
    let r = Registry { models: vec![model(Size::Seven.id(), 7_600_000_000, 4_683_073_536), model(Size::OneAndAHalf.id(), 1_500_000_000, 1_117_320_768)] };
    let cfg = ModelsConfig::default();
    assert_eq!(coder::coder_among(&r, &cfg, 16_000, 0).map(|m| m.id.as_str()), Some(Size::Seven.id()));
    assert_eq!(coder::coder_among(&r, &cfg, 8_000, 0).map(|m| m.id.as_str()), Some(Size::OneAndAHalf.id()));
    let off = ModelsConfig { coder: "off".into(), ..Default::default() };
    assert!(coder::coder_among(&r, &off, 64_000, 0).is_none());
    // Its own port, past the deep model's, and its own context.
    let c = coder::coder_settings(&cfg);
    assert_eq!(c.port, cfg.port + 2);
    assert_eq!(c.context, coder::CONTEXT_DEFAULT);
}

// ---------------------------------------------------------------------------
// Swapping: the talking model let go for the build, and given back.
// ---------------------------------------------------------------------------

/// The coding model's server: started and stopped, healthy once started.
struct StandIn {
    up: Arc<AtomicBool>,
    starts: Arc<AtomicUsize>,
    fails: bool,
}

impl Engine for StandIn {
    fn start(&mut self) -> Result<(), String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        if self.fails {
            return Err("the port is taken".into());
        }
        self.up.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn stop(&mut self) {
        self.up.store(false, Ordering::SeqCst);
    }
    fn running(&mut self) -> bool {
        self.up.load(Ordering::SeqCst)
    }
    fn healthy(&mut self) -> bool {
        self.up.load(Ordering::SeqCst)
    }
}

/// The talking model's helpers: running or not, in use or not, and what was
/// asked of them. Letting it go gives its memory back.
struct Room {
    running: bool,
    in_use: bool,
    let_go: usize,
    back: Vec<bool>,
    free: Arc<AtomicU64>,
    gives_back_mb: u64,
}

impl ChatRoom for Room {
    fn chat_running(&self) -> bool {
        self.running
    }
    fn chat_in_use(&self) -> bool {
        self.in_use
    }
    fn let_chat_go(&mut self) {
        self.running = false;
        self.let_go += 1;
        self.free.fetch_add(self.gives_back_mb, Ordering::SeqCst);
    }
    fn bring_chat_back(&mut self, now: bool) {
        self.back.push(now);
    }
}

fn coder_with(fails: bool, free: Arc<AtomicU64>, need_mb: u64) -> (Coder, Arc<AtomicUsize>) {
    struct BoundedCoderFixture;
    impl Llm for BoundedCoderFixture {
        fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> { Ok("```python\nprint('from the coder')\n```".into()) }
        fn supports_bounded_chat(&self) -> bool { true }
        fn chat_until(&self, _: &atlas::brain::ChatRequest, text: &mut dyn FnMut(&str) -> bool, keep: &dyn Fn() -> bool) -> atlas::error::Result<atlas::brain::ChatReply> {
            let mut reply = String::new();
            for chunk in ["```python\n", "print('from the coder')\n", "```"] {
                if !keep() || !text(chunk) { return Err(atlas::error::AtlasError::Platform("fixture coder stopped".into())); }
                reply.push_str(chunk);
            }
            Ok(atlas::brain::ChatReply::from_text(&reply))
        }
    }
    let starts = Arc::new(AtomicUsize::new(0));
    let engine = StandIn { up: Arc::new(AtomicBool::new(false)), starts: starts.clone(), fails };
    let conn: Arc<dyn Llm> = Arc::new(BoundedCoderFixture);
    let f = free.clone();
    let mut brain = DeepBrain::new(Box::new(engine), conn, Size::Seven.id(), need_mb, Duration::ZERO, Box::new(move || f.load(Ordering::SeqCst)));
    brain.load_limit = Duration::from_secs(5);
    (Coder::new(brain, 16384), starts)
}

/// Wait until a code call is waiting on the coding model.
fn until_wanted(c: &Coder) {
    for _ in 0..200 {
        if c.brain.wanted() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the code call never asked for the coding model");
}

fn call(c: &Coder) -> std::thread::JoinHandle<atlas::error::Result<LongReply>> {
    let llm = c.llm().expect("a coding model is set up");
    std::thread::spawn(move || llm.complete_long("write code", "a script", 2000))
}

#[test]
fn the_talking_model_steps_aside_for_the_build_and_comes_back_lazily() {
    // 3 GB free; the coder needs 5 GB plus headroom; the talking model holds 3.5 GB.
    let free = Arc::new(AtomicU64::new(3_000));
    let (mut c, starts) = coder_with(false, free.clone(), 5_000);
    let mut room = Room { running: true, in_use: false, let_go: 0, back: vec![], free, gives_back_mb: 3_500 };
    let waiting = call(&c);
    until_wanted(&c);

    let said = c.keep(&mut room).join(" ");
    assert_eq!(room.let_go, 1, "let go to make room: {said}");
    assert!(c.holds_chat_room);
    assert_eq!(starts.load(Ordering::SeqCst), 0, "not started in the same breath as the memory is handed back");

    std::thread::sleep(coder::SETTLE + Duration::from_millis(100));
    c.keep(&mut room);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    c.keep(&mut room);
    assert_eq!(c.brain.gate.state(), State::Up);

    let reply = waiting.join().unwrap().expect("the coding model answered");
    assert!(reply.text.contains("from the coder"));

    // Idle: the coding model goes, and the talking model comes back with
    // the next thing you say, not at once.
    let said = c.keep(&mut room).join(" ");
    assert_ne!(c.brain.gate.state(), State::Up, "{said}");
    assert!(!c.holds_chat_room);
    assert_eq!(room.back, vec![false], "lazily: {said}");
}

#[test]
fn both_fitting_means_nothing_is_let_go() {
    let free = Arc::new(AtomicU64::new(9_000));
    let (mut c, starts) = coder_with(false, free.clone(), 5_000);
    let mut room = Room { running: true, in_use: false, let_go: 0, back: vec![], free, gives_back_mb: 3_500 };
    let waiting = call(&c);
    until_wanted(&c);
    c.keep(&mut room);
    c.keep(&mut room);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(waiting.join().unwrap().is_ok());
    assert_eq!(room.let_go, 0, "room for both: the talking model stays");
    c.keep(&mut room);
    assert!(room.back.is_empty(), "nothing was let go, so nothing is brought back");
}

#[test]
fn a_talking_model_in_use_is_never_let_go_and_the_build_goes_down_the_chain() {
    let free = Arc::new(AtomicU64::new(3_000));
    let (mut c, starts) = coder_with(false, free.clone(), 5_000);
    let mut room = Room { running: true, in_use: true, let_go: 0, back: vec![], free, gives_back_mb: 3_500 };

    // The build, with the coding model first and the model on this computer next.
    let coder_llm = c.llm().unwrap();
    let local: Arc<dyn Llm> = Arc::new(MockLlm("```python\nprint('from the local model')\n```".into()));
    let builder = std::thread::spawn(move || {
        let writers: Vec<(Writer, &dyn Llm)> = vec![(Writer::Coder, coder_llm.as_ref()), (Writer::Local, local.as_ref())];
        build_it::build_with("a script", Lang::Python, &writers, 2, |_| Check::Passed(vec![]))
    });
    until_wanted(&c);
    c.keep(&mut room);
    assert_eq!(room.let_go, 0, "a turn is using it");
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    assert_eq!(c.brain.gate.state(), State::Unavailable);

    let (outcome, by) = builder.join().unwrap();
    assert!(matches!(outcome, Outcome::Built { .. }));
    assert_eq!(by, Some(Writer::Local), "said truthfully: the model on this computer wrote it");
}

#[test]
fn a_coding_model_that_wont_start_hands_the_talking_model_back_at_once() {
    let free = Arc::new(AtomicU64::new(3_000));
    let (mut c, starts) = coder_with(true, free.clone(), 5_000);
    let mut room = Room { running: true, in_use: false, let_go: 0, back: vec![], free, gives_back_mb: 3_500 };
    let waiting = call(&c);
    until_wanted(&c);
    c.keep(&mut room);
    assert_eq!(room.let_go, 1);
    std::thread::sleep(coder::SETTLE + Duration::from_millis(100));
    let said = c.keep(&mut room).join(" ");
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(room.back, vec![true], "the build still needs a model, so now: {said}");
    assert!(waiting.join().unwrap().is_err(), "the call fails, for the next writer to take");
}

#[test]
fn a_turn_takes_the_room_back_from_the_coding_model() {
    let free = Arc::new(AtomicU64::new(3_000));
    let (mut c, _) = coder_with(false, free.clone(), 5_000);
    let mut room = Room { running: true, in_use: false, let_go: 0, back: vec![], free, gives_back_mb: 3_500 };
    let waiting = call(&c);
    until_wanted(&c);
    c.keep(&mut room);
    assert!(c.holds_chat_room);
    let said = c.give_way_to_a_turn(&mut room).expect("it held the room");
    assert!(said.contains("stopped the coding model"), "{said}");
    assert!(!c.holds_chat_room);
    assert_eq!(room.back, vec![true]);
    assert!(c.give_way_to_a_turn(&mut room).is_none(), "once");
    drop(waiting);
}

#[test]
fn the_coding_model_writes_first_and_is_named() {
    let have = build_it::Available { coder: true, your_second: true, local: true, ..Default::default() };
    assert_eq!(build_it::writers(&have).first(), Some(&Writer::Coder));
    assert_eq!(Writer::Coder.named_with(&coder::plain_name(Size::Seven.id())), "the coding model on this computer (Qwen2.5-Coder 7B)");
    assert_eq!(coder::plain_name(Size::OneAndAHalf.id()), "Qwen2.5-Coder 1.5B");
}

#[test]
fn a_build_says_the_coding_model_writes_it() {
    struct BoundedFixture;
    impl Llm for BoundedFixture {
        fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> { Ok("```python\nprint('hi')\n```".into()) }
        fn supports_bounded_chat(&self) -> bool { true }
        fn chat_until(&self, _: &atlas::brain::ChatRequest, text: &mut dyn FnMut(&str) -> bool, keep: &dyn Fn() -> bool) -> atlas::error::Result<atlas::brain::ChatReply> {
            let mut reply = String::new();
            for chunk in ["```python\n", "print('hi')\n", "```"] {
                if !keep() || !text(chunk) { return Err(atlas::error::AtlasError::Platform("fixture generation stopped".into())); }
                reply.push_str(chunk);
            }
            Ok(atlas::brain::ChatReply::from_text(&reply))
        }
    }
    use atlas::config::Config;
    use atlas::daemon::Daemon;
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::store::Store;
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let llm: Arc<dyn Llm> = Arc::new(BoundedFixture);
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("daemon")), Proactive::new(ProactiveConfig::default()));
    d.find_coding_agents_with_for_test(|_| None);
    let (coder, _) = coder_with(false, Arc::new(AtomicU64::new(64_000)), 5_000);
    let gate = coder.brain.gate.clone();
    d.use_coder_for_test(coder);
    let reply = d.turn("make me a tool that totals my receipts", 100);
    assert!(reply.contains("the coding model on this computer (Qwen2.5-Coder 7B)"), "{reply}");
    assert!(reply.contains("If it gets stuck"), "and who has a go after it: {reply}");
    // And it was: the draft went to the coding model first. Nothing ticks
    // here to start its server, so it couldn't be had, and the call fell
    // through to the next writer.
    let said = d.errands_done_for_test().join("\n");
    assert_eq!(gate.fell_back.load(Ordering::SeqCst), 1, "{said}");
    assert_eq!(gate.served.load(Ordering::SeqCst), 0);
}

// ---------------------------------------------------------------------------
// Reading the project.
// ---------------------------------------------------------------------------

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A project with the thing asked about well away from the top: twelve
/// files of filler first in every listing order, and a folder of build
/// output that must never be read.
fn project(tag: &str) -> PathBuf {
    let root = tmp(tag);
    write(&root, "Cargo.toml", "[package]\nname = \"ledger\"\nversion = \"0.1.0\"\n");
    for i in 0..12 {
        let body: String = (0..60).map(|n| format!("pub fn helper_{i}_{n}(x: u32) -> u32 {{ x + {n} }}\n")).collect();
        write(&root, &format!("src/aa_filler_{i:02}.rs"), &body);
    }
    write(
        &root,
        "src/config/reader.rs",
        "use std::collections::HashMap;\n\n/// Reads the settings file.\npub fn parse_config(text: &str) -> HashMap<String, String> {\n    let mut out = HashMap::new();\n    for line in text.lines() {\n        let (k, v) = line.split_once('=').unwrap();\n        out.insert(k.trim().to_string(), v.trim().to_string());\n    }\n    out\n}\n",
    );
    write(&root, "src/main.rs", "mod config;\nfn main() {\n    let c = config::reader::parse_config(\"a=1\");\n    println!(\"{:?}\", c);\n}\n");
    write(&root, "app/session.py", &format!("{}\ndef load_user(store, key):\n    return store['user'][key]\n", "# session handling\n".repeat(30)));
    write(&root, "target/debug/build/huge.rs", "pub fn parse_config() {}\n");
    write(&root, "node_modules/x/index.js", "function parse_config() {}\n");
    root
}

#[test]
fn a_function_named_in_the_request_is_read_where_it_is_defined() {
    let root = project("named");
    let r = atlas::projectread::relevant(&root, "parse_config panics on a line with no equals sign -- make it skip those", 2_000);
    assert_eq!(r.files.first().map(String::as_str), Some("src/config/reader.rs"), "{:?}", r.files);
    assert!(r.text.contains("pub fn parse_config(text: &str)"), "the definition itself:\n{}", r.text);
    assert!(r.text.contains("src/config/reader.rs (lines 1-"), "with where it is:\n{}", r.text);
    assert!(!r.text.contains("target/") && !r.text.contains("node_modules"), "never build output or dependencies");
}

#[test]
fn an_error_pasted_in_points_at_its_file() {
    let root = project("error");
    let asked = "fix this:\nTraceback (most recent call last):\n  File \"app/session.py\", line 32, in load_user\n    return store['user'][key]\nKeyError: 'user'";
    let r = atlas::projectread::relevant(&root, asked, 2_000);
    assert_eq!(r.files.first().map(String::as_str), Some("app/session.py"), "{:?}", r.files);
    assert!(r.text.contains("def load_user"), "{}", r.text);
}

#[test]
fn plain_words_find_a_name_written_in_snake_or_camel_case() {
    let root = project("words");
    write(&root, "src/totals.rs", "pub fn monthlyTotal(rows: &[u32]) -> u32 { rows.iter().sum() }\n");
    let r = atlas::projectread::relevant(&root, "make the monthly total skip negative rows", 1_500);
    assert_eq!(r.files.first().map(String::as_str), Some("src/totals.rs"), "{:?}", r.files);
}

#[test]
fn the_tree_is_there_and_the_whole_thing_stays_within_its_budget() {
    let root = project("budget");
    // A big file, many times the budget.
    write(&root, "src/big.rs", &"pub fn parse_config_again() { let x = 1; }\n".repeat(5_000));
    for budget in [600usize, 1_500, 4_000] {
        let r = atlas::projectread::relevant(&root, "parse_config and parse_config_again", budget);
        assert!(r.tokens <= budget, "{} tokens for a budget of {budget}", r.tokens);
        assert!(r.text.len().div_ceil(3) <= budget, "{} chars for a budget of {budget}", r.text.len());
        assert!(r.text.starts_with("The project's files:"), "the tree first");
        assert!(r.text.contains("src/config/") && r.text.contains("reader.rs"), "{}", r.text);
    }
}

#[test]
fn the_budget_follows_the_models_context() {
    use atlas::projectread::budget_for;
    assert_eq!(budget_for(Some(8192)), 2867);
    assert_eq!(budget_for(Some(16384)), 5734);
    assert_eq!(budget_for(None), budget_for(Some(build_it::CONTEXT_ASSUMED)));
    assert_eq!(budget_for(Some(1024)), 600, "never nothing");
    assert_eq!(budget_for(Some(131072)), 12_000, "never the whole of a huge context");
}

#[test]
fn no_folder_reads_nothing() {
    let r = atlas::projectread::relevant(Path::new("/no/such/folder/anywhere"), "parse_config", 2_000);
    assert!(r.text.is_empty() && r.files.is_empty());
}

// ---------------------------------------------------------------------------
// What a fix round is shown.
// ---------------------------------------------------------------------------

fn cargo_failure() -> String {
    let mut out = String::new();
    for i in 0..150 {
        out.push_str(&format!("   Compiling dependency-{i} v0.1.{i}\n"));
    }
    for _ in 0..40 {
        out.push_str("warning: unused variable: `x`\n --> src/lib.rs:2:9\n  |\n2 |     let x = 1;\n  |         ^\n\n");
    }
    out.push_str("error[E0425]: cannot find value `totl` in this scope\n --> src/main.rs:7:20\n  |\n7 |     println!(\"{}\", totl);\n  |                    ^^^^ help: a local variable with a similar name exists: `total`\n\n");
    out.push_str("error: could not compile `ledger` (bin \"ledger\") due to 1 previous error\n");
    out
}

#[test]
fn a_compilers_failure_is_cut_to_the_error_and_where_it_is() {
    let full = cargo_failure();
    let cut = build_it::trim_failure(&full, build_it::FAILURE_MOST);
    assert!(cut.len() <= build_it::FAILURE_MOST, "{} chars", cut.len());
    assert!(cut.contains("error[E0425]: cannot find value `totl`"), "{cut}");
    assert!(cut.contains("--> src/main.rs:7:20"), "where: {cut}");
    assert!(cut.contains("similar name exists: `total`"), "the hint: {cut}");
    assert!(cut.contains("could not compile"), "the last line: {cut}");
    assert!(!cut.contains("Compiling dependency-3 "), "not the progress lines: {cut}");
}

#[test]
fn a_python_failure_keeps_the_end_where_it_says_what_happened() {
    let mut full = String::from("Traceback (most recent call last):\n");
    for i in 0..120 {
        full.push_str(&format!("  File \"lib/step_{i}.py\", line {i}, in step_{i}\n    step_{}()\n", i + 1));
    }
    full.push_str("KeyError: 'user'\n");
    let cut = build_it::trim_failure(&full, 1_200);
    assert!(cut.len() <= 1_200, "{} chars", cut.len());
    assert!(cut.ends_with("KeyError: 'user'"), "{cut}");
    assert!(cut.starts_with("Traceback"), "{cut}");
}

#[test]
fn short_output_is_left_whole_and_output_with_no_error_keeps_its_end() {
    assert_eq!(build_it::trim_failure("1 test failed: totals", 2_000), "1 test failed: totals");
    let plain: String = (0..400).map(|i| format!("line {i}\n")).collect();
    let cut = build_it::trim_failure(&plain, 300);
    assert!(cut.len() <= 304 && cut.contains("line 399"), "{cut}");
}

/// A model that remembers what each call was shown.
struct Recorder {
    asked: Mutex<Vec<String>>,
}

impl Llm for Recorder {
    fn complete(&self, _: &str, user: &str) -> atlas::error::Result<String> {
        self.asked.lock().unwrap().push(user.to_string());
        Ok("```python\nprint(total)\n```".into())
    }
}

#[test]
fn the_fix_round_is_shown_the_trimmed_failure() {
    let llm = Recorder { asked: Mutex::new(vec![]) };
    let full = cargo_failure();
    let mut checks = 0;
    let o = build_it::build_loop("totals", Lang::Python, &llm, 3, |_| {
        checks += 1;
        if checks == 1 {
            Check::Failed(full.clone())
        } else {
            Check::Passed(vec![])
        }
    });
    assert!(matches!(o, Outcome::Built { rounds: 1, .. }), "{o:?}");
    let asked = llm.asked.lock().unwrap();
    let fix = asked.iter().find(|a| a.contains("The tool said")).expect("a fix round was asked");
    assert!(fix.contains("cannot find value `totl`"), "the error reached the model");
    assert!(!fix.contains("Compiling dependency-3 "), "the noise didn't");
    assert!(fix.len() < full.len() / 2, "{} of {}", fix.len(), full.len());
}

#[test]
fn rounds_are_capped_whatever_the_settings_say() {
    let llm = MockLlm("```python\nprint(1)\n```".into());
    let o = build_it::build_loop("anything", Lang::Python, &llm, 500, |_| Check::Failed("error: still wrong".into()));
    match o {
        Outcome::Struggled { rounds, .. } => assert_eq!(rounds, build_it::MOST_ROUNDS),
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// A change to a file of yours: checked by the project's own tests, fixed
// from what they said, and never written over the file before you say so.
// ---------------------------------------------------------------------------

/// Writes a wrong `double` first and the right one when shown what failed;
/// remembers every fix round it was asked.
struct Doubler {
    fixes: Mutex<Vec<String>>,
}

impl Llm for Doubler {
    fn supports_bounded_chat(&self) -> bool { true }
    fn chat_until(&self, request: &atlas::brain::ChatRequest, on_text: &mut dyn FnMut(&str) -> bool, keep: &dyn Fn() -> bool) -> atlas::error::Result<atlas::brain::ChatReply> {
        let mut user = String::new();
        for message in &request.messages {
            if !keep() { return Err(atlas::error::AtlasError::Platform("fixture generation stopped".into())); }
            user.push_str(&message.content);
            user.push('\n');
        }
        // This fixture performs only bounded in-memory work. Each emitted
        // chunk honors both controls; it never delegates a blocking call.
        if !keep() { return Err(atlas::error::AtlasError::Platform("fixture generation stopped".into())); }
        let text = self.complete("", &user)?;
        for chunk in text.as_bytes().chunks(32) {
            if !keep() || !on_text(std::str::from_utf8(chunk).unwrap()) {
                return Err(atlas::error::AtlasError::Platform("fixture generation stopped".into()));
            }
        }
        Ok(atlas::brain::ChatReply { text, ..Default::default() })
    }
    fn complete(&self, _: &str, user: &str) -> atlas::error::Result<String> {
        if user.contains("The tool said") {
            self.fixes.lock().unwrap().push(user.to_string());
            return Ok("```rust\npub fn double(x: i32) -> i32 {\n    x * 2\n}\n```".into());
        }
        if user.contains("What to build") {
            return Ok("```rust\npub fn double(x: i32) -> i32 {\n    x * 3\n}\n```".into());
        }
        Ok("It doubles a number.".into())
    }
}

#[test]
fn the_fixture_model_honors_cancel_before_generation_and_during_output() {
    let model = Doubler { fixes: Mutex::new(Vec::new()) };
    let request = atlas::brain::ChatRequest::default();
    let mut chunks = 0;
    assert!(model.chat_until(&request, &mut |_| { chunks += 1; true }, &|| false).is_err());
    assert_eq!(chunks, 0);
    assert!(model.chat_until(&request, &mut |_| false, &|| true).is_err());
}

#[test]
fn a_change_to_your_file_is_fixed_from_the_projects_own_tests_and_not_written() {
    use atlas::config::Config;
    use atlas::daemon::Daemon;
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::store::Store;
    if std::process::Command::new("cargo").arg("--version").output().is_err() {
        eprintln!("no cargo here: the project's own tests can't run");
        return;
    }
    let proj = tmp("own-tests").join("doubling");
    write(&proj, "Cargo.toml", "[package]\nname = \"doubling_for_atlas_test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    let original = "pub fn double(x: i32) -> i32 {\n    x + 1\n}\n";
    write(&proj, "src/lib.rs", original);
    // The project's own test, spelled so it isn't read as one of these.
    let attribute = format!("#[{}]", "test");
    write(&proj, "tests/doubles.rs", &format!("{attribute}\nfn three_doubled_is_six() {{\n    assert_eq!(doubling_for_atlas_test::double(3), 6);\n}}\n"));

    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let doubler = Arc::new(Doubler { fixes: Mutex::new(vec![]) });
    let llm: Arc<dyn Llm> = doubler.clone();
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("own-tests-store")), Proactive::new(ProactiveConfig::default()));
    d.find_coding_agents_with_for_test(|_| None);
    let reply = d.turn(&format!("in {}, add a feature that makes double in src/lib.rs really double", proj.display()), 100);
    assert!(reply.contains("doubling"), "{reply}");
    // Two runs of the project's tests: on a busy machine longer than one
    // wait for the errands allows.
    let mut said = String::new();
    for _ in 0..6 {
        said.push_str(&d.errands_done_for_test().join("\n"));
        if said.contains("Queued") || said.contains("couldn't") {
            break;
        }
    }

    let fixes = doubler.fixes.lock().unwrap();
    let first = fixes.first().unwrap_or_else(|| panic!("no fix round was asked; it said: {said}"));
    assert!(first.contains("three_doubled_is_six") || first.contains("left: 9"), "the project's own test failure reached the fix:\n{first}");
    assert!(said.contains("its own tests pass"), "proven in place: {said}");
    assert_eq!(std::fs::read_to_string(proj.join("src/lib.rs")).unwrap(), original, "nothing written over your file until you say implement");
}
