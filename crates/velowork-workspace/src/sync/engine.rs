//! 自动同步引擎的「信号」基础设施。
//!
//! - `notify_config_changed`：由设置层（`save_and_notify`）在任意配置变更后调用，
//!   唤醒后台的自动同步循环，实现「配置修改即触发同步」。
//! - `register_sync_signal`：由真正的引擎循环（位于 `velowork-app`，因其需要读取
//!   `SettingsState` 并操作 UI 提示）在启动时注册发送端。
//!
//! 之所以把信号全局放在本 crate：设置层（`velowork-app-core`）依赖本 crate，
//! 但本 crate 不能反向依赖 `velowork-app-core`，故信号通道在此落地，引擎循环在
//! 上层组装。

use std::sync::Mutex;

use futures::channel::mpsc;

/// 全局信号发送端。配置变更时通过它唤醒同步循环。
static SIGNAL: Mutex<Option<mpsc::UnboundedSender<()>>> = Mutex::new(None);

/// 注册信号发送端（引擎启动时调用一次）。
pub fn register_sync_signal(tx: mpsc::UnboundedSender<()>) {
    *SIGNAL.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
}

/// 通知后台同步循环「配置已变更，请尽快同步」。
///
/// 若引擎尚未启动（发送端未注册），则静默忽略——启动后会按定时策略补齐。
pub fn notify_config_changed() {
    if let Some(tx) = SIGNAL.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.unbounded_send(());
    }
}
