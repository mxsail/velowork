//! Headless app-logic layer: global observable settings and the
//! action-execution glue over the workspace. Decoupled from the UI views and
//! the app coordinator (which still live in the `velowork` binary for now).

pub mod settings;
pub mod settings_persister;
pub mod workspace;
