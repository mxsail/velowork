//! Global transfer store backing the status-bar transfer manager.
//!
//! SFTP uploads/downloads register a `TransferTask` here on start and update
//! progress as they stream. The status bar reads this store to color its
//! transfer button and to render the transfer popup.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use gpui::*;

/// Direction of a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// Origin location for the transfer flying animation.
#[derive(Clone, Debug)]
pub enum TransferFlyOrigin {
    /// Explicit coordinate in window space (e.g. from SFTP selected row or panel center).
    Point(Point<Pixels>),
    /// Center of the active application window/viewport (for OS file dialog collapse).
    DialogCenter,
}

/// Event emitted when a transfer starts and a flying ghost animation should play.
#[derive(Clone, Debug)]
pub struct TransferFlyEvent {
    pub origin: TransferFlyOrigin,
    pub is_multiple: bool,
}

/// Lifecycle status of a transfer task.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransferStatus {
    /// Actively transferring.
    #[default]
    Active,
    /// Paused by the user (upload/download still resumes later).
    Paused,
    /// Failed with an error.
    Error,
    /// Completed successfully.
    Complete,
}

/// A single SFTP upload or download.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TransferTask {
    pub id: String,
    pub name: String,
    pub direction: TransferDirection,
    /// Local side (path on this machine).
    pub local_path: String,
    /// Remote side (path on the SSH/SFTP server).
    pub remote_path: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub status: TransferStatus,
    /// Current throughput in bytes/sec.
    pub speed_bps: f64,
    pub error: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub completed_at: Option<i64>,
}

impl TransferTask {
    /// Progress percentage in `0.0..=100.0`.
    pub fn progress(&self) -> f32 {
        if self.total_bytes == 0 {
            return if self.status == TransferStatus::Complete {
                100.0
            } else {
                0.0
            };
        }
        (self.transferred_bytes as f32 / self.total_bytes as f32 * 100.0).clamp(0.0, 100.0)
    }

    /// Estimated time remaining in seconds.
    ///
    /// Returns `None` if not transferring, if throughput is 0 or negative,
    /// or if already finished. Capped at 30 days to avoid displaying absurd numbers.
    pub fn eta_seconds(&self) -> Option<u64> {
        if self.status != TransferStatus::Active {
            return None;
        }
        if self.speed_bps <= 0.0 || self.total_bytes == 0 || self.transferred_bytes >= self.total_bytes {
            return None;
        }
        let remaining_bytes = self.total_bytes.saturating_sub(self.transferred_bytes);
        let secs = (remaining_bytes as f64 / self.speed_bps).ceil() as u64;
        Some(secs.min(2_592_000))
    }
}

/// Format ETA seconds into human-readable string (`01:25`, `00:45`, `02:15:30`).
pub fn format_eta(secs: Option<u64>) -> String {
    let Some(s) = secs else {
        return "--".to_string();
    };
    if s < 60 {
        format!("00:{:02}", s)
    } else if s < 3600 {
        format!("{:02}:{:02}", s / 60, s % 60)
    } else {
        format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    }
}

/// Format UNIX timestamp into user-friendly time string.
/// Same day: `HH:MM:SS`. Across days: `MM-DD HH:MM`.
pub fn format_timestamp(ts: Option<i64>) -> String {
    let Some(ts) = ts else {
        return String::new();
    };
    let Some(dt) = chrono::DateTime::from_timestamp(ts, 0) else {
        return String::new();
    };
    let local_dt: chrono::DateTime<chrono::Local> = chrono::DateTime::from(dt);
    let today = chrono::Local::now().date_naive();
    if local_dt.date_naive() == today {
        local_dt.format("%H:%M:%S").to_string()
    } else {
        local_dt.format("%m-%d %H:%M").to_string()
    }
}

/// Humanize transfer errors: convert raw connection/socket drop errors
/// into localized, friendly messages.
pub fn humanize_transfer_error(err_str: &str, disconnect_msg: &str) -> String {
    let lower = err_str.to_lowercase();
    if lower.contains("channel closed")
        || lower.contains("connection reset")
        || lower.contains("broken pipe")
        || lower.contains("unexpectedeof")
        || lower.contains("unexpected eof")
        || lower.contains("closed by server")
        || lower.contains("session disconnected")
        || lower.contains("not connected")
        || lower.contains("timed out")
        || lower.contains("timeout")
    {
        disconnect_msg.to_string()
    } else {
        err_str.to_string()
    }
}

/// Returns the path to `<data_root>/transfers.json`.
fn transfers_file_path() -> Option<std::path::PathBuf> {
    if let Some(dr) = velowork_core::data_root::try_current() {
        Some(dr.root().join("transfers.json"))
    } else {
        Some(velowork_core::data_root::system_default_data_root().join("transfers.json"))
    }
}

/// Progress published by the SFTP I/O task and polled by a GPUI ticker
/// so the status-bar list updates live without round-tripping through
/// the async runtime on every chunk.
#[derive(Clone, Debug, Default)]
pub struct TransferProgress {
    pub transferred: u64,
    pub total: u64,
    pub speed_bps: f64,
    pub done: bool,
    pub status: TransferStatus,
    pub error: Option<String>,
}

static NEXT_TRANSFER_ID: AtomicU64 = AtomicU64::new(0);

/// Monotonic, process-unique transfer id.
pub fn next_transfer_id() -> String {
    let n = NEXT_TRANSFER_ID.fetch_add(1, Ordering::Relaxed);
    format!("xfer-{}", n)
}

pub const TRANSFER_CONTROL_ACTIVE: u8 = 0;
pub const TRANSFER_CONTROL_PAUSED: u8 = 1;
pub const TRANSFER_CONTROL_CANCELLED: u8 = 2;

/// Observable store of all in-flight and recently-finished transfers.
#[derive(Clone, Debug, Default)]
pub struct TransferStore {
    pub tasks: Vec<TransferTask>,
    pub controls: std::collections::HashMap<String, Arc<std::sync::atomic::AtomicU8>>,
    /// Tracks whether an unacknowledged error occurred in the current session.
    pub has_unseen_error: bool,
}

impl TransferStore {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            controls: std::collections::HashMap::new(),
            has_unseen_error: false,
        }
    }

    /// Mark all errors as seen (e.g. when opening the transfer popup).
    pub fn mark_errors_seen(&mut self) {
        self.has_unseen_error = false;
    }

    /// Register a control handle for in-flight pause/resume/cancellation.
    pub fn register_control(&mut self, id: &str, control: Arc<std::sync::atomic::AtomicU8>) {
        self.controls.insert(id.to_string(), control);
    }

    /// Retrieve the control handle for a transfer task.
    pub fn get_control(&self, id: &str) -> Option<Arc<std::sync::atomic::AtomicU8>> {
        self.controls.get(id).cloned()
    }

    /// Insert or replace a task by id.
    pub fn add(&mut self, mut task: TransferTask) {
        if task.created_at.is_none() {
            task.created_at = Some(chrono::Local::now().timestamp());
        }
        self.tasks.retain(|t| t.id != task.id);
        self.tasks.push(task);
    }

    pub fn get(&self, id: &str) -> Option<&TransferTask> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// Update transferred bytes + speed for an in-flight task.
    pub fn update_progress(&mut self, id: &str, transferred: u64, total: u64, speed_bps: f64) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.transferred_bytes = transferred;
            if total > 0 {
                t.total_bytes = total;
            }
            t.speed_bps = speed_bps;
        }
    }

    pub fn set_status(&mut self, id: &str, status: TransferStatus, error: Option<String>) {
        let mut finished = false;
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.status = status;
            t.error = error;
            if status == TransferStatus::Complete || status == TransferStatus::Error {
                if t.completed_at.is_none() {
                    t.completed_at = Some(chrono::Local::now().timestamp());
                }
                finished = true;
            }
        }
        if status == TransferStatus::Error {
            self.has_unseen_error = true;
        }
        if let Some(ctrl) = self.controls.get(id) {
            match status {
                TransferStatus::Active => ctrl.store(TRANSFER_CONTROL_ACTIVE, Ordering::Relaxed),
                TransferStatus::Paused => ctrl.store(TRANSFER_CONTROL_PAUSED, Ordering::Relaxed),
                _ => {}
            }
        }
        if finished {
            self.save_persisted();
        }
    }

    /// Mark all active/paused transfers for a given session as failed (e.g. on SSH disconnect).
    pub fn fail_session_transfers(&mut self, session_id: &str, error_msg: String) {
        let now = chrono::Local::now().timestamp();
        let mut changed = false;
        for t in &mut self.tasks {
            if (t.status == TransferStatus::Active || t.status == TransferStatus::Paused)
                && t.session_id.as_deref() == Some(session_id)
            {
                t.status = TransferStatus::Error;
                t.error = Some(error_msg.clone());
                t.completed_at = Some(now);
                changed = true;
                if let Some(ctrl) = self.controls.get(&t.id) {
                    ctrl.store(TRANSFER_CONTROL_CANCELLED, Ordering::Relaxed);
                }
            }
        }
        if changed {
            self.has_unseen_error = true;
            self.save_persisted();
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.tasks.retain(|t| t.id != id);
        if let Some(ctrl) = self.controls.remove(id) {
            ctrl.store(TRANSFER_CONTROL_CANCELLED, Ordering::Relaxed);
        }
        if !self.tasks.iter().any(|t| t.status == TransferStatus::Error) {
            self.has_unseen_error = false;
        }
        self.save_persisted();
    }

    /// Drop all completed and errored tasks.
    pub fn clear_completed(&mut self) {
        let finished_ids: Vec<String> = self
            .tasks
            .iter()
            .filter(|t| t.status == TransferStatus::Complete || t.status == TransferStatus::Error)
            .map(|t| t.id.clone())
            .collect();
        self.tasks.retain(|t| t.status != TransferStatus::Complete && t.status != TransferStatus::Error);
        for id in finished_ids {
            self.controls.remove(&id);
        }
        self.has_unseen_error = false;
        self.save_persisted();
    }

    /// Pause every active transfer.
    pub fn pause_all(&mut self) {
        for t in &mut self.tasks {
            if t.status == TransferStatus::Active {
                t.status = TransferStatus::Paused;
            }
        }
        for ctrl in self.controls.values() {
            if ctrl.load(Ordering::Relaxed) == TRANSFER_CONTROL_ACTIVE {
                ctrl.store(TRANSFER_CONTROL_PAUSED, Ordering::Relaxed);
            }
        }
    }

    /// Resume every paused transfer.
    pub fn resume_all(&mut self) {
        for t in &mut self.tasks {
            if t.status == TransferStatus::Paused {
                t.status = TransferStatus::Active;
            }
        }
        for ctrl in self.controls.values() {
            if ctrl.load(Ordering::Relaxed) == TRANSFER_CONTROL_PAUSED {
                ctrl.store(TRANSFER_CONTROL_ACTIVE, Ordering::Relaxed);
            }
        }
    }

    /// Aggregate indicator for the status-bar button color.
    ///
    /// `None` = no transfers (button hidden). Otherwise:
    /// active → active (green)
    /// paused → paused (yellow)
    /// error (only if unseen in current run) → error (red)
    /// complete / historical → complete (gray)
    pub fn indicator(&self) -> Option<TransferStatus> {
        if self.tasks.is_empty() {
            return None;
        }
        if self.tasks.iter().any(|t| t.status == TransferStatus::Active) {
            return Some(TransferStatus::Active);
        }
        if self.tasks.iter().any(|t| t.status == TransferStatus::Paused) {
            return Some(TransferStatus::Paused);
        }
        if self.has_unseen_error && self.tasks.iter().any(|t| t.status == TransferStatus::Error) {
            return Some(TransferStatus::Error);
        }
        Some(TransferStatus::Complete)
    }

    /// Load persisted completed/failed tasks from local disk (`<data_root>/transfers.json`).
    pub fn load_persisted(&mut self) {
        let Some(path) = transfers_file_path() else { return; };
        if !path.exists() {
            return;
        }
        let Ok(content) = std::fs::read_to_string(&path) else { return; };
        let Ok(mut loaded_tasks) = serde_json::from_str::<Vec<TransferTask>>(&content) else { return; };

        let now = chrono::Local::now().timestamp();
        for t in &mut loaded_tasks {
            if t.status == TransferStatus::Active || t.status == TransferStatus::Paused {
                t.status = TransferStatus::Error;
                t.error = Some("传输已中断".to_string());
                if t.completed_at.is_none() {
                    t.completed_at = Some(now);
                }
            }
        }

        for loaded in loaded_tasks {
            if !self.tasks.iter().any(|t| t.id == loaded.id) {
                self.tasks.push(loaded);
            }
        }

        if self.tasks.len() > 200 {
            let start = self.tasks.len() - 200;
            self.tasks.drain(0..start);
        }

        self.has_unseen_error = false;
    }

    /// Atomically persist finished tasks (Complete / Error) to local disk.
    pub fn save_persisted(&self) {
        let Some(path) = transfers_file_path() else { return; };
        let finished_tasks: Vec<TransferTask> = self
            .tasks
            .iter()
            .filter(|t| t.status == TransferStatus::Complete || t.status == TransferStatus::Error)
            .cloned()
            .collect();

        let to_save: &[TransferTask] = if finished_tasks.len() > 200 {
            &finished_tasks[finished_tasks.len() - 200..]
        } else {
            &finished_tasks
        };

        if let Ok(json) = serde_json::to_string_pretty(to_save) {
            let _ = velowork_core::atomic_io::write_string_atomic(&path, &json);
        }
    }

    /// Trigger a flying animation towards the status bar transfer icon.
    pub fn trigger_fly(
        &mut self,
        origin: TransferFlyOrigin,
        is_multiple: bool,
        cx: &mut Context<Self>,
    ) {
        cx.emit(TransferFlyEvent {
            origin,
            is_multiple,
        });
    }
}

impl EventEmitter<TransferFlyEvent> for TransferStore {}

/// Global handle to the `TransferStore` entity.
pub struct GlobalTransferStore(pub Entity<TransferStore>);

impl Global for GlobalTransferStore {}

/// Helper to format a byte count into a compact human string.
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

/// Build the human-friendly throughput speed string with adaptive units.
pub fn format_speed(bps: f64) -> String {
    if bps <= 0.0 {
        return "--".to_string();
    }
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    if bps >= GB {
        format!("{:.2} GB/s", bps / GB)
    } else if bps >= MB {
        format!("{:.2} MB/s", bps / MB)
    } else if bps >= KB {
        format!("{:.1} KB/s", bps / KB)
    } else {
        format!("{:.0} B/s", bps)
    }
}

/// Build the `transferred / total` label shown under the progress bar.
pub fn format_amount(transferred: u64, total: u64) -> String {
    if total == 0 {
        if transferred == 0 {
            "--".to_string()
        } else {
            format_bytes(transferred)
        }
    } else {
        format!("{} / {}", format_bytes(transferred), format_bytes(total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_eta() {
        let mut task = TransferTask {
            id: "test-1".into(),
            name: "file.bin".into(),
            direction: TransferDirection::Upload,
            local_path: "/tmp/file.bin".into(),
            remote_path: "/remote/file.bin".into(),
            total_bytes: 100_000_000,
            transferred_bytes: 50_000_000,
            status: TransferStatus::Active,
            speed_bps: 10_000_000.0,
            error: None,
            session_id: None,
            created_at: None,
            completed_at: None,
        };

        // 50MB remaining at 10MB/s = 5 seconds
        assert_eq!(task.eta_seconds(), Some(5));
        assert_eq!(format_eta(task.eta_seconds()), "00:05");

        // When paused, ETA should be None
        task.status = TransferStatus::Paused;
        assert_eq!(task.eta_seconds(), None);
        assert_eq!(format_eta(task.eta_seconds()), "--");

        // When speed is 0, ETA should be None
        task.status = TransferStatus::Active;
        task.speed_bps = 0.0;
        assert_eq!(task.eta_seconds(), None);

        // When complete, ETA should be None
        task.status = TransferStatus::Complete;
        task.speed_bps = 10_000_000.0;
        assert_eq!(task.eta_seconds(), None);
    }

    #[test]
    fn test_format_eta_units() {
        assert_eq!(format_eta(Some(5)), "00:05");
        assert_eq!(format_eta(Some(75)), "01:15");
        assert_eq!(format_eta(Some(3665)), "01:01:05");
        assert_eq!(format_eta(None), "--");
    }

    #[test]
    fn test_humanize_transfer_error() {
        let msg = "SSH 连接已断开";
        assert_eq!(
            humanize_transfer_error("Custom { kind: UnexpectedEof, error: \"channel closed\" }", msg),
            msg
        );
        assert_eq!(
            humanize_transfer_error("Connection reset by peer (os error 104)", msg),
            msg
        );
        assert_eq!(
            humanize_transfer_error("Broken pipe (os error 32)", msg),
            msg
        );
        assert_eq!(
            humanize_transfer_error("Permission denied", msg),
            "Permission denied"
        );
    }

    #[test]
    fn test_clear_completed_removes_both_complete_and_error() {
        let mut store = TransferStore::new();
        store.add(TransferTask {
            id: "t1".into(),
            name: "f1".into(),
            direction: TransferDirection::Upload,
            local_path: "".into(),
            remote_path: "".into(),
            total_bytes: 100,
            transferred_bytes: 100,
            status: TransferStatus::Complete,
            speed_bps: 0.0,
            error: None,
            session_id: None,
            created_at: None,
            completed_at: None,
        });
        store.add(TransferTask {
            id: "t2".into(),
            name: "f2".into(),
            direction: TransferDirection::Upload,
            local_path: "".into(),
            remote_path: "".into(),
            total_bytes: 100,
            transferred_bytes: 50,
            status: TransferStatus::Error,
            speed_bps: 0.0,
            error: Some("Failed".into()),
            session_id: None,
            created_at: None,
            completed_at: None,
        });
        store.add(TransferTask {
            id: "t3".into(),
            name: "f3".into(),
            direction: TransferDirection::Upload,
            local_path: "".into(),
            remote_path: "".into(),
            total_bytes: 100,
            transferred_bytes: 50,
            status: TransferStatus::Active,
            speed_bps: 10.0,
            error: None,
            session_id: None,
            created_at: None,
            completed_at: None,
        });

        assert_eq!(store.tasks.len(), 3);
        store.clear_completed();
        assert_eq!(store.tasks.len(), 1);
        assert_eq!(store.tasks[0].id, "t3");
        assert_eq!(store.has_unseen_error, false);
    }

    #[test]
    fn test_fail_session_transfers() {
        let mut store = TransferStore::new();
        store.add(TransferTask {
            id: "s1-t1".into(),
            name: "f1".into(),
            direction: TransferDirection::Upload,
            local_path: "".into(),
            remote_path: "".into(),
            total_bytes: 100,
            transferred_bytes: 10,
            status: TransferStatus::Active,
            speed_bps: 10.0,
            error: None,
            session_id: Some("session-a".into()),
            created_at: None,
            completed_at: None,
        });
        store.add(TransferTask {
            id: "s2-t1".into(),
            name: "f2".into(),
            direction: TransferDirection::Upload,
            local_path: "".into(),
            remote_path: "".into(),
            total_bytes: 100,
            transferred_bytes: 10,
            status: TransferStatus::Active,
            speed_bps: 10.0,
            error: None,
            session_id: Some("session-b".into()),
            created_at: None,
            completed_at: None,
        });

        store.fail_session_transfers("session-a", "SSH 连接已断开".into());

        let t1 = store.get("s1-t1").unwrap();
        assert_eq!(t1.status, TransferStatus::Error);
        assert_eq!(t1.error.as_deref(), Some("SSH 连接已断开"));
        assert!(t1.completed_at.is_some());

        let t2 = store.get("s2-t1").unwrap();
        assert_eq!(t2.status, TransferStatus::Active);
        assert_eq!(store.has_unseen_error, true);

        // Indicator should reflect error
        assert_eq!(store.indicator(), Some(TransferStatus::Active)); // t2 is still active
        store.set_status("s2-t1", TransferStatus::Complete, None);
        assert_eq!(store.indicator(), Some(TransferStatus::Error)); // now error is visible

        // Mark seen clears the indicator back to complete/gray
        store.mark_errors_seen();
        assert_eq!(store.indicator(), Some(TransferStatus::Complete));
    }
}
