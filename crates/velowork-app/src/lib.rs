#![recursion_limit = "4096"]
//! Velowork's UI/app layer: the GPUI views, the app coordinator, keybindings,
//! action dispatch, and the thin re-export shim modules over the lower-level
//! crates. Extracted out of the `velowork` binary so the binary stays a thin
//! entry point and this layer compiles as its own crate.

// The app-logic layer (global settings + workspace action glue) lives
// in `velowork-app-core`. Re-exported as `crate::settings` / `crate::workspace` so
// the moved code's `crate::settings::...` / `crate::workspace::...` references
// keep working unchanged.
pub use velowork_app_core::{settings, workspace};

// `macros.rs` declares `#[macro_export] macro_rules! impl_focusable`, which must
// stay exported at this crate's root so `impl_focusable!` resolves in the moved
// code (and as `velowork_app::impl_focusable!` from the binary).
#[macro_use]
mod macros;

pub mod action_dispatch;
pub mod ai_runtime;
pub mod app;
/// Unified application state aggregate (`AppState`) — the single composition
/// root that bundles the six state-domain stores.
pub mod app_state;
/// App bootstrap — explicit, single-responsibility `init_*` functions that
/// own each concern (settings, i18n, workspace, theme, stores, extensions, pty).
pub mod init;
pub mod elements;
pub mod font_cache;
pub mod keybindings;
pub mod logging;
/// 后台自动同步引擎（定时 + 配置变更触发 WebDAV 双向同步）。
pub mod sync_engine;
pub mod simple_root;
pub mod platform;
pub mod soft_close;
pub mod terminal;
pub mod theme;
pub mod ui;
pub mod views;

use std::sync::Arc;
use gpui::Global;

/// Global wrapper around Arc<PtyManager> for runtime access across GPUI contexts.
#[derive(Clone)]
pub struct GlobalPtyManager(pub Arc<velowork_terminal::PtyManager>);
impl Global for GlobalPtyManager {}
