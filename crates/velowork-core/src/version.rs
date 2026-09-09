//! 版本编排（`VersionManager`）：统一管理 Profile Layout / Database / Config /
//! Bundle 四类版本，并提供可回滚的 `MigrationPlan`。
//!
//! 版本状态持久化于 `manifest.json`（`ProfileManifest`）。具体迁移逻辑（v2 布局
//! 迁移、DB schema 演进、Config 拆分、Bundle 格式升级）由各阶段注册为
//! [`MigrationPlan`] 后由 [`VersionManager::ensure_layout`] 等驱动，任一
//! 步骤失败可自动逆序回滚已完成步骤。

use crate::profiles::{ProfileManifest, ProfilePaths};
use anyhow::{Context, Result};

pub type MigrationFn = Box<dyn Fn(&ProfilePaths) -> Result<()> + Send + Sync>;

/// 一个可回滚的迁移步骤。
pub struct MigrationStep {
    pub name: String,
    pub apply: MigrationFn,
    pub rollback: MigrationFn,
}

impl MigrationStep {
    /// 创建步骤。`apply` 为前进逻辑，`rollback` 为回滚逻辑。
    pub fn new(
        name: impl Into<String>,
        apply: impl Fn(&ProfilePaths) -> Result<()> + Send + Sync + 'static,
        rollback: impl Fn(&ProfilePaths) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            apply: Box::new(apply),
            rollback: Box::new(rollback),
        }
    }
}

/// 一组有序迁移步骤；任一失败则按逆序回滚已完成步骤。
pub struct MigrationPlan {
    pub name: String,
    pub steps: Vec<MigrationStep>,
}

impl MigrationPlan {
    /// 创建空计划。
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            steps: Vec::new(),
        }
    }

    /// 追加一个步骤。
    pub fn step(mut self, step: MigrationStep) -> Self {
        self.steps.push(step);
        self
    }

    /// 顺序执行步骤；失败时对已完成步骤逆序回滚，并返回首个错误。
    pub fn run(&self, profile: &ProfilePaths) -> Result<()> {
        let mut done: Vec<&MigrationStep> = Vec::new();
        for step in &self.steps {
            if let Err(e) = (step.apply)(profile) {
                eprintln!(
                    "Migration plan '{}': step '{}' failed: {e}; rolling back…",
                    self.name, step.name
                );
                for completed in done.iter().rev() {
                    if let Err(re) = (completed.rollback)(profile) {
                        eprintln!(
                            "Migration plan '{}': rollback of '{}' failed: {re}",
                            self.name, completed.name
                        );
                    }
                }
                return Err(e);
            }
            done.push(step);
        }
        Ok(())
    }
}

/// 统一编排 Profile Layout / Database / Config / Bundle 各类版本。
///
/// 版本状态持久化于 `manifest.json`。本结构只提供编排与读写能力；具体迁移
/// 由各阶段注册为 [`MigrationPlan`] 后驱动。
pub struct VersionManager;

impl VersionManager {
    /// 读取当前版本状态（不存在则返回默认，各版本为 0）。
    pub fn read(&self, profile: &ProfilePaths) -> ProfileManifest {
        ProfileManifest::read(&profile.root).unwrap_or_default()
    }

    /// 写回版本状态。
    pub fn write(&self, profile: &ProfilePaths, manifest: &ProfileManifest) -> Result<()> {
        manifest.write(&profile.root)
    }

    /// 确保 Profile Layout 达到 `target` 版本：未达标时运行 `plan`，成功后写回
    /// `layout_version`。已达标则跳过（幂等）。
    pub fn ensure_layout(
        &self,
        profile: &ProfilePaths,
        target: u32,
        plan: &MigrationPlan,
    ) -> Result<()> {
        let current = self.read(profile);
        if current.layout_version >= target {
            return Ok(());
        }
        plan.run(profile)?;
        let mut manifest = current;
        manifest.layout_version = target;
        manifest.profile_id = profile.id.clone();
        self.write(profile, &manifest)
            .with_context(|| format!("writing manifest after layout migration to v{target}"))
    }

    /// 记录 Database 版本（由 sqlite-core 在迁移后调用）。
    pub fn set_db_version(&self, profile: &ProfilePaths, version: u32) -> Result<()> {
        let mut manifest = self.read(profile);
        manifest.db_version = version;
        manifest.profile_id = profile.id.clone();
        self.write(profile, &manifest)
    }

    /// 记录 Config 版本（由 ConfigStore 在迁移后调用）。
    pub fn set_config_version(&self, profile: &ProfilePaths, version: u32) -> Result<()> {
        let mut manifest = self.read(profile);
        manifest.config_version = version;
        manifest.profile_id = profile.id.clone();
        self.write(profile, &manifest)
    }

    /// 记录 Bundle 版本（由 SyncEngine 在导出时调用）。
    pub fn set_bundle_version(&self, profile: &ProfilePaths, version: u32) -> Result<()> {
        let mut manifest = self.read(profile);
        manifest.bundle_version = version;
        manifest.profile_id = profile.id.clone();
        self.write(profile, &manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn temp_paths(dir: &TempDir) -> ProfilePaths {
        let root = dir.path().join("profiles/test");
        fs::create_dir_all(&root).unwrap();
        ProfilePaths {
            id: "test".into(),
            root,
            config_root: dir.path().to_path_buf(),
        }
    }

    #[test]
    fn plan_runs_all_steps_and_records_version() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let plan = MigrationPlan::new("test")
            .step(MigrationStep::new(
                "a",
                |p| {
                    fs::write(p.root.join("a"), "1")?;
                    Ok(())
                },
                |p| {
                    let _ = fs::remove_file(p.root.join("a"));
                    Ok(())
                },
            ))
            .step(MigrationStep::new(
                "b",
                |p| {
                    fs::write(p.root.join("b"), "1")?;
                    Ok(())
                },
                |p| {
                    let _ = fs::remove_file(p.root.join("b"));
                    Ok(())
                },
            ));
        VersionManager
            .ensure_layout(&paths, 1, &plan)
            .expect("plan should succeed");
        assert!(paths.root.join("a").exists());
        assert!(paths.root.join("b").exists());
        assert_eq!(VersionManager.read(&paths).layout_version, 1);
    }

    #[test]
    fn plan_rolls_back_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let plan = MigrationPlan::new("test")
            .step(MigrationStep::new(
                "a",
                |p| {
                    fs::write(p.root.join("a"), "1")?;
                    Ok(())
                },
                |p| {
                    let _ = fs::remove_file(p.root.join("a"));
                    Ok(())
                },
            ))
            .step(MigrationStep::new(
                "b",
                |_p| anyhow::bail!("boom"),
                |_p| Ok(()),
            ));
        let err = VersionManager.ensure_layout(&paths, 1, &plan);
        assert!(err.is_err(), "plan should fail");
        // step a 的副作用应被回滚
        assert!(!paths.root.join("a").exists());
        // 版本未推进
        assert_eq!(VersionManager.read(&paths).layout_version, 0);
    }

    #[test]
    fn ensure_layout_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let plan = MigrationPlan::new("test").step(MigrationStep::new(
            "a",
            |p| {
                fs::write(p.root.join("a"), "1")?;
                Ok(())
            },
            |p| {
                let _ = fs::remove_file(p.root.join("a"));
                Ok(())
            },
        ));
        VersionManager.ensure_layout(&paths, 1, &plan).unwrap();
        // 再次运行：已达标，跳过 plan（不重复 apply，仍幂等）
        VersionManager.ensure_layout(&paths, 1, &plan).unwrap();
        assert_eq!(VersionManager.read(&paths).layout_version, 1);
    }

    #[test]
    fn set_versions_persist() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        VersionManager.set_db_version(&paths, 5).unwrap();
        VersionManager.set_config_version(&paths, 3).unwrap();
        VersionManager.set_bundle_version(&paths, 2).unwrap();
        let m = VersionManager.read(&paths);
        assert_eq!(m.db_version, 5);
        assert_eq!(m.config_version, 3);
        assert_eq!(m.bundle_version, 2);
    }
}
