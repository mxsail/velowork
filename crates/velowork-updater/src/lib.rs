#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]
#![recursion_limit = "256"]

pub mod checker;
pub mod downloader;
pub mod installer;
pub mod orchestrator;
mod process;
mod status;
mod update_checker;

// Re-export public types used by the host app
pub use installer::{cleanup_old_binary, restart_app};
pub use orchestrator::{run_install, run_manual_check};
pub use status::{GlobalUpdateInfo, UpdateInfo, UpdateStatus, UpdateStatusWidget, UpdateWidgetEvent};
pub use update_checker::start_update_checker;

/// Initialize the updater: clean up old binary, set GlobalUpdateInfo, and start background checker.
/// Called by the host app at startup.
/// `app_version` is the host application's version (from root Cargo.toml).
/// `auto_check_enabled` is whether background periodic checks are enabled.
pub fn init(app_version: &str, auto_check_enabled: bool, cx: &mut gpui::App) {
    installer::cleanup_old_binary();

    let update_info = UpdateInfo::new(app_version.to_string(), auto_check_enabled);
    cx.set_global(GlobalUpdateInfo(update_info.clone()));

    start_update_checker(update_info, cx);
}
