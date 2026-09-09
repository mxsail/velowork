//! Elevation shadow tokens for floating popovers, menus, and preview overlays.

use gpui::{hsla, point, px, BoxShadow};

/// Ant Design / VS Code 风格的高层级浮层双层弥散阴影 (Elevation Dropdown / Popover Shadow)。
///
/// 包含：
/// 1. 大范围柔和环境光漫反射：`offset: (0, 6px), blur: 16px, alpha: 0.28`
/// 2. 近距离轮廓微投影：`offset: (0, 2px), blur: 6px, alpha: 0.16`
///
/// 适用于菜单下拉 (PopupMenu)、搜索下拉 (OverlayMenu)、选择器弹层 (Select Popover) 及标签预览弹窗 (TerminalPreview)。
pub fn elevation_menu_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.28),
            offset: point(px(0.0), px(6.0)),
            blur_radius: px(16.0),
            spread_radius: px(0.0),
            inset: false,
        },
        BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.16),
            offset: point(px(0.0), px(2.0)),
            blur_radius: px(6.0),
            spread_radius: px(0.0),
            inset: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elevation_menu_shadow_structure() {
        let shadows = elevation_menu_shadow();
        assert_eq!(shadows.len(), 2);
        assert_eq!(shadows[0].blur_radius, px(16.0));
        assert_eq!(shadows[1].blur_radius, px(6.0));
    }
}
