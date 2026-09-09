use gpui::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use velowork_core::storage::database;
use velowork_state::{
    collect_tunnel_profiles, TunnelId, TunnelNode, TunnelProfile, TunnelRuntimeInfo,
};

use crate::repositories::tunnel_tree::TunnelTreeRepository;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TunnelEvent {
    TunnelAdded(TunnelId),
    TunnelUpdated(TunnelId),
    TunnelRemoved(TunnelId),
    TunnelRuntimeChanged(TunnelId),
}

impl EventEmitter<TunnelEvent> for TunnelStore {}

pub struct TunnelStore {
    nodes: Vec<TunnelNode>,
    runtime_info: HashMap<TunnelId, TunnelRuntimeInfo>,
    save_pending: Arc<AtomicBool>,
}

pub struct GlobalTunnelStore(pub Entity<TunnelStore>);
impl Global for GlobalTunnelStore {}

impl TunnelStore {
    /// Load the persisted tunnel tree.
    ///
    /// 优先从 `velowork.db` 的 `tunnel_tree_node` 表加载（单库方案）。
    /// 若数据库不可用（测试 / 未初始化场景），则返回空树。
    pub fn new() -> Self {
        let nodes = if let Some(db) = database() {
            let repo = TunnelTreeRepository::new(db);
            repo.load_tree().unwrap_or_default()
        } else {
            Vec::new()
        };
        Self {
            nodes,
            runtime_info: HashMap::new(),
            save_pending: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 从数据库重新加载隧道树并通知 UI 刷新（在 WebDAV 恢复后调用）
    pub fn reload_from_disk(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(db) = database() {
            let repo = TunnelTreeRepository::new(db);
            if let Ok(tree) = repo.load_tree() {
                self.nodes = tree;
            }
        }
        cx.notify();
    }

    pub fn nodes(&self) -> &[TunnelNode] {
        &self.nodes
    }

    pub fn find_tunnel(&self, id: &str) -> Option<&TunnelProfile> {
        crate::tunnels::tunnel_find_node_ref(&self.nodes, id).and_then(|n| n.profile())
    }

    pub fn get_runtime_info(&self, id: &str) -> TunnelRuntimeInfo {
        self.runtime_info.get(id).cloned().unwrap_or_default()
    }

    /// Add a tunnel profile under `parent` (or at the root when `parent` is `None`).
    pub fn add_tunnel_to(
        &mut self,
        profile: TunnelProfile,
        parent: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let id = profile.id.clone();
        let node = TunnelNode::Tunnel { profile };
        crate::tunnels::tunnel_insert_node(&mut self.nodes, parent, node);
        self.persist(cx);
        cx.emit(TunnelEvent::TunnelAdded(id));
        cx.notify();
    }

    pub fn update_tunnel(
        &mut self,
        profile: TunnelProfile,
        new_parent: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let id = profile.id.clone();

        // Check if the tunnel needs to move to a different folder.
        let current_parent = crate::tunnels::tunnel_parent_id_of(&self.nodes, &id).flatten();
        let target_parent = new_parent.map(|s| s.to_string());
        if current_parent != target_parent {
            // Remove from old location, re-insert under the new parent.
            if let Some(mut node) = crate::tunnels::tunnel_remove_node(&mut self.nodes, &id) {
                if let TunnelNode::Tunnel { profile: p } = &mut node {
                    *p = profile;
                }
                crate::tunnels::tunnel_insert_node(&mut self.nodes, new_parent, node);
            }
        } else if let Some(node) = crate::tunnels::tunnel_find_node_mut(&mut self.nodes, &id) {
            if let TunnelNode::Tunnel { profile: p } = node {
                *p = profile;
            }
        } else {
            return;
        }

        self.persist(cx);
        cx.emit(TunnelEvent::TunnelUpdated(id));
        cx.notify();
    }

    /// Create a new folder under `parent` (or at root) with optional project_id, returning its id.
    pub fn add_folder_for_project(
        &mut self,
        name: &str,
        parent: Option<&str>,
        project_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> String {
        let id = crate::tunnels::new_tunnel_id();
        let folder = TunnelNode::Folder {
            id: id.clone(),
            name: name.to_string(),
            project_id: project_id.map(|s| s.to_string()),
            expanded: true,
            children: Vec::new(),
        };
        crate::tunnels::tunnel_insert_node(&mut self.nodes, parent, folder);
        self.persist(cx);
        cx.emit(TunnelEvent::TunnelAdded(id.clone()));
        cx.notify();
        id
    }

    /// Create a new folder under `parent` (or at root), returning its id.
    pub fn add_folder(
        &mut self,
        name: &str,
        parent: Option<&str>,
        cx: &mut Context<Self>,
    ) -> String {
        self.add_folder_for_project(name, parent, None, cx)
    }

    /// Recursively ensures a multi-level folder path exists, returning the leaf folder ID.
    pub fn ensure_folder_path_for_project(
        &mut self,
        segments: &[String],
        project_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let result = crate::tunnels::ensure_tunnel_folder_path(&mut self.nodes, segments, project_id);
        if let Some(ref id) = result {
            self.persist(cx);
            cx.emit(TunnelEvent::TunnelAdded(id.clone()));
            cx.notify();
        }
        result
    }

    /// Set a folder's expanded/collapsed state.
    pub fn set_folder_expanded(
        &mut self,
        id: &str,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(node) = crate::tunnels::tunnel_find_node_mut(&mut self.nodes, id) {
            if let TunnelNode::Folder { expanded: e, .. } = node {
                *e = expanded;
            }
            self.persist(cx);
            cx.emit(TunnelEvent::TunnelUpdated(id.to_string()));
            cx.notify();
        }
    }

    /// Move a node to a new parent at a given index (keeps identity + subtree).
    pub fn move_node(
        &mut self,
        id: &str,
        new_parent: Option<&str>,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        crate::tunnels::tunnel_move_node(&mut self.nodes, id, new_parent, index);
        self.persist(cx);
        cx.emit(TunnelEvent::TunnelUpdated(id.to_string()));
        cx.notify();
    }

    /// Remove a node by id, returning the ids of every tunnel it contained
    /// (so the caller can stop the running engines). Recurses into folders.
    pub fn remove_node(&mut self, id: &str, cx: &mut Context<Self>) -> Vec<TunnelId> {
        let mut removed_ids = Vec::new();
        if let Some(node) = crate::tunnels::tunnel_remove_node(&mut self.nodes, id) {
            let mut profiles = Vec::new();
            collect_tunnel_profiles(&node, &mut profiles);
            for p in profiles {
                self.runtime_info.remove(&p.id);
                removed_ids.push(p.id);
            }
            self.persist(cx);
            cx.emit(TunnelEvent::TunnelRemoved(id.to_string()));
            cx.notify();
        }
        removed_ids
    }

    pub fn rename_node(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        if let Some(node) = crate::tunnels::tunnel_find_node_mut(&mut self.nodes, id) {
            match node {
                TunnelNode::Folder { name: n, .. } => *n = name.to_string(),
                TunnelNode::Tunnel { profile } => profile.name = name.to_string(),
            }
            self.persist(cx);
            cx.emit(TunnelEvent::TunnelUpdated(id.to_string()));
            cx.notify();
        }
    }

    pub fn update_runtime_info(
        &mut self,
        id: &str,
        info: TunnelRuntimeInfo,
        cx: &mut Context<Self>,
    ) {
        self.runtime_info.insert(id.to_string(), info);
        cx.emit(TunnelEvent::TunnelRuntimeChanged(id.to_string()));
        cx.notify();
    }

    /// Persist the in-memory tree to SQLite via `TunnelTreeRepository`.
    /// Uses debounced async offload to avoid blocking the GPUI main thread.
    fn persist(&mut self, cx: &mut Context<Self>) {
        self.save_pending.store(true, Ordering::Relaxed);
        let save_pending = self.save_pending.clone();

        cx.spawn(async move |this, cx| {
            smol::Timer::after(std::time::Duration::from_millis(300)).await;

            if save_pending.swap(false, Ordering::Relaxed) {
                let nodes = cx.update(|cx| {
                    this.upgrade().map(|e| e.read(cx).nodes.clone())
                });
                if let Some(nodes) = nodes {
                    let result = smol::unblock(move || {
                        if let Some(db) = database() {
                            TunnelTreeRepository::new(db).save_tree(&nodes)
                        } else {
                            log::warn!("[tunnel_store] Database not available; tunnel tree not persisted");
                            Ok(())
                        }
                    }).await;
                    if let Err(e) = result {
                        log::error!("[tunnel_store] Failed to persist tunnel tree to database | error: {:#}", e);
                    } else {
                        crate::sync::notify_config_changed();
                    }
                }
            }
        }).detach();
    }

    /// Synchronously flush any pending tunnel tree save (called on quit).
    pub fn flush_pending_save(&self) {
        if self.save_pending.swap(false, Ordering::Relaxed) {
            if let Some(db) = database() {
                if let Err(e) = TunnelTreeRepository::new(db).save_tree(&self.nodes) {
                    log::error!("[tunnel_store] Failed to flush tunnel tree on quit | error: {:#}", e);
                } else {
                    crate::sync::notify_config_changed();
                }
            }
        }
    }
}
