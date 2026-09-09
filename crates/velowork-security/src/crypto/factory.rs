//! 集中算法分发：`algorithm_id` → `Box<dyn CryptoProvider>`。
//!
//! 新增算法只改此处（加一个 Provider + 一行 `match`），业务层与 Repository 零改动。

use crate::credential::AlgorithmId;
use crate::crypto::CryptoProvider;
use crate::error::{SecurityError, Result};

use super::aes_gcm::AesGcmProvider;

/// 算法工厂。
pub struct CryptoProviderFactory;

impl CryptoProviderFactory {
    /// 返回对应算法的加密提供方；V1 仅实现 AES-256-GCM。
    pub fn provider_for(algo: AlgorithmId) -> Result<Box<dyn CryptoProvider>> {
        match algo {
            AlgorithmId::Aes256Gcm => Ok(Box::new(AesGcmProvider)),
            AlgorithmId::ChaCha20Poly1305 | AlgorithmId::AesSiv => {
                Err(SecurityError::UnsupportedAlgorithm(algo))
            }
        }
    }

    /// 默认算法（新写入凭据使用）。
    pub fn default_algorithm() -> AlgorithmId {
        AlgorithmId::Aes256Gcm
    }
}
