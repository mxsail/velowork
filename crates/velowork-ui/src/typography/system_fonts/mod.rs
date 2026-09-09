//! System font auto-detection for Linux, macOS, and Windows.

pub mod linux;
pub mod macos;
pub mod windows;

#[derive(Debug, Clone)]
pub struct DetectedSystemFont {
    pub family: String,
    pub size: Option<f32>,
}

/// Detect native OS system UI font family and size baseline.
pub fn detect_system_ui_font() -> DetectedSystemFont {
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
}

/// Detect native OS system monospace font family.
pub fn detect_system_mono_font() -> String {
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
}
