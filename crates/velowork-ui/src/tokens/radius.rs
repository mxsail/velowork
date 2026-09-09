//! Border radius tokens (unscaled — radii are resolution-independent).

use gpui::{px, Pixels};

/// Extra small radius (2px) - tight badges, dense chips
pub const RADIUS_XS: Pixels = px(2.0);

/// Small radius (3px) - badges, small elements
pub const RADIUS_SM: Pixels = px(3.0);

/// Standard radius (4px) - cards, containers
pub const RADIUS_STD: Pixels = px(4.0);

/// Medium radius (6px) - buttons, inputs, controls
pub const RADIUS_MD: Pixels = px(6.0);

/// Large radius (8px) - grouped cards, dialogs
pub const RADIUS_LG: Pixels = px(8.0);

/// Extra large radius (10px) - floating card containers, modal windows
pub const RADIUS_XL: Pixels = px(10.0);

/// Card container radius token (8px)
pub const RADIUS_CARD: Pixels = RADIUS_LG;
