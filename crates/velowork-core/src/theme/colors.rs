use super::types::FolderColor;

/// Theme colors - all UI colors in one struct
#[derive(Clone, Copy, Debug)]
pub struct ThemeColors {
    // Background colors
    //
    // ── bg_secondary vs bg_panel 判断标准（唯一权威规则）──
    // 开发时只问一句："这个区域是不是从主工作区独立出来的？"
    //   不是（在主工作区内部，是卡片/控件/侧栏等） → bg_secondary
    //   是（独立面板/侧边栏/停靠区/对话框外壳，自身就是一层壳）→ bg_panel
    // 其余：bg_primary=最底层主工作区；bg_header=条状头部(标题栏/标签栏)；
    //      bg_hover=交互悬停；bg_selection=选中态（蓝调）。
    pub bg_primary: u32,
    pub bg_secondary: u32,
    pub bg_header: u32,
    pub bg_panel: u32,
    pub bg_selection: u32,
    pub bg_hover: u32,

    // Semantic accent (design spec: #5B6BD6, hover #6E7BF0)
    pub accent: u32,

    // Border colors
    pub border: u32,
    pub border_active: u32,

    // Text colors
    pub text_primary: u32,
    pub text_secondary: u32,
    pub text_muted: u32,

    // Status colors
    pub success: u32,
    pub warning: u32,
    pub error: u32,

    // Folder colors (12 distinct colors for project folders)
    pub folder_default: u32,
    pub folder_red: u32,
    pub folder_orange: u32,
    pub folder_yellow: u32,
    pub folder_lime: u32,
    pub folder_green: u32,
    pub folder_teal: u32,
    pub folder_cyan: u32,
    pub folder_blue: u32,
    pub folder_indigo: u32,
    pub folder_purple: u32,
    pub folder_pink: u32,
}

/// Default dark theme (refined dark palette)
pub const DARK_THEME: ThemeColors = ThemeColors {
    bg_primary: 0x0f1015,
    bg_secondary: 0x181920,
    bg_header: 0x14151b,
    bg_panel: 0x14151b,
    bg_selection: 0x312e81,
    bg_hover: 0x1f2029,
    accent: 0x6366F1,
    border: 0x272835,
    border_active: 0x6366F1,
    text_primary: 0xededf0,
    text_secondary: 0x9da3ae,
    text_muted: 0x636979,
    success: 0x34d399,
    warning: 0xfbbf24,
    error: 0xf87171,
    folder_default: 0xfbbf24,
    folder_red: 0xf87171,
    folder_orange: 0xfb923c,
    folder_yellow: 0xfacc15,
    folder_lime: 0xa3e635,
    folder_green: 0x34d399,
    folder_teal: 0x2dd4bf,
    folder_cyan: 0x38bdf8,
    folder_blue: 0x60a5fa,
    folder_indigo: 0x818cf8,
    folder_purple: 0xc084fc,
    folder_pink: 0xf472b6,
};

/// Default light theme (refined light palette)
pub const LIGHT_THEME: ThemeColors = ThemeColors {
    bg_primary: 0xffffff,
    bg_secondary: 0xf8fafc,
    bg_header: 0xf1f5f9,
    bg_panel: 0xf1f5f9,
    bg_selection: 0xc7d2fe,
    bg_hover: 0xe2e8f0,
    accent: 0x4f46e5,
    border: 0xe2e8f0,
    border_active: 0x4f46e5,
    text_primary: 0x0f172a,
    text_secondary: 0x475569,
    text_muted: 0x94a3b8,
    success: 0x059669,
    warning: 0xd97706,
    error: 0xdc2626,
    folder_default: 0xd97706,
    folder_red: 0xdc2626,
    folder_orange: 0xea580c,
    folder_yellow: 0xca8a04,
    folder_lime: 0x65a30d,
    folder_green: 0x16a34a,
    folder_teal: 0x0d9488,
    folder_cyan: 0x0891b2,
    folder_blue: 0x2563eb,
    folder_indigo: 0x4f46e5,
    folder_purple: 0x7c3aed,
    folder_pink: 0xdb2777,
};

/// Pastel Dark theme (Ghostty Builtin Pastel Dark)
pub const PASTEL_DARK_THEME: ThemeColors = ThemeColors {
    bg_primary: 0x1a1a1a,
    bg_secondary: 0x222222,
    bg_header: 0x1a1a1a,
    bg_panel: 0x2d2d2d,
    bg_selection: 0x3a4268,
    bg_hover: 0x303030,
    accent: 0x6E7BF0,
    border: 0x404040,
    border_active: 0x96cbfe,
    text_primary: 0xe3e6ee,
    text_secondary: 0x9aa0ac,
    text_muted: 0x646975,
    success: 0xa8ff60,
    warning: 0xffffb6,
    error: 0xff6c60,
    folder_default: 0xe0af68,
    folder_red: 0xf7768e,
    folder_orange: 0xff9e64,
    folder_yellow: 0xe0af68,
    folder_lime: 0xb8e655,
    folder_green: 0x9ece6a,
    folder_teal: 0x2ac3a2,
    folder_cyan: 0x67e8f9,
    folder_blue: 0x7dcfff,
    folder_indigo: 0x7f7ff5,
    folder_purple: 0xbb9af7,
    folder_pink: 0xf472b6,
};

/// High Contrast theme for accessibility
pub const HIGH_CONTRAST_THEME: ThemeColors = ThemeColors {
    bg_primary: 0x000000,
    bg_secondary: 0x0a0a0a,
    bg_header: 0x000000,
    bg_panel: 0x1a1a1a,
    bg_selection: 0x0066cc,
    bg_hover: 0x1a1a1a,
    accent: 0x9AA8FF,
    border: 0x6fc3df,
    border_active: 0x00aaff,
    text_primary: 0xffffff,
    text_secondary: 0xe0e0e0,
    text_muted: 0xb0b0b0,
    success: 0x00ff00,
    warning: 0xffff00,
    error: 0xff0000,
    folder_default: 0xffff00,
    folder_red: 0xff5555,
    folder_orange: 0xffaa00,
    folder_yellow: 0xffff00,
    folder_lime: 0x88ff00,
    folder_green: 0x55ff55,
    folder_teal: 0x00e5cc,
    folder_cyan: 0x55e5ff,
    folder_blue: 0x55aaff,
    folder_indigo: 0x8888ff,
    folder_purple: 0xff55ff,
    folder_pink: 0xff77aa,
};

impl ThemeColors {
    /// Determine if this is a dark theme based on background luminance.
    pub fn is_dark(&self) -> bool {
        let (r, g, b) = Self::hex_to_rgb(self.bg_primary);
        // Relative luminance approximation
        let luminance = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
        luminance < 128.0
    }

    /// Get RGB tuple from a hex color
    pub fn hex_to_rgb(hex: u32) -> (u8, u8, u8) {
        (
            ((hex >> 16) & 0xFF) as u8,
            ((hex >> 8) & 0xFF) as u8,
            (hex & 0xFF) as u8,
        )
    }

    /// Get the actual color value for a folder color option
    pub fn get_folder_color(&self, color: FolderColor) -> u32 {
        match color {
            FolderColor::Default => self.folder_default,
            FolderColor::Red => self.folder_red,
            FolderColor::Orange => self.folder_orange,
            FolderColor::Yellow => self.folder_yellow,
            FolderColor::Lime => self.folder_lime,
            FolderColor::Green => self.folder_green,
            FolderColor::Teal => self.folder_teal,
            FolderColor::Cyan => self.folder_cyan,
            FolderColor::Blue => self.folder_blue,
            FolderColor::Indigo => self.folder_indigo,
            FolderColor::Purple => self.folder_purple,
            FolderColor::Pink => self.folder_pink,
        }
    }
}
