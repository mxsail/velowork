use crate::views::overlays::dialogs::service_dialog::ServiceDialogMode;
use crate::views::overlays::overlay_manager::OverlayManager;
use crate::views::overlays::menus::service_context_menu::{
    ServiceMenuRequest, ServiceMenuTarget,
};
use crate::views::panels::quick_commands_panel::send_command_to_focused_terminal;
use crate::keybindings::RenameActiveNode;
use gpui::prelude::*;
use gpui::*;
use std::collections::{HashMap, HashSet};
use velowork_i18n::{i18n, t as i18n_t};
use crate::views::panels::toast::ToastManager;
use velowork_state::{
    ServiceDefinition, ServiceNode, ServiceOp, ServiceStatus,
};
use velowork_terminal::TerminalsRegistry;
use velowork_ui::behavior::{
    HoverBehavior, SelectedBehavior, StatefulElementBehaviorExt,
};
use velowork_ui::dock::{Panel, PanelInfo, PanelKind};
use velowork_ui::icon::{folder_tree_icon, AppIcon};
use velowork_ui::icon_button::icon_button;
use velowork_ui::input::InputState;
use velowork_ui::theme::{surface_bg, surface_bg_t, theme, with_alpha, ThemeColors};
use velowork_ui::tokens::{
    ui_space_md, ui_space_sm, ui_space_xs, ui_text_md, ui_text_xs,
    ICON_LG, ICON_SM, ICON_STD, RADIUS_STD,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::SemanticPalette;
use velowork_ui::{h_flex, v_flex};
use velowork_workspace::focus::FocusManager;
use velowork_workspace::services::{
    service_find_node_ref, service_folder_name_exists, service_node_name_exists,
    service_parent_id_of,
};
use velowork_workspace::state::Workspace;
use velowork_workspace::stores::{GlobalServiceStore, GlobalSessionStore};

fn service_row_appearance(t: &ThemeColors, cx: &App) -> velowork_ui::ControlAppearance {
    velowork_ui::tree::tree_row_appearance(t, cx)
}

fn service_row_height(cx: &App) -> Pixels {
    velowork_ui::tree_row_height(cx)
}

#[derive(Clone)]
pub(crate) enum InlineServiceFolderState {
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

#[derive(Clone)]
pub(crate) struct InlineServiceState {
    pub id: String,
    pub original_name: String,
    pub input: Option<Entity<InputState>>,
}

fn convert_service_nodes(
    nodes: &[ServiceNode],
) -> Vec<velowork_ui::TreeNodeData<String, ServiceNode>> {
    nodes
        .iter()
        .map(|node| match node {
            ServiceNode::Folder {
                id, name, children, ..
            } => velowork_ui::TreeNodeData::new(id.clone(), name.clone(), true)
                .with_children(convert_service_nodes(children))
                .with_payload(node.clone()),
            ServiceNode::Service { def } => {
                velowork_ui::TreeNodeData::new(def.id.clone(), def.name.clone(), false)
                    .with_payload(node.clone())
            }
        })
        .collect()
}

fn collect_service_expanded_keys(
    nodes: &[ServiceNode],
    force_expand: bool,
    out: &mut HashSet<String>,
) {
    for node in nodes {
        if let ServiceNode::Folder {
            id,
            expanded,
            children,
            ..
        } = node
        {
            if force_expand || *expanded {
                out.insert(id.clone());
            }
            collect_service_expanded_keys(children, force_expand, out);
        }
    }
}

fn collect_service_visible_ids(
    nodes: &[ServiceNode],
    filter: &str,
    force_expand: bool,
    out: &mut Vec<String>,
) {
    for node in nodes {
        match node {
            ServiceNode::Folder {
                id,
                expanded,
                children,
                ..
            } => {
                let matches_self = filter.is_empty() || node_matches_service(node, filter);
                let matches_child = children_match_service(children, filter);
                if matches_self || matches_child {
                    out.push(id.clone());
                    if force_expand || *expanded || !filter.is_empty() {
                        collect_service_visible_ids(children, filter, force_expand, out);
                    }
                }
            }
            ServiceNode::Service { def } => {
                if filter.is_empty() || node_matches_service(node, filter) {
                    out.push(def.id.clone());
                }
            }
        }
    }
}

fn node_matches_service(node: &ServiceNode, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let lower = filter.to_lowercase();
    match node {
        ServiceNode::Folder { name, .. } => name.to_lowercase().contains(&lower),
        ServiceNode::Service { def } => {
            def.name.to_lowercase().contains(&lower)
                || def
                    .session_id
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&lower)
                || def.alive_command.to_lowercase().contains(&lower)
        }
    }
}

fn children_match_service(nodes: &[ServiceNode], filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    nodes.iter().any(|n| match n {
        ServiceNode::Folder { children, .. } => {
            node_matches_service(n, filter) || children_match_service(children, filter)
        }
        ServiceNode::Service { .. } => node_matches_service(n, filter),
    })
}

#[derive(Clone)]
pub struct ServiceDrag {
    pub node_id: String,
    pub label: String,
    pub is_folder: bool,
}

pub struct ServiceDragView {
    pub label: String,
    pub is_folder: bool,
}

impl Render for ServiceDragView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let icon_el = if self.is_folder {
            AppIcon::Folder
        } else {
            AppIcon::SquareActivity
        };
        h_flex()
            .gap(ui_space_sm(cx))
            .px(ui_space_md(cx))
            .py(ui_space_xs(cx))
            .bg(surface_bg_t(t.bg_secondary, &t))
            .border_1()
            .border_color(p.border_subtle)
            .rounded(RADIUS_STD)
            .shadow_md()
            .child(icon_el.size(ICON_STD).text_color(p.text_primary))
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(p.text_primary)
                    .child(self.label.clone()),
            )
    }
}

pub struct ServiceMonitorPanel {
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,
    terminals: TerminalsRegistry,
    overlay_manager: Entity<OverlayManager>,
    focus_handle: FocusHandle,
    nodes: Vec<ServiceNode>,
    show_service_search: bool,
    filter_input: Option<Entity<InputState>>,
    selected_service_ids: HashSet<String>,
    service_selection_anchor: Option<String>,
    service_focused_index: Option<usize>,
    service_visible_order: Vec<String>,
    inline_folder: Option<InlineServiceFolderState>,
    inline_folder_sub: Option<Subscription>,
    inline_service: Option<InlineServiceState>,
    inline_service_sub: Option<Subscription>,
    service_scroll_handle: ScrollHandle,
    operating_services: HashMap<String, ServiceOp>,
    _subscription: Option<Subscription>,
    selected_node_bounds: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Bounds<Pixels>>>>,
}

impl ServiceMonitorPanel {
    pub fn new(
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        terminals: TerminalsRegistry,
        overlay_manager: Entity<OverlayManager>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self {
            workspace,
            focus_manager,
            terminals,
            overlay_manager,
            focus_handle: cx.focus_handle(),
            nodes: Vec::new(),
            show_service_search: false,
            filter_input: None,
            selected_service_ids: HashSet::new(),
            service_selection_anchor: None,
            service_focused_index: None,
            service_visible_order: Vec::new(),
            inline_folder: None,
            inline_folder_sub: None,
            inline_service: None,
            inline_service_sub: None,
            service_scroll_handle: ScrollHandle::new(),
            operating_services: HashMap::new(),
            _subscription: None,
            selected_node_bounds: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
        };

        panel.reload_nodes(cx);
        panel.subscribe_store(cx);

        cx.observe(&panel.workspace, |this: &mut Self, _ws, cx| {
            this.reload_nodes(cx);
            cx.notify();
        })
        .detach();

        cx.observe(&panel.focus_manager, |this: &mut Self, _fm, cx| {
            this.reload_nodes(cx);
            cx.notify();
        })
        .detach();

        cx.observe(&panel.overlay_manager, |_this: &mut Self, _om, cx| {
            cx.notify();
        })
        .detach();

        panel.register_overlay_callbacks(cx);
        panel
    }

    pub fn is_selection_active(&self, window: &Window, cx: &App) -> bool {
        let is_focused = self.focus_handle.is_focused(window);
        let om = self.overlay_manager.read(cx);
        let has_my_context_menu = om.has_service_context_menu();
        let has_my_modal = om.active_modal_belongs_to(&self.focus_handle);
        is_focused || has_my_context_menu || has_my_modal
    }

    fn register_overlay_callbacks(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity();
        self.overlay_manager.update(cx, |om, _cx| {
            let entity1 = entity.clone();
            om.on_service_create_folder = Some(std::sync::Arc::new(move |parent_id, cx| {
                entity1.update(cx, |this, cx| this.inline_create_folder(parent_id, cx));
            }));

            let entity2 = entity.clone();
            om.on_service_rename_folder = Some(std::sync::Arc::new(move |id, name, cx| {
                entity2.update(cx, |this, cx| this.inline_rename_folder(&id, &name, cx));
            }));

            let entity3 = entity.clone();
            om.on_service_rename = Some(std::sync::Arc::new(move |id, name, cx| {
                entity3.update(cx, |this, cx| this.inline_rename_service(&id, &name, cx));
            }));
        });
    }

    fn active_project_id(&self, cx: &App) -> Option<String> {
        let raw = self.focus_manager.read(cx).active_project_id().cloned();
        if let Some(ref pid) = raw {
            if self.workspace.read(cx).project(pid).is_some() {
                return raw;
            }
        }
        self.workspace.read(cx).projects().first().map(|p| p.id.clone())
    }

    fn reload_nodes(&mut self, cx: &App) {
        let active_pid = self.active_project_id(cx);
        let all_nodes = cx
            .try_global::<GlobalServiceStore>()
            .map(|s| s.0.read(cx).nodes().to_vec())
            .unwrap_or_default();
        let old_selected = self.selected_service_ids.clone();
        self.nodes = filter_service_tree(&all_nodes, active_pid.as_deref(), cx);
        self.selected_service_ids
            .retain(|id| service_find_node_ref(&self.nodes, id).is_some());
        if self.selected_service_ids.is_empty() && !old_selected.is_empty() {
            if let Some(deleted_id) = old_selected.iter().next() {
                if let Some(pos) = self.service_visible_order.iter().position(|id| id == deleted_id) {
                    let remaining_visible: Vec<String> = self.service_visible_order
                        .iter()
                        .filter(|id| !old_selected.contains(*id) && service_find_node_ref(&self.nodes, id).is_some())
                        .cloned()
                        .collect();
                    if !remaining_visible.is_empty() {
                        let next_idx = if pos < remaining_visible.len() {
                            pos
                        } else {
                            remaining_visible.len() - 1
                        };
                        let next_id = remaining_visible[next_idx].clone();
                        self.selected_service_ids.insert(next_id);
                        self.service_focused_index = Some(next_idx);
                    }
                }
            }
        }
    }

    fn subscribe_store(&mut self, cx: &mut Context<Self>) {
        if let Some(store) = cx.try_global::<GlobalServiceStore>() {
            let store_entity = store.0.clone();
            self._subscription = Some(cx.observe(&store_entity, |this, _, cx| {
                this.reload_nodes(cx);
                cx.notify();
            }));
        }
        if let Some(engine) = cx.try_global::<velowork_terminal::GlobalServiceMonitorEngine>() {
            let engine_entity = engine.0.clone();
            cx.observe(&engine_entity, |_, _, cx| {
                cx.notify();
            })
            .detach();
        }
    }

    fn open_dialog(
        &self,
        mode: ServiceDialogMode,
        initial: Option<ServiceDefinition>,
        cx: &mut Context<Self>,
    ) {
        let origin = Some(self.focus_handle.clone());
        self.overlay_manager
            .update(cx, |om, cx| om.show_service_dialog_with_origin(mode, initial, origin.clone(), origin, cx));
    }

    fn show_menu(
        &mut self,
        target: ServiceMenuTarget,
        position: Point<Pixels>,
        status: Option<ServiceStatus>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ServiceMenuTarget::Node(ref node) = target {
            let id = node.id().to_string();
            if !self.selected_service_ids.contains(&id) {
                self.selected_service_ids.clear();
                self.selected_service_ids.insert(id.clone());
                self.service_selection_anchor = Some(id);
                window.focus(&self.focus_handle, cx);
            }
        }

        let req = ServiceMenuRequest {
            position,
            target,
            selected_ids: self.selected_service_ids.iter().cloned().collect(),
            status,
            ops_only: false,
        };
        let terms = self.terminals.clone();
        let origin = Some(self.focus_handle.clone());
        self.overlay_manager
            .update(cx, |om, cx| om.show_service_context_menu_with_origin(req, terms, origin, window, cx));
    }

    fn status_color(status: &ServiceStatus, p: &SemanticPalette) -> Hsla {
        match status {
            ServiceStatus::Running => p.status_success,
            ServiceStatus::Stopped => p.status_error,
            ServiceStatus::Unknown => p.text_muted,
            _ => p.status_warning,
        }
    }

    fn runtime_status(&self, def: &ServiceDefinition, cx: &App) -> ServiceStatus {
        if !def.monitor_enabled {
            return ServiceStatus::Unknown;
        }
        if let Some(engine) = cx.try_global::<velowork_terminal::GlobalServiceMonitorEngine>() {
            if let Some(runtime) = engine.0.read(cx).runtime_for(&def.id) {
                return runtime.status.clone();
            }
        }
        ServiceStatus::Unknown
    }

    fn select_single_node(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected_service_ids.clear();
        self.selected_service_ids.insert(id.to_string());
        self.service_selection_anchor = Some(id.to_string());
        self.service_focused_index = self.current_focus_index();
        cx.notify();
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected_service_ids.clear();
        self.service_selection_anchor = None;
        self.service_focused_index = None;
        cx.notify();
    }

    fn current_focus_index(&self) -> Option<usize> {
        self.service_selection_anchor
            .as_ref()
            .and_then(|id| self.service_visible_order.iter().position(|x| x == id))
    }

    fn focused_node_id(&self) -> Option<String> {
        self.service_selection_anchor
            .clone()
            .or_else(|| self.selected_service_ids.iter().next().cloned())
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.selected_service_ids.is_empty() || self.service_visible_order.is_empty() {
            return;
        }
        let idx = self.current_focus_index().unwrap_or(0);
        let new_idx = if delta > 0 {
            (idx + delta as usize).min(self.service_visible_order.len() - 1)
        } else {
            idx.saturating_sub((-delta) as usize)
        };
        let new_id = self.service_visible_order[new_idx].clone();
        self.selected_service_ids.clear();
        self.selected_service_ids.insert(new_id.clone());
        self.service_selection_anchor = Some(new_id.clone());
        self.service_focused_index = Some(new_idx);
        self.scroll_node_into_view(&new_id, cx);
        cx.notify();
    }

    fn scroll_node_into_view(&self, id: &str, cx: &App) {
        let Some(idx) = self.service_visible_order.iter().position(|x| x == id) else {
            return;
        };
        let row_h: Pixels = service_row_height(cx);
        let pad_top: Pixels = ui_space_xs(cx);
        let top = pad_top + row_h * idx as f32;
        let bottom = top + row_h;

        let viewport_h = self.service_scroll_handle.bounds().size.height;
        if viewport_h == px(0.0) {
            return;
        }
        let cur = self.service_scroll_handle.offset().y;
        let row_top_vp = top + cur;
        let row_bottom_vp = bottom + cur;

        let mut new_off = if row_top_vp < px(0.0) {
            -top
        } else if row_bottom_vp > viewport_h {
            viewport_h - bottom
        } else {
            return;
        };
        let max_off = self.service_scroll_handle.max_offset().y;
        new_off = new_off.max(-max_off).min(px(0.0));
        self.service_scroll_handle
            .set_offset(Point::new(px(0.0), new_off));
    }

    fn activate_focused(&mut self, cx: &mut Context<Self>) {
        if self.selected_service_ids.is_empty() {
            return;
        }
        let id = match self.focused_node_id() {
            Some(id) => id,
            None => return,
        };
        if matches!(
            service_find_node_ref(&self.nodes, &id),
            Some(ServiceNode::Folder { .. })
        ) {
            if let Some(entity) = cx.try_global::<GlobalServiceStore>().map(|s| s.0.clone()) {
                entity.update(cx, |s, cx| s.toggle_folder(&id, cx));
            }
        }
    }

    fn enter_focused(&mut self, cx: &mut Context<Self>) {
        if self.selected_service_ids.is_empty() {
            return;
        }
        let id = match self.focused_node_id() {
            Some(id) => id,
            None => return,
        };
        match service_find_node_ref(&self.nodes, &id).cloned() {
            Some(ServiceNode::Folder { .. }) => {
                if let Some(entity) = cx.try_global::<GlobalServiceStore>().map(|s| s.0.clone()) {
                    entity.update(cx, |s, cx| s.toggle_folder(&id, cx));
                }
            }
            Some(ServiceNode::Service { def }) => {
                self.open_dialog(ServiceDialogMode::Edit, Some(def), cx);
            }
            None => {}
        }
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_service_search = !self.show_service_search;
        if !self.show_service_search {
            self.filter_input = None;
        } else {
            let input = self.filter_input.get_or_insert_with(|| {
                let input = cx.new(|cx| {
                    InputState::new(cx)
                        .placeholder(i18n!(cx, "service.search_tooltip"))
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

    /// 根据当前选中项计算新建目录的父节点：
    /// - 选中单个目录 → 在该目录下新建
    /// - 选中服务 → 在该服务所属目录下新建
    /// - 未选中或选中多个 → 在根目录新建
    fn selected_folder_parent(&self, cx: &App) -> Option<String> {
        let Some(store) = cx.try_global::<GlobalServiceStore>().map(|s| s.0.clone()) else {
            return None;
        };
        let tree = store.read(cx).nodes().to_vec();
        let selected: Vec<String> = self.selected_service_ids.iter().cloned().collect();
        if selected.len() != 1 {
            return None;
        }
        let id = &selected[0];
        match service_find_node_ref(&tree, id) {
            Some(ServiceNode::Folder { .. }) => Some(id.clone()),
            Some(ServiceNode::Service { .. }) => service_parent_id_of(&tree, id).flatten(),
            None => None,
        }
    }

    fn inline_create_folder(&mut self, parent_id: Option<String>, cx: &mut Context<Self>) {
        let placeholder = i18n!(cx, "service.new_folder");
        self.inline_folder = Some(InlineServiceFolderState::Creating {
            parent_id,
            input: None,
            placeholder,
        });
        cx.notify();
    }

    fn inline_rename_folder(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        self.inline_folder = Some(InlineServiceFolderState::Renaming {
            id: id.to_string(),
            original_name: name.to_string(),
            input: None,
        });
        cx.notify();
    }

    fn inline_rename_service(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        self.inline_service = Some(InlineServiceState {
            id: id.to_string(),
            original_name: name.to_string(),
            input: None,
        });
        cx.notify();
    }

    pub fn commit_inline_folder(&mut self, cx: &mut Context<Self>) {
        self.inline_folder_sub = None;
        if let Some(state) = self.inline_folder.take() {
            match state {
                InlineServiceFolderState::Creating { parent_id, input, placeholder } => {
                    let val = input
                        .as_ref()
                        .map(|i| i.read(cx).text().to_string().trim().to_string())
                        .unwrap_or_default();
                    if !val.is_empty() {
                        if service_folder_name_exists(&self.nodes, parent_id.as_deref(), &val, None) {
                            self.inline_folder =
                                Some(InlineServiceFolderState::Creating { parent_id, input, placeholder });
                            cx.notify();
                            return;
                        }
                        let active_pid = self.active_project_id(cx);
                        if let Some(entity) = cx.try_global::<GlobalServiceStore>().map(|s| s.0.clone()) {
                            let new_id = entity
                                .update(cx, |s, cx| s.add_folder_for_project(parent_id.as_deref(), &val, active_pid.as_deref(), cx));
                            self.reload_nodes(cx);
                            if let Some(pid) = &parent_id {
                                self.selected_service_ids.remove(pid);
                            }
                            self.selected_service_ids.insert(new_id.clone());
                            self.service_selection_anchor = Some(new_id);
                        }
                    }
                }
                InlineServiceFolderState::Renaming {
                    id,
                    original_name,
                    input,
                } => {
                    let val = input
                        .as_ref()
                        .map(|i| i.read(cx).text().to_string().trim().to_string())
                        .unwrap_or_default();
                    if !val.is_empty() && val != original_name {
                        let parent_id = service_parent_id_of(&self.nodes, &id).flatten();
                        if service_folder_name_exists(
                            &self.nodes,
                            parent_id.as_deref(),
                            &val,
                            Some(&id),
                        ) {
                            self.inline_folder = Some(InlineServiceFolderState::Renaming {
                                id,
                                original_name,
                                input,
                            });
                            cx.notify();
                            return;
                        }
                        if let Some(entity) = cx.try_global::<GlobalServiceStore>().map(|s| s.0.clone()) {
                            entity.update(cx, |s, cx| s.rename_node(&id, &val, cx));
                            self.reload_nodes(cx);
                        }
                    }
                }
            }
        }
        cx.notify();
    }

    pub fn commit_inline_service(&mut self, cx: &mut Context<Self>) {
        self.inline_service_sub = None;
        if let Some(state) = self.inline_service.take() {
            let val = state
                .input
                .as_ref()
                .map(|i| i.read(cx).text().to_string().trim().to_string())
                .unwrap_or_default();
            if !val.is_empty() && val != state.original_name {
                let parent_id = service_parent_id_of(&self.nodes, &state.id).flatten();
                if service_node_name_exists(&self.nodes, parent_id.as_deref(), &val, Some(&state.id)) {
                    self.inline_service = Some(state);
                    cx.notify();
                    return;
                }
                if let Some(entity) = cx.try_global::<GlobalServiceStore>().map(|s| s.0.clone()) {
                    entity.update(cx, |s, cx| s.rename_node(&state.id, &val, cx));
                    self.reload_nodes(cx);
                }
            }
        }
        cx.notify();
    }

    fn drop_node(&mut self, drag: &ServiceDrag, target_id: &str, target_is_folder: bool, cx: &mut Context<Self>) {
        if drag.node_id == target_id {
            return;
        }
        if let Some(entity) = cx.try_global::<GlobalServiceStore>().map(|s| s.0.clone()) {
            let node_id = drag.node_id.clone();
            let target_id_str = target_id.to_string();
            entity.update(cx, |s, cx| {
                s.move_node(&node_id, Some(&target_id_str), target_is_folder, cx);
            });
            self.reload_nodes(cx);
        }
    }

    fn drop_node_to_root(&mut self, drag: &ServiceDrag, cx: &mut Context<Self>) {
        if drag.node_id.is_empty() {
            return;
        }
        if let Some(entity) = cx.try_global::<GlobalServiceStore>().map(|s| s.0.clone()) {
            let id = drag.node_id.clone();
            entity.update(cx, |s, cx| s.move_node(&id, None, false, cx));
            self.reload_nodes(cx);
        }
    }

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
                InlineServiceFolderState::Creating { input, placeholder, .. } => {
                    if input.is_none() {
                        let ph = placeholder.clone();
                        let i = cx.new(|cx| InputState::new(cx).placeholder(ph));
                        *input = Some(i);
                    }
                    input.clone()
                }
                InlineServiceFolderState::Renaming { input, original_name, .. } => {
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
            self.inline_folder_sub = Some(cx.on_blur(&focus_handle, window, move |this, window, cx| {
                this.commit_inline_folder(cx);
                window.focus(&this.focus_handle, cx);
            }));
        }

        let ap = service_row_appearance(t, cx);
        div()
            .id("service-inline-folder-container")
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .id("service-inline-folder-row")
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
                            .child(AppIcon::ChevronRight.size(ap.icon_size).text_color(rgb(t.text_secondary)))
                            .child(folder_tree_icon(folder_icon_id, is_expanded, depth, ap.icon_size, t))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(velowork_ui::Input::new(&input))
                                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                        if e.keystroke.key == "enter" {
                                            this.commit_inline_folder(cx);
                                            window.focus(&this.focus_handle, cx);
                                            cx.stop_propagation();
                                        } else if e.keystroke.key == "escape" {
                                            this.inline_folder = None;
                                            this.inline_folder_sub = None;
                                            window.focus(&this.focus_handle, cx);
                                            cx.notify();
                                            cx.stop_propagation();
                                        }
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_inline_service_input_row(
        &mut self,
        depth: usize,
        t: &ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_ent = if let Some(ref mut state) = self.inline_service {
            if state.input.is_none() {
                let name = state.original_name.clone();
                let i = cx.new(|cx| InputState::new(cx).default_value(name));
                state.input = Some(i);
            }
            state.input.clone()
        } else {
            None
        };

        let Some(input) = input_ent else {
            return div().into_any_element();
        };

        if self.inline_service_sub.is_none() {
            let focus_handle = input.focus_handle(cx);
            window.focus(&focus_handle, cx);
            self.inline_service_sub = Some(cx.on_blur(&focus_handle, window, move |this, window, cx| {
                this.commit_inline_service(cx);
                window.focus(&this.focus_handle, cx);
            }));
        }

        let ap = service_row_appearance(t, cx);
        div()
            .id("service-inline-rename-container")
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .id("service-inline-rename-row")
                    .h(ap.height)
                    .pl(px(8.0 + depth as f32 * 14.0 + 16.0))
                    .pr(ui_space_md(cx))
                    .flex()
                    .items_center()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(ui_space_xs(cx))
                            .child(AppIcon::SquareActivity.size(ap.icon_size).text_color(rgb(t.text_primary)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(velowork_ui::Input::new(&input))
                                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                        if e.keystroke.key == "enter" {
                                            this.commit_inline_service(cx);
                                            window.focus(&this.focus_handle, cx);
                                            cx.stop_propagation();
                                        } else if e.keystroke.key == "escape" {
                                            this.inline_service = None;
                                            this.inline_service_sub = None;
                                            window.focus(&this.focus_handle, cx);
                                            cx.notify();
                                            cx.stop_propagation();
                                        }
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_single_node(
        &mut self,
        node: &ServiceNode,
        depth: usize,
        t: &ThemeColors,
        is_expanded: bool,
        is_selected: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = SemanticPalette::from_theme(t);
        let ap = service_row_appearance(t, cx);
        let node_id = node.id().to_string();
        let _node_name = node.name().to_string();
        let node_clone = node.clone();
        let border_active = t.border_active;
        let bg_hover = t.bg_hover;

        match node {
            ServiceNode::Folder { id, name, .. } => {
                let folder_id = id.clone();
                let folder_name = name.clone();
                let folder_id_toggle = id.clone();
                let node_id_click = id.clone();

                let font_color = if is_selected { p.text_primary } else { p.text_secondary };

                div()
                    .id(SharedString::from(format!("svc-folder-{}", id)))
                    .h(ap.height)
                    .pl(px(8.0 + depth as f32 * 14.0))
                    .pr(ui_space_sm(cx))
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .rounded(RADIUS_STD)
                    .border_1()
                    .border_color(with_alpha(0x00000000, 0.0))
                    .text_color(font_color)
                    .stateful_behavior(HoverBehavior {
                        hover_bg: surface_bg(t.bg_hover, cx),
                        hover_fg: Some(p.text_primary),
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
                    .on_click(cx.listener(move |this, _event: &ClickEvent, window, cx| {
                        window.focus(&this.focus_handle, cx);
                        this.select_single_node(&node_id_click, cx);
                        if let Some(entity) = cx.try_global::<GlobalServiceStore>().map(|s| s.0.clone()) {
                            entity.update(cx, |s, cx| s.toggle_folder(&folder_id_toggle, cx));
                        }
                        cx.stop_propagation();
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.focus_handle, cx);
                            this.select_single_node(&node_id, cx);
                            this.show_menu(ServiceMenuTarget::Node(node_clone.clone()), event.position, None, window, cx);
                        }),
                    )
                    .on_drag(
                        ServiceDrag {
                            node_id: folder_id.clone(),
                            label: folder_name.clone(),
                            is_folder: true,
                        },
                        move |drag, _position, _window, cx| {
                            cx.new(|_| ServiceDragView {
                                label: drag.label.clone(),
                                is_folder: true,
                            })
                        },
                    )
                    .drag_over::<ServiceDrag>(move |style, drag: &ServiceDrag, _, _| {
                        if drag.is_folder {
                            style.border_t_2().border_color(rgb(border_active))
                        } else {
                            style.bg(rgb(bg_hover))
                        }
                    })
                    .on_drop(cx.listener(move |this, drag: &ServiceDrag, _, cx| {
                        this.drop_node(drag, &folder_id, true, cx);
                    }))
                    .child(
                        h_flex()
                            .gap(ui_space_xs(cx))
                            .items_center()
                            .child(
                                (if is_expanded {
                                    AppIcon::ChevronDown
                                } else {
                                    AppIcon::ChevronRight
                                })
                                .size(ap.icon_size)
                                .text_color(rgb(t.text_muted)),
                            )
                            .child(folder_tree_icon(id, is_expanded, depth, ap.icon_size, t))
                            .child(
                                div()
                                    .text_size(ap.font_size)
                                    .child(name.clone()),
                            ),
                    )
                    .into_any_element()
            }
            ServiceNode::Service { def } => {
                let border_active = t.border_active;
                let service_id = def.id.clone();
                let service_id_menu = def.id.clone();
                let service_name = def.name.clone();
                let status = self.runtime_status(def, cx);
                let color = Self::status_color(&status, &p);
                let node_id_click = service_id.clone();
                let def_edit = def.clone();
                let def_action = def.clone();

                let session_tag = match &def.session_id {
                    Some(sid) => {
                        let store = cx.try_global::<GlobalSessionStore>().map(|s| s.0.read(cx));
                        store
                            .as_ref()
                            .and_then(|st| st.find_session(sid))
                            .map(|s| s.name.clone())
                            .unwrap_or_else(|| sid.clone())
                    }
                    None => i18n!(cx, "service.session_current"),
                };

                let group_id = SharedString::from(format!("svc-row-{}", service_id));

                let show_start = status != ServiceStatus::Running;
                let show_stop = status != ServiceStatus::Stopped;
                let show_restart = status != ServiceStatus::Stopped;

                let start_btn = if show_start {
                    self.action_icon_btn(
                        AppIcon::Play,
                        i18n!(cx, "service.start"),
                        ServiceOp::Start,
                        &def_action,
                        t,
                        &p,
                        cx,
                    )
                } else {
                    None
                };

                let stop_btn = if show_stop {
                    self.action_icon_btn(
                        AppIcon::Stop,
                        i18n!(cx, "service.stop"),
                        ServiceOp::Stop,
                        &def_action,
                        t,
                        &p,
                        cx,
                    )
                } else {
                    None
                };

                let restart_btn = if show_restart {
                    self.action_icon_btn(
                        AppIcon::Refresh,
                        i18n!(cx, "service.restart"),
                        ServiceOp::Restart,
                        &def_action,
                        t,
                        &p,
                        cx,
                    )
                } else {
                    None
                };

                let has_actions = start_btn.is_some() || stop_btn.is_some() || restart_btn.is_some();
                let font_color = if is_selected { p.text_primary } else { p.text_secondary };

                div()
                    .id(SharedString::from(format!("svc-item-{}", service_id)))
                    .group(group_id.clone())
                    .h(ap.height)
                    .pl(px(8.0 + depth as f32 * 14.0 + 12.0))
                    .pr(ui_space_sm(cx))
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .rounded(RADIUS_STD)
                    .border_1()
                    .border_color(with_alpha(0x00000000, 0.0))
                    .text_color(font_color)
                    .stateful_behavior(HoverBehavior {
                        hover_bg: surface_bg(t.bg_hover, cx),
                        hover_fg: Some(p.text_primary),
                        ..Default::default()
                    })
                    .stateful_behavior(SelectedBehavior {
                        selected: is_selected,
                        bg: surface_bg(t.bg_selection, cx),
                        fg: None,
                    })
                    .when(is_selected, |d| {
                        let nid = service_id.clone();
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
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        window.focus(&this.focus_handle, cx);
                        if event.click_count() == 2 {
                            this.open_dialog(ServiceDialogMode::Edit, Some(def_edit.clone()), cx);
                        } else {
                            this.select_single_node(&node_id_click, cx);
                        }
                        cx.stop_propagation();
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.focus_handle, cx);
                            this.select_single_node(&service_id_menu, cx);
                            this.show_menu(ServiceMenuTarget::Node(node_clone.clone()), event.position, Some(status), window, cx);
                        }),
                    )
                    .on_drag(
                        ServiceDrag {
                            node_id: service_id.clone(),
                            label: service_name.clone(),
                            is_folder: false,
                        },
                        move |drag, _position, _window, cx| {
                            cx.new(|_| ServiceDragView {
                                label: drag.label.clone(),
                                is_folder: false,
                            })
                        },
                    )
                    .drag_over::<ServiceDrag>(move |style, drag: &ServiceDrag, _, _| {
                        if drag.is_folder {
                            style.border_t_2().border_color(rgb(border_active))
                        } else {
                            style.border_b_2().border_color(rgb(border_active))
                        }
                    })
                    .on_drop(cx.listener(move |this, drag: &ServiceDrag, _, cx| {
                        this.drop_node(drag, &service_id, false, cx);
                    }))
                    .child(
                        h_flex()
                            .gap(ui_space_sm(cx))
                            .items_center()
                            .child(div().size(px(7.0)).rounded_full().bg(color))
                            .child(
                                div()
                                    .text_size(ap.font_size)
                                    .child(def.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_xs(cx))
                                    .text_color(p.text_muted)
                                    .child(format!("({})", session_tag)),
                            ),
                    )
                    .when(has_actions, |d| {
                        d.child(
                            h_flex()
                                .gap(ui_space_xs(cx))
                                .items_center()
                                .opacity(0.0)
                                .group_hover(group_id.clone(), |s| s.opacity(1.0))
                                .when(is_selected, |s| s.opacity(1.0))
                                .children(start_btn)
                                .children(stop_btn)
                                .children(restart_btn),
                        )
                    })
                    .into_any_element()
            }
        }
    }

    fn action_icon_btn(
        &self,
        icon: AppIcon,
        tooltip: String,
        op: ServiceOp,
        def: &ServiceDefinition,
        t: &ThemeColors,
        p: &SemanticPalette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let def = def.clone();

        let has_cmd = match op {
            ServiceOp::Start => def.effective_start_command().is_some(),
            ServiceOp::Stop => def.effective_stop_command().is_some(),
            ServiceOp::Restart => def.effective_restart_command().is_some(),
        };

        if !has_cmd {
            return None;
        }

        let focus_mgr = self.focus_manager.clone();
        let ws = self.workspace.clone();
        let terms = self.terminals.clone();
        let om = self.overlay_manager.clone();

        let _ = (&focus_mgr, &ws, &terms, &om);

        let is_loading = self.operating_services.get(&def.id) == Some(&op);
        let is_any_operating = self.operating_services.contains_key(&def.id);

        let btn_group = SharedString::from(format!("svc-act-grp-{}-{:?}", def.id, op));
        let elem_id = SharedString::from(format!("svc-act-{}-{:?}", def.id, op));

        if is_loading {
            let anim_id = format!("svc-act-spinner-{}-{:?}", def.id, op);
            let spinner = velowork_ui::spinner::loading_spinner(anim_id, ICON_SM, p.text_primary);
            return Some(
                div()
                    .id(elem_id)
                    .size(ICON_LG)
                    .rounded(RADIUS_STD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.50)
                    .cursor(CursorStyle::Arrow)
                    .tooltip({
                        let tip = tooltip.clone();
                        move |_, cx| cx.new(|_| Tooltip::new(tip.clone())).into()
                    })
                    .child(spinner)
                    .into_any_element(),
            );
        }

        if is_any_operating {
            return Some(
                div()
                    .id(elem_id)
                    .size(ICON_LG)
                    .rounded(RADIUS_STD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.35)
                    .cursor(CursorStyle::Arrow)
                    .child(icon.size(ICON_SM).text_color(p.text_muted))
                    .into_any_element(),
            );
        }

        let this_entity = cx.entity().downgrade();
        Some(
            div()
                .id(elem_id)
                .group(btn_group.clone())
                .size(ICON_LG)
                .rounded(RADIUS_STD)
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(surface_bg_t(t.bg_hover, t)))
                .tooltip({
                    let tip = tooltip.clone();
                    move |_, cx| cx.new(|_| Tooltip::new(tip.clone())).into()
                })
                .on_click(cx.listener(move |this, _, _window, cx| {
                    let cmd = match op {
                        ServiceOp::Start => def.effective_start_command(),
                        ServiceOp::Stop => def.effective_stop_command(),
                        ServiceOp::Restart => def.effective_restart_command(),
                    };
                    if let Some(cmd) = cmd {
                        this.operating_services.insert(def.id.clone(), op);
                        cx.notify();

                        let requires_confirm = def.command_policy.requires_confirm(op);
                        let service_name = def.name.clone();
                        let display_cmd = cmd.clone();
                        let def_clone = def.clone();
                        let focus_mgr = focus_mgr.clone();
                        let ws = ws.clone();
                        let terms = terms.clone();
                        let this_ent = this_entity.clone();

                        let action_fn = move |cx: &mut App| {
                            let op_label = match op {
                                ServiceOp::Start => i18n_t(cx, "service.start"),
                                ServiceOp::Stop => i18n_t(cx, "service.stop"),
                                ServiceOp::Restart => i18n_t(cx, "service.restart"),
                            };
                            if def_clone.session_id.is_some() {
                                if let Some(engine) = cx.try_global::<velowork_terminal::GlobalServiceMonitorEngine>() {
                                    let engine_entity = engine.0.clone();
                                    if let Some(handle) = engine_entity.read(cx).snapshot_for_probe().2 {
                                        let def_clone = def_clone.clone();
                                        let svc_name = def_clone.name.clone();
                                        let op_label = op_label.clone();
                                        let this_ent = this_ent.clone();
                                        cx.spawn(async move |cx| {
                                            let res = cx.background_executor().spawn({
                                                let def_clone = def_clone.clone();
                                                let handle = handle.clone();
                                                async move {
                                                    velowork_terminal::ServiceMonitorEngine::exec_command(
                                                        &def_clone,
                                                        op,
                                                        &handle,
                                                    )
                                                }
                                            }).await;

                                            let _ = cx.update(|cx| {
                                                if let Some(panel) = this_ent.upgrade() {
                                                    let _ = panel.update(cx, |this, cx| {
                                                        this.operating_services.remove(&def_clone.id);
                                                        cx.notify();
                                                    });
                                                }
                                                match res {
                                                    Ok(_) => {
                                                        ToastManager::success(
                                                            format!("{}「{}」成功", op_label, svc_name),
                                                            cx,
                                                        );
                                                    }
                                                    Err(e) => {
                                                        ToastManager::error(
                                                            format!("{}「{}」失败: {}", op_label, svc_name, e),
                                                            cx,
                                                        );
                                                    }
                                                }
                                            });

                                            let (session_id, services, handle) = cx.update(|cx| engine_entity.read(cx).snapshot_for_probe());
                                            if let (Some(_), Some(handle)) = (session_id, handle) {
                                                let results = cx.background_executor().spawn(async move {
                                                    velowork_terminal::ServiceMonitorEngine::probe(&services, &handle)
                                                }).await;
                                                cx.update(|cx| {
                                                    engine_entity.update(cx, |e, cx| {
                                                        e.apply_results(results, cx);
                                                    });
                                                });
                                            }
                                        }).detach();
                                        return;
                                    }
                                }
                            }
                            if let Some(panel) = this_ent.upgrade() {
                                let _ = panel.update(cx, |this, cx| {
                                    this.operating_services.remove(&def_clone.id);
                                    cx.notify();
                                });
                            }
                            send_command_to_focused_terminal(&focus_mgr, &ws, &terms, &cmd, cx);
                            ToastManager::info(
                                format!("已发送{}命令至终端", op_label),
                                cx,
                            );
                        };

                        if requires_confirm {
                            let op_label = tooltip.clone();
                            om.update(cx, |om, cx| {
                                om.request_service_action_confirm(
                                    service_name,
                                    op_label,
                                    display_cmd,
                                    cx,
                                    action_fn,
                                );
                            });
                        } else {
                            action_fn(cx);
                        }
                    }
                }))
                .child(
                    icon.size(ICON_SM)
                        .text_color(p.text_secondary)
                        .group_hover(btn_group, |s| s.text_color(p.text_primary)),
                )
                .into_any_element(),
        )
    }
}

impl Focusable for ServiceMonitorPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ServiceMonitorPanel {
    fn metadata(&self, cx: &App) -> PanelInfo {
        PanelInfo::new(
            "services",
            i18n!(cx, "service.title"),
            AppIcon::SquareActivity,
            PanelKind::Custom,
        )
    }

    fn focus_handle(&self, _cx: &App) -> Option<FocusHandle> {
        Some(self.focus_handle.clone())
    }
}

impl Render for ServiceMonitorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        if self.show_service_search && self.filter_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "service.search_tooltip"))
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

        let mut visible_order = Vec::new();
        collect_service_visible_ids(&self.nodes, &filter_text, false, &mut visible_order);
        self.service_visible_order = visible_order;

        let mut force_expanded = HashSet::new();
        collect_service_expanded_keys(&self.nodes, false, &mut force_expanded);

        let is_panel_active = self.is_selection_active(window, cx);
        let active_selected_keys = if is_panel_active {
            self.selected_service_ids.clone()
        } else {
            HashSet::new()
        };

        let tree_nodes = convert_service_nodes(&self.nodes);
        let t_color = t.clone();
        let filter_clone = filter_text.clone();
        self.selected_node_bounds.borrow_mut().clear();
        let tree_widget = velowork_ui::tree::<Self, String, ServiceNode>("service-tree")
            .nodes(tree_nodes)
            .expanded_keys(force_expanded)
            .selected_keys(active_selected_keys)
            .render_before_children({
                let t_color = t_color.clone();
                move |this, parent_key, depth, window, cx| {
                    let parent_id = parent_key.map(|k| k.as_str());
                    let is_creating_here = match &this.inline_folder {
                        Some(InlineServiceFolderState::Creating {
                            parent_id: target_pid,
                            ..
                        }) => target_pid.as_deref() == parent_id,
                        _ => false,
                    };
                    if is_creating_here {
                        return Some(this.render_inline_folder_input_row(
                            "service-inline-folder-create",
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
                    let node_id = &tree_ctx.node.key;
                    let depth = tree_ctx.depth;
                    if let Some(InlineServiceFolderState::Renaming { id, .. }) = &this.inline_folder {
                        if id == node_id {
                            return this.render_inline_folder_input_row(
                                node_id,
                                tree_ctx.is_expanded,
                                depth,
                                &t_color,
                                window,
                                cx,
                            );
                        }
                    }
                    if let Some(InlineServiceState { id, .. }) = &this.inline_service {
                        if id == node_id {
                            return this.render_inline_service_input_row(
                                depth,
                                &t_color,
                                window,
                                cx,
                            );
                        }
                    }
                    let node = match tree_ctx.node.payload.as_ref() {
                        Some(n) => n,
                        None => return div().into_any_element(),
                    };
                    if !filter_str.is_empty() && !node_matches_service(node, &filter_str) {
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

        let add_tip = i18n!(cx, "service.add");
        let folder_tip = i18n!(cx, "service.new_folder");
        let search_tip = i18n!(cx, "service.search_tooltip");

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
                if this.inline_folder.is_none() && this.inline_service.is_none() {
                    if let Some(id) = this.selected_service_ids.iter().next().cloned() {
                        if let Some(node) = service_find_node_ref(&this.nodes, &id).cloned() {
                            match node {
                                ServiceNode::Folder { name, .. } => {
                                    this.inline_rename_folder(&id, &name, cx);
                                }
                                ServiceNode::Service { def } => {
                                    this.inline_rename_service(&id, &def.name, cx);
                                }
                            }
                        }
                    }
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.inline_folder.is_some() || this.inline_service.is_some() {
                    return;
                }
                let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                if cmd_or_ctrl && event.keystroke.key.as_str() == "f" {
                    this.show_service_search = true;
                    let input = this.filter_input.get_or_insert_with(|| {
                        let input = cx.new(|cx| {
                            InputState::new(cx)
                                .placeholder(i18n!(cx, "service.search_tooltip"))
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
                        if this.selected_service_ids.is_empty() {
                            if let Some(first_id) = this.service_visible_order.first().cloned() {
                                this.selected_service_ids.insert(first_id);
                                this.service_focused_index = Some(0);
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
                            if matches!(service_find_node_ref(&this.nodes, &id), Some(ServiceNode::Folder { .. })) {
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
                            if matches!(service_find_node_ref(&this.nodes, &id), Some(ServiceNode::Folder { .. })) {
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
                        if this.selected_service_ids.is_empty() {
                            if let Some(first_id) = this.service_visible_order.first().cloned() {
                                this.selected_service_ids.insert(first_id);
                                this.service_focused_index = Some(0);
                                cx.notify();
                            }
                            cx.stop_propagation();
                            return;
                        }
                        this.activate_focused(cx);
                        cx.stop_propagation();
                    }
                    "enter" => {
                        if !panel_focused || this.selected_service_ids.is_empty() {
                            return;
                        }
                        this.enter_focused(cx);
                        cx.stop_propagation();
                    }
                    "escape" => {
                        if this.show_service_search {
                            this.show_service_search = false;
                            if let Some(ref input) = this.filter_input {
                                input.update(cx, |inp, cx| inp.set_value("", cx));
                            }
                            window.focus(&this.focus_handle, cx);
                            cx.stop_propagation();
                            cx.notify();
                            return;
                        }
                        if !this.selected_service_ids.is_empty() {
                            this.clear_selection(cx);
                            cx.stop_propagation();
                        }
                    }
                    "delete" => {
                        if !this.selected_service_ids.is_empty() {
                            let ids: Vec<String> = this.selected_service_ids.iter().cloned().collect();
                            let origin = Some(this.focus_handle.clone());
                            let click_origin = ids.iter().find_map(|id| {
                                this.selected_node_bounds.borrow().get(id).map(|b| b.center())
                            }).or_else(|| {
                                this.selected_node_bounds.borrow().values().next().map(|b| b.center())
                            });
                            this.overlay_manager.update(cx, |om, cx| {
                                if let Some(pt) = click_origin {
                                    om.record_click_origin(pt);
                                }
                                om.request_service_delete_confirm_with_origin(
                                    ids,
                                    origin.clone(),
                                    origin,
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
                        icon_button("btn-add-service", AppIcon::Plus, &t, cx)
                            .tooltip(move |_, cx| {
                                let tip = add_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus_handle, cx);
                                let parent = this.selected_folder_parent(cx);
                                this.open_dialog(
                                    ServiceDialogMode::Create { parent_id: parent },
                                    None,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        icon_button("btn-add-service-folder", AppIcon::NewFolder, &t, cx)
                            .tooltip(move |_, cx| {
                                let tip = folder_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus_handle, cx);
                                let parent = this.selected_folder_parent(cx);
                                this.inline_create_folder(parent, cx);
                            })),
                    )
                    .child(div().w(px(1.0)).h(ICON_STD).bg(p.border_subtle))
                    .child(
                        icon_button("btn-service-search", AppIcon::Search, &t, cx)
                            .when(self.show_service_search, |b| b.bg(surface_bg_t(t.bg_hover, &t)))
                            .tooltip(move |_, cx| {
                                let tip = search_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_search(window, cx);
                            })),
                    ),
            )
            .when(self.show_service_search, |d| {
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
                            .id("service-tree-scroll")
                            .size_full()
                            .track_scroll(&self.service_scroll_handle)
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .py(ui_space_xs(cx))
                            .px(ui_space_xs(cx))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    this.show_menu(ServiceMenuTarget::Root, event.position, None, window, cx);
                                }),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_selection(cx);
                                window.focus(&this.focus_handle, cx);
                            }))
                            .on_drop(cx.listener(move |this, drag: &ServiceDrag, _, cx| {
                                this.drop_node_to_root(drag, cx);
                            }))
                            .child(tree_widget.render(self, window, cx))
                            .child(
                                div()
                                    .id("svc-tree-dropzone")
                                    .flex_1()
                                    .min_h(px(24.0))
                                    .when(!filter_text.is_empty(), |d| d.hidden())
                                    .drag_over::<ServiceDrag>(move |style, _drag: &ServiceDrag, _, _| {
                                        style.bg(rgb(t.bg_hover))
                                    })
                                    .on_drop(cx.listener(move |this, drag: &ServiceDrag, _, cx| {
                                        this.drop_node_to_root(drag, cx);
                                    })),
                            ),
                    ),
            )
    }
}

pub fn register_toolbar_panel(registry: &mut velowork_ui::dock::RightToolbarRegistry) {
    registry.register(velowork_ui::dock::ToolbarPanelSpec {
        id: "services".to_string(),
        icon: AppIcon::SquareActivity,
        title_key: "service.title".to_string(),
        order: 40,
        is_visible: std::sync::Arc::new(|_cx| true),
        factory: std::sync::Arc::new(|ctx, _window, cx| {
            let app_ctx = ctx
                .downcast_ref::<super::AppPanelCreationContext>()
                .expect("AppPanelCreationContext required");
            let workspace = app_ctx.workspace.clone();
            let focus_manager = app_ctx.focus_manager.clone();
            let terminals = app_ctx.terminals.clone();
            let overlay_manager = app_ctx.overlay_manager.clone();
            let p = cx.new(|cx| {
                ServiceMonitorPanel::new(workspace, focus_manager, terminals, overlay_manager, cx)
            });
            velowork_ui::dock::AnyPanel::new(p)
        }),
    });
}

fn filter_service_tree(nodes: &[ServiceNode], target_pid: Option<&str>, cx: &App) -> Vec<ServiceNode> {
    let pid_str = target_pid.unwrap_or("");
    let session_ids: std::collections::HashSet<String> = if let Some(store) = cx.try_global::<GlobalSessionStore>() {
        let tree = store.0.read(cx).tree_for_project(target_pid);
        let mut set = std::collections::HashSet::new();
        collect_session_ids_from_tree(tree, &mut set);
        set
    } else {
        std::collections::HashSet::new()
    };

    fn filter_list(nodes: &[ServiceNode], target_pid: &str, session_ids: &std::collections::HashSet<String>) -> Vec<ServiceNode> {
        let mut out = Vec::new();
        for node in nodes {
            match node {
                ServiceNode::Folder { id, name, project_id, expanded, children } => {
                    let sub = filter_list(children, target_pid, session_ids);
                    let pid_matches = match project_id {
                        Some(pid) => pid == target_pid || target_pid.is_empty() || target_pid == "default",
                        None => target_pid.is_empty() || target_pid == "default",
                    };
                    if pid_matches || !sub.is_empty() {
                        out.push(ServiceNode::Folder {
                            id: id.clone(),
                            name: name.clone(),
                            project_id: project_id.clone(),
                            expanded: *expanded,
                            children: sub,
                        });
                    }
                }
                ServiceNode::Service { def } => {
                    let pid_matches = match &def.project_id {
                        Some(pid) => pid == target_pid || target_pid.is_empty() || target_pid == "default",
                        None => target_pid.is_empty() || target_pid == "default",
                    };
                    let session_matches = def.session_id.as_ref().map_or(false, |sid| !sid.is_empty() && session_ids.contains(sid));
                    if pid_matches || session_matches {
                        out.push(ServiceNode::Service { def: def.clone() });
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
