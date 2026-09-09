//! 安全子系统统一错误类型。

use thiserror::Error;

use rusqlite::Error as RusqliteError;

/// 安全子系统错误。
#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("unsupported algorithm: {0:?}")]
    UnsupportedAlgorithm(crate::credential::AlgorithmId),

    #[error("key provider error: {0}")]
    KeyProvider(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("invalid data: {0}")]
    InvalidData(String),

    #[error("security context is not unlocked")]
    NotUnlocked,

    #[error("authentication failed")]
    AuthFailed,

    #[error("unsupported operation: {0}")]
    Unsupported(String),
}

/// 安全子系统统一 `Result` 别名。
pub type Result<T> = std::result::Result<T, SecurityError>;

impl SecurityError {
    pub fn crypto(msg: impl Into<String>) -> Self {
        SecurityError::Crypto(msg.into())
    }
    pub fn key_provider(msg: impl Into<String>) -> Self {
        SecurityError::KeyProvider(msg.into())
    }
    pub fn storage(msg: impl Into<String>) -> Self {
        SecurityError::Storage(msg.into())
    }
    pub fn invalid(msg: impl Into<String>) -> Self {
        SecurityError::InvalidData(msg.into())
    }
    pub fn unsupported(msg: impl Into<String>) -> Self {
        SecurityError::Unsupported(msg.into())
    }
}

impl From<RusqliteError> for SecurityError {
    fn from(e: RusqliteError) -> Self {
        SecurityError::storage(format!("sqlite: {e}"))
    }
}
