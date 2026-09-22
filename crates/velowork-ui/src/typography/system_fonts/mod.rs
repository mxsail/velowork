//! System font auto-detection for Linux, macOS, and Windows.

pub mod linux;
pub mod macos;
pub mod windows;

#[derive(Debug, Clone)]
pub struct DetectedSystemFont {
    pub family: String,
    pub size: Option<f32>,
}

use std::sync::OnceLock;

static CACHED_UI_FONT: OnceLock<DetectedSystemFont> = OnceLock::new();
static CACHED_MONO_FONT: OnceLock<String> = OnceLock::new();

/// Detect native OS system UI font family and size baseline (cached via OnceLock).
pub fn detect_system_ui_font() -> DetectedSystemFont {
    CACHED_UI_FONT
        .get_or_init(|| {
            #[cfg(target_os = "linux")]
            {
                linux::detect_ui_font()
            }
            #[cfg(target_os = "macos")]
            {
                macos::detect_ui_font()
            }
            #[cfg(target_os = "windows")]
            {
                windows::detect_ui_font()
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            {
                DetectedSystemFont {
                    family: "Inter".to_string(),
                    size: Some(13.0),
                }
            }
        })
        .clone()
}

/// Detect native OS system monospace font family (cached via OnceLock).
pub fn detect_system_mono_font() -> String {
    CACHED_MONO_FONT
        .get_or_init(|| {
            #[cfg(target_os = "linux")]
            {
                linux::detect_mono_font()
            }
            #[cfg(target_os = "macos")]
            {
                macos::detect_mono_font()
            }
            #[cfg(target_os = "windows")]
            {
                windows::detect_mono_font()
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            {
                "JetBrains Mono".to_string()
            }
        })
        .clone()
}
