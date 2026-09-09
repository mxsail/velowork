use gpui::*;
use gpui::prelude::*;
use crate::keybindings::RenameActiveNode;
use velowork_ui::input::InputState;
use velowork_ui::theme::{surface_bg, surface_bg_t, theme, with_alpha, ThemeColors};
use velowork_ui::icon::{AppIcon, folder_tree_icon};
use velowork_ui::icon_button::icon_button_sized;
use velowork_ui::tokens::{
    ui_space_lg, ui_space_md, ui_space_sm, ui_space_xs, ui_text, ui_text_md, ui_text_sm,
    ICON_STD, RADIUS_STD,
};
use velowork_ui::dock::{Panel, PanelInfo, PanelAction};
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::{h_flex, v_flex, SemanticPalette};
use velowork_ui::tooltip::Tooltip;
use velowork_workspace::focus::FocusManager;
use velowork_workspace::state::Workspace;
use velowork_terminal::TerminalsRegistry;
use std::collections::HashSet;
use std::sync::Arc;
use velowork_workspace::quick_commands::{
    new_quick_command_id, qc_find_node_mut, qc_find_node_ref, qc_folder_name_exists,
    qc_insert_node, qc_move_node, qc_node_name_exists, qc_parent_id_of, qc_unique_duplicate_name,
    QuickCommandNode,
};
use crate::settings::settings_entity;
use crate::views::overlays::{
    open_quick_command_context_menu, QuickCommandContextMenuEvent, QuickCommandDialogMode,
    QuickCommandMenuRequest, QuickCommandMenuTarget,
};
use velowork_ui::menu::PopupMenu;
use crate::views::overlays::overlay_manager::OverlayManager;
use velowork_i18n::i18n;
use velowork_ui::{
    ControlAppearance, HoverBehavior, SelectedBehavior, StatefulElementBehaviorExt,
};

/// 快捷指令树行外观：由设计系统 Compact 档统一解析（行高/字体/图标尺寸），
/// 替代散落的硬编码 px 值。
fn qc_row_appearance(t: &ThemeColors, cx: &App) -> ControlAppearance {
    velowork_ui::tree::tree_row_appearance(t, cx)
}

/// 快捷指令树行高（纯几何，与主题无关）。供滚动计算使用。
fn qc_row_height(cx: &App) -> Pixels {
    velowork_ui::tree_row_height(cx)
}

#[derive(Clone)]
pub(crate) enum InlineQcFolderState {
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

/// 行内重命名快捷指令（命令节点）的状态。命令节点只支持重命名（无「新建」），
/// 因此只有一个 `Renaming` 变体。
#[derive(Clone)]
pub(crate) enum InlineQcCommandState {
    Renaming {
        id: String,
        original_name: String,
        input: Option<Entity<InputState>>,
    },
}

fn convert_qc_tree_nodes(nodes: &[QuickCommandNode]) -> Vec<velowork_ui::TreeNodeData<String, QuickCommandNode>> {
    nodes
        .iter()
        .map(|node| match node {
            QuickCommandNode::Folder { id, name, children, .. } => {
                velowork_ui::TreeNodeData::new(id.clone(), name.clone(), true)
                    .with_children(convert_qc_tree_nodes(children))
                    .with_payload(node.clone())
            }
            QuickCommandNode::Command { id, name, .. } => {
                velowork_ui::TreeNodeData::new(id.clone(), name.clone(), false)
                    .with_payload(node.clone())
            }
        })
        .collect()
}

fn collect_qc_expanded_keys(nodes: &[QuickCommandNode], force_expand: bool, out: &mut std::collections::HashSet<String>) {
    for node in nodes {
        if let QuickCommandNode::Folder { id, expanded, children, .. } = node {
            if *expanded || force_expand {
                out.insert(id.clone());
                collect_qc_expanded_keys(children, force_expand, out);
            }
        }
    }
}

pub struct QuickCommandsPanel {
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,
    terminals: TerminalsRegistry,
    overlay_manager: Entity<OverlayManager>,
    focus_handle: FocusHandle,

    filter_input: Option<Entity<InputState>>,
    nodes: Vec<QuickCommandNode>,
    show_qc_search: bool,

    selected_qc_ids: HashSet<String>,
    qc_selection_anchor: Option<String>,
    qc_visible_order: Vec<String>,
    /// 键盘导航用的焦点索引（基于 `qc_visible_order`）。同会话树一样显式维护，
    /// 避免 `qc_visible_order` 存在重复 id 时方向键跳错节点。
    qc_focused_index: Option<usize>,
    /// 快捷指令树滚动容器句柄，用于方向键导航时把焦点行滚动进可视区。
    qc_scroll_handle: ScrollHandle,
    pub(crate) inline_folder: Option<InlineQcFolderState>,
    pub(crate) inline_folder_sub: Option<Subscription>,
    /// 命令节点行内重命名的当前状态（仅重命名，无行内新建）。
    pub(crate) inline_command: Option<InlineQcCommandState>,
    pub(crate) inline_command_sub: Option<Subscription>,
    pub(crate) overlay_registry: Option<Entity<velowork_ui::overlay_registry::OverlayRegistry>>,
    context_menu: Option<Entity<PopupMenu>>,
}

impl QuickCommandsPanel {
    pub fn set_overlay_registry(&mut self, registry: Entity<velowork_ui::overlay_registry::OverlayRegistry>) {
        self.overlay_registry = Some(registry);
    }

    pub fn new(
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        terminals: TerminalsRegistry,
        overlay_manager: Entity<OverlayManager>,
        cx: &mut Context<Self>,
    ) -> Self {
        let init_pid = focus_manager.read(cx).active_project_id().cloned();
        let nodes = settings_entity(cx)
            .read(cx)
            .settings
            .quick_commands_for_project(init_pid.as_deref())
            .to_vec();

        let settings_entity = settings_entity(cx);
        cx.observe(&settings_entity, |this: &mut Self, _s, cx| {
            this.reload_from_settings(cx);
        })
        .detach();

        cx.observe(&workspace, |this: &mut Self, _ws, cx| {
            this.reload_from_settings(cx);
        })
        .detach();

        cx.observe(&focus_manager, |this: &mut Self, _fm, cx| {
            this.reload_from_settings(cx);
        })
        .detach();

        let weak_panel = cx.entity().downgrade();
        overlay_manager.update(cx, |om, _cx| {
            let weak = weak_panel.clone();
            om.on_qc_create_folder = Some(std::sync::Arc::new(move |parent_id, cx| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |p, cx| {
                        p.inline_create_folder(parent_id, cx);
                    });
                }
            }));
            let weak = weak_panel.clone();
            om.on_qc_rename_folder = Some(std::sync::Arc::new(move |id, name, cx| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |p, cx| {
                        p.inline_rename_folder(id, name, cx);
                    });
                }
            }));
            let weak = weak_panel.clone();
            om.on_qc_created = Some(std::sync::Arc::new(move |id, cx| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |p, cx| {
                        p.select_qc_id(&id, cx);
                    });
                }
            }));
            let weak = weak_panel.clone();
            om.on_qc_rename_command = Some(std::sync::Arc::new(move |id, name, cx| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |p, cx| {
                        p.inline_rename_command(id, name, cx);
                    });
                }
            }));
        });

        let panel = Self {
            workspace,
            focus_manager,
            terminals,
            overlay_manager,
            focus_handle: cx.focus_handle(),
            filter_input: None,
            nodes,
            show_qc_search: false,
            selected_qc_ids: HashSet::new(),
            qc_selection_anchor: None,
            qc_visible_order: Vec::new(),
            qc_focused_index: None,
            qc_scroll_handle: ScrollHandle::new(),
            inline_folder: None,
            inline_folder_sub: None,
            inline_command: None,
            inline_command_sub: None,
            overlay_registry: None,
            context_menu: None,
        };

        cx.observe(&panel.overlay_manager, |_this: &mut Self, _om, cx| {
            cx.notify();
        })
        .detach();

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

    fn reload_from_settings(&mut self, cx: &mut Context<Self>) {
        let active_pid = self.active_project_id(cx);
        let tree = settings_entity(cx)
            .read(cx)
            .settings
            .quick_commands_for_project(active_pid.as_deref())
            .to_vec();
        self.nodes = tree;
        let old_selected = self.selected_qc_ids.clone();
        self.selected_qc_ids.retain(|id| qc_find_node_mut(&mut self.nodes, id).is_some());
        if self.selected_qc_ids.is_empty() && !old_selected.is_empty() {
            if let Some(deleted_id) = old_selected.iter().next() {
                if let Some(pos) = self.qc_visible_order.iter().position(|id| id == deleted_id) {
                    let remaining_visible: Vec<String> = self.qc_visible_order
                        .iter()
                        .filter(|id| !old_selected.contains(*id) && qc_find_node_mut(&mut self.nodes, id).is_some())
                        .cloned()
                        .collect();
                    if !remaining_visible.is_empty() {
                        let next_idx = if pos < remaining_visible.len() {
                            pos
                        } else {
                            remaining_visible.len() - 1
                        };
                        let next_id = remaining_visible[next_idx].clone();
                        self.selected_qc_ids.insert(next_id);
                        self.qc_focused_index = Some(next_idx);
                    }
                }
            }
        }
        if let Some(ref anchor) = self.qc_selection_anchor {
            if qc_find_node_mut(&mut self.nodes, anchor).is_none() {
                self.qc_selection_anchor = self.selected_qc_ids.iter().next().cloned();
            }
        }
        cx.notify();
    }

    pub fn select_qc_id(&mut self, id: &str, cx: &mut Context<Self>) {
        self.reload_from_settings(cx);
        self.selected_qc_ids.clear();
        self.selected_qc_ids.insert(id.to_string());
        self.qc_selection_anchor = Some(id.to_string());
        self.qc_focused_index = self.qc_visible_order.iter().position(|x| x == id);
        cx.notify();
    }

    fn execute_command(&self, cmd: &str, window: Option<&mut Window>, cx: &mut Context<Self>) {
        send_command_to_focused_terminal(
            &self.focus_manager,
            &self.workspace,
            &self.terminals,
            cmd,
            cx,
        );
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
        });
        if let Some(window) = window {
            if let Some(handle) = self.focus_manager.read(cx).focused_terminal_state().and_then(|state| {
                let pane_map = velowork_views_terminal::layout::navigation::get_pane_map(crate::workspace::state::WindowId::Main);
                pane_map.find_pane(&state.project_id, &state.layout_path).and_then(|pane| pane.focus_handle.clone())
            }) {
                window.focus(&handle, cx);
            }
        }
    }

    fn on_command_click(&mut self, node: &QuickCommandNode, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if let QuickCommandNode::Command {
            command, variables, ..
        } = node
        {
            if variables.is_empty() {
                self.execute_command(command, window, cx);
            } else {
                let template = command.clone();
                let name = node.name().to_string();
                let vars = variables.clone();
                let focus_manager = self.focus_manager.clone();
                let workspace = self.workspace.clone();
                let terminals = self.terminals.clone();
                let origin = self.focus_handle.clone();
                let panel = self.focus_handle.clone();
                self.overlay_manager.update(cx, |om, cx| {
                    om.show_quick_command_var_dialog_with_origin(
                        focus_manager,
                        workspace,
                        terminals,
                        name,
                        template,
                        vars,
                        Some(origin),
                        Some(panel),
                        cx,
                    );
                });
            }
        }
    }

    fn toggle_folder(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_string();
        let active_pid = self.active_project_id(cx);
        settings_entity(cx)
            .update(cx, |s, cx| {
                let tree = s.settings.quick_commands_for_project_mut(active_pid.as_deref());
                if let Some(node) =
                    qc_find_node_mut(tree, &id)
                {
                    if let QuickCommandNode::Folder { expanded, .. } = node {
                        *expanded = !*expanded;
                    }
                }
                s.save_and_notify(cx);
            });
        self.reload_from_settings(cx);
    }

    fn clear_qc_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selected_qc_ids.is_empty() || self.qc_selection_anchor.is_some() {
            self.selected_qc_ids.clear();
            self.qc_selection_anchor = None;
            self.qc_focused_index = None;
            cx.notify();
        }
    }

    fn handle_qc_node_click(
        &mut self,
        node: &QuickCommandNode,
        is_folder: bool,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Ensure the panel is focused so keyboard shortcuts (arrows/Enter/Esc) apply.
        window.focus(&self.focus_handle, cx);
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::None);
        });

        let mods = event.modifiers();
        let ctrl = mods.control || mods.platform;
        let shift = mods.shift;
        let node_id = node.id().to_string();

        // 点击节点后，键盘导航焦点应跟随到被点击的行。
        let clicked_index = self.qc_visible_order.iter().position(|x| x == &node_id);

        if shift {
            let anchor = self.qc_selection_anchor.clone();
            let range: Option<Vec<String>> = anchor.and_then(|anchor| {
                let ai = self.qc_visible_order.iter().position(|id| id == &anchor)?;
                let ti = self.qc_visible_order.iter().position(|id| id == &node_id)?;
                let (lo, hi) = if ai <= ti { (ai, ti) } else { (ti, ai) };
                Some(self.qc_visible_order[lo..=hi].to_vec())
            });
            match range {
                Some(ids) => {
                    self.selected_qc_ids = ids.into_iter().collect();
                }
                None => {
                    self.selected_qc_ids.clear();
                    self.selected_qc_ids.insert(node_id.clone());
                    self.qc_selection_anchor = Some(node_id.clone());
                }
            }
            self.qc_focused_index = clicked_index;
            cx.notify();
            return;
        }

        if ctrl {
            if self.selected_qc_ids.contains(&node_id) {
                self.selected_qc_ids.remove(&node_id);
            } else {
                self.selected_qc_ids.insert(node_id.clone());
            }
            self.qc_selection_anchor = Some(node_id.clone());
            self.qc_focused_index = clicked_index;
            cx.notify();
            return;
        }

        if is_folder {
            self.selected_qc_ids.clear();
            self.selected_qc_ids.insert(node_id.clone());
            self.qc_selection_anchor = Some(node_id.clone());
            self.qc_focused_index = clicked_index;
            self.toggle_folder(&node_id, cx);
            return;
        }

        if event.click_count() >= 2 {
            self.on_command_click(node, Some(window), cx);
        } else {
            self.selected_qc_ids.clear();
            self.selected_qc_ids.insert(node_id.clone());
            self.qc_selection_anchor = Some(node_id.clone());
            self.qc_focused_index = clicked_index;
            cx.notify();
        }
    }

    fn show_menu(
        &mut self,
        target: QuickCommandMenuTarget,
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

        if let QuickCommandMenuTarget::Node(ref node) = target {
            let id = node.id().to_string();
            if !self.selected_qc_ids.contains(&id) {
                self.selected_qc_ids.clear();
                self.selected_qc_ids.insert(id.clone());
                self.qc_selection_anchor = Some(id);
                window.focus(&self.focus_handle, cx);
            }
        }

        let request = QuickCommandMenuRequest {
            position,
            target,
            selected_ids: self.selected_qc_ids.iter().cloned().collect(),
        };

        let this_weak = cx.entity().downgrade();
        let this_weak_close = this_weak.clone();
        let menu = open_quick_command_context_menu(
            request,
            self.overlay_registry.clone(),
            move |event, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| match event {
                        QuickCommandContextMenuEvent::Close => {
                            cx.notify();
                        }
                        QuickCommandContextMenuEvent::NewCommand { parent_id } => {
                            let parent_id = parent_id.clone();
                            let origin = Some(this.focus_handle.clone());
                            this.overlay_manager.update(cx, |om, cx| {
                                om.show_quick_command_dialog_with_origin(
                                    QuickCommandDialogMode::CreateCommand {
                                        parent_id,
                                        initial_content: None,
                                    },
                                    origin.clone(),
                                    origin,
                                    cx,
                                );
                            });
                            cx.notify();
                        }
                        QuickCommandContextMenuEvent::NewFolder { parent_id } => {
                            this.inline_create_folder(parent_id.clone(), cx);
                        }
                        QuickCommandContextMenuEvent::Edit { node } => {
                            let node = node.clone();
                            let origin = Some(this.focus_handle.clone());
                            this.overlay_manager.update(cx, |om, cx| {
                                if node.is_folder() {
                                    om.show_quick_command_dialog_with_origin(QuickCommandDialogMode::EditFolder { node }, origin.clone(), origin, cx);
                                } else {
                                    om.show_quick_command_dialog_with_origin(QuickCommandDialogMode::EditCommand { node }, origin.clone(), origin, cx);
                                }
                            });
                            cx.notify();
                        }
                        QuickCommandContextMenuEvent::Rename { node } => {
                            let node = node.clone();
                            if node.is_folder() {
                                this.inline_rename_folder(node.id().to_string(), node.name().to_string(), cx);
                            } else {
                                this.inline_rename_command(node.id().to_string(), node.name().to_string(), cx);
                            }
                        }
                        QuickCommandContextMenuEvent::Duplicate { command_id } => {
                            let cid = command_id.clone();
                            let active_pid = this.active_project_id(cx);
                            let settings_entity = crate::settings::settings_entity(cx);
                            let mut new_clone_id = None;
                            settings_entity.update(cx, |s, cx| {
                                let tree = s.settings.quick_commands_for_project_mut(active_pid.as_deref());
                                let parent = qc_parent_id_of(tree, &cid).flatten();
                                let source = match qc_find_node_mut(tree, &cid) {
                                    Some(QuickCommandNode::Command { name, command, variables, .. }) => {
                                        Some((name.clone(), command.clone(), variables.clone()))
                                    }
                                    _ => None,
                                };
                                let clone = match source {
                                    Some((name, command, variables)) => {
                                        let new_name = qc_unique_duplicate_name(
                                            tree,
                                            parent.as_deref(),
                                            &name,
                                        );
                                        Some(QuickCommandNode::Command {
                                            id: new_quick_command_id(),
                                            name: new_name,
                                            command,
                                            variables,
                                        })
                                    }
                                    None => None,
                                };
                                if let Some(clone) = clone {
                                    let clone_id = clone.id().to_string();
                                    new_clone_id = Some(clone_id);
                                    if let Some(ref p_id) = parent {
                                        if let Some(pnode) = qc_find_node_mut(tree, p_id) {
                                            if let QuickCommandNode::Folder { expanded, .. } = pnode {
                                                *expanded = true;
                                            }
                                        }
                                    }
                                    qc_insert_node(tree, parent.as_deref(), clone);
                                    s.save_and_notify(cx);
                                }
                            });
                            if let Some(clone_id) = new_clone_id {
                                this.select_qc_id(&clone_id, cx);
                                this.scroll_qc_node_into_view(&clone_id, cx);
                            }
                        }
                        QuickCommandContextMenuEvent::Delete { ids } => {
                            let ids = ids.clone();
                            let origin = Some(this.focus_handle.clone());
                            this.overlay_manager.update(cx, |om, cx| {
                                om.request_quick_command_delete_confirm_with_origin(ids, origin.clone(), origin, cx);
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

    fn active_project_id(&self, cx: &App) -> Option<String> {
        self.focus_manager.read(cx).active_project_id().cloned()
    }

    fn get_selected_folder_id(&self) -> Option<String> {
        for id in &self.selected_qc_ids {
            if Self::is_folder_node(&self.nodes, id) {
                return Some(id.clone());
            }
        }
        None
    }

    fn is_folder_node(nodes: &[QuickCommandNode], target_id: &str) -> bool {
        for node in nodes {
            match node {
                QuickCommandNode::Folder { id, children, .. } => {
                    if id == target_id {
                        return true;
                    }
                    if Self::is_folder_node(children, target_id) {
                        return true;
                    }
                }
                QuickCommandNode::Command { .. } => {}
            }
        }
        false
    }

    pub fn inline_create_folder(
        &mut self,
        parent_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let parent_id = parent_id.or_else(|| self.get_selected_folder_id());
        let active_pid = self.active_project_id(cx);

        if let Some(ref pid) = parent_id {
            let id = pid.clone();
            let pid_clone = active_pid.clone();
            settings_entity(cx).update(cx, |s, cx| {
                let tree = s.settings.quick_commands_for_project_mut(pid_clone.as_deref());
                if let Some(node) = qc_find_node_mut(tree, &id) {
                    if let QuickCommandNode::Folder { expanded, .. } = node {
                        *expanded = true;
                    }
                }
                s.save_and_notify(cx);
            });
            self.reload_from_settings(cx);
        }

        let placeholder = i18n!(cx, "common.new_folder");

        // 重置订阅，使新输入框在下次渲染时获得初始焦点。
        self.inline_folder_sub = None;
        self.inline_folder = Some(InlineQcFolderState::Creating {
            parent_id,
            input: None,
            placeholder,
        });

        cx.notify();
    }

    pub fn inline_rename_folder(
        &mut self,
        id: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        // 重置订阅，使新输入框在下次渲染时获得初始焦点。
        self.inline_folder_sub = None;
        self.inline_folder = Some(InlineQcFolderState::Renaming {
            id,
            original_name: name,
            input: None,
        });

        cx.notify();
    }

    /// F2 快捷键：重命名当前选中的节点。
    /// F2：重命名当前选中的节点（文件夹与命令均可重命名）。
    pub fn rename_selected_qc_node(&mut self, id: &str, cx: &mut Context<Self>) {
        // 在不可变借用内取出节点（owned），随后释放借用再调用可变方法。
        let node = {
            let settings_entity = crate::settings::settings_entity(cx);
            let s = settings_entity.read(cx);
            let tree = s
                .settings
                .quick_commands_for_project(self.active_project_id(cx).as_deref());
            qc_find_node_ref(tree, id).cloned()
        };
        let node = match node {
            Some(n) => n,
            None => return,
        };
        match &node {
            QuickCommandNode::Folder { id, name, .. } => {
                self.inline_rename_folder(id.clone(), name.clone(), cx);
            }
            QuickCommandNode::Command { id, name, .. } => {
                self.inline_rename_command(id.clone(), name.clone(), cx);
            }
        }
    }

    pub fn cancel_inline_folder(&mut self, cx: &mut Context<Self>) {
        if self.inline_folder.is_some() {
            self.inline_folder = None;
            self.inline_folder_sub = None;
            cx.notify();
        }
    }

    pub fn commit_inline_folder(&mut self, cx: &mut Context<Self>) {
        self.inline_folder_sub = None;
        if let Some(state) = self.inline_folder.take() {
            let active_pid = self.active_project_id(cx);
            match state {
                InlineQcFolderState::Creating { parent_id, input, placeholder } => {
                    let val = input
                        .as_ref()
                        .map(|i| i.read(cx).text().to_string().trim().to_string())
                        .unwrap_or_default();
                    if !val.is_empty() {
                        let qc_tree = settings_entity(cx).read(cx).settings.quick_commands_for_project(active_pid.as_deref());
                        if qc_folder_name_exists(qc_tree, parent_id.as_deref(), &val, None) {
                            self.inline_folder = Some(InlineQcFolderState::Creating { parent_id, input, placeholder });
                            cx.notify();
                            return;
                        }
                        let new_folder_id = uuid::Uuid::new_v4().to_string();
                        let new_folder = QuickCommandNode::Folder {
                            id: new_folder_id.clone(),
                            name: val,
                            expanded: true,
                            children: Vec::new(),
                        };
                        let active_pid_clone = active_pid.clone();
                        settings_entity(cx).update(cx, |s, cx| {
                            let tree = s.settings.quick_commands_for_project_mut(active_pid_clone.as_deref());
                            if let Some(pid) = &parent_id {
                                if let Some(node) = qc_find_node_mut(tree, pid) {
                                    if let QuickCommandNode::Folder { expanded, children, .. } = node {
                                        *expanded = true;
                                        children.push(new_folder);
                                    }
                                }
                            } else {
                                tree.push(new_folder);
                            }
                            s.save_and_notify(cx);
                        });
                        self.reload_from_settings(cx);
                        self.selected_qc_ids.clear();
                        self.selected_qc_ids.insert(new_folder_id.clone());
                        self.qc_selection_anchor = Some(new_folder_id);
                    }
                }
                InlineQcFolderState::Renaming {
                    id,
                    original_name,
                    input,
                } => {
                    let val = input
                        .as_ref()
                        .map(|i| i.read(cx).text().to_string().trim().to_string())
                        .unwrap_or_default();
                    if !val.is_empty() && val != original_name {
                        let qc_tree = settings_entity(cx).read(cx).settings.quick_commands_for_project(active_pid.as_deref());
                        let parent_id = qc_parent_id_of(qc_tree, &id).flatten();
                        if qc_folder_name_exists(qc_tree, parent_id.as_deref(), &val, Some(&id)) {
                            self.inline_folder = Some(InlineQcFolderState::Renaming {
                                id,
                                original_name,
                                input,
                            });
                            cx.notify();
                            return;
                        }
                        let active_pid_clone = active_pid.clone();
                        settings_entity(cx).update(cx, |s, cx| {
                            let tree = s.settings.quick_commands_for_project_mut(active_pid_clone.as_deref());
                            if let Some(node) = qc_find_node_mut(tree, &id) {
                                if let QuickCommandNode::Folder { name, .. } = node {
                                    *name = val;
                                }
                            }
                            s.save_and_notify(cx);
                        });
                        self.reload_from_settings(cx);
                    }
                }
            }
            cx.notify();
        }
    }

    /// 行内重命名命令节点（仅名称）。与「编辑」打开完整对话框区分。
    pub fn inline_rename_command(
        &mut self,
        id: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        // 重置订阅，使新输入框在下次渲染时获得初始焦点。
        self.inline_command_sub = None;
        self.inline_command = Some(InlineQcCommandState::Renaming {
            id,
            original_name: name,
            input: None,
        });

        cx.notify();
    }

    /// 取消行内重命名（命令节点）。
    pub fn cancel_inline_command(&mut self, cx: &mut Context<Self>) {
        if self.inline_command.is_some() {
            self.inline_command = None;
            self.inline_command_sub = None;
            cx.notify();
        }
    }

    /// 提交行内重命名（命令节点）：仅更新名称（同名或空则不改动）。
    pub fn commit_inline_command(&mut self, cx: &mut Context<Self>) {
        self.inline_command_sub = None;
        if let Some(state) = self.inline_command.take() {
            let active_pid = self.active_project_id(cx);
            let InlineQcCommandState::Renaming {
                id,
                original_name,
                input,
            } = state;
            let val = input
                .as_ref()
                .map(|i| i.read(cx).text().to_string().trim().to_string())
                .unwrap_or_default();
            if !val.is_empty() && val != original_name {
                // 同目录（含文件夹、指令）已存在同名节点则保留输入框让用户改。
                let tree = settings_entity(cx)
                    .read(cx)
                    .settings
                    .quick_commands_for_project(active_pid.as_deref());
                let parent_id = qc_parent_id_of(tree, &id);
                if qc_node_name_exists(tree, parent_id.flatten().as_deref(), &val, Some(&id)) {
                    self.inline_command = Some(InlineQcCommandState::Renaming {
                        id,
                        original_name,
                        input,
                    });
                    cx.notify();
                    return;
                }
                let active_pid_clone = active_pid.clone();
                settings_entity(cx).update(cx, |s, cx| {
                    let tree =
                        s.settings
                            .quick_commands_for_project_mut(active_pid_clone.as_deref());
                    if let Some(node) = qc_find_node_mut(tree, &id) {
                        if let QuickCommandNode::Command { name, .. } = node {
                            *name = val;
                        }
                    }
                    s.save_and_notify(cx);
                });
                self.reload_from_settings(cx);
            }
            cx.notify();
        }
    }

    /// 当前键盘焦点的索引（基于 `qc_visible_order`）。
    /// 优先复用显式维护的 `qc_focused_index`；仅当该索引因树结构变化而错位
    /// （越界或指向的 id 已不是当前焦点节点）时，才回退按 id 查找。
    fn current_qc_focus_index(&self) -> Option<usize> {
        if let Some(i) = self.qc_focused_index {
            if i < self.qc_visible_order.len() {
                if let Some(fid) = self.qc_focused_node_id() {
                    if self.qc_visible_order[i] == fid {
                        return Some(i);
                    }
                }
            }
        }
        self.qc_focused_node_id()
            .and_then(|id| self.qc_visible_order.iter().position(|x| x == &id))
    }

    /// 当前「键盘焦点」节点：优先返回选区锚点，否则返回任一选中项。
    fn qc_focused_node_id(&self) -> Option<String> {
        self.qc_selection_anchor
            .clone()
            .or_else(|| self.selected_qc_ids.iter().next().cloned())
    }

    /// 方向键移动焦点（同时把单选选中移动到该节点）。
    /// `delta > 0` 向下，`delta < 0` 向上。基于显式索引，免疫重复 id。
    fn move_qc_focus(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.selected_qc_ids.is_empty() || self.qc_visible_order.is_empty() {
            return;
        }
        let idx = match self.current_qc_focus_index() {
            Some(i) => i,
            None => 0,
        };
        let new_idx = if delta > 0 {
            (idx + delta as usize).min(self.qc_visible_order.len() - 1)
        } else {
            idx.saturating_sub((-delta) as usize)
        };
        let new_id = self.qc_visible_order[new_idx].clone();
        self.selected_qc_ids.clear();
        self.selected_qc_ids.insert(new_id.clone());
        self.qc_selection_anchor = Some(new_id.clone());
        self.qc_focused_index = Some(new_idx);
        self.scroll_qc_node_into_view(&new_id, cx);
        cx.notify();
    }

    /// 将指定节点行滚动进快捷指令树可视区。行高由设计系统 Compact 档统一解析、
    /// 容器内边距上侧 4px，故节点 i 的内容纵坐标 top = 4 + i*row_h、
    /// bottom = 4 + (i+1)*row_h。仅在行未完全可见时滚动，并保持最小位移。
    fn scroll_qc_node_into_view(&self, id: &str, cx: &App) {
        let Some(idx) = self.qc_visible_order.iter().position(|x| x == id) else {
            return;
        };
        let row_h: Pixels = qc_row_height(cx);
        let pad_top: Pixels = px(4.0);
        let top = pad_top + row_h * idx as f32;
        let bottom = top + row_h;

        let viewport_h = self.qc_scroll_handle.bounds().size.height;
        if viewport_h == px(0.0) {
            return; // 尚未完成布局
        }
        let cur = self.qc_scroll_handle.offset().y;
        let row_top_vp = top + cur;
        let row_bottom_vp = bottom + cur;

        let mut new_off = if row_top_vp < px(0.0) {
            -top
        } else if row_bottom_vp > viewport_h {
            viewport_h - bottom
        } else {
            return; // 已完全可见，无需滚动
        };
        let max_off = self.qc_scroll_handle.max_offset().y;
        new_off = new_off.max(-max_off).min(px(0.0));
        self.qc_scroll_handle
            .set_offset(Point::new(px(0.0), new_off));
    }

    /// 空格：对当前键盘焦点节点执行「鼠标左键单击」的等价操作——
    /// 文件夹展开/收起，命令单选。要求已有选中节点，否则无操作。
    fn activate_focused_qc(&mut self, cx: &mut Context<Self>) {
        if self.selected_qc_ids.is_empty() {
            return;
        }
        let id = match self.qc_focused_node_id() {
            Some(id) => id,
            None => return,
        };
        if Self::is_folder_node(&self.nodes, &id) {
            self.toggle_folder(&id, cx);
        } else {
            self.qc_focused_index = self.current_qc_focus_index();
            cx.notify();
        }
    }

    fn new_quick_command(&mut self, cx: &mut Context<Self>) {
        let parent_id = self.get_selected_folder_id();
        let origin = Some(self.focus_handle.clone());
        self.overlay_manager.update(cx, |om, cx| {
            om.show_quick_command_dialog_with_origin(
                QuickCommandDialogMode::CreateCommand {
                    parent_id,
                    initial_content: None,
                },
                origin.clone(),
                origin,
                cx,
            );
        });
    }

    fn new_quick_folder(&mut self, cx: &mut Context<Self>) {
        self.inline_create_folder(None, cx);
    }

    fn toggle_qc_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_qc_search = !self.show_qc_search;
        if !self.show_qc_search {
            self.filter_input = None;
        } else {
            let input = self.filter_input.get_or_insert_with(|| {
                let input = cx.new(|cx| {
                    InputState::new(cx)
                        .placeholder(i18n!(cx, "quick_commands.search_tooltip"))
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

    fn qc_drop_node(
        &mut self,
        drag: &QcDrag,
        target_id: &str,
        target_is_folder: bool,
        cx: &mut Context<Self>,
    ) {
        if drag.node_id == target_id {
            return;
        }
        let target_id = target_id.to_string();
        let active_pid = self.active_project_id(cx);
        settings_entity(cx)
            .update(cx, |s, cx| {
                let nodes = s.settings.quick_commands_for_project_mut(active_pid.as_deref());
                let target_index: Option<(Option<String>, usize)> = {
                    let parent = qc_parent_id_of(nodes, &target_id);
                    let parent_id: Option<String> = match &parent {
                        Some(Some(pid)) => Some(pid.clone()),
                        _ => None,
                    };
                    let idx = match &parent_id {
                        None => nodes.iter().position(|n| n.id() == target_id),
                        Some(pid) => qc_find_node_mut(nodes, pid).and_then(|n| {
                            if let QuickCommandNode::Folder { children, .. } = n {
                                children.iter().position(|c| c.id() == target_id)
                            } else {
                                None
                            }
                        }),
                    };
                    idx.map(|i| (parent_id, i))
                };
                if let Some((parent_id, target_index)) = target_index {
                    let (new_parent, new_index) = if !drag.is_folder && target_is_folder {
                        (Some(target_id), usize::MAX)
                    } else {
                        (parent_id, target_index)
                    };
                    qc_move_node(
                        nodes,
                        &drag.node_id,
                        new_parent.as_deref(),
                        new_index,
                    );
                    s.save_and_notify(cx);
                }
            });
        self.reload_from_settings(cx);
    }

    /// Move a dragged node to the root level (top-level of the tree). Used when
    /// the user drops a node onto the empty drop zone at the bottom of the tree,
    /// giving a reliable way to pull a node out of a folder back to the top level
    /// (otherwise a node dropped into a folder could get "stuck" with no root
    /// target to drop onto).
    fn qc_drop_node_to_root(
        &mut self,
        drag: &QcDrag,
        cx: &mut Context<Self>,
    ) {
        if drag.node_id.is_empty() {
            return;
        }
        let active_pid = self.active_project_id(cx);
        settings_entity(cx)
            .update(cx, |s, cx| {
                let nodes = s.settings.quick_commands_for_project_mut(active_pid.as_deref());
                qc_move_node(nodes, &drag.node_id, None, usize::MAX);
                s.save_and_notify(cx);
            });
        self.reload_from_settings(cx);
    }

    fn render_inline_qc_folder_input_row(
        &mut self,
        // Stable icon id of the folder being created/renamed, identical to the
        // normal folder row's `format!("qc-folder-{}", id)`. Keeps the inline
        // row's folder icon the *same* element (same open/closed state) as the
        // row it replaces — a rename must never reset or swap the icon.
        folder_icon_id: &str,
        // True when the folder is expanded, so the inline row shows the open
        // glyph (matching the pre-rename row) instead of forcing it closed.
        is_expanded: bool,
        depth: usize,
        t: &ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_ent = if let Some(ref mut state) = self.inline_folder {
            let inp = match state {
                InlineQcFolderState::Creating { input, placeholder, .. } => {
                    if input.is_none() {
                        let ph = placeholder.clone();
                        let i = cx.new(|cx| InputState::new(cx).placeholder(ph));
                        *input = Some(i);
                    }
                    input.clone()
                }
                InlineQcFolderState::Renaming { input, original_name, .. } => {
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

        let current_val = input.read(cx).text().to_string().trim().to_string();
        let active_pid = self.active_project_id(cx);
        let (parent_id, except_id) = match &self.inline_folder {
            Some(InlineQcFolderState::Creating { parent_id, .. }) => (parent_id.clone(), None),
            Some(InlineQcFolderState::Renaming { id, .. }) => {
                let qc_tree = settings_entity(cx).read(cx).settings.quick_commands_for_project(active_pid.as_deref());
                let parent_id = qc_parent_id_of(qc_tree, id).flatten();
                (parent_id, Some(id.clone()))
            }
            None => (None, None),
        };

        let is_duplicate = if !current_val.is_empty() {
            let qc_tree = settings_entity(cx).read(cx).settings.quick_commands_for_project(active_pid.as_deref());
            qc_folder_name_exists(qc_tree, parent_id.as_deref(), &current_val, except_id.as_deref())
        } else {
            false
        };

        let border_color = if is_duplicate {
            rgb(t.error)
        } else {
            rgb(t.border_active)
        };

        let ap = qc_row_appearance(t, cx);
        div()
            .id("qc-inline-folder-container")
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .id("qc-inline-folder-row")
                    .h(ap.height)
                    .pl(ui_space_md(cx) + depth as f32 * ICON_STD)
                    .pr(ui_space_md(cx))
                    .flex()
                    .items_center()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.0))
                            .child(
                                AppIcon::ChevronRight
                                    .size(ap.icon_size)
                                    .text_color(rgb(t.text_secondary)),
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
                                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
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
                                    })),
                            ),
                    ),
            )
            .when(is_duplicate, |container| {
                container.child(
                    div()
                        .my(px(2.0))
                        .ml(px(37.0) + depth as f32 * ICON_STD)
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
                                .child(i18n!(cx, "dock.duplicate_folder_error")),
                        ),
                )
            })
            .into_any_element()
    }

    /// 行内重命名命令节点的输入框行。仅修改名称，交互（Enter 提交 / Esc 取消 /
    /// 失焦提交 / 焦点不离开面板）与文件夹行内重命名及会话树完全一致。
    fn render_inline_qc_command_input_row(
        &mut self,
        // 稳定图标 id，与正常命令行的 `format!("qc-cmd-{}", id)` 一致，
        // 使行内行的图标元素与它所替换的命令行相同。
        _icon_id: &str,
        depth: usize,
        t: &ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_ent = if let Some(ref mut state) = self.inline_command {
            let InlineQcCommandState::Renaming { input, original_name, .. } = state;
            if input.is_none() {
                let name = original_name.clone();
                let i = cx.new(|cx| InputState::new(cx).default_value(name));
                *input = Some(i);
            }
            input.clone()
        } else {
            None
        };

        let Some(input) = input_ent else {
            return div().into_any_element();
        };

        if self.inline_command_sub.is_none() {
            let focus_handle = input.focus_handle(cx);
            window.focus(&focus_handle, cx);
            self.inline_command_sub = Some(cx.on_blur(&focus_handle, window, move |this, window, cx| {
                this.commit_inline_command(cx);
                if this.inline_command.is_none() {
                    window.focus(&this.focus_handle, cx);
                }
            }));
        }

        let ap = qc_row_appearance(t, cx);
        div()
            .id("qc-inline-cmd-container")
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .id("qc-inline-cmd-row")
                    .h(ap.height)
                    .pl(ui_space_md(cx) + px(depth as f32 * f32::from(velowork_ui::tokens::ui_space_tree_indent(cx))))
                    .pr(ui_space_xs(cx))
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
                        AppIcon::QuickCommand
                            .size(ap.icon_size)
                            .text_color(rgb(t.text_muted)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(velowork_ui::Input::new(&input))
                            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                if e.keystroke.key == "enter" {
                                    this.commit_inline_command(cx);
                                    if this.inline_command.is_none() {
                                        window.focus(&this.focus_handle, cx);
                                    }
                                    // 阻止 Enter 冒泡到树容器（避免触发「执行指令」）。
                                    cx.stop_propagation();
                                } else if e.keystroke.key == "escape" {
                                    this.cancel_inline_command(cx);
                                    window.focus(&this.focus_handle, cx);
                                    cx.stop_propagation();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_single_qc_node(
        &mut self,
        node: &QuickCommandNode,
        depth: usize,
        t: &ThemeColors,
        is_open: bool,
        is_selected: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            QuickCommandNode::Folder {
                id,
                name,
                children,
                ..
            } => {
                let count = children.len();
                let fname = name.clone();
                let node_clone = node.clone();
                let fid_drag = id.clone();
                let fname_drag = name.clone();
                let t_border = t.border_active;
                let t_bg = t.bg_selection;
                let ap = qc_row_appearance(t, cx);
                let font_color = if is_selected { t.text_primary } else { t.text_secondary };

                let header = div()
                    .id(ElementId::Name(format!("qc-folder-{}", id).into()))
                    .h(ap.height)
                    .pl(ui_space_md(cx) + px(depth as f32 * f32::from(velowork_ui::tokens::ui_space_tree_indent(cx))))
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
                    .when(is_selected, |d| d.border_color(rgb(t.border_active)))
                    .on_drag(
                        QcDrag {
                            node_id: fid_drag.clone(),
                            node_name: fname_drag.clone(),
                            is_folder: true,
                        },
                        move |drag, position, _window, cx| {
                            let cursor_y = position.y;
                            let cursor_x = position.x;
                            cx.new(move |_| QcDragView {
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
                            .child(
                                folder_tree_icon(
                                    &format!("qc-folder-{}", id),
                                    is_open,
                                    depth,
                                    ap.icon_size,
                                    &t,
                                ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(ap.font_size)
                                    .child(fname),
                            ),
                    )
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_muted))
                            .child(format!("{}", count)),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.handle_qc_node_click(&node_clone, true, event, window, cx);
                        cx.stop_propagation();
                    }))
                    .drag_over::<QcDrag>(move |style, drag: &QcDrag, _, _| {
                        if drag.is_folder {
                            style.border_t_2().border_color(rgb(t_border))
                        } else {
                            style.bg(rgb(t_bg))
                        }
                    })
                    .on_drop(cx.listener({
                        let target_id = id.clone();
                        move |this, drag: &QcDrag, _window, cx| {
                            this.qc_drop_node(drag, &target_id, true, cx);
                        }
                    }))
                    .on_mouse_down(MouseButton::Right, {
                        let node_clone = node.clone();
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.show_menu(
                                QuickCommandMenuTarget::Node(node_clone.clone()),
                                event.position,
                                window,
                                cx,
                            );
                        })
                    });

                let is_renaming_this = match &self.inline_folder {
                    Some(InlineQcFolderState::Renaming { id: target_id, .. }) => target_id == id,
                    _ => false,
                };

                if is_renaming_this {
                    return self.render_inline_qc_folder_input_row(
                        &format!("qc-folder-{}", id),
                        is_open,
                        depth,
                        &t,
                        window,
                        cx,
                    );
                }
                header.into_any_element()
            }
            QuickCommandNode::Command { id, name, .. } => {
                let cname = name.clone();
                let has_vars = matches!(
                    node,
                    QuickCommandNode::Command { variables, .. } if !variables.is_empty()
                );
                let cid_drag = id.clone();
                let cname_drag = name.clone();
                let t_border = t.border_active;

                let is_renaming_this_command = match &self.inline_command {
                    Some(InlineQcCommandState::Renaming { id: target_id, .. }) => target_id == id,
                    _ => false,
                };
                if is_renaming_this_command {
                    return self.render_inline_qc_command_input_row(
                        &format!("qc-cmd-{}", id),
                        depth,
                        &t,
                        window,
                        cx,
                    );
                }

                let ap = qc_row_appearance(t, cx);
                let font_color = if is_selected { t.text_primary } else { t.text_secondary };
                div()
                    .id(ElementId::Name(format!("qc-cmd-{}", id).into()))
                    .group(format!("qc-cmd-{}", id))
                    .h(ap.height)
                    .pl(ui_space_md(cx) + px(depth as f32 * f32::from(velowork_ui::tokens::ui_space_tree_indent(cx))))
                    .pr(ui_space_xs(cx))
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
                    .when(is_selected, |d| d.border_color(rgb(t.border_active)))
                    .on_drag(
                        QcDrag {
                            node_id: cid_drag.clone(),
                            node_name: cname_drag.clone(),
                            is_folder: false,
                        },
                        move |drag, position, _window, cx| {
                            let cursor_y = position.y;
                            let cursor_x = position.x;
                            cx.new(move |_| QcDragView {
                                name: drag.node_name.clone(),
                                is_folder: false,
                                cursor_offset_x: cursor_x,
                                cursor_offset_y: cursor_y,
                            })
                        },
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(ui_space_xs(cx))
                            .child(
                                div()
                                    .size(ap.icon_size)
                                    .flex_shrink_0(),
                            )
                            .child(
                                AppIcon::QuickCommand
                                    .size(ap.icon_size)
                                    .text_color(rgb(t.text_muted))
                                    .flex_shrink_0(),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(ap.font_size)
                                    .child(cname),
                            )
                            .when(has_vars, |d| {
                                d.child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.accent))
                                        .child("⚭"),
                                )
                            }),
                    )
                    .child(
                        icon_button_sized(
                            format!("qc-cmd-send-{}", id),
                            AppIcon::Send,
                            18.0,
                            12.0,
                            t,
                        )
                        .opacity(0.0)
                        .group_hover(format!("qc-cmd-{}", id), |s| s.opacity(1.0))
                        .on_click(cx.listener({
                            let node_send = node.clone();
                            move |this, _, window, cx| {
                                this.on_command_click(&node_send, Some(window), cx);
                            }
                        })),
                    )
                    .on_click(cx.listener({
                        let node_clone = node.clone();
                        move |this, event: &ClickEvent, window, cx| {
                            this.handle_qc_node_click(&node_clone, false, event, window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .drag_over::<QcDrag>(move |style, drag: &QcDrag, _, _| {
                        if drag.is_folder {
                            style.border_b_2().border_color(rgb(t_border))
                        } else {
                            style.border_t_2().border_color(rgb(t_border))
                        }
                    })
                    .on_drop(cx.listener({
                        let target_id = id.clone();
                        move |this, drag: &QcDrag, _window, cx| {
                            this.qc_drop_node(drag, &target_id, false, cx);
                        }
                    }))
                    .on_mouse_down(MouseButton::Right, {
                        let node_clone = node.clone();
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.show_menu(
                                QuickCommandMenuTarget::Node(node_clone.clone()),
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

impl Focusable for QuickCommandsPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for QuickCommandsPanel {
    fn metadata(&self, cx: &App) -> PanelInfo {
        PanelInfo::new(
            "quick_commands",
            i18n!(cx, "quick_commands.panel"),
            AppIcon::QuickCommand,
            velowork_ui::dock::types::PanelKind::Custom,
        )
    }

    fn custom_actions(&self, _cx: &App) -> Vec<PanelAction> {
        Vec::new()
    }

    fn focus_handle(&self, _cx: &App) -> Option<FocusHandle> {
        Some(self.focus_handle.clone())
    }
}

impl Render for QuickCommandsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        if self.show_qc_search && self.filter_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "quick_commands.search_tooltip"))
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
        qc_collect_visible_ids(&roots, &filter_text, force, &mut visible_order);
        self.qc_visible_order = visible_order;

        let tree_nodes = convert_qc_tree_nodes(&roots);
        let mut expanded_keys = std::collections::HashSet::new();
        collect_qc_expanded_keys(&roots, force, &mut expanded_keys);

        let is_panel_active = self.is_selection_active(window, cx);
        let active_selected_keys = if is_panel_active {
            self.selected_qc_ids.clone()
        } else {
            std::collections::HashSet::new()
        };

        let t_color = t.clone();
        let filter_clone = filter_text.clone();
        let tree_widget = velowork_ui::tree::<Self, String, QuickCommandNode>("qc-tree")
            .nodes(tree_nodes)
            .expanded_keys(expanded_keys)
            .selected_keys(active_selected_keys)
            .render_before_children({
                let t_color = t_color.clone();
                move |this, parent_key, depth, window, cx| {
                    let parent_id = parent_key.map(|k| k.as_str());
                    let is_creating_here = match &this.inline_folder {
                        Some(InlineQcFolderState::Creating { parent_id: target_pid, .. }) => {
                            target_pid.as_deref() == parent_id
                        }
                        _ => false,
                    };
                    if is_creating_here {
                        return Some(this.render_inline_qc_folder_input_row(
                            "qc-inline-folder-create",
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
                    if !filter_str.is_empty() && !node_matches(node, &filter_str) {
                        return div().into_any_element();
                    }
                    this.render_single_qc_node(
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

        let tree_children = vec![tree_widget.render(self, window, cx)];
        let is_empty = roots.is_empty();

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
                if this.inline_folder.is_none() && this.inline_command.is_none() {
                    if let Some(id) = this.selected_qc_ids.iter().next().cloned() {
                        this.rename_selected_qc_node(&id, cx);
                    }
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // 行内编辑（新建/重命名）进行时，输入框自行处理按键，面板不拦截，
                // 避免与文本输入（如 Delete 删除字符）冲突。
                if this.inline_folder.is_some() || this.inline_command.is_some() {
                    return;
                }
                let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                if cmd_or_ctrl && event.keystroke.key.as_str() == "f" {
                    this.show_qc_search = true;
                    let input = this.filter_input.get_or_insert_with(|| {
                        let input = cx.new(|cx| {
                            InputState::new(cx)
                                .placeholder(i18n!(cx, "quick_commands.search_tooltip"))
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

                // 方向键导航等要求焦点在面板上且已有选中节点。
                let panel_focused = this.focus_handle.is_focused(window);
                match event.keystroke.key.as_str() {
                    // 方向键导航：前提是焦点在面板上且已有选中节点。
                    "down" | "up" => {
                        if !panel_focused {
                            return;
                        }
                        if this.selected_qc_ids.is_empty() {
                            if let Some(first_id) = this.qc_visible_order.first().cloned() {
                                this.selected_qc_ids.insert(first_id);
                                this.qc_focused_index = Some(0);
                                cx.notify();
                            }
                            cx.stop_propagation();
                            return;
                        }
                        this.move_qc_focus(
                            if event.keystroke.key == "down" { 1 } else { -1 },
                            cx,
                        );
                        cx.stop_propagation();
                    }
                    "right" => {
                        if !panel_focused {
                            return;
                        }
                        if let Some(id) = this.qc_focused_node_id() {
                            if Self::is_folder_node(&this.nodes, &id) {
                                this.toggle_folder(&id, cx);
                            } else {
                                this.move_qc_focus(1, cx);
                            }
                            cx.stop_propagation();
                        }
                    }
                    "left" => {
                        if !panel_focused {
                            return;
                        }
                        if let Some(id) = this.qc_focused_node_id() {
                            if Self::is_folder_node(&this.nodes, &id) {
                                this.toggle_folder(&id, cx);
                            } else {
                                this.move_qc_focus(-1, cx);
                            }
                            cx.stop_propagation();
                        }
                    }
                    // 空格：对当前焦点节点执行鼠标左键单击的等价操作
                    // （文件夹展开/收起，命令单选）。
                    "space" => {
                        if !panel_focused {
                            return;
                        }
                        if this.selected_qc_ids.is_empty() {
                            if let Some(first_id) = this.qc_visible_order.first().cloned() {
                                this.selected_qc_ids.insert(first_id);
                                this.qc_focused_index = Some(0);
                                cx.notify();
                            }
                            cx.stop_propagation();
                            return;
                        }
                        this.activate_focused_qc(cx);
                        cx.stop_propagation();
                    }
                    // Enter：对当前焦点节点执行默认操作（命令执行，文件夹展开/收起）。
                    "enter" => {
                        if !panel_focused || this.selected_qc_ids.is_empty() {
                            return;
                        }
                        let id = this.qc_focused_node_id();
                        if let Some(id) = id {
                            let node = {
                                let settings_entity = crate::settings::settings_entity(cx);
                                let s = settings_entity.read(cx);
                                let tree = s
                                    .settings
                                    .quick_commands_for_project(this.active_project_id(cx).as_deref());
                                qc_find_node_ref(tree, &id).cloned()
                            };
                            if let Some(node) = node {
                                if node.is_folder() {
                                    this.toggle_folder(&id, cx);
                                } else {
                                    this.on_command_click(&node, Some(window), cx);
                                }
                            }
                        }
                        cx.stop_propagation();
                    }
                    // Esc：清空当前选择（与行内重命名的 Esc 取消区分：此处无行内编辑）。
                    "escape" => {
                        if this.show_qc_search {
                            this.show_qc_search = false;
                            if let Some(ref input) = this.filter_input {
                                input.update(cx, |inp, cx| inp.set_value("", cx));
                            }
                            window.focus(&this.focus_handle, cx);
                            cx.stop_propagation();
                            cx.notify();
                            return;
                        }
                        if !this.selected_qc_ids.is_empty() {
                            this.clear_qc_selection(cx);
                            cx.stop_propagation();
                        }
                    }
                    // Delete：删除当前选中的所有节点（弹窗确认）。
                    "delete" => {
                        if !this.selected_qc_ids.is_empty() {
                            let ids: Vec<String> =
                                this.selected_qc_ids.iter().cloned().collect();
                            let origin = Some(this.focus_handle.clone());
                            this.overlay_manager.update(cx, |om, cx| {
                                om.request_quick_command_delete_confirm_with_origin(ids, origin.clone(), origin, cx);
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
                    .px(ui_space_md(cx))
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .items_center()
                    .gap(ui_space_xs(cx))
                    .child(
                        velowork_ui::icon_button::icon_button("btn-add-qc-cmd", AppIcon::Plus, &t, cx)
                            .tooltip(move |_, cx| {
                                let __tip = i18n!(cx, "quick_commands.new_command");
                                cx.new(|_| Tooltip::new(__tip)).into()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus_handle, cx);
                                this.new_quick_command(cx);
                            })),
                    )
                    .child(
                        velowork_ui::icon_button::icon_button("btn-add-qc-folder", AppIcon::NewFolder, &t, cx)
                            .tooltip(move |_, cx| {
                                let __tip = i18n!(cx, "common.new_folder");
                                cx.new(|_| Tooltip::new(__tip)).into()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus_handle, cx);
                                this.new_quick_folder(cx);
                            })),
                    )
                    .child(div().w(px(1.0)).h(ICON_STD).bg(p.border_subtle).mx(ui_space_xs(cx)))
                    .child(
                        velowork_ui::icon_button::icon_button("btn-qc-search", AppIcon::Search, &t, cx)
                            .when(self.show_qc_search, |b| b.bg(surface_bg_t(t.bg_hover, &t)))
                            .tooltip(move |_, cx| {
                                let __tip = i18n!(cx, "quick_commands.search_tooltip");
                                cx.new(|_| Tooltip::new(__tip)).into()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_qc_search(window, cx);
                            })),
                    ),
            )
            .when(self.show_qc_search, |d| {
                d.child(
                    div().px(ui_space_md(cx)).py(ui_space_sm(cx)).when_some(self.filter_input.as_ref(), |this, inp| {
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
                            .id("qc-tree")
                            .size_full()
                            .track_scroll(&self.qc_scroll_handle)
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .py(ui_space_xs(cx))
                            .px(ui_space_xs(cx))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    this.show_menu(
                                        QuickCommandMenuTarget::Root,
                                        event.position,
                                        window,
                                        cx,
                                    );
                                }),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_qc_selection(cx);
                                // 聚焦面板，使方向键导航在点击空白区域后仍可用。
                                window.focus(&this.focus_handle, cx);
                            }))
                            .when(is_empty, |d| {
                                let empty_msg = if !filter_text.is_empty() {
                                    i18n!(cx, "quick_commands.no_match")
                                } else {
                                    i18n!(cx, "quick_commands.empty_list")
                                };
                                d.child(velowork_ui::empty_state::empty_state(empty_msg, &t, cx))
                            })
                            .children(tree_children)
                            // 树底部常驻的根级放置区：拖拽节点悬停于此并释放，即把该节点
                            // 移回顶层（根目录）。这样把一个节点拖入某目录后，仍有明确的
                            // 目标可将其拖出，避免"拖进去就拖不出来"的问题。高亮反馈与
                            // 目录悬停一致。
                            .child(
                                div()
                                    .id("qc-tree-dropzone")
                                    .flex_1()
                                    .min_h(px(24.0))
                                    .drag_over::<QcDrag>(move |style, _, _, _| {
                                        style.bg(rgb(t.bg_hover))
                                    })
                                    .on_drop(cx.listener(move |this, drag: &QcDrag, _window, cx| {
                                        this.qc_drop_node_to_root(drag, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .left_0()
                            .child(Scrollbar::vertical(&self.qc_scroll_handle)),
                    ),
            )
            .when_some(self.context_menu.clone(), |d, menu| d.child(menu))
    }
}

// ── Quick-command tree drag-and-drop types ──────────────────────────────

/// Drag payload for a node (command or folder) in the quick-command tree.
#[derive(Clone)]
pub struct QcDrag {
    pub node_id: String,
    pub node_name: String,
    pub is_folder: bool,
}

/// Drag preview for a quick-command node (the floating card that follows the cursor).
pub struct QcDragView {
    pub name: String,
    pub is_folder: bool,
    pub cursor_offset_x: Pixels,
    pub cursor_offset_y: Pixels,
}

impl Render for QcDragView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div()
            .child(
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
                            AppIcon::QuickCommand
                        })
                        .size(ui_space_lg(cx))
                        .text_color(rgb(t.text_muted)),
                    )
                    .child(self.name.clone()),
            )
    }
}

/// Whether a node (or any descendant) matches the filter text.
pub(crate) fn node_matches(node: &QuickCommandNode, filter: &str) -> bool {
    match node {
        QuickCommandNode::Command { name, command, .. } => {
            name.to_lowercase().contains(filter)
                || command.to_lowercase().contains(filter)
        }
        QuickCommandNode::Folder { name, children, .. } => {
            name.to_lowercase().contains(filter)
                || children.iter().any(|c| node_matches(c, filter))
        }
    }
}

/// Collect the ids of all currently visible quick-command nodes in
/// display order (a folder id is emitted, then its children when
/// expanded). Traverses exactly what is displayed on-screen.
pub(crate) fn qc_collect_visible_ids(
    nodes: &[QuickCommandNode],
    filter: &str,
    force: bool,
    out: &mut Vec<String>,
) {
    let mut ordered: Vec<&QuickCommandNode> = nodes.iter().collect();
    ordered.sort_by(|a, b| {
        let a_folder = matches!(a, QuickCommandNode::Folder { .. });
        let b_folder = matches!(b, QuickCommandNode::Folder { .. });
        b_folder.cmp(&a_folder)
    });
    for node in ordered {
        if !force && !node_matches(node, filter) {
            continue;
        }
        match node {
            QuickCommandNode::Folder { id, expanded, children, .. } => {
                out.push(id.clone());
                if *expanded || force {
                    qc_collect_visible_ids(children, filter, force, out);
                }
            }
            QuickCommandNode::Command { id, .. } => {
                out.push(id.clone());
            }
        }
    }
}

/// Send a command string to the currently focused terminal.
pub(crate) fn send_command_to_focused_terminal(
    focus_manager: &Entity<FocusManager>,
    workspace: &Entity<Workspace>,
    terminals: &TerminalsRegistry,
    cmd: &str,
    cx: &App,
) {
    let terminal_id = focus_manager
        .read(cx)
        .focused_terminal_state()
        .and_then(|state| {
            workspace
                .read(cx)
                .project(&state.project_id)
                .and_then(|p| p.layout.as_ref())
                .and_then(|layout| layout.get_at_path(&state.layout_path))
                .and_then(|node| match node {
                    velowork_workspace::state::LayoutNode::Terminal { terminal_id, .. } => {
                        terminal_id.clone()
                    }
                    _ => None,
                })
        });

    if let Some(id) = terminal_id {
        let terminals = terminals.lock();
        if let Some(terminal) = terminals.get(&id) {
            let mut cmd_str = cmd.to_string();
            if !cmd_str.ends_with('\n') && !cmd_str.ends_with('\r') {
                cmd_str.push('\r');
            }
            terminal.send_bytes(cmd_str.as_bytes());
        }
    }
}

/// Register Quick Commands panel to the right toolbar.
pub fn register_toolbar_panel(registry: &mut velowork_ui::dock::RightToolbarRegistry) {
    registry.register(velowork_ui::dock::ToolbarPanelSpec {
        id: "quick_commands".to_string(),
        icon: AppIcon::QuickCommand,
        title_key: "dock.panel.quick_commands".to_string(),
        order: 20,
        is_visible: std::sync::Arc::new(|_cx| true),
        factory: std::sync::Arc::new(|ctx, _window, cx| {
            let app_ctx = ctx
                .downcast_ref::<super::AppPanelCreationContext>()
                .expect("AppPanelCreationContext required");
            let workspace = app_ctx.workspace.clone();
            let focus_manager = app_ctx.focus_manager.clone();
            let terminals = app_ctx.terminals.clone();
            let overlay_manager = app_ctx.overlay_manager.clone();
            let overlay_reg = app_ctx.overlay_registry.clone();
            let p = cx.new(|cx| {
                let mut qp = QuickCommandsPanel::new(workspace, focus_manager, terminals, overlay_manager, cx);
                qp.set_overlay_registry(overlay_reg);
                qp
            });
            velowork_ui::dock::AnyPanel::new(p)
        }),
    });
}