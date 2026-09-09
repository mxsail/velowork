//! Bundle 低层打包组件（序列化 / 压缩 / 清单）。
//!
//! 本组件**完全不知道** WebDAV / Export / 导出密码等上层语义：它只负责把一组
//! 文件（任意业务载荷，如凭据明文、Workspace、Theme…）打包成可校验、可压缩的
//! 二进制归档，并附带 [`BundleManifest`] 描述元数据。
//!
//! 加密（导出密码链）由 Application 层的 `ExportService` 负责：它先调用
//! [`Bundle::to_archive`] 得到明文归档字节，再用导出密码经 `CryptoProvider`
//! 加密得到 `SealedBundle`。本地 DEK 与导出密码是两条互不依赖的密钥链，
//! 但共用同一 `Bundle` 格式。
//!
//! 归档格式（未压缩前的内存布局）：
//! ```text
//! [u32 LE: manifest_json_len][manifest_json][for each file: u64 LE content_len][content...]
//! ```
//! 然后整体 zstd 压缩；`manifest.checksum` 为**未压缩容器**的 sha256 hex。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crypto::{
    compress, decompress, default_kdf_parameters, derive_master_key, generate_salt,
    AesGcmProvider, CryptoProvider, KdfParameters,
};
use crate::credential::{AlgorithmId, EncryptedBlob};
use crate::error::{Result, SecurityError};
use crate::key_provider::DataKey;

/// Bundle 格式版本（布局/封装方式演进时 +1）。
pub const BUNDLE_VERSION: u32 = 1;
/// 数据模式（schema）版本：与 [`BundleManifest`] 字段结构绑定，升级时 +1。
pub const SCHEMA_VERSION: u32 = 1;

/// 导出模式：本质是 [`Capability`] 的一组 Preset。
///
/// - `Backup`：全量（含 Credentials）。
/// - `Share`：不含 SSH / History / Credentials，适合 GitHub 公开分享。
/// - `EnterpriseMigration`：全量（含 Credentials），企业迁移。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportMode {
    Backup,
    Share,
    EnterpriseMigration,
}

impl ExportMode {
    /// 该模式包含的 Capability 预设。
    pub fn capabilities(&self) -> Vec<Capability> {
        match self {
            ExportMode::Backup => vec![
                Capability::Ai,
                Capability::Ssh,
                Capability::Plugins,
                Capability::Snippets,
                Capability::History,
                Capability::Workspace,
                Capability::Theme,
                Capability::Credentials,
            ],
            ExportMode::Share => vec![
                Capability::Workspace,
                Capability::Theme,
                Capability::Plugins,
                Capability::Snippets,
            ],
            ExportMode::EnterpriseMigration => vec![
                Capability::Ai,
                Capability::Ssh,
                Capability::Plugins,
                Capability::Snippets,
                Capability::History,
                Capability::Workspace,
                Capability::Theme,
                Capability::Credentials,
            ],
        }
    }

    /// 解析模式名（与 `serde` 序列化一致）。
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(name: &str) -> Option<Self> {
        match name {
            "backup" => Some(ExportMode::Backup),
            "share" => Some(ExportMode::Share),
            "enterprise_migration" => Some(ExportMode::EnterpriseMigration),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ExportMode::Backup => "backup",
            ExportMode::Share => "share",
            ExportMode::EnterpriseMigration => "enterprise_migration",
        }
    }
}

/// 选择性同步/导出的能力单元。未来企业版可做细粒度裁剪（如"只同步 Theme"）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Ai,
    Ssh,
    Plugins,
    Snippets,
    History,
    Workspace,
    Theme,
    Credentials,
    Custom(String),
}

impl Capability {
    pub fn as_str(&self) -> &str {
        match self {
            Capability::Ai => "ai",
            Capability::Ssh => "ssh",
            Capability::Plugins => "plugins",
            Capability::Snippets => "snippets",
            Capability::History => "history",
            Capability::Workspace => "workspace",
            Capability::Theme => "theme",
            Capability::Credentials => "credentials",
            Capability::Custom(s) => s.as_str(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "ai" => Capability::Ai,
            "ssh" => Capability::Ssh,
            "plugins" => Capability::Plugins,
            "snippets" => Capability::Snippets,
            "history" => Capability::History,
            "workspace" => Capability::Workspace,
            "theme" => Capability::Theme,
            "credentials" => Capability::Credentials,
            other => Capability::Custom(other.to_string()),
        }
    }
}

/// 单个文件条目（元数据，不含内容本身）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleFileEntry {
    /// 包内路径（业务层定义，如 `credentials/ssh.json`）。
    pub path: String,
    /// 内容字节长度。
    pub size: usize,
    /// 内容 sha256 hex（完整性自检）。
    pub checksum: String,
    /// 可选业务类型标签。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Bundle 清单：未来升级容易（schema_version / capabilities 等）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleManifest {
    /// Bundle 格式版本（封装布局）。
    pub bundle_version: u32,
    /// 数据 schema 版本（字段结构）。
    pub schema_version: u32,
    /// 创建时间（ISO-8601）。
    pub created_at: String,
    /// 应用版本（如 `1.2.3`）。
    pub app_version: String,
    /// 来源 profile id。
    pub profile_id: String,
    /// 导出模式名（backup / share / enterprise_migration）。
    pub export_mode: String,
    /// 归档字节是否已被上层（ExportService）加密。
    pub encrypted: bool,
    /// 压缩算法（当前固定 `zstd`）。
    pub compression: String,
    /// 未压缩容器的 sha256 hex。
    pub checksum: String,
    /// 包含的能力单元字符串列表。
    pub capabilities: Vec<String>,
    /// 文件条目清单（与 `Bundle.files` 一一对应）。
    pub files: Vec<BundleFileEntry>,
}

/// 一个 Bundle：清单 + 文件内容。
pub struct Bundle {
    manifest: BundleManifest,
    /// 与 `manifest.files` 顺序一致的内容字节。
    files: Vec<Vec<u8>>,
}

impl Bundle {
    /// 构造空 Bundle。
    pub fn new(profile_id: &str, mode: ExportMode, capabilities: &[Capability], app_version: &str) -> Self {
        let caps: Vec<String> = capabilities.iter().map(|c| c.as_str().to_string()).collect();
        let manifest = BundleManifest {
            bundle_version: BUNDLE_VERSION,
            schema_version: SCHEMA_VERSION,
            created_at: now_iso(),
            app_version: app_version.to_string(),
            profile_id: profile_id.to_string(),
            export_mode: mode.as_str().to_string(),
            encrypted: false,
            compression: "zstd".to_string(),
            checksum: String::new(),
            capabilities: caps,
            files: Vec::new(),
        };
        Self {
            manifest,
            files: Vec::new(),
        }
    }

    /// 追加一个文件。内容会被立即计算 sha256 与长度并记入清单。
    pub fn add_file(&mut self, path: &str, content: &[u8], kind: Option<&str>) -> Result<()> {
        if path.is_empty() {
            return Err(SecurityError::invalid("bundle file path must not be empty"));
        }
        if self.manifest.files.iter().any(|e| e.path == path) {
            return Err(SecurityError::invalid(format!(
                "duplicate bundle file path: {path}"
            )));
        }
        let checksum = sha256_hex(content);
        self.manifest.files.push(BundleFileEntry {
            path: path.to_string(),
            size: content.len(),
            checksum,
            kind: kind.map(|k| k.to_string()),
        });
        self.files.push(content.to_vec());
        Ok(())
    }

    /// 读取已加入文件的内容（按路径）。
    pub fn get_file(&self, path: &str) -> Option<&[u8]> {
        self.manifest
            .files
            .iter()
            .position(|e| e.path == path)
            .and_then(|i| self.files.get(i))
            .map(|v| v.as_slice())
    }

    /// 清单（只读视图）。
    pub fn manifest(&self) -> &BundleManifest {
        &self.manifest
    }

    /// 序列化为可校验、可压缩的归档字节（明文，未加密）。
    pub fn to_archive(&mut self) -> Result<Vec<u8>> {
        // 1) 以空 checksum 的 manifest 构造规范容器，计算校验和（与 from_archive 同一基准）。
        let blank_json = serde_json::to_vec(&self.manifest)
            .map_err(|e| SecurityError::invalid(format!("serialize manifest: {e}")))?;
        let container = build_container(&blank_json, &self.files);
        let checksum = sha256_hex(&container);

        // 2) 回填 checksum 后重新序列化，得到最终容器并压缩。
        self.manifest.checksum = checksum;
        let final_json = serde_json::to_vec(&self.manifest)
            .map_err(|e| SecurityError::invalid(format!("serialize manifest: {e}")))?;
        let final_container = build_container(&final_json, &self.files);

        compress(&final_container).map_err(|e| SecurityError::crypto(format!("bundle compress: {e}")))
    }

    /// 从归档字节还原 Bundle（解密后的明文归档）。
    pub fn from_archive(bytes: &[u8]) -> Result<Bundle> {
        let container = decompress(bytes)
            .map_err(|e| SecurityError::invalid(format!("bundle decompress: {e}")))?;
        let (manifest, files) = parse_container(&container)?;

        // 校验整体 checksum：以"checksum 置空"的规范容器为基准重算。
        let stored = manifest.checksum.clone();
        let mut canonical = manifest.clone();
        canonical.checksum = String::new();
        let canonical_json = serde_json::to_vec(&canonical)
            .map_err(|e| SecurityError::invalid(format!("serialize manifest: {e}")))?;
        let recomputed = sha256_hex(&build_container(&canonical_json, &files));
        if recomputed != stored {
            return Err(SecurityError::invalid("bundle checksum mismatch"));
        }

        // 逐项校验文件 checksum。
        for (entry, content) in manifest.files.iter().zip(files.iter()) {
            if sha256_hex(content) != entry.checksum {
                return Err(SecurityError::invalid(format!(
                    "bundle file checksum mismatch: {}",
                    entry.path
                )));
            }
            if content.len() != entry.size {
                return Err(SecurityError::invalid(format!(
                    "bundle file size mismatch: {}",
                    entry.path
                )));
            }
        }

        Ok(Bundle { manifest, files })
    }
}

/// 组装未压缩容器：`[u32 manifest_len][manifest_json][u64 content_len][content]...`
fn build_container(manifest_json: &[u8], files: &[Vec<u8>]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(manifest_json.len() as u32).to_le_bytes());
    buf.extend_from_slice(manifest_json);
    for f in files {
        buf.extend_from_slice(&(f.len() as u64).to_le_bytes());
        buf.extend_from_slice(f);
    }
    buf
}

/// 解析未压缩容器。
fn parse_container(container: &[u8]) -> Result<(BundleManifest, Vec<Vec<u8>>)> {
    let mut pos = 4usize;
    if container.len() < 4 {
        return Err(SecurityError::invalid("bundle container too short"));
    }
    let manifest_len = u32::from_le_bytes(
        container[0..4]
            .try_into()
            .map_err(|_| SecurityError::invalid("bundle container header corrupt"))?,
    ) as usize;
    if pos + manifest_len > container.len() {
        return Err(SecurityError::invalid("bundle manifest out of bounds"));
    }
    let manifest_json = &container[pos..pos + manifest_len];
    let manifest: BundleManifest = serde_json::from_slice(manifest_json)
        .map_err(|e| SecurityError::invalid(format!("parse manifest: {e}")))?;
    pos += manifest_len;

    let mut files = Vec::with_capacity(manifest.files.len());
    for _ in 0..manifest.files.len() {
        if pos + 8 > container.len() {
            return Err(SecurityError::invalid("bundle file length out of bounds"));
        }
        let content_len = u64::from_le_bytes(
            container[pos..pos + 8]
                .try_into()
                .map_err(|_| SecurityError::invalid("bundle file length header corrupt"))?,
        ) as usize;
        pos += 8;
        if pos + content_len > container.len() {
            return Err(SecurityError::invalid("bundle file content out of bounds"));
        }
        files.push(container[pos..pos + content_len].to_vec());
        pos += content_len;
    }
    Ok((manifest, files))
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// 封口后的 Bundle：用**导出密码**（独立于本地 DEK 的第二条密钥链）加密的归档。
///
/// 与 [`Bundle`] 共用同一归档格式；`SealedBundle` 只多了导出密码派生所需的
/// KDF 参数与 salt，以及 AES-GCM 的 nonce/密文。导入时凭同一导出密码解口。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SealedBundle {
    /// 封口格式版本。
    pub version: u32,
    /// KDF 算法名（当前固定 `Argon2id`）。
    pub kdf_algorithm: String,
    /// KDF 参数 JSON（`{"m":..,"t":..,"p":..}`），便于未来调参或换 KDF。
    pub kdf_parameters: String,
    /// KDF salt（导出密码派生用）。
    pub salt: Vec<u8>,
    /// AES-GCM nonce（12 字节）。
    pub nonce: Vec<u8>,
    /// AES-GCM 密文（含认证 tag）。
    pub ciphertext: Vec<u8>,
}

impl Bundle {
    /// 用导出密码封口：导出密码 → Argon2id 派生密钥 → AES-256-GCM 加密归档。
    /// 与本地 DEK 完全无关（两条互不依赖的密钥链）。
    pub fn seal(&mut self, password: &str) -> Result<SealedBundle> {
        let archive = self.to_archive()?;
        let salt = generate_salt();
        let params = default_kdf_parameters();
        let key = derive_master_key(password, &salt, &params)?;
        let dek = DataKey::new(*key);
        let provider = AesGcmProvider;
        let blob = provider.encrypt(&dek, &archive, b"velowork-export-bundle")?;
        Ok(SealedBundle {
            version: 1,
            kdf_algorithm: "Argon2id".to_string(),
            kdf_parameters: serde_json::to_string(&params)
                .map_err(|e| SecurityError::invalid(format!("serialize kdf params: {e}")))?,
            salt: salt.to_vec(),
            nonce: blob.nonce,
            ciphertext: blob.ciphertext,
        })
    }

    /// 用导出密码解口：还原为明文 [`Bundle`]。密码错误将认证失败。
    pub fn open(sealed: &SealedBundle, password: &str) -> Result<Bundle> {
        let params: KdfParameters = serde_json::from_str(&sealed.kdf_parameters)
            .map_err(|e| SecurityError::invalid(format!("parse kdf params: {e}")))?;
        let key = derive_master_key(password, &sealed.salt, &params)?;
        let dek = DataKey::new(*key);
        let provider = AesGcmProvider;
        let blob = EncryptedBlob::new(AlgorithmId::Aes256Gcm, sealed.nonce.clone(), sealed.ciphertext.clone());
        let archive = provider.decrypt(&dek, &blob, b"velowork-export-bundle")?;
        Bundle::from_archive(&archive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_round_trip_preserves_files() {
        let mut b = Bundle::new("work", ExportMode::Backup, &ExportMode::Backup.capabilities(), "1.0.0");
        b.add_file("credentials/ssh.json", b"{\"user\":\"a\"}", Some("credentials"))
            .unwrap();
        b.add_file("theme/dark.json", b"{\"bg\":\"#000\"}", None).unwrap();

        let archive = b.to_archive().unwrap();
        let restored = Bundle::from_archive(&archive).unwrap();

        assert_eq!(restored.manifest().files.len(), 2);
        assert_eq!(
            restored.get_file("credentials/ssh.json"),
            Some(&b"{\"user\":\"a\"}"[..])
        );
        assert_eq!(
            restored.get_file("theme/dark.json"),
            Some(&b"{\"bg\":\"#000\"}"[..])
        );
        assert_eq!(restored.manifest().schema_version, SCHEMA_VERSION);
        assert_eq!(restored.manifest().export_mode, "backup");
        assert!(restored.manifest().capabilities.contains(&"credentials".to_string()));
    }

    #[test]
    fn bundle_rejects_tampered_archive() {
        let mut b = Bundle::new("work", ExportMode::Share, &ExportMode::Share.capabilities(), "1.0.0");
        b.add_file("workspace/main.json", b"data", None).unwrap();
        let mut archive = b.to_archive().unwrap();
        // 翻转一个字节以破坏校验和。
        let last = archive.len() - 1;
        archive[last] ^= 0xFF;
        assert!(Bundle::from_archive(&archive).is_err());
    }

    #[test]
    fn export_mode_presets_are_distinct() {
        let backup = ExportMode::Backup.capabilities();
        let share = ExportMode::Share.capabilities();
        assert!(backup.contains(&Capability::Credentials));
        assert!(backup.contains(&Capability::Ssh));
        assert!(!share.contains(&Capability::Credentials));
        assert!(!share.contains(&Capability::Ssh));
        assert!(!share.contains(&Capability::History));
    }

    #[test]
    fn duplicate_path_rejected() {
        let mut b = Bundle::new("work", ExportMode::Backup, &[], "1.0.0");
        b.add_file("x", b"1", None).unwrap();
        assert!(b.add_file("x", b"2", None).is_err());
    }

    #[test]
    fn sealed_bundle_round_trip_with_export_password() {
        let mut b = Bundle::new("work", ExportMode::Backup, &ExportMode::Backup.capabilities(), "1.0.0");
        b.add_file("credentials/ssh.json", b"{\"user\":\"a\"}", Some("credentials"))
            .unwrap();
        let sealed = b.seal("export-pw").unwrap();
        // 错误密码必须解口失败（认证失败）。
        assert!(Bundle::open(&sealed, "wrong").is_err());
        // 正确密码还原一致。
        let restored = Bundle::open(&sealed, "export-pw").unwrap();
        assert_eq!(
            restored.get_file("credentials/ssh.json"),
            Some(&b"{\"user\":\"a\"}"[..])
        );
        assert_eq!(restored.manifest().export_mode, "backup");
    }

    /// 镜像 `ExportService` 的核心逻辑：凭据经 `SecretValue::encode` → base64 →
    /// Bundle 文件 → `seal`/`open` → 解码，全程与本地 DEK 无关（导出密码链）。
    #[test]
    fn credential_export_import_via_bundle() {
        use base64::Engine;
        use crate::credential::SecretValue;

        let cases = [
            SecretValue::password("hunter2"),
            SecretValue::api_key("sk-123"),
            SecretValue::oauth_token("acc", Some("ref".into())),
            SecretValue::private_key(vec![1, 2, 3, 4]),
        ];

        let mut b =
            Bundle::new("work", ExportMode::Backup, &ExportMode::Backup.capabilities(), "1.0.0");
        for (i, v) in cases.iter().enumerate() {
            let entry = serde_json::json!({
                "id": format!("work:ssh_password:h{i}"),
                "kind": "ssh_password",
                "value_b64": base64::engine::general_purpose::STANDARD.encode(v.encode()),
            });
            b.add_file(
                &format!("credentials/{i}.json"),
                serde_json::to_vec(&entry).unwrap().as_slice(),
                Some("credentials"),
            )
            .unwrap();
        }

        let sealed = b.seal("export-pw").unwrap();
        assert!(Bundle::open(&sealed, "wrong").is_err());

        let restored = Bundle::open(&sealed, "export-pw").unwrap();
        for (i, v) in cases.iter().enumerate() {
            let content = restored.get_file(&format!("credentials/{i}.json")).unwrap();
            let entry: serde_json::Value = serde_json::from_slice(content).unwrap();
            let raw = base64::engine::general_purpose::STANDARD
                .decode(entry["value_b64"].as_str().unwrap())
                .unwrap();
            let back = SecretValue::decode(&raw).unwrap();
            assert_eq!(v, &back);
        }
    }
}
