//! Item 20 (Eric's yes, 1 Oct 2026): the search and voice models on the
//! laptop's NPU, through ONNX Runtime and Intel's OpenVINO plugin.
//!
//! What can be checked off the laptop is checked here: the download is
//! pinned and only fetched where there's an Intel NPU, the fixed shapes are
//! written the way the provider reads them, and -- when a real ONNX Runtime
//! and the models are put in `ATLAS_ORT_TEST_ROOT` -- the runtime path
//! gives the same vectors `tract` does. The NPU itself is measured on the
//! laptop with `atlas npu-check`.

use atlas::npu;

#[test]
fn the_plugin_is_pinned_and_lands_where_atlas_looks() {
    // Built for every platform so the pin is checked everywhere; only
    // Windows fetches it.
    if let Some(p) = npu::npu_piece() {
        assert_eq!(p.sha256.len(), 64);
        assert_eq!(p.bytes, 117_914_126);
        assert_eq!(p.key_path(), npu::PLUGIN);
        assert!(p.url.starts_with("https://api.nuget.org/"), "{}", p.url);
    } else {
        assert!(!cfg!(all(windows, target_arch = "x86_64")));
    }
}

#[test]
fn only_a_computer_with_an_intel_npu_fetches_it() {
    assert!(npu::is_intel_npu("Intel(R) AI Boost"));
    assert!(npu::is_intel_npu("Intel(R) NPU"));
    assert!(!npu::is_intel_npu("AMD IPU Device"));
    assert!(!npu::is_intel_npu("Qualcomm(R) Hexagon(TM) NPU"));
    if !npu::has_intel_npu() {
        assert!(npu::pieces().is_empty());
    }
}

#[test]
fn fixed_shapes_are_written_the_way_the_provider_reads_them() {
    let v = npu::reshape_value(&[("input_ids".into(), vec![1, 16]), ("attention_mask".into(), vec![1, 16])]);
    assert_eq!(v, "input_ids[1,16],attention_mask[1,16]");
}

#[test]
fn agreement_is_cosine_for_unit_vectors() {
    assert!((npu::agreement(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
    assert!(npu::agreement(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
    assert_eq!(npu::agreement(&[1.0], &[1.0, 0.0]), 0.0);
}

#[test]
fn pooling_a_flat_output_matches_the_mean_over_real_tokens() {
    // Two real tokens of width 2, two pads that must not count.
    let hidden = [1.0, 0.0, 3.0, 0.0, 9.0, 9.0, 9.0, 9.0];
    let v = atlas::meaningnative::pool_flat(&hidden, 2, 4);
    assert_eq!(v, vec![1.0, 0.0]);
}

#[test]
fn without_the_runtime_the_reason_is_said_and_nothing_breaks() {
    let empty = std::env::temp_dir().join(format!("atlas-npu-none-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&empty);
    // On this machine the engine may already be set up by another test (it
    // is once per process); either way the check names a reason or none.
    let report = npu::check(&empty);
    assert!(report.contains("NPU engine:"), "{report}");
    assert!(report.contains("Search:") && report.contains("Voice ID:"), "{report}");
}

/// With a real ONNX Runtime and the two models in `ATLAS_ORT_TEST_ROOT`
/// (laid out as an install: `tools/kokoro/libonnxruntime.so` or the .dll,
/// `models/understanding/...`, `models/campplus_en_voxceleb.onnx`), the
/// runtime path gives tract's answers. Skipped when it isn't set.
#[test]
fn the_runtime_path_gives_the_same_answers_as_tract() {
    let Ok(root) = std::env::var("ATLAS_ORT_TEST_ROOT") else { return };
    let root = std::path::PathBuf::from(root);
    let model = root.join(atlas::meaningnative::MODEL);
    let names = npu::Session::input_names(&root, &model).expect("ONNX Runtime opens the model");
    assert_eq!(names.len(), 3, "{names:?}");
    let n = 32i64;
    let shapes: Vec<(String, Vec<i64>)> = names.iter().map(|x| (x.clone(), vec![1, n])).collect();
    let s = npu::Session::open(&root, &model, &shapes, npu::Where::Cpu).expect("opens on the processor");
    assert_eq!(s.on, npu::Where::Cpu);
    // "hello world" as the encoder's own ids, padded to 32.
    let mut enc = atlas::meaningnative::Native::load(&root).expect("model and words");
    let tract = enc.embed_on_processor("hello world").unwrap();
    let ids = vec![101i64, 7592, 2088, 102];
    let mut padded = ids.clone();
    padded.resize(n as usize, 0);
    let mut mask = vec![1i64; ids.len()];
    mask.resize(n as usize, 0);
    let out = s
        .run(vec![
            npu::In::I64(names[0].clone(), vec![1, n], padded),
            npu::In::I64(names[1].clone(), vec![1, n], mask),
            npu::In::I64(names[2].clone(), vec![1, n], vec![0; n as usize]),
        ])
        .expect("runs");
    let ort = atlas::meaningnative::pool_flat(&out[0], ids.len(), n as usize);
    let agree = npu::agreement(&tract, &ort);
    assert!(agree > 0.9999, "ONNX Runtime and tract disagree: {agree}");

    // The voice model on 200-frame windows.
    let voice = root.join("models").join("campplus_en_voxceleb.onnx");
    let vnames = npu::Session::input_names(&root, &voice).unwrap();
    let vs = npu::Session::open(&root, &voice, &[(vnames[0].clone(), vec![1, 200, 80])], npu::Where::Cpu).unwrap();
    let frames: Vec<f32> = (0..200 * 80).map(|i| ((i % 97) as f32 / 97.0) - 0.5).collect();
    let out = vs.run(vec![npu::In::F32(vnames[0].clone(), vec![1, 200, 80], frames)]).unwrap();
    assert_eq!(out[0].len(), 512);
}

#[test]
fn the_free_dimensions_are_fixed_by_their_own_names() {
    // MiniLM's export: input_ids is [batch_size, sequence_length].
    let fixed = npu::free_dimensions_of(&["batch_size".into(), "sequence_length".into()], &[1, 32]);
    assert_eq!(fixed, vec![("batch_size".to_string(), 1), ("sequence_length".to_string(), 32)]);
    // An already-fixed dimension has no name and is left alone.
    let fixed = npu::free_dimensions_of(&["".into(), "frames".into(), "".into()], &[1, 200, 80]);
    assert_eq!(fixed, vec![("frames".to_string(), 200)]);
}

#[test]
fn the_npu_is_kept_only_when_it_agrees_and_is_quicker() {
    use std::time::Duration;
    let ms = Duration::from_millis;
    // Measured on the laptop before the fix: 326 ms against 20 -- dropped.
    assert!(!npu::worth_keeping(ms(326), ms(20), 1.0));
    assert!(npu::worth_keeping(ms(8), ms(20), 0.9999));
    // Quicker but a different answer: dropped.
    assert!(!npu::worth_keeping(ms(8), ms(20), 0.9));
}

#[test]
fn the_sizes_are_written_into_a_copy_of_the_model() {
    // A tiny ONNX model, hand-encoded: graph { input { name: "x", type {
    // tensor_type { elem_type: 1, shape { dim { dim_param: "batch" } dim {
    // dim_param: "len" } } } } } }, plus a field the rewrite must keep (ir_version 8).
    fn ld(field: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![(field << 3) | 2, body.len() as u8];
        v.extend_from_slice(body);
        v
    }
    let dim_batch = ld(1, &ld(2, b"batch"));
    let dim_len = ld(1, &ld(2, b"len"));
    let shape = ld(2, &[dim_batch, dim_len].concat());
    let tensor = ld(1, &[vec![0x08, 0x01], shape].concat());
    let value = [ld(1, b"x"), ld(2, &tensor)].concat();
    let graph = ld(7, &ld(11, &value));
    let model = [vec![0x08, 0x08], graph].concat();

    assert_eq!(atlas::onnxfix::input_shapes(&model).unwrap(), vec![("x".to_string(), vec![-1, -1])]);
    let fixed = atlas::onnxfix::with_fixed_inputs(&model, &[("x".into(), vec![1, 32])]).unwrap();
    assert_eq!(atlas::onnxfix::input_shapes(&fixed).unwrap(), vec![("x".to_string(), vec![1, 32])]);
    // The rest of the file is untouched: ir_version still first.
    assert_eq!(&fixed[..2], &[0x08, 0x08]);
    // An input that isn't there is refused rather than half-done.
    assert!(atlas::onnxfix::with_fixed_inputs(&model, &[("y".into(), vec![1])]).is_none());
}

/// The real search model, when `ATLAS_ORT_TEST_ROOT` has it: written with
/// fixed sizes, it still loads in ONNX Runtime and declares them.
#[test]
fn the_real_search_model_takes_fixed_sizes() {
    let Ok(root) = std::env::var("ATLAS_ORT_TEST_ROOT") else { return };
    let model = std::path::PathBuf::from(root).join(atlas::meaningnative::MODEL);
    let bytes = std::fs::read(&model).unwrap();
    let names: Vec<String> = atlas::onnxfix::input_shapes(&bytes).unwrap().into_iter().map(|(n, _)| n).collect();
    let shapes: Vec<(String, Vec<i64>)> = names.iter().map(|n| (n.clone(), vec![1, 16])).collect();
    let fixed = atlas::onnxfix::with_fixed_inputs(&bytes, &shapes).unwrap();
    for (_, dims) in atlas::onnxfix::input_shapes(&fixed).unwrap() {
        assert_eq!(dims, vec![1, 16]);
    }
    let out = std::env::temp_dir().join("atlas-fixed-minilm.onnx");
    std::fs::write(&out, &fixed).unwrap();
    let root = std::path::PathBuf::from(std::env::var("ATLAS_ORT_TEST_ROOT").unwrap());
    let back = npu::Session::input_names(&root, &out).unwrap();
    assert_eq!(back, names);
    let _ = std::fs::remove_file(out);
}
