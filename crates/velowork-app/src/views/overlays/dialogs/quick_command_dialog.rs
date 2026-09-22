//! Quick-command edit / create dialog overlay ("快捷指令" editor).

use crate::keybindings::Cancel;
use crate::settings::settings;
use crate::settings::settings_entity;
use crate::views::components::{modal_content, modal_header};
use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::dialog_actions::dialog_actions;
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::form::form_item;
use velowork_ui::icon::AppIcon;
use velowork_ui::input::{InputEvent, InputFocusRingExt, InputState, TextareaState};
use velowork_ui::overlay::CloseEvent;
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::select::{Select, SelectEvent, SelectOption, SelectState};
use velowork_ui::theme::theme;
use velowork_ui::tokens::{
    mono_font_family, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, ui_text_md, ui_text_sm,
};
use velowork_ui::icon_button::{icon_button, FocusableIconButtonExt};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex};
use velowork_workspace::folder_path::parse_and_validate_folder_path;
use velowork_workspace::quick_commands::{
    extract_quick_command_vars, find_invalid_var_placeholder, find_unclosed_var_placeholder,
    has_empty_var_placeholder, new_quick_command_id, qc_collect_folders, qc_ensure_folder_path,
    qc_find_node_mut, qc_insert_node, qc_node_name_exists, qc_parent_id_of, qc_remove_node,
    QuickCommandNode, QuickCommandVar,
};

/// Mode the dialog is operating in.
pub enum QuickCommandDialogMode {
    /// Create a new command under `parent_id` (or at root when `None`), optionally with initial content prefilled.
    CreateCommand {
        parent_id: Option<String>,
        initial_content: Option<String>,
    },
    /// Create a new folder under `parent_id` (or at root when `None`).
    CreateFolder { parent_id: Option<String> },
    /// Edit an existing command (node is a clone of the stored one).
    EditCommand { node: QuickCommandNode },
    /// Edit an existing folder (node is a clone of the stored one).
    EditFolder { node: QuickCommandNode },
}

/// One editable variable row in the dialog.
struct VarDraft {
    name: String,
    default_value: Entity<InputState>,
    hint: Entity<InputState>,
    /// 删除该变量行的按钮焦点句柄（键盘导航）。
    remove_focus: FocusHandle,
    /// 是否在指令模板中已无引用（已填写内容的草稿保留并警示）。
    unreferenced: bool,
}

pub struct QuickCommandDialog {
    mode: QuickCommandDialogMode,
    project_id: Option<String>,
    focus_handle: FocusHandle,
    /// Persistent focus handle for the Cancel action button (keyboard nav).
    cancel_focus: FocusHandle,
    /// Persistent focus handle for the Confirm/Save action button (keyboard nav).
    confirm_focus: FocusHandle,
    /// Persistent focus handle for the "添加变量" button (keyboard nav).
    add_var_focus: FocusHandle,
    /// Persistent focus handle for the new directory button (keyboard nav).
    new_dir_focus: FocusHandle,
    /// Persistent focus handle for the directory inline confirm button (keyboard nav).
    dir_confirm_focus: FocusHandle,
    /// Persistent focus handle for the directory inline cancel button (keyboard nav).
    dir_cancel_focus: FocusHandle,
    prefill_name: String,
    prefill_content: String,
    name_input: Option<Entity<InputState>>,
    content_input: Option<Entity<TextareaState>>,
    /// Selected parent folder id for a command (`None` = root level).
    directory: Option<String>,
    /// 窗口级 `OverlayRegistry`，用于「所属目录」下拉的 ClickOutside 自动收起。
    overlay_registry: Option<Entity<OverlayRegistry>>,
    variables: Vec<VarDraft>,
    initial_focus_done: bool,
    /// 「所属目录」下拉选择组件（替代原有手写下拉）。
    directory_select: Entity<SelectState<SharedString>>,
    /// 名称错误提示（如名称在同目录已存在），绑定在名称输入框下方。
    name_error_msg: Option<String>,
    /// 指令内容错误提示（如未闭合占位符、空占位符、非法字符），绑定在指令内容下方。
    content_error_msg: Option<String>,
    creating_directory: bool,
    directory_input: Option<Entity<InputState>>,
    scroll_handle: ScrollHandle,
    previous_focus_handle: Option<FocusHandle>,
}

pub enum QuickCommandDialogEvent {
    Close,
    Created { id: String },
}

impl EventEmitter<QuickCommandDialogEvent> for QuickCommandDialog {}

impl CloseEvent for QuickCommandDialogEvent {
    fn is_close(&self) -> bool {
        matches!(self, Self::Close)
    }
}

impl QuickCommandDialog {
    pub fn new(
        mode: QuickCommandDialogMode,
        project_id: Option<String>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Prefill when editing or creating with initial_content.
        let (prefill_name, prefill_content, prefill_vars, prefill_dir) = match &mode {
            QuickCommandDialogMode::CreateCommand { parent_id, initial_content } => {
                (
                    String::new(),
                    initial_content.clone().unwrap_or_default(),
                    vec![],
                    parent_id.clone(),
                )
            }
            QuickCommandDialogMode::CreateFolder { parent_id } => {
                (String::new(), String::new(), vec![], parent_id.clone())
            }
            QuickCommandDialogMode::EditCommand { node } => {
                let dir = qc_parent_id_of(&settings(cx).quick_commands, node.id()).flatten();
                let vars = match node {
                    QuickCommandNode::Command { variables, .. } => variables.clone(),
                    _ => vec![],
                };
                (
                    node.name().to_string(),
                    match node {
                        QuickCommandNode::Command { command, .. } => command.clone(),
                        _ => String::new(),
                    },
                    vars,
                    dir,
                )
            }
            QuickCommandDialogMode::EditFolder { node } => {
                // 回填当前父目录，避免编辑目录时丢失其所属层级。
                let dir = qc_parent_id_of(&settings(cx).quick_commands, node.id()).flatten();
                (node.name().to_string(), String::new(), vec![], dir)
            }
        };

        let directory_select = cx.new(|cx| SelectState::new(cx));

        let name_placeholder = i18n!(cx, "quick_commands.name_placeholder");
        let name_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(name_placeholder)
                .default_value(&prefill_name)
        });

        let is_command = matches!(
            &mode,
            QuickCommandDialogMode::CreateCommand { .. }
                | QuickCommandDialogMode::EditCommand { .. }
        );

        let content_input = if is_command {
            let placeholder = i18n!(cx, "quick_commands.content_placeholder");
            let input = cx.new(|cx| {
                TextareaState::new(cx)
                    .multiline()
                    .highlight_vars()
                    .placeholder(placeholder)
                    .default_value(&prefill_content)
            });
            Some(input)
        } else {
            None
        };

        let mut initial_vars = Vec::new();
        if is_command {
            let current_vars = extract_quick_command_vars(&prefill_content);
            let def_placeholder = i18n!(cx, "quick_commands.var_default");
            let hint_placeholder = i18n!(cx, "quick_commands.var_hint");

            for v in &prefill_vars {
                let is_referenced = current_vars.contains(&v.name);
                let def_val = v.default_value.clone();
                let hint_val = v.hint.clone();
                let def_inp = cx.new(|cx| {
                    InputState::new(cx)
                        .placeholder(def_placeholder.clone())
                        .default_value(&def_val)
                });
                let hint_inp = cx.new(|cx| {
                    InputState::new(cx)
                        .placeholder(hint_placeholder.clone())
                        .default_value(&hint_val)
                });
                initial_vars.push(VarDraft {
                    name: v.name.clone(),
                    default_value: def_inp,
                    hint: hint_inp,
                    remove_focus: cx.focus_handle(),
                    unreferenced: !is_referenced,
                });
            }

            for var_name in &current_vars {
                if !initial_vars.iter().any(|d| &d.name == var_name) {
                    let def_inp = cx.new(|cx| {
                        InputState::new(cx).placeholder(def_placeholder.clone())
                    });
                    let hint_inp = cx.new(|cx| {
                        InputState::new(cx).placeholder(hint_placeholder.clone())
                    });
                    initial_vars.push(VarDraft {
                        name: var_name.clone(),
                        default_value: def_inp,
                        hint: hint_inp,
                        remove_focus: cx.focus_handle(),
                        unreferenced: false,
                    });
                }
            }
        }

        Self {
            mode,
            project_id,
            focus_handle: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            add_var_focus: cx.focus_handle(),
            new_dir_focus: cx.focus_handle(),
            dir_confirm_focus: cx.focus_handle(),
            dir_cancel_focus: cx.focus_handle(),
            prefill_name,
            prefill_content,
            name_input: Some(name_input),
            content_input,
            directory: prefill_dir,
            overlay_registry,
            variables: initial_vars,
            initial_focus_done: false,
            directory_select,
            name_error_msg: None,
            content_error_msg: None,
            creating_directory: false,
            directory_input: None,
            scroll_handle: ScrollHandle::new(),
            previous_focus_handle: None,
        }
    }

    fn close(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if let Some(window) = window {
            if let Some(prev) = self.previous_focus_handle.take() {
                window.focus(&prev, cx);
            }
        }
        cx.emit(QuickCommandDialogEvent::Close);
    }

    /// 订阅「所属目录」Select 与输入框的变更事件。
    /// 仅在 Entity 创建后调用一次（由 `show_quick_command_dialog` 负责）。
    pub fn setup_selects(&mut self, cx: &mut Context<Self>) {
        if let Some(reg) = self.overlay_registry.clone().or_else(|| OverlayRegistry::global(cx)) {
            self.directory_select.update(cx, |s, _| s.set_overlay_registry(reg));
        }
        let dir = self.directory_select.clone();
        cx.subscribe(
            &dir,
            move |this, _st, ev: &SelectEvent<SharedString>, cxx| {
                let SelectEvent::Change(v) = ev;
                let val = v.clone().unwrap_or_default().to_string();
                this.directory = if val.is_empty() { None } else { Some(val) };
                cxx.notify();
            },
        )
        .detach();

        if let Some(name_input) = &self.name_input {
            cx.subscribe(name_input, |this, _emitter, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) && this.name_error_msg.is_some() {
                    this.name_error_msg = None;
                    cx.notify();
                }
            })
            .detach();
        }

        if let Some(content_input) = &self.content_input {
            cx.subscribe(content_input, |this, _emitter, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    if this.content_error_msg.is_some() {
                        this.content_error_msg = None;
                    }
                    this.sync_variables_with_content(cx);
                }
            })
            .detach();
        }
    }

    /// 将当前父目录同步到「所属目录」Select 组件（含动态文件夹列表）。
    pub fn refresh_selects(&mut self, cx: &mut Context<Self>) {
        let mut folders = vec![];
        let s = settings(cx);
        let qc_tree = s.quick_commands_for_project(self.project_id.as_deref());
        qc_collect_folders(qc_tree, "", &mut folders);
        // 编辑文件夹时排除自身及其子树，避免循环嵌套。
        let excluded = self.excluded_folder_ids();
        folders.retain(|(id, _)| !excluded.contains(id));

        let mut opts: Vec<SelectOption<SharedString>> = vec![SelectOption::new(
            SharedString::from(""),
            i18n!(cx, "quick_commands.no_directory"),
        )];
        for (id, label) in folders {
            opts.push(SelectOption::new(SharedString::from(id), label));
        }
        let cur = self.directory.clone().unwrap_or_default();
        self.directory_select.update(cx, |st, cx| {
            st.set_options(opts, cx);
            st.set_selected_value(Some(SharedString::from(cur)), cx);
        });
    }

    fn is_command(&self) -> bool {
        matches!(
            self.mode,
            QuickCommandDialogMode::CreateCommand { .. }
                | QuickCommandDialogMode::EditCommand { .. }
        )
    }

    fn title(&self, cx: &App) -> String {
        match &self.mode {
            QuickCommandDialogMode::CreateCommand { .. } => i18n!(cx, "quick_commands.new_command"),
            QuickCommandDialogMode::CreateFolder { .. } => i18n!(cx, "workspace.folder.create"),
            QuickCommandDialogMode::EditCommand { .. }
            | QuickCommandDialogMode::EditFolder { .. } => i18n!(cx, "common.action.edit"),
        }
    }

    /// 根据指令文本内容自动同步变量定义列表。
    fn sync_variables_with_content(&mut self, cx: &mut Context<Self>) {
        let Some(content_input) = &self.content_input else {
            return;
        };
        let content = content_input.read(cx).text().to_string();
        let current_vars = extract_quick_command_vars(&content);

        let mut kept_drafts: Vec<VarDraft> = Vec::new();
        let mut unreferenced_drafts: Vec<VarDraft> = Vec::new();

        for var_name in &current_vars {
            if let Some(pos) = self.variables.iter().position(|v| &v.name == var_name) {
                let mut draft = self.variables.remove(pos);
                draft.unreferenced = false;
                kept_drafts.push(draft);
            } else {
                let def_placeholder = i18n!(cx, "quick_commands.var_default");
                let hint_placeholder = i18n!(cx, "quick_commands.var_hint");
                let default_value = cx.new(|cx| InputState::new(cx).placeholder(def_placeholder));
                let hint = cx.new(|cx| InputState::new(cx).placeholder(hint_placeholder));
                kept_drafts.push(VarDraft {
                    name: var_name.clone(),
                    default_value,
                    hint,
                    remove_focus: cx.focus_handle(),
                    unreferenced: false,
                });
            }
        }

        for mut draft in self.variables.drain(..) {
            let has_content = !draft.default_value.read(cx).text().trim().is_empty()
                || !draft.hint.read(cx).text().trim().is_empty();
            if has_content {
                draft.unreferenced = true;
                unreferenced_drafts.push(draft);
            }
        }

        kept_drafts.extend(unreferenced_drafts);
        self.variables = kept_drafts;
        cx.notify();
    }

    /// 点击变量胶囊在指令文本框中循环定位并选中该占位符。
    fn locate_variable_in_content(&self, var_name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(content_input) = &self.content_input else {
            return;
        };
        let target = format!("{{{{{}}}}}", var_name);
        let text = content_input.read(cx).text().to_string();
        let cur_sel = content_input.read(cx).selection();
        let occurrences: Vec<usize> = text.match_indices(&target).map(|(idx, _)| idx).collect();
        if occurrences.is_empty() {
            content_input.update(cx, |input, cx| {
                input.focus(window, cx);
            });
            return;
        }
        let next_idx = if let Some(sel) = cur_sel {
            occurrences.iter().find(|&&idx| idx > sel.start).copied().unwrap_or(occurrences[0])
        } else {
            occurrences[0]
        };
        let end = next_idx + target.len();
        content_input.update(cx, |input, cx| {
            input.set_selection(Some(next_idx..end), false, cx);
            input.focus(window, cx);
        });
    }

    /// 移除指定下标的变量定义，并逆向从指令文本中清理掉对应的占位符。
    fn remove_variable_at(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.variables.len() {
            return;
        }
        let removed = self.variables.remove(idx);
        let placeholder = format!("{{{{{}}}}}", removed.name);
        if let Some(content_input) = &self.content_input {
            let text = content_input.read(cx).text().to_string();
            if text.contains(&placeholder) {
                let new_text = text.replace(&placeholder, "");
                content_input.update(cx, |input, cx| {
                    input.set_value(new_text, cx);
                });
            }
        }
        cx.notify();
    }

    /// 在指令框当前光标处插入新变量模板，并自动聚焦重命名。
    fn add_variable(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(content_input) = &self.content_input else {
            return;
        };
        let existing_names: Vec<String> = self.variables.iter().map(|v| v.name.clone()).collect();
        let next_name = if !existing_names.iter().any(|n| n == "variable") {
            "variable".to_string()
        } else {
            let mut i = 1;
            loop {
                let cand = format!("variable_{}", i);
                if !existing_names.iter().any(|n| n == &cand) {
                    break cand;
                }
                i += 1;
            }
        };

        content_input.update(cx, |input, cx| {
            input.insert_variable_template(&next_name, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name_error_msg = None;
        self.content_error_msg = None;
        let name = self
            .name_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.prefill_name.clone())
            .trim()
            .to_string();
        if name.is_empty() {
            if let Some(name_inp) = &self.name_input {
                name_inp.update(cx, |input, cx| input.focus(window, cx));
            }
            return;
        }

        let is_command = self.is_command();
        let command = if is_command {
            self.content_input
                .as_ref()
                .map(|i| i.read(cx).text().to_string())
                .unwrap_or_else(|| self.prefill_content.clone())
        } else {
            String::new()
        };

        if is_command {
            if find_unclosed_var_placeholder(&command) {
                self.content_error_msg = Some(i18n!(cx, "quick_commands.err_unclosed_variable"));
                if let Some(ci) = &self.content_input {
                    ci.update(cx, |input, cx| input.focus(window, cx));
                }
                cx.notify();
                return;
            }
            if has_empty_var_placeholder(&command) {
                self.content_error_msg = Some(i18n!(cx, "quick_commands.err_empty_variable"));
                if let Some(ci) = &self.content_input {
                    ci.update(cx, |input, cx| input.focus(window, cx));
                }
                cx.notify();
                return;
            }
            if let Some(invalid_name) = find_invalid_var_placeholder(&command) {
                self.content_error_msg = Some(
                    i18n!(cx, "quick_commands.err_invalid_variable")
                        .replace("{name}", &invalid_name),
                );
                if let Some(ci) = &self.content_input {
                    ci.update(cx, |input, cx| input.focus(window, cx));
                }
                cx.notify();
                return;
            }
        }

        let variables: Vec<QuickCommandVar> = if is_command {
            self.variables
                .iter()
                .filter(|v| !v.unreferenced)
                .map(|v| QuickCommandVar {
                    name: v.name.clone(),
                    default_value: v.default_value.read(cx).text().to_string(),
                    hint: v.hint.read(cx).text().to_string(),
                })
                .collect()
        } else {
            vec![]
        };

        let parent = self.directory.clone();

        // 同目录（含文件夹、指令）重名校验：文件名不要求唯一时跳过。
        // 编辑场景排除节点自身；不同目录之间允许重名。
        if matches!(
            &self.mode,
            QuickCommandDialogMode::CreateCommand { .. }
                | QuickCommandDialogMode::EditCommand { .. }
        ) {
            let tree = settings_entity(cx)
                .read(cx)
                .settings
                .quick_commands_for_project(self.project_id.as_deref());
            let (except_id, check_parent): (Option<String>, Option<String>) = match &self.mode {
                QuickCommandDialogMode::CreateCommand { .. } => (None, parent.clone()),
                QuickCommandDialogMode::EditCommand { node } => {
                    let p = qc_parent_id_of(tree, node.id());
                    (Some(node.id().to_string()), p.flatten())
                }
                _ => (None, None),
            };
            if qc_node_name_exists(tree, check_parent.as_deref(), &name, except_id.as_deref()) {
                self.name_error_msg =
                    Some(i18n!(cx, "sftp.dialog.name_exists").replace("{name}", &name));
                if let Some(name_inp) = &self.name_input {
                    name_inp.update(cx, |input, cx| input.focus(window, cx));
                }
                cx.notify();
                return;
            }
        }

        let (remove_id, new_node) = match &self.mode {
            QuickCommandDialogMode::CreateCommand { .. } => (
                None,
                QuickCommandNode::Command {
                    id: new_quick_command_id(),
                    name,
                    command,
                    variables,
                },
            ),
            QuickCommandDialogMode::CreateFolder { .. } => (
                None,
                QuickCommandNode::Folder {
                    id: new_quick_command_id(),
                    name,
                    expanded: true,
                    children: vec![],
                },
            ),
            QuickCommandDialogMode::EditCommand { node }
            | QuickCommandDialogMode::EditFolder { node } => {
                let id = node.id().to_string();
                let new_node = if node.is_folder() {
                    QuickCommandNode::Folder {
                        id: id.clone(),
                        name,
                        expanded: true,
                        children: match node {
                            QuickCommandNode::Folder { children, .. } => children.clone(),
                            _ => vec![],
                        },
                    }
                } else {
                    QuickCommandNode::Command {
                        id: id.clone(),
                        name,
                        command,
                        variables,
                    }
                };
                (Some(id), new_node)
            }
        };

        let created_id = new_node.id().to_string();
        let pid = self.project_id.clone();
        settings_entity(cx).update(cx, move |s, cx| {
            let tree = s.settings.quick_commands_for_project_mut(pid.as_deref());
            if let Some(rid) = &remove_id {
                qc_remove_node(tree, rid);
            }
            if let Some(ref p_id) = parent {
                if let Some(pnode) = qc_find_node_mut(tree, p_id) {
                    if let QuickCommandNode::Folder { expanded, .. } = pnode {
                        *expanded = true; // Auto expand parent folder
                    }
                }
            }
            qc_insert_node(tree, parent.as_deref(), new_node);
            s.save_and_notify(cx);
        });

        cx.emit(QuickCommandDialogEvent::Created { id: created_id });
        self.close(Some(window), cx);
    }

    fn render_directory_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        if self.creating_directory {
            let confirm_tip = i18n!(cx, "common.action.confirm");
            let cancel_tip = i18n!(cx, "common.action.cancel");
            let confirm = icon_button("dir-confirm", AppIcon::Check, &t, cx)
                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(confirm_tip.clone())).into())
                .focus_action(&self.dir_confirm_focus, &t, cx.listener(move |this, _, window, cx| {
                    this.commit_new_directory(window, cx);
                }));
            let cancel = icon_button("dir-cancel", AppIcon::Close, &t, cx)
                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(cancel_tip.clone())).into())
                .focus_action(&self.dir_cancel_focus, &t, cx.listener(move |this, _, window, cx| {
                    this.cancel_new_directory(window, cx);
                }));

            let input_handle = self.directory_input.as_ref().map(|i| i.read(cx).focus_handle(cx));
            let confirm_focus = self.dir_confirm_focus.clone();
            let cancel_focus = self.dir_cancel_focus.clone();

            h_flex()
                .gap(SPACE_SM)
                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                    let key = e.keystroke.key.as_str();
                    if key == "tab" || key == "\t" {
                        let is_shift = e.keystroke.modifiers.shift;
                        let mut handles = Vec::with_capacity(3);
                        if let Some(ref ih) = input_handle {
                            handles.push(ih.clone());
                        }
                        handles.push(confirm_focus.clone());
                        handles.push(cancel_focus.clone());
                        FocusGroup::cycle_handles(&handles, is_shift, window, cx);
                        cx.stop_propagation();
                    } else if key == "escape" {
                        this.cancel_new_directory(window, cx);
                        cx.stop_propagation();
                    } else if (key == "enter" || key == "\n")
                        && let Some(ref ih) = input_handle
                        && ih.is_focused(window)
                    {
                        this.commit_new_directory(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(
                    div()
                        .flex_1()
                        .when_some(self.directory_input.as_ref(), |this, dir_inp| {
                            this.child(velowork_ui::Input::new(dir_inp).cleanable(true))
                        }),
                )
                .child(confirm)
                .child(cancel)
                .into_any_element()
        } else {
            let new_tip = i18n!(cx, "workspace.folder.create");
            let new_btn = icon_button("folder-new", AppIcon::NewFolder, &t, cx)
                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(new_tip.clone())).into())
                .focus_action(&self.new_dir_focus, &t, cx.listener(move |this, _, window, cx| {
                    this.creating_directory = true;
                    let initial_text = if let Some(ref sel_id) = this.directory {
                        let mut folders = vec![];
                        let s = settings(cx);
                        let qc_tree = s.quick_commands_for_project(this.project_id.as_deref());
                        qc_collect_folders(qc_tree, "", &mut folders);
                        folders
                            .iter()
                            .find(|(id, _)| id == sel_id)
                            .map(|(_, path)| format!("{}/", path))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let placeholder = i18n!(cx, "workspace.folder.create_placeholder");
                    let inp = cx.new(|cx| {
                        InputState::new(cx)
                            .placeholder(placeholder)
                            .default_value(&initial_text)
                    });
                    this.directory_input = Some(inp);
                    if let Some(dir_inp) = this.directory_input.as_ref() {
                        dir_inp.update(cx, |st, cx| st.focus(window, cx));
                    }
                    cx.notify();
                }));
            h_flex()
                .gap(SPACE_SM)
                .child(div().flex_1().child(Select::new(&self.directory_select)))
                .child(new_btn)
                .into_any_element()
        }
    }

    fn commit_new_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self
            .directory_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_default();
        match parse_and_validate_folder_path(&raw) {
            Ok(segments) => {
                let pid = self.project_id.clone();
                let new_id = settings_entity(cx).update(cx, |s, cx| {
                    let tree = s.settings.quick_commands_for_project_mut(pid.as_deref());
                    let id = qc_ensure_folder_path(tree, &segments);
                    if id.is_some() {
                        s.save_and_notify(cx);
                    }
                    id
                });
                if let Some(id) = new_id {
                    self.directory = Some(id);
                }
                self.directory_input = None;
                self.creating_directory = false;
                self.refresh_selects(cx);
                let dir_sel_handle = self.directory_select.read(cx).focus_handle().clone();
                window.focus(&dir_sel_handle, cx);
                cx.notify();
            }
            Err(err) => {
                velowork_workspace::toast::ToastManager::warning(err, cx);
                if let Some(dir_inp) = self.directory_input.as_ref() {
                    dir_inp.update(cx, |st, cx| st.focus(window, cx));
                }
            }
        }
    }

    fn cancel_new_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.creating_directory = false;
        self.directory_input = None;
        window.focus(&self.new_dir_focus, cx);
        cx.notify();
    }

    /// Ids that must not be selectable as a parent directory. When editing a
    /// folder, the folder itself and all of its descendants are excluded so a
    /// folder can never be moved inside itself (which would create a cycle).
    fn excluded_folder_ids(&self) -> std::collections::HashSet<String> {
        fn collect(node: &QuickCommandNode, out: &mut std::collections::HashSet<String>) {
            out.insert(node.id().to_string());
            if let QuickCommandNode::Folder { children, .. } = node {
                for c in children {
                    collect(c, out);
                }
            }
        }
        let mut set = std::collections::HashSet::new();
        if let QuickCommandDialogMode::EditFolder { node } = &self.mode {
            collect(node, &mut set);
        }
        set
    }

    fn render_variables(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let has_vars = !self.variables.is_empty();

        v_flex()
            .gap(SPACE_MD)
            .when(has_vars, |this| {
                this.child(
                    h_flex()
                        .gap(SPACE_SM)
                        .items_center()
                        .px(SPACE_XS)
                        .text_size(ui_text_sm(cx))
                        .text_color(rgb(t.text_muted))
                        .child(
                            div()
                                .flex_1()
                                .child(i18n!(cx, "quick_commands.var_name")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .child(i18n!(cx, "quick_commands.var_default")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .child(i18n!(cx, "quick_commands.var_hint")),
                        )
                        .child(div().w(px(20.0))),
                )
            })
            .children(self.variables.iter().enumerate().map(|(i, v)| {
                let idx = i;
                let var_name = v.name.clone();
                let is_unref = v.unreferenced;

                let badge_bg = if is_unref {
                    p.status_warning.opacity(0.12)
                } else {
                    p.surface_raised
                };
                let badge_border = if is_unref {
                    p.status_warning.opacity(0.5)
                } else {
                    p.border_subtle
                };

                let locate_tip = if is_unref {
                    i18n!(cx, "quick_commands.var_unreferenced")
                } else {
                    i18n!(cx, "quick_commands.click_to_locate_var")
                };

                h_flex()
                    .gap(SPACE_SM)
                    .items_center()
                    .child(
                        div()
                            .id(ElementId::Name(format!("qc-var-badge-{}", idx).into()))
                            .flex_1()
                            .h(px(28.0))
                            .px(SPACE_SM)
                            .bg(badge_bg)
                            .border_1()
                            .border_color(badge_border)
                            .rounded(RADIUS_STD)
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .hover(|s| s.bg(p.surface_hover))
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(locate_tip.clone())).into())
                            .on_click(cx.listener({
                                let name = var_name.clone();
                                move |this, _, window, cx| {
                                    this.locate_variable_in_content(&name, window, cx);
                                }
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(ui_text_sm(cx))
                                    .font_family(mono_font_family(cx))
                                    .text_color(if is_unref { p.status_warning } else { p.editor_variable })
                                    .truncate()
                                    .child(var_name),
                            )
                            .when(is_unref, |badge| {
                                badge.child(
                                    AppIcon::Info
                                        .size(px(13.0))
                                        .text_color(p.status_warning),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(velowork_ui::Input::new(&v.default_value).cleanable(true)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(velowork_ui::Input::new(&v.hint).cleanable(true)),
                    )
                    .child({
                        let remove_focus = v.remove_focus.clone();
                        div()
                            .id(ElementId::Name(format!("qc-var-rm-{}", idx).into()))
                            .track_focus(&remove_focus)
                            .focus_ring_on(&remove_focus, &t)
                            .cursor_pointer()
                            .w(px(20.0))
                            .h(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_STD)
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "\n" | " ") {
                                    this.remove_variable_at(idx, cx);
                                }
                            }))
                            .child(AppIcon::Trash.size(px(13.0)).text_color(rgb(t.text_muted)))
                            .tooltip(move |_, cx| {
                                let __tip = i18n!(cx, "quick_commands.remove_variable");
                                cx.new(|_| Tooltip::new(__tip)).into()
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_variable_at(idx, cx);
                            }))
                    })
            }))
            .child(
                div()
                    .id("qc-add-var")
                    .border_2()
                    .border_color(gpui::transparent_black())
                    .track_focus(&self.add_var_focus)
                    .focus(|s| s.border_color(p.surface_accent_hover))
                    .cursor_pointer()
                    .px(SPACE_MD)
                    .py(SPACE_XS)
                    .rounded(RADIUS_STD)
                    .bg(p.surface_accent)
                    .hover(|s| s.bg(p.surface_accent_hover))
                    .flex()
                    .items_center()
                    .gap(SPACE_XS)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "\n" | " ") {
                            this.add_variable(window, cx);
                        }
                    }))
                    .child(
                        AppIcon::Plus
                            .size(px(12.0))
                            .text_color(p.text_on_accent),
                    )
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .text_color(p.text_on_accent)
                            .child(i18n!(cx, "quick_commands.add_variable")),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.add_variable(window, cx);
                    })),
            )
    }
}

impl Render for QuickCommandDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        // 同步「所属目录」下拉 Select 组件的选项与当前选中值。
        self.refresh_selects(cx);

        if !self.initial_focus_done {
            self.initial_focus_done = true;
            if self.previous_focus_handle.is_none() {
                self.previous_focus_handle = window.focused(cx);
            }
            if let Some(name_input) = self.name_input.as_ref() {
                name_input.update(cx, |input, cx| input.focus(window, cx));
            }
        }

        let is_command = self.is_command();
        let title = self.title(cx);
        
        let focus_group = FocusGroup::new().with_scroll(self.scroll_handle.clone());
        if let Some(h) = self.name_input.as_ref().map(|i| i.read(cx).focus_handle(cx)) {
            focus_group.add(h);
        }
        if !self.creating_directory {
            focus_group.add(self.directory_select.read(cx).focus_handle().clone());
            focus_group.add_same_row(self.new_dir_focus.clone());
        }
        if is_command
            && let Some(h) = self.content_input.as_ref().map(|i| i.read(cx).focus_handle(cx))
        {
            focus_group.add_with_height(h, 100.0);
        }
        for v in &self.variables {
            focus_group.add(v.default_value.read(cx).focus_handle(cx));
            focus_group.add_same_row(v.hint.read(cx).focus_handle(cx));
            focus_group.add_same_row(v.remove_focus.clone());
        }
        focus_group.add(self.add_var_focus.clone());
        focus_group.add_unscrolled(self.cancel_focus.clone());
        focus_group.add_unscrolled(self.confirm_focus.clone());

        let name_fh = self.name_input.as_ref().map(|i| i.read(cx).focus_handle(cx));
        let content_fh = self.content_input.as_ref().map(|i| i.read(cx).focus_handle(cx));

        let win_size = window.viewport_size();
        let card_w = px(650.0).min(win_size.width - px(48.0));
        let card_h = px(580.0).min(win_size.height - px(80.0)).max(px(240.0));

        let body_content = v_flex()
            .id(ElementId::Name("qc-dialog-scroll".into()))
            .relative()
            .flex()
            .flex_col()
            .p(px(20.0))
            .gap(SPACE_LG)
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .min_h(px(0.0))
            .w_full()
            .child(
                form_item("qc-name")
                    .label(i18n!(cx, "quick_commands.name"))
                    .focus_opt(name_fh)
                    .required(true)
                    .error_opt(self.name_error_msg.clone())
                    .child(
                        div().flex_1().when_some(self.name_input.as_ref(), |this, name| {
                            this.child(velowork_ui::Input::new(name).cleanable(true))
                        }),
                    )
                    .render(&t, cx),
            )
            .child(
                // 目录选择器对指令与目录都显示：目录也可选择所属
                // 父目录，从而支持多级子目录的创建与移动。
                form_item("qc-directory")
                    .label(i18n!(cx, "quick_commands.directory"))
                    .focus(self.directory_select.read(cx).focus_handle())
                    .child(self.render_directory_selector(cx))
                    .render(&t, cx),
            )
            .when(is_command, |d| {
                d.child(
                    form_item("qc-content")
                        .label(i18n!(cx, "quick_commands.content"))
                        .focus_opt(content_fh)
                        .required(true)
                        .error_opt(self.content_error_msg.clone())
                        .child(
                            div().flex_1().when_some(self.content_input.as_ref(), |this, content| {
                                this.child(velowork_ui::Input::new(content).fill_height().h(px(100.0)))
                            }),
                        )
                        .render(&t, cx),
                )
            })
            .when(is_command, |d| d.child(self.render_variables(cx)));

        let body_container = div()
            .relative()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_hidden()
            .child(body_content.h_full())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .left_0()
                    .child(Scrollbar::vertical(&self.scroll_handle)),
            );

        modal_content("qc-dialog", cx)
            .w(card_w)
            .h(card_h)
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .key_context("QuickCommandDialog")
            .tab_cycle(&focus_group)
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                this.close(Some(window), cx);
            }))
            .focus_scope_on_click(&self.focus_handle)
            .child(modal_header(
                &title,
                None::<&str>,
                &t,
                cx,
                cx.listener(|this, _, window, cx| this.close(Some(window), cx)),
            ))
            .child(body_container)
            .child(
                h_flex()
                    .flex_shrink_0()
                    .h(px(48.0))
                    .items_center()
                    .justify_end()
                    .px(SPACE_LG)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .child(dialog_actions(
                        &i18n!(cx, "common.action.cancel"),
                        cx.listener(|this, _, window, cx| this.close(Some(window), cx)),
                        &i18n!(cx, "common.action.save"),
                        cx.listener(|this, _, window, cx| this.save(window, cx)),
                        &self.cancel_focus,
                        &self.confirm_focus,
                        &t,
                    )),
            )
    }
}

velowork_ui::impl_focusable!(QuickCommandDialog);
