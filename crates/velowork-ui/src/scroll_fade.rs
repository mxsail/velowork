use gpui::*;
use std::rc::Rc;
use crate::scrollable::ScrollbarHandle;

/// Dynamic scroll fade edge overlay.
///
/// Automatically adds a smooth top/bottom fade gradient that makes content appear
/// to "fade into the background" when scrolled, without hard clipping.
///
/// - Top fade shows only when content can scroll up (`offset.y > 1.0px`).
/// - Bottom fade shows only when content can scroll down (`offset.y + viewport_h < content_h - 1.0px`).
/// - When content fits within the viewport or reaches the boundary, the corresponding fade is hidden.
/// - Hitboxes are NOT inserted: 100% mouse event pass-through (clicks, selection, scroll wheel).
pub struct ScrollFade {
    scroll_handle: Rc<dyn ScrollbarHandle>,
    shadow_color: Option<Hsla>,
    fade_size: Pixels,
    max_opacity: Option<f32>,
}

impl ScrollFade {
    /// Create a new vertical elevation drop shadow with an explicit base shadow color.
    pub fn vertical<H: ScrollbarHandle + Clone>(handle: &H, _unused_color: impl Into<Hsla>) -> Self {
        Self {
            scroll_handle: Rc::new(handle.clone()),
            shadow_color: None,
            fade_size: px(8.0),
            max_opacity: None,
        }
    }

    /// Create a new vertical elevation drop shadow overlay.
    pub fn vertical_themed<H: ScrollbarHandle + Clone>(handle: &H) -> Self {
        Self {
            scroll_handle: Rc::new(handle.clone()),
            shadow_color: None,
            fade_size: px(8.0),
            max_opacity: None,
        }
    }

    /// Set an explicit shadow color (defaults to theme-adapted ambient black).
    pub fn shadow_color(mut self, color: impl Into<Hsla>) -> Self {
        self.shadow_color = Some(color.into());
        self
    }

    /// Customize the shadow spread height (default 8px).
    pub fn fade_size(mut self, size: impl Into<Pixels>) -> Self {
        self.fade_size = size.into();
        self
    }

    /// Customize the peak opacity factor (defaults to 0.28 on dark themes, 0.09 on light themes).
    pub fn max_opacity(mut self, opacity: f32) -> Self {
        self.max_opacity = Some(opacity.clamp(0.0, 1.0));
        self
    }
}

impl IntoElement for ScrollFade {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ScrollFade {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.inset = Edges {
            top: px(0.0).into(),
            left: px(0.0).into(),
            bottom: px(0.0).into(),
            right: px(0.0).into(),
        };
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        // Deliberately do NOT insert any hitbox.
        // This guarantees 100% mouse event pass-through (pointer-events: none).
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let container_h = bounds.size.height;
        if container_h < px(4.0) {
            return;
        }

        let scroll_size = self.scroll_handle.content_size();
        let scroll_pos = self.scroll_handle.offset();
        let content_h = scroll_size.height;
        // In GPUI, vertical scroll offset is negative when content scrolls down (e.g. -50px).
        // Taking the absolute value gives the distance scrolled down from the top.
        let scroll_y = scroll_pos.y.abs();

        let can_scroll_up = scroll_y > px(0.5);
        let dist_to_bottom = (content_h - (scroll_y + container_h)).max(px(0.0));
        let can_scroll_down = dist_to_bottom > px(0.5);

        if !can_scroll_up && !can_scroll_down {
            return;
        }

        let effective_fade = self.fade_size.min(container_h / 2.0);
        if effective_fade <= px(1.0) {
            return;
        }

        // 极重要：半透明防护原则。
        // 当窗口背景透明度小于 100%（例如 0.46 等半透明/毛玻璃状态）时，在已有透明图层上绘制任何渐变遮罩，
        // 都会发生二次 Alpha 复合（Alpha Compounding），导致渐变区与底部工具栏/外围容器交界处出现明显的矩形深浅分界线。
        // 因此严格遵守：仅当透明度为 100%（完全不透明实心背景）时开启滚动边缘淡出；只要存在透明度，立即自动静默关闭遮罩，
        // 确保毛玻璃透明质感纯净通透、零色块、零硬边。
        let global_op = crate::theme::bg_opacity(_cx);
        if global_op < 1.0 {
            return;
        }

        let t = crate::theme::theme(_cx);
        let shadow = self.shadow_color.unwrap_or(hsla(0.0, 0.0, 0.0, 1.0));
        let default_peak = if t.is_dark() { 0.28 } else { 0.09 };
        let peak = self.max_opacity.unwrap_or(default_peak);

        // 动态距离平滑缓冲 (Hermite Smoothstep Ramp):
        // 临界感知区间为 16px。在 0~16px 之间，投影因数由 0.0 平滑升至 1.0，
        // 触底或触顶时投影完全消退蒸发，内容与工具栏自然贴合平齐。
        let ramp_distance = px(16.0);
        let top_ramp = if can_scroll_up {
            let r = (scroll_y / ramp_distance).clamp(0.0, 1.0);
            r * r * (3.0 - 2.0 * r)
        } else {
            0.0
        };
        let bottom_ramp = if can_scroll_down {
            let r = (dist_to_bottom / ramp_distance).clamp(0.0, 1.0);
            r * r * (3.0 - 2.0 * r)
        } else {
            0.0
        };

        // 方案 A：环境光景深悬浮微投影（Elevation Drop Shadow）
        // 彻底摆脱“在正文上方盖半透明色块”的弊病，文字颜色保持 100% 清晰锐利，
        // 仅在顶栏/底栏接缝处投射一道极其幽微的 8px 漫反射微阴影，完美呈现纸张滑入托盘的物理隐喻。
        if can_scroll_up && top_ramp > 0.001 {
            let top_bounds = Bounds::new(
                bounds.origin,
                size(bounds.size.width, effective_fade),
            );
            window.paint_quad(fill(
                top_bounds,
                linear_gradient(
                    180.0,
                    linear_color_stop(shadow.alpha(peak * top_ramp), 0.0),
                    linear_color_stop(shadow.alpha(0.0), 1.0),
                ),
            ));
        }

        if can_scroll_down && bottom_ramp > 0.001 {
            let bottom_y = bounds.origin.y + container_h - effective_fade;
            let bottom_bounds = Bounds::new(
                point(bounds.origin.x, bottom_y),
                size(bounds.size.width, effective_fade),
            );
            window.paint_quad(fill(
                bottom_bounds,
                linear_gradient(
                    0.0,
                    linear_color_stop(shadow.alpha(peak * bottom_ramp), 0.0),
                    linear_color_stop(shadow.alpha(0.0), 1.0),
                ),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smoothstep_bounds() {
        let ramp_distance = 20.0f32;
        let compute_ramp = |val: f32| -> f32 {
            let r = (val / ramp_distance).clamp(0.0, 1.0);
            r * r * (3.0 - 2.0 * r)
        };

        assert_eq!(compute_ramp(0.0), 0.0);
        assert!((compute_ramp(10.0) - 0.5).abs() < 1e-4);
        assert_eq!(compute_ramp(20.0), 1.0);
        assert_eq!(compute_ramp(50.0), 1.0);
        assert_eq!(compute_ramp(-10.0), 0.0);
    }
}
