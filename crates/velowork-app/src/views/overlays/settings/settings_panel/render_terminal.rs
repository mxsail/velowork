use crate::settings::settings_entity;
use crate::theme::{theme, ThemeColors};
use crate::ui::tokens::{
    ICON_STD, SELECT_WIDTH_MD, SPACE_2XS,
    SPACE_SM, SPACE_XL, SPACE_XS, ui_text_xs,
};
use crate::workspace::settings::CursorShape;
use gpui::prelude::FluentBuilder;
use gpui::*;
use std::path::Path;
use velowork_i18n::i18n;
use velowork_ui::button::Button;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::scrollable::ScrollbarShow;
use velowork_ui::select::Select;
use velowork_ui::{h_flex, v_flex};
use velowork_ui::ControlSize;

use super::components::*;
use super::SettingsPanel;

impl SettingsPanel {
    pub(super) fn render_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let s = settings_entity(cx).read(cx).settings.clone();

        let local_section_title = i18n!(cx, "settings.terminal_section_local");
        let display_section_title = i18n!(cx, "settings.terminal_display_section");
        let behavior_section_title = i18n!(cx, "settings.terminal_section_behavior");

        let show_shell_selector_label = i18n!(cx, "settings.show_shell_selector");
        let show_shell_selector_desc = i18n!(cx, "settings.show_shell_selector_desc");
        let cursor_blink_label = i18n!(cx, "settings.cursor_blink");
        let scrollback_lines_label = i18n!(cx, "settings.scrollback_lines");
        let scrollback_lines_desc = i18n!(cx, "settings.scrollback_lines_desc");
        let ctrl_c_copies_label = i18n!(cx, "settings.ctrl_c_copies");
        let copy_on_select_label = i18n!(cx, "settings.copy_on_select");
        let right_click_paste_label = i18n!(cx, "settings.right_click_paste");
        let right_click_paste_desc = i18n!(cx, "settings.right_click_paste_desc");
        let show_line_numbers_label = i18n!(cx, "settings.show_line_numbers");
        let restore_terminals_label = i18n!(cx, "settings.restore_terminals");
        let idle_detection_label = i18n!(cx, "settings.idle_detection");
        let undo_close_label = i18n!(cx, "settings.undo_close");
        let idle_timeout_label = i18n!(cx, "settings.idle_timeout");
        let undo_window_label = i18n!(cx, "settings.undo_window");
        let terminal_bg_image_label = i18n!(cx, "settings.terminal_background_image");
        let terminal_bg_image_blur_label = i18n!(cx, "settings.terminal_background_image_blur");
        let terminal_bg_image_blur_desc = i18n!(cx, "settings.terminal_background_image_blur_desc");

        let has_bg_image = s
            .terminal_background_image
            .as_deref()
            .map(|p| !p.trim().is_empty())
            .unwrap_or(false);
        let has_valid_bg_image = if let Some(cache) = velowork_views_terminal::terminal_background_cache(cx) {
            let c = cache.read(cx);
            has_bg_image && c.error().is_none()
        } else {
            has_bg_image
        };

        div()
            .flex()
            .flex_col()
            .gap(SPACE_XL)
            // 1. 本地环境与会话配置 (仅适用于本地 Shell 与 PTY 会话)
            .child(
                div()
                    .child(section_header(&local_section_title, &t, cx))
                    .child(
                        section_container(&t)
                            .child(self.render_shell_dropdown_row(&s.default_shell, cx))
                            .child(self.render_toggle_with_desc(
                                "show-shell-selector",
                                &show_shell_selector_label,
                                &show_shell_selector_desc,
                                s.show_shell_selector,
                                true,
                                |state, val, cx| state.set_show_shell_selector(val, cx),
                                cx,
                            ))
                            .child(self.render_session_backend_dropdown_row(&s.session_backend, cx))
                            .child(self.render_term_type_row(cx))
                            .child(self.render_charset_row(cx))
                            .child(self.render_toggle(
                                "restore-terminals",
                                &restore_terminals_label,
                                s.restore_terminals_on_startup,
                                false,
                                |state, val, cx| state.set_restore_terminals_on_startup(val, cx),
                                cx,
                            )),
                    ),
            )
            // 2. 终端显示与光标外观
            .child(
                div()
                    .child(section_header(&display_section_title, &t, cx))
                    .child(
                        section_container(&t)
                            .child(self.render_color_scheme_row(&s.color_scheme, cx))
                            .child(self.render_cursor_style_row(s.cursor_style, cx))
                            .child(self.render_toggle(
                                "cursor-blink",
                                &cursor_blink_label,
                                s.cursor_blink,
                                true,
                                |state, val, cx| state.set_cursor_blink(val, cx),
                                cx,
                            ))
                            .child(self.render_terminal_scrollbar_show_row(s.terminal_scrollbar_show, cx))
                            .child(self.render_integer_stepper_with_desc(
                                "scrollback",
                                scrollback_lines_label,
                                scrollback_lines_desc,
                                s.scrollback_lines,
                                100,
                                100000,
                                500,
                                70.0,
                                true,
                                |state, val, cx| state.set_scrollback_lines(val, cx),
                                window,
                                cx,
                            ))
                            .child(self.render_toggle(
                                "show-line-numbers",
                                &show_line_numbers_label,
                                s.show_line_numbers,
                                true,
                                |state, val, cx| state.set_show_line_numbers(val, cx),
                                cx,
                            ))
                            .child(self.render_terminal_bg_image_row(&terminal_bg_image_label, &t, cx))
                            .when(has_valid_bg_image, |d| {
                                d.child(self.render_toggle_with_desc(
                                    "terminal-bg-blur",
                                    &terminal_bg_image_blur_label,
                                    &terminal_bg_image_blur_desc,
                                    s.terminal_background_image_blur,
                                    false,
                                    |state, val, cx| state.set_terminal_background_image_blur(val, cx),
                                    cx,
                                ))
                            }),
                    ),
            )
            // 3. 终端交互与安全防护
            .child(
                div()
                    .child(section_header(&behavior_section_title, &t, cx))
                    .child(
                        section_container(&t)
                            .child(self.render_toggle(
                                "ctrl-c-copies",
                                &ctrl_c_copies_label,
                                s.terminal_ctrl_c_copies_selection,
                                true,
                                |state, val, cx| state.set_terminal_ctrl_c_copies_selection(val, cx),
                                cx,
                            ))
                            .child(self.render_toggle(
                                "copy-on-select",
                                &copy_on_select_label,
                                s.terminal_copy_on_select,
                                true,
                                |state, val, cx| state.set_terminal_copy_on_select(val, cx),
                                cx,
                            ))
                            .child(self.render_toggle_with_desc(
                                "right-click-paste",
                                &right_click_paste_label,
                                &right_click_paste_desc,
                                s.terminal_right_click_paste,
                                true,
                                |state, val, cx| state.set_terminal_right_click_paste(val, cx),
                                cx,
                            ))
                            .child(self.render_word_selection_delimiters_row(&t, cx))
                            .child(self.render_bell_style_row(s.bell_style, cx))
                            .child(self.render_integer_stepper_with_desc(
                                "bell-cooldown",
                                i18n!(cx, "settings.terminal.bell_cooldown"),
                                i18n!(cx, "settings.terminal.bell_cooldown_desc"),
                                s.bell_cooldown_ms,
                                0,
                                5000,
                                100,
                                70.0,
                                true,
                                |state, val, cx| state.set_bell_cooldown_ms(val, cx),
                                window,
                                cx,
                            ))
                            .child(self.render_toggle_with_desc(
                                "shell-integration",
                                &i18n!(cx, "settings.shell_integration"),
                                &i18n!(cx, "settings.shell_integration_desc"),
                                s.shell_integration,
                                true,
                                |state, val, cx| state.set_shell_integration(val, cx),
                                cx,
                            ))
                            .child(self.render_toggle_with_desc(
                                "bracketed-paste",
                                &i18n!(cx, "settings.bracketed_paste"),
                                &i18n!(cx, "settings.bracketed_paste_desc"),
                                s.bracketed_paste,
                                true,
                                |state, val, cx| state.set_bracketed_paste(val, cx),
                                cx,
                            ))
                            .child(self.render_toggle_with_desc(
                                "osc52-clipboard",
                                &i18n!(cx, "settings.osc52_clipboard"),
                                &i18n!(cx, "settings.osc52_clipboard_desc"),
                                s.osc52_clipboard,
                                true,
                                |state, val, cx| state.set_osc52_clipboard(val, cx),
                                cx,
                            ))
                            .child(self.render_toggle_with_desc(
                                "true-color",
                                &i18n!(cx, "settings.true_color"),
                                &i18n!(cx, "settings.true_color_desc"),
                                s.true_color,
                                true,
                                |state, val, cx| state.set_true_color(val, cx),
                                cx,
                            ))
                            .child(self.render_toggle(
                                "idle-detection",
                                &idle_detection_label,
                                s.idle_timeout_secs > 0,
                                true,
                                |state, val, cx| state.set_idle_timeout_secs(if val { 5 } else { 0 }, cx),
                                cx,
                            ))
                            .when(s.idle_timeout_secs > 0, |el| {
                                el.child(self.render_integer_stepper(
                                    "idle-timeout",
                                    &idle_timeout_label,
                                    s.idle_timeout_secs,
                                    1,
                                    3600,
                                    1,
                                    50.0,
                                    true,
                                    |state, val, cx| state.set_idle_timeout_secs(val, cx),
                                    window,
                                    cx,
                                ))
                            })
                            .child(self.render_toggle(
                                "close-grace",
                                &undo_close_label,
                                s.terminal_close_grace_secs > 0,
                                s.terminal_close_grace_secs > 0,
                                |state, val, cx| state.set_terminal_close_grace_secs(if val { 5 } else { 0 }, cx),
                                cx,
                            ))
                            .when(s.terminal_close_grace_secs > 0, |el| {
                                el.child(self.render_integer_stepper(
                                    "close-grace-secs",
                                    &undo_window_label,
                                    s.terminal_close_grace_secs,
                                    1,
                                    60,
                                    1,
                                    50.0,
                                    false,
                                    |state, val, cx| state.set_terminal_close_grace_secs(val, cx),
                                    window,
                                    cx,
                                ))
                            }),
                    ),
            )
            // 4. 历史命令与智能补全 (独立历史记录数据库与建议)
            .child(
                div()
                    .child(section_header(&i18n!(cx, "command_history.title"), &t, cx))
                    .child(
                        section_container(&t)
                            .child(self.render_toggle_with_desc(
                                "history-auto-completion",
                                &i18n!(cx, "command_history.setting_auto_completion"),
                                &i18n!(cx, "command_history.setting_auto_completion_desc"),
                                s.command_history_auto_completion,
                                true,
                                |state, val, cx| state.set_command_history_auto_completion(val, cx),
                                cx,
                            ))
                            .child(self.render_toggle_with_desc(
                                "history-ignore-space",
                                &i18n!(cx, "command_history.setting_ignore_space"),
                                &i18n!(cx, "command_history.setting_ignore_space_desc"),
                                s.command_history_ignore_space,
                                true,
                                |state, val, cx| state.set_command_history_ignore_space(val, cx),
                                cx,
                            ))
                            .child(self.render_command_history_ignored_commands_row(&t, cx))
                            .child(self.render_integer_stepper_with_desc(
                                "history-max-count",
                                i18n!(cx, "command_history.setting_max_count"),
                                i18n!(cx, "command_history.setting_max_count_desc"),
                                s.command_history_max_count as u32,
                                10,
                                50000,
                                100,
                                70.0,
                                true,
                                |state, val, cx| state.set_command_history_max_count(val as usize, cx),
                                window,
                                cx,
                            ))
                            .child(self.render_integer_stepper_with_desc(
                                "history-retention-days",
                                i18n!(cx, "command_history.setting_retention_days"),
                                i18n!(cx, "command_history.setting_retention_days_desc"),
                                s.command_history_retention_days,
                                0,
                                3650,
                                1,
                                70.0,
                                false,
                                |state, val, cx| state.set_command_history_retention_days(val, cx),
                                window,
                                cx,
                            )),
                    ),
            )
    }

    fn render_terminal_scrollbar_show_row(&mut self, current: ScrollbarShow, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.terminal_scrollbar_show");
        let focus_handle = self.get_or_create_radio_focus_handle("terminal-scrollbar-show", cx);

        settings_row("terminal-scrollbar-show".to_string(), &label, &t, cx, true).child(
            RadioGroup::new("terminal-scrollbar-show-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(
                    ScrollbarShow::all_variants()
                        .iter()
                        .map(|&mode| RadioOption::new(mode, i18n!(cx, mode.translation_key())))
                        .collect(),
                )
                .on_change(move |&mode, _, cx| {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_terminal_scrollbar_show(mode, cx);
                    });
                }),
        )
    }

    fn render_cursor_style_row(&mut self, current: CursorShape, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let cursor_style_label = i18n!(cx, "settings.cursor_style");
        let focus_handle = self.get_or_create_radio_focus_handle("cursor-style", cx);

        settings_row("cursor-style".to_string(), &cursor_style_label, &t, cx, true).child(
            RadioGroup::new("cursor-style-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(
                    CursorShape::all_variants()
                        .iter()
                        .map(|&style| RadioOption::new(style, i18n!(cx, style.translation_key())))
                        .collect(),
                )
                .on_change(move |&style, _, cx| {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_cursor_style(style, cx);
                    });
                }),
        )
    }

    fn render_bell_style_row(&mut self, current: velowork_core::types::BellStyle, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.terminal.bell_style");
        let desc = i18n!(cx, "settings.terminal.bell_style_desc");
        let focus_handle = self.get_or_create_radio_focus_handle("bell-style", cx);

        settings_row_with_desc("bell-style".to_string(), &label, &desc, &t, cx, true).child(
            RadioGroup::new("bell-style-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(
                    velowork_core::types::BellStyle::all_variants()
                        .iter()
                        .map(|&style| RadioOption::new(style, i18n!(cx, style.title_key())))
                        .collect(),
                )
                .on_change(move |&style, _, cx| {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_bell_style(style, cx);
                    });
                }),
        )
    }

    fn render_term_type_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "ssh.terminal.term");
        settings_row("term-type".to_string(), &label, &t, cx, true).child(
            div().w(SELECT_WIDTH_MD).child(Select::new(&self.term_type_select)),
        )
    }

    fn render_charset_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "ssh.terminal.charset");
        settings_row("charset".to_string(), &label, &t, cx, true).child(
            div().w(SELECT_WIDTH_MD).child(Select::new(&self.charset_select)),
        )
    }

    fn render_color_scheme_row(&mut self, _current: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.color_scheme");
        let manage_fh = self.get_or_create_button_focus_handle("manage-color-schemes-btn", cx);
        settings_row("color-scheme".to_string(), &label, &t, cx, true).child(
            h_flex()
                .gap(SPACE_SM)
                .items_center()
                .child(div().w(SELECT_WIDTH_MD).child(Select::new(&self.color_scheme_select)))
                .child(
                    Button::new("manage-color-schemes-btn", &t)
                        .label(i18n!(cx, "terminal_color_schemes.manage_button"))
                        .default()
                        .tooltip(i18n!(cx, "terminal_color_schemes.manage_tooltip"))
                        .focus_handle(&manage_fh)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.dialog_trigger_focus_handle = Some(this.get_or_create_button_focus_handle("manage-color-schemes-btn", cx));
                            this.open_color_scheme_dialog(cx);
                        })),
                ),
        )
    }

    fn render_word_selection_delimiters_row(
        &self,
        t: &ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = i18n!(cx, "settings.word_selection_delimiters");
        let desc = i18n!(cx, "settings.word_selection_delimiters_desc");
        settings_row_with_desc(
            "word-selection-delimiters".to_string(),
            &label,
            &desc,
            t,
            cx,
            true,
        )
        .child(
            div()
                .w(px(220.0))
                .child(velowork_ui::Input::new(&self.word_selection_delimiters_input)),
        )
    }

    fn render_command_history_ignored_commands_row(
        &self,
        t: &ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = i18n!(cx, "command_history.setting_ignored_commands");
        let desc = i18n!(cx, "command_history.setting_ignored_commands_desc");
        settings_row_with_desc(
            "command-history-ignored-commands".to_string(),
            &label,
            &desc,
            t,
            cx,
            true,
        )
        .child(
            div()
                .w(px(220.0))
                .child(velowork_ui::Input::new(&self.command_history_ignored_commands_input)),
        )
    }

    /// Terminal background image row: a path auto-complete input followed by a
    /// "browse" button (placed after the input, per the spec) that opens a native
    /// file picker restricted to images. The input and the button share a fixed
    /// 32px height so they line up; the path-completion suggestions are shown as
    /// a floating overlay anchored below the input (see the main `render`).
    fn render_terminal_bg_image_row(
        &self,
        label: &str,
        t: &ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = SemanticPalette::from_context(cx);
        let bounds_setter = Self::bounds_setter(cx, |s, b| s.bg_image_input_bounds = b);

        let cache_opt = velowork_views_terminal::terminal_background_cache(cx);
        let (is_loading, status_row) = if let Some(cache) = cache_opt {
            let c = cache.read(cx);
            let is_loading = c.loading();
            let status = if is_loading {
                Some(
                    h_flex()
                        .items_center()
                        .gap(SPACE_XS)
                        .child(velowork_ui::spinner::loading_spinner(
                            "bg-img-spinner",
                            ICON_STD,
                            p.text_muted,
                        ))
                        .child(
                            div()
                                .text_size(ui_text_xs(cx))
                                .text_color(p.text_muted)
                                .child(i18n!(cx, "settings.terminal_background_image_processing")),
                        ),
                )
            } else if let Some(err) = c.error() {
                let msg = match err {
                    velowork_views_terminal::BackgroundImageError::NotFound => {
                        i18n!(cx, "settings.terminal_bg_image_err_not_found")
                    }
                    velowork_views_terminal::BackgroundImageError::IsDirectory => {
                        i18n!(cx, "settings.terminal_bg_image_err_is_directory")
                    }
                    velowork_views_terminal::BackgroundImageError::PermissionDenied => {
                        i18n!(cx, "settings.terminal_bg_image_err_permission_denied")
                    }
                    velowork_views_terminal::BackgroundImageError::UnsupportedFormat => {
                        i18n!(cx, "settings.terminal_bg_image_err_unsupported_format")
                    }
                    velowork_views_terminal::BackgroundImageError::DecodeFailed => {
                        i18n!(cx, "settings.terminal_bg_image_err_corrupt")
                    }
                };
                Some(
                    h_flex()
                        .items_center()
                        .gap(SPACE_XS)
                        .child(
                            AppIcon::Close
                                .size(ICON_STD)
                                .text_color(p.status_error),
                        )
                        .child(
                            div()
                                .text_size(ui_text_xs(cx))
                                .text_color(p.status_error)
                                .child(msg),
                        ),
                )
            } else if c.image().is_some() && self.bg_image_show_success {
                Some(
                    h_flex()
                        .items_center()
                        .gap(SPACE_XS)
                        .child(
                            AppIcon::Check
                                .size(ICON_STD)
                                .text_color(p.status_success),
                        )
                        .child(
                            div()
                                .text_size(ui_text_xs(cx))
                                .text_color(p.status_success)
                                .child(i18n!(cx, "settings.terminal_background_image_ready")),
                        ),
                )
            } else {
                None
            };
            (is_loading, status)
        } else {
            (false, None)
        };

        v_flex()
            .w_full()
            .gap(SPACE_2XS)
            .child(
                settings_row("terminal-bg-image".to_string(), label, t, cx, true).child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .justify_end()
                        .gap(SPACE_SM)
                        .items_center()
                        .child(
                            // Relative wrapper around just the input so the bounds canvas
                            // captures the input area (not the button) for anchoring.
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .max_w(relative(0.5))
                                .child(self.terminal_bg_image_input.clone())
                                .child(canvas(bounds_setter, |_, _, _, _| {}).absolute().inset_0()),
                        )
                        .child(self.render_image_picker_button(is_loading, cx)),
                ),
            )
            .when_some(status_row, |this, row| {
                this.child(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .pr(px(36.0))
                        .child(row),
                )
            })
    }

    /// "Browse" button shown after the path input. Opens a native file picker
    /// restricted to image files. Icon-only (image svg) with a Chinese tooltip.
    fn render_image_picker_button(&self, is_loading: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.terminal_background_image_picker");
        let focus_handle = self.get_or_create_button_focus_handle("terminal-bg-image-picker", cx);
        Button::new("terminal-bg-image-picker", &t)
            .size(ControlSize::Default)
            .icon_left(AppIcon::Image)
            .tooltip(label)
            .focus_handle(&focus_handle)
            .loading(is_loading)
            .disabled(is_loading)
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_image_picker(window, cx);
            }))
    }

    /// Path-completion suggestions rendered as a floating dropdown panel (uses
    /// the virtualized `render_path_suggestions` with max 8 items per page and
    /// keyboard arrow navigation following).
    pub(super) fn render_bg_image_suggestions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        velowork_ui::render_path_suggestions(
            "terminal-bg-image-suggestions",
            &self.terminal_bg_image_input,
            cx,
        )
    }

    /// Open a native file picker (images only) and apply the chosen path.
    fn open_image_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = i18n!(cx, "settings.terminal_background_image_picker");
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(selected))) = paths.await
                && let Some(path) = selected.first().cloned()
                && is_image_path(&path)
            {
                let p = path.to_string_lossy().to_string();
                    let _ = this.update(cx, |this, cx| {
                        this.terminal_bg_image_input
                            .update(cx, |st, cx| st.set_value_quiet(p.clone(), cx));
                        settings_entity(cx).update(cx, |state, cx| {
                            state.set_terminal_background_image(Some(p), cx);
                        });
                    });
            }
        })
        .detach();
    }
}

/// Whether the given path points to a supported local image format.
fn is_image_path(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("png")
            | Some("jpg")
            | Some("jpeg")
            | Some("webp")
            | Some("gif")
            | Some("bmp")
            | Some("tiff")
            | Some("tif")
            | Some("ico")
            | Some("svg")
    )
}
