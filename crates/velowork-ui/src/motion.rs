//! Motion and animation utilities conforming to Ant Design Motion specifications.
//!
//! Provides natural easing curves (cubic-bezier), origin-aware modal transform
//! calculators, and coordinated motion states for dialogs and panels.

use gpui::*;
use std::time::Duration;

/// Standard animation duration constants (matching spring modal specifications).
pub const DURATION_FAST: Duration = Duration::from_millis(100);
pub const DURATION_MASK_ENTER: Duration = Duration::from_millis(180);
pub const DURATION_MODAL_ENTER: Duration = Duration::from_millis(280);
pub const DURATION_MODAL_LEAVE: Duration = Duration::from_millis(180);
pub const DURATION_MODAL_MORPH: Duration = Duration::from_millis(280);
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

/// Dynamic Island fluid spring morphing curve `cubic-bezier(0.32, 0.72, 0.0, 1.0)`.
pub fn ease_out_morph(t: f32) -> f32 {
    cubic_bezier(t, 0.32, 0.72, 0.0, 1.0)
}

/// Panel morphing curve matching HTML prototype `cubic-bezier(0.22, 1, 0.36, 1)`.
pub fn ease_out_panel(t: f32) -> f32 {
    cubic_bezier(t, 0.22, 1.0, 0.36, 1.0)
}

/// Tab collapse easing curve `cubic-bezier(0.4, 0, 0.2, 1)`.
pub fn ease_tab_collapse(t: f32) -> f32 {
    cubic_bezier(t, 0.4, 0.0, 0.2, 1.0)
}

/// Tab expand easing curve `cubic-bezier(0.2, 0, 0, 1)`.
pub fn ease_tab_expand(t: f32) -> f32 {
    cubic_bezier(t, 0.2, 0.0, 0.0, 1.0)
}

/// Ant Design 6.6.3 Modal Open easing `motionEaseInOutCirc`: `cubic-bezier(0.78, 0.14, 0.15, 0.86)`.
pub fn motion_ease_in_out_circ(t: f32) -> f32 {
    cubic_bezier(t, 0.78, 0.14, 0.15, 0.86)
}

/// Ant Design 6.6.3 Modal Close easing `motionEaseOutCirc`: `cubic-bezier(0.08, 0.82, 0.17, 1.0)`.
pub fn motion_ease_out_circ(t: f32) -> f32 {
    cubic_bezier(t, 0.08, 0.82, 0.17, 1.0)
}

/// Ant Design Enter easing: strong deceleration curve `cubic-bezier(0.08, 0.82, 0.17, 1.0)`.
///
/// Starts fast with energy and decelerates smoothly to rest.
pub fn ease_out_cubic(t: f32) -> f32 {
    cubic_bezier(t, 0.08, 0.82, 0.17, 1.0)
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

/// Geometric morph targets for Dynamic Island collapse animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MorphExitState {
    pub start_bounds: Bounds<Pixels>,
    pub target_bounds: Bounds<Pixels>,
    pub start_radius: Pixels,
    pub target_radius: Pixels,
}

/// Calculated values for Dynamic Island morphing transition.
#[derive(Clone, Copy, Debug)]
pub struct MorphMotionValues {
    pub current_bounds: Bounds<Pixels>,
    pub border_radius: Pixels,
    pub inner_content_opacity: f32,
    pub dest_content_opacity: f32,
    pub backdrop_opacity: f32,
    pub card_opacity: f32,
    pub is_morphing: bool,
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
    /// Optional morph exit configuration for Dynamic Island transitions.
    pub morph_exit: Option<MorphExitState>,
}

impl Default for ModalMotionState {
    fn default() -> Self {
        Self {
            progress: 1.0,
            is_closing: false,
            origin: None,
            morph_exit: None,
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
            morph_exit: None,
        }
    }

    /// Creates a modal motion state configured for Dynamic Island morph exit.
    pub fn new_morph_exit(start_bounds: Bounds<Pixels>, target_bounds: Bounds<Pixels>) -> Self {
        Self {
            progress: 1.0,
            is_closing: true,
            origin: None,
            morph_exit: Some(MorphExitState {
                start_bounds,
                target_bounds,
                start_radius: px(16.0),
                target_radius: crate::tokens::RADIUS_LG,
            }),
        }
    }

    /// Mark the modal as closing and begin leaving.
    pub fn start_closing(&mut self) {
        self.is_closing = true;
    }

    /// Compute values during morph exit. Returns `None` if not in morph mode.
    pub fn compute_morph_values(&self) -> Option<MorphMotionValues> {
        let morph = self.morph_exit.as_ref()?;
        let raw_p = self.progress.clamp(0.0, 1.0);
        // During morph exit, progress goes from 1.0 down to 0.0.
        // Parameter t represents elapsed progress from 0.0 to 1.0.
        let t = (1.0 - raw_p).clamp(0.0, 1.0);
        let t_morph = ease_out_morph(t);

        let cur_x = morph.start_bounds.origin.x
            + (morph.target_bounds.origin.x - morph.start_bounds.origin.x) * t_morph;
        let cur_y = morph.start_bounds.origin.y
            + (morph.target_bounds.origin.y - morph.start_bounds.origin.y) * t_morph;
        let cur_w = morph.start_bounds.size.width
            + (morph.target_bounds.size.width - morph.start_bounds.size.width) * t_morph;
        let cur_h = morph.start_bounds.size.height
            + (morph.target_bounds.size.height - morph.start_bounds.size.height) * t_morph;

        let border_radius = morph.start_radius
            + (morph.target_radius - morph.start_radius) * t_morph;

        // Content fades out rapidly in first 80ms of 280ms (~0.2857 progress)
        let content_factor = (t / 0.2857).min(1.0);
        let inner_content_opacity = (1.0 - content_factor).max(0.0);

        // Destination toolbar preview fades in during 110ms ~ 250ms (0.38 ~ 0.88 progress)
        let dest_content_opacity = if t <= 0.38 {
            0.0
        } else if t >= 0.88 {
            1.0
        } else {
            let p = (t - 0.38) / 0.50;
            (p * p * (3.0 - 2.0 * p)).clamp(0.0, 1.0)
        };

        // Backdrop fades out in 180ms of 280ms (~0.6428 progress)
        let mask_factor = (t / 0.6428).min(1.0);
        let backdrop_opacity = (1.0 - motion_ease_out_circ(mask_factor)).max(0.0);

        let card_opacity = if t >= 0.98 {
            (1.0 - t) / 0.02
        } else {
            1.0
        };

        Some(MorphMotionValues {
            current_bounds: Bounds::new(Point::new(cur_x, cur_y), Size::new(cur_w, cur_h)),
            border_radius,
            inner_content_opacity,
            dest_content_opacity,
            backdrop_opacity,
            card_opacity,
            is_morphing: true,
        })
    }

    /// Check if the motion animation has completely finished.
    pub fn is_finished(&self) -> bool {
        if self.is_closing {
            self.progress <= 0.001
        } else {
            self.progress >= 0.999
        }
    }

    /// Compute coordinated render values with known card dimensions and custom target resting center.
    ///
    /// Implements Ant Design 6.6.3 curves with Desktop Clamped Directional Vector:
    /// - Vector-clamped travel distance (max 40px) along trigger direction to prevent jarring cross-screen motion.
    /// - Division-by-zero safety: near-zero distance (<1px) gracefully falls back to vertical float (-16px).
    /// - Keyboard fallback: `origin == None` gracefully falls back to -16px vertical settle (Spotlight/Raycast metaphor).
    /// - Decoupled backdrop mask timing (reaches 100% in 180ms).
    pub fn compute_card_values_at(
        &self,
        _window_size: Size<Pixels>,
        card_size: Size<Pixels>,
        target_center: Point<Pixels>,
    ) -> ModalMotionValues {
        let max_distance = 40.0_f32;
        let (target_offset_x, target_offset_y) = if let Some(origin) = self.origin {
            let dx = f32::from(origin.x - target_center.x);
            let dy = f32::from(origin.y - target_center.y);
            let distance = (dx * dx + dy * dy).sqrt();

            // Zero-division protection: if distance is near zero, fallback to vertical settle
            if distance > 1.0 {
                let scale = (max_distance / distance).min(1.0);
                (px(dx * scale), px(dy * scale))
            } else {
                (px(0.0), px(-16.0))
            }
        } else {
            // Keyboard shortcut fallback: -16px vertical float downwards into center
            (px(0.0), px(-16.0))
        };

        if self.is_closing {
            // Exit animation: Ant Design 6.6.3 motionEaseOutCirc cubic-bezier(0.08, 0.82, 0.17, 1)
            // Flies back towards origin and fades out smoothly
            let raw_p = self.progress.clamp(0.0, 1.0);
            let t = motion_ease_out_circ(raw_p);
            let backdrop_opacity = t;
            let card_opacity = t;
            let scale = 0.2 + 0.8 * t;
            let width = card_size.width;
            let height = card_size.height;
            let offset_x = target_offset_x * (1.0 - t);
            let offset_y = target_offset_y * (1.0 - t);

            ModalMotionValues {
                backdrop_opacity,
                card_opacity,
                scale,
                width,
                height,
                offset: Point::new(offset_x, offset_y),
            }
        } else {
            // Enter animation: Ant Design 6.6.3 motionEaseInOutCirc cubic-bezier(0.78, 0.14, 0.15, 0.86)
            // Emerges from trigger origin into resting center with smooth circular curve
            let raw_p = self.progress.clamp(0.0, 1.0);
            let t_enter = motion_ease_in_out_circ(raw_p);

            // Ant Design timing decoupling:
            // Mask quickly fades in over DURATION_MASK_ENTER (180ms) with motionEaseInOutCirc,
            // quickly establishing the dark backdrop canvas, while the dialog card emerges independently
            // and settles over DURATION_MODAL_ENTER (280ms).
            let mask_ratio = DURATION_MODAL_ENTER.as_secs_f32() / DURATION_MASK_ENTER.as_secs_f32();
            let mask_p = (raw_p * mask_ratio).min(1.0);
            let backdrop_opacity = motion_ease_in_out_circ(mask_p);

            let card_opacity = t_enter;
            let scale = 0.2 + 0.8 * t_enter;
            let width = card_size.width;
            let height = card_size.height;
            let offset_x = target_offset_x * (1.0 - t_enter);
            let offset_y = target_offset_y * (1.0 - t_enter);

            ModalMotionValues {
                backdrop_opacity,
                card_opacity,
                scale,
                width,
                height,
                offset: Point::new(offset_x, offset_y),
            }
        }
    }

    /// Compute coordinated render values with known card dimensions (centered in window).
    pub fn compute_card_values(
        &self,
        window_size: Size<Pixels>,
        card_size: Size<Pixels>,
    ) -> ModalMotionValues {
        let center = Point::new(window_size.width / 2.0, window_size.height / 2.0);
        self.compute_card_values_at(window_size, card_size, center)
    }

    /// Compute coordinated render values given the window's total viewport size and resting center.
    pub fn compute_values_at(
        &self,
        window_size: Size<Pixels>,
        target_center: Point<Pixels>,
    ) -> ModalMotionValues {
        self.compute_card_values_at(
            window_size,
            Size::new(px(600.0), px(450.0)),
            target_center,
        )
    }

    /// Compute coordinated render values given the window's total viewport size (default centered card).
    pub fn compute_values(&self, window_size: Size<Pixels>) -> ModalMotionValues {
        let center = Point::new(window_size.width / 2.0, window_size.height / 2.0);
        self.compute_values_at(window_size, center)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ease_in_cubic, ease_in_out, ease_out_bounce, ease_out_cubic, motion_ease_in_out_circ,
        motion_ease_out_circ, ModalMotionState,
    };
    use gpui::{Bounds, Point, Size, px};

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
        assert_eq!(motion_ease_in_out_circ(0.0), 0.0);
        assert_eq!(motion_ease_in_out_circ(1.0), 1.0);
        assert_eq!(motion_ease_out_circ(0.0), 0.0);
        assert_eq!(motion_ease_out_circ(1.0), 1.0);
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

        // At enter progress 0.0: clamped travel distance along origin vector to 40px
        let val_0 = state.compute_card_values(win_size, card_size);
        assert_eq!(val_0.backdrop_opacity, 0.0);
        assert_eq!(val_0.card_opacity, 0.0);
        assert_eq!(val_0.scale, 0.2);
        assert_eq!(val_0.width, px(600.0));
        assert_eq!(val_0.height, px(400.0));
        let dist = ((f32::from(val_0.offset.x) * f32::from(val_0.offset.x))
            + (f32::from(val_0.offset.y) * f32::from(val_0.offset.y)))
            .sqrt();
        assert!((dist - 40.0).abs() < 0.001, "distance was {dist}, expected 40.0px");

        // At enter progress 1.0: fully settled at center (0, 0) with scale 1.0
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

        // At exit progress 0.0: shrunk back toward clamped origin vector with scale 0.2 and opacity 0
        let mut state_exit = state;
        state_exit.start_closing();
        state_exit.progress = 0.0;
        let val_exit = state_exit.compute_card_values(win_size, card_size);
        assert_eq!(val_exit.backdrop_opacity, 0.0);
        assert_eq!(val_exit.card_opacity, 0.0);
        assert_eq!(val_exit.scale, 0.2);
        assert_eq!(val_exit.offset.x, val_0.offset.x);
        assert_eq!(val_exit.offset.y, val_0.offset.y);
    }

    #[test]
    fn test_zero_distance_and_keyboard_fallback() {
        let win_size = Size {
            width: px(1000.0),
            height: px(800.0),
        };
        let card_size = Size {
            width: px(600.0),
            height: px(400.0),
        };

        // 1. Keyboard trigger (origin is None): -16px vertical float
        let state_kb = ModalMotionState::new_opening(None);
        let val_kb = state_kb.compute_card_values(win_size, card_size);
        assert_eq!(val_kb.offset.x, px(0.0));
        assert_eq!(val_kb.offset.y, px(-16.0));

        // 2. Click origin exactly at center (distance < 1.0px): zero-division safe fallback
        let state_zero = ModalMotionState::new_opening(Some(Point::new(px(500.1), px(400.2))));
        let val_zero = state_zero.compute_card_values(win_size, card_size);
        assert_eq!(val_zero.offset.x, px(0.0));
        assert_eq!(val_zero.offset.y, px(-16.0));
        assert!(!f32::from(val_zero.offset.x).is_nan());
        assert!(!f32::from(val_zero.offset.y).is_nan());
    }

    #[test]
    fn test_backdrop_mask_timing_decoupling() {
        let win_size = Size {
            width: px(1000.0),
            height: px(800.0),
        };
        let card_size = Size {
            width: px(600.0),
            height: px(400.0),
        };
        let mut state = ModalMotionState::new_opening(None);

        // At t=0 (progress=0.0): mask is completely transparent
        state.progress = 0.0;
        let val_0 = state.compute_card_values(win_size, card_size);
        assert_eq!(val_0.backdrop_opacity, 0.0);

        // At t=100ms (~progress=0.3571): mask reaches ~75% (motionEaseInOutCirc)
        state.progress = 100.0 / 280.0;
        let val_mid = state.compute_card_values(win_size, card_size);
        assert!(
            val_mid.backdrop_opacity > 0.70 && val_mid.backdrop_opacity < 0.80,
            "backdrop was {}, expected ~0.747",
            val_mid.backdrop_opacity
        );

        // At t=180ms (~progress=0.6428): mask has completely reached 1.0 and settled
        state.progress = 180.0 / 280.0;
        let val_180 = state.compute_card_values(win_size, card_size);
        assert_eq!(val_180.backdrop_opacity, 1.0);
        // But the card animation is still actively running (progress < 1.0)
        assert!(state.progress < 1.0);

        // At t=280ms (progress=1.0): both mask and card are fully settled
        state.progress = 1.0;
        let val_1 = state.compute_card_values(win_size, card_size);
        assert_eq!(val_1.backdrop_opacity, 1.0);
        assert_eq!(val_1.offset.y, px(0.0));
    }

    #[test]
    fn test_dynamic_island_morph_exit() {
        let start_bounds = Bounds::new(Point::new(px(300.0), px(200.0)), Size::new(px(400.0), px(300.0)));
        let target_bounds = Bounds::new(Point::new(px(360.0), px(20.0)), Size::new(px(280.0), px(36.0)));
        let mut state = ModalMotionState::new_morph_exit(start_bounds, target_bounds);

        // At start of morph (progress=1.0, t=0.0):
        state.progress = 1.0;
        let val_start = state.compute_morph_values().expect("morph values should be available");
        assert_eq!(val_start.current_bounds.origin.x, px(300.0));
        assert_eq!(val_start.current_bounds.origin.y, px(200.0));
        assert_eq!(val_start.current_bounds.size.width, px(400.0));
        assert_eq!(val_start.current_bounds.size.height, px(300.0));
        assert_eq!(val_start.inner_content_opacity, 1.0);
        assert_eq!(val_start.dest_content_opacity, 0.0);
        assert_eq!(val_start.border_radius, px(16.0));

        // At 80ms into 280ms morph (~t=0.2857, progress = 1.0 - 0.2857 = 0.7143):
        // Inner form content should have completely faded out, preview not yet shown
        state.progress = 1.0 - (80.0 / 280.0);
        let val_80ms = state.compute_morph_values().unwrap();
        assert_eq!(val_80ms.inner_content_opacity, 0.0);
        assert_eq!(val_80ms.dest_content_opacity, 0.0);

        // At end of morph (progress=0.0, t=1.0):
        state.progress = 0.0;
        let val_end = state.compute_morph_values().unwrap();
        assert_eq!(val_end.current_bounds.origin.x, px(360.0));
        assert_eq!(val_end.current_bounds.origin.y, px(20.0));
        assert_eq!(val_end.current_bounds.size.width, px(280.0));
        assert_eq!(val_end.current_bounds.size.height, px(36.0));
        assert_eq!(val_end.border_radius, crate::tokens::RADIUS_LG);
        assert_eq!(val_end.backdrop_opacity, 0.0);
        assert_eq!(val_end.dest_content_opacity, 1.0);
    }
}
