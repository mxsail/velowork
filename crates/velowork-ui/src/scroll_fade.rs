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
    bg_color: Option<Hsla>,
    fade_size: Pixels,
}

impl ScrollFade {
    /// Create a new vertical scroll fade overlay with an explicit background color.
    pub fn vertical<H: ScrollbarHandle + Clone>(handle: &H, bg_color: impl Into<Hsla>) -> Self {
        Self {
            scroll_handle: Rc::new(handle.clone()),
            bg_color: Some(bg_color.into()),
            fade_size: px(36.0),
        }
    }

    /// Create a new vertical scroll fade overlay inheriting the contextual theme background.
    pub fn vertical_themed<H: ScrollbarHandle + Clone>(handle: &H) -> Self {
        Self {
            scroll_handle: Rc::new(handle.clone()),
            bg_color: None,
            fade_size: px(36.0),
        }
    }

    /// Customize the fade gradient height (default 36px).
    pub fn fade_size(mut self, size: impl Into<Pixels>) -> Self {
        self.fade_size = size.into();
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

        let can_scroll_up = scroll_y > px(1.0);
        let can_scroll_down = scroll_y + container_h < content_h - px(1.0);

        let effective_fade = self.fade_size.min(container_h / 2.0);
        // 极重要：半透明防护原则。
        // 当窗口背景透明度小于 100%（例如 0.46 等半透明/毛玻璃状态）时，在已有透明图层上绘制任何渐变遮罩，
        // 都会发生二次 Alpha 复合（Alpha Compounding），导致渐变区与底部工具栏/外围容器交界处出现明显的矩形深浅分界线。
        // 因此严格遵守：仅当透明度为 100%（完全不透明实心背景）时开启滚动边缘淡出；只要存在透明度，立即自动静默关闭遮罩，
        // 确保毛玻璃透明质感纯净通透、零色块、零硬边。
        let global_op = crate::theme::bg_opacity(_cx);
        if global_op < 1.0 {
            return;
        }

        let bg = self.bg_color.unwrap_or_else(|| {
            let t = crate::theme::theme(_cx);
            crate::theme::surface_bg(t.bg_panel, _cx)
        });

        let a0 = 1.0;
        let a1 = 0.45;
        let a2 = 0.10;
        let a3 = 0.0;

        let h0 = effective_fade * 0.35;
        let h1 = effective_fade * 0.35;
        let h2 = effective_fade - h0 - h1;

        if can_scroll_up {
            // 上方渐变：从上到下淡出（顶部最深，向下逐渐透明融进内容）
            // 切片 0 (顶部): alpha 从 a0 降到 a1
            let b0 = Bounds::new(bounds.origin, size(bounds.size.width, h0));
            window.paint_quad(fill(
                b0,
                linear_gradient(
                    180.0,
                    linear_color_stop(Hsla { a: a0, ..bg }, 0.0),
                    linear_color_stop(Hsla { a: a1, ..bg }, 1.0),
                ),
            ));
            // 切片 1 (中部): alpha 从 a1 降到 a2
            let b1 = Bounds::new(
                point(bounds.origin.x, bounds.origin.y + h0),
                size(bounds.size.width, h1),
            );
            window.paint_quad(fill(
                b1,
                linear_gradient(
                    180.0,
                    linear_color_stop(Hsla { a: a1, ..bg }, 0.0),
                    linear_color_stop(Hsla { a: a2, ..bg }, 1.0),
                ),
            ));
            // 切片 2 (下端消融区): alpha 从 a2 平滑归零 a3
            let b2 = Bounds::new(
                point(bounds.origin.x, bounds.origin.y + h0 + h1),
                size(bounds.size.width, h2),
            );
            window.paint_quad(fill(
                b2,
                linear_gradient(
                    180.0,
                    linear_color_stop(Hsla { a: a2, ..bg }, 0.0),
                    linear_color_stop(Hsla { a: a3, ..bg }, 1.0),
                ),
            ));
        }

        if can_scroll_down {
            // 下方渐变：从下到上淡出（底部最深，向上逐渐透明融进内容）
            // 在 GPUI WGSL shader 中，angle = 0.0 方向为 (0, -1) 朝上，stop 0 在底端，stop 1 在顶端。
            let base_y = bounds.origin.y + container_h;
            // 切片 0 (底部最深区): alpha 从 a0 (底) 降到 a1 (顶)
            let b0 = Bounds::new(
                point(bounds.origin.x, base_y - h0),
                size(bounds.size.width, h0),
            );
            window.paint_quad(fill(
                b0,
                linear_gradient(
                    0.0,
                    linear_color_stop(Hsla { a: a0, ..bg }, 0.0),
                    linear_color_stop(Hsla { a: a1, ..bg }, 1.0),
                ),
            ));
            // 切片 1 (中部): alpha 从 a1 (底) 降到 a2 (顶)
            let b1 = Bounds::new(
                point(bounds.origin.x, base_y - h0 - h1),
                size(bounds.size.width, h1),
            );
            window.paint_quad(fill(
                b1,
                linear_gradient(
                    0.0,
                    linear_color_stop(Hsla { a: a1, ..bg }, 0.0),
                    linear_color_stop(Hsla { a: a2, ..bg }, 1.0),
                ),
            ));
            // 切片 2 (上端消融区): alpha 从 a2 (底) 平滑归零 a3 (顶)
            let b2 = Bounds::new(
                point(bounds.origin.x, base_y - effective_fade),
                size(bounds.size.width, h2),
            );
            window.paint_quad(fill(
                b2,
                linear_gradient(
                    0.0,
                    linear_color_stop(Hsla { a: a2, ..bg }, 0.0),
                    linear_color_stop(Hsla { a: a3, ..bg }, 1.0),
                ),
            ));
        }
    }
}
