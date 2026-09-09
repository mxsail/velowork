//! ProfileExporter：把 Profile 业务目录（config/data/themes/sessions）与凭据
//! 组装为 `Bundle` 并封口；或从封口 `Bundle` 还原。
//!
//! 同步链路：`Repository → ProfileExporter → Bundle → SyncProvider`。
//! 本模块不感知任何同步后端（WebDAV / 云 / S3 / Git），只产出/消费 `Bundle`。
//!
//! 显式排除 `cache/`、`logs/`、`runtime/`、`locks/`、`marketplace/` 等非业务
//! 目录，使远端只承载可同步、可备份的业务单元。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use base64::Engine as _;

use anyhow::{Context, Result, bail};
use velowork_core::profiles::ProfilePaths;
use velowork_core::storage::{Database, database, migrations::CURRENT_SCHEMA_VERSION};

use crate::repositories::ai::{AiConversationRow, AiMessageRow, AiRepository};
use crate::repositories::credential::{CredentialApplicationService, CredentialMeta};
use crate::repositories::history::{HistoryEntry, HistoryRepository};
use crate::repositories::service_tree::{ServiceTreeRepository, ServiceTreeRow};
use crate::repositories::ssh_session_tree::{SessionTreeRow, SshSessionTreeRepository};
use crate::repositories::tunnel_tree::{TunnelTreeRepository, TunnelTreeRow};
use crate::repositories::workspace::WorkspaceRepository;
use crate::settings::SyncDataScope;
use crate::sync::bundle::{Bundle, BundlePart, SealedBundle};

/// Profile Layout 版本（v2 分层布局）。
const PROFILE_LAYOUT_VERSION: u32 = 2;

/// 把 Profile 业务数据导出为封口 Bundle。
pub struct ProfileExporter;

impl ProfileExporter {
    /// 收集 Profile 业务数据并封口为不可变快照（默认全量 Scope）。
    pub fn export(
        &self,
        profile: &ProfilePaths,
        cred: &CredentialApplicationService,
        db: Option<Arc<Database>>,
        passphrase: &str,
        snapshot_id: &str,
        parent_snapshot: Option<&str>,
        device_id: &str,
    ) -> Result<SealedBundle> {
        self.export_scoped(
            profile,
            cred,
            db,
            passphrase,
            snapshot_id,
            parent_snapshot,
            device_id,
            &SyncDataScope::default(),
        )
    }

    /// 根据用户配置的 `SyncDataScope` 收集 Profile 业务数据并封口为不可变快照。
    pub fn export_scoped(
        &self,
        profile: &ProfilePaths,
        cred: &CredentialApplicationService,
        db: Option<Arc<Database>>,
        passphrase: &str,
        snapshot_id: &str,
        parent_snapshot: Option<&str>,
        device_id: &str,
        scope: &SyncDataScope,
    ) -> Result<SealedBundle> {
        let mut bundle = Bundle::new(&profile.id, CURRENT_SCHEMA_VERSION, PROFILE_LAYOUT_VERSION);
        bundle.set_snapshot(
            snapshot_id.to_string(),
            parent_snapshot.map(|s| s.to_string()),
            device_id.to_string(),
        );

        // config/ 目录（settings/ai/sync/keybindings）
        if scope.settings || scope.quick_commands {
            let data = dir_to_json(&profile.config_dir()).context("export config/")?;
            let filtered_data = filter_config_for_scope(&data, scope.settings, scope.quick_commands)?;
            bundle.parts.push(BundlePart {
                name: "config".into(),
                data: filtered_data,
            });
        }
        // themes/
        if scope.themes {
            bundle.parts.push(BundlePart {
                name: "themes".into(),
                data: dir_to_json(&profile.themes_dir()).context("export themes/")?,
            });
        }
        // sessions/（仅 sshconfig/pem/import）
        if scope.sessions {
            bundle.parts.push(BundlePart {
                name: "sessions".into(),
                data: dir_to_json(&profile.sessions_dir()).context("export sessions/")?,
            });
        }

        let tree_db = db.clone().or_else(database).or_else(|| {
            if profile.database_path().exists() {
                Database::open(&profile.database_path()).ok().map(Arc::new)
            } else {
                None
            }
        });

        if let Some(tree_db) = tree_db {
            if scope.sessions {
                if let Ok(rows) = SshSessionTreeRepository::new(tree_db.clone()).export_rows() {
                    if !rows.is_empty() {
                        bundle.parts.push(BundlePart {
                            name: "session_tree".into(),
                            data: serde_json::to_vec(&rows).context("serialize session_tree")?,
                        });
                    }
                }
            }
            if scope.tunnels {
                if let Ok(rows) = TunnelTreeRepository::new(tree_db.clone()).export_rows() {
                    if !rows.is_empty() {
                        bundle.parts.push(BundlePart {
                            name: "tunnel_tree".into(),
                            data: serde_json::to_vec(&rows).context("serialize tunnel_tree")?,
                        });
                    }
                }
            }
            if scope.services {
                if let Ok(rows) = ServiceTreeRepository::new(tree_db.clone()).export_rows() {
                    if !rows.is_empty() {
                        bundle.parts.push(BundlePart {
                            name: "service_tree".into(),
                            data: serde_json::to_vec(&rows).context("serialize service_tree")?,
                        });
                    }
                }
            }
            if scope.settings {
                if let Ok(Some(ws_json)) = WorkspaceRepository::new(tree_db.clone()).export_data(WorkspaceRepository::DEFAULT_WORKSPACE_ID) {
                    bundle.parts.push(BundlePart {
                        name: "workspace".into(),
                        data: ws_json.into_bytes(),
                    });
                }
            }
            if scope.ai_chat {
                let ai_repo = AiRepository::new(tree_db.clone());
                if let Ok(convs) = ai_repo.export_conversations() {
                    if !convs.is_empty() {
                        bundle.parts.push(BundlePart {
                            name: "ai_conversations".into(),
                            data: serde_json::to_vec(&convs).context("serialize ai_conversations")?,
                        });
                    }
                }
                if let Ok(msgs) = ai_repo.export_messages() {
                    if !msgs.is_empty() {
                        bundle.parts.push(BundlePart {
                            name: "ai_messages".into(),
                            data: serde_json::to_vec(&msgs).context("serialize ai_messages")?,
                        });
                    }
                }
            }
            if scope.command_history {
                if let Ok(hist) = HistoryRepository::new(tree_db).export_rows() {
                    if !hist.is_empty() {
                        bundle.parts.push(BundlePart {
                            name: "history".into(),
                            data: serde_json::to_vec(&hist).context("serialize history")?,
                        });
                    }
                }
            }
        }

        // credentials.bin：导出 metadata + secret
        if scope.credentials {
            let creds = cred.export_credentials(&profile.id)?;
            let cred_json = serde_json::to_vec(&creds).context("serialize credentials")?;
            bundle.parts.push(BundlePart {
                name: "credentials".into(),
                data: cred_json,
            });
        }

        bundle.seal(passphrase)
    }

    /// 从封口 Bundle 还原到 Profile 目录（全量还原）。
    pub fn import(
        &self,
        sealed: &SealedBundle,
        profile: &ProfilePaths,
        cred: &CredentialApplicationService,
        passphrase: &str,
        db: Option<Arc<Database>>,
    ) -> Result<()> {
        self.import_scoped(sealed, profile, cred, passphrase, db, &SyncDataScope::default())
    }

    /// 根据用户指定的 `SyncDataScope` 从封口 Bundle 还原数据。
    pub fn import_scoped(
        &self,
        sealed: &SealedBundle,
        profile: &ProfilePaths,
        cred: &CredentialApplicationService,
        passphrase: &str,
        db: Option<Arc<Database>>,
        scope: &SyncDataScope,
    ) -> Result<()> {
        let bundle = sealed.open(passphrase)?;
        for part in &bundle.parts {
            match part.name.as_str() {
                "config" if scope.settings || scope.quick_commands => {
                    import_config_scoped(&part.data, &profile.config_dir(), scope.settings, scope.quick_commands)
                        .context("import config/")?;
                }
                "themes" if scope.themes => {
                    json_to_dir(&part.data, &profile.themes_dir()).context("import themes/")?;
                }
                "sessions" if scope.sessions => {
                    json_to_dir(&part.data, &profile.sessions_dir()).context("import sessions/")?;
                }
                "session_tree" if scope.sessions => {
                    let rows: Vec<SessionTreeRow> =
                        serde_json::from_slice(&part.data).context("parse session_tree")?;
                    let tree_db = db.clone().or_else(database).or_else(|| {
                        Database::open(&profile.database_path())
                            .ok()
                            .map(Arc::new)
                    });
                    if let Some(tree_db) = tree_db {
                        SshSessionTreeRepository::new(tree_db)
                            .import_rows(&rows)
                            .context("import session_tree")?;
                    } else {
                        bail!("无法打开目标 profile 的数据库以恢复会话树");
                    }
                }
                "tunnel_tree" if scope.tunnels => {
                    let rows: Vec<TunnelTreeRow> =
                        serde_json::from_slice(&part.data).context("parse tunnel_tree")?;
                    let tree_db = db.clone().or_else(database).or_else(|| {
                        Database::open(&profile.database_path())
                            .ok()
                            .map(Arc::new)
                    });
                    if let Some(tree_db) = tree_db {
                        TunnelTreeRepository::new(tree_db)
                            .import_rows(&rows)
                            .context("import tunnel_tree")?;
                    } else {
                        bail!("无法打开目标 profile 的数据库以恢复隧道树");
                    }
                }
                "service_tree" if scope.services => {
                    let rows: Vec<ServiceTreeRow> =
                        serde_json::from_slice(&part.data).context("parse service_tree")?;
                    let tree_db = db.clone().or_else(database).or_else(|| {
                        Database::open(&profile.database_path())
                            .ok()
                            .map(Arc::new)
                    });
                    if let Some(tree_db) = tree_db {
                        ServiceTreeRepository::new(tree_db)
                            .import_rows(&rows)
                            .context("import service_tree")?;
                    } else {
                        bail!("无法打开目标 profile 的数据库以恢复服务树");
                    }
                }
                "workspace" if scope.settings => {
                    let json_str = String::from_utf8(part.data.clone()).context("parse workspace utf8")?;
                    let tree_db = db.clone().or_else(database).or_else(|| {
                        Database::open(&profile.database_path())
                            .ok()
                            .map(Arc::new)
                    });
                    if let Some(tree_db) = tree_db {
                        WorkspaceRepository::new(tree_db)
                            .import_data(WorkspaceRepository::DEFAULT_WORKSPACE_ID, Some("Default Workspace"), &json_str)
                            .context("import workspace")?;
                    } else {
                        bail!("无法打开目标 profile 的数据库以恢复工作区");
                    }
                }
                "ai_conversations" if scope.ai_chat => {
                    let rows: Vec<AiConversationRow> =
                        serde_json::from_slice(&part.data).context("parse ai_conversations")?;
                    let tree_db = db.clone().or_else(database).or_else(|| {
                        Database::open(&profile.database_path())
                            .ok()
                            .map(Arc::new)
                    });
                    if let Some(tree_db) = tree_db {
                        AiRepository::new(tree_db)
                            .import_conversations(&rows)
                            .context("import ai_conversations")?;
                    }
                }
                "ai_messages" if scope.ai_chat => {
                    let rows: Vec<AiMessageRow> =
                        serde_json::from_slice(&part.data).context("parse ai_messages")?;
                    let tree_db = db.clone().or_else(database).or_else(|| {
                        Database::open(&profile.database_path())
                            .ok()
                            .map(Arc::new)
                    });
                    if let Some(tree_db) = tree_db {
                        AiRepository::new(tree_db)
                            .import_messages(&rows)
                            .context("import ai_messages")?;
                    }
                }
                "history" if scope.command_history => {
                    let entries: Vec<HistoryEntry> =
                        serde_json::from_slice(&part.data).context("parse history")?;
                    let tree_db = db.clone().or_else(database).or_else(|| {
                        Database::open(&profile.database_path())
                            .ok()
                            .map(Arc::new)
                    });
                    if let Some(tree_db) = tree_db {
                        HistoryRepository::new(tree_db)
                            .import_rows(&entries)
                            .context("import history")?;
                    }
                }
                "credentials" if scope.credentials => {
                    let creds: Vec<(CredentialMeta, String)> =
                        serde_json::from_slice(&part.data).context("parse credentials")?;
                    for (meta, secret) in creds {
                        let (profile_id, kind, id) = split_account(&meta.id);
                        cred.store(
                            &profile_id,
                            &kind,
                            &id,
                            &secret,
                            meta.name.as_deref(),
                        )
                        .context("import credential")?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// 把目录递归收集为 `相对路径 -> base64(内容)` 的 JSON map。
fn dir_to_json(dir: &Path) -> Result<Vec<u8>> {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    if dir.exists() {
        collect_files(dir, dir, &mut map)?;
    }
    let json = serde_json::to_vec(&map)?;
    Ok(json)
}

fn should_skip_file(rel: &str) -> bool {
    let lower = rel.to_lowercase();
    if lower == "sync_state.json" || lower.ends_with("/sync_state.json") {
        return true;
    }
    if lower.ends_with(".lock") || lower.ends_with(".tmp") || lower.ends_with(".bak") {
        return true;
    }
    false
}

fn collect_files(root: &Path, dir: &Path, map: &mut BTreeMap<String, String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, map)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .context("strip prefix")?
                .to_string_lossy()
                .replace('\\', "/");
            if should_skip_file(&rel) {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            map.insert(
                rel,
                base64::engine::general_purpose::STANDARD.encode(&bytes),
            );
        }
    }
    Ok(())
}

/// 把 `dir_to_json` 产出的 JSON map 还原为目录树（覆盖已存在文件）。
fn json_to_dir(data: &[u8], dir: &Path) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let map: BTreeMap<String, String> = serde_json::from_slice(data)?;
    std::fs::create_dir_all(dir)?;
    for (rel, b64) in map {
        if should_skip_file(&rel) {
            continue;
        }
        let path = dir.join(&rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .context("base64 decode")?;
        std::fs::write(&path, &bytes)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            let is_sessions_dir = dir.ends_with("sessions") || rel.starts_with("sessions/");
            let is_private_key = filename.ends_with(".pem")
                || filename.ends_with(".key")
                || filename.starts_with("id_")
                || is_sessions_dir;
            if is_private_key {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    Ok(())
}

/// 根据 scope 过滤 config 目录的 JSON map：
fn filter_config_for_scope(
    data: &[u8],
    include_settings: bool,
    include_quick_commands: bool,
) -> Result<Vec<u8>> {
    if include_settings && include_quick_commands {
        return Ok(data.to_vec());
    }
    let mut map: BTreeMap<String, String> = serde_json::from_slice(data)?;
    if !include_settings && include_quick_commands {
        // 仅保留 settings.json 并且其中只保留快捷指令
        let mut new_map = BTreeMap::new();
        if let Some(s_b64) = map.get("settings.json") {
            let s_bytes = base64::engine::general_purpose::STANDARD.decode(s_b64)?;
            if let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(&s_bytes) {
                if let Some(obj) = val.as_object_mut() {
                    let qc = obj.remove("quick_commands");
                    let pqc = obj.remove("project_quick_commands");
                    let mut filtered_obj = serde_json::Map::new();
                    if let Some(q) = qc {
                        filtered_obj.insert("quick_commands".to_string(), q);
                    }
                    if let Some(pq) = pqc {
                        filtered_obj.insert("project_quick_commands".to_string(), pq);
                    }
                    let filtered_bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(filtered_obj))?;
                    new_map.insert(
                        "settings.json".to_string(),
                        base64::engine::general_purpose::STANDARD.encode(filtered_bytes),
                    );
                }
            }
        }
        return Ok(serde_json::to_vec(&new_map)?);
    } else if include_settings && !include_quick_commands {
        // 保留所有文件，但 settings.json 中剔除快捷指令
        if let Some(s_b64) = map.get("settings.json") {
            let s_bytes = base64::engine::general_purpose::STANDARD.decode(s_b64)?;
            if let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(&s_bytes) {
                if let Some(obj) = val.as_object_mut() {
                    obj.remove("quick_commands");
                    obj.remove("project_quick_commands");
                    let filtered_bytes = serde_json::to_vec_pretty(&val)?;
                    map.insert(
                        "settings.json".to_string(),
                        base64::engine::general_purpose::STANDARD.encode(filtered_bytes),
                    );
                }
            }
        }
        return Ok(serde_json::to_vec(&map)?);
    }
    Ok(data.to_vec())
}

/// 根据 scope 恢复 config 目录：
fn import_config_scoped(
    data: &[u8],
    config_dir: &Path,
    include_settings: bool,
    include_quick_commands: bool,
) -> Result<()> {
    if include_settings && include_quick_commands {
        return json_to_dir(data, config_dir);
    }
    if !include_settings && include_quick_commands {
        // 仅恢复快捷指令：提取远端 settings.json 中的 quick_commands，合并写入本地 settings.json
        let map: BTreeMap<String, String> = serde_json::from_slice(data)?;
        if let Some(s_b64) = map.get("settings.json") {
            let s_bytes = base64::engine::general_purpose::STANDARD.decode(s_b64)?;
            if let Ok(remote_val) = serde_json::from_slice::<serde_json::Value>(&s_bytes) {
                let local_path = config_dir.join("settings.json");
                let mut local_val = if local_path.exists() {
                    std::fs::read_to_string(&local_path)
                        .ok()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                        .unwrap_or_else(|| serde_json::json!({}))
                } else {
                    serde_json::json!({})
                };
                if let Some(local_obj) = local_val.as_object_mut() {
                    if let Some(remote_qc) = remote_val.get("quick_commands") {
                        local_obj.insert("quick_commands".to_string(), remote_qc.clone());
                    }
                    if let Some(remote_pqc) = remote_val.get("project_quick_commands") {
                        local_obj.insert("project_quick_commands".to_string(), remote_pqc.clone());
                    }
                    std::fs::create_dir_all(config_dir)?;
                    let out = serde_json::to_string_pretty(&local_val)?;
                    std::fs::write(&local_path, out)?;
                }
            }
        }
        return Ok(());
    }
    if include_settings && !include_quick_commands {
        // 恢复偏好设置，但保留本地的快捷指令
        let local_path = config_dir.join("settings.json");
        let (local_qc, local_pqc) = if local_path.exists() {
            std::fs::read_to_string(&local_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .map(|val| {
                    (
                        val.get("quick_commands").cloned(),
                        val.get("project_quick_commands").cloned(),
                    )
                })
                .unwrap_or((None, None))
        } else {
            (None, None)
        };

        // 写入所有远端文件
        json_to_dir(data, config_dir)?;

        // 重新缝合本地 quick_commands
        if (local_qc.is_some() || local_pqc.is_some()) && local_path.exists() {
            if let Ok(s) = std::fs::read_to_string(&local_path) {
                if let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(s.as_bytes()) {
                    if let Some(obj) = val.as_object_mut() {
                        if let Some(qc) = local_qc {
                            obj.insert("quick_commands".to_string(), qc);
                        }
                        if let Some(pqc) = local_pqc {
                            obj.insert("project_quick_commands".to_string(), pqc);
                        }
                        let out = serde_json::to_string_pretty(&val)?;
                        let _ = std::fs::write(&local_path, out);
                    }
                }
            }
        }
    }
    Ok(())
}

/// 切分 keyring account `"<profile>:<kind>:<id>"` 为三元组（最多三段）。
fn split_account(account: &str) -> (String, String, String) {
    let parts: Vec<&str> = account.splitn(3, ':').collect();
    let profile = parts.first().copied().unwrap_or("").to_string();
    let kind = parts.get(1).copied().unwrap_or("").to_string();
    let id = parts.get(2).copied().unwrap_or("").to_string();
    (profile, kind, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use crate::repositories::credential::CredentialApplicationService;
    use crate::secure_storage::account_for;
    use std::sync::Arc;
    use velowork_core::storage::Database;
    use velowork_security::service::SecurityService;
    use velowork_state::{SessionTreeNode, SshSession};

    fn tmp_profile() -> (tempfile::TempDir, ProfilePaths) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("profiles").join("default");
        let p = ProfilePaths {
            id: "default".to_string(),
            root: root.clone(),
            config_root: dir.path().to_path_buf(),
        };
        std::fs::create_dir_all(p.config_dir()).unwrap();
        std::fs::create_dir_all(p.themes_dir()).unwrap();
        std::fs::create_dir_all(p.sessions_dir()).unwrap();
        std::fs::create_dir_all(p.data_dir()).unwrap();
        (dir, p)
    }

    fn cred_svc() -> CredentialApplicationService {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let security = SecurityService::new("velowork-test", "default", db.clone()).unwrap();
        CredentialApplicationService::new(db, security)
    }

    #[test]
    fn export_import_roundtrip_config_and_credentials() {
        let (_d, profile) = tmp_profile();
        // 写一个 config 文件
        let mut f = std::fs::File::create(profile.config_dir().join("settings.json")).unwrap();
        f.write_all(br#"{"font_size":16}"#).unwrap();
        // 写一个 session 文件（sshconfig）
        let mut s = std::fs::File::create(profile.sessions_dir().join("host.sshconfig")).unwrap();
        s.write_all(b"Host x\n  HostName y").unwrap();

        let cred = cred_svc();
        cred.store("default", "api_key", "openai", "sk-secret", Some("OpenAI"))
            .unwrap();

        let exporter = ProfileExporter;
        let sealed = exporter
            .export(&profile, &cred, None, "pw", "snap-test-1", None, "dev-test")
            .unwrap();

        // 导入到另一个 Profile
        let (_d2, profile2) = tmp_profile();
        let cred2 = cred_svc();
        exporter.import(&sealed, &profile2, &cred2, "pw", None).unwrap();

        let imported_settings =
            std::fs::read_to_string(profile2.config_dir().join("settings.json")).unwrap();
        assert!(imported_settings.contains("font_size"));

        let imported_session =
            std::fs::read_to_string(profile2.sessions_dir().join("host.sshconfig")).unwrap();
        assert!(imported_session.contains("HostName y"));

        // 凭据还原
        let metas = cred2.list("default").unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].id, account_for("default", "api_key", "openai"));
        assert_eq!(cred2.load_secret(&metas[0].id).unwrap(), "sk-secret");
    }

    #[test]
    fn export_wrong_passphrase_on_import_fails() {
        let (_d, profile) = tmp_profile();
        let cred = cred_svc();
        let exporter = ProfileExporter;
        let sealed = exporter
            .export(&profile, &cred, None, "right", "snap-test-1", None, "dev-test")
            .unwrap();
        let (_d2, profile2) = tmp_profile();
        let cred2 = cred_svc();
        assert!(exporter.import(&sealed, &profile2, &cred2, "wrong", None).is_err());
    }

    /// 回归：工作区数据（位于 `velowork.db` 的 `workspace` 表）必须随同步导出并在恢复时写回目标数据库。
    #[test]
    fn workspace_roundtrip() {
        use velowork_state::{ProjectData, WorkspaceData};

        let (_d, profile) = tmp_profile();
        let db = Arc::new(Database::open(&profile.database_path()).unwrap());
        let repo = WorkspaceRepository::new(db);
        let ws = WorkspaceData {
            version: 2,
            projects: vec![ProjectData {
                id: "p1".to_string(),
                name: "proj-1".to_string(),
                path: "/home/user/proj-1".to_string(),
                layout: None,
                ..Default::default()
            }],
            project_order: vec!["p1".to_string()],
            folders: Vec::new(),
            service_panel_heights: std::collections::HashMap::new(),
            main_window: Default::default(),
            extra_windows: Vec::new(),
        };
        repo.save_workspace(WorkspaceRepository::DEFAULT_WORKSPACE_ID, Some("Default Workspace"), &ws)
            .unwrap();

        let cred = cred_svc();
        let exporter = ProfileExporter;
        let sealed = exporter
            .export(
                &profile,
                &cred,
                Some(Arc::new(Database::open(&profile.database_path()).unwrap())),
                "pw",
                "snap-test-2",
                None,
                "dev-test",
            )
            .unwrap();

        let (_d2, profile2) = tmp_profile();
        let cred2 = cred_svc();
        exporter.import(&sealed, &profile2, &cred2, "pw", None).unwrap();

        let db2 = Arc::new(Database::open(&profile2.database_path()).unwrap());
        let imported = WorkspaceRepository::new(db2)
            .load_workspace(WorkspaceRepository::DEFAULT_WORKSPACE_ID)
            .unwrap()
            .unwrap();
        assert_eq!(imported.projects.len(), 1);
        assert_eq!(imported.projects[0].id, "p1");
        assert_eq!(imported.projects[0].name, "proj-1");

        // workspace.json / ssh_sessions.json 不应再被生成
        assert!(
            !profile2.root.join("workspace.json").exists(),
            "workspace.json 不应再被生成"
        );
    }

    /// 回归：SSH 会话树（位于 `velowork.db` 的 `session_tree_node` 表）必须随同步导出，
    /// 并在恢复时写回目标 profile 的数据库，且树形结构与父子关系完整保留。
    #[test]
    fn session_tree_roundtrip() {
        let (_d, profile) = tmp_profile();
        // 在 profile 的 db 中写入会话树
        let db = Arc::new(Database::open(&profile.database_path()).unwrap());
        let repo = SshSessionTreeRepository::new(db);
        let tree = vec![
            SessionTreeNode::Folder {
                id: "f1".to_string(),
                name: "Personal".to_string(),
                is_collapsed: false,
                children: vec![SessionTreeNode::Session {
                    session: SshSession {
                        id: "s1".to_string(),
                        name: "dev".to_string(),
                        host: "1.1.1.1".to_string(),
                        ..Default::default()
                    },
                }],
            },
            SessionTreeNode::Session {
                session: SshSession {
                    id: "s2".to_string(),
                    name: "top".to_string(),
                    host: "2.2.2.2".to_string(),
                    ..Default::default()
                },
            },
        ];
        repo.save_tree(&tree).unwrap();

        let cred = cred_svc();
        let exporter = ProfileExporter;
        let sealed = exporter
            .export(
                &profile,
                &cred,
                Some(Arc::new(Database::open(&profile.database_path()).unwrap())),
                "pw",
                "snap-test-3",
                None,
                "dev-test",
            )
            .unwrap();

        // 导入到另一个 profile（db 参数 None → 打开目标 profile 的 db 文件）
        let (_d2, profile2) = tmp_profile();
        let cred2 = cred_svc();
        exporter
            .import(&sealed, &profile2, &cred2, "pw", None)
            .unwrap();

        // 从目标 profile 的 db 读回并校验树形结构
        let db2 = Arc::new(Database::open(&profile2.database_path()).unwrap());
        let imported = SshSessionTreeRepository::new(db2).load_tree().unwrap();
        assert_eq!(imported.len(), 2, "应恢复 2 个根节点");
        match &imported[0] {
            SessionTreeNode::Session { session } => {
                assert_eq!(session.id, "s2");
                assert_eq!(session.parent_folder_id, None);
            }
            _ => panic!("期望第一个根节点是会话"),
        }
        match &imported[1] {
            SessionTreeNode::Folder { id, children, .. } => {
                assert_eq!(id, "f1");
                assert_eq!(children.len(), 1);
                if let SessionTreeNode::Session { session } = &children[0] {
                    assert_eq!(session.parent_folder_id.as_deref(), Some("f1"));
                } else {
                    panic!("期望文件夹下是会话节点");
                }
            }
            _ => panic!("期望第二个根节点是文件夹"),
        }
    }

    #[test]
    fn tunnel_tree_roundtrip() {
        use velowork_state::{TunnelKind, TunnelNode, TunnelProfile, ReconnectPolicy};

        let (_d, profile) = tmp_profile();
        let db = Arc::new(Database::open(&profile.database_path()).unwrap());
        let repo = TunnelTreeRepository::new(db);
        let tree = vec![
            TunnelNode::Folder {
                id: "tf1".to_string(),
                name: "Work".to_string(),
                project_id: None,
                expanded: true,
                children: vec![TunnelNode::Tunnel {
                    profile: TunnelProfile {
                        id: "t1".to_string(),
                        name: "dev-fwd".to_string(),
                        project_id: None,
                        session_id: "s1".to_string(),
                        enabled: true,
                        auto_start: false,
                        reconnect: ReconnectPolicy::InheritSession,
                        kind: TunnelKind::Dynamic {
                            local_bind: "127.0.0.1:1080".parse().unwrap(),
                        },
                        description: None,
                    },
                }],
            },
            TunnelNode::Tunnel {
                profile: TunnelProfile {
                    id: "t2".to_string(),
                    name: "root-tunnel".to_string(),
                    project_id: None,
                    session_id: "s2".to_string(),
                    enabled: false,
                    auto_start: false,
                    reconnect: ReconnectPolicy::Never,
                    kind: TunnelKind::Local {
                        local_bind: "127.0.0.1:8080".parse().unwrap(),
                        remote_target: "localhost:80".to_string(),
                    },
                    description: None,
                },
            },
        ];
        repo.save_tree(&tree).unwrap();

        let cred = cred_svc();
        let exporter = ProfileExporter;
        let sealed = exporter
            .export(
                &profile,
                &cred,
                Some(Arc::new(Database::open(&profile.database_path()).unwrap())),
                "pw",
                "snap-test-4",
                None,
                "dev-test",
            )
            .unwrap();

        let (_d2, profile2) = tmp_profile();
        let cred2 = cred_svc();
        exporter
            .import(&sealed, &profile2, &cred2, "pw", None)
            .unwrap();

        let db2 = Arc::new(Database::open(&profile2.database_path()).unwrap());
        let imported = TunnelTreeRepository::new(db2).load_tree().unwrap();
        assert_eq!(imported.len(), 2, "应恢复 2 个根节点");
        match &imported[0] {
            TunnelNode::Tunnel { profile } => {
                assert_eq!(profile.id, "t2");
            }
            _ => panic!("期望第一个根节点是隧道"),
        }
        match &imported[1] {
            TunnelNode::Folder { id, children, .. } => {
                assert_eq!(id, "tf1");
                assert_eq!(children.len(), 1);
            }
            _ => panic!("期望第二个根节点是文件夹"),
        }
    }

    #[test]
    fn service_tree_roundtrip() {
        use velowork_state::{ServiceDefinition, ServiceKind, ServiceNode};

        let (_d, profile) = tmp_profile();
        let db = Arc::new(Database::open(&profile.database_path()).unwrap());
        let repo = ServiceTreeRepository::new(db);
        let tree = vec![
            ServiceNode::Folder {
                id: "sf1".to_string(),
                name: "Web".to_string(),
                project_id: None,
                expanded: true,
                children: vec![ServiceNode::Service {
                    def: ServiceDefinition {
                        id: "svc1".to_string(),
                        name: "nginx".to_string(),
                        project_id: None,
                        kind: ServiceKind::Command,
                        session_id: Some("s1".to_string()),
                        alive_command: "systemctl is-active nginx".to_string(),
                        ..Default::default()
                    },
                }],
            },
            ServiceNode::Service {
                def: ServiceDefinition {
                    id: "svc2".to_string(),
                    name: "redis".to_string(),
                    project_id: None,
                    kind: ServiceKind::Command,
                    session_id: Some("s2".to_string()),
                    alive_command: "systemctl is-active redis".to_string(),
                    ..Default::default()
                },
            },
        ];
        repo.save_tree(&tree).unwrap();

        let cred = cred_svc();
        let exporter = ProfileExporter;
        let sealed = exporter
            .export(
                &profile,
                &cred,
                Some(Arc::new(Database::open(&profile.database_path()).unwrap())),
                "pw",
                "snap-test-5",
                None,
                "dev-test",
            )
            .unwrap();

        let (_d2, profile2) = tmp_profile();
        let cred2 = cred_svc();
        exporter
            .import(&sealed, &profile2, &cred2, "pw", None)
            .unwrap();

        let db2 = Arc::new(Database::open(&profile2.database_path()).unwrap());
        let imported = ServiceTreeRepository::new(db2).load_tree().unwrap();
        assert_eq!(imported.len(), 2, "应恢复 2 个根节点");
        match &imported[0] {
            ServiceNode::Folder { id, children, .. } => {
                assert_eq!(id, "sf1");
                assert_eq!(children.len(), 1);
            }
            _ => panic!("期望第一个根节点是文件夹"),
        }
        match &imported[1] {
            ServiceNode::Service { def } => {
                assert_eq!(def.id, "svc2");
            }
            _ => panic!("期望第二个根节点是服务"),
        }
    }

    #[test]
    fn test_sync_state_and_private_keys_exclusion_and_permissions() {
        let (_d, profile) = tmp_profile();
        let cred = cred_svc();

        // 写入正常配置、私有同步状态文件以及临时文件
        std::fs::write(profile.config_dir().join("settings.json"), b"{\"font_size\": 14}").unwrap();
        std::fs::write(profile.config_dir().join("sync_state.json"), b"{\"device_id\": \"dev-x\"}").unwrap();
        std::fs::write(profile.config_dir().join("test.lock"), b"lock").unwrap();
        std::fs::write(profile.config_dir().join("test.tmp"), b"tmp").unwrap();

        let exporter = ProfileExporter;
        let sealed = exporter
            .export(
                &profile,
                &cred,
                None,
                "pw",
                "snap-test-exclude",
                None,
                "dev-test",
            )
            .unwrap();

        let opened = sealed.open("pw").unwrap();
        let config_part = opened.parts.iter().find(|f| f.name == "config").unwrap();
        let map: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&config_part.data).unwrap();
        assert!(map.contains_key("settings.json"));
        assert!(!map.contains_key("sync_state.json"), "sync_state.json 绝不能打包进 Bundle");
        assert!(!map.contains_key("test.lock"), "lock 文件必须被排除");
        assert!(!map.contains_key("test.tmp"), "tmp 文件必须被排除");

        // 导入时验证
        let (_d2, profile2) = tmp_profile();
        let cred2 = cred_svc();
        exporter.import(&sealed, &profile2, &cred2, "pw", None).unwrap();
        assert!(profile2.config_dir().join("settings.json").exists());
        assert!(!profile2.config_dir().join("sync_state.json").exists());
    }

    #[test]
    fn test_scoped_export_and_import_quick_commands_vs_settings() {
        use base64::Engine;
        let (_d, profile) = tmp_profile();
        let cred = cred_svc();

        // settings.json 同时包含偏好与快捷指令
        let initial_json = serde_json::json!({
            "font_size": 16,
            "quick_commands": [
                { "id": "cmd1", "label": "List", "command": "ls -la" }
            ],
            "project_quick_commands": {
                "proj1": [{ "id": "cmd2", "label": "Build", "command": "cargo build" }]
            }
        });
        std::fs::write(
            profile.config_dir().join("settings.json"),
            serde_json::to_vec(&initial_json).unwrap(),
        )
        .unwrap();

        let exporter = ProfileExporter;

        // 1. 仅导出 quick_commands
        let mut qc_scope = SyncDataScope::default();
        qc_scope.settings = false;
        qc_scope.quick_commands = true;

        let sealed_qc = exporter
            .export_scoped(
                &profile,
                &cred,
                None,
                "pw",
                "snap-qc",
                None,
                "dev-qc",
                &qc_scope,
            )
            .unwrap();

        let opened_qc = sealed_qc.open("pw").unwrap();
        let config_part = opened_qc.parts.iter().find(|f| f.name == "config").unwrap();
        let map: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&config_part.data).unwrap();
        assert!(map.contains_key("settings.json"));
        let s_b64 = map.get("settings.json").unwrap();
        let s_bytes = base64::engine::general_purpose::STANDARD.decode(s_b64).unwrap();
        let parsed_qc: serde_json::Value = serde_json::from_slice(&s_bytes).unwrap();
        assert!(parsed_qc.get("font_size").is_none(), "仅导出快捷指令时不能包含通用偏好");
        assert!(parsed_qc.get("quick_commands").is_some(), "必须包含 quick_commands");

        // 2. 目标机器原本有自己的 font_size: 20
        let (_d2, profile2) = tmp_profile();
        let cred2 = cred_svc();
        let target_initial = serde_json::json!({
            "font_size": 20
        });
        std::fs::write(
            profile2.config_dir().join("settings.json"),
            serde_json::to_vec(&target_initial).unwrap(),
        )
        .unwrap();

        // 导入快捷指令包，验证本地 font_size 不会被冲掉
        exporter.import_scoped(&sealed_qc, &profile2, &cred2, "pw", None, &qc_scope).unwrap();
        let imported_bytes = std::fs::read(profile2.config_dir().join("settings.json")).unwrap();
        let imported_json: serde_json::Value = serde_json::from_slice(&imported_bytes).unwrap();
        assert_eq!(imported_json["font_size"], 20, "仅导入快捷指令时原有偏好必须完好保留");
        assert_eq!(imported_json["quick_commands"][0]["id"], "cmd1");
    }
}
