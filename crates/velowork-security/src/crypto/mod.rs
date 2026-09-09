//! 对称加密抽象：`CryptoProvider` trait + 算法实现分发入口。
//!
//! 所有算法实现都必须通过 [`CryptoProvider`] trait 暴露，业务层只依赖 trait，
//! 具体算法由 [`factory::CryptoProviderFactory`] 集中分发，避免 `match(algorithm)`
//! 散落全项目。

mod aes_gcm;
mod argon2;
mod compress;
mod factory;

pub use aes_gcm::AesGcmProvider;
pub use argon2::{default_kdf_parameters, derive_master_key, generate_salt, KdfParameters};
pub use compress::{compress, decompress};
pub use factory::CryptoProviderFactory;

use zeroize::Zeroizing;

use crate::credential::{AlgorithmId, EncryptedBlob};
use crate::error::Result;
use crate::key_provider::DataKey;

/// 对称加密提供方。算法实现必须支持 AAD（附加认证数据）绑定记录身份防篡改。
pub trait CryptoProvider: Send + Sync {
    /// 该提供方实现的算法标识。
    fn algorithm(&self) -> AlgorithmId;

    /// 用 `key` 加密 `plaintext`，`aad` 绑定记录身份（profile + id + kind）。
    fn encrypt(&self, key: &DataKey, plaintext: &[u8], aad: &[u8]) -> Result<EncryptedBlob>;

    /// 用 `key` 解密 `blob`，`aad` 必须与加密时一致，否则认证失败。
    fn decrypt(&self, key: &DataKey, blob: &EncryptedBlob, aad: &[u8])
        -> Result<Zeroizing<Vec<u8>>>;
}

impl std::fmt::Debug for dyn CryptoProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptoProvider")
            .field("algorithm", &self.algorithm())
            .finish()
    }
}
