//! Channel-driven dedicated Settings Persistence Actor.
//!
//! Provides non-blocking snapshot updates from the UI thread, dynamic debouncing
//! (200ms debounce with 1000ms max-wait throttle limit), sequential single-writer
//! disk I/O, and deterministic synchronous flush on application quit.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use velowork_workspace::settings::AppSettings;

/// Command sent to the dedicated persistence worker thread/task.
enum PersistCommand {
    /// UI thread updated the settings snapshot; worker should debounce and persist.
    Notify,
    /// Synchronous flush request; worker must immediately write latest snapshot and signal ACK.
    Flush(SyncSender<()>),
}

/// Actor handle for settings persistence.
#[derive(Clone)]
pub struct SettingsPersister {
    inner: Arc<PersisterInner>,
}

struct PersisterInner {
    snapshot: Mutex<Option<AppSettings>>,
    tx: smol::channel::Sender<PersistCommand>,
    last_saved_hash: AtomicU64,
}

pub(crate) fn hash_settings(settings: &AppSettings) -> u64 {
    use std::hash::{Hash, Hasher};
    let json = serde_json::to_string(settings).unwrap_or_default();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    json.hash(&mut hasher);
    hasher.finish()
}

impl SettingsPersister {
    /// Spawn the persistence actor with initial settings snapshot.
    pub fn spawn(initial_settings: &AppSettings) -> Self {
        let initial_hash = hash_settings(initial_settings);
        let (tx, rx) = smol::channel::bounded::<PersistCommand>(64);

        let inner = Arc::new(PersisterInner {
            snapshot: Mutex::new(None),
            tx,
            last_saved_hash: AtomicU64::new(initial_hash),
        });

        let worker_inner = Arc::clone(&inner);
        smol::spawn(async move {
            Self::worker_loop(worker_inner, rx).await;
        })
        .detach();

        Self { inner }
    }

    /// Send a new settings snapshot to be persisted (completely non-blocking for the caller).
    pub fn send_save(&self, settings: AppSettings) {
        *self.inner.snapshot.lock() = Some(settings);
        let _ = self.inner.tx.try_send(PersistCommand::Notify);
    }

    /// Synchronously flush any pending settings to disk (blocking wait with timeout).
    /// Called during app quit, profile switch, or critical synchronization boundaries.
    pub fn flush_sync(&self) {
        // If there is nothing pending in the snapshot buffer, return immediately.
        let has_pending = self.inner.snapshot.lock().is_some();
        if !has_pending {
            return;
        }

        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel::<()>(1);
        if self.inner.tx.send_blocking(PersistCommand::Flush(ack_tx)).is_ok() {
            let _ = ack_rx.recv_timeout(Duration::from_millis(1500));
        }
    }

    /// Worker main loop with adaptive debounce and max-wait throttle.
    async fn worker_loop(
        inner: Arc<PersisterInner>,
        rx: smol::channel::Receiver<PersistCommand>,
    ) {
        let debounce_dur = Duration::from_millis(200);
        let max_wait_dur = Duration::from_millis(1000);
        let mut first_pending_at: Option<Instant> = None;

        while let Ok(cmd) = rx.recv().await {
            match cmd {
                PersistCommand::Notify => {
                    if first_pending_at.is_none() {
                        first_pending_at = Some(Instant::now());
                    }

                    loop {
                        let elapsed = first_pending_at.map(|t| t.elapsed()).unwrap_or_default();
                        if elapsed >= max_wait_dur {
                            // Max-wait throttle limit reached, force flush now.
                            first_pending_at = None;
                            Self::do_persist(&inner).await;
                            break;
                        }

                        let remaining_max = max_wait_dur.saturating_sub(elapsed);
                        let current_timeout = debounce_dur.min(remaining_max);

                        enum Event {
                            Timeout,
                            NewCmd(Result<PersistCommand, smol::channel::RecvError>),
                        }

                        let timer = smol::Timer::after(current_timeout);
                        let event = smol::future::race(
                            async {
                                timer.await;
                                Event::Timeout
                            },
                            async { Event::NewCmd(rx.recv().await) },
                        )
                        .await;

                        match event {
                            Event::Timeout => {
                                first_pending_at = None;
                                Self::do_persist(&inner).await;
                                break;
                            }
                            Event::NewCmd(Ok(PersistCommand::Notify)) => {
                                // Another update arrived while waiting, continue debounce loop.
                                continue;
                            }
                            Event::NewCmd(Ok(PersistCommand::Flush(ack))) => {
                                first_pending_at = None;
                                Self::do_persist(&inner).await;
                                let _ = ack.send(());
                                break;
                            }
                            Event::NewCmd(Err(_)) => {
                                // Channel disconnected, flush remaining snapshot and exit.
                                Self::do_persist(&inner).await;
                                return;
                            }
                        }
                    }
                }
                PersistCommand::Flush(ack) => {
                    first_pending_at = None;
                    Self::do_persist(&inner).await;
                    let _ = ack.send(());
                }
            }
        }

        // Final cleanup upon channel drop
        Self::do_persist(&inner).await;
    }

    /// Atomically extract the latest snapshot, check hash, and execute off-thread blocking write.
    async fn do_persist(inner: &PersisterInner) {
        let snapshot = inner.snapshot.lock().take();
        if let Some(settings) = snapshot {
            let current_hash = hash_settings(&settings);
            if current_hash == inner.last_saved_hash.load(Ordering::Relaxed) {
                return; // Content identical to last saved state, skip disk IO
            }

            // Run blocking atomic write under SETTINGS_LOCK on the blocking threadpool.
            let save_result = smol::unblock(move || {
                velowork_workspace::settings::save_settings(&settings)
            })
            .await;

            match save_result {
                Ok(()) => {
                    inner.last_saved_hash.store(current_hash, Ordering::Relaxed);
                    log::debug!("[settings:persister] Successfully saved settings to disk");
                }
                Err(e) => {
                    log::error!("[settings:persister] Failed to save settings to disk | error: {:#}", e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_settings_consistency() {
        let s1 = AppSettings::default();
        let mut s2 = AppSettings::default();
        assert_eq!(hash_settings(&s1), hash_settings(&s2));

        s2.font_size = 24.0;
        assert_ne!(hash_settings(&s1), hash_settings(&s2));
    }

    #[test]
    fn test_persister_flush_empty() {
        let initial = AppSettings::default();
        let persister = SettingsPersister::spawn(&initial);
        // Flush when nothing was sent should return quickly without error
        persister.flush_sync();
    }

    #[test]
    fn test_persister_send_and_flush() {
        let initial = AppSettings::default();
        let persister = SettingsPersister::spawn(&initial);
        let mut modified = initial.clone();
        modified.font_size = 32.0;
        persister.send_save(modified.clone());
        persister.flush_sync();
        assert_eq!(
            persister.inner.last_saved_hash.load(Ordering::Relaxed),
            hash_settings(&modified)
        );
    }
}
