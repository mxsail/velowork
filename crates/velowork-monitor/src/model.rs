//! Structured monitor data. Pure data — no transport, no SSH, no parsing
//! logic. The UI consumes [`MonitorSnapshot`] and nothing else.

use serde::{Deserialize, Serialize};

/// A complete point-in-time view of a (remote or local) host's resources.
///
/// Produced by a `MonitorSource` + `MonitorParser` and consumed by the UI.
/// Contains no transport/SSH details.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MonitorSnapshot {
    pub host: HostInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub network: NetworkInfo,
    pub disks: Vec<DiskInfo>,
    pub load: LoadInfo,
    pub processes: u32,
    pub users: u32,
    pub uptime_secs: u64,
}

/// Static host identity. Collected once and cached; safe to reuse across ticks.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct HostInfo {
    pub hostname: String,
    pub kernel: String,
    pub os_name: String,
    pub os_version: String,
    /// UI-friendly architecture label, e.g. `x86`, `ARM64`, `RISC-V`.
    pub arch: String,
    pub cpu_brand: String,
    pub cpu_cores: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CpuInfo {
    /// Aggregate busy ratio across all cores, 0..100.
    pub usage_pct: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkInfo {
    /// Bytes/sec received since the previous sample.
    pub rx_rate: f64,
    /// Bytes/sec transmitted since the previous sample.
    pub tx_rate: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DiskInfo {
    pub mount: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    /// 0..100.
    pub used_pct: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LoadInfo {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}
