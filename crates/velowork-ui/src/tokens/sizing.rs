//! Standard dialog / overlay width tokens.
//!
//! Every overlay that needs a fixed width should pull from these tokens instead
//! of writing a bare `px(720.0)`. Centralizing the values means a global visual
//! change (e.g. "make all dialogs a touch wider") is a one-line edit.
//!
//! The unscaled `DIALOG_*` constants are the source of truth; the `dialog_*`
//! helpers apply the global UI zoom so the values stay consistent with the rest
//! of the token system.

use gpui::{App, Pixels, px};

/// Compact overlay (e.g. small confirmations, quick pickers).
pub const DIALOG_SM: Pixels = px(360.0);
/// Default overlay width (most forms / dialogs).
pub const DIALOG_MD: Pixels = px(480.0);
/// Large overlay (settings, session editor).
pub const DIALOG_LG: Pixels = px(640.0);
/// Extra-large overlay (settings with side panel, diff viewer chrome).
pub const DIALOG_XL: Pixels = px(860.0);

pub fn dialog_sm(cx: &App) -> Pixels {
    px(360.0 * super::scale::ui_zoom_factor(cx))
}
pub fn dialog_md(cx: &App) -> Pixels {
    px(480.0 * super::scale::ui_zoom_factor(cx))
}
pub fn dialog_lg(cx: &App) -> Pixels {
    px(640.0 * super::scale::ui_zoom_factor(cx))
}
pub fn dialog_xl(cx: &App) -> Pixels {
    px(860.0 * super::scale::ui_zoom_factor(cx))
}

/// Max height of a scrollable overlay list (palette / menu / dropdown results).
///
/// Lists are capped instead of growing with the result count so a palette never
/// covers the whole window.
pub const OVERLAY_LIST_MAX_H: Pixels = px(360.0);
/// Edge length of a color swatch in the color picker grid.
pub const SWATCH_SIZE: Pixels = px(24.0);
/// Edge length of one day cell in the date picker grid.
pub const DATE_CELL_SIZE: Pixels = px(32.0);

pub fn overlay_list_max_h(cx: &App) -> Pixels {
    px(360.0 * super::scale::ui_zoom_factor(cx))
}
pub fn swatch_size(cx: &App) -> Pixels {
    px(24.0 * super::scale::ui_zoom_factor(cx))
}
pub fn date_cell_size(cx: &App) -> Pixels {
    px(32.0 * super::scale::ui_zoom_factor(cx))
}

// =============================================================================
// Control Heights — aligned with ControlSize (Compact: 24px, Default: 28px, Large: 36px)
// =============================================================================

/// Small / Compact control height (24px) - dense inputs, compact buttons, tree items
pub const CONTROL_HEIGHT_SM: Pixels = px(24.0);
/// Default control height (28px) - standard text inputs, selects, buttons
pub const CONTROL_HEIGHT_MD: Pixels = px(28.0);
/// Large control height (36px) - hero buttons, prominent inputs
pub const CONTROL_HEIGHT_LG: Pixels = px(36.0);

pub fn control_height_sm(cx: &App) -> Pixels {
    px(24.0 * super::scale::ui_zoom_factor(cx))
}

pub fn control_height_md(cx: &App) -> Pixels {
    px(28.0 * super::scale::ui_zoom_factor(cx))
}

pub fn control_height_lg(cx: &App) -> Pixels {
    px(36.0 * super::scale::ui_zoom_factor(cx))
}

/// Dialog / Modal footer bar height (48px)
pub const DIALOG_FOOTER_HEIGHT: Pixels = px(48.0);

pub fn dialog_footer_height(cx: &App) -> Pixels {
    px(48.0 * super::scale::ui_zoom_factor(cx))
}

// =============================================================================
// Component Heights (Chips, Bars)
// =============================================================================

/// Compact chip / indicator height (18px)
pub const HEIGHT_CHIP: Pixels = px(18.0);
/// Status bar height (24px) - bottom status strip
pub const HEIGHT_STATUS_BAR: Pixels = px(24.0);
/// Right vertical icon toolbar strip width (30px)
pub const RIGHT_TOOLBAR_WIDTH: Pixels = px(30.0);

pub fn ui_height_chip(cx: &App) -> Pixels {
    px(18.0 * super::scale::ui_zoom_factor(cx))
}

pub fn ui_height_status_bar(cx: &App) -> Pixels {
    px(24.0 * super::scale::ui_zoom_factor(cx))
}

pub fn ui_right_toolbar_width(cx: &App) -> Pixels {
    px(30.0 * super::scale::ui_zoom_factor(cx))
}

// =============================================================================
// Titlebar Sizing
// =============================================================================

/// macOS titlebar left offset
pub const TITLEBAR_MAC_OFFSET: Pixels = px(120.0);

// =============================================================================
// Dropdown & Select Sizing
// =============================================================================

/// Compact dropdown width (e.g. font weight, font style) (140px)
pub const SELECT_WIDTH_SM: Pixels = px(140.0);
/// Standard dropdown width (e.g. shell, session backend) (180px)
pub const SELECT_WIDTH_MD: Pixels = px(180.0);
/// Wide / Adaptive dropdown min width (220px)
pub const SELECT_MIN_WIDTH_ADAPTIVE: Pixels = px(220.0);
/// Wide / Adaptive dropdown max width (340px)
pub const SELECT_MAX_WIDTH_ADAPTIVE: Pixels = px(340.0);

/// Popover list item height (28px)
pub const POPOVER_LIST_ITEM_H: Pixels = CONTROL_HEIGHT_SM;
/// Popover list group header height (24px)
pub const POPOVER_LIST_HEADER_H: Pixels = HEIGHT_STATUS_BAR;
/// Popover list min height (40px)
pub const POPOVER_LIST_MIN_H: Pixels = CONTROL_HEIGHT_LG;
/// Popover list max height (280px)
pub const POPOVER_LIST_MAX_H: Pixels = px(280.0);
/// Base menu offset for CSD titlebar
pub const TITLEBAR_MENU_OFFSET: Pixels = px(48.0);

/// Bottom spacer for categorized long-scroll containers (settings panel, session dialog),
/// ensuring the last category card can always scroll fully to the top of the viewport.
pub const SCROLL_BOTTOM_SPACER_H: Pixels = px(480.0);

// ─── Scrollbar Tokens (macOS / Zed 精致微浮动风) ───

/// 滚动条纵向容器热区宽度 (10px)
pub const SCROLLBAR_TRACK_WIDTH: Pixels = px(10.0);
/// 滚动条横向容器热区高度 (8px)
pub const SCROLLBAR_TRACK_HEIGHT_HORIZONTAL: Pixels = px(8.0);
/// Popover scrollbar container width
pub const POPOVER_SCROLLBAR_W: Pixels = SCROLLBAR_TRACK_WIDTH;
/// 滑块常态厚度 (4px)
pub const SCROLLBAR_THUMB_WIDTH: Pixels = px(4.0);
/// 滑块交互/悬停态厚度 (6px)
pub const SCROLLBAR_THUMB_ACTIVE_WIDTH: Pixels = px(6.0);
/// 滑块常态圆角 (2px)
pub const SCROLLBAR_THUMB_RADIUS: Pixels = px(2.0);
/// 滑块交互态圆角 (3px)
pub const SCROLLBAR_THUMB_ACTIVE_RADIUS: Pixels = px(3.0);
/// 滑块外边缘留白边距 (2px)
pub const SCROLLBAR_THUMB_INSET: Pixels = px(2.0);
/// 滑块最小视觉长度 (32px)
pub const SCROLLBAR_MIN_THUMB_SIZE: f32 = 32.0;

/// 滚动条常态透明度 (0.35)
pub const SCROLLBAR_ALPHA_NORMAL: f32 = 0.35;
/// 滚动条悬停透明度 (0.60)
pub const SCROLLBAR_ALPHA_HOVER: f32 = 0.60;
/// 滚动条拖拽透明度 (0.80)
pub const SCROLLBAR_ALPHA_DRAG: f32 = 0.80;
