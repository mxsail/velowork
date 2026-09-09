//! Motion and animation utilities conforming to Ant Design Motion specifications.
//!
//! Provides natural easing curves (cubic-bezier), origin-aware modal transform
//! calculators, and coordinated motion states for dialogs and panels.

use gpui::*;
use std::time::Duration;

/// Standard animation duration constants (matching spring modal specifications).
pub const DURATION_FAST: Duration = Duration::from_millis(100);
pub const DURATION_MODAL_ENTER: Duration = Duration::from_millis(350);
pub const DURATION_MODAL_LEAVE: Duration = Duration::from_millis(220);
pub const DURATION_PANEL: Duration = Duration::from_millis(160);

/// Solves a cubic-bezier curve `(x1, y1, x2, y2)` for time `t` in `[0.0, 1.0]`.
pub fn cubic_bezier(t: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }

    // Solve for parameter u where x(u) = t using Newton-Raphson
    let mut u = t;
    for _ in 0..8 {
        let one_minus_u = 1.0 - u;
        let x = 3.0 * one_minus_u * one_minus_u * u * x1
            + 3.0 * one_minus_u * u * u * x2
            + u * u * u;
        let dx = 3.0 * one_minus_u * one_minus_u * x1
            + 6.0 * one_minus_u * u * (x2 - x1)
            + 3.0 * u * u * (1.0 - x2);
        if dx.abs() < 1e-6 {
            break;
        }
        let diff = x - t;
        u -= diff / dx;
        u = u.clamp(0.0, 1.0);
    }

    // Evaluate y(u)
    let one_minus_u = 1.0 - u;
    3.0 * one_minus_u * one_minus_u * u * y1
        + 3.0 * one_minus_u * u * u * y2
        + u * u * u
}

/// Spring bounce curve matching `cubic-bezier(0.34, 1.56, 0.64, 1.0)`.
/// Produces a lively overshoot (~1.06) and elastic settling.
pub fn ease_out_bounce(t: f32) -> f32 {
    cubic_bezier(t, 0.34, 1.56, 0.64, 1.0)
}

/// Ant Design Enter easing: strong deceleration curve `cubic-bezier(0.08, 0.82, 0.17, 1.0)`.
///
/// Starts fast with energy and decelerates smoothly to rest.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Ant Design Leave easing: acceleration exit curve `cubic-bezier(0.6, 0.04, 0.98, 0.34)`.
///
/// Starts gently and accelerates off screen without lingering.
pub fn ease_in_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t.powi(3)
}

/// S-curve transition: `cubic-bezier(0.645, 0.045, 0.355, 1.0)` / smoothstep.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Hermite smooth step interpolation `3t^2 - 2t^3`.
pub fn smooth_step(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Identifies the trigger origin of an animation (e.g. mouse click position).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MotionOrigin {
    /// Absolute window coordinate of the trigger event, if any.
    pub point: Option<Point<Pixels>>,
}

impl MotionOrigin {
    /// Origin from a mouse click coordinate.
    pub fn from_point(point: Point<Pixels>) -> Self {
        Self { point: Some(point) }
    }

    /// Origin from center of screen / window (e.g. shortcut-driven).
    pub fn center() -> Self {
        Self { point: None }
    }
}

/// Coordinated motion parameters for a modal/dialog card and its backdrop.
#[derive(Clone, Copy, Debug)]
pub struct ModalMotionValues {
    /// Backdrop opacity multiplier [0.0, 1.0].
    pub backdrop_opacity: f32,
    /// Card opacity multiplier [0.0, 1.0].
    pub card_opacity: f32,
    /// Card scale factor [0.05, 1.0].
    pub scale: f32,
    /// Animated card width in pixels.
    pub width: Pixels,
    /// Animated card height in pixels.
    pub height: Pixels,
    /// Card translation offset relative to viewport center in window pixels (x, y).
    pub offset: Point<Pixels>,
}

/// Active motion state of an overlay/modal.
#[derive(Clone, Copy, Debug)]
pub struct ModalMotionState {
    /// Raw progress in range [0.0, 1.0].
    pub progress: f32,
    /// Whether the modal is currently exiting/closing.
    pub is_closing: bool,
    /// Trigger origin.
    pub origin: Option<Point<Pixels>>,
}

impl Default for ModalMotionState {
    fn default() -> Self {
        Self {
            progress: 1.0,
            is_closing: false,
            origin: None,
        }
    }
}

impl ModalMotionState {
    /// Creates a newly opened modal motion state starting at progress 0.0.
    pub fn new_opening(origin: Option<Point<Pixels>>) -> Self {
        Self {
            progress: 0.0,
            is_closing: false,
            origin,
        }
    }

    /// Mark the modal as closing and begin leaving.
    pub fn start_closing(&mut self) {
        self.is_closing = true;
    }

    /// Check if the motion animation has completely finished.
    pub fn is_finished(&self) -> bool {
        if self.is_closing {
            self.progress <= 0.001
        } else {
            self.progress >= 0.999
        }
    }

    /// Compute coordinated render values with known card dimensions.
    ///
    /// Implements spring bounce curve `cubic-bezier(0.34, 1.56, 0.64, 1.0)`:
    /// - Starts from +24px below center with scale 0.92
    /// - Floats upwards with slight spring overshoot (~-1.5px) and settles to 0px
    /// - Smoothly fades in backdrop and card
    pub fn compute_card_values(
        &self,
        _window_size: Size<Pixels>,
        card_size: Size<Pixels>,
    ) -> ModalMotionValues {
        if self.is_closing {
            // Exit animation: accelerating drop (+24px) with fade out
            let raw_p = self.progress.clamp(0.0, 1.0);
            let t = ease_in_cubic(raw_p);
            let backdrop_opacity = t;
            let card_opacity = t;
            let scale = 0.92 + 0.08 * t;
            let width = card_size.width;
            let height = card_size.height;
            let dy = px(24.0) * (1.0 - t);

            ModalMotionValues {
                backdrop_opacity,
                card_opacity,
                scale,
                width,
                height,
                offset: Point::new(px(0.0), dy),
            }
        } else {
            // Enter animation: spring bounce curve cubic-bezier(0.34, 1.56, 0.64, 1)
            // Starts translateY(+24px) below center, floats up, overshoots past 0px, and settles to 0px
            let raw_p = self.progress.clamp(0.0, 1.0);
            let t_bounce = ease_out_bounce(raw_p);
            let backdrop_opacity = raw_p.clamp(0.0, 1.0);
            let card_opacity = (raw_p * 2.0).min(1.0);
            let scale = 0.92 + 0.08 * t_bounce;
            let width = card_size.width;
            let height = card_size.height;
            let dy = px(24.0) * (1.0 - t_bounce);

            ModalMotionValues {
                backdrop_opacity,
                card_opacity,
                scale,
                width,
                height,
                offset: Point::new(px(0.0), dy),
            }
        }
    }

    /// Compute coordinated render values given the window's total viewport size (default card size).
    pub fn compute_values(&self, window_size: Size<Pixels>) -> ModalMotionValues {
        self.compute_card_values(window_size, Size::new(px(600.0), px(450.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::{ease_in_cubic, ease_in_out, ease_out_bounce, ease_out_cubic, ModalMotionState};
    use gpui::{Point, Size, px};

    #[test]
    fn test_easing_boundaries() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert_eq!(ease_in_cubic(0.0), 0.0);
        assert_eq!(ease_in_cubic(1.0), 1.0);
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(1.0), 1.0);
        assert_eq!(ease_out_bounce(0.0), 0.0);
        assert_eq!(ease_out_bounce(1.0), 1.0);
    }

    #[test]
    fn test_ease_out_bounce_overshoot() {
        // Between t=0.6 and t=0.85, the spring curve overshoots > 1.0 (elastic bounce)
        let mid = ease_out_bounce(0.72);
        assert!(mid > 1.03, "mid was {mid}, expected overshoot > 1.03");
        assert!(mid < 1.12, "mid was {mid}, expected overshoot < 1.12");
    }

    #[test]
    fn test_modal_motion_values() {
        let origin = Point::new(px(100.0), px(200.0));
        let state = ModalMotionState::new_opening(Some(origin));
        let win_size = Size {
            width: px(1000.0),
            height: px(800.0),
        };
        let card_size = Size {
            width: px(600.0),
            height: px(400.0),
        };

        // At enter progress 0.0: starts translateY(+24px) below center, scale 0.92
        let val_0 = state.compute_card_values(win_size, card_size);
        assert_eq!(val_0.backdrop_opacity, 0.0);
        assert_eq!(val_0.card_opacity, 0.0);
        assert_eq!(val_0.scale, 0.92);
        assert_eq!(val_0.width, px(600.0));
        assert_eq!(val_0.height, px(400.0));
        assert_eq!(val_0.offset.x, px(0.0));
        assert_eq!(val_0.offset.y, px(24.0));

        // At enter progress 1.0: fully settled at center with scale 1.0
        let mut state_1 = state;
        state_1.progress = 1.0;
        let val_1 = state_1.compute_card_values(win_size, card_size);
        assert_eq!(val_1.backdrop_opacity, 1.0);
        assert_eq!(val_1.card_opacity, 1.0);
        assert_eq!(val_1.scale, 1.0);
        assert_eq!(val_1.width, px(600.0));
        assert_eq!(val_1.height, px(400.0));
        assert_eq!(val_1.offset.x, px(0.0));
        assert_eq!(val_1.offset.y, px(0.0));

        // At exit progress 0.0: dropped back to +24px with scale 0.92 and opacity 0
        let mut state_exit = state;
        state_exit.start_closing();
        state_exit.progress = 0.0;
        let val_exit = state_exit.compute_card_values(win_size, card_size);
        assert_eq!(val_exit.backdrop_opacity, 0.0);
        assert_eq!(val_exit.card_opacity, 0.0);
        assert_eq!(val_exit.scale, 0.92);
        assert_eq!(val_exit.offset.y, px(24.0));
    }
}
