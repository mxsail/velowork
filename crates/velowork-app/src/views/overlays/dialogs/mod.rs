//! Dialog overlays.

pub mod project_add_dialog;
pub use project_add_dialog as add_project_dialog;
pub mod project_manage_dialog;
pub use project_manage_dialog as manage_projects_dialog;
pub mod quick_command_dialog;
pub mod quick_command_variable_dialog;
pub use quick_command_variable_dialog as quick_command_var_dialog;
pub mod tunnel_dialog;
pub mod service_dialog;
pub mod log_record_dialog;
pub mod log_saved_dialog;
pub mod update_dialog;
pub mod about_dialog;
pub mod help_dialog;
pub mod session_dialog;
pub mod import_session_dialog;
pub mod project_export_dialog;
pub mod project_import_dialog;
pub mod terminal_color_scheme_dialog;
pub mod attachment_preview_dialog;

pub use service_dialog::{ServiceDialog, ServiceDialogEvent, ServiceDialogMode};
pub use import_session_dialog::{ImportSessionsDialog, ImportSessionsDialogEvent};
pub use project_export_dialog::{ProjectExportDialog, ProjectExportDialogEvent};
pub use project_import_dialog::{ProjectImportDialog, ProjectImportDialogEvent};
pub use terminal_color_scheme_dialog::{TerminalColorSchemeDialog, TerminalColorSchemeDialogEvent};
pub use attachment_preview_dialog::{AttachmentPreviewDialog, AttachmentPreviewDialogEvent};
