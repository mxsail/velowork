//! Modal overlay views.
//!
//! This module contains views for modal overlays:
//! - Detached terminal windows
//! - Command palette
//! - Context menu
//! - Diff viewer
//! - File search
//! - File viewer
//! - Keybindings help
//! - Session manager
//! - Settings panel
//! - Shell selector
//! - Theme selector
//! - Worktree dialog

pub mod overlay_manager;
pub use overlay_manager::OverlayManager;
pub mod project_hover;
pub mod context_menu;
pub use menus::folder_context_menu;
pub use menus::project_context_menu;
pub mod detached_overlay;
pub mod detached_terminal;
pub mod keybindings_help;
pub mod log_console;
pub mod shell_selector_overlay;
pub mod tab_context_menu;
pub mod terminal_context_menu;
pub mod rename_directory_dialog;
pub mod transfer_popup;
pub mod lock_screen;
pub mod terminal_ai_inline;
pub use terminal_ai_inline::*;

pub mod dialogs;
pub mod menus;
pub mod pickers;
pub mod settings;
pub mod viewers;

pub use transfer_popup::{TransferPopup, TransferPopupEvent};
pub use shell_selector_overlay::{ShellSelectorOverlay, ShellSelectorOverlayEvent};
pub use dialogs::log_record_dialog::{LogRecordDialog, LogRecordDialogEvent};
pub use dialogs::log_saved_dialog::{LogSavedDialog, LogSavedDialogEvent};
pub use dialogs::quick_command_dialog::{
    QuickCommandDialog, QuickCommandDialogEvent, QuickCommandDialogMode,
};
pub use dialogs::quick_command_variable_dialog::{
    QuickCommandVarDialog, QuickCommandVarDialogEvent,
};
pub use menus::quick_command_context_menu::{
    open_quick_command_context_menu, QuickCommandContextMenuEvent, QuickCommandMenuRequest,
    QuickCommandMenuTarget,
};
pub use menus::command_history_context_menu::{
    open_command_history_context_menu, CommandHistoryContextMenuEvent, CommandHistoryMenuRequest,
};
pub use dialogs::tunnel_dialog::{TunnelDialog, TunnelDialogEvent, TunnelDialogMode};
pub use menus::service_context_menu::{
    open_service_context_menu, ServiceContextMenuEvent, ServiceMenuRequest, ServiceMenuTarget,
};
pub use menus::tunnel_context_menu::{
    open_tunnel_context_menu, TunnelContextMenuEvent, TunnelMenuRequest, TunnelMenuTarget,
};
