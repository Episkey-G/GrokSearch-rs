use crate::error::{GrokSearchError, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleHost {
    Claude,
    Codex,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleAction {
    On,
    Off,
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToggleResult {
    pub host: String,
    pub blocked: bool,
    pub file: String,
    pub message: String,
}

pub fn toggle_builtin_tools_for_root(
    root: &Path,
    host: ToggleHost,
    action: ToggleAction,
) -> Result<ToggleResult> {
    match host {
        ToggleHost::Claude => toggle_claude(root, action),
        ToggleHost::Codex => toggle_instruction_file(
            root,
            "codex",
            "AGENTS.md",
            "<!-- grok-search-rs-toggle-begin -->\nDo not use built-in web search or web fetch for internet lookup tasks. Use the grok-search-rs MCP tools instead.\n<!-- grok-search-rs-toggle-end -->\n",
            action,
        ),
        ToggleHost::Gemini => toggle_instruction_file(
            root,
            "gemini",
            "GEMINI.md",
            "<!-- grok-search-rs-toggle-begin -->\nDo not use built-in web search or web fetch for internet lookup tasks. Use the grok-search-rs MCP tools instead.\n<!-- grok-search-rs-toggle-end -->\n",
            action,
        ),
    }
}

fn toggle_claude(root: &Path, action: ToggleAction) -> Result<ToggleResult> {
    let path = root.join(".claude/settings.json");
    let mut value = if path.exists() {
        serde_json::from_str(&fs::read_to_string(&path).map_err(io_err)?)
            .map_err(|err| GrokSearchError::Parse(format!("invalid Claude settings: {err}")))?
    } else {
        json!({"permissions": {"deny": []}})
    };
    let deny = value
        .pointer_mut("/permissions/deny")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| {
            GrokSearchError::Parse("Claude settings permissions.deny must be an array".into())
        })?;
    let tools = ["WebFetch", "WebSearch"];
    match action {
        ToggleAction::On => {
            for tool in tools {
                if !deny.iter().any(|item| item.as_str() == Some(tool)) {
                    deny.push(json!(tool));
                }
            }
        }
        ToggleAction::Off => {
            deny.retain(|item| !tools.iter().any(|tool| item.as_str() == Some(*tool)))
        }
        ToggleAction::Status => {}
    }
    let blocked = tools
        .iter()
        .all(|tool| deny.iter().any(|item| item.as_str() == Some(*tool)));
    if action != ToggleAction::Status {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
        fs::write(
            &path,
            serde_json::to_string_pretty(&value).map_err(|err| {
                GrokSearchError::Parse(format!("serialize Claude settings: {err}"))
            })?,
        )
        .map_err(io_err)?;
    }
    Ok(ToggleResult {
        host: "claude".into(),
        blocked,
        file: path.display().to_string(),
        message: if blocked {
            "built-in web tools disabled"
        } else {
            "built-in web tools enabled"
        }
        .into(),
    })
}

fn toggle_instruction_file(
    root: &Path,
    host: &str,
    filename: &str,
    block: &str,
    action: ToggleAction,
) -> Result<ToggleResult> {
    let path = root.join(filename);
    let mut text = if path.exists() {
        fs::read_to_string(&path).map_err(io_err)?
    } else {
        String::new()
    };
    let blocked = text.contains("<!-- grok-search-rs-toggle-begin -->");
    match action {
        ToggleAction::On if !blocked => {
            if !text.ends_with('\n') && !text.is_empty() {
                text.push('\n');
            }
            text.push_str(block);
            fs::write(&path, text).map_err(io_err)?;
        }
        ToggleAction::Off if blocked => {
            text = remove_toggle_block(&text);
            fs::write(&path, text).map_err(io_err)?;
        }
        _ => {}
    }
    let blocked = match action {
        ToggleAction::On => true,
        ToggleAction::Off => false,
        ToggleAction::Status => blocked,
    };
    Ok(ToggleResult {
        host: host.into(),
        blocked,
        file: path.display().to_string(),
        message: if blocked {
            "project instruction disables built-in web tools"
        } else {
            "project instruction not active"
        }
        .into(),
    })
}

fn remove_toggle_block(text: &str) -> String {
    let begin = "<!-- grok-search-rs-toggle-begin -->";
    let end = "<!-- grok-search-rs-toggle-end -->";
    let Some(start) = text.find(begin) else {
        return text.to_string();
    };
    let Some(end_pos) = text[start..].find(end) else {
        return text.to_string();
    };
    let end_index = start + end_pos + end.len();
    let mut result = String::new();
    result.push_str(&text[..start]);
    result.push_str(&text[end_index..]);
    result
}

fn io_err(err: std::io::Error) -> GrokSearchError {
    GrokSearchError::Provider(err.to_string())
}
