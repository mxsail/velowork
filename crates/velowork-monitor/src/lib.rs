//! Remote/local host monitoring.
//!
//! Collects system metrics from kernel interfaces (`/proc`, `/sys`, `uname`,
//! `df`, ...) over a single SSH exec (or locally via `sh -c`), parses the
//! combined output into a [`MonitorSnapshot`], and exposes it to the UI.
//!
//! The UI never sees a Linux command — it only consumes [`MonitorSnapshot`].

pub mod model;
pub mod command;
pub mod parser;
pub mod cache;
pub mod collector;
pub mod linux;

pub use model::{CpuInfo, DiskInfo, HostInfo, LoadInfo, MemoryInfo, MonitorSnapshot, NetworkInfo};
pub use command::{LinuxCommandBuilder, Section};
pub use parser::{DynamicPart, MonitorParser, RawSections, StaticPart};
pub use cache::SampleCache;
pub use collector::{MonitorCollector, MonitorOptions, MonitorSource, FAST_INTERVAL, SLOW_INTERVAL};
pub use linux::LinuxLocalSource;

/// Errors produced while executing a monitor command or parsing its output.
#[derive(Debug, thiserror::Error)]
pub enum MonitorError {
    #[error("monitor command execution failed: {0}")]
    Exec(String),
    #[error("monitor output parsing failed: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, MonitorError>;
