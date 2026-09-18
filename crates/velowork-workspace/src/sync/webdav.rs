//! WebDAV 同步后端：作为 `SyncProvider` 的一种实现，仅负责「存取快照」。
//!
//! 本模块**不解析、不合并、不修改**快照内容——它把封口后的 `SealedBundle`
//! 以 `snapshots/<snapshot_id>/{manifest.json,bundle.bin}` 的形式上传，并维护
//! 一个 `latest.json` 指向当前最新快照。真正的冲突检测与合并由客户端
//! `SyncEngine`（`snapshot.rs`）按快照祖先关系完成。
//!
//! 远端布局示例：
//! ```text
//! <remote_path>/
//!   latest.json              -> {"latest": "snap-1700000000-ab12cd"}
//!   snapshots/
//!     snap-1700000000-ab12cd/manifest.json
//!     snap-1700000000-ab12cd/bundle.bin
//!     snap-1700000001-ef34gh/manifest.json
//!     snap-1700000001-ef34gh/bundle.bin
//! ```

use anyhow::{Context, Result};
use reqwest_dav::{Auth, ClientBuilder, Depth};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::settings::WebDavConfig;
use crate::sync::bundle::{BundleManifest, SealedBundle};
use crate::sync::provider::{Capabilities, SyncProvider};

/// 同步结果（供 UI / 自动引擎展示）。
#[derive(Debug, Default)]
pub struct SyncResult {
    pub uploaded: usize,
    pub downloaded: usize,
    /// 本次同步检测到的冲突数量（已按策略自动裁决）。
    pub conflicts: usize,
    pub errors: Vec<String>,
}

/// WebDAV sync client
pub struct WebDavSync {
    client: reqwest_dav::Client,
    config: WebDavConfig,
}

impl WebDavSync {
    /// Create a new WebDAV sync client
    pub fn new(config: &WebDavConfig, password: Option<&str>) -> Result<Self> {
        // 未显式传入密码但已持久化时，从系统密钥库读取（支持免交互的自动同步）
        let resolved = password
            .map(|s| s.to_string())
            .or_else(|| {
                if config.password_stored {
                    crate::secure_storage::load_webdav_password()
                } else {
                    None
                }
            });
        let password_ref = resolved.as_deref().unwrap_or_default();

        let client = ClientBuilder::new()
            .set_host(config.server_url.clone())
            .set_auth(Auth::Basic(
                config.username.clone(),
                password_ref.to_string(),
            ))
            .build()
            .context("Failed to create WebDAV client")?;

        Ok(Self {
            client,
            config: config.clone(),
        })
    }

    /// Test the WebDAV connection
    pub async fn test_connection(&self) -> Result<()> {
        let remote_path = self.config.remote_path.clone();
        let server_url = self.config.server_url.clone();

        if remote_path.trim().is_empty() {
            // 未配置远程路径，探测根路径以验证服务器与认证
            return match self.client.list("/", Depth::Number(0)).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    Self::log_webdav_error("测试连接(根路径)", &server_url, "/", &e);
                    Err(anyhow::anyhow!("连接服务器失败：{}", Self::webdav_error_message(&e)))
                }
            };
        }

        // 集合路径补结尾斜杠：部分服务器（如 fnOS）对无斜杠的 PROPFIND 返回 404，
        // 而带斜杠才命中真实集合；MKCOL 对同一路径则能正确识别集合已存在(405)。
        let list_path = Self::with_trailing_slash(&remote_path);

        match self.client.list(&list_path, Depth::Number(0)).await {
            Ok(_) => Ok(()),
            Err(e) if Self::is_not_found(&e) => {
                log::warn!(
                    "[webdav] 远程路径 {} PROPFIND 返回 404，改用 MKCOL 探针验证 | server_url={}",
                    list_path,
                    server_url
                );
                // 路径可能不存在，自动创建该目录（含逐级父目录）
                self.ensure_remote_dir(&remote_path).await?;
                // 用 MKCOL 探针复核：201=新建成功 / 405·409=集合已存在，均视为可达
                self.probe_path(&server_url, &remote_path).await
            }
            Err(e) => {
                Self::log_webdav_error("测试连接", &server_url, &list_path, &e);
                Err(anyhow::anyhow!("连接服务器失败：{}", Self::webdav_error_message(&e)))
            }
        }
    }

    /// 逐级创建远程目录（含父目录）。已存在的目录忽略错误，最终由 MKCOL 探针验证。
    ///
    /// 注意：
    /// - 必须保留前导斜杠，向服务器发送**绝对路径**。否则部分服务器（如 fnOS）
    ///   对相对路径返回 405/409，导致中间目录实际未被创建，后续上传返回 403。
    /// - 集合路径必须带**结尾斜杠**。部分服务器（如 fnOS）对无斜杠的 MKCOL 返回
    ///   405（被当成文件而非集合），同样会导致目录建不起来。
    async fn ensure_remote_dir(&self, path: &str) -> Result<()> {
        let cleaned = path.trim_end_matches('/');
        if cleaned.is_empty() {
            return Ok(());
        }
        // 保留原始路径的前导斜杠，拼接出的每段都是绝对路径。
        let leading = if cleaned.starts_with('/') { "/" } else { "" };
        let mut built = String::new();
        for seg in cleaned.split('/').filter(|s| !s.is_empty()) {
            if built.is_empty() {
                built = format!("{}{}", leading, seg);
            } else {
                built.push('/');
                built.push_str(seg);
            }
            // 集合地址补结尾斜杠：部分服务器对无斜杠 MKCOL 返回 405。
            let mkcol_path = format!("{}/", built);
            // MKCOL 已存在的目录通常返回 405/409，属于预期内的正常状态（集合已存在），记录 debug 日志即可；其他非预期错误才记录 warn
            match self.client.mkcol(&mkcol_path).await {
                Ok(_) => log::info!("[webdav] 已创建远程目录 {}", built),
                Err(e) if Self::is_exists_or_conflict(&e) => {
                    log::debug!("[webdav] 远程目录 {} 已存在 (HTTP 405/409)", built);
                }
                Err(e) => log::warn!(
                    "[webdav] 创建远程目录 {} 时返回(已忽略): {} | 原始: {:?}",
                    built,
                    Self::webdav_error_message(&e),
                    e
                ),
            }
        }
        Ok(())
    }

    /// 用 MKCOL 探针验证远程路径可达：201 新建成功 / 405·409 集合已存在，均视为成功。
    async fn probe_path(&self, server_url: &str, path: &str) -> Result<()> {
        let list_path = Self::with_trailing_slash(path);
        match self.client.mkcol(&list_path).await {
            Ok(_) => {
                log::info!("[webdav] 远程路径 {} 已创建并验证成功", list_path);
                Ok(())
            }
            Err(e) if Self::is_exists_or_conflict(&e) => {
                log::info!("[webdav] 远程路径 {} 已存在并验证成功", list_path);
                Ok(())
            }
            Err(e) => {
                Self::log_webdav_error("远程路径验证(MKCOL 探针)", server_url, &list_path, &e);
                Err(anyhow::anyhow!(
                    "远程路径无法访问：{}",
                    Self::webdav_error_message(&e)
                ))
            }
        }
    }

    /// 给路径补上结尾斜杠（集合地址约定），不改变已有斜杠。
    fn with_trailing_slash(path: &str) -> String {
        let t = path.trim();
        if t.ends_with('/') {
            t.to_string()
        } else {
            format!("{}/", t)
        }
    }

    /// 判断 MKCOL 返回是否表示“集合已存在/冲突”（405 或 409）。
    fn is_exists_or_conflict(e: &reqwest_dav::Error) -> bool {
        let code = match e {
            reqwest_dav::Error::Decode(reqwest_dav::DecodeError::Server(s)) => s.response_code,
            reqwest_dav::Error::Decode(reqwest_dav::DecodeError::StatusMismatched(m)) => {
                m.response_code
            }
            _ => return false,
        };
        code == 405 || code == 409
    }

    /// 把完整的 WebDAV 连接错误写入系统日志文件（含原始 Debug 信息，便于排查）。
    fn log_webdav_error(stage: &str, server_url: &str, path: &str, e: &reqwest_dav::Error) {
        log::error!(
            "[webdav] {} 失败 | server_url={} path={} | 可读错误: {} | 原始错误: {:?}",
            stage,
            server_url,
            path,
            Self::webdav_error_message(e),
            e
        );
    }

    /// 判断是否为“资源不存在”(HTTP 404)
    fn is_not_found(e: &reqwest_dav::Error) -> bool {
        match e {
            reqwest_dav::Error::Decode(reqwest_dav::DecodeError::StatusMismatched(m)) => {
                m.response_code == 404
            }
            reqwest_dav::Error::Decode(reqwest_dav::DecodeError::Server(s)) => {
                s.response_code == 404
            }
            _ => false,
        }
    }

    /// 把 reqwest_dav 的错误转成可读的原始报错文本（不做翻译，保留底层原始信息）。
    fn webdav_error_message(e: &reqwest_dav::Error) -> String {
        match e {
            reqwest_dav::Error::Reqwest(re) => re.to_string(),
            reqwest_dav::Error::Decode(d) => match d {
                reqwest_dav::DecodeError::Server(s) => {
                    format!("HTTP {}: {}", s.response_code, s.message)
                }
                reqwest_dav::DecodeError::StatusMismatched(m) => {
                    format!("HTTP {} (期望 {})", m.response_code, m.expected_code)
                }
                other => format!("{:?}", other),
            },
            reqwest_dav::Error::ReqwestDecode(de) => format!("{:?}", de),
            reqwest_dav::Error::MissingAuthContext => format!("{:?}", e),
        }
    }

    /// Upload a single file
    async fn upload_file(&self, remote_path: &str, content: &[u8]) -> Result<()> {
        // 逐级创建父目录（含中间目录，如 snapshots/），再上传文件。
        // 仅创建文件所在的“直接父目录”会导致中间目录缺失，从而上传返回 403/409。
        if let Some(parent) = Path::new(remote_path).parent() {
            let parent_path = parent.to_string_lossy().to_string();
            if !parent_path.is_empty() {
                let _ = self.ensure_remote_dir(&parent_path).await;
            }
        }

        self.client
            .put(remote_path, content.to_vec())
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "上传文件失败: {} | 路径: {}",
                    Self::webdav_error_message(&e),
                    remote_path
                )
            })?;

        Ok(())
    }

    /// Download a single file
    async fn download_file(&self, remote_path: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(remote_path)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "下载远程文件失败: {} | 路径: {}",
                    Self::webdav_error_message(&e),
                    remote_path
                )
            })?;

        let content = response
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("读取文件内容失败: {}", e))?;

        Ok(content.to_vec())
    }

    /// 下载单个文件，若服务器明确返回 404 则返回 Ok(None)；成功返回 Ok(Some(bytes))；其他错误（网络/鉴权/5xx）返回 Err。
    async fn download_file_optional(&self, remote_path: &str) -> Result<Option<Vec<u8>>> {
        let response = match self.client.get(remote_path).await {
            Ok(r) => r,
            Err(e) if Self::is_not_found(&e) => return Ok(None),
            Err(e) => {
                Self::log_webdav_error("下载文件", &self.config.server_url, remote_path, &e);
                return Err(anyhow::anyhow!(
                    "下载远程文件失败: {} | 路径: {}",
                    Self::webdav_error_message(&e),
                    remote_path
                ));
            }
        };

        let content = response
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("读取文件内容失败: {}", e))?;

        Ok(Some(content.to_vec()))
    }

    /// 读取 `latest.json`，返回当前最新快照 ID（无则返回 `None`）。
    async fn pull_latest_id(&self) -> Result<Option<String>> {
        let base = self.config.remote_path.trim_end_matches('/');
        let path = format!("{}/latest.json", base);
        match self.download_file_optional(&path).await? {
            Some(bytes) => {
                let v: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|e| anyhow::anyhow!("解析 latest.json 失败: {e}"))?;
                Ok(v.get("latest").and_then(|l| l.as_str()).map(|s| s.to_string()))
            }
            None => {
                log::info!("[webdav] 远端尚无 {}（HTTP 404），判定为首次同步空存储", path);
                Ok(None)
            }
        }
    }

}

/// WebDAV 作为 `SyncProvider` 的一种实现：推送/拉取封口快照。
impl SyncProvider for WebDavSync {
    fn capabilities(&self) -> Capabilities {
        // WebDAV 仅是传输层；以下能力实际由客户端 `SyncEngine` 在其之上实现：
        // - 冲突检测 / 合并（按快照祖先关系）
        // - 历史版本（snapshots/ 目录保留多版本）
        // - 版本回滚（可读取任意历史快照）
        // 传输层本身不加密（Bundle 已在客户端 AES-GCM 加密）、不支持增量。
        Capabilities {
            encryption: false,
            delta: false,
            conflict: true,
            history: true,
            versioning: true,
        }
    }

    async fn test_connection(&self) -> Result<()> {
        self.test_connection().await
    }

    async fn push(&self, bundle: &SealedBundle) -> Result<()> {
        let id = &bundle.manifest.snapshot_id;
        if id.is_empty() {
            anyhow::bail!("快照 ID 为空，无法推送");
        }
        let base = self.config.remote_path.trim_end_matches('/');
        let dir = format!("{}/snapshots/{}", base, id);

        let files = bundle.to_remote_files()?; // [manifest.json, bundle.bin]
        for (name, content) in files {
            self.upload_file(&format!("{}/{}", dir, name), &content)
                .await?;
        }

        // 更新 latest.json 指向本次推送的快照。
        let latest = serde_json::json!({ "latest": id });
        self.upload_file(
            &format!("{}/latest.json", base),
            latest.to_string().as_bytes(),
        )
        .await?;

        Ok(())
    }

    async fn pull(&self) -> Result<SealedBundle> {
        let base = self.config.remote_path.trim_end_matches('/');
        let id = self
            .pull_latest_id()
            .await?
            .ok_or_else(|| anyhow::anyhow!("远端尚无快照（latest.json 不存在）"))?;
        let dir = format!("{}/snapshots/{}", base, id);
        let manifest = self
            .download_file(&format!("{}/manifest.json", dir))
            .await
            .context("下载远端 manifest.json 失败")?;
        let bin = self
            .download_file(&format!("{}/bundle.bin", dir))
            .await
            .context("下载远端 bundle.bin 失败")?;
        SealedBundle::from_remote_files(&manifest, &bin)
    }

    async fn pull_manifest(&self) -> Result<Option<BundleManifest>> {
        let base = self.config.remote_path.trim_end_matches('/');
        let id = match self.pull_latest_id().await? {
            Some(id) => id,
            None => return Ok(None),
        };
        let dir = format!("{}/snapshots/{}", base, id);
        let manifest = self
            .download_file(&format!("{}/manifest.json", dir))
            .await
            .context("下载远端 manifest.json 失败")?;
        let m: BundleManifest =
            serde_json::from_slice(&manifest).context("解析远端 manifest 失败")?;
        Ok(Some(m))
    }
}

/// `latest.json` 的内容结构。
#[derive(Serialize, Deserialize, Default)]
#[allow(dead_code)]
struct LatestMarker {
    latest: String,
}
