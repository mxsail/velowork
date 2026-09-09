//! Drives metric collection on a cadence and assembles a [`MonitorSnapshot`].
//!
//! Static/slow metrics are refreshed every [`SLOW_INTERVAL`]; dynamic metrics
//! every [`FAST_INTERVAL`]. The collector owns the delta state so the UI only
//! ever sees finished snapshots.

use crate::cache::SampleCache;
use crate::command::LinuxCommandBuilder;
use crate::linux::LinuxLocalSource;
use crate::model::MonitorSnapshot;
use crate::parser::{MonitorParser, RawSections};
use crate::Result;
use std::time::{Duration, Instant};

/// Dynamic-metric refresh period (CPU, memory, network, process/user counts).
pub const FAST_INTERVAL: Duration = Duration::from_secs(1);
/// Slow-changing-metric refresh period (host identity, disks, uptime, load).
pub const SLOW_INTERVAL: Duration = Duration::from_secs(10);

/// A source of raw, marker-delimited monitor output. [`LinuxLocalSource`] runs
/// the probe locally; an SSH-backed source can be slotted in later without
/// touching the parser or the UI.
// `Send + Sync` so a collector holding a boxed source can be moved onto a
// background thread and shared across the worker / UI boundary.
pub trait MonitorSource: Send + Sync {
    fn exec(&self, script: &str) -> Result<String>;
}

/// Fine-grained options controlling which metrics are collected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorOptions {
    pub enable_cpu: bool,
    pub enable_mem: bool,
    pub enable_disk: bool,
}

impl Default for MonitorOptions {
    fn default() -> Self {
        Self {
            enable_cpu: true,
            enable_mem: true,
            enable_disk: false,
        }
    }
}

/// Schedules collection and merges fast/slow sections into a snapshot.
pub struct MonitorCollector {
    source: Box<dyn MonitorSource>,
    cache: SampleCache,
    options: MonitorOptions,
    last_fast: Instant,
    last_slow: Instant,
}

impl MonitorCollector {
    pub fn new(source: Box<dyn MonitorSource>) -> Self {
        let now = Instant::now();
        Self {
            source,
            cache: SampleCache::default(),
            options: MonitorOptions::default(),
            last_fast: now,
            // Force an immediate slow refresh on the first tick.
            last_slow: now.checked_sub(SLOW_INTERVAL).unwrap_or(now),
        }
    }

    pub fn with_options(mut self, options: MonitorOptions) -> Self {
        self.options = options;
        self
    }

    pub fn set_options(&mut self, options: MonitorOptions) {
        if !options.enable_disk && self.options.enable_disk {
            self.cache.disks.clear();
        }
        self.options = options;
    }

    pub fn options(&self) -> MonitorOptions {
        self.options
    }

    /// Convenience constructor using the local `sh -c` source.
    pub fn local() -> Self {
        Self::new(Box::new(LinuxLocalSource))
    }

    /// Run one collection pass, refreshing slow sections on their own cadence
    /// and dynamic sections every call.
    pub fn tick(&mut self) -> Result<MonitorSnapshot> {
        let now = Instant::now();

        if now.duration_since(self.last_slow) >= SLOW_INTERVAL {
            let out = self.source.exec(&LinuxCommandBuilder::slow_script_with(self.options.enable_disk))?;
            let sections = RawSections::parse_script(&out);
            let static_part = MonitorParser::parse_static(&sections);
            self.cache.host = Some(static_part.host);
            if self.options.enable_disk {
                self.cache.disks = static_part.disks;
            } else {
                self.cache.disks.clear();
            }
            self.cache.load = static_part.load;
            self.cache.uptime_secs = static_part.uptime_secs;
            self.last_slow = now;
        }

        let elapsed = self.last_fast.elapsed().as_secs_f64().max(0.01);
        let out = self.source.exec(&LinuxCommandBuilder::fast_script())?;
        let sections = RawSections::parse_script(&out);
        let dynamic = MonitorParser::parse_dynamic(&sections, &mut self.cache, elapsed)?;
        self.last_fast = now;

        let host = self.cache.host.clone().unwrap_or_default();
        Ok(MonitorSnapshot {
            host,
            cpu: dynamic.cpu,
            memory: dynamic.memory,
            network: dynamic.network,
            disks: self.cache.disks.clone(),
            load: self.cache.load.clone(),
            processes: dynamic.processes,
            users: dynamic.users,
            uptime_secs: self.cache.uptime_secs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_collect_succeeds() {
        let mut c = MonitorCollector::local();
        let snap = c.tick().expect("local collect should succeed");
        // At least one identifier should be present on a real Linux host.
        assert!(!snap.host.hostname.is_empty() || !snap.host.kernel.is_empty());
        // Multiple ticks should not error (exercises CPU/net deltas).
        let _ = c.tick();
    }
}
