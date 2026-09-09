#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

//! velowork-state — Pure data types for workspace state.
//!
//! Holds the serializable data structures that describe a workspace:
//! projects, folders, layouts (re-exported from `velowork-layout`), and worktree
//! metadata. No GPUI, no behavior beyond a few pure helpers.

mod toast;
mod transient;
mod window_id;
mod window_state;
mod windows;
mod workspace_data;
mod ssh_sessions;
mod tunnels;
mod service_monitor;

pub use velowork_layout::{LayoutNode, SplitDirection};
pub use toast::{Toast, ToastAction, ToastActionStyle, ToastLevel};
pub use transient::{DropZone, FocusedTerminalState};
pub use window_id::WindowId;
pub use window_state::{ProjectLayoutMode, ProjectSortMode, WindowBounds, WindowState};
pub use workspace_data::{
    FolderData, ProjectData, WorkspaceData,
    is_bash_prompt_title,
};
pub use ssh_sessions::{SshSession, SessionTerminalOptions, SshAuthType, SessionTreeNode, SshSessionConfig, SshTestResult, IconColor, ProxyType, CompressionType, StrictHostKey, KeepAliveStrategy, SessionProtocol};
pub use tunnels::*;
pub use service_monitor::*;
