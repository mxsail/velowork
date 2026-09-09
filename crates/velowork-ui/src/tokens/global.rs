//! Global UI configuration providers.
//!
//! These `Global` singletons are registered at startup and read by the scaled
//! token helpers so that every component honors the user's UI settings.

use gpui::{App, Global};
use super::super::design::density::UiDensity;
use super::super::typography;

// =============================================================================
// Global UI font size provider (fine text-size trim)
// =============================================================================

/// Global function pointer that reads the current `ui_font_size` (px) from the
/// host app's settings. The host registers this at startup; the `ui_text_*`
/// helpers divide by [`super::scale::DEFAULT_UI_FONT_SIZE`] to derive a text multiplier.
pub struct GlobalUiFontSize(pub fn(&App) -> f32);

impl Global for GlobalUiFontSize {}

pub(super) fn get_ui_font_size(cx: &App) -> f32 {
    cx.try_global::<GlobalUiFontSize>()
        .map(|g| (g.0)(cx))
        .unwrap_or(super::scale::DEFAULT_UI_FONT_SIZE)
}

// =============================================================================
// Global UI scale provider (global UI zoom, percent)
// =============================================================================

/// Global function pointer that reads the current `ui_scale` (percent, 80..=200)
/// from the host app's settings. Governs the *global* UI zoom.
pub struct GlobalUiScale(pub fn(&App) -> f32);

impl Global for GlobalUiScale {}

pub(super) fn get_ui_scale(cx: &App) -> f32 {
    cx.try_global::<GlobalUiScale>()
        .map(|g| (g.0)(cx))
        .unwrap_or(super::scale::DEFAULT_UI_SCALE)
}

// =============================================================================
// Global UI density provider
// =============================================================================

/// Global function pointer that reads the current `ui_density` from the host app's settings.
pub struct GlobalUiDensity(pub fn(&App) -> UiDensity);

impl Global for GlobalUiDensity {}

pub fn get_ui_density(cx: &App) -> UiDensity {
    cx.try_global::<GlobalUiDensity>()
        .map(|g| (g.0)(cx))
        .unwrap_or_default()
}

// =============================================================================
// Global UI font family provider
// =============================================================================

/// Global function pointer that reads the configured **UI** font family from the
/// host app's settings. Empty / `"System"` / `"System Default"` means "inherit
/// the OS native UI font" detected by FontResolver.
pub struct GlobalFontFamily(pub fn(&App) -> String);

impl Global for GlobalFontFamily {}

const DEFAULT_FONT_FAMILY: &str = "System Default";

fn get_font_family(cx: &App) -> String {
    cx.try_global::<GlobalFontFamily>()
        .map(|g| (g.0)(cx))
        .unwrap_or_else(|| DEFAULT_FONT_FAMILY.to_string())
}

/// The configured or system-resolved UI font family string.
pub fn ui_font_family(cx: &App) -> String {
    let fam = get_font_family(cx);
    if fam.is_empty() || fam == "System" || fam == "System Default" || fam == "Auto" {
        typography::detect_system_ui_font().family
    } else {
        fam
    }
}

/// Whether a UI font family is available to apply on containers.
pub fn use_custom_ui_font(cx: &App) -> bool {
    let fam = ui_font_family(cx);
    !fam.is_empty()
}

// =============================================================================
// Global monospace font family provider (code, logs, file/diff viewer)
// =============================================================================

/// Global function pointer that reads the configured **monospace** font family
/// from the host app's settings. Used for code snippets, logs and the file /
/// diff viewer. Empty / `"Auto"` / `"System Default"` means "use detected system monospace".
pub struct GlobalMonoFontFamily(pub fn(&App) -> String);

impl Global for GlobalMonoFontFamily {}

const DEFAULT_MONO_FONT_FAMILY: &str = "JetBrains Mono";

fn get_mono_font_family(cx: &App) -> String {
    cx.try_global::<GlobalMonoFontFamily>()
        .map(|g| (g.0)(cx))
        .unwrap_or_else(|| DEFAULT_MONO_FONT_FAMILY.to_string())
}

/// The resolved monospace font family.
pub fn mono_font_family(cx: &App) -> String {
    let fam = get_mono_font_family(cx);
    if fam.is_empty() || fam == "Auto" || fam == "System Default" {
        typography::detect_system_mono_font()
    } else {
        fam
    }
}

// =============================================================================
// Global Markdown font family provider (reading font for markdown / AI text)
// =============================================================================

/// Global function pointer that reads the configured **Markdown** reading font
/// family from the host app's settings.
pub struct GlobalMarkdownFontFamily(pub fn(&App) -> String);

impl Global for GlobalMarkdownFontFamily {}

const DEFAULT_MARKDOWN_FONT_FAMILY: &str = "System Default";

fn get_markdown_font_family(cx: &App) -> String {
    cx.try_global::<GlobalMarkdownFontFamily>()
        .map(|g| (g.0)(cx))
        .unwrap_or_else(|| DEFAULT_MARKDOWN_FONT_FAMILY.to_string())
}

/// The resolved Markdown reading font family.
pub fn markdown_font_family(cx: &App) -> String {
    let fam = get_markdown_font_family(cx);
    if fam.is_empty() || fam == "Auto" || fam == "System Default" || fam == "System" {
        typography::detect_system_ui_font().family
    } else {
        fam
    }
}

/// Whether a custom Markdown font family is set.
pub fn use_custom_markdown_font(cx: &App) -> bool {
    let fam = get_markdown_font_family(cx);
    !fam.is_empty() && fam != "Auto" && fam != "System Default" && fam != "System"
}
