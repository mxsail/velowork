use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

impl SplitDirection {
    /// Return the perpendicular direction (Horizontal <-> Vertical).
    pub fn flipped(self) -> Self {
        match self {
            SplitDirection::Horizontal => SplitDirection::Vertical,
            SplitDirection::Vertical => SplitDirection::Horizontal,
        }
    }
}

/// Dock container position in the workspace layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DockPosition {
    Left,
    Right,
    Bottom,
    Top,
}

/// 滚动条显隐模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScrollbarShow {
    /// 滚动时显示，空闲后淡出（默认）。
    #[default]
    Scrolling,
    /// 悬停时显示。
    Hover,
    /// 始终显示。
    Always,
    /// 从不显示（隐藏）。
    Never,
}

impl ScrollbarShow {
    pub fn is_hover(&self) -> bool {
        matches!(self, Self::Hover)
    }

    pub fn is_always(&self) -> bool {
        matches!(self, Self::Always)
    }

    pub fn is_never(&self) -> bool {
        matches!(self, Self::Never)
    }

    pub fn all_variants() -> &'static [ScrollbarShow] {
        &[
            ScrollbarShow::Scrolling,
            ScrollbarShow::Hover,
            ScrollbarShow::Always,
            ScrollbarShow::Never,
        ]
    }

    pub fn translation_key(&self) -> &'static str {
        match self {
            ScrollbarShow::Scrolling => "settings.scrollbar_show_scrolling",
            ScrollbarShow::Hover => "settings.scrollbar_show_hover",
            ScrollbarShow::Always => "settings.scrollbar_show_always",
            ScrollbarShow::Never => "settings.scrollbar_show_never",
        }
    }
}

/// Terminal cursor shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorShape {
    /// Full-cell block cursor (Linux-style)
    Block,
    /// Thin vertical bar cursor (editor-style, default)
    #[default]
    Bar,
    /// Horizontal underline cursor
    Underline,
}

impl CursorShape {
    pub fn display_name(self) -> &'static str {
        match self {
            CursorShape::Block => "Block",
            CursorShape::Bar => "Bar",
            CursorShape::Underline => "Underline",
        }
    }

    pub fn translation_key(self) -> &'static str {
        match self {
            CursorShape::Block => "settings.cursor_shape.block",
            CursorShape::Bar => "settings.cursor_shape.bar",
            CursorShape::Underline => "settings.cursor_shape.underline",
        }
    }

    pub fn all_variants() -> &'static [CursorShape] {
        &[CursorShape::Block, CursorShape::Bar, CursorShape::Underline]
    }
}

/// 终端 Bell 提示方式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BellStyle {
    /// 视觉提示：终端边框黄色闪烁（默认）
    #[default]
    Visual,
    /// 听觉提示：发出系统提示音
    Audible,
    /// 视觉与听觉兼备
    Both,
    /// 禁用：忽略 Bell 事件
    Disabled,
}

impl BellStyle {
    pub const ALL: &'static [BellStyle] = &[
        BellStyle::Visual,
        BellStyle::Audible,
        BellStyle::Both,
        BellStyle::Disabled,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            BellStyle::Visual => "Visual",
            BellStyle::Audible => "Audible",
            BellStyle::Both => "Both",
            BellStyle::Disabled => "Disabled",
        }
    }

    pub fn title_key(self) -> &'static str {
        match self {
            BellStyle::Visual => "ssh.terminal.bell_style_visual",
            BellStyle::Audible => "ssh.terminal.bell_style_audible",
            BellStyle::Both => "ssh.terminal.bell_style_both",
            BellStyle::Disabled => "ssh.terminal.bell_style_disabled",
        }
    }

    pub fn all_variants() -> &'static [BellStyle] {
        Self::ALL
    }
}
