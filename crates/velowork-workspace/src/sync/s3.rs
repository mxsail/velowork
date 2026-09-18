//! S3 兼容对象存储同步后端：作为 `SyncProvider` 的一种实现，仅负责「存取快照对象」。
//!
//! 支持 AWS S3、Cloudflare R2、MinIO、阿里云 OSS、腾讯云 COS、Ceph 等标准 S3 兼容协议。
//! 本模块不解析、不修改快照业务内容，仅以不可变对象文件方式将 `SealedBundle` 上传至
//! `<prefix>/snapshots/<snapshot_id>/{manifest.json,bundle.bin}` 并维护 `<prefix>/latest.json`。
//!
//! 远端布局示例：
//! ```text
//! <bucket>/<prefix>/
//!   latest.json              -> {"latest": "snap-1700000000-ab12cd"}
//!   snapshots/
//!     snap-1700000000-ab12cd/manifest.json
//!     snap-1700000000-ab12cd/bundle.bin
//!     snap-1700000001-ef34gh/manifest.json
//!     snap-1700000001-ef34gh/bundle.bin
//! ```

use std::str::FromStr;

use anyhow::{bail, Context, Result};
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;

use crate::settings::S3Config;
use crate::sync::bundle::{BundleManifest, SealedBundle};
use crate::sync::provider::{Capabilities, SyncProvider};

/// S3 sync client
pub struct S3Sync {
    bucket: Box<Bucket>,
    config: S3Config,
}

impl S3Sync {
    /// 创建新的 S3 同步客户端
    pub fn new(config: &S3Config, secret_key: Option<&str>) -> Result<Self> {
        // 未显式传入密钥但已持久化时，从安全密钥库读取
        let resolved = secret_key
            .map(|s| s.to_string())
            .or_else(|| {
                if config.secret_key_stored {
                    crate::secure_storage::load_s3_secret_key()
                } else {
                    None
                }
            });
        let secret_key_ref = resolved.as_deref().unwrap_or_default();

        let endpoint_raw = config.endpoint.trim();
        let region_str = if config.region.trim().is_empty() {
            "us-east-1".to_string()
        } else {
            config.region.trim().to_string()
        };

        let region = if !endpoint_raw.is_empty() {
            let endpoint_normalized = if !endpoint_raw.starts_with("http://") && !endpoint_raw.starts_with("https://") {
                format!("https://{}", endpoint_raw.trim_end_matches('/'))
            } else {
                endpoint_raw.trim_end_matches('/').to_string()
            };
            Region::Custom {
                region: region_str,
                endpoint: endpoint_normalized,
            }
        } else {
            Region::from_str(&region_str).unwrap_or(Region::UsEast1)
        };

        let credentials = Credentials::new(
            Some(&config.access_key_id),
            Some(secret_key_ref),
            None,
            None,
            None,
        )
        .context("初始化 S3 Credentials 失败")?;

        let bucket_name = config.bucket.trim();
        let mut bucket = Bucket::new(bucket_name, region, credentials)
            .context("初始化 S3 Bucket 失败")?;

        // 智能自适应：当 Endpoint 为 IP 地址（如 192.168.x.x）或 localhost 时，
        // 虚拟主机风格（{bucket}.{ip}）在网络规范中必然报错（invalid IPv4 address）。
        // 此时自动强制开启 Path-Style，域名则遵从用户配置，兼顾自建 MinIO 与公有云兼容性。
        if config.path_style || is_ip_or_localhost(endpoint_raw) {
            bucket.set_path_style();
        }

        Ok(Self {
            bucket,
            config: config.clone(),
        })
    }

    /// 获取规范化的存储前缀（去首尾斜杠）
    fn prefix(&self) -> String {
        self.config.prefix.trim().trim_matches('/').to_string()
    }

    /// 根据前缀拼装完整对象 Key
    fn key(&self, sub_path: &str) -> String {
        let p = self.prefix();
        let clean_sub = sub_path.trim_start_matches('/');
        if p.is_empty() {
            clean_sub.to_string()
        } else {
            format!("{p}/{clean_sub}")
        }
    }

    /// 拉取当前最新快照的 ID；若不存在返回 None
    async fn pull_latest_id(&self) -> Result<Option<String>> {
        let latest_key = self.key("latest.json");
        let resp = match self.bucket.get_object(&latest_key).await {
            Ok(resp) => resp,
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("404") || err_str.contains("NoSuchKey") {
                    return Ok(None);
                }
                return Err(e).with_context(|| format!("读取 S3 对象失败: {latest_key}"));
            }
        };

        if resp.status_code() == 404 {
            return Ok(None);
        }
        if resp.status_code() != 200 {
            bail!("读取 S3 对象失败: {latest_key}，HTTP 状态码: {}", resp.status_code());
        }

        let v: serde_json::Value = serde_json::from_slice(resp.bytes())
            .with_context(|| format!("解析 S3 对象内容失败: {latest_key}"))?;
        let id = v.get("latest").and_then(|v| v.as_str()).map(|s| s.to_string());
        Ok(id)
    }

    /// 测试与 S3 存储桶的连通性与读写权限
    pub async fn test_connection(&self) -> Result<()> {
        let prefix = self.prefix();
        let delimiter = Some("/".to_string());
        let results = self.bucket.list(prefix, delimiter).await;
        match results {
            Ok(_) => Ok(()),
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("NoSuchBucket") || err_msg.contains("404") {
                    bail!("存储桶不存在，请先在云服务商或 MinIO 控制台创建存储桶 '{}'", self.config.bucket);
                } else if err_msg.contains("InvalidAccessKeyId") || err_msg.contains("SignatureDoesNotMatch") || err_msg.contains("403") {
                    bail!("S3 认证失败，请检查 Access Key ID 与 Secret Access Key 是否正确");
                } else {
                    bail!("连接 S3 失败：{err_msg}");
                }
            }
        }
    }
}

impl SyncProvider for S3Sync {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            encryption: true, // S3 通常默认走 HTTPS TLS
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
            bail!("快照 ID 为空，无法推送");
        }
        let files = bundle.to_remote_files()?;
        for (name, content) in files {
            let object_key = self.key(&format!("snapshots/{id}/{name}"));
            let mime = if name.ends_with(".json") {
                "application/json"
            } else {
                "application/octet-stream"
            };
            let resp = self
                .bucket
                .put_object_with_content_type(&object_key, &content, mime)
                .await
                .with_context(|| format!("上传 S3 对象失败: {object_key}"))?;
            if resp.status_code() != 200 {
                bail!("上传 S3 对象失败: {object_key}，HTTP 状态码: {}", resp.status_code());
            }
        }

        // 更新 latest.json
        let latest = serde_json::json!({ "latest": id });
        let latest_key = self.key("latest.json");
        let resp = self
            .bucket
            .put_object_with_content_type(&latest_key, latest.to_string().as_bytes(), "application/json")
            .await
            .with_context(|| format!("更新 S3 对象失败: {latest_key}"))?;
        if resp.status_code() != 200 {
            bail!("更新 S3 对象失败: {latest_key}，HTTP 状态码: {}", resp.status_code());
        }
        Ok(())
    }

    async fn pull(&self) -> Result<SealedBundle> {
        let id = self
            .pull_latest_id()
            .await?
            .ok_or_else(|| anyhow::anyhow!("远端尚无快照（latest.json 不存在）"))?;
        let manifest_key = self.key(&format!("snapshots/{id}/manifest.json"));
        let bundle_key = self.key(&format!("snapshots/{id}/bundle.bin"));

        let manifest_resp = self
            .bucket
            .get_object(&manifest_key)
            .await
            .with_context(|| format!("下载 S3 manifest 失败: {manifest_key}"))?;
        if manifest_resp.status_code() != 200 {
            bail!("下载 S3 manifest 失败: {manifest_key}，HTTP 状态码: {}", manifest_resp.status_code());
        }

        let bundle_resp = self
            .bucket
            .get_object(&bundle_key)
            .await
            .with_context(|| format!("下载 S3 bundle 失败: {bundle_key}"))?;
        if bundle_resp.status_code() != 200 {
            bail!("下载 S3 bundle 失败: {bundle_key}，HTTP 状态码: {}", bundle_resp.status_code());
        }

        SealedBundle::from_remote_files(manifest_resp.bytes(), bundle_resp.bytes())
    }

    async fn pull_manifest(&self) -> Result<Option<BundleManifest>> {
        let id = match self.pull_latest_id().await? {
            Some(id) => id,
            None => return Ok(None),
        };
        let manifest_key = self.key(&format!("snapshots/{id}/manifest.json"));
        let manifest_resp = match self.bucket.get_object(&manifest_key).await {
            Ok(resp) => resp,
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("404") || err_str.contains("NoSuchKey") {
                    return Ok(None);
                }
                return Err(e).with_context(|| format!("下载 S3 manifest 失败: {manifest_key}"));
            }
        };

        if manifest_resp.status_code() == 404 {
            return Ok(None);
        }
        if manifest_resp.status_code() != 200 {
            bail!("下载 S3 manifest 失败: {manifest_key}，HTTP 状态码: {}", manifest_resp.status_code());
        }

        let m: BundleManifest = serde_json::from_slice(manifest_resp.bytes())
            .context("解析远端 S3 manifest 失败")?;
        Ok(Some(m))
    }
}

/// 判断 Endpoint 的 host 是否为 IP 地址或 localhost
fn is_ip_or_localhost(endpoint_raw: &str) -> bool {
    let clean = endpoint_raw
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let host = if clean.starts_with('[') && let Some(bracket_end) = clean.find(']') {
        &clean[1..bracket_end]
    } else {
        clean.split([':', '/']).next().unwrap_or("")
    };
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ip_or_localhost() {
        assert!(is_ip_or_localhost("http://192.168.2.22:9000"));
        assert!(is_ip_or_localhost("https://192.168.2.22:9000/"));
        assert!(is_ip_or_localhost("192.168.2.22"));
        assert!(is_ip_or_localhost("http://127.0.0.1:9000"));
        assert!(is_ip_or_localhost("http://localhost:9000"));
        assert!(is_ip_or_localhost("http://[::1]:9000"));

        assert!(!is_ip_or_localhost("https://s3.us-west-2.amazonaws.com"));
        assert!(!is_ip_or_localhost("https://my-bucket.r2.cloudflarestorage.com"));
        assert!(!is_ip_or_localhost("https://oss-cn-hangzhou.aliyuncs.com"));
    }
}
