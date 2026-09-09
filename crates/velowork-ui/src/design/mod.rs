//! Design infrastructure — **frozen architectural base**.
//!
//! This module aggregates the core design subsystems (density, semantic palette,
//! control appearance, and resource management). After this freeze, consumers
//! should only *consume* these abstractions — no new design-layer abstractions
//! should be added.

pub mod appearance;
pub mod density;
pub mod resources;
pub mod semantic;
