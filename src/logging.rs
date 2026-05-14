use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::{GrokSearchError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugEvent {
    pub event: String,
    pub payload: Value,
}

impl DebugEvent {
    pub fn new(event: impl Into<String>, payload: Value) -> Self {
        Self {
            event: event.into(),
            payload: redact_json_value(payload),
        }
    }
}

pub fn write_jsonl_event(path: impl AsRef<Path>, event: &DebugEvent) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| GrokSearchError::Provider(format!("create log dir failed: {err}")))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| GrokSearchError::Provider(format!("open log failed: {err}")))?;
    let line = serde_json::to_string(event)
        .map_err(|err| GrokSearchError::Parse(format!("serialize log failed: {err}")))?;
    writeln!(file, "{line}")
        .map_err(|err| GrokSearchError::Provider(format!("write log failed: {err}")))?;
    Ok(())
}

pub fn redact_json_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(redact_object(map)),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_json_value).collect()),
        other => other,
    }
}

fn redact_object(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .map(|(key, value)| {
            if is_secret_key(&key) {
                (key, json!("***"))
            } else {
                (key, redact_json_value(value))
            }
        })
        .collect()
}

fn is_secret_key(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    lowered.contains("authorization")
        || lowered.contains("api_key")
        || lowered.contains("apikey")
        || lowered.contains("token")
        || lowered.contains("secret")
}
