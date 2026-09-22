use crate::views::overlays::overlay_manager::OverlayManager;
use crate::views::overlays::{TunnelDialogMode, TunnelMenuRequest, TunnelMenuTarget};
use crate::views::overlays::menus::tunnel_context_menu::{
    open_tunnel_context_menu, TunnelContextMenuEvent,
};
use velowork_ui::menu::PopupMenu;
use gpui::prelude::*;
use gpui::*;
use std::collections::HashSet;
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_state::{
    ReconnectPolicy, SshSession, TunnelKind, TunnelNode, TunnelProfile, TunnelRuntimeInfo,
    TunnelStatus,
};
use velowork_terminal::GlobalTunnelEngine;
use velowork_ui::behavior::{HoverBehavior, SelectedBehavior, StatefulElementBehaviorExt};
use velowork_ui::dock::{Panel, PanelAction, PanelInfo, PanelKind};
use crate::keybindings::RenameActiveNode;
use velowork_ui::icon::{AppIcon, folder_tree_icon};
use velowork_ui::input::InputState;
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::theme::{ThemeColors, surface_bg, surface_bg_t, theme, with_alpha};
use velowork_ui::tokens::{
    ui_space_md, ui_space_xs, ui_text, ui_text_md, ui_text_sm, ui_text_xs, ICON_STD,
    RADIUS_STD,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex, SemanticPalette};
use velowork_workspace::focus::FocusManager;
use velowork_workspace::state::Workspace;
use velowork_workspace::stores::{GlobalSessionStore, GlobalTunnelStore, TunnelEvent};
use velowork_workspace::tunnels::{
    new_tunnel_id, tunnel_find_node_ref, tunnel_folder_name_exists, tunnel_node_name_exists,
    tunnel_parent_id_of, tunnel_unique_duplicate_name,
};

/// 隧道树行外观：由设计系统 Compact 档统一解析（行高/字体/图标尺寸），
/// 替代散落的硬编码 px 值。
fn tunnel_row_appearance(t: &ThemeColors, cx: &App) -> velowork_ui::ControlAppearance {
    velowork_ui::tree::tree_row_appearance(t, cx)
}

/// 隧道树行高（纯几何，与主题无关）。供滚动计算使用。
fn tunnel_row_height(cx: &App) -> Pixels {
    velowork_ui::tree_row_height(cx)
}

#[derive(Clone)]
pub(crate) enum InlineTunnelFolderState {
    Creating {
        parent_id: Option<String>,
        input: Option<Entity<InputState>>,
        placeholder: String,
    },
    Renaming {
        id: String,
        original_name: String,
        input: Option<Entity<InputState>>,
    },
}

/// 隧道行内重命名状态。隧道不支持行内新建，因此只保留重命名一种。
#[derive(Clone)]
pub(crate) struct InlineTunnelState {
    pub id: String,
    pub original_name: String,
    pub input: Option<Entity<InputState>>,
}

fn convert_tunnel_nodes(
    nodes: &[TunnelNode],
) -> Vec<velowork_ui::TreeNodeData<String, TunnelNode>> {
    nodes
        .iter()
        .map(|node| match node {
            TunnelNode::Folder {
                id, name, children, ..
            } => velowork_ui::TreeNodeData::new(id.clone(), name.clone(), true)
                .with_children(convert_tunnel_nodes(children))
                .with_payload(node.clone()),
            TunnelNode::Tunnel { profile } => {
                velowork_ui::TreeNodeData::new(profile.id.clone(), profile.name.clone(), false)
                    .with_payload(node.clone())
            }
        })
        .collect()
}

fn collect_tunnel_expanded_keys(
    nodes: &[TunnelNode],
    force_expand: bool,
    out: &mut HashSet<String>,
) {
    for node in nodes {
        if let TunnelNode::Folder {
            id,
            expanded,
            children,
            ..
        } = node
        {
            if *expanded || force_expand {
                out.insert(id.clone());
                collect_tunnel_expanded_keys(children, force_expand, out);
            }
        }
    }
}

pub struct TunnelsPanel {
    _workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,
    overlay_manager: Entity<OverlayManager>,
    engine: Arc<velowork_terminal::TunnelEngine>,
    focus_handle: FocusHandle,

    filter_input: Option<Entity<InputState>>,
    nodes: Vec<TunnelNode>,
    show_tunnel_search: bool,

    selected_tunnel_ids: HashSet<String>,
    tunnel_selection_anchor: Option<String>,
    tunnel_visible_order: Vec<String>,
    /// 键盘导航用的焦点索引（基于 `tunnel_visible_order`）。
    tunnel_focused_index: Option<usize>,
    /// 隧道树滚动容器句柄，用于方向键导航时把焦点行滚动进可视区。
    tunnel_scroll_handle: ScrollHandle,

    /// 文件夹行内新建/重命名状态。
    inline_folder: Option<InlineTunnelFolderState>,
    inline_folder_sub: Option<Subscription>,
    /// 隧道行内重命名状态（与文件夹行内重命名解耦，隧道仅支持重命名）。
    inline_tunnel: Option<InlineTunnelState>,
    inline_tunnel_sub: Option<Subscription>,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    context_menu: Option<Entity<PopupMenu>>,
    selected_node_bounds: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Bounds<Pixels>>>>,
}

impl TunnelsPanel {
    pub fn set_overlay_registry(&mut self, registry: Entity<OverlayRegistry>) {
        self.overlay_registry = Some(registry);
    }

    pub fn new(
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        overlay_manager: Entity<OverlayManager>,
        cx: &mut Context<Self>,
    ) -> Self {
        let engine = cx
            .try_global::<GlobalTunnelEngine>()
            .map(|e| e.0.clone())
            .unwrap_or_else(|| {
                Arc::new(velowork_terminal::TunnelEngine::new(Arc::new(
                    velowork_terminal::ConnectionManager::new(std::time::Duration::from_secs(300)),
                )))
            });

        if let Some(entity) = cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone()) {
            cx.subscribe(&entity, |this, _s, _event: &TunnelEvent, cx| {
                this.reload_nodes(cx);
            })
            .detach();
            cx.observe(&entity, |this: &mut Self, _, cx| {
                this.reload_nodes(cx);
            })
            .detach();
        }

        cx.observe(&workspace, |this: &mut Self, _ws, cx| {
            this.reload_nodes(cx);
        })
        .detach();

        cx.observe(&focus_manager, |this: &mut Self, _fm, cx| {
            this.reload_nodes(cx);
        })
        .detach();

        // 把行内新建/重命名文件夹的回调挂到 overlay_manager，供右键菜单
        // 「新建文件夹」「重命名」触发（与快捷指令面板一致）。
        let weak_panel = cx.entity().downgrade();
        overlay_manager.update(cx, |om, _cx| {
            let weak = weak_panel.clone();
            om.on_tunnel_create_folder = Some(Arc::new(move |parent_id, cx| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |p, cx| p.inline_create_folder(parent_id, cx));
                }
            }));
            let weak = weak_panel.clone();
            om.on_tunnel_rename_folder = Some(Arc::new(move |id, name, cx| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |p, cx| p.inline_rename_folder(id, name, cx));
                }
            }));
            // 隧道右键「重命名」触发行内重命名输入框（与文件夹一致）。
            let weak = weak_panel.clone();
            om.on_tunnel_rename = Some(Arc::new(move |id, name, cx| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |p, cx| p.inline_rename_tunnel(id, name, cx));
                }
            }));
        });

        let mut panel = Self {
            _workspace: workspace,
            focus_manager,
            overlay_manager,
            engine,
            focus_handle: cx.focus_handle(),
            filter_input: None,
            nodes: Vec::new(),
            show_tunnel_search: false,
            selected_tunnel_ids: HashSet::new(),
            tunnel_selection_anchor: None,
            tunnel_visible_order: Vec::new(),
            tunnel_focused_index: None,
            tunnel_scroll_handle: ScrollHandle::new(),
            inline_folder: None,
            inline_folder_sub: None,
            inline_tunnel: None,
            inline_tunnel_sub: None,
            overlay_registry: None,
            context_menu: None,
            selected_node_bounds: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
        };

        cx.observe(&panel.overlay_manager, |_this: &mut Self, _om, cx| {
            cx.notify();
        })
        .detach();

        // 初始化时立即按当前活跃项目过滤隧道节点（与 ServicePanel 一致），
        // 避免首次渲染时显示未过滤的全部隧道。
        panel.reload_nodes(cx);

        panel
    }

    pub fn is_selection_active(&self, window: &Window, cx: &App) -> bool {
        let is_focused = self.focus_handle.is_focused(window);
        let has_my_context_menu = self.context_menu.is_some();
        let has_my_modal = self
            .overlay_manager
            .read(cx)
            .active_modal_belongs_to(&self.focus_handle);
        is_focused || has_my_context_menu || has_my_modal
    }

    fn active_project_id(&self, cx: &App) -> Option<String> {
        let raw = self.focus_manager.read(cx).active_project_id().cloned();
        if let Some(ref pid) = raw {
            if self._workspace.read(cx).project(pid).is_some() {
                return raw;
            }
        }
        self._workspace.read(cx).projects().first().map(|p| p.id.clone())
    }

    fn reload_nodes(&mut self, cx: &mut Context<Self>) {
        let active_pid = self.active_project_id(cx);
        let all_nodes = cx
            .try_global::<GlobalTunnelStore>()
            .map(|s| s.0.read(cx).nodes().to_vec())
            .unwrap_or_default();
        let new_nodes = filter_tunnel_tree(&all_nodes, active_pid.as_deref(), cx);
        log::debug!(
            "[tunnel_panel] reload_nodes: active_pid={:?}, all={}, filtered={}",
            active_pid,
            all_nodes.len(),
            new_nodes.len()
        );
        let old_selected = self.selected_tunnel_ids.clone();
        self.selected_tunnel_ids
            .retain(|id| tunnel_find_node_ref(&new_nodes, id).is_some());
        if self.selected_tunnel_ids.is_empty() && !old_selected.is_empty() {
            if let Some(deleted_id) = old_selected.iter().next() {
                if let Some(pos) = self.tunnel_visible_order.iter().position(|id| id == deleted_id) {
                    let remaining_visible: Vec<String> = self.tunnel_visible_order
                        .iter()
                        .filter(|id| !old_selected.contains(*id) && tunnel_find_node_ref(&new_nodes, id).is_some())
                        .cloned()
                        .collect();
                    if !remaining_visible.is_empty() {
                        let next_idx = if pos < remaining_visible.len() {
                            pos
                        } else {
                            remaining_visible.len() - 1
                        };
                        let next_id = remaining_visible[next_idx].clone();
                        self.selected_tunnel_ids.insert(next_id);
                        self.tunnel_focused_index = Some(next_idx);
                    }
                }
            }
        }
        if let Some(ref anchor) = self.tunnel_selection_anchor {
            if tunnel_find_node_ref(&new_nodes, anchor).is_none() {
                self.tunnel_selection_anchor = self.selected_tunnel_ids.iter().next().cloned();
            }
        }
        self.nodes = new_nodes;
        cx.notify();
    }

    fn find_session(&self, session_id: &str, cx: &App) -> Option<SshSession> {
        let store = cx.try_global::<GlobalSessionStore>()?;
        store.0.read(cx).find_session(session_id).cloned()
    }

    fn session_name(&self, session_id: &str, cx: &App) -> String {
        if session_id.is_empty() {
            return i18n!(cx, "tunnel.no_session");
        }
        self.find_session(session_id, cx)
            .map(|s| s.name)
            .unwrap_or_else(|| i18n!(cx, "tunnel.unknown_session"))
    }

    // ── 启动 / 停止 ──────────────────────────────────────────────────────────

    fn toggle_tunnel(&mut self, profile: &TunnelProfile, cx: &mut Context<Self>) {
        let info = self.engine.get_runtime_info(&profile.id);
        let is_running = info
            .map(|i| i.status == TunnelStatus::Running)
            .unwrap_or(false);

        if is_running {
            self.engine.stop_tunnel(&profile.id);
            self.clear_runtime(&profile.id, cx);
        } else if let Some(session) = self.find_session(&profile.session_id, cx) {
            let jump_session = session.jump_session_id.as_deref()
                .and_then(|jid| self.find_session(jid, cx));
            let engine = self.engine.clone();
            let profile = profile.clone();
            let tunnel_id = profile.id.clone();
            cx.spawn(async move |_this, cx| {
                if let Err(e) = engine.start_tunnel(profile, session, jump_session).await {
                    log::error!("[tunnel_panel] Failed to start tunnel {}: {}", tunnel_id, e);
                    let _ = cx.update(|cx| {
                        velowork_workspace::toast::ToastManager::error(format!("Failed to start tunnel: {}", e), cx);
                    });
                }
                let _ = cx.update(|cx| {
                    if let Some(info) = engine.get_runtime_info(&tunnel_id) {
                        if let Some(entity) =
                            cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone())
                        {
                            entity.update(cx, |s, cx| s.update_runtime_info(&tunnel_id, info, cx));
                        }
                    }
                });
            })
            .detach();
        }
    }

    fn clear_runtime(&self, id: &str, cx: &mut Context<Self>) {
        if let Some(entity) = cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone()) {
            entity.update(cx, |s, cx| {
                s.update_runtime_info(id, TunnelRuntimeInfo::default(), cx)
            });
        }
    }

    // ── 选择逻辑 ────────────────────────────────────────────────────────────

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selected_tunnel_ids.is_empty() || self.tunnel_selection_anchor.is_some() {
            self.selected_tunnel_ids.clear();
            self.tunnel_selection_anchor = None;
            self.tunnel_focused_index = None;
            cx.notify();
        }
    }

    fn handle_node_click(
        &mut self,
        node: &TunnelNode,
        is_folder: bool,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::None);
        });

        let mods = event.modifiers();
        let ctrl = mods.control || mods.platform;
        let shift = mods.shift;
        let node_id = node.id().to_string();

        let clicked_index = self.tunnel_visible_order.iter().position(|x| x == &node_id);

        if shift {
            let anchor = self.tunnel_selection_anchor.clone();
            let range: Option<Vec<String>> = anchor.and_then(|anchor| {
                let ai = self
                    .tunnel_visible_order
                    .iter()
                    .position(|id| id == &anchor)?;
                let ti = self
                    .tunnel_visible_order
                    .iter()
                    .position(|id| id == &node_id)?;
                let (lo, hi) = if ai <= ti { (ai, ti) } else { (ti, ai) };
                Some(self.tunnel_visible_order[lo..=hi].to_vec())
            });
            match range {
                Some(ids) => {
                    self.selected_tunnel_ids = ids.into_iter().collect();
                }
                None => {
                    self.selected_tunnel_ids.clear();
                    self.selected_tunnel_ids.insert(node_id.clone());
                    self.tunnel_selection_anchor = Some(node_id.clone());
                }
            }
            self.tunnel_focused_index = clicked_index;
            cx.notify();
            return;
        }

        if ctrl {
            if self.selected_tunnel_ids.contains(&node_id) {
                self.selected_tunnel_ids.remove(&node_id);
            } else {
                self.selected_tunnel_ids.insert(node_id.clone());
            }
            self.tunnel_selection_anchor = Some(node_id.clone());
            self.tunnel_focused_index = clicked_index;
            cx.notify();
            return;
        }

        if is_folder {
            self.selected_tunnel_ids.clear();
            self.selected_tunnel_ids.insert(node_id.clone());
            self.tunnel_selection_anchor = Some(node_id.clone());
            self.tunnel_focused_index = clicked_index;
            if let TunnelNode::Folder { expanded, .. } = node {
                let new_expanded = !*expanded;
                let id = node_id.clone();
                if let Some(entity) = cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone()) {
                    entity.update(cx, |s, cx| s.set_folder_expanded(&id, new_expanded, cx));
                }
            }
            cx.notify();
            return;
        }

        if event.click_count() >= 2 {
            // 双击隧道：启动 / 停止。
            if let TunnelNode::Tunnel { profile } = node {
                self.toggle_tunnel(profile, cx);
            }
        } else {
            self.selected_tunnel_ids.clear();
            self.selected_tunnel_ids.insert(node_id.clone());
            self.tunnel_selection_anchor = Some(node_id.clone());
            self.tunnel_focused_index = clicked_index;
            cx.notify();
        }
    }

    fn get_selected_folder_id(&self) -> Option<String> {
        for id in &self.selected_tunnel_ids {
            if matches!(
                tunnel_find_node_ref(&self.nodes, id),
                Some(TunnelNode::Folder { .. })
            ) {
                return Some(id.clone());
            }
        }
        None
    }

    // ── 右键菜单 ────────────────────────────────────────────────────────────

    fn show_menu(
        &mut self,
        target: TunnelMenuTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ctx) = self.context_menu.take() {
            ctx.update(cx, |m, cx| {
                m.set_on_close(None);
                m.close(window, cx);
            });
        }

        if let TunnelMenuTarget::Node(ref node) = target {
            let id = node.id().to_string();
            if !self.selected_tunnel_ids.contains(&id) {
                self.selected_tunnel_ids.clear();
                self.selected_tunnel_ids.insert(id.clone());
                self.tunnel_selection_anchor = Some(id);
                window.focus(&self.focus_handle, cx);
            }
        }

        let request = TunnelMenuRequest {
            position,
            target,
            selected_ids: self.selected_tunnel_ids.iter().cloned().collect(),
        };

        let overlay_manager = self.overlay_manager.clone();
        let this_weak = cx.entity().downgrade();
        let this_weak_close = this_weak.clone();

        let menu = open_tunnel_context_menu(
            request,
            self.overlay_registry.clone(),
            move |event, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| match event {
                        TunnelContextMenuEvent::Close => {
                            cx.notify();
                        }
                        TunnelContextMenuEvent::NewTunnel { parent_id } => {
                            let pid = parent_id.clone();
                            let active_pid = this.active_project_id(cx);
                            let origin = this.focus_handle.clone();
                            let panel = this.focus_handle.clone();
                            overlay_manager.update(cx, |om, cx| {
                                om.show_tunnel_dialog_with_origin(
                                    TunnelDialogMode::Create { parent_id: pid, project_id: active_pid },
                                    Some(origin),
                                    Some(panel),
                                    cx,
                                );
                            });
                            cx.notify();
                        }
                        TunnelContextMenuEvent::NewFolder { parent_id } => {
                            this.inline_create_folder(parent_id.clone(), cx);
                            cx.notify();
                        }
                        TunnelContextMenuEvent::Edit { node } => {
                            if let TunnelNode::Tunnel { profile } = node {
                                let prof = profile.clone();
                                let origin = this.focus_handle.clone();
                                let panel = this.focus_handle.clone();
                                overlay_manager.update(cx, |om, cx| {
                                    om.show_tunnel_dialog_with_origin(
                                        TunnelDialogMode::Edit { profile: prof },
                                        Some(origin),
                                        Some(panel),
                                        cx,
                                    );
                                });
                            }
                            cx.notify();
                        }
                        TunnelContextMenuEvent::Rename { node } => {
                            match node {
                                TunnelNode::Folder { id, name, .. } => {
                                    this.inline_rename_folder(id.clone(), name.clone(), cx);
                                }
                                TunnelNode::Tunnel { profile } => {
                                    this.inline_rename_tunnel(profile.id.clone(), profile.name.clone(), cx);
                                }
                            }
                            cx.notify();
                        }
                        TunnelContextMenuEvent::Copy { node } => {
                            if let TunnelNode::Tunnel { profile } = node {
                                if let Some(store) = cx.try_global::<velowork_workspace::stores::GlobalTunnelStore>() {
                                    let store_entity = store.0.clone();
                                    let mut new_id = None;
                                    store_entity.update(cx, |s, cx| {
                                        let nodes = s.nodes().to_vec();
                                        let parent = tunnel_parent_id_of(&nodes, &profile.id).flatten();
                                        let new_name = tunnel_unique_duplicate_name(
                                            &nodes,
                                            parent.as_deref(),
                                            &profile.name,
                                        );
                                        let mut new_profile = profile.clone();
                                        let nid = new_tunnel_id();
                                        new_profile.id = nid.clone();
                                        new_profile.name = new_name;
                                        new_id = Some(nid);
                                        s.add_tunnel_to(new_profile, parent.as_deref(), cx);
                                    });
                                    if let Some(nid) = new_id {
                                        this.selected_tunnel_ids.clear();
                                        this.selected_tunnel_ids.insert(nid.clone());
                                        this.tunnel_selection_anchor = Some(nid.clone());
                                        this.scroll_node_into_view(&nid, cx);
                                    }
                                }
                            }
                            cx.notify();
                        }
                        TunnelContextMenuEvent::Delete { ids } => {
                            let ids_clone = ids.clone();
                            let origin = this.focus_handle.clone();
                            let panel = this.focus_handle.clone();
                            let click_origin = ids.iter().find_map(|id| {
                                this.selected_node_bounds.borrow().get(id).map(|b| b.center())
                            }).or_else(|| {
                                this.selected_node_bounds.borrow().values().next().map(|b| b.center())
                            });
                            overlay_manager.update(cx, |om, cx| {
                                if let Some(pt) = click_origin {
                                    om.record_click_origin(pt);
                                }
                                om.request_tunnel_delete_confirm_with_origin(
                                    ids_clone,
                                    Some(origin),
                                    Some(panel),
                                    cx,
                                );
                            });
                            cx.notify();
                        }
                    });
                }
            },
            window,
            cx,
        );

        menu.update(cx, |m, _| {
            m.set_previous_focus(Some(self.focus_handle.clone()));
        });

        let menu_id = menu.entity_id();
        let on_close = Arc::new(move |window: &mut Window, cx: &mut App| {
            if let Some(this) = this_weak_close.upgrade() {
                this.update(cx, |this, cx| {
                    if this.context_menu.as_ref().map(|m| m.entity_id()) == Some(menu_id) {
                        this.context_menu = None;
                        if window.focused(cx).is_none() {
                            window.focus(&this.focus_handle, cx);
                        }
                        cx.notify();
                    }
                });
            }
        });
        menu.update(cx, |m, _| m.set_on_close(Some(on_close)));

        self.context_menu = Some(menu);
        cx.notify();
    }

    // ── 对话框 ──────────────────────────────────────────────────────────────

    fn open_add_dialog(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if let Some(window) = window {
            window.focus(&self.focus_handle, cx);
        }
        let parent_id = self.get_selected_folder_id();
        let active_pid = self.active_project_id(cx);
        let origin = self.focus_handle.clone();
        let panel = self.focus_handle.clone();
        self.overlay_manager.update(cx, |om, cx| {
            om.show_tunnel_dialog_with_origin(
                TunnelDialogMode::Create { parent_id, project_id: active_pid },
                Some(origin),
                Some(panel),
                cx,
            );
        });
    }

    // ── 行内文件夹新建 / 重命名 ─────────────────────────────────────────────

    pub fn inline_create_folder(&mut self, parent_id: Option<String>, cx: &mut Context<Self>) {
        let parent_id = parent_id.or_else(|| self.get_selected_folder_id());

        if let Some(ref pid) = parent_id {
            let pid = pid.clone();
            if let Some(entity) = cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone()) {
                entity.update(cx, |s, cx| s.set_folder_expanded(&pid, true, cx));
            }
            self.reload_nodes(cx);
        }

        let placeholder = i18n!(cx, "workspace.folder.create");
        self.inline_folder_sub = None;
        self.inline_folder = Some(InlineTunnelFolderState::Creating {
            parent_id,
            input: None,
            placeholder,
        });
        cx.notify();
    }

    pub fn inline_rename_folder(&mut self, id: String, name: String, cx: &mut Context<Self>) {
        self.inline_folder_sub = None;
        self.inline_folder = Some(InlineTunnelFolderState::Renaming {
            id,
            original_name: name,
            input: None,
        });
        cx.notify();
    }

    /// F2：重命名当前选中的节点（文件夹行内重命名，隧道打开编辑对话框）。
    pub fn rename_selected_node(&mut self, id: &str, cx: &mut Context<Self>) {
        let node = tunnel_find_node_ref(&self.nodes, id).cloned();
        match node {
            Some(TunnelNode::Folder { id, name, .. }) => {
                self.inline_rename_folder(id, name, cx);
            }
            Some(TunnelNode::Tunnel { profile }) => {
                self.inline_rename_tunnel(profile.id.clone(), profile.name.clone(), cx);
            }
            None => {}
        }
    }

    pub fn cancel_inline_folder(&mut self, cx: &mut Context<Self>) {
        if self.inline_folder.is_some() {
            self.inline_folder = None;
            self.inline_folder_sub = None;
            cx.notify();
        }
    }

    // ── 隧道行内重命名 ────────────────────────────────────────────────────────

    /// 触发某个隧道的行内重命名输入框。
    pub fn inline_rename_tunnel(&mut self, id: String, name: String, cx: &mut Context<Self>) {
        self.inline_tunnel_sub = None;
        self.inline_tunnel = Some(InlineTunnelState {
            id: id.clone(),
            original_name: name,
            input: None,
        });
        // 选中该行，便于退场后焦点回到该隧道。
        self.selected_tunnel_ids.clear();
        self.selected_tunnel_ids.insert(id.clone());
        self.tunnel_selection_anchor = Some(id.clone());
        self.tunnel_focused_index = self.tunnel_visible_order.iter().position(|x| x == &id);
        cx.notify();
    }

    pub fn cancel_inline_tunnel(&mut self, cx: &mut Context<Self>) {
        if self.inline_tunnel.is_some() {
            self.inline_tunnel = None;
            self.inline_tunnel_sub = None;
            cx.notify();
        }
    }

    pub fn commit_inline_tunnel(&mut self, cx: &mut Context<Self>) {
        self.inline_tunnel_sub = None;
        if let Some(state) = self.inline_tunnel.take() {
            let val = state
                .input
                .as_ref()
                .map(|i| i.read(cx).text().to_string().trim().to_string())
                .unwrap_or_default();
            if !val.is_empty() && val != state.original_name {
                // 同目录（含文件夹、隧道）已存在同名节点则保留输入框让用户改。
                let parent_id = tunnel_parent_id_of(&self.nodes, &state.id).flatten();
                if tunnel_node_name_exists(
                    &self.nodes,
                    parent_id.as_deref(),
                    &val,
                    Some(&state.id),
                ) {
                    self.inline_tunnel = Some(InlineTunnelState {
                        id: state.id,
                        original_name: state.original_name,
                        input: state.input,
                    });
                    cx.notify();
                    return;
                }
                if let Some(entity) = cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone()) {
                    entity.update(cx, |s, cx| s.rename_node(&state.id, &val, cx));
                    self.reload_nodes(cx);
                }
            }
            cx.notify();
        }
    }

    fn render_inline_tunnel_input_row(
        &mut self,
        profile: &TunnelProfile,
        depth: usize,
        t: &ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_ent = if let Some(ref mut state) = self.inline_tunnel {
            if state.input.is_none() {
                let inp = cx.new(|cx| {
                    InputState::new(cx).default_value(&state.original_name)
                });
                state.input = Some(inp);
            }
            state.input.clone()
        } else {
            None
        };

        let Some(input) = input_ent else {
            return div().into_any_element();
        };

        if self.inline_tunnel_sub.is_none() {
            let focus_handle = input.focus_handle(cx);
            window.focus(&focus_handle, cx);
            self.inline_tunnel_sub =
                Some(cx.on_blur(&focus_handle, window, move |this, window, cx| {
                    this.commit_inline_tunnel(cx);
                    window.focus(&this.focus_handle, cx);
                }));
        }

        let ap = tunnel_row_appearance(t, cx);

        let (kind_icon, kind_color) = match &profile.kind {
            TunnelKind::Local { .. } => (AppIcon::TunnelL, t.success),
            TunnelKind::Remote { .. } => (AppIcon::TunnelR, t.warning),
            TunnelKind::Dynamic { .. } => (AppIcon::TunnelD, t.accent),
        };

        div()
            .id("tunnel-inline-tunnel-container")
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .id("tunnel-inline-tunnel-row")
                    .h(ap.height)
                    .pl(px(8.0 + depth as f32 * 14.0))
                    .pr(ui_space_md(cx))
                    .flex()
                    .items_center()
                    .gap(ui_space_xs(cx))
                    .child(
                        div()
                            .w(ap.icon_size)
                            .h(ap.icon_size)
                            .flex_shrink_0(),
                    )
                    .child(
                        kind_icon
                            .size(ap.icon_size)
                            .flex_shrink_0()
                            .text_color(rgb(kind_color)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(velowork_ui::Input::new(&input))
                            .on_key_down(cx.listener(
                                move |this, e: &KeyDownEvent, window, cx| {
                                    if e.keystroke.key == "enter" {
                                        this.commit_inline_tunnel(cx);
                                        if this.inline_tunnel.is_none() {
                                            window.focus(&this.focus_handle, cx);
                                        }
                                        cx.stop_propagation();
                                    } else if e.keystroke.key == "escape" {
                                        this.cancel_inline_tunnel(cx);
                                        window.focus(&this.focus_handle, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            )),
                    ),
            )
            .into_any_element()
    }

    pub fn commit_inline_folder(&mut self, cx: &mut Context<Self>) {
        self.inline_folder_sub = None;
        if let Some(state) = self.inline_folder.take() {
            match state {
                InlineTunnelFolderState::Creating { parent_id, input, placeholder } => {
                    let val = input
                        .as_ref()
                        .map(|i| i.read(cx).text().to_string().trim().to_string())
                        .unwrap_or_default();
                    if !val.is_empty() {
                        if tunnel_folder_name_exists(&self.nodes, parent_id.as_deref(), &val, None)
                        {
                            self.inline_folder =
                                Some(InlineTunnelFolderState::Creating { parent_id, input, placeholder });
                            cx.notify();
                            return;
                        }
                        let active_pid = self.active_project_id(cx);
                        if let Some(entity) =
                            cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone())
                        {
                            let new_id = entity
                                .update(cx, |s, cx| s.add_folder_for_project(&val, parent_id.as_deref(), active_pid.as_deref(), cx));
                            self.reload_nodes(cx);
                            self.selected_tunnel_ids.clear();
                            self.selected_tunnel_ids.insert(new_id.clone());
                            self.tunnel_selection_anchor = Some(new_id);
                        }
                    }
                }
                InlineTunnelFolderState::Renaming {
                    id,
                    original_name,
                    input,
                } => {
                    let val = input
                        .as_ref()
                        .map(|i| i.read(cx).text().to_string().trim().to_string())
                        .unwrap_or_default();
                    if !val.is_empty() && val != original_name {
                        let parent_id = tunnel_parent_id_of(&self.nodes, &id).flatten();
                        if tunnel_folder_name_exists(
                            &self.nodes,
                            parent_id.as_deref(),
                            &val,
                            Some(&id),
                        ) {
                            self.inline_folder = Some(InlineTunnelFolderState::Renaming {
                                id,
                                original_name,
                                input,
                            });
                            cx.notify();
                            return;
                        }
                        if let Some(entity) =
                            cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone())
                        {
                            entity.update(cx, |s, cx| s.rename_node(&id, &val, cx));
                            self.reload_nodes(cx);
                        }
                    }
                }
            }
            cx.notify();
        }
    }

    // ── 键盘导航 ────────────────────────────────────────────────────────────

    fn current_focus_index(&self) -> Option<usize> {
        if let Some(i) = self.tunnel_focused_index {
            if i < self.tunnel_visible_order.len() {
                if let Some(fid) = self.focused_node_id() {
                    if self.tunnel_visible_order[i] == fid {
                        return Some(i);
                    }
                }
            }
        }
        self.focused_node_id()
            .and_then(|id| self.tunnel_visible_order.iter().position(|x| x == &id))
    }

    fn focused_node_id(&self) -> Option<String> {
        self.tunnel_selection_anchor
            .clone()
            .or_else(|| self.selected_tunnel_ids.iter().next().cloned())
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.selected_tunnel_ids.is_empty() || self.tunnel_visible_order.is_empty() {
            return;
        }
        let idx = match self.current_focus_index() {
            Some(i) => i,
            None => 0,
        };
        let new_idx = if delta > 0 {
            (idx + delta as usize).min(self.tunnel_visible_order.len() - 1)
        } else {
            idx.saturating_sub((-delta) as usize)
        };
        let new_id = self.tunnel_visible_order[new_idx].clone();
        self.selected_tunnel_ids.clear();
        self.selected_tunnel_ids.insert(new_id.clone());
        self.tunnel_selection_anchor = Some(new_id.clone());
        self.tunnel_focused_index = Some(new_idx);
        self.scroll_node_into_view(&new_id, cx);
        cx.notify();
    }

    fn scroll_node_into_view(&self, id: &str, cx: &App) {
        let Some(idx) = self.tunnel_visible_order.iter().position(|x| x == id) else {
            return;
        };
        let row_h: Pixels = tunnel_row_height(cx);
        let pad_top: Pixels = ui_space_xs(cx);
        let top = pad_top + row_h * idx as f32;
        let bottom = top + row_h;

        let viewport_h = self.tunnel_scroll_handle.bounds().size.height;
        if viewport_h == px(0.0) {
            return;
        }
        let cur = self.tunnel_scroll_handle.offset().y;
        let row_top_vp = top + cur;
        let row_bottom_vp = bottom + cur;

        let mut new_off = if row_top_vp < px(0.0) {
            -top
        } else if row_bottom_vp > viewport_h {
            viewport_h - bottom
        } else {
            return;
        };
        let max_off = self.tunnel_scroll_handle.max_offset().y;
        new_off = new_off.max(-max_off).min(px(0.0));
        self.tunnel_scroll_handle
            .set_offset(Point::new(px(0.0), new_off));
    }

    /// 空格：对焦点节点执行等价操作——文件夹展开/收起，隧道单选。
    fn activate_focused(&mut self, cx: &mut Context<Self>) {
        if self.selected_tunnel_ids.is_empty() {
            return;
        }
        let id = match self.focused_node_id() {
            Some(id) => id,
            None => return,
        };
        if matches!(
            tunnel_find_node_ref(&self.nodes, &id),
            Some(TunnelNode::Folder { .. })
        ) {
            if let Some(entity) = cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone()) {
                let cur = tunnel_find_node_ref(&self.nodes, &id).and_then(|n| match n {
                    TunnelNode::Folder { expanded, .. } => Some(*expanded),
                    _ => None,
                });
                if let Some(expanded) = cur {
                    let id = id.clone();
                    entity.update(cx, |s, cx| s.set_folder_expanded(&id, !expanded, cx));
                }
            }
        } else {
            self.tunnel_focused_index = self.current_focus_index();
            cx.notify();
        }
    }

    /// Enter：文件夹展开/收起；隧道启动 / 停止。
    fn enter_focused(&mut self, cx: &mut Context<Self>) {
        if self.selected_tunnel_ids.is_empty() {
            return;
        }
        let id = match self.focused_node_id() {
            Some(id) => id,
            None => return,
        };
        match tunnel_find_node_ref(&self.nodes, &id).cloned() {
            Some(TunnelNode::Folder { expanded, .. }) => {
                let new_expanded = !expanded;
                if let Some(entity) = cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone()) {
                    let id = id.clone();
                    entity.update(cx, |s, cx| s.set_folder_expanded(&id, new_expanded, cx));
                }
            }
            Some(TunnelNode::Tunnel { profile }) => {
                self.toggle_tunnel(&profile, cx);
            }
            None => {}
        }
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_tunnel_search = !self.show_tunnel_search;
        if !self.show_tunnel_search {
            self.filter_input = None;
        } else {
            let input = self.filter_input.get_or_insert_with(|| {
                let input = cx.new(|cx| {
                    InputState::new(cx)
                        .placeholder(i18n!(cx, "tunnel.search_tooltip"))
                });
                let input_clone = input.clone();
                cx.subscribe(
                    &input_clone,
                    |_this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                        cx.notify();
                    },
                )
                .detach();
                input
            });
            input.update(cx, |inp, cx| {
                inp.focus(window, cx);
                inp.select_all(cx);
            });
        }
        cx.notify();
    }

    // ── 拖拽移动 ────────────────────────────────────────────────────────────

    fn drop_node(
        &mut self,
        drag: &TunnelDrag,
        target_id: &str,
        target_is_folder: bool,
        cx: &mut Context<Self>,
    ) {
        if drag.node_id == target_id {
            return;
        }
        let target_id = target_id.to_string();
        let store = match cx.try_global::<GlobalTunnelStore>() {
            Some(s) => s.0.clone(),
            None => return,
        };
        let nodes = self.nodes.clone();
        let Some(inner) = tunnel_parent_id_of(&nodes, &target_id) else {
            return;
        };
        let parent_id: Option<String> = inner;
        let siblings: &[TunnelNode] = match &parent_id {
            None => &nodes,
            Some(p) => match tunnel_find_node_ref(&nodes, p) {
                Some(TunnelNode::Folder { children, .. }) => children,
                _ => &nodes,
            },
        };
        let target_index = siblings.iter().position(|c| c.id() == target_id);

        let (new_parent, new_index) = match target_index {
            Some(idx) => {
                if !drag.is_folder && target_is_folder {
                    (Some(target_id.clone()), usize::MAX)
                } else {
                    (parent_id.clone(), idx)
                }
            }
            None => (parent_id.clone(), usize::MAX),
        };

        store.update(cx, |s, cx| {
            s.move_node(&drag.node_id, new_parent.as_deref(), new_index, cx);
        });
        self.reload_nodes(cx);
    }

    fn drop_node_to_root(&mut self, drag: &TunnelDrag, cx: &mut Context<Self>) {
        if drag.node_id.is_empty() {
            return;
        }
        if let Some(entity) = cx.try_global::<GlobalTunnelStore>().map(|s| s.0.clone()) {
            let id = drag.node_id.clone();
            entity.update(cx, |s, cx| s.move_node(&id, None, usize::MAX, cx));
        }
        self.reload_nodes(cx);
    }

    // ── 行内文件夹输入行 ────────────────────────────────────────────────────

    fn render_inline_folder_input_row(
        &mut self,
        folder_icon_id: &str,
        is_expanded: bool,
        depth: usize,
        t: &ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_ent = if let Some(ref mut state) = self.inline_folder {
            let inp = match state {
                InlineTunnelFolderState::Creating { input, placeholder, .. } => {
                    if input.is_none() {
                        let ph = placeholder.clone();
                        let i = cx.new(|cx| InputState::new(cx).placeholder(ph));
                        *input = Some(i);
                    }
                    input.clone()
                }
                InlineTunnelFolderState::Renaming { input, original_name, .. } => {
                    if input.is_none() {
                        let name = original_name.clone();
                        let i = cx.new(|cx| InputState::new(cx).default_value(name));
                        *input = Some(i);
                    }
                    input.clone()
                }
            };
            inp
        } else {
            None
        };

        let Some(input) = input_ent else {
            return div().into_any_element();
        };

        if self.inline_folder_sub.is_none() {
            let focus_handle = input.focus_handle(cx);
            window.focus(&focus_handle, cx);
            self.inline_folder_sub =
                Some(cx.on_blur(&focus_handle, window, move |this, window, cx| {
                    this.commit_inline_folder(cx);
                    window.focus(&this.focus_handle, cx);
                }));
        }

        let current_val = input.read(cx).text().to_string().trim().to_string();
        let (parent_id, except_id) = match &self.inline_folder {
            Some(InlineTunnelFolderState::Creating { parent_id, .. }) => (parent_id.clone(), None),
            Some(InlineTunnelFolderState::Renaming { id, .. }) => {
                let parent_id = tunnel_parent_id_of(&self.nodes, id).flatten();
                (parent_id, Some(id.clone()))
            }
            None => (None, None),
        };

        let is_duplicate = if !current_val.is_empty() {
            tunnel_folder_name_exists(
                &self.nodes,
                parent_id.as_deref(),
                &current_val,
                except_id.as_deref(),
            )
        } else {
            false
        };

        let border_color = if is_duplicate {
            rgb(t.error)
        } else {
            rgb(t.border_active)
        };

        let ap = tunnel_row_appearance(t, cx);
        div()
            .id("tunnel-inline-folder-container")
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .id("tunnel-inline-folder-row")
                    .h(ap.height)
                    .pl(px(8.0 + depth as f32 * 14.0))
                    .pr(ui_space_md(cx))
                    .flex()
                    .items_center()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(ui_space_xs(cx))
                            .child(
                            AppIcon::ChevronRight
                                .size(ap.icon_size)
                                .text_color(rgb(t.text_muted)),
                            )
                            .child(folder_tree_icon(
                                folder_icon_id,
                                is_expanded,
                                depth,
                                ap.icon_size,
                                t,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(velowork_ui::Input::new(&input).border_color(border_color))
                                    .on_key_down(cx.listener(
                                        move |this, e: &KeyDownEvent, window, cx| {
                                            if e.keystroke.key == "enter" {
                                                if !is_duplicate {
                                                    this.commit_inline_folder(cx);
                                                    if this.inline_folder.is_none() {
                                                        window.focus(&this.focus_handle, cx);
                                                    }
                                                }
                                                cx.stop_propagation();
                                            } else if e.keystroke.key == "escape" {
                                                this.cancel_inline_folder(cx);
                                                window.focus(&this.focus_handle, cx);
                                                cx.stop_propagation();
                                            }
                                        },
                                    )),
                            ),
                    ),
            )
            .when(is_duplicate, |container| {
                container.child(
                    div()
                        .my(px(2.0))
                        .ml(px(37.0 + depth as f32 * 14.0))
                        .mr(ui_space_md(cx))
                        .px(ui_space_md(cx))
                        .py(ui_space_xs(cx))
                        .bg(surface_bg_t(t.bg_secondary, &t))
                        .border_1()
                        .border_color(rgb(t.error))
                        .rounded(RADIUS_STD)
                        .child(
                            div()
                                .text_size(ui_text(11.0, cx))
                                .text_color(rgb(t.error))
                                .child(i18n!(cx, "tunnel.duplicate_folder_error")),
                        ),
                )
            })
            .into_any_element()
    }

    // ── 单节点渲染 ──────────────────────────────────────────────────────────

    fn render_single_node(
        &mut self,
        node: &TunnelNode,
        depth: usize,
        t: &ThemeColors,
        is_open: bool,
        is_selected: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            TunnelNode::Folder {
                id, name, children, ..
            } => {
                let count = children.len();
                let node_clone = node.clone();
                let fid_drag = id.clone();
                let fname_drag = name.clone();
                let t_border = t.border_active;
                let t_bg = t.bg_selection;
                let ap = tunnel_row_appearance(t, cx);
                let font_color = if is_selected { t.text_primary } else { t.text_secondary };

                let header = div()
                    .id(ElementId::Name(format!("tunnel-folder-{}", id).into()))
                    .h(ap.height)
                    .pl(px(8.0 + depth as f32 * 14.0))
                    .pr(ui_space_md(cx))
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .rounded(RADIUS_STD)
                    .border_1()
                    .border_color(with_alpha(0x00000000, 0.0))
                    .text_color(rgb(font_color))
                    .stateful_behavior(HoverBehavior {
                        hover_bg: surface_bg(t.bg_hover, cx),
                        hover_fg: Some(rgb(t.text_primary).into()),
                        ..Default::default()
                    })
                    .stateful_behavior(SelectedBehavior {
                        selected: is_selected,
                        bg: surface_bg(t.bg_selection, cx),
                        fg: None,
                    })
                    .when(is_selected, |d| {
                        let nid = id.clone();
                        let bounds_map = self.selected_node_bounds.clone();
                        d.border_color(rgb(t.border_active)).child(
                            canvas(
                                move |bounds, _, _| {
                                    bounds_map.borrow_mut().insert(nid, bounds);
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                    })
                    .on_drag(
                        TunnelDrag {
                            node_id: fid_drag.clone(),
                            node_name: fname_drag.clone(),
                            is_folder: true,
                        },
                        move |drag, position, _window, cx| {
                            let cursor_y = position.y;
                            let cursor_x = position.x;
                            cx.new(move |_| TunnelDragView {
                                name: drag.node_name.clone(),
                                is_folder: true,
                                cursor_offset_x: cursor_x,
                                cursor_offset_y: cursor_y,
                            })
                        },
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(ui_space_xs(cx))
                            .child(
                                (if is_open {
                                    AppIcon::ChevronDown
                                } else {
                                    AppIcon::ChevronRight
                                })
                                .size(ap.icon_size)
                                .text_color(rgb(t.text_muted)),
                            )
                            .child(folder_tree_icon(
                                &format!("tunnel-folder-{}", id),
                                is_open,
                                depth,
                                ap.icon_size,
                                &t,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(ap.font_size)
                                    .child(name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_muted))
                            .child(format!("{}", count)),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.handle_node_click(&node_clone, true, event, window, cx);
                        cx.stop_propagation();
                    }))
                    .drag_over::<TunnelDrag>(move |style, drag: &TunnelDrag, _, _| {
                        if drag.is_folder {
                            style.border_t_2().border_color(rgb(t_border))
                        } else {
                            style.bg(rgb(t_bg))
                        }
                    })
                    .on_drop(cx.listener({
                        let target_id = id.clone();
                        move |this, drag: &TunnelDrag, _window, cx| {
                            this.drop_node(drag, &target_id, true, cx);
                        }
                    }))
                    .on_mouse_down(MouseButton::Right, {
                        let node_clone = node.clone();
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.show_menu(
                                TunnelMenuTarget::Node(node_clone.clone()),
                                event.position,
                                window,
                                cx,
                            );
                        })
                    });

                let is_renaming_this = match &self.inline_folder {
                    Some(InlineTunnelFolderState::Renaming { id: target_id, .. }) => {
                        target_id == id
                    }
                    _ => false,
                };

                if is_renaming_this {
                    return self.render_inline_folder_input_row(
                        &format!("tunnel-folder-{}", id),
                        is_open,
                        depth,
                        &t,
                        window,
                        cx,
                    );
                }
                header.into_any_element()
            }
            TunnelNode::Tunnel { profile } => {
                let pid = profile.id.to_string();
                let node_clone = node.clone();
                let cid_drag = profile.id.clone();
                let cname_drag = profile.name.clone();
                let t_border = t.border_active;

                let ap = tunnel_row_appearance(t, cx);

                let is_renaming_tunnel = match &self.inline_tunnel {
                    Some(s) => s.id == profile.id,
                    None => false,
                };
                if is_renaming_tunnel {
                    return self.render_inline_tunnel_input_row(
                        profile,
                        depth,
                        &t,
                        window,
                        cx,
                    );
                }

                let info = self.engine.get_runtime_info(&profile.id);
                let status = info
                    .as_ref()
                    .map(|r| r.status.clone())
                    .unwrap_or(TunnelStatus::Stopped);
                let is_running = status == TunnelStatus::Running;
                let session_name = self.session_name(&profile.session_id, cx);

                let subtext = tunnel_subtext(profile);
                let tooltip_text = tunnel_tooltip(profile, &session_name, info.as_ref(), cx);

                let profile_toggle = profile.clone();

                // L/R/D badge icon
                let kind_icon = match &profile.kind {
                    TunnelKind::Local { .. } => AppIcon::TunnelL,
                    TunnelKind::Remote { .. } => AppIcon::TunnelR,
                    TunnelKind::Dynamic { .. } => AppIcon::TunnelD,
                };
                let kind_color = match &profile.kind {
                    TunnelKind::Local { .. } => t.success,
                    TunnelKind::Remote { .. } => t.warning,
                    TunnelKind::Dynamic { .. } => t.accent,
                };

                // Toggle color: green=running, yellow=error/reconnecting, default=stopped
                let toggle_color = match &status {
                    TunnelStatus::Running => t.success,
                    TunnelStatus::Error(_) | TunnelStatus::Reconnecting { .. } => t.warning,
                    TunnelStatus::Stopped => t.border_active,
                };

                let group_id = format!("tunnel-node-{}", pid);
                let font_color = if is_selected { t.text_primary } else { t.text_secondary };

                div()
                    .id(ElementId::Name(format!("tunnel-node-{}", pid).into()))
                    .group(group_id.clone())
                    .h(ap.height)
                    .pl(px(8.0 + depth as f32 * 14.0))
                    .pr(ui_space_md(cx))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(ui_space_xs(cx))
                    .cursor_pointer()
                    .rounded(RADIUS_STD)
                    .border_1()
                    .border_color(with_alpha(0x00000000, 0.0))
                    .text_color(rgb(font_color))
                    .tooltip(move |_, cx| {
                        cx.new(|_| Tooltip::new(tooltip_text.clone())).into()
                    })
                    .stateful_behavior(HoverBehavior {
                        hover_bg: surface_bg(t.bg_hover, cx),
                        hover_fg: Some(rgb(t.text_primary).into()),
                        ..Default::default()
                    })
                    .stateful_behavior(SelectedBehavior {
                        selected: is_selected,
                        bg: surface_bg(t.bg_selection, cx),
                        fg: None,
                    })
                    .when(is_selected, |d| {
                        let nid = pid.clone();
                        let bounds_map = self.selected_node_bounds.clone();
                        d.border_color(rgb(t.border_active)).child(
                            canvas(
                                move |bounds, _, _| {
                                    bounds_map.borrow_mut().insert(nid, bounds);
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                    })
                    .on_drag(
                        TunnelDrag {
                            node_id: cid_drag.clone(),
                            node_name: cname_drag.clone(),
                            is_folder: false,
                        },
                        move |drag, position, _window, cx| {
                            let cursor_y = position.y;
                            let cursor_x = position.x;
                            cx.new(move |_| TunnelDragView {
                                name: drag.node_name.clone(),
                                is_folder: false,
                                cursor_offset_x: cursor_x,
                                cursor_offset_y: cursor_y,
                            })
                        },
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(ui_space_xs(cx))
                            .items_center()
                            .overflow_hidden()
                            // Chevron placeholder for 1:1 alignment with folder row
                            .child(
                                div()
                                    .w(ap.icon_size)
                                    .h(ap.icon_size)
                                    .flex_shrink_0(),
                            )
                            // L/R/D badge icon (SVG)
                            .child(
                                kind_icon
                                    .size(ap.icon_size)
                                    .flex_shrink_0()
                                    .text_color(rgb(kind_color)),
                            )
                            // Name + Subtext container
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(ui_space_xs(cx))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .max_w_full()
                                            .text_size(ap.font_size)
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(profile.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_1()
                                            .min_w(px(0.0))
                                            .text_size(ui_text_xs(cx))
                                            .text_color(rgb(t.text_muted))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(subtext),
                                    ),
                            ),
                    )
                    // Toggle switch (hidden unless hovered when stopped; always visible when running)
                    .child(
                        div()
                            .id(ElementId::Name(format!("tunnel-toggle-{}", pid).into()))
                            .flex_shrink_0()
                            .cursor_pointer()
                            .when(!is_running, |d| {
                                d.opacity(0.0)
                                    .group_hover(group_id.clone(), |s| s.opacity(1.0))
                            })
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.toggle_tunnel(&profile_toggle, cx);
                                cx.stop_propagation();
                            }))
                            .child(
                                velowork_ui::Switch::new(format!("tunnel-sw-{}", pid))
                                    .checked(is_running)
                                    .active_color(toggle_color),
                            ),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.handle_node_click(&node_clone, false, event, window, cx);
                        cx.stop_propagation();
                    }))
                    .drag_over::<TunnelDrag>(move |style, drag: &TunnelDrag, _, _| {
                        if drag.is_folder {
                            style.border_b_2().border_color(rgb(t_border))
                        } else {
                            style.border_t_2().border_color(rgb(t_border))
                        }
                    })
                    .on_drop(cx.listener({
                        let target_id = pid.clone();
                        move |this, drag: &TunnelDrag, _window, cx| {
                            this.drop_node(drag, &target_id, false, cx);
                        }
                    }))
                    .on_mouse_down(MouseButton::Right, {
                        let node_clone = node.clone();
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.show_menu(
                                TunnelMenuTarget::Node(node_clone.clone()),
                                event.position,
                                window,
                                cx,
                            );
                        })
                    })
                    .into_any_element()
            }
        }
    }
}

impl Focusable for TunnelsPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for TunnelsPanel {
    fn metadata(&self, cx: &App) -> PanelInfo {
        PanelInfo::new(
            "tunnels",
            i18n!(cx, "tunnel.title"),
            AppIcon::Tunnel,
            PanelKind::Custom,
        )
    }

    fn custom_actions(&self, _cx: &App) -> Vec<PanelAction> {
        Vec::new()
    }

    fn focus_handle(&self, _cx: &App) -> Option<FocusHandle> {
        Some(self.focus_handle.clone())
    }
}

impl Render for TunnelsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        if self.show_tunnel_search && self.filter_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "tunnel.search_tooltip"))
            });
            let input_clone = input.clone();
            cx.subscribe(&input_clone, |_this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                cx.notify();
            })
            .detach();
            self.filter_input = Some(input);
        }

        let filter_text = self
            .filter_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string().to_lowercase())
            .unwrap_or_default();
        let force = !filter_text.is_empty();

        let roots = self.nodes.clone();
        let mut visible_order = Vec::new();
        collect_tunnel_visible_ids(&roots, &filter_text, force, &mut visible_order);
        self.tunnel_visible_order = visible_order;

        let tree_nodes = convert_tunnel_nodes(&roots);
        let mut expanded_keys = HashSet::new();
        collect_tunnel_expanded_keys(&roots, force, &mut expanded_keys);

        let is_panel_active = self.is_selection_active(window, cx);
        let active_selected_keys = if is_panel_active {
            self.selected_tunnel_ids.clone()
        } else {
            HashSet::new()
        };

        let t_color = t.clone();
        let filter_clone = filter_text.clone();
        let tree_widget = velowork_ui::tree::<Self, String, TunnelNode>("tunnel-tree")
            .nodes(tree_nodes)
            .expanded_keys(expanded_keys)
            .selected_keys(active_selected_keys)
            .render_before_children({
                let t_color = t_color.clone();
                move |this, parent_key, depth, window, cx| {
                    let parent_id = parent_key.map(|k| k.as_str());
                    let is_creating_here = match &this.inline_folder {
                        Some(InlineTunnelFolderState::Creating {
                            parent_id: target_pid,
                            ..
                        }) => target_pid.as_deref() == parent_id,
                        _ => false,
                    };
                    if is_creating_here {
                        return Some(this.render_inline_folder_input_row(
                            "tunnel-inline-folder-create",
                            false,
                            depth,
                            &t_color,
                            window,
                            cx,
                        ));
                    }
                    None
                }
            })
            .render_node({
                let t_color = t_color.clone();
                let filter_str = filter_clone.clone();
                move |this, tree_ctx, window, cx| {
                    let node = match tree_ctx.node.payload.as_ref() {
                        Some(n) => n,
                        None => return div().into_any_element(),
                    };
                    if !filter_str.is_empty() && !node_matches_tunnel(node, &filter_str) {
                        return div().into_any_element();
                    }
                    this.render_single_node(
                        node,
                        tree_ctx.depth,
                        &t_color,
                        tree_ctx.is_expanded,
                        tree_ctx.is_selected,
                        window,
                        cx,
                    )
                }
            });

        self.selected_node_bounds.borrow_mut().clear();
        let tree_children = vec![tree_widget.render(self, window, cx)];
        let is_empty = roots.is_empty();

        let add_tip = i18n!(cx, "tunnel.add");
        let folder_tip = i18n!(cx, "workspace.folder.create");
        let search_tip = i18n!(cx, "tunnel.search_tooltip");

        v_flex()
            .size_full()
            .flex_col()
            .min_h_0()
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !this.focus_handle.is_focused(window) {
                        window.focus(&this.focus_handle, cx);
                    }
                    this.focus_manager.update(cx, |fm, _| {
                        fm.request_focus(velowork_workspace::focus::FocusLayer::None);
                    });
                }),
            )
            .on_action(cx.listener(|this, _: &RenameActiveNode, _window, cx| {
                if this.inline_folder.is_none() {
                    if let Some(id) = this.selected_tunnel_ids.iter().next().cloned() {
                        this.rename_selected_node(&id, cx);
                    }
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.inline_folder.is_some() {
                    return;
                }
                let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                if cmd_or_ctrl && event.keystroke.key.as_str() == "f" {
                    this.show_tunnel_search = true;
                    let input = this.filter_input.get_or_insert_with(|| {
                        let input = cx.new(|cx| {
                            InputState::new(cx)
                                .placeholder(i18n!(cx, "tunnel.search_tooltip"))
                        });
                        let input_clone = input.clone();
                        cx.subscribe(
                            &input_clone,
                            |_this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                                cx.notify();
                            },
                        )
                        .detach();
                        input
                    });
                    input.update(cx, |inp, cx| {
                        inp.focus(window, cx);
                        inp.select_all(cx);
                    });
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }

                let panel_focused = this.focus_handle.is_focused(window);
                match event.keystroke.key.as_str() {
                    "down" | "up" => {
                        if !panel_focused {
                            return;
                        }
                        if this.selected_tunnel_ids.is_empty() {
                            if let Some(first_id) = this.tunnel_visible_order.first().cloned() {
                                this.selected_tunnel_ids.insert(first_id);
                                this.tunnel_focused_index = Some(0);
                                cx.notify();
                            }
                            cx.stop_propagation();
                            return;
                        }
                        this.move_focus(if event.keystroke.key == "down" { 1 } else { -1 }, cx);
                        cx.stop_propagation();
                    }
                    "right" => {
                        if !panel_focused {
                            return;
                        }
                        if let Some(id) = this.focused_node_id() {
                            if matches!(tunnel_find_node_ref(&this.nodes, &id), Some(TunnelNode::Folder { .. })) {
                                this.activate_focused(cx);
                            } else {
                                this.move_focus(1, cx);
                            }
                            cx.stop_propagation();
                        }
                    }
                    "left" => {
                        if !panel_focused {
                            return;
                        }
                        if let Some(id) = this.focused_node_id() {
                            if matches!(tunnel_find_node_ref(&this.nodes, &id), Some(TunnelNode::Folder { .. })) {
                                this.activate_focused(cx);
                            } else {
                                this.move_focus(-1, cx);
                            }
                            cx.stop_propagation();
                        }
                    }
                    "space" => {
                        if !panel_focused {
                            return;
                        }
                        if this.selected_tunnel_ids.is_empty() {
                            if let Some(first_id) = this.tunnel_visible_order.first().cloned() {
                                this.selected_tunnel_ids.insert(first_id);
                                this.tunnel_focused_index = Some(0);
                                cx.notify();
                            }
                            cx.stop_propagation();
                            return;
                        }
                        this.activate_focused(cx);
                        cx.stop_propagation();
                    }
                    "enter" => {
                        if !panel_focused || this.selected_tunnel_ids.is_empty() {
                            return;
                        }
                        this.enter_focused(cx);
                        cx.stop_propagation();
                    }
                    "escape" => {
                        if this.show_tunnel_search {
                            this.show_tunnel_search = false;
                            if let Some(ref input) = this.filter_input {
                                input.update(cx, |inp, cx| inp.set_value("", cx));
                            }
                            window.focus(&this.focus_handle, cx);
                            cx.stop_propagation();
                            cx.notify();
                            return;
                        }
                        if !this.selected_tunnel_ids.is_empty() {
                            this.clear_selection(cx);
                            cx.stop_propagation();
                        }
                    }
                    "delete" => {
                        if !this.selected_tunnel_ids.is_empty() {
                            let ids: Vec<String> =
                                this.selected_tunnel_ids.iter().cloned().collect();
                            let origin = this.focus_handle.clone();
                            let panel = this.focus_handle.clone();
                            let click_origin = ids.iter().find_map(|id| {
                                this.selected_node_bounds.borrow().get(id).map(|b| b.center())
                            }).or_else(|| {
                                this.selected_node_bounds.borrow().values().next().map(|b| b.center())
                            });
                            this.overlay_manager.update(cx, |om, cx| {
                                if let Some(pt) = click_origin {
                                    om.record_click_origin(pt);
                                }
                                om.request_tunnel_delete_confirm_with_origin(
                                    ids,
                                    Some(origin),
                                    Some(panel),
                                    cx,
                                );
                            });
                            cx.stop_propagation();
                        }
                    }
                    _ => {}
                }
            }))
            .child(
                h_flex()
                    .h(px(velowork_ui::tab_height(cx)))
                    .px(ui_space_xs(cx))
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .items_center()
                    .gap(ui_space_xs(cx))
                    .child(
                        velowork_ui::icon_button::icon_button("btn-add-tunnel", AppIcon::Plus, &t, cx)
                            .tooltip(move |_, cx| {
                                let tip = add_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_add_dialog(Some(window), cx);
                            })),
                    )
                    .child(
                        velowork_ui::icon_button::icon_button("btn-add-tunnel-folder", AppIcon::NewFolder, &t, cx)
                            .tooltip(move |_, cx| {
                                let tip = folder_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus_handle, cx);
                                this.inline_create_folder(None, cx);
                            })),
                    )
                    .child(div().w(px(1.0)).h(ICON_STD).bg(p.border_subtle))
                    .child(
                        velowork_ui::icon_button::icon_button("btn-tunnel-search", AppIcon::Search, &t, cx)
                            .when(self.show_tunnel_search, |b| b.bg(surface_bg_t(t.bg_hover, &t)))
                            .tooltip(move |_, cx| {
                                let tip = search_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_search(window, cx);
                            })),
                    ),
            )
            .when(self.show_tunnel_search, |d| {
                d.child(
                    div().px(ui_space_xs(cx)).py(ui_space_xs(cx)).when_some(self.filter_input.as_ref(), |this, inp| {
                        this.child(velowork_ui::Input::new(inp).search(true))
                    }),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("tunnel-tree-scroll")
                            .size_full()
                            .track_scroll(&self.tunnel_scroll_handle)
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .py(ui_space_xs(cx))
                            .px(ui_space_xs(cx))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    this.show_menu(TunnelMenuTarget::Root, event.position, window, cx);
                                }),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_selection(cx);
                                window.focus(&this.focus_handle, cx);
                            }))
                            .when(is_empty, |d| {
                                d.child(velowork_ui::empty_state::empty_state(i18n!(cx, "tunnel.no_tunnels"), &t, cx))
                            })
                            .children(tree_children)
                            .child(
                                div()
                                    .id("tunnel-tree-dropzone")
                                    .flex_1()
                                    .min_h(px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(ui_text_xs(cx))
                                    .text_color(rgb(t.text_muted))
                                    .drag_over::<TunnelDrag>(move |style, _, _, _| {
                                        style.bg(rgb(t.bg_hover))
                                    })
                                    .on_drop(cx.listener(
                                        move |this, drag: &TunnelDrag, _window, cx| {
                                            this.drop_node_to_root(drag, cx);
                                        },
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .left_0()
                            .child(Scrollbar::vertical(&self.tunnel_scroll_handle)),
                    ),
            )
            .when_some(self.context_menu.clone(), |d, menu| d.child(menu))
    }
}

// ── Tunnel tree drag-and-drop types ────────────────────────────────────────

/// Drag payload for a node (tunnel or folder) in the tunnel tree.
#[derive(Clone)]
pub struct TunnelDrag {
    pub node_id: String,
    pub node_name: String,
    pub is_folder: bool,
}

/// Drag preview for a tunnel node (the floating card that follows the cursor).
pub struct TunnelDragView {
    pub name: String,
    pub is_folder: bool,
    pub cursor_offset_x: Pixels,
    pub cursor_offset_y: Pixels,
}

impl Render for TunnelDragView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div().child(
            div()
                .absolute()
                .top(self.cursor_offset_y + ui_space_xs(cx))
                .left(self.cursor_offset_x)
                .px(ui_space_md(cx))
                .py(ui_space_xs(cx))
                .bg(rgb(t.bg_panel))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(RADIUS_STD)
                .shadow_lg()
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_primary))
                .flex()
                .items_center()
                .gap(ui_space_xs(cx))
                .child(
                (if self.is_folder {
                        AppIcon::Folder
                    } else {
                        AppIcon::Network
                    })
                    .size(px(12.0))
                    .text_color(rgb(t.text_muted)),
                )
                .child(self.name.clone()),
        )
    }
}

/// Whether a node (or any descendant) matches the filter text.
fn node_matches_tunnel(node: &TunnelNode, filter: &str) -> bool {
    match node {
        TunnelNode::Tunnel { profile } => {
            profile.name.to_lowercase().contains(filter)
                || profile
                    .kind
                    .display_summary()
                    .to_lowercase()
                    .contains(filter)
        }
        TunnelNode::Folder { name, children, .. } => {
            name.to_lowercase().contains(filter)
                || children.iter().any(|c| node_matches_tunnel(c, filter))
        }
    }
}

/// Collect the ids of all currently visible tunnel nodes in display order.
pub(crate) fn collect_tunnel_visible_ids(
    nodes: &[TunnelNode],
    filter: &str,
    force: bool,
    out: &mut Vec<String>,
) {
    let mut ordered: Vec<&TunnelNode> = nodes.iter().collect();
    ordered.sort_by(|a, b| {
        let a_folder = matches!(a, TunnelNode::Folder { .. });
        let b_folder = matches!(b, TunnelNode::Folder { .. });
        b_folder.cmp(&a_folder)
    });
    for node in ordered {
        if !force && !node_matches_tunnel(node, filter) {
            continue;
        }
        match node {
            TunnelNode::Folder {
                id,
                expanded,
                children,
                ..
            } => {
                out.push(id.clone());
                if *expanded || force {
                    collect_tunnel_visible_ids(children, filter, force, out);
                }
            }
            TunnelNode::Tunnel { profile } => {
                out.push(profile.id.clone());
            }
        }
    }
}

/// Register Tunnels panel to the right toolbar.
pub fn register_toolbar_panel(registry: &mut velowork_ui::dock::RightToolbarRegistry) {
    registry.register(velowork_ui::dock::ToolbarPanelSpec {
        id: "tunnels".to_string(),
        icon: velowork_ui::icon::AppIcon::Tunnel,
        title_key: "tunnel.title".to_string(),
        order: 30,
        is_visible: std::sync::Arc::new(|_cx| true),
        factory: std::sync::Arc::new(|ctx, _window, cx| {
            let app_ctx = ctx
                .downcast_ref::<super::AppPanelCreationContext>()
                .expect("AppPanelCreationContext required");
            let workspace = app_ctx.workspace.clone();
            let focus_manager = app_ctx.focus_manager.clone();
            let overlay_manager = app_ctx.overlay_manager.clone();
            let overlay_reg = app_ctx.overlay_registry.clone();
            let p = cx.new(|cx| {
                let mut tp = TunnelsPanel::new(workspace, focus_manager, overlay_manager, cx);
                tp.set_overlay_registry(overlay_reg);
                tp
            });

            velowork_ui::dock::AnyPanel::new(p)
        }),
    });
}

fn filter_tunnel_tree(nodes: &[TunnelNode], target_pid: Option<&str>, cx: &App) -> Vec<TunnelNode> {
    let pid_str = target_pid.unwrap_or("");
    let session_ids: std::collections::HashSet<String> = if let Some(store) = cx.try_global::<GlobalSessionStore>() {
        let tree = store.0.read(cx).tree_for_project(target_pid);
        let mut set = std::collections::HashSet::new();
        collect_session_ids_from_tree(tree, &mut set);
        set
    } else {
        std::collections::HashSet::new()
    };

    fn filter_list(nodes: &[TunnelNode], target_pid: &str, session_ids: &std::collections::HashSet<String>) -> Vec<TunnelNode> {
        let mut out = Vec::new();
        for node in nodes {
            match node {
                TunnelNode::Folder { id, name, project_id, expanded, children } => {
                    let sub = filter_list(children, target_pid, session_ids);
                    let pid_matches = match project_id {
                        Some(pid) => pid == target_pid || target_pid.is_empty() || target_pid == "default",
                        None => target_pid.is_empty() || target_pid == "default",
                    };
                    if pid_matches || !sub.is_empty() {
                        out.push(TunnelNode::Folder {
                            id: id.clone(),
                            name: name.clone(),
                            project_id: project_id.clone(),
                            expanded: *expanded,
                            children: sub,
                        });
                    }
                }
                TunnelNode::Tunnel { profile } => {
                    let pid_matches = match &profile.project_id {
                        Some(pid) => pid == target_pid || target_pid.is_empty() || target_pid == "default",
                        None => target_pid.is_empty() || target_pid == "default",
                    };
                    let session_matches = !profile.session_id.is_empty() && session_ids.contains(&profile.session_id);
                    if pid_matches || session_matches {
                        out.push(TunnelNode::Tunnel { profile: profile.clone() });
                    }
                }
            }
        }
        out
    }

    filter_list(nodes, pid_str, &session_ids)
}

fn collect_session_ids_from_tree(nodes: &[velowork_state::SessionTreeNode], set: &mut std::collections::HashSet<String>) {
    for node in nodes {
        match node {
            velowork_state::SessionTreeNode::Session { session } => {
                set.insert(session.id.clone());
            }
            velowork_state::SessionTreeNode::Folder { children, .. } => {
                collect_session_ids_from_tree(children, set);
            }
        }
    }
}

/// 格式化隧道树节点行内副文本（核心端口对，例如 `8080:80` 或 `1080`）
pub fn tunnel_subtext(profile: &TunnelProfile) -> String {
    match &profile.kind {
        TunnelKind::Local { local_bind, remote_target } => {
            let remote_port = remote_target
                .rsplit_once(':')
                .map(|(_, port)| port)
                .unwrap_or(remote_target.as_str());
            format!("{}:{}", local_bind.port(), remote_port)
        }
        TunnelKind::Remote { remote_bind, local_target } => {
            let remote_port = remote_bind
                .rsplit_once(':')
                .map(|(_, port)| port)
                .unwrap_or(remote_bind.as_str());
            format!("{}:{}", remote_port, local_target.port())
        }
        TunnelKind::Dynamic { local_bind } => {
            format!("{}", local_bind.port())
        }
    }
}

/// 格式化隧道多行结构化 Tooltip 提示
pub fn tunnel_tooltip(
    profile: &TunnelProfile,
    session_name: &str,
    info: Option<&TunnelRuntimeInfo>,
    cx: &App,
) -> String {
    let mut lines = Vec::new();

    // 1. 类型与名称
    let type_label = match &profile.kind {
        TunnelKind::Local { .. } => i18n!(cx, "tunnel.tooltip.type_local"),
        TunnelKind::Remote { .. } => i18n!(cx, "tunnel.tooltip.type_remote"),
        TunnelKind::Dynamic { .. } => i18n!(cx, "tunnel.tooltip.type_dynamic"),
    };
    lines.push(format!("[{}] {}", type_label, profile.name));

    // 2. 转发规则
    lines.push(format!(
        "{}: {}",
        i18n!(cx, "tunnel.tooltip.endpoints"),
        profile.kind.display_summary()
    ));

    // 3. 关联 SSH 会话
    let session_display = if session_name.is_empty() {
        i18n!(cx, "tunnel.no_session")
    } else {
        session_name.to_string()
    };
    lines.push(format!(
        "{}: {}",
        i18n!(cx, "tunnel.tooltip.associated_session"),
        session_display
    ));

    // 4. 运行状态与流量统计
    let status = info
        .as_ref()
        .map(|r| r.status.clone())
        .unwrap_or(TunnelStatus::Stopped);
    match &status {
        TunnelStatus::Running => {
            let rx_mb = (info.map(|r| r.rx_bytes).unwrap_or(0) as f64) / (1024.0 * 1024.0);
            let tx_mb = (info.map(|r| r.tx_bytes).unwrap_or(0) as f64) / (1024.0 * 1024.0);
            let conns = info.map(|r| r.active_connections).unwrap_or(0);
            lines.push(format!(
                "{}: {} (Rx {:.2} MB / Tx {:.2} MB, {} {})",
                i18n!(cx, "tunnel.tooltip.status"),
                i18n!(cx, "common.status.running"),
                rx_mb,
                tx_mb,
                conns,
                i18n!(cx, "tunnel.tooltip.active_connections")
            ));
        }
        TunnelStatus::Stopped => {
            lines.push(format!(
                "{}: {}",
                i18n!(cx, "tunnel.tooltip.status"),
                i18n!(cx, "common.status.stopped")
            ));
        }
        TunnelStatus::Reconnecting { attempt, backoff_secs } => {
            let detail = i18n!(cx, "tunnel.tooltip.reconnecting_detail")
                .replace("{attempt}", &attempt.to_string())
                .replace("{secs}", &backoff_secs.to_string());
            lines.push(format!(
                "{}: {} ({})",
                i18n!(cx, "tunnel.tooltip.status"),
                i18n!(cx, "tunnel.tooltip.status_reconnecting"),
                detail
            ));
        }
        TunnelStatus::Error(err) => {
            lines.push(format!(
                "{}: {} ({})",
                i18n!(cx, "tunnel.tooltip.status"),
                i18n!(cx, "tunnel.tooltip.status_error"),
                err
            ));
        }
    }

    // 5. 重连策略与自启
    let reconnect_desc = match profile.reconnect {
        ReconnectPolicy::InheritSession => i18n!(cx, "tunnel.tooltip.reconnect_inherit"),
        ReconnectPolicy::Always => i18n!(cx, "tunnel.tooltip.reconnect_always"),
        ReconnectPolicy::Never => i18n!(cx, "tunnel.tooltip.reconnect_never"),
    };
    let auto_start_desc = if profile.auto_start {
        format!(" ({})", i18n!(cx, "tunnel.tooltip.auto_start"))
    } else {
        String::new()
    };
    lines.push(format!(
        "{}: {}{}",
        i18n!(cx, "tunnel.tooltip.reconnect_policy"),
        reconnect_desc,
        auto_start_desc
    ));

    // 6. 备注说明（若有）
    if let Some(desc) = profile.description.as_deref() {
        let trimmed = desc.trim();
        if !trimmed.is_empty() {
            lines.push(format!("{}: {}", i18n!(cx, "tunnel.tooltip.notes"), trimmed));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{tunnel_subtext, tunnel_tooltip, ReconnectPolicy, TunnelKind, TunnelProfile, TunnelRuntimeInfo, TunnelStatus};
    use gpui::App;
    use velowork_i18n::{init_locale, Locale};

    #[core::prelude::v1::test]
    fn test_tunnel_subtext_formatting() {
        // Local tunnel: 127.0.0.1:8080 -> 10.0.0.1:80 => 8080:80
        let local_prof = TunnelProfile {
            id: "t1".into(),
            name: "Web Tunnel".into(),
            project_id: None,
            session_id: "s1".into(),
            enabled: true,
            auto_start: false,
            reconnect: ReconnectPolicy::InheritSession,
            kind: TunnelKind::Local {
                local_bind: "127.0.0.1:8080".parse().unwrap(),
                remote_target: "10.0.0.1:80".into(),
            },
            description: None,
        };
        assert_eq!(tunnel_subtext(&local_prof), "8080:80");

        // Remote tunnel: 0.0.0.0:9000 -> 127.0.0.1:3000 => 9000:3000
        let remote_prof = TunnelProfile {
            id: "t2".into(),
            name: "Remote API".into(),
            project_id: None,
            session_id: "s1".into(),
            enabled: true,
            auto_start: false,
            reconnect: ReconnectPolicy::Always,
            kind: TunnelKind::Remote {
                remote_bind: "0.0.0.0:9000".into(),
                local_target: "127.0.0.1:3000".parse().unwrap(),
            },
            description: None,
        };
        assert_eq!(tunnel_subtext(&remote_prof), "9000:3000");

        // Dynamic tunnel: 127.0.0.1:1080 => 1080
        let dynamic_prof = TunnelProfile {
            id: "t3".into(),
            name: "SOCKS5 Proxy".into(),
            project_id: None,
            session_id: "s1".into(),
            enabled: true,
            auto_start: true,
            reconnect: ReconnectPolicy::Never,
            kind: TunnelKind::Dynamic {
                local_bind: "127.0.0.1:1080".parse().unwrap(),
            },
            description: Some("Proxy notes".into()),
        };
        assert_eq!(tunnel_subtext(&dynamic_prof), "1080");
    }

    #[gpui::test]
    fn test_tunnel_tooltip_formatting(cx: &mut App) {
        init_locale(Locale::Zh, cx);

        let local_prof = TunnelProfile {
            id: "t1".into(),
            name: "MySQL 转发".into(),
            project_id: None,
            session_id: "s1".into(),
            enabled: true,
            auto_start: true,
            reconnect: ReconnectPolicy::InheritSession,
            kind: TunnelKind::Local {
                local_bind: "127.0.0.1:3306".parse().unwrap(),
                remote_target: "10.0.0.2:3306".into(),
            },
            description: Some("数据库隧道".into()),
        };

        let runtime_info = TunnelRuntimeInfo {
            status: TunnelStatus::Running,
            rx_bytes: 2 * 1024 * 1024,
            tx_bytes: 1 * 1024 * 1024,
            active_connections: 3,
            bound_port: None,
        };

        let tip = tunnel_tooltip(&local_prof, "生产跳板机", Some(&runtime_info), cx);
        assert!(tip.contains("[本地端口转发 (-L)] MySQL 转发"));
        assert!(tip.contains("转发规则: 127.0.0.1:3306 ➔ 10.0.0.2:3306"));
        assert!(tip.contains("关联会话: 生产跳板机"));
        assert!(tip.contains("运行状态: 运行中 (Rx 2.00 MB / Tx 1.00 MB, 3 活动连接)"));
        assert!(tip.contains("重连策略: 继承会话 (随会话自启)"));
        assert!(tip.contains("备注说明: 数据库隧道"));

        let recon_info = TunnelRuntimeInfo {
            status: TunnelStatus::Reconnecting { attempt: 2, backoff_secs: 5 },
            rx_bytes: 0,
            tx_bytes: 0,
            active_connections: 0,
            bound_port: None,
        };
        let tip_recon = tunnel_tooltip(&local_prof, "生产跳板机", Some(&recon_info), cx);
        assert!(tip_recon.contains("运行状态: 重连中 (第 2 次尝试，5s 后重试)"));
    }
}


