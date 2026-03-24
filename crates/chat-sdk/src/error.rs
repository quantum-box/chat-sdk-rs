use thiserror::Error;

/// Result type alias for chat-sdk operations.
pub type ChatResult<T> = Result<T, ChatError>;

/// Errors that can occur during chat operations.
#[derive(Debug, Error)]
pub enum ChatError {
    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("platform API error: {0}")]
    Api(String),

    #[error("channel not found: {0}")]
    ChannelNotFound(String),

    #[error("rate limited, retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("{0}")]
    Other(String),
}
