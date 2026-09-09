//! 工作区与终端布局（`WorkspaceData`）的 Repository 层。
//!
//! 工作区数据原先持久化在 profile 根目录的 `workspace.json`。现统一落入 `velowork.db` 的
//! `workspace` 表：以 `id = 'default'` 为主工作区行存储序列化 JSON。

use std::sync::Arc;

use anyhow::{Context, Result};
use velowork_core::storage::{Database, rusqlite};
use velowork_state::WorkspaceData;

use super::now_iso8601;

/// `workspace` 表的数据访问对象。
pub struct WorkspaceRepository {
    db: Arc<Database>,
}

impl WorkspaceRepository {
    pub const DEFAULT_WORKSPACE_ID: &'static str = "default";

    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// 表中是否有任何记录。
    pub fn is_empty(&self) -> Result<bool> {
        let conn = self.db.conn();
        let count: u32 = conn
            .query_row("SELECT COUNT(*) FROM workspace", [], |r| r.get(0))
            .context("count workspace")?;
        Ok(count == 0)
    }

    /// 加载指定 ID 的工作区数据（默认 ID 为 `"default"`）。
    pub fn load_workspace(&self, id: &str) -> Result<Option<WorkspaceData>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare("SELECT data FROM workspace WHERE id = ?1")
            .context("prepare load workspace")?;
        let mut rows = stmt
            .query_map(rusqlite::params![id], |row| {
                let data: String = row.get(0)?;
                Ok(data)
            })
            .context("query workspace")?;

        if let Some(row) = rows.next() {
            let json_str = row.context("read workspace data row")?;
            let data: WorkspaceData =
                serde_json::from_str(&json_str).context("deserialize WorkspaceData")?;
            Ok(Some(data))
        } else {
            Ok(None)
        }
    }

    /// 保存指定 ID 的工作区数据（UPSERT 语义）。
    pub fn save_workspace(&self, id: &str, name: Option<&str>, data: &WorkspaceData) -> Result<()> {
        let conn = self.db.conn();
        let json_str = serde_json::to_string(data).context("serialize WorkspaceData")?;
        let now = now_iso8601();

        conn.execute(
            "INSERT INTO workspace (id, name, data, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(id) DO UPDATE SET \
                 name = excluded.name, \
                 data = excluded.data, \
                 updated_at = excluded.updated_at",
            rusqlite::params![id, name, json_str, now, now],
        )
        .context("upsert workspace")?;

        Ok(())
    }

    /// 导出指定 ID 的原始 JSON 字符串（用于同步 Bundle）。
    pub fn export_data(&self, id: &str) -> Result<Option<String>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare("SELECT data FROM workspace WHERE id = ?1")
            .context("prepare export workspace data")?;
        let mut rows = stmt
            .query_map(rusqlite::params![id], |row| row.get::<_, String>(0))
            .context("query export workspace data")?;

        if let Some(row) = rows.next() {
            Ok(Some(row.context("read export workspace data")?))
        } else {
            Ok(None)
        }
    }

    /// 导入原始 JSON 字符串（用于同步恢复）。
    pub fn import_data(&self, id: &str, name: Option<&str>, json_data: &str) -> Result<()> {
        // 校验是否为合法的 WorkspaceData JSON
        let _parsed: WorkspaceData =
            serde_json::from_str(json_data).context("validate import WorkspaceData json")?;

        let conn = self.db.conn();
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO workspace (id, name, data, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(id) DO UPDATE SET \
                 name = excluded.name, \
                 data = excluded.data, \
                 updated_at = excluded.updated_at",
            rusqlite::params![id, name, json_data, now, now],
        )
        .context("import workspace")?;

        Ok(())
    }

    /// 清空所有工作区记录。
    pub fn clear(&self) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM workspace", [])
            .context("clear workspace table")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use velowork_state::ProjectData;

    fn repo() -> WorkspaceRepository {
        WorkspaceRepository::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    fn sample_workspace() -> WorkspaceData {
        WorkspaceData {
            version: 2,
            projects: vec![ProjectData {
                id: "p1".into(),
                name: "Test Project".into(),
                path: "/home/user/project".into(),
                layout: None,
                ..Default::default()
            }],
            project_order: vec!["p1".into()],
            folders: Vec::new(),
            service_panel_heights: std::collections::HashMap::new(),
            main_window: Default::default(),
            extra_windows: Vec::new(),
        }
    }

    #[test]
    fn save_and_load_roundtrip() {
        let r = repo();
        assert!(r.is_empty().unwrap());

        let ws = sample_workspace();
        r.save_workspace("default", Some("Default Workspace"), &ws)
            .unwrap();

        assert!(!r.is_empty().unwrap());
        let loaded = r.load_workspace("default").unwrap().unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].id, "p1");
        assert_eq!(loaded.projects[0].name, "Test Project");
    }

    #[test]
    fn export_and_import_roundtrip() {
        let r1 = repo();
        let ws = sample_workspace();
        r1.save_workspace("default", Some("Default Workspace"), &ws)
            .unwrap();

        let exported = r1.export_data("default").unwrap().unwrap();

        let r2 = repo();
        assert!(r2.is_empty().unwrap());
        r2.import_data("default", Some("Default Workspace"), &exported)
            .unwrap();

        let loaded = r2.load_workspace("default").unwrap().unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].id, "p1");
    }

    #[test]
    fn reload_after_clear_is_none() {
        let r = repo();
        let ws = sample_workspace();
        r.save_workspace("default", None, &ws).unwrap();
        r.clear().unwrap();
        assert!(r.load_workspace("default").unwrap().is_none());
        assert!(r.is_empty().unwrap());
    }
}
