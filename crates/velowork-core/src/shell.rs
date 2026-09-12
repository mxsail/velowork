//! Shell type — the serializable description of which shell a terminal runs.
//!
//! This is pure data plus pure-string helpers, so it lives in `velowork-core` and
//! can be referenced by data-only crates (`velowork-state`, `velowork-layout`) without
//! pulling in PTY/process machinery. The behavioral part — turning a `ShellType`
//! into a spawnable `portable_pty::CommandBuilder` — lives in `velowork-terminal`
//! (see `ShellCommandExt::build_command`).

use serde::{Deserialize, Serialize};

/// Shell type for terminal creation
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
#[derive(Default)]
pub enum ShellType {
    /// Use system default shell (CommandBuilder::new_default_prog())
    #[default]
    Default,

    /// Windows Command Prompt (cmd.exe)
    #[cfg(windows)]
    Cmd,

    /// Windows PowerShell or PowerShell Core
    #[cfg(windows)]
    PowerShell {
        /// Use pwsh.exe (PowerShell Core) instead of powershell.exe
        #[serde(default)]
        core: bool,
    },

    /// Windows Subsystem for Linux
    #[cfg(windows)]
    Wsl {
        /// Specific distro name, or None for default
        #[serde(default)]
        distro: Option<String>,
    },

    /// Custom shell with path and arguments
    Custom {
        path: String,
        #[serde(default)]
        args: Vec<String>,
    },

    /// Welcome / dashboard view tab
    Welcome,
}

impl ShellType {
    /// Create a shell type that runs a single command via the user's shell.
    /// Uses `$SHELL -ic` on Unix (interactive, so .bashrc/.zshrc is sourced)
    /// and `cmd /C` on Windows.
    pub fn for_command(command: String) -> Self {
        if cfg!(windows) {
            ShellType::Custom {
                path: "cmd".to_string(),
                args: vec!["/C".to_string(), command],
            }
        } else {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            ShellType::Custom {
                path: shell,
                args: vec!["-ic".to_string(), command],
            }
        }
    }

    /// Resolve `ShellType::Default` into a concrete shell by checking
    /// the project's default shell first, then the global setting.
    /// Non-Default variants are returned unchanged.
    pub fn resolve_default(self, project_shell: Option<&ShellType>, global_shell: &ShellType) -> ShellType {
        if self == ShellType::Default {
            project_shell.cloned().unwrap_or_else(|| global_shell.clone())
        } else {
            self
        }
    }

    /// Get a display name for this shell type
    pub fn display_name(&self) -> String {
        match self {
            ShellType::Default => "System Default".to_string(),
            ShellType::Welcome => "Welcome".to_string(),
            #[cfg(windows)]
            ShellType::Cmd => "Command Prompt".to_string(),
            #[cfg(windows)]
            ShellType::PowerShell { core: false } => "Windows PowerShell".to_string(),
            #[cfg(windows)]
            ShellType::PowerShell { core: true } => "PowerShell Core".to_string(),
            #[cfg(windows)]
            ShellType::Wsl { distro: None } => "WSL (Default)".to_string(),
            #[cfg(windows)]
            ShellType::Wsl { distro: Some(d) } => format!("WSL ({})", d),
            ShellType::Custom { path, .. } => {
                // Extract filename from path
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path)
                    .to_string()
            }
        }
    }

    /// Whether this shell runs on a remote host.
    ///
    /// Currently only the `ssh` custom shell is remote — it spawns a shell on a
    /// connected host rather than locally, so its shell cannot be switched via
    /// the local shell selector. Local shells (`Default` and non-ssh `Custom`)
    /// return `false`.
    pub fn is_remote(&self) -> bool {
        matches!(self, ShellType::Custom { path, .. } if path == "ssh")
    }

    /// Short, human-readable name of a *local* shell, used for compact UI
    /// labels (e.g. the command panel host list, terminal tab names, detached
    /// windows).
    ///
    /// * `Default` → the actual shell resolved from `$SHELL` on Unix (e.g.
    ///   `zsh`, `bash`), falling back to `sh`; on Windows its short name.
    /// * `Custom` → the executable's basename (e.g. `bash`, `fish`).
    /// * built-in variants (`Cmd`, `PowerShell`, `Wsl`) → their short name
    ///   (`CMD`, `pwsh`, `WSL`, ...).
    ///
    /// For remote (`ssh`) shells this is meaningless — callers should show the
    /// session name instead.
    pub fn local_shell_name(&self) -> String {
        match self {
            ShellType::Default => {
                #[cfg(not(windows))]
                {
                    std::env::var("SHELL")
                        .ok()
                        .and_then(|s| {
                            std::path::Path::new(&s)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| n.to_string())
                        })
                        .unwrap_or_else(|| "sh".to_string())
                }
                #[cfg(windows)]
                {
                    self.short_display_name().to_string()
                }
            }
            ShellType::Welcome => "Welcome".to_string(),
            ShellType::Custom { path, .. } => std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("sh")
                .to_string(),
            #[cfg(windows)]
            other => other.short_display_name().to_string(),
        }
    }

    /// Get a short display name for compact UI elements (e.g., shell indicator chips)
    pub fn short_display_name(&self) -> &'static str {
        match self {
            ShellType::Default => "Default",
            ShellType::Welcome => "Welcome",
            #[cfg(windows)]
            ShellType::Cmd => "CMD",
            #[cfg(windows)]
            ShellType::PowerShell { core } => {
                if *core { "pwsh" } else { "PS" }
            }
            #[cfg(windows)]
            ShellType::Wsl { .. } => "WSL",
            ShellType::Custom { .. } => "Custom",
        }
    }

    /// Convert to the full command string (executable + args).
    /// Used by shell_wrapper to produce the correct command to wrap.
    pub fn to_command_string(&self) -> String {
        match self {
            ShellType::Default => "${SHELL:-sh}".to_string(),
            ShellType::Welcome => String::new(),
            #[cfg(windows)]
            ShellType::Cmd => "cmd.exe".to_string(),
            #[cfg(windows)]
            ShellType::PowerShell { core } => {
                if *core { "pwsh.exe -NoLogo" } else { "powershell.exe -NoLogo" }.to_string()
            }
            #[cfg(windows)]
            ShellType::Wsl { distro } => {
                match distro {
                    Some(d) => format!("wsl.exe -d {}", d),
                    None => "wsl.exe".to_string(),
                }
            }
            ShellType::Custom { path, args } => {
                if args.is_empty() {
                    shell_quote(path)
                } else {
                    let quoted_args: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
                    format!("{} {}", shell_quote(path), quoted_args.join(" "))
                }
            }
        }
    }

    /// Extract the session id (if any) from a custom shell (`ssh`, `serial`, `telnet`, `local`).
    pub fn session_id(&self) -> Option<&str> {
        if let ShellType::Custom { path, args } = self
            && (path == "ssh" || path == "serial" || path == "telnet" || path == "local") {
            let mut i = 0;
            while i < args.len() {
                if (args[i] == "--id" || args[i] == "--session-id") && i + 1 < args.len() {
                    return Some(&args[i + 1]);
                }
                i += 1;
            }
        }
        None
    }

    /// Extract the SSH session id (if any) from an `ssh` custom shell's `--id` / `--session-id` flag.
    /// Backward-compatible alias for [`session_id`](Self::session_id).
    pub fn ssh_session_id(&self) -> Option<&str> {
        self.session_id()
    }
}

/// Shell-quote a string for embedding in a shell command.
/// Returns the string as-is if it contains no special characters,
/// otherwise wraps in single quotes with proper escaping.
fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    // If it only contains safe characters, no quoting needed
    if s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'.' || b == b'-' || b == b'_' || b == b'=' || b == b':') {
        return s.to_string();
    }
    // Single-quote and escape embedded single quotes
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_extraction() {
        let ssh_shell = ShellType::Custom {
            path: "ssh".to_string(),
            args: vec![
                "-p".to_string(),
                "22".to_string(),
                "--id".to_string(),
                "session-123".to_string(),
                "user@host".to_string(),
            ],
        };
        assert_eq!(ssh_shell.session_id(), Some("session-123"));
        assert_eq!(ssh_shell.ssh_session_id(), Some("session-123"));

        let serial_shell = ShellType::Custom {
            path: "serial".to_string(),
            args: vec![
                "--id".to_string(),
                "serial-456".to_string(),
                "--port".to_string(),
                "COM3".to_string(),
                "--baud".to_string(),
                "115200".to_string(),
            ],
        };
        assert_eq!(serial_shell.session_id(), Some("serial-456"));

        let telnet_shell = ShellType::Custom {
            path: "telnet".to_string(),
            args: vec![
                "--id".to_string(),
                "telnet-789".to_string(),
                "--host".to_string(),
                "192.168.1.1".to_string(),
                "--port".to_string(),
                "23".to_string(),
            ],
        };
        assert_eq!(telnet_shell.session_id(), Some("telnet-789"));

        let local_shell = ShellType::Custom {
            path: "local".to_string(),
            args: vec![
                "--id".to_string(),
                "local-101".to_string(),
                "--shell".to_string(),
                "zsh".to_string(),
            ],
        };
        assert_eq!(local_shell.session_id(), Some("local-101"));

        let default_shell = ShellType::Default;
        assert_eq!(default_shell.session_id(), None);
        assert_eq!(default_shell.ssh_session_id(), None);
    }
}
