//! 独立的「帮助」弹窗 (Help Dialog)。
//!
//! 复刻 ui-design/velowork-ui.html (#helpDialogOverlay) 设计规范：
//! 包含帮助头部（图标 + 标题 + Tooltip 关闭按钮）、三大内容板块（快速入门快捷键列表、常用操作功能说明、获取更多帮助外链）以及底部操作栏。
//! 内容完全通过 i18n 提供，遵循项目「界面文字必须国际化」的约束。

use crate::keybindings::Cancel;
use crate::theme::{ThemeColors, theme};
use crate::ui::tokens::{ui_text_md, ui_text_ms, ui_text_sm, ui_text_xs};
use crate::views::components::modal_content;
use gpui::prelude::*;
use gpui::*;
use velowork_core::process::open_url;
use velowork_i18n::i18n;
use velowork_ui::button::Button;
use velowork_ui::design::appearance::ControlSize;
use velowork_ui::icon::AppIcon;
use velowork_ui::scrollable::ScrollableElement;
use velowork_ui::tokens::{
    RADIUS_MD, RADIUS_STD, RADIUS_XS, SPACE_2XS, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL,
    SPACE_XS,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex};

/// 独立帮助弹窗实体。
pub struct HelpDialog {
    focus_handle: FocusHandle,
    close_button_focus: FocusHandle,
}

/// 弹窗事件：关闭时由 `toggle_overlay!` / `open_overlay!` 订阅并处理。
pub enum HelpDialogEvent {
    Close,
}

impl EventEmitter<HelpDialogEvent> for HelpDialog {}

impl HelpDialog {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            close_button_focus: cx.focus_handle(),
        }
    }

    /// 获取关闭按钮的焦点句柄（用于展示型弹窗打开时获得初始焦点）。
    pub fn close_button_focus(&self) -> FocusHandle {
        self.close_button_focus.clone()
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(HelpDialogEvent::Close);
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let _t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let title = i18n!(cx, "help.title");
        let close_tip = i18n!(cx, "common.close");

        div()
            .px(SPACE_XL)
            .py(px(12.0))
            .border_b_1()
            .border_color(p.border_subtle)
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text_primary)
                    .child(title),
            )
            .child(
                div()
                    .id("help-dialog-close-icon")
                    .cursor_pointer()
                    .w(px(24.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(RADIUS_STD)
                    .hover(|s| s.bg(p.surface_hover))
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(close_tip.clone())).into())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.close(cx)),
                    )
                    .child(AppIcon::Close.size(px(14.0)).text_color(p.text_muted)),
            )
    }

    fn render_group_title(&self, title: String, _t: &ThemeColors, cx: &App) -> impl IntoElement {
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        div()
            .text_size(ui_text_sm(cx))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.text_muted)
            .mb(SPACE_XS)
            .child(title)
    }

    fn render_shortcut_item(
        &self,
        label: String,
        keys: &[&str],
        _t: &ThemeColors,
        cx: &App,
    ) -> impl IntoElement {
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        div()
            .flex()
            .items_center()
            .justify_between()
            .px(SPACE_MD)
            .py(px(6.0))
            .rounded(RADIUS_MD)
            .hover(|s| s.bg(p.surface_hover))
            .child(
                div()
                    .text_size(ui_text_ms(cx))
                    .text_color(p.text_secondary)
                    .child(label),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(SPACE_XS)
                    .children(keys.iter().map(|key| {
                        div()
                            .px(SPACE_SM)
                            .py(px(2.0))
                            .bg(p.surface_card)
                            .border_1()
                            .border_color(p.border_subtle)
                            .rounded(RADIUS_XS)
                            .text_size(ui_text_xs(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text_muted)
                            .child(key.to_string())
                    })),
            )
    }

    fn render_help_link(
        &self,
        label: String,
        display_url: String,
        target_url: &'static str,
        t: &ThemeColors,
        cx: &App,
    ) -> impl IntoElement {
        let target = target_url.to_string();
        h_flex()
            .items_center()
            .gap(SPACE_2XS)
            .text_size(ui_text_ms(cx))
            .child(div().text_color(rgb(t.text_secondary)).child(label))
            .child(
                div()
                    .cursor_pointer()
                    .text_color(rgb(t.accent))
                    .hover(|s| s.underline())
                    .child(display_url)
                    .on_mouse_down(MouseButton::Left, move |_, _, _| {
                        open_url(&target);
                    }),
            )
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let mod_key = if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Ctrl"
        };

        let quick_start_title = i18n!(cx, "help.quick_start");
        let s_new_ssh = i18n!(cx, "help.shortcut_new_ssh");
        let s_toggle_sidebar = i18n!(cx, "help.shortcut_toggle_sidebar");
        let s_toggle_right = i18n!(cx, "help.shortcut_toggle_right_panel");
        let s_toggle_bottom = i18n!(cx, "help.shortcut_toggle_bottom_panel");
        let s_search = i18n!(cx, "help.shortcut_search_connections");
        let s_close_tab = i18n!(cx, "help.shortcut_close_tab");

        let common_ops_title = i18n!(cx, "help.common_operations");
        let conn_title = i18n!(cx, "help.connection_mgmt_title");
        let conn_desc = i18n!(cx, "help.connection_mgmt_desc");
        let qc_title = i18n!(cx, "help.quick_commands_title");
        let qc_prefix = i18n!(cx, "help.quick_commands_desc_prefix");
        let qc_var = i18n!(cx, "help.quick_commands_var");
        let qc_suffix = i18n!(cx, "help.quick_commands_desc_suffix");
        let sftp_title = i18n!(cx, "help.file_transfer_title");
        let sftp_desc = i18n!(cx, "help.file_transfer_desc");
        let ai_title = i18n!(cx, "help.ai_assistant_title");
        let ai_desc = i18n!(cx, "help.ai_assistant_desc");

        let more_help_title = i18n!(cx, "help.get_more_help");
        let docs_label = i18n!(cx, "help.docs_label");
        let docs_url = i18n!(cx, "help.docs_url");
        let feedback_label = i18n!(cx, "help.feedback_label");
        let feedback_url = i18n!(cx, "help.feedback_url");
        let community_label = i18n!(cx, "help.community_label");
        let community_url = i18n!(cx, "help.community_url");

        v_flex()
            .gap(SPACE_XL)
            .p(SPACE_XL)
            .overflow_y_scrollbar()
            // 1. 快速入门 (Quick Start)
            .child(
                v_flex()
                    .gap(SPACE_XS)
                    .child(self.render_group_title(quick_start_title, &t, cx))
                    .child(
                        v_flex()
                            .gap(px(2.0))
                            .child(self.render_shortcut_item(s_new_ssh, &[mod_key, "N"], &t, cx))
                            .child(self.render_shortcut_item(
                                s_toggle_sidebar,
                                &[mod_key, "B"],
                                &t,
                                cx,
                            ))
                            .child(self.render_shortcut_item(
                                s_toggle_right,
                                &[mod_key, "\\"],
                                &t,
                                cx,
                            ))
                            .child(self.render_shortcut_item(
                                s_toggle_bottom,
                                &[mod_key, "`"],
                                &t,
                                cx,
                            ))
                            .child(self.render_shortcut_item(s_search, &[mod_key, "F"], &t, cx))
                            .child(self.render_shortcut_item(s_close_tab, &[mod_key, "W"], &t, cx)),
                    ),
            )
            // 2. 常用操作 (Common Operations)
            .child(
                v_flex()
                    .gap(SPACE_XS)
                    .child(self.render_group_title(common_ops_title, &t, cx))
                    .child(
                        v_flex()
                            .gap(SPACE_MD)
                            .child(
                                div()
                                    .text_size(ui_text_ms(cx))
                                    .line_height(relative(1.6))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(t.text_primary))
                                            .child(conn_title),
                                    )
                                    .child(
                                        div().text_color(rgb(t.text_secondary)).child(conn_desc),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_ms(cx))
                                    .line_height(relative(1.6))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(t.text_primary))
                                            .child(qc_title),
                                    )
                                    .child(
                                        h_flex()
                                            .flex_wrap()
                                            .items_center()
                                            .child(
                                                div()
                                                    .text_color(p.text_secondary)
                                                    .child(qc_prefix),
                                            )
                                            .child(
                                                div()
                                                    .px(SPACE_XS)
                                                    .py(px(1.0))
                                                    .mx(SPACE_2XS)
                                                    .rounded(RADIUS_XS)
                                                    .bg(p.surface_card)
                                                    .border_1()
                                                    .border_color(p.border_subtle)
                                                    .text_size(ui_text_xs(cx))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(p.text_primary)
                                                    .child(qc_var),
                                            )
                                            .child(
                                                div()
                                                    .text_color(p.text_secondary)
                                                    .child(qc_suffix),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_ms(cx))
                                    .line_height(relative(1.6))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(p.text_primary)
                                            .child(sftp_title),
                                    )
                                    .child(
                                        div().text_color(p.text_secondary).child(sftp_desc),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_ms(cx))
                                    .line_height(relative(1.6))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(p.text_primary)
                                            .child(ai_title),
                                    )
                                    .child(div().text_color(p.text_secondary).child(ai_desc)),
                            ),
                    ),
            )
            // 3. 获取更多帮助 (Get More Help)
            .child(
                v_flex()
                    .gap(SPACE_XS)
                    .child(self.render_group_title(more_help_title, &t, cx))
                    .child(
                        v_flex()
                            .gap(SPACE_SM)
                            .child(self.render_help_link(
                                docs_label,
                                docs_url,
                                "https://github.com/mxsail/velowork/wiki",
                                &t,
                                cx,
                            ))
                            .child(self.render_help_link(
                                feedback_label,
                                feedback_url,
                                "https://github.com/mxsail/velowork/issues",
                                &t,
                                cx,
                            ))
                            .when(!community_url.is_empty(), |f| {
                                f.child(self.render_help_link(
                                    community_label,
                                    community_url,
                                    "",
                                    &t,
                                    cx,
                                ))
                            }),
                    ),
            )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let close_label = i18n!(cx, "common.close");

        h_flex()
            .h(px(48.0))
            .flex_shrink_0()
            .px(SPACE_LG)
            .border_t_1()
            .border_color(p.border_subtle)
            .items_center()
            .justify_end()
            .child(
                Button::new("help-dialog-close-btn", &t)
                    .primary()
                    .size(ControlSize::Default)
                    .focus_handle(&self.close_button_focus)
                    .label(close_label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close(cx);
                    })),
            )
    }
}

impl Render for HelpDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();

        modal_content("help-dialog-card", cx)
            .w(px(560.0))
            .max_h(px(640.0))
            .track_focus(&focus_handle)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| this.close(cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(self.render_header(cx))
            .child(self.render_body(cx))
            .child(self.render_footer(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    async fn test_help_dialog_close_button_focus_initialization(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            velowork_workspace::init_settings(cx);
            velowork_i18n::init_locale(velowork_i18n::Locale::Zh, cx);
            let dialog = cx.new(HelpDialog::new);
            let close_focus = dialog.read(cx).close_button_focus();
            let card_focus = dialog.read(cx).focus_handle.clone();
            // Ensure close button focus handle is created and distinct from card focus
            assert_ne!(close_focus, card_focus);
        });
    }
}
