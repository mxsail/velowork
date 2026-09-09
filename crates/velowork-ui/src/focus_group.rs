//! Universal FocusGroup and Tab / Shift+Tab keyboard focus navigation utilities.
//!
//! Provides automatic focus cycle management across form fields and dialog controls.

use gpui::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
struct FocusGroupScrollState {
    scroll_handle: ScrollHandle,
    bounds: Vec<(FocusHandle, (f32, f32))>,
    current_y: f32,
    default_row_height: f32,
    gap: f32,
    last_bounds: Option<(f32, f32)>,
}

/// Focus navigation group managing automatic Tab / Shift+Tab cycling across a list of [`FocusHandle`]s.
#[derive(Clone, Default)]
pub struct FocusGroup {
    handles: Rc<RefCell<Vec<FocusHandle>>>,
    scroll_state: Rc<RefCell<Option<FocusGroupScrollState>>>,
}

impl FocusGroup {
    /// Create a new empty [`FocusGroup`].
    pub fn new() -> Self {
        Self {
            handles: Rc::new(RefCell::new(Vec::new())),
            scroll_state: Rc::new(RefCell::new(None)),
        }
    }

    /// Enable automatic scroll-into-view using the given [`ScrollHandle`].
    /// Default top padding: 20.0px, row height: 56.0px, gap: 16.0px.
    pub fn with_scroll(self, scroll_handle: ScrollHandle) -> Self {
        self.with_scroll_config(scroll_handle, 20.0, 56.0, 16.0)
    }

    /// Enable automatic scroll-into-view with custom layout metrics.
    pub fn with_scroll_config(
        self,
        scroll_handle: ScrollHandle,
        top_padding: f32,
        default_row_height: f32,
        gap: f32,
    ) -> Self {
        *self.scroll_state.borrow_mut() = Some(FocusGroupScrollState {
            scroll_handle,
            bounds: Vec::new(),
            current_y: top_padding,
            default_row_height,
            gap,
            last_bounds: None,
        });
        self
    }

    /// Reset all collected handles in this group.
    pub fn clear(&self) {
        self.handles.borrow_mut().clear();
        if let Some(ref mut state) = *self.scroll_state.borrow_mut() {
            state.bounds.clear();
            state.current_y = 20.0;
            state.last_bounds = None;
        }
    }

    /// Register a single [`FocusHandle`] into the group.
    /// If scroll tracking is active, calculates row bounds and advances the vertical cursor.
    pub fn add(&self, handle: FocusHandle) {
        if let Some(ref mut state) = *self.scroll_state.borrow_mut() {
            let top = state.current_y;
            let bottom = top + state.default_row_height;
            state.current_y = bottom + state.gap;
            state.bounds.push((handle.clone(), (top, bottom)));
            state.last_bounds = Some((top, bottom));
        }
        self.handles.borrow_mut().push(handle);
    }

    /// Register a [`FocusHandle`] that resides on the SAME horizontal row as the previous field.
    pub fn add_same_row(&self, handle: FocusHandle) {
        if let Some(ref mut state) = *self.scroll_state.borrow_mut() {
            if let Some(bounds) = state.last_bounds {
                state.bounds.push((handle.clone(), bounds));
            }
        }
        self.handles.borrow_mut().push(handle);
    }

    /// Register a [`FocusHandle`] with a custom control height (e.g. multiline textarea).
    pub fn add_with_height(&self, handle: FocusHandle, height: f32) {
        if let Some(ref mut state) = *self.scroll_state.borrow_mut() {
            let top = state.current_y;
            let bottom = top + height;
            state.current_y = bottom + state.gap;
            state.bounds.push((handle.clone(), (top, bottom)));
            state.last_bounds = Some((top, bottom));
        }
        self.handles.borrow_mut().push(handle);
    }

    /// Register a [`FocusHandle`] that is outside the scroll container (e.g. dialog footer buttons).
    pub fn add_unscrolled(&self, handle: FocusHandle) {
        self.handles.borrow_mut().push(handle);
    }

    /// Register multiple [`FocusHandle`]s into the group.
    pub fn extend(&self, handles: impl IntoIterator<Item = FocusHandle>) {
        for handle in handles {
            self.add(handle);
        }
    }

    /// Return a cloned snapshot of all handles in this group.
    pub fn handles(&self) -> Vec<FocusHandle> {
        self.handles.borrow().clone()
    }

    /// Cycle focus to next (or previous with shift) handle. Returns true if a handle was focused.
    pub fn cycle(&self, is_shift: bool, window: &mut Window, cx: &mut App) -> bool {
        let handles = self.handles.borrow();
        if handles.is_empty() {
            return false;
        }

        let current_idx = handles.iter().position(|h| h.is_focused(window));
        let next_idx = match current_idx {
            Some(idx) => {
                if is_shift {
                    (idx + handles.len() - 1) % handles.len()
                } else {
                    (idx + 1) % handles.len()
                }
            }
            None => {
                if is_shift {
                    handles.len() - 1
                } else {
                    0
                }
            }
        };

        let next_handle = &handles[next_idx];
        window.focus(next_handle, cx);

        // 统一视口自适应平滑滚动（Scroll Into View with 40px Padding）
        if let Some(ref state) = *self.scroll_state.borrow() {
            if let Some(&(_, (item_top, item_bottom))) = state.bounds.iter().find(|(h, _)| h == next_handle) {
                Self::scroll_into_view(&state.scroll_handle, item_top, item_bottom);
            }
        }

        true
    }

    /// Static helper to cycle through any slice of [`FocusHandle`]s without scroll tracking.
    pub fn cycle_handles(handles: &[FocusHandle], is_shift: bool, window: &mut Window, cx: &mut App) -> bool {
        if handles.is_empty() {
            return false;
        }

        let current_idx = handles.iter().position(|h| h.is_focused(window));
        let next_idx = match current_idx {
            Some(idx) => {
                if is_shift {
                    (idx + handles.len() - 1) % handles.len()
                } else {
                    (idx + 1) % handles.len()
                }
            }
            None => {
                if is_shift {
                    handles.len() - 1
                } else {
                    0
                }
            }
        };

        window.focus(&handles[next_idx], cx);
        true
    }

    /// 视口自适应平滑滚动（Scroll Into View with 40px Padding）：
    /// 当控件处于视口之外或距离边缘不足 40px 时，自动平滑滚动使其处于舒适可视带内。
    pub fn scroll_into_view(scroll_handle: &ScrollHandle, item_top: f32, item_bottom: f32) {
        let viewport_h = f32::from(scroll_handle.bounds().size.height);
        if viewport_h <= 0.0 {
            return;
        }

        let cur_scroll_y = -f32::from(scroll_handle.offset().y);
        let max_offset_y = f32::from(scroll_handle.max_offset().y);

        let pad = 40.0;
        let vis_top = item_top - cur_scroll_y;
        let vis_bottom = item_bottom - cur_scroll_y;

        let target_scroll_y = if vis_bottom > viewport_h - pad {
            let needed = item_bottom - (viewport_h - pad);
            needed.max(item_top - pad)
        } else if vis_top < pad {
            item_top - pad
        } else {
            return;
        };

        let clamped_scroll_y = target_scroll_y.clamp(0.0, max_offset_y.max(0.0));
        scroll_handle.set_offset(point(px(0.0), px(-clamped_scroll_y)));
    }
}

/// Extension trait for attaching Tab focus cycling to GPUI elements.
pub trait FocusGroupExt: Sized {
    /// Attach automated Tab / Shift+Tab focus navigation using a [`FocusGroup`].
    fn tab_cycle(self, focus_group: &FocusGroup) -> Self;

    /// Attach automated Tab / Shift+Tab focus navigation using a dynamic handle supplier closure.
    fn tab_cycle_with<F>(self, get_handles: F) -> Self
    where
        F: Fn(&mut Window, &mut App) -> Vec<FocusHandle> + 'static;
}

impl FocusGroupExt for Stateful<Div> {
    fn tab_cycle(self, focus_group: &FocusGroup) -> Self {
        let fg = focus_group.clone();
        self.on_key_down(move |event: &KeyDownEvent, window, cx| {
            let key = event.keystroke.key.as_str();
            if key == "tab" || key == "\t" {
                let is_shift = event.keystroke.modifiers.shift;
                if fg.cycle(is_shift, window, cx) {
                    cx.stop_propagation();
                }
            }
        })
    }

    fn tab_cycle_with<F>(self, get_handles: F) -> Self
    where
        F: Fn(&mut Window, &mut App) -> Vec<FocusHandle> + 'static,
    {
        self.on_key_down(move |event: &KeyDownEvent, window, cx| {
            let key = event.keystroke.key.as_str();
            if key == "tab" || key == "\t" {
                let is_shift = event.keystroke.modifiers.shift;
                let handles = get_handles(window, cx);
                if FocusGroup::cycle_handles(&handles, is_shift, window, cx) {
                    cx.stop_propagation();
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FocusGroup;

    #[test]
    fn test_focus_group_collection() {
        let fg = FocusGroup::new();
        assert!(fg.handles().is_empty());

        // Test clear and extend
        fg.clear();
        assert_eq!(fg.handles().len(), 0);
    }

    #[gpui::test]
    fn test_focus_group_scroll_bounds(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let h1 = cx.focus_handle();
            let h2 = cx.focus_handle();
            let h3 = cx.focus_handle();
            let footer = cx.focus_handle();

            let scroll_handle = gpui::ScrollHandle::new();
            let fg = FocusGroup::new().with_scroll_config(scroll_handle, 20.0, 56.0, 16.0);

            fg.add(h1.clone());
            fg.add(h2.clone());
            fg.add_same_row(h3.clone());
            fg.add_unscrolled(footer.clone());

            assert_eq!(fg.handles().len(), 4);

            let state_ref = fg.scroll_state.borrow();
            let state = state_ref.as_ref().unwrap();

            // h1: [20.0, 76.0]
            let h1_bounds = state.bounds.iter().find(|(h, _)| h == &h1).map(|(_, b)| *b);
            assert_eq!(h1_bounds, Some((20.0, 76.0)));

            // h2: [92.0, 148.0]
            let h2_bounds = state.bounds.iter().find(|(h, _)| h == &h2).map(|(_, b)| *b);
            assert_eq!(h2_bounds, Some((92.0, 148.0)));

            // h3 (same row as h2): [92.0, 148.0]
            let h3_bounds = state.bounds.iter().find(|(h, _)| h == &h3).map(|(_, b)| *b);
            assert_eq!(h3_bounds, Some((92.0, 148.0)));

            // footer: unscrolled (none in bounds)
            let footer_bounds = state.bounds.iter().find(|(h, _)| h == &footer);
            assert!(footer_bounds.is_none());
        });
    }
}
