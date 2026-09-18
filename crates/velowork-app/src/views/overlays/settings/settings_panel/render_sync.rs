use std::time::Duration;

use crate::settings::settings_entity;
use crate::theme::theme;
use crate::ui::tokens::{
    ui_text_md, ui_text_sm, RADIUS_LG, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS,
};
use velowork_ui::button::Button;
use velowork_ui::checkbox::Checkbox;
use velowork_ui::design::appearance::ControlVariant;
use velowork_ui::input::Input;
use velowork_ui::select::Select;
use velowork_ui::switch::Switch;
use velowork_ui::tooltip::{format_soft_break_text, Tooltip};
use crate::workspace::persistence;
use crate::workspace::settings::{SyncProvider, SyncSettings};
use crate::workspace::state::GlobalWorkspace;
use crate::workspace::sync::{create_sync_provider, simplify_sync_error, SyncProvider as _};
use velowork_workspace::stores::{GlobalFocusStore, GlobalSessionStore};
use velowork_state::WindowId;
use gpui::*;
use gpui::prelude::*;
use velowork_i18n::i18n;
use velowork_workspace::toast::{Toast, ToastAction, ToastActionStyle, ToastManager};

use super::{SettingsPanel, SyncTestStatus};

impl SettingsPanel {
    pub(super) fn render_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let s = settings_entity(cx).read(cx).settings.clone();

        let sync_provider_label = i18n!(cx, "settings.sync.provider.label");
        let server_url_label = i18n!(cx, "settings.sync.webdav.server");
        let username_label = i18n!(cx, "settings.sync.webdav.username");
        let password_label = i18n!(cx, "settings.sync.password_label");
        let remote_path_label = i18n!(cx, "settings.sync.webdav.path");
        let auto_sync_label = i18n!(cx, "settings.sync.auto_sync");
        let auto_sync_desc = i18n!(cx, "settings.sync.auto_sync_desc");
        let sync_now_label = i18n!(cx, "settings.sync.sync_now");
        let last_sync_label = i18n!(cx, "settings.sync.last_sync");
        let never_label = i18n!(cx, "settings.sync.never");
        let restore_backup_label = i18n!(cx, "settings.sync.restore_backup");
        let force_push_label = i18n!(cx, "settings.sync.force_push");
        let test_connection_label = i18n!(cx, "settings.sync.test_connection");
        let test_connection_testing_label = i18n!(cx, "settings.sync.test_connection_testing");
        let cancel_test_label = i18n!(cx, "settings.sync.cancel_test");
        let test_cancelled_label = i18n!(cx, "settings.sync.test_cancelled");
        let scope_title_label = i18n!(cx, "settings.sync.scope_title");
        let disaster_recovery_label = i18n!(cx, "settings.sync.disaster_recovery");
        let disaster_recovery_desc = i18n!(cx, "settings.sync.disaster_recovery_desc");

        // S3 labels
        let s3_endpoint_label = i18n!(cx, "settings.sync.s3.endpoint");
        let s3_bucket_label = i18n!(cx, "settings.sync.s3.bucket");
        let s3_region_label = i18n!(cx, "settings.sync.s3.region");
        let s3_access_key_label = i18n!(cx, "settings.sync.s3.access_key");
        let s3_secret_key_label = i18n!(cx, "settings.sync.s3.secret_key");
        let s3_prefix_label = i18n!(cx, "settings.sync.s3.prefix");
        let s3_path_style_label = i18n!(cx, "settings.sync.s3.path_style");
        let s3_path_style_desc = i18n!(cx, "settings.sync.s3.path_style_desc");

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(SPACE_LG)
            // 顶部标题栏：左侧「同步方式」，右侧同步目标下拉框（默认选中「无」）
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb(SPACE_MD)
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(t.text_primary))
                            .child(sync_provider_label),
                    )
                    .child(
                        div()
                            .w(px(140.0))
                            .child(Select::new(&self.sync_provider_select)),
                    ),
            )
            .when(s.sync.enabled, |d| {
                d
                 // 1. 提供商连接配置项（根据选中的 Provider 切换 WebDAV / S3）
                 .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(SPACE_MD)
                        .when(s.sync.provider == SyncProvider::WebDav, |d| {
                            d
                                // 服务器地址（整行）
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(SPACE_XS)
                                        .child(
                                            div()
                                                .text_size(ui_text_sm(cx))
                                                .text_color(rgb(t.text_muted))
                                                .child(server_url_label),
                                        )
                                        .child(Input::new(&self.sync_server_url_input)),
                                )
                                // 用户名 & 密码（同一行，左右各半）
                                .child(
                                    div()
                                        .flex()
                                        .gap(SPACE_MD)
                                        .child(
                                            div()
                                                .flex_1()
                                                .flex()
                                                .flex_col()
                                                .gap(SPACE_XS)
                                                .child(
                                                    div()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.text_muted))
                                                        .child(username_label),
                                                )
                                                .child(Input::new(&self.sync_username_input)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .flex()
                                                .flex_col()
                                                .gap(SPACE_XS)
                                                .child(
                                                    div()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.text_muted))
                                                        .child(password_label),
                                                )
                                                .child(Input::new(&self.sync_password_input)),
                                        ),
                                )
                                // 远程存储路径（整行）
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(SPACE_XS)
                                        .child(
                                            div()
                                                .text_size(ui_text_sm(cx))
                                                .text_color(rgb(t.text_muted))
                                                .child(remote_path_label),
                                        )
                                        .child(Input::new(&self.sync_remote_path_input)),
                                )
                        })
                        .when(s.sync.provider == SyncProvider::S3, |d| {
                            d
                                // S3 终端节点 (Endpoint)
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(SPACE_XS)
                                        .child(
                                            div()
                                                .text_size(ui_text_sm(cx))
                                                .text_color(rgb(t.text_muted))
                                                .child(s3_endpoint_label),
                                        )
                                        .child(Input::new(&self.sync_s3_endpoint_input)),
                                )
                                // 存储桶 & 区域（同一行，左右各半）
                                .child(
                                    div()
                                        .flex()
                                        .gap(SPACE_MD)
                                        .child(
                                            div()
                                                .flex_1()
                                                .flex()
                                                .flex_col()
                                                .gap(SPACE_XS)
                                                .child(
                                                    div()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.text_muted))
                                                        .child(s3_bucket_label),
                                                )
                                                .child(Input::new(&self.sync_s3_bucket_input)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .flex()
                                                .flex_col()
                                                .gap(SPACE_XS)
                                                .child(
                                                    div()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.text_muted))
                                                        .child(s3_region_label),
                                                )
                                                .child(Input::new(&self.sync_s3_region_input)),
                                        ),
                                )
                                // Access Key ID & Secret Access Key（同一行，左右各半）
                                .child(
                                    div()
                                        .flex()
                                        .gap(SPACE_MD)
                                        .child(
                                            div()
                                                .flex_1()
                                                .flex()
                                                .flex_col()
                                                .gap(SPACE_XS)
                                                .child(
                                                    div()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.text_muted))
                                                        .child(s3_access_key_label),
                                                )
                                                .child(Input::new(&self.sync_s3_access_key_input)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .flex()
                                                .flex_col()
                                                .gap(SPACE_XS)
                                                .child(
                                                    div()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.text_muted))
                                                        .child(s3_secret_key_label),
                                                )
                                                .child(Input::new(&self.sync_s3_secret_key_input)),
                                        ),
                                )
                                // 路径前缀（整行）
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(SPACE_XS)
                                        .child(
                                            div()
                                                .text_size(ui_text_sm(cx))
                                                .text_color(rgb(t.text_muted))
                                                .child(s3_prefix_label),
                                        )
                                        .child(Input::new(&self.sync_s3_prefix_input)),
                                )
                                // 强制 Path-Style 访问
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .pt(SPACE_XS)
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.0))
                                                .child(
                                                    div()
                                                        .text_size(ui_text_sm(cx))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(rgb(t.text_primary))
                                                        .child(s3_path_style_label),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.text_muted))
                                                        .child(s3_path_style_desc),
                                                ),
                                        )
                                        .child({
                                            let fh = self.get_or_create_toggle_focus_handle("sync-s3-path-style", cx);
                                            Switch::new("sync-s3-path-style")
                                                .focus(&fh)
                                                .checked(s.sync.s3.path_style)
                                                .on_click(cx.listener(|_, &val, _, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_s3_path_style(val, cx);
                                                    });
                                                }))
                                        }),
                                )
                        })
                        // 测试连接按钮（右侧对齐，左侧显示状态，长文本防溢出与换行折行保护）
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(SPACE_MD)
                                .pt(SPACE_XS)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .max_h(px(60.0))
                                        .overflow_hidden()
                                        .children(
                                            match &self.sync_test_status {
                                                SyncTestStatus::Testing => Some(
                                                    div()
                                                        .id("sync-test-result-testing")
                                                        .whitespace_normal()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.text_secondary))
                                                        .child(format_soft_break_text(&test_connection_testing_label)),
                                                ),
                                                SyncTestStatus::Cancelled => Some(
                                                    div()
                                                        .id("sync-test-result-cancelled")
                                                        .whitespace_normal()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.text_secondary))
                                                        .child(format_soft_break_text(&test_cancelled_label)),
                                                ),
                                                SyncTestStatus::Success(msg) => Some(
                                                    div()
                                                        .id("sync-test-result-success")
                                                        .whitespace_normal()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.success))
                                                        .child(format_soft_break_text(msg)),
                                                ),
                                                SyncTestStatus::Failed(msg) => {
                                                    let raw_msg = msg.clone();
                                                    let detail_msg = self.sync_test_detail.clone().unwrap_or_default();
                                                    let copy_tip = i18n!(cx, "settings.sync.test_connection_copy_tooltip");
                                                    let copied_toast = i18n!(cx, "settings.sync.test_connection_copied");

                                                    let tooltip_text = if detail_msg.is_empty() || detail_msg == raw_msg {
                                                        format!("{}\n({})", raw_msg, copy_tip)
                                                    } else {
                                                        format!("{}\n详细原因: {}\n({})", raw_msg, detail_msg, copy_tip)
                                                    };
                                                    let copy_content = if detail_msg.is_empty() {
                                                        raw_msg
                                                    } else {
                                                        format!("{}\n详细错误: {}", raw_msg, detail_msg)
                                                    };

                                                    Some(
                                                        div()
                                                            .id("sync-test-result-error")
                                                            .whitespace_normal()
                                                            .text_size(ui_text_sm(cx))
                                                            .text_color(rgb(t.error))
                                                            .cursor_pointer()
                                                            .tooltip(move |_, cx| {
                                                                cx.new(|_| Tooltip::new(tooltip_text.clone())).into()
                                                            })
                                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                                cx.write_to_clipboard(ClipboardItem::new_string(copy_content.clone()));
                                                                ToastManager::post(Toast::info(copied_toast.clone()), cx);
                                                            }))
                                                            .child(format_soft_break_text(msg)),
                                                    )
                                                }
                                                SyncTestStatus::Idle => None,
                                            },
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .child({
                                            let is_testing = self.sync_test_status == SyncTestStatus::Testing;
                                            let test_fh = self.get_or_create_button_focus_handle("sync-test-conn-btn", cx);
                                            Button::new("sync-test-conn-btn", &t)
                                                .variant(ControlVariant::Secondary)
                                                .label(if is_testing {
                                                    cancel_test_label
                                                } else {
                                                    test_connection_label
                                                })
                                                .loading(false)
                                                .disabled(false)
                                                .focus_handle(&test_fh)
                                                .on_click(cx.listener(|this, _, _window, cx| {
                                                    if this.sync_test_status == SyncTestStatus::Testing {
                                                        this.cancel_sync_test(cx);
                                                    } else {
                                                        this.test_sync_connection(cx);
                                                    }
                                                }))
                                        }),
                                ),
                        ),
                 )
                 // 2. 同步数据范围卡片（带边框，圆角与弹窗保持一致）
                 .child(
                    div()
                        .bg(p.surface_card)
                        .border_1()
                        .border_color(p.border_subtle)
                        .rounded(RADIUS_LG)
                        .p(SPACE_LG)
                        .flex()
                        .flex_col()
                        .gap(SPACE_MD)
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text_primary))
                                .child(scope_title_label),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_y(SPACE_MD)
                                .gap_x(SPACE_MD)
                                .child(
                                    div()
                                        .w(relative(0.31))
                                        .min_w(px(160.0))
                                        .child({
                                            let fh = self.get_or_create_toggle_focus_handle("sync-scope-sessions", cx);
                                            Checkbox::new("sync-scope-sessions")
                                                .focus(&fh)
                                                .checked(s.sync.data_scope.sessions)
                                                .label(i18n!(cx, "settings.sync.scope_sessions"))
                                                .on_click(move |&checked, _window, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_sync_scope_item("sessions", checked, cx);
                                                    });
                                                })
                                        }),
                                )
                                .child(
                                    div()
                                        .w(relative(0.31))
                                        .min_w(px(160.0))
                                        .child({
                                            let fh = self.get_or_create_toggle_focus_handle("sync-scope-tunnels", cx);
                                            Checkbox::new("sync-scope-tunnels")
                                                .focus(&fh)
                                                .checked(s.sync.data_scope.tunnels)
                                                .label(i18n!(cx, "settings.sync.scope_tunnels"))
                                                .on_click(move |&checked, _window, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_sync_scope_item("tunnels", checked, cx);
                                                    });
                                                })
                                        }),
                                )
                                .child(
                                    div()
                                        .w(relative(0.31))
                                        .min_w(px(160.0))
                                        .child({
                                            let fh = self.get_or_create_toggle_focus_handle("sync-scope-services", cx);
                                            Checkbox::new("sync-scope-services")
                                                .focus(&fh)
                                                .checked(s.sync.data_scope.services)
                                                .label(i18n!(cx, "settings.sync.scope_services"))
                                                .on_click(move |&checked, _window, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_sync_scope_item("services", checked, cx);
                                                    });
                                                })
                                        }),
                                )
                                .child(
                                    div()
                                        .w(relative(0.31))
                                        .min_w(px(160.0))
                                        .child({
                                            let fh = self.get_or_create_toggle_focus_handle("sync-scope-qc", cx);
                                            Checkbox::new("sync-scope-qc")
                                                .focus(&fh)
                                                .checked(s.sync.data_scope.quick_commands)
                                                .label(i18n!(cx, "settings.sync.scope_quick_commands"))
                                                .on_click(move |&checked, _window, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_sync_scope_item("quick_commands", checked, cx);
                                                    });
                                                })
                                        }),
                                )
                                .child(
                                    div()
                                        .w(relative(0.31))
                                        .min_w(px(160.0))
                                        .child({
                                            let fh = self.get_or_create_toggle_focus_handle("sync-scope-ai", cx);
                                            Checkbox::new("sync-scope-ai")
                                                .focus(&fh)
                                                .checked(s.sync.data_scope.ai_chat)
                                                .label(i18n!(cx, "settings.sync.scope_ai_chat"))
                                                .on_click(move |&checked, _window, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_sync_scope_item("ai_chat", checked, cx);
                                                    });
                                                })
                                        }),
                                )
                                .child(
                                    div()
                                        .w(relative(0.31))
                                        .min_w(px(160.0))
                                        .child({
                                            let fh = self.get_or_create_toggle_focus_handle("sync-scope-history", cx);
                                            Checkbox::new("sync-scope-history")
                                                .focus(&fh)
                                                .checked(s.sync.data_scope.command_history)
                                                .label(i18n!(cx, "settings.sync.scope_command_history"))
                                                .on_click(move |&checked, _window, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_sync_scope_item("command_history", checked, cx);
                                                    });
                                                })
                                        }),
                                )
                                .child(
                                    div()
                                        .w(relative(0.31))
                                        .min_w(px(160.0))
                                        .child({
                                            let fh = self.get_or_create_toggle_focus_handle("sync-scope-settings", cx);
                                            Checkbox::new("sync-scope-settings")
                                                .focus(&fh)
                                                .checked(s.sync.data_scope.settings)
                                                .label(i18n!(cx, "settings.sync.scope_settings"))
                                                .on_click(move |&checked, _window, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_sync_scope_item("settings", checked, cx);
                                                    });
                                                })
                                        }),
                                )
                                .child(
                                    div()
                                        .w(relative(0.31))
                                        .min_w(px(160.0))
                                        .child({
                                            let fh = self.get_or_create_toggle_focus_handle("sync-scope-themes", cx);
                                            Checkbox::new("sync-scope-themes")
                                                .focus(&fh)
                                                .checked(s.sync.data_scope.themes)
                                                .label(i18n!(cx, "settings.sync.scope_themes"))
                                                .on_click(move |&checked, _window, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_sync_scope_item("themes", checked, cx);
                                                    });
                                                })
                                        }),
                                )
                                .child(
                                    div()
                                        .w(relative(0.31))
                                        .min_w(px(160.0))
                                        .child({
                                            let fh = self.get_or_create_toggle_focus_handle("sync-scope-credentials", cx);
                                            Checkbox::new("sync-scope-credentials")
                                                .focus(&fh)
                                                .checked(s.sync.data_scope.credentials)
                                                .label(i18n!(cx, "settings.sync.scope_credentials"))
                                                .on_click(move |&checked, _window, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_sync_scope_item("credentials", checked, cx);
                                                    });
                                                })
                                        }),
                                ),
                        ),
                 )
                 // 3. 自动同步与执行（去掉标题与外侧边框，直接呈现内容）
                 .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(SPACE_MD)
                        // 自动同步与间隔调节合并在同一行
                        .child(
                            div()
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
                                                .text_size(ui_text_md(cx))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(rgb(t.text_primary))
                                                .child(auto_sync_label),
                                        )
                                        .child(
                                            div()
                                                .text_size(ui_text_sm(cx))
                                                .text_color(rgb(t.text_muted))
                                                .child(auto_sync_desc),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(SPACE_MD)
                                        .child(
                                            self.render_compact_stepper(
                                                "sync-interval",
                                                s.sync.sync_interval_secs,
                                                &t,
                                                window,
                                                cx,
                                            ),
                                        )
                                        .child({
                                            let auto_sync_fh = self.get_or_create_toggle_focus_handle("sync-auto-sync-toggle", cx);
                                            Switch::new("sync-auto-sync-toggle")
                                                .focus(&auto_sync_fh)
                                                .checked(s.sync.auto_sync)
                                                .on_click(cx.listener(|_, &val, _, cx| {
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        state.set_auto_sync(val, cx);
                                                    });
                                                }))
                                        }),
                                ),
                        )
                        // 上次同步时间与立即同步按钮（底部行）
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(SPACE_MD)
                                .pt(SPACE_MD)
                                .border_t_1()
                                .border_color(p.border_subtle)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .max_h(px(60.0))
                                        .overflow_hidden()
                                        .flex()
                                        .flex_col()
                                        .gap(SPACE_XS)
                                        .child(
                                            div()
                                                .text_size(ui_text_sm(cx))
                                                .text_color(rgb(t.text_muted))
                                                .child(format!(
                                                    "{}: {}",
                                                    last_sync_label,
                                                    s.sync.last_sync_at.as_deref().unwrap_or(&never_label)
                                                )),
                                        )
                                        .children(
                                            match &self.sync_result {
                                                Some(Ok(msg)) => Some(
                                                    div()
                                                        .id("sync-result-success")
                                                        .whitespace_normal()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(rgb(t.success))
                                                        .child(format_soft_break_text(msg)),
                                                ),
                                                Some(Err(msg)) => {
                                                    let raw_msg = msg.clone();
                                                    let detail_msg = self.sync_detail.clone().unwrap_or_default();
                                                    let copy_tip = i18n!(cx, "settings.sync.test_connection_copy_tooltip");
                                                    let copied_toast = i18n!(cx, "settings.sync.test_connection_copied");

                                                    let tooltip_text = if detail_msg.is_empty() || detail_msg == raw_msg {
                                                        format!("{}\n({})", raw_msg, copy_tip)
                                                    } else {
                                                        format!("{}\n详细原因: {}\n({})", raw_msg, detail_msg, copy_tip)
                                                    };
                                                    let copy_content = if detail_msg.is_empty() {
                                                        raw_msg
                                                    } else {
                                                        format!("{}\n详细错误: {}", raw_msg, detail_msg)
                                                    };

                                                    Some(
                                                        div()
                                                            .id("sync-result-error")
                                                            .whitespace_normal()
                                                            .text_size(ui_text_sm(cx))
                                                            .text_color(rgb(t.error))
                                                            .cursor_pointer()
                                                            .tooltip(move |_, cx| {
                                                                cx.new(|_| Tooltip::new(tooltip_text.clone())).into()
                                                            })
                                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                                cx.write_to_clipboard(ClipboardItem::new_string(copy_content.clone()));
                                                                ToastManager::post(Toast::info(copied_toast.clone()), cx);
                                                            }))
                                                            .child(format_soft_break_text(msg)),
                                                    )
                                                }
                                                None => None,
                                            },
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .child({
                                            let sync_now_fh = self.get_or_create_button_focus_handle("sync-now-btn", cx);
                                            Button::new("sync-now-btn", &t)
                                                .variant(ControlVariant::Primary)
                                                .label(if self.sync_in_progress {
                                                    i18n!(cx, "settings.sync.syncing")
                                                } else {
                                                    sync_now_label
                                                })
                                                .loading(self.sync_in_progress)
                                                .disabled(self.sync_in_progress)
                                                .focus_handle(&sync_now_fh)
                                                .on_click(cx.listener(|this, _, _window, cx| {
                                                    if !this.sync_in_progress {
                                                        this.sync_now(cx);
                                                    }
                                                }))
                                        }),
                                ),
                        ),
                 )
                 // 4. 容灾与高级恢复（保留标题，无外侧边框）
                 .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(SPACE_MD)
                        .pt(SPACE_MD)
                        .border_t_1()
                        .border_color(p.border_subtle)
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text_primary))
                                .child(disaster_recovery_label),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .justify_between()
                                .gap(SPACE_MD)
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.text_muted))
                                        .child(disaster_recovery_desc),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(SPACE_SM)
                                        .child({
                                            let restore_fh = self.get_or_create_button_focus_handle("sync-restore-backup-btn", cx);
                                            Button::new("sync-restore-backup-btn", &t)
                                                .variant(ControlVariant::Secondary)
                                                .label(if self.restore_in_progress {
                                                    i18n!(cx, "settings.sync.restoring")
                                                } else {
                                                    restore_backup_label.clone()
                                                })
                                                .loading(self.restore_in_progress)
                                                .disabled(self.restore_in_progress)
                                                .focus_handle(&restore_fh)
                                                .on_click(cx.listener(|this, _, _window, cx| {
                                                    if !this.restore_in_progress {
                                                        this.confirm_restore_from_cloud(cx);
                                                    }
                                                }))
                                        })
                                        .child({
                                            let force_fh = self.get_or_create_button_focus_handle("sync-force-push-btn", cx);
                                            Button::new("sync-force-push-btn", &t)
                                                .variant(ControlVariant::Secondary)
                                                .label(force_push_label.clone())
                                                .focus_handle(&force_fh)
                                                .on_click(cx.listener(|this, _, _window, cx| {
                                                    this.confirm_force_push_to_cloud(cx);
                                                }))
                                        }),
                                ),
                        )
                        .children(
                            match &self.restore_result {
                                Some(Ok(msg)) => Some(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.success))
                                        .child(msg.clone()),
                                ),
                                Some(Err(msg)) => Some(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.error))
                                        .child(msg.clone()),
                                ),
                                None => None,
                            },
                        ),
                 )
            })
    }

    /// 紧凑型数字步进器（用于同步间隔，单位秒）
    fn render_compact_stepper(
        &mut self,
        id: &str,
        value: u32,
        t: &crate::theme::ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let val_str = format!("{}s", value);
        let input_entity = self.get_or_create_stepper_input(id, &val_str, window, cx);

        let input_dec = input_entity.clone();
        let input_inc = input_entity.clone();

        velowork_ui::number_stepper(id.to_string(), input_entity, t)
            .width(px(84.0))
            .min(5.0)
            .max(86400.0)
            .step(60.0)
            .on_dec(cx.listener(move |_, _, _window, cx| {
                let new_val = value.saturating_sub(60).clamp(5, 86400);
                input_dec.update(cx, |s, cx| s.set_value(format!("{}s", new_val), cx));
                settings_entity(cx).update(cx, |state, cx| {
                    state.set_sync_interval_secs(new_val, cx);
                });
            }))
            .on_inc(cx.listener(move |_, _, _window, cx| {
                let new_val = (value + 60).clamp(5, 86400);
                input_inc.update(cx, |s, cx| s.set_value(format!("{}s", new_val), cx));
                settings_entity(cx).update(cx, |state, cx| {
                    state.set_sync_interval_secs(new_val, cx);
                });
            }))
            .on_commit(cx.listener(move |_, text: &str, _window, cx| {
                let cleaned = text.trim().trim_end_matches('s').trim_end_matches('S');
                if let Ok(num) = cleaned.parse::<u32>() {
                    let clamped = num.clamp(5, 86400);
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_sync_interval_secs(clamped, cx);
                    });
                }
            }))
    }

    /// 点击「从云端恢复」：先弹出带「恢复」操作的确认提示，避免误操作覆盖本地配置。
    fn confirm_restore_from_cloud(&mut self, cx: &mut Context<Self>) {
        if self.restore_in_progress {
            return;
        }
        let toast = Toast::warning(i18n!(cx, "settings.sync.restore_confirm"))
            .with_actions(vec![ToastAction::new(
                "webdav_restore_confirm",
                i18n!(cx, "settings.sync.restore_confirm_action"),
                ToastActionStyle::Danger,
            )])
            .with_ttl(Duration::from_secs(15));
        ToastManager::post(toast, cx);
    }

    /// 点击「强制覆盖云端」：弹出带「强制覆盖」操作的危险确认提示。
    fn confirm_force_push_to_cloud(&mut self, cx: &mut Context<Self>) {
        let toast = Toast::warning(i18n!(cx, "settings.sync.force_push_confirm"))
            .with_actions(vec![ToastAction::new(
                "webdav_force_push_confirm",
                i18n!(cx, "settings.sync.force_push_action"),
                ToastActionStyle::Danger,
            )])
            .with_ttl(Duration::from_secs(15));
        ToastManager::post(toast, cx);
    }

    /// 测试当前同步提供商配置是否能连通存储端。
    fn test_sync_connection(&mut self, cx: &mut Context<Self>) {
        let s = settings_entity(cx).read(cx).settings.clone();
        let mut sync = s.sync.clone();

        // 立即把界面最新输入同步回设置状态，避免时序不同步
        match sync.provider {
            SyncProvider::WebDav => {
                let server_url = self.sync_server_url_input.read(cx).text().trim().to_string();
                let username = self.sync_username_input.read(cx).text().trim().to_string();
                let remote_path = self.sync_remote_path_input.read(cx).text().trim().to_string();
                let password_input = self.sync_password_input.read(cx).text().to_string();

                settings_entity(cx).update(cx, |state, cx| {
                    state.set_webdav_server_url(server_url.clone(), cx);
                    state.set_webdav_username(username.clone(), cx);
                    state.set_webdav_remote_path(remote_path.clone(), cx);
                });
                sync.webdav.server_url = server_url;
                sync.webdav.username = username;
                sync.webdav.remote_path = remote_path;

                let secret = if password_input.is_empty() {
                    velowork_workspace::secure_storage::load_webdav_password().unwrap_or_default()
                } else {
                    password_input
                };
                self.do_test_connection(sync, if secret.is_empty() { None } else { Some(secret) }, cx);
            }
            SyncProvider::S3 => {
                let endpoint = self.sync_s3_endpoint_input.read(cx).text().trim().to_string();
                let bucket = self.sync_s3_bucket_input.read(cx).text().trim().to_string();
                let region = self.sync_s3_region_input.read(cx).text().trim().to_string();
                let access_key_id = self.sync_s3_access_key_input.read(cx).text().trim().to_string();
                let secret_key_input = self.sync_s3_secret_key_input.read(cx).text().to_string();
                let prefix = self.sync_s3_prefix_input.read(cx).text().trim().to_string();

                settings_entity(cx).update(cx, |state, cx| {
                    state.set_s3_endpoint(endpoint.clone(), cx);
                    state.set_s3_bucket(bucket.clone(), cx);
                    state.set_s3_region(region.clone(), cx);
                    state.set_s3_access_key_id(access_key_id.clone(), cx);
                    state.set_s3_prefix(prefix.clone(), cx);
                });
                sync.s3.endpoint = endpoint;
                sync.s3.bucket = bucket;
                sync.s3.region = region;
                sync.s3.access_key_id = access_key_id;
                sync.s3.prefix = prefix;

                let secret = if secret_key_input.is_empty() {
                    velowork_workspace::secure_storage::load_s3_secret_key().unwrap_or_default()
                } else {
                    secret_key_input
                };
                self.do_test_connection(sync, if secret.is_empty() { None } else { Some(secret) }, cx);
            }
        }
    }

    /// 终止当前正在进行的同步连接测试
    pub(super) fn cancel_sync_test(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.sync_test_abort_handle.take() {
            handle.abort();
        }
        self.sync_test_task = None;
        self.sync_test_status = SyncTestStatus::Cancelled;
        self.sync_test_detail = None;
        ToastManager::info(i18n!(cx, "settings.sync.test_cancelled"), cx);
        log::info!("[sync] 用户主动终止了连接测试");
        cx.notify();
    }

    /// 重置同步连接测试状态（例如输入框变更时调用，中断后台任务并恢复闲置状态）
    pub(super) fn reset_sync_test(&mut self, cx: &mut Context<Self>) {
        let had_abort = self.sync_test_abort_handle.is_some();
        let was_not_idle = self.sync_test_status != SyncTestStatus::Idle;

        if let Some(handle) = self.sync_test_abort_handle.take() {
            handle.abort();
        }
        self.sync_test_task = None;
        self.sync_test_status = SyncTestStatus::Idle;
        self.sync_test_detail = None;

        // 仅当确实中止了进行中的任务，或原先状态非 Idle 时，才触发重绘
        if had_abort || was_not_idle {
            cx.notify();
        }
    }

    fn do_test_connection(&mut self, sync: SyncSettings, secret: Option<String>, cx: &mut Context<Self>) {
        // 先确保清理旧的句柄与任务
        if let Some(handle) = self.sync_test_abort_handle.take() {
            handle.abort();
        }
        self.sync_test_task = None;

        if let Err(err) = sync.validate_configuration() {
            let err_reason = i18n!(cx, err.translation_key());
            let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), err_reason);
            log::warn!("[sync] 测试连接校验失败: {}", err_reason);
            ToastManager::error(err_msg.clone(), cx);
            self.sync_test_status = SyncTestStatus::Failed(err_msg);
            cx.notify();
            return;
        }

        let provider = match create_sync_provider(&sync, secret.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                let detail = format!("{:#}", e);
                log::error!("[sync] 创建同步客户端失败: {}", detail);
                let simple_reason = simplify_sync_error(&detail);
                let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), simple_reason);
                ToastManager::error(err_msg.clone(), cx);
                self.sync_test_status = SyncTestStatus::Failed(err_msg);
                self.sync_test_detail = Some(detail);
                cx.notify();
                return;
            }
        };

        self.sync_test_status = SyncTestStatus::Testing;
        self.sync_test_detail = None;
        cx.notify();

        log::info!("[sync] 开始测试连接 | provider={:?}", sync.provider);

        let provider_kind = sync.provider;
        let tokio_task = velowork_terminal::pty_manager::get_tokio_runtime().spawn(async move {
            tokio::time::timeout(std::time::Duration::from_secs(30), provider.test_connection()).await
        });
        self.sync_test_abort_handle = Some(tokio_task.abort_handle());

        let task = cx.spawn(async move |this, cx| {
            let join_result = tokio_task.await;
            this.update(cx, |this, cx| {
                this.sync_test_abort_handle = None;
                // 锁卫：若用户已主动终止或状态已变迁，不覆盖状态与弹窗
                if this.sync_test_status != SyncTestStatus::Testing {
                    return;
                }

                match join_result {
                    Ok(Ok(Ok(_))) => {
                        log::info!("[sync] 同步连接测试成功 | provider={:?}", provider_kind);
                        // 连接成功：确保存储凭据已持久化
                        match provider_kind {
                            SyncProvider::WebDav => {
                                let pw = this.sync_password_input.read(cx).text().to_string();
                                if !pw.is_empty() {
                                    if let Err(e) = velowork_workspace::secure_storage::store_webdav_password(&pw) {
                                        log::warn!("[sync] 测试成功后保存 WebDAV 密码失败: {}", e);
                                    }
                                    settings_entity(cx)
                                        .update(cx, |state, cx| state.set_webdav_password_stored(true, cx));
                                }
                            }
                            SyncProvider::S3 => {
                                let sk = this.sync_s3_secret_key_input.read(cx).text().to_string();
                                if !sk.is_empty() {
                                    if let Err(e) = velowork_workspace::secure_storage::store_s3_secret_key(&sk) {
                                        log::warn!("[sync] 测试成功后保存 S3 Secret Key 失败: {}", e);
                                    }
                                    settings_entity(cx)
                                        .update(cx, |state, cx| state.set_s3_secret_key_stored(true, cx));
                                }
                            }
                        }
                        let success_msg = i18n!(cx, "settings.sync.test_connection_success");
                        ToastManager::info(success_msg.clone(), cx);
                        this.sync_test_status = SyncTestStatus::Success(success_msg);
                        this.sync_test_detail = None;
                    }
                    Ok(Ok(Err(e))) => {
                        let detailed_err = format!("{:#}", e);
                        log::error!("[sync] 同步连接测试失败 | provider={:?} | 详细原因: {}", provider_kind, detailed_err);
                        let simple_reason = simplify_sync_error(&detailed_err);
                        let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), simple_reason);
                        ToastManager::error(err_msg.clone(), cx);
                        this.sync_test_status = SyncTestStatus::Failed(err_msg);
                        this.sync_test_detail = Some(detailed_err);
                    }
                    Ok(Err(_elapsed)) => {
                        let timeout_msg = "连接超时 (30s)";
                        log::warn!("[sync] 同步连接测试超时 | provider={:?}", provider_kind);
                        let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), timeout_msg);
                        ToastManager::error(err_msg.clone(), cx);
                        this.sync_test_status = SyncTestStatus::Failed(err_msg);
                        this.sync_test_detail = Some(timeout_msg.to_string());
                    }
                    Err(join_err) => {
                        if join_err.is_cancelled() {
                            log::info!("[sync] 测试任务已在底层取消");
                            if this.sync_test_status == SyncTestStatus::Testing {
                                this.sync_test_status = SyncTestStatus::Cancelled;
                            }
                        } else {
                            let detailed_err = format!("{:#}", join_err);
                            log::error!("[sync] 测试任务异常: {}", detailed_err);
                            let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), detailed_err);
                            ToastManager::error(err_msg.clone(), cx);
                            this.sync_test_status = SyncTestStatus::Failed(err_msg);
                            this.sync_test_detail = Some(detailed_err);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        });
        self.sync_test_task = Some(task);
    }

    /// 立即同步：导出本地快照，与远端比较祖先关系后 push/pull/三向智能合并。
    fn sync_now(&mut self, cx: &mut Context<Self>) {
        if self.sync_in_progress {
            return;
        }
        let s = settings_entity(cx).read(cx).settings.clone();
        let mut sync = s.sync.clone();

        let secret = match sync.provider {
            SyncProvider::WebDav => {
                let server_url = self.sync_server_url_input.read(cx).text().trim().to_string();
                let username = self.sync_username_input.read(cx).text().trim().to_string();
                let remote_path = self.sync_remote_path_input.read(cx).text().trim().to_string();
                let password_input = self.sync_password_input.read(cx).text().to_string();

                settings_entity(cx).update(cx, |state, cx| {
                    state.set_webdav_server_url(server_url.clone(), cx);
                    state.set_webdav_username(username.clone(), cx);
                    state.set_webdav_remote_path(remote_path.clone(), cx);
                });
                sync.webdav.server_url = server_url;
                sync.webdav.username = username;
                sync.webdav.remote_path = remote_path;

                if password_input.is_empty() {
                    velowork_workspace::secure_storage::load_webdav_password().unwrap_or_default()
                } else {
                    password_input
                }
            }
            SyncProvider::S3 => {
                let endpoint = self.sync_s3_endpoint_input.read(cx).text().trim().to_string();
                let bucket = self.sync_s3_bucket_input.read(cx).text().trim().to_string();
                let region = self.sync_s3_region_input.read(cx).text().trim().to_string();
                let access_key_id = self.sync_s3_access_key_input.read(cx).text().trim().to_string();
                let secret_key_input = self.sync_s3_secret_key_input.read(cx).text().to_string();
                let prefix = self.sync_s3_prefix_input.read(cx).text().trim().to_string();

                settings_entity(cx).update(cx, |state, cx| {
                    state.set_s3_endpoint(endpoint.clone(), cx);
                    state.set_s3_bucket(bucket.clone(), cx);
                    state.set_s3_region(region.clone(), cx);
                    state.set_s3_access_key_id(access_key_id.clone(), cx);
                    state.set_s3_prefix(prefix.clone(), cx);
                });
                sync.s3.endpoint = endpoint;
                sync.s3.bucket = bucket;
                sync.s3.region = region;
                sync.s3.access_key_id = access_key_id;
                sync.s3.prefix = prefix;

                if secret_key_input.is_empty() {
                    velowork_workspace::secure_storage::load_s3_secret_key().unwrap_or_default()
                } else {
                    secret_key_input
                }
            }
        };

        if let Err(err) = sync.validate_configuration() {
            let err_reason = i18n!(cx, err.translation_key());
            let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), err_reason);
            log::warn!("[sync] 立即同步校验失败: {}", err_reason);
            ToastManager::error(err_msg.clone(), cx);
            self.sync_result = Some(Err(err_msg));
            cx.notify();
            return;
        }

        let strategy = sync.conflict_strategy;
        let scope = sync.data_scope.clone();
        let provider_kind = sync.provider;

        let ctx = match crate::sync_engine::build_sync_context(&sync, if secret.is_empty() { None } else { Some(&secret) }) {
            Ok(c) => c,
            Err(e) => {
                let detail = format!("{:#}", e);
                log::error!("[sync] 构建同步上下文失败: {}", detail);
                let simple_reason = simplify_sync_error(&detail);
                let err_msg = format!("构建同步失败: {simple_reason}");
                ToastManager::error(err_msg.clone(), cx);
                self.sync_result = Some(Err(err_msg));
                self.sync_detail = Some(detail);
                cx.notify();
                return;
            }
        };

        self.sync_in_progress = true;
        self.sync_result = None;
        self.sync_detail = None;
        cx.notify();

        log::info!("[sync] 开始立即同步 | provider={:?}", provider_kind);

        cx.spawn(async move |this, cx| {
            let (provider, profile, cred, db, passphrase) = ctx;
            let passphrase_for_store = passphrase.clone();
            let result = velowork_terminal::pty_manager::get_tokio_runtime()
                .spawn(async move {
                    velowork_workspace::sync::sync_snapshot(
                        &provider, profile, &cred, db, &passphrase, strategy, &scope,
                    )
                    .await
                })
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("{}", e)));
            cx.update(|cx| {
                if let Some(entity) = this.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.sync_in_progress = false;
                        match &result {
                            Ok(r) => {
                                let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                                settings_entity(cx)
                                    .update(cx, |state, cx| state.set_last_sync_at(Some(now), cx));
                                if r.downloaded > 0 {
                                    settings_entity(cx).update(cx, |state, cx| {
                                        state.reload_settings(cx);
                                    });
                                    crate::keybindings::reload_keybindings(cx);
                                    reload_runtime_state_after_restore(cx);
                                }
                                match provider_kind {
                                    SyncProvider::WebDav => {
                                        let pw = this.sync_password_input.read(cx).text().to_string();
                                        if !pw.is_empty() {
                                            if let Err(e) =
                                                velowork_workspace::secure_storage::store_webdav_password(&pw)
                                            {
                                                log::warn!("[sync] 同步后保存 WebDAV 密码失败: {}", e);
                                            }
                                            settings_entity(cx).update(cx, |state, cx| {
                                                state.set_webdav_password_stored(true, cx)
                                            });
                                        }
                                    }
                                    SyncProvider::S3 => {
                                        let sk = this.sync_s3_secret_key_input.read(cx).text().to_string();
                                        if !sk.is_empty() {
                                            if let Err(e) =
                                                velowork_workspace::secure_storage::store_s3_secret_key(&sk)
                                            {
                                                log::warn!("[sync] 同步后保存 S3 Secret Key 失败: {}", e);
                                            }
                                            settings_entity(cx).update(cx, |state, cx| {
                                                state.set_s3_secret_key_stored(true, cx)
                                            });
                                        }
                                    }
                                }
                                let _ = velowork_workspace::secure_storage::store_sync_passphrase(
                                    &passphrase_for_store,
                                );
                                let extra = if r.errors.is_empty() {
                                    String::new()
                                } else {
                                    format!(", {} errors", r.errors.len())
                                };
                                let msg = format!(
                                    "{} (↑{} ↓{} ⚠{}{})",
                                    i18n!(cx, "settings.sync.sync_success"),
                                    r.uploaded,
                                    r.downloaded,
                                    r.conflicts,
                                    extra
                                );
                                log::info!("[sync] 立即同步成功: {}", msg);
                                ToastManager::info(msg.clone(), cx);
                                this.sync_result = Some(Ok(msg));
                                this.sync_detail = None;
                            }
                            Err(e) => {
                                let detailed_err = format!("{:#}", e);
                                log::error!("[sync] 立即同步失败: {}", detailed_err);
                                let simple_reason = simplify_sync_error(&detailed_err);
                                let err_msg = format!("同步失败: {simple_reason}");
                                ToastManager::error(err_msg.clone(), cx);
                                this.sync_result = Some(Err(err_msg));
                                this.sync_detail = Some(detailed_err);
                            }
                        }
                        cx.notify();
                    });
                }
            });
        })
        .detach();
    }
}

/// 强制覆盖云端：以本地数据为准强制推送到云端。
pub fn force_push_to_cloud(cx: &App) {
    let sync = settings_entity(cx).read(cx).settings.sync.clone();
    let scope = sync.data_scope.clone();
    if let Err(err) = sync.validate_configuration() {
        let err_reason = i18n!(cx, err.translation_key());
        let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.force_push_failed"), err_reason);
        log::warn!("[sync] 强制覆盖云端失败: {}", err_reason);
        ToastManager::error(err_msg, cx);
        return;
    }

    let ctx = match crate::sync_engine::build_sync_context(&sync, None) {
        Ok(c) => c,
        Err(e) => {
            log::error!("[sync] 构建强制覆盖云端上下文失败: {:#}", e);
            ToastManager::error(format!("构建失败: {e}"), cx);
            return;
        }
    };

    log::info!("[sync] 开始强制覆盖云端 | provider={:?}", sync.provider);

    cx.spawn(async move |cx| {
        let (provider, profile, cred, db, passphrase) = ctx;
        let result = velowork_terminal::pty_manager::get_tokio_runtime()
            .spawn(async move {
                velowork_workspace::sync::force_push_to_cloud(
                    &provider, profile, &cred, db, &passphrase, &scope,
                )
                .await
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("{}", e)));

        cx.update(|cx| {
            match &result {
                Ok(_) => {
                    log::info!("[sync] 强制覆盖云端成功");
                    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    settings_entity(cx).update(cx, |state, cx| state.set_last_sync_at(Some(now), cx));
                    ToastManager::success(
                        i18n!(cx, "settings.sync.force_push_success"),
                        cx,
                    );
                }
                Err(e) => {
                    let detailed_err = format!("{:#}", e);
                    log::error!("[sync] 强制覆盖云端失败: {}", detailed_err);
                    let simple_reason = simplify_sync_error(&detailed_err);
                    ToastManager::error(
                        format!("{}: {}", i18n!(cx, "settings.sync.force_push_failed"), simple_reason),
                        cx,
                    );
                }
            }
        });
    })
    .detach();
}

/// 从云端恢复：拉取远端最新快照并覆盖本地（忽略冲突），随后重新加载设置与
/// 快捷键到运行中的实体。由恢复确认 toast 的 action 触发。
pub fn restore_from_cloud(cx: &App) {
    let sync = settings_entity(cx).read(cx).settings.sync.clone();
    let scope = sync.data_scope.clone();
    if let Err(err) = sync.validate_configuration() {
        let err_reason = i18n!(cx, err.translation_key());
        let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.restore_failed"), err_reason);
        log::warn!("[sync] 从云端恢复失败: {}", err_reason);
        ToastManager::error(err_msg, cx);
        return;
    }

    let ctx = match crate::sync_engine::build_sync_context(&sync, None) {
        Ok(c) => c,
        Err(e) => {
            let detailed_err = format!("{:#}", e);
            log::error!("[sync] 构建云端恢复上下文失败: {}", detailed_err);
            let simple_reason = simplify_sync_error(&detailed_err);
            ToastManager::error(format!("构建恢复失败: {simple_reason}"), cx);
            return;
        }
    };

    log::info!("[sync] 开始从云端恢复快照 | provider={:?}", sync.provider);

    cx.spawn(async move |cx| {
        let (provider, profile, cred, db, passphrase) = ctx;
        let result = velowork_terminal::pty_manager::get_tokio_runtime()
            .spawn(async move {
                velowork_workspace::sync::restore_from_cloud_scoped(
                    &provider, profile, &cred, db, &passphrase, &scope,
                )
                .await
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("{}", e)));

        cx.update(|cx| {
            match &result {
                Ok(count) => {
                    log::info!("[sync] 从云端恢复成功 ({} files)", count);
                    settings_entity(cx).update(cx, |state, cx| {
                        state.reload_settings(cx);
                    });
                    crate::keybindings::reload_keybindings(cx);
                    reload_runtime_state_after_restore(cx);
                    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    settings_entity(cx).update(cx, |state, cx| state.set_last_sync_at(Some(now), cx));
                    ToastManager::success(
                        format!(
                            "{} ({} files)",
                            i18n!(cx, "settings.sync.restore_success"),
                            count
                        ),
                        cx,
                    );
                }
                Err(e) => {
                    let detailed_err = format!("{:#}", e);
                    log::error!("[sync] 从云端恢复失败: {}", detailed_err);
                    let simple_reason = simplify_sync_error(&detailed_err);
                    ToastManager::error(
                        format!("{}: {}", i18n!(cx, "settings.sync.restore_failed"), simple_reason),
                        cx,
                    );
                }
            }
        });
    })
    .detach();
}

/// 云端恢复成功后，把磁盘上已更新的 `workspace.json`（项目 / 文件夹 / 布局）、
/// `velowork.db` 的 `session_tree_node` 表（SSH 会话连接）、`tunnel_tree_node` 表（SSH 隧道）
/// 以及 `service_tree_node` 表（服务监控树）热重载进运行中的实体，使恢复内容立即生效。
pub(crate) fn reload_runtime_state_after_restore(cx: &mut App) {
    // 1) WorkspaceData：项目 / 文件夹 / 布局（来自 config/workspace.json）
    let backend = settings_entity(cx).read(cx).settings.session_backend;
    match persistence::load_workspace(backend) {
        Ok(restored) => {
            let ws = cx.global::<GlobalWorkspace>().0.clone();
            if let Some(fm) = cx
                .global::<GlobalFocusStore>()
                .0
                .read(cx)
                .manager(WindowId::Main)
            {
                fm.update(cx, |fm, cx| {
                    ws.update(cx, |ws, cx| {
                        ws.replace_data(fm, restored, cx);
                    });
                    cx.notify();
                });
            } else {
                log::warn!("[sync] 未找到主窗口 FocusManager，跳过 workspace 热重载");
            }
        }
        Err(e) => log::warn!("[sync] 从磁盘重新加载 workspace 失败: {}", e),
    }

    // 2) SSH 会话连接（来自 velowork.db）
    let store = cx.global::<GlobalSessionStore>().0.clone();
    store.update(cx, |store, cx| {
        store.reload_from_disk(cx);
    });

    // 3) SSH 隧道树（来自 velowork.db）
    if let Some(store) = cx
        .try_global::<velowork_workspace::stores::GlobalTunnelStore>()
        .map(|g| g.0.clone())
    {
        store.update(cx, |store, cx| {
            store.reload_from_disk(cx);
        });
    }

    // 4) 服务监控树（来自 velowork.db）
    if let Some(store) = cx
        .try_global::<velowork_workspace::stores::GlobalServiceStore>()
        .map(|g| g.0.clone())
    {
        store.update(cx, |store, cx| {
            store.reload_from_disk(cx);
        });
    }

    // 5) 通知所有面板与运行时数据已恢复
    crate::sync_engine::notify_runtime_state_restored(cx);

    // 6) 强制刷新所有活动窗口，确保视觉即刻同步生效
    cx.refresh_windows();
}
