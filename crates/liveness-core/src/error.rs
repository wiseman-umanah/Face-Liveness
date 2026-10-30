use thiserror::Error;

#[derive(Debug, Error)]
pub enum LivenessError {
    #[error("invalid frame: {0}")]
    InvalidFrame(String),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("model error: {0}")]
    Model(String),
}

pub type Result<T> = std::result::Result<T, LivenessError>;
