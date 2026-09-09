//! `SessionStore` — the single-source owner of all SSH session / connection-tree data.
//!
//! Previously the SSH tree lived as a plain field on the `Workspace` god-object
//! and was mutated through a half-dozen `Workspace` methods that all ended in a
//! blind `cx.notify()`. That broadcast a change to EVERY component observing
//! `Workspace`, and offered no way to know *what* changed — the root cause of
//! "SessionPanel edits cascade into MainView/Editor/Terminal re-renders" and of
//! untraceable updates.
//!
//! `SessionStore` fixes both:
//! - It is the ONLY type that may mutate `SshSessionConfig`. Every edit goes
//!   through one of its methods; direct tree mutation elsewhere is gone.
//! - Each mutator emits a typed `SessionEvent` so consumers subscribe to the
//!   exact change they care about (a renamed node, an added session, …)
//!   instead of re-rendering on any Workspace noise.

use gpui::*;
use velowork_core::storage::database;
use velowork_state::{SshSession, SshSessionConfig, SessionTreeNode};
use crate::repositories::ssh_session_tree::SshSessionTreeRepository;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Canonical, single-source owner of the SSH session tree.
pub struct SessionStore {
    config: SshSessionConfig,
    save_pending: Arc<AtomicBool>,
}

/// Fine-grained change events emitted by [`SessionStore`].
///
/// Consumers subscribe to these (instead of `observe(&workspace)`) so only the
/// components that actually care about a specific change re-render.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// A folder or session node was added.
    NodeAdded,
    /// A session's data was replaced (carries the session id).
    SessionUpdated(String),
    /// A node (folder or session) was removed.
    NodeRemoved,
    /// A folder's collapsed state toggled (carries the folder id).
    FolderToggled(String),
    /// Every folder's collapsed state changed at once (carries the new value).
    AllFoldersCollapsed(bool),
    /// A node was renamed (carries the node id).
    NodeRenamed(String),
    /// The whole tree was replaced.
    TreeReplaced,
}

impl EventEmitter<SessionEvent> for SessionStore {}

impl SessionStore {
    /// Load the persisted SSH tree from `velowork.db` `session_tree_node` table.
    pub fn new() -> Self {
        if let Some(db) = database() {
            let repo = SshSessionTreeRepository::new(db);
            if let Ok(config) = repo.load_config() {
                return Self { config, save_pending: Arc::new(AtomicBool::new(false)) };
            }
            log::warn!("[session] 从数据库加载会话树失败，回退空树");
        }
        Self {
            config: SshSessionConfig::default(),
            save_pending: Arc::new(AtomicBool::new(false)),
        }
    }

    // ---- queries (UI reads ONLY through these) ----

    /// Immutable view of the session tree for default or legacy project.
    pub fn tree(&self) -> &[SessionTreeNode] {
        self.tree_for_project(None)
    }

    /// Immutable view of the session tree for a specific project.
    pub fn tree_for_project(&self, project_id: Option<&str>) -> &[SessionTreeNode] {
        self.config.tree_for_project(project_id)
    }

    /// Look up a session by id (used by terminal tabs to resolve a name).
    pub fn find_session(&self, id: &str) -> Option<&SshSession> {
        self.config.find_session(id)
    }

    /// All sessions across every project (deduplicated by id).
    pub fn all_sessions(&self) -> Vec<(String, String)> {
        self.config
            .all_sessions()
            .into_iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect()
    }

    /// All SSH protocol sessions across every project (deduplicated by id).
    pub fn all_ssh_sessions(&self) -> Vec<(String, String)> {
        self.config
            .all_ssh_sessions()
            .into_iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect()
    }

    /// Find the parent folder id of a node in the tree.
    pub fn find_parent_id(nodes: &[SessionTreeNode], child_id: &str) -> Option<String> {
        crate::ssh_sessions::find_parent_folder_id_in_tree(nodes, child_id)
    }

    /// Search for a session by id within a given tree slice.
    fn find_session_in_nodes<'a>(nodes: &'a [SessionTreeNode], id: &str) -> Option<&'a SshSession> {
        for node in nodes {
            match node {
                SessionTreeNode::Session { session } => {
                    if session.id == id {
                        return Some(session);
                    }
                }
                SessionTreeNode::Folder { children, .. } => {
                    if let Some(s) = Self::find_session_in_nodes(children, id) {
                        return Some(s);
                    }
                }
            }
        }
        None
    }

    pub fn folder_name_exists_in_parent(
        nodes: &[SessionTreeNode],
        parent_id: Option<&str>,
        target_name: &str,
        except_id: Option<&str>,
    ) -> bool {
        let name_trimmed = target_name.trim();
        if name_trimmed.is_empty() {
            return false;
        }

        let siblings: Option<&[SessionTreeNode]> = if let Some(pid) = parent_id {
            Self::find_folder_children(nodes, pid)
        } else {
            Some(nodes)
        };

        if let Some(siblings) = siblings {
            siblings.iter().any(|node| {
                if let SessionTreeNode::Folder { id, name, .. } = node {
                    if let Some(eid) = except_id {
                        if id == eid {
                            return false;
                        }
                    }
                    name.trim() == name_trimmed
                } else {
                    false
                }
            })
        } else {
            false
        }
    }

    fn find_folder_children<'a>(
        nodes: &'a [SessionTreeNode],
        target_id: &str,
    ) -> Option<&'a [SessionTreeNode]> {
        for node in nodes {
            if let SessionTreeNode::Folder { id, children, .. } = node {
                if id == target_id {
                    return Some(children);
                }
                if let Some(found) = Self::find_folder_children(children, target_id) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// 校验同一目录下（含全部节点类型：目录、会话、渠道）是否已存在同名节点。
    /// 用于叶子节点（会话/渠道）的新建、编辑与重命名校验；`except_id` 用于
    /// 排除节点自身（重命名/编辑场景）。不同目录之间允许重名。
    pub fn node_name_exists_in_parent(
        nodes: &[SessionTreeNode],
        parent_id: Option<&str>,
        target_name: &str,
        except_id: Option<&str>,
    ) -> bool {
        let name_trimmed = target_name.trim();
        if name_trimmed.is_empty() {
            return false;
        }

        let siblings: Option<&[SessionTreeNode]> = if let Some(pid) = parent_id {
            Self::find_folder_children(nodes, pid)
        } else {
            Some(nodes)
        };

        if let Some(siblings) = siblings {
            siblings.iter().any(|node| {
                let (id, name) = match node {
                    SessionTreeNode::Folder { id, name, .. } => (id.as_str(), name.as_str()),
                    SessionTreeNode::Session { session } => {
                        (session.id.as_str(), session.name.as_str())
                    }
                };
                if let Some(eid) = except_id {
                    if id == eid {
                        return false;
                    }
                }
                name.trim() == name_trimmed
            })
        } else {
            false
        }
    }

    // ---- mutations (the ONLY writers) ----

    pub fn add_folder(&mut self, parent_id: Option<&str>, name: String, cx: &mut Context<Self>) -> Option<String> {
        self.add_folder_for_project(None, parent_id, name, cx)
    }

    pub fn add_folder_for_project(
        &mut self,
        project_id: Option<&str>,
        parent_id: Option<&str>,
        name: String,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let name_trimmed = name.trim().to_string();
        let tree_ref = self.config.tree_for_project(project_id);
        if Self::folder_name_exists_in_parent(tree_ref, parent_id, &name_trimmed, None) {
            log::warn!("[session] 相同层级下已存在同名文件夹: {}", name_trimmed);
            return None;
        }
        let folder_id = uuid::Uuid::new_v4().to_string();
        let folder = SessionTreeNode::Folder {
            id: folder_id.clone(),
            name: name_trimmed,
            children: Vec::new(),
            is_collapsed: false,
        };
        let tree = self.config.tree_for_project_mut(project_id);
        crate::ssh_sessions::add_node_to_tree(tree, parent_id, folder);
        self.persist_and_emit(SessionEvent::NodeAdded, cx);
        Some(folder_id)
    }

    /// Recursively ensures a multi-level folder path exists for a project, returning the leaf folder ID.
    pub fn ensure_folder_path_for_project(
        &mut self,
        project_id: Option<&str>,
        segments: &[String],
        cx: &mut Context<Self>,
    ) -> Option<String> {
        if segments.is_empty() {
            return None;
        }

        let mut current_parent_id: Option<String> = None;

        for seg in segments {
            let seg_trimmed = seg.trim();
            if seg_trimmed.is_empty() {
                continue;
            }

            let tree_ref = self.config.tree_for_project(project_id);
            let existing_id = match current_parent_id.as_deref() {
                Some(pid) => {
                    if let Some(SessionTreeNode::Folder { children, .. }) =
                        crate::ssh_sessions::find_node_in_tree(tree_ref, pid)
                    {
                        children.iter().find_map(|child| {
                            if let SessionTreeNode::Folder { id, name, .. } = child {
                                if name == seg_trimmed {
                                    return Some(id.clone());
                                }
                            }
                            None
                        })
                    } else {
                        None
                    }
                }
                None => tree_ref.iter().find_map(|node| {
                    if let SessionTreeNode::Folder { id, name, .. } = node {
                        if name == seg_trimmed {
                            return Some(id.clone());
                        }
                    }
                    None
                }),
            };

            if let Some(found_id) = existing_id {
                current_parent_id = Some(found_id);
            } else {
                let folder_id = self.add_folder_for_project(
                    project_id,
                    current_parent_id.as_deref(),
                    seg_trimmed.to_string(),
                    cx,
                )?;
                current_parent_id = Some(folder_id);
            }
        }

        current_parent_id
    }

    pub fn add_session(&mut self, parent_id: Option<&str>, session: SshSession, cx: &mut Context<Self>) {
        self.add_session_for_project(None, parent_id, session, cx);
    }

    pub fn add_session_for_project(
        &mut self,
        project_id: Option<&str>,
        parent_id: Option<&str>,
        session: SshSession,
        cx: &mut Context<Self>,
    ) {
        let node = SessionTreeNode::Session { session };
        let tree = self.config.tree_for_project_mut(project_id);
        crate::ssh_sessions::add_node_to_tree(tree, parent_id, node);
        self.persist_and_emit(SessionEvent::NodeAdded, cx);
    }

    /// Duplicate a session: create a copy with a new ID and name suffix.
    /// The copy is inserted right after the original in the same parent folder.
    /// Returns the ID of the newly created session.
    pub fn duplicate_session(&mut self, session_id: &str, cx: &mut Context<Self>) -> Option<String> {
        self.duplicate_session_for_project(None, session_id, cx)
    }

    pub fn duplicate_session_for_project(
        &mut self,
        project_id: Option<&str>,
        session_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let tree = self.config.tree_for_project(project_id);
        let original = Self::find_session_in_nodes(tree, session_id)?.clone();
        let parent_id = crate::ssh_sessions::find_parent_folder_id_in_tree(tree, session_id);

        // Create a new session with a new ID and a non-colliding duplicate name.
        // The first copy is named "X - 复制"; if that name already exists among
        // the siblings, subsequent copies append a numeric suffix ("X - 复制 2",
        // "X - 复制 3", ...) so copying the same session repeatedly never
        // produces two siblings with the same name.
        let new_id = uuid::Uuid::new_v4().to_string();
        let new_name = {
            let existing =
                crate::ssh_sessions::sibling_session_names(tree, parent_id.as_deref());
            let mut cand = format!("{} - 复制", original.name);
            if existing.iter().any(|n| n == &cand) {
                let mut n = 2u32;
                loop {
                    let next = format!("{} - 复制 {}", original.name, n);
                    if !existing.iter().any(|e| e == &next) {
                        cand = next;
                        break;
                    }
                    n += 1;
                }
            }
            cand
        };
        let mut new_session = original;
        new_session.id = new_id.clone();
        new_session.name = new_name;

        // Find the index of the original session in its parent
        let insert_index = if let Some(ref pid) = parent_id {
            crate::ssh_sessions::find_node_index_in_parent(tree, session_id, pid)
                .map(|i| i + 1)
                .unwrap_or(0)
        } else {
            // Root level: find index in root
            tree.iter()
                .position(|n| n.id() == session_id)
                .map(|i| i + 1)
                .unwrap_or(0)
        };

        let node = SessionTreeNode::Session { session: new_session };
        let tree_mut = self.config.tree_for_project_mut(project_id);
        crate::ssh_sessions::insert_node_at_index(
            tree_mut,
            parent_id.as_deref(),
            insert_index,
            node,
        );
        self.persist_and_emit(SessionEvent::NodeAdded, cx);
        Some(new_id)
    }

    pub fn update_session(&mut self, session: SshSession, cx: &mut Context<Self>) {
        self.update_session_for_project(None, session, cx);
    }

    pub fn update_session_for_project(
        &mut self,
        project_id: Option<&str>,
        session: SshSession,
        cx: &mut Context<Self>,
    ) {
        let id = session.id.clone();
        let tree = self.config.tree_for_project_mut(project_id);
        let current_parent = crate::ssh_sessions::find_parent_folder_id_in_tree(tree, &session.id);
        let target_parent = session.parent_folder_id.clone();

        crate::ssh_sessions::update_session_in_tree(tree, session.clone());

        // If the parent folder changed, re-home the node under the new parent.
        if current_parent != target_parent {
            crate::ssh_sessions::remove_node_from_tree(tree, &session.id);
            let parent_id_ref = target_parent.as_deref();
            let new_node = SessionTreeNode::Session { session };
            let tree = self.config.tree_for_project_mut(project_id);
            crate::ssh_sessions::add_node_to_tree(tree, parent_id_ref, new_node);
        }

        self.persist_and_emit(SessionEvent::SessionUpdated(id), cx);
    }

    pub fn toggle_folder_collapsed(&mut self, folder_id: &str, cx: &mut Context<Self>) {
        self.toggle_folder_collapsed_for_project(None, folder_id, cx);
    }

    pub fn toggle_folder_collapsed_for_project(
        &mut self,
        project_id: Option<&str>,
        folder_id: &str,
        cx: &mut Context<Self>,
    ) {
        let tree = self.config.tree_for_project_mut(project_id);
        crate::ssh_sessions::toggle_folder_collapsed_in_tree(tree, folder_id);
        self.persist_and_emit(SessionEvent::FolderToggled(folder_id.to_string()), cx);
    }

    pub fn set_folder_collapsed(&mut self, folder_id: &str, collapsed: bool, cx: &mut Context<Self>) {
        self.set_folder_collapsed_for_project(None, folder_id, collapsed, cx);
    }

    pub fn set_folder_collapsed_for_project(
        &mut self,
        project_id: Option<&str>,
        folder_id: &str,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        let tree = self.config.tree_for_project_mut(project_id);
        crate::ssh_sessions::set_folder_collapsed_in_tree(tree, folder_id, collapsed);
        self.persist_and_emit(SessionEvent::FolderToggled(folder_id.to_string()), cx);
    }

    pub fn delete_node(&mut self, target_id: &str, cx: &mut Context<Self>) {
        self.delete_node_for_project(None, target_id, cx);
    }

    pub fn delete_node_for_project(
        &mut self,
        project_id: Option<&str>,
        target_id: &str,
        cx: &mut Context<Self>,
    ) {
        let tree = self.config.tree_for_project_mut(project_id);
        if crate::ssh_sessions::remove_node_from_tree(tree, target_id) {
            self.persist_and_emit(SessionEvent::NodeRemoved, cx);
        }
    }

    pub fn set_all_folders_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.set_all_folders_collapsed_for_project(None, collapsed, cx);
    }

    pub fn set_all_folders_collapsed_for_project(
        &mut self,
        project_id: Option<&str>,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        let tree = self.config.tree_for_project_mut(project_id);
        crate::ssh_sessions::set_all_folders_collapsed_in_tree(tree, collapsed);
        self.persist_and_emit(SessionEvent::AllFoldersCollapsed(collapsed), cx);
    }

    /// Move a node (session or folder) to a new parent at a specific index.
    /// Prevents cycles (moving a folder into its own descendant).
    pub fn move_node(
        &mut self,
        source_id: &str,
        target_parent_id: Option<&str>,
        target_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.move_node_for_project(None, source_id, target_parent_id, target_index, cx);
    }

    pub fn move_node_for_project(
        &mut self,
        project_id: Option<&str>,
        source_id: &str,
        target_parent_id: Option<&str>,
        target_index: usize,
        cx: &mut Context<Self>,
    ) {
        let tree = self.config.tree_for_project_mut(project_id);
        if crate::ssh_sessions::move_node_in_tree(
            tree,
            source_id,
            target_parent_id,
            target_index,
        ) {
            self.persist_and_emit(SessionEvent::TreeReplaced, cx);
        }
    }

    /// Rename a folder or session node. This is the single place that mutates a
    /// node name — it replaces the previous ad-hoc `rename_in_tree` closure
    /// that mutated `Workspace.ssh_sessions.tree` directly and saved on its own.
    pub fn rename_node(&mut self, node_id: &str, new_name: &str, cx: &mut Context<Self>) {
        self.rename_node_for_project(None, node_id, new_name, cx);
    }

    pub fn rename_node_for_project(
        &mut self,
        project_id: Option<&str>,
        node_id: &str,
        new_name: &str,
        cx: &mut Context<Self>,
    ) {
        let new_name_trimmed = new_name.trim();
        let tree_ref = self.config.tree_for_project(project_id);
        let parent_id = Self::find_parent_id(tree_ref, node_id);
        if Self::folder_name_exists_in_parent(tree_ref, parent_id.as_deref(), new_name_trimmed, Some(node_id)) {
            log::warn!("[session] 相同层级下已存在同名文件夹: {}", new_name_trimmed);
            return;
        }
        let tree = self.config.tree_for_project_mut(project_id);
        rename_node_in_tree(tree, node_id, new_name_trimmed);
        self.persist_and_emit(SessionEvent::NodeRenamed(node_id.to_string()), cx);
    }

    /// Persist to the database and emit a typed event. Centralizing persistence
    /// here means UI code can NEVER write `ssh_sessions.json` directly.
    ///
    /// 使用 debounce + async offload 避免在 GPUI 主线程上阻塞 SQLite I/O。
    /// 数据库不可用时回退到旧的 JSON 落盘，保证可用性。
    fn persist_and_emit(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        self.save_pending.store(true, Ordering::Relaxed);
        let save_pending = self.save_pending.clone();

        cx.spawn(async move |this, cx| {
            smol::Timer::after(std::time::Duration::from_millis(300)).await;

            if save_pending.swap(false, Ordering::Relaxed) {
                let config = cx.update(|cx| {
                    this.upgrade().map(|e| e.read(cx).config.clone())
                });
                if let Some(config) = config {
                    let result = smol::unblock(move || {
                        if let Some(db) = velowork_core::storage::database() {
                            SshSessionTreeRepository::new(db).save_config(&config)
                        } else {
                            Ok(())
                        }
                    }).await;
                    if let Err(e) = result {
                        log::error!("[session] 持久化会话树失败 | error: {:#}", e);
                    } else {
                        crate::sync::notify_config_changed();
                    }
                }
            }
        }).detach();

        cx.emit(event);
        cx.notify();
    }

    /// Synchronously flush any pending session tree save (called on quit).
    pub fn flush_pending_save(&self) {
        if self.save_pending.swap(false, Ordering::Relaxed) {
            if let Some(db) = velowork_core::storage::database() {
                if let Err(e) = SshSessionTreeRepository::new(db).save_config(&self.config) {
                    log::error!("[session] 退出时刷新会话树失败 | error: {:#}", e);
                } else {
                    crate::sync::notify_config_changed();
                }
            }
        }
    }

    /// 从数据库重新加载整个 SSH 会话树（例如云端恢复写入了 `session_tree_node`
    /// 表之后）。替换 `self.config` 并 emit `TreeReplaced`，使所有订阅者刷新。
    pub fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        let reloaded = if let Some(db) = database() {
            SshSessionTreeRepository::new(db)
                .load_config()
                .ok()
        } else {
            None
        };
        if let Some(config) = reloaded {
            self.config = config;
        }
        cx.emit(SessionEvent::TreeReplaced);
        cx.notify();
    }
}

/// Rename a folder or session node by id. Kept here (not a free function) so the
/// only place that mutates node names is this store.
fn rename_node_in_tree(nodes: &mut Vec<SessionTreeNode>, target_id: &str, new_name: &str) -> bool {
    for node in nodes.iter_mut() {
        match node {
            SessionTreeNode::Folder { id, name, children, .. } => {
                if id == target_id {
                    *name = new_name.to_string();
                    return true;
                }
                if rename_node_in_tree(children, target_id, new_name) {
                    return true;
                }
            }
            SessionTreeNode::Session { session } => {
                if session.id == target_id {
                    session.name = new_name.to_string();
                    return true;
                }
            }
        }
    }
    false
}

#[derive(Clone)]
pub struct GlobalSessionStore(pub Entity<SessionStore>);

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn test_duplicate_folder_name_check() {
        let tree = vec![
            SessionTreeNode::Folder {
                id: "f1".to_string(),
                name: "Work".to_string(),
                children: vec![
                    SessionTreeNode::Folder {
                        id: "f1-1".to_string(),
                        name: "Projects".to_string(),
                        children: vec![],
                        is_collapsed: false,
                    },
                ],
                is_collapsed: false,
            },
        ];

        // "Work" already exists at root
        assert!(SessionStore::folder_name_exists_in_parent(&tree, None, "Work", None));
        assert!(SessionStore::folder_name_exists_in_parent(&tree, None, " Work ", None));
        // "Other" does not exist at root
        assert!(!SessionStore::folder_name_exists_in_parent(&tree, None, "Other", None));

        // Inside "f1", "Projects" exists
        assert!(SessionStore::folder_name_exists_in_parent(&tree, Some("f1"), "Projects", None));
        assert!(!SessionStore::folder_name_exists_in_parent(&tree, Some("f1"), "Tasks", None));

        // Renaming "f1-1" (Projects) to "Projects" should be allowed when except_id is "f1-1"
        assert!(!SessionStore::folder_name_exists_in_parent(&tree, Some("f1"), "Projects", Some("f1-1")));
    }

    #[core::prelude::v1::test]
    fn test_project_based_data_isolation() {
        let mut config = SshSessionConfig::default();

        let proj1_nodes = config.tree_for_project_mut(Some("proj-1"));
        proj1_nodes.push(SessionTreeNode::Folder {
            id: "p1-f1".to_string(),
            name: "Proj1 Folder".to_string(),
            children: vec![],
            is_collapsed: false,
        });

        let proj2_nodes = config.tree_for_project_mut(Some("proj-2"));
        proj2_nodes.push(SessionTreeNode::Folder {
            id: "p2-f1".to_string(),
            name: "Proj2 Folder".to_string(),
            children: vec![],
            is_collapsed: false,
        });

        assert_eq!(config.tree_for_project(Some("proj-1")).len(), 1);
        assert_eq!(config.tree_for_project(Some("proj-1"))[0].name(), "Proj1 Folder");

        assert_eq!(config.tree_for_project(Some("proj-2")).len(), 1);
        assert_eq!(config.tree_for_project(Some("proj-2"))[0].name(), "Proj2 Folder");

        assert_eq!(config.tree_for_project(Some("proj-3")).len(), 0);
    }
}

impl Global for GlobalSessionStore {}
