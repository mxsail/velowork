use crate::services::*;
use gpui::*;
use velowork_core::storage::database;
use velowork_state::{ServiceDefinition, ServiceId, ServiceNode, ServiceTree};

use crate::repositories::service_tree::ServiceTreeRepository;

/// 服务监控配置变更事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceEvent {
    ServiceAdded(ServiceId),
    ServiceUpdated(ServiceId),
    ServiceRemoved(ServiceId),
    TreeChanged,
}

impl EventEmitter<ServiceEvent> for ServiceStore {}

/// 服务监控配置 Store（绑定当前 Profile，支持树结构，持久化到 velowork.db 中的 service_tree_node 表）。
pub struct ServiceStore {
    nodes: ServiceTree,
}

pub struct GlobalServiceStore(pub Entity<ServiceStore>);
impl Global for GlobalServiceStore {}

impl ServiceStore {
    pub fn new() -> Self {
        let nodes = if let Some(db) = database() {
            let repo = ServiceTreeRepository::new(db);
            repo.load_tree().unwrap_or_default()
        } else {
            Vec::new()
        };
        Self { nodes }
    }

    /// 从数据库重新加载服务树并通知 UI 刷新（在 WebDAV 恢复后调用）
    pub fn reload_from_disk(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(db) = database() {
            let repo = ServiceTreeRepository::new(db);
            if let Ok(tree) = repo.load_tree() {
                self.nodes = tree;
            }
        }
        cx.emit(ServiceEvent::TreeChanged);
        cx.notify();
    }

    pub fn nodes(&self) -> &[ServiceNode] {
        &self.nodes
    }

    pub fn all_services(&self) -> Vec<ServiceDefinition> {
        let mut defs = Vec::new();
        collect_all_service_definitions(&self.nodes, &mut defs);
        defs
    }

    pub fn services(&self) -> Vec<ServiceDefinition> {
        self.all_services()
    }

    pub fn monitored_for_session(&self, session_id: &str) -> Vec<ServiceDefinition> {
        self.all_services()
            .into_iter()
            .filter(|s| s.monitor_enabled && s.session_id.as_deref() == Some(session_id))
            .collect()
    }

    pub fn find(&self, id: &str) -> Option<ServiceDefinition> {
        service_find_node_ref(&self.nodes, id).and_then(|n| n.def().cloned())
    }

    pub fn upsert(&mut self, def: ServiceDefinition, cx: &mut Context<Self>) {
        let id = def.id.clone();
        if let Some(existing) = service_find_node_mut(&mut self.nodes, &id) {
            if let ServiceNode::Service { def: d } = existing {
                *d = def;
                self.persist();
                cx.emit(ServiceEvent::ServiceUpdated(id));
                cx.notify();
                return;
            }
        }
        service_insert_node(&mut self.nodes, None, ServiceNode::Service { def });
        self.persist();
        cx.emit(ServiceEvent::ServiceAdded(id));
        cx.notify();
    }

    pub fn add_folder_for_project(
        &mut self,
        parent_id: Option<&str>,
        name: &str,
        project_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> String {
        let fid = new_service_folder_id();
        let folder = ServiceNode::Folder {
            id: fid.clone(),
            name: name.to_string(),
            project_id: project_id.map(|s| s.to_string()),
            expanded: true,
            children: Vec::new(),
        };
        service_insert_node(&mut self.nodes, parent_id, folder);
        self.persist();
        cx.emit(ServiceEvent::TreeChanged);
        cx.notify();
        fid
    }

    pub fn add_folder(
        &mut self,
        parent_id: Option<&str>,
        name: &str,
        cx: &mut Context<Self>,
    ) -> String {
        self.add_folder_for_project(parent_id, name, None, cx)
    }

    /// Recursively ensures a multi-level folder path exists, returning the leaf folder ID.
    pub fn ensure_folder_path_for_project(
        &mut self,
        segments: &[String],
        project_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let result = crate::services::ensure_service_folder_path(&mut self.nodes, segments, project_id);
        if result.is_some() {
            self.persist();
            cx.emit(ServiceEvent::TreeChanged);
            cx.notify();
        }
        result
    }

    pub fn rename_node(&mut self, id: &str, new_name: &str, cx: &mut Context<Self>) -> bool {
        if service_rename_node(&mut self.nodes, id, new_name) {
            self.persist();
            cx.emit(ServiceEvent::TreeChanged);
            cx.notify();
            true
        } else {
            false
        }
    }

    pub fn remove(&mut self, id: &str, cx: &mut Context<Self>) {
        if service_remove_node(&mut self.nodes, id).is_some() {
            self.persist();
            cx.emit(ServiceEvent::ServiceRemoved(id.to_string()));
            cx.notify();
        }
    }

    pub fn remove_nodes(&mut self, ids: &[String], cx: &mut Context<Self>) {
        for id in ids {
            service_remove_node(&mut self.nodes, id);
        }
        self.persist();
        cx.emit(ServiceEvent::TreeChanged);
        cx.notify();
    }

    pub fn move_node(
        &mut self,
        source_id: &str,
        target_id: Option<&str>,
        as_child: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if service_move_node(&mut self.nodes, source_id, target_id, as_child) {
            self.persist();
            cx.emit(ServiceEvent::TreeChanged);
            cx.notify();
            true
        } else {
            false
        }
    }

    pub fn toggle_folder(&mut self, id: &str, cx: &mut Context<Self>) {
        service_toggle_folder(&mut self.nodes, id);
        self.persist();
        cx.notify();
    }

    fn persist(&self) {
        if let Some(db) = database() {
            if let Err(e) = ServiceTreeRepository::new(db).save_tree(&self.nodes) {
                log::error!("[service_store] Failed to save service tree | error: {:#}", e);
            } else {
                crate::sync::notify_config_changed();
            }
        } else {
            log::warn!("[service_store] Database not available; service tree not persisted");
        }
    }
}
