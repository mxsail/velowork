//! 服务监控树（`ServiceNode`）的 Repository 层。
//!
//! 服务树原先持久化在 profile 根目录的 `service_monitors.json`。现统一落入 `velowork.db` 的
//! `service_tree_node` 表：把递归树扁平化为带 `parent_id` + `sort_index` 的行。
//!
//! 设计取舍：每次保存采用「事务内清空 + 递归全量重插」。

use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use velowork_core::storage::{Database, rusqlite};
use velowork_state::{ServiceDefinition, ServiceNode};

use super::now_iso8601;

fn default_revision() -> u64 {
    1
}

/// 表中的一行（扁平化表示）。同时用于同步 bundle 的导入/导出。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceTreeRow {
    pub id: String,
    /// `"folder"` 或 `"service"`。
    pub kind: String,
    /// 父节点 id；`None` 表示根级。
    pub parent_id: Option<String>,
    /// 同级排序下标。
    pub sort_index: i64,
    /// 文件夹名或服务名（冗余存储，便于查询/显示）。
    pub name: String,
    /// 仅文件夹节点有意义。
    pub is_expanded: bool,
    /// 所属项目 id；`None` 表示默认项目。
    pub project_id: Option<String>,
    /// 仅服务节点：`ServiceDefinition` 的 JSON；文件夹为 `None`。
    pub payload: Option<String>,
    pub updated_at: String,
    /// 逻辑版本号（用于因果排序与多设备冲突合并）
    #[serde(default = "default_revision")]
    pub revision: u64,
    /// 修改设备 ID
    #[serde(default)]
    pub device_id: String,
}

/// `service_tree_node` 表的 CRUD / 树映射。
pub struct ServiceTreeRepository {
    db: Arc<Database>,
}

impl ServiceTreeRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// 表是否为空。
    pub fn is_empty(&self) -> Result<bool> {
        let conn = self.db.conn();
        let c: u32 = conn
            .query_row("SELECT COUNT(*) FROM service_tree_node", [], |r| r.get(0))
            .context("count service_tree_node")?;
        Ok(c == 0)
    }

    /// 全量清空（用于恢复覆盖场景）。
    pub fn clear(&self) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM service_tree_node", [])
            .context("clear service_tree_node")?;
        Ok(())
    }

    /// 全量加载服务树（按 `parent_id` 分组、`sort_index` 排序重建）。
    pub fn load_tree(&self) -> Result<Vec<ServiceNode>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, parent_id, sort_index, name, is_expanded, project_id, payload, updated_at, revision, device_id \
                 FROM service_tree_node ORDER BY sort_index ASC",
            )
            .context("prepare load service tree")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ServiceTreeRow {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    parent_id: row.get(2)?,
                    sort_index: row.get(3)?,
                    name: row.get(4)?,
                    is_expanded: row.get::<_, i64>(5)? != 0,
                    project_id: row.get(6)?,
                    payload: row.get(7)?,
                    updated_at: row.get(8)?,
                    revision: row.get::<_, i64>(9).map(|v| v as u64).unwrap_or(1),
                    device_id: row.get(10).unwrap_or_default(),
                })
            })
            .context("query service tree rows")?;
        let mut all = Vec::new();
        for r in rows {
            all.push(r.context("map service tree row")?);
        }
        Ok(build_tree(all))
    }

    /// 全量保存（事务内清空后递归插入）。
    pub fn save_tree(&self, nodes: &[ServiceNode]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin save service tree tx")?;
        tx.execute("DELETE FROM service_tree_node", [])
            .context("clear before save service tree")?;
        let now = now_iso8601();
        insert_nodes(&tx, nodes, None, 0, &now)?;
        tx.commit().context("commit save service tree")?;
        Ok(())
    }

    /// 导出所有行（用于同步 bundle）。
    pub fn export_rows(&self) -> Result<Vec<ServiceTreeRow>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, parent_id, sort_index, name, is_expanded, project_id, payload, updated_at, revision, device_id \
                 FROM service_tree_node ORDER BY sort_index ASC",
            )
            .context("prepare export service rows")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ServiceTreeRow {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    parent_id: row.get(2)?,
                    sort_index: row.get(3)?,
                    name: row.get(4)?,
                    is_expanded: row.get::<_, i64>(5)? != 0,
                    project_id: row.get(6)?,
                    payload: row.get(7)?,
                    updated_at: row.get(8)?,
                    revision: row.get::<_, i64>(9).map(|v| v as u64).unwrap_or(1),
                    device_id: row.get(10).unwrap_or_default(),
                })
            })
            .context("query export service rows")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map export service row")?);
        }
        Ok(out)
    }

    /// 导入所有行（用于同步恢复）：清空后插入。
    pub fn import_rows(&self, rows: &[ServiceTreeRow]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin import service tx")?;
        tx.execute("DELETE FROM service_tree_node", [])
            .context("clear before import service")?;
        for row in rows {
            tx.execute(
                "INSERT INTO service_tree_node \
                 (id, kind, parent_id, sort_index, name, is_expanded, project_id, payload, updated_at, revision, device_id) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![
                    row.id,
                    row.kind,
                    row.parent_id,
                    row.sort_index,
                    row.name,
                    row.is_expanded as i64,
                    row.project_id,
                    row.payload,
                    row.updated_at,
                    row.revision as i64,
                    row.device_id,
                ],
            )
            .context("insert imported service row")?;
        }
        tx.commit().context("commit import service rows")?;
        Ok(())
    }
}

/// 递归把树插入 `service_tree_node`（事务内）。
fn insert_nodes(
    tx: &rusqlite::Transaction<'_>,
    nodes: &[ServiceNode],
    parent_id: Option<&str>,
    base: usize,
    now: &str,
) -> Result<()> {
    for (idx, node) in nodes.iter().enumerate() {
        let sort_index = (base + idx) as i64;
        match node {
            ServiceNode::Folder {
                id,
                name,
                project_id,
                expanded,
                children,
            } => {
                tx.execute(
                    "INSERT INTO service_tree_node \
                     (id, kind, parent_id, sort_index, name, is_expanded, project_id, payload, updated_at) \
                     VALUES (?1,'folder',?2,?3,?4,?5,?6,NULL,?7)",
                    rusqlite::params![
                        id,
                        parent_id,
                        sort_index,
                        name,
                        *expanded as i64,
                        project_id.as_deref(),
                        now
                    ],
                )
                .context("insert service folder node")?;
                insert_nodes(tx, children, Some(id.as_str()), 0, now)?;
            }
            ServiceNode::Service { def } => {
                let payload = serde_json::to_string(def).context("serialize service definition")?;
                tx.execute(
                    "INSERT INTO service_tree_node \
                     (id, kind, parent_id, sort_index, name, is_expanded, project_id, payload, updated_at) \
                     VALUES (?1,'service',?2,?3,?4,1,?5,?6,?7)",
                    rusqlite::params![
                        def.id,
                        parent_id,
                        sort_index,
                        def.name,
                        def.project_id.as_deref(),
                        payload,
                        now
                    ],
                )
                .context("insert service node")?;
            }
        }
    }
    Ok(())
}

/// 把扁平行列表重建为树。`parent_id IS NULL` 视为根级。
fn build_tree(rows: Vec<ServiceTreeRow>) -> Vec<ServiceNode> {
    use std::collections::HashMap;
    let mut by_parent: HashMap<Option<String>, Vec<ServiceTreeRow>> = HashMap::new();
    for r in rows {
        by_parent.entry(r.parent_id.clone()).or_default().push(r);
    }
    for v in by_parent.values_mut() {
        v.sort_by_key(|r| r.sort_index);
    }
    fn build(
        parent: Option<String>,
        by_parent: &HashMap<Option<String>, Vec<ServiceTreeRow>>,
    ) -> Vec<ServiceNode> {
        let children = match by_parent.get(&parent) {
            Some(c) => c,
            None => return Vec::new(),
        };
        children
            .iter()
            .map(|row| {
                if row.kind == "folder" {
                    ServiceNode::Folder {
                        id: row.id.clone(),
                        name: row.name.clone(),
                        project_id: row.project_id.clone(),
                        expanded: row.is_expanded,
                        children: build(Some(row.id.clone()), by_parent),
                    }
                } else {
                    let def: ServiceDefinition = row
                        .payload
                        .as_ref()
                        .and_then(|p| serde_json::from_str(p).ok())
                        .unwrap_or_else(|| ServiceDefinition {
                            id: row.id.clone(),
                            name: row.name.clone(),
                            project_id: row.project_id.clone(),
                            ..Default::default()
                        });
                    ServiceNode::Service { def }
                }
            })
            .collect()
    }
    build(None, &by_parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> ServiceTreeRepository {
        ServiceTreeRepository::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    fn sample_tree() -> Vec<ServiceNode> {
        vec![
            ServiceNode::Folder {
                id: "sf1".into(),
                name: "Web Services".into(),
                project_id: None,
                expanded: true,
                children: vec![ServiceNode::Service {
                    def: ServiceDefinition {
                        id: "svc1".into(),
                        name: "nginx".into(),
                        project_id: None,
                        kind: velowork_state::ServiceKind::Command,
                        session_id: Some("s1".into()),
                        alive_command: "systemctl is-active nginx".into(),
                        ..Default::default()
                    },
                }],
            },
            ServiceNode::Service {
                def: ServiceDefinition {
                    id: "svc2".into(),
                    name: "redis".into(),
                    project_id: None,
                    kind: velowork_state::ServiceKind::Command,
                    session_id: Some("s2".into()),
                    alive_command: "systemctl is-active redis".into(),
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
            ServiceNode::Folder { id, children, .. } => {
                assert_eq!(id, "sf1");
                assert_eq!(children.len(), 1);
            }
            _ => panic!("expected folder first"),
        }
        match &loaded[1] {
            ServiceNode::Service { def } => {
                assert_eq!(def.id, "svc2");
            }
            _ => panic!("expected service second"),
        }
    }

    #[test]
    fn export_import_rows_roundtrip() {
        let r = repo();
        r.save_tree(&sample_tree()).unwrap();
        let rows = r.export_rows().unwrap();
        assert_eq!(rows.len(), 3); // 1 folder + 2 services

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
}
