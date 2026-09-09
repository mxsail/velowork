//! AES-256-GCM 实现（含 AAD 绑定）。
//!
//! ChaCha20-Poly1305 / AES-SIV 在 V1 暂不实现，由 [`super::factory`] 在请求时返回
//! `UnsupportedAlgorithm`。算法升级只需在此文件新增 Provider 并在工厂登记。

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use zeroize::Zeroizing;

use crate::credential::{AlgorithmId, EncryptedBlob};
use crate::error::{SecurityError, Result};
use crate::key_provider::DataKey;

use super::CryptoProvider;

/// AES-256-GCM 加密提供方。
pub struct AesGcmProvider;

impl CryptoProvider for AesGcmProvider {
    fn algorithm(&self) -> AlgorithmId {
        AlgorithmId::Aes256Gcm
    }

    fn encrypt(&self, key: &DataKey, plaintext: &[u8], aad: &[u8]) -> Result<EncryptedBlob> {
        let cipher = Aes256Gcm::new_from_slice(&key.0[..])
            .map_err(|e| SecurityError::crypto(format!("create aes cipher: {e}")))?;
        let mut nonce = [0u8; 12];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce);
        let payload = Payload { msg: plaintext, aad };
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), payload)
            .map_err(|e| SecurityError::crypto(format!("aes encrypt: {e}")))?;
        Ok(EncryptedBlob::new(
            AlgorithmId::Aes256Gcm,
            nonce.to_vec(),
            ciphertext,
        ))
    }

    fn decrypt(
        &self,
        key: &DataKey,
        blob: &EncryptedBlob,
        aad: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>> {
        if blob.algorithm != AlgorithmId::Aes256Gcm {
            return Err(SecurityError::UnsupportedAlgorithm(blob.algorithm));
        }
        if blob.nonce.len() != 12 {
            return Err(SecurityError::invalid("aes nonce must be 12 bytes"));
        }
        let cipher = Aes256Gcm::new_from_slice(&key.0[..])
            .map_err(|e| SecurityError::crypto(format!("create aes cipher: {e}")))?;
        let payload = Payload {
            msg: &blob.ciphertext,
            aad,
        };
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&blob.nonce), payload)
            .map_err(|_| SecurityError::AuthFailed)?;
        Ok(Zeroizing::new(plaintext))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_provider::DataKey;

    #[test]
    fn aes_gcm_round_trip_with_aad() {
        let key = DataKey::new([42u8; 32]);
        let provider = AesGcmProvider;
        let aad = b"work:ssh:host-1";
        let pt = b"super-secret";
        let blob = provider.encrypt(&key, pt, aad).unwrap();
        let dec = provider.decrypt(&key, &blob, aad).unwrap();
        assert_eq!(&dec[..], pt);
    }

    #[test]
    fn aes_gcm_rejects_tampered_aad() {
        let key = DataKey::new([42u8; 32]);
        let provider = AesGcmProvider;
        let blob = provider.encrypt(&key, b"data", b"work:ssh:host-1").unwrap();
        // AAD 不一致必须认证失败。
        assert!(provider.decrypt(&key, &blob, b"tampered").is_err());
    }
}
