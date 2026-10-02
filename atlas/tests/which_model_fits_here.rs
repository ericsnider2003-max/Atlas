//! Choosing a model for the machine Atlas is actually on.
//!
//! `models.rs` calls itself "the rest of what Ollama does, in Rust": find the
//! models on disk, read their metadata, work out what fits in memory, format
//! the prompt the way each expects, and run `llama-server` directly so there
//! is one fewer piece of someone else's software in the path.
//!
//! All of it was written. **None of it was ever called.** Nothing scanned a
//! real folder, `choose`, `best_fit` and `explain` had no production caller,
//! and `scan_reporting` — which exists precisely so an unreadable folder is
//! not reported as "no models installed" — had never reported anything.
//!
//! And the budget it sized every decision against was a number typed into
//! `config/tools.yaml`, under a comment reasoning about 15.7GB of RAM and
//! Windows using 12.8GB of it. That reasoning was right and writing the number
//! down was still wrong: it is a **measurement**, and a measurement in a
//! config file stops being true the moment you close a browser, add a stick of
//! RAM, or run Atlas on the other machine. `fit.rs` already reads it.
//!
//! These tests write real GGUF bytes to a real folder, because a registry
//! tested against hand-built `Model` structs is exactly what already existed
//! while the scan had never run.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::fit::Machine;
use atlas::intent::{Intent, Parser};
use atlas::models::{budget_bytes, estimate_memory, layers_here, Model, ModelsConfig, Registry};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

// --- real GGUF bytes, so the scan is tested against files ---

#[derive(Default)]
struct Builder {
    kv: Vec<(String, Vec<u8>)>,
    tensors: Vec<Vec<u8>>,
    n_tensors: u64,
}

fn u32b(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn u64b(v: u64) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn strb(s: &str) -> Vec<u8> {
    let mut o = u64b(s.len() as u64);
    o.extend_from_slice(s.as_bytes());
    o
}

impl Builder {
    fn kv_str(mut self, key: &str, val: &str) -> Self {
        let mut b = u32b(8);
        b.extend(strb(val));
        self.kv.push((key.into(), b));
        self
    }
    fn kv_u32(mut self, key: &str, val: u32) -> Self {
        let mut b = u32b(4);
        b.extend(u32b(val));
        self.kv.push((key.into(), b));
        self
    }
    fn tensor(mut self, name: &str, dims: &[u64], kind: u32, offset: u64) -> Self {
        let mut b = strb(name);
        b.extend(u32b(dims.len() as u32));
        for d in dims {
            b.extend(u64b(*d));
        }
        b.extend(u32b(kind));
        b.extend(u64b(offset));
        self.tensors.push(b);
        self.n_tensors += 1;
        self
    }
    fn build(self) -> Vec<u8> {
        let mut o = b"GGUF".to_vec();
        o.extend(u32b(3));
        o.extend(u64b(self.n_tensors));
        o.extend(u64b(self.kv.len() as u64));
        for (k, v) in &self.kv {
            o.extend(strb(k));
            o.extend(v);
        }
        for t in &self.tensors {
            o.extend(t);
        }
        o
    }
}

/// A model file with weights the size a real one of this shape would have.
///
/// `kind: 12` is Q4_K. Every block gets its seven real tensors rather than a
/// token one, because the first version of this fixture declared exactly two
/// tensors and produced a "7B" model weighing 79MB — so every machine, down
/// to 200MB of RAM, was told the large model fitted. The fixture was wrong and
/// it made the code look wrong.
fn a_model(embed: u64, blocks: u32, ctx: u32) -> Vec<u8> {
    let mut b = Builder::default()
        .kv_str("general.architecture", "llama")
        .kv_u32("llama.context_length", ctx)
        .kv_u32("llama.block_count", blocks)
        .kv_u32("llama.embedding_length", embed as u32)
        .kv_u32("llama.attention.head_count", 32)
        .kv_u32("llama.attention.head_count_kv", 8)
        .tensor("token_embd.weight", &[embed, 32_000], 12, 0);
    let ff = embed * 4;
    for i in 0..blocks {
        for (name, dims) in [
            ("attn_q", [embed, embed]),
            ("attn_k", [embed, embed]),
            ("attn_v", [embed, embed]),
            ("attn_output", [embed, embed]),
            ("ffn_gate", [embed, ff]),
            ("ffn_up", [embed, ff]),
            ("ffn_down", [ff, embed]),
        ] {
            b = b.tensor(&format!("blk.{i}.{name}.weight"), &dims, 12, 1_000 + i as u64);
        }
    }
    b.build()
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-models-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A folder with a small and a large model in it.
fn a_folder(tag: &str) -> PathBuf {
    let dir = tmp(tag);
    std::fs::write(dir.join("small-1b.gguf"), a_model(1024, 16, 8192)).unwrap();
    std::fs::write(dir.join("large-7b.gguf"), a_model(4096, 32, 8192)).unwrap();
    dir
}

fn cfg_at(dir: &Path) -> ModelsConfig {
    ModelsConfig { dir: dir.display().to_string(), context: 4096, ..Default::default() }
}

fn machine(free_mb: u64, vram_mb: u64) -> Machine {
    Machine {
        total_ram_mb: free_mb * 2,
        free_ram_mb: free_mb,
        cpu_cores: 8,
        vram_mb,
        disk_free_mb: 100_000,
        ..Default::default()
    }
}

// ===================== the budget is measured ===========================

#[test]
fn the_same_config_gives_different_budgets_on_different_machines() {
    // The whole point. A config number is the same everywhere; a machine is
    // not, and the budget is a fact about the machine.
    let cfg = ModelsConfig::default();
    let big = budget_bytes(&cfg, &machine(32_000, 0));
    let small = budget_bytes(&cfg, &machine(2_000, 0));
    assert!(big > small, "the budget did not follow the machine");
}

#[test]
fn a_configured_budget_may_only_lower_what_was_measured() {
    // "Never use more than this even if there is room" is a real thing to
    // want. "Use more than the machine has" produces a model that will not
    // load, which is the failure `fit.rs` exists to prevent.
    let m = machine(32_000, 0);
    let measured = budget_bytes(&ModelsConfig { memory_budget_mb: 0, ..Default::default() }, &m);

    let capped =
        budget_bytes(&ModelsConfig { memory_budget_mb: 1_000, ..Default::default() }, &m);
    assert_eq!(capped, 1_000 * 1024 * 1024, "the cap was ignored");

    let greedy =
        budget_bytes(&ModelsConfig { memory_budget_mb: 999_999, ..Default::default() }, &m);
    assert_eq!(greedy, measured, "a config raised the budget past the machine");

    // 1 Oct 2026: `memory_budget_mb: '2'` in the settings left Atlas with no
    // model ("there is 2MB spare", 5 GB free). Up to 64 can only mean GB.
    let gb = budget_bytes(&ModelsConfig { memory_budget_mb: 2, ..Default::default() }, &m);
    assert_eq!(gb, 2 * 1024 * 1024 * 1024);
    assert_eq!(atlas::models::budget_set_mb(256), 256, "a real MB figure stays one");
}

#[test]
fn zero_means_whatever_this_machine_can_spare() {
    let m = machine(16_000, 0);
    let cfg = ModelsConfig { memory_budget_mb: 0, ..Default::default() };
    assert_eq!(budget_bytes(&cfg, &m), m.budget_mb() * 1024 * 1024);
}

// ===================== scanning a real folder ===========================

#[test]
fn a_folder_of_real_gguf_files_is_read() {
    let dir = a_folder("scan");
    let (registry, trouble) = Registry::scan_reporting(&dir);
    assert!(trouble.is_none(), "a good folder reported trouble: {trouble:?}");
    assert_eq!(registry.models.len(), 2, "the scan did not read both files");
    assert!(registry.get("small-1b").is_some(), "ids come from the filename stem");
}

#[test]
fn a_missing_folder_is_reported_rather_than_read_as_no_models_installed() {
    // These are very different problems and the old empty-registry answer sent
    // you to the wrong one. `scan_reporting` exists for this and had never
    // reported anything to anyone.
    let (registry, trouble) = Registry::scan_reporting(Path::new("/definitely/not/here"));
    assert!(registry.models.is_empty());
    assert!(trouble.is_some(), "a missing folder was read as an empty one");
}

#[test]
fn one_bad_download_does_not_hide_the_good_models() {
    let dir = a_folder("corrupt");
    std::fs::write(dir.join("half-downloaded.gguf"), b"GGUF\x03\x00\x00").unwrap();
    let (registry, _) = Registry::scan_reporting(&dir);
    assert_eq!(registry.models.len(), 2, "a truncated file took the others with it");
}

// ===================== what fits ========================================

#[test]
fn a_roomy_machine_gets_the_larger_model_and_a_cramped_one_gets_the_smaller() {
    let dir = a_folder("fit");
    // 27 Sep 2026: with no ceiling. The default now stops at the talk
    // ceiling (`a_conversation_gets_the_quick_model_not_the_biggest`); what
    // this test protects -- the choice follows the machine -- is unchanged.
    let cfg = ModelsConfig { talk_ceiling_b: 0, ..cfg_at(&dir) };
    let (registry, _) = Registry::scan_reporting(&dir);

    let roomy = machine(64_000, 0);
    let big = registry.choose_for(&cfg, budget_bytes(&cfg, &roomy)).expect("nothing fit a big machine");

    let tight = machine(3_000, 0);
    let small = registry.choose_for(&cfg, budget_bytes(&cfg, &tight));

    assert_eq!(big.id, "large-7b");
    match small {
        Some(m) => assert_eq!(m.id, "small-1b", "a cramped machine was given the large model"),
        None => {}
    }
    assert!(
        small.map(|m| m.parameters).unwrap_or(0) < big.parameters,
        "the same model was chosen for both machines"
    );
}

#[test]
fn a_machine_too_small_for_anything_says_so_rather_than_choosing_badly() {
    let dir = a_folder("nothing-fits");
    let cfg = cfg_at(&dir);
    let (registry, _) = Registry::scan_reporting(&dir);
    let m = machine(200, 0);

    assert!(registry.choose_for(&cfg, budget_bytes(&cfg, &m)).is_none());
    let said = registry.explain_for(&cfg, &m, budget_bytes(&cfg, &m));
    assert!(said.starts_with("Nothing fits"), "got: {said}");
    assert!(said.contains("smallest"), "it did not say how close it got: {said}");
}

#[test]
fn asking_for_a_model_by_name_gets_that_one() {
    let dir = a_folder("prefer");
    let mut cfg = cfg_at(&dir);
    cfg.prefer = "small-1b".into();
    let (registry, _) = Registry::scan_reporting(&dir);
    let m = machine(64_000, 0);

    let chosen = registry.choose_for(&cfg, budget_bytes(&cfg, &m)).unwrap();
    assert_eq!(chosen.id, "small-1b", "a preference was overruled by what fits");
    assert!(registry.explain_for(&cfg, &m, budget_bytes(&cfg, &m)).contains("you asked for this one"));
}

#[test]
fn the_explanation_names_the_number_it_was_deciding_against() {
    // "It used the small one" is only not a mystery if you can see the budget.
    let dir = a_folder("why");
    let cfg = cfg_at(&dir);
    let (registry, _) = Registry::scan_reporting(&dir);
    let m = machine(64_000, 0);
    let said = registry.explain_for(&cfg, &m, budget_bytes(&cfg, &m));
    assert!(said.contains("MB spare"), "the budget was not stated: {said}");
    assert!(said.contains("needs ~"), "what it costs was not stated: {said}");
}

#[test]
fn an_empty_folder_says_what_machine_it_was_looking_at() {
    let dir = tmp("empty");
    let cfg = cfg_at(&dir);
    let (registry, _) = Registry::scan_reporting(&dir);
    let m = machine(16_000, 0);
    let said = registry.explain_for(&cfg, &m, budget_bytes(&cfg, &m));
    assert!(said.contains("No models"), "got: {said}");
    assert!(said.contains("spare"), "it did not say what it had to work with: {said}");
}

// ===================== graphics =========================================

#[test]
fn integrated_graphics_get_no_layers_offloaded() {
    // `Machine::usable_vram_mb` exists because integrated graphics share
    // system memory. Counting that as spare video memory offloads more than
    // fits, and the driver starts paging — slower than offloading nothing.
    //
    // 27 Sep 2026: with a processor-only server (none configured here). A
    // graphics build gets every layer:
    // `a_graphics_build_of_the_server_puts_the_model_on_the_graphics`.
    let dir = a_folder("igpu");
    let (registry, _) = Registry::scan_reporting(&dir);
    let cfg = cfg_at(&dir);
    let model = registry.get("small-1b").unwrap();

    assert_eq!(layers_here(model, &cfg, &machine(16_000, 0)), 0);
}

#[test]
fn a_conversation_gets_the_quick_model_not_the_biggest() {
    // Eric, 27 Sep 2026: talking to Atlas felt slow. The largest model that
    // fits is the slowest one that fits; setup downloads a 4B for talking,
    // and anything bigger dropped in the folder used to win the moment
    // memory allowed. Under the ceiling, the largest; none under it, the
    // smallest over it; named, exactly that one.
    let dir = a_folder("talk-ceiling");
    let cfg = cfg_at(&dir);
    let (registry, _) = Registry::scan_reporting(&dir);
    let roomy = machine(64_000, 0);
    let chosen = registry.choose_for(&cfg, budget_bytes(&cfg, &roomy)).unwrap();
    assert_eq!(chosen.id, "small-1b", "a roomy machine talked through the big model by default");

    let only_big = tmp("talk-ceiling-only-big");
    std::fs::write(only_big.join("large-7b.gguf"), a_model(4096, 32, 8192)).unwrap();
    std::fs::write(only_big.join("larger.gguf"), a_model(4096, 48, 8192)).unwrap();
    let (big_ones, _) = Registry::scan_reporting(&only_big);
    let cfg_big = cfg_at(&only_big);
    let chosen = big_ones.choose_for(&cfg_big, budget_bytes(&cfg_big, &roomy)).unwrap();
    assert_eq!(chosen.id, "large-7b", "nothing under the ceiling: the smallest over it, not the largest");

    let named = ModelsConfig { prefer: "large-7b".into(), ..cfg_at(&dir) };
    assert_eq!(registry.choose_for(&named, budget_bytes(&named, &roomy)).unwrap().id, "large-7b");
}

#[test]
fn a_graphics_build_of_the_server_puts_the_model_on_the_graphics() {
    // 27 Sep 2026: `fit::measure` never sees a card's memory, so the layers
    // came out 0 on every machine -- and setup downloads llama.cpp's Vulkan
    // build to use the laptop's own (integrated, shared-memory) graphics.
    // The model ran on the processor alone.
    let dir = a_folder("vulkan-build");
    let (registry, _) = Registry::scan_reporting(&dir);
    let model = registry.get("small-1b").unwrap();
    let tools = tmp("vulkan-build-tools");
    let server = tools.join("llama-server.exe");
    std::fs::write(&server, b"").unwrap();
    std::fs::write(tools.join("ggml-vulkan.dll"), b"").unwrap();
    let with_server = |gpu: &str| ModelsConfig {
        server: Some(atlas::tools::ExternalTool { command: server.display().to_string(), ..Default::default() }),
        gpu_layers: gpu.into(),
        ..cfg_at(&dir)
    };
    assert!(atlas::models::is_graphics_build(&server), "the Vulkan library beside it was not noticed");
    assert_eq!(layers_here(model, &with_server("auto"), &machine(16_000, 0)), atlas::models::ALL_LAYERS);
    assert_eq!(layers_here(model, &with_server("off"), &machine(16_000, 0)), 0, "off was not honoured");
    assert_eq!(layers_here(model, &with_server("20"), &machine(16_000, 0)), 20, "a number was not honoured");

    // A processor-only build beside no graphics library gets none.
    let plain = tmp("cpu-build-tools");
    let cpu_server = plain.join("llama-server.exe");
    std::fs::write(&cpu_server, b"").unwrap();
    std::fs::write(plain.join("ggml-cpu.dll"), b"").unwrap();
    assert!(!atlas::models::is_graphics_build(&cpu_server));
    let cpu_cfg = ModelsConfig {
        server: Some(atlas::tools::ExternalTool { command: cpu_server.display().to_string(), ..Default::default() }),
        ..cfg_at(&dir)
    };
    assert_eq!(layers_here(model, &cpu_cfg, &machine(16_000, 0)), 0);
}

#[test]
fn a_real_graphics_card_gets_layers() {
    let dir = a_folder("gpu");
    let (registry, _) = Registry::scan_reporting(&dir);
    let cfg = cfg_at(&dir);
    let model = registry.get("small-1b").unwrap();

    let with_card = layers_here(model, &cfg, &machine(32_000, 24_000));
    assert!(with_card > 0, "a card with 24GB was given no layers");
}

// ===================== reached from the running program =================

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon_with<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn the_phrases_for_asking_reach_the_registry() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&cfg.commands);
    for said in ["which model", "what can you run", "what models do i have"] {
        assert_eq!(parser.parse(said), Intent::WhichModel, "did not reach it: {said}");
    }
}

#[test]
fn a_real_daemon_scans_a_real_folder_and_says_what_it_found() {
    let dir = a_folder("daemon");
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().models.dir = dir.display().to_string();
    let p = plat();
    let mut d = daemon_with(&c, &p, "daemon-store");

    let said = d.which_model();
    assert!(
        said.contains("small-1b") || said.contains("large-7b") || said.starts_with("Nothing fits"),
        "the daemon did not read the folder: {said}"
    );
    assert!(!said.contains("can't read"), "a good folder was reported unreadable: {said}");
}

#[test]
fn a_daemon_with_no_models_folder_says_which_problem_it_is() {
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().models.dir = "/definitely/not/here".into();
    let p = plat();
    let mut d = daemon_with(&c, &p, "missing-store");

    let said = d.which_model();
    assert!(said.contains("can't read the models folder"), "got: {said}");
}

#[test]
fn having_a_model_but_no_server_is_said_rather_than_implied() {
    // The ordinary case on a machine with GGUF files and no llama-server:
    // Atlas knows what it *would* run and cannot run it. Silence there reads
    // as "it's working".
    let dir = a_folder("noserver");
    let mut c = Config::load(Path::new("config")).unwrap();
    let models = &mut c.tools.as_mut().unwrap().models;
    models.dir = dir.display().to_string();
    models.server = None;
    let p = plat();
    let mut d = daemon_with(&c, &p, "noserver-store");

    let said = d.which_model();
    if !said.starts_with("Nothing fits") {
        assert!(said.contains("no llama-server"), "got: {said}");
    }
}

// ===================== the estimate both halves share ===================

#[test]
fn the_chooser_and_the_supervisor_size_the_same_model_the_same_way() {
    // `footprint_mb` feeds `lifecycle`'s budget and `estimate_memory` feeds
    // `best_fit`. If they disagreed, Atlas would choose a model the
    // supervisor then refuses to start.
    let dir = a_folder("agree");
    let (registry, _) = Registry::scan_reporting(&dir);
    let cfg = cfg_at(&dir);
    let m: &Model = registry.get("large-7b").unwrap();
    assert_eq!(
        atlas::models::footprint_mb(m, &cfg),
        estimate_memory(m, cfg.context) / (1024 * 1024)
    );
}

// ===================== the connection Atlas builds itself ===============

/// The fetch tool from the shipped config, which is how Atlas makes any HTTP
/// request — one way of doing it, configured once.
fn http() -> atlas::tools::ExternalTool {
    let c = Config::load(Path::new("config")).unwrap();
    c.tools.as_ref().unwrap().research.fetch.clone().expect("a fetch tool in the shipped config")
}

#[test]
fn the_derived_connection_points_at_llama_server_not_ollama() {
    let dir = a_folder("derived");
    let cfg = cfg_at(&dir);
    let (registry, _) = Registry::scan_reporting(&dir);
    let model = registry.get("small-1b").unwrap();

    let lc = atlas::models::llm_config_for(model, &cfg, &http());
    assert_eq!(
        lc.response_path, "content",
        "llama.cpp returns text at `content`; `response` is Ollama's shape"
    );
    let joined = lc.tool.args.join(" ");
    assert!(
        joined.contains(&format!("127.0.0.1:{}", cfg.port)),
        "the endpoint was not resolved into the request: {joined}"
    );
    assert!(!joined.contains("{url}"), "an unresolved placeholder was left in: {joined}");
    assert!(!joined.to_lowercase().contains("ollama"));
}

#[test]
fn the_prompt_is_wrapped_the_way_this_model_expects() {
    // The whole reason this is derived rather than hand-written. A body with
    // the wrong control tokens still gets answers, just worse ones, and
    // nothing tells you.
    let dir = tmp("template");
    let mut bytes = Builder::default()
        .kv_str("general.architecture", "llama")
        .kv_u32("llama.context_length", 4096)
        .kv_u32("llama.block_count", 4)
        .kv_u32("llama.embedding_length", 512)
        .kv_str("tokenizer.chat_template", "<|im_start|>system")
        .tensor("token_embd.weight", &[512, 32_000], 12, 0)
        .build();
    bytes.shrink_to_fit();
    std::fs::write(dir.join("chatml-model.gguf"), bytes).unwrap();

    let cfg = cfg_at(&dir);
    let (registry, _) = Registry::scan_reporting(&dir);
    let model = registry.get("chatml-model").expect("the model was not read");
    let lc = atlas::models::llm_config_for(model, &cfg, &http());

    assert!(lc.request.contains("im_start"), "the model's own template was ignored: {}", lc.request);
    assert!(lc.request.contains("{system}"), "the placeholders ShellLlm fills were lost");
    assert!(lc.request.contains("{user}"));
}

#[test]
fn a_hand_written_connection_is_never_silently_overridden() {
    // If you have written a connection you mean it. This asserts the
    // precedence in `main.rs` rather than the behaviour, because the
    // behaviour needs a running server — and the precedence is the part that
    // would quietly do the wrong thing.
    let src = crate::common::source_of("main");
    // The precedence lived inline in `run_daemon`, which is part of why the
    // *other* front door never had a model at all: `atlas voice` could not
    // reach fifty lines of logic nested in another function, so it didn't,
    // and answered five intents out of fifty-six. It is now
    // `fn model_connection`, called by both doors — so this checks the
    // helper, and checks that both doors use it, which is a stronger claim
    // than the old one could make.
    // Since 27 Sep 2026 the helper is the library's `models::connection`, so
    // the phone core builds the same one (it had none: OPEN_GAPS P.7); main's
    // `model_connection` hands straight to it.
    let at = src.find("fn model_connection").expect("the derived connection is gone");
    assert!(src[at..at + 400].contains("atlas::models::connection(tc)"), "main's doors no longer use the library's connection");
    let lib = std::fs::read_to_string("src/models.rs").expect("models.rs");
    let at = lib.find("pub fn connection(").expect("models::connection is gone");
    let body = &lib[at..at + 1600];
    let mobile = std::fs::read_to_string("src/mobile.rs").expect("mobile.rs");
    assert!(mobile.contains("crate::models::connection(tools)"), "the phone core builds no model connection again");
    assert!(
        body.contains("if tc.llm.is_some() {") && body.contains("None"),
        "a hand-written tools.llm no longer wins over the derived one"
    );
    assert!(
        body.contains("tc.llm") && body.contains(".or(derived.as_ref())"),
        "the derived connection is built and then not used"
    );
    assert_eq!(
        src.matches("model_connection(").count(),
        // The definition, the daemon, `atlas voice`, `atlas fix`, the
        // one-shot `handle`, and the typing prompt's Atlas (26 Sep 2026: the
        // last two were built with no model).
        6,
        "every door and the definition — if this drops, one door has no model again"
    );
}

#[test]
fn a_machine_with_no_models_derives_no_connection() {
    // Nothing to point at. This must stay `None` rather than becoming a
    // connection to a server that will never answer.
    let dir = tmp("none-derived");
    let cfg = cfg_at(&dir);
    let (registry, _) = Registry::scan_reporting(&dir);
    assert!(registry.choose_for(&cfg, 64 << 30).is_none());
}

#[test]
fn both_endpoints_are_on_the_loopback_and_never_a_public_interface() {
    // `server_args` binds the server to 127.0.0.1 deliberately — a model
    // server on 0.0.0.0 is an open prompt endpoint on whatever network the
    // machine is on. These two build the URLs Atlas then talks to, and they
    // have to agree with that binding or Atlas is reaching for an address the
    // server was deliberately not given.
    let port = 8123;
    for url in [atlas::models::health_url(port), atlas::models::completion_url(port)] {
        assert!(url.starts_with("http://127.0.0.1:"), "not loopback: {url}");
        assert!(url.contains(&port.to_string()), "the configured port was ignored: {url}");
    }
    assert!(atlas::models::health_url(port).ends_with("/health"));
    assert!(atlas::models::completion_url(port).ends_with("/completion"));
}
