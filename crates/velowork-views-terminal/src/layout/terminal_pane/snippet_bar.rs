//! Inline snippet editor bar for quick commands with variables.

use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::input::{Input, SimpleInputState};
use velowork_ui::theme::{surface_bg_t, ThemeColors};
use velowork_ui::tokens::{
    ui_text_md, RADIUS_MD, RADIUS_STD, SPACE_MD, SPACE_SM, SPACE_XS,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex};
use velowork_workspace::quick_commands::QuickCommandVar;

#[derive(Clone, Debug)]
pub struct SnippetVariableSlot {
    pub name: String,
    pub hint: String,
    pub default_value: String,
    pub is_modified: bool,
    pub input: Entity<SimpleInputState>,
}

#[derive(Clone, Debug)]
pub struct TerminalSnippetState {
    pub command_id: String,
    pub command_name: String,
    pub template: String,
    pub slots: Vec<SnippetVariableSlot>,
    pub active_index: usize,
    pub original_input: String,
}

/// Find the next slot index, prioritizing unmodified slots, or cycling sequentially.
pub fn find_next_slot_index(current: usize, total: usize, is_unmodified: impl Fn(usize) -> bool) -> usize {
    if total == 0 {
        return 0;
    }
    for i in 1..=total {
        let idx = (current + i) % total;
        if is_unmodified(idx) {
            return idx;
        }
    }
    (current + 1) % total
}

/// Replace all `{{name}}` placeholders in `template` with corresponding values.
pub fn interpolate_command(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut res = template.to_string();
    for (name, val) in replacements {
        let placeholder = format!("{{{{{}}}}}", name);
        res = res.replace(&placeholder, val);
    }
    res
}

impl TerminalSnippetState {
    pub fn new(
        id: String,
        name: String,
        template: String,
        vars: Vec<QuickCommandVar>,
        original_input: String,
        cx: &mut App,
    ) -> Self {
        let mut slots = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for v in vars {
            if seen.insert(v.name.clone()) {
                let default_val = v.default_value.clone();
                let hint = v.hint.clone();
                let input = cx.new(|cx| {
                    let mut st = SimpleInputState::new(cx)
                        .default_value(&default_val)
                        .cleanable(true);
                    if !hint.trim().is_empty() {
                        st = st.placeholder(&hint);
                    }
                    st
                });
                slots.push(SnippetVariableSlot {
                    name: v.name,
                    hint,
                    default_value: default_val,
                    is_modified: false,
                    input,
                });
            }
        }
        Self {
            command_id: id,
            command_name: name,
            template,
            slots,
            active_index: 0,
            original_input,
        }
    }

    /// Jump to the next unmodified slot, or cycle sequentially if all have been modified.
    pub fn jump_next(&mut self, window: &mut Window, cx: &mut App) {
        if self.slots.is_empty() {
            return;
        }
        let total = self.slots.len();
        let next_idx = find_next_slot_index(self.active_index, total, |idx| !self.slots[idx].is_modified);
        self.select_slot(next_idx, window, cx);
    }

    /// Jump to the previous slot.
    pub fn jump_prev(&mut self, window: &mut Window, cx: &mut App) {
        if self.slots.is_empty() {
            return;
        }
        let total = self.slots.len();
        let prev_idx = if self.active_index == 0 {
            total - 1
        } else {
            self.active_index - 1
        };
        self.select_slot(prev_idx, window, cx);
    }

    /// Activate a specific slot by index and auto-select its text for overwrite.
    pub fn select_slot(&mut self, idx: usize, window: &mut Window, cx: &mut App) {
        if idx < self.slots.len() {
            self.active_index = idx;
            let input = self.slots[idx].input.clone();
            input.update(cx, |inp, cx| {
                inp.select_all(cx);
                inp.focus(window, cx);
            });
        }
    }

    /// Generate the interpolated command by replacing all `{{name}}` placeholders with current input values.
    pub fn render_interpolated_command(&self, cx: &App) -> String {
        let pairs: Vec<(&str, &str)> = self
            .slots
            .iter()
            .map(|s| (s.name.as_str(), s.input.read(cx).value()))
            .collect();
        interpolate_command(&self.template, &pairs)
    }

    /// Synchronize slot modified status from inputs
    pub fn sync_modified_status(&mut self, cx: &App) {
        for slot in &mut self.slots {
            let current_val = slot.input.read(cx).value();
            if current_val != slot.default_value {
                slot.is_modified = true;
            }
        }
    }
}

pub fn render_snippet_bar(
    state: &TerminalSnippetState,
    cursor_pos: Option<Point<Pixels>>,
    viewport_size: Size<Pixels>,
    t: &ThemeColors,
    on_select_slot: impl Fn(usize, &mut Window, &mut App) + 'static + Send + Sync,
    cx: &App,
) -> AnyElement {
    let on_select_slot = std::sync::Arc::new(on_select_slot);

    let (left, top) = match cursor_pos {
        Some(pos) => {
            let x = pos.x.max(px(4.0));
            let bar_h = px(140.0);
            let y = if pos.y + bar_h > viewport_size.height && pos.y > bar_h {
                pos.y - bar_h - px(4.0)
            } else {
                pos.y + px(24.0)
            };
            (x, y)
        }
        None => (px(8.0), px(36.0)),
    };

    let active_slot = state.slots.get(state.active_index);
    let preview_cmd = state.render_interpolated_command(cx);
    let nav_hint = i18n!(cx, "quick_commands.snippet_nav_hint");
    let preview_title = i18n!(cx, "quick_commands.snippet_preview_title");
    let pure_name = state
        .command_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&state.command_name);

    div()
        .id("terminal-snippet-bar")
        .occlude()
        .absolute()
        .left(left)
        .top(top)
        .min_w(px(420.0))
        .max_w(px(640.0))
        .bg(surface_bg_t(t.bg_panel, t))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(RADIUS_MD)
        .shadow_lg()
        .p(SPACE_MD)
        .gap(SPACE_SM)
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap(SPACE_SM)
                // 1. Title bar (Pure command name on left, variable pills on right; NO directory shown)
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .items_center()
                        .justify_between()
                        .gap(SPACE_SM)
                        .child(
                            h_flex()
                                .flex_shrink_0()
                                .max_w(px(220.0))
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .text_size(ui_text_md(cx))
                                        .text_color(rgb(t.accent))
                                        .child("⚡"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(ui_text_md(cx))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(t.text_primary))
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .child(pure_name.to_string()),
                                ),
                        )
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .items_center()
                                .justify_end()
                                .gap(px(6.0))
                                .overflow_hidden()
                                .children(state.slots.iter().enumerate().map(|(idx, slot)| {
                                    let is_active = idx == state.active_index;
                                    let is_mod = slot.is_modified;
                                    let cb = on_select_slot.clone();
                                    let t_copy = *t;
                                    let slot_name = slot.name.clone();
                                    let slot_hint = slot.hint.clone();

                                    h_flex()
                                        .id(ElementId::Name(format!("slot-pill-{}", idx).into()))
                                        .items_center()
                                        .px(px(8.0))
                                        .py(px(3.0))
                                        .rounded(RADIUS_STD)
                                        .border_1()
                                        .border_color(if is_active {
                                            rgb(t.border_active)
                                        } else {
                                            rgb(t.border)
                                        })
                                        .bg(if is_active {
                                            surface_bg_t(t.bg_selection, t)
                                        } else {
                                            transparent_black()
                                        })
                                        .gap(px(6.0))
                                        .cursor_pointer()
                                        .hover(move |s| {
                                            if !is_active {
                                                s.bg(surface_bg_t(t_copy.bg_hover, &t_copy))
                                            } else {
                                                s
                                            }
                                        })
                                        .tooltip(move |_, cx| {
                                            let text = if slot_hint.is_empty() {
                                                slot_name.clone()
                                            } else {
                                                format!("{}: {}", slot_name, slot_hint)
                                            };
                                            cx.new(|_| Tooltip::new(text)).into()
                                        })
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            cb(idx, window, cx);
                                        })
                                        .child(
                                            div()
                                                .w(px(6.0))
                                                .h(px(6.0))
                                                .rounded_full()
                                                .bg(if is_mod {
                                                    rgb(t.accent)
                                                } else {
                                                    rgb(t.text_muted)
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_size(ui_text_md(cx))
                                                .text_color(if is_active {
                                                    rgb(t.text_primary)
                                                } else {
                                                    rgb(t.text_secondary)
                                                })
                                                .child(slot.name.clone()),
                                        )
                                })),
                        ),
                )
                // 2. Active variable input using the UNIFIED input component
                .when_some(active_slot, |parent, slot| {
                    let hint_text = if slot.hint.is_empty() {
                        slot.name.clone()
                    } else {
                        format!("{} ({})", slot.name, slot.hint)
                    };

                    parent.child(
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .gap(SPACE_XS)
                            .child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(t.text_secondary))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(hint_text),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .child(
                                        Input::new(&slot.input)
                                            .cleanable(true),
                                    ),
                            ),
                    )
                })
                // 3. Command real-time preview (matching QuickCommandVariableDialog preview style)
                .child(
                    v_flex()
                        .w_full()
                        .min_w_0()
                        .gap(SPACE_XS)
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text_secondary))
                                .child(preview_title),
                        )
                        .child(
                            div()
                                .id("snippet-preview-box")
                                .w_full()
                                .min_w_0()
                                .min_h(px(40.0))
                                .p(px(8.0))
                                .rounded(RADIUS_STD)
                                .border_1()
                                .border_color(rgb(t.border))
                                .bg(surface_bg_t(t.bg_primary, t))
                                .text_size(ui_text_md(cx))
                                .text_color(rgb(t.text_primary))
                                .font_family(".SystemMonoFont")
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .tooltip({
                                    let tip = preview_cmd.clone();
                                    move |_, cx| cx.new(|_| Tooltip::new(tip.clone())).into()
                                })
                                .child(if preview_cmd.is_empty() {
                                    i18n!(cx, "quick_commands.preview_placeholder")
                                } else {
                                    preview_cmd
                                }),
                        ),
                )
                // 4. Keyboard Navigation Hint
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.text_muted))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(nav_hint),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snippet_state_jump_next_unmodified() {
        let is_unmodified = |idx| idx == 1;
        let next = find_next_slot_index(0, 2, is_unmodified);
        assert_eq!(next, 1);

        // When all are modified, cycles to the next one
        let all_modified = |_| false;
        let next_cycle = find_next_slot_index(0, 2, all_modified);
        assert_eq!(next_cycle, 1);
    }

    #[test]
    fn test_snippet_mirrored_variable_linking() {
        let pairs = [("tag", "v1.0")];
        let template = "docker tag app:{{tag}} repo:{{tag}}";
        let rendered = interpolate_command(template, &pairs);
        assert_eq!(rendered, "docker tag app:v1.0 repo:v1.0");
    }
}
