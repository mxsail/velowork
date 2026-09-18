//! 凭据强类型与三层数据模型。
//!
//! - [`Credential`]：业务层对象，永远不含 nonce / ciphertext / algorithm 等加密细节。
//! - [`CredentialRecord`]：Repository 映射对象，持有密文 [`EncryptedBlob`]，是 SQLite 落库单位。
//! - [`EncryptedBlob`]：存储层密文载体，Repository 负责拆列写入 SQLite。
//!
//! 算法升级（AES-GCM → ChaCha20 → AES-SIV → XChaCha20，甚至 ciphertext/header/aad 分离）
//! Repository 零改动。

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{SecurityError, Result};

/// 加密算法标识。以整数枚举存储于 DB `algorithm_id` 列，避免算法名字符串漂移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlgorithmId {
    Aes256Gcm = 1,
    ChaCha20Poly1305 = 2,
    AesSiv = 3,
}

impl AlgorithmId {
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: i64) -> Result<Self> {
        match v {
            1 => Ok(AlgorithmId::Aes256Gcm),
            2 => Ok(AlgorithmId::ChaCha20Poly1305),
            3 => Ok(AlgorithmId::AesSiv),
            other => Err(SecurityError::invalid(format!("unknown algorithm_id {other}"))),
        }
    }
}

/// 凭据种类。Rust 枚举顺序可随意调整，DB 永远稳定（见 [`SecretKind::to_db`]）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    SshPassword,
    ApiKey,
    OAuth,
    WebDav,
    Git,
    S3,
    Plugin,
    Custom(String),
}

impl SecretKind {
    /// 显式 DB 判别值，与 Rust 枚举顺序解耦。新增 Aws/Azure/K8s/License 不改既有值。
    pub fn to_db(&self) -> i64 {
        match self {
            SecretKind::SshPassword => 1,
            SecretKind::WebDav => 2,
            SecretKind::ApiKey => 3,
            SecretKind::OAuth => 4,
            SecretKind::Git => 5,
            SecretKind::S3 => 6,
            SecretKind::Plugin => 100,
            SecretKind::Custom(_) => 999, // 具体名称存 metadata
        }
    }

    pub fn from_db(v: i64) -> Result<Self> {
        Ok(match v {
            1 => SecretKind::SshPassword,
            2 => SecretKind::WebDav,
            3 => SecretKind::ApiKey,
            4 => SecretKind::OAuth,
            5 => SecretKind::Git,
            6 => SecretKind::S3,
            72..=100 if v == 100 => SecretKind::Plugin,
            100 => SecretKind::Plugin,
            999 => SecretKind::Custom("custom".to_string()),
            other => return Err(SecurityError::invalid(format!("unknown secret kind {other}"))),
        })
    }
}

/// 敏感值统一 `Zeroizing` 包裹，退出作用域自动清零，避免驻留内存。
#[derive(Clone, PartialEq)]
pub enum SecretValue {
    Password(Zeroizing<String>),
    ApiKey(Zeroizing<String>),
    OAuthToken {
        access: Zeroizing<String>,
        refresh: Option<Zeroizing<String>>,
    },
    PrivateKey(Zeroizing<Vec<u8>>),
    Certificate(Zeroizing<Vec<u8>>),
    Binary(Zeroizing<Vec<u8>>),
}

impl SecretValue {
    // —— 便捷构造 ——
    pub fn password(s: impl Into<String>) -> Self {
        SecretValue::Password(Zeroizing::new(s.into()))
    }
    pub fn api_key(s: impl Into<String>) -> Self {
        SecretValue::ApiKey(Zeroizing::new(s.into()))
    }
    pub fn oauth_token(access: impl Into<String>, refresh: Option<String>) -> Self {
        SecretValue::OAuthToken {
            access: Zeroizing::new(access.into()),
            refresh: refresh.map(Zeroizing::new),
        }
    }
    pub fn private_key(b: Vec<u8>) -> Self {
        SecretValue::PrivateKey(Zeroizing::new(b))
    }
    pub fn certificate(b: Vec<u8>) -> Self {
        SecretValue::Certificate(Zeroizing::new(b))
    }
    pub fn binary(b: Vec<u8>) -> Self {
        SecretValue::Binary(Zeroizing::new(b))
    }

    /// 取首个字符串值（Password / ApiKey / OAuth access），便于业务层读取明文。
    pub fn as_str(&self) -> Option<&str> {
        match self {
            SecretValue::Password(s) => Some(s),
            SecretValue::ApiKey(s) => Some(s),
            SecretValue::OAuthToken { access, .. } => Some(access),
            _ => None,
        }
    }

    /// 编码为可加密的明文字节（经 DEK 加密落库用）。
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            SecretValue::Password(s) => {
                buf.push(1);
                write_str(&mut buf, s);
            }
            SecretValue::ApiKey(s) => {
                buf.push(2);
                write_str(&mut buf, s);
            }
            SecretValue::OAuthToken { access, refresh } => {
                buf.push(3);
                write_str(&mut buf, access);
                match refresh {
                    Some(r) => {
                        buf.push(1);
                        write_str(&mut buf, r);
                    }
                    None => buf.push(0),
                }
            }
            SecretValue::PrivateKey(b) => {
                buf.push(4);
                write_bytes(&mut buf, b);
            }
            SecretValue::Certificate(b) => {
                buf.push(5);
                write_bytes(&mut buf, b);
            }
            SecretValue::Binary(b) => {
                buf.push(6);
                write_bytes(&mut buf, b);
            }
        }
        buf
    }

    /// 从 [`encode`] 产物还原。
    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.is_empty() {
            return Err(SecurityError::invalid("empty secret value"));
        }
        let tag = buf[0];
        let mut pos = 1;
        let v = match tag {
            1 => SecretValue::Password(Zeroizing::new(read_str(buf, &mut pos)?)),
            2 => SecretValue::ApiKey(Zeroizing::new(read_str(buf, &mut pos)?)),
            3 => {
                let access = Zeroizing::new(read_str(buf, &mut pos)?);
                let refresh = if pos < buf.len() && buf[pos] == 1 {
                    pos += 1;
                    Some(Zeroizing::new(read_str(buf, &mut pos)?))
                } else {
                    None
                };
                SecretValue::OAuthToken { access, refresh }
            }
            4 => SecretValue::PrivateKey(Zeroizing::new(read_bytes(buf, &mut pos)?)),
            5 => SecretValue::Certificate(Zeroizing::new(read_bytes(buf, &mut pos)?)),
            6 => SecretValue::Binary(Zeroizing::new(read_bytes(buf, &mut pos)?)),
            other => {
                return Err(SecurityError::invalid(format!(
                    "unknown secret value tag {other}"
                )))
            }
        };
        Ok(v)
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretValue::Password(_) => f.write_str("Password(***)"),
            SecretValue::ApiKey(_) => f.write_str("ApiKey(***)"),
            SecretValue::OAuthToken { .. } => f.write_str("OAuthToken(***)"),
            SecretValue::PrivateKey(_) => f.write_str("PrivateKey(***)"),
            SecretValue::Certificate(_) => f.write_str("Certificate(***)"),
            SecretValue::Binary(_) => f.write_str("Binary(***)"),
        }
    }
}

fn write_str(buf: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
    buf.extend_from_slice(b);
}

fn write_bytes(buf: &mut Vec<u8>, b: &[u8]) {
    buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
    buf.extend_from_slice(b);
}

fn read_u32(buf: &[u8], pos: &mut usize) -> Result<u32> {
    if *pos + 4 > buf.len() {
        return Err(SecurityError::invalid("truncated secret value"));
    }
    let mut a = [0u8; 4];
    a.copy_from_slice(&buf[*pos..*pos + 4]);
    *pos += 4;
    Ok(u32::from_le_bytes(a))
}

fn read_str(buf: &[u8], pos: &mut usize) -> Result<String> {
    let len = read_u32(buf, pos)? as usize;
    if *pos + len > buf.len() {
        return Err(SecurityError::invalid("truncated secret value"));
    }
    let s = String::from_utf8(buf[*pos..*pos + len].to_vec())
        .map_err(|e| SecurityError::invalid(format!("utf8: {e}")))?;
    *pos += len;
    Ok(s)
}

fn read_bytes(buf: &[u8], pos: &mut usize) -> Result<Vec<u8>> {
    let len = read_u32(buf, pos)? as usize;
    if *pos + len > buf.len() {
        return Err(SecurityError::invalid("truncated secret value"));
    }
    let b = buf[*pos..*pos + len].to_vec();
    *pos += len;
    Ok(b)
}

/// 存储层密文载体（落 SQLite 列），Repository 负责拆开写 version / algorithm / nonce / ciphertext。
///
/// 算法升级（AES-GCM → ChaCha20 → AES-SIV → XChaCha20，甚至 ciphertext/header/aad 分离）
/// Repository 零改动；`version` 让 `decrypt` 走 `match(version)` 适配未来 nonce/AAD/编码变化。
#[derive(Debug, Clone)]
pub struct EncryptedBlob {
    pub version: i32,
    pub algorithm: AlgorithmId,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl EncryptedBlob {
    pub fn new(algorithm: AlgorithmId, nonce: Vec<u8>, ciphertext: Vec<u8>) -> Self {
        Self {
            version: 1,
            algorithm,
            nonce,
            ciphertext,
        }
    }
}

/// 业务层凭据对象：永远不含 nonce / ciphertext / algorithm 等加密细节。
#[derive(Debug, Clone)]
pub struct Credential {
    pub id: String, // "<profile>:<kind>:<id>"
    pub kind: SecretKind,
    pub name: Option<String>,
    pub value: SecretValue,
    pub metadata: serde_json::Value,
}

/// Repository 映射对象：持有密文 [`EncryptedBlob`]，是 SQLite 落库单位。业务层不可见。
#[derive(Debug, Clone)]
pub struct CredentialRecord {
    pub id: String,
    pub kind: SecretKind,
    pub name: Option<String>,
    pub blob: EncryptedBlob,
    pub metadata: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 由 profile_id + credential_id + kind 派生 AAD，绑定记录身份防篡改；不单独存列。
///
/// 即使整条记录被复制，攻击者也无法篡改 `kind` / `profile` / `id` 同时保持认证成功。
pub fn derive_aad(profile_id: &str, credential_id: &str, kind: &SecretKind) -> Vec<u8> {
    let mut aad = Vec::new();
    aad.extend_from_slice(profile_id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(credential_id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(kind.to_db().to_string().as_bytes());
    aad
}

/// 企业版扩展点：未来 Hashicorp Vault / Azure Key Vault / AWS Secrets Manager 直接实现此 trait，
pub type CredentialMetaRow = (String, SecretKind, Option<String>, serde_json::Value);

/// 底层凭据存储抽象（标准 SQLite 表或系统 Keychain 回退）。
pub trait SecretRepository: Send + Sync {
    fn put(
        &self,
        id: &str,
        blob: &EncryptedBlob,
        kind: SecretKind,
        name: Option<&str>,
        metadata: &serde_json::Value,
    ) -> Result<()>;
    fn get(&self, id: &str) -> Result<EncryptedBlob>;
    /// 读取凭据的元数据（kind / name / metadata），不含密文。用于重建业务对象。
    fn meta(&self, id: &str) -> Result<(SecretKind, Option<String>, serde_json::Value)>;
    fn delete(&self, id: &str) -> Result<()>;
    fn list(
        &self,
        kind: Option<SecretKind>,
    ) -> Result<Vec<CredentialMetaRow>>;
}

pub mod manager;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_value_round_trip() {
        let cases = [
            SecretValue::password("hunter2"),
            SecretValue::api_key("sk-123"),
            SecretValue::oauth_token("acc", Some("ref".into())),
            SecretValue::oauth_token("acc", None),
            SecretValue::private_key(vec![1, 2, 3, 4]),
            SecretValue::certificate(vec![9, 8, 7]),
            SecretValue::binary(vec![0, 255, 128]),
        ];
        for v in &cases {
            let buf = v.encode();
            let back = SecretValue::decode(&buf).unwrap();
            assert_eq!(v, &back);
        }
    }

    #[test]
    fn aad_derivation_is_stable() {
        let a = derive_aad("work", "work:ssh:h1", &SecretKind::SshPassword);
        let b = derive_aad("work", "work:ssh:h1", &SecretKind::SshPassword);
        assert_eq!(a, b);
        let c = derive_aad("work", "work:ssh:h1", &SecretKind::ApiKey);
        assert_ne!(a, c);
    }

    #[test]
    fn secret_kind_db_discriminants_stable() {
        assert_eq!(SecretKind::SshPassword.to_db(), 1);
        assert_eq!(SecretKind::WebDav.to_db(), 2);
        assert_eq!(SecretKind::ApiKey.to_db(), 3);
        assert_eq!(SecretKind::Git.to_db(), 5);
        assert_eq!(SecretKind::S3.to_db(), 6);
        assert_eq!(SecretKind::Plugin.to_db(), 100);
        assert_eq!(SecretKind::Custom("x".into()).to_db(), 999);
        assert_eq!(SecretKind::from_db(1).unwrap(), SecretKind::SshPassword);
        assert_eq!(SecretKind::from_db(6).unwrap(), SecretKind::S3);
        assert_eq!(SecretKind::from_db(999).unwrap(), SecretKind::Custom("custom".into()));
    }
}
