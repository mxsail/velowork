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
use crate::workspace::persistence;
use crate::workspace::settings::WebDavConfig;
use crate::workspace::state::GlobalWorkspace;
use crate::workspace::sync::WebDavSync;
use velowork_workspace::stores::{GlobalFocusStore, GlobalSessionStore};
use velowork_state::WindowId;
use gpui::*;
use gpui::prelude::*;
use velowork_i18n::i18n;
use velowork_workspace::toast::{Toast, ToastAction, ToastActionStyle, ToastManager};

use super::SettingsPanel;

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
        let scope_title_label = i18n!(cx, "settings.sync.scope_title");
        let disaster_recovery_label = i18n!(cx, "settings.sync.disaster_recovery");
        let disaster_recovery_desc = i18n!(cx, "settings.sync.disaster_recovery_desc");

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
                 // 1. WebDAV 连接配置项（外侧无边框）
                 .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(SPACE_MD)
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
                        // 测试连接按钮（右侧对齐，左侧显示状态）
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .pt(SPACE_XS)
                                .child(
                                    div()
                                        .flex_1()
                                        .children(
                                            match &self.sync_test_result {
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
                                .child({
                                    let test_fh = self.get_or_create_button_focus_handle("sync-test-conn-btn", cx);
                                    Button::new("sync-test-conn-btn", &t)
                                        .variant(ControlVariant::Secondary)
                                        .label(if self.sync_test_in_progress {
                                            test_connection_testing_label
                                        } else {
                                            test_connection_label
                                        })
                                        .loading(self.sync_test_in_progress)
                                        .disabled(self.sync_test_in_progress)
                                        .focus_handle(&test_fh)
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            if !this.sync_test_in_progress {
                                                this.test_webdav_connection(cx);
                                            }
                                        }))
                                }),
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
                                .pt(SPACE_MD)
                                .border_t_1()
                                .border_color(p.border_subtle)
                                .child(
                                    div()
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
                input_dec.update(cx, |s, cx| s.set_value(&format!("{}s", new_val), cx));
                settings_entity(cx).update(cx, |state, cx| {
                    state.set_sync_interval_secs(new_val, cx);
                });
            }))
            .on_inc(cx.listener(move |_, _, _window, cx| {
                let new_val = (value + 60).clamp(5, 86400);
                input_inc.update(cx, |s, cx| s.set_value(&format!("{}s", new_val), cx));
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

    /// 测试当前 WebDAV 配置是否能连通服务器。
    fn test_webdav_connection(&mut self, cx: &mut Context<Self>) {
        let server_url = self.sync_server_url_input.read(cx).text().trim().to_string();
        let username = self.sync_username_input.read(cx).text().trim().to_string();
        let remote_path = self.sync_remote_path_input.read(cx).text().trim().to_string();
        let password_input = self.sync_password_input.read(cx).text().to_string();

        // 立即把界面最新输入同步回设置状态，避免时序不同步
        settings_entity(cx).update(cx, |state, cx| {
            state.set_webdav_server_url(server_url.clone(), cx);
            state.set_webdav_username(username.clone(), cx);
            state.set_webdav_remote_path(remote_path.clone(), cx);
        });

        let mut config: WebDavConfig = settings_entity(cx).read(cx).settings.sync.webdav.clone();
        config.server_url = server_url.clone();
        config.username = username.clone();
        config.remote_path = remote_path.clone();

        // 输入框为空时，回退读取系统密钥库中已持久化的密码，避免每次重新输入
        let password = if password_input.is_empty() {
            velowork_workspace::secure_storage::load_webdav_password().unwrap_or_default()
        } else {
            password_input
        };

        if config.server_url.trim().is_empty() {
            let err_reason = i18n!(cx, "settings.sync.error_server_url_empty");
            let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), err_reason);
            log::warn!("[webdav] 测试连接失败: {}", err_reason);
            ToastManager::error(err_msg.clone(), cx);
            self.sync_test_result = Some(Err(err_msg));
            cx.notify();
            return;
        }

        let webdav = match WebDavSync::new(
            &config,
            if password.is_empty() { None } else { Some(&password) },
        ) {
            Ok(w) => w,
            Err(e) => {
                let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), e);
                log::error!("[webdav] 创建 WebDAV 客户端失败 | server_url={} | 错误: {:#}", config.server_url, e);
                ToastManager::error(err_msg.clone(), cx);
                self.sync_test_result = Some(Err(err_msg));
                cx.notify();
                return;
            }
        };

        self.sync_test_in_progress = true;
        self.sync_test_result = None;
        cx.notify();

        log::info!(
            "[webdav] 开始测试连接 | server_url={} username={} remote_path={}",
            config.server_url,
            config.username,
            config.remote_path
        );

        let server_url_log = config.server_url.clone();
        cx.spawn(async move |this, cx| {
            // reqwest 底层需要 Tokio reactor，必须在共享的 Tokio 运行时上执行，
            // 不能直接在 GPUI 的 smol 执行器里 await。
            let result = velowork_terminal::pty_manager::get_tokio_runtime()
                .spawn(async move { webdav.test_connection().await })
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("{}", e)));
            this.update(cx, |this, cx| {
                this.sync_test_in_progress = false;
                match &result {
                    Ok(_) => {
                        log::info!("[webdav] WebDAV 连接测试成功 | server_url={}", server_url_log);
                        // 连接成功：确保密码已持久化，避免下次重新输入
                        let pw = this.sync_password_input.read(cx).text().to_string();
                        if !pw.is_empty() {
                            if let Err(e) = velowork_workspace::secure_storage::store_webdav_password(&pw) {
                                log::warn!("[webdav] 测试成功后保存密码失败: {}", e);
                            }
                            settings_entity(cx)
                                .update(cx, |state, cx| state.set_webdav_password_stored(true, cx));
                        }
                        let success_msg = i18n!(cx, "settings.sync.test_connection_success");
                        ToastManager::info(success_msg.clone(), cx);
                        this.sync_test_result = Some(Ok(success_msg));
                    }
                    Err(e) => {
                        let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), e);
                        log::error!("[webdav] WebDAV 连接测试失败 | server_url={} | 错误: {:#}", server_url_log, e);
                        ToastManager::error(err_msg.clone(), cx);
                        this.sync_test_result = Some(Err(err_msg));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 立即同步：导出本地快照，与远端比较祖先关系后 push/pull/三向智能合并。
    fn sync_now(&mut self, cx: &mut Context<Self>) {
        if self.sync_in_progress {
            return;
        }
        let server_url = self.sync_server_url_input.read(cx).text().trim().to_string();
        let username = self.sync_username_input.read(cx).text().trim().to_string();
        let remote_path = self.sync_remote_path_input.read(cx).text().trim().to_string();
        let password_input = self.sync_password_input.read(cx).text().to_string();

        settings_entity(cx).update(cx, |state, cx| {
            state.set_webdav_server_url(server_url.clone(), cx);
            state.set_webdav_username(username.clone(), cx);
            state.set_webdav_remote_path(remote_path.clone(), cx);
        });

        let mut config: WebDavConfig = settings_entity(cx).read(cx).settings.sync.webdav.clone();
        config.server_url = server_url;
        config.username = username;
        config.remote_path = remote_path;

        let strategy = settings_entity(cx).read(cx).settings.sync.conflict_strategy;
        let scope = settings_entity(cx).read(cx).settings.sync.data_scope.clone();
        let webdav_password = if password_input.is_empty() {
            velowork_workspace::secure_storage::load_webdav_password().unwrap_or_default()
        } else {
            password_input
        };

        if config.server_url.trim().is_empty() {
            let err_reason = i18n!(cx, "settings.sync.error_server_url_empty");
            let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), err_reason);
            log::warn!("[webdav] 立即同步失败: {}", err_reason);
            ToastManager::error(err_msg.clone(), cx);
            self.sync_result = Some(Err(err_msg));
            cx.notify();
            return;
        }

        let ctx = match crate::sync_engine::build_sync_context(&config, &webdav_password) {
            Ok(c) => c,
            Err(e) => {
                let err_msg = format!("构建同步失败: {e}");
                log::error!("[webdav] 构建同步上下文失败: {:#}", e);
                ToastManager::error(err_msg.clone(), cx);
                self.sync_result = Some(Err(err_msg));
                cx.notify();
                return;
            }
        };

        self.sync_in_progress = true;
        self.sync_result = None;
        cx.notify();

        log::info!("[webdav] 开始立即同步 | server_url={}", config.server_url);

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
                                let pw = this.sync_password_input.read(cx).text().to_string();
                                if !pw.is_empty() {
                                    if let Err(e) =
                                        velowork_workspace::secure_storage::store_webdav_password(&pw)
                                    {
                                        log::warn!("[webdav] 同步后保存密码失败: {}", e);
                                    }
                                    settings_entity(cx).update(cx, |state, cx| {
                                        state.set_webdav_password_stored(true, cx)
                                    });
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
                                log::info!("[webdav] 立即同步成功: {}", msg);
                                ToastManager::info(msg.clone(), cx);
                                this.sync_result = Some(Ok(msg));
                            }
                            Err(e) => {
                                let err_msg = format!("同步失败: {e}");
                                log::error!("[webdav] 立即同步失败: {:#}", e);
                                ToastManager::error(err_msg.clone(), cx);
                                this.sync_result = Some(Err(err_msg));
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
    let config: WebDavConfig = settings_entity(cx).read(cx).settings.sync.webdav.clone();
    let scope = settings_entity(cx).read(cx).settings.sync.data_scope.clone();
    if config.server_url.trim().is_empty() {
        let err_reason = i18n!(cx, "settings.sync.error_server_url_empty");
        let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), err_reason);
        log::warn!("[webdav] 强制覆盖云端失败: {}", err_reason);
        ToastManager::error(err_msg, cx);
        return;
    }
    let webdav_password = velowork_workspace::secure_storage::load_webdav_password().unwrap_or_default();
    let ctx = match crate::sync_engine::build_sync_context(&config, &webdav_password) {
        Ok(c) => c,
        Err(e) => {
            log::error!("[webdav] 构建强制覆盖云端上下文失败: {:#}", e);
            ToastManager::error(format!("构建失败: {e}"), cx);
            return;
        }
    };

    log::info!("[webdav] 开始强制覆盖云端 | server_url={}", config.server_url);

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
                    log::info!("[webdav] 强制覆盖云端成功");
                    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    settings_entity(cx).update(cx, |state, cx| state.set_last_sync_at(Some(now), cx));
                    ToastManager::success(
                        i18n!(cx, "settings.sync.force_push_success"),
                        cx,
                    );
                }
                Err(e) => {
                    log::error!("[webdav] 强制覆盖云端失败: {:#}", e);
                    ToastManager::error(
                        format!("{}: {}", i18n!(cx, "settings.sync.force_push_failed"), e),
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
    let config: WebDavConfig = settings_entity(cx).read(cx).settings.sync.webdav.clone();
    let scope = settings_entity(cx).read(cx).settings.sync.data_scope.clone();
    if config.server_url.trim().is_empty() {
        let err_reason = i18n!(cx, "settings.sync.error_server_url_empty");
        let err_msg = format!("{}: {}", i18n!(cx, "settings.sync.test_connection_failed"), err_reason);
        log::warn!("[webdav] 从云端恢复失败: {}", err_reason);
        ToastManager::error(err_msg, cx);
        return;
    }
    let webdav_password = velowork_workspace::secure_storage::load_webdav_password().unwrap_or_default();
    let ctx = match crate::sync_engine::build_sync_context(&config, &webdav_password) {
        Ok(c) => c,
        Err(e) => {
            log::error!("[webdav] 构建云端恢复上下文失败: {:#}", e);
            ToastManager::error(format!("构建恢复失败: {e}"), cx);
            return;
        }
    };

    log::info!("[webdav] 开始从云端恢复快照 | server_url={}", config.server_url);

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
                    log::info!("[webdav] 从云端恢复成功 ({} files)", count);
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
                    log::error!("[webdav] 从云端恢复失败: {:#}", e);
                    ToastManager::error(
                        format!("{}: {}", i18n!(cx, "settings.sync.restore_failed"), e),
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
}
