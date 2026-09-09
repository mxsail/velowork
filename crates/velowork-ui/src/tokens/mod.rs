//! Design tokens for consistent UI spacing, sizing, typography and color usage.
//!
//! This module defines named constants and scaled helper functions so that every
//! crate can consume a single source of truth for UI dimensions and fonts.
//!
//! ## Two scaling knobs
//!
//! * `ui_scale` (settings, percent, default `100`, range `80..=200`) is the
//!   **global UI zoom**. It affects *everything* — text sizes **and** spacing —
//!   so the whole interface grows/shrinks together.
//! * `ui_font_size` (settings, px, default `13`) is a *fine text-size trim*
//!   applied only to text. It lets a user nudge body text without resizing the
//!   chrome around it.
//!
//! Both compose with an OS-derived font scale (registered once at startup via
//! [`scale::set_system_font_scale`]) so the app honors the platform's
//! accessibility font size where detectable.
//!
//! ## Submodules
//!
//! * [`global`] — `Global*` providers registered at startup.
//! * [`scale`] — zoom/text scale factors.
//! * [`spacing`] — padding/margin/gap tokens.
//! * [`typography`] — text size tokens.
//! * [`radius`] — border radius tokens.
//! * [`icon`] — icon size tokens.
//! * [`component`] — component height tokens.

pub mod component;
pub mod global;
pub mod icon;
pub mod radius;
pub mod scale;
pub mod shadow;
pub mod sizing;
pub mod spacing;
pub mod typography;

// Re-export everything at the `tokens::` level for backward compatibility.
// Existing code using `tokens::SPACE_LG` or `tokens::ui_text_md(cx)` continues
// to work without changes.

pub use component::*;
pub use global::*;
pub use icon::*;
pub use radius::*;
pub use scale::*;
pub use shadow::*;
pub use sizing::*;
pub use spacing::*;
pub use typography::*;
