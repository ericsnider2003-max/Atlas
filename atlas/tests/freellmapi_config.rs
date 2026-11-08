use atlas::online::DEFAULT_FREELLMAPI_MODEL;
use atlas::voice::ToolsConfig;

#[test]
fn shipped_freellmapi_config_matches_the_local_desktop_gateway_contract() {
    let yaml = std::fs::read_to_string("config/tools.yaml").expect("shipped tools.yaml");
    let tools: ToolsConfig = serde_yaml::from_str(&yaml).expect("tools.yaml parses");
    let cfg = tools.freellmapi;
    let section = yaml
        .split("\n")
        .skip_while(|line| line.trim_end() != "freellmapi:")
        .skip(1)
        .take_while(|line| line.starts_with(' ') || line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!section.is_empty(), "freellmapi section must be present");

    assert!(!cfg.enabled, "remote-capable routing must remain opt-in");
    assert_eq!(cfg.endpoint, "http://127.0.0.1:31415/v1/chat/completions");
    assert_eq!(cfg.model, DEFAULT_FREELLMAPI_MODEL);
    assert!(cfg.endpoint.starts_with("http://127.0.0.1:"));
    assert!(cfg.verify, "delegated research must be locally verified");
    assert!(!cfg.token_vault.trim().is_empty());
    assert!(!section.contains("freellmapi-"));
    assert!(!section.contains("Bearer "));
    assert!(!section.contains("https://"));
}

#[test]
fn default_model_uses_freellmapi_router_so_catalog_updates_do_not_break_atlas() {
    assert_eq!(DEFAULT_FREELLMAPI_MODEL, "auto");
}
