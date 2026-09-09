//! 隧道树（`TunnelNode`）的 Repository 层。
//!
//! 隧道树原先持久化在 profile 根目录的 `tunnels.json`。现统一落入 `velowork.db` 的
//! `tunnel_tree_node` 表：把递归树扁平化为带 `parent_id` + `sort_index` 的行。
//!
//! 设计取舍：每次保存采用「事务内清空 + 递归全量重插」。

use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use velowork_core::storage::{Database, rusqlite};
use velowork_state::{TunnelNode, TunnelProfile};

use super::now_iso8601;

fn default_revision() -> u64 {
    1
}

/// 表中的一行（扁平化表示）。同时用于同步 bundle 的导入/导出。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelTreeRow {
    pub id: String,
    /// `"folder"` 或 `"tunnel"`。
    pub kind: String,
    /// 父节点 id；`None` 表示根级。
    pub parent_id: Option<String>,
    /// 同级排序下标。
    pub sort_index: i64,
    /// 文件夹名或隧道名（冗余存储，便于查询/显示）。
    pub name: String,
    /// 仅文件夹节点有意义。
    pub is_expanded: bool,
    /// 所属项目 id；`None` 表示默认项目。
    pub project_id: Option<String>,
    /// 仅隧道节点：`TunnelProfile` 的 JSON；文件夹为 `None`。
    pub payload: Option<String>,
    pub updated_at: String,
    /// 逻辑版本号（用于因果排序与多设备冲突合并）
    #[serde(default = "default_revision")]
    pub revision: u64,
    /// 修改设备 ID
    #[serde(default)]
    pub device_id: String,
}

/// `tunnel_tree_node` 表的 CRUD / 树映射。
pub struct TunnelTreeRepository {
    db: Arc<Database>,
}

impl TunnelTreeRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// 表是否为空。
    pub fn is_empty(&self) -> Result<bool> {
        let conn = self.db.conn();
        let c: u32 = conn
            .query_row("SELECT COUNT(*) FROM tunnel_tree_node", [], |r| r.get(0))
            .context("count tunnel_tree_node")?;
        Ok(c == 0)
    }

    /// 全量清空（用于恢复覆盖场景）。
    pub fn clear(&self) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM tunnel_tree_node", [])
            .context("clear tunnel_tree_node")?;
        Ok(())
    }

    /// 全量加载隧道树（按 `parent_id` 分组、`sort_index` 排序重建）。
    pub fn load_tree(&self) -> Result<Vec<TunnelNode>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, parent_id, sort_index, name, is_expanded, project_id, payload, updated_at, revision, device_id \
                 FROM tunnel_tree_node ORDER BY sort_index ASC",
            )
            .context("prepare load tunnel tree")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(TunnelTreeRow {
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
            .context("query tunnel tree rows")?;
        let mut all = Vec::new();
        for r in rows {
            all.push(r.context("map tunnel tree row")?);
        }
        Ok(build_tree(all))
    }

    /// 全量保存（事务内清空后递归插入）。
    pub fn save_tree(&self, nodes: &[TunnelNode]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin save tunnel tree tx")?;
        tx.execute("DELETE FROM tunnel_tree_node", [])
            .context("clear before save tunnel tree")?;
        let now = now_iso8601();
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO tunnel_tree_node \
                 (id, kind, parent_id, sort_index, name, is_expanded, project_id, payload, updated_at, revision, device_id) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,1,'')",
            ).context("prepare insert tunnel stmt")?;
            insert_nodes(&mut stmt, nodes, None, 0, &now)?;
        }
        tx.commit().context("commit save tunnel tree")?;
        Ok(())
    }

    /// 导出所有行（用于同步 bundle）。
    pub fn export_rows(&self) -> Result<Vec<TunnelTreeRow>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, parent_id, sort_index, name, is_expanded, project_id, payload, updated_at, revision, device_id \
                 FROM tunnel_tree_node ORDER BY sort_index ASC",
            )
            .context("prepare export tunnel rows")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(TunnelTreeRow {
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
            .context("query export tunnel rows")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map export tunnel row")?);
        }
        Ok(out)
    }

    /// 导入所有行（用于同步恢复）：清空后插入。
    pub fn import_rows(&self, rows: &[TunnelTreeRow]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin import tunnel tx")?;
        tx.execute("DELETE FROM tunnel_tree_node", [])
            .context("clear before import tunnel")?;
        for row in rows {
            tx.execute(
                "INSERT INTO tunnel_tree_node \
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
            .context("insert imported tunnel row")?;
        }
        tx.commit().context("commit import tunnel rows")?;
        Ok(())
    }
}

/// 递归把树插入 `tunnel_tree_node`（事务内）。
fn insert_nodes(
    stmt: &mut rusqlite::CachedStatement<'_>,
    nodes: &[TunnelNode],
    parent_id: Option<&str>,
    base: usize,
    now: &str,
) -> Result<()> {
    for (idx, node) in nodes.iter().enumerate() {
        let sort_index = (base + idx) as i64;
        match node {
            TunnelNode::Folder {
                id,
                name,
                project_id,
                expanded,
                children,
            } => {
                stmt.execute(
                    rusqlite::params![
                        id,
                        "folder",
                        parent_id,
                        sort_index,
                        name,
                        *expanded as i64,
                        project_id.as_deref(),
                        Option::<String>::None,
                        now
                    ],
                )
                .context("insert tunnel folder node")?;
                insert_nodes(stmt, children, Some(id.as_str()), 0, now)?;
            }
            TunnelNode::Tunnel { profile } => {
                let payload = serde_json::to_string(profile).context("serialize tunnel profile")?;
                stmt.execute(
                    rusqlite::params![
                        profile.id,
                        "tunnel",
                        parent_id,
                        sort_index,
                        profile.name,
                        1i64,
                        profile.project_id.as_deref(),
                        Some(&payload),
                        now
                    ],
                )
                .context("insert tunnel node")?;
            }
        }
    }
    Ok(())
}

/// 把扁平行列表重建为树。`parent_id IS NULL` 视为根级。
fn build_tree(rows: Vec<TunnelTreeRow>) -> Vec<TunnelNode> {
    use std::collections::HashMap;
    let mut by_parent: HashMap<Option<String>, Vec<TunnelTreeRow>> = HashMap::new();
    for r in rows {
        by_parent.entry(r.parent_id.clone()).or_default().push(r);
    }
    for v in by_parent.values_mut() {
        v.sort_by_key(|r| r.sort_index);
    }
    fn build(
        parent: Option<String>,
        by_parent: &HashMap<Option<String>, Vec<TunnelTreeRow>>,
    ) -> Vec<TunnelNode> {
        let children = match by_parent.get(&parent) {
            Some(c) => c,
            None => return Vec::new(),
        };
        children
            .iter()
            .map(|row| {
                if row.kind == "folder" {
                    TunnelNode::Folder {
                        id: row.id.clone(),
                        name: row.name.clone(),
                        project_id: row.project_id.clone(),
                        expanded: row.is_expanded,
                        children: build(Some(row.id.clone()), by_parent),
                    }
                } else {
                    let profile: TunnelProfile = row
                        .payload
                        .as_ref()
                        .and_then(|p| serde_json::from_str(p).ok())
                        .unwrap_or_else(|| TunnelProfile {
                            id: row.id.clone(),
                            name: row.name.clone(),
                            project_id: row.project_id.clone(),
                            session_id: String::new(),
                            enabled: false,
                            auto_start: false,
                            reconnect: velowork_state::ReconnectPolicy::InheritSession,
                            kind: velowork_state::TunnelKind::Dynamic {
                                local_bind: "127.0.0.1:1080".parse().unwrap(),
                            },
                            description: None,
                        });
                    TunnelNode::Tunnel { profile }
                }
            })
            .collect()
    }
    let mut tree = build(None, &by_parent);
    crate::tunnels::tunnel_sort_siblings(&mut tree);
    tree
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> TunnelTreeRepository {
        TunnelTreeRepository::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    fn sample_tree() -> Vec<TunnelNode> {
        vec![
            TunnelNode::Folder {
                id: "tf1".into(),
                name: "Work".into(),
                project_id: None,
                expanded: true,
                children: vec![TunnelNode::Tunnel {
                    profile: TunnelProfile {
                        id: "t1".into(),
                        name: "dev-fwd".into(),
                        project_id: None,
                        session_id: "s1".into(),
                        enabled: true,
                        auto_start: false,
                        reconnect: velowork_state::ReconnectPolicy::InheritSession,
                        kind: velowork_state::TunnelKind::Dynamic {
                            local_bind: "127.0.0.1:1080".parse().unwrap(),
                        },
                        description: None,
                    },
                }],
            },
            TunnelNode::Tunnel {
                profile: TunnelProfile {
                    id: "t2".into(),
                    name: "root-tunnel".into(),
                    project_id: None,
                    session_id: "s2".into(),
                    enabled: false,
                    auto_start: false,
                    reconnect: velowork_state::ReconnectPolicy::Never,
                    kind: velowork_state::TunnelKind::Local {
                        local_bind: "127.0.0.1:8080".parse().unwrap(),
                        remote_target: "localhost:80".into(),
                    },
                    description: None,
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
        // tunnel_sort_siblings sorts tunnels before folders
        match &loaded[0] {
            TunnelNode::Tunnel { profile } => {
                assert_eq!(profile.id, "t2");
            }
            _ => panic!("expected root tunnel first"),
        }
        match &loaded[1] {
            TunnelNode::Folder { id, children, .. } => {
                assert_eq!(id, "tf1");
                assert_eq!(children.len(), 1);
            }
            _ => panic!("expected folder second"),
        }
    }

    #[test]
    fn export_import_rows_roundtrip() {
        let r = repo();
        r.save_tree(&sample_tree()).unwrap();
        let rows = r.export_rows().unwrap();
        assert_eq!(rows.len(), 3); // 1 folder + 2 tunnels

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
