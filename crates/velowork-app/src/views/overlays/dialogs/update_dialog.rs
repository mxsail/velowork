//! 独立的「更新」弹窗。
//!
//! 从原设置弹窗中剥离，作为独立模态呈现完整的更新逻辑与交互：
//! 显示当前版本、检查更新、下载进度、安装、重启，以及查看更新日志。
//! 更新状态来自全局 `GlobalUpdateInfo`，检查/安装动作复用既有的
//! `CheckForUpdates` / `InstallUpdate` action（与状态栏 widget 同一套逻辑），
//! 因此无需重写更新流程，仅做 UI 呈现与交互编排。

use crate::keybindings::{Cancel, CheckForUpdates, InstallUpdate};
use crate::theme::{surface_bg_t, theme};
use crate::ui::tokens::{ui_text_md, ui_text_ms, ui_text_xl};
use velowork_ui::button::Button;
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::h_flex;
use velowork_ui::scrollable::ScrollableElement;
use velowork_ui::tokens::{SPACE_SM, SPACE_MD, SPACE_LG, SPACE_XL, RADIUS_STD, RADIUS_LG};
use crate::views::components::modal_content;
use gpui::*;
use velowork_core::process::open_url;
use velowork_updater::installer::restart_app;
use velowork_updater::{GlobalUpdateInfo, UpdateStatus};
use velowork_i18n::i18n;

/// 独立更新弹窗实体。
pub struct UpdateDialog {
    focus_handle: FocusHandle,
}

/// 弹窗事件：关闭时由 `toggle_overlay!` / `open_overlay!` 订阅并处理。
pub enum UpdateDialogEvent {
    Close,
}

impl EventEmitter<UpdateDialogEvent> for UpdateDialog {}

impl UpdateDialog {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }

    /// 获取弹窗卡片的焦点句柄。
    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(UpdateDialogEvent::Close);
    }

    pub(super) fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let _t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let title = i18n!(cx, "update.title");

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
                    .text_size(ui_text_xl(cx))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text_primary)
                    .child(title),
            )
            .child(
                div()
                    .id("update-dialog-close")
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
                    .child("\u{2715}")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.close(cx)),
                    ),
            )
    }

    /// 主体：版本卡片 + 状态内容 + 操作按钮。
    pub(super) fn render_body(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let current_version_label = i18n!(cx, "update.current_version");
        let up_to_date_label = i18n!(cx, "update.up_to_date");

        let (current_version, status) = match cx.try_global::<GlobalUpdateInfo>() {
            Some(g) => (g.0.app_version(), g.0.status()),
            None => (env!("CARGO_PKG_VERSION").to_string(), UpdateStatus::Idle),
        };

        let mut body = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .gap(SPACE_XL)
            .p(SPACE_XL)
            .child(
                div()
                    .bg(p.surface_card)
                    .rounded(RADIUS_LG)
                    .border_1()
                    .border_color(p.border_subtle)
                    .p(SPACE_LG)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(ui_text_ms(cx))
                                    .text_color(rgb(t.text_muted))
                                    .child(current_version_label),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_xl(cx))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(t.text_primary))
                                    .child(current_version.clone()),
                            ),
                    )
                    .child(self.render_status_badge(&status, cx)),
            );

        match &status {
            UpdateStatus::Idle => {
                body = body.child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.text_muted))
                        .child(up_to_date_label),
                );
            }
            UpdateStatus::Checking => {
                body = body.child(self.render_status_line(
                    &i18n!(cx, "update.checking"),
                    t.warning,
                    cx,
                ));
            }
            UpdateStatus::Available { version, .. } => {
                let label = i18n!(cx, "update.available");
                body = body.child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.text_primary))
                        .child(format!("{} {}", label, version)),
                );
            }
            UpdateStatus::Downloading { version, progress } => {
                let label = i18n!(cx, "update.downloading");
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(SPACE_SM)
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(rgb(t.text_primary))
                                .child(format!("{} v{}... {}%", label, version, progress)),
                        )
                        .child(self.render_progress_bar(*progress, cx)),
                );
            }
            UpdateStatus::Ready { version, .. } => {
                let label = i18n!(cx, "update.ready");
                body = body.child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.success))
                        .child(format!("{} v{}", label, version)),
                );
                body = body.child(self.render_whats_new(version, cx));
            }
            UpdateStatus::Installing { version } => {
                let label = i18n!(cx, "update.installing");
                body = body.child(self.render_status_line(
                    &format!("{} v{}...", label, version),
                    t.warning,
                    cx,
                ));
            }
            UpdateStatus::ReadyToRestart { version } => {
                let label = i18n!(cx, "update.ready");
                body = body.child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.success))
                        .child(format!("{} v{}", label, version)),
                );
                body = body.child(self.render_whats_new(version, cx));
            }
            UpdateStatus::BrewUpdate { version } => {
                let label = i18n!(cx, "update.brew");
                body = body.child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.text_muted))
                        .child(format!("v{} — {}", version, label)),
                );
            }
            UpdateStatus::Failed { error } => {
                let label = i18n!(cx, "update.failed");
                body = body.child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.error))
                        .child(format!("{}: {}", label, error)),
                );
            }
        }

        // 操作按钮区移到独立 Footer
        body
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let _t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let status = match cx.try_global::<GlobalUpdateInfo>() {
            Some(g) => g.0.status(),
            None => UpdateStatus::Idle,
        };
        h_flex()
            .h(px(48.0))
            .flex_shrink_0()
            .px(SPACE_LG)
            .border_t_1()
            .border_color(p.border_subtle)
            .items_center()
            .justify_end()
            .child(self.render_actions(&status, cx))
    }

    /// 状态徽标点（小圆点 + 文案），与状态栏语义一致。
    fn render_status_badge(&self, status: &UpdateStatus, cx: &App) -> impl IntoElement {
        let t = theme(cx);
        let (text, color) = match status {
            UpdateStatus::Idle => (i18n!(cx, "update.up_to_date"), t.text_muted),
            UpdateStatus::Checking => (i18n!(cx, "update.checking"), t.warning),
            UpdateStatus::Available { .. } => (i18n!(cx, "update.available"), t.warning),
            UpdateStatus::Downloading { .. } => (i18n!(cx, "update.downloading"), t.warning),
            UpdateStatus::Ready { .. } => (i18n!(cx, "update.ready"), t.success),
            UpdateStatus::Installing { .. } => (i18n!(cx, "update.installing"), t.warning),
            UpdateStatus::ReadyToRestart { .. } => {
                (i18n!(cx, "update.ready"), t.success)
            }
            UpdateStatus::BrewUpdate { .. } => (i18n!(cx, "update.brew"), t.text_muted),
            UpdateStatus::Failed { .. } => (i18n!(cx, "update.failed"), t.error),
        };
        div()
            .text_size(ui_text_ms(cx))
            .text_color(rgb(color))
            .child(text)
    }

    fn render_status_line(&self, text: &str, color: u32, cx: &App) -> impl IntoElement {
        div()
            .text_size(ui_text_md(cx))
            .text_color(rgb(color))
            .child(text.to_string())
    }

    fn render_progress_bar(&self, progress: u8, cx: &App) -> impl IntoElement {
        let t = theme(cx);
        let pct = progress.min(100) as f32 / 100.0;
        div()
            .h(px(6.0))
            .rounded_full()
            .bg(surface_bg_t(t.bg_hover, &t))
            .child(
                div()
                    .h_full()
                    .rounded_full()
                    .bg(rgb(t.border_active))
                    .w(gpui::DefiniteLength::Fraction(pct)),
            )
    }

    fn render_whats_new(&self, version: &str, cx: &App) -> impl IntoElement {
        let t = theme(cx);
        let release_url =
            format!("https://github.com/mxsail/velowork/releases/tag/v{}", version);
        let label = i18n!(cx, "update.whats_new");
        div()
            .id("update-whats-new")
            .cursor_pointer()
            .text_size(ui_text_md(cx))
            .text_color(rgb(t.text_muted))
            .hover(|s| s.text_color(rgb(t.text_primary)))
            .child(label)
            .on_click(move |_, _, _cx| {
                open_url(&release_url);
            })
    }

    /// 操作按钮：根据状态展示「检查/重试」「安装」「重启」等。
    fn render_actions(&self, status: &UpdateStatus, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let busy = matches!(
            status,
            UpdateStatus::Checking | UpdateStatus::Downloading { .. } | UpdateStatus::Installing { .. }
        );

        let mut row = div()
            .flex()
            .flex_row()
            .gap(SPACE_MD)
            .justify_end()
            .items_center();

        // 「检查更新」/「重试」按钮（忙碌时禁用）
        if !busy {
            let is_retry = matches!(status, UpdateStatus::Failed { .. });
            let label = if is_retry {
                i18n!(cx, "common.retry")
            } else {
                i18n!(cx, "update.check")
            };
            let check_btn = Button::new("update-check", &t)
                .label(label)
                .on_click(cx.listener(|_, _, window, cx| {
                    window.dispatch_action(Box::new(CheckForUpdates), cx);
                }));
            row = row.child(check_btn);
        }

        match status {
            UpdateStatus::Ready { .. } => {
                let install_btn = Button::new("update-install", &t)
                    .primary()
                    .label(i18n!(cx, "update.install"))
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(InstallUpdate), cx);
                    }));
                row = row.child(install_btn);
            }
            UpdateStatus::ReadyToRestart { .. } => {
                let restart_btn = Button::new("update-restart", &t)
                    .primary()
                    .label(i18n!(cx, "update.restart"))
                    .on_click(cx.listener(|_, _, _, cx| {
                        restart_app(cx);
                    }));
                row = row.child(restart_btn);
            }
            _ => {}
        }

        row
    }
}

impl Render for UpdateDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();

        modal_content("update-dialog-modal", cx)
            .w(px(440.0))
            .max_h(px(560.0))
            .track_focus(&focus_handle)
            .key_context("UpdateDialog")
            .on_action(cx.listener(|this, _: &Cancel, _, cx| this.close(cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .focus_scope_on_click(&focus_handle)
            .child(self.render_header(cx))
            .child(self.render_body(cx))
            .child(self.render_footer(cx))
    }
}
