//! 快照同步引擎：祖先比较 + push/pull/冲突裁决。
//!
//! 纯逻辑，不依赖 UI。传输由 `SyncProvider`（WebDAV）完成。
//!
//! 模型：每次同步把本地 Profile 导出为一个**不可变快照**（Snapshot），其
//! `snapshot_id` 随机生成，`parent_snapshot` 指向上次推送的本地快照。远端
//! `latest.json` 指向当前最新快照。SyncEngine 比较本地与远端的祖先关系，
//! 得出「仅本地改 / 仅远端改 / 双方分叉 / 已最新」四种决策，再按冲突策略
//! 裁决。冲突策略（`LocalWins` / `RemoteWins` / `NewerWins`）决定分叉时
//! 以哪一侧覆盖另一侧。

use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use velowork_core::profiles::ProfilePaths;
use velowork_core::storage::Database;

use crate::repositories::credential::CredentialApplicationService;
use crate::settings::{SyncConflictStrategy, SyncDataScope};
use crate::sync::backup::SyncBackupManager;
use crate::sync::bundle::BundleManifest;
use crate::sync::exporter::ProfileExporter;
use crate::sync::merge::BundleMerger;
use crate::sync::provider::SyncProvider;
use crate::sync::webdav::SyncResult;

/// 本地同步状态，持久化于 `<profile>/config/sync_state.json`。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LocalSyncState {
    /// 本设备 ID（首次同步时生成并持久化）。
    pub device_id: String,
    /// 上次成功推送的本地快照 ID（作为下次推送的 parent_snapshot）。
    pub last_pushed_snapshot: Option<String>,
    /// 上次成功推送快照的内容哈希（用于判断本地自上次推送后是否变更）。
    pub last_pushed_content_hash: Option<String>,
    /// 上次成功推送快照的创建时间（ISO8601 秒），用于 NewerWins 比较。
    pub last_pushed_created_at: Option<String>,
    /// 上次成功拉取的远端快照 ID。
    pub last_pulled_snapshot: Option<String>,
}

impl LocalSyncState {
    fn load(profile: &ProfilePaths) -> Self {
        let path = profile.config_dir().join("sync_state.json");
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    fn save(&self, profile: &ProfilePaths) -> Result<()> {
        let path = profile.config_dir().join("sync_state.json");
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let json = serde_json::to_string_pretty(self).context("serialize local sync state")?;
        std::fs::write(&path, json).context("save local sync state")?;
        Ok(())
    }
}

/// 同步决策。
enum SyncDecision {
    UpToDate,
    Push,
    Pull,
    Conflict,
}

/// 生成唯一快照 ID：`snap-<unix_ts>-<random>`。
fn generate_snapshot_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut rnd = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut rnd);
    let rnd = rnd.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    format!("snap-{}-{}", ts, rnd)
}

/// 生成设备 ID（首次同步时）。
fn generate_device_id() -> String {
    let mut rnd = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut rnd);
    let rnd = rnd.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    format!("dev-{}", rnd)
}

/// 比较本地与远端快照的祖先关系，得出同步决策。
fn decide(
    state: &LocalSyncState,
    local: &BundleManifest,
    local_changed: bool,
    remote: Option<&BundleManifest>,
) -> SyncDecision {
    let remote = match remote {
        None => {
            // 无远端：本地有变更则推送（首次同步）。
            return if local_changed {
                SyncDecision::Push
            } else {
                SyncDecision::UpToDate
            };
        }
        Some(r) => r,
    };

    // 远端即本地上次推送 → 本地未变更则已是最新。
    if state.last_pushed_snapshot.as_deref() == Some(remote.snapshot_id.as_str()) {
        return if local_changed {
            SyncDecision::Push
        } else {
            SyncDecision::UpToDate
        };
    }

    // 本地是远端的后代（local.parent == remote.snapshot_id）：仅本地改 → push。
    if local.parent_snapshot.as_deref() == Some(remote.snapshot_id.as_str()) {
        return if local_changed {
            SyncDecision::Push
        } else {
            SyncDecision::UpToDate
        };
    }

    // 远端是本地的后代（remote.parent == last_pushed）：仅远端改 → pull。
    if remote.parent_snapshot.as_deref() == state.last_pushed_snapshot.as_deref() {
        return SyncDecision::Pull;
    }

    // 若本地从未推送过（新安装设备）：检查本地是否尚未创建任何会话业务数据
    if state.last_pushed_snapshot.is_none() {
        let has_local_sessions = local.files.iter().any(|f| f.name == "session_tree");
        if !has_local_sessions {
            log::info!("[sync] 本地从未推送过快照且无会话数据，判定为新安装设备接入，直接拉取云端数据");
            return SyncDecision::Pull;
        }
    }

    // 否则双方分叉 → 冲突。
    SyncDecision::Conflict
}

/// 执行一次快照同步：导出本地快照 → 拉取远端 manifest → 比较祖先 →
/// push / pull / 细粒度三向智能合并，并持久化本地同步状态。
pub async fn sync_snapshot<P: SyncProvider + ?Sized>(
    provider: &P,
    profile: &ProfilePaths,
    cred: &CredentialApplicationService,
    db: Option<std::sync::Arc<Database>>,
    passphrase: &str,
    _strategy: SyncConflictStrategy,
    scope: &SyncDataScope,
) -> Result<SyncResult> {
    let mut state = LocalSyncState::load(profile);
    if state.device_id.is_empty() {
        state.device_id = generate_device_id();
    }

    // 1. 导出本地快照（fresh）。
    let snapshot_id = generate_snapshot_id();
    let sealed = ProfileExporter.export_scoped(
        profile,
        cred,
        db.clone(),
        passphrase,
        &snapshot_id,
        state.last_pushed_snapshot.as_deref(),
        &state.device_id,
        scope,
    )?;
    let local_manifest = sealed.manifest.clone();
    let local_changed = state
        .last_pushed_content_hash
        .as_deref()
        != Some(local_manifest.content_sha256.as_str());

    let baseline_path = profile.data_dir().join("sync_baseline.bundle");
    let mut result = SyncResult::default();

    // 2. 拉取远端 manifest。
    let remote = provider.pull_manifest().await?;

    // 3. 决策。
    let decision = decide(&state, &local_manifest, local_changed, remote.as_ref());

    match decision {
        SyncDecision::UpToDate => {}
        SyncDecision::Push => {
            provider.push(&sealed).await?;
            result.uploaded += 1;
            state.last_pushed_snapshot = Some(snapshot_id);
            state.last_pushed_content_hash = Some(local_manifest.content_sha256.clone());
            state.last_pushed_created_at = Some(local_manifest.created_at.clone());
            let _ = std::fs::write(&baseline_path, serde_json::to_vec(&sealed).unwrap_or_default());
        }
        SyncDecision::Pull => {
            let remote_sealed = provider.pull().await?;
            ProfileExporter.import_scoped(&remote_sealed, profile, cred, passphrase, db.clone(), scope)?;
            result.downloaded += 1;
            let _ = std::fs::write(&baseline_path, serde_json::to_vec(&remote_sealed).unwrap_or_default());
            if let Some(rm) = &remote {
                state.last_pushed_snapshot = Some(rm.snapshot_id.clone());
                state.last_pushed_content_hash = Some(rm.content_sha256.clone());
                state.last_pushed_created_at = Some(rm.created_at.clone());
                state.last_pulled_snapshot = Some(rm.snapshot_id.clone());
            }
        }
        SyncDecision::Conflict => {
            // 检测到双端分叉：执行细粒度三向因果智能合并
            result.conflicts += 1;

            // 1) 同步前自动创建本地安全快照备份（滚动保留 10 份）
            let _ = SyncBackupManager::create_safety_backup(
                profile,
                cred,
                db.clone(),
                passphrase,
                &state.device_id,
            );

            // 2) 拉取远端完整快照与本地基线
            let remote_sealed = provider.pull().await?;
            let remote_bundle = remote_sealed.open(passphrase)?;
            let local_bundle = sealed.open(passphrase)?;

            let base_bundle = if baseline_path.exists() {
                std::fs::read(&baseline_path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<crate::sync::bundle::SealedBundle>(&bytes).ok())
                    .and_then(|s| s.open(passphrase).ok())
            } else {
                None
            };

            // 3) 执行实体级智能三向合并（传入真实的共同祖先 Base）
            let mut merged_bundle = BundleMerger::merge(
                base_bundle.as_ref(),
                &local_bundle,
                &remote_bundle,
                &state.device_id,
                scope,
            )?;

            let merged_snapshot_id = generate_snapshot_id();
            merged_bundle.set_snapshot(
                merged_snapshot_id.clone(),
                remote.as_ref().map(|r| r.snapshot_id.clone()),
                state.device_id.clone(),
            );

            // 4) 封口并写回本地
            let merged_sealed = merged_bundle.seal(passphrase)?;
            ProfileExporter.import_scoped(
                &merged_sealed,
                profile,
                cred,
                passphrase,
                db.clone(),
                scope,
            )?;

            // 5) 推送合并后的快照至云端
            provider.push(&merged_sealed).await?;
            result.uploaded += 1;
            result.downloaded += 1;

            let _ = std::fs::write(&baseline_path, serde_json::to_vec(&merged_sealed).unwrap_or_default());

            // 6) 更新本地同步状态
            state.last_pushed_snapshot = Some(merged_snapshot_id.clone());
            state.last_pushed_content_hash = Some(merged_sealed.manifest.content_sha256.clone());
            state.last_pushed_created_at = Some(merged_sealed.manifest.created_at.clone());
            state.last_pulled_snapshot = Some(merged_snapshot_id);
        }
    }

    state.save(profile)?;
    Ok(result)
}

/// 强制覆盖云端：以本地配置为准强制推送到云端（忽略冲突与远端快照）。
pub async fn force_push_to_cloud<P: SyncProvider + ?Sized>(
    provider: &P,
    profile: &ProfilePaths,
    cred: &CredentialApplicationService,
    db: Option<std::sync::Arc<Database>>,
    passphrase: &str,
    scope: &SyncDataScope,
) -> Result<usize> {
    let mut state = LocalSyncState::load(profile);
    if state.device_id.is_empty() {
        state.device_id = generate_device_id();
    }
    let snapshot_id = generate_snapshot_id();
    let sealed = ProfileExporter.export_scoped(
        profile,
        cred,
        db,
        passphrase,
        &snapshot_id,
        None,
        &state.device_id,
        scope,
    )?;
    provider.push(&sealed).await?;

    state.last_pushed_snapshot = Some(snapshot_id);
    state.last_pushed_content_hash = Some(sealed.manifest.content_sha256.clone());
    state.last_pushed_created_at = Some(sealed.manifest.created_at.clone());
    state.save(profile)?;
    Ok(1)
}

pub async fn restore_from_cloud<P: SyncProvider + ?Sized>(
    provider: &P,
    profile: &ProfilePaths,
    cred: &CredentialApplicationService,
    db: Option<std::sync::Arc<Database>>,
    passphrase: &str,
) -> Result<usize> {
    restore_from_cloud_scoped(
        provider,
        profile,
        cred,
        db,
        passphrase,
        &SyncDataScope::default(),
    )
    .await
}

/// 强制从云端恢复指定范围的数据：拉取远端最新快照并覆盖本地（忽略冲突）。
pub async fn restore_from_cloud_scoped<P: SyncProvider + ?Sized>(
    provider: &P,
    profile: &ProfilePaths,
    cred: &CredentialApplicationService,
    db: Option<std::sync::Arc<Database>>,
    passphrase: &str,
    scope: &SyncDataScope,
) -> Result<usize> {
    // 恢复前先创建本地安全备份
    let mut state = LocalSyncState::load(profile);
    if state.device_id.is_empty() {
        state.device_id = generate_device_id();
    }
    let _ = SyncBackupManager::create_safety_backup(
        profile,
        cred,
        db.clone(),
        passphrase,
        &state.device_id,
    );

    let sealed = provider.pull().await?;
    ProfileExporter.import_scoped(&sealed, profile, cred, passphrase, db.clone(), scope)?;

    // 写入基线缓存
    let baseline_path = profile.data_dir().join("sync_baseline.bundle");
    let _ = std::fs::write(&baseline_path, serde_json::to_vec(&sealed).unwrap_or_default());

    // 更新本地同步状态，使后续 push 以远端为基线。
    if let Ok(opened) = sealed.open(passphrase) {
        state.last_pulled_snapshot = Some(opened.snapshot_id.clone());
        state.last_pushed_snapshot = Some(opened.snapshot_id.clone());
        state.last_pushed_content_hash = Some(opened.content_sha256.clone());
        state.last_pushed_created_at = Some(sealed.manifest.created_at.clone());
        state.save(profile)?;
    }
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(snapshot_id: &str, parent: Option<&str>, content: &str) -> BundleManifest {
        BundleManifest {
            bundle_version: 2,
            profile_id: "default".into(),
            database_schema: 1,
            profile_version: 2,
            snapshot_id: snapshot_id.into(),
            parent_snapshot: parent.map(|s| s.to_string()),
            device_id: "dev-1".into(),
            content_sha256: content.into(),
            created_at: "0".into(),
            sha256: String::new(),
            files: Vec::new(),
        }
    }

    #[test]
    fn decide_no_remote_pushes_when_changed() {
        let state = LocalSyncState::default();
        let local = manifest("snap-1", None, "h1");
        assert!(matches!(
            decide(&state, &local, true, None),
            SyncDecision::Push
        ));
        assert!(matches!(
            decide(&state, &local, false, None),
            SyncDecision::UpToDate
        ));
    }

    #[test]
    fn decide_remote_is_local_parent_pulls() {
        let state = LocalSyncState {
            last_pushed_snapshot: Some("snap-A".into()),
            ..Default::default()
        };
        let local = manifest("snap-B", Some("snap-A"), "hB");
        // 远端是本地上次推送 → 本地未变更即最新。
        let remote = manifest("snap-A", None, "hA");
        assert!(matches!(
            decide(&state, &local, false, Some(&remote)),
            SyncDecision::UpToDate
        ));

        // 远端是本地上次推送的后代（远端改了）→ pull。
        let remote2 = manifest("snap-C", Some("snap-A"), "hC");
        assert!(matches!(
            decide(&state, &local, false, Some(&remote2)),
            SyncDecision::Pull
        ));
    }

    #[test]
    fn decide_diverged_is_conflict() {
        let state = LocalSyncState {
            last_pushed_snapshot: Some("snap-A".into()),
            ..Default::default()
        };
        let local = manifest("snap-B", Some("snap-A"), "hB");
        // 远端基于另一个分支（非 snap-A 的后代，也非 snap-A 本身）→ 冲突。
        let remote = manifest("snap-X", Some("snap-Y"), "hX");
        assert!(matches!(
            decide(&state, &local, true, Some(&remote)),
            SyncDecision::Conflict
        ));
    }
}
