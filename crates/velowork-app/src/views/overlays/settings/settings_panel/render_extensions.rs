use crate::settings::settings_entity;
use crate::theme::theme;
use gpui::*;
use velowork_extensions::ExtensionRegistry;
use velowork_i18n::i18n;

use super::components::*;
use super::SettingsPanel;

impl SettingsPanel {
    pub(super) fn render_extensions(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let enabled_extensions = {
            let guard = settings_entity(cx).read(cx);
            guard.settings.enabled_extensions.clone()
        };

        let ext_infos: Vec<(String, String)> = cx
            .try_global::<ExtensionRegistry>()
            .map(|registry| {
                registry
                    .extensions()
                    .iter()
                    .map(|ext| (ext.manifest.id.to_string(), ext.manifest.name.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        if ext_infos.is_empty() {
            let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
            let empty_card = section_container(&t)
                .p(velowork_ui::tokens::SPACE_XL)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(velowork_ui::tokens::SPACE_SM)
                .child(
                    velowork_ui::icon::AppIcon::Layers2
                        .size(px(32.0))
                        .text_color(p.text_muted),
                )
                .child(
                    div()
                        .text_size(velowork_ui::tokens::ui_text_md(cx))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text_primary)
                        .child(i18n!(cx, "settings.extensions.empty_title")),
                )
                .child(
                    div()
                        .text_size(velowork_ui::tokens::ui_text_sm(cx))
                        .text_color(p.text_muted)
                        .child(i18n!(cx, "settings.extensions.empty_desc")),
                );

            return div()
                .child(section_header(&i18n!(cx, "settings.nav.extensions"), &t, cx))
                .child(empty_card);
        }

        let mut section = section_container(&t);

        for (i, (ext_id, ext_name)) in ext_infos.iter().enumerate() {
            let enabled = enabled_extensions.contains(ext_id);
            let toggle_id = format!("ext-{}", ext_id);
            let has_border = i + 1 < ext_infos.len();
            let ext_id_for_closure = ext_id.clone();
            section = section.child(self.render_toggle(
                &toggle_id,
                ext_name,
                enabled,
                has_border,
                move |state, val, cx| state.set_extension_enabled(&ext_id_for_closure, val, cx),
                cx,
            ));
        }

        div()
            .child(section_header(&i18n!(cx, "settings.nav.extensions"), &t, cx))
            .child(section)
    }
}
