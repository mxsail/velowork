//! Window decoration manager for cross-platform and Linux desktop environment adaptation.

use std::env;
use std::fs;
use gpui::{App, Decorations, Global, Window};

use smallvec::{SmallVec, smallvec};

use crate::title_bar::WindowControlType;

/// Function pointer to read custom window decoration config from application settings.
pub struct GlobalWindowDecorationConfig(pub fn(&App) -> WindowDecorationConfig);
impl Global for GlobalWindowDecorationConfig {}

/// Function pointer to check if custom titlebar is active for a window according to settings & window decorations.
pub struct GlobalIsCustomTitlebar(pub fn(&Window, &App) -> bool);
impl Global for GlobalIsCustomTitlebar {}

/// Function pointer to read titlebar height from application settings.
pub struct GlobalTitlebarHeight(pub fn(&App) -> f32);
impl Global for GlobalTitlebarHeight {}

/// Function pointer to read window control icon size from application settings.
pub struct GlobalWindowControlIconSize(pub fn(&App) -> f32);
impl Global for GlobalWindowControlIconSize {}

/// Function pointer to read window control button gap from application settings.
pub struct GlobalWindowControlButtonGap(pub fn(&App) -> f32);
impl Global for GlobalWindowControlButtonGap {}

/// Function pointer to read window control margin from application settings.
pub struct GlobalWindowControlMargin(pub fn(&App) -> f32);
impl Global for GlobalWindowControlMargin {}

/// Function pointer to read window corner radius from application settings.
pub struct GlobalWindowCornerRadius(pub fn(&App) -> f32);
impl Global for GlobalWindowCornerRadius {}

pub fn get_window_corner_radius(cx: &App) -> f32 {
    cx.try_global::<GlobalWindowCornerRadius>()
        .map(|g| (g.0)(cx))
        .unwrap_or_else(|| {
            cx.try_global::<crate::overlay::WindowCornerRadius>()
                .map(|r| r.0)
                .unwrap_or(8.0)
        })
}

pub fn get_window_control_button_gap(cx: &App) -> f32 {
    cx.try_global::<GlobalWindowControlButtonGap>()
        .map(|g| (g.0)(cx))
        .unwrap_or(4.0)
}

pub fn get_window_control_margin(cx: &App) -> f32 {
    cx.try_global::<GlobalWindowControlMargin>()
        .map(|g| (g.0)(cx))
        .unwrap_or(8.0)
}

/// Tab width mode preference for tab bars
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TabWidthMode {
    Compact,
    #[default]
    TitleLength,
    Equal,
}

/// Function pointer to read tab width mode from application settings.
pub struct GlobalTabWidthMode(pub fn(&App) -> TabWidthMode);
impl Global for GlobalTabWidthMode {}

pub fn get_tab_width_mode(cx: &App) -> TabWidthMode {
    cx.try_global::<GlobalTabWidthMode>()
        .map(|g| (g.0)(cx))
        .unwrap_or_default()
}

pub fn get_window_decoration_config(cx: &App) -> WindowDecorationConfig {
    cx.try_global::<GlobalWindowDecorationConfig>()
        .map(|g| (g.0)(cx))
        .unwrap_or_else(WindowDecorationConfig::detect)
}

pub fn is_custom_titlebar(window: &Window, cx: &App) -> bool {
    cx.try_global::<GlobalIsCustomTitlebar>()
        .map(|g| (g.0)(window, cx))
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
                true
            } else {
                matches!(window.window_decorations(), Decorations::Client { .. })
            }
        })
}

pub fn get_titlebar_height(cx: &App) -> f32 {
    let scale = crate::tokens::ui_scale_factor(cx);
    cx.try_global::<GlobalTitlebarHeight>()
        .map(|g| (g.0)(cx) * scale)
        .unwrap_or_else(|| crate::tab::tab_height(cx))
}

pub fn get_window_control_icon_size(cx: &App) -> f32 {
    cx.try_global::<GlobalWindowControlIconSize>()
        .map(|g| (g.0)(cx))
        .unwrap_or(18.0)
}

/// Window control button position (Left vs Right)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowButtonPosition {
    Left,
    Right,
}

/// Window control button visual style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowControlStyle {
    Windows11,
    MacOS,
    LinuxCSD,
    KDEBreeze,
}

impl WindowControlStyle {
    pub fn button_size(self) -> (gpui::Pixels, gpui::Pixels) {
        match self {
            WindowControlStyle::Windows11 => (gpui::px(44.0), gpui::px(32.0)),
            WindowControlStyle::MacOS => (gpui::px(12.0), gpui::px(12.0)),
            WindowControlStyle::KDEBreeze => (gpui::px(20.0), gpui::px(20.0)),
            WindowControlStyle::LinuxCSD => (gpui::px(24.0), gpui::px(24.0)),
        }
    }

    pub fn container_gap(self) -> gpui::Pixels {
        match self {
            WindowControlStyle::Windows11 => gpui::px(0.0),
            WindowControlStyle::MacOS => gpui::px(6.0),
            WindowControlStyle::KDEBreeze => gpui::px(4.0),
            WindowControlStyle::LinuxCSD => gpui::px(2.0),
        }
    }

    pub fn icon_size(self) -> gpui::Pixels {
        match self {
            WindowControlStyle::Windows11 => gpui::px(12.0),
            WindowControlStyle::MacOS => gpui::px(12.0),
            WindowControlStyle::KDEBreeze => gpui::px(12.0),
            WindowControlStyle::LinuxCSD => gpui::px(16.0),
        }
    }
}

/// Full configuration for window decoration buttons
#[derive(Debug, Clone, PartialEq)]
pub struct WindowDecorationConfig {
    pub position: WindowButtonPosition,
    pub buttons: SmallVec<[WindowControlType; 4]>,
    pub style: WindowControlStyle,
    pub custom_gap: Option<f32>,
    pub custom_margin: Option<f32>,
}

impl Default for WindowDecorationConfig {
    fn default() -> Self {
        Self::detect()
    }
}

impl WindowDecorationConfig {
    /// Constructs a decoration config based on system detection and custom overrides.
    pub fn from_custom(
        preset_style: Option<WindowControlStyle>,
        position: Option<WindowButtonPosition>,
        custom_gap: Option<f32>,
        custom_margin: Option<f32>,
    ) -> Self {
        let mut base = Self::detect();

        if let Some(style) = preset_style {
            base.style = style;
        }

        if let Some(pos) = position {
            base.position = pos;
            match pos {
                WindowButtonPosition::Left => {
                    base.buttons = smallvec![
                        WindowControlType::Close,
                        WindowControlType::Minimize,
                        WindowControlType::Maximize,
                    ];
                }
                WindowButtonPosition::Right => {
                    base.buttons = smallvec![
                        WindowControlType::Minimize,
                        WindowControlType::Maximize,
                        WindowControlType::Close,
                    ];
                }
            }
        }

        base.custom_gap = custom_gap;
        base.custom_margin = custom_margin;
        base
    }

    pub fn effective_gap(&self, cx: &App) -> gpui::Pixels {
        let scale = crate::tokens::ui_scale_factor(cx);
        if let Some(g) = self.custom_gap {
            gpui::px(g * scale)
        } else {
            gpui::px(get_window_control_button_gap(cx) * scale)
        }
    }

    pub fn effective_margin(&self, cx: &App) -> gpui::Pixels {
        let scale = crate::tokens::ui_scale_factor(cx);
        if let Some(m) = self.custom_margin {
            gpui::px(m * scale)
        } else {
            gpui::px(get_window_control_margin(cx) * scale)
        }
    }

    /// Detects the native window decoration config for the current operating system / desktop environment.
    pub fn detect() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self {
                position: WindowButtonPosition::Right,
                buttons: smallvec![
                    WindowControlType::Minimize,
                    WindowControlType::Maximize,
                    WindowControlType::Close,
                ],
                style: WindowControlStyle::Windows11,
                custom_gap: None,
                custom_margin: None,
            }
        }

        #[cfg(target_os = "macos")]
        {
            Self {
                position: WindowButtonPosition::Left,
                buttons: smallvec![
                    WindowControlType::Close,
                    WindowControlType::Minimize,
                    WindowControlType::Maximize,
                ],
                style: WindowControlStyle::MacOS,
                custom_gap: None,
                custom_margin: None,
            }
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            Self::detect_linux()
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    fn detect_linux() -> Self {
        // Try KDE kwinrc configuration first
        if let Some(config) = Self::detect_kde() {
            return config;
        }

        // Try GNOME gsettings / button-layout next
        if let Some(config) = Self::detect_gnome() {
            return config;
        }

        // Fallback default for Linux (Right side: Minimize, Maximize, Close)
        Self {
            position: WindowButtonPosition::Right,
            buttons: smallvec![
                WindowControlType::Minimize,
                WindowControlType::Maximize,
                WindowControlType::Close,
            ],
            style: WindowControlStyle::LinuxCSD,
            custom_gap: None,
            custom_margin: None,
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    fn detect_kde() -> Option<Self> {
        let home = env::var("HOME").ok()?;
        let kwinrc_path = format!("{}/.config/kwinrc", home);
        let content = fs::read_to_string(kwinrc_path).ok()?;

        let mut in_kdec = false;
        let mut buttons_left = String::new();
        let mut buttons_right = String::new();

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                in_kdec = line.eq_ignore_ascii_case("[org.kde.kdecoration2]");
                continue;
            }

            if in_kdec {
                if let Some((key, val)) = line.split_once('=') {
                    let key = key.trim();
                    let val = val.trim();
                    if key.eq_ignore_ascii_case("ButtonsOnLeft") {
                        buttons_left = val.to_string();
                    } else if key.eq_ignore_ascii_case("ButtonsOnRight") {
                        buttons_right = val.to_string();
                    }
                }
            }
        }

        let parse_kwin_buttons = |s: &str| -> SmallVec<[WindowControlType; 4]> {
            let mut res = smallvec![];
            for ch in s.chars() {
                match ch {
                    'I' => res.push(WindowControlType::Minimize),
                    'A' => res.push(WindowControlType::Maximize),
                    'X' => res.push(WindowControlType::Close),
                    _ => {}
                }
            }
            res
        };

        let left_controls = parse_kwin_buttons(&buttons_left);
        let right_controls = parse_kwin_buttons(&buttons_right);

        if !left_controls.is_empty()
            && (right_controls.is_empty() || left_controls.contains(&WindowControlType::Close))
        {
            Some(Self {
                position: WindowButtonPosition::Left,
                buttons: left_controls,
                style: WindowControlStyle::KDEBreeze,
                custom_gap: None,
                custom_margin: None,
            })
        } else if !right_controls.is_empty() {
            Some(Self {
                position: WindowButtonPosition::Right,
                buttons: right_controls,
                style: WindowControlStyle::KDEBreeze,
                custom_gap: None,
                custom_margin: None,
            })
        } else {
            None
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    fn detect_gnome() -> Option<Self> {
        let output = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.wm.preferences", "button-layout"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let layout_str = stdout.trim().trim_matches('\'');

        let (left_str, right_str) = layout_str.split_once(':')?;

        let parse_gnome_buttons = |s: &str| -> SmallVec<[WindowControlType; 4]> {
            let mut res = smallvec![];
            for part in s.split(',') {
                match part.trim() {
                    "minimize" => res.push(WindowControlType::Minimize),
                    "maximize" => res.push(WindowControlType::Maximize),
                    "close" => res.push(WindowControlType::Close),
                    _ => {}
                }
            }
            res
        };

        let left_controls = parse_gnome_buttons(left_str);
        let right_controls = parse_gnome_buttons(right_str);

        if !left_controls.is_empty() {
            Some(Self {
                position: WindowButtonPosition::Left,
                buttons: left_controls,
                style: WindowControlStyle::LinuxCSD,
                custom_gap: None,
                custom_margin: None,
            })
        } else if !right_controls.is_empty() {
            Some(Self {
                position: WindowButtonPosition::Right,
                buttons: right_controls,
                style: WindowControlStyle::LinuxCSD,
                custom_gap: None,
                custom_margin: None,
            })
        } else {
            None
        }
    }
}
