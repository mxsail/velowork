//! App bootstrap — explicit, single-responsibility `init_*` functions.
//!
//! Each function owns one concern (settings, i18n, workspace, theme, stores,
//! extensions, pty). `main.rs` is the composition root that calls them in
//! order; the few handles needed to build the main window are returned so the
//! caller threads them into the `open_window` closure.
//!
//! Everything here runs inside the `Application::run` closure with `&mut App`,
//! before any window exists.

use std::sync::Arc;

use async_channel::Receiver;
use gpui::*;

use velowork_app_core::settings::SettingsState;
use velowork_state::WorkspaceData;
use velowork_workspace::settings::AppSettings;

use crate::app_state::AppState;
use crate::settings;
use crate::theme;
use crate::theme::{AppTheme, GlobalTheme};
use crate::views::panels::toast::{Toast, ToastManager};
use crate::workspace::persistence;
use velowork_terminal::pty_manager::{PtyEvent, PtyManager};
use velowork_terminal::{ConnectionManager, GlobalTunnelEngine, TunnelEngine};
use velowork_workspace::stores::{
    ConnectionStore, FocusStore, GlobalConnectionStore, GlobalFocusStore, GlobalServiceStore,
    GlobalSessionStore, GlobalTunnelStore, GlobalWindowStore, ServiceStore, SessionStore,
    TunnelStore, WindowStore,
};

/// Initialize global settings and return the entity plus a cloned copy of the
/// current `AppSettings` for downstream bootstrap steps.
pub fn init_settings(cx: &mut App) -> (Entity<SettingsState>, AppSettings) {
    let settings_entity = settings::init_settings(cx);
    let app_settings = settings_entity.read(cx).get().clone();
    // Default window-corner radius for modal backdrops (overwritten per-frame by
    // the root window view once it knows the active rounded-corner state).
    cx.set_global(velowork_ui::WindowCornerRadius(0.0));
    (settings_entity, app_settings)
}

/// Apply the locale from settings to the i18n layer.
pub fn init_i18n(app_settings: &AppSettings, cx: &mut App) {
    velowork_i18n::init_locale(app_settings.locale.clone(), cx);
}

/// Load (or create) the workspace, clearing per-project layouts when
/// "restore terminals on startup" is disabled.
pub fn init_workspace(app_settings: &AppSettings, cx: &mut App) -> WorkspaceData {
    let mut workspace_data = persistence::load_workspace(app_settings.session_backend).unwrap_or_else(|e| {
        log::error!(
            "[app:init] Failed to load workspace | error: {:#} | backup={:?} | Using default workspace",
            e,
            persistence::get_workspace_path().with_extension("json.bak")
        );
        let backup_path = persistence::get_workspace_path().with_extension("json.bak");
        ToastManager::post(
            Toast::error(format!(
                "Workspace file was corrupted. A backup was saved to {}. \
                 Starting with default workspace. Auto-save is disabled to protect your data — \
                 restart the app after fixing the file.",
                backup_path.display()
            ))
            .with_ttl(std::time::Duration::from_secs(30)),
            cx,
        );
        persistence::default_workspace()
    });

    // When "restore terminals on startup" is disabled, clear each project's
    // layout so it shows the empty state instead of restoring terminals.
    if !app_settings.restore_terminals_on_startup {
        for project in &mut workspace_data.projects {
            project.layout = None;
        }
    }

    workspace_data
}

/// Create the theme entity from settings and register it as the global theme.
/// Restores the active custom theme's colors when applicable.
pub fn init_theme(app_settings: &AppSettings, cx: &mut App) -> Entity<AppTheme> {
    let theme_entity = cx.new(|_cx| {
        let mut theme = AppTheme::new(
            app_settings.dark_color_theme,
            app_settings.light_color_theme,
            app_settings.color_schema,
            app_settings.custom_theme_id.as_deref(),
            true,
        );
        theme.set_opacity(app_settings.bg_opacity);
        theme
    });
    cx.set_global(GlobalTheme(theme_entity.clone()));
    cx.set_global(velowork_theme::GlobalThemeProvider(|cx| {
        theme::theme(cx)
    }));

    // Keep the global theme in sync with the settings value at runtime. This
    // drives both the background opacity and — more importantly — the active
    // appearance (`color_schema`) plus the dedicated dark/light palettes. When
    // `color_schema` is `System`, `set_system_appearance` (wired in `main.rs`
    // via `observe_window_appearance`) decides which palette is shown; for an
    // explicit Dark/Light choice the selected palette is applied directly.
    let theme_to_sync = theme_entity.clone();
    cx.observe(&settings::settings_entity(cx), move |_, cx| {
        let settings = settings::settings_entity(cx).read(cx).settings.clone();
        theme_to_sync.update(cx, |theme, cx| {
            theme.sync_from_settings(
                settings.dark_color_theme,
                settings.light_color_theme,
                settings.color_schema,
                settings.custom_theme_id.as_deref(),
            );
            theme.set_opacity(settings.bg_opacity.clamp(0.1, 1.0));
            // Notify so the whole UI re-renders with the new palette/appearance.
            // Without this, changing `color_schema` (or a dark/light palette) only
            // mutates the theme entity in memory but never propagates to views.
            cx.notify();
        });

        // Keep terminal background image cache in sync when settings change
        if let Some(cache) = velowork_views_terminal::terminal_background_cache(cx) {
            let tvs = velowork_views_terminal::terminal_view_settings(cx);
            let path = tvs.terminal_background_image.clone();
            let blur = tvs.terminal_background_image_blur;
            cache.update(cx, |cache, cx| {
                cache.ensure_loaded(path, blur, cx);
            });
        }
    })
    .detach();

    theme_entity
}

/// Register the extension system: registry, extension theme provider,
/// and the extension settings-store bridge.
pub fn init_extensions(cx: &mut App) {
    // Initialize extension registry
    let ext_registry = velowork_extensions::ExtensionRegistry::new();
    cx.set_global(ext_registry);

    // Register theme provider for extensions
    cx.set_global(velowork_extensions::GlobalThemeProvider(|cx| {
        theme::theme(cx)
    }));

    // Register extension settings store (bridge for extensions and view crates to read/write settings).
    // Known namespaces ("terminal", "git") map to/from individual AppSettings fields.
    // Unknown namespaces fall back to the generic extension_settings map.
    cx.set_global(velowork_extensions::ExtensionSettingsStore::new(
        |namespace, cx| {
            let s = settings::settings_entity(cx).read(cx);
            match namespace {
                "terminal" => {
                    serde_json::to_value(&velowork_views_terminal::TerminalViewSettings {
                        font_size: s.settings.font_size,
                        line_height: s.settings.line_height,
                        font_family: s.settings.font_family.clone(),
                        font_weight: s.settings.font_weight.clone(),
                        font_style: s.settings.font_style.clone(),
                        cursor_style: s.settings.cursor_style,
                        cursor_blink: s.settings.cursor_blink,
                        bell_style: s.settings.bell_style,
                        bell_cooldown_ms: s.settings.bell_cooldown_ms,
                        show_focused_border: s.settings.show_focused_border,
                        show_shell_selector: s.settings.show_shell_selector,
                        idle_timeout_secs: s.settings.idle_timeout_secs,
                        color_tinted_background: s.settings.color_tinted_background,
                        file_opener: s.settings.file_opener.clone(),
                        default_shell: s.settings.default_shell.clone(),
                        ctrl_c_copies_selection: s.settings.terminal_ctrl_c_copies_selection,
                        show_line_numbers: s.settings.show_line_numbers,
                        restore_terminals_on_startup: s.settings.restore_terminals_on_startup,
                        terminal_background_image: s.settings.terminal_background_image.clone(),
                        terminal_background_image_blur: s.settings.terminal_background_image_blur,
                        terminal_scrollbar_show: s.settings.terminal_scrollbar_show,
                        color_scheme: s.settings.color_scheme.clone(),
                        custom_terminal_color_schemes: s.settings.custom_terminal_color_schemes.clone(),
                        charset: s.settings.charset.clone(),
                        term_type: s.settings.term_type.clone(),
                        wrap_mode: s.settings.wrap_mode,
                        scrollback_lines: s.settings.scrollback_lines,
                        word_selection_delimiters: s.settings.word_selection_delimiters.clone(),
                        terminal_copy_on_select: s.settings.terminal_copy_on_select,
                        terminal_right_click_paste: s.settings.terminal_right_click_paste,
                        command_history_max_count: s.settings.command_history_max_count,
                        command_history_retention_days: s.settings.command_history_retention_days,
                        command_history_auto_completion: s.settings.command_history_auto_completion,
                        command_history_ignored_commands: s.settings.command_history_ignored_commands.clone(),
                        command_history_ignore_space: s.settings.command_history_ignore_space,
                        shell_integration: s.settings.shell_integration,
                        bracketed_paste: s.settings.bracketed_paste,
                        osc52_clipboard: s.settings.osc52_clipboard,
                        true_color: s.settings.true_color,
                        ai_enabled: s.settings.ai_enabled,
                        terminal_ai_floating_toolbar_enabled: s.settings.terminal_ai_floating_toolbar_enabled,
                    })
                    .ok()
                }
                _ => s.settings.extension_settings.get(namespace).cloned(),
            }
        },
        |namespace, value, cx| match namespace {
            "terminal" => {
                if let Ok(tvs) =
                    serde_json::from_value::<velowork_views_terminal::TerminalViewSettings>(value)
                {
                    settings::settings_entity(cx)
                        .update(cx, |state, cx| {
                            state.settings.font_size = tvs.font_size;
                            state.settings.line_height = tvs.line_height;
                            state.settings.font_family = tvs.font_family;
                            state.settings.font_weight = tvs.font_weight;
                            state.settings.font_style = tvs.font_style;
                            state.settings.cursor_style = tvs.cursor_style;
                            state.settings.cursor_blink = tvs.cursor_blink;
                            state.settings.bell_style = tvs.bell_style;
                            state.settings.bell_cooldown_ms = tvs.bell_cooldown_ms;
                            state.settings.show_focused_border = tvs.show_focused_border;
                            state.settings.show_shell_selector = tvs.show_shell_selector;
                            state.settings.idle_timeout_secs = tvs.idle_timeout_secs;
                            state.settings.color_tinted_background = tvs.color_tinted_background;
                            state.settings.file_opener = tvs.file_opener;
                            state.settings.default_shell = tvs.default_shell;
                            state.settings.terminal_ctrl_c_copies_selection =
                                tvs.ctrl_c_copies_selection;
                            state.settings.show_line_numbers = tvs.show_line_numbers;
                            state.settings.restore_terminals_on_startup =
                                tvs.restore_terminals_on_startup;
                            state.settings.terminal_background_image =
                                tvs.terminal_background_image.clone();
                            state.settings.terminal_background_image_blur =
                                tvs.terminal_background_image_blur;
                            state.settings.terminal_scrollbar_show = tvs.terminal_scrollbar_show;
                            state.settings.color_scheme = tvs.color_scheme;
                            state.settings.custom_terminal_color_schemes = tvs.custom_terminal_color_schemes;
                            state.settings.charset = tvs.charset;
                            state.settings.term_type = tvs.term_type.clone();
                            if cx.has_global::<crate::GlobalPtyManager>() {
                                cx.global::<crate::GlobalPtyManager>().0.set_default_term_type(tvs.term_type);
                            }
                            state.settings.wrap_mode = tvs.wrap_mode;
                            state.settings.scrollback_lines = tvs.scrollback_lines;
                            state.settings.word_selection_delimiters = tvs.word_selection_delimiters;
                            state.settings.terminal_copy_on_select = tvs.terminal_copy_on_select;
                            state.settings.terminal_right_click_paste = tvs.terminal_right_click_paste;
                            state.settings.command_history_max_count = tvs.command_history_max_count;
                            state.settings.command_history_retention_days = tvs.command_history_retention_days;
                            state.settings.command_history_auto_completion = tvs.command_history_auto_completion;
                            state.settings.command_history_ignored_commands = tvs.command_history_ignored_commands;
                            state.settings.command_history_ignore_space = tvs.command_history_ignore_space;
                            state.settings.terminal_ai_floating_toolbar_enabled = tvs.terminal_ai_floating_toolbar_enabled;
                            state.save_and_notify(cx);
                        });
                }
            }
            _ => {
                settings::settings_entity(cx)
                    .update(cx, |state, cx| {
                        state.set_extension_setting(namespace, value, cx);
                    });
            }
        },
    ));
}

/// Initialize updater: sets GlobalUpdateInfo, cleans old binary, and starts background checker if enabled.
pub fn init_updater(app_settings: &AppSettings, cx: &mut App) {
    velowork_updater::init(env!("CARGO_PKG_VERSION"), app_settings.auto_check_updates, cx);
}

/// Build the four domain stores, aggregate them into the unified `AppState`,
/// register the per-store globals, the project-hover global, and the
/// cross-crate theme / UI-font / UI-scale providers.
pub fn init_stores(
    cx: &mut App,
    settings_entity: &Entity<SettingsState>,
    theme_entity: &Entity<AppTheme>,
) {
    // Shared, singleton terminal background-image cache (decodes once, shared
    // by every terminal tab). Preloads the configured image at startup. This
    // must run after `init_settings` (which sets the `GlobalSettings` global)
    // because the preload reads terminal-view settings via `settings_entity`.
    velowork_views_terminal::init_terminal_background_cache(cx);

    // ---- Unified state layer: AppState aggregates the six stores ----
    // Each store is the sole owner + sole writer of its domain; UI reads
    // through store queries and pushes changes through store mutations.
    let session_store = cx.new(|_| SessionStore::new());
    let connection_store = cx.new(|_| ConnectionStore::new());
    let window_store = cx.new(|_| WindowStore::new());
    let focus_store = cx.new(|_| FocusStore::new());

    // Per-store globals so low-level crates (e.g. velowork-views-terminal)
    // can reach a store without depending on velowork-app.
    cx.set_global(GlobalSessionStore(session_store.clone()));
    cx.set_global(GlobalConnectionStore(connection_store.clone()));
    cx.set_global(GlobalWindowStore(window_store.clone()));
    cx.set_global(GlobalFocusStore(focus_store.clone()));

    // Tunnel store (tree of folders + tunnels) and the shared tunnel engine.
    let tunnel_store = cx.new(|_| TunnelStore::new());
    cx.set_global(GlobalTunnelStore(tunnel_store.clone()));
    let tunnel_conn_mgr = Arc::new(ConnectionManager::new(std::time::Duration::from_secs(300)));
    cx.set_global(GlobalTunnelEngine(Arc::new(TunnelEngine::new(
        tunnel_conn_mgr,
    ))));

    // Service store (tree of folders + services) and background service monitor engine.
    let service_store = cx.new(|_| ServiceStore::new());
    cx.set_global(GlobalServiceStore(service_store.clone()));
    let service_monitor_engine = cx.new(|_| velowork_terminal::ServiceMonitorEngine::new());
    cx.set_global(velowork_terminal::GlobalServiceMonitorEngine(service_monitor_engine));

    // The single composition root.
    cx.set_global(AppState {
        session: session_store,
        connection: connection_store,
        settings: settings_entity.clone(),
        theme: theme_entity.clone(),
        window: window_store,
        focus: focus_store,
    });

    // Register theme provider for velowork-theme / velowork-ui crate
    cx.set_global(velowork_theme::GlobalThemeProvider(|cx| {
        theme::theme(cx)
    }));

    // Register UI font size provider for all crates (fine text-size trim)
    cx.set_global(velowork_ui::tokens::GlobalUiFontSize(|cx| {
        settings::settings_entity(cx).read(cx).settings.ui_font_size
    }));

    // Register global UI zoom provider (percent, 80..=200)
    cx.set_global(velowork_ui::tokens::GlobalUiScale(|cx| {
        settings::settings_entity(cx).read(cx).settings.ui_scale
    }));

    // Register UI density provider for all crates (Compact / Default / Comfortable)
    cx.set_global(velowork_ui::tokens::GlobalUiDensity(|cx| {
        settings::settings_entity(cx).read(cx).settings.ui_density
    }));

    // Register UI font family provider for all crates.
    cx.set_global(velowork_ui::tokens::GlobalFontFamily(|cx| {
        settings::settings_entity(cx)
            .read(cx)
            .settings
            .ui_font_family
            .clone()
    }));

    // Register monospace font family provider for code / logs / file viewer (unified with terminal & code font).
    cx.set_global(velowork_ui::tokens::GlobalMonoFontFamily(|cx| {
        let s = &settings::settings_entity(cx).read(cx).settings;
        if !s.font_family.is_empty() {
            s.font_family.clone()
        } else if !s.mono_font_family.is_empty() {
            s.mono_font_family.clone()
        } else {
            String::new()
        }
    }));

    // Register Markdown reading font family provider for Markdown / AI assistant text (unified with UI font).
    cx.set_global(velowork_ui::tokens::GlobalMarkdownFontFamily(|cx| {
        let s = &settings::settings_entity(cx).read(cx).settings;
        if !s.ui_font_family.is_empty() {
            s.ui_font_family.clone()
        } else if !s.markdown_font_family.is_empty() {
            s.markdown_font_family.clone()
        } else {
            String::new()
        }
    }));

    // Register window decoration config provider for all crates (titlebar preset, position, gap, margin)
    cx.set_global(velowork_ui::decorations::GlobalWindowDecorationConfig(|cx| {
        let s = &settings::settings_entity(cx).read(cx).settings;
        let style_override = match s.titlebar_preset {
            velowork_workspace::settings::CustomTitlebarPreset::Auto => None,
            velowork_workspace::settings::CustomTitlebarPreset::MacOS => Some(velowork_ui::decorations::WindowControlStyle::MacOS),
            velowork_workspace::settings::CustomTitlebarPreset::Windows11 => Some(velowork_ui::decorations::WindowControlStyle::Windows11),
            velowork_workspace::settings::CustomTitlebarPreset::LinuxCSD => Some(velowork_ui::decorations::WindowControlStyle::LinuxCSD),
            velowork_workspace::settings::CustomTitlebarPreset::KDEBreeze => Some(velowork_ui::decorations::WindowControlStyle::KDEBreeze),
        };
        let pos_override = match s.titlebar_position {
            velowork_workspace::settings::CustomTitlebarPosition::Auto => None,
            velowork_workspace::settings::CustomTitlebarPosition::Left => Some(velowork_ui::decorations::WindowButtonPosition::Left),
            velowork_workspace::settings::CustomTitlebarPosition::Right => Some(velowork_ui::decorations::WindowButtonPosition::Right),
        };
        velowork_ui::decorations::WindowDecorationConfig::from_custom(
            style_override,
            pos_override,
            Some(s.window_control_button_gap),
            Some(s.window_control_margin),
        )
    }));

    // Register custom titlebar check provider for all crates
    cx.set_global(velowork_ui::decorations::GlobalIsCustomTitlebar(|window, cx| {
        let is_custom_setting = settings::settings_entity(cx).read(cx).settings.titlebar_style
            == velowork_workspace::settings::TitlebarStyle::Custom;
        if !is_custom_setting {
            return false;
        }
        if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            true
        } else {
            matches!(window.window_decorations(), gpui::Decorations::Client { .. })
        }
    }));

    // Register titlebar height provider
    cx.set_global(velowork_ui::decorations::GlobalTitlebarHeight(|cx| {
        settings::settings_entity(cx).read(cx).settings.titlebar_height
    }));

    // Register window control icon size provider
    cx.set_global(velowork_ui::decorations::GlobalWindowControlIconSize(|cx| {
        settings::settings_entity(cx).read(cx).settings.window_control_icon_size
    }));

    // Register window control button gap provider
    cx.set_global(velowork_ui::decorations::GlobalWindowControlButtonGap(|cx| {
        settings::settings_entity(cx).read(cx).settings.window_control_button_gap
    }));

    // Register window control margin provider
    cx.set_global(velowork_ui::decorations::GlobalWindowControlMargin(|cx| {
        settings::settings_entity(cx).read(cx).settings.window_control_margin
    }));

    // Register tab width mode provider for all crates
    cx.set_global(velowork_ui::decorations::GlobalTabWidthMode(|cx| {
        match settings::settings_entity(cx).read(cx).settings.tab_width_mode {
            velowork_workspace::settings::TabWidthMode::Compact => velowork_ui::decorations::TabWidthMode::Compact,
            velowork_workspace::settings::TabWidthMode::TitleLength => velowork_ui::decorations::TabWidthMode::TitleLength,
            velowork_workspace::settings::TabWidthMode::Equal => velowork_ui::decorations::TabWidthMode::Equal,
        }
    }));

    // Register window corner radius provider for all crates
    cx.set_global(velowork_ui::decorations::GlobalWindowCornerRadius(|cx| {
        settings::settings_entity(cx).read(cx).settings.window_corner_radius
    }));

    // Follow the OS UI font size where detectable (best-effort).
    velowork_ui::tokens::set_system_font_scale(detect_system_font_scale());

    // Shared, cross-window hover state for the Switch Project overlay.
    // Hovering a project row publishes its id here; every window observes it
    // to ring-highlight the matching project panel (incl. other windows).
    let project_hover = cx.new(|_| crate::views::overlays::project_hover::ProjectHoverState::new());
    cx.set_global(crate::views::overlays::project_hover::GlobalProjectHover(
        project_hover,
    ));
}

/// Create the PTY manager from the configured session backend.
pub fn init_pty(app_settings: &AppSettings) -> (Arc<PtyManager>, Receiver<PtyEvent>) {
    let (pty_manager, pty_events) = PtyManager::new(app_settings.session_backend);
    pty_manager.set_default_term_type(app_settings.term_type.clone());
    let mode_str = match app_settings.proxy_mode {
        velowork_workspace::settings::ProxyMode::None => "none",
        velowork_workspace::settings::ProxyMode::System => "system",
        velowork_workspace::settings::ProxyMode::Http => "http",
    };
    pty_manager.set_global_proxy(velowork_terminal::GlobalProxySettings {
        mode: mode_str.to_string(),
        host: app_settings.proxy_host.clone(),
        port: app_settings.proxy_port,
    });
    let pty_manager = Arc::new(pty_manager);
    (pty_manager, pty_events)
}

/// Best-effort OS UI font-size detection, returned as a scale factor relative to
/// the app's 13px design baseline.
fn detect_system_font_scale() -> f32 {
    let sys_font = velowork_ui::typography::detect_system_ui_font();
    if let Some(px_size) = sys_font.size {
        (px_size / 13.0).clamp(0.75, 2.0)
    } else {
        1.0
    }
}
