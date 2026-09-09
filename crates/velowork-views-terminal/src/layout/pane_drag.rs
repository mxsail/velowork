//! Pane drag-and-drop types for terminal rearrangement.

use velowork_ui::theme::theme;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::theme::with_alpha;
use velowork_ui::tokens::{ui_text_md, SPACE_SM, SPACE_LG};
use gpui::*;
use velowork_ui::h_flex;

/// Drag payload emitted from a terminal header.
#[derive(Clone)]
pub struct PaneDrag {
    pub project_id: String,
    pub layout_path: Vec<usize>,
    pub terminal_id: String,
    pub terminal_name: String,
}

/// Ghost view rendered while dragging a terminal pane.
pub struct PaneDragView {
    label: String,
}

impl PaneDragView {
    pub fn new(label: String) -> Self {
        Self { label }
    }
}

impl Render for PaneDragView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        div()
            .px(SPACE_LG)
            .py(SPACE_SM)
            .bg(with_alpha(t.bg_primary, 0.95))
            .border_1()
            .border_color(p.border_active)
            .rounded(px(6.0))
            .shadow_xl()
            .text_size(ui_text_md(cx))
            .text_color(p.text_primary)
            .font_weight(FontWeight::MEDIUM)
            .child(
                h_flex()
                    .gap(SPACE_SM)
                    .child(
                        AppIcon::Terminal
                            .size(px(12.0))
                            .text_color(p.status_success),
                    )
                    .child(self.label.clone()),
            )
    }
}

/// Re-export DropZone from workspace state.
pub use velowork_workspace::state::DropZone;
