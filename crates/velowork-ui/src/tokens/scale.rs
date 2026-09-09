//! Scale factors for UI zoom and text sizing.
//!
//! Two scaling knobs:
//! * `ui_scale` (settings, percent, default `100`, range `80..=200`) — global UI zoom.
//! * `ui_font_size` (settings, px, default `13`) — fine text-size trim.
//!
//! Both compose with an OS-derived font scale registered at startup via
//! [`set_system_font_scale`].

use gpui::App;
use std::sync::OnceLock;

pub(super) const DEFAULT_UI_FONT_SIZE: f32 = 13.0;
pub(super) const DEFAULT_UI_SCALE: f32 = 100.0;

// =============================================================================
// OS-derived font scale (registered once at startup)
// =============================================================================

/// OS-derived UI font scale, computed once at startup and cached so the
/// per-call helpers don't re-detect the OS font on every layout.
static SYSTEM_FONT_SCALE: OnceLock<f32> = OnceLock::new();

/// Register the OS-derived UI font scale. Call once at startup (before any view
/// renders), e.g. from `main.rs`. Clamped to a sane range.
pub fn set_system_font_scale(scale: f32) {
    let _ = SYSTEM_FONT_SCALE.set(scale.max(0.5).min(2.0));
}

fn get_system_font_scale() -> f32 {
    *SYSTEM_FONT_SCALE.get().unwrap_or(&1.0)
}

// =============================================================================
// Scale factors
// =============================================================================

/// Global zoom factor (`ui_scale`% x OS scale). Affects text **and** spacing.
pub fn ui_zoom_factor(cx: &App) -> f32 {
    let zoom = (super::global::get_ui_scale(cx) / 100.0).clamp(0.8, 2.0);
    zoom * get_system_font_scale()
}

/// Text-only scale factor (`ui_zoom_factor` x `ui_font_size` trim).
pub fn ui_text_scale(cx: &App) -> f32 {
    let base = (super::global::get_ui_font_size(cx) / DEFAULT_UI_FONT_SIZE).clamp(0.5, 2.5);
    ui_zoom_factor(cx) * base
}

/// Public accessor for the global zoom factor. Use this to scale bespoke
/// dimensions (custom widths, icon sizes) so they honor the UI zoom.
pub fn ui_scale_factor(cx: &App) -> f32 {
    ui_zoom_factor(cx)
}
