use grok_search_rs::toggle::{toggle_builtin_tools_for_root, ToggleAction, ToggleHost};

#[test]
fn claude_toggle_blocks_webfetch_and_websearch() {
    let dir = tempfile::tempdir().unwrap();
    let result = toggle_builtin_tools_for_root(dir.path(), ToggleHost::Claude, ToggleAction::On)
        .expect("toggle");

    assert!(result.blocked);
    let text = std::fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
    assert!(text.contains("WebFetch"));
    assert!(text.contains("WebSearch"));
}

#[test]
fn codex_toggle_writes_project_instruction() {
    let dir = tempfile::tempdir().unwrap();
    let result = toggle_builtin_tools_for_root(dir.path(), ToggleHost::Codex, ToggleAction::On)
        .expect("toggle");

    assert!(result.blocked);
    let text = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(text.contains("grok-search-rs"));
    assert!(text.contains("Do not use built-in web search"));
}
