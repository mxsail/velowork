//! 细粒度三向因果智能合并引擎 (3-Way Fine-grained Causal Merge Engine)
//!
//! 基于 `(revision, updated_at, device_id)` 三级确定性全序，对 Local、Remote 与 Base
//! 快照执行细粒度实体级合并，彻底解决整包覆盖导致的数据丢失与因果错乱问题。

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result};
use base64::Engine as _;
use velowork_core::storage::migrations::CURRENT_SCHEMA_VERSION;

use crate::repositories::ai::{AiConversationRow, AiMessageRow};
use crate::repositories::credential::CredentialMeta;
use crate::repositories::history::HistoryEntry;
use crate::repositories::service_tree::ServiceTreeRow;
use crate::repositories::ssh_session_tree::SessionTreeRow;
use crate::repositories::tunnel_tree::TunnelTreeRow;
use crate::quick_commands::QuickCommandNode;
use crate::settings::SyncDataScope;
use crate::sync::bundle::{Bundle, BundlePart};

/// Profile Layout 版本（v2 分层布局）。
const PROFILE_LAYOUT_VERSION: u32 = 2;

/// 三级确定性全序比较函数：
/// 1. 逻辑版本号 `revision`（优先）
/// 2. 物理时间戳 `updated_at`（辅助）
/// 3. 设备标识 `device_id` 字典序（终裁，保证全网多端绝对收敛一致）
pub fn compare_causal_order(
    rev_a: u64,
    ts_a: &str,
    dev_a: &str,
    rev_b: u64,
    ts_b: &str,
    dev_b: &str,
) -> Ordering {
    match rev_a.cmp(&rev_b) {
        Ordering::Equal => match ts_a.cmp(ts_b) {
            Ordering::Equal => dev_a.cmp(dev_b),
            other => other,
        },
        other => other,
    }
}

/// 智能合并引擎
pub struct BundleMerger;

impl BundleMerger {
    /// 执行两个（或结合 Base）Bundle 的三向因果智能合并
    pub fn merge(
        base: Option<&Bundle>,
        local: &Bundle,
        remote: &Bundle,
        local_device_id: &str,
        scope: &SyncDataScope,
    ) -> Result<Bundle> {
        let mut merged = Bundle::new(
            &local.profile_id,
            CURRENT_SCHEMA_VERSION,
            PROFILE_LAYOUT_VERSION,
        );

        // 1. 合并 config/ (含 settings.json, quick_commands 等)
        if scope.settings || scope.quick_commands {
            let base_config = base.and_then(|b| b.get_part("config"));
            let local_config = local.get_part("config");
            let remote_config = remote.get_part("config");
            if let Some(merged_config) =
                Self::merge_dir_json(base_config, local_config, remote_config)?
            {
                merged.parts.push(BundlePart {
                    name: "config".into(),
                    data: merged_config,
                });
            }
        }

        // 2. 合并 themes/
        if scope.themes {
            let base_themes = base.and_then(|b| b.get_part("themes"));
            let local_themes = local.get_part("themes");
            let remote_themes = remote.get_part("themes");
            if let Some(merged_themes) =
                Self::merge_dir_json(base_themes, local_themes, remote_themes)?
            {
                merged.parts.push(BundlePart {
                    name: "themes".into(),
                    data: merged_themes,
                });
            }
        }

        // 3. 合并 sessions/ (本地 sshconfig/pem/import 目录)
        if scope.sessions {
            let base_sessions = base.and_then(|b| b.get_part("sessions"));
            let local_sessions = local.get_part("sessions");
            let remote_sessions = remote.get_part("sessions");
            if let Some(merged_sessions) =
                Self::merge_dir_json(base_sessions, local_sessions, remote_sessions)?
            {
                merged.parts.push(BundlePart {
                    name: "sessions".into(),
                    data: merged_sessions,
                });
            }
        }

        // 4. 合并 SSH 会话树 (session_tree)
        if scope.sessions {
            let base_rows: Vec<SessionTreeRow> = base
                .and_then(|b| b.get_part("session_tree"))
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let local_rows: Vec<SessionTreeRow> = local
                .get_part("session_tree")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let remote_rows: Vec<SessionTreeRow> = remote
                .get_part("session_tree")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();

            let merged_rows = Self::merge_session_trees(
                &base_rows,
                &local_rows,
                &remote_rows,
                local_device_id,
            );
            if !merged_rows.is_empty() {
                merged.parts.push(BundlePart {
                    name: "session_tree".into(),
                    data: serde_json::to_vec(&merged_rows).context("serialize merged session_tree")?,
                });
            }
        }

        // 5. 合并 隧道树 (tunnel_tree)
        if scope.tunnels {
            let base_rows: Vec<TunnelTreeRow> = base
                .and_then(|b| b.get_part("tunnel_tree"))
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let local_rows: Vec<TunnelTreeRow> = local
                .get_part("tunnel_tree")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let remote_rows: Vec<TunnelTreeRow> = remote
                .get_part("tunnel_tree")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();

            let merged_rows = Self::merge_tunnel_trees(
                &base_rows,
                &local_rows,
                &remote_rows,
                local_device_id,
            );
            if !merged_rows.is_empty() {
                merged.parts.push(BundlePart {
                    name: "tunnel_tree".into(),
                    data: serde_json::to_vec(&merged_rows).context("serialize merged tunnel_tree")?,
                });
            }
        }

        // 6. 合并 服务监控树 (service_tree)
        if scope.services {
            let base_rows: Vec<ServiceTreeRow> = base
                .and_then(|b| b.get_part("service_tree"))
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let local_rows: Vec<ServiceTreeRow> = local
                .get_part("service_tree")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let remote_rows: Vec<ServiceTreeRow> = remote
                .get_part("service_tree")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();

            let merged_rows = Self::merge_service_trees(
                &base_rows,
                &local_rows,
                &remote_rows,
                local_device_id,
            );
            if !merged_rows.is_empty() {
                merged.parts.push(BundlePart {
                    name: "service_tree".into(),
                    data: serde_json::to_vec(&merged_rows).context("serialize merged service_tree")?,
                });
            }
        }

        // 7. 合并 工作区 (workspace)
        if scope.settings {
            let local_ws = local.get_part("workspace");
            let remote_ws = remote.get_part("workspace");
            // 工作区优先保留本地正在运行的工作区结构，若本地无则采用远端
            if let Some(ws) = local_ws.or(remote_ws) {
                merged.parts.push(BundlePart {
                    name: "workspace".into(),
                    data: ws.data.clone(),
                });
            }
        }

        // 8. 合并 AI 对话记录与消息 (ai_conversations / ai_messages)
        if scope.ai_chat {
            let base_convs: Vec<AiConversationRow> = base
                .and_then(|b| b.get_part("ai_conversations"))
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let local_convs: Vec<AiConversationRow> = local
                .get_part("ai_conversations")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let remote_convs: Vec<AiConversationRow> = remote
                .get_part("ai_conversations")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();

            let merged_convs = Self::merge_ai_conversations(
                &base_convs,
                &local_convs,
                &remote_convs,
                local_device_id,
            );
            if !merged_convs.is_empty() {
                merged.parts.push(BundlePart {
                    name: "ai_conversations".into(),
                    data: serde_json::to_vec(&merged_convs).context("serialize merged ai_conversations")?,
                });
            }

            let base_msgs: Vec<AiMessageRow> = base
                .and_then(|b| b.get_part("ai_messages"))
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let local_msgs: Vec<AiMessageRow> = local
                .get_part("ai_messages")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let remote_msgs: Vec<AiMessageRow> = remote
                .get_part("ai_messages")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();

            let merged_msgs = Self::merge_ai_messages(
                &base_msgs,
                &local_msgs,
                &remote_msgs,
                local_device_id,
            );
            if !merged_msgs.is_empty() {
                merged.parts.push(BundlePart {
                    name: "ai_messages".into(),
                    data: serde_json::to_vec(&merged_msgs).context("serialize merged ai_messages")?,
                });
            }
        }

        // 9. 合并 终端历史命令 (history)
        if scope.command_history {
            let base_hist: Vec<HistoryEntry> = base
                .and_then(|b| b.get_part("history"))
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let local_hist: Vec<HistoryEntry> = local
                .get_part("history")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let remote_hist: Vec<HistoryEntry> = remote
                .get_part("history")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();

            let merged_hist = Self::merge_history(&base_hist, &local_hist, &remote_hist);
            if !merged_hist.is_empty() {
                merged.parts.push(BundlePart {
                    name: "history".into(),
                    data: serde_json::to_vec(&merged_hist).context("serialize merged history")?,
                });
            }
        }

        // 10. 合并 账号与连接凭据 (credentials)
        if scope.credentials {
            let base_creds: Vec<(CredentialMeta, String)> = base
                .and_then(|b| b.get_part("credentials"))
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let local_creds: Vec<(CredentialMeta, String)> = local
                .get_part("credentials")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();
            let remote_creds: Vec<(CredentialMeta, String)> = remote
                .get_part("credentials")
                .and_then(|p| serde_json::from_slice(&p.data).ok())
                .unwrap_or_default();

            let merged_creds = Self::merge_credentials(&base_creds, &local_creds, &remote_creds);
            if !merged_creds.is_empty() {
                merged.parts.push(BundlePart {
                    name: "credentials".into(),
                    data: serde_json::to_vec(&merged_creds).context("serialize merged credentials")?,
                });
            }
        }

        Ok(merged)
    }

    /// 会话树三向细粒度因果合并
    pub fn merge_session_trees(
        base: &[SessionTreeRow],
        local: &[SessionTreeRow],
        remote: &[SessionTreeRow],
        local_device_id: &str,
    ) -> Vec<SessionTreeRow> {
        let base_map: HashMap<&str, &SessionTreeRow> =
            base.iter().map(|r| (r.id.as_str(), r)).collect();
        let local_map: HashMap<&str, &SessionTreeRow> =
            local.iter().map(|r| (r.id.as_str(), r)).collect();
        let remote_map: HashMap<&str, &SessionTreeRow> =
            remote.iter().map(|r| (r.id.as_str(), r)).collect();

        let mut all_ids: BTreeSet<&str> = BTreeSet::new();
        all_ids.extend(base_map.keys());
        all_ids.extend(local_map.keys());
        all_ids.extend(remote_map.keys());

        let mut result = Vec::new();
        for id in all_ids {
            let b = base_map.get(id).copied();
            let l = local_map.get(id).copied();
            let r = remote_map.get(id).copied();

            match (b, l, r) {
                (Some(_base_row), Some(local_row), Some(remote_row)) => {
                    let cmp = compare_causal_order(
                        local_row.revision,
                        &local_row.updated_at,
                        &local_row.device_id,
                        remote_row.revision,
                        &remote_row.updated_at,
                        &remote_row.device_id,
                    );
                    let mut winner = if cmp >= Ordering::Equal {
                        local_row.clone()
                    } else {
                        remote_row.clone()
                    };
                    // 若发生并发冲突修改，合并胜出方递增 revision
                    if local_row.payload != remote_row.payload || local_row.name != remote_row.name {
                        winner.revision = local_row.revision.max(remote_row.revision) + 1;
                        winner.device_id = local_device_id.to_string();
                    }
                    result.push(winner);
                }
                (None, Some(local_row), Some(remote_row)) => {
                    // 两端均新增同 ID 节点
                    let cmp = compare_causal_order(
                        local_row.revision,
                        &local_row.updated_at,
                        &local_row.device_id,
                        remote_row.revision,
                        &remote_row.updated_at,
                        &remote_row.device_id,
                    );
                    let mut winner = if cmp >= Ordering::Equal {
                        local_row.clone()
                    } else {
                        remote_row.clone()
                    };
                    winner.revision = local_row.revision.max(remote_row.revision) + 1;
                    winner.device_id = local_device_id.to_string();
                    result.push(winner);
                }
                (Some(base_row), Some(local_row), None) => {
                    // 远端删除了该节点：检查本地是否在 base 后修改过
                    if local_row.revision > base_row.revision || local_row.updated_at > base_row.updated_at {
                        // 本地修改过 -> 保护修改，保留节点并提升版本
                        let mut kept = local_row.clone();
                        kept.revision += 1;
                        kept.device_id = local_device_id.to_string();
                        result.push(kept);
                    }
                    // 否则本地未修改，接受远端删除
                }
                (Some(base_row), None, Some(remote_row)) => {
                    // 本地删除了该节点：检查远端是否在 base 后修改过
                    if remote_row.revision > base_row.revision || remote_row.updated_at > base_row.updated_at {
                        // 远端修改过 -> 保护修改，复活节点
                        result.push(remote_row.clone());
                    }
                    // 否则远端未修改，接受本地删除
                }
                (None, Some(local_row), None) => {
                    // 本地新增
                    result.push(local_row.clone());
                }
                (None, None, Some(remote_row)) => {
                    // 远端新增
                    result.push(remote_row.clone());
                }
                (Some(_), None, None) => {
                    // 双端均已删除
                }
                (None, None, None) => {}
            }
        }

        result.sort_by_key(|r| r.sort_index);
        result
    }

    /// 隧道树三向因果合并
    pub fn merge_tunnel_trees(
        base: &[TunnelTreeRow],
        local: &[TunnelTreeRow],
        remote: &[TunnelTreeRow],
        local_device_id: &str,
    ) -> Vec<TunnelTreeRow> {
        let base_map: HashMap<&str, &TunnelTreeRow> =
            base.iter().map(|r| (r.id.as_str(), r)).collect();
        let local_map: HashMap<&str, &TunnelTreeRow> =
            local.iter().map(|r| (r.id.as_str(), r)).collect();
        let remote_map: HashMap<&str, &TunnelTreeRow> =
            remote.iter().map(|r| (r.id.as_str(), r)).collect();

        let mut all_ids: BTreeSet<&str> = BTreeSet::new();
        all_ids.extend(base_map.keys());
        all_ids.extend(local_map.keys());
        all_ids.extend(remote_map.keys());

        let mut result = Vec::new();
        for id in all_ids {
            let b = base_map.get(id).copied();
            let l = local_map.get(id).copied();
            let r = remote_map.get(id).copied();

            match (b, l, r) {
                (Some(_), Some(local_row), Some(remote_row)) | (None, Some(local_row), Some(remote_row)) => {
                    let cmp = compare_causal_order(
                        local_row.revision,
                        &local_row.updated_at,
                        &local_row.device_id,
                        remote_row.revision,
                        &remote_row.updated_at,
                        &remote_row.device_id,
                    );
                    let mut winner = if cmp >= Ordering::Equal {
                        local_row.clone()
                    } else {
                        remote_row.clone()
                    };
                    if local_row.payload != remote_row.payload || local_row.name != remote_row.name {
                        winner.revision = local_row.revision.max(remote_row.revision) + 1;
                        winner.device_id = local_device_id.to_string();
                    }
                    result.push(winner);
                }
                (Some(base_row), Some(local_row), None) => {
                    if local_row.revision > base_row.revision || local_row.updated_at > base_row.updated_at {
                        let mut kept = local_row.clone();
                        kept.revision += 1;
                        kept.device_id = local_device_id.to_string();
                        result.push(kept);
                    }
                }
                (Some(base_row), None, Some(remote_row)) => {
                    if remote_row.revision > base_row.revision || remote_row.updated_at > base_row.updated_at {
                        result.push(remote_row.clone());
                    }
                }
                (None, Some(local_row), None) => result.push(local_row.clone()),
                (None, None, Some(remote_row)) => result.push(remote_row.clone()),
                _ => {}
            }
        }
        result.sort_by_key(|r| r.sort_index);
        result
    }

    /// 服务树三向因果合并
    pub fn merge_service_trees(
        base: &[ServiceTreeRow],
        local: &[ServiceTreeRow],
        remote: &[ServiceTreeRow],
        local_device_id: &str,
    ) -> Vec<ServiceTreeRow> {
        let base_map: HashMap<&str, &ServiceTreeRow> =
            base.iter().map(|r| (r.id.as_str(), r)).collect();
        let local_map: HashMap<&str, &ServiceTreeRow> =
            local.iter().map(|r| (r.id.as_str(), r)).collect();
        let remote_map: HashMap<&str, &ServiceTreeRow> =
            remote.iter().map(|r| (r.id.as_str(), r)).collect();

        let mut all_ids: BTreeSet<&str> = BTreeSet::new();
        all_ids.extend(base_map.keys());
        all_ids.extend(local_map.keys());
        all_ids.extend(remote_map.keys());

        let mut result = Vec::new();
        for id in all_ids {
            let b = base_map.get(id).copied();
            let l = local_map.get(id).copied();
            let r = remote_map.get(id).copied();

            match (b, l, r) {
                (Some(_), Some(local_row), Some(remote_row)) | (None, Some(local_row), Some(remote_row)) => {
                    let cmp = compare_causal_order(
                        local_row.revision,
                        &local_row.updated_at,
                        &local_row.device_id,
                        remote_row.revision,
                        &remote_row.updated_at,
                        &remote_row.device_id,
                    );
                    let mut winner = if cmp >= Ordering::Equal {
                        local_row.clone()
                    } else {
                        remote_row.clone()
                    };
                    if local_row.payload != remote_row.payload || local_row.name != remote_row.name {
                        winner.revision = local_row.revision.max(remote_row.revision) + 1;
                        winner.device_id = local_device_id.to_string();
                    }
                    result.push(winner);
                }
                (Some(base_row), Some(local_row), None) => {
                    if local_row.revision > base_row.revision || local_row.updated_at > base_row.updated_at {
                        let mut kept = local_row.clone();
                        kept.revision += 1;
                        kept.device_id = local_device_id.to_string();
                        result.push(kept);
                    }
                }
                (Some(base_row), None, Some(remote_row)) => {
                    if remote_row.revision > base_row.revision || remote_row.updated_at > base_row.updated_at {
                        result.push(remote_row.clone());
                    }
                }
                (None, Some(local_row), None) => result.push(local_row.clone()),
                (None, None, Some(remote_row)) => result.push(remote_row.clone()),
                _ => {}
            }
        }
        result.sort_by_key(|r| r.sort_index);
        result
    }

    /// AI 会话三向因果合并
    pub fn merge_ai_conversations(
        base: &[AiConversationRow],
        local: &[AiConversationRow],
        remote: &[AiConversationRow],
        local_device_id: &str,
    ) -> Vec<AiConversationRow> {
        let base_map: HashMap<&str, &AiConversationRow> =
            base.iter().map(|r| (r.id.as_str(), r)).collect();
        let local_map: HashMap<&str, &AiConversationRow> =
            local.iter().map(|r| (r.id.as_str(), r)).collect();
        let remote_map: HashMap<&str, &AiConversationRow> =
            remote.iter().map(|r| (r.id.as_str(), r)).collect();

        let mut all_ids: BTreeSet<&str> = BTreeSet::new();
        all_ids.extend(base_map.keys());
        all_ids.extend(local_map.keys());
        all_ids.extend(remote_map.keys());

        let mut result = Vec::new();
        for id in all_ids {
            let b = base_map.get(id).copied();
            let l = local_map.get(id).copied();
            let r = remote_map.get(id).copied();

            match (b, l, r) {
                (Some(_), Some(local_row), Some(remote_row)) | (None, Some(local_row), Some(remote_row)) => {
                    let cmp = compare_causal_order(
                        local_row.revision,
                        &local_row.updated_at,
                        &local_row.device_id,
                        remote_row.revision,
                        &remote_row.updated_at,
                        &remote_row.device_id,
                    );
                    let mut winner = if cmp >= Ordering::Equal {
                        local_row.clone()
                    } else {
                        remote_row.clone()
                    };
                    if local_row.updated_at != remote_row.updated_at || local_row.title != remote_row.title {
                        winner.revision = local_row.revision.max(remote_row.revision) + 1;
                        winner.device_id = local_device_id.to_string();
                    }
                    result.push(winner);
                }
                (Some(_), Some(local_row), None) => result.push(local_row.clone()),
                (Some(_), None, Some(remote_row)) => result.push(remote_row.clone()),
                (None, Some(local_row), None) => result.push(local_row.clone()),
                (None, None, Some(remote_row)) => result.push(remote_row.clone()),
                _ => {}
            }
        }
        result.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        result
    }

    /// AI 消息合并（按 ID 并集，最新消息全量保留）
    pub fn merge_ai_messages(
        _base: &[AiMessageRow],
        local: &[AiMessageRow],
        remote: &[AiMessageRow],
        _local_device_id: &str,
    ) -> Vec<AiMessageRow> {
        let mut map: HashMap<&str, &AiMessageRow> = HashMap::new();
        for msg in local {
            map.insert(&msg.id, msg);
        }
        for msg in remote {
            map.entry(&msg.id).or_insert(msg);
        }
        let mut result: Vec<AiMessageRow> = map.into_values().cloned().collect();
        result.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        result
    }

    /// 终端历史命令合并：按 (project_id, command, session_id) 聚合去重，累加频次并取最新时间戳
    pub fn merge_history(
        _base: &[HistoryEntry],
        local: &[HistoryEntry],
        remote: &[HistoryEntry],
    ) -> Vec<HistoryEntry> {
        let mut map: HashMap<(Option<String>, String, Option<String>), HistoryEntry> = HashMap::new();

        for e in local.iter().chain(remote.iter()) {
            let key = (e.project_id.clone(), e.command.clone(), e.session_id.clone());
            map.entry(key)
                .and_modify(|existing| {
                    existing.execution_count = existing.execution_count.saturating_add(e.execution_count);
                    if e.timestamp > existing.timestamp {
                        existing.timestamp = e.timestamp.clone();
                        existing.cwd = e.cwd.clone();
                        existing.exit_code = e.exit_code;
                        existing.duration_ms = e.duration_ms;
                    }
                })
                .or_insert_with(|| e.clone());
        }

        let mut out: Vec<HistoryEntry> = map.into_values().collect();
        out.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        out
    }

    /// 安全凭据合并：按凭据 ID 并集
    pub fn merge_credentials(
        _base: &[(CredentialMeta, String)],
        local: &[(CredentialMeta, String)],
        remote: &[(CredentialMeta, String)],
    ) -> Vec<(CredentialMeta, String)> {
        let mut map: HashMap<String, (CredentialMeta, String)> = HashMap::new();
        for item in remote {
            map.insert(item.0.id.clone(), item.clone());
        }
        for item in local {
            // 本地凭据优先覆盖远端
            map.insert(item.0.id.clone(), item.clone());
        }
        map.into_values().collect()
    }

    /// 目录 JSON map（如 `config/` 或 `themes/`）的三向合并
    pub fn merge_dir_json(
        base: Option<&BundlePart>,
        local: Option<&BundlePart>,
        remote: Option<&BundlePart>,
    ) -> Result<Option<Vec<u8>>> {
        let base_map: BTreeMap<String, String> = base
            .and_then(|p| serde_json::from_slice(&p.data).ok())
            .unwrap_or_default();
        let local_map: BTreeMap<String, String> = local
            .and_then(|p| serde_json::from_slice(&p.data).ok())
            .unwrap_or_default();
        let remote_map: BTreeMap<String, String> = remote
            .and_then(|p| serde_json::from_slice(&p.data).ok())
            .unwrap_or_default();

        if local_map.is_empty() && remote_map.is_empty() {
            return Ok(None);
        }

        let mut all_files: BTreeSet<&str> = BTreeSet::new();
        all_files.extend(base_map.keys().map(|s| s.as_str()));
        all_files.extend(local_map.keys().map(|s| s.as_str()));
        all_files.extend(remote_map.keys().map(|s| s.as_str()));

        let mut merged_map: BTreeMap<String, String> = BTreeMap::new();
        for file in all_files {
            let b_val = base_map.get(file);
            let l_val = local_map.get(file);
            let r_val = remote_map.get(file);

            match (b_val, l_val, r_val) {
                (Some(_), Some(l), Some(r)) => {
                    if l == r {
                        merged_map.insert(file.to_string(), l.clone());
                    } else if file.ends_with(".json") {
                        // 尝试针对 JSON 文件进行键值级递归三向合并
                        let merged_json = Self::merge_json_str(
                            b_val.and_then(|s| decode_b64_str(s).ok()).as_deref(),
                            decode_b64_str(l).ok().as_deref().unwrap_or_default(),
                            decode_b64_str(r).ok().as_deref().unwrap_or_default(),
                        );
                        merged_map.insert(
                            file.to_string(),
                            base64::engine::general_purpose::STANDARD.encode(merged_json),
                        );
                    } else {
                        // 非 JSON 文件：保留本地
                        merged_map.insert(file.to_string(), l.clone());
                    }
                }
                (None, Some(l), Some(r)) => {
                    if file.ends_with(".json") {
                        let merged_json = Self::merge_json_str(
                            None,
                            decode_b64_str(l).ok().as_deref().unwrap_or_default(),
                            decode_b64_str(r).ok().as_deref().unwrap_or_default(),
                        );
                        merged_map.insert(
                            file.to_string(),
                            base64::engine::general_purpose::STANDARD.encode(merged_json),
                        );
                    } else {
                        merged_map.insert(file.to_string(), l.clone());
                    }
                }
                (Some(b), Some(l), None) => {
                    // 远端删除了该文件：若本地有修改则保留
                    if b != l {
                        merged_map.insert(file.to_string(), l.clone());
                    }
                }
                (Some(b), None, Some(r)) => {
                    // 本地删除了该文件：若远端有修改则保留
                    if b != r {
                        merged_map.insert(file.to_string(), r.clone());
                    }
                }
                (None, Some(l), None) => {
                    merged_map.insert(file.to_string(), l.clone());
                }
                (None, None, Some(r)) => {
                    merged_map.insert(file.to_string(), r.clone());
                }
                _ => {}
            }
        }

        Ok(Some(serde_json::to_vec(&merged_map)?))
    }

    /// JSON 字符串的键值级合并（针对 settings.json 等配置）
    fn merge_json_str(base: Option<&str>, local: &str, remote: &str) -> String {
        let b_val: Option<serde_json::Value> =
            base.and_then(|s| serde_json::from_str(s).ok());
        let l_val: serde_json::Value =
            serde_json::from_str(local).unwrap_or(serde_json::Value::Null);
        let r_val: serde_json::Value =
            serde_json::from_str(remote).unwrap_or(serde_json::Value::Null);

        let merged = Self::merge_json_value(b_val.as_ref(), &l_val, &r_val);
        serde_json::to_string_pretty(&merged).unwrap_or_else(|_| local.to_string())
    }

    fn merge_json_value(
        base: Option<&serde_json::Value>,
        local: &serde_json::Value,
        remote: &serde_json::Value,
    ) -> serde_json::Value {
        match (local, remote) {
            (serde_json::Value::Object(l_map), serde_json::Value::Object(r_map)) => {
                let b_map = base.and_then(|b| b.as_object());
                let mut out = serde_json::Map::new();

                let mut all_keys: BTreeSet<&str> = BTreeSet::new();
                all_keys.extend(l_map.keys().map(|s| s.as_str()));
                all_keys.extend(r_map.keys().map(|s| s.as_str()));
                if let Some(bm) = b_map {
                    all_keys.extend(bm.keys().map(|s| s.as_str()));
                }

                for k in all_keys {
                    let b_child = b_map.and_then(|m| m.get(k));
                    let l_child = l_map.get(k);
                    let r_child = r_map.get(k);

                    match (b_child, l_child, r_child) {
                        (Some(_), Some(lv), Some(rv)) => {
                            if lv == rv {
                                out.insert(k.to_string(), lv.clone());
                            } else if k == "quick_commands" {
                                // 针对快捷指令数组执行树级 ID 合并
                                out.insert(k.to_string(), Self::merge_quick_commands_json(b_child, lv, rv));
                            } else {
                                out.insert(k.to_string(), Self::merge_json_value(b_child, lv, rv));
                            }
                        }
                        (None, Some(lv), Some(rv)) => {
                            if lv == rv {
                                out.insert(k.to_string(), lv.clone());
                            } else if k == "quick_commands" {
                                out.insert(k.to_string(), Self::merge_quick_commands_json(None, lv, rv));
                            } else {
                                out.insert(k.to_string(), Self::merge_json_value(None, lv, rv));
                            }
                        }
                        (Some(bv), Some(lv), None) => {
                            if bv != lv {
                                out.insert(k.to_string(), lv.clone());
                            }
                        }
                        (Some(bv), None, Some(rv)) => {
                            if bv != rv {
                                out.insert(k.to_string(), rv.clone());
                            }
                        }
                        (None, Some(lv), None) => {
                            out.insert(k.to_string(), lv.clone());
                        }
                        (None, None, Some(rv)) => {
                            out.insert(k.to_string(), rv.clone());
                        }
                        _ => {}
                    }
                }
                serde_json::Value::Object(out)
            }
            // 非对象类型发生分歧时：基于三向 Base 关系判定
            _ => {
                if let Some(bv) = base {
                    if local == bv && remote != bv {
                        // 本地未改动，远端改动 -> 采纳远端修改
                        remote.clone()
                    } else {
                        // 本地有改动（或双端均有改动发生冲突）-> 本地优先
                        local.clone()
                    }
                } else {
                    // 无 Base 场景（如新属性或新设备首连分叉）：若一方为 Null 则采纳非空方，否则本地优先
                    if local.is_null() && !remote.is_null() {
                        remote.clone()
                    } else {
                        local.clone()
                    }
                }
            }
        }
    }

    /// 快捷指令树节点的递归三向合并
    fn merge_quick_commands_json(
        _base: Option<&serde_json::Value>,
        local: &serde_json::Value,
        remote: &serde_json::Value,
    ) -> serde_json::Value {
        let l_nodes: Vec<QuickCommandNode> =
            serde_json::from_value(local.clone()).unwrap_or_default();
        let r_nodes: Vec<QuickCommandNode> =
            serde_json::from_value(remote.clone()).unwrap_or_default();

        let merged_nodes = Self::merge_qc_nodes(&l_nodes, &r_nodes);
        serde_json::to_value(&merged_nodes).unwrap_or_else(|_| local.clone())
    }

    fn merge_qc_nodes(
        local: &[QuickCommandNode],
        remote: &[QuickCommandNode],
    ) -> Vec<QuickCommandNode> {
        let mut map: HashMap<String, QuickCommandNode> = HashMap::new();

        for node in remote {
            map.insert(node.id().to_string(), node.clone());
        }

        for node in local {
            match map.get_mut(node.id()) {
                Some(existing) => {
                    // 同 ID 节点：如果是文件夹，递归合并子节点
                    if let (
                        QuickCommandNode::Folder {
                            children: r_children,
                            ..
                        },
                        QuickCommandNode::Folder {
                            children: l_children,
                            ..
                        },
                    ) = (existing, node)
                    {
                        *r_children = Self::merge_qc_nodes(l_children, r_children);
                    } else {
                        // 命令节点：本地覆盖远端
                        map.insert(node.id().to_string(), node.clone());
                    }
                }
                None => {
                    map.insert(node.id().to_string(), node.clone());
                }
            }
        }

        map.into_values().collect()
    }
}

fn decode_b64_str(b64: &str) -> Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("base64 decode")?;
    String::from_utf8(bytes).context("utf8 decode")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_causal_order() {
        // 1. Revision 优先（即使时间戳落后）
        assert_eq!(
            compare_causal_order(2, "2026-08-01T00:00:00Z", "dev-A", 1, "2026-08-02T00:00:00Z", "dev-B"),
            Ordering::Greater
        );
        // 2. Revision 相同，时间戳辅助
        assert_eq!(
            compare_causal_order(1, "2026-08-02T00:00:00Z", "dev-A", 1, "2026-08-01T00:00:00Z", "dev-B"),
            Ordering::Greater
        );
        // 3. Revision 与时间戳均相同，Device ID 字典序终裁
        assert_eq!(
            compare_causal_order(1, "2026-08-01T00:00:00Z", "dev-Z", 1, "2026-08-01T00:00:00Z", "dev-A"),
            Ordering::Greater
        );
    }

    #[test]
    fn test_merge_session_trees_both_added() {
        let base = vec![];
        let local = vec![SessionTreeRow {
            id: "s1".into(),
            kind: "session".into(),
            parent_id: None,
            sort_index: 0,
            name: "Server A".into(),
            is_collapsed: false,
            payload: Some("{}".into()),
            updated_at: "2026-08-01T00:00:00Z".into(),
            project_id: None,
            revision: 1,
            device_id: "dev-A".into(),
        }];
        let remote = vec![SessionTreeRow {
            id: "s2".into(),
            kind: "session".into(),
            parent_id: None,
            sort_index: 1,
            name: "Server B".into(),
            is_collapsed: false,
            payload: Some("{}".into()),
            updated_at: "2026-08-01T00:00:00Z".into(),
            project_id: None,
            revision: 1,
            device_id: "dev-B".into(),
        }];

        let merged = BundleMerger::merge_session_trees(&base, &local, &remote, "dev-A");
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|r| r.id == "s1"));
        assert!(merged.iter().any(|r| r.id == "s2"));
    }

    #[test]
    fn test_merge_session_trees_conflict_lww_revision() {
        let base = vec![SessionTreeRow {
            id: "s1".into(),
            kind: "session".into(),
            parent_id: None,
            sort_index: 0,
            name: "Base Name".into(),
            is_collapsed: false,
            payload: Some("{}".into()),
            updated_at: "2026-08-01T00:00:00Z".into(),
            project_id: None,
            revision: 1,
            device_id: "dev-0".into(),
        }];
        // Local modified, rev = 2
        let local = vec![SessionTreeRow {
            id: "s1".into(),
            kind: "session".into(),
            parent_id: None,
            sort_index: 0,
            name: "Local Name".into(),
            is_collapsed: false,
            payload: Some("{}".into()),
            updated_at: "2026-08-01T01:00:00Z".into(),
            project_id: None,
            revision: 2,
            device_id: "dev-A".into(),
        }];
        // Remote modified offline multiple times, rev = 5, but clock was behind
        let remote = vec![SessionTreeRow {
            id: "s1".into(),
            kind: "session".into(),
            parent_id: None,
            sort_index: 0,
            name: "Remote Name".into(),
            is_collapsed: false,
            payload: Some("{\"port\":2222}".into()),
            updated_at: "2026-08-01T00:30:00Z".into(),
            project_id: None,
            revision: 5,
            device_id: "dev-B".into(),
        }];

        let merged = BundleMerger::merge_session_trees(&base, &local, &remote, "dev-A");
        assert_eq!(merged.len(), 1);
        // Remote with higher revision wins
        assert_eq!(merged[0].name, "Remote Name");
        assert_eq!(merged[0].payload.as_deref(), Some("{\"port\":2222}"));
        assert_eq!(merged[0].revision, 6); // bumped
    }

    #[test]
    fn test_merge_history_accumulates() {
        let h1 = vec![HistoryEntry {
            id: 1,
            project_id: Some("p1".into()),
            profile_id: None,
            session_id: None,
            command: "git status".into(),
            cwd: None,
            exit_code: Some(0),
            duration_ms: Some(50),
            execution_count: 5,
            timestamp: "2026-08-01T00:00:00Z".into(),
        }];
        let h2 = vec![HistoryEntry {
            id: 2,
            project_id: Some("p1".into()),
            profile_id: None,
            session_id: None,
            command: "git status".into(),
            cwd: None,
            exit_code: Some(0),
            duration_ms: Some(40),
            execution_count: 3,
            timestamp: "2026-08-02T00:00:00Z".into(),
        }];

        let merged = BundleMerger::merge_history(&[], &h1, &h2);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].execution_count, 8);
        assert_eq!(merged[0].timestamp, "2026-08-02T00:00:00Z");
    }
}
