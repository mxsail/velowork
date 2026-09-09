//! 全平台通用的自定义「关于」弹窗 (About Dialog)。
//!
//! 参考 ui-design/velowork-ui.html (#aboutDialogOverlay) 规范设计：
//! 显示产品 Logo、名称、简介、当前版本及动态更新状态、技术栈与版权声明、开源许可协议及关闭按钮。

use crate::keybindings::{Cancel, CheckForUpdates};
use crate::theme::{theme, with_alpha, ThemeColors};
use crate::ui::tokens::{ui_text, ui_text_md, ui_text_ms, ui_text_sm, ui_text_xs, ui_text_xl};
use crate::views::components::modal_content;
use gpui::*;
use velowork_core::process::open_url;
use velowork_updater::{GlobalUpdateInfo, UpdateStatus};
use velowork_i18n::i18n;
use velowork_ui::brand_logo;
use velowork_ui::button::Button;
use velowork_ui::design::appearance::ControlSize;
use velowork_ui::h_flex;
use velowork_ui::icon::AppIcon;
use velowork_ui::tokens::{RADIUS_LG, RADIUS_STD, SPACE_2XS, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL};
use velowork_ui::tooltip::Tooltip;

/// 独立关于弹窗实体。
pub struct AboutDialog {
    focus_handle: FocusHandle,
    close_button_focus: FocusHandle,
    has_checked: bool,
}

/// 弹窗事件：由 `OverlayManager` 订阅并处理。
pub enum AboutDialogEvent {
    Close,
    OpenUpdateDialog,
}

impl EventEmitter<AboutDialogEvent> for AboutDialog {}

impl AboutDialog {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            close_button_focus: cx.focus_handle(),
            has_checked: false,
        }
    }

    /// 获取关闭按钮的焦点句柄（用于展示型弹窗打开时获得初始焦点）。
    pub fn close_button_focus(&self) -> FocusHandle {
        self.close_button_focus.clone()
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(AboutDialogEvent::Close);
    }

    fn open_update(&self, cx: &mut Context<Self>) {
        cx.emit(AboutDialogEvent::OpenUpdateDialog);
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let _t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let title = i18n!(cx, "about.title");

        div()
            .px(SPACE_XL)
            .py(px(10.0))
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
                    .id("about-dialog-close-icon")
                    .cursor_pointer()
                    .w(px(24.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(RADIUS_STD)
                    .hover(|s| s.bg(p.surface_hover))
                    .text_size(ui_text_xl(cx))
                    .text_color(p.text_muted)
                    .child("✕")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.close(cx)),
                    ),
            )
    }

    fn render_version_bar(&self, t: &ThemeColors, cx: &mut Context<Self>) -> impl IntoElement {
        let status = match cx.try_global::<GlobalUpdateInfo>() {
            Some(g) => g.0.status(),
            None => UpdateStatus::Idle,
        };

        let is_checking = matches!(status, UpdateStatus::Checking);
        let has_update = matches!(
            status,
            UpdateStatus::Available { .. }
                | UpdateStatus::Downloading { .. }
                | UpdateStatus::Ready { .. }
                | UpdateStatus::Installing { .. }
                | UpdateStatus::ReadyToRestart { .. }
                | UpdateStatus::BrewUpdate { .. }
        );
        let is_failed = matches!(status, UpdateStatus::Failed { .. });

        let status_action = if is_checking {
            Button::new("about-btn-checking", t)
                .default()
                .size(ControlSize::Compact)
                .loading(true)
                .label(i18n!(cx, "about.checking"))
                .disabled(true)
                .into_any_element()
        } else if has_update {
            Button::new("about-btn-new-version", t)
                .primary()
                .size(ControlSize::Compact)
                .label(i18n!(cx, "about.new_version"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_update(cx);
                }))
                .into_any_element()
        } else if is_failed {
            let retry_tip = i18n!(cx, "common.retry");
            div()
                .id("about-failed-retry-chip")
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(8.0))
                .py(px(2.0))
                .rounded(px(10.0))
                .bg(with_alpha(t.error, 0.15))
                .hover(|s| s.bg(with_alpha(t.error, 0.25)))
                .active(|s| s.bg(with_alpha(t.error, 0.35)))
                .text_size(ui_text_xs(cx))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(t.error))
                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(retry_tip.clone())).into())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.has_checked = true;
                        window.dispatch_action(Box::new(CheckForUpdates), cx);
                    }),
                )
                .child(i18n!(cx, "about.check_failed"))
                .child(
                    AppIcon::Refresh
                        .size(px(12.0))
                        .text_color(rgb(t.error)),
                )
                .into_any_element()
        } else if self.has_checked {
            div()
                .px(px(8.0))
                .py(px(2.0))
                .rounded(px(10.0))
                .bg(with_alpha(t.success, 0.15))
                .text_size(ui_text_xs(cx))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(t.success))
                .child(i18n!(cx, "about.up_to_date"))
                .into_any_element()
        } else {
            Button::new("about-btn-check", t)
                .default()
                .size(ControlSize::Compact)
                .label(i18n!(cx, "about.check_updates"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.has_checked = true;
                    window.dispatch_action(Box::new(CheckForUpdates), cx);
                }))
                .into_any_element()
        };

        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        div()
            .flex()
            .items_center()
            .justify_center()
            .gap(SPACE_MD)
            .bg(p.surface_card)
            .border_1()
            .border_color(p.border_subtle)
            .rounded(RADIUS_LG)
            .px(SPACE_LG)
            .py(SPACE_SM)
            .mb(SPACE_XL)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(SPACE_SM)
                    .child(
                        div()
                            .text_size(ui_text_ms(cx))
                            .text_color(p.text_secondary)
                            .child(i18n!(cx, "about.version")),
                    )
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.text_primary)
                            .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                    ),
            )
            .child(status_action)
    }

    fn render_body(&mut self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        div()
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .pt(SPACE_XL)
            .pb(SPACE_XL)
            .px(SPACE_XL)
            // App Logo
            .child(
                brand_logo(px(68.0), window, cx)
                    .mb(SPACE_MD),
            )
            // App Name
            .child(
                div()
                    .text_size(ui_text(22.0, cx))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(t.text_primary))
                    .mb(SPACE_2XS)
                    .child("velowork"),
            )
            // Subtitle
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(rgb(t.text_muted))
                    .mb(SPACE_LG)
                    .child(i18n!(cx, "about.subtitle")),
            )
            // Version and Update Action Bar
            .child(self.render_version_bar(&t, cx))
            // Technology Stack & Copyright
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(SPACE_2XS)
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_muted))
                    .child(i18n!(cx, "about.built_with"))
                    .child(i18n!(cx, "about.ssh_support"))
                    .child(
                        div()
                            .mt(SPACE_SM)
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "about.copyright")),
                    ),
            )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let license_label = i18n!(cx, "about.license");
        let close_label = i18n!(cx, "common.close");

        h_flex()
            .h(px(48.0))
            .flex_shrink_0()
            .px(SPACE_LG)
            .border_t_1()
            .border_color(p.border_subtle)
            .items_center()
            .justify_between()
            .child(
                Button::new("about-license-btn", &t)
                    .default()
                    .size(ControlSize::Default)
                    .label(license_label)
                    .on_click(|_, _, _| {
                        open_url("https://github.com/mxsail/velowork/blob/main/LICENSE");
                    }),
            )
            .child(
                Button::new("about-close-btn", &t)
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

impl Render for AboutDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();

        modal_content("about-dialog-card", cx)
            .w(px(360.0))
            .track_focus(&focus_handle)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| this.close(cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(self.render_header(cx))
            .child(self.render_body(window, cx))
            .child(self.render_footer(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    async fn test_about_dialog_close_button_focus_initialization(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            velowork_workspace::init_settings(cx);
            velowork_i18n::init_locale(velowork_i18n::Locale::Zh, cx);
            let dialog = cx.new(AboutDialog::new);
            let close_focus = dialog.read(cx).close_button_focus();
            let card_focus = dialog.read(cx).focus_handle.clone();
            // Ensure close button focus handle is created and distinct from card focus
            assert_ne!(close_focus, card_focus);
        });
    }
}
