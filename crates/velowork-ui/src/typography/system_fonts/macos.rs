use super::DetectedSystemFont;

pub fn detect_ui_font() -> DetectedSystemFont {
    DetectedSystemFont {
        family: ".SF NS Text".to_string(),
        size: Some(13.0),
    }
}

pub fn detect_mono_font() -> String {
    "Menlo".to_string()
}
