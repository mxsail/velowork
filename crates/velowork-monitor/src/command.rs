//! Builds the single-shot probe script for a Linux host.
//!
//! Every metric is read from kernel interfaces only — no `htop`/`free`/`iostat`
//! — so it works on Alpine, containers and minimal images. Sections are
//! delimited by `__MARKER__` echo lines so the parser can split reliably.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Host,
    Kernel,
    Os,
    Arch,
    CpuInfo,
    Cores,
    Proc,
    Stat,
    Mem,
    Net,
    Disk,
    Uptime,
    Load,
    Users,
}

impl Section {
    /// Marker line printed before this section's command output.
    pub fn marker(self) -> &'static str {
        match self {
            Section::Host => "__HOST__",
            Section::Kernel => "__KERNEL__",
            Section::Os => "__OS__",
            Section::Arch => "__ARCH__",
            Section::CpuInfo => "__CPUINFO__",
            Section::Cores => "__CORES__",
            Section::Proc => "__PROC__",
            Section::Stat => "__STAT__",
            Section::Mem => "__MEM__",
            Section::Net => "__NET__",
            Section::Disk => "__DISK__",
            Section::Uptime => "__UPTIME__",
            Section::Load => "__LOAD__",
            Section::Users => "__USERS__",
        }
    }

    /// The shell command whose output follows the marker.
    pub fn command(self) -> &'static str {
        match self {
            Section::Host => "cat /proc/sys/kernel/hostname",
            Section::Kernel => "uname -r",
            Section::Os => "cat /etc/os-release",
            Section::Arch => "uname -m",
            Section::CpuInfo => "cat /proc/cpuinfo",
            Section::Cores => "nproc",
            Section::Proc => "ls /proc | grep -cE '^[0-9]+$'",
            Section::Stat => "cat /proc/stat",
            Section::Mem => "cat /proc/meminfo",
            Section::Net => "cat /proc/net/dev",
            // Force C locale so the header row and column order stay in
            // English regardless of the host's `LANG`/`LC_*`. Without this, a
            // localized header (e.g. Chinese `文件系统 ... 挂载点`) slips past
            // the parser's header-skip check and is rendered as a bogus first
            // row (name in the first column, zeros elsewhere).
            Section::Disk => "LC_ALL=C df -PT",
            Section::Uptime => "cat /proc/uptime",
            Section::Load => "cat /proc/loadavg",
            Section::Users => "w -h 2>/dev/null || who",
        }
    }

    pub(crate) fn from_marker(line: &str) -> Option<Section> {
        match line {
            "__HOST__" => Some(Section::Host),
            "__KERNEL__" => Some(Section::Kernel),
            "__OS__" => Some(Section::Os),
            "__ARCH__" => Some(Section::Arch),
            "__CPUINFO__" => Some(Section::CpuInfo),
            "__CORES__" => Some(Section::Cores),
            "__PROC__" => Some(Section::Proc),
            "__STAT__" => Some(Section::Stat),
            "__MEM__" => Some(Section::Mem),
            "__NET__" => Some(Section::Net),
            "__DISK__" => Some(Section::Disk),
            "__UPTIME__" => Some(Section::Uptime),
            "__LOAD__" => Some(Section::Load),
            "__USERS__" => Some(Section::Users),
            _ => None,
        }
    }
}

/// Builds marker-delimited probe scripts for a Linux host.
pub struct LinuxCommandBuilder;

impl LinuxCommandBuilder {
    /// Build a script that echoes each requested section's marker then runs its
    /// command, ending with `__END__`.
    pub fn script_for(sections: &[Section]) -> String {
        let mut script = String::new();
        for &sec in sections {
            script.push_str(&format!("echo {}\n{}\n", sec.marker(), sec.command()));
        }
        script.push_str("echo __END__\n");
        script
    }

    /// Dynamic metrics sampled frequently (1s): CPU, memory, network, process
    /// and user counts.
    pub fn fast_script() -> String {
        Self::script_for(&[
            Section::Stat,
            Section::Mem,
            Section::Net,
            Section::Proc,
            Section::Users,
        ])
    }

    /// Static + slowly-changing metrics sampled infrequently (10s): host
    /// identity, CPU model/cores, disks (optional), uptime, load average.
    pub fn slow_script_with(include_disk: bool) -> String {
        let mut sections = vec![
            Section::Host,
            Section::Kernel,
            Section::Os,
            Section::Arch,
            Section::CpuInfo,
            Section::Cores,
        ];
        if include_disk {
            sections.push(Section::Disk);
        }
        sections.push(Section::Uptime);
        sections.push(Section::Load);
        Self::script_for(&sections)
    }

    /// Static + slowly-changing metrics sampled infrequently (10s): host
    /// identity, CPU model/cores, disks, uptime, load average.
    pub fn slow_script() -> String {
        Self::slow_script_with(true)
    }

    /// All sections in one script (used for local validation / tests).
    pub fn full_script() -> String {
        Self::script_for(&[
            Section::Host,
            Section::Kernel,
            Section::Os,
            Section::Arch,
            Section::CpuInfo,
            Section::Cores,
            Section::Proc,
            Section::Stat,
            Section::Mem,
            Section::Net,
            Section::Disk,
            Section::Uptime,
            Section::Load,
            Section::Users,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_script_contains_markers_and_commands() {
        let s = LinuxCommandBuilder::full_script();
        assert!(s.contains("echo __HOST__"));
        assert!(s.contains("cat /proc/sys/kernel/hostname"));
        assert!(s.contains("echo __END__"));
        // Each section appears exactly once as a marker.
        assert_eq!(s.matches("__HOST__").count(), 1);
    }

    #[test]
    fn fast_script_has_no_static_sections() {
        let s = LinuxCommandBuilder::fast_script();
        assert!(!s.contains("__HOST__"));
        assert!(!s.contains("__DISK__"));
        assert!(s.contains("__STAT__"));
        assert!(s.contains("__NET__"));
    }
}
