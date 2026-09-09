//! Secret detection, redaction, and sanitization for commands and terminal history.

pub mod secret_redactor;

pub use secret_redactor::{SecretRedactor, REDACTED_PLACEHOLDER};
