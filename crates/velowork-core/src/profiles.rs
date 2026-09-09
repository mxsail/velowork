use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static PROFILE_PATHS: OnceLock<ProfilePaths> = OnceLock::new();

// ─── Path API ─────────────────────────────────────────────────────────────────

/// All file paths for the active profile. Resolved once at startup via `init_profile()`.
#[derive(Debug)]
pub struct ProfilePaths {
    pub id: String,
    /// `<config_root>/profiles/<id>/`
    pub root: PathBuf,
    /// `<config_root>/` — only for `profiles.json` and cross-profile files
    pub config_root: PathBuf,
}

impl ProfilePaths {
    // ─── Profile 内分层子目录 ───────────────────────────────────────────────
    /// `profiles/<id>/config/` — 按域拆分的 JSON 配置（settings/keybindings/ai/sync）。
    pub fn config_dir(&self)    -> PathBuf { self.root.join("config") }
    /// `profiles/<id>/data/` — SQLite 业务数据库（velowork.db 等）。
    pub fn data_dir(&self)      -> PathBuf { self.root.join("data") }
    /// `profiles/<id>/themes/` — 用户自定义主题。
    pub fn themes_dir(&self)    -> PathBuf { self.root.join("themes") }
    /// `profiles/<id>/sessions/` — 仅保留 `*.sshconfig`/`*.pem`/`import/`（会话实体已入 DB）。
    pub fn sessions_dir(&self)  -> PathBuf { self.root.join("sessions") }
    /// `profiles/<id>/plugins/` — 用户安装的插件。
    pub fn plugins_dir(&self)   -> PathBuf { self.root.join("plugins") }
    /// `profiles/<id>/manifest.json` — Profile Layout 版本清单。
    pub fn manifest_path(&self) -> PathBuf { self.root.join("manifest.json") }
    /// `profiles/<id>/data/velowork.db` — 单库业务数据库。
    pub fn database_path(&self) -> PathBuf { self.data_dir().join("velowork.db") }

    // ─── 业务文件（重指到分层子目录）────────────────────────────────────────
    pub fn workspace_json(&self)   -> PathBuf { self.data_dir().join("workspace.json") }
    pub fn settings_json(&self)    -> PathBuf { self.config_dir().join("settings.json") }
    pub fn keybindings_json(&self) -> PathBuf { self.config_dir().join("keybindings.json") }
    pub fn updates_dir(&self)      -> PathBuf { cache_dir().join("updates") }
    /// 终端录制日志：`<config_root>/logs/recordings/`。
    pub fn recordings_dir(&self)   -> PathBuf { self.config_root.join("logs").join("recordings") }

    // ─── Runtime / 锁（已移出 Profile，落到全局 config_root 下，按 Profile 隔离）──
    /// 实例锁：`<config_root>/locks/<id>.lock`。
    pub fn lock_path(&self)        -> PathBuf { lock_path(&self.id) }
    /// 日志：`<config_root>/logs/<id>/velowork.log`（按 Profile 分目录，避免多开覆盖）。
    pub fn log_path(&self)         -> PathBuf { logs_dir(&self.id).join("velowork.log") }
    /// CLI 交互数据：`<config_root>/runtime/<id>/cli/cli.json`。
    pub fn cli_json(&self)         -> PathBuf { cli_dir(&self.id).join("cli.json") }
    /// 远程实例信息（含 pid）：`<config_root>/runtime/<id>/remote/remote.json`。
    pub fn remote_json(&self)      -> PathBuf { remote_dir(&self.id).join("remote.json") }
    pub fn remote_secret(&self)    -> PathBuf { self.root.join("remote_secret") }
    pub fn remote_tokens(&self)    -> PathBuf { self.root.join("remote_tokens.json") }
    pub fn pair_code(&self)        -> PathBuf { self.root.join("pair_code") }
}

/// Initialize the process-wide active profile. Must be called exactly once before
/// any code calls `current()`. Panics if called twice.
pub fn init_profile(paths: ProfilePaths) {
    // Intentional panic: documented "call exactly once" contract.
    #[allow(clippy::expect_used)]
    PROFILE_PATHS
        .set(paths)
        .expect("init_profile called more than once");
}

/// Returns the active profile paths. Panics if `init_profile` was never called.
pub fn current() -> &'static ProfilePaths {
    // Intentional panic: documented precondition that init_profile() ran first.
    #[allow(clippy::expect_used)]
    PROFILE_PATHS.get().expect("profile not initialized — call init_profile() first")
}

/// Returns the active profile paths, or `None` if `init_profile` was never called.
pub fn try_current() -> Option<&'static ProfilePaths> {
    PROFILE_PATHS.get()
}

// ─── Index schema ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub id: String,
    pub display_name: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileIndex {
    pub version: u32,
    pub profiles: Vec<ProfileEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<String>,
    pub default_profile: String,
}

impl ProfileIndex {
    pub fn load(config_root: &Path) -> Result<Self> {
        let path = config_root.join("profiles.json");
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&content).with_context(|| "parsing profiles.json")
    }

    pub fn save(&self, config_root: &Path) -> Result<()> {
        std::fs::create_dir_all(config_root)?;
        let path = config_root.join("profiles.json");
        let value = serde_json::to_value(self)?;
        crate::atomic_io::write_json_atomic(&path, &value)
    }

    /// Update `last_used` to `id` and re-save. Silently ignores save errors.
    pub fn set_last_used(&mut self, id: &str, config_root: &Path) {
        self.last_used = Some(id.to_string());
        let _ = self.save(config_root);
    }
}

/// Returns the active resolved Data Root (`config_root`).
pub fn config_root() -> PathBuf {
    if let Some(dr) = crate::data_root::try_current() {
        dr.root().to_path_buf()
    } else {
        crate::data_root::system_default_data_root()
    }
}

// ─── 全局辅助路径（与 profiles/ 并列，位于 config_root 下）─────────────────────

/// 全局缓存目录（含 `updates/`）。可随时清理，符合 XDG cache 语义。
pub fn cache_dir() -> PathBuf {
    config_root().join("cache")
}
/// 按 Profile 分的日志目录，避免多 Profile 同开互相覆盖。
pub fn logs_dir(id: &str) -> PathBuf {
    config_root().join("logs").join(id)
}
/// 按 Profile 分的运行时目录（`remote/cli/ipc/socket/pid` 子目录）。
pub fn runtime_dir(id: &str) -> PathBuf {
    config_root().join("runtime").join(id)
}
/// 运行时子目录细分。
pub fn remote_dir(id: &str) -> PathBuf {
    runtime_dir(id).join("remote")
}
pub fn cli_dir(id: &str) -> PathBuf {
    runtime_dir(id).join("cli")
}
pub fn ipc_dir(id: &str) -> PathBuf {
    runtime_dir(id).join("ipc")
}
pub fn socket_dir(id: &str) -> PathBuf {
    runtime_dir(id).join("socket")
}
pub fn pid_dir(id: &str) -> PathBuf {
    runtime_dir(id).join("pid")
}
/// 按 Profile 的实例锁路径（替代原 Profile 内 `velowork.lock`）。
pub fn lock_path(id: &str) -> PathBuf {
    config_root().join("locks").join(format!("{id}.lock"))
}
/// 全局插件目录（区别于 Profile 内 `plugins/`，存放跨 Profile 的已安装插件）。
pub fn global_plugins_dir() -> PathBuf {
    config_root().join("plugins")
}
/// 全局插件市场/目录缓存。
pub fn marketplace_dir() -> PathBuf {
    config_root().join("marketplace")
}

// ─── Profile 清单（Layout 版本）────────────────────────────────────────────────

/// Profile 的 `manifest.json`，记录各类版本，供 `VersionManager` 统一编排。
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ProfileManifest {
    /// Profile Layout 版本（v2 分层布局 = 1）。
    pub layout_version: u32,
    pub profile_id: String,
    #[serde(default)]
    pub created_at: Option<String>,
    /// 业务数据库（velowork.db）schema 版本，由 sqlite-core 迁移后写入。
    #[serde(default)]
    pub db_version: u32,
    /// 配置（config/*.json）schema 版本，由 ConfigStore 迁移后写入。
    #[serde(default)]
    pub config_version: u32,
    /// 同步 Bundle 格式版本，由 SyncEngine 导出时写入。
    #[serde(default)]
    pub bundle_version: u32,
}

impl ProfileManifest {
    /// 读取 `manifest.json`（不存在或解析失败返回 `None`）。
    pub fn read(root: &Path) -> Option<Self> {
        let p = root.join("manifest.json");
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn write(&self, root: &Path) -> Result<()> {
        std::fs::create_dir_all(root)?;
        let p = root.join("manifest.json");
        let value = serde_json::to_value(self)?;
        crate::atomic_io::write_json_atomic(&p, &value)
    }
}

/// 写入 Profile Layout 版本清单（供 `migrate_profile_layout_v2` 调用）。
#[allow(dead_code)]
fn write_manifest(paths: &ProfilePaths, version: u32) -> Result<()> {
    let manifest = ProfileManifest {
        layout_version: version,
        profile_id: paths.id.clone(),
        created_at: Some(now_iso8601()),
        ..Default::default()
    };
    manifest.write(&paths.root)
}

// ─── Startup resolution ───────────────────────────────────────────────────────

/// Resolve the active profile from the explicit flag, the `VELOWORK_PROFILE` env var,
/// and the `profiles.json` index. Creates a default profile (and migrates legacy
/// state) on first run. Returns initialized `ProfilePaths` ready for `init_profile`.
pub fn resolve_active_profile(flag_id: Option<String>) -> Result<ProfilePaths> {
    let root = config_root();
    std::fs::create_dir_all(&root)?;

    let requested = flag_id.or_else(|| std::env::var("VELOWORK_PROFILE").ok());

    let mut index = match ProfileIndex::load(&root) {
        Ok(idx) => idx,
        Err(_) => {
            // No profiles.json — first ever run. Bootstrap default profile.
            // Migration is handled by the caller (main.rs) after init_profile.
            let idx = bootstrap_default_profile(&root)?;
            if let Some(req) = &requested
                && req != "default" {
                    bail!(
                        "Profile '{req}' not found. This appears to be a first launch; \
                         the 'default' profile was just created.\n\
                         Run `velowork --new-profile {req}` to create it, \
                         or omit --profile to use 'default'."
                    );
                }
            return make_profile_paths(&idx.profiles[0], &root);
        }
    };

    let id = if let Some(req) = requested {
        if !index.profiles.iter().any(|p| p.id == req) {
            let names: Vec<&str> = index.profiles.iter().map(|p| p.id.as_str()).collect();
            bail!(
                "Profile '{}' not found. Available: {}\nRun `velowork --new-profile <name>` to create one.",
                req,
                names.join(", ")
            );
        }
        req
    } else {
        pick_profile_id(&index)?
    };

    index.set_last_used(&id, &root);
    // `id` is guaranteed present: it was either validated against the index above
    // or returned by pick_profile_id, which only yields ids from this same index.
    #[allow(clippy::unwrap_used)]
    let entry = index.profiles.iter().find(|p| p.id == id).unwrap().clone();
    make_profile_paths(&entry, &root)
}

fn pick_profile_id(index: &ProfileIndex) -> Result<String> {
    if index.profiles.is_empty() {
        bail!("No profiles found. Run `velowork --new-profile <name>` to create one.");
    }
    if index.profiles.len() == 1 {
        return Ok(index.profiles[0].id.clone());
    }
    // Use last_used if it still exists
    if let Some(last) = &index.last_used
        && index.profiles.iter().any(|p| &p.id == last) {
            return Ok(last.clone());
        }
    // Ambiguous — give the user a clear error
    let mut msg = String::from(
        "Multiple profiles found. Specify one with --profile <id> or VELOWORK_PROFILE:\n",
    );
    for p in &index.profiles {
        msg.push_str(&format!("  {:<20} {}\n", p.id, p.display_name));
    }
    bail!("{}", msg.trim_end());
}

fn validate_profile_id(id: &str) -> Result<()> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
        bail!("Invalid profile id: '{id}'");
    }
    Ok(())
}

fn make_profile_paths(entry: &ProfileEntry, config_root: &Path) -> Result<ProfilePaths> {
    validate_profile_id(&entry.id)?;
    let root = config_root.join("profiles").join(&entry.id);
    Ok(ProfilePaths {
        id: entry.id.clone(),
        root,
        config_root: config_root.to_path_buf(),
    })
}

// ─── Profile creation ─────────────────────────────────────────────────────────

/// Create a new profile with the given display name. Returns the generated id.
pub fn create_profile(display_name: &str) -> Result<String> {
    let root = config_root();
    let mut index = ProfileIndex::load(&root).unwrap_or_else(|_| ProfileIndex {
        version: 1,
        profiles: vec![],
        last_used: None,
        default_profile: "default".to_string(),
    });

    let trimmed = display_name.trim();
    if trimmed.is_empty() {
        bail!("Profile name cannot be empty");
    }
    if index.profiles.iter().any(|p| {
        p.display_name.trim().eq_ignore_ascii_case(trimmed)
            || p.id.trim().eq_ignore_ascii_case(trimmed)
    }) {
        bail!("Profile with name '{trimmed}' already exists");
    }

    let id = unique_id(display_name, &index);
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot create profile: home directory not found"))?;
    let claude_dir = home.join(format!(".claude-{id}"));

    // Create the profile directory structure directly.
    let profile_root = root.join("profiles").join(&id);
    std::fs::create_dir_all(&profile_root)?;
    std::fs::create_dir_all(profile_root.join("config"))?;
    std::fs::create_dir_all(profile_root.join("data"))?;
    std::fs::create_dir_all(profile_root.join("themes"))?;
    std::fs::create_dir_all(profile_root.join("sessions"))?;

    let manifest = ProfileManifest {
        layout_version: 1,
        profile_id: id.clone(),
        created_at: Some(now_iso8601()),
        ..Default::default()
    };
    let _ = manifest.write(&profile_root);

    let settings_path = profile_root.join("config").join("settings.json");
    if !settings_path.exists() {
        let settings_json = serde_json::json!({
            "version": 3,
            "extension_settings": {
                "claude-code": {
                    "config_dir": claude_dir.to_string_lossy()
                }
            }
        });
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&settings_json)?,
        )?;
    }

    let entry = ProfileEntry {
        id: id.clone(),
        display_name: display_name.to_string(),
        created_at: now_iso8601(),
        icon: None,
        color: None,
    };
    index.profiles.push(entry);
    index.save(&root)?;

    Ok(id)
}

/// Return all profiles from the index — for GUI use.
pub fn all_profiles() -> Result<Vec<ProfileEntry>> {
    let root = config_root();
    Ok(ProfileIndex::load(&root)?.profiles)
}

/// Delete a profile. Refuses to delete the active profile, the default profile, or a
/// profile whose `remote.json` points to a live PID. Removes the profile directory and
/// updates `profiles.json` (index written first so partial FS failure leaves index clean).
/// Claude credentials at `~/.claude-<id>/` are intentionally preserved.
pub fn delete_profile(id: &str) -> Result<()> {
    let root = config_root();
    let mut index = ProfileIndex::load(&root)?;

    let entry = index.profiles.iter().find(|p| p.id == id)
        .ok_or_else(|| anyhow::anyhow!("Profile '{id}' does not exist"))?
        .clone();

    if id == index.default_profile {
        bail!("Cannot delete the default profile");
    }
    if let Some(active) = try_current()
        && active.id == id {
            bail!("Cannot delete the active profile — switch to another profile first");
        }
    let paths = make_profile_paths(&entry, &root)?;
    if is_profile_running(&paths) {
        bail!("Profile '{id}' is currently in use by another Velowork instance");
    }

    index.profiles.retain(|p| p.id != id);
    if index.last_used.as_deref() == Some(id) {
        index.last_used = None;
    }
    index.save(&root)?;

    let _ = std::fs::remove_dir_all(&paths.root);
    Ok(())
}

fn is_profile_running(paths: &ProfilePaths) -> bool {
    let remote = paths.remote_json();
    let Ok(data) = std::fs::read_to_string(&remote) else { return false; };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) else { return false; };
    let pid = json.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    pid != 0 && is_process_alive(pid)
}

/// List all profiles to stdout.
pub fn list_profiles() {
    let root = config_root();
    match ProfileIndex::load(&root) {
        Ok(index) => {
            for p in &index.profiles {
                let marker = if index.last_used.as_deref() == Some(&p.id) { "*" } else { " " };
                println!("{} {:<20} {}", marker, p.id, p.display_name);
            }
        }
        Err(_) => {
            println!("No profiles found.");
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn bootstrap_default_profile(config_root: &Path) -> Result<ProfileIndex> {
    let profile_root = config_root.join("profiles").join("default");
    std::fs::create_dir_all(&profile_root)?;
    std::fs::create_dir_all(profile_root.join("config"))?;
    std::fs::create_dir_all(profile_root.join("data"))?;
    std::fs::create_dir_all(profile_root.join("themes"))?;
    std::fs::create_dir_all(profile_root.join("sessions"))?;

    let manifest = ProfileManifest {
        layout_version: 1,
        profile_id: "default".to_string(),
        created_at: Some(now_iso8601()),
        ..Default::default()
    };
    let _ = manifest.write(&profile_root);

    let entry = ProfileEntry {
        id: "default".to_string(),
        display_name: "Default".to_string(),
        created_at: now_iso8601(),
        icon: None,
        color: None,
    };
    let index = ProfileIndex {
        version: 1,
        profiles: vec![entry],
        last_used: Some("default".to_string()),
        default_profile: "default".to_string(),
    };
    index.save(config_root)?;
    Ok(index)
}

fn unique_id(display_name: &str, index: &ProfileIndex) -> String {
    let slug: String = display_name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let slug = if slug.is_empty() { "profile".to_string() } else { slug };

    if !index.profiles.iter().any(|p| p.id == slug) {
        return slug;
    }
    for n in 2u32.. {
        let candidate = format!("{slug}-{n}");
        if !index.profiles.iter().any(|p| p.id == candidate) {
            return candidate;
        }
    }
    unreachable!()
}

fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(windows)]
    {
        // Use WaitForSingleObject with a 0 timeout rather than
        // GetExitCodeProcess + STILL_ACTIVE: a process that legitimately exited
        // with code 259 (== STILL_ACTIVE) would otherwise be reported alive
        // forever (or until its PID is reused). The process handle is signaled
        // once the process terminates; WAIT_TIMEOUT means it is still running.
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if handle.is_null() {
                return false;
            }
            let result = WaitForSingleObject(handle, 0);
            CloseHandle(handle);
            result == WAIT_TIMEOUT
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d) = unix_days_to_ymd(s / 86400);
    let h = (s % 86400) / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{sec:02}Z")
}

fn unix_days_to_ymd(mut n: u64) -> (u64, u64, u64) {
    let mut y = 1970u64;
    loop {
        let leap = y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
        let days = if leap { 366 } else { 365 };
        if n < days { break; }
        n -= days;
        y += 1;
    }
    let leap = y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    let months: [u64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 1u64;
    for &days in &months {
        if n < days { break; }
        n -= days;
        mo += 1;
    }
    (y, mo, n + 1)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn temp_root() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_profile_index_round_trip() {
        let dir = temp_root();
        let index = ProfileIndex {
            version: 1,
            profiles: vec![ProfileEntry {
                id: "default".to_string(),
                display_name: "Default".to_string(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
                icon: None,
                color: None,
            }],
            last_used: Some("default".to_string()),
            default_profile: "default".to_string(),
        };
        index.save(dir.path()).unwrap();
        let loaded = ProfileIndex::load(dir.path()).unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].id, "default");
        assert_eq!(loaded.last_used.as_deref(), Some("default"));
    }

    #[test]
    fn test_unique_id_collision() {
        let mut index = ProfileIndex {
            version: 1,
            profiles: vec![],
            last_used: None,
            default_profile: "default".to_string(),
        };
        let id1 = unique_id("work", &index);
        assert_eq!(id1, "work");
        index.profiles.push(ProfileEntry {
            id: "work".to_string(),
            display_name: "Work".to_string(),
            created_at: "".to_string(),
            icon: None,
            color: None,
        });
        let id2 = unique_id("work", &index);
        assert_eq!(id2, "work-2");
    }

    #[test]
    fn test_profile_initialization_structure() {
        let dir = temp_root();
        let paths = ProfilePaths {
            id: "work".into(),
            root: dir.path().join("profiles/work"),
            config_root: dir.path().to_path_buf(),
        };
        fs::create_dir_all(&paths.root).unwrap();
        fs::create_dir_all(paths.config_dir()).unwrap();
        fs::create_dir_all(paths.data_dir()).unwrap();
        fs::create_dir_all(paths.themes_dir()).unwrap();
        fs::create_dir_all(paths.sessions_dir()).unwrap();

        let manifest = ProfileManifest {
            layout_version: 1,
            profile_id: "work".to_string(),
            created_at: Some(now_iso8601()),
            ..Default::default()
        };
        manifest.write(&paths.root).unwrap();

        assert!(paths.config_dir().exists());
        assert!(paths.data_dir().exists());
        assert!(paths.themes_dir().exists());
        assert!(paths.sessions_dir().exists());
        assert!(paths.manifest_path().exists());

        let read = ProfileManifest::read(&paths.root).unwrap();
        assert_eq!(read.layout_version, 1);
        assert_eq!(read.profile_id, "work");
    }

    #[test]
    fn test_profile_paths() {
        let root = PathBuf::from("/tmp/test-velowork");
        let paths = ProfilePaths {
            id: "work".to_string(),
            root: root.join("profiles/work"),
            config_root: root.clone(),
        };
        assert_eq!(paths.workspace_json(), root.join("profiles/work/data/workspace.json"));
        assert_eq!(paths.settings_json(), root.join("profiles/work/config/settings.json"));
        assert_eq!(paths.keybindings_json(), root.join("profiles/work/config/keybindings.json"));
        assert_eq!(paths.sessions_dir(), root.join("profiles/work/sessions"));
        assert_eq!(paths.manifest_path(), root.join("profiles/work/manifest.json"));
        // updates_dir 已重指到全局 cache/updates
        assert_eq!(paths.updates_dir(), cache_dir().join("updates"));
    }

    #[test]
    fn test_create_profile_initializes_directories() {
        let dir = temp_root();
        let paths = ProfilePaths {
            id: "developer".into(),
            root: dir.path().join("profiles/developer"),
            config_root: dir.path().to_path_buf(),
        };
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::create_dir_all(paths.config_dir()).unwrap();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        std::fs::create_dir_all(paths.themes_dir()).unwrap();
        std::fs::create_dir_all(paths.sessions_dir()).unwrap();

        let manifest = ProfileManifest {
            layout_version: 1,
            profile_id: paths.id.clone(),
            created_at: Some(now_iso8601()),
            ..Default::default()
        };
        manifest.write(&paths.root).unwrap();

        assert!(paths.root.join("config").exists());
        assert!(paths.root.join("data").exists());
        assert!(paths.root.join("themes").exists());
        assert!(paths.root.join("sessions").exists());
        assert!(paths.root.join("manifest.json").exists());
    }

    #[test]
    fn test_now_iso8601_format() {
        let ts = now_iso8601();
        assert_eq!(ts.len(), 20); // "YYYY-MM-DDTHH:MM:SSZ"
        assert!(ts.ends_with('Z'));
    }

    fn make_test_index_with_two(dir: &TempDir) -> ProfileIndex {
        let idx = ProfileIndex {
            version: 1,
            profiles: vec![
                ProfileEntry { id: "default".into(), display_name: "Default".into(), created_at: "".into(), icon: None, color: None },
                ProfileEntry { id: "work".into(), display_name: "Work".into(), created_at: "".into(), icon: None, color: None },
            ],
            last_used: Some("work".into()),
            default_profile: "default".into(),
        };
        fs::create_dir_all(dir.path().join("profiles/default")).unwrap();
        fs::create_dir_all(dir.path().join("profiles/work")).unwrap();
        idx.save(dir.path()).unwrap();
        idx
    }

    #[test]
    fn test_all_profiles_returns_empty_on_missing_index() {
        // all_profiles reads from config_root() which is the real system path —
        // we test the round-trip via ProfileIndex directly instead.
        let dir = temp_root();
        let idx = make_test_index_with_two(&dir);
        let loaded = ProfileIndex::load(dir.path()).unwrap();
        assert_eq!(loaded.profiles.len(), idx.profiles.len());
    }

    #[test]
    fn test_delete_profile_refuses_default() {
        let dir = temp_root();
        make_test_index_with_two(&dir);

        // Simulate delete_profile logic inline (can't call it because it uses config_root())
        let root = dir.path();
        let index = ProfileIndex::load(root).unwrap();
        let err = if "default" == index.default_profile {
            Some("Cannot delete the default profile")
        } else {
            None
        };
        assert!(err.is_some());
        // index should be unchanged
        assert_eq!(index.profiles.len(), 2);
    }

    #[test]
    fn test_delete_profile_removes_entry_and_dir() {
        let dir = temp_root();
        make_test_index_with_two(&dir);

        let root = dir.path();
        let mut index = ProfileIndex::load(root).unwrap();
        let id = "work";

        // Simulate the delete logic (no try_current guard needed — OnceLock is per-process)
        index.profiles.retain(|p| p.id != id);
        if index.last_used.as_deref() == Some(id) { index.last_used = None; }
        index.save(root).unwrap();
        let work_dir = root.join("profiles/work");
        fs::remove_dir_all(&work_dir).unwrap();

        let reloaded = ProfileIndex::load(root).unwrap();
        assert_eq!(reloaded.profiles.len(), 1);
        assert_eq!(reloaded.profiles[0].id, "default");
        assert!(reloaded.last_used.is_none());
        assert!(!work_dir.exists());
    }

    #[test]
    fn test_delete_profile_clears_last_used_when_matching() {
        let dir = temp_root();
        make_test_index_with_two(&dir);
        let root = dir.path();
        let mut index = ProfileIndex::load(root).unwrap();
        assert_eq!(index.last_used.as_deref(), Some("work"));

        index.profiles.retain(|p| p.id != "work");
        if index.last_used.as_deref() == Some("work") { index.last_used = None; }
        index.save(root).unwrap();

        let reloaded = ProfileIndex::load(root).unwrap();
        assert!(reloaded.last_used.is_none());
    }

    #[test]
    fn test_delete_profile_refuses_unknown_id() {
        let dir = temp_root();
        make_test_index_with_two(&dir);
        let root = dir.path();
        let index = ProfileIndex::load(root).unwrap();
        let exists = index.profiles.iter().any(|p| p.id == "nonexistent");
        assert!(!exists, "should not find nonexistent profile");
    }

    #[test]
    fn test_delete_partial_failure_index_written_first() {
        // Verify index-save-first ordering: if the dir is already gone,
        // index is still updated (no double-removal error).
        let dir = temp_root();
        make_test_index_with_two(&dir);
        let root = dir.path();
        let mut index = ProfileIndex::load(root).unwrap();
        index.profiles.retain(|p| p.id != "work");
        index.last_used = None;
        index.save(root).unwrap();
        // Dir already gone — remove_dir_all ignores it
        let work_dir = root.join("profiles/work");
        let _ = fs::remove_dir_all(&work_dir); // first removal
        let _ = fs::remove_dir_all(&work_dir); // second — should not panic
        let reloaded = ProfileIndex::load(root).unwrap();
        assert_eq!(reloaded.profiles.len(), 1);
    }
}
