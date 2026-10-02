//! Learning how you talk (2 Oct 2026, "Atlas doesn't really understand me").
//!
//! Five things, each proved here through the daemon itself where it can be,
//! with a model that only counts how often it was asked:
//!
//! 1. A correction is learned, and the same words -- or near enough -- are
//!    done next time without asking the model; "forget that phrase" undoes
//!    it.
//! 2. Every miss is written down, the list is bounded, and "what did you
//!    misunderstand this week" reads it back.
//! 3. What you've told Atlas about the people and the project a sentence
//!    names reaches the model, and a medical note never does.
//! 4. The talking model's size follows the machine's numbers: the 4B on a
//!    16 GB laptop with shared graphics, the 8B where there's room.
//! 5. A correction that shows the words were misheard is a hearing miss, and
//!    the same mishearing twice is mended and handed to the speech model.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const T: u64 = 1_790_900_000;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-talk-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

/// A model that only says it isn't sure, and counts how often it was asked.
struct Counting(Arc<AtomicUsize>);
impl atlas::brain::Llm for Counting {
    fn complete(&self, _system: &str, _user: &str) -> atlas::error::Result<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("{\"action\":\"say\",\"arg\":null,\"say\":\"I'm not sure what you mean.\"}".into())
    }
}

fn counted<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> (Daemon<'a>, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let llm: Arc<dyn atlas::brain::Llm> = Arc::new(Counting(n.clone()));
    (Daemon::new(c, p, Some(llm), Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default())), n)
}

// ============ 1. a correction is learned ===================================

#[test]
fn a_correction_is_learned_and_the_same_words_skip_the_model() {
    let (c, p) = (cfg(), plat());
    let (mut d, asked) = counted(&c, &p, "learned");
    d.turn("play some tunes", T);
    assert_eq!(asked.load(Ordering::SeqCst), 1, "words the phrases don't know go to the model");

    let reply = d.turn("no, I meant open notepad", T + 10);
    assert!(reply.contains("play some tunes"), "learning should be said: {reply}");
    let kept = &d.phrasebook.phrases;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].tool, "open_app");
    assert_eq!(kept[0].arg.to_lowercase(), "notepad");
    assert!(kept[0].sure, "a correction is sure from the start");
    // Kept in the store, not only in memory: a friend's Atlas has its own.
    let back = atlas::phrasebook::Phrasebook::load(&d.store);
    assert_eq!(back.phrases.len(), 1);

    let before = asked.load(Ordering::SeqCst);
    d.turn("play some tunes", T + 600);
    assert_eq!(asked.load(Ordering::SeqCst), before, "the learned wording went to the model anyway");
    assert_eq!(d.session.last_intent, Some(atlas::intent::Intent::OpenApp("notepad".into())));
    // Near enough: politeness and a filler word don't make it a new request.
    d.turn("Atlas, could you play me some tunes please?", T + 700);
    assert_eq!(asked.load(Ordering::SeqCst), before, "a near wording went to the model");
    assert_eq!(d.phrasebook.phrases[0].used, 2);
    // A different request that shares words is not routed.
    d.turn("play some tunes by the river on the radio", T + 800);
    assert_eq!(asked.load(Ordering::SeqCst), before + 1, "a different request was taken as the learned one");
}

#[test]
fn forgetting_a_phrase_sends_the_words_back_to_the_model() {
    let (c, p) = (cfg(), plat());
    let (mut d, asked) = counted(&c, &p, "forget");
    d.turn("play some tunes", T);
    d.turn("I meant open notepad", T + 5);
    let listed = d.turn("what have you learned about how I talk", T + 20);
    assert!(listed.contains("play some tunes") && listed.contains("open notepad"), "{listed}");

    let reply = d.turn("forget that phrase", T + 30);
    assert!(reply.starts_with("Forgotten"), "{reply}");
    assert!(d.phrasebook.phrases.is_empty());
    assert!(atlas::phrasebook::Phrasebook::load(&d.store).phrases.is_empty(), "forgotten in memory only");
    let before = asked.load(Ordering::SeqCst);
    d.turn("play some tunes", T + 600);
    assert_eq!(asked.load(Ordering::SeqCst), before + 1, "a forgotten wording still routed");
}

#[test]
fn a_learned_wording_whose_action_is_undone_is_dropped() {
    let (c, p) = (cfg(), plat());
    let (mut d, _) = counted(&c, &p, "undone");
    d.turn("play some tunes", T);
    d.turn("no, I meant open notepad", T + 5);
    d.turn("play some tunes", T + 600);
    d.turn("that's not what i asked", T + 610);
    assert!(d.phrasebook.phrases.is_empty(), "a lesson that led somewhere wrong was kept");
    assert!(d.misses.misses.iter().any(|m| m.why == atlas::misses::Why::Corrected && m.said == "play some tunes"));
}

#[test]
fn what_i_should_have_done_instead_is_learned_when_it_is_something_i_can_do() {
    let (c, p) = (cfg(), plat());
    let (mut d, asked) = counted(&c, &p, "instead");
    d.turn("play some tunes", T);
    let q = d.turn("that's not what i asked", T + 5);
    assert!(q.contains("What should I have done instead?"), "{q}");
    let reply = d.turn("open notepad", T + 10);
    assert!(reply.contains("I'll remember \"play some tunes\" means that."), "{reply}");
    assert_eq!(d.phrasebook.phrases[0].taught, atlas::phrasebook::Taught::Answered);
    assert!(d.misses.misses.iter().any(|m| m.why == atlas::misses::Why::Corrected && m.said == "play some tunes"));
    let before = asked.load(Ordering::SeqCst);
    d.turn("play some tunes", T + 600);
    assert_eq!(asked.load(Ordering::SeqCst), before);
}

#[test]
fn a_rephrase_after_a_miss_is_learned_but_not_sure() {
    // No model: an unrecognised sentence ends as not understood.
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("rephrase")), Proactive::new(ProactiveConfig::default()));
    d.turn("the scribbly pad thing", T);
    assert!(d.misses.misses.iter().any(|m| m.why == atlas::misses::Why::NotUnderstood), "{:?}", d.misses.misses);
    let r = d.turn("open notepad", T + 20);
    let kept = &d.phrasebook.phrases;
    assert_eq!(kept.len(), 1, "{kept:?} {r}");
    assert!(!kept[0].sure, "a rephrase is kept, not trusted yet");
    // Too long after, it's a new request, not a rephrase.
    d.turn("summon the scribbling pad", T + 1000);
    d.turn("open notepad", T + 1000 + atlas::misses::REPHRASED_WITHIN_SECS + 5);
    assert_eq!(d.phrasebook.phrases.len(), 1);
}

#[test]
fn nothing_is_learned_from_a_fact_correction_or_a_question_answered() {
    let (c, p) = (cfg(), plat());
    let (mut d, _) = counted(&c, &p, "facts");
    d.turn("what car do I drive", T);
    d.turn("no, I meant my car is a Toyota", T + 5);
    assert!(d.phrasebook.phrases.is_empty(), "a fact about your world became a wording: {:?}", d.phrasebook.phrases);
}

#[test]
fn intents_round_trip_or_are_not_kept_as_commands() {
    use atlas::intent::Intent;
    let got = atlas::phrasebook::intent_as_tool(&Intent::OpenApp("spotify".into()), "open spotify");
    assert_eq!(got, Some(("open_app".into(), "spotify".into())));
    assert!(!atlas::phrasebook::learnable_action(&Intent::Unlock("hunter2".into())), "the passphrase must never be learned");
    assert!(!atlas::phrasebook::learnable_action(&Intent::Say("hi".into())));
}

// ============ 2. misses are logged, bounded and read back ==================

#[test]
fn misses_are_written_down_and_read_back_by_the_week() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("misses")), Proactive::new(ProactiveConfig::default()));
    d.turn("flibber the jabberwock", T);
    d.turn("flibber the jabberwock", T + 100);
    d.turn("wibble my wobble", T + 200);
    assert!(d.misses.misses.len() >= 3, "{:?}", d.misses.misses);
    let kept = atlas::misses::MissLog::load(&d.store);
    assert_eq!(kept.misses.len(), d.misses.misses.len(), "the misses weren't saved");
    let report = d.turn("what did you misunderstand this week", T + 300);
    assert!(report.contains("flibber the jabberwock") && report.contains("2 times"), "{report}");
    assert!(report.contains("didn't understand"), "{report}");
}

#[test]
fn the_misses_log_is_bounded_by_count_and_age() {
    let now = atlas::store::now();
    let mut log = atlas::misses::MissLog::default();
    for i in 0..(atlas::misses::MOST_MISSES + 150) {
        log.note_miss(atlas::misses::Miss {
            at: now - 1000 + i as u64 * 31,
            said: format!("request number {i}"),
            did: "said \"?\"".into(),
            why: atlas::misses::Why::NotUnderstood,
            by_voice: false,
        });
    }
    assert_eq!(log.misses.len(), atlas::misses::MOST_MISSES);
    assert!(log.misses[0].said.ends_with("150"), "the oldest should go first");
    log.misses.insert(0, atlas::misses::Miss { at: 5, said: "ancient".into(), did: String::new(), why: atlas::misses::Why::Undone, by_voice: false });
    log.keep_within(now);
    assert!(log.misses.iter().all(|m| m.said != "ancient"), "older than the keeping window was kept");
}

// ============ 3. what you've told Atlas reaches the model ==================

/// A model that takes messages, and keeps everything it was sent.
struct Reads(Arc<Mutex<String>>);
impl atlas::brain::Llm for Reads {
    fn complete(&self, system: &str, user: &str) -> atlas::error::Result<String> {
        let mut g = self.0.lock().unwrap();
        g.push_str(system);
        g.push_str(user);
        Ok("{\"action\":\"say\",\"arg\":null,\"say\":\"Sure.\"}".into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &atlas::brain::ChatRequest, _on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<atlas::brain::ChatReply> {
        let mut g = self.0.lock().unwrap();
        for m in &req.messages {
            g.push_str(&m.content);
            g.push('\n');
        }
        Ok(atlas::brain::ChatReply::from_text("Sure."))
    }
}

#[test]
fn the_people_and_project_a_sentence_names_reach_the_model() {
    let (c, p) = (cfg(), plat());
    let dir = tmp("context");
    let store = Store::new(&dir);
    // Priya, with one note worth knowing and one that is medical.
    let mut people = atlas::people::People::default();
    people.note("Priya", "runs the design side of Northwind", T - 100).unwrap();
    people.note("Priya", "her blood pressure medication changed", T - 50).unwrap();
    store.save("people", &people).unwrap();
    let seen = Arc::new(Mutex::new(String::new()));
    let llm: Arc<dyn atlas::brain::Llm> = Arc::new(Reads(seen.clone()));
    let mut d = Daemon::new(&c, &p, Some(llm), store, Proactive::new(ProactiveConfig::default()));
    d.person.projects.push(("Northwind".into(), T - 86_400));
    d.facts.learn(atlas::facts::Fact::stated("the Northwind launch is planned for November", T - 3_600), T - 3_600);
    d.turn(
        "I keep going back and forth about whether we should be telling Priya about the Northwind delay before Friday or after",
        T,
    );
    let prompt = seen.lock().unwrap().clone();
    assert!(prompt.contains("runs the design side of Northwind"), "what you told Atlas about Priya didn't reach the model:\n{prompt}");
    assert!(prompt.contains("their project Northwind"), "the project wasn't named:\n{prompt}");
    assert!(prompt.contains("launch is planned for November"), "what you said about the project didn't reach the model:\n{prompt}");
    assert!(!prompt.contains("blood pressure"), "a medical note reached the model:\n{prompt}");
}

#[test]
fn a_learned_wording_is_a_hint_when_it_is_only_close() {
    let (c, p) = (cfg(), plat());
    let seen = Arc::new(Mutex::new(String::new()));
    let llm: Arc<dyn atlas::brain::Llm> = Arc::new(Reads(seen.clone()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("hint")), Proactive::new(ProactiveConfig::default()));
    d.turn("play some tunes", T);
    d.turn("no, I meant open notepad", T + 5);
    seen.lock().unwrap().clear();
    d.turn("play some tunes from the eighties", T + 600);
    let prompt = seen.lock().unwrap().clone();
    assert!(prompt.contains("When they said \"play some tunes\" before, they meant \"open notepad\""), "{prompt}");
}

// ============ 4. the talking model follows the machine =====================

fn machine(total_mb: u64, free_mb: u64, vram_mb: u64) -> atlas::fit::Machine {
    atlas::fit::Machine { total_ram_mb: total_mb, free_ram_mb: free_mb, cpu_cores: 8, vram_mb, ..Default::default() }
}

#[test]
fn the_bigger_model_only_where_there_is_room_for_it() {
    let cfg = atlas::models::ModelsConfig::default();
    let files: u64 = atlas::getpieces::bigger_talk_model().iter().map(|p| p.bytes).sum();
    let needs = atlas::models::bigger_talk_needs(&cfg, files);
    let helpers = 1_200;
    // Eric's laptop: 16 GB shared with integrated graphics.
    assert!(!atlas::models::room_for_bigger_talk(&machine(16_384, 6_000, 0), &cfg, helpers, needs));
    // 32 GB: room.
    assert!(atlas::models::room_for_bigger_talk(&machine(32_768, 12_000, 0), &cfg, helpers, needs));
    // 16 GB with a graphics card of its own (8 GB): room.
    assert!(atlas::models::room_for_bigger_talk(&machine(16_384, 6_000, 8_192), &cfg, helpers, needs));
    // An 8 GB machine: nowhere near.
    assert!(!atlas::models::room_for_bigger_talk(&machine(8_192, 3_000, 0), &cfg, 0, needs));
    // A memory limit you set is the limit, however big the machine.
    let capped = atlas::models::ModelsConfig { memory_budget_mb: 4_096, ..cfg.clone() };
    assert!(!atlas::models::room_for_bigger_talk(&machine(65_536, 40_000, 0), &capped, helpers, needs));
    // The helpers count: a 24 GB machine has room until they don't leave it.
    assert!(atlas::models::room_for_bigger_talk(&machine(24_576, 8_000, 0), &cfg, 500, needs));
    assert!(!atlas::models::room_for_bigger_talk(&machine(24_576, 8_000, 0), &cfg, 4_000, needs));
}

#[test]
fn pick_talks_through_the_bigger_model_only_when_it_fits() {
    use atlas::models::{Model, ModelsConfig, Registry};
    let model = |id: &str, params: u64, bytes: u64| Model {
        path: PathBuf::from(format!("/nowhere/{id}.gguf")),
        id: id.into(),
        architecture: "qwen3vl".into(),
        quant: "Q4_K".into(),
        parameters: params,
        weight_bytes: bytes,
        max_context: 32_768,
        chat_template: None,
    };
    let registry = Registry {
        models: vec![
            model(atlas::deepbrain::FASTER_TALK, 4_000_000_000, 2_497_281_664),
            model(atlas::deepbrain::BIGGER_TALK, 8_000_000_000, 5_027_784_800),
        ],
    };
    let cfg = ModelsConfig { talk: "faster".into(), ..ModelsConfig::default() };
    let small = atlas::models::pick(&registry, &cfg, &machine(16_384, 6_000, 0)).unwrap();
    assert_eq!(small.id, atlas::deepbrain::FASTER_TALK);
    let big = atlas::models::pick(&registry, &cfg, &machine(32_768, 16_000, 0)).unwrap();
    assert_eq!(big.id, atlas::deepbrain::BIGGER_TALK);
    // "better" or a model by name is yours, and kept to.
    let named = ModelsConfig { prefer: atlas::deepbrain::FASTER_TALK.into(), ..cfg.clone() };
    assert_eq!(atlas::models::pick(&registry, &named, &machine(32_768, 16_000, 0)).unwrap().id, atlas::deepbrain::FASTER_TALK);
}

#[test]
fn the_bigger_models_files_are_pinned_and_qwens_own() {
    for p in atlas::getpieces::bigger_talk_model() {
        assert!(p.url.starts_with("https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF/resolve/f982a07559d4a2f6c8744d840bf6fccab30eea96/"), "{}", p.url);
        assert_eq!(p.sha256.len(), 64);
        assert!(p.bytes > 700_000_000);
    }
}

// ============ 5. hearing misses ============================================

#[test]
fn a_mishearing_corrected_twice_is_mended_and_given_to_the_speech_model() {
    let (c, p) = (cfg(), plat());
    let (mut d, _) = counted(&c, &p, "hearing");
    d.heard_by_voice("open spot if i");
    d.turn("open spot if i", T);
    d.turn("no, I said open spotify", T + 5);
    assert_eq!(d.misses.heard_wrong.len(), 1, "{:?}", d.misses.heard_wrong);
    assert_eq!(d.misses.heard_wrong[0].heard, "spot if i");
    assert_eq!(d.misses.heard_wrong[0].meant, "spotify");
    assert!(d.misses.misses.iter().any(|m| m.why == atlas::misses::Why::Misheard));
    assert!(!d.vocab.hints(atlas::improve::HINTS_GIVEN).iter().any(|w| w.eq_ignore_ascii_case("spotify")), "once is not yet a pattern");

    d.heard_by_voice("open spot if i");
    d.turn("open spot if i", T + 3_600);
    // Mended before it was read the second time? Not yet: it takes two.
    d.turn("I said open spotify", T + 3_605);
    assert_eq!(d.misses.heard_wrong[0].times, 2);
    assert!(d.vocab.hints(atlas::improve::HINTS_GIVEN).iter().any(|w| w.eq_ignore_ascii_case("spotify")), "the right word didn't reach the speech model's hints");

    // A third time, heard the same way: mended before anything reads it.
    d.heard_by_voice("open spot if i");
    d.turn("open spot if i", T + 7_200);
    assert_eq!(d.session.last_intent, Some(atlas::intent::Intent::OpenApp("spotify".into())));

    // Typed, the same words are what you typed: nothing is mended or blamed
    // on hearing.
    let before = d.misses.heard_wrong[0].times;
    d.turn("open spot if i", T + 9_000);
    d.turn("I said open spotify", T + 9_005);
    assert_eq!(d.misses.heard_wrong[0].times, before, "a typed sentence was counted as misheard");
}

#[test]
fn the_report_says_how_well_it_has_been_hearing() {
    let mut log = atlas::misses::MissLog::default();
    log.heard_as("spot if i", "spotify", T);
    log.heard_as("spot if i", "spotify", T + 1);
    let numbers = atlas::misses::HearingNumbers {
        ears: vec![("Webcam".into(), 30, 2), ("Laptop array".into(), 6, 9)],
        typical: Some(0.71),
        struggling: false,
    };
    let said = atlas::misses::week_report(&log, &numbers, 3, T + 10);
    assert!(said.contains("36 of 47 spoken turns understood"), "{said}");
    assert!(said.contains("Laptop array misses the most"), "{said}");
    assert!(said.contains("\"spot if i\" for \"spotify\""), "{said}");
    assert!(said.contains("3 of your own ways"), "{said}");
}
