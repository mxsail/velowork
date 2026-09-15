use gpui::{
    canvas, point, px, App, Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, Styled, Window,
};

use crate::design::semantic::SemanticPalette;

/// 环状进度指示条组件（Progress Ring）
///
/// 遵循 GPUI 矢量绘制范式，基于 `PathBuilder` 抗锯齿路径在 `canvas` 中精确实时渲染。
/// 具备两极退化保护（0% 与 100% 重合保护）、描边外缘边界防裁切以及分级三态语义色感知。
pub struct ProgressRing {
    progress: f32,
    size: Pixels,
    stroke_width: Pixels,
    track_color: Option<Hsla>,
    progress_color: Option<Hsla>,
}

impl ProgressRing {
    /// 创建新的环状指示条，传入当前进度比例（0.0 ~ 1.0，支持超过 1.0 的超标比例）
    pub fn new(progress: f32) -> Self {
        Self {
            progress,
            size: px(16.0),
            stroke_width: px(2.0),
            track_color: None,
            progress_color: None,
        }
    }

    /// 设置圆环外框尺寸（正方形宽高，默认 16px）
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// 设置描边线宽（默认 2.0px）
    pub fn stroke_width(mut self, stroke_width: Pixels) -> Self {
        self.stroke_width = stroke_width;
        self
    }

    /// 显式覆盖底轨颜色（未指定时默认使用 `p.border_subtle`）
    pub fn track_color(mut self, color: Hsla) -> Self {
        self.track_color = Some(color);
        self
    }

    /// 显式覆盖进度弧颜色（未指定时根据进度自适应三态语义状态色）
    pub fn progress_color(mut self, color: Hsla) -> Self {
        self.progress_color = Some(color);
        self
    }
}

impl IntoElement for ProgressRing {
    type Element = gpui::AnyElement;

    fn into_element(self) -> Self::Element {
        let progress = self.progress;
        let size = self.size;
        let stroke_width = self.stroke_width;
        let explicit_track = self.track_color;
        let explicit_prog = self.progress_color;

        canvas(
            move |_bounds, _window, _cx| (),
            move |bounds: Bounds<Pixels>, (), window: &mut Window, cx: &mut App| {
                let p = SemanticPalette::from_context(cx);

                let track_color = explicit_track.unwrap_or(p.border_subtle);
                let progress_color = explicit_prog.unwrap_or_else(|| {
                    if progress > 0.90 {
                        p.status_error
                    } else if progress > 0.75 {
                        p.status_warning
                    } else {
                        p.status_info
                    }
                });

                let center_x = bounds.origin.x + bounds.size.width / 2.0;
                let center_y = bounds.origin.y + bounds.size.height / 2.0;
                let center = point(center_x, center_y);

                let d = bounds.size.width.min(bounds.size.height).min(size);
                // 有效半径 r = (d - stroke_width) / 2，确保外扩半线宽后绝不溢出裁剪
                let r = (d - stroke_width).max(px(1.0)) / 2.0;

                // 1. 绘制 360° 背景底轨
                if let Some(path) = build_full_circle_path(center, r, stroke_width) {
                    window.paint_path(path, track_color);
                }

                // 2. 绘制前景进度弧（具备退化保护）
                let clamped_p = progress.clamp(0.0, 1.0);
                if clamped_p >= 0.999 {
                    // 满环（双半圆闭合环）
                    if let Some(path) = build_full_circle_path(center, r, stroke_width) {
                        window.paint_path(path, progress_color);
                    }
                } else if clamped_p > 0.001 {
                    // 部分进度弧
                    if let Some(path) = build_arc_path(center, r, stroke_width, clamped_p) {
                        window.paint_path(path, progress_color);
                    }
                }
            },
        )
        .size(size)
        .into_any_element()
    }
}

/// 构建闭合完整圆环路径（由两段 180° 半圆弧构成，防止起点与终点重合退化）
pub fn build_full_circle_path(
    center: Point<Pixels>,
    r: Pixels,
    stroke_width: Pixels,
) -> Option<gpui::Path<Pixels>> {
    let mut builder = PathBuilder::stroke(stroke_width);
    let top = point(center.x, center.y - r);
    let bottom = point(center.x, center.y + r);
    let radii = point(r, r);

    builder.move_to(top);
    builder.arc_to(radii, px(0.0), false, true, bottom);
    builder.arc_to(radii, px(0.0), false, true, top);
    builder.build().ok()
}

/// 从 12 点钟方向顺时针构建进度弧路径
pub fn build_arc_path(
    center: Point<Pixels>,
    r: Pixels,
    stroke_width: Pixels,
    progress: f32,
) -> Option<gpui::Path<Pixels>> {
    let mut builder = PathBuilder::stroke(stroke_width);
    let top = point(center.x, center.y - r);
    let radii = point(r, r);

    // 起点位于 12 点钟（-π/2），顺时针扫掠角度 θ = 2π * progress
    let angle = (progress * std::f32::consts::TAU) - std::f32::consts::FRAC_PI_2;
    let r_val = f32::from(r);
    let end_x = center.x + px(r_val * angle.cos());
    let end_y = center.y + px(r_val * angle.sin());
    let end_point = point(end_x, end_y);

    let large_arc = progress > 0.5;

    builder.move_to(top);
    builder.arc_to(radii, px(0.0), large_arc, true, end_point);
    builder.build().ok()
}

/// 将 token 数量格式化为带 k 单位的易读字符串（满整千省略小数，非整千保留 1 位小数）
pub fn format_token_count(tokens: usize) -> String {
    if tokens < 1000 {
        tokens.to_string()
    } else {
        let k = tokens as f64 / 1000.0;
        if tokens % 1000 == 0 {
            format!("{}k", tokens / 1000)
        } else {
            format!("{:.1}k", k)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_ring_path_builders() {
        let center = point(px(50.0), px(50.0));
        let r = px(20.0);
        let stroke = px(2.0);

        // 1. 完整圆路径构建无 panic
        let full_path = build_full_circle_path(center, r, stroke);
        assert!(full_path.is_some(), "full circle path should build successfully");

        // 2. 各象限进度弧构建无 panic
        for p in [0.1, 0.25, 0.5, 0.75, 0.9] {
            let arc_path = build_arc_path(center, r, stroke, p);
            assert!(arc_path.is_some(), "arc path for progress {} should build successfully", p);
        }
    }

    #[test]
    fn test_format_token_count() {
        assert_eq!(format_token_count(0), "0");
        assert_eq!(format_token_count(350), "350");
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(1000), "1k");
        assert_eq!(format_token_count(1200), "1.2k");
        assert_eq!(format_token_count(89900), "89.9k");
        assert_eq!(format_token_count(128000), "128k");
        assert_eq!(format_token_count(8000000), "8000k");
    }
}
