use gpui::*;
use gpui::prelude::*;
use crate::theme::{surface_bg, theme};
use crate::{h_flex, v_flex};
use super::types::*;
use super::panel::{DockPanel, DockPanelEvent};

pub struct DockManager {
    pub left_panel: Option<Entity<DockPanel>>,
    pub right_panel: Option<Entity<DockPanel>>,
    pub bottom_panel: Option<Entity<DockPanel>>,
    pub center_view: Option<AnyView>,
    pub floating_panels: Vec<Entity<DockPanel>>,

    focus_handle: FocusHandle,
}

impl DockManager {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            left_panel: None,
            right_panel: None,
            bottom_panel: None,
            center_view: None,
            floating_panels: Vec::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn set_left_panel(&mut self, panel: Entity<DockPanel>, cx: &mut Context<Self>) {
        let weak_self = cx.entity().downgrade();
        cx.subscribe(&panel, move |_, _, _event: &DockPanelEvent, cx| {
            if let Some(this) = weak_self.upgrade() {
                let _ = this.update(cx, |_, cx| {
                    cx.notify();
                });
            }
        }).detach();
        self.left_panel = Some(panel);
        cx.notify();
    }

    pub fn set_right_panel(&mut self, panel: Entity<DockPanel>, cx: &mut Context<Self>) {
        let weak_self = cx.entity().downgrade();
        cx.subscribe(&panel, move |_, _, _event: &DockPanelEvent, cx| {
            if let Some(this) = weak_self.upgrade() {
                let _ = this.update(cx, |_, cx| {
                    cx.notify();
                });
            }
        }).detach();
        self.right_panel = Some(panel);
        cx.notify();
    }

    pub fn set_bottom_panel(&mut self, panel: Entity<DockPanel>, cx: &mut Context<Self>) {
        let weak_self = cx.entity().downgrade();
        cx.subscribe(&panel, move |_, _, _event: &DockPanelEvent, cx| {
            if let Some(this) = weak_self.upgrade() {
                let _ = this.update(cx, |_, cx| {
                    cx.notify();
                });
            }
        }).detach();
        self.bottom_panel = Some(panel);
        cx.notify();
    }

    pub fn set_center_view(&mut self, view: AnyView, cx: &mut Context<Self>) {
        self.center_view = Some(view);
        cx.notify();
    }

    pub fn add_floating_panel(&mut self, panel: Entity<DockPanel>, cx: &mut Context<Self>) {
        let weak_self = cx.entity().downgrade();
        cx.subscribe(&panel, move |_, _, _event: &DockPanelEvent, cx| {
            if let Some(this) = weak_self.upgrade() {
                let _ = this.update(cx, |_, cx| {
                    cx.notify();
                });
            }
        }).detach();
        self.floating_panels.push(panel);
        cx.notify();
    }

    pub fn has_maximized_panel(&self, cx: &App) -> bool {
        if let Some(ref p) = self.left_panel {
            if p.read(cx).mode == PanelMode::Maximized {
                return true;
            }
        }
        if let Some(ref p) = self.right_panel {
            if p.read(cx).mode == PanelMode::Maximized {
                return true;
            }
        }
        if let Some(ref p) = self.bottom_panel {
            if p.read(cx).mode == PanelMode::Maximized {
                return true;
            }
        }
        false
    }

    pub fn get_fullscreen_panel(&self, cx: &App) -> Option<Entity<DockPanel>> {
        if let Some(ref p) = self.left_panel {
            if p.read(cx).mode == PanelMode::Fullscreen {
                return Some(p.clone());
            }
        }
        if let Some(ref p) = self.right_panel {
            if p.read(cx).mode == PanelMode::Fullscreen {
                return Some(p.clone());
            }
        }
        if let Some(ref p) = self.bottom_panel {
            if p.read(cx).mode == PanelMode::Fullscreen {
                return Some(p.clone());
            }
        }
        None
    }
}

impl Focusable for DockManager {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DockManager {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        // Fullscreen handles
        if let Some(ref fullscreen_panel) = self.get_fullscreen_panel(cx) {
            return div()
                .size_full()
                .child(fullscreen_panel.clone());
        }

        let has_maximized = self.has_maximized_panel(cx);

        let left_el = self.left_panel.clone().and_then(|panel| {
            let mode = panel.read(cx).mode;
            let collapse = panel.read(cx).collapse_state;
            let is_max = mode == PanelMode::Maximized;
            let should_show = (!has_maximized || is_max) && collapse != PanelCollapseState::Hidden;
            if should_show { Some(panel) } else { None }
        });

        let right_el = self.right_panel.clone().and_then(|panel| {
            let mode = panel.read(cx).mode;
            let collapse = panel.read(cx).collapse_state;
            let is_max = mode == PanelMode::Maximized;
            let should_show = (!has_maximized || is_max) && collapse != PanelCollapseState::Hidden;
            if should_show { Some(panel) } else { None }
        });

        let bottom_el = self.bottom_panel.clone().and_then(|panel| {
            let mode = panel.read(cx).mode;
            let collapse = panel.read(cx).collapse_state;
            let is_max = mode == PanelMode::Maximized;
            let should_show = (!has_maximized || is_max) && collapse != PanelCollapseState::Hidden;
            if should_show { Some(panel) } else { None }
        });

        let center_el = self.center_view.clone().and_then(|view| {
            let should_show = !has_maximized;
            if should_show { Some(view) } else { None }
        });

        // Compute a per-panel width cap so a panel can never overflow the dock
        // (and push its right-aligned content off-screen). The right panel is the
        // last flex child, so when its clamped `size` exceeds the available
        // width it overflows the window to the right. Cap each side-panel's
        // effective width to (viewport - sibling - min center).
        let vw = f32::from(_window.viewport_size().width);
        let min_center = 220.0;
        let left_w = left_el
            .as_ref()
            .map(|p| p.read(cx).size)
            .unwrap_or(0.0);
        let right_w = right_el
            .as_ref()
            .map(|p| p.read(cx).size)
            .unwrap_or(0.0);
        if let Some(right) = right_el.as_ref() {
            let cap = (vw - left_w - min_center).max(250.0);
            right.update(cx, |p, _| {
                if p.max_render_size != Some(cap) {
                    p.max_render_size = Some(cap);
                }
            });
        }
        if let Some(left) = left_el.as_ref() {
            let cap = (vw - right_w - min_center).max(250.0);
            left.update(cx, |p, _| {
                if p.max_render_size != Some(cap) {
                    p.max_render_size = Some(cap);
                }
            });
        }

        h_flex()
            .size_full()
            .bg(surface_bg(t.bg_primary, cx))
            .when_some(left_el, |this, left| this.child(left))
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w(px(0.0))
                    .when_some(center_el, |this, center| this.child(div().flex_1().w_full().min_h(px(0.0)).child(center)))
                    .when_some(bottom_el, |this, bottom| this.child(bottom))
            )
            .when_some(right_el, |this, right| this.child(right))
            .children(self.floating_panels.clone())
    }
}
