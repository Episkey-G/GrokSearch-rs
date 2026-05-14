use grok_search_rs::logging::redact_json_value;

#[test]
fn debug_log_redacts_authorization_and_api_keys() {
    let value = serde_json::json!({
        "Authorization": "Bearer secret-token",
        "XAI_API_KEY": "xai-secret",
        "tools": [{"type": "web_search"}]
    });

    let redacted = redact_json_value(value);
    let text = serde_json::to_string(&redacted).unwrap();

    assert!(text.contains("web_search"));
    assert!(!text.contains("secret-token"));
    assert!(!text.contains("xai-secret"));
}
