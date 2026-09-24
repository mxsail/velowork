//! Elevation shadow tokens for floating popovers, menus, and preview overlays.

use gpui::{hsla, point, px, BoxShadow};

/// Apple macOS / Linear 风格的高层级浮层紧凑细腻微投影 (Elevation Dropdown / Popover Shadow)。
///
/// 采用双层紧凑物理光影模型，彻底消除发虚和大面积外溢：
/// 1. 近景接触微轮廓 (Ambient Contact)：`offset: (0, 1px), blur: 2px, alpha: 0.25`
///    紧贴卡片边缘，提供清晰锐利的边界分离，使浮层与背景自然脱开；
/// 2. 悬浮紧致投影 (Key Elevation)：`offset: (0, 2px), blur: 5px, alpha: 0.18`
///    向下投射小巧克制的细腻投影，水平外延严格控制在 4px 以内，绝不形成大片暗晕。
///
/// 适用于菜单下拉 (PopupMenu)、搜索下拉 (OverlayMenu)、选择器弹层 (Select Popover) 及标签预览弹窗 (TerminalPreview)。
pub fn elevation_menu_shadow() -> Vec<BoxShadow> {
    vec![
        // 近景接触轮廓层：小模糊、紧贴边缘
        BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.25),
            offset: point(px(0.0), px(1.0)),
            blur_radius: px(2.0),
            spread_radius: px(0.0),
            inset: false,
        },
        // 悬浮紧致投影层：向下微投影，水平四周严格收敛
        BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.18),
            offset: point(px(0.0), px(2.0)),
            blur_radius: px(5.0),
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
        assert_eq!(shadows[0].blur_radius, px(2.0));
        assert_eq!(shadows[1].blur_radius, px(5.0));
    }
}
