use atlas::private_models::{PrivateModelKind, PrivateModelManifest, PrivateModelRuntime};

#[test]
fn private_manifest_requires_supported_kind_and_runtime() {
    let value = serde_json::json!({
        "schema": 1,
        "kind": "voice",
        "model_id": "eric-f5-v1",
        "runtime": "f5_tts",
        "entrypoint": "tools/private-voice/speak.cmd",
        "artifact_dir": "voice",
        "sha256": "abc",
        "license": "base-license",
        "dataset_version": "voice-001"
    });
    let manifest = PrivateModelManifest::from_value(&value).expect("valid manifest");
    assert_eq!(manifest.kind, PrivateModelKind::Voice);
    assert_eq!(manifest.runtime, PrivateModelRuntime::F5Tts);
}

#[test]
fn private_manifest_rejects_paths_outside_private_root() {
    let value = serde_json::json!({
        "schema": 1,
        "kind": "reasoning",
        "model_id": "atlas-reasoning-v1",
        "runtime": "command",
        "entrypoint": "..\\public.exe",
        "artifact_dir": "reasoning",
        "sha256": "abc",
        "license": "base-license",
        "dataset_version": "reasoning-001"
    });
    let manifest = PrivateModelManifest::from_value(&value).expect("valid metadata");
    assert!(manifest
        .validate_under(std::path::Path::new("C:\\private"),)
        .is_err());
}

#[test]
fn private_manifest_rejects_unknown_runtime() {
    let value = serde_json::json!({
        "schema": 1,
        "kind": "voice",
        "model_id": "bad",
        "runtime": "arbitrary-executable",
        "entrypoint": "speak.cmd",
        "artifact_dir": "voice",
        "sha256": "abc",
        "license": "base-license",
        "dataset_version": "voice-001"
    });
    assert!(PrivateModelManifest::from_value(&value).is_err());
}
