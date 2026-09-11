use crate::action_dispatch::ActionDispatcher;
use crate::settings::settings_entity;
use crate::terminal::backend::TerminalBackend;
use crate::theme::{ThemeColors, surface_bg_t, theme};
use crate::ui::tokens::{
    ICON_MICRO, ICON_SM, RADIUS_CARD, RADIUS_MD, RADIUS_STD,
    ui_space_lg, ui_space_sm, ui_space_xs,
    ui_text_ms, ui_text_sm, ui_text_xl,
};
use crate::views::layout::layout_container::LayoutContainer;
use crate::views::layout::split_pane::ActiveDrag;
use crate::workspace::request_broker::RequestBroker;
use crate::workspace::state::{ProjectData, WindowId, Workspace};
use gpui::prelude::*;
use gpui::*;
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_ui::icon::AppIcon;
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::simple_input::{InputChangedEvent, SimpleInputState};
use velowork_ui::scrollable::{Scrollbar, ScrollbarShow};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::motion::ease_tab_expand;
use velowork_ui::{SemanticPalette, v_flex};

use velowork_views_terminal::welcome;

use crate::views::window::TerminalsRegistry;
use velowork_core::api::ActionRequest;

/// A single project column with header and layout
pub struct ProjectColumn {
    /// Identifies which window-scoped slot on the shared `Workspace` this
    /// project column addresses. Always `WindowId::Main` today (single-window
    /// runtime); slice 05 spawns extras that mint distinct
    /// `WindowId::Extra(uuid)`s. Read in-impl via `self.window_id` -- the
    /// hide-project button's `on_click` listener in `render_header`
    /// captures it as a `window_id_for_hide` local hoisted alongside
    /// `workspace_for_hide` and `project_id_for_hide`, which the move
    /// closure then captures by Copy for the
    /// `toggle_project_overview_visibility` call.
    pub(crate) window_id: WindowId,
    workspace: Entity<Workspace>,
    focus_manager: Entity<crate::workspace::focus::FocusManager>,
    request_broker: Entity<RequestBroker>,
    project_id: String,
    #[allow(dead_code)]
    backend: Arc<dyn TerminalBackend>,
    #[allow(dead_code)]
    terminals: TerminalsRegistry,
    /// Overlay registry for centralized click-outside dismissal across all
    /// dock overlays and popovers in the bottom dock and detached windows.
    overlay_registry: Entity<OverlayRegistry>,
    /// Stored layout container entity (must be created in new(), not render())
    pub(crate) layout_container: Option<Entity<LayoutContainer<ActionDispatcher>>>,
    /// Shared drag state for resize operations
    active_drag: ActiveDrag,
    /// Action dispatcher for routing terminal actions (local or remote)
    action_dispatcher: Option<ActionDispatcher>,
    /// Quick connect / session search input state for the welcome dashboard
    quick_connect_input: Entity<SimpleInputState>,
    /// Keyboard navigation selection index for welcome dashboard
    welcome_selected_index: Option<usize>,
    /// Horizontal scroll handle for floating hidden taskbar
    taskbar_scroll_handle: ScrollHandle,
    /// Bounding box of this project column tracked during render
    pub(crate) column_bounds: Option<Bounds<Pixels>>,
}

impl ProjectColumn {
    // GPUI view constructor: each param is a distinct injected dependency.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        window_id: WindowId,
        workspace: Entity<Workspace>,
        focus_manager: Entity<crate::workspace::focus::FocusManager>,
        request_broker: Entity<RequestBroker>,
        project_id: String,
        backend: Arc<dyn TerminalBackend>,
        terminals: TerminalsRegistry,
        active_drag: ActiveDrag,
        overlay_registry: Entity<OverlayRegistry>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Observe workspace, focus_manager, and settings_entity so ProjectColumn re-renders on state changes
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        cx.observe(&focus_manager, |_, _, cx| cx.notify()).detach();
        cx.observe(&settings_entity(cx), |_, _, cx| cx.notify())
            .detach();

        let session_store_entity = cx
            .try_global::<velowork_workspace::stores::GlobalSessionStore>()
            .map(|s| s.0.clone());
        if let Some(store) = session_store_entity {
            cx.observe(&store, |_, _, cx| cx.notify()).detach();
        }
        let conn_store_entity = cx
            .try_global::<velowork_workspace::stores::GlobalConnectionStore>()
            .map(|c| c.0.clone());
        if let Some(store) = conn_store_entity {
            cx.observe(&store, |_, _, cx| cx.notify()).detach();
        }

        let quick_connect_input = cx.new(|cx| {
            SimpleInputState::new(cx).placeholder(i18n!(cx, "welcome.quick_connect_placeholder"))
        });
        cx.subscribe(&quick_connect_input, |this: &mut Self, _, _: &InputChangedEvent, cx| {
            this.welcome_selected_index = None;
            cx.notify();
        })
        .detach();
        let fm = focus_manager.clone();
        cx.subscribe(
            &quick_connect_input,
            move |_this, _input, _: &velowork_ui::simple_input::InputFocusedEvent, cx| {
                fm.update(cx, |fm, _| {
                    fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
                });
            },
        )
        .detach();

        Self {
            window_id,
            workspace,
            focus_manager,
            request_broker,
            project_id,
            backend,
            terminals,
            overlay_registry,
            layout_container: None,
            active_drag,
            action_dispatcher: None,
            quick_connect_input,
            welcome_selected_index: None,
            taskbar_scroll_handle: ScrollHandle::new(),
            column_bounds: None,
        }
    }

    /// Identifies which window-scoped slot on the shared `Workspace` this
    /// project column addresses. Always `WindowId::Main` today (single-window
    /// runtime); slice 05 spawns extras that mint distinct
    /// `WindowId::Extra(uuid)`s. The field is read directly within `render_header`
    /// via the `window_id_for_hide` hoist captured by the hide-project button's
    /// `on_click` move closure. This public getter exists for external callers
    /// (e.g. the slice 05 spawn flow on `Velowork`) that need to address
    /// window-scoped state on `Workspace` in the same window this project
    /// column inhabits. Marked `#[allow(dead_code)]` because rustc tracks
    /// fields and methods separately -- the field being used at runtime does
    /// NOT mark the getter as used.
    #[allow(dead_code)]
    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// Return the last tracked bounding box for this project column.
    /// Used by `spawn_terminals_for_project` to pre-warm the PTY size when
    /// the pane_map is still empty (e.g., first open from welcome state).
    pub fn column_bounds(&self) -> Option<Bounds<Pixels>> {
        self.column_bounds
    }

    /// Set the action dispatcher (used for remote projects).
    pub fn set_action_dispatcher(&mut self, dispatcher: Option<ActionDispatcher>) {
        self.action_dispatcher = dispatcher;
    }

    pub(crate) fn ensure_layout_container(&mut self, project_path: String, cx: &mut Context<Self>) {
        if self.layout_container.is_none() {
            let workspace = self.workspace.clone();
            let focus_manager = self.focus_manager.clone();
            let request_broker = self.request_broker.clone();
            let project_id = self.project_id.clone();
            let backend = self.backend.clone();
            let terminals = self.terminals.clone();
            let active_drag = self.active_drag.clone();
            let action_dispatcher = self.action_dispatcher.clone();
            let window_id = self.window_id;
            let overlay_registry = self.overlay_registry.clone();

            self.layout_container = Some(cx.new(move |_cx| {
                let mut lc = LayoutContainer::new(
                    workspace,
                    focus_manager,
                    request_broker,
                    window_id,
                    project_id,
                    project_path,
                    vec![],
                    backend,
                    terminals,
                    active_drag,
                    action_dispatcher,
                );
                lc.set_overlay_registry(overlay_registry);
                lc
            }));
        } else if let Some(container) = &self.layout_container {
            // Update project_path if it changed
            container.update(cx, |c, _| {
                c.set_project_path(project_path);
            });
        }
    }

    fn get_project<'a>(&self, workspace: &'a Workspace) -> Option<&'a ProjectData> {
        workspace.project(&self.project_id)
    }

    fn render_hidden_taskbar(
        &self,
        project: &ProjectData,
        t: ThemeColors,
        cx: &App,
    ) -> impl IntoElement {
        let minimized_terminals = project
            .layout
            .as_ref()
            .map(|l| l.collect_minimized_terminals())
            .unwrap_or_default();
        let detached_terminals = project
            .layout
            .as_ref()
            .map(|l| l.collect_detached_terminals())
            .unwrap_or_default();

        if minimized_terminals.is_empty() && detached_terminals.is_empty() {
            return div().into_any_element();
        }

        let p = SemanticPalette::from_theme(&t);
        let session_store = cx.global::<velowork_workspace::stores::GlobalSessionStore>().0.read(cx);
        let suffixes = velowork_views_terminal::layout::session_labels::duplicate_session_suffixes(self.workspace.read(cx));

        let restore_tip = i18n!(cx, "project.restore_terminal_tooltip");
        let attach_tip = i18n!(cx, "project.attach_terminal_tooltip");

        let is_terminal_connection_lost = |tid: &str, cx: &App| -> bool {
            if let Some(s) = self.backend.get_ssh_session(tid) {
                if let Some(sid) = self.backend.get_ssh_session_id(tid) {
                    let store_connected = cx
                        .global::<velowork_workspace::stores::GlobalConnectionStore>()
                        .0
                        .read(cx)
                        .is_connected(&sid);
                    !store_connected || s.is_closed()
                } else {
                    s.is_closed()
                }
            } else if let Some(sid) = self.backend.get_ssh_session_id(tid) {
                let store_connected = cx
                    .global::<velowork_workspace::stores::GlobalConnectionStore>()
                    .0
                    .read(cx)
                    .is_connected(&sid);
                !store_connected
            } else {
                let lock = self.terminals.lock();
                if let Some(t) = lock.get(tid) {
                    t.shell_pid().is_none()
                } else {
                    !tid.starts_with("welcome:")
                }
            }
        };

        let minimized_elements = minimized_terminals
            .into_iter()
            .map(|(terminal_id, layout_path, shell_type)| {
                let workspace = self.workspace.clone();
                let focus_manager = self.focus_manager.clone();
                let project_id = self.project_id.clone();
                let restore_tip = restore_tip.clone();

                let is_remote = shell_type.is_remote() || project.is_remote || self.backend.is_remote();
                let connection_lost = is_terminal_connection_lost(&terminal_id, cx);
                let icon_color = if connection_lost { p.status_error } else { p.status_success };
                let icon = if is_remote { AppIcon::Server } else { AppIcon::Terminal };

                let terminal_name = {
                    let osc_title = self
                        .terminals
                        .lock()
                        .get(&terminal_id)
                        .and_then(|t| t.title());
                    let base_name = if matches!(shell_type, velowork_core::shell::ShellType::Welcome) {
                        i18n!(cx, "welcome.title")
                    } else {
                        velowork_views_terminal::layout::session_labels::terminal_base_name(
                            &terminal_id,
                            &shell_type,
                            is_remote,
                            osc_title.as_deref(),
                            project,
                            &session_store,
                        )
                    };
                    if let Some(&suffix) = suffixes.get(&terminal_id) {
                        if suffix > 1 {
                            format!("{}:{}", base_name, suffix)
                        } else {
                            base_name
                        }
                    } else {
                        base_name
                    }
                };

                let tip_text = format!("{}: {}", restore_tip, terminal_name);

                let enable_tab_preview = velowork_app_core::settings::settings(cx).enable_tab_preview;
                let preview_snapshot = self.terminals.lock().get(&terminal_id).map(|t| t.preview_snapshot(16));
                let is_welcome = matches!(shell_type, velowork_core::shell::ShellType::Welcome);
                let terminal_font = velowork_views_terminal::terminal_view_settings(cx).font_family.clone();

                let (protocol_badge, connection_info, icon_color) = if is_welcome {
                    (
                        Some(i18n!(cx, "welcome.title")),
                        Some(i18n!(cx, "welcome.subtitle")),
                        p.surface_accent,
                    )
                } else if let velowork_core::shell::ShellType::Custom { path, args } = &shell_type {
                    if path == "ssh" {
                        let store = session_store;
                        let mut session_id = None;
                        let mut host_arg = None;
                        let mut i = 0;
                        while i < args.len() {
                            if (args[i] == "--id" || args[i] == "--session-id") && i + 1 < args.len() {
                                session_id = Some(args[i + 1].clone());
                                i += 2;
                            } else if (args[i] == "-p" || args[i] == "-i") && i + 1 < args.len() {
                                i += 2;
                            } else if !args[i].starts_with('-') {
                                host_arg = Some(args[i].clone());
                                i += 1;
                            } else {
                                i += 1;
                            }
                        }
                        let conn_info = if let Some(sid) = session_id {
                            if let Some(session) = store.find_session(&sid) {
                                let user_prefix = if !session.username.is_empty() {
                                    format!("{}@", session.username)
                                } else {
                                    String::new()
                                };
                                let port_suffix = if session.port != 22 && session.port != 0 {
                                    format!(":{}", session.port)
                                } else {
                                    String::new()
                                };
                                Some(format!("{}{}{}", user_prefix, session.host, port_suffix))
                            } else {
                                host_arg
                            }
                        } else {
                            host_arg
                        };
                        (Some("SSH".to_string()), conn_info, icon_color)
                    } else if path == "serial" {
                        let store = session_store;
                        let mut session_id = None;
                        let mut port_arg = None;
                        let mut baud_arg = None;
                        let mut i = 0;
                        while i < args.len() {
                            if args[i] == "--id" && i + 1 < args.len() {
                                session_id = Some(args[i + 1].clone());
                                i += 2;
                            } else if args[i] == "--port" && i + 1 < args.len() {
                                port_arg = Some(args[i + 1].clone());
                                i += 2;
                            } else if args[i] == "--baud" && i + 1 < args.len() {
                                baud_arg = Some(args[i + 1].clone());
                                i += 2;
                            } else {
                                i += 1;
                            }
                        }
                        let conn_info = if let Some(sid) = session_id {
                            if let Some(session) = store.find_session(&sid) {
                                let port_str = session.serial_port.as_deref().unwrap_or(port_arg.as_deref().unwrap_or(""));
                                Some(format!("{} · {} baud", port_str, session.serial_baud_rate))
                            } else {
                                port_arg.map(|p| format!("{} · {} baud", p, baud_arg.unwrap_or_else(|| "115200".to_string())))
                            }
                        } else {
                            port_arg.map(|p| format!("{} · {} baud", p, baud_arg.unwrap_or_else(|| "115200".to_string())))
                        };
                        (Some("Serial".to_string()), conn_info, if connection_lost { icon_color } else { p.status_warning })
                    } else if path == "telnet" {
                        let store = session_store;
                        let mut session_id = None;
                        let mut host_arg = None;
                        let mut port_arg = None;
                        let mut i = 0;
                        while i < args.len() {
                            if args[i] == "--id" && i + 1 < args.len() {
                                session_id = Some(args[i + 1].clone());
                                i += 2;
                            } else if args[i] == "--host" && i + 1 < args.len() {
                                host_arg = Some(args[i + 1].clone());
                                i += 2;
                            } else if args[i] == "--port" && i + 1 < args.len() {
                                port_arg = Some(args[i + 1].clone());
                                i += 2;
                            } else {
                                i += 1;
                            }
                        }
                        let conn_info = if let Some(sid) = session_id {
                            if let Some(session) = store.find_session(&sid) {
                                let host = session.telnet_host.as_deref().unwrap_or(session.host.as_str());
                                let port = if session.telnet_port > 0 { session.telnet_port } else { 23 };
                                Some(format!("{}:{}", host, port))
                            } else if let Some(host) = host_arg {
                                Some(format!("{}:{}", host, port_arg.unwrap_or_else(|| "23".to_string())))
                            } else {
                                None
                            }
                        } else if let Some(host) = host_arg {
                            Some(format!("{}:{}", host, port_arg.unwrap_or_else(|| "23".to_string())))
                        } else {
                            None
                        };
                        (Some("Telnet".to_string()), conn_info, if connection_lost { icon_color } else { p.status_info })
                    } else {
                        (Some(if is_remote { "SSH".to_string() } else { "Local".to_string() }), Some(path.clone()), icon_color)
                    }
                } else {
                    (
                        Some(if is_remote { "SSH".to_string() } else { "Local".to_string() }),
                        Some(shell_type.display_name()),
                        icon_color,
                    )
                };

                let term_palette = self.terminals
                    .lock()
                    .get(&terminal_id)
                    .and_then(|t| t.palette())
                    .unwrap_or_else(|| {
                        let tvs = velowork_views_terminal::terminal_view_settings(cx);
                        velowork_core::theme::get_terminal_palette_with_custom(
                            &tvs.color_scheme,
                            &tvs.custom_terminal_color_schemes,
                        )
                    });

                let background_builder = {
                    let anim_id: SharedString = format!("minimized-preview-bg-{}", terminal_id).into();
                    Some(std::sync::Arc::new(move |cx: &App| {
                        velowork_views_terminal::terminal_background_element(cx, anim_id.clone(), false, f32::from(RADIUS_MD))
                    }) as std::sync::Arc<dyn Fn(&App) -> Option<AnyElement> + Send + Sync>)
                };

                let preview_props = velowork_ui::TerminalPreviewProps {
                    title: terminal_name.clone(),
                    icon,
                    icon_color: Some(icon_color),
                    is_remote,
                    status_text: Some(if connection_lost {
                        i18n!(cx, "status.connection_lost")
                    } else {
                        restore_tip.clone()
                    }),
                    idle_text: None,
                    is_disconnected: connection_lost,
                    is_welcome,
                    connection_info,
                    protocol_badge,
                    snapshot: preview_snapshot,
                    font_family: Some(terminal_font),
                    palette: Some(term_palette),
                    background_builder,
                };

                let card_el = div()
                    .id(ElementId::Name(format!("minimized-{}", terminal_id).into()))
                    .flex_shrink_0()
                    .cursor_pointer()
                    .max_w(px(160.0))
                    .h_full()
                    .px(ui_space_sm(cx))
                    .rounded(RADIUS_STD)
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(surface_bg_t(t.bg_panel, &t))
                    .hover(|s| s.bg(rgb(t.bg_selection)).border_color(rgb(t.border_active)))
                    .flex()
                    .items_center()
                    .gap(ui_space_xs(cx))
                    .overflow_hidden()
                    .text_size(ui_text_sm(cx));

                let card_el = if enable_tab_preview {
                    let props = preview_props;
                    card_el.tooltip(move |_, cx| {
                        let p_inner = props.clone();
                        cx.new(|_| {
                            Tooltip::element(move |_window, cx| {
                                velowork_ui::terminal_preview_card(p_inner.clone(), cx).into_any_element()
                            })
                            .bare()
                            .direction(velowork_ui::tooltip::TooltipDirection::Top)
                        })
                        .into()
                    })
                } else {
                    let tip = tip_text.clone();
                    card_el.tooltip(move |_, cx| {
                        cx.new(|_| Tooltip::new(tip.clone())).into()
                    })
                };

                let card_el = card_el
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                icon
                                    .size(ICON_SM)
                                    .flex_shrink_0()
                                    .text_color(icon_color),
                            )
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_color(rgb(t.text_primary))
                            .text_ellipsis()
                            .child(terminal_name)
                    )
                    .child(
                        AppIcon::ChevronUp
                            .size(ICON_MICRO)
                            .flex_shrink_0()
                            .text_color(rgb(t.text_muted))
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click({
                        let project_id = project_id.clone();
                        let terminal_id = terminal_id.clone();
                        let workspace = workspace.clone();
                        let focus_manager = focus_manager.clone();
                        let layout_container = self.layout_container.clone();
                        move |_, _window, cx| {
                            let tid_for_lc = terminal_id.clone();
                            focus_manager.update(cx, |fm, cx| {
                                workspace.update(cx, |ws, cx| {
                                    ws.restore_terminal_by_id(fm, &project_id, &terminal_id, cx);
                                });
                                cx.notify();
                            });
                            if let Some(ref lc) = layout_container {
                                lc.update(cx, |this, cx| {
                                    this.mark_restoring(&tid_for_lc, cx);
                                });
                            }
                        }
                    })
                    .on_mouse_down(MouseButton::Right, {
                        let project_id = project_id.clone();
                        let layout_path = layout_path.clone();
                        let request_broker = self.request_broker.clone();
                        move |event: &MouseDownEvent, _window, cx| {
                            request_broker.update(cx, |broker, cx| {
                                broker.push_overlay_request(
                                    velowork_workspace::requests::OverlayRequest::Project(
                                        velowork_workspace::requests::ProjectOverlay {
                                            project_id: project_id.clone(),
                                            kind: velowork_workspace::requests::ProjectOverlayKind::TabContextMenu {
                                                tab_index: 0,
                                                num_tabs: 1,
                                                layout_path: layout_path.clone(),
                                                position: event.position,
                                            },
                                        }
                                    ),
                                    cx,
                                );
                            });
                            cx.stop_propagation();
                        }
                    });

                let ws_version = self.workspace.read(cx).data_version();
                let enable_animations = velowork_app_core::settings::settings(cx).enable_animations;
                if enable_animations {
                    card_el.with_animation(
                        format!("minimized-capsule-enter-{}-{}", terminal_id, ws_version),
                        Animation::new(std::time::Duration::from_millis(240))
                            .with_easing(ease_tab_expand),
                        |this, delta| {
                            let t = delta;
                            this.relative()
                                .left(px(-12.0 * (1.0 - t)))
                                .opacity(t)
                                .max_w(px(160.0 * t))
                        },
                    ).into_any_element()
                } else {
                    card_el.into_any_element()
                }
            });

        let detached_elements = detached_terminals
            .into_iter()
            .map(|(terminal_id, _layout_path, shell_type)| {
                let workspace = self.workspace.clone();
                let terminal_id_for_click = terminal_id.clone();
                let attach_tip = attach_tip.clone();

                let is_remote = shell_type.is_remote() || project.is_remote || self.backend.is_remote();
                let connection_lost = is_terminal_connection_lost(&terminal_id, cx);
                let icon_color = if connection_lost { p.status_error } else { p.status_success };
                let icon = if is_remote { AppIcon::Server } else { AppIcon::Terminal };

                let terminal_name = {
                    let osc_title = self
                        .terminals
                        .lock()
                        .get(&terminal_id)
                        .and_then(|t| t.title());
                    let base_name = if matches!(shell_type, velowork_core::shell::ShellType::Welcome) {
                        i18n!(cx, "welcome.title")
                    } else {
                        velowork_views_terminal::layout::session_labels::terminal_base_name(
                            &terminal_id,
                            &shell_type,
                            is_remote,
                            osc_title.as_deref(),
                            project,
                            &session_store,
                        )
                    };
                    if let Some(&suffix) = suffixes.get(&terminal_id) {
                        if suffix > 1 {
                            format!("{}:{}", base_name, suffix)
                        } else {
                            base_name
                        }
                    } else {
                        base_name
                    }
                };

                let tip_text = format!("{}: {}", attach_tip, terminal_name);

                let enable_tab_preview = velowork_app_core::settings::settings(cx).enable_tab_preview;
                let preview_snapshot = self.terminals.lock().get(&terminal_id).map(|t| t.preview_snapshot(16));
                let is_welcome = matches!(shell_type, velowork_core::shell::ShellType::Welcome);
                let terminal_font = velowork_views_terminal::terminal_view_settings(cx).font_family.clone();

                let (protocol_badge, connection_info, icon_color) = if is_welcome {
                    (
                        Some(i18n!(cx, "welcome.title")),
                        Some(i18n!(cx, "welcome.subtitle")),
                        p.surface_accent,
                    )
                } else if let velowork_core::shell::ShellType::Custom { path, args } = &shell_type {
                    if path == "ssh" {
                        let store = session_store;
                        let mut session_id = None;
                        let mut host_arg = None;
                        let mut i = 0;
                        while i < args.len() {
                            if (args[i] == "--id" || args[i] == "--session-id") && i + 1 < args.len() {
                                session_id = Some(args[i + 1].clone());
                                i += 2;
                            } else if (args[i] == "-p" || args[i] == "-i") && i + 1 < args.len() {
                                i += 2;
                            } else if !args[i].starts_with('-') {
                                host_arg = Some(args[i].clone());
                                i += 1;
                            } else {
                                i += 1;
                            }
                        }
                        let conn_info = if let Some(sid) = session_id {
                            if let Some(session) = store.find_session(&sid) {
                                let user_prefix = if !session.username.is_empty() {
                                    format!("{}@", session.username)
                                } else {
                                    String::new()
                                };
                                let port_suffix = if session.port != 22 && session.port != 0 {
                                    format!(":{}", session.port)
                                } else {
                                    String::new()
                                };
                                Some(format!("{}{}{}", user_prefix, session.host, port_suffix))
                            } else {
                                host_arg
                            }
                        } else {
                            host_arg
                        };
                        (Some("SSH".to_string()), conn_info, icon_color)
                    } else if path == "serial" {
                        let store = session_store;
                        let mut session_id = None;
                        let mut port_arg = None;
                        let mut baud_arg = None;
                        let mut i = 0;
                        while i < args.len() {
                            if args[i] == "--id" && i + 1 < args.len() {
                                session_id = Some(args[i + 1].clone());
                                i += 2;
                            } else if args[i] == "--port" && i + 1 < args.len() {
                                port_arg = Some(args[i + 1].clone());
                                i += 2;
                            } else if args[i] == "--baud" && i + 1 < args.len() {
                                baud_arg = Some(args[i + 1].clone());
                                i += 2;
                            } else {
                                i += 1;
                            }
                        }
                        let conn_info = if let Some(sid) = session_id {
                            if let Some(session) = store.find_session(&sid) {
                                let port_str = session.serial_port.as_deref().unwrap_or(port_arg.as_deref().unwrap_or(""));
                                Some(format!("{} · {} baud", port_str, session.serial_baud_rate))
                            } else {
                                port_arg.map(|p| format!("{} · {} baud", p, baud_arg.unwrap_or_else(|| "115200".to_string())))
                            }
                        } else {
                            port_arg.map(|p| format!("{} · {} baud", p, baud_arg.unwrap_or_else(|| "115200".to_string())))
                        };
                        (Some("Serial".to_string()), conn_info, if connection_lost { icon_color } else { p.status_warning })
                    } else if path == "telnet" {
                        let store = session_store;
                        let mut session_id = None;
                        let mut host_arg = None;
                        let mut port_arg = None;
                        let mut i = 0;
                        while i < args.len() {
                            if args[i] == "--id" && i + 1 < args.len() {
                                session_id = Some(args[i + 1].clone());
                                i += 2;
                            } else if args[i] == "--host" && i + 1 < args.len() {
                                host_arg = Some(args[i + 1].clone());
                                i += 2;
                            } else if args[i] == "--port" && i + 1 < args.len() {
                                port_arg = Some(args[i + 1].clone());
                                i += 2;
                            } else {
                                i += 1;
                            }
                        }
                        let conn_info = if let Some(sid) = session_id {
                            if let Some(session) = store.find_session(&sid) {
                                let host = session.telnet_host.as_deref().unwrap_or(session.host.as_str());
                                let port = if session.telnet_port > 0 { session.telnet_port } else { 23 };
                                Some(format!("{}:{}", host, port))
                            } else if let Some(host) = host_arg {
                                Some(format!("{}:{}", host, port_arg.unwrap_or_else(|| "23".to_string())))
                            } else {
                                None
                            }
                        } else if let Some(host) = host_arg {
                            Some(format!("{}:{}", host, port_arg.unwrap_or_else(|| "23".to_string())))
                        } else {
                            None
                        };
                        (Some("Telnet".to_string()), conn_info, if connection_lost { icon_color } else { p.status_info })
                    } else {
                        (Some(if is_remote { "SSH".to_string() } else { "Local".to_string() }), Some(path.clone()), icon_color)
                    }
                } else {
                    (
                        Some(if is_remote { "SSH".to_string() } else { "Local".to_string() }),
                        Some(shell_type.display_name()),
                        icon_color,
                    )
                };

                let term_palette = self.terminals
                    .lock()
                    .get(&terminal_id)
                    .and_then(|t| t.palette())
                    .unwrap_or_else(|| {
                        let tvs = velowork_views_terminal::terminal_view_settings(cx);
                        velowork_core::theme::get_terminal_palette_with_custom(
                            &tvs.color_scheme,
                            &tvs.custom_terminal_color_schemes,
                        )
                    });

                let background_builder = {
                    let anim_id: SharedString = format!("detached-preview-bg-{}", terminal_id).into();
                    Some(std::sync::Arc::new(move |cx: &App| {
                        velowork_views_terminal::terminal_background_element(cx, anim_id.clone(), false, f32::from(RADIUS_MD))
                    }) as std::sync::Arc<dyn Fn(&App) -> Option<AnyElement> + Send + Sync>)
                };

                let preview_props = velowork_ui::TerminalPreviewProps {
                    title: terminal_name.clone(),
                    icon,
                    icon_color: Some(icon_color),
                    is_remote,
                    status_text: Some(if connection_lost {
                        i18n!(cx, "status.connection_lost")
                    } else {
                        attach_tip.clone()
                    }),
                    idle_text: None,
                    is_disconnected: connection_lost,
                    is_welcome,
                    connection_info,
                    protocol_badge,
                    snapshot: preview_snapshot,
                    font_family: Some(terminal_font),
                    palette: Some(term_palette),
                    background_builder,
                };

                let card_el = div()
                    .id(ElementId::Name(format!("detached-{}", terminal_id).into()))
                    .flex_shrink_0()
                    .cursor_pointer()
                    .max_w(px(160.0))
                    .h_full()
                    .px(ui_space_sm(cx))
                    .rounded(RADIUS_STD)
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(surface_bg_t(t.bg_panel, &t))
                    .hover(|s| s.bg(rgb(t.bg_selection)).border_color(rgb(t.border_active)))
                    .flex()
                    .items_center()
                    .gap(ui_space_xs(cx))
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_primary));

                let card_el = if enable_tab_preview {
                    let props = preview_props;
                    card_el.tooltip(move |_, cx| {
                        let p_inner = props.clone();
                        cx.new(|_| {
                            Tooltip::element(move |_window, cx| {
                                velowork_ui::terminal_preview_card(p_inner.clone(), cx).into_any_element()
                            })
                            .bare()
                            .direction(velowork_ui::tooltip::TooltipDirection::Top)
                        })
                        .into()
                    })
                } else {
                    let tip = tip_text.clone();
                    card_el.tooltip(move |_, cx| {
                        cx.new(|_| Tooltip::new(tip.clone())).into()
                    })
                };

                card_el
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                icon
                                    .size(ICON_SM)
                                    .flex_shrink_0()
                                    .text_color(icon_color),
                            )
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_color(rgb(t.text_primary))
                            .text_ellipsis()
                            .child(terminal_name)
                    )
                    .child(
                        AppIcon::ExternalLink
                            .size(ICON_MICRO)
                            .flex_shrink_0()
                            .text_color(rgb(t.text_muted))
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(move |_, _window, cx| {
                        workspace.update(cx, |ws, cx| {
                            ws.attach_terminal(&terminal_id_for_click, cx);
                        });
                    })
            });

        let scroll_container_id = format!("hidden-taskbar-scroll-container-{}", self.project_id);
        let scroll_id = format!("hidden-taskbar-scroll-{}", self.project_id);

        div()
            .id(ElementId::Name(scroll_container_id.into()))
            .occlude()
            .absolute()
            .bottom(ui_space_sm(cx))
            .left(ui_space_sm(cx))
            .right(ui_space_sm(cx))
            .h(px(28.0))
            .child(
                div()
                    .id(ElementId::Name(scroll_id.into()))
                    .size_full()
                    .flex()
                    .items_center()
                    .gap(ui_space_xs(cx))
                    .overflow_x_scroll()
                    .track_scroll(&self.taskbar_scroll_handle)
                    .children(minimized_elements)
                    .children(detached_elements),
            )
            .child(
                Scrollbar::horizontal(&self.taskbar_scroll_handle)
                    .scrollbar_show(ScrollbarShow::Hover),
            )
            .into_any_element()
    }

    /// Render the creating-state placeholder (no background: the shared
    /// backdrop is painted once by the parent `ProjectColumn`).
    fn render_creating_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        v_flex()
            .items_center()
            .justify_center()
            .size_full()
            .gap(ui_space_lg(cx))
            .child(
                AppIcon::GitBranch
                    .size(px(48.0))
                    .text_color(rgb(t.text_muted)),
            )
            .child(
                div()
                    .text_size(ui_text_xl(cx))
                    .text_color(rgb(t.text_secondary))
                    .child(i18n!(cx, "project.settingting_up_worktree")),
            )
            .child(
                div()
                    .text_size(ui_text_ms(cx))
                    .text_color(rgb(t.text_muted))
                    .max_w(px(240.0))
                    .text_center()
                    .child(i18n!(cx, "project.settingting_up_worktree_desc")),
            )
    }

    /// Connect to an SSH/Serial/Telnet/Local session and focus its terminal
    fn connect_to_session(
        &self,
        session: velowork_state::SshSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shell = welcome::session_to_shell_type(&session);
        let session_id = session.id.clone();
        let pid = self.project_id.clone();
        let mut target_path = None;

        self.focus_manager.update(cx, |fm, cx| {
            self.workspace.update(cx, |ws, cx| {
                if let Some(conn_store_global) =
                    cx.try_global::<velowork_workspace::stores::GlobalConnectionStore>()
                {
                    let conn_store = conn_store_global.0.clone();
                    conn_store.update(cx, |store, cx| {
                        store.mark_connected(&session_id, cx);
                    });
                }
                let has_layout = ws
                    .project(&pid)
                    .map(|p| p.layout.is_some())
                    .unwrap_or(false);
                if has_layout {
                    let path = fm
                        .focused_terminal_state()
                        .filter(|state| state.project_id == pid)
                        .map(|state| state.layout_path.clone())
                        .unwrap_or_default();
                    // If the currently focused pane is a Welcome placeholder,
                    // replace it in-place rather than adding a new Tab sibling.
                    let focused_is_welcome = ws
                        .get_terminal_shell(&pid, &path)
                        .map(|st| st == velowork_core::shell::ShellType::Welcome)
                        .unwrap_or(false);
                    if focused_is_welcome {
                        ws.replace_terminal_shell(&pid, &path, shell.clone(), cx);
                        ws.set_focused_terminal(fm, pid.clone(), path, cx);
                    } else {
                        ws.add_tab_with_shell(fm, &pid, &path, shell.clone(), cx);
                    }
                } else {
                    ws.add_terminal_with_shell(fm, &pid, shell.clone(), cx);
                }
                if let Some(path) = fm
                    .focused_terminal_state()
                    .filter(|state| state.project_id == pid)
                    .map(|state| state.layout_path.clone())
                {
                    target_path = Some(path);
                }
            });
        });

        let pid = pid.clone();
        let window_id = self.window_id;
        if let Some(path) = target_path {
            Self::schedule_focus_pane(window, window_id, pid, path, 10, cx);
        }
    }

    fn schedule_focus_pane(
        window: &mut Window,
        window_id: WindowId,
        project_id: String,
        path: Vec<usize>,
        retries_left: usize,
        cx: &mut App,
    ) {
        let pane_map = velowork_views_terminal::layout::navigation::get_pane_map(window_id);
        if let Some(pane) = pane_map.find_pane(&project_id, &path) {
            if let Some(ref fh) = pane.focus_handle {
                window.focus(fh, cx);
                return;
            }
        }
        if retries_left > 0 {
            window.on_next_frame(move |window, cx| {
                Self::schedule_focus_pane(window, window_id, project_id, path, retries_left - 1, cx);
            });
        } else {
            log::warn!(
                "project_panel schedule_focus_pane exhausted retries for project {} at path {:?}",
                project_id,
                path
            );
        }
    }

    fn execute_welcome_action(
        &mut self,
        action: welcome::WelcomeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            welcome::WelcomeAction::StartTerminal => {
                let pid = self.project_id.clone();
                let dispatcher = self.action_dispatcher.clone();
                let workspace = self.workspace.clone();
                let focus_manager = self.focus_manager.clone();
                let window_id = self.window_id;

                focus_manager.update(cx, |_fm, cx| {
                    workspace.update(cx, |ws, cx| {
                        ws.touch_project(&pid);
                        ws.bump_activity(&pid, cx);
                    });
                });
                if let Some(ref d) = dispatcher {
                    d.dispatch(
                        ActionRequest::CreateTerminal {
                            project_id: pid.clone(),
                        },
                        cx,
                    );
                }
                let pid = pid.clone();
                Self::schedule_focus_pane(window, window_id, pid, vec![], 10, cx);
            }
            welcome::WelcomeAction::ConnectSession(session) => {
                self.connect_to_session(session, window, cx);
            }
            welcome::WelcomeAction::NewSession => {
                window.dispatch_action(Box::new(crate::keybindings::NewSession), cx);
            }
            welcome::WelcomeAction::AiAssistant => {
                window.dispatch_action(Box::new(crate::keybindings::ShowAiAssistant), cx);
            }
            welcome::WelcomeAction::QuickCommands => {
                window.dispatch_action(Box::new(crate::keybindings::ShowQuickCommandsPanel), cx);
            }
            welcome::WelcomeAction::ImportSessions => {
                window.dispatch_action(Box::new(crate::keybindings::ShowImportSessionDialog), cx);
            }
        }
    }

    /// 焦点聚焦入口：优先聚焦活动终端实体，若为欢迎页/空状态则聚焦快速连接输入框
    pub fn focus_active_element(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
        });

        // 若尚未创建 layout_container 但项目存在 layout，主动执行预热
        if self.layout_container.is_none() {
            let ws = self.workspace.read(cx);
            if let Some(project) = self.get_project(ws) {
                if project.layout.is_some() {
                    let path = project.path.clone();
                    self.ensure_layout_container(path, cx);
                }
            }
        }

        if let Some(ref lc) = self.layout_container {
            if let Some(pane) = lc.read(cx).active_terminal_pane(cx) {
                let handle = pane.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
                if window.focused(cx).is_some() {
                    return;
                }
            }
        }

        self.focus_quick_connect(window, cx);
    }

    /// Focus the welcome quick-connect input in this project column
    pub fn focus_quick_connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
        });
        self.quick_connect_input.update(cx, |s, cx| {
            s.focus(window, cx);
        });
        cx.notify();
    }

    /// Render the modern card dashboard when no terminal is attached.
    fn render_empty_state(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shortcuts = welcome::WelcomeShortcuts::from_cx(cx);

        welcome::render_welcome_dashboard(
            &self.project_id,
            &self.quick_connect_input,
            self.welcome_selected_index,
            shortcuts,
            window,
            cx,
            cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if let Some(action) = welcome::handle_welcome_key_down(
                    &this.project_id,
                    &this.quick_connect_input,
                    &mut this.welcome_selected_index,
                    event,
                    cx,
                ) {
                    this.execute_welcome_action(action, window, cx);
                }
                cx.notify();
            }),
            cx.listener(|this, _, window, cx| {
                if let Some(action) = welcome::handle_welcome_quick_connect(
                    &this.project_id,
                    &this.quick_connect_input,
                    &mut this.welcome_selected_index,
                    cx,
                ) {
                    this.execute_welcome_action(action, window, cx);
                }
            }),
            cx.listener(|this, action: &welcome::WelcomeAction, window, cx| {
                this.execute_welcome_action(action.clone(), window, cx);
            }),
        )
    }
}

impl Render for ProjectColumn {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let workspace = self.workspace.read(cx);
        let project = self.get_project(workspace).cloned();

        match project {
            Some(project) => {
                let has_layout = project.layout.is_some();

                let is_creating = workspace.is_creating_project(&self.project_id);

                // The single shared terminal backdrop (`term_background` color +
                // optional image, global opacity applied once) is painted exactly
                // ONCE here at the bottom-most layer of the central region. Both
                // the terminal `LayoutContainer` and the empty / creating
                // placeholder states render ABOVE this layer as transparent
                // components, so the backdrop looks identical in every state and
                // the global transparency is never applied more than once.

                let is_windowed = !window.is_maximized() && !window.is_fullscreen();
                let is_custom_titlebar = velowork_ui::decorations::is_custom_titlebar(window, cx);
                let window_corner_radius =
                    settings_entity(cx).read(cx).settings.window_corner_radius;
                let has_rounded_corners =
                    is_custom_titlebar && is_windowed && window_corner_radius > 0.0;
                let terminal_fullscreen = self.focus_manager.read(cx).has_fullscreen();

                let card_r = if terminal_fullscreen {
                    if has_rounded_corners {
                        window_corner_radius
                    } else {
                        0.0
                    }
                } else {
                    f32::from(RADIUS_CARD)
                };

                let has_real_term = project
                    .layout
                    .as_ref()
                    .map(|l| l.has_real_terminal())
                    .unwrap_or(false);
                let bg_element = if has_layout && has_real_term {
                    velowork_views_terminal::layout::layout_container::render_terminal_shared_background(
                        cx,
                        card_r,
                        terminal_fullscreen,
                    )
                } else {
                    let palette = SemanticPalette::from_theme(&t);
                    div()
                        .absolute()
                        .inset_0()
                        .size_full()
                        .bg(palette.surface_card)
                        .when(card_r > 0.0, |d| {
                            if terminal_fullscreen {
                                d.rounded_bl(px(card_r)).rounded_br(px(card_r))
                            } else {
                                d.rounded(px(card_r))
                            }
                        })
                        .into_any_element()
                };

                let content = if has_layout {
                    self.ensure_layout_container(project.path.clone(), cx);

                    div()
                        .id("project-column-content")
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .when_some(self.layout_container.clone(), |d, container| {
                            d.child(container)
                        })
                        .into_any_element()
                } else {
                    let non_layout_content = if is_creating {
                        self.render_creating_state(cx).into_any_element()
                    } else {
                        self.render_empty_state(window, cx).into_any_element()
                    };

                    div()
                        .id("console")
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .relative()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.focus_manager.update(cx, |fm, _| {
                                    fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
                                });
                                this.quick_connect_input.update(cx, |inp, cx| inp.focus(window, cx));
                            }),
                        )
                        .child(non_layout_content)
                        .into_any_element()
                };

                let hidden_taskbar = self.render_hidden_taskbar(&project, t, cx);

                let this_entity = cx.entity().downgrade();
                let bounds_tracker = canvas(
                    move |bounds, _window, cx| {
                        if let Some(entity) = this_entity.upgrade() {
                            entity.update(cx, |this, _| {
                                this.column_bounds = Some(bounds);
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0();

                div()
                    .id("project-column-main")
                    .relative()
                    .flex()
                    .flex_col()
                    .size_full()
                    .min_h_0()
                    .overflow_hidden()
                    .child(bg_element)
                    .child(content)
                    .child(hidden_taskbar)
                    .child(bounds_tracker)
                    .into_any_element()
            }

            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(t.text_muted))
                .child(i18n!(cx, "project.not_found"))
                .into_any_element(),
        }
    }
}
