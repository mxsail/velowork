//! Incremental cache used by [`MonitorCollector`] to derive rates and to avoid
//! re-collecting static host info every tick.

use crate::model::{DiskInfo, HostInfo, LoadInfo};

#[derive(Default)]
pub struct SampleCache {
    /// Previous aggregate `/proc/stat` totals (for the CPU delta).
    pub prev_cpu_total: u64,
    pub prev_cpu_busy: u64,
    pub has_prev_cpu: bool,

    /// Previous aggregate `/proc/net/dev` byte counters (for throughput delta).
    pub prev_net_rx: u64,
    pub prev_net_tx: u64,
    pub has_prev_net: bool,

    /// Cached static host identity (collected once).
    pub host: Option<HostInfo>,
    /// Cached slow-changing metrics (disks, load, uptime).
    pub disks: Vec<DiskInfo>,
    pub load: LoadInfo,
    pub uptime_secs: u64,
}
