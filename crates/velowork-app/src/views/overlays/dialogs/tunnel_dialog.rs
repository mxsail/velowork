//! Tunnel edit / create dialog overlay.

use crate::keybindings::Cancel;
use crate::views::components::{modal_content, modal_header};
use velowork_ui::dialog_actions::dialog_actions;
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::form::{form_item, FormLayout};
use gpui::prelude::*;
use gpui::*;
use std::net::SocketAddr;
use velowork_i18n::i18n;
use velowork_state::{ReconnectPolicy, TunnelKind, TunnelProfile};
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::theme::theme;
use velowork_ui::tokens::{ui_text_sm, SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG, RADIUS_STD};
use velowork_ui::{h_flex, v_flex};
use velowork_ui::input::{InputState, TextareaState};
use velowork_ui::icon::AppIcon;
use velowork_ui::icon_button::{icon_button, FocusableIconButtonExt};
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::select::{Select, SelectEvent, SelectOption, SelectState};
use velowork_ui::tooltip::Tooltip;
use velowork_workspace::folder_path::parse_and_validate_folder_path;
use velowork_workspace::stores::{GlobalSessionStore, GlobalTunnelStore};
use velowork_workspace::tunnels::{
    tunnel_collect_folders, tunnel_node_name_exists, tunnel_parent_id_of,
};

/// Mode the dialog is operating in.
pub enum TunnelDialogMode {
    /// Create a new tunnel under `parent_id` (or at root when `None`).
    Create { parent_id: Option<String>, project_id: Option<String> },
    /// Edit an existing tunnel (clone of the stored profile).
    Edit { profile: TunnelProfile },
}

pub struct TunnelDialog {
    mode: TunnelDialogMode,
    focus_handle: FocusHandle,
    /// Persistent focus handle for the Cancel action button (keyboard nav).
    cancel_focus: FocusHandle,
    /// Persistent focus handle for the Confirm/Save action button (keyboard nav).
    confirm_focus: FocusHandle,
    /// Persistent focus handle for the auto-start toggle (keyboard nav).
    auto_start_focus: FocusHandle,
    /// Persistent focus handle for the local-only toggle (keyboard nav).
    local_only_focus: FocusHandle,
    /// Persistent focus handle for the new directory button (keyboard nav).
    new_dir_focus: FocusHandle,
    /// Persistent focus handle for the directory inline confirm button (keyboard nav).
    dir_confirm_focus: FocusHandle,
    /// Persistent focus handle for the directory inline cancel button (keyboard nav).
    dir_cancel_focus: FocusHandle,
    prefill_name: String,
    prefill_bind_port: String,
    prefill_target_host: String,
    prefill_target_port: String,
    prefill_desc: String,
    name_input: Option<Entity<InputState>>,
    /// Selected parent folder id (所属目录); `None` = root level.
    directory: Option<String>,
    selected_session_id: String,
    /// 0 = Local (-L), 1 = Remote (-R), 2 = Dynamic (-D).
    kind_index: usize,
    bind_port_input: Option<Entity<InputState>>,
    target_host_input: Option<Entity<InputState>>,
    target_port_input: Option<Entity<InputState>>,
    auto_start: bool,
    /// When true, bind address uses 127.0.0.1 (localhost only); when false, uses 0.0.0.0.
    local_only: bool,
    description_input: Option<Entity<TextareaState>>,
    error_msg: Option<String>,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    initial_focus_done: bool,
    kind_select: Entity<SelectState<SharedString>>,
    directory_select: Entity<SelectState<SharedString>>,
    session_select: Entity<SelectState<SharedString>>,
    creating_directory: bool,
    directory_input: Option<Entity<InputState>>,
    scroll_handle: ScrollHandle,
    previous_focus_handle: Option<FocusHandle>,
}

#[derive(Clone)]
pub enum TunnelDialogEvent {
    Close,
    Saved { id: String },
}

impl EventEmitter<TunnelDialogEvent> for TunnelDialog {}

impl TunnelDialog {
    pub fn new(
        mode: TunnelDialogMode,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let kind_select = cx.new(|cx| SelectState::new(cx));
        let directory_select = cx.new(|cx| SelectState::new(cx));
        let session_select = cx.new(|cx| SelectState::new(cx));

        let (prefill_name, kind_idx, session_id, bind_port, target_host, target_port, auto_start, local_only, desc, directory) =
            match &mode {
                TunnelDialogMode::Create { parent_id, .. } => (
                    String::new(),
                    0,
                    String::new(),
                    String::new(), // 空 = 动态端口
                    "127.0.0.1".to_string(),
                    String::new(),
                    false,
                    true,
                    String::new(),
                    parent_id.clone(),
                ),
                TunnelDialogMode::Edit { profile } => {
                    let (k_idx, b_port, t_host, t_port, l_only) = match &profile.kind {
                        TunnelKind::Local { local_bind, remote_target } => {
                            let b_p = if local_bind.port() == 0 {
                                String::new()
                            } else {
                                local_bind.port().to_string()
                            };
                            let (h, p) = remote_target
                                .rsplit_once(':')
                                .map(|(h, p)| (h.to_string(), p.to_string()))
                                .unwrap_or_else(|| (remote_target.clone(), String::new()));
                            let only = !local_bind.ip().is_unspecified();
                            (0, b_p, h, p, only)
                        }
                        TunnelKind::Remote { remote_bind, local_target } => {
                            let (only, b_p) = remote_bind
                                .rsplit_once(':')
                                .map(|(h, p)| (!h.starts_with("0.0.0.0"), p.to_string()))
                                .unwrap_or((true, remote_bind.clone()));
                            let h = local_target.ip().to_string();
                            let p = local_target.port().to_string();
                            (1, b_p, h, p, only)
                        }
                        TunnelKind::Dynamic { local_bind } => {
                            let b_p = if local_bind.port() == 0 {
                                String::new()
                            } else {
                                local_bind.port().to_string()
                            };
                            let only = !local_bind.ip().is_unspecified();
                            (2, b_p, String::new(), String::new(), only)
                        }
                    };
                    let dir = tunnel_parent_id_of(
                        &tunnel_tree(cx),
                        &profile.id,
                    )
                    .flatten();
                    (
                        profile.name.clone(),
                        k_idx,
                        profile.session_id.clone(),
                        b_port,
                        t_host,
                        t_port,
                        profile.auto_start,
                        l_only,
                        profile.description.clone().unwrap_or_default(),
                        dir,
                    )
                }
            };

        Self {
            mode,
            focus_handle: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            auto_start_focus: cx.focus_handle(),
            local_only_focus: cx.focus_handle(),
            new_dir_focus: cx.focus_handle(),
            dir_confirm_focus: cx.focus_handle(),
            dir_cancel_focus: cx.focus_handle(),
            prefill_name,
            prefill_bind_port: bind_port,
            prefill_target_host: target_host,
            prefill_target_port: target_port,
            prefill_desc: desc,
            name_input: None,
            directory,
            selected_session_id: session_id,
            kind_index: kind_idx,
            bind_port_input: None,
            target_host_input: None,
            target_port_input: None,
            auto_start,
            local_only,
            description_input: None,
            error_msg: None,
            overlay_registry,
            initial_focus_done: false,
            kind_select,
            directory_select,
            session_select,
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
        cx.emit(TunnelDialogEvent::Close);
    }

    /// 订阅全部 Select 组件的变更事件，写回隧道配置字段。
    /// 仅在 Entity 创建后调用一次（由 `show_tunnel_dialog` 负责）。
    pub fn setup_selects(&mut self, cx: &mut Context<Self>) {
        if let Some(reg) = self.overlay_registry.clone().or_else(|| OverlayRegistry::global(cx)) {
            self.kind_select.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
            self.directory_select.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
            self.session_select.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        }
        let kind = self.kind_select.clone();
        cx.subscribe(&kind, move |this, _st, ev: &SelectEvent<SharedString>, cxx| {
            let SelectEvent::Change(v) = ev;
            if let Some(val) = v {
                if let Ok(idx) = val.parse::<usize>() {
                    if idx < 3 {
                        this.kind_index = idx;
                        cxx.notify();
                    }
                }
            }
        })
        .detach();

        let dir = self.directory_select.clone();
        cx.subscribe(&dir, move |this, _st, ev: &SelectEvent<SharedString>, cxx| {
            let SelectEvent::Change(v) = ev;
            let val = v.clone().unwrap_or_default().to_string();
            this.directory = if val.is_empty() { None } else { Some(val) };
            cxx.notify();
        })
        .detach();

        let sess = self.session_select.clone();
        cx.subscribe(&sess, move |this, _st, ev: &SelectEvent<SharedString>, cxx| {
            let SelectEvent::Change(v) = ev;
            this.selected_session_id = v.clone().unwrap_or_default().to_string();
            cxx.notify();
        })
        .detach();
    }

    /// 将当前配置同步到三个 Select 组件的选项与选中值（目录 / 会话列表为动态数据）。
    pub fn refresh_selects(&mut self, cx: &mut Context<Self>) {
        let kind_opts: Vec<SelectOption<SharedString>> = vec![
            SelectOption::new(SharedString::from("0"), i18n!(cx, "tunnels.type_local")),
            SelectOption::new(SharedString::from("1"), i18n!(cx, "tunnels.type_remote")),
            SelectOption::new(SharedString::from("2"), i18n!(cx, "tunnels.type_dynamic")),
        ];
        self.kind_select.update(cx, |s, cx| {
            s.set_options(kind_opts, cx);
            s.set_selected_value(Some(SharedString::from(self.kind_index.to_string())), cx);
        });

        let mut folders = vec![];
        tunnel_collect_folders(&tunnel_tree(cx), "", &mut folders);
        let mut dir_opts: Vec<SelectOption<SharedString>> = vec![SelectOption::new(
            SharedString::from(""),
            i18n!(cx, "tunnels.no_directory"),
        )];
        for (id, label) in folders {
            dir_opts.push(SelectOption::new(SharedString::from(id), label));
        }
        let cur_dir = self.directory.clone().unwrap_or_default();
        self.directory_select.update(cx, |s, cx| {
            s.set_options(dir_opts, cx);
            s.set_selected_value(Some(SharedString::from(cur_dir)), cx);
        });

        let sessions = self.collect_sessions(cx);
        let mut sess_opts: Vec<SelectOption<SharedString>> = vec![SelectOption::new(
            SharedString::from(""),
            i18n!(cx, "tunnels.no_session"),
        )];
        for (id, name) in &sessions {
            sess_opts.push(SelectOption::new(SharedString::from(id.clone()), name.clone()));
        }
        let cur_sess = self.selected_session_id.clone();
        self.session_select.update(cx, |s, cx| {
            s.set_options(sess_opts, cx);
            s.set_selected_value(Some(SharedString::from(cur_sess)), cx);
        });
    }

    /// 仅获取协议类型为 SSH 的会话列表
    fn collect_sessions(&self, cx: &App) -> Vec<(String, String)> {
        if let Some(store) = cx.try_global::<GlobalSessionStore>() {
            store.0.read(cx).all_ssh_sessions()
        } else {
            Vec::new()
        }
    }

    fn on_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .name_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.prefill_name.clone())
            .trim()
            .to_string();
        if name.is_empty() {
            self.error_msg = Some(i18n!(cx, "tunnels.name_required"));
            cx.notify();
            return;
        }

        // 同目录（含文件夹、隧道）重名校验；不同目录之间允许重名。
        let tree = tunnel_tree(cx);
        let (except_id, check_parent): (Option<String>, Option<String>) = match &self.mode {
            TunnelDialogMode::Create { .. } => (None, self.directory.clone()),
            TunnelDialogMode::Edit { profile } => {
                let p = tunnel_parent_id_of(&tree, &profile.id).flatten();
                (Some(profile.id.clone()), p)
            }
        };
        if tunnel_node_name_exists(&tree, check_parent.as_deref(), &name, except_id.as_deref()) {
            self.error_msg = Some(i18n!(cx, "sftp.dialog.name_exists").replace("{name}", &name));
            cx.notify();
            return;
        }

        let session_id = self.selected_session_id.clone();
        if session_id.is_empty() {
            self.error_msg = Some(i18n!(cx, "tunnels.session_required"));
            cx.notify();
            return;
        }

        let bind_port_str = self
            .bind_port_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.prefill_bind_port.clone())
            .trim()
            .to_string();

        let target_host_str = self
            .target_host_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.prefill_target_host.clone())
            .trim()
            .to_string();

        let target_port_str = self
            .target_port_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.prefill_target_port.clone())
            .trim()
            .to_string();

        let bind_ip = if self.local_only { "127.0.0.1" } else { "0.0.0.0" };

        let kind = match self.kind_index {
            0 => {
                // 本地端口转发 (-L): local_bind -> remote_target
                let bind_port: u16 = if bind_port_str.is_empty() {
                    0
                } else {
                    match bind_port_str.parse::<u16>() {
                        Ok(p) => p,
                        Err(_) => {
                            self.error_msg = Some(i18n!(cx, "tunnels.invalid_port"));
                            cx.notify();
                            return;
                        }
                    }
                };

                let target_host = if target_host_str.is_empty() {
                    "127.0.0.1"
                } else {
                    &target_host_str
                };

                if target_port_str.is_empty() {
                    self.error_msg = Some(i18n!(cx, "tunnels.target_port_required"));
                    cx.notify();
                    return;
                }
                let target_port: u16 = match target_port_str.parse::<u16>() {
                    Ok(p) if p > 0 => p,
                    _ => {
                        self.error_msg = Some(i18n!(cx, "tunnels.invalid_port"));
                        cx.notify();
                        return;
                    }
                };

                let local_bind: SocketAddr = format!("{}:{}", bind_ip, bind_port).parse().unwrap();
                let remote_target = format!("{}:{}", target_host, target_port);

                TunnelKind::Local {
                    local_bind,
                    remote_target,
                }
            }
            1 => {
                // 远程端口转发 (-R): remote_bind -> local_target
                if bind_port_str.is_empty() {
                    self.error_msg = Some(i18n!(cx, "tunnels.remote_bind_required"));
                    cx.notify();
                    return;
                }
                let remote_port: u16 = match bind_port_str.parse::<u16>() {
                    Ok(p) if p > 0 => p,
                    _ => {
                        self.error_msg = Some(i18n!(cx, "tunnels.invalid_port"));
                        cx.notify();
                        return;
                    }
                };

                let local_host = if target_host_str.is_empty() {
                    "127.0.0.1"
                } else {
                    &target_host_str
                };

                if target_port_str.is_empty() {
                    self.error_msg = Some(i18n!(cx, "tunnels.target_port_required"));
                    cx.notify();
                    return;
                }
                let local_port: u16 = match target_port_str.parse::<u16>() {
                    Ok(p) if p > 0 => p,
                    _ => {
                        self.error_msg = Some(i18n!(cx, "tunnels.invalid_port"));
                        cx.notify();
                        return;
                    }
                };

                let local_ip: std::net::IpAddr = if local_host.is_empty() || local_host.eq_ignore_ascii_case("localhost") {
                    "127.0.0.1".parse().unwrap()
                } else {
                    match local_host.parse() {
                        Ok(ip) => ip,
                        Err(_) => {
                            self.error_msg = Some(i18n!(cx, "tunnels.invalid_local_target"));
                            cx.notify();
                            return;
                        }
                    }
                };

                let remote_bind = format!("{}:{}", bind_ip, remote_port);
                let local_target = SocketAddr::new(local_ip, local_port);

                TunnelKind::Remote {
                    remote_bind,
                    local_target,
                }
            }
            _ => {
                // 动态 SOCKS5 代理 (-D): local_bind
                let bind_port: u16 = if bind_port_str.is_empty() {
                    0
                } else {
                    match bind_port_str.parse::<u16>() {
                        Ok(p) => p,
                        Err(_) => {
                            self.error_msg = Some(i18n!(cx, "tunnels.invalid_port"));
                            cx.notify();
                            return;
                        }
                    }
                };

                let local_bind: SocketAddr = format!("{}:{}", bind_ip, bind_port).parse().unwrap();

                TunnelKind::Dynamic {
                    local_bind,
                }
            }
        };

        let id = match &self.mode {
            TunnelDialogMode::Edit { profile } => profile.id.clone(),
            TunnelDialogMode::Create { .. } => uuid::Uuid::new_v4().to_string(),
        };
        let desc = self
            .description_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_else(|| self.prefill_desc.clone())
            .trim()
            .to_string();

        let target_project_id = match &self.mode {
            TunnelDialogMode::Edit { profile } => profile.project_id.clone(),
            TunnelDialogMode::Create { project_id, .. } => {
                let from_parent = self.directory.as_deref().and_then(|dir_id| {
                    if let Some(store) = cx.try_global::<GlobalTunnelStore>() {
                        let nodes = store.0.read(cx).nodes();
                        if let Some(node) = velowork_workspace::tunnels::tunnel_find_node_ref(nodes, dir_id) {
                            if let velowork_state::TunnelNode::Folder { project_id, .. } = node {
                                return project_id.clone();
                            }
                        }
                    }
                    None
                });
                from_parent.or_else(|| project_id.clone())
            }
        };

        let profile = TunnelProfile {
            id: id.clone(),
            name,
            project_id: target_project_id,
            session_id,
            enabled: true,
            auto_start: self.auto_start,
            reconnect: ReconnectPolicy::InheritSession,
            kind,
            description: if desc.is_empty() { None } else { Some(desc) },
        };

        let parent = self.directory.clone();
        let editing = matches!(self.mode, TunnelDialogMode::Edit { .. });
        if let Some(store) = cx.try_global::<GlobalTunnelStore>() {
            let entity = store.0.clone();
            entity.update(cx, |s, cx| {
                if editing {
                    s.update_tunnel(profile, parent.as_deref(), cx);
                } else {
                    s.add_tunnel_to(profile, parent.as_deref(), cx);
                }
            });
        }

        cx.emit(TunnelDialogEvent::Saved { id });
        self.close(Some(window), cx);
    }
}

/// Read the current tunnel tree (used to resolve a tunnel's parent folder).
fn tunnel_tree(cx: &App) -> Vec<velowork_state::TunnelNode> {
    if let Some(store) = cx.try_global::<GlobalTunnelStore>() {
        store.0.read(cx).nodes().to_vec()
    } else {
        Vec::new()
    }
}

impl Render for TunnelDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        // 同步下拉 Select 组件的选项与当前选中值（目录/会话列表为动态数据）。
        self.refresh_selects(cx);

        let bind_ph = if self.kind_index == 1 {
            "8080".to_string()
        } else {
            i18n!(cx, "tunnels.bind_port_placeholder")
        };
        let target_port_ph = i18n!(cx, "tunnels.target_port_placeholder");

        if let Some(bind_input) = self.bind_port_input.as_ref() {
            bind_input.update(cx, |input, _| input.set_placeholder(&bind_ph));
        }

        if self.name_input.is_none() {
            let placeholder = i18n!(cx, "tunnels.name_placeholder");
            let default_val = self.prefill_name.clone();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(placeholder)
                    .default_value(&default_val)
            });
            self.name_input = Some(input);
        }

        if self.bind_port_input.is_none() {
            let default_val = self.prefill_bind_port.clone();
            let ph = bind_ph.clone();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(ph)
                    .default_value(&default_val)
                    .digits_only(true)
                    .max_length(5)
                    .max_number(65535)
            });
            self.bind_port_input = Some(input);
        }

        if self.target_host_input.is_none() {
            let placeholder = "127.0.0.1";
            let default_val = self.prefill_target_host.clone();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(placeholder)
                    .default_value(&default_val)
            });
            self.target_host_input = Some(input);
        }

        if self.target_port_input.is_none() {
            let default_val = self.prefill_target_port.clone();
            let ph = target_port_ph.clone();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(ph)
                    .default_value(&default_val)
                    .digits_only(true)
                    .max_length(5)
                    .max_number(65535)
            });
            self.target_port_input = Some(input);
        }

        if self.description_input.is_none() {
            let placeholder = i18n!(cx, "tunnels.description");
            let default_val = self.prefill_desc.clone();
            let input = cx.new(|cx| {
                TextareaState::new(cx)
                    .multiline()
                    .placeholder(placeholder)
                    .default_value(&default_val)
            });
            self.description_input = Some(input);
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

        let is_editing = matches!(self.mode, TunnelDialogMode::Edit { .. });
        let title_text = if is_editing {
            i18n!(cx, "tunnels.edit")
        } else {
            i18n!(cx, "tunnels.add")
        };

        let focus_group = FocusGroup::new().with_scroll(self.scroll_handle.clone());
        if let Some(h) = self.name_input.as_ref().map(|i| i.read(cx).focus_handle(cx)) {
            focus_group.add(h);
        }
        if !self.creating_directory {
            focus_group.add(self.directory_select.read(cx).focus_handle().clone());
            focus_group.add_same_row(self.new_dir_focus.clone());
        }
        focus_group.add(self.session_select.read(cx).focus_handle().clone());
        focus_group.add(self.kind_select.read(cx).focus_handle().clone());
        if let Some(h) = self.bind_port_input.as_ref().map(|i| i.read(cx).focus_handle(cx)) {
            focus_group.add(h);
        }
        if self.kind_index != 2 {
            if let Some(h) = self.target_host_input.as_ref().map(|i| i.read(cx).focus_handle(cx)) {
                focus_group.add(h);
            }
            if let Some(h) = self.target_port_input.as_ref().map(|i| i.read(cx).focus_handle(cx)) {
                focus_group.add_same_row(h);
            }
        }
        focus_group.add(self.auto_start_focus.clone());
        focus_group.add_same_row(self.local_only_focus.clone());
        if let Some(h) = self.description_input.as_ref().map(|i| i.read(cx).focus_handle(cx)) {
            focus_group.add(h);
        }
        focus_group.add_unscrolled(self.cancel_focus.clone());
        focus_group.add_unscrolled(self.confirm_focus.clone());

        let win_size = window.viewport_size();
        let card_w = px(560.0).min(win_size.width - px(48.0));
        let card_h = px(600.0).min(win_size.height - px(80.0)).max(px(240.0));

        let body_content = v_flex()
            .id(ElementId::Name("tunnel-dialog-scroll".into()))
            .relative()
            .flex()
            .flex_col()
            .p(px(20.0))
            .gap(SPACE_LG)
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .min_h(px(0.0))
            .w_full()
                            // 1. 隧道名称
                            .child(
                                form_item("tunnel-name")
                                    .label(i18n!(cx, "tunnels.name"))
                                    .required(true)
                                    .focus_opt(self.name_input.as_ref().map(|i| i.read(cx).focus_handle(cx)))
                                    .child(
                                        div().flex_1().when_some(self.name_input.as_ref(), |this, name| {
                                             this.child(velowork_ui::Input::new(name).cleanable(true))
                                        }),
                                    )
                                    .render(&t, cx),
                            )
                            // 2. 所属目录（紧随名称之后）
                            .child(
                                form_item("tunnel-directory")
                                    .label(i18n!(cx, "tunnels.directory"))
                                    .focus(self.directory_select.read(cx).focus_handle())
                                    .child(self.render_directory_selector(cx))
                                    .render(&t, cx),
                            )
                            // 3. 关联会话（仅列出 SSH 会话）
                            .child(
                                form_item("tunnel-session")
                                    .label(i18n!(cx, "tunnels.session"))
                                    .required(true)
                                    .focus(self.session_select.read(cx).focus_handle())
                                    .child(self.render_session_selector(cx))
                                    .render(&t, cx),
                            )
                            // 4. 转发类型
                            .child(
                                form_item("tunnel-kind")
                                    .label(i18n!(cx, "tunnels.kind"))
                                    .focus(self.kind_select.read(cx).focus_handle())
                                    .child(self.render_kind_selector(cx))
                                    .render(&t, cx),
                            )
                            // 5. 端点配置（监听端口 + 目标主机/目标端口并排）
                            .child(
                                form_item("tunnel-bind-port")
                                    .label(if self.kind_index == 1 {
                                        i18n!(cx, "tunnels.remote_bind_port")
                                    } else {
                                        i18n!(cx, "tunnels.bind_port")
                                    })
                                    .required(self.kind_index == 1)
                                    .focus_opt(self.bind_port_input.as_ref().map(|i| i.read(cx).focus_handle(cx)))
                                    .child(
                                        div().flex_1().when_some(self.bind_port_input.as_ref(), |this, bind| {
                                            this.child(velowork_ui::Input::new(bind).cleanable(true))
                                        }),
                                    )
                                    .render(&t, cx),
                            )
                            .when(self.kind_index != 2, |d| {
                                d.child(
                                    form_item("tunnel-target-endpoints")
                                        .label(if self.kind_index == 1 {
                                            i18n!(cx, "tunnels.local_target")
                                        } else {
                                            i18n!(cx, "tunnels.remote_target")
                                        })
                                        .required(true)
                                        .focus_opt(self.target_host_input.as_ref().map(|i| i.read(cx).focus_handle(cx)))
                                        .child(
                                            h_flex()
                                                .flex_1()
                                                .gap(SPACE_SM)
                                                .child(
                                                    div().flex_1().when_some(self.target_host_input.as_ref(), |this, host| {
                                                        this.child(velowork_ui::Input::new(host).cleanable(true))
                                                    }),
                                                )
                                                .child(
                                                    div().w(px(140.0)).when_some(self.target_port_input.as_ref(), |this, port| {
                                                        this.child(velowork_ui::Input::new(port).cleanable(true))
                                                    }),
                                                ),
                                        )
                                        .render(&t, cx),
                                )
                            })
                            // 6. 策略开关组
                            .child(
                                form_item("tunnel-autostart-item")
                                    .label(i18n!(cx, "tunnels.auto_start"))
                                    .layout(FormLayout::Horizontal)
                                    .focus(&self.auto_start_focus)
                                    .child(
                                        velowork_ui::Switch::new("tunnel-autostart")
                                            .focus(&self.auto_start_focus)
                                            .checked(self.auto_start)
                                            .on_click({
                                                let entity = cx.entity();
                                                move |checked: &bool, _, cx| {
                                                    let val = *checked;
                                                    entity.update(cx, |this, cx| {
                                                        this.auto_start = val;
                                                        cx.notify();
                                                    });
                                                }
                                            }),
                                    )
                                    .render(&t, cx),
                            )
                            .child(
                                form_item("tunnel-localonly-item")
                                    .label(i18n!(cx, "tunnels.local_only"))
                                    .layout(FormLayout::Horizontal)
                                    .focus(&self.local_only_focus)
                                    .child(
                                        velowork_ui::Switch::new("tunnel-localonly")
                                            .focus(&self.local_only_focus)
                                            .checked(self.local_only)
                                            .on_click({
                                                let entity = cx.entity();
                                                move |checked: &bool, _, cx| {
                                                    let val = *checked;
                                                    entity.update(cx, |this, cx| {
                                                        this.local_only = val;
                                                        cx.notify();
                                                    });
                                                }
                                            }),
                                    )
                                    .render(&t, cx),
                            )
                            // 7. 备注说明
                            .child(
                                form_item("tunnel-description")
                                    .label(i18n!(cx, "tunnels.description"))
                                    .focus_opt(self.description_input.as_ref().map(|i| i.read(cx).focus_handle(cx)))
                                    .child(
                                        div().flex_1().when_some(self.description_input.as_ref(), |this, desc| {
                                            this.child(velowork_ui::Input::new(desc).fill_height().h(px(80.0)))
                                        }),
                                    )
                                    .render(&t, cx),
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
                                        .child(err)
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

        modal_content("tunnel-dialog", cx)
            .w(card_w)
            .h(card_h)
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .key_context("TunnelDialog")
            .tab_cycle(&focus_group)
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                this.close(Some(window), cx);
            }))
            .focus_scope_on_click(&self.focus_handle)
            .child(modal_header(
                &title_text,
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
                        &i18n!(cx, "common.cancel"),
                        cx.listener(|this, _, window, cx| this.close(Some(window), cx)),
                        &i18n!(cx, "common.save"),
                        cx.listener(|this, _, window, cx| this.on_save(window, cx)),
                        &self.cancel_focus,
                        &self.confirm_focus,
                        &t,
                    )),
            )
    }
}

impl TunnelDialog {
    fn render_kind_selector(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        Select::new(&self.kind_select).into_any_element()
    }

    fn render_directory_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        if self.creating_directory {
            let confirm_tip = i18n!(cx, "common.confirm");
            let cancel_tip = i18n!(cx, "common.cancel");
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
            let new_tip = i18n!(cx, "common.new_folder");
            let new_btn = icon_button("folder-new", AppIcon::NewFolder, &t, cx)
                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(new_tip.clone())).into())
                .focus_action(&self.new_dir_focus, &t, cx.listener(move |this, _, window, cx| {
                    this.creating_directory = true;
                    let initial_text = if let Some(ref sel_id) = this.directory {
                        let mut folders = vec![];
                        tunnel_collect_folders(&tunnel_tree(cx), "", &mut folders);
                        folders
                            .iter()
                            .find(|(id, _)| id == sel_id)
                            .map(|(_, path)| format!("{}/", path))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let placeholder = i18n!(cx, "common.new_folder_placeholder");
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
                if let Some(store) = cx.try_global::<GlobalTunnelStore>() {
                    let entity = store.0.clone();
                    let new_id = entity.update(cx, |st, cx| {
                        st.ensure_folder_path_for_project(&segments, None, cx)
                    });
                    if let Some(id) = new_id {
                        self.directory = Some(id);
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

    fn render_session_selector(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        Select::new(&self.session_select).into_any_element()
    }
}

velowork_ui::impl_focusable!(TunnelDialog);

