#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

//! VeloWork 分层安全架构底座。
//!
//! 收敛散落在 `secure_storage.rs` / `credential_vault.rs` / `bundle.rs` 的加密与
//! Keyring 逻辑为两套抽象：
//!
//! - [`crypto`]：`CryptoProvider` 对称加密抽象 + [`crypto::CryptoProviderFactory`] 集中算法分发。
//! - [`key_provider`]：`KeyProvider` 抽象（DEK 来源与生命周期）。
//!
//! 业务层永远只接触 [`crate::credential`] 的强类型与（后续）`SecurityService` 门面，
//! 绝不直接接触 nonce / ciphertext / algorithm 等加密细节。

pub mod bundle;
pub mod context;
pub mod crypto;
pub mod credential;
pub mod error;
pub mod key_provider;
pub mod repository;
pub mod service;

pub use error::{Result, SecurityError};
