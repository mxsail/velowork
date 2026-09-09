//! 本地同步安全备份管理：在合并或拉取前自动生成本地快照备份，并滚动保留最近 10 份。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{Context, Result};
use velowork_core::profiles::ProfilePaths;
use velowork_core::storage::Database;

use crate::repositories::credential::CredentialApplicationService;
use crate::sync::exporter::ProfileExporter;

/// 默认本地最多保留的历史同步备份数量
pub const DEFAULT_MAX_SYNC_BACKUPS: usize = 10;

/// 同步安全备份管理器
pub struct SyncBackupManager;

impl SyncBackupManager {
    /// 获取同步备份存放目录 `<profile>/backups/sync`
    pub fn backup_dir(profile: &ProfilePaths) -> PathBuf {
        profile.root.join("backups").join("sync")
    }

    /// 在同步/合并前创建本地完整安全备份并自动轮转
    pub fn create_safety_backup(
        profile: &ProfilePaths,
        cred: &CredentialApplicationService,
        db: Option<Arc<Database>>,
        passphrase: &str,
        device_id: &str,
    ) -> Result<PathBuf> {
        let dir = Self::backup_dir(profile);
        std::fs::create_dir_all(&dir).context("create sync backup dir")?;

        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let backup_snap_id = format!("safety-backup-{}-{}", ts, rand::random::<u32>());

        let sealed = ProfileExporter.export(
            profile,
            cred,
            db,
            passphrase,
            &backup_snap_id,
            None,
            device_id,
        )?;

        let filename = format!("sync_backup_{}.bundle", ts);
        let backup_path = dir.join(&filename);
        let bytes = serde_json::to_vec_pretty(&sealed).context("serialize safety backup bundle")?;
        std::fs::write(&backup_path, bytes).context("write safety backup bundle")?;

        // 自动轮转历史备份（保留最新 10 份）
        let _ = Self::rotate_backups(&dir, DEFAULT_MAX_SYNC_BACKUPS);

        Ok(backup_path)
    }

    /// 轮转清理旧备份，严格保留最新的 `max_keep` 份文件
    pub fn rotate_backups(dir: &Path, max_keep: usize) -> Result<usize> {
        if !dir.exists() {
            return Ok(0);
        }

        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "bundle") {
                if let Ok(meta) = entry.metadata() {
                    let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    entries.push((modified, path));
                }
            }
        }

        // 按修改时间由新到旧排序
        entries.sort_by(|a, b| b.0.cmp(&a.0));

        let mut deleted = 0;
        if entries.len() > max_keep {
            for (_, path) in entries.into_iter().skip(max_keep) {
                if std::fs::remove_file(&path).is_ok() {
                    deleted += 1;
                }
            }
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_rotate_backups() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("backups");
        std::fs::create_dir_all(&dir).unwrap();

        // 创建 15 个模拟备份文件
        for i in 0..15 {
            let file_path = dir.join(format!("sync_backup_{:02}.bundle", i));
            std::fs::write(&file_path, b"test bundle").unwrap();
            // 微调修改时间以确保先后顺序
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let deleted = SyncBackupManager::rotate_backups(&dir, 10).unwrap();
        assert_eq!(deleted, 5);

        let remaining: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(remaining.len(), 10);
    }
}
