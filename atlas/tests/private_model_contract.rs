use atlas::private_models::{
    validate_reasoning_action, PrivateModelKind, PrivateModelManifest, PrivateModelRuntime,
};

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

#[test]
fn reasoning_provider_output_must_be_a_structured_action() {
    let valid = serde_json::json!({
        "action": "create_reminder",
        "arguments": {"text": "call mum"},
        "requires_confirmation": false
    });
    assert!(validate_reasoning_action(&valid).is_ok());
    let invalid = serde_json::json!({"action": "create_reminder", "arguments": []});
    assert!(validate_reasoning_action(&invalid).is_err());
}

#[test]
fn structured_private_action_reaches_the_same_intent_gate_as_a_local_model() {
    let decision = atlas::brain::parse_decision(
        r#"{"action":"research","arguments":{"topic":"Rust memory safety"},"requires_confirmation":false,"say":""}"#,
    )
    .expect("structured private action should reach the decision parser");
    assert!(matches!(
        decision.intent,
        atlas::intent::Intent::Research(topic) if topic == "Rust memory safety"
    ));
}

#[test]
fn enabled_private_provider_is_validated_from_its_real_root() {
    let root = std::env::temp_dir().join(format!("atlas-private-model-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("reasoning")).expect("reasoning dir");
    std::fs::write(root.join("reasoning/run.cmd"), b"@echo off\n").expect("entrypoint");
    let sha256 = atlas::digest::sha256_file_hex(&root.join("reasoning/run.cmd")).unwrap();
    std::fs::write(
        root.join("reasoning/manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "kind": "reasoning",
            "model_id": "atlas-reasoning-v1",
            "runtime": "command",
            "entrypoint": "reasoning/run.cmd",
            "artifact_dir": "reasoning",
            "sha256": sha256,
            "license": "private",
            "dataset_version": "reasoning-001"
        }))
        .expect("manifest json"),
    )
    .expect("manifest");
    let cfg = atlas::private_models::PrivateModelsConfig {
        root: root.clone(),
        reasoning: atlas::private_models::PrivateModelSlot {
            enabled: true,
            manifest: "reasoning/manifest.json".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(cfg.validate_enabled().is_ok());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn enabled_private_provider_requires_entrypoint_and_artifacts() {
    let root = std::env::temp_dir().join(format!(
        "atlas-private-missing-artifacts-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("private root");
    let manifest = serde_json::json!({
        "schema": 1,
        "kind": "reasoning",
        "model_id": "atlas-reasoning-v1",
        "runtime": "command",
        "entrypoint": "reasoning/run.cmd",
        "artifact_dir": "reasoning",
        "sha256": "abc",
        "license": "private",
        "dataset_version": "reasoning-001"
    });
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&manifest).expect("manifest json"),
    )
    .expect("manifest");
    let cfg = atlas::private_models::PrivateModelsConfig {
        root: root.clone(),
        reasoning: atlas::private_models::PrivateModelSlot {
            enabled: true,
            manifest: "manifest.json".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(cfg.validate_enabled().is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_enabled_private_provider_with_a_missing_manifest_is_a_hard_error() {
    let cfg = atlas::private_models::PrivateModelsConfig {
        root: "C:/missing-atlas-private".into(),
        reasoning: atlas::private_models::PrivateModelSlot {
            enabled: true,
            manifest: "reasoning/manifest.json".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(cfg.validate_enabled().is_err());
    let mut tools = atlas::voice::ToolsConfig::default();
    tools.private_models = cfg;
    let error = match atlas::models::connection_checked(&tools) {
        Ok(_) => panic!("typed private error"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("private model configuration"));
}

#[test]
fn private_manifest_path_must_stay_under_private_root() {
    let root = std::env::temp_dir().join(format!(
        "atlas-private-manifest-root-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("private root");
    let outside = root
        .parent()
        .expect("temp parent")
        .join("outside-manifest.json");
    std::fs::write(&outside, b"{}").expect("outside manifest");
    let cfg = atlas::private_models::PrivateModelsConfig {
        root: root.clone(),
        reasoning: atlas::private_models::PrivateModelSlot {
            enabled: true,
            manifest: "../outside-manifest.json".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(cfg.validate_enabled().is_err());
    let _ = std::fs::remove_file(outside);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn enabled_private_reasoning_provider_is_the_runtime_connection() {
    let root = std::env::temp_dir().join(format!("atlas-private-runtime-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("reasoning")).expect("reasoning dir");
    std::fs::write(
        root.join("reasoning/run.cmd"),
        b"@echo off\nfindstr /c:\"user\" >nul\nif errorlevel 1 exit /b 7\necho {\"text\":\"private-runtime-ok\"}\nexit /b 0\n",
    )
    .expect("entrypoint");
    let sha256 = atlas::digest::sha256_file_hex(&root.join("reasoning/run.cmd")).unwrap();
    std::fs::write(
        root.join("reasoning/manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "kind": "reasoning",
            "model_id": "atlas-reasoning-v1",
            "runtime": "command",
            "entrypoint": "reasoning/run.cmd",
            "artifact_dir": "reasoning",
            "sha256": sha256,
            "license": "private",
            "dataset_version": "reasoning-001"
        }))
        .expect("manifest json"),
    )
    .expect("manifest");

    let mut tools = atlas::voice::ToolsConfig::default();
    tools.private_models = atlas::private_models::PrivateModelsConfig {
        root: root.clone(),
        reasoning: atlas::private_models::PrivateModelSlot {
            enabled: true,
            manifest: "reasoning/manifest.json".into(),
            mode: "validated_json".into(),
            max_context_chars: 1000,
            timeout_seconds: 10,
            ..Default::default()
        },
        ..Default::default()
    };
    let llm = atlas::models::connection(&tools).expect("private connection");
    assert_eq!(
        llm.complete("system", "user").expect("private response"),
        "private-runtime-ok"
    );
    std::fs::write(root.join("reasoning/plain.cmd"), b"@echo off\necho plain\n")
        .expect("malformed-output entrypoint");
    let plain_sha256 = atlas::digest::sha256_file_hex(&root.join("reasoning/plain.cmd")).unwrap();
    std::fs::write(
        root.join("reasoning/plain-manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "kind": "reasoning",
            "model_id": "atlas-reasoning-plain-v1",
            "runtime": "command",
            "entrypoint": "reasoning/plain.cmd",
            "artifact_dir": "reasoning",
            "sha256": plain_sha256,
            "license": "private",
            "dataset_version": "reasoning-001"
        }))
        .expect("plain manifest json"),
    )
    .expect("plain manifest");
    let mut malformed_tools = tools.clone();
    malformed_tools.private_models.reasoning.manifest = "reasoning/plain-manifest.json".into();
    let malformed = atlas::models::connection(&malformed_tools).expect("plain connection");
    assert!(
        malformed.complete("system", "user").is_err(),
        "private reasoning must reject unstructured stdout rather than treating it as a reply"
    );
    assert!(llm.complete("system", &"x".repeat(1000)).is_err());
    let _ = std::fs::remove_dir_all(root);
}
