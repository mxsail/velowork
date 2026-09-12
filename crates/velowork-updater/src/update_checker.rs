use crate::status::{UpdateInfo, UpdateStatus};
use gpui::*;

/// Spawn the update checker loop on the App context.
pub fn start_update_checker(update_info: UpdateInfo, cx: &mut App) {
    let token = match update_info.try_start() {
        Some(t) => t,
        None => return,
    };

    cx.spawn(async move |cx| {
        // Initial delay — check cancellation every second
        for _ in 0..30 {
            if update_info.is_cancelled(token) {
                update_info.mark_stopped(token);
                return;
            }
            smol::Timer::after(std::time::Duration::from_secs(1)).await;
        }

        loop {
            if update_info.is_cancelled(token) {
                update_info.mark_stopped(token);
                return;
            }

            // If auto-check is disabled in settings, wait and re-check later
            if !update_info.is_auto_check_enabled() {
                for _ in 0..60 {
                    if update_info.is_cancelled(token) {
                        update_info.mark_stopped(token);
                        return;
                    }
                    smol::Timer::after(std::time::Duration::from_secs(5)).await;
                }
                continue;
            }

            // Pause while a manual check is in progress
            while update_info.is_manual_active() {
                if update_info.is_cancelled(token) {
                    update_info.mark_stopped(token);
                    return;
                }
                smol::Timer::after(std::time::Duration::from_secs(1)).await;
            }

            // If an update was already found, stop
            match update_info.status() {
                UpdateStatus::Ready { .. }
                | UpdateStatus::ReadyToRestart { .. }
                | UpdateStatus::Installing { .. }
                | UpdateStatus::BrewUpdate { .. } => {
                    update_info.mark_stopped(token);
                    return;
                }
                _ => {}
            }

            update_info.set_status(UpdateStatus::Checking);
            cx.update(|cx| cx.refresh_windows());

            match crate::checker::check_for_update(update_info.app_version()).await {
                Ok(Some(release)) => {
                    if update_info.is_homebrew() {
                        update_info.set_status(UpdateStatus::BrewUpdate {
                            version: release.version,
                        });
                        cx.update(|cx| cx.refresh_windows());
                        update_info.mark_stopped(token);
                        return;
                    }

                    if update_info.is_cancelled(token) || !update_info.is_auto_check_enabled() {
                        update_info.mark_stopped(token);
                        return;
                    }

                    let asset_url = release.asset_url;
                    let asset_name = release.asset_name;
                    let version = release.version;
                    let checksum_url = release.checksum_url;

                    update_info.set_status(UpdateStatus::Downloading {
                        version: version.clone(),
                        progress: 0,
                    });
                    cx.update(|cx| cx.refresh_windows());

                    let mut last_err: Option<anyhow::Error> = None;
                    for attempt in 0..3u32 {
                        if attempt > 0 {
                            let delay_secs = 30u64 * (1 << (attempt - 1));
                            for _ in 0..delay_secs {
                                if update_info.is_cancelled(token) {
                                    update_info.mark_stopped(token);
                                    return;
                                }
                                smol::Timer::after(std::time::Duration::from_secs(1)).await;
                            }
                            update_info.set_status(UpdateStatus::Downloading {
                                version: version.clone(),
                                progress: 0,
                            });
                            cx.update(|cx| cx.refresh_windows());
                        }

                        let download = crate::downloader::download_asset(
                            asset_url.clone(),
                            asset_name.clone(),
                            version.clone(),
                            update_info.clone(),
                            token,
                            checksum_url.clone(),
                        );
                        let mut download = std::pin::pin!(download);

                        let download_result: anyhow::Result<std::path::PathBuf> = loop {
                            let polled = std::future::poll_fn(|task_cx| {
                                match download.as_mut().poll(task_cx) {
                                    std::task::Poll::Ready(r) => std::task::Poll::Ready(Some(r)),
                                    std::task::Poll::Pending => std::task::Poll::Ready(None),
                                }
                            })
                            .await;
                            match polled {
                                Some(r) => break r,
                                None => {
                                    smol::Timer::after(std::time::Duration::from_millis(250)).await;
                                    cx.update(|cx| cx.refresh_windows());
                                }
                            }
                        };

                        match download_result {
                            Ok(path) => {
                                update_info.set_status(UpdateStatus::Ready {
                                    version: version.clone(),
                                    path,
                                });
                                cx.update(|cx| cx.refresh_windows());
                                update_info.mark_stopped(token);
                                return;
                            }
                            Err(e) => {
                                if update_info.is_cancelled(token) {
                                    update_info.mark_stopped(token);
                                    return;
                                }
                                log::warn!(
                                    "[updater] Download attempt {}/3 failed | error: {:#}",
                                    attempt + 1,
                                    e
                                );
                                last_err = Some(e);
                            }
                        }
                    }

                    if let Some(e) = last_err {
                        log::error!("[updater] Download failed after 3 attempts | error: {:#}", e);
                        update_info.set_status(UpdateStatus::Failed {
                            error: e.to_string(),
                        });
                        cx.update(|cx| cx.refresh_windows());
                    }
                }
                Ok(None) => {
                    update_info.set_status(UpdateStatus::Idle);
                    cx.update(|cx| cx.refresh_windows());
                }
                Err(e) => {
                    log::warn!("[updater] Background update check failed | error: {:#}", e);
                    update_info.set_status(UpdateStatus::Failed {
                        error: e.to_string(),
                    });
                    cx.update(|cx| cx.refresh_windows());
                }
            }

            // Keep Failed status visible for 60 seconds before clearing
            if matches!(update_info.status(), UpdateStatus::Failed { .. }) {
                for _ in 0..60 {
                    if update_info.is_cancelled(token) {
                        update_info.mark_stopped(token);
                        return;
                    }
                    smol::Timer::after(std::time::Duration::from_secs(1)).await;
                }
                if matches!(update_info.status(), UpdateStatus::Failed { .. }) {
                    update_info.set_status(UpdateStatus::Idle);
                    cx.update(|cx| cx.refresh_windows());
                }
            }

            // Wait 24 hours, checking cancellation every minute
            for _ in 0..(24 * 60) {
                if update_info.is_cancelled(token) {
                    update_info.mark_stopped(token);
                    return;
                }
                smol::Timer::after(std::time::Duration::from_secs(60)).await;
            }
        }
    })
    .detach();
}
