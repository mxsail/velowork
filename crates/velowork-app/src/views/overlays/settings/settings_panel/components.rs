use crate::theme::ThemeColors;
use gpui::*;
use velowork_ui::input::{Input, InputState};

// Re-export from velowork-ui
pub use velowork_ui::settings::{
    section_container, section_header, settings_row, settings_row_with_desc,
};
pub use velowork_ui::radio::{RadioGroup, RadioMode, RadioOption};
pub use velowork_ui::form::{form_item, FormLayout};

/// Available monospace font families
pub(super) const FONT_FAMILIES: &[&str] = &[
    "JetBrains Mono",
    "Menlo",
    "SF Mono",
    "Monaco",
    "Fira Code",
    "Source Code Pro",
    "Consolas",
    "DejaVu Sans Mono",
    "Ubuntu Mono",
    "Hack",
];

/// Render a single-line setting row with label on the left and text input box (220px) on the right,
/// backed by the unified `form_item` specification.
pub(super) fn render_input_row(
    id: impl Into<SharedString>,
    label: &str,
    input: &Entity<InputState>,
    t: &ThemeColors,
    _has_border: bool,
    cx: &App,
) -> Stateful<Div> {
    let id_str: SharedString = id.into();
    form_item(id_str)
        .label(label.to_string())
        .layout(FormLayout::Horizontal)
        .justify_between(true)
        .child(
            div()
                .w(px(220.0))
                .child(Input::new(input)),
        )
        .render(t, cx)
}
