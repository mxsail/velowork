//! Elevation shadow tokens for floating popovers, menus, and preview overlays.

use gpui::{hsla, point, px, BoxShadow};

/// Apple macOS / Human Interface Guidelines 风格的高层级浮层精细凝聚光影 (Elevation Dropdown / Popover Shadow)。
///
/// 采用双层物理光影模型：
/// 1. 近景接触轮廓层 (Ambient & Contact Shadow)：`offset: (0, 1px), blur: 3px, alpha: 0.35`
///    紧贴卡片边缘，提供高对比度的边界分离，让卡片立刻与背景分离开，清晰锐利；
/// 2. 纵深立体主投影 (Key Light Elevation Shadow)：`offset: (0, 6px), blur: 12px, alpha: 0.24`
///    垂直向下投影，收紧水平横向漫散，呈现沉稳扎实的悬浮高度，杜绝阴影外溢窗口。
///
/// 适用于菜单下拉 (PopupMenu)、搜索下拉 (OverlayMenu)、选择器弹层 (Select Popover) 及标签预览弹窗 (TerminalPreview)。
pub fn elevation_menu_shadow() -> Vec<BoxShadow> {
    vec![
        // 近景接触轮廓层：高对比度、小模糊，立刻从背景中凸显轮廓
        BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.35),
            offset: point(px(0.0), px(1.0)),
            blur_radius: px(3.0),
            spread_radius: px(0.0),
            inset: false,
        },
        // 纵深立体投影层：向下投射优雅沉稳的主阴影
        BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.24),
            offset: point(px(0.0), px(6.0)),
            blur_radius: px(12.0),
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
        assert_eq!(shadows[0].blur_radius, px(3.0));
        assert_eq!(shadows[1].blur_radius, px(12.0));
    }
}
