//! Dock container state and animation controller.
//!
//! Manages dock container visibility, auto-hide behavior, and animation state
//! for any DockPosition (Left, Right, Bottom, Top).

use velowork_core::types::DockPosition;

/// Structural constraints defining minimum and maximum dock dimensions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockSizeConstraints {
    pub min: f32,
    pub max: f32,
}

/// Contextual requirements and environmental bounds provided during a resize operation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DockResizeContext {
    /// Dynamic minimum size required by the active panel content (e.g. tab header / toolbar).
    pub content_min: Option<f32>,
    /// Maximum available space calculated from viewport (e.g. window_width - min_center_area).
    pub window_limit: Option<f32>,
}

/// The effective resolved bounds after resolving content requirements and window limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedDockConstraints {
    pub min: f32,
    pub max: f32,
}

impl DockSizeConstraints {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    /// Resolve effective constraints.
    ///
    /// Architectural Invariant: Window boundary constraints strictly take precedence
    /// over content minimum requirements (`window_limit` > `content_min`) to prevent
    /// narrow windows from having their center area squeezed out.
    pub fn resolve_constraints(&self, context: &DockResizeContext) -> ResolvedDockConstraints {
        let effective_max = context.window_limit.map_or(self.max, |wl| wl.min(self.max));
        let raw_min = context.content_min.map_or(self.min, |cm| cm.max(self.min));
        let effective_min = raw_min.min(effective_max);
        ResolvedDockConstraints {
            min: effective_min,
            max: effective_max,
        }
    }

    /// Resolve and clamp the requested dimension according to the given context.
    pub fn resolve(&self, requested: f32, context: &DockResizeContext) -> f32 {
        let resolved = self.resolve_constraints(context);
        requested.clamp(resolved.min, resolved.max)
    }

    /// Clamp a dimension strictly between standard structural min and max bounds.
    pub fn clamp(&self, requested: f32) -> f32 {
        requested.clamp(self.min, self.max)
    }
}

/// Standard structural constraints for side docks (Left / Right).
pub const SIDEBAR_CONSTRAINTS: DockSizeConstraints = DockSizeConstraints {
    min: 140.0,
    max: 800.0,
};

/// Standard structural constraints for bottom dock.
pub const BOTTOM_DOCK_CONSTRAINTS: DockSizeConstraints = DockSizeConstraints {
    min: 120.0,
    max: 800.0,
};

pub const DEFAULT_DOCK_SIZE: f32 = 280.0;
pub const MIN_DOCK_SIZE: f32 = SIDEBAR_CONSTRAINTS.min;
pub const MAX_DOCK_SIZE: f32 = SIDEBAR_CONSTRAINTS.max;

/// Animation duration in milliseconds.
pub const ANIMATION_DURATION_MS: u64 = 140;

/// Frame time for ~60fps animation.
pub const FRAME_TIME_MS: u64 = 16;

/// Result of a dock state change that may require animation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationTarget {
    /// No animation needed
    None,
    /// Animate to fully open (1.0)
    Open,
    /// Animate to fully closed (0.0)
    Close,
}

impl AnimationTarget {
    /// Get the target value for animation.
    pub fn value(self) -> Option<f32> {
        match self {
            AnimationTarget::None => None,
            AnimationTarget::Open => Some(1.0),
            AnimationTarget::Close => Some(0.0),
        }
    }
}

/// Controller for dock state and behavior for a specific DockPosition.
#[derive(Debug, Clone)]
pub struct DockController {
    position: DockPosition,
    open: bool,
    animation: f32,
    auto_hide: bool,
    hover_shown: bool,
    size: f32,
}

impl DockController {
    pub fn new(position: DockPosition, open: bool, auto_hide: bool, size: f32) -> Self {
        let constraints = match position {
            DockPosition::Bottom => BOTTOM_DOCK_CONSTRAINTS,
            _ => SIDEBAR_CONSTRAINTS,
        };
        let clamped_size = constraints.clamp(size);
        Self {
            position,
            open,
            animation: if open { 1.0 } else { 0.0 },
            auto_hide,
            hover_shown: false,
            size: clamped_size,
        }
    }

    pub fn position(&self) -> DockPosition {
        self.position
    }

    pub fn size(&self) -> f32 {
        self.size
    }

    pub fn width(&self) -> f32 {
        self.size
    }

    pub fn set_size(&mut self, size: f32) {
        let constraints = match self.position {
            DockPosition::Bottom => BOTTOM_DOCK_CONSTRAINTS,
            _ => SIDEBAR_CONSTRAINTS,
        };
        self.size = constraints.clamp(size);
    }

    pub fn set_size_with_min(&mut self, size: f32, min_size: f32) {
        let constraints = match self.position {
            DockPosition::Bottom => BOTTOM_DOCK_CONSTRAINTS,
            _ => SIDEBAR_CONSTRAINTS,
        };
        let context = DockResizeContext {
            content_min: Some(min_size),
            window_limit: None,
        };
        self.size = constraints.resolve(size, &context);
    }

    /// Resize with full contextual constraints resolution.
    pub fn resize(
        &mut self,
        requested: f32,
        constraints: &DockSizeConstraints,
        context: &DockResizeContext,
    ) -> f32 {
        self.size = constraints.resolve(requested, context);
        self.size
    }

    pub fn set_width(&mut self, width: f32) {
        self.set_size(width);
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Forcefully set the open/closed state (without flipping). Used when the
    /// dock is shown/hidden by an external toggle (e.g. IDEA-style tool window
    /// buttons) rather than the dock's own collapse animation.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        if !open {
            self.hover_shown = false;
        }
    }

    pub fn is_auto_hide(&self) -> bool {
        self.auto_hide
    }

    pub fn is_hover_shown(&self) -> bool {
        self.hover_shown
    }

    pub fn animation(&self) -> f32 {
        self.animation
    }

    pub fn set_animation(&mut self, value: f32) {
        self.animation = value.clamp(0.0, 1.0);
    }

    pub fn current_size(&self) -> f32 {
        self.animation * self.size
    }

    pub fn current_width(&self) -> f32 {
        self.current_size()
    }

    pub fn should_render(&self) -> bool {
        self.animation > 0.01
    }

    pub fn is_animating(&self) -> bool {
        self.animation > 0.001 && self.animation < 0.999
    }

    pub fn toggle(&mut self) -> AnimationTarget {
        self.open = !self.open;
        self.hover_shown = false;

        if self.open {
            AnimationTarget::Open
        } else {
            AnimationTarget::Close
        }
    }

    pub fn toggle_auto_hide(&mut self) -> AnimationTarget {
        self.auto_hide = !self.auto_hide;

        if self.auto_hide && self.open {
            self.open = false;
            AnimationTarget::Close
        } else {
            AnimationTarget::None
        }
    }

    pub fn show_on_hover(&mut self) -> AnimationTarget {
        if self.auto_hide && !self.open && !self.hover_shown {
            self.hover_shown = true;
            AnimationTarget::Open
        } else {
            AnimationTarget::None
        }
    }

    pub fn hide_on_leave(&mut self) -> AnimationTarget {
        if self.auto_hide && self.hover_shown {
            self.hover_shown = false;
            AnimationTarget::Close
        } else {
            AnimationTarget::None
        }
    }

    pub fn ease_progress_ratio(current: f32, target: f32, ratio: f32) -> f32 {
        let t = ratio.clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(2);
        current + (target - current) * eased
    }

    pub fn ease_progress(current: f32, target: f32, step: usize, total_steps: usize) -> f32 {
        if total_steps == 0 {
            return target;
        }
        Self::ease_progress_ratio(current, target, step as f32 / total_steps as f32)
    }

    pub fn animation_steps() -> usize {
        (ANIMATION_DURATION_MS / FRAME_TIME_MS).max(1) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dock_size_constraints_clamp() {
        let constraints = SIDEBAR_CONSTRAINTS;
        assert_eq!(constraints.clamp(100.0), 140.0);
        assert_eq!(constraints.clamp(280.0), 280.0);
        assert_eq!(constraints.clamp(1000.0), 800.0);
    }

    #[test]
    fn test_dock_size_constraints_resolve_dynamic_content_min() {
        let constraints = SIDEBAR_CONSTRAINTS;
        let ctx = DockResizeContext {
            content_min: Some(180.0),
            window_limit: Some(600.0),
        };
        // Clamped to dynamic content min 180
        assert_eq!(constraints.resolve(100.0, &ctx), 180.0);
        assert_eq!(constraints.resolve(300.0, &ctx), 300.0);
        // Clamped to window limit 600
        assert_eq!(constraints.resolve(700.0, &ctx), 600.0);
    }

    #[test]
    fn test_dock_size_constraints_window_priority_over_content_min() {
        let constraints = SIDEBAR_CONSTRAINTS;
        // Invariant 7: window boundary priority over content min requirement
        let ctx = DockResizeContext {
            content_min: Some(300.0),
            window_limit: Some(220.0),
        };
        let resolved = constraints.resolve_constraints(&ctx);
        assert_eq!(resolved.max, 220.0);
        assert_eq!(resolved.min, 220.0);
        assert_eq!(constraints.resolve(250.0, &ctx), 220.0);
    }

    #[test]
    fn test_dock_controller_resize() {
        let mut controller = DockController::new(DockPosition::Left, true, false, 280.0);
        assert_eq!(controller.size(), 280.0);

        let ctx = DockResizeContext {
            content_min: Some(160.0),
            window_limit: Some(700.0),
        };
        let new_size = controller.resize(100.0, &SIDEBAR_CONSTRAINTS, &ctx);
        assert_eq!(new_size, 160.0);
        assert_eq!(controller.size(), 160.0);

        controller.resize(500.0, &SIDEBAR_CONSTRAINTS, &ctx);
        assert_eq!(controller.size(), 500.0);
    }
}
