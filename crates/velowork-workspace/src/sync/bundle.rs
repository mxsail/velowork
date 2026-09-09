//! 同步 Bundle：导出方把 Profile 各目录/文件组装为 `Bundle`，再 `seal` 成
//! 加密压缩的 `SealedBundle`（AES-GCM + zstd），供 `SyncProvider` 推送。
//!
//! 封口后的 `SealedBundle` 由两部分组成：
//! - `manifest.json`：明文清单（含 `sha256` 完整性校验、各 part 文件列表）。
//! - `bundle.bin`：`<salt(16)><nonce(12)><AES-GCM 密文>`，密文为各 part 经
//!   zstd 压缩后拼接的字节流。
//!
//! 解密时先校验整体 `sha256`，再按 `manifest.files[].compressed_len` 切分并
//! 逐 part 校验 `sha256`，保证传输与存储完整。

use anyhow::{Context, Result, bail};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// 当前 Bundle 格式版本。升版说明：
/// - v1：整包加密快照（无快照 lineage 元数据）。
/// - v2：引入 `snapshot_id` / `parent_snapshot` / `device_id`，使 Bundle 成为
///   不可变快照（Snapshot），供 SyncEngine 按祖先关系做 push/pull/merge 决策。
pub const BUNDLE_VERSION: u32 = 2;

/// Bundle 中单个 part 的元数据。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BundleFileEntry {
    /// part 名（`config` / `data` / `themes` / `sessions` / `credentials`）。
    pub name: String,
    /// 该 part 原始明文的 sha256（用于解密后逐 part 校验）。
    pub sha256: String,
    /// 压缩后字节长度（用于切分拼接流）。
    pub compressed_len: u64,
    /// 原始字节长度。
    pub original_len: u64,
}

/// Bundle 清单（明文，随 `manifest.json` 推送）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BundleManifest {
    pub bundle_version: u32,
    pub profile_id: String,
    /// `velowork.db` 的 schema 版本（`PRAGMA user_version`）。
    pub database_schema: u32,
    /// Profile Layout 版本（v2 分层布局 = 2）。
    pub profile_version: u32,
    /// 本快照的唯一 ID（随机生成，不可变）。
    pub snapshot_id: String,
    /// 父快照 ID：本快照基于哪个快照导出。首次导出为 `None`；后续为上次推送的
    /// 本地快照 ID。用于 SyncEngine 判断「仅本地改 / 仅远端改 / 双方分叉」。
    pub parent_snapshot: Option<String>,
    /// 导出本快照的设备 ID（首次同步时生成并持久化于本地同步状态）。
    pub device_id: String,
    /// 业务内容的确定性哈希（由各 part 的 `name` + 明文 sha256 拼接后求 sha256）。
    /// 与 `payload` 的 sha256（含随机 salt/nonce）不同，本字段对相同内容稳定，
    /// 供 SyncEngine 判断「本地自上次推送后是否变更」。
    #[serde(default)]
    pub content_sha256: String,
    pub created_at: String,
    /// 密文（payload）的 sha256，用于整体完整性校验。
    pub sha256: String,
    pub files: Vec<BundleFileEntry>,
}

/// Bundle 中的一个数据块（未压缩的原始字节）。
pub struct BundlePart {
    pub name: String,
    pub data: Vec<u8>,
}

/// 未封口的 Bundle：各 part 为原始字节。
pub struct Bundle {
    pub profile_id: String,
    pub database_schema: u32,
    pub profile_version: u32,
    pub snapshot_id: String,
    pub parent_snapshot: Option<String>,
    pub device_id: String,
    /// 业务内容确定性哈希（封口时由 `seal` 计算，见 `BundleManifest` 同名字段）。
    pub content_sha256: String,
    pub parts: Vec<BundlePart>,
}

impl Bundle {
    pub fn new(profile_id: &str, database_schema: u32, profile_version: u32) -> Self {
        Self {
            profile_id: profile_id.to_string(),
            database_schema,
            profile_version,
            snapshot_id: String::new(),
            parent_snapshot: None,
            device_id: String::new(),
            content_sha256: String::new(),
            parts: Vec::new(),
        }
    }

    /// 设置快照 lineage 元数据（snapshot_id / parent_snapshot / device_id）。
    /// 必须在 `seal` 之前调用。
    pub fn set_snapshot(
        &mut self,
        snapshot_id: String,
        parent_snapshot: Option<String>,
        device_id: String,
    ) {
        self.snapshot_id = snapshot_id;
        self.parent_snapshot = parent_snapshot;
        self.device_id = device_id;
    }

    /// 获取指定名称的数据块
    pub fn get_part(&self, name: &str) -> Option<&BundlePart> {
        self.parts.iter().find(|p| p.name == name)
    }

    /// 用 passphrase 派生密钥，zstd 压缩各 part 后 AES-GCM 加密，产出封口 Bundle。
    pub fn seal(&self, passphrase: &str) -> Result<SealedBundle> {
        let mut compressed = Vec::new();
        let mut files = Vec::with_capacity(self.parts.len());
        for part in &self.parts {
            let c = zstd::encode_all(part.data.as_slice(), 0).context("zstd compress part")?;
            files.push(BundleFileEntry {
                name: part.name.clone(),
                sha256: sha256_hex(&part.data),
                compressed_len: c.len() as u64,
                original_len: part.data.len() as u64,
            });
            compressed.extend_from_slice(&c);
        }

        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        let key = derive_key(passphrase, &salt);
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow::anyhow!("invalid key length: {e}"))?;

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, Payload { msg: &compressed, aad: b"velowork-bundle" })
            .map_err(|e| anyhow::anyhow!("aes-gcm encrypt bundle: {e}"))?;

        // 业务内容确定性哈希：各 part 的 name + 明文 sha256 拼接后求 sha256。
        // 与 payload 的 sha256（含随机 salt/nonce）无关，对相同内容保持稳定。
        let mut content_hasher = Sha256::new();
        for part in &self.parts {
            content_hasher.update(part.name.as_bytes());
            content_hasher.update(b":");
            content_hasher.update(sha256_hex(&part.data).as_bytes());
            content_hasher.update(b";");
        }
        let content_sha256 = sha256_hex(&content_hasher.finalize());

        let manifest = BundleManifest {
            bundle_version: BUNDLE_VERSION,
            profile_id: self.profile_id.clone(),
            database_schema: self.database_schema,
            profile_version: self.profile_version,
            snapshot_id: self.snapshot_id.clone(),
            parent_snapshot: self.parent_snapshot.clone(),
            device_id: self.device_id.clone(),
            content_sha256,
            created_at: now_iso8601(),
            sha256: sha256_hex(&ciphertext),
            files,
        };

        Ok(SealedBundle {
            manifest,
            payload: ciphertext,
            salt,
            nonce: nonce_bytes,
        })
    }
}

/// 已封口、可推送的 Bundle。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SealedBundle {
    pub manifest: BundleManifest,
    /// AES-GCM 密文（各 part 压缩后拼接）。
    pub payload: Vec<u8>,
    /// 密钥派生盐（明文，非机密）。
    pub salt: [u8; 16],
    /// AES-GCM nonce（明文，非机密）。
    pub nonce: [u8; 12],
}

impl SealedBundle {
    /// 解密并解压，还原为未封口 `Bundle`。校验整体与逐 part 的 sha256。
    pub fn open(&self, passphrase: &str) -> Result<Bundle> {
        let key = derive_key(passphrase, &self.salt);
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow::anyhow!("invalid key length: {e}"))?;
        let nonce = Nonce::from_slice(&self.nonce);

        let plaintext = cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &self.payload,
                    aad: b"velowork-bundle",
                },
            )
            .map_err(|e| anyhow::anyhow!("aes-gcm decrypt bundle (wrong passphrase?): {e}"))?;

        // 整体完整性校验
        if sha256_hex(&self.payload) != self.manifest.sha256 {
            bail!("bundle integrity check failed (sha256 mismatch)");
        }

        // 按 manifest 切分并逐 part 校验
        let mut cursor = 0usize;
        let mut parts = Vec::with_capacity(self.manifest.files.len());
        for entry in &self.manifest.files {
            let len = entry.compressed_len as usize;
            if cursor + len > plaintext.len() {
                bail!("bundle truncated while reading part '{}'", entry.name);
            }
            let c = &plaintext[cursor..cursor + len];
            let data = zstd::decode_all(c).context("zstd decompress part")?;
            if sha256_hex(&data) != entry.sha256 {
                bail!("part '{}' sha256 mismatch after decompress", entry.name);
            }
            parts.push(BundlePart {
                name: entry.name.clone(),
                data,
            });
            cursor += len;
        }

        Ok(Bundle {
            profile_id: self.manifest.profile_id.clone(),
            database_schema: self.manifest.database_schema,
            profile_version: self.manifest.profile_version,
            snapshot_id: self.manifest.snapshot_id.clone(),
            parent_snapshot: self.manifest.parent_snapshot.clone(),
            device_id: self.manifest.device_id.clone(),
            content_sha256: self.manifest.content_sha256.clone(),
            parts,
        })
    }

    /// 序列化为待推送的远程文件列表：`(remote_name, content)`。
    pub fn to_remote_files(&self) -> Result<Vec<(String, Vec<u8>)>> {
        let manifest_json = serde_json::to_vec_pretty(&self.manifest).context("serialize manifest")?;
        let mut bundle_bin = Vec::with_capacity(28 + self.payload.len());
        bundle_bin.extend_from_slice(&self.salt);
        bundle_bin.extend_from_slice(&self.nonce);
        bundle_bin.extend_from_slice(&self.payload);
        Ok(vec![
            ("manifest.json".to_string(), manifest_json),
            ("bundle.bin".to_string(), bundle_bin),
        ])
    }

    /// 从远程文件还原封口 Bundle。
    pub fn from_remote_files(manifest_json: &[u8], bundle_bin: &[u8]) -> Result<Self> {
        let manifest: BundleManifest =
            serde_json::from_slice(manifest_json).context("parse manifest.json")?;
        if bundle_bin.len() < 28 {
            bail!("bundle.bin too short");
        }
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&bundle_bin[..16]);
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&bundle_bin[16..28]);
        let payload = bundle_bin[28..].to_vec();
        Ok(SealedBundle {
            manifest,
            payload,
            salt,
            nonce,
        })
    }
}

/// 由 passphrase + salt 派生 32 字节 AES 密钥（SHA-256 拉伸 100k 轮）。
fn derive_key(passphrase: &str, salt: &[u8; 16]) -> [u8; 32] {
    let mut block = {
        let mut h = Sha256::new();
        h.update(salt);
        h.update(passphrase.as_bytes());
        h.finalize().to_vec()
    };
    for _ in 0..100_000 {
        let mut h = Sha256::new();
        h.update(&block);
        block = h.finalize().to_vec();
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&block);
    key
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn now_iso8601() -> String {
    // 与 repositories 的 now_iso8601 同格式，但本模块不依赖 velowork-workspace 内部。
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 简易 UTC 时间戳（秒级），足够 Bundle 用途。
    format!("{}", secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_then_open_is_lossless() {
        let mut bundle = Bundle::new("default", 1, 2);
        bundle.parts.push(BundlePart {
            name: "config".into(),
            data: br#"{"font_size":16}"#.to_vec(),
        });
        bundle.parts.push(BundlePart {
            name: "data".into(),
            data: vec![1, 2, 3, 4, 5, 6, 7, 8],
        });

        let sealed = bundle.seal("hunter2").unwrap();
        assert_eq!(sealed.manifest.files.len(), 2);
        assert_eq!(sealed.manifest.profile_id, "default");

        let opened = sealed.open("hunter2").unwrap();
        assert_eq!(opened.profile_id, "default");
        assert_eq!(opened.parts.len(), 2);
        assert_eq!(opened.parts[0].name, "config");
        assert_eq!(opened.parts[0].data, br#"{"font_size":16}"#);
        assert_eq!(opened.parts[1].data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let mut bundle = Bundle::new("default", 1, 2);
        bundle.parts.push(BundlePart {
            name: "config".into(),
            data: b"secret".to_vec(),
        });
        let sealed = bundle.seal("right").unwrap();
        assert!(sealed.open("wrong").is_err());
    }

    #[test]
    fn remote_file_roundtrip() {
        let mut bundle = Bundle::new("p", 1, 2);
        bundle.parts.push(BundlePart {
            name: "c".into(),
            data: b"hello".to_vec(),
        });
        let sealed = bundle.seal("pw").unwrap();
        let files = sealed.to_remote_files().unwrap();
        let manifest = files.iter().find(|(n, _)| n == "manifest.json").unwrap().1.clone();
        let bin = files.iter().find(|(n, _)| n == "bundle.bin").unwrap().1.clone();
        let restored = SealedBundle::from_remote_files(&manifest, &bin).unwrap();
        let opened = restored.open("pw").unwrap();
        assert_eq!(opened.parts[0].data, b"hello");
    }
}
