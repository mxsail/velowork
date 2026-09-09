//! 安全持久化层。

pub mod security_repository;

pub use security_repository::{
    delete_config, load_config, load_state, now_iso, save_config, save_state, SecurityConfig,
    SecurityState, SqliteSecurityRepository, UnlockStrategy, SECURITY_SCHEMA_VERSION,
};
