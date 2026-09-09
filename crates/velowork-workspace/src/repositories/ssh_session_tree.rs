//! SSH 会话连接树（`SessionTreeNode`）的 Repository 层。
//!
//! 会话树原先持久化在 profile 根目录的 `ssh_sessions.json`（嵌套 JSON）。现统一
//! 落入 `velowork.db` 的 `session_tree_node` 表：把递归树扁平化为带 `parent_id`
//! + `sort_index` 的行，与 `host` / `history` 等其他业务表共存于单库，便于统一
//! 查询与同步。
//!
//! 设计取舍：每次保存采用「事务内清空 + 递归全量重插」。会话树规模很小
//! （几十到几百节点），全量重写比维护增量 diff 简单且不易出错；`updated_at`
//! 仅作审计，不影响重建顺序（顺序由 `sort_index` 决定）。

use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use velowork_core::storage::{Database, rusqlite};
use velowork_state::{SessionTreeNode, SshSession, SshSessionConfig};

use super::now_iso8601;

fn default_revision() -> u64 {
    1
}

/// 表中的一行（扁平化表示）。同时用于同步 bundle 的导入/导出。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionTreeRow {
    pub id: String,
    /// `"folder"` 或 `"session"`。
    pub kind: String,
    /// 父节点 id；`None` 表示根级。
    pub parent_id: Option<String>,
    /// 同级排序下标。
    pub sort_index: i64,
    /// 文件夹名或会话名（冗余存储，便于查询/显示）。
    pub name: String,
    /// 仅文件夹节点有意义。
    pub is_collapsed: bool,
    /// 仅会话节点：`SshSession` 的 JSON；文件夹为 `None`。
    pub payload: Option<String>,
    pub updated_at: String,
    /// 所属项目 id；`None` 表示默认项目。
    pub project_id: Option<String>,
    /// 逻辑版本号（用于因果排序与多设备冲突合并）
    #[serde(default = "default_revision")]
    pub revision: u64,
    /// 修改设备 ID
    #[serde(default)]
    pub device_id: String,
}

/// `session_tree_node` 表的纯 CRUD / 树映射。
pub struct SshSessionTreeRepository {
    db: Arc<Database>,
}

impl SshSessionTreeRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// 表是否为空（用于判断是否需要从旧 JSON 迁移）。
    pub fn is_empty(&self) -> Result<bool> {
        let conn = self.db.conn();
        let c: u32 = conn
            .query_row("SELECT COUNT(*) FROM session_tree_node", [], |r| r.get(0))
            .context("count session_tree_node")?;
        Ok(c == 0)
    }

    /// 全量清空（用于恢复覆盖场景）。
    pub fn clear(&self) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM session_tree_node", [])
            .context("clear session_tree_node")?;
        Ok(())
    }

    /// 全量加载会话树（按 `parent_id` 分组、`sort_index` 排序重建）。
    /// 仅加载默认项目（project_id IS NULL）的节点，保持向后兼容。
    pub fn load_tree(&self) -> Result<Vec<SessionTreeNode>> {
        self.load_tree_for_project(None)
    }

    /// 加载特定项目的会话树。
    pub fn load_tree_for_project(&self, project_id: Option<&str>) -> Result<Vec<SessionTreeNode>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, parent_id, sort_index, name, is_collapsed, payload, updated_at, project_id, revision, device_id \
                 FROM session_tree_node WHERE project_id IS ?1 ORDER BY sort_index ASC",
            )
            .context("prepare load tree")?;
        let rows = stmt
            .query_map(rusqlite::params![project_id], |row| {
                Ok(SessionTreeRow {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    parent_id: row.get(2)?,
                    sort_index: row.get(3)?,
                    name: row.get(4)?,
                    is_collapsed: row.get::<_, i64>(5)? != 0,
                    payload: row.get(6)?,
                    updated_at: row.get(7)?,
                    project_id: row.get(8)?,
                    revision: row.get::<_, i64>(9).map(|v| v as u64).unwrap_or(1),
                    device_id: row.get(10).unwrap_or_default(),
                })
            })
            .context("query tree rows")?;
        let mut all = Vec::new();
        for r in rows {
            all.push(r.context("map tree row")?);
        }
        Ok(build_tree(all))
    }

    /// 加载完整的 SshSessionConfig（包含默认树和所有项目树）。
    pub fn load_config(&self) -> Result<SshSessionConfig> {
        use std::collections::HashMap;
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, parent_id, sort_index, name, is_collapsed, payload, updated_at, project_id, revision, device_id \
                 FROM session_tree_node ORDER BY sort_index ASC",
            )
            .context("prepare load config")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SessionTreeRow {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    parent_id: row.get(2)?,
                    sort_index: row.get(3)?,
                    name: row.get(4)?,
                    is_collapsed: row.get::<_, i64>(5)? != 0,
                    payload: row.get(6)?,
                    updated_at: row.get(7)?,
                    project_id: row.get(8)?,
                    revision: row.get::<_, i64>(9).map(|v| v as u64).unwrap_or(1),
                    device_id: row.get(10).unwrap_or_default(),
                })
            })
            .context("query config rows")?;

        let mut default_rows: Vec<SessionTreeRow> = Vec::new();
        let mut project_rows: HashMap<String, Vec<SessionTreeRow>> = HashMap::new();
        for r in rows {
            let row = r.context("map config row")?;
            match &row.project_id {
                None => default_rows.push(row),
                Some(pid) => project_rows.entry(pid.clone()).or_default().push(row),
            }
        }

        let tree = build_tree(default_rows);
        let by_project: HashMap<String, Vec<SessionTreeNode>> = project_rows
            .into_iter()
            .map(|(pid, rows)| (pid, build_tree(rows)))
            .collect();

        Ok(SshSessionConfig { tree, by_project })
    }

    /// 全量保存默认项目树。
    pub fn save_tree(&self, nodes: &[SessionTreeNode]) -> Result<()> {
        self.save_tree_for_project(None, nodes)
    }

    /// 保存指定项目的会话树（事务内仅清空该项目的节点并插入）。
    pub fn save_tree_for_project(&self, project_id: Option<&str>, nodes: &[SessionTreeNode]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin save_tree_for_project tx")?;
        tx.execute("DELETE FROM session_tree_node WHERE project_id IS ?1", rusqlite::params![project_id])
            .context("clear project tree before save")?;
        let now = now_iso8601();
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO session_tree_node \
                 (id, kind, parent_id, sort_index, name, is_collapsed, payload, updated_at, project_id, revision, device_id) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,1,'')",
            ).context("prepare insert stmt")?;
            insert_nodes(&mut stmt, nodes, None, 0, &now, project_id)?;
        }
        tx.commit().context("commit save_tree_for_project")?;
        Ok(())
    }

    /// 插入指定项目的行列表（用于项目导入）。
    pub fn insert_project_rows(&self, project_id: Option<&str>, rows: &[SessionTreeRow]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin insert_project_rows tx")?;
        tx.execute("DELETE FROM session_tree_node WHERE project_id IS ?1", rusqlite::params![project_id])
            .context("clear before insert_project_rows")?;
        for row in rows {
            tx.execute(
                "INSERT INTO session_tree_node \
                 (id, kind, parent_id, sort_index, name, is_collapsed, payload, updated_at, project_id, revision, device_id) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![
                    row.id,
                    row.kind,
                    row.parent_id,
                    row.sort_index,
                    row.name,
                    row.is_collapsed as i64,
                    row.payload,
                    row.updated_at,
                    project_id,
                    row.revision as i64,
                    row.device_id
                ],
            )
            .context("insert imported project row")?;
        }
        tx.commit().context("commit insert_project_rows")?;
        Ok(())
    }

    /// 删除指定项目的所有会话节点。
    pub fn delete_tree_for_project(&self, project_id: Option<&str>) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM session_tree_node WHERE project_id IS ?1", rusqlite::params![project_id])
            .context("delete tree for project")?;
        Ok(())
    }

    /// 全量保存完整的 SshSessionConfig（默认树 + 所有项目树）。
    pub fn save_config(&self, config: &SshSessionConfig) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin save_config tx")?;
        tx.execute("DELETE FROM session_tree_node", [])
            .context("clear before save_config")?;
        let now = now_iso8601();
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO session_tree_node \
                 (id, kind, parent_id, sort_index, name, is_collapsed, payload, updated_at, project_id, revision, device_id) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,1,'')",
            ).context("prepare insert stmt")?;
            // 写入默认项目树
            insert_nodes(&mut stmt, &config.tree, None, 0, &now, None)?;
            // 写入各项目独立树
            for (pid, tree) in &config.by_project {
                insert_nodes(&mut stmt, tree, None, 0, &now, Some(pid.as_str()))?;
            }
        }
        tx.commit().context("commit save_config tx")?;
        Ok(())
    }

    /// 导出所有行（用于同步 bundle）。
    pub fn export_rows(&self) -> Result<Vec<SessionTreeRow>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, parent_id, sort_index, name, is_collapsed, payload, updated_at, project_id, revision, device_id \
                 FROM session_tree_node ORDER BY sort_index ASC",
            )
            .context("prepare export rows")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SessionTreeRow {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    parent_id: row.get(2)?,
                    sort_index: row.get(3)?,
                    name: row.get(4)?,
                    is_collapsed: row.get::<_, i64>(5)? != 0,
                    payload: row.get(6)?,
                    updated_at: row.get(7)?,
                    project_id: row.get(8)?,
                    revision: row.get::<_, i64>(9).map(|v| v as u64).unwrap_or(1),
                    device_id: row.get(10).unwrap_or_default(),
                })
            })
            .context("query export rows")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map export row")?);
        }
        Ok(out)
    }

    /// 导入所有行（用于同步恢复）：清空后插入。
    pub fn import_rows(&self, rows: &[SessionTreeRow]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin import tx")?;
        tx.execute("DELETE FROM session_tree_node", [])
            .context("clear before import")?;
        for row in rows {
            tx.execute(
                "INSERT INTO session_tree_node \
                 (id, kind, parent_id, sort_index, name, is_collapsed, payload, updated_at, project_id, revision, device_id) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![
                    row.id,
                    row.kind,
                    row.parent_id,
                    row.sort_index,
                    row.name,
                    row.is_collapsed as i64,
                    row.payload,
                    row.updated_at,
                    row.project_id,
                    row.revision as i64,
                    row.device_id
                ],
            )
            .context("insert imported row")?;
        }
        tx.commit().context("commit import rows")?;
        Ok(())
    }
}

/// 递归把树插入 `session_tree_node`（事务内）。
fn insert_nodes(
    stmt: &mut rusqlite::CachedStatement<'_>,
    nodes: &[SessionTreeNode],
    parent_id: Option<&str>,
    base: usize,
    now: &str,
    project_id: Option<&str>,
) -> Result<()> {
    for (idx, node) in nodes.iter().enumerate() {
        let sort_index = (base + idx) as i64;
        match node {
            SessionTreeNode::Folder {
                id,
                name,
                children,
                is_collapsed,
            } => {
                stmt.execute(
                    rusqlite::params![id, "folder", parent_id, sort_index, name, *is_collapsed as i64, Option::<String>::None, now, project_id],
                )
                .context("insert folder node")?;
                insert_nodes(stmt, children, Some(id.as_str()), 0, now, project_id)?;
            }
            SessionTreeNode::Session { session } => {
                let payload = serde_json::to_string(session).context("serialize session")?;
                stmt.execute(
                    rusqlite::params![
                        session.id,
                        "session",
                        parent_id,
                        sort_index,
                        session.name,
                        0i64,
                        Some(&payload),
                        now,
                        project_id
                    ],
                )
                .context("insert session node")?;
            }
        }
    }
    Ok(())
}

/// 把扁平行列表重建为树。`parent_id IS NULL` 视为根级；
/// 重建时会把父文件夹 id 回填进会话的 `parent_folder_id`，保持 `update_session`
/// 的父目录变化判定逻辑一致。
fn build_tree(rows: Vec<SessionTreeRow>) -> Vec<SessionTreeNode> {
    use std::collections::HashMap;
    let mut by_parent: HashMap<Option<String>, Vec<SessionTreeRow>> = HashMap::new();
    for r in rows {
        by_parent.entry(r.parent_id.clone()).or_default().push(r);
    }
    for v in by_parent.values_mut() {
        v.sort_by_key(|r| r.sort_index);
    }
    fn build(
        parent: Option<String>,
        by_parent: &HashMap<Option<String>, Vec<SessionTreeRow>>,
    ) -> Vec<SessionTreeNode> {
        let children = match by_parent.get(&parent) {
            Some(c) => c,
            None => return Vec::new(),
        };
        children
            .iter()
            .map(|row| {
                if row.kind == "folder" {
                    SessionTreeNode::Folder {
                        id: row.id.clone(),
                        name: row.name.clone(),
                        children: build(Some(row.id.clone()), by_parent),
                        is_collapsed: row.is_collapsed,
                    }
                } else {
                    let mut session: SshSession = row
                        .payload
                        .as_ref()
                        .and_then(|p| serde_json::from_str(p).ok())
                        .unwrap_or_else(|| SshSession {
                            id: row.id.clone(),
                            name: row.name.clone(),
                            ..Default::default()
                        });
                    session.parent_folder_id = parent.clone();
                    SessionTreeNode::Session { session }
                }
            })
            .collect()
    }
    let mut tree = build(None, &by_parent);
    // Ensure the canonical "sessions before folders" sibling ordering on load,
    // regardless of the persisted sort_index values.
    crate::ssh_sessions::sort_siblings(&mut tree);
    tree
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> SshSessionTreeRepository {
        SshSessionTreeRepository::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    fn sample_tree() -> Vec<SessionTreeNode> {
        vec![
            SessionTreeNode::Folder {
                id: "f1".into(),
                name: "Personal".into(),
                is_collapsed: false,
                children: vec![SessionTreeNode::Session {
                    session: SshSession {
                        id: "s1".into(),
                        name: "dev".into(),
                        host: "1.1.1.1".into(),
                        port: 22,
                        username: "root".into(),
                        ..Default::default()
                    },
                }],
            },
            SessionTreeNode::Session {
                session: SshSession {
                    id: "s2".into(),
                    name: "top".into(),
                    host: "2.2.2.2".into(),
                    ..Default::default()
                },
            },
        ]
    }

    #[test]
    fn save_and_load_roundtrip() {
        let r = repo();
        let tree = sample_tree();
        r.save_tree(&tree).unwrap();
        let loaded = r.load_tree().unwrap();
        assert_eq!(loaded.len(), 2);
        match &loaded[0] {
            SessionTreeNode::Session { session } => {
                assert_eq!(session.id, "s2");
                assert_eq!(session.parent_folder_id, None);
            }
            _ => panic!("expected root session first"),
        }
        match &loaded[1] {
            SessionTreeNode::Folder { id, children, .. } => {
                assert_eq!(id, "f1");
                assert_eq!(children.len(), 1);
                if let SessionTreeNode::Session { session } = &children[0] {
                    // parent_folder_id 应被回填
                    assert_eq!(session.parent_folder_id.as_deref(), Some("f1"));
                } else {
                    panic!("expected session child");
                }
            }
            _ => panic!("expected folder second"),
        }
    }

    #[test]
    fn export_import_rows_roundtrip() {
        let r = repo();
        r.save_tree(&sample_tree()).unwrap();
        let rows = r.export_rows().unwrap();
        assert_eq!(rows.len(), 3); // 1 folder + 2 sessions

        let r2 = repo();
        assert!(r2.is_empty().unwrap());
        r2.import_rows(&rows).unwrap();
        let loaded = r2.load_tree().unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn is_empty_reflects_state() {
        let r = repo();
        assert!(r.is_empty().unwrap());
        r.save_tree(&sample_tree()).unwrap();
        assert!(!r.is_empty().unwrap());
    }

    #[test]
    fn reload_after_clear_is_empty_tree() {
        let r = repo();
        r.save_tree(&sample_tree()).unwrap();
        r.clear().unwrap();
        assert!(r.load_tree().unwrap().is_empty());
    }

    // 确保 SshSessionConfig 与仓库互操作（迁移场景会用到）。
    #[test]
    fn config_to_tree_and_back() {
        let tree = sample_tree();
        let cfg = SshSessionConfig { tree, by_project: std::collections::HashMap::new() };
        let r = repo();
        r.save_tree(&cfg.tree).unwrap();
        let loaded = r.load_tree().unwrap();
        assert_eq!(loaded.len(), 2);
        // 再次落库后仍能读回
        r.save_tree(&loaded).unwrap();
        assert_eq!(r.load_tree().unwrap().len(), 2);
    }

    #[test]
    fn save_and_load_config_with_projects() {
        let r = repo();
        let mut config = SshSessionConfig {
            tree: sample_tree(),
            by_project: std::collections::HashMap::new(),
        };
        config.by_project.insert("proj-1".into(), vec![
            SessionTreeNode::Session {
                session: SshSession {
                    id: "s3".into(),
                    name: "proj1-session".into(),
                    host: "3.3.3.3".into(),
                    ..Default::default()
                },
            },
        ]);
        r.save_config(&config).unwrap();
        let loaded = r.load_config().unwrap();
        assert_eq!(loaded.tree.len(), 2);
        assert_eq!(loaded.by_project.len(), 1);
        assert_eq!(loaded.by_project["proj-1"].len(), 1);
    }
}
