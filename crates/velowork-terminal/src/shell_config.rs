//! Shell configuration for Windows and cross-platform terminal support
//!
//! Provides shell type detection and command building for different shells:
//! - cmd.exe (Command Prompt)
//! - powershell.exe (Windows PowerShell)
//! - pwsh.exe (PowerShell Core)
//! - WSL (Windows Subsystem for Linux)
//! - Custom shell paths

use portable_pty::CommandBuilder;

// `ShellType` itself is pure data and lives in `velowork-core` so data-only crates
// can use it without the PTY stack. Re-exported here so existing
// `velowork_terminal::shell_config::ShellType` paths keep working.
pub use velowork_core::shell::ShellType;

/// Build a spawnable `CommandBuilder` from a [`ShellType`].
///
/// This is the behavioral half of `ShellType` — kept in `velowork-terminal`
/// because it depends on `portable_pty`. Implemented as an extension trait so
/// call sites keep the ergonomic `shell.build_command(cwd)` form.
pub trait ShellCommandExt {
    fn build_command(&self, cwd: &str) -> CommandBuilder;
}

impl ShellCommandExt for ShellType {
    /// Build a CommandBuilder for this shell type
    fn build_command(&self, cwd: &str) -> CommandBuilder {
        match self {
            ShellType::Default => {
                let mut cmd = CommandBuilder::new_default_prog();
                cmd.cwd(cwd);
                cmd
            }
            #[cfg(windows)]
            ShellType::Cmd => {
                let mut cmd = CommandBuilder::new("cmd.exe");
                cmd.cwd(cwd);
                cmd
            }
            #[cfg(windows)]
            ShellType::PowerShell { core } => {
                let exe = if *core { "pwsh.exe" } else { "powershell.exe" };
                let mut cmd = CommandBuilder::new(exe);
                // -NoLogo reduces startup noise
                cmd.arg("-NoLogo");
                cmd.cwd(cwd);
                cmd
            }
            #[cfg(windows)]
            ShellType::Wsl { distro } => {
                let mut cmd = CommandBuilder::new("wsl.exe");
                if let Some(d) = distro {
                    cmd.arg("-d");
                    cmd.arg(d);
                }
                // Convert Windows path to WSL path
                let wsl_path = windows_path_to_wsl(cwd);
                cmd.arg("--cd");
                cmd.arg(&wsl_path);
                cmd
            }
            ShellType::Custom { path, args } => {
                let mut cmd = CommandBuilder::new(path);
                for arg in args {
                    cmd.arg(arg);
                }
                cmd.cwd(cwd);
                cmd
            }
            ShellType::Welcome => {
                let mut cmd = CommandBuilder::new_default_prog();
                cmd.cwd(cwd);
                cmd
            }
        }
    }
}

use parking_lot::RwLock;

static CACHED_SHELLS: RwLock<Option<Vec<AvailableShell>>> = RwLock::new(None);

/// Information about an available shell
#[derive(Clone, Debug)]
pub struct AvailableShell {
    pub shell_type: ShellType,
    pub name: String,
    pub available: bool,
}

/// Detect all available shells on the system (cached).
pub fn available_shells() -> Vec<AvailableShell> {
    if let Some(shells) = CACHED_SHELLS.read().as_ref() {
        return shells.clone();
    }

    let shells = detect_available_shells_uncached();
    *CACHED_SHELLS.write() = Some(shells.clone());
    shells
}

/// Force re-detect all available shells and update the cache.
pub fn refresh_available_shells() -> Vec<AvailableShell> {
    let shells = detect_available_shells_uncached();
    *CACHED_SHELLS.write() = Some(shells.clone());
    shells
}

fn detect_available_shells_uncached() -> Vec<AvailableShell> {
    let mut shells = vec![AvailableShell {
        shell_type: ShellType::Default,
        name: "System Default".to_string(),
        available: true,
    }];

    #[cfg(windows)]
    {
        // Command Prompt is always available on Windows
        shells.push(AvailableShell {
            shell_type: ShellType::Cmd,
            name: "Command Prompt".to_string(),
            available: true,
        });

        // Windows PowerShell is always available on modern Windows
        shells.push(AvailableShell {
            shell_type: ShellType::PowerShell { core: false },
            name: "Windows PowerShell".to_string(),
            available: true,
        });

        // Check for PowerShell Core (pwsh.exe)
        let pwsh_available = is_pwsh_available();
        shells.push(AvailableShell {
            shell_type: ShellType::PowerShell { core: true },
            name: "PowerShell Core".to_string(),
            available: pwsh_available,
        });

        // Check for WSL
        let wsl_distros = detect_wsl_distros();
        if !wsl_distros.is_empty() {
            // Add default WSL option
            shells.push(AvailableShell {
                shell_type: ShellType::Wsl { distro: None },
                name: "WSL (Default)".to_string(),
                available: true,
            });

            // Add each specific distro
            for distro in wsl_distros {
                shells.push(AvailableShell {
                    shell_type: ShellType::Wsl {
                        distro: Some(distro.clone()),
                    },
                    name: format!("WSL ({})", distro),
                    available: true,
                });
            }
        }
    }

    #[cfg(not(windows))]
    {
        // On Unix, check for common shells
        let unix_shells = [
            ("/bin/bash", "Bash", "Bourne Again Shell"),
            ("/bin/zsh", "Zsh", "Z Shell"),
            ("/bin/fish", "Fish", "Friendly Interactive Shell"),
            ("/bin/sh", "sh", "Bourne Shell"),
        ];

        for (path, name, _desc) in unix_shells {
            if std::path::Path::new(path).exists() {
                shells.push(AvailableShell {
                    shell_type: ShellType::Custom {
                        path: path.to_string(),
                        args: vec![],
                    },
                    name: name.to_string(),
                    available: true,
                });
            }
        }
    }

    shells
}

/// Helper to check if an executable exists in PATH without launching a subprocess
#[cfg(windows)]
fn is_in_path(cmd: &str) -> bool {
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let full = dir.join(cmd);
            if full.is_file() {
                return true;
            }
        }
    }
    false
}

/// Check if PowerShell Core (pwsh.exe) is available
///
/// Uses zero-process PATH and filesystem checks instead of launching `pwsh.exe -Version`
/// which avoids expensive .NET CLR initialization (takes >1000ms) on UI threads.
#[cfg(windows)]
fn is_pwsh_available() -> bool {
    if is_in_path("pwsh.exe") || is_in_path("pwsh") {
        return true;
    }

    // Check standard installation paths
    if let Ok(program_files) = std::env::var("ProgramFiles") {
        let p = std::path::Path::new(&program_files).join(r"PowerShell\7\pwsh.exe");
        if p.is_file() {
            return true;
        }
    }
    if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
        let p = std::path::Path::new(&program_files_x86).join(r"PowerShell\7\pwsh.exe");
        if p.is_file() {
            return true;
        }
    }
    if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
        let p = std::path::Path::new(&local_appdata).join(r"Microsoft\WindowsApps\pwsh.exe");
        if p.is_file() {
            return true;
        }
    }

    false
}

/// Detect installed WSL distributions on Windows.
///
/// Fast-path reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss` from the Windows
/// Registry (<0.1ms, zero subprocesses), completely eliminating UI stutter from `wsl.exe -l -q`.
#[cfg(windows)]
pub fn detect_wsl_distros() -> Vec<String> {
    if let Ok(hkcu) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Lxss")
    {
        let mut distros = Vec::new();
        for subkey_name in hkcu.enum_keys().filter_map(|k| k.ok()) {
            if let Ok(sub) = hkcu.open_subkey(&subkey_name) {
                if let Ok(name) = sub.get_value::<String, _>("DistributionName") {
                    let trimmed = name.trim().trim_matches('\0');
                    if !trimmed.is_empty() {
                        distros.push(trimmed.to_string());
                    }
                }
            }
        }
        if !distros.is_empty() {
            return distros;
        }
    }

    Vec::new()
}

/// Detect installed WSL distributions on non-Windows platforms (always empty).
#[cfg(not(windows))]
pub fn detect_wsl_distros() -> Vec<String> {
    Vec::new()
}

/// Parse a WSL UNC path into (distro_name, linux_path).
///
/// Recognized formats:
/// - `\\wsl.localhost\Distro\path` or `\\wsl$\Distro\path` (backslash)
/// - `//wsl.localhost/Distro/path` or `//wsl$/Distro/path` (forward-slash)
#[cfg(windows)]
pub fn parse_wsl_unc_path(path: &str) -> Option<(String, String)> {
    let normalized = path.replace('\\', "/");

    // Must start with // (UNC prefix after normalization)
    let rest = normalized.strip_prefix("//")?;

    // Check for wsl.localhost/ or wsl$/
    let after_host = rest.strip_prefix("wsl.localhost/")
        .or_else(|| rest.strip_prefix("wsl$/"))?;

    // Next segment is the distro name
    let (distro, linux_path) = match after_host.find('/') {
        Some(idx) => (&after_host[..idx], &after_host[idx..]),
        None => (after_host, "/"),
    };

    if distro.is_empty() {
        return None;
    }

    Some((distro.to_string(), linux_path.to_string()))
}

/// Convert a Windows path to WSL path format
/// Example: C:\Users\name -> /mnt/c/Users/name
/// Also handles WSL UNC paths: \\wsl.localhost\Ubuntu\home\user -> /home/user
#[cfg(windows)]
pub fn windows_path_to_wsl(windows_path: &str) -> String {
    // Check for WSL UNC paths first
    if let Some((_distro, linux_path)) = parse_wsl_unc_path(windows_path) {
        return linux_path;
    }

    let path = windows_path.replace('\\', "/");

    // Check for drive letter (e.g., C:/)
    if path.len() >= 2 && path.chars().nth(1) == Some(':') {
        if let Some(drive) = path.chars().next() {
            let rest = &path[2..];
            format!("/mnt/{}{}", drive.to_ascii_lowercase(), rest)
        } else {
            // Fallback: should not happen if len >= 2, but return path as-is
            path
        }
    } else {
        // Relative path or already Unix-style
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn test_windows_path_to_wsl() {
        assert_eq!(
            windows_path_to_wsl("C:\\Users\\test"),
            "/mnt/c/Users/test"
        );
        assert_eq!(
            windows_path_to_wsl("D:\\Projects\\app"),
            "/mnt/d/Projects/app"
        );
        assert_eq!(windows_path_to_wsl("/already/unix"), "/already/unix");
    }

    #[test]
    #[cfg(windows)]
    fn test_wsl_unc_path_conversion() {
        // Backslash UNC paths
        assert_eq!(
            windows_path_to_wsl("\\\\wsl.localhost\\Ubuntu\\home\\user\\project"),
            "/home/user/project"
        );
        assert_eq!(
            windows_path_to_wsl("\\\\wsl$\\Ubuntu\\home\\user"),
            "/home/user"
        );
        // Forward-slash UNC paths
        assert_eq!(
            windows_path_to_wsl("//wsl.localhost/Debian/tmp"),
            "/tmp"
        );
        assert_eq!(
            windows_path_to_wsl("//wsl$/Arch/etc/config"),
            "/etc/config"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_parse_wsl_unc_path() {
        // wsl.localhost backslash
        let (distro, path) = parse_wsl_unc_path("\\\\wsl.localhost\\Ubuntu\\home\\user").unwrap();
        assert_eq!(distro, "Ubuntu");
        assert_eq!(path, "/home/user");

        // wsl$ backslash
        let (distro, path) = parse_wsl_unc_path("\\\\wsl$\\Debian\\tmp\\file").unwrap();
        assert_eq!(distro, "Debian");
        assert_eq!(path, "/tmp/file");

        // Forward-slash variant
        let (distro, path) = parse_wsl_unc_path("//wsl.localhost/Arch/etc").unwrap();
        assert_eq!(distro, "Arch");
        assert_eq!(path, "/etc");

        // Distro only (no sub-path)
        let (distro, path) = parse_wsl_unc_path("\\\\wsl.localhost\\Ubuntu").unwrap();
        assert_eq!(distro, "Ubuntu");
        assert_eq!(path, "/");

        // Not a WSL UNC path
        assert!(parse_wsl_unc_path("C:\\Users\\test").is_none());
        assert!(parse_wsl_unc_path("/regular/path").is_none());
    }

    #[test]
    fn to_command_string_custom_no_args() {
        let shell = ShellType::Custom {
            path: "/usr/bin/fish".to_string(),
            args: vec![],
        };
        assert_eq!(shell.to_command_string(), "/usr/bin/fish");
    }

    #[test]
    fn test_shell_type_display_name() {
        assert_eq!(ShellType::Default.display_name(), "System Default");

        let custom = ShellType::Custom {
            path: "/bin/bash".to_string(),
            args: vec![],
        };
        assert_eq!(custom.display_name(), "bash");
    }

    #[test]
    fn test_available_shells_caching() {
        let first = available_shells();
        assert!(!first.is_empty());
        let second = available_shells();
        assert_eq!(first.len(), second.len());

        let refreshed = refresh_available_shells();
        assert_eq!(first.len(), refreshed.len());
    }

    #[test]
    fn test_detect_wsl_distros_does_not_panic() {
        let _ = detect_wsl_distros();
    }
}
