//! Layout management views.
//!
//! This module contains views for managing terminal layouts:
//! - Layout containers (tabs, splits)
//! - Split panes with resize handles
//! - Individual terminal panes
//! - Focus navigation between panes

pub mod layout_container;
pub mod navigation;
pub mod pane_drag;
pub mod split_pane;
pub mod session_labels;
mod tabs;
pub mod terminal_pane;
