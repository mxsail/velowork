use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_ROOT: OnceLock<DataRoot> = OnceLock::new();

/// Data Root storage mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DataRootMode {
    /// System standard config directory (e.g. `~/.config/velowork` or `%APPDATA%/velowork`).
    #[default]
    Default,
    /// Application executable directory (portable mode, e.g. `<app_dir>/data`).
    AppDir,
    /// User-specified custom directory.
    Custom,
}

impl DataRootMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Default => "System Default",
            Self::AppDir => "Application Directory (Portable)",
            Self::Custom => "Custom Directory",
        }
    }
}

/// Bootstrap configuration persisted in `bootstrap.json`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapConfig {
    pub mode: DataRootMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_path: Option<PathBuf>,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            mode: DataRootMode::Default,
            custom_path: None,
        }
    }
}

impl BootstrapConfig {
    /// Load bootstrap configuration from a given file path.
    pub fn load_from_file(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save bootstrap configuration atomically to a directory as `bootstrap.json`.
    pub fn save_to_dir(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let file = dir.join("bootstrap.json");
        let val = serde_json::to_value(self)?;
        crate::atomic_io::write_json_atomic(&file, &val)
    }
}

/// The resolved Data Root for the running process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataRoot {
    mode: DataRootMode,
    root: PathBuf,
}

impl DataRoot {
    pub fn new(mode: DataRootMode, root: PathBuf) -> Self {
        Self { mode, root }
    }

    /// Active data root mode.
    pub fn mode(&self) -> DataRootMode {
        self.mode
    }

    /// Canonical path to data root (`<config_root>`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Global profiles index: `<config_root>/profiles.json`.
    pub fn profiles_index_path(&self) -> PathBuf {
        self.root.join("profiles.json")
    }

    /// Profiles directory: `<config_root>/profiles/`.
    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }

    /// Root directory for a specific profile: `<config_root>/profiles/<id>/`.
    pub fn profile_root(&self, id: &str) -> PathBuf {
        self.profiles_dir().join(id)
    }

    /// Global logs directory: `<config_root>/logs/`.
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Profile-scoped log directory: `<config_root>/logs/<id>/`.
    pub fn profile_logs_dir(&self, id: &str) -> PathBuf {
        self.logs_dir().join(id)
    }

    /// Profile application log file: `<config_root>/logs/<id>/velowork.log`.
    pub fn profile_log_path(&self, id: &str) -> PathBuf {
        self.profile_logs_dir(id).join("velowork.log")
    }

    /// Terminal recordings archive directory: `<config_root>/logs/recordings/`.
    pub fn recordings_dir(&self) -> PathBuf {
        self.logs_dir().join("recordings")
    }

    /// Instance lock files directory: `<config_root>/locks/`.
    pub fn locks_dir(&self) -> PathBuf {
        self.root.join("locks")
    }

    /// Instance lock file for a profile: `<config_root>/locks/<id>.lock`.
    pub fn lock_path(&self, id: &str) -> PathBuf {
        self.locks_dir().join(format!("{id}.lock"))
    }

    /// Runtime IPC/sockets directory for a profile: `<config_root>/runtime/<id>/`.
    pub fn runtime_dir(&self, id: &str) -> PathBuf {
        self.root.join("runtime").join(id)
    }

    /// Global cache directory: `<config_root>/cache/`.
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Global installed plugins directory: `<config_root>/plugins/`.
    pub fn global_plugins_dir(&self) -> PathBuf {
        self.root.join("plugins")
    }

    /// Global plugin marketplace cache directory: `<config_root>/marketplace/`.
    pub fn marketplace_dir(&self) -> PathBuf {
        self.root.join("marketplace")
    }
}

// ─── Global Singleton Management ──────────────────────────────────────────────

/// Initialize the global DataRoot. Must be called once during startup.
pub fn init(data_root: DataRoot) {
    #[allow(clippy::expect_used)]
    DATA_ROOT
        .set(data_root)
        .expect("data_root::init called more than once");
}

/// Returns a reference to the active global DataRoot.
pub fn current() -> &'static DataRoot {
    #[allow(clippy::expect_used)]
    DATA_ROOT
        .get()
        .expect("DataRoot not initialized — call data_root::init() first")
}

/// Returns the active global DataRoot if initialized, otherwise `None`.
pub fn try_current() -> Option<&'static DataRoot> {
    DATA_ROOT.get()
}

// ─── Resolution Strategy ──────────────────────────────────────────────────────

/// System standard config directory (`~/Library/Application Support/velowork` on macOS; `~/.config/velowork` on Linux; `%APPDATA%/velowork` on Windows).
pub fn system_default_data_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("velowork")
}

/// Application executable directory.
pub fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Portable application data directory (`<app_dir>/.config`).
pub fn app_portable_data_root() -> PathBuf {
    app_dir().join(".config")
}

/// Check if a directory exists and is empty (contains no entries).
pub fn is_dir_empty(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => false,
    }
}

/// Check whether a path looks like a Cargo build directory (target/debug, target/release, etc.)
pub fn is_cargo_target_dir(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("/target/debug")
        || s.contains("\\target\\debug")
        || s.contains("/target/release")
        || s.contains("\\target\\release")
        || s.contains("/target/deps")
        || s.contains("\\target\\deps")
        || s.ends_with("/target")
        || s.ends_with("\\target")
}

/// Resolve Data Root following strict precedence:
/// 1. Explicit CLI flag (`--config-root <path>`, `--portable`) or `VELOWORK_CONFIG_ROOT` env var.
/// 2. App-local marker/config (`<app_dir>/portable` or `<app_dir>/bootstrap.json`).
/// 3. Global system config (`<system_default>/bootstrap.json`).
/// 4. Fallback to System Default.
pub fn resolve_data_root(
    cli_override: Option<PathBuf>,
    cli_portable: bool,
) -> Result<DataRoot> {
    // 1. CLI / Env explicit override
    if cli_portable {
        let root = app_portable_data_root();
        return Ok(DataRoot::new(DataRootMode::AppDir, root));
    }

    if let Some(path) = cli_override {
        return Ok(DataRoot::new(DataRootMode::Custom, path));
    }

    if let Ok(env_path) = std::env::var("VELOWORK_CONFIG_ROOT")
        && !env_path.trim().is_empty() {
        let path = PathBuf::from(env_path.trim());
        return Ok(DataRoot::new(DataRootMode::Custom, path));
    }

    let exe_dir = app_dir();
    let is_cargo_dev = is_cargo_target_dir(&exe_dir);

    // 2. App-local portable marker or bootstrap.json
    // In cargo dev builds (target/debug, etc.), ignore portable markers to prevent volatile target dir from eating user data
    if !is_cargo_dev && (exe_dir.join("portable").exists() || exe_dir.join("velowork.portable").exists()) {
        let root = app_portable_data_root();
        return Ok(DataRoot::new(DataRootMode::AppDir, root));
    }

    let app_bootstrap = exe_dir.join("bootstrap.json");
    if !is_cargo_dev && let Some(config) = BootstrapConfig::load_from_file(&app_bootstrap) {
        return data_root_from_bootstrap(config);
    }

    // 3. System default bootstrap.json
    let system_default = system_default_data_root();
    let sys_bootstrap = system_default.join("bootstrap.json");
    if let Some(config) = BootstrapConfig::load_from_file(&sys_bootstrap) {
        // If system bootstrap specifies AppDir mode but we're in a cargo dev build or running without app-dir portable marker, fallback safely to default
        if config.mode == DataRootMode::AppDir && (is_cargo_dev || !exe_dir.join("portable").exists()) {
            return Ok(DataRoot::new(DataRootMode::Default, system_default));
        }
        return data_root_from_bootstrap(config);
    }

    // 4. Default fallback
    Ok(DataRoot::new(DataRootMode::Default, system_default))
}

fn data_root_from_bootstrap(config: BootstrapConfig) -> Result<DataRoot> {
    match config.mode {
        DataRootMode::Default => Ok(DataRoot::new(DataRootMode::Default, system_default_data_root())),
        DataRootMode::AppDir => Ok(DataRoot::new(DataRootMode::AppDir, app_portable_data_root())),
        DataRootMode::Custom => {
            if let Some(path) = config.custom_path {
                Ok(DataRoot::new(DataRootMode::Custom, path))
            } else {
                Ok(DataRoot::new(DataRootMode::Default, system_default_data_root()))
            }
        }
    }
}

/// Save bootstrap setting for subsequent launches.
pub fn persist_bootstrap_config(config: &BootstrapConfig) -> Result<()> {
    let sys_default = system_default_data_root();
    config.save_to_dir(&sys_default)?;

    let exe_dir = app_dir();
    let is_cargo_dev = is_cargo_target_dir(&exe_dir);

    if !is_cargo_dev {
        let app_portable_marker = exe_dir.join("portable");
        let app_bootstrap = exe_dir.join("bootstrap.json");

        if config.mode == DataRootMode::AppDir {
            // If app dir is writable, place marker / bootstrap there as well
            let _ = std::fs::write(&app_portable_marker, b"");
            let _ = config.save_to_dir(&exe_dir);
        } else {
            // Clean up app-dir portable markers if switching away from AppDir mode
            let _ = std::fs::remove_file(&app_portable_marker);
            let _ = std::fs::remove_file(&app_bootstrap);
        }
    }

    Ok(())
}

/// Helper to recursively copy directories for Data Root migration.
/// Skips active lock files.
pub fn migrate_data_root(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        bail!("Source directory does not exist: {}", from.display());
    }
    std::fs::create_dir_all(to)?;
    copy_dir_recursive(from, to)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        // Skip lock files
        if name_str.ends_with(".lock") || name_str == "locks" {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&file_name);

        if file_type.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_bootstrap_load_save() {
        let temp = TempDir::new().unwrap();
        let config = BootstrapConfig {
            mode: DataRootMode::Custom,
            custom_path: Some(temp.path().join("my_custom_root")),
        };
        config.save_to_dir(temp.path()).unwrap();

        let loaded = BootstrapConfig::load_from_file(&temp.path().join("bootstrap.json")).unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn test_data_root_paths() {
        let root_path = PathBuf::from("/tmp/velowork_test_root");
        let dr = DataRoot::new(DataRootMode::Custom, root_path.clone());
        assert_eq!(dr.root(), root_path);
        assert_eq!(dr.logs_dir(), root_path.join("logs"));
        assert_eq!(dr.recordings_dir(), root_path.join("logs").join("recordings"));
        assert_eq!(dr.profile_logs_dir("default"), root_path.join("logs").join("default"));
        assert_eq!(dr.profile_log_path("default"), root_path.join("logs").join("default").join("velowork.log"));
        assert_eq!(dr.profiles_dir(), root_path.join("profiles"));
        assert_eq!(dr.profile_root("default"), root_path.join("profiles").join("default"));
        assert_eq!(dr.locks_dir(), root_path.join("locks"));
        assert_eq!(dr.lock_path("default"), root_path.join("locks").join("default.lock"));
    }

    #[test]
    fn test_copy_dir_recursive() {
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();

        std::fs::write(src.path().join("test.txt"), "hello").unwrap();
        std::fs::create_dir_all(src.path().join("sub")).unwrap();
        std::fs::write(src.path().join("sub").join("nested.txt"), "world").unwrap();
        std::fs::write(src.path().join("test.lock"), "lock").unwrap();

        migrate_data_root(src.path(), dst.path()).unwrap();

        assert_eq!(std::fs::read_to_string(dst.path().join("test.txt")).unwrap(), "hello");
        assert_eq!(std::fs::read_to_string(dst.path().join("sub").join("nested.txt")).unwrap(), "world");
        // Lock file should be skipped
        assert!(!dst.path().join("test.lock").exists());
    }

    #[test]
    fn test_app_portable_data_root() {
        let portable = app_portable_data_root();
        assert!(portable.ends_with(".config"));
    }

    #[test]
    fn test_is_dir_empty() {
        let temp = TempDir::new().unwrap();
        assert!(is_dir_empty(temp.path()));
        std::fs::write(temp.path().join("file.txt"), "hello").unwrap();
        assert!(!is_dir_empty(temp.path()));
    }
}
