use atlas::gguf::{Gguf, QuantType, Value};
use atlas::models::{
    completion_body, estimate_memory, gpu_layers_for, server_args, Model, ModelsConfig, Registry,
    Template,
};
use std::io::Cursor;
use std::path::PathBuf;

// --- a GGUF writer, so the reader is tested against real bytes ---

#[derive(Default)]
struct Builder {
    kv: Vec<(String, Vec<u8>)>,
    tensors: Vec<Vec<u8>>,
    n_tensors: u64,
}

fn u32b(v: u32) -> Vec<u8> { v.to_le_bytes().to_vec() }
fn u64b(v: u64) -> Vec<u8> { v.to_le_bytes().to_vec() }
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
    fn kv_str_array(mut self, key: &str, items: &[&str]) -> Self {
        let mut b = u32b(9);
        b.extend(u32b(8));
        b.extend(u64b(items.len() as u64));
        for s in items {
            b.extend(strb(s));
        }
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

fn llama7b() -> Vec<u8> {
    Builder::default()
        .kv_str("general.architecture", "llama")
        .kv_str("general.name", "TestLlama 7B")
        .kv_u32("llama.context_length", 8192)
        .kv_u32("llama.block_count", 32)
        .kv_u32("llama.embedding_length", 4096)
        .kv_u32("llama.attention.head_count", 32)
        .kv_u32("llama.attention.head_count_kv", 32)
        .kv_str("tokenizer.chat_template", "<|im_start|>system")
        .tensor("token_embd.weight", &[4096, 32000], 12, 0)
        .tensor("blk.0.attn_q.weight", &[4096, 4096], 12, 1000)
        .build()
}

// ================= reading a model file =================

#[test]
fn a_gguf_header_round_trips() {
    let g = Gguf::read(Cursor::new(llama7b())).unwrap();
    assert_eq!(g.version, 3);
    assert_eq!(g.architecture(), Some("llama"));
    assert_eq!(g.name(), Some("TestLlama 7B"));
    assert_eq!(g.context_length(), Some(8192));
    assert_eq!(g.block_count(), Some(32));
    assert_eq!(g.tensors.len(), 2);
}

#[test]
fn a_file_that_is_not_a_model_is_rejected_clearly() {
    let e = Gguf::read(Cursor::new(b"not a model at all".to_vec())).unwrap_err().to_string();
    assert!(e.contains("GGUF"), "should name the problem: {e}");
}

#[test]
fn an_unsupported_version_is_refused_rather_than_misread() {
    let mut bytes = llama7b();
    bytes[4..8].copy_from_slice(&99u32.to_le_bytes());
    assert!(Gguf::read(Cursor::new(bytes)).is_err());
}

#[test]
fn a_corrupt_header_cannot_make_atlas_allocate_wildly() {
    let mut bytes = llama7b();
    bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes()); // tensor_count
    let e = Gguf::read(Cursor::new(bytes)).unwrap_err().to_string();
    assert!(e.contains("implausible"), "got: {e}");
}

#[test]
fn tensor_shapes_and_quantization_are_read() {
    let g = Gguf::read(Cursor::new(llama7b())).unwrap();
    let t = &g.tensors[0];
    assert_eq!(t.name, "token_embd.weight");
    assert_eq!(t.dims, vec![4096, 32000]);
    assert_eq!(t.kind.name(), "Q4_K");
    assert_eq!(t.elements(), 4096 * 32000);
}

#[test]
fn quantization_names_and_widths_are_right() {
    assert_eq!(QuantType(1).name(), "F16");
    assert_eq!(QuantType(1).bits_per_weight(), 16.0);
    assert_eq!(QuantType(12).name(), "Q4_K");
    assert!(QuantType(12).bits_per_weight() < 5.0, "Q4 must be about 4 bits");
    assert!(QuantType(14).bits_per_weight() > QuantType(12).bits_per_weight());
}

#[test]
fn the_dominant_quantization_is_what_the_bulk_of_the_weights_use() {
    let bytes = Builder::default()
        .kv_str("general.architecture", "llama")
        .tensor("big.weight", &[4096, 32000], 12, 0)   // Q4_K, huge
        .tensor("small.norm", &[4096], 0, 100)          // F32, tiny
        .build();
    let g = Gguf::read(Cursor::new(bytes)).unwrap();
    assert_eq!(g.dominant_quant().name(), "Q4_K", "one F32 norm doesn't make it an F32 model");
}

#[test]
fn a_huge_vocabulary_is_counted_without_being_held_in_memory() {
    // Real models list 100k+ tokens. Loading them all to answer "how big is
    // this" would be absurd, so long arrays are skipped and counted.
    let tokens: Vec<String> = (0..9000).map(|i| format!("tok{i}")).collect();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let bytes = Builder::default()
        .kv_str("general.architecture", "llama")
        .kv_str_array("tokenizer.ggml.tokens", &refs)
        .kv_u32("llama.context_length", 4096)
        .build();
    let g = Gguf::read(Cursor::new(bytes)).unwrap();
    assert_eq!(g.get("tokenizer.ggml.tokens").map(|v| v.len()), Some(9000));
    assert_eq!(g.context_length(), Some(4096), "parsing continued past the big array");
}

#[test]
fn small_arrays_are_kept_in_full() {
    let bytes = Builder::default()
        .kv_str("general.architecture", "llama")
        .kv_str_array("small.list", &["a", "b", "c"])
        .build();
    let g = Gguf::read(Cursor::new(bytes)).unwrap();
    match g.get("small.list") {
        Some(Value::Array(a)) => {
            assert_eq!(a.len(), 3);
            assert_eq!(a[1].as_str(), Some("b"));
        }
        other => panic!("expected an array, got {other:?}"),
    }
}

// ================= will it fit? =================

#[test]
fn the_kv_cache_is_counted_not_just_the_weights() {
    // The term people forget: a model that "fits" runs out of memory partway
    // through a long conversation because the cache grew.
    let g = Gguf::read(Cursor::new(llama7b())).unwrap();
    let short = g.memory_needed(2048);
    let long = g.memory_needed(32768);
    assert!(long > short, "a longer context must cost more");
    assert!(g.kv_cache_bytes(32768) > g.kv_cache_bytes(2048) * 10);
}

#[test]
fn kv_cache_scales_with_layers_and_heads() {
    let g = Gguf::read(Cursor::new(llama7b())).unwrap();
    // 2 (k+v) * 2 bytes * 32 layers * 32 kv heads * 128 head_dim * 1024 ctx
    assert_eq!(g.kv_cache_bytes(1024), 2 * 2 * 32 * 32 * 128 * 1024);
}

#[test]
fn a_model_with_no_architecture_metadata_does_not_panic() {
    let bytes = Builder::default().tensor("x", &[10], 0, 0).build();
    let g = Gguf::read(Cursor::new(bytes)).unwrap();
    assert_eq!(g.context_length(), None);
    assert_eq!(g.kv_cache_bytes(4096), 0, "unknown shape means no guess");
}

// ================= choosing a model =================

fn model(id: &str, params: u64, bytes: u64) -> Model {
    Model {
        path: PathBuf::from(format!("models/{id}.gguf")),
        id: id.into(),
        architecture: "llama".into(),
        quant: "Q4_K".into(),
        parameters: params,
        weight_bytes: bytes,
        max_context: 8192,
        chat_template: None,
    }
}

fn registry() -> Registry {
    Registry {
        models: vec![
            model("big-70b", 70_000_000_000, 40 * 1024 * 1024 * 1024),
            model("mid-8b", 8_000_000_000, 5 * 1024 * 1024 * 1024),
            model("small-3b", 3_000_000_000, 2 * 1024 * 1024 * 1024),
        ],
    }
}

#[test]
fn the_largest_model_that_fits_is_chosen() {
    let r = registry();
    let m = r.best_fit(8 * 1024 * 1024 * 1024, 4096).unwrap();
    assert_eq!(m.id, "mid-8b");
}

#[test]
fn a_model_that_would_swap_to_disk_is_not_chosen() {
    // Bigger is better only until it doesn't fit; past that it pages and
    // becomes unusable, which is worse than a smaller model that runs.
    let r = registry();
    assert_eq!(r.best_fit(3 * 1024 * 1024 * 1024, 4096).unwrap().id, "small-3b");
}

#[test]
fn nothing_fitting_is_a_clear_answer_not_a_crash() {
    let r = registry();
    assert!(r.best_fit(100 * 1024 * 1024, 4096).is_none());
    let cfg = ModelsConfig { memory_budget_mb: 100, ..Default::default() };
    let why = r.explain(&cfg);
    assert!(why.contains("nothing fits"), "got: {why}");
    assert!(why.contains("small-3b"), "names the closest option: {why}");
}

#[test]
fn an_explicit_preference_wins_over_the_automatic_pick() {
    let r = registry();
    let cfg = ModelsConfig { prefer: "small-3b".into(), memory_budget_mb: 64_000, ..Default::default() };
    assert_eq!(r.choose(&cfg).unwrap().id, "small-3b");
}

#[test]
fn a_longer_context_can_change_which_model_fits() {
    let r = registry();
    let budget = 6_500 * 1024 * 1024;
    let short = r.best_fit(budget, 2048).map(|m| m.id.clone());
    let long = r.best_fit(budget, 8192).map(|m| m.id.clone());
    assert_ne!(short, long, "context length must affect the choice");
}

#[test]
fn an_empty_models_directory_says_so() {
    let r = Registry::default();
    assert!(r.explain(&ModelsConfig::default()).contains("no GGUF models"));
    // Contrast case, not just the phrase: a registry that actually has a
    // model must not say the directory is empty.
    let d = std::env::temp_dir().join("atlas-models-empty-contrast");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("good.gguf"), llama7b()).unwrap();
    let with_one = Registry::scan(&d);
    assert_eq!(with_one.models.len(), 1);
    assert!(!with_one.explain(&ModelsConfig::default()).contains("no GGUF models"));
}

#[test]
fn scanning_a_directory_skips_unreadable_files_instead_of_aborting() {
    let d = std::env::temp_dir().join("atlas-models-test");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("good.gguf"), llama7b()).unwrap();
    std::fs::write(d.join("broken.gguf"), b"garbage").unwrap();
    std::fs::write(d.join("notes.txt"), b"ignore me").unwrap();

    let r = Registry::scan(&d);
    assert_eq!(r.models.len(), 1, "one bad download must not hide the rest");
    assert_eq!(r.models[0].id, "good");
    assert_eq!(r.models[0].architecture, "llama");
}

#[test]
fn parameter_counts_get_a_human_label() {
    assert_eq!(model("x", 7_000_000_000, 0).size_label(), "7B");
    assert_eq!(model("x", 400_000_000, 0).size_label(), "400M");
}

// ================= prompt formatting =================

#[test]
fn the_template_is_taken_from_the_model_file_when_it_has_one() {
    assert_eq!(Template::detect(Some("<|im_start|>system"), "whatever"), Template::ChatMl);
    assert_eq!(Template::detect(Some("<|start_header_id|>"), "whatever"), Template::Llama3);
    assert_eq!(Template::detect(Some("[INST]"), "whatever"), Template::Mistral);
    assert_eq!(Template::detect(Some("<start_of_turn>"), "whatever"), Template::Gemma);
}

#[test]
fn the_model_name_is_the_fallback_when_the_file_carries_no_template() {
    assert_eq!(Template::detect(None, "qwen2.5-7b-instruct"), Template::ChatMl);
    assert_eq!(Template::detect(None, "Meta-Llama-3-8B"), Template::Llama3);
    assert_eq!(Template::detect(None, "mistral-7b"), Template::Mistral);
    assert_eq!(Template::detect(None, "something-unknown"), Template::Plain);
}

#[test]
fn each_format_produces_the_markers_its_model_was_trained_on() {
    // Feeding a model the wrong markers degrades it subtly, which is worse
    // than an outright failure because you don't notice.
    let c = Template::ChatMl.render("be brief", "hello");
    assert!(c.starts_with("<|im_start|>system"));
    assert!(c.ends_with("<|im_start|>assistant\n"));

    let l = Template::Llama3.render("be brief", "hello");
    assert!(l.starts_with("<|begin_of_text|>"));
    assert!(l.contains("<|eot_id|>"));

    let m = Template::Mistral.render("be brief", "hello");
    assert!(m.starts_with("[INST]") && m.ends_with("[/INST]"));
    assert!(m.contains("be brief"), "mistral has no system role, so it folds in");
}

#[test]
fn stop_tokens_match_the_format() {
    assert!(Template::ChatMl.stop_tokens().contains(&"<|im_end|>"));
    assert!(Template::Llama3.stop_tokens().contains(&"<|eot_id|>"));
    assert!(!Template::Gemma.stop_tokens().is_empty());
}

#[test]
fn the_completion_request_is_valid_json_with_the_right_stops() {
    let body = completion_body("say \"hi\"\nnow", Template::ChatMl, 256);
    let v: serde_json::Value = serde_json::from_str(&body).expect("must be valid json");
    assert_eq!(v["n_predict"], 256);
    assert_eq!(v["prompt"], "say \"hi\"\nnow", "quotes and newlines survive");
    assert!(v["stop"].as_array().unwrap().iter().any(|s| s == "<|im_end|>"));
}

// ================= running llama.cpp directly =================

#[test]
fn the_server_is_never_bound_to_a_public_interface() {
    let args = server_args(&model("m", 7_000_000_000, 4 << 30), &ModelsConfig::default(), 0);
    let host = args.iter().position(|a| a == "--host").map(|i| &args[i + 1]);
    assert_eq!(host.map(String::as_str), Some("127.0.0.1"), "must not listen publicly");
}

#[test]
fn the_context_never_exceeds_what_the_model_supports() {
    let m = model("m", 7_000_000_000, 4 << 30); // max_context 8192
    let cfg = ModelsConfig { context: 999_999, ..Default::default() };
    let args = server_args(&m, &cfg, 0);
    let ctx = args.iter().position(|a| a == "-c").map(|i| args[i + 1].clone()).unwrap();
    assert_eq!(ctx, "8192");
}

#[test]
fn a_context_of_zero_means_the_models_own_maximum() {
    let m = model("m", 7_000_000_000, 4 << 30);
    let cfg = ModelsConfig { context: 0, ..Default::default() };
    let args = server_args(&m, &cfg, 0);
    let ctx = args.iter().position(|a| a == "-c").map(|i| args[i + 1].clone()).unwrap();
    assert_eq!(ctx, "8192");
}

#[test]
fn with_no_gpu_nothing_is_offloaded() {
    let m = model("m", 7_000_000_000, 4 << 30);
    assert_eq!(gpu_layers_for(&m, 0, 4096), 0);
}

#[test]
fn a_model_that_fits_vram_is_fully_offloaded() {
    let m = model("m", 3_000_000_000, 2 << 30);
    assert_eq!(gpu_layers_for(&m, 24 << 30, 4096), 999);
}

#[test]
fn a_model_too_big_for_vram_is_only_partly_offloaded() {
    // Offloading more than fits is slower than offloading none, because the
    // driver starts paging.
    let m = model("m", 70_000_000_000, 40 << 30);
    let layers = gpu_layers_for(&m, 8 << 30, 4096);
    assert!(layers > 0 && layers < 999, "partial, not all or nothing: {layers}");
}

#[test]
fn memory_estimates_grow_with_both_size_and_context() {
    let small = model("s", 3_000_000_000, 2 << 30);
    let big = model("b", 8_000_000_000, 5 << 30);
    assert!(estimate_memory(&big, 4096) > estimate_memory(&small, 4096));
    assert!(estimate_memory(&small, 32768) > estimate_memory(&small, 2048));
}

#[test]
fn the_shipped_config_can_run_without_ollama() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();
    let server = t.models.server.clone().expect("a llama-server entry");
    assert!(
        !server.command.to_lowercase().contains("ollama"),
        "the direct path must not go through Ollama: {}", server.command
    );
    // The budget is *measured*, not configured. This used to assert the
    // opposite -- that the shipped config carried a number greater than zero
    // -- which encoded the assumption that a config file is where the amount
    // of free memory on a machine belongs. It is not: it stops being true the
    // moment you close a browser or run Atlas on the other machine.
    //
    // 0 means "whatever `fit.rs` measures here". A non-zero value is a cap
    // and may only lower what was measured, never raise it.
    assert_eq!(
        t.models.memory_budget_mb, 0,
        "the shipped config hardcodes a memory budget again; `fit.rs` measures it"
    );
    assert!(t.models.context > 0);

    // And the measured budget has to actually reach the chooser.
    let plenty = atlas::fit::Machine { total_ram_mb: 64_000, free_ram_mb: 32_000, ..Default::default() };
    let cramped = atlas::fit::Machine { total_ram_mb: 4_000, free_ram_mb: 900, ..Default::default() };
    assert!(
        atlas::models::budget_bytes(&t.models, &plenty)
            > atlas::models::budget_bytes(&t.models, &cramped),
        "the same config gives the same budget on two very different machines"
    );
}

#[test]
fn a_vision_projector_is_not_offered_as_a_model_to_talk_with() {
    // On Eric's laptop (26 Sep 2026) `mmproj-Qwen3VL-4B-…` — half of the
    // picture model — was named as "the smallest I have".
    let d = std::env::temp_dir().join(format!("atlas-models-projector-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("good.gguf"), llama7b()).unwrap();
    let clip = Builder::default()
        .kv_str("general.architecture", "clip")
        .tensor("v.patch_embd.weight", &[1024, 1024], 8, 0)
        .build();
    std::fs::write(d.join("mmproj-Something-Q8_0.gguf"), clip).unwrap();
    let r = Registry::scan(&d);
    let ids: Vec<&str> = r.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["good"], "{ids:?}");
    assert!(model("mmproj-x", 1, 1).is_a_projector(), "named as one, whatever it says inside");
}
