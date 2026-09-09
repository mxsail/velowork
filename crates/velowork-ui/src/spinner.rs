//! Unified Loading Spinner Component.
//!
//! Provides a rotating loading spinner powered by Lucide's `loader-circle` SVG
//! with a smooth 360-degree continuous rotation animation.

use std::time::Duration;
use gpui::prelude::*;
use gpui::*;
use crate::icon::AppIcon;

/// Create a rotating loading spinner element.
///
/// # Arguments
/// - `anim_id`: Unique animation identifier for the element.
/// - `size`: Icon size (e.g. `ICON_SM`, `ICON_STD`, `px(14.0)`).
/// - `color`: Text color for the spinner stroke.
pub fn loading_spinner(
    anim_id: impl Into<ElementId>,
    size: impl Into<Pixels>,
    color: impl Into<Hsla>,
) -> impl IntoElement {
    let anim_elem_id = anim_id.into();
    let size = size.into();
    let color = color.into();
    let anim_key = format!("{:?}-spin", anim_elem_id);

    div()
        .id(anim_elem_id)
        .flex()
        .items_center()
        .justify_center()
        .with_animation(
            anim_key,
            Animation::new(Duration::from_millis(1000)).repeat(),
            move |this, delta| {
                let angle = delta * std::f32::consts::TAU;
                this.child(
                    AppIcon::LoaderCircle
                        .size(size)
                        .text_color(color)
                        .with_transformation(Transformation::rotate(radians(angle))),
                )
            },
        )
}
