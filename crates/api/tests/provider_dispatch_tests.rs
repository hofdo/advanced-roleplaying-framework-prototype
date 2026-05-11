use api::state::{build_provider_from_config, provider_from_record};
use persistence::ProviderRecord;
use serde_json::json;
use uuid::Uuid;

fn make_record(provider_type: &str) -> ProviderRecord {
    ProviderRecord {
        id: Uuid::new_v4(),
        name: "test".into(),
        provider_type: provider_type.into(),
        base_url: "http://localhost:8081/v1".into(),
        model: "test-model".into(),
        api_key_secret_ref: None,
        capabilities: json!({
            "supports_streaming": true,
            "supports_json_mode": false,
            "supports_tool_calls": false,
            "supports_seed": false,
            "max_context_tokens": null,
            "request_timeout_seconds": 120,
            "stream_idle_timeout_seconds": 30,
            "max_retries": 3
        }),
        is_default: false,
    }
}

#[test]
fn provider_from_record_dispatches_openai_compatible() {
    let record = make_record("openai_compatible");
    let provider = provider_from_record(&record).expect("should build");
    let caps = provider.capabilities();
    assert!(caps.supports_streaming);
}

#[test]
fn provider_from_record_dispatches_empty_type_as_openai_compatible() {
    let record = make_record("");
    assert!(provider_from_record(&record).is_ok());
}

#[test]
fn provider_from_record_dispatches_llama_cpp() {
    let record = make_record("llama_cpp");
    let provider = provider_from_record(&record).expect("should build");
    let caps = provider.capabilities();
    assert!(caps.supports_streaming);
}

#[test]
fn provider_from_record_dispatches_openrouter() {
    let record = make_record("openrouter");
    assert!(provider_from_record(&record).is_ok());
}

#[test]
fn provider_from_record_rejects_unknown_type() {
    let record = make_record("unknown_provider_xyz");
    match provider_from_record(&record) {
        Err(e) => assert!(e.to_string().contains("unknown provider_type")),
        Ok(_) => panic!("expected an error for unknown provider type"),
    }
}

#[test]
fn provider_from_record_resolves_env_secret() {
    unsafe { std::env::set_var("TEST_DISPATCH_API_KEY", "test_secret") };
    let mut record = make_record("openai_compatible");
    record.api_key_secret_ref = Some("env:TEST_DISPATCH_API_KEY".into());
    assert!(provider_from_record(&record).is_ok());
    unsafe { std::env::remove_var("TEST_DISPATCH_API_KEY") };
}

#[test]
fn provider_from_record_fails_loudly_for_missing_env_secret() {
    unsafe { std::env::remove_var("TEST_DISPATCH_MISSING_KEY") };
    let mut record = make_record("openai_compatible");
    record.api_key_secret_ref = Some("env:TEST_DISPATCH_MISSING_KEY".into());
    match provider_from_record(&record) {
        Err(e) => assert!(e.to_string().contains("TEST_DISPATCH_MISSING_KEY")),
        Ok(_) => panic!("expected an error for missing env secret"),
    }
}

fn make_config(provider_type: &str) -> shared::ProviderConfig {
    shared::ProviderConfig {
        name: "test".into(),
        provider_type: provider_type.into(),
        base_url: "http://localhost:8081/v1".into(),
        model: "test-model".into(),
        api_key: None,
        supports_streaming: true,
        supports_json_mode: false,
        max_context_tokens: None,
        request_timeout_seconds: 30,
        stream_idle_timeout_seconds: 60,
        max_retries: 3,
        http_referer: None,
        x_title: None,
        provider_routing: None,
        include_usage: true,
    }
}

#[test]
fn build_provider_from_config_dispatches_openai_compatible() {
    let config = make_config("openai_compatible");
    let provider = build_provider_from_config(&config).expect("should build");
    assert!(provider.capabilities().supports_streaming);
}

#[test]
fn build_provider_from_config_dispatches_empty_type_as_openai_compatible() {
    let config = make_config("");
    assert!(build_provider_from_config(&config).is_ok());
}

#[test]
fn build_provider_from_config_dispatches_llama_cpp() {
    let config = make_config("llama_cpp");
    let provider = build_provider_from_config(&config).expect("should build");
    assert!(provider.capabilities().supports_streaming);
}

#[test]
fn build_provider_from_config_dispatches_openrouter() {
    let mut config = make_config("openrouter");
    config.base_url = "https://openrouter.ai/api/v1".into();
    config.model = "openai/gpt-4o".into();
    config.http_referer = Some("https://example.com".into());
    config.x_title = Some("Test App".into());
    assert!(build_provider_from_config(&config).is_ok());
}

#[test]
fn build_provider_from_config_rejects_unknown_type() {
    let config = make_config("unknown_xyz");
    match build_provider_from_config(&config) {
        Err(e) => assert!(e.to_string().contains("unknown provider_type")),
        Ok(_) => panic!("expected an error for unknown provider type"),
    }
}
