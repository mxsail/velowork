//! Parses the marker-delimited output of [`LinuxCommandBuilder`] into
//! structured [`model`] types. All parsing is pure and transport-agnostic.

use crate::cache::SampleCache;
use crate::command::Section;
use crate::model::*;
use crate::{MonitorError, Result};
use std::collections::HashMap;

/// Raw, unparsed output of one or more probed sections, keyed by [`Section`].
#[derive(Debug, Default)]
pub struct RawSections {
    inner: HashMap<Section, String>,
}

impl RawSections {
    /// Split a combined script output into per-section text blocks. A line that
    /// exactly equals a section marker starts a new block; everything until the
    /// next marker (or `__END__`) belongs to that section.
    pub fn parse_script(output: &str) -> RawSections {
        let mut inner = HashMap::new();
        let mut current: Option<Section> = None;
        let mut buf = String::new();
        for line in output.lines() {
            let trimmed = line.trim();
            // `__END__` terminates the current block without being captured.
            if trimmed == "__END__" {
                if let Some(prev) = current.take() {
                    inner.insert(prev, buf.trim().to_string());
                    buf.clear();
                }
                continue;
            }
            if let Some(sec) = Section::from_marker(trimmed) {
                if let Some(prev) = current.take() {
                    inner.insert(prev, buf.trim().to_string());
                    buf.clear();
                }
                current = Some(sec);
            } else if current.is_some() {
                buf.push_str(line);
                buf.push('\n');
            }
        }
        if let Some(prev) = current.take() {
            inner.insert(prev, buf.trim().to_string());
        }
        RawSections { inner }
    }

    pub fn get(&self, sec: Section) -> Option<&str> {
        self.inner.get(&sec).map(|s| s.as_str())
    }
}

/// Static (slow-changing) host data.
pub struct StaticPart {
    pub host: HostInfo,
    pub disks: Vec<DiskInfo>,
    pub load: LoadInfo,
    pub uptime_secs: u64,
}

/// Dynamic (frequently sampled) host data.
pub struct DynamicPart {
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub network: NetworkInfo,
    pub processes: u32,
    pub users: u32,
}

/// Parses raw sections into structured data.
pub struct MonitorParser;

impl MonitorParser {
    pub fn parse_static(sections: &RawSections) -> StaticPart {
        let host = Self::parse_host(sections);
        let disks = Self::parse_disks(sections);
        let load = Self::parse_load(sections);
        let uptime_secs = Self::parse_uptime(sections);
        StaticPart { host, disks, load, uptime_secs }
    }

    /// Parse the dynamic sections. `elapsed` is the seconds since the previous
    /// sample, used to derive network throughput; CPU deltas come from `cache`.
    pub fn parse_dynamic(
        sections: &RawSections,
        cache: &mut SampleCache,
        elapsed: f64,
    ) -> Result<DynamicPart> {
        let cpu = Self::parse_cpu(sections, cache)?;
        let memory = Self::parse_memory(sections)?;
        let network = Self::parse_network(sections, cache, elapsed)?;
        let processes = Self::parse_proc(sections);
        let users = Self::parse_users(sections);
        Ok(DynamicPart { cpu, memory, network, processes, users })
    }

    // ---- static ----

    fn parse_host(sections: &RawSections) -> HostInfo {
        let hostname = sections.get(Section::Host).map(str::trim).unwrap_or("").to_string();
        let kernel = sections.get(Section::Kernel).map(str::trim).unwrap_or("").to_string();
        let (os_name, os_version) = parse_os_release(sections.get(Section::Os).unwrap_or(""));
        let arch = map_arch(sections.get(Section::Arch).map(str::trim).unwrap_or(""));
        let cpu_brand = parse_cpu_brand(sections.get(Section::CpuInfo).unwrap_or(""));
        let cpu_cores = sections
            .get(Section::Cores)
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        HostInfo { hostname, kernel, os_name, os_version, arch, cpu_brand, cpu_cores }
    }

    fn parse_disks(sections: &RawSections) -> Vec<DiskInfo> {
        let Some(text) = sections.get(Section::Disk) else { return Vec::new() };
        let mut disks = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("Filesystem") || trimmed.contains("Mounted on") {
                continue;
            }
            // Filesystem Type 1024-blocks Used Available Capacity Mounted-on
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 7 {
                continue;
            }
            // Defensive guard against localized/unexpected headers slipping
            // past the check above: the 1024-blocks column must be numeric on
            // a real data row. A header row ("1024-块" / "1024-blocks") fails
            // to parse, so skip it instead of emitting an all-zero entry.
            let Ok(blocks) = parts[2].parse::<u64>() else {
                continue;
            };
            let total = blocks * 1024;
            let used = parts[3].parse::<u64>().unwrap_or(0) * 1024;
            let available = parts[4].parse::<u64>().unwrap_or(0) * 1024;
            let used_pct = parts[5].trim_end_matches('%').parse::<u32>().unwrap_or(0);
            let mount = parts[6..].join(" ");
            disks.push(DiskInfo {
                mount,
                filesystem: parts[1].to_string(),
                total_bytes: total,
                available_bytes: available,
                used_bytes: used,
                used_pct,
            });
        }
        disks
    }

    fn parse_load(sections: &RawSections) -> LoadInfo {
        let mut load = LoadInfo::default();
        if let Some(text) = sections.get(Section::Load) {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() >= 3 {
                load.one = parts[0].parse().unwrap_or(0.0);
                load.five = parts[1].parse().unwrap_or(0.0);
                load.fifteen = parts[2].parse().unwrap_or(0.0);
            }
        }
        load
    }

    fn parse_uptime(sections: &RawSections) -> u64 {
        sections
            .get(Section::Uptime)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<f64>().ok())
            .map(|s| s as u64)
            .unwrap_or(0)
    }

    // ---- dynamic ----

    fn parse_cpu(sections: &RawSections, cache: &mut SampleCache) -> Result<CpuInfo> {
        let text = sections
            .get(Section::Stat)
            .ok_or_else(|| MonitorError::Parse("missing __STAT__ section".into()))?;
        let line = text
            .lines()
            .find(|l| l.starts_with("cpu "))
            .ok_or_else(|| MonitorError::Parse("no cpu line in /proc/stat".into()))?;
        let vals: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|v| v.parse::<u64>().ok())
            .collect();
        if vals.is_empty() {
            return Err(MonitorError::Parse("empty cpu counters".into()));
        }
        let total: u64 = vals.iter().sum();
        let idle = vals.get(3).copied().unwrap_or(0);
        let iowait = vals.get(4).copied().unwrap_or(0);
        let busy = total.saturating_sub(idle).saturating_sub(iowait);

        // First sample only seeds the baseline; usage becomes meaningful after.
        let usage = if cache.has_prev_cpu {
            let dt = total.saturating_sub(cache.prev_cpu_total);
            let db = busy.saturating_sub(cache.prev_cpu_busy);
            if dt > 0 {
                (db as f32 / dt as f32) * 100.0
            } else {
                0.0
            }
        } else {
            0.0
        };
        cache.prev_cpu_total = total;
        cache.prev_cpu_busy = busy;
        cache.has_prev_cpu = true;

        Ok(CpuInfo { usage_pct: usage.clamp(0.0, 100.0) })
    }

    fn parse_memory(sections: &RawSections) -> Result<MemoryInfo> {
        let Some(text) = sections.get(Section::Mem) else {
            return Ok(MemoryInfo::default());
        };
        let mut mem_total = 0u64;
        let mut mem_avail = 0u64;
        let mut mem_free = 0u64;
        let mut swap_total = 0u64;
        let mut swap_free = 0u64;
        for line in text.lines() {
            let mut it = line.split_whitespace();
            let key = it.next().unwrap_or("");
            // /proc/meminfo values are in kB.
            let val: u64 = it.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024;
            match key {
                "MemTotal:" => mem_total = val,
                "MemAvailable:" => mem_avail = val,
                "MemFree:" => mem_free = val,
                "SwapTotal:" => swap_total = val,
                "SwapFree:" => swap_free = val,
                _ => {}
            }
        }
        let used = if mem_avail > 0 {
            mem_total.saturating_sub(mem_avail)
        } else {
            mem_total.saturating_sub(mem_free)
        };
        let swap_used = swap_total.saturating_sub(swap_free);
        Ok(MemoryInfo {
            total_bytes: mem_total,
            available_bytes: mem_avail,
            used_bytes: used,
            swap_total_bytes: swap_total,
            swap_used_bytes: swap_used,
        })
    }

    fn parse_network(
        sections: &RawSections,
        cache: &mut SampleCache,
        elapsed: f64,
    ) -> Result<NetworkInfo> {
        let Some(text) = sections.get(Section::Net) else {
            return Ok(NetworkInfo::default());
        };
        let mut rx_total = 0u64;
        let mut tx_total = 0u64;
        for line in text.lines().skip(1) {
            // Header: "Inter-| ..." then "face: ...". Skip until we hit "iface:".
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (iface, rest) = match line.split_once(':') {
                Some(x) => x,
                None => continue,
            };
            if iface.trim() == "lo" {
                continue;
            }
            let vals: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse::<u64>().ok()).collect();
            // /proc/net/dev layout after the colon:
            //   rx_bytes rx_pkts ... [8] = tx_bytes
            if vals.len() > 8 {
                rx_total += vals[0];
                tx_total += vals[8];
            }
        }
        let (rx_rate, tx_rate) = if cache.has_prev_net && elapsed > 0.0 {
            (
                (rx_total.saturating_sub(cache.prev_net_rx)) as f64 / elapsed,
                (tx_total.saturating_sub(cache.prev_net_tx)) as f64 / elapsed,
            )
        } else {
            (0.0, 0.0)
        };
        cache.prev_net_rx = rx_total;
        cache.prev_net_tx = tx_total;
        cache.has_prev_net = true;
        Ok(NetworkInfo { rx_rate, tx_rate })
    }

    fn parse_proc(sections: &RawSections) -> u32 {
        sections.get(Section::Proc).and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0)
    }

    fn parse_users(sections: &RawSections) -> u32 {
        sections
            .get(Section::Users)
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count() as u32)
            .unwrap_or(0)
    }
}

// ---- helpers ----

fn parse_os_release(text: &str) -> (String, String) {
    let mut name = String::new();
    let mut version = String::new();
    let mut pretty = String::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "NAME" => name = v,
                "VERSION_ID" => version = v,
                "PRETTY_NAME" => pretty = v,
                _ => {}
            }
        }
    }
    // Prefer the structured NAME/VERSION_ID pair when present.
    if !name.is_empty() {
        return (name, version);
    }
    if !pretty.is_empty() {
        // "Ubuntu 24.04.1 LTS" -> (name="Ubuntu", version="24.04.1").
        if let Some((n, v)) = pretty.rsplit_once(' ')
            && v.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return (n.to_string(), v.to_string());
        }
        return (pretty, String::new());
    }
    (name, version)
}

fn map_arch(arch: &str) -> String {
    match arch {
        "x86_64" => "x86",
        "aarch64" | "arm64" => "ARM64",
        "armv7l" | "armv6l" => "ARM",
        "riscv64" => "RISC-V",
        other => other,
    }
    .to_string()
}

fn parse_cpu_brand(text: &str) -> String {
    let mut brand = String::new();
    let mut hardware = String::new();
    let mut processor = String::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim();
            let val = v.trim();
            if key == "model name" && brand.is_empty() {
                brand = val.to_string();
            } else if key == "Hardware" && hardware.is_empty() {
                hardware = val.to_string();
            } else if key == "Processor" && processor.is_empty() {
                processor = val.to_string();
            }
        }
    }
    if !brand.is_empty() {
        brand
    } else if !hardware.is_empty() {
        hardware
    } else {
        processor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
__HOST__
myserver
__KERNEL__
6.8.0-52-generic
__OS__
NAME=\"Ubuntu\"
VERSION_ID=\"24.04.1\"
PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"
__ARCH__
x86_64
__CPUINFO__
model name : AMD Ryzen 9 5900X
__CORES__
16
__DISK__
Filesystem     Type   1024-blocks   Used   Available Capacity Mounted on
/dev/sda1      ext4    100000000   20000000 80000000   20% /
__UPTIME__
12345.67 23456.78
__LOAD__
0.10 0.20 0.30 1/234 5678
__STAT__
cpu  100 0 50 800 20 0 0 0 0 0
cpu0 50 0 25 400 10 0 0 0 0 0
__MEM__
MemTotal:        1000000 kB
MemAvailable:    600000 kB
MemFree:         400000 kB
SwapTotal:       200000 kB
SwapFree:        100000 kB
__NET__
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
  eth0: 1000 0 0 0 0 0 0 0 2000 0 0 0 0 0 0 0
__PROC__
345
__USERS__
root  pts/0  2026-07-14 10:00
__END__
";

    #[test]
    fn parses_static_fields() {
        let secs = RawSections::parse_script(SAMPLE);
        let s = MonitorParser::parse_static(&secs);
        assert_eq!(s.host.hostname, "myserver");
        assert_eq!(s.host.kernel, "6.8.0-52-generic");
        assert_eq!(s.host.os_name, "Ubuntu");
        assert_eq!(s.host.os_version, "24.04.1");
        assert_eq!(s.host.arch, "x86");
        assert_eq!(s.host.cpu_brand, "AMD Ryzen 9 5900X");
        assert_eq!(s.host.cpu_cores, 16);
        assert_eq!(s.disks.len(), 1);
        assert_eq!(s.disks[0].mount, "/");
        assert_eq!(s.disks[0].used_pct, 20);
        assert_eq!(s.disks[0].total_bytes, 100000000 * 1024);
        assert_eq!(s.load.one, 0.10);
        assert_eq!(s.uptime_secs, 12345);
    }

    #[test]
    fn derives_cpu_and_memory() {
        let secs = RawSections::parse_script(SAMPLE);
        let mut cache = SampleCache::default();
        // First sample seeds the baseline; usage is 0.
        let d = MonitorParser::parse_dynamic(&secs, &mut cache, 1.0).unwrap();
        assert_eq!(d.cpu.usage_pct, 0.0);
        assert_eq!(d.memory.total_bytes, 1000000 * 1024);
        assert_eq!(d.memory.used_bytes, 400000 * 1024);
        assert_eq!(d.memory.swap_used_bytes, 100000 * 1024);
        assert_eq!(d.processes, 345);
        assert_eq!(d.users, 1);
        assert_eq!(d.network.rx_rate, 0.0); // first sample, no delta
    }

    #[test]
    fn cpu_usage_after_two_samples() {
        let secs = RawSections::parse_script(SAMPLE);
        let mut cache = SampleCache::default();
        let _ = MonitorParser::parse_dynamic(&secs, &mut cache, 1.0).unwrap();
        // 1s later, 50 busy of 120 total delta -> ~41.6%.
        let secs2 = RawSections::parse_script("\
__STAT__
cpu  150 0 75 900 20 0 0 0 0 0
");
        let d = MonitorParser::parse_dynamic(&secs2, &mut cache, 1.0).unwrap();
        // Δtotal=175, Δbusy=75 -> 75/175 ≈ 42.86%.
        assert!((d.cpu.usage_pct - 42.86).abs() < 0.5);
    }
}
