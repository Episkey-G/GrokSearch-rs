use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<AnthropicMessage>,
    pub tools: Vec<AnthropicTool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn as_text(&self) -> &str {
        match self {
            Self::Text { text } => text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicTool {
    pub name: String,
    pub input_schema: Value,
}

impl AnthropicTool {
    pub fn web_search() -> Self {
        Self {
            name: "web_search".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicResponse {
    pub content: String,
    pub sources: Vec<crate::model::source::Source>,
}
