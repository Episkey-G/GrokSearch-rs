use thiserror::Error;

#[derive(Debug, Error)]
pub enum GrokSearchError {
    #[error("missing required config: {0}")]
    MissingConfig(&'static str),
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("upstream timeout: {0}")]
    Timeout(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("parse error: {0}")]
    Parse(String),
}

impl GrokSearchError {
    /// JSON-RPC 2.0 error code mapping. See https://www.jsonrpc.org/specification#error_object
    pub fn code(&self) -> i32 {
        match self {
            // -32700 Parse error: invalid JSON
            GrokSearchError::Parse(_) => -32700,
            // -32602 Invalid params
            GrokSearchError::InvalidParams(_) => -32602,
            // -32004 (server-defined) resource not found
            GrokSearchError::NotFound(_) => -32004,
            // -32002 (server-defined) upstream timeout
            GrokSearchError::Timeout(_) => -32002,
            // -32001 (server-defined) upstream / provider failure
            GrokSearchError::Provider(_) => -32001,
            // -32003 (server-defined) missing config
            GrokSearchError::MissingConfig(_) => -32003,
        }
    }
}

pub type Result<T> = std::result::Result<T, GrokSearchError>;
