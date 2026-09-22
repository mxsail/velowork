//! 服务监控配置对话框（仿 TunnelDialog 统一设计系统与交互）。
//!
//! 提供新增/编辑单个 `ServiceDefinition` 的表单，保存时写入 `GlobalServiceStore`。

use crate::keybindings::Cancel;
use crate::views::components::{modal_content, modal_header};
use gpui::prelude::FluentBuilder;
use gpui::*;
use velowork_i18n::i18n;
use velowork_state::{ServiceCommandPolicy, ServiceDefinition, ServiceKind, collect_folder_nodes};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::dialog_actions::dialog_actions;
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::form::{FormLayout, form_item};
use velowork_ui::icon::AppIcon;
use velowork_ui::icon_button::{FocusableIconButtonExt, icon_button};
use velowork_ui::input::InputState;
use velowork_ui::overlay::OverlayRegistry;
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::select::{Select, SelectEvent, SelectOption, SelectState};
use velowork_ui::theme::{ThemeColors, theme};
use velowork_ui::tokens::{RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, ui_text_sm};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex};
use velowork_workspace::folder_path::parse_and_validate_folder_path;
use velowork_workspace::stores::{GlobalServiceStore, GlobalSessionStore};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceDialogMode {
    Create { parent_id: Option<String> },
    Edit,
}

#[derive(Clone, Debug)]
pub enum ServiceDialogEvent {
    Saved { def: ServiceDefinition },
    Close,
}

impl EventEmitter<ServiceDialogEvent> for ServiceDialog {}

pub struct ServiceDialog {
    mode: ServiceDialogMode,
    def: ServiceDefinition,
    focus_handle: FocusHandle,
    cancel_focus: FocusHandle,
    confirm_focus: FocusHandle,
    monitor_focus: FocusHandle,
    new_dir_focus: FocusHandle,
    dir_confirm_focus: FocusHandle,
    dir_cancel_focus: FocusHandle,
    name_input: Option<Entity<InputState>>,
    alive_input: Option<Entity<InputState>>,
    start_input: Option<Entity<InputState>>,
    stop_input: Option<Entity<InputState>>,
    restart_input: Option<Entity<InputState>>,
    workdir_input: Option<Entity<InputState>>,
    kind_select: Entity<SelectState<SharedString>>,
    policy_select: Entity<SelectState<SharedString>>,
    session_select: Entity<SelectState<SharedString>>,
    directory_select: Entity<SelectState<SharedString>>,
    directory_input: Option<Entity<InputState>>,
    creating_directory: bool,
    selected_directory_id: Option<String>,
    selected_kind_idx: usize,
    selected_policy_idx: usize,
    selected_session_id: String,
    monitor_enabled: bool,
    strings: ServiceDialogStrings,
    error_msg: Option<String>,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    initial_focus_done: bool,
    scroll_handle: ScrollHandle,
    previous_focus_handle: Option<FocusHandle>,
}

#[derive(Clone, Debug)]
struct ServiceDialogStrings {
    title_create: String,
    title_edit: String,
    name: String,
    kind: String,
    session: String,
    directory: String,
    no_directory: String,
    new_folder: String,
    new_folder_placeholder: String,
    session_placeholder: String,
    alive: String,
    alive_help: String,
    alive_tooltip: String,
    alive_placeholder_command: String,
    alive_placeholder_systemd: String,
    alive_placeholder_docker: String,
    start: String,
    stop: String,
    restart: String,
    workdir: String,
    workdir_placeholder: String,
    monitor: String,
    policy: String,
    policy_always: String,
    policy_dangerous: String,
    policy_direct: String,
    kind_command: String,
    kind_systemd: String,
    kind_docker: String,
    hint: String,
}

impl ServiceDialog {
    pub fn new(
        mode: ServiceDialogMode,
        initial: Option<ServiceDefinition>,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut def = initial.unwrap_or_else(ServiceDefinition::default);
        if def.id.is_empty() {
            def.id = format!("svc_{}", uuid_short());
        }
        let default_parent = match &mode {
            ServiceDialogMode::Create { parent_id } => parent_id.clone(),
            ServiceDialogMode::Edit => None,
        };
        let s = build_strings(cx);

        let selected_kind_idx = match &def.kind {
            ServiceKind::Command => 0,
            ServiceKind::Systemd { .. } => 1,
            ServiceKind::Docker { .. } => 2,
        };

        let selected_policy_idx = match def.command_policy {
            ServiceCommandPolicy::AlwaysConfirm => 0,
            ServiceCommandPolicy::ConfirmDangerous => 1,
            ServiceCommandPolicy::Direct => 2,
        };

        let selected_session_id = def.session_id.clone().unwrap_or_default();
        let monitor_enabled = def.monitor_enabled;

        let directory_select = cx.new(|cx| SelectState::new(cx));
        let kind_select = cx.new(|cx| SelectState::new(cx));
        let policy_select = cx.new(|cx| SelectState::new(cx));
        let session_select = cx.new(|cx| SelectState::new(cx));

        Self {
            mode,
            def,
            focus_handle: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            monitor_focus: cx.focus_handle(),
            new_dir_focus: cx.focus_handle(),
            dir_confirm_focus: cx.focus_handle(),
            dir_cancel_focus: cx.focus_handle(),
            name_input: None,
            alive_input: None,
            start_input: None,
            stop_input: None,
            restart_input: None,
            workdir_input: None,
            kind_select,
            policy_select,
            session_select,
            directory_select,
            directory_input: None,
            creating_directory: false,
            selected_directory_id: default_parent,
            selected_kind_idx,
            selected_policy_idx,
            selected_session_id,
            monitor_enabled,
            strings: s,
            error_msg: None,
            overlay_registry,
            initial_focus_done: false,
            scroll_handle: ScrollHandle::new(),
            previous_focus_handle: None,
        }
    }

    /// 订阅全部 Select 组件的变更事件，写回配置状态
    pub fn setup_selects(&mut self, cx: &mut Context<Self>) {
        if let Some(reg) = self
            .overlay_registry
            .clone()
            .or_else(|| OverlayRegistry::global(cx))
        {
            self.kind_select
                .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
            self.policy_select
                .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
            self.session_select
                .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
            self.directory_select
                .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        }
        let kind = self.kind_select.clone();
        cx.subscribe(
            &kind,
            move |this, _st, ev: &SelectEvent<SharedString>, cxx| {
                let SelectEvent::Change(v) = ev;
                if let Some(val) = v {
                    if let Ok(idx) = val.parse::<usize>() {
                        if idx < 3 {
                            this.selected_kind_idx = idx;
                            cxx.notify();
                        }
                    }
                }
            },
        )
        .detach();

        let policy = self.policy_select.clone();
        cx.subscribe(
            &policy,
            move |this, _st, ev: &SelectEvent<SharedString>, cxx| {
                let SelectEvent::Change(v) = ev;
                if let Some(val) = v {
                    if let Ok(idx) = val.parse::<usize>() {
                        if idx < 3 {
                            this.selected_policy_idx = idx;
                            cxx.notify();
                        }
                    }
                }
            },
        )
        .detach();

        let sess = self.session_select.clone();
        cx.subscribe(
            &sess,
            move |this, _st, ev: &SelectEvent<SharedString>, cxx| {
                let SelectEvent::Change(v) = ev;
                this.selected_session_id = v.clone().unwrap_or_default().to_string();
                cxx.notify();
            },
        )
        .detach();

        let dir = self.directory_select.clone();
        cx.subscribe(
            &dir,
            move |this, _st, ev: &SelectEvent<SharedString>, cxx| {
                let SelectEvent::Change(v) = ev;
                this.selected_directory_id =
                    v.as_ref().filter(|v| !v.is_empty()).map(|v| v.to_string());
                cxx.notify();
            },
        )
        .detach();
    }

    /// 刷新下拉选项与选中状态
    pub fn refresh_selects(&mut self, cx: &mut Context<Self>) {
        let s = &self.strings;

        let kind_opts = vec![
            SelectOption::new(SharedString::from("0"), s.kind_command.clone()),
            SelectOption::new(SharedString::from("1"), s.kind_systemd.clone()),
            SelectOption::new(SharedString::from("2"), s.kind_docker.clone()),
        ];
        self.kind_select.update(cx, |st, cx| {
            st.set_options(kind_opts, cx);
            st.set_selected_value(
                Some(SharedString::from(self.selected_kind_idx.to_string())),
                cx,
            );
        });

        let policy_opts = vec![
            SelectOption::new(SharedString::from("0"), s.policy_always.clone()),
            SelectOption::new(SharedString::from("1"), s.policy_dangerous.clone()),
            SelectOption::new(SharedString::from("2"), s.policy_direct.clone()),
        ];
        self.policy_select.update(cx, |st, cx| {
            st.set_options(policy_opts, cx);
            st.set_selected_value(
                Some(SharedString::from(self.selected_policy_idx.to_string())),
                cx,
            );
        });

        let sessions = self.collect_sessions(cx);
        let mut sess_opts = vec![SelectOption::new(
            SharedString::from(""),
            s.session_placeholder.clone(),
        )];
        for (id, name) in &sessions {
            sess_opts.push(SelectOption::new(
                SharedString::from(id.clone()),
                name.clone(),
            ));
        }
        let cur_sess = self.selected_session_id.clone();
        self.session_select.update(cx, |st, cx| {
            st.set_options(sess_opts, cx);
            st.set_selected_value(Some(SharedString::from(cur_sess)), cx);
        });

        let mut dir_opts = vec![SelectOption::new(
            SharedString::from(""),
            s.no_directory.clone(),
        )];
        let folders = self.collect_folders(cx);
        for (id, name) in &folders {
            dir_opts.push(SelectOption::new(
                SharedString::from(id.clone()),
                name.clone(),
            ));
        }
        let cur_dir = self.selected_directory_id.clone().unwrap_or_default();
        self.directory_select.update(cx, |st, cx| {
            st.set_options(dir_opts, cx);
            st.set_selected_value(Some(SharedString::from(cur_dir)), cx);
        });
    }

    fn collect_folders(&self, cx: &App) -> Vec<(String, String)> {
        if let Some(store) = cx.try_global::<GlobalServiceStore>() {
            let mut out = Vec::new();
            let nodes = store.0.read(cx).nodes();
            collect_folder_nodes(nodes, "", &mut out);
            out
        } else {
            Vec::new()
        }
    }

    fn collect_sessions(&self, cx: &App) -> Vec<(String, String)> {
        if let Some(store) = cx.try_global::<GlobalSessionStore>() {
            store.0.read(cx).all_sessions()
        } else {
            Vec::new()
        }
    }

    fn collect(&mut self, cx: &mut Context<Self>) -> ServiceDefinition {
        let mut def = self.def.clone();
        def.name = self
            .name_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.def.name.clone())
            .trim()
            .to_string();

        let session = self.selected_session_id.trim().to_string();
        def.session_id = if session.is_empty() {
            None
        } else {
            Some(session)
        };

        def.alive_command = self
            .alive_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.def.alive_command.clone())
            .trim()
            .to_string();
        def.start_command = opt(self
            .start_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.def.start_command.clone().unwrap_or_default())
            .trim()
            .to_string());
        def.stop_command = opt(self
            .stop_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.def.stop_command.clone().unwrap_or_default())
            .trim()
            .to_string());
        def.restart_command = opt(self
            .restart_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.def.restart_command.clone().unwrap_or_default())
            .trim()
            .to_string());
        def.workdir = opt(self
            .workdir_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.def.workdir.clone().unwrap_or_default())
            .trim()
            .to_string());

        def.kind = match self.selected_kind_idx {
            1 => ServiceKind::Systemd {
                unit: String::new(),
            },
            2 => ServiceKind::Docker {
                container: String::new(),
            },
            _ => ServiceKind::Command,
        };

        def.command_policy = match self.selected_policy_idx {
            1 => ServiceCommandPolicy::ConfirmDangerous,
            2 => ServiceCommandPolicy::Direct,
            _ => ServiceCommandPolicy::AlwaysConfirm,
        };

        def.monitor_enabled = self.monitor_enabled;
        def
    }

    fn on_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .name_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.def.name.clone())
            .trim()
            .to_string();
        if name.is_empty() {
            self.error_msg = Some(i18n!(cx, "service.name_required"));
            cx.notify();
            return;
        }

        let alive = self
            .alive_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.def.alive_command.clone())
            .trim()
            .to_string();
        if alive.is_empty() {
            self.error_msg = Some(i18n!(cx, "service.alive_required"));
            cx.notify();
            return;
        }

        let def = self.collect(cx);

        // 将服务归属到所选目录（目录由服务树表达，不在 def 上）。
        if let Some(store) = cx.try_global::<GlobalServiceStore>() {
            let entity = store.0.clone();
            entity.update(cx, |st, cx| {
                st.upsert(def.clone(), cx);
                st.move_node(&def.id, self.selected_directory_id.as_deref(), true, cx);
            });
        }

        cx.emit(ServiceDialogEvent::Saved { def });
        self.close(Some(window), cx);
    }

    fn close(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if let Some(window) = window {
            if let Some(prev) = self.previous_focus_handle.take() {
                window.focus(&prev, cx);
            }
        }
        cx.emit(ServiceDialogEvent::Close);
    }

    /// 渲染"所属目录"字段：下拉选择多级目录 + 新建目录（内联输入，支持多级路径如 A/B/C）。
    fn render_directory_field(&mut self, t: &ThemeColors, cx: &mut Context<Self>) -> Stateful<Div> {
        let s = self.strings.clone();

        let field_body = if self.creating_directory {
            let confirm_tip = i18n!(cx, "common.action.confirm");
            let cancel_tip = i18n!(cx, "common.action.cancel");
            let confirm = icon_button("dir-confirm", AppIcon::Check, t, cx)
                .tooltip(move |_, cx| {
                    cx.new(|_| velowork_ui::Tooltip::new(confirm_tip.clone()))
                        .into()
                })
                .focus_action(
                    &self.dir_confirm_focus,
                    t,
                    cx.listener(move |this, _, window, cx| {
                        this.commit_new_directory(window, cx);
                    }),
                );
            let cancel = icon_button("dir-cancel", AppIcon::Close, t, cx)
                .tooltip(move |_, cx| {
                    cx.new(|_| velowork_ui::Tooltip::new(cancel_tip.clone()))
                        .into()
                })
                .focus_action(
                    &self.dir_cancel_focus,
                    t,
                    cx.listener(move |this, _, window, cx| {
                        this.cancel_new_directory(window, cx);
                    }),
                );

            let input_handle = self
                .directory_input
                .as_ref()
                .map(|i| i.read(cx).focus_handle(cx));
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
        } else {
            let new_btn = icon_button("folder-new", AppIcon::NewFolder, t, cx)
                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(s.new_folder.clone())).into())
                .focus_action(
                    &self.new_dir_focus,
                    t,
                    cx.listener(move |this, _, window, cx| {
                        this.creating_directory = true;
                        let initial_text = if let Some(ref sel_id) = this.selected_directory_id {
                            let folders = this.collect_folders(cx);
                            folders
                                .iter()
                                .find(|(id, _)| id == sel_id)
                                .map(|(_, path)| format!("{}/", path))
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };
                        let placeholder = this.strings.new_folder_placeholder.clone();
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
                    }),
                );
            h_flex()
                .gap(SPACE_SM)
                .child(div().flex_1().child(Select::new(&self.directory_select)))
                .child(new_btn)
        };

        form_item("service-directory")
            .label(s.directory.clone())
            .focus(self.directory_select.read(cx).focus_handle())
            .child(field_body)
            .render(t, cx)
    }

    fn commit_new_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self
            .directory_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_default();
        match parse_and_validate_folder_path(&raw) {
            Ok(segments) => {
                if let Some(store) = cx.try_global::<GlobalServiceStore>() {
                    let entity = store.0.clone();
                    let new_id = entity.update(cx, |st, cx| {
                        st.ensure_folder_path_for_project(&segments, None, cx)
                    });
                    if let Some(id) = new_id {
                        self.selected_directory_id = Some(id);
                    }
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
}

velowork_ui::impl_focusable!(ServiceDialog);

fn opt(v: String) -> Option<String> {
    let t = v.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", n % 0xFFFFFFF)
}

fn build_strings(cx: &App) -> ServiceDialogStrings {
    ServiceDialogStrings {
        title_create: i18n!(cx, "service.add"),
        title_edit: i18n!(cx, "service.edit"),
        name: i18n!(cx, "service.name"),
        kind: i18n!(cx, "service.kind"),
        session: i18n!(cx, "service.session"),
        directory: i18n!(cx, "service.directory"),
        no_directory: i18n!(cx, "service.no_directory"),
        new_folder: i18n!(cx, "workspace.folder.create"),
        new_folder_placeholder: i18n!(cx, "workspace.folder.create_placeholder"),
        session_placeholder: i18n!(cx, "service.session_unbound"),
        alive: i18n!(cx, "service.alive_command"),
        alive_help: i18n!(cx, "service.alive_command_help"),
        alive_tooltip: i18n!(cx, "service.alive_command_tooltip"),
        alive_placeholder_command: i18n!(cx, "service.alive_placeholder_command"),
        alive_placeholder_systemd: i18n!(cx, "service.alive_placeholder_systemd"),
        alive_placeholder_docker: i18n!(cx, "service.alive_placeholder_docker"),
        start: i18n!(cx, "service.start_command"),
        stop: i18n!(cx, "service.stop_command"),
        restart: i18n!(cx, "service.restart_command"),
        workdir: i18n!(cx, "service.workdir"),
        workdir_placeholder: i18n!(cx, "service.workdir"),
        monitor: i18n!(cx, "service.monitor_enabled"),
        policy: i18n!(cx, "service.command_policy"),
        policy_always: i18n!(cx, "service.policy_always_confirm"),
        policy_dangerous: i18n!(cx, "service.policy_confirm_dangerous"),
        policy_direct: i18n!(cx, "service.policy_direct"),
        kind_command: i18n!(cx, "service.type_command"),
        kind_systemd: i18n!(cx, "service.type_systemd"),
        kind_docker: i18n!(cx, "service.type_docker"),
        hint: i18n!(cx, "service.hint"),
    }
}

impl Render for ServiceDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.strings.clone();

        if self.name_input.is_none() {
            let placeholder = s.name.clone();
            let default_val = self.def.name.clone();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(placeholder)
                    .default_value(&default_val)
            });
            self.name_input = Some(input);
        }

        let alive_placeholder = match self.selected_kind_idx {
            1 => s.alive_placeholder_systemd.clone(),
            2 => s.alive_placeholder_docker.clone(),
            _ => s.alive_placeholder_command.clone(),
        };

        if self.alive_input.is_none() {
            let default_val = self.def.alive_command.clone();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(alive_placeholder)
                    .default_value(&default_val)
            });
            self.alive_input = Some(input);
        } else if let Some(inp) = &self.alive_input {
            inp.update(cx, |inp, _cx| {
                inp.set_placeholder(alive_placeholder);
            });
        }

        if self.start_input.is_none() {
            let placeholder = s.start.clone();
            let default_val = self.def.start_command.clone().unwrap_or_default();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(placeholder)
                    .default_value(&default_val)
            });
            self.start_input = Some(input);
        }

        if self.stop_input.is_none() {
            let placeholder = s.stop.clone();
            let default_val = self.def.stop_command.clone().unwrap_or_default();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(placeholder)
                    .default_value(&default_val)
            });
            self.stop_input = Some(input);
        }

        if self.restart_input.is_none() {
            let placeholder = s.restart.clone();
            let default_val = self.def.restart_command.clone().unwrap_or_default();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(placeholder)
                    .default_value(&default_val)
            });
            self.restart_input = Some(input);
        }

        if self.workdir_input.is_none() {
            let placeholder = s.workdir_placeholder.clone();
            let default_val = self.def.workdir.clone().unwrap_or_default();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(placeholder)
                    .default_value(&default_val)
            });
            self.workdir_input = Some(input);
        }

        if self.creating_directory && self.directory_input.is_none() {
            let placeholder = s.new_folder_placeholder.clone();
            let input = cx.new(|cx| InputState::new(cx).placeholder(placeholder));
            self.directory_input = Some(input);
        }

        if !self.initial_focus_done {
            self.initial_focus_done = true;
            if self.previous_focus_handle.is_none() {
                self.previous_focus_handle = window.focused(cx);
            }
            if let Some(name_input) = self.name_input.as_ref() {
                name_input.update(cx, |input, cx| input.focus(window, cx));
            }
        }

        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let title = match self.mode {
            ServiceDialogMode::Create { .. } => s.title_create.clone(),
            ServiceDialogMode::Edit => s.title_edit.clone(),
        };

        let footer = dialog_actions(
            &i18n!(cx, "common.action.cancel"),
            cx.listener(|this, _, window, cx| this.close(Some(window), cx)),
            &i18n!(cx, "common.action.save"),
            cx.listener(|this, _, window, cx| this.on_save(window, cx)),
            &self.cancel_focus,
            &self.confirm_focus,
            &t,
        );

        let focus_group = FocusGroup::new().with_scroll(self.scroll_handle.clone());
        if let Some(h) = self
            .name_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx))
        {
            focus_group.add(h);
        }
        if !self.creating_directory {
            focus_group.add(self.directory_select.read(cx).focus_handle().clone());
            focus_group.add_same_row(self.new_dir_focus.clone());
        }
        focus_group.add(self.session_select.read(cx).focus_handle().clone());
        focus_group.add(self.kind_select.read(cx).focus_handle().clone());
        focus_group.add_same_row(self.policy_select.read(cx).focus_handle().clone());
        if let Some(h) = self
            .alive_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx))
        {
            focus_group.add_with_height(h, 75.0);
        }
        if let Some(h) = self
            .workdir_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx))
        {
            focus_group.add(h);
        }
        if let Some(h) = self
            .start_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx))
        {
            focus_group.add(h);
        }
        if let Some(h) = self
            .stop_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx))
        {
            focus_group.add(h);
        }
        if let Some(h) = self
            .restart_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx))
        {
            focus_group.add(h);
        }
        focus_group.add_unscrolled(self.monitor_focus.clone());
        focus_group.add_unscrolled(self.cancel_focus.clone());
        focus_group.add_unscrolled(self.confirm_focus.clone());

        let name_fh = self
            .name_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx));
        let alive_fh = self
            .alive_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx));
        let workdir_fh = self
            .workdir_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx));
        let start_fh = self
            .start_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx));
        let stop_fh = self
            .stop_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx));
        let restart_fh = self
            .restart_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx));

        let win_size = window.viewport_size();
        let card_w = px(560.0).min(win_size.width - px(48.0));
        let card_h = px(620.0).min(win_size.height - px(80.0)).max(px(240.0));

        let body_content =
            v_flex()
                .id(ElementId::Name("service-dialog-scroll".into()))
                .relative()
                .flex()
                .flex_col()
                .p(px(20.0))
                .gap(SPACE_LG)
                .overflow_y_scroll()
                .track_scroll(&self.scroll_handle)
                .min_h(px(0.0))
                .w_full()
                // 1. 服务名称
                .child(
                    form_item("service-name")
                        .label(s.name.clone())
                        .focus_opt(name_fh)
                        .required(true)
                        .child(
                            div()
                                .flex_1()
                                .when_some(self.name_input.as_ref(), |this, name| {
                                    this.child(velowork_ui::Input::new(name).cleanable(true))
                                }),
                        )
                        .render(&t, cx),
                )
                // 2. 所属目录（紧随名称之后）
                .child(self.render_directory_field(&t, cx))
                // 3. 关联会话
                .child(
                    form_item("service-session")
                        .label(s.session.clone())
                        .focus(self.session_select.read(cx).focus_handle())
                        .child(Select::new(&self.session_select).into_any_element())
                        .render(&t, cx),
                )
                // 4. 服务类型与确认策略
                .child(
                    h_flex()
                        .gap(SPACE_MD)
                        .child(
                            form_item("service-kind")
                                .label(s.kind.clone())
                                .focus(self.kind_select.read(cx).focus_handle())
                                .child(Select::new(&self.kind_select).into_any_element())
                                .render(&t, cx)
                                .flex_1(),
                        )
                        .child(
                            form_item("service-policy")
                                .label(s.policy.clone())
                                .focus(self.policy_select.read(cx).focus_handle())
                                .child(Select::new(&self.policy_select).into_any_element())
                                .render(&t, cx)
                                .flex_1(),
                        ),
                )
                // 5. 存活检测命令
                .child(
                    form_item("service-alive")
                        .label(s.alive.clone())
                        .focus_opt(alive_fh)
                        .required(true)
                        .tooltip(s.alive_tooltip.clone())
                        .help(s.alive_help.clone())
                        .child(div().flex_1().when_some(
                            self.alive_input.as_ref(),
                            |this, alive| {
                                this.child(velowork_ui::Input::new(alive).cleanable(true))
                            },
                        ))
                        .render(&t, cx),
                )
                // 6. 工作目录
                .child(
                    form_item("service-workdir")
                        .label(s.workdir.clone())
                        .focus_opt(workdir_fh)
                        .child(div().flex_1().when_some(
                            self.workdir_input.as_ref(),
                            |this, workdir| {
                                this.child(velowork_ui::Input::new(workdir).cleanable(true))
                            },
                        ))
                        .render(&t, cx),
                )
                // 7. 启动命令
                .child(
                    form_item("service-start")
                        .label(s.start.clone())
                        .focus_opt(start_fh)
                        .child(div().flex_1().when_some(
                            self.start_input.as_ref(),
                            |this, start| {
                                this.child(velowork_ui::Input::new(start).cleanable(true))
                            },
                        ))
                        .render(&t, cx),
                )
                // 8. 停止命令
                .child(
                    form_item("service-stop")
                        .label(s.stop.clone())
                        .focus_opt(stop_fh)
                        .child(
                            div()
                                .flex_1()
                                .when_some(self.stop_input.as_ref(), |this, stop| {
                                    this.child(velowork_ui::Input::new(stop).cleanable(true))
                                }),
                        )
                        .render(&t, cx),
                )
                // 9. 重启命令
                .child(
                    form_item("service-restart")
                        .label(s.restart.clone())
                        .focus_opt(restart_fh)
                        .child(div().flex_1().when_some(
                            self.restart_input.as_ref(),
                            |this, restart| {
                                this.child(velowork_ui::Input::new(restart).cleanable(true))
                            },
                        ))
                        .render(&t, cx),
                )
                // 10. 监控状态开关
                .child(
                    form_item("service-monitor")
                        .label(s.monitor.clone())
                        .layout(FormLayout::Horizontal)
                        .focus(&self.monitor_focus)
                        .child(
                            velowork_ui::Switch::new("service-monitor-toggle")
                                .focus(&self.monitor_focus)
                                .checked(self.monitor_enabled)
                                .on_click({
                                    let entity = cx.entity();
                                    move |checked: &bool, _, cx| {
                                        let val = *checked;
                                        entity.update(cx, |this, cx| {
                                            this.monitor_enabled = val;
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .render(&t, cx),
                )
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(p.text_muted)
                        .child(s.hint.clone()),
                )
                .when_some(self.error_msg.clone(), |el, err| {
                    el.child(
                        div()
                            .px(SPACE_MD)
                            .py(SPACE_XS)
                            .bg(rgb(t.error))
                            .text_color(rgb(t.text_primary))
                            .rounded(RADIUS_STD)
                            .text_size(ui_text_sm(cx))
                            .child(err),
                    )
                });

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

        modal_content("service-dialog", cx)
            .w(card_w)
            .h(card_h)
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .key_context("ServiceDialog")
            .tab_cycle(&focus_group)
            .on_action(
                cx.listener(|this: &mut Self, _: &Cancel, window, cx| this.close(Some(window), cx)),
            )
            .focus_scope_on_click(&self.focus_handle)
            .child(modal_header(
                &title,
                None::<&str>,
                &t,
                cx,
                cx.listener(|this: &mut Self, _, window, cx| this.close(Some(window), cx)),
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
                    .child(footer),
            )
    }
}
