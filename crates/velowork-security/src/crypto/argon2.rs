//! Argon2id 主密码派生（KDF）。
//!
//! 参数以 [`KdfParameters`] 序列化存于 `security_config.kdf_parameters`（JSON），
//! 便于未来调参或换 KDF。

use argon2::{Argon2, Params, Version};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{SecurityError, Result};

/// Argon2id 参数。内存以 KiB 计（64 MiB = 65536 KiB）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParameters {
    pub m: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfParameters {
    /// 序列化为 JSON（避免依赖额外心智负担，手动格式化保证永不失败）。
    pub fn to_json(&self) -> String {
        format!("{{\"m\":{},\"t\":{},\"p\":{}}}", self.m, self.t, self.p)
    }

    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|e| SecurityError::invalid(format!("kdf params: {e}")))
    }
}

/// 默认 Argon2id 参数：m=64 MiB, t=3, p=4。
pub fn default_kdf_parameters() -> KdfParameters {
    KdfParameters {
        m: 64 * 1024,
        t: 3,
        p: 4,
    }
}

/// 生成 16 字节随机 salt（原始字节，存 DB）。
pub fn generate_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

/// 由主密码派生 32 字节 MasterKey（Argon2id）。`salt` 为原始 16 字节。
pub fn derive_master_key(
    secret: &str,
    salt: &[u8],
    params: &KdfParameters,
) -> Result<Zeroizing<[u8; 32]>> {
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        Version::V0x13,
        Params::new(params.m, params.t, params.p, None)
            .map_err(|e| SecurityError::crypto(format!("argon2 params: {e}")))?,
    );
    // `hash_password_into` 接收 base64 编码的 salt 字节。
    let salt_b64 = base64::engine::general_purpose::STANDARD.encode(salt);
    let mut out = [0u8; 32];
    argon2
        .hash_password_into(secret.as_bytes(), salt_b64.as_bytes(), &mut out)
        .map_err(|e| SecurityError::crypto(format!("argon2 derive: {e}")))?;
    Ok(Zeroizing::new(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_derivation_is_deterministic_per_salt() {
        let salt = generate_salt();
        let p = default_kdf_parameters();
        let k1 = derive_master_key("pw", &salt, &p).unwrap();
        let k2 = derive_master_key("pw", &salt, &p).unwrap();
        assert_eq!(&*k1, &*k2);
        let k3 = derive_master_key("other", &salt, &p).unwrap();
        assert_ne!(&*k1, &*k3);
    }

    #[test]
    fn kdf_parameters_json_round_trip() {
        let p = default_kdf_parameters();
        let json = p.to_json();
        let back = KdfParameters::from_json(&json).unwrap();
        assert_eq!(p, back);
    }
}
