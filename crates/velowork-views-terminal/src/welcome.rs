//! Welcome dashboard rendering and actions shared between TerminalPane and ProjectColumn.

use gpui::prelude::FluentBuilder;
use gpui::*;
use velowork_core::shell::ShellType;
use velowork_i18n::i18n;
use velowork_state::{SessionProtocol, SessionTreeNode, SshAuthType, SshSession};
use velowork_ui::brand_logo;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::input::focus_ring_shadows;
use velowork_ui::simple_input::{SimpleInput, SimpleInputState};
use velowork_ui::theme::theme;
use velowork_ui::tokens::*;
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex};
use velowork_workspace::stores::{GlobalConnectionStore, GlobalSessionStore};

/// Action triggered from the welcome dashboard
#[derive(Clone, Debug)]
pub enum WelcomeAction {
    StartTerminal,
    ConnectSession(SshSession),
    NewSession,
    AiAssistant,
    QuickCommands,
    ImportSessions,
}

use std::sync::Arc;

/// Dynamic keyboard shortcuts for the 4 quick start cards
#[derive(Clone, Debug, Default)]
pub struct WelcomeShortcuts {
    pub terminal: Option<String>,
    pub session: Option<String>,
    pub ai: Option<String>,
    pub quick_commands: Option<String>,
}

/// Global shortcut provider for resolving action keybindings in lower-level crates
#[derive(Clone)]
pub struct GlobalShortcutProvider(pub Arc<dyn Fn(&str) -> Option<String> + Send + Sync>);
impl Global for GlobalShortcutProvider {}

impl WelcomeShortcuts {
    /// Resolve welcome shortcuts dynamically from the global shortcut provider or fallback to defaults
    pub fn from_cx(cx: &App) -> Self {
        if let Some(provider) = cx.try_global::<GlobalShortcutProvider>() {
            Self {
                terminal: (provider.0)("AddTab"),
                session: (provider.0)("NewSession"),
                ai: (provider.0)("ShowAiAssistant"),
                quick_commands: (provider.0)("ShowQuickCommandsPanel"),
            }
        } else {
            Self {
                terminal: Some("Ctrl+Shift+`".to_string()),
                session: Some("Ctrl+N".to_string()),
                ai: Some("Ctrl+Shift+A".to_string()),
                quick_commands: Some("Ctrl+Shift+K".to_string()),
            }
        }
    }
}

/// Retrieve ordered and filtered sessions for welcome dashboard
pub fn get_welcome_display_sessions(
    project_id: &str,
    quick_connect_input: &Entity<SimpleInputState>,
    cx: &App,
) -> (Vec<SshSession>, bool, String) {
    let (recent_ids, saved_sessions) = {
        let session_store = cx.try_global::<GlobalSessionStore>();
        let conn_store = cx.try_global::<GlobalConnectionStore>();

        let recent_ids = conn_store
            .map(|cs| cs.0.read(cx).recent_session_ids().to_vec())
            .unwrap_or_default();

        let saved = if let Some(ss) = session_store {
            let s_ref = ss.0.read(cx);
            let mut list = Vec::new();
            fn collect_nodes(nodes: &[SessionTreeNode], out: &mut Vec<SshSession>) {
                for node in nodes {
                    match node {
                        SessionTreeNode::Session { session } => out.push(session.clone()),
                        SessionTreeNode::Folder { children, .. } => collect_nodes(children, out),
                    }
                }
            }
            collect_nodes(s_ref.tree_for_project(Some(project_id)), &mut list);
            collect_nodes(s_ref.tree(), &mut list);
            list
        } else {
            Vec::new()
        };

        (recent_ids, saved)
    };

    let mut ordered_sessions = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for id in &recent_ids {
        if let Some(session) = saved_sessions.iter().find(|s| &s.id == id) {
            if seen.insert(session.id.clone()) {
                ordered_sessions.push(session.clone());
            }
        }
    }

    for session in saved_sessions {
        if seen.insert(session.id.clone()) {
            ordered_sessions.push(session);
        }
    }

    let search_query = quick_connect_input.read(cx).value().trim().to_string();
    let is_searching = !search_query.is_empty();
    let search_lower = search_query.to_lowercase();

    let display_sessions: Vec<SshSession> = if is_searching {
        ordered_sessions
            .into_iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&search_lower)
                    || s.host.to_lowercase().contains(&search_lower)
                    || s.username.to_lowercase().contains(&search_lower)
                    || format!("{}@{}", s.username, s.host)
                        .to_lowercase()
                        .contains(&search_lower)
            })
            .collect()
    } else {
        ordered_sessions.into_iter().take(6).collect()
    };

    (display_sessions, is_searching, search_query)
}

/// Handle submit / Enter in quick connect bar
pub fn handle_welcome_quick_connect(
    project_id: &str,
    quick_connect_input: &Entity<SimpleInputState>,
    welcome_selected_index: &mut Option<usize>,
    cx: &mut App,
) -> Option<WelcomeAction> {
    let text = quick_connect_input.read(cx).value().trim().to_string();
    if text.is_empty() {
        quick_connect_input.update(cx, |input, cx| input.set_value("", cx));
        *welcome_selected_index = None;
        return Some(WelcomeAction::StartTerminal);
    }

    if let Some(store_global) = cx.try_global::<GlobalSessionStore>() {
        let s_ref = store_global.0.read(cx);
        let mut list = Vec::new();
        fn collect_nodes(nodes: &[SessionTreeNode], out: &mut Vec<SshSession>) {
            for node in nodes {
                match node {
                    SessionTreeNode::Session { session } => out.push(session.clone()),
                    SessionTreeNode::Folder { children, .. } => collect_nodes(children, out),
                }
            }
        }
        collect_nodes(s_ref.tree_for_project(Some(project_id)), &mut list);
        collect_nodes(s_ref.tree(), &mut list);

        if let Some(session) = list
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(&text) || s.host.eq_ignore_ascii_case(&text))
        {
            let session = session.clone();
            quick_connect_input.update(cx, |input, cx| input.set_value("", cx));
            *welcome_selected_index = None;
            return Some(WelcomeAction::ConnectSession(session));
        }
    }

    let is_explicit_ssh = text.starts_with("ssh ");
    let has_at = text.contains('@');
    let has_colon = text.contains(':');
    let has_dot = text.contains('.');
    let is_localhost = text.eq_ignore_ascii_case("localhost") || text.starts_with("localhost:");

    // Only perform quick connect if text meets direct connection format requirements
    if !is_explicit_ssh && !has_at && !has_colon && !has_dot && !is_localhost {
        return None;
    }

    let clean = text.strip_prefix("ssh ").unwrap_or(&text).trim();
    let (user_part, host_port_part) = if let Some((u, h)) = clean.split_once('@') {
        (u.trim(), h.trim())
    } else {
        ("", clean)
    };

    let (host_str, port_num) = if let Some((h, p)) = host_port_part.split_once(':') {
        (h.trim(), p.trim().parse::<u16>().unwrap_or(22))
    } else {
        (host_port_part, 22)
    };

    if host_str.is_empty() {
        return None;
    }

    let default_username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".to_string());
    let final_username = if user_part.is_empty() {
        default_username
    } else {
        user_part.to_string()
    };

    let session = SshSession {
        id: uuid::Uuid::new_v4().to_string(),
        name: if user_part.is_empty() {
            host_str.to_string()
        } else {
            format!("{}@{}", user_part, host_str)
        },
        host: host_str.to_string(),
        port: port_num,
        username: final_username,
        protocol: SessionProtocol::Ssh,
        ..Default::default()
    };

    quick_connect_input.update(cx, |input, cx| input.set_value("", cx));
    *welcome_selected_index = None;
    Some(WelcomeAction::ConnectSession(session))
}

/// Handle key down events (Up, Down, Enter, Escape) on quick connect bar
pub fn handle_welcome_key_down(
    project_id: &str,
    quick_connect_input: &Entity<SimpleInputState>,
    welcome_selected_index: &mut Option<usize>,
    event: &KeyDownEvent,
    cx: &mut App,
) -> Option<WelcomeAction> {
    let key = event.keystroke.key.as_str();
    match key {
        "down" => {
            let (sessions, _, _) =
                get_welcome_display_sessions(project_id, quick_connect_input, cx);
            if !sessions.is_empty() {
                let next = match *welcome_selected_index {
                    None => 0,
                    Some(curr) => (curr + 1).min(sessions.len() - 1),
                };
                *welcome_selected_index = Some(next);
            }
            None
        }
        "up" => {
            let (sessions, _, _) =
                get_welcome_display_sessions(project_id, quick_connect_input, cx);
            if !sessions.is_empty() {
                let prev = match *welcome_selected_index {
                    None => None,
                    Some(0) => None,
                    Some(curr) => Some(curr - 1),
                };
                *welcome_selected_index = prev;
            }
            None
        }
        "enter" => {
            if let Some(idx) = *welcome_selected_index {
                let (sessions, _, _) =
                    get_welcome_display_sessions(project_id, quick_connect_input, cx);
                if let Some(session) = sessions.get(idx) {
                    let session = session.clone();
                    quick_connect_input.update(cx, |input, cx| input.set_value("", cx));
                    *welcome_selected_index = None;
                    return Some(WelcomeAction::ConnectSession(session));
                }
            }
            handle_welcome_quick_connect(
                project_id,
                quick_connect_input,
                welcome_selected_index,
                cx,
            )
        }
        "escape" => {
            quick_connect_input.update(cx, |input, cx| input.set_value("", cx));
            *welcome_selected_index = None;
            None
        }
        _ => None,
    }
}

/// Render the welcome dashboard
pub fn render_welcome_dashboard(
    project_id: &str,
    quick_connect_input: &Entity<SimpleInputState>,
    welcome_selected_index: Option<usize>,
    shortcuts: WelcomeShortcuts,
    window: &Window,
    cx: &App,
    on_key_down: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    on_submit: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_action: impl Fn(&WelcomeAction, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<Div> {
    let t = theme(cx);
    let p = SemanticPalette::from_context(cx);

    let on_action = std::rc::Rc::new(on_action);

    let (display_sessions, is_searching, search_query) =
        get_welcome_display_sessions(project_id, quick_connect_input, cx);

    let shortcut_terminal = shortcuts.terminal.as_deref().or(Some("Ctrl+Shift+`"));
    let shortcut_session = shortcuts.session.as_deref().or(Some("Ctrl+N"));
    let shortcut_ai = shortcuts.ai.as_deref();
    let shortcut_quick_commands = shortcuts.quick_commands.as_deref();

    let on_action_terminal = on_action.clone();
    let on_action_session = on_action.clone();
    let on_action_ai = on_action.clone();
    let on_action_commands = on_action.clone();
    let on_action_import = on_action.clone();

    v_flex()
        .id("welcome-dashboard-scroll")
        .items_center()
        .justify_start()
        .size_full()
        .overflow_y_scroll()
        .px(SPACE_XL)
        .py(px(52.0))
        .child(
            v_flex()
                .max_w(px(840.0))
                .w_full()
                .items_center()
                .gap(px(36.0))
                // --- 1. Hero Header: Logo, Title & Subtitle ---
                .child(
                    v_flex().items_center().gap(SPACE_MD).child(
                        h_flex()
                            .items_center()
                            .gap(SPACE_MD)
                            .child(brand_logo(px(56.0), window, cx))
                            .child(
                                v_flex()
                                    .items_start()
                                    .gap(SPACE_2XS)
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap(SPACE_SM)
                                            .child(
                                                div()
                                                    .text_size(ui_text(24.0, cx))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(p.text_primary)
                                                    .child(i18n!(cx, "welcome.title")),
                                            )
                                            .child(
                                                div()
                                                    .px(SPACE_XS)
                                                    .py(px(1.5))
                                                    .rounded(RADIUS_XS)
                                                    .bg(p.surface_accent.opacity(0.12))
                                                    .border_1()
                                                    .border_color(p.surface_accent.opacity(0.35))
                                                    .text_size(ui_text_xs(cx))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(p.surface_accent)
                                                    .child("v0.1.0"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(ui_text_ms(cx))
                                            .text_color(p.text_muted)
                                            .child(i18n!(cx, "welcome.subtitle")),
                                    ),
                            ),
                    ),
                )
                // --- 2. Quick Connect / Search Bar ---
                .child({
                    let is_focused = quick_connect_input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window);
                    let ring = focus_ring_shadows(&t);

                    h_flex()
                        .id("welcome-quick-connect-bar")
                        .w_full()
                        .max_w(px(660.0))
                        .h(px(40.0))
                        .items_center()
                        .gap(SPACE_SM)
                        .pl(SPACE_MD)
                        .pr(px(4.0))
                        .rounded(RADIUS_LG)
                        .bg(if is_focused {
                            p.surface_hover
                        } else {
                            p.surface_card
                        })
                        .border_1()
                        .border_color(if is_focused {
                            p.border_active
                        } else {
                            p.border_subtle
                        })
                        .when(is_focused, |s| s.shadow(ring))
                        .when(!is_focused, |s| {
                            s.hover(|h| {
                                h.border_color(p.surface_accent.opacity(0.6))
                                    .bg(p.surface_hover)
                            })
                        })
                        .cursor_text()
                        .on_mouse_down(MouseButton::Left, {
                            let input = quick_connect_input.clone();
                            move |_event, window, cx| {
                                input.update(cx, |s, cx| s.focus(window, cx));
                            }
                        })
                        .on_key_down(on_key_down)
                        .child(
                            AppIcon::Terminal
                                .size(ICON_STD)
                                .text_color(p.surface_accent),
                        )
                        .child(
                            div()
                                .flex_1()
                                .h_full()
                                .flex()
                                .items_center()
                                .child(SimpleInput::new(quick_connect_input).appearance(false)),
                        )
                        .child(
                            h_flex()
                                .id("welcome-quick-connect-submit")
                                .cursor_pointer()
                                .h(px(32.0))
                                .items_center()
                                .justify_center()
                                .gap(SPACE_XS)
                                .px(SPACE_MD)
                                .rounded(RADIUS_MD)
                                .bg(p.surface_accent)
                                .text_color(p.text_on_accent)
                                .hover(|s| s.opacity(0.9))
                                .tooltip({
                                    let tip = i18n!(cx, "welcome.quick_connect_tip");
                                    move |_, cx| cx.new(|_| Tooltip::new(tip.clone())).into()
                                })
                                .on_click(on_submit)
                                .child(
                                    div()
                                        .text_size(ui_text_ms(cx))
                                        // .font_weight(FontWeight::SEMIBOLD)
                                        .child(i18n!(cx, "welcome.quick_connect_btn")),
                                )
                                .child(
                                    AppIcon::ChevronRight
                                        .size(ICON_SM)
                                        .text_color(p.text_on_accent),
                                ),
                        )
                })
                // --- 3. Core Actions Grid (2x2) - Hidden during search ---
                .when(!is_searching, |parent| {
                    parent.child(
                        v_flex()
                            .w_full()
                            .gap(SPACE_MD)
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(p.text_primary)
                                    .pb(px(2.0))
                                    .child(i18n!(cx, "welcome.quick_start")),
                            )
                            .child(
                                v_flex()
                                    .w_full()
                                    .gap(SPACE_MD)
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .gap(SPACE_MD)
                                            .child(render_action_card(
                                                "welcome-card-start-terminal",
                                                AppIcon::Terminal,
                                                i18n!(cx, "welcome.start_terminal"),
                                                i18n!(cx, "welcome.start_terminal_desc"),
                                                p.surface_accent,
                                                shortcut_terminal,
                                                move |_, window, cx| {
                                                    on_action_terminal(
                                                        &WelcomeAction::StartTerminal,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                                &p,
                                                cx,
                                            ))
                                            .child(render_action_card(
                                                "welcome-card-new-session",
                                                AppIcon::Plus,
                                                i18n!(cx, "session.new_session"),
                                                i18n!(cx, "welcome.new_session_desc"),
                                                rgb(0x10b981).into(),
                                                shortcut_session,
                                                move |_, window, cx| {
                                                    on_action_session(
                                                        &WelcomeAction::NewSession,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                                &p,
                                                cx,
                                            )),
                                    )
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .gap(SPACE_MD)
                                            .child(render_action_card(
                                                "welcome-card-ai-assistant",
                                                AppIcon::AiAssistant,
                                                i18n!(cx, "welcome.ai_assistant"),
                                                i18n!(cx, "welcome.ai_assistant_desc"),
                                                rgb(0x8b5cf6).into(),
                                                shortcut_ai,
                                                move |_, window, cx| {
                                                    on_action_ai(
                                                        &WelcomeAction::AiAssistant,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                                &p,
                                                cx,
                                            ))
                                            .child(render_action_card(
                                                "welcome-card-quick-commands",
                                                AppIcon::QuickCommand,
                                                i18n!(cx, "welcome.quick_commands"),
                                                i18n!(cx, "welcome.quick_commands_desc"),
                                                rgb(0xf59e0b).into(),
                                                shortcut_quick_commands,
                                                move |_, window, cx| {
                                                    on_action_commands(
                                                        &WelcomeAction::QuickCommands,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                                &p,
                                                cx,
                                            )),
                                    ),
                            ),
                    )
                })
                // --- 4. Recent Sessions / Search Results Section ---
                .child(
                    v_flex()
                        .w_full()
                        .gap(SPACE_MD)
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .items_center()
                                .pb(px(2.0))
                                .child(
                                    div()
                                        .text_size(ui_text_md(cx))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(p.text_primary)
                                        .child(if is_searching {
                                            format!(
                                                "{}: \"{}\" ({})",
                                                i18n!(cx, "welcome.search_results"),
                                                search_query,
                                                display_sessions.len()
                                            )
                                        } else {
                                            i18n!(cx, "welcome.recent_sessions")
                                        }),
                                )
                                .child(
                                    h_flex()
                                        .id("welcome-import-sessions-link")
                                        .cursor_pointer()
                                        .items_center()
                                        .gap(SPACE_2XS)
                                        .text_size(ui_text_xs(cx))
                                        .text_color(p.text_muted)
                                        .hover(|s| s.text_color(p.surface_accent))
                                        .child(i18n!(cx, "import_session.title"))
                                        .child(
                                            AppIcon::ExternalLink
                                                .size(ICON_SM)
                                                .text_color(p.text_muted),
                                        )
                                        .on_click(move |_, window, cx| {
                                            on_action_import(
                                                &WelcomeAction::ImportSessions,
                                                window,
                                                cx,
                                            );
                                        }),
                                ),
                        )
                        .child(if display_sessions.is_empty() {
                            let (empty_title, empty_desc) = if is_searching {
                                (
                                    i18n!(cx, "welcome.no_match_sessions"),
                                    i18n!(cx, "welcome.no_match_sessions_desc"),
                                )
                            } else {
                                (
                                    i18n!(cx, "welcome.no_sessions"),
                                    i18n!(cx, "welcome.no_sessions_desc"),
                                )
                            };
                            v_flex()
                                .w_full()
                                .items_center()
                                .justify_center()
                                .py(SPACE_XL)
                                .gap(SPACE_XS)
                                .rounded(RADIUS_MD)
                                .bg(p.surface_card)
                                .border_1()
                                .border_color(p.border_subtle)
                                .child(AppIcon::Server.size(ICON_MD).text_color(p.text_muted))
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(p.text_muted)
                                        .child(empty_title),
                                )
                                .child(
                                    div()
                                        .text_size(ui_text_xs(cx))
                                        .text_color(p.text_muted)
                                        .child(empty_desc),
                                )
                                .into_any_element()
                        } else if is_searching {
                            // Single-column vertical list for search results with selection support
                            v_flex()
                                .w_full()
                                .gap(SPACE_SM)
                                .children(display_sessions.into_iter().enumerate().map(|(i, s)| {
                                    let is_selected = welcome_selected_index == Some(i);
                                    let on_action = on_action.clone();
                                    div().w_full().min_w_0().child(render_recent_session_card(
                                        s,
                                        is_selected,
                                        &p,
                                        cx,
                                        move |s, window, cx| {
                                            on_action(
                                                &WelcomeAction::ConnectSession(s),
                                                window,
                                                cx,
                                            );
                                        },
                                    ))
                                }))
                                .into_any_element()
                        } else {
                            // 2-column grid for standard recent sessions
                            let mut rows = Vec::new();
                            for (chunk_idx, chunk) in display_sessions.chunks(2).enumerate() {
                                let mut row_children = Vec::new();
                                for (i, s) in chunk.iter().enumerate() {
                                    let global_idx = chunk_idx * 2 + i;
                                    let is_selected = welcome_selected_index == Some(global_idx);
                                    let s_clone = s.clone();
                                    let on_action = on_action.clone();
                                    row_children.push(div().flex_1().w_0().min_w_0().child(
                                        render_recent_session_card(
                                            s_clone,
                                            is_selected,
                                            &p,
                                            cx,
                                            move |s, window, cx| {
                                                on_action(
                                                    &WelcomeAction::ConnectSession(s),
                                                    window,
                                                    cx,
                                                );
                                            },
                                        ),
                                    ));
                                }
                                if chunk.len() == 1 {
                                    row_children.push(div().flex_1().w_0().min_w_0());
                                }
                                rows.push(h_flex().w_full().gap(SPACE_MD).children(row_children));
                            }
                            v_flex()
                                .w_full()
                                .gap(SPACE_MD)
                                .children(rows)
                                .into_any_element()
                        }),
                ),
        )
}

#[allow(clippy::too_many_arguments)]
fn render_action_card(
    id: &'static str,
    icon: AppIcon,
    title: String,
    desc: String,
    accent_color: Hsla,
    shortcut: Option<&str>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    p: &SemanticPalette,
    cx: &App,
) -> impl IntoElement {
    let card_bg = p.surface_card;
    let hover_bg = p.surface_hover;
    let hover_border = p.surface_accent.opacity(0.8);

    h_flex()
        .id(id)
        .cursor_pointer()
        .flex_1()
        .w_0()
        .min_w_0()
        .items_center()
        .gap(SPACE_MD)
        .p(SPACE_LG)
        .rounded(RADIUS_MD)
        .bg(card_bg)
        .border_1()
        .border_color(p.border_subtle)
        .hover(move |s| s.bg(hover_bg).border_color(hover_border))
        .on_click(on_click)
        .child(
            div()
                .w(px(38.0))
                .h(px(38.0))
                .flex_shrink_0()
                .rounded(RADIUS_MD)
                .bg(accent_color.opacity(0.12))
                .border_1()
                .border_color(accent_color.opacity(0.25))
                .flex()
                .items_center()
                .justify_center()
                .child(icon.size(ICON_STD).text_color(accent_color)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text_primary)
                        .truncate()
                        .child(title),
                )
                .child(
                    div()
                        .text_size(ui_text_xs(cx))
                        .text_color(p.text_muted)
                        .truncate()
                        .child(desc),
                ),
        )
        .children(shortcut.map(|sc| {
            div()
                .flex_shrink_0()
                .px(SPACE_XS)
                .py(px(2.0))
                .rounded(RADIUS_XS)
                .bg(p.surface_raised)
                .border_1()
                .border_color(p.border_subtle)
                .text_size(ui_text_xs(cx))
                .text_color(p.text_muted)
                .font_weight(FontWeight::MEDIUM)
                .child(sc.to_string())
        }))
}

fn render_recent_session_card(
    session: SshSession,
    is_selected: bool,
    p: &SemanticPalette,
    cx: &App,
    on_click: impl Fn(SshSession, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let card_bg = if is_selected {
        p.surface_raised
    } else {
        p.surface_card
    };
    let border_color = if is_selected {
        p.surface_accent
    } else {
        p.border_subtle
    };
    let hover_bg = p.surface_hover;
    let hover_border = p.surface_accent.opacity(0.8);
    let id_str: SharedString = format!("recent-session-{}", session.id).into();

    let icon = match session.protocol {
        SessionProtocol::Ssh => AppIcon::Server,
        SessionProtocol::Telnet => AppIcon::Telnet,
        SessionProtocol::Serial => AppIcon::Serial,
        SessionProtocol::Local => AppIcon::Terminal,
    };

    let sub_info = match session.protocol {
        SessionProtocol::Ssh => format!("{}:{}", session.host, session.port),
        SessionProtocol::Telnet => session.telnet_host.clone().unwrap_or(session.host.clone()),
        SessionProtocol::Serial => session
            .serial_port
            .clone()
            .unwrap_or_else(|| "COM".to_string()),
        SessionProtocol::Local => "Local Terminal".to_string(),
    };

    let session_for_click = session.clone();
    let tooltip_text = format!("{}\n{}", session.name, sub_info);

    h_flex()
        .id(id_str)
        .cursor_pointer()
        .w_full()
        .min_w_0()
        .items_center()
        .justify_between()
        .px(SPACE_MD)
        .py(SPACE_SM)
        .rounded(RADIUS_MD)
        .bg(card_bg)
        .border_1()
        .border_color(border_color)
        .hover(move |s| s.bg(hover_bg).border_color(hover_border))
        .tooltip(move |_, cx| cx.new(|_| Tooltip::new(tooltip_text.clone())).into())
        .on_click(move |_, window, cx| {
            on_click(session_for_click.clone(), window, cx);
        })
        .child(
            h_flex()
                .items_center()
                .gap(SPACE_SM)
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .w(px(28.0))
                        .h(px(28.0))
                        .flex_shrink_0()
                        .rounded(RADIUS_STD)
                        .bg(p.surface_accent.opacity(0.1))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon.size(ICON_SM).text_color(p.surface_accent)),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .gap(px(1.0))
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_size(ui_text_sm(cx))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text_primary)
                                .truncate()
                                .child(session.name.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_size(ui_text_xs(cx))
                                .text_color(p.text_muted)
                                .truncate()
                                .child(sub_info),
                        ),
                ),
        )
        .child(
            AppIcon::ChevronRight
                .size(ICON_SM)
                .flex_shrink_0()
                .text_color(p.text_muted),
        )
}

/// Convert SshSession to ShellType for terminal execution
pub fn session_to_shell_type(session: &SshSession) -> ShellType {
    match session.protocol {
        SessionProtocol::Serial => {
            let port = session.serial_port.as_deref().unwrap_or("");
            let baud = session.serial_baud_rate.to_string();
            ShellType::Custom {
                path: "serial".to_string(),
                args: vec![
                    "--id".to_string(),
                    session.id.clone(),
                    "--port".to_string(),
                    port.to_string(),
                    "--baud".to_string(),
                    baud,
                ],
            }
        }
        SessionProtocol::Telnet => {
            let host = session
                .telnet_host
                .as_deref()
                .unwrap_or(session.host.as_str());
            let port = if session.telnet_port > 0 {
                session.telnet_port
            } else {
                23
            };
            ShellType::Custom {
                path: "telnet".to_string(),
                args: vec![
                    "--id".to_string(),
                    session.id.clone(),
                    "--host".to_string(),
                    host.to_string(),
                    "--port".to_string(),
                    port.to_string(),
                ],
            }
        }
        SessionProtocol::Local => {
            let mut args = vec!["--id".to_string(), session.id.clone()];
            if let Some(ref shell_str) = session.local_shell {
                args.push("--shell".to_string());
                args.push(shell_str.clone());
            }
            ShellType::Custom {
                path: "local".to_string(),
                args,
            }
        }
        SessionProtocol::Ssh => {
            let username = &session.username;
            let host = &session.host;
            let port = session.port;

            let mut args = Vec::new();
            if let SshAuthType::PrivateKey { key_path, .. } = &session.auth_type {
                if !key_path.is_empty() {
                    args.push("-i".to_string());
                    args.push(key_path.clone());
                }
            }
            args.push("-p".to_string());
            args.push(port.to_string());
            args.push("--id".to_string());
            args.push(session.id.clone());
            args.push(format!("{}@{}", username, host));

            ShellType::Custom {
                path: "ssh".to_string(),
                args,
            }
        }
    }
}
