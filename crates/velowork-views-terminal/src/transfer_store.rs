//! Global transfer store backing the status-bar transfer manager.
//!
//! SFTP uploads/downloads register a `TransferTask` here on start and update
//! progress as they stream. The status bar reads this store to color its
//! transfer button and to render the transfer popup.

use std::sync::atomic::{AtomicU64, Ordering};

use gpui::*;

/// Direction of a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// Lifecycle status of a transfer task.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
#[derive(Clone, Debug)]
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

/// Observable store of all in-flight and recently-finished transfers.
#[derive(Clone, Debug, Default)]
pub struct TransferStore {
    pub tasks: Vec<TransferTask>,
}

impl TransferStore {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
        }
    }

    /// Insert or replace a task by id.
    pub fn add(&mut self, task: TransferTask) {
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
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.status = status;
            t.error = error;
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.tasks.retain(|t| t.id != id);
    }

    /// Drop all completed tasks.
    pub fn clear_completed(&mut self) {
        self.tasks.retain(|t| t.status != TransferStatus::Complete);
    }

    /// Pause every active transfer.
    pub fn pause_all(&mut self) {
        for t in &mut self.tasks {
            if t.status == TransferStatus::Active {
                t.status = TransferStatus::Paused;
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
    }

    /// Aggregate indicator for the status-bar button color.
    ///
    /// `None` = no transfers (button hidden). Otherwise the most
    /// significant status: error > paused > active > (all) complete.
    pub fn indicator(&self) -> Option<TransferStatus> {
        if self.tasks.is_empty() {
            return None;
        }
        if self.tasks.iter().any(|t| t.status == TransferStatus::Error) {
            return Some(TransferStatus::Error);
        }
        if self.tasks.iter().any(|t| t.status == TransferStatus::Paused) {
            return Some(TransferStatus::Paused);
        }
        if self.tasks.iter().any(|t| t.status == TransferStatus::Active) {
            return Some(TransferStatus::Active);
        }
        Some(TransferStatus::Complete)
    }
}

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

/// Build the human "X MB/s" throughput string.
pub fn format_speed(bps: f64) -> String {
    format!("{:.1} MB/s", bps / 1_048_576.0)
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
