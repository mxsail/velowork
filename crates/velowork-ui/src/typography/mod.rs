//! Typography system and cross-platform FontResolver for Velowork.

pub mod font_resolver;
pub mod system_fonts;
pub mod types;

pub use font_resolver::{FontResolver, FontResolverOptions};
pub use system_fonts::{detect_system_mono_font, detect_system_ui_font};
pub use types::{FontDomain, ResolvedFont, Typography};
