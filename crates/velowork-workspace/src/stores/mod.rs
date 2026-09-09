//! Unified state-layer stores.
//!
//! Each domain of application state has exactly ONE store — a GPUI `Model`
//! (`Entity`) that is the sole owner and sole writer of its data. UI components
//! never hold or mutate business data directly: they read through store query
//! methods and push changes through store mutation methods. Every mutation
//! emits a typed `*Event` so consumers subscribe to the exact change they
//! care about instead of blindly re-rendering on a global `cx.notify()`.
//!
//! `AppState` (in `velowork-app`) aggregates the six stores into one handle
//! reachable from anywhere via `cx.global::<AppState>()`. Each store also
//! exposes its own `GlobalXxx` wrapper for crate-level access without pulling
//! in the whole app (so low-level crates like `velowork-views-terminal`
//! can read/write without depending on `velowork-app`).

mod connection;
mod focus;
mod session;
mod service;
mod tunnel;
mod window;

pub use connection::{ConnectionEvent, ConnectionInfo, ConnectionStore, GlobalConnectionStore};
pub use focus::{FocusEvent, FocusStore, GlobalFocusStore};
pub use session::{GlobalSessionStore, SessionEvent, SessionStore};
pub use service::{GlobalServiceStore, ServiceEvent, ServiceStore};
pub use tunnel::{GlobalTunnelStore, TunnelEvent, TunnelStore};
pub use window::{GlobalWindowStore, WindowEntry, WindowEvent, WindowStore};
