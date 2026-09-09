//! Unified application state aggregate.
//!
//! `AppState` is the single composition root that bundles every state domain
//! into one global handle. It does NOT own any data itself — it only holds
//! references (`Entity`) to the per-domain stores. This keeps ownership where
//! it belongs (in each store) while giving the rest of the app one place to
//! reach any store.
//!
//! Access:
//! ```ignore
//! let stores = cx.global::<AppState>();
//! stores.session.read(cx).tree();
//! stores.session.update(cx, |s, cx| s.add_session(...));
//! ```
//!
//! For crate-level access (no app dependency) use the per-store globals
//! (`GlobalSessionStore`, `GlobalConnectionStore`, …) defined alongside each
//! store in `velowork-workspace::stores`.

use gpui::*;
use velowork_app_core::settings::SettingsState;
use crate::theme::AppTheme;
use velowork_workspace::stores::{ConnectionStore, FocusStore, SessionStore, WindowStore};

/// The unified state root. Holds one handle per state domain.
///
/// `settings` and `theme` reuse the existing `SettingsState` / `AppTheme`
/// entities (they already are good stores). The other four live in
/// `velowork_workspace::stores`.
#[derive(Clone)]
pub struct AppState {
    /// SSH session / connection-tree data (canonical owner).
    pub session: Entity<SessionStore>,
    /// Live SSH connection state.
    pub connection: Entity<ConnectionStore>,
    /// Global app settings.
    pub settings: Entity<SettingsState>,
    /// Theme mode + colors.
    pub theme: Entity<AppTheme>,
    /// Per-window metadata (bounds / active / focused).
    pub window: Entity<WindowStore>,
    /// App-level focus authority over per-window `FocusManager`s.
    pub focus: Entity<FocusStore>,
}

impl Global for AppState {}
