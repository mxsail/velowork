use gpui::*;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::h_flex;
use velowork_ui::tokens::ui_text_sm;

/// Status of the update process.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    #[allow(dead_code)]
    Available {
        version: String,
        asset_url: String,
        asset_name: String,
    },
    Downloading {
        version: String,
        progress: u8,
    },
    Ready {
        version: String,
        path: std::path::PathBuf,
    },
    Installing {
        version: String,
    },
    ReadyToRestart {
        version: String,
    },
    BrewUpdate {
        version: String,
    },
    Failed {
        error: String,
    },
}

struct UpdateInfoInner {
    status: UpdateStatus,
    dismissed: bool,
    is_homebrew: bool,
    manual_check_active: bool,
}

/// Thread-safe shared update state, readable from any thread/view.
#[derive(Clone)]
pub struct UpdateInfo {
    inner: Arc<Mutex<UpdateInfoInner>>,
    running: Arc<AtomicBool>,
    cancel_token: Arc<AtomicU64>,
    app_version: Arc<String>,
    auto_check_enabled: Arc<AtomicBool>,
}

impl UpdateInfo {
    pub fn new(app_version: String, auto_check_enabled: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(UpdateInfoInner {
                status: UpdateStatus::Idle,
                dismissed: false,
                is_homebrew: is_homebrew_install(),
                manual_check_active: false,
            })),
            running: Arc::new(AtomicBool::new(false)),
            cancel_token: Arc::new(AtomicU64::new(0)),
            app_version: Arc::new(app_version),
            auto_check_enabled: Arc::new(AtomicBool::new(auto_check_enabled)),
        }
    }

    pub fn app_version(&self) -> String {
        (*self.app_version).clone()
    }

    pub fn status(&self) -> UpdateStatus {
        self.inner.lock().status.clone()
    }

    pub fn set_status(&self, status: UpdateStatus) {
        let mut inner = self.inner.lock();
        if matches!(
            status,
            UpdateStatus::Available { .. }
                | UpdateStatus::Downloading { .. }
                | UpdateStatus::Ready { .. }
                | UpdateStatus::Installing { .. }
                | UpdateStatus::ReadyToRestart { .. }
                | UpdateStatus::BrewUpdate { .. }
                | UpdateStatus::Failed { .. }
        ) {
            inner.dismissed = false;
        }
        inner.status = status;
    }

    pub fn is_homebrew(&self) -> bool {
        self.inner.lock().is_homebrew
    }

    pub fn is_dismissed(&self) -> bool {
        self.inner.lock().dismissed
    }

    pub fn dismiss(&self) {
        self.inner.lock().dismissed = true;
    }

    pub fn try_start_manual(&self) -> bool {
        let mut inner = self.inner.lock();
        if inner.manual_check_active {
            return false;
        }
        if matches!(
            inner.status,
            UpdateStatus::Checking | UpdateStatus::Downloading { .. }
        ) {
            return false;
        }
        inner.manual_check_active = true;
        inner.dismissed = false;
        true
    }

    pub fn is_manual_active(&self) -> bool {
        self.inner.lock().manual_check_active
    }

    pub fn finish_manual(&self) {
        self.inner.lock().manual_check_active = false;
    }

    pub fn try_start(&self) -> Option<u64> {
        if self.inner.lock().manual_check_active {
            return None;
        }
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            Some(self.cancel_token.load(Ordering::SeqCst))
        } else {
            None
        }
    }

    pub fn cancel(&self) {
        self.cancel_token.fetch_add(1, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self, token: u64) -> bool {
        self.cancel_token.load(Ordering::SeqCst) != token
    }

    pub fn current_token(&self) -> u64 {
        self.cancel_token.load(Ordering::SeqCst)
    }

    pub fn mark_stopped(&self, token: u64) {
        if self.cancel_token.load(Ordering::SeqCst) == token {
            self.running.store(false, Ordering::SeqCst);
        }
    }

    pub fn is_auto_check_enabled(&self) -> bool {
        self.auto_check_enabled.load(Ordering::Relaxed)
    }

    pub fn set_auto_check_enabled(&self, enabled: bool) {
        self.auto_check_enabled.store(enabled, Ordering::Relaxed);
    }
}

/// Detect if running from a Homebrew installation.
pub fn is_homebrew_install() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| {
            let s = p.to_string_lossy();
            s.contains("/Caskroom/") || s.contains("/Cellar/")
        })
        .unwrap_or(false)
}

/// GPUI global wrapper for UpdateInfo.
#[derive(Clone)]
pub struct GlobalUpdateInfo(pub UpdateInfo);

impl Global for GlobalUpdateInfo {}

fn open_url(url: &str) {
    velowork_core::process::open_url(url);
}

/// Events emitted by the UpdateStatusWidget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateWidgetEvent {
    OpenDialog,
}

/// Status bar widget that shows update status.
pub struct UpdateStatusWidget;

impl EventEmitter<UpdateWidgetEvent> for UpdateStatusWidget {}

impl UpdateStatusWidget {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for UpdateStatusWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(update_info) = cx.try_global::<GlobalUpdateInfo>() else {
            return div().size_0().into_any_element();
        };
        let info = &update_info.0;
        if info.is_dismissed() {
            return div().size_0().into_any_element();
        }

        let p = SemanticPalette::from_context(cx);

        match info.status() {
            UpdateStatus::Ready { version, .. } => {
                let release_url = format!(
                    "https://github.com/mxsail/velowork/releases/tag/v{}",
                    version
                );
                h_flex()
                    .id("update-ready")
                    .gap(px(6.0))
                    .items_center()
                    .text_size(ui_text_sm(cx))
                    .child(
                        div()
                            .id("update-install")
                            .cursor_pointer()
                            .text_color(p.status_success)
                            .child(i18n!(cx, "update.available"))
                            .on_click(cx.listener(|_this, _, _window, cx| {
                                cx.emit(UpdateWidgetEvent::OpenDialog);
                            })),
                    )
                    .child(
                        div()
                            .id("whats-new")
                            .cursor_pointer()
                            .text_color(p.text_muted)
                            .hover(|s| s.text_color(p.text_primary))
                            .child(i18n!(cx, "update.whats_new"))
                            .on_click(move |_, _, _cx| {
                                open_url(&release_url);
                            }),
                    )
                    .into_any_element()
            }
            UpdateStatus::Installing { version } => {
                div()
                    .px(px(6.0))
                    .py(px(1.0))
                    .text_color(p.status_warning)
                    .text_size(ui_text_sm(cx))
                    .child(format!("{}: v{}...", i18n!(cx, "update.installing"), version))
                    .into_any_element()
            }
            UpdateStatus::ReadyToRestart { .. } => {
                div()
                    .id("update-restart")
                    .cursor_pointer()
                    .px(px(6.0))
                    .py(px(1.0))
                    .text_color(p.status_success)
                    .text_size(ui_text_sm(cx))
                    .child(i18n!(cx, "update.restart_to_update"))
                    .on_click(move |_, _, cx| {
                        crate::installer::restart_app(cx);
                    })
                    .into_any_element()
            }
            UpdateStatus::Downloading { version, progress } => {
                h_flex()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_color(p.status_warning)
                            .text_size(ui_text_sm(cx))
                            .child(format!(
                                "{} v{}... {}%",
                                i18n!(cx, "update.downloading"),
                                version,
                                progress
                            )),
                    )
                    .into_any_element()
            }
            UpdateStatus::Checking => {
                div()
                    .px(px(6.0))
                    .py(px(1.0))
                    .text_color(p.text_muted)
                    .text_size(ui_text_sm(cx))
                    .child(i18n!(cx, "update.checking"))
                    .into_any_element()
            }
            UpdateStatus::Failed { ref error } => {
                let info_dismiss = info.clone();
                div()
                    .id("update-failed")
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_color(p.status_error)
                            .text_size(ui_text_sm(cx))
                            .child(format!("{}: {}", i18n!(cx, "update.failed"), error)),
                    )
                    .child(
                        div()
                            .id("update-failed-dismiss")
                            .cursor_pointer()
                            .text_color(p.text_muted)
                            .text_size(ui_text_sm(cx))
                            .child("✕")
                            .on_click(move |_, _, _cx| {
                                info_dismiss.dismiss();
                            }),
                    )
                    .into_any_element()
            }
            UpdateStatus::BrewUpdate { version } => {
                let info_dismiss = info.clone();
                div()
                    .id("update-brew")
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_color(p.text_muted)
                            .text_size(ui_text_sm(cx))
                            .child(format!("v{} — {}", version, i18n!(cx, "update.brew"))),
                    )
                    .child(
                        div()
                            .id("update-dismiss")
                            .cursor_pointer()
                            .text_color(p.text_muted)
                            .text_size(ui_text_sm(cx))
                            .child("✕")
                            .on_click(move |_, _, _cx| {
                                info_dismiss.dismiss();
                            }),
                    )
                    .into_any_element()
            }
            _ => div().size_0().into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{UpdateInfo, UpdateStatus};

    #[test]
    fn test_update_info_lifecycle() {
        let info = UpdateInfo::new("0.1.0".to_string(), true);
        assert_eq!(info.app_version(), "0.1.0");
        assert_eq!(info.status(), UpdateStatus::Idle);
        assert!(!info.is_dismissed());
        assert!(info.is_auto_check_enabled());

        // Toggle auto_check
        info.set_auto_check_enabled(false);
        assert!(!info.is_auto_check_enabled());
        info.set_auto_check_enabled(true);
        assert!(info.is_auto_check_enabled());

        // Test dismiss logic
        info.dismiss();
        assert!(info.is_dismissed());

        // Setting a new active status un-dismisses it
        info.set_status(UpdateStatus::Checking);
        assert!(info.is_dismissed()); // Checking doesn't un-dismiss
        info.set_status(UpdateStatus::Available {
            version: "0.2.0".to_string(),
            asset_url: "https://example.com/asset".to_string(),
            asset_name: "velowork-update".to_string(),
        });
        assert!(!info.is_dismissed()); // Available does un-dismiss
    }

    #[test]
    fn test_cancellation_tokens() {
        let info = UpdateInfo::new("0.1.0".to_string(), true);
        let token1 = info.try_start().expect("should start first time");
        assert_eq!(token1, 0);
        assert!(!info.is_cancelled(token1));

        // Concurrent start should return None
        assert!(info.try_start().is_none());

        // Cancel
        info.cancel();
        assert!(info.is_cancelled(token1));

        // Can start again with new token
        let token2 = info.try_start().expect("should start after cancel");
        assert_ne!(token1, token2);
        assert!(!info.is_cancelled(token2));
    }
}
