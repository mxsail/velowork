//! Subprocess and updates directory resolution for velowork-updater.

/// Create a [`std::process::Command`] that does **not** flash a console window
/// on Windows. Delegates to the shared helper in `velowork-core`.
pub fn command(program: &str) -> std::process::Command {
    velowork_core::process::command(program)
}

/// Get the updates directory for the active profile or cache root.
pub fn get_updates_dir() -> std::path::PathBuf {
    if let Some(p) = velowork_core::profiles::try_current() {
        p.updates_dir()
    } else {
        dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("velowork")
            .join("updates")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_updates_dir_ends_with_updates() {
        let dir = get_updates_dir();
        assert!(dir.ends_with("updates"));
    }
}

