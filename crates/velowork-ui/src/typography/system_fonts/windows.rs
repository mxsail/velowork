use super::DetectedSystemFont;

pub fn detect_ui_font() -> DetectedSystemFont {
    DetectedSystemFont {
        family: "Segoe UI".to_string(),
        size: Some(12.0),
    }
}

pub fn detect_mono_font() -> String {
    "Consolas".to_string()
}
