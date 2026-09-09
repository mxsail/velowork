use super::DetectedSystemFont;

pub fn detect_ui_font() -> DetectedSystemFont {
    // 1. Try GNOME / Cinnamon / MATE / XFCE interface font via gsettings
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "font-name"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let trimmed = s.trim().trim_matches(|c: char| c == '\'' || c == '"');
            if !trimmed.is_empty() {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() > 1 {
                    if let Ok(pt_size) = parts.last().unwrap_or(&"").parse::<f32>() {
                        let family = parts[..parts.len() - 1].join(" ");
                        let text_scaling = detect_gnome_text_scaling();
                        let px_size = pt_size * (96.0 / 72.0) * text_scaling;
                        return DetectedSystemFont {
                            family,
                            size: Some(px_size),
                        };
                    }
                }
                return DetectedSystemFont {
                    family: trimmed.to_string(),
                    size: None,
                };
            }
        }
    }

    // 2. Try KDE Plasma config (~/.config/kdeglobals)
    if let Some(home) = std::env::var_os("HOME") {
        let kde_path = std::path::Path::new(&home).join(".config/kdeglobals");
        if let Ok(content) = std::fs::read_to_string(kde_path) {
            let mut in_general = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    in_general = trimmed.eq_ignore_ascii_case("[General]");
                    continue;
                }
                if in_general && trimmed.starts_with("font=") {
                    let val = &trimmed["font=".len()..];
                    let parts: Vec<&str> = val.split(',').collect();
                    if !parts.is_empty() {
                        let family = parts[0].trim().to_string();
                        let pt_size = parts.get(1).and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(11.0);
                        let px_size = pt_size * (96.0 / 72.0);
                        return DetectedSystemFont {
                            family,
                            size: Some(px_size),
                        };
                    }
                }
            }
        }
    }

    // 3. Fallback for Linux: 11pt = 14.667px
    DetectedSystemFont {
        family: "Cantarell".to_string(),
        size: Some(11.0 * (96.0 / 72.0)),
    }
}

fn detect_gnome_text_scaling() -> f32 {
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "text-scaling-factor"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Ok(factor) = s.trim().parse::<f32>() {
                if factor > 0.5 && factor < 3.0 {
                    return factor;
                }
            }
        }
    }
    1.0
}

pub fn detect_mono_font() -> String {
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "monospace-font-name"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let trimmed = s.trim().trim_matches(|c: char| c == '\'' || c == '"');
            if !trimmed.is_empty() {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() > 1 {
                    if parts.last().unwrap_or(&"").parse::<f32>().is_ok() {
                        return parts[..parts.len() - 1].join(" ");
                    }
                }
                return trimmed.to_string();
            }
        }
    }

    // KDE fallback
    if let Some(home) = std::env::var_os("HOME") {
        let kde_path = std::path::Path::new(&home).join(".config/kdeglobals");
        if let Ok(content) = std::fs::read_to_string(kde_path) {
            let mut in_general = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    in_general = trimmed.eq_ignore_ascii_case("[General]");
                    continue;
                }
                if in_general && trimmed.starts_with("fixed=") {
                    let val = &trimmed["fixed=".len()..];
                    let parts: Vec<&str> = val.split(',').collect();
                    if !parts.is_empty() {
                        return parts[0].trim().to_string();
                    }
                }
            }
        }
    }

    "JetBrains Mono".to_string()
}
