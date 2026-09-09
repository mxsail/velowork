//! Composable behavior modifiers for GPUI elements.
//!
//! Inspired by Flutter's `GestureDetector` and Compose's `Modifier` chains,
//! behaviors are small, reusable decorators that augment a GPUI `Div` with
//! interactive states (hover, focus ring, etc.) without polluting widget structs
//! with capability flags.
//!
//! ## Usage
//!
//! ```ignore
//! use velowork_ui::behavior::{
//!     ElementBehaviorExt, FocusRingBehavior, HoverBehavior, SelectedBehavior,
//! };
//!
//! // Background-only hover:
//! div().behavior(HoverBehavior { hover_bg: accent_color, ..Default::default() })
//!     // Foreground + underline hover (e.g. a Link button):
//!     .behavior(HoverBehavior {
//!         hover_fg: Some(accent_color),
//!         underline: true,
//!         ..Default::default()
//!     })
//!     // Selected highlight:
//!     .behavior(SelectedBehavior { selected: true, bg: sel_bg, fg: None })
//!     // Focus ring:
//!     .behavior(FocusRingBehavior { is_focused: true, ring_color: focus_color });
//! ```

use gpui::{Div, Hsla, InteractiveElement, Stateful, StatefulInteractiveElement, Styled};

/// Fully transparent `Hsla`.
fn transparent() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.0, 0.0)
}

// =============================================================================
// Core trait — works on non-stateful Div
// =============================================================================

/// A composable behavior that decorates a GPUI `Div`.
///
/// Implementations receive the element and return a modified version.
pub trait Behavior {
    /// Apply this behavior to `element`, returning the decorated element.
    fn apply(self, element: Div) -> Div;
}

// =============================================================================
// Stateful trait — works on Stateful<Div> (elements with an id)
// =============================================================================

/// A composable behavior that decorates a GPUI `Stateful<Div>`.
///
/// Use this for behaviors that require element identity (e.g. `.active()`,
/// `.on_click()`).
pub trait StatefulBehavior {
    /// Apply this behavior to a stateful `element`.
    fn apply_stateful(self, element: Stateful<Div>) -> Stateful<Div>;
}

// =============================================================================
// Concrete behaviors (non-stateful)
// =============================================================================

/// Adds a hover state to an element: an optional background, an optional
/// foreground, and an optional underline. All three are applied together
/// inside a single `.hover()` closure — GPUI's `.hover()` cannot be chained
/// multiple times without later calls overwriting earlier ones.
#[derive(Debug, Clone, Copy)]
pub struct HoverBehavior {
    /// Background color shown on hover. Use `Hsla::default()` (transparent)
    /// when only the foreground / underline should change.
    pub hover_bg: Hsla,
    /// Foreground (text) color shown on hover. `None` keeps the base color.
    pub hover_fg: Option<Hsla>,
    /// Whether to underline text on hover.
    pub underline: bool,
    /// Optional opacity applied on hover (e.g. color-swatch feedback).
    pub hover_opacity: Option<f32>,
}

impl Default for HoverBehavior {
    fn default() -> Self {
        Self {
            hover_bg: transparent(),
            hover_fg: None,
            underline: false,
            hover_opacity: None,
        }
    }
}

impl Behavior for HoverBehavior {
    fn apply(self, element: Div) -> Div {
        let bg = self.hover_bg;
        let fg = self.hover_fg;
        let ul = self.underline;
        let opacity = self.hover_opacity;
        element
            .hover(move |style| {
                let mut style = style;
                if bg.a > 0.0 {
                    style = style.bg(bg);
                }
                if let Some(color) = fg {
                    style = style.text_color(color);
                }
                if ul {
                    style = style.underline();
                }
                if let Some(o) = opacity {
                    style = style.opacity(o);
                }
                style
            })
    }
}

/// Adds a 2 px focus ring border when `is_focused` is true.
#[derive(Debug, Clone, Copy)]
pub struct FocusRingBehavior {
    /// Whether the element is currently focused.
    pub is_focused: bool,
    /// Color of the focus ring.
    pub ring_color: Hsla,
}

impl Behavior for FocusRingBehavior {
    fn apply(self, element: Div) -> Div {
        if self.is_focused {
            let color = self.ring_color;
            element
                .border_2()
                .border_color(color)
        } else {
            element
        }
    }
}

impl StatefulBehavior for HoverBehavior {
    fn apply_stateful(self, element: Stateful<Div>) -> Stateful<Div> {
        let bg = self.hover_bg;
        let fg = self.hover_fg;
        let ul = self.underline;
        let opacity = self.hover_opacity;
        element
            .hover(move |style| {
                let mut style = style;
                if bg.a > 0.0 {
                    style = style.bg(bg);
                }
                if let Some(color) = fg {
                    style = style.text_color(color);
                }
                if ul {
                    style = style.underline();
                }
                if let Some(o) = opacity {
                    style = style.opacity(o);
                }
                style
            })
    }
}

impl StatefulBehavior for FocusRingBehavior {
    fn apply_stateful(self, element: Stateful<Div>) -> Stateful<Div> {
        if self.is_focused {
            let color = self.ring_color;
            element
                .border_2()
                .border_color(color)
        } else {
            element
        }
    }
}

// =============================================================================
// Concrete behaviors (stateful — require element ID)
// =============================================================================

/// Adds a pressed/active opacity feedback. Requires a `Stateful<Div>`.
#[derive(Debug, Clone, Copy)]
pub struct ActiveFeedbackBehavior {
    /// Opacity during the active/pressed state (0.0 – 1.0).
    pub active_opacity: f32,
}

impl Default for ActiveFeedbackBehavior {
    fn default() -> Self {
        Self {
            active_opacity: 0.85,
        }
    }
}

impl StatefulBehavior for ActiveFeedbackBehavior {
    fn apply_stateful(self, element: Stateful<Div>) -> Stateful<Div> {
        let opacity = self.active_opacity;
        element
            .active(move |s| s.opacity(opacity))
    }
}

// =============================================================================
// Selection behavior (non-stateful + stateful)
// =============================================================================

/// Applies a selected / highlighted appearance (background and optional
/// foreground) when `selected` is true. Replaces ad-hoc
/// `.when(is_selected, |d| d.bg(..))` patterns in list / tree / dropdown
/// controls, keeping the "is this row selected" boolean out of widget render
/// logic.
#[derive(Debug, Clone, Copy)]
pub struct SelectedBehavior {
    /// Whether the element is currently selected.
    pub selected: bool,
    /// Background color applied when selected.
    pub bg: Hsla,
    /// Optional foreground color applied when selected.
    pub fg: Option<Hsla>,
}

impl Behavior for SelectedBehavior {
    fn apply(self, element: Div) -> Div {
        if !self.selected {
            return element;
        }
        let mut el = element.bg(self.bg);
        if let Some(color) = self.fg {
            el = el.text_color(color);
        }
        el
    }
}

impl StatefulBehavior for SelectedBehavior {
    fn apply_stateful(self, element: Stateful<Div>) -> Stateful<Div> {
        if !self.selected {
            return element;
        }
        let mut el = element.bg(self.bg);
        if let Some(color) = self.fg {
            el = el.text_color(color);
        }
        el
    }
}

// =============================================================================
// Extension traits
// =============================================================================

/// Extension trait that adds `.behavior(b)` chain calls to GPUI `Div`.
pub trait ElementBehaviorExt {
    /// Apply a [`Behavior`] modifier, returning the decorated element.
    fn behavior<B: Behavior>(self, behavior: B) -> Self;
}

impl ElementBehaviorExt for Div {
    fn behavior<B: Behavior>(self, behavior: B) -> Self {
        behavior.apply(self)
    }
}

/// Extension trait that adds `.stateful_behavior(b)` chain calls to `Stateful<Div>`.
pub trait StatefulElementBehaviorExt {
    /// Apply a [`StatefulBehavior`] modifier, returning the decorated element.
    fn stateful_behavior<B: StatefulBehavior>(self, behavior: B) -> Self;
}

impl StatefulElementBehaviorExt for Stateful<Div> {
    fn stateful_behavior<B: StatefulBehavior>(self, behavior: B) -> Self {
        behavior.apply_stateful(self)
    }
}

