//! 内存整理与物理内存 Purge 实用工具。
//!
//! 提供 `trim_process_memory()` 函数，利用 `jemalloc` 的 `arena.purge` 机制
//! 或系统的内存整理接口，将由于频繁反序列化/临时计算产生的 Dirty Pages (脏页)
//! 主动还给操作系统 RSS，在保持高性能的同时控制内存占用。

/// 主动触发进程内存整理，将未使用的脏页归还给操作系统 RSS。
///
/// 安全无副作用：仅归还无用的空闲保留页，不会释放/破坏正在使用的任何内存对象。
pub fn trim_process_memory() {
    #[cfg(all(unix, feature = "jemalloc"))]
    {
        // 刷新 jemalloc 统计 epoch，然后对所有 arena 执行 purge
        unsafe {
            let mut epoch: u64 = 1;
            let mut sz = std::mem::size_of::<u64>();
            let _ = tikv_jemalloc_sys::mallctl(
                c"epoch".as_ptr() as *const _,
                &mut epoch as *mut _ as *mut _,
                &mut sz,
                &epoch as *const _ as *mut _,
                sz,
            );
            // 归还所有 arena 的空闲脏页给 OS
            let _ = tikv_jemalloc_sys::mallctl(
                c"arena.4096.purge".as_ptr() as *const _,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            );
        }
        log::debug!("Triggered jemalloc process memory purge");
    }
    #[cfg(all(target_os = "linux", not(feature = "jemalloc")))]
    {
        unsafe {
            extern "C" {
                fn malloc_trim(pad: usize) -> i32;
            }
            let _ = malloc_trim(0);
        }
    }
}

/// Process memory statistics (in kilobytes) read from OS status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessMemoryStats {
    /// Total resident set size (VmRSS) in KB.
    pub rss_kb: u64,
    /// Anonymous resident memory (RssAnon: heap, allocations, stacks) in KB.
    pub anonymous_kb: u64,
    /// File-backed resident memory (RssFile: shared libraries, executable code, font files) in KB.
    pub file_backed_kb: u64,
}

/// Read current process memory stats from the OS.
pub fn read_process_memory_stats() -> Option<ProcessMemoryStats> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let mut stats = ProcessMemoryStats::default();

        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:")
                && let Some(val_str) = rest.split_whitespace().next() {
                stats.rss_kb = val_str.parse().unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("RssAnon:")
                && let Some(val_str) = rest.split_whitespace().next() {
                stats.anonymous_kb = val_str.parse().unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("RssFile:")
                && let Some(val_str) = rest.split_whitespace().next() {
                stats.file_backed_kb = val_str.parse().unwrap_or(0);
            }
        }

        if stats.rss_kb > 0 {
            Some(stats)
        } else {
            None
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_process_memory_does_not_panic() {
        trim_process_memory();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_read_process_memory_stats() {
        let stats = read_process_memory_stats();
        assert!(stats.is_some());
        let s = stats.unwrap();
        assert!(s.rss_kb > 0);
    }
}
