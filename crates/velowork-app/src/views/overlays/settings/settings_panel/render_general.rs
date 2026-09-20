use crate::settings::settings_entity;
use crate::theme::theme;
use crate::workspace::settings::{CloseBehavior, ProxyMode};
use gpui::*;
use gpui::prelude::*;
use velowork_ui::select::Select;
use velowork_i18n::{i18n, Locale};

use super::components::*;
use super::SettingsPanel;

impl SettingsPanel {
    pub(super) fn render_general(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let (
            start_on_boot,
            auto_check_updates,
            close_behavior,
            locale,
            proxy_mode,
            proxy_port,
            notifications_enabled,
        ) = {
            let guard = settings_entity(cx).read(cx);
            let s = &guard.settings;
            (
                s.start_on_boot,
                s.auto_check_updates,
                s.close_behavior,
                s.locale,
                s.proxy_mode,
                s.proxy_port,
                s.notifications.enabled,
            )
        };

        // Section: Application
        let app_section = {
            let start_on_boot_label = i18n!(cx, "settings.start_on_boot");
            let auto_check_updates_label = i18n!(cx, "settings.auto_check_updates");

            section_container(&t)
                .child(self.render_toggle(
                    "start-on-boot", &start_on_boot_label, start_on_boot, true,
                    |state, val, cx| {
                        state.set_start_on_boot(val, cx);
                        cx.background_executor().spawn(async move {
                            if let Err(e) = crate::platform::autostart::set_autostart(val) {
                                log::warn!("Failed to set autostart: {e}");
                            }
                        }).detach();
                    }, cx,
                ))
                .child(self.render_toggle(
                    "auto-check-updates", &auto_check_updates_label, auto_check_updates, true,
                    |state, val, cx| {
                        state.set_auto_check_updates(val, cx);
                        if let Some(gui) = cx.try_global::<velowork_updater::GlobalUpdateInfo>() {
                            gui.0.set_auto_check_enabled(val);
                        }
                    }, cx,
                ))
                .child(self.render_close_behavior_row(close_behavior, cx))
        };

        // Section: Language
        let language_section = {
            section_container(&t)
                .child(self.render_language_row(locale, cx))
        };

        // Section: Network & Proxy
        let network_section = {
            let proxy_host_label = i18n!(cx, "settings.proxy_host");
            let proxy_port_label = i18n!(cx, "settings.proxy_port");

            section_container(&t)
                .child(self.render_proxy_mode_row(proxy_mode, cx))
                .when(proxy_mode == ProxyMode::Http, |d| {
                    d.child(render_input_row(
                        "proxy-host",
                        &proxy_host_label,
                        &self.proxy_host_input,
                        &t,
                        true,
                        cx,
                    ))
                    .child(self.render_number_stepper(
                        "proxy-port", &proxy_port_label, proxy_port as f32,
                        "{}", 1.0, 65535.0, 1.0, 60.0, true,
                        |state, val, cx| state.set_proxy_port(val as u16, cx), window, cx,
                    ))
                })
        };


        // Section: Notifications
        let notifications_section = {
            let desktop_notifications_label = i18n!(cx, "settings.desktop_notifications");
            let desktop_notifications_desc = i18n!(cx, "settings.desktop_notifications_desc");

            section_container(&t).child(self.render_toggle_with_desc(
                "desktop-notifications",
                &desktop_notifications_label,
                &desktop_notifications_desc,
                notifications_enabled,
                false,
                |state, val, cx| state.set_notifications_enabled(val, cx),
                cx,
            ))
        };

        let language_label = i18n!(cx, "settings.nav.language");
        let app_label = i18n!(cx, "settings.nav.general");
        let proxy_label = i18n!(cx, "settings.proxy_mode.label");
        let notifications_label = i18n!(cx, "settings.nav.notifications");

        div()
            .child(section_header(&language_label, &t, cx))
            .child(language_section)
            .child(section_header(&app_label, &t, cx))
            .child(app_section)
            .child(section_header(&proxy_label, &t, cx))
            .child(network_section)
            .child(section_header(&notifications_label, &t, cx))
            .child(notifications_section)
    }

    fn render_language_row(&mut self, _current: Locale, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.nav.language");

        settings_row("language".to_string(), &label, &t, cx, true)
            .child(div().w(px(160.0)).child(Select::new(&self.language_select)))
    }

    fn render_close_behavior_row(
        &mut self,
        current: CloseBehavior,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.close_behavior.label");
        let focus_handle = self.get_or_create_radio_focus_handle("close-behavior", cx);

        settings_row("close-behavior".to_string(), &label, &t, cx, true).child(
            RadioGroup::new("close-behavior-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(
                    CloseBehavior::all_variants()
                        .iter()
                        .map(|&behavior| RadioOption::new(behavior, i18n!(cx, behavior.translation_key())))
                        .collect(),
                )
                .on_change(move |&behavior, _, cx| {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_close_behavior(behavior, cx);
                    });
                }),
        )
    }

    fn render_proxy_mode_row(
        &mut self,
        current: ProxyMode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.proxy_mode.label");
        let focus_handle = self.get_or_create_radio_focus_handle("proxy-mode", cx);

        settings_row("proxy-mode".to_string(), &label, &t, cx, true).child(
            RadioGroup::new("proxy-mode-radio")
                .focus(&focus_handle)
                .mode(RadioMode::Button)
                .selected(Some(current))
                .options(
                    ProxyMode::all_variants()
                        .iter()
                        .map(|&mode| RadioOption::new(mode, i18n!(cx, mode.translation_key())))
                        .collect(),
                )
                .on_change(move |&mode, _, cx| {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_proxy_mode(mode, cx);
                    });
                }),
        )
    }
}
