use thiserror::Error;

#[derive(Debug, Error)]
pub enum GrokSearchError {
    #[error("missing required config: {0}")]
    MissingConfig(&'static str),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("parse error: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, GrokSearchError>;
