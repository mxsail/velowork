//! Side panel views.
//!
//! This module contains views for side panels:
//! - Sidebar with project and terminal list
//! - Project columns for multi-project workspace
//! - Status bar at the bottom

pub mod project_panel;
pub use project_panel as project_column;
pub mod status_bar;
pub mod toast;
pub mod session_panel;
pub mod quick_command_panel;
pub use quick_command_panel as quick_commands_panel;
pub mod ai_assistant_panel;
pub mod tunnel_panel;
pub use tunnel_panel as tunnels_panel;
pub mod service_panel;
pub use service_panel as service_monitor_panel;
pub mod command_history_panel;

pub mod registry;

pub use registry::*;
pub use tunnel_panel::TunnelsPanel;
pub use service_panel::ServiceMonitorPanel;
pub use command_history_panel::CommandHistoryPanel;


