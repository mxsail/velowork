//! Local (non-SSH) monitor source: runs the probe script via `sh -c`.
//!
//! Used for validation/tests today. The exact same script works unchanged over
//! SSH — only this `exec` implementation needs to change.

use crate::{MonitorError, MonitorSource, Result};

/// Runs the probe locally through the system shell.
pub struct LinuxLocalSource;

impl MonitorSource for LinuxLocalSource {
    fn exec(&self, script: &str) -> Result<String> {
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .output()
            .map_err(|e| MonitorError::Exec(e.to_string()))?;
        if !output.status.success() {
            return Err(MonitorError::Exec(format!(
                "monitor probe exited with status {:?}",
                output.status.code()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
