//! Smoke tests — verify that the app initializes correctly after crate extraction.
//!
//! These tests catch integration issues like missing GPUI globals,
//! mismatched action types, or broken re-exports.

#[cfg(test)]
mod tests {
    use gpui::AppContext as _;

    /// Helper: register all GPUI globals that view crates depend on.
    fn init_globals(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            // Settings entity
            let settings_entity = cx.new(|_cx| {
                velowork_app::settings::SettingsState::new(Default::default())
            });
            cx.set_global(velowork_app::settings::GlobalSettings(settings_entity.clone()));

            // Theme — AppTheme is a GPUI Entity, not a Global
            let theme_entity = cx.new(|_cx| velowork_app::theme::AppTheme::new(
                velowork_app::theme::ColorTheme::Dark,
                velowork_app::theme::ColorTheme::Light,
                velowork_workspace::settings::ColorSchema::Dark,
                None,
                false,
            ));
            cx.set_global(velowork_app::theme::GlobalTheme(theme_entity.clone()));

            // Theme provider for view crates
            cx.set_global(velowork_ui::theme::GlobalThemeProvider(|cx| {
                velowork_app::theme::theme(cx)
            }));

            // UI font size provider for view crates
            cx.set_global(velowork_ui::tokens::GlobalUiFontSize(|cx| {
                velowork_app::settings::settings_entity(cx).read(cx).settings.ui_font_size
            }));

            // Extension settings store (used by terminal and git view crates)
            cx.set_global(velowork_extensions::ExtensionSettingsStore::new(
                |namespace, cx| {
                    let s = velowork_app::settings::settings_entity(cx).read(cx);
                    match namespace {
                        "terminal" => serde_json::to_value(&velowork_views_terminal::TerminalViewSettings {
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
                        }).ok(),
                        _ => s.settings.extension_settings.get(namespace).cloned(),
                    }
                },
                |_namespace, _value, _cx| {
                    // no-op for tests
                },
            ));

        // Unified state layer (mirrors main.rs setup)
        let session_store = cx.new(|_| velowork_workspace::stores::SessionStore::new());
        let connection_store = cx.new(|_| velowork_workspace::stores::ConnectionStore::new());
        let window_store = cx.new(|_| velowork_workspace::stores::WindowStore::new());
        let focus_store = cx.new(|_| velowork_workspace::stores::FocusStore::new());
        let service_store = cx.new(|_| velowork_workspace::stores::ServiceStore::new());
        cx.set_global(velowork_workspace::stores::GlobalSessionStore(session_store.clone()));
        cx.set_global(velowork_workspace::stores::GlobalConnectionStore(connection_store.clone()));
        cx.set_global(velowork_workspace::stores::GlobalWindowStore(window_store.clone()));
        cx.set_global(velowork_workspace::stores::GlobalFocusStore(focus_store.clone()));
        cx.set_global(velowork_workspace::stores::GlobalServiceStore(service_store.clone()));
        cx.set_global(velowork_app::app_state::AppState {
            session: session_store,
            connection: connection_store,
            settings: settings_entity.clone(),
            theme: theme_entity.clone(),
            window: window_store,
            focus: focus_store,
        });

        });
    }

    #[gpui::test]
    fn smoke_terminal_view_settings_readable(cx: &mut gpui::TestAppContext) {
        init_globals(cx);
        cx.update(|cx| {
            let settings = velowork_views_terminal::terminal_view_settings(cx);
            assert!(settings.font_size > 0.0);
            assert!(!settings.font_family.is_empty());
        });
    }

    #[gpui::test]
    fn smoke_theme_provider_returns_colors(cx: &mut gpui::TestAppContext) {
        init_globals(cx);
        cx.update(|cx| {
            let colors = velowork_ui::theme::theme(cx);
            // Just verify it doesn't panic and returns valid colors
            assert!(colors.bg_primary != 0 || colors.text_primary != 0);
        });
    }

    #[gpui::test]
    fn smoke_workspace_entity_creates(cx: &mut gpui::TestAppContext) {
        init_globals(cx);
        let _workspace = cx.new(|_cx| {
            velowork_workspace::state::Workspace::new(velowork_workspace::state::WorkspaceData {
                version: 1,
                projects: vec![],
                project_order: vec![],
                folders: vec![],
                service_panel_heights: Default::default(),
                main_window: Default::default(),
                extra_windows: Vec::new(),
            })
        });
    }

    #[gpui::test]
    fn smoke_keybinding_actions_are_crate_types(cx: &mut gpui::TestAppContext) {
        init_globals(cx);
        cx.update(|cx| {
            // Register keybindings (this exercises the action type mapping)
            velowork_app::keybindings::register_keybindings(cx);
        });
    }

    #[gpui::test]
    fn smoke_stores_register_and_events(cx: &mut gpui::TestAppContext) {
        init_globals(cx);
        cx.update(|cx| {
            let focus_store = cx
                .global::<velowork_workspace::stores::GlobalFocusStore>()
                .0
                .clone();
            let window_store = cx
                .global::<velowork_workspace::stores::GlobalWindowStore>()
                .0
                .clone();

            // FocusStore: register a per-window FocusManager and drive a layer change.
            let fm = cx.new(|_| velowork_workspace::focus::FocusManager::new());
            focus_store.update(cx, |s, cx| {
                s.register(velowork_workspace::state::WindowId::Main, fm.clone(), cx)
            });
            assert_eq!(
                focus_store
                    .read(cx)
                    .current_layer(velowork_workspace::state::WindowId::Main, cx),
                velowork_workspace::focus::FocusLayer::None
            );
            assert!(focus_store
                .read(cx)
                .manager(velowork_workspace::state::WindowId::Main)
                .is_some());
            focus_store.update(cx, |s, cx| {
                s.request_layer(
                    velowork_workspace::state::WindowId::Main,
                    velowork_workspace::focus::FocusLayer::Dialog,
                    cx,
                )
            });
            assert_eq!(
                focus_store
                    .read(cx)
                    .current_layer(velowork_workspace::state::WindowId::Main, cx),
                velowork_workspace::focus::FocusLayer::Dialog
            );

            // WindowStore: register a window, mutate bounds, then close it.
            let bounds = velowork_workspace::state::WindowBounds {
                origin_x: 0.0,
                origin_y: 0.0,
                width: 800.0,
                height: 600.0,
            };
            window_store.update(cx, |s, cx| {
                s.register(velowork_workspace::state::WindowId::Main, bounds, cx)
            });
            assert!(window_store
                .read(cx)
                .entry(velowork_workspace::state::WindowId::Main)
                .is_some());
            let new_bounds = velowork_workspace::state::WindowBounds {
                origin_x: 10.0,
                origin_y: 20.0,
                width: 100.0,
                height: 200.0,
            };
            window_store.update(cx, |s, cx| {
                s.set_bounds(velowork_workspace::state::WindowId::Main, new_bounds, cx)
            });
            assert_eq!(
                window_store
                    .read(cx)
                    .entry(velowork_workspace::state::WindowId::Main)
                    .unwrap()
                    .bounds,
                new_bounds
            );

            // ServiceStore: verify add_folder and upsert.
            let service_store = cx
                .global::<velowork_workspace::stores::GlobalServiceStore>()
                .0
                .clone();
            let folder_id = service_store.update(cx, |s, cx| s.add_folder(None, "TestFolder", cx));
            assert!(!folder_id.is_empty());
            assert!(service_store.read(cx).nodes().iter().any(|n| n.name() == "TestFolder"));

            window_store.update(cx, |s, cx| {
                s.close(velowork_workspace::state::WindowId::Main, cx)
            });
            assert!(window_store
                .read(cx)
                .entry(velowork_workspace::state::WindowId::Main)
                .is_none());
        });
    }
}

