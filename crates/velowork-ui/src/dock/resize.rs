use gpui::*;
use std::cell::RefCell;
use std::rc::Rc;

const DEFAULT_DIVIDER_SIZE: f32 = 6.0;
const HANDLE_HITBOX_SIZE: f32 = 10.0;

/// One-shot drag-start callback, shared into the element's paint closure and
/// `take()`n on first invocation.
type DragStartCallback = Rc<RefCell<Option<Box<dyn FnOnce(Point<Pixels>, &mut App)>>>>;

pub struct ResizeHandle {
    is_horizontal: bool,
    divider_size: f32,
    #[allow(dead_code)]
    border_color: u32,
    border_active_color: u32,
    is_active: bool,
    on_drag_start: DragStartCallback,
}

impl ResizeHandle {
    pub fn new(
        is_horizontal: bool,
        border_color: u32,
        border_active_color: u32,
        on_drag_start: impl FnOnce(Point<Pixels>, &mut App) + 'static,
    ) -> Self {
        Self {
            is_horizontal,
            divider_size: DEFAULT_DIVIDER_SIZE,
            border_color,
            border_active_color,
            is_active: false,
            on_drag_start: Rc::new(RefCell::new(Some(Box::new(on_drag_start)))),
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.divider_size = size;
        self
    }

    pub fn is_active(mut self, active: bool) -> Self {
        self.is_active = active;
        self
    }
}

impl IntoElement for ResizeHandle {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ResizeHandle {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

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
        let style = if self.is_horizontal {
            Style {
                size: Size {
                    width: relative(1.0).into(),
                    height: px(self.divider_size).into(),
                },
                flex_shrink: 0.0,
                ..Default::default()
            }
        } else {
            Style {
                size: Size {
                    width: px(self.divider_size).into(),
                    height: relative(1.0).into(),
                },
                flex_shrink: 0.0,
                ..Default::default()
            }
        };

        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        let expand = px((HANDLE_HITBOX_SIZE - self.divider_size).max(0.0) / 2.0);
        let hitbox_bounds = if self.is_horizontal {
            Bounds::new(
                point(bounds.origin.x, bounds.origin.y - expand),
                size(bounds.size.width, px(HANDLE_HITBOX_SIZE)),
            )
        } else {
            Bounds::new(
                point(bounds.origin.x - expand, bounds.origin.y),
                size(px(HANDLE_HITBOX_SIZE), bounds.size.height),
            )
        };

        window.insert_hitbox(hitbox_bounds, HitboxBehavior::BlockMouseExceptScroll)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let is_hovered = hitbox.is_hovered(window);
        if self.is_active || is_hovered {
            // Floating gap highlight indicator in the whitespace area (fills entire gap)
            let radius = px((self.divider_size / 2.0).round().max(1.0));
            window.paint_quad(
                fill(bounds, rgb(self.border_active_color))
                    .corner_radii(radius),
            );
        }

        let cursor = if self.is_horizontal {
            CursorStyle::ResizeUpDown
        } else {
            CursorStyle::ResizeLeftRight
        };
        window.set_cursor_style(cursor, hitbox);

        let on_drag_start = self.on_drag_start.clone();
        let hitbox_id = hitbox.id;
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && e.button == MouseButton::Left
                && hitbox_id.is_hovered(window)
            {
                if let Some(cb) = on_drag_start.borrow_mut().take() {
                    cb(e.position, cx);
                }
                cx.stop_propagation();
            }
        });
    }
}
