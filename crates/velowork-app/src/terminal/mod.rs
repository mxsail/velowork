// Re-export everything from the velowork-terminal crate.
// This allows existing `use crate::terminal::*` imports to keep working.
pub use velowork_terminal::backend;
pub use velowork_terminal::pty_manager;
pub use velowork_terminal::session_backend;
pub use velowork_terminal::shell_config;
pub use velowork_terminal::terminal;

use gpui::App;
use velowork_i18n::i18n;
use velowork_terminal::shell_config::{available_shells, ShellType};
use velowork_ui::select::SelectOption;

/// 获取系统中所有可用 Shell 的规范化 `(显示名称, ShellType)` 列表。
///
/// 1. 自动过滤未安装/不可用的 Shell；
/// 2. 默认 Shell 自动解析为多语言文案（如中文下的「系统默认」）。
pub fn available_shell_items(cx: &App) -> Vec<(String, ShellType)> {
    let default_label = i18n!(cx, "ssh.local.default_shell");
    available_shells()
        .into_iter()
        .filter(|sh| sh.available)
        .map(|sh| {
            let label = if matches!(sh.shell_type, ShellType::Default) {
                default_label.clone()
            } else {
                sh.name
            };
            (label, sh.shell_type)
        })
        .collect()
}

/// 获取供 `SelectState<ShellType>` 直接消费的选项列表（如设置面板）。
pub fn available_shell_select_options(cx: &App) -> Vec<SelectOption<ShellType>> {
    available_shell_items(cx)
        .into_iter()
        .map(|(label, shell_type)| SelectOption::new(shell_type, label))
        .collect()
}
