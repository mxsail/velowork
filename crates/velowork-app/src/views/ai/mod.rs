//! AI view infrastructure: shared types, session entity, command extraction, and unified message rendering.

pub mod commands;
pub mod message_view;
pub mod session;
pub mod types;

pub use commands::*;
pub use message_view::*;
pub use session::*;
pub use types::*;
