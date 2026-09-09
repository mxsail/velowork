use crate::theme::{theme, ThemeColors};
use crate::ui::tokens::{ui_space_md, ui_space_sm, ui_space_xs, ui_text_md};
use gpui::*;
use gpui::prelude::*;
use velowork_i18n::i18n;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::input::Input;
use velowork_ui::tokens::{
    SPACE_SM, SPACE_MD, SPACE_LG, RADIUS_MD, RADIUS_STD, ui_icon_std_ts,
};
use velowork_ui::theme::with_alpha;

use super::SettingsPanel;

impl SettingsPanel {
    pub(super) fn render_category_nav(
        &self,
        narrow: bool,
        _has_rounded_corners: bool,
        _radius: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let categories = self.ordered_categories(cx);

        let is_nav_focused = self.nav_focus_handle.is_focused(window);

        if narrow {
            div()
                .id("settings-category-nav")
                .w_full()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .gap(px(2.0))
                .overflow_x_scroll()
                .bg(p.surface_card)
                .px(ui_space_sm(cx))
                .py(ui_space_xs(cx))
                .border_b_1()
                .border_color(p.border_subtle)
                .children(categories.iter().map(|cat| {
                    let is_active = *cat == self.active_category;
                    let category = cat.clone();
                    let label = self.category_title(cat, cx);
                    let icon = Some(cat.icon());
                    let handler =
                        cx.listener(move |this, _, _, cx| this.nav_to_category(category.clone(), cx));
                    Self::render_nav_item(&label, icon, is_active, is_nav_focused, true, &t, cx, handler)
                }))
        } else {
            div()
                .id("settings-category-nav")
                .w(px(220.0))
                .flex_shrink_0()
                .border_r_1()
                .border_color(p.border_subtle)
                .overflow_hidden()
                .flex()
                .flex_col()
                .min_h_0()
                .child(self.render_nav_search(cx))
                .child({
                    let categories_len = categories.len();
                    let current_cat = self.active_category.clone();
                    let cats_clone = categories.clone();

                    div()
                        .id("settings-nav-list")
                        .track_focus(&self.nav_focus_handle)
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                            let key = event.keystroke.key.as_str();
                            if key == "up" || key == "down" {
                                cx.stop_propagation();
                                if categories_len == 0 {
                                    return;
                                }
                                let cur_idx = cats_clone
                                    .iter()
                                    .position(|c| c == &this.active_category)
                                    .unwrap_or(0);
                                let next_idx = if key == "down" {
                                    (cur_idx + 1) % categories_len
                                } else {
                                    (cur_idx + categories_len - 1) % categories_len
                                };
                                if let Some(target_cat) = cats_clone.get(next_idx) {
                                    this.nav_to_category(target_cat.clone(), cx);
                                }
                            }
                        }))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .overflow_y_scroll()
                        .flex_1()
                        .min_h_0()
                        .px(SPACE_SM)
                        .py(ui_space_md(cx))
                        .children(categories.iter().map(|cat| {
                            let is_active = *cat == current_cat;
                            let category = cat.clone();
                            let label = self.category_title(cat, cx);
                            let icon = Some(cat.icon());
                            let handler = cx.listener(move |this, _, _, cx| {
                                this.nav_to_category(category.clone(), cx)
                            });
                            Self::render_nav_item(&label, icon, is_active, is_nav_focused, false, &t, cx, handler)
                        }))
                })
                .child(
                    div()
                        .p(SPACE_LG)
                        .child(
                            div()
                                .id("edit-settings-file-btn")
                                .cursor_pointer()
                                .px(SPACE_MD)
                                .py(SPACE_SM)
                                .rounded(RADIUS_MD)
                                .flex()
                                .items_center()
                                .gap(SPACE_SM)
                                .bg(p.surface_card)
                                .border_1()
                                .border_color(p.border_subtle)
                                .hover(|s| s.bg(p.surface_hover))
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_secondary)
                                .child(AppIcon::Code.size(ui_icon_std_ts(cx)).text_color(p.text_secondary))
                                .child(i18n!(cx, "settings.edit_json"))
                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                    crate::settings::open_settings_file();
                                    this.close(cx);
                                })),
                        ),
                )
        }
    }

    pub(super) fn render_sidebar(
        &self,
        narrow: bool,
        has_rounded_corners: bool,
        radius: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.render_category_nav(narrow, has_rounded_corners, radius, window, cx)
    }

    fn render_nav_search(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let search_text = self.nav_search_input.read(cx).value().to_string();
        let _has_text = !search_text.is_empty();
        let _clear_tip = i18n!(cx, "dock.clear_search");
        let _focus_handle = self.nav_search_input.read(cx).focus_handle(cx);

        div().p(SPACE_LG).child(
            div()
                .w_full()
                .child(Input::new(&self.nav_search_input).search(true))
        )
    }

    fn render_nav_item<T: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>(
        label: &str,
        icon: Option<AppIcon>,
        is_active: bool,
        is_nav_focused: bool,
        narrow: bool,
        _t: &ThemeColors,
        cx: &App,
        on_click: T,
    ) -> impl IntoElement + use<T> {
        let p = SemanticPalette::from_context(cx);
        if narrow {
            div()
                .id(ElementId::Name(format!("settings-nav-{}", label).into()))
                .cursor_pointer()
                .flex_shrink_0()
                .px(ui_space_md(cx))
                .py(ui_space_sm(cx))
                .flex()
                .items_center()
                .gap(ui_space_sm(cx))
                .text_size(ui_text_md(cx))
                .when_some(icon, |d, ico| {
                    d.child(ico.size(ui_icon_std_ts(cx)).text_color(if is_active {
                        p.text_primary
                    } else {
                        p.text_secondary
                    }))
                })
                .when(is_active, |d| {
                    d.text_color(p.text_primary)
                        .border_b_2()
                        .border_color(p.border_active)
                        .when(is_nav_focused, |d| d.bg(p.surface_accent.opacity(0.18)))
                })
                .when(!is_active, |d| {
                    d.text_color(p.text_secondary)
                        .hover(|s| s.bg(p.surface_hover))
                })
                .child(label.to_string())
                .on_mouse_down(MouseButton::Left, on_click)
        } else {
            let is_item_focused = is_active && is_nav_focused;
            div()
                .id(ElementId::Name(format!("settings-nav-{}", label).into()))
                .cursor_pointer()
                .px(SPACE_MD)
                .py(SPACE_MD)
                .rounded(RADIUS_STD)
                .border_1()
                .border_color(if is_item_focused {
                    p.border_active
                } else if is_active {
                    p.surface_accent.opacity(0.3)
                } else {
                    with_alpha(0x00000000, 0.0)
                })
                .flex()
                .items_center()
                .gap(SPACE_MD)
                .text_size(ui_text_md(cx))
                .when(is_active, |d| {
                    d.bg(if is_item_focused {
                        p.surface_accent.opacity(0.20)
                    } else {
                        p.surface_accent.opacity(0.14)
                    })
                    .text_color(p.text_primary)
                })
                .when(!is_active, |d| {
                    d.text_color(p.text_secondary)
                        .hover(|s| s.bg(p.surface_hover))
                })
                .when_some(icon, |d, ico| {
                    d.child(ico.size(ui_icon_std_ts(cx)).text_color(if is_active {
                        p.text_primary
                    } else {
                        p.text_secondary
                    }))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .child(label.to_string()),
                )
                .on_mouse_down(MouseButton::Left, on_click)
        }
    }
}
