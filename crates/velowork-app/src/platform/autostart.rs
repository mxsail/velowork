//! Native cross-platform autostart support for Velowork.
//!
//! - **macOS**: Writes `~/Library/LaunchAgents/com.mxsail.velowork.plist`.
//! - **Linux**: Writes `~/.config/autostart/velowork.desktop`.
//! - **Windows**: Manages `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Velowork`
//!   via Windows `reg.exe` with `CREATE_NO_WINDOW` to avoid console flicker.

use std::path::PathBuf;

/// Check if autostart is supported on the current platform.
pub fn is_autostart_supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos", target_os = "windows"))
}

/// Check if autostart is currently enabled in the OS.
pub fn is_autostart_enabled() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::desktop_file_path()
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    #[cfg(target_os = "macos")]
    {
        macos::plist_path()
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    #[cfg(target_os = "windows")]
    {
        windows::is_registered()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// Enable or disable autostart.
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        if enabled {
            linux::enable()
        } else {
            linux::disable()
        }
    }

    #[cfg(target_os = "macos")]
    {
        if enabled {
            macos::enable()
        } else {
            macos::disable()
        }
    }

    #[cfg(target_os = "windows")]
    {
        if enabled {
            windows::enable()
        } else {
            windows::disable()
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = enabled;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Linux (XDG Desktop Entry)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::fs;

    pub fn desktop_file_path() -> Option<PathBuf> {
        dirs::config_dir().map(|cfg| cfg.join("autostart").join("velowork.desktop"))
    }

    pub fn enable() -> std::io::Result<()> {
        let Some(path) = desktop_file_path() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine config directory",
            ));
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let exe = std::env::current_exe()?;
        let exe_str = exe.to_string_lossy();

        let content = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Velowork\n\
             Comment=Terminal workspace\n\
             Exec=\"{exe_str}\"\n\
             Terminal=false\n\
             StartupNotify=false\n\
             Categories=Development;System;TerminalEmulator;\n"
        );

        fs::write(&path, content)?;
        log::info!("Linux autostart desktop entry created at {:?}", path);
        Ok(())
    }

    pub fn disable() -> std::io::Result<()> {
        if let Some(path) = desktop_file_path()
            && path.exists()
        {
            fs::remove_file(&path)?;
            log::info!("Linux autostart desktop entry removed from {:?}", path);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// macOS (LaunchAgent Plist)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::fs;

    pub fn plist_path() -> Option<PathBuf> {
        dirs::home_dir().map(|home| {
            home.join("Library")
                .join("LaunchAgents")
                .join("com.mxsail.velowork.plist")
        })
    }

    pub fn enable() -> std::io::Result<()> {
        let Some(path) = plist_path() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine home directory",
            ));
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let exe = std::env::current_exe()?;
        let exe_str = exe.to_string_lossy();

        // Escape XML entities in executable path
        let escaped_exe = exe_str
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;");

        let content = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
                 <key>Label</key>\n\
                 <string>com.mxsail.velowork</string>\n\
                 <key>ProgramArguments</key>\n\
                 <array>\n\
                     <string>{escaped_exe}</string>\n\
                 </array>\n\
                 <key>RunAtLoad</key>\n\
                 <true/>\n\
                 <key>ProcessType</key>\n\
                 <string>Interactive</string>\n\
             </dict>\n\
             </plist>\n"
        );

        fs::write(&path, content)?;
        log::info!("macOS autostart LaunchAgent created at {:?}", path);
        Ok(())
    }

    pub fn disable() -> std::io::Result<()> {
        if let Some(path) = plist_path()
            && path.exists()
        {
            fs::remove_file(&path)?;
            log::info!("macOS autostart LaunchAgent removed from {:?}", path);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Windows (Registry via reg.exe)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const APP_NAME: &str = "Velowork";

    pub fn is_registered() -> bool {
        let mut cmd = Command::new("reg.exe");
        cmd.args(["query", REG_KEY, "/v", APP_NAME]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        match cmd.status() {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    pub fn enable() -> std::io::Result<()> {
        let exe = std::env::current_exe()?;
        let exe_str = exe.to_string_lossy();
        let quoted_val = format!("\"{}\"", exe_str);

        let mut cmd = Command::new("reg.exe");
        cmd.args(["add", REG_KEY, "/v", APP_NAME, "/t", "REG_SZ", "/d", &quoted_val, "/f"]);
        cmd.creation_flags(CREATE_NO_WINDOW);

        let status = cmd.status()?;
        if status.success() {
            log::info!("Windows autostart registry entry added for {:?}", exe);
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("reg.exe add failed with status: {status}"),
            ))
        }
    }

    pub fn disable() -> std::io::Result<()> {
        let mut cmd = Command::new("reg.exe");
        cmd.args(["delete", REG_KEY, "/v", APP_NAME, "/f"]);
        cmd.creation_flags(CREATE_NO_WINDOW);

        let _ = cmd.status();
        log::info!("Windows autostart registry entry removed");
        Ok(())
    }
}
