//! Transfer flying ghost animation overlay.
//!
//! Provides a macOS/browser-like parabolic flying silhouette when file
//! transfers are triggered, landing onto the status bar transfer icon.

use gpui::prelude::*;
use gpui::*;
use std::time::{Duration, Instant};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::motion::ease_out_cubic;
use velowork_ui::tokens::*;
use velowork_views_terminal::transfer_store::TransferFlyOrigin;

/// Total duration of the flying animation from origin to destination.
pub const FLY_DURATION: Duration = Duration::from_millis(420);

/// Maximum number of concurrent ghost items to render simultaneously.
pub const MAX_CONCURRENT_FLIES: usize = 5;

/// Base width and height of the ghost card at scale 1.0.
const BASE_CARD_SIZE: f32 = 36.0;

/// Base icon size at scale 1.0.
const BASE_ICON_SIZE: f32 = 20.0;

/// Height of the parabolic arc apex in pixels.
const ARC_HEIGHT: f32 = 56.0;

/// A single flying ghost animation item.
#[derive(Clone, Debug)]
pub struct TransferFlyItem {
    pub origin: TransferFlyOrigin,
    pub start_time: Instant,
    pub duration: Duration,
    pub is_multiple: bool,
}

/// Render geometry computed for a single frame.
#[derive(Clone, Debug)]
pub struct FlyRenderState {
    pub pos: Point<Pixels>,
    pub scale: f32,
    pub opacity: f32,
    pub is_multiple: bool,
}

impl TransferFlyItem {
    /// Computes the visual transform for the current frame.
    /// Returns `None` if the animation duration has expired.
    pub fn compute_geometry(
        &self,
        viewport: Size<Pixels>,
        target_pos: Point<Pixels>,
    ) -> Option<FlyRenderState> {
        let elapsed = self.start_time.elapsed();
        if elapsed >= self.duration {
            return None;
        }

        let t = (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0);

        // Determine starting coordinate
        let start_pt = match &self.origin {
            TransferFlyOrigin::Point(pt) => *pt,
            TransferFlyOrigin::DialogCenter => {
                point(viewport.width / 2.0, viewport.height * 0.45)
            }
        };

        // Horizontal motion: smooth ease-out-cubic
        let x_progress = ease_out_cubic(t);
        let cur_x = start_pt.x + (target_pos.x - start_pt.x) * x_progress;

        // Vertical motion: parabolic arc combined with vertical ease
        let y_progress = ease_out_cubic(t);
        let base_y = start_pt.y + (target_pos.y - start_pt.y) * y_progress;
        // Parabolic rise peaks at t = 0.5 with factor 1.0
        let arc = (4.0 * t * (1.0 - t)) * px(ARC_HEIGHT);
        let cur_y = base_y - arc;

        // Scale: shrink smoothly from 1.0 down to ~0.32
        let scale = (1.0 - 0.68 * ease_out_cubic(t)).max(0.32);

        // Opacity: high visibility for the first 80%, smooth fade-out in the final 20%
        let opacity = if t < 0.8 {
            0.90
        } else {
            0.90 * (1.0 - (t - 0.8) / 0.2)
        };

        Some(FlyRenderState {
            pos: point(cur_x, cur_y),
            scale,
            opacity,
            is_multiple: self.is_multiple,
        })
    }
}

/// Manages active flying transfer animations.
pub struct TransferFlyAnimationManager {
    items: Vec<TransferFlyItem>,
}

impl Default for TransferFlyAnimationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TransferFlyAnimationManager {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Add a new flying animation. Old items are pruned first.
    pub fn add(&mut self, origin: TransferFlyOrigin, is_multiple: bool) {
        self.items.retain(|i| i.start_time.elapsed() < i.duration);
        if self.items.len() < MAX_CONCURRENT_FLIES {
            self.items.push(TransferFlyItem {
                origin,
                start_time: Instant::now(),
                duration: FLY_DURATION,
                is_multiple,
            });
        }
    }

    /// Returns whether any animations are currently active.
    pub fn is_active(&self) -> bool {
        self.items.iter().any(|i| i.start_time.elapsed() < i.duration)
    }

    /// Advance active items and return whether any item has finished on this tick
    /// (to trigger target icon arrival feedback).
    pub fn tick(&mut self) -> (bool /* has_active */, bool /* has_arrived */) {
        let prev_len = self.items.len();
        self.items.retain(|i| i.start_time.elapsed() < i.duration);
        let arrived = self.items.len() < prev_len;
        (!self.items.is_empty(), arrived)
    }

    /// Render the flying ghost overlay above the window.
    pub fn render(
        &self,
        viewport: Size<Pixels>,
        target_pos: Point<Pixels>,
        p: SemanticPalette,
    ) -> Option<AnyElement> {
        let active_states: Vec<FlyRenderState> = self
            .items
            .iter()
            .filter_map(|item| item.compute_geometry(viewport, target_pos))
            .collect();

        if active_states.is_empty() {
            return None;
        }

        let overlay = deferred(
            div()
                .id("transfer-fly-overlay")
                .absolute()
                .inset_0()
                .children(active_states.into_iter().enumerate().map(|(idx, state)| {
                    let card_w = px(BASE_CARD_SIZE * state.scale);
                    let card_h = px(BASE_CARD_SIZE * state.scale);
                    let icon_sz = px(BASE_ICON_SIZE * state.scale);
                    let left = state.pos.x - card_w / 2.0;
                    let top = state.pos.y - card_h / 2.0;

                    div()
                        .id(ElementId::Name(format!("fly-item-{}", idx).into()))
                        .absolute()
                        .left(left)
                        .top(top)
                        .w(card_w)
                        .h(card_h)
                        // Secondary card for multi-file stacked effect
                        .when(state.is_multiple, |d| {
                            let offset = px(4.0 * state.scale);
                            d.child(
                                div()
                                    .absolute()
                                    .left(offset)
                                    .top(offset)
                                    .w(card_w)
                                    .h(card_h)
                                    .rounded(RADIUS_STD)
                                    .bg(p.surface_card)
                                    .border_1()
                                    .border_color(p.border_subtle)
                                    .shadow(elevation_menu_shadow())
                                    .opacity(state.opacity * 0.45),
                            )
                        })
                        // Main foreground card
                        .child(
                            div()
                                .relative()
                                .w(card_w)
                                .h(card_h)
                                .rounded(RADIUS_STD)
                                .bg(p.surface_card)
                                .border_1()
                                .border_color(p.border_active)
                                .shadow(elevation_menu_shadow())
                                .opacity(state.opacity)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    AppIcon::File
                                        .size(icon_sz)
                                        .text_color(p.text_primary),
                                ),
                        )
                })),
        );

        Some(overlay.into_any_element())
    }
}
