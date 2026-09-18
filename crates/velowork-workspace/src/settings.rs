pub use velowork_core::theme::{ColorTheme, UiDensity};
pub use velowork_i18n::Locale;
use velowork_terminal::session_backend::SessionBackend;
use velowork_terminal::shell_config::ShellType;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub use velowork_core::types::CursorShape;

fn default_true() -> bool {
    true
}

/// Default palette for the light appearance (`ColorTheme::Light`).
fn default_light_color_theme() -> ColorTheme {
    ColorTheme::Light
}

/// Native desktop notifications for background-terminal activity.
///
/// Opt-in: the whole feature is off until `enabled` is turned on. The
/// per-source flags then choose which events raise a notification. None of
/// these fire for the pane the user is actively looking at (the focused pane
/// in a foreground window).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotificationSettings {
    /// Master switch. Off by default — the feature is opt-in.
    #[serde(default)]
    pub enabled: bool,
    /// Raise a notification for `OSC 9` / `OSC 777` terminal alerts
    /// (e.g. `printf '\033]9;done\007'`, Claude Code's "waiting for input").
    #[serde(default = "default_true")]
    pub osc: bool,
    /// Raise a notification when a background terminal rings the bell (BEL).
    #[serde(default = "default_true")]
    pub bell: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            osc: true,
            bell: true,
        }
    }
}

/// Window state for a detached overlay (windowed / maximized / fullscreen).
/// The bounds in `DetachedWindowBounds` are the *restore* bounds — what the
/// window snaps back to when leaving maximized or fullscreen mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetachedWindowState {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
}

/// Last-used bounds of a detached overlay window. Persisted so the window
/// reopens at the same position, size, and state (incl. maximized/fullscreen)
/// instead of resetting to a small default each time.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct DetachedWindowBounds {
    pub origin_x: f32,
    pub origin_y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub state: DetachedWindowState,
}

/// Default sidebar width in pixels.
pub const DEFAULT_SIDEBAR_WIDTH: f32 = 280.0;
/// Minimum sidebar width in pixels.
pub const MIN_SIDEBAR_WIDTH: f32 = crate::dock_controller::SIDEBAR_CONSTRAINTS.min;
/// Maximum sidebar width in pixels.
pub const MAX_SIDEBAR_WIDTH: f32 = crate::dock_controller::SIDEBAR_CONSTRAINTS.max;

fn default_sidebar_width() -> f32 {
    DEFAULT_SIDEBAR_WIDTH
}

// ============================================================
// New types for Settings Dialog (Phase 1)
// ============================================================

/// Proxy mode for network connections
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    #[default]
    None,
    System,
    Http,
}

impl ProxyMode {
    pub fn display_name(self) -> &'static str {
        match self {
            ProxyMode::None => "No Proxy",
            ProxyMode::System => "System Proxy",
            ProxyMode::Http => "HTTP Proxy",
        }
    }

    pub fn translation_key(self) -> &'static str {
        match self {
            ProxyMode::None => "settings.proxy_mode.none",
            ProxyMode::System => "settings.proxy_mode.system",
            ProxyMode::Http => "settings.proxy_mode.http",
        }
    }

    pub fn all_variants() -> &'static [ProxyMode] {
        &[ProxyMode::None, ProxyMode::System, ProxyMode::Http]
    }
}

/// Close window behavior
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloseBehavior {
    #[serde(alias = "minimizetotray", alias = "closetotray")]
    Minimize,
    #[default]
    #[serde(alias = "quit")]
    Exit,
}

impl CloseBehavior {
    pub fn display_name(self) -> &'static str {
        match self {
            CloseBehavior::Minimize => "Minimize Window",
            CloseBehavior::Exit => "Exit Application",
        }
    }

    pub fn translation_key(self) -> &'static str {
        match self {
            CloseBehavior::Minimize => "settings.close_behavior.minimize",
            CloseBehavior::Exit => "settings.close_behavior.exit",
        }
    }

    pub fn all_variants() -> &'static [CloseBehavior] {
        &[
            CloseBehavior::Minimize,
            CloseBehavior::Exit,
        ]
    }
}

/// Appearance mode (dark / light / follow system). Re-exported from
/// `velowork_core::theme` so the workspace settings module stays the single
/// source of truth for settings types while `velowork-theme` (which cannot
/// depend on this crate) can use the same type.
pub use velowork_core::theme::ColorSchema;


/// Title bar style option (Custom vs Native)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TitlebarStyle {
    #[default]
    Custom,
    Native,
}

impl TitlebarStyle {
    pub fn translation_key(self) -> &'static str {
        match self {
            TitlebarStyle::Custom => "settings.titlebar_style.custom",
            TitlebarStyle::Native => "settings.titlebar_style.native",
        }
    }

    pub fn all_variants() -> &'static [TitlebarStyle] {
        &[TitlebarStyle::Custom, TitlebarStyle::Native]
    }
}

/// 文本抗锯齿模式 (Text Antialiasing Mode)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAntialiasingMode {
    #[default]
    PlatformDefault,
    Subpixel,
    Grayscale,
}

impl TextAntialiasingMode {
    pub fn translation_key(self) -> &'static str {
        match self {
            TextAntialiasingMode::PlatformDefault => "settings.text_antialiasing.platform_default",
            TextAntialiasingMode::Subpixel => "settings.text_antialiasing.subpixel",
            TextAntialiasingMode::Grayscale => "settings.text_antialiasing.grayscale",
        }
    }

    pub fn all_variants() -> &'static [TextAntialiasingMode] {
        &[
            TextAntialiasingMode::PlatformDefault,
            TextAntialiasingMode::Subpixel,
            TextAntialiasingMode::Grayscale,
        ]
    }
}

/// Preset visual style for custom titlebar buttons
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomTitlebarPreset {
    #[default]
    Auto,
    MacOS,
    Windows11,
    LinuxCSD,
    KDEBreeze,
}

impl CustomTitlebarPreset {
    pub fn translation_key(self) -> &'static str {
        match self {
            CustomTitlebarPreset::Auto => "settings.titlebar_preset.auto",
            CustomTitlebarPreset::MacOS => "settings.titlebar_preset.macos",
            CustomTitlebarPreset::Windows11 => "settings.titlebar_preset.windows11",
            CustomTitlebarPreset::LinuxCSD => "settings.titlebar_preset.linux_csd",
            CustomTitlebarPreset::KDEBreeze => "settings.titlebar_preset.kde_breeze",
        }
    }

    pub fn all_variants() -> &'static [CustomTitlebarPreset] {
        &[
            CustomTitlebarPreset::Auto,
            CustomTitlebarPreset::MacOS,
            CustomTitlebarPreset::Windows11,
            CustomTitlebarPreset::LinuxCSD,
            CustomTitlebarPreset::KDEBreeze,
        ]
    }
}

/// Custom titlebar button position (Left vs Right)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomTitlebarPosition {
    #[default]
    Auto,
    Left,
    Right,
}

impl CustomTitlebarPosition {
    pub fn translation_key(self) -> &'static str {
        match self {
            CustomTitlebarPosition::Auto => "settings.titlebar_position.auto",
            CustomTitlebarPosition::Left => "settings.titlebar_position.left",
            CustomTitlebarPosition::Right => "settings.titlebar_position.right",
        }
    }

    pub fn all_variants() -> &'static [CustomTitlebarPosition] {
        &[
            CustomTitlebarPosition::Auto,
            CustomTitlebarPosition::Left,
            CustomTitlebarPosition::Right,
        ]
    }
}


/// Tab width mode
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabWidthMode {
    #[default]
    Compact,
    TitleLength,
    Equal,
}

impl TabWidthMode {
    pub fn display_name(self) -> &'static str {
        match self {
            TabWidthMode::Compact => "Compact",
            TabWidthMode::TitleLength => "Title Length",
            TabWidthMode::Equal => "Equal Width",
        }
    }

    pub fn translation_key(self) -> &'static str {
        match self {
            TabWidthMode::Compact => "settings.tab_width_mode.compact",
            TabWidthMode::TitleLength => "settings.tab_width_mode.title_length",
            TabWidthMode::Equal => "settings.tab_width_mode.equal",
        }
    }

    pub fn all_variants() -> &'static [TabWidthMode] {
        &[
            TabWidthMode::Compact,
            TabWidthMode::TitleLength,
            TabWidthMode::Equal,
        ]
    }
}

/// Terminal wrap mode
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WrapMode {
    #[default]
    NoWrap,
    WindowEdge,
}

impl WrapMode {
    pub fn display_name(self) -> &'static str {
        match self {
            WrapMode::NoWrap => "No Wrap",
            WrapMode::WindowEdge => "Window Edge",
        }
    }

    pub fn translation_key(self) -> &'static str {
        match self {
            WrapMode::NoWrap => "settings.wrap_mode.no_wrap",
            WrapMode::WindowEdge => "settings.wrap_mode.window_edge",
        }
    }

    pub fn all_variants() -> &'static [WrapMode] {
        &[WrapMode::NoWrap, WrapMode::WindowEdge]
    }
}

/// File sort order
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileSortBy {
    #[default]
    Name,
    Size,
    Date,
    Type,
}

impl FileSortBy {
    pub fn display_name(self) -> &'static str {
        match self {
            FileSortBy::Name => "Name",
            FileSortBy::Size => "Size",
            FileSortBy::Date => "Date Modified",
            FileSortBy::Type => "Type",
        }
    }

    pub fn translation_key(self) -> &'static str {
        match self {
            FileSortBy::Name => "settings.file_sort_by.name",
            FileSortBy::Size => "settings.file_sort_by.size",
            FileSortBy::Date => "settings.file_sort_by.date",
            FileSortBy::Type => "settings.file_sort_by.type",
        }
    }

    pub fn all_variants() -> &'static [FileSortBy] {
        &[
            FileSortBy::Name,
            FileSortBy::Size,
            FileSortBy::Date,
            FileSortBy::Type,
        ]
    }
}

/// Sync provider type
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncProvider {
    #[default]
    WebDav,
    S3,
}

/// 本地与云端配置冲突时的处理策略。
///
/// 双向同步采用「三方合并」：以「上次成功同步时的哈希」为基线。
/// - 仅本地改动 → 推送本地；仅云端改动 → 拉取云端；
/// - 双方都改（冲突）→ 按本策略裁决。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncConflictStrategy {
    /// 本地优先：冲突时以本地配置覆盖云端。
    LocalWins,
    /// 云端优先：冲突时以云端配置覆盖本地。
    RemoteWins,
    /// 较新优先：按修改时间，较新的一方覆盖较旧的一方（默认，最贴近单人多设备场景）。
    #[default]
    NewerWins,
}

impl SyncConflictStrategy {
    pub fn translation_key(self) -> &'static str {
        match self {
            SyncConflictStrategy::LocalWins => "settings.sync.conflict.local_wins",
            SyncConflictStrategy::RemoteWins => "settings.sync.conflict.remote_wins",
            SyncConflictStrategy::NewerWins => "settings.sync.conflict.newer_wins",
        }
    }

    pub fn all_variants() -> &'static [SyncConflictStrategy] {
        &[
            SyncConflictStrategy::LocalWins,
            SyncConflictStrategy::RemoteWins,
            SyncConflictStrategy::NewerWins,
        ]
    }
}

impl SyncProvider {
    pub fn display_name(self) -> &'static str {
        match self {
            SyncProvider::WebDav => "WebDAV",
            SyncProvider::S3 => "S3",
        }
    }

    pub fn translation_key(self) -> &'static str {
        match self {
            SyncProvider::WebDav => "settings.sync.provider.webdav",
            SyncProvider::S3 => "settings.sync.provider.s3",
        }
    }

    pub fn all_variants() -> &'static [SyncProvider] {
        &[SyncProvider::WebDav, SyncProvider::S3]
    }
}

/// Security settings for credential encryption
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecuritySettings {
    /// Whether sensitive data encryption is enabled
    #[serde(default)]
    pub encryption_enabled: bool,
    /// PBKDF2 iterations for key derivation
    #[serde(default = "default_pbkdf2_iterations")]
    pub pbkdf2_iterations: u32,
    /// Base64-encoded salt for password verification
    #[serde(default)]
    pub password_salt: Option<String>,
    /// Base64-encoded password verifier (encrypted test block)
    #[serde(default)]
    pub password_verifier: Option<String>,
    /// Password timeout in seconds (0 = never timeout)
    #[serde(default = "default_password_timeout")]
    pub password_timeout_secs: u32,
    /// Active security mode mirror: `"standard"` (DEK in keyring, no password)
    /// or `"enhanced"` (DEK encrypted by master password). Source of truth lives
    /// in the profile DB `security_config` table; this is a cached mirror for UI.
    #[serde(default = "default_security_mode")]
    pub security_mode: String,
    /// Whether a master password is configured (Enhanced mode). Mirror of the
    /// DB `security_config` presence; UI-only.
    #[serde(default)]
    pub master_password_set: bool,
}

fn default_security_mode() -> String {
    "standard".to_string()
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            encryption_enabled: false,
            pbkdf2_iterations: default_pbkdf2_iterations(),
            password_salt: None,
            password_verifier: None,
            password_timeout_secs: default_password_timeout(),
            security_mode: default_security_mode(),
            master_password_set: false,
        }
    }
}

fn default_pbkdf2_iterations() -> u32 {
    100_000
}

fn default_password_timeout() -> u32 {
    600 // 10 minutes
}

/// WebDAV configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebDavConfig {
    /// WebDAV server URL
    #[serde(default)]
    pub server_url: String,
    /// WebDAV username
    #[serde(default)]
    pub username: String,
    /// Whether a password has been stored (actual password in keychain)
    #[serde(default)]
    pub password_stored: bool,
    /// Remote path for sync files
    #[serde(default = "default_webdav_path")]
    pub remote_path: String,
}

impl Default for WebDavConfig {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            username: String::new(),
            password_stored: false,
            remote_path: default_webdav_path(),
        }
    }
}

fn default_webdav_path() -> String {
    "/velowork/".to_string()
}

fn default_s3_prefix() -> String {
    "velowork/".to_string()
}

fn default_s3_region() -> String {
    "us-east-1".to_string()
}

/// S3 兼容对象存储配置
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct S3Config {
    /// S3 服务端点 URL（如 https://s3.amazonaws.com 或 http://127.0.0.1:9000）
    #[serde(default)]
    pub endpoint: String,
    /// 存储桶名称
    #[serde(default)]
    pub bucket: String,
    /// 存储区域（默认 us-east-1）
    #[serde(default = "default_s3_region")]
    pub region: String,
    /// Access Key ID
    #[serde(default)]
    pub access_key_id: String,
    /// 标记 Secret Access Key 是否已安全持久化至系统密钥库
    #[serde(default)]
    pub secret_key_stored: bool,
    /// 远端存储前缀路径（默认 velowork/）
    #[serde(default = "default_s3_prefix")]
    pub prefix: String,
    /// 是否强制使用路径样式（Path-Style，MinIO/本地私有云存储推荐）
    #[serde(default)]
    pub path_style: bool,
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            bucket: String::new(),
            region: default_s3_region(),
            access_key_id: String::new(),
            secret_key_stored: false,
            prefix: default_s3_prefix(),
            path_style: false,
        }
    }
}

fn default_scope_true() -> bool {
    true
}

/// 用户自定义同步数据范围配置
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDataScope {
    /// SSH 会话与服务器节点
    #[serde(default = "default_scope_true")]
    pub sessions: bool,
    /// 端口转发与隧道
    #[serde(default = "default_scope_true")]
    pub tunnels: bool,
    /// 服务树
    #[serde(default = "default_scope_true")]
    pub services: bool,
    /// 快捷指令
    #[serde(default = "default_scope_true")]
    pub quick_commands: bool,
    /// AI 对话与消息记录
    #[serde(default = "default_scope_true")]
    pub ai_chat: bool,
    /// 终端命令历史
    #[serde(default = "default_scope_true")]
    pub command_history: bool,
    /// 应用偏好与全局配置
    #[serde(default = "default_scope_true")]
    pub settings: bool,
    /// 自定义主题
    #[serde(default = "default_scope_true")]
    pub themes: bool,
    /// 账号与连接凭据
    #[serde(default = "default_scope_true")]
    pub credentials: bool,
}

impl Default for SyncDataScope {
    fn default() -> Self {
        Self {
            sessions: true,
            tunnels: true,
            services: true,
            quick_commands: true,
            ai_chat: true,
            command_history: true,
            settings: true,
            themes: true,
            credentials: true,
        }
    }
}

/// Sync settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncSettings {
    /// Whether sync is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Sync provider type
    #[serde(default)]
    pub provider: SyncProvider,
    /// WebDAV configuration
    #[serde(default)]
    pub webdav: WebDavConfig,
    /// S3 configuration
    #[serde(default)]
    pub s3: S3Config,
    /// Whether to auto-sync on startup
    #[serde(default)]
    pub auto_sync: bool,
    /// Auto-sync interval in seconds
    #[serde(default = "default_sync_interval")]
    pub sync_interval_secs: u32,
    /// Last sync timestamp (ISO 8601)
    #[serde(default)]
    pub last_sync_at: Option<String>,
    /// 本地与云端配置冲突时的处理策略（保留兼容）
    #[serde(default)]
    pub conflict_strategy: SyncConflictStrategy,
    /// 同步数据范围（默认全部启用）
    #[serde(default)]
    pub data_scope: SyncDataScope,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: SyncProvider::default(),
            webdav: WebDavConfig::default(),
            s3: S3Config::default(),
            auto_sync: false,
            sync_interval_secs: default_sync_interval(),
            last_sync_at: None,
            conflict_strategy: SyncConflictStrategy::default(),
            data_scope: SyncDataScope::default(),
        }
    }
}

/// 同步配置校验失败错误（结构化领域错误）
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncConfigError {
    #[error("Sync is disabled")]
    Disabled,
    #[error("WebDAV server URL is empty")]
    MissingServerUrl,
    #[error("WebDAV server URL must start with http:// or https://")]
    InvalidServerUrl,
    #[error("S3 endpoint is empty")]
    MissingS3Endpoint,
    #[error("S3 endpoint must start with http:// or https://")]
    InvalidS3Endpoint,
    #[error("S3 bucket name is empty")]
    MissingS3Bucket,
    #[error("S3 access key is empty")]
    MissingS3AccessKey,
}

impl SyncConfigError {
    /// 对应的 i18n 国际化键名
    pub fn translation_key(&self) -> &'static str {
        match self {
            Self::Disabled => "settings.sync.error_sync_disabled",
            Self::MissingServerUrl => "settings.sync.error_server_url_empty",
            Self::InvalidServerUrl => "settings.sync.error_server_url_invalid",
            Self::MissingS3Endpoint => "settings.sync.s3.error_endpoint_empty",
            Self::InvalidS3Endpoint => "settings.sync.s3.error_endpoint_invalid",
            Self::MissingS3Bucket => "settings.sync.s3.error_bucket_empty",
            Self::MissingS3AccessKey => "settings.sync.s3.error_access_key_empty",
        }
    }
}

impl SyncSettings {
    /// 校验当前同步源所需的必填字段是否已配置。
    /// 兼容未来多种源（WebDAV, S3, Git 等）。
    pub fn validate_configuration(&self) -> Result<(), SyncConfigError> {
        if !self.enabled {
            return Err(SyncConfigError::Disabled);
        }
        match self.provider {
            SyncProvider::WebDav => {
                let url = self.webdav.server_url.trim();
                if url.is_empty() {
                    return Err(SyncConfigError::MissingServerUrl);
                }
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err(SyncConfigError::InvalidServerUrl);
                }
                Ok(())
            }
            SyncProvider::S3 => {
                let ep = self.s3.endpoint.trim();
                if ep.is_empty() {
                    return Err(SyncConfigError::MissingS3Endpoint);
                }
                if !ep.starts_with("http://") && !ep.starts_with("https://") {
                    return Err(SyncConfigError::InvalidS3Endpoint);
                }
                if self.s3.bucket.trim().is_empty() {
                    return Err(SyncConfigError::MissingS3Bucket);
                }
                if self.s3.access_key_id.trim().is_empty() {
                    return Err(SyncConfigError::MissingS3AccessKey);
                }
                Ok(())
            }
        }
    }
}

fn default_sync_interval() -> u32 {
    1800 // 30 minutes
}

/// File finder filter preferences.
///
/// Persisted default for the "Go to File" dialog's gitignore toggle. The
/// dialog initializes from this value and writes back to it when the user
/// toggles the filter, so the last-used state is also the default for
/// future opens.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileFinderSettings {
    /// Include files matched by .gitignore / git exclude rules.
    #[serde(default)]
    pub show_ignored: bool,
}

/// Sidebar settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SidebarSettings {
    /// Whether the sidebar is open
    #[serde(default)]
    pub is_open: bool,
    /// Whether auto-hide mode is enabled
    #[serde(default)]
    pub auto_hide: bool,
    /// Sidebar width in pixels
    #[serde(default = "default_sidebar_width")]
    pub width: f32,
}

impl Default for SidebarSettings {
    fn default() -> Self {
        Self {
            is_open: true,
            auto_hide: false,
            width: DEFAULT_SIDEBAR_WIDTH,
        }
    }
}

use crate::quick_commands::{qc_sort_siblings, QuickCommandNode};

/// Current settings schema version - increment when making breaking changes
pub const SETTINGS_VERSION: u32 = 16;

/// App settings (persisted separately from workspace)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSettings {
    /// Settings schema version for migration support
    #[serde(default = "default_settings_version")]
    pub version: u32,
    /// UI language locale
    #[serde(default)]
    pub locale: Locale,
    /// Color theme (palette) used when the active appearance is dark.
    #[serde(default)]
    pub dark_color_theme: ColorTheme,
    /// Color theme (palette) used when the active appearance is light.
    #[serde(default = "default_light_color_theme")]
    pub light_color_theme: ColorTheme,
    /// Custom theme file stem (e.g. "example-theme" for themes/example-theme.json).
    /// Only used when the active `dark_color_theme` / `light_color_theme` is `Custom`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_theme_id: Option<String>,
    /// Name of the currently active session (None = default workspace.json)
    #[serde(default)]
    pub active_session: Option<String>,
    /// Sidebar settings
    #[serde(default)]
    pub sidebar: SidebarSettings,
    /// Right sidebar settings
    #[serde(default)]
    pub right_sidebar: SidebarSettings,
    /// Whether the right vertical icon toolbar is open/visible
    #[serde(default = "default_true")]
    pub right_toolbar_open: bool,
    /// Whether to show border around focused terminal
    #[serde(default = "default_show_focused_border")]
    pub show_focused_border: bool,
    /// Titlebar style (Custom or Native)
    #[serde(default)]
    pub titlebar_style: TitlebarStyle,
    /// Titlebar preset style (Auto, MacOS, Windows11, LinuxCSD, KDEBreeze)
    #[serde(default)]
    pub titlebar_preset: CustomTitlebarPreset,
    /// Titlebar button position (Auto, Left, Right)
    #[serde(default)]
    pub titlebar_position: CustomTitlebarPosition,
    /// Custom titlebar height in px (default: 32.0)
    #[serde(default = "default_titlebar_height")]
    pub titlebar_height: f32,
    /// Gap between window control buttons in px (default: 4.0)
    #[serde(default = "default_window_control_button_gap")]
    pub window_control_button_gap: f32,
    /// Window control area margin in px (default: 8.0)
    #[serde(default = "default_window_control_margin")]
    pub window_control_margin: f32,
    /// Window corner radius in custom titlebar mode (default: 8.0)
    #[serde(default = "default_window_corner_radius")]
    pub window_corner_radius: f32,
    /// Window control icon size in custom titlebar mode (default: 18.0)
    #[serde(default = "default_window_control_icon_size")]
    pub window_control_icon_size: f32,
    /// Tint project backgrounds with the folder color
    #[serde(default)]
    pub color_tinted_background: bool,
    /// Whether UI animations and micro-transitions are enabled
    #[serde(default = "default_true")]
    pub enable_animations: bool,

    // Font settings
    /// Terminal font size (default: 14.0)
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// Terminal font family (default: "JetBrains Mono")
    #[serde(default = "default_font_family")]
    pub font_family: String,
    /// Terminal base font weight (default: "Normal"). Allowed: Normal/Medium/Bold.
    #[serde(default = "default_font_weight")]
    pub font_weight: String,
    /// Terminal base font style (default: "Normal"). Allowed: Normal/Italic.
    #[serde(default = "default_font_style")]
    pub font_style: String,
    /// Line height multiplier (default: 1.3)
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    /// UI font size for panels/dialogs (default: 13.0)
    #[serde(default = "default_ui_font_size")]
    pub ui_font_size: f32,
    /// Global UI zoom as a percentage (default: 100, range 80..=200).
    /// Scales the *entire* UI (text and spacing) together.
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    /// Interface compactness level (Compact / Default / Comfortable)
    #[serde(default)]
    pub ui_density: UiDensity,
    /// File viewer/diff viewer font size (default: 12.0)
    #[serde(default = "default_file_font_size")]
    pub file_font_size: f32,
    /// Monospace font family for code snippets, logs and the file/diff viewer.
    /// Empty string = the bundled default monospace (JetBrains Mono).
    #[serde(default = "default_mono_font_family")]
    pub mono_font_family: String,
    /// Reading font family for Markdown documents and rendered AI text.
    /// Empty string = System Default.
    #[serde(default = "default_markdown_font_family")]
    pub markdown_font_family: String,
    /// 文本抗锯齿渲染模式（系统默认 / ClearType 次像素 / 灰度抗锯齿）
    #[serde(default)]
    pub text_antialiasing: TextAntialiasingMode,

    // Terminal settings
    /// Cursor shape: Block, Bar, or Underline (default: Bar)
    #[serde(default)]
    pub cursor_style: CursorShape,
    /// Enable cursor blinking (default: true)
    #[serde(default = "default_cursor_blink")]
    pub cursor_blink: bool,
    /// Terminal Bell 提示方式 (Visual, Audible, Both, Disabled) (default: Visual)
    #[serde(default)]
    pub bell_style: velowork_core::types::BellStyle,
    /// Terminal Bell 节流冷却时间（单位毫秒，0 表示不节流，默认 500）
    #[serde(default = "default_bell_cooldown_ms")]
    pub bell_cooldown_ms: u32,
    /// Number of scrollback lines (default: 10000)
    #[serde(default = "default_scrollback_lines")]
    pub scrollback_lines: u32,
    /// Grace period, in seconds, before a *busy* terminal (one with a running
    /// foreground process) is actually killed when closed. During this window
    /// the pane is removed but the PTY keeps running and a toast offers "Undo".
    /// `0` disables the feature entirely (close kills immediately, as before).
    #[serde(default = "default_terminal_close_grace_secs")]
    pub terminal_close_grace_secs: u32,
    /// Whether to prompt for confirmation when closing a terminal tab (default: true)
    #[serde(default = "default_true")]
    pub confirm_close_tab: bool,
    /// Terminal background image path or URL. When set, the terminal content
    /// background becomes transparent so the image shows through behind the text.
    /// Empty / unset means no background image (default terminal background color).
    #[serde(default)]
    pub terminal_background_image: Option<String>,
    /// Apply a blur to the terminal background image to improve text readability.
    #[serde(default)]
    pub terminal_background_image_blur: bool,
    /// Terminal scrollbar display mode (Scrolling, Hover, Always, Never)
    #[serde(default)]
    pub terminal_scrollbar_show: velowork_core::types::ScrollbarShow,
    /// Delimiters used when double clicking to select words in terminal
    #[serde(default = "default_word_selection_delimiters")]
    pub word_selection_delimiters: String,
    /// Automatically copy selected text to clipboard on selection end
    #[serde(default)]
    pub terminal_copy_on_select: bool,
    /// Right-click in terminal pastes clipboard content directly (Shift+Right-click opens context menu)
    #[serde(default)]
    pub terminal_right_click_paste: bool,

    // Shell settings
    /// Default shell type for new terminals
    #[serde(default)]
    pub default_shell: ShellType,
    /// Show shell selector in terminal header (default: false)
    #[serde(default)]
    pub show_shell_selector: bool,

    // Session persistence settings
    /// Session backend for terminal persistence (tmux/screen/none/auto)
    #[serde(default)]
    pub session_backend: SessionBackend,
    /// Whether to reconnect/reopen previously-open terminal sessions on app
    /// startup. When `false` (default), terminal panes start with a fresh
    /// shell instead of reattaching to the session that was active before
    /// the app was closed (i.e. previously-unclosed sessions are not reopened).
    #[serde(default = "default_restore_terminals_on_startup")]
    pub restore_terminals_on_startup: bool,

    // File opener settings
    /// Editor command to open file paths (e.g. "code", "cursor", "zed", "subl", "vim")
    /// Empty string = use system default (open/xdg-open/start)
    #[serde(default = "default_file_opener")]
    pub file_opener: String,

    /// Minimum project column width in pixels (default: 400)
    #[serde(default = "default_min_column_width")]
    pub min_column_width: f32,

    /// When true, detachable overlays open directly in a separate OS window instead of as a modal.
    #[serde(default)]
    pub detached_overlays_by_default: bool,

    /// Last bounds used by a detached overlay window. Restored on next open
    /// so the window doesn't reset to a small default each time.
    #[serde(default)]
    pub detached_overlay_bounds: Option<DetachedWindowBounds>,

    /// Status bar server resource monitor popup width in pixels (default: 600.0)
    #[serde(default = "default_monitor_popup_width")]
    pub monitor_popup_width: f32,
    /// Status bar server resource monitor popup height in pixels (default: 520.0)
    #[serde(default = "default_monitor_popup_height")]
    pub monitor_popup_height: f32,

    /// Set of enabled extension IDs (replaces per-extension bool flags).
    #[serde(default)]
    pub enabled_extensions: HashSet<String>,

    /// Per-extension settings (keyed by extension ID).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extension_settings: HashMap<String, serde_json::Value>,

    /// Idle timeout in seconds for "waiting for input" detection (default: 5, 0 = disabled)
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u32,

    /// When true, Ctrl+C in a terminal pane copies the active selection (and clears it)
    /// instead of sending SIGINT. Ctrl+C without a selection still sends SIGINT.
    /// Ctrl+Shift+C continues to copy unconditionally regardless of this setting.
    /// Default: false (Ctrl+C always sends SIGINT — matches GNOME Terminal / Kitty).
    #[serde(default)]
    pub terminal_ctrl_c_copies_selection: bool,

    /// File finder filter preferences. The "Go to File" dialog reads these
    /// when opened and writes them back when the user toggles a filter, so
    /// the last-used state is also the default for future opens.
    #[serde(default)]
    pub file_finder: FileFinderSettings,

    /// Native desktop notifications for background-terminal activity
    /// (OSC 9/777 alerts and the bell). Opt-in — see [`NotificationSettings`].
    #[serde(default)]
    pub notifications: NotificationSettings,

    /// When true, line numbers are shown in a gutter on the left side of each
    /// terminal pane.
    #[serde(default)]
    pub show_line_numbers: bool,

    // ============================================================
    // New fields for Settings Dialog (Phase 1)
    // ============================================================

    // General settings (new)
    /// Whether to start application on system boot
    #[serde(default)]
    pub start_on_boot: bool,
    /// Whether to automatically check for updates periodically (default: true)
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    /// Close window behavior
    #[serde(default)]
    pub close_behavior: CloseBehavior,
    /// Proxy mode
    #[serde(default)]
    pub proxy_mode: ProxyMode,
    /// HTTP proxy host
    #[serde(default)]
    pub proxy_host: String,
    /// HTTP proxy port
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,

    // Appearance settings (new)
    /// Color theme selection
    #[serde(default)]
    pub color_schema: ColorSchema,
    /// Background opacity (0.0 - 1.0)
    #[serde(default = "default_bg_opacity")]
    pub bg_opacity: f32,
    /// Tab width mode
    #[serde(default)]
    pub tab_width_mode: TabWidthMode,
    /// Enable live terminal content preview on tab/capsule hover
    #[serde(default = "default_true")]
    pub enable_tab_preview: bool,

    // Font settings (new)
    /// UI font family
    #[serde(default = "default_ui_font_family")]
    pub ui_font_family: String,

    // Terminal settings (new)
    /// Terminal color scheme name
    #[serde(default = "default_color_scheme")]
    pub color_scheme: String,
    /// Custom terminal color schemes
    #[serde(default)]
    pub custom_terminal_color_schemes: Vec<velowork_core::theme::CustomTerminalColorScheme>,
    /// Terminal charset encoding
    #[serde(default = "default_charset")]
    pub charset: String,
    /// Terminal type (TERM environment variable)
    #[serde(default = "default_term_type")]
    pub term_type: String,
    /// Terminal wrap mode
    #[serde(default)]
    pub wrap_mode: WrapMode,
    /// Enable shell integration (OSC 133 prompt markers and command status)
    #[serde(default = "default_true")]
    pub shell_integration: bool,
    /// Enable bracketed paste mode protection
    #[serde(default = "default_true")]
    pub bracketed_paste: bool,
    /// Enable OSC 52 remote clipboard integration
    #[serde(default)]
    pub osc52_clipboard: bool,
    /// Enable 24-bit TrueColor support
    #[serde(default = "default_true")]
    pub true_color: bool,

    // File manager settings (new)
    /// Whether to show hidden files in file manager
    #[serde(default)]
    pub show_hidden_files: bool,
    /// File sort order
    #[serde(default)]
    pub file_sort_by: FileSortBy,
    /// Whether to show alternating row backgrounds
    #[serde(default = "default_true")]
    pub alternating_row_bg: bool,
    /// Default permission (octal string, e.g. "0644") applied when creating a
    /// new file via the SFTP file list.
    #[serde(default = "default_sftp_file_mode")]
    pub sftp_default_file_mode: String,
    /// Default permission (octal string, e.g. "0755") applied when creating a
    /// new directory via the SFTP file list.
    #[serde(default = "default_sftp_dir_mode")]
    pub sftp_default_dir_mode: String,

    // Security settings
    #[serde(default)]
    pub security: SecuritySettings,

    // Sync settings
    #[serde(default)]
    pub sync: SyncSettings,

    /// Quick-command tree for the right-panel "快捷指令" feature.
    /// Each entry is either a folder (group) or a command (with optional
    /// `{{variable}}` placeholders). Backward-compatible: missing in old
    /// settings files, defaults to an empty tree.
    #[serde(default)]
    pub quick_commands: Vec<QuickCommandNode>,

    /// Per-project quick-command tree mapping (project_id -> tree).
    #[serde(default)]
    pub project_quick_commands: std::collections::HashMap<String, Vec<QuickCommandNode>>,

    // ============================================================
    // AI Assistant settings
    // ============================================================
    /// Whether the AI assistant feature is enabled globally
    #[serde(default = "default_true")]
    pub ai_enabled: bool,

    /// List of configured AI models (OpenAI-compatible providers)
    #[serde(default)]
    pub ai_models: Vec<AiModelConfig>,

    /// ID of the default AI model for new conversations
    #[serde(default)]
    pub ai_default_model_id: Option<String>,

    /// List of user-configured search engines for the terminal's "Search Online" action
    #[serde(default)]
    pub search_engines: Vec<SearchEngineConfig>,

    /// Default temperature for AI model requests (0.0 - 2.0)
    #[serde(default = "default_ai_temperature")]
    pub ai_temperature: f32,

    /// Default max tokens for AI model requests
    #[serde(default = "default_ai_max_tokens")]
    pub ai_max_tokens: u32,

    /// List of disabled AI skills
    #[serde(default)]
    pub ai_disabled_skills: Vec<String>,

    /// AI 消息上下文自动压缩策略
    #[serde(default)]
    pub ai_compression_strategy: AiCompressionStrategy,

    /// 自动压缩触发的 Token 阈值（默认 8192）
    #[serde(default = "default_ai_max_context_tokens")]
    pub ai_max_context_tokens: usize,

    /// 最多保留的历史消息条数（针对滑动窗口或最大记忆轮数，默认 20）
    #[serde(default = "default_ai_max_history_messages")]
    pub ai_max_history_messages: usize,

    /// 是否开启自动上下文压缩
    #[serde(default = "default_true")]
    pub ai_auto_compress: bool,

    /// 是否在终端划选文本后自动显示 AI 悬浮微工具条
    #[serde(default = "default_true")]
    pub terminal_ai_floating_toolbar_enabled: bool,

    /// 终端内就地呼出 AI 对话框快捷键（默认 ctrl-k）
    #[serde(default = "default_inline_ai_shortcut")]
    pub terminal_ai_inline_shortcut: String,

    // ============================================================
    // Command History settings
    // ============================================================
    /// Maximum number of command history records preserved per project (default: 1000)
    #[serde(default = "default_command_history_max_count")]
    pub command_history_max_count: usize,

    /// Retention period for command history in days (default: 30, 0 = unlimited)
    #[serde(default = "default_command_history_retention_days")]
    pub command_history_retention_days: u32,

    /// Whether terminal inline auto-completion / suggestions from history is enabled (default: true)
    #[serde(default = "default_true")]
    pub command_history_auto_completion: bool,

    /// Ignored trivial commands for command history (default: ["ls", "ll", "la", "l", "pwd", "clear", "cls", "exit", "history"])
    #[serde(default = "default_command_history_ignored_commands")]
    pub command_history_ignored_commands: Vec<String>,

    /// Ignore commands starting with a leading space (default: true)
    #[serde(default = "default_true")]
    pub command_history_ignore_space: bool,
}

impl AppSettings {
    pub fn is_ai_skill_enabled(&self, skill_name: &str) -> bool {
        !self.ai_disabled_skills.iter().any(|s| s == skill_name)
    }
    pub fn quick_commands_for_project(&self, project_id: Option<&str>) -> &[QuickCommandNode] {
        let pid = project_id.unwrap_or("default");
        if let Some(nodes) = self.project_quick_commands.get(pid) {
            nodes.as_slice()
        } else if !self.quick_commands.is_empty() {
            &self.quick_commands
        } else {
            &[]
        }
    }

    pub fn quick_commands_for_project_mut(
        &mut self,
        project_id: Option<&str>,
    ) -> &mut Vec<QuickCommandNode> {
        let pid = match project_id {
            Some(id) if !id.trim().is_empty() => id.trim().to_string(),
            _ => "default".to_string(),
        };
        if !self.project_quick_commands.contains_key(&pid) {
            if pid == "default" && !self.quick_commands.is_empty() {
                self.project_quick_commands
                    .insert(pid.clone(), std::mem::take(&mut self.quick_commands));
            } else {
                self.project_quick_commands.insert(pid.clone(), Vec::new());
            }
        }
        self.project_quick_commands
            .get_mut(&pid)
            .expect("inserted above")
    }

    /// Extract baseline terminal defaults with safe fallback for color scheme.
    pub fn terminal_defaults(&self) -> velowork_terminal::TerminalDefaults {
        let valid_color_scheme = if self.color_scheme.trim().is_empty() {
            "default".to_string()
        } else {
            self.color_scheme.clone()
        };

        velowork_terminal::TerminalDefaults {
            font_family: self.font_family.clone(),
            font_size: self.font_size,
            color_scheme: valid_color_scheme,
            cursor_shape: self.cursor_style,
            cursor_blink: self.cursor_blink,
            bell_style: self.bell_style,
            bell_cooldown_ms: self.bell_cooldown_ms,
            scrollback_lines: self.scrollback_lines,
            word_separators: self.word_selection_delimiters.clone(),
            charset: self.charset.clone(),
            term_type: self.term_type.clone(),
            shell_integration: self.shell_integration,
            bracketed_paste: self.bracketed_paste,
            osc52_clipboard: self.osc52_clipboard,
            true_color: self.true_color,
        }
    }
}

/// AI 消息上下文自动压缩策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiCompressionStrategy {
    /// 智能摘要：将早期多轮历史消息摘要压缩为 [历史摘要]，保留最新消息
    Summarize,
    /// 滑动窗口：仅保留最近 N 条消息，丢弃较早消息
    SlidingWindow,
    /// Token 截断：超出 Token 上限时按 Token 从旧到新删除
    TruncateOldest,
}

impl Default for AiCompressionStrategy {
    fn default() -> Self {
        Self::Summarize
    }
}

/// Configuration for a single AI model (OpenAI-compatible provider).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AiModelConfig {
    /// Unique identifier for this model config (UUID)
    pub id: String,
    /// Display name (e.g. "OpenAI", "Claude", "DeepSeek")
    pub name: String,
    /// Base URL for the API endpoint (e.g. "https://api.openai.com/v1")
    pub base_url: String,
    /// API key for authentication (stored securely in keyring/security service, not serialized in plain JSON)
    #[serde(default, skip_serializing)]
    pub api_key: String,
    /// Model ID to use in requests (e.g. "gpt-4o", "claude-3.5-sonnet")
    pub model_id: String,
    /// Whether this model is currently enabled
    pub enabled: bool,
    /// Optional description or notes
    #[serde(default)]
    pub description: String,
}

/// A user-configured search engine used by the terminal's "Search Online"
/// context-menu action. The `url` must contain a `%s` placeholder that is
/// replaced with the URL-encoded selected text when performing a search.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchEngineConfig {
    /// Unique identifier for this engine config (UUID)
    pub id: String,
    /// Display name (e.g. "Google", "Bing", "MDN")
    pub name: String,
    /// Search URL with a `%s` placeholder for the query term
    pub url: String,
    /// Optional keyword (e.g. `@mdn`) used to quickly identify the engine
    #[serde(default)]
    pub keyword: String,
    /// Whether this engine is currently enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl SearchEngineConfig {
    pub fn new(name: String, url: String, keyword: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            url,
            keyword,
            enabled: true,
        }
    }

    /// Returns the search URL with `%s` replaced by the (URL-encoded) query.
    pub fn build_url(&self, query: &str) -> String {
        let encoded = url_encode(query);
        self.url.replace("%s", &encoded)
    }
}

/// Percent-encodes a query string for use in a URL (RFC 3986), using `+` for
/// spaces (form-style). Mirrors what browsers send for `application/x-www-form-urlencoded`.
fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(char::from_digit((byte >> 4) as u32, 16).unwrap().to_ascii_uppercase());
                out.push(char::from_digit((byte & 0xF) as u32, 16).unwrap().to_ascii_uppercase());
            }
        }
    }
    out
}

impl AiModelConfig {
    pub fn new(name: String, base_url: String, api_key: String, model_id: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            base_url,
            api_key,
            model_id,
            enabled: true,
            description: String::new(),
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            locale: Locale::default(),
            custom_theme_id: None,
            dark_color_theme: ColorTheme::default(),
            light_color_theme: default_light_color_theme(),
            active_session: None,
            sidebar: SidebarSettings::default(),
            right_sidebar: SidebarSettings {
                is_open: false,
                ..Default::default()
            },
            right_toolbar_open: true,
            show_focused_border: default_show_focused_border(),
            titlebar_style: TitlebarStyle::default(),
            titlebar_preset: CustomTitlebarPreset::default(),
            titlebar_position: CustomTitlebarPosition::default(),
            titlebar_height: default_titlebar_height(),
            window_control_button_gap: default_window_control_button_gap(),
            window_control_margin: default_window_control_margin(),
            window_corner_radius: default_window_corner_radius(),
            window_control_icon_size: default_window_control_icon_size(),
            color_tinted_background: false,
            enable_animations: true,
            font_size: default_font_size(),
            font_family: default_font_family(),
            font_weight: default_font_weight(),
            font_style: default_font_style(),
            line_height: default_line_height(),
            ui_font_size: default_ui_font_size(),
            ui_scale: default_ui_scale(),
            ui_density: UiDensity::default(),
            file_font_size: default_file_font_size(),
            mono_font_family: default_mono_font_family(),
            markdown_font_family: default_markdown_font_family(),
            text_antialiasing: TextAntialiasingMode::default(),
            cursor_style: CursorShape::default(),
            cursor_blink: default_cursor_blink(),
            bell_style: velowork_core::types::BellStyle::default(),
            bell_cooldown_ms: default_bell_cooldown_ms(),
            scrollback_lines: default_scrollback_lines(),
            terminal_close_grace_secs: default_terminal_close_grace_secs(),
            confirm_close_tab: true,
            terminal_background_image: None,
            terminal_background_image_blur: false,
            terminal_scrollbar_show: velowork_core::types::ScrollbarShow::default(),
            word_selection_delimiters: default_word_selection_delimiters(),
            terminal_copy_on_select: false,
            terminal_right_click_paste: false,
            default_shell: ShellType::default(),
            show_shell_selector: false,
            session_backend: SessionBackend::default(),
            restore_terminals_on_startup: default_restore_terminals_on_startup(),
            file_opener: default_file_opener(),
            min_column_width: default_min_column_width(),
            detached_overlays_by_default: false,
            detached_overlay_bounds: None,
            monitor_popup_width: default_monitor_popup_width(),
            monitor_popup_height: default_monitor_popup_height(),
            enabled_extensions: HashSet::new(),
            extension_settings: HashMap::new(),
            idle_timeout_secs: default_idle_timeout_secs(),
            terminal_ctrl_c_copies_selection: false,
            file_finder: FileFinderSettings::default(),
            notifications: NotificationSettings::default(),
            show_line_numbers: false,
            // New fields
            start_on_boot: false,
            auto_check_updates: true,
            close_behavior: CloseBehavior::default(),
            proxy_mode: ProxyMode::default(),
            proxy_host: String::new(),
            proxy_port: default_proxy_port(),
            color_schema: ColorSchema::default(),
            bg_opacity: default_bg_opacity(),
            tab_width_mode: TabWidthMode::default(),
            enable_tab_preview: true,
            ui_font_family: default_ui_font_family(),
            color_scheme: default_color_scheme(),
            custom_terminal_color_schemes: Vec::new(),
            charset: default_charset(),
            term_type: default_term_type(),
            wrap_mode: WrapMode::default(),
            shell_integration: true,
            bracketed_paste: true,
            osc52_clipboard: false,
            true_color: true,
            show_hidden_files: false,
            file_sort_by: FileSortBy::default(),
            alternating_row_bg: true,
            sftp_default_file_mode: default_sftp_file_mode(),
            sftp_default_dir_mode: default_sftp_dir_mode(),
            security: SecuritySettings::default(),
            sync: SyncSettings::default(),
            quick_commands: Vec::new(),
            project_quick_commands: std::collections::HashMap::new(),
            ai_enabled: true,
            ai_models: Vec::new(),
            ai_default_model_id: None,
            search_engines: Vec::new(),
            ai_temperature: default_ai_temperature(),
            ai_max_tokens: default_ai_max_tokens(),
            ai_disabled_skills: Vec::new(),
            ai_compression_strategy: AiCompressionStrategy::default(),
            ai_max_context_tokens: default_ai_max_context_tokens(),
            ai_max_history_messages: default_ai_max_history_messages(),
            ai_auto_compress: true,
            terminal_ai_floating_toolbar_enabled: true,
            terminal_ai_inline_shortcut: default_inline_ai_shortcut(),
            command_history_max_count: default_command_history_max_count(),
            command_history_retention_days: default_command_history_retention_days(),
            command_history_auto_completion: true,
            command_history_ignored_commands: default_command_history_ignored_commands(),
            command_history_ignore_space: true,
        }
    }
}

pub fn default_command_history_max_count() -> usize {
    1000
}

pub fn default_command_history_retention_days() -> u32 {
    30
}

pub fn default_command_history_ignored_commands() -> Vec<String> {
    vec![
        "ls".to_string(),
        "ll".to_string(),
        "la".to_string(),
        "l".to_string(),
        "pwd".to_string(),
        "clear".to_string(),
        "cls".to_string(),
        "exit".to_string(),
        "history".to_string(),
    ]
}

fn default_settings_version() -> u32 {
    // Return 0 for settings files without version field (pre-versioning)
    0
}

fn default_show_focused_border() -> bool {
    false
}

fn default_window_corner_radius() -> f32 {
    8.0
}

fn default_titlebar_height() -> f32 {
    32.0
}

fn default_window_control_button_gap() -> f32 {
    4.0
}

fn default_window_control_margin() -> f32 {
    8.0
}

fn default_window_control_icon_size() -> f32 {
    18.0
}

fn default_font_size() -> f32 {
    14.0
}

fn default_font_family() -> String {
    "JetBrains Mono".to_string()
}

fn default_line_height() -> f32 {
    1.3
}

fn default_ui_font_size() -> f32 {
    13.0
}

fn default_file_font_size() -> f32 {
    12.0
}

fn default_ui_scale() -> f32 {
    100.0
}

fn default_mono_font_family() -> String {
    String::new()
}

fn default_markdown_font_family() -> String {
    String::new()
}

fn default_font_weight() -> String {
    "Normal".to_string()
}

fn default_font_style() -> String {
    "Normal".to_string()
}

fn default_cursor_blink() -> bool {
    true
}

fn default_bell_cooldown_ms() -> u32 {
    500
}

fn default_restore_terminals_on_startup() -> bool {
    // Default OFF: previously-open terminal sessions are NOT reopened on
    // app restart; panes start with a fresh shell instead.
    false
}

fn default_scrollback_lines() -> u32 {
    10000
}

fn default_word_selection_delimiters() -> String {
    r#"/,=+:;@#$&?%~|"'`(){}[]<>"#.to_string()
}

fn default_terminal_close_grace_secs() -> u32 {
    5
}

fn default_file_opener() -> String {
    String::new()
}

fn default_sftp_file_mode() -> String {
    "0644".to_string()
}

fn default_sftp_dir_mode() -> String {
    "0755".to_string()
}

fn default_min_column_width() -> f32 {
    400.0
}

fn default_idle_timeout_secs() -> u32 {
    0
}

pub fn default_monitor_popup_width() -> f32 {
    600.0
}

pub fn default_monitor_popup_height() -> f32 {
    520.0
}

// New default functions for Phase 1
fn default_proxy_port() -> u16 {
    1080
}

fn default_bg_opacity() -> f32 {
    1.0
}

fn default_ui_font_family() -> String {
    "System Default".to_string()
}

fn default_color_scheme() -> String {
    "Dark".to_string()
}

fn default_charset() -> String {
    "UTF-8".to_string()
}

fn default_term_type() -> String {
    velowork_core::DEFAULT_TERM_TYPE.to_string()
}

fn default_ai_temperature() -> f32 {
    0.7
}

fn default_ai_max_tokens() -> u32 {
    4096
}

fn default_ai_max_context_tokens() -> usize {
    8192
}

fn default_ai_max_history_messages() -> usize {
    20
}

fn default_inline_ai_shortcut() -> String {
    "ctrl-k".to_string()
}

/// Get the directory that holds the config files (`config/`).
///
/// Uses the active profile's `config_dir()` (`profiles/<id>/config`) so config
/// files land in the layered layout. Falls back to `<config_root>/config` before
/// a profile is initialized.
fn config_dir_for_settings() -> std::path::PathBuf {
    if let Some(p) = velowork_core::profiles::try_current() {
        p.config_dir()
    } else {
        velowork_core::profiles::config_root().join("config")
    }
}

/// Get the main settings file path (`config/settings.json`).
pub fn get_settings_path() -> std::path::PathBuf {
    config_dir_for_settings().join("settings.json")
}

/// Atomically write `value` as pretty JSON to `path` (tmp + fsync + rename,
/// 0o600 on unix). Mirrors the safety of the old `save_settings_locked`.
fn write_json_atomic(path: &std::path::Path, value: &serde_json::Value) -> Result<()> {
    velowork_core::atomic_io::write_json_atomic(path, value)
}

/// Sort all quick-command trees (default + per-project) so commands appear
/// before folders. Applied after loading/deserializing settings to guarantee the
/// canonical sibling ordering even for data persisted by older builds.
fn sort_quick_command_trees(settings: &mut AppSettings) {
    qc_sort_siblings(&mut settings.quick_commands);
    for tree in settings.project_quick_commands.values_mut() {
        qc_sort_siblings(tree);
    }
}

/// Load app settings from `config/settings.json` with robust error handling and migration support.
pub fn load_settings() -> AppSettings {
    let dir = config_dir_for_settings();
    let settings_path = dir.join("settings.json");
    log::info!("[settings] loading from {}", settings_path.display());

    let content = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!(
                "[settings] no config file found at {} ({}), using defaults",
                settings_path.display(),
                e
            );
            return AppSettings::default();
        }
    };

    // First, try direct deserialization (fast path for valid settings)
    match serde_json::from_str::<AppSettings>(&content) {
        Ok(mut settings) => {
            let old_version = settings.version;
            settings = migrate_settings(settings);
            sort_quick_command_trees(&mut settings);
            hydrate_and_migrate_ai_keys(&mut settings);
            if settings.version != old_version {
                log::info!(
                    "[settings] Migrated from v{} to v{}",
                    old_version,
                    settings.version
                );
                if let Err(e) = save_settings(&settings) {
                    log::warn!("[settings] Failed to save migrated settings | error: {:#}", e);
                }
            }
            return settings;
        }
        Err(e) => {
            log::warn!(
                "[settings] Failed to parse settings directly: {:#}, attempting partial recovery",
                e
            );
        }
    }

    // Fallback: partial recovery using serde_json::Value
    match recover_settings_from_json(&content) {
        Ok(mut settings) => {
            log::info!("[settings] Successfully recovered settings with partial data");
            settings = migrate_settings(settings);
            sort_quick_command_trees(&mut settings);
            hydrate_and_migrate_ai_keys(&mut settings);
            // Save the recovered settings to fix the file
            if let Err(e) = save_settings(&settings) {
                log::warn!("[settings] Failed to save recovered settings | error: {:#}", e);
            }
            settings
        }
        Err(e) => {
            log::error!("[settings] Failed to recover settings | error: {:#}", e);
            log::error!("[settings] Using default settings. Your old settings files have been preserved.");
            let mut settings = AppSettings::default();
            hydrate_and_migrate_ai_keys(&mut settings);
            settings
        }
    }
}

fn hydrate_and_migrate_ai_keys(settings: &mut AppSettings) {
    let mut needs_resave = false;
    for m in &mut settings.ai_models {
        if let Some(key) = crate::secure_storage::load_ai_api_key(&m.id) {
            m.api_key = key;
        } else if !m.api_key.is_empty() {
            let _ = crate::secure_storage::store_ai_api_key(&m.id, &m.api_key);
            needs_resave = true;
        }
    }
    if needs_resave {
        let _ = save_settings(settings);
    }
}

/// Attempt to recover settings from a potentially malformed JSON file.
///
/// Every `AppSettings` field carries `#[serde(default)]`, so the only thing
/// that makes a settings file fail to deserialize directly is a single field
/// holding a value of the wrong type. Rather than enumerate fields by hand
/// (which silently drops every field not listed, and breaks whenever a new
/// setting is added without updating this function), we recover *generically*:
/// we drop only the offending key(s) from the JSON object and let
/// `#[serde(default)]` fill the gaps. Every field that parses — including
/// fields added after this function was written — is preserved.
fn recover_settings_from_json(content: &str) -> Result<AppSettings> {
    use anyhow::Context;
    use serde_json::{Map, Value};

    // Compute the recovered `AppSettings` (fast path or cleaned), then funnel
    // both paths through `clamp_settings` before returning so that numeric
    // fields are bounded exactly as the old hand-rolled recovery did.
    let mut settings = if let Ok(settings) = serde_json::from_str::<AppSettings>(content) {
        // Fast path: the file is valid as-is. (`load_settings` already tries
        // this, but recovering directly keeps the function correct in isolation
        // and cheap in the common case.)
        settings
    } else {
        let value: Value =
            serde_json::from_str(content).context("Settings file is not valid JSON")?;

        let obj = value
            .as_object()
            .context("Settings file root is not a JSON object")?;

        // Rebuild the object key-by-key, keeping a key only if the accumulated
        // object still deserializes into `AppSettings`. Because no field uses
        // `#[serde(flatten)]`, aliases, or `deny_unknown_fields`, top-level keys
        // are independent: a key that parses on its own keeps parsing alongside
        // the others, and a key with a wrong-typed value is the only one dropped.
        let mut cleaned = Map::new();
        for (key, val) in obj {
            let mut candidate = cleaned.clone();
            candidate.insert(key.clone(), val.clone());
            if serde_json::from_value::<AppSettings>(Value::Object(candidate.clone())).is_ok() {
                cleaned = candidate;
            } else {
                log::warn!("[settings] Could not parse setting '{key}', falling back to its default");
            }
        }

        serde_json::from_value::<AppSettings>(Value::Object(cleaned))
            .context("Failed to deserialize recovered settings")?
    };

    clamp_settings(&mut settings);
    Ok(settings)
}

/// Clamp numeric settings into their valid ranges, preserving the bounds the
/// old hand-rolled `recover_settings_from_json` enforced.
fn clamp_settings(settings: &mut AppSettings) {
    settings.font_size = settings.font_size.clamp(8.0, 48.0);
    settings.line_height = settings.line_height.clamp(1.0, 3.0);
    settings.ui_font_size = settings.ui_font_size.clamp(8.0, 24.0);
    settings.file_font_size = settings.file_font_size.clamp(8.0, 24.0);
    settings.scrollback_lines = settings.scrollback_lines.clamp(100, 100_000);
    // 0 = disabled; otherwise cap the grace window at a sane upper bound.
    settings.terminal_close_grace_secs = settings.terminal_close_grace_secs.min(60);
}

/// Ensure settings version is set to current version
fn migrate_settings(mut settings: AppSettings) -> AppSettings {
    settings.version = SETTINGS_VERSION;
    settings
}

/// Process-level mutex for settings file access.
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

/// Save app settings to disk.
pub fn save_settings(settings: &AppSettings) -> Result<()> {
    let _slow = velowork_core::timing::SlowGuard::new("save_settings");
    let _guard = SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    save_settings_locked(settings)
}

/// Inner save — caller MUST already hold `SETTINGS_LOCK`.
///
/// Saves the unified `AppSettings` directly to `config/settings.json`.
fn save_settings_locked(settings: &AppSettings) -> Result<()> {
    for m in &settings.ai_models {
        if !m.api_key.is_empty() {
            let _ = crate::secure_storage::store_ai_api_key(&m.id, &m.api_key);
        }
    }
    let dir = config_dir_for_settings();
    std::fs::create_dir_all(&dir)?;

    let value = serde_json::to_value(settings)?;
    let serde_json::Value::Object(map) = value else {
        bail!("serialized settings is not a JSON object");
    };

    write_json_atomic(&dir.join("settings.json"), &serde_json::Value::Object(map))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recover_keeps_valid_and_future_fields_drops_wrong_typed() {
        // (a) a valid known field, (b) a field with a wrong type, and
        // (c) a brand-new field not enumerated anywhere. Recovery must keep
        // (a) and (c) and reset only (b) to its default.
        let json = r#"{
            "font_family": "Fira Code",
            "font_size": "not-a-number",
            "some_future_setting_we_dont_know_about": {"deep": [1, 2, 3]}
        }"#;
        let recovered = recover_settings_from_json(json).unwrap();
        // (a) valid known field preserved
        assert_eq!(recovered.font_family, "Fira Code");
        // (b) wrong-typed field reset to default
        assert_eq!(recovered.font_size, default_font_size());
        // (c) the unknown future field neither breaks recovery nor pollutes a
        // known field; everything not present falls back to its serde default
        // (here `version` has no key, so it gets `default_settings_version()`).
        assert_eq!(recovered.version, default_settings_version());
        assert_eq!(recovered.cursor_blink, default_cursor_blink());
    }

    #[test]
    fn recover_fully_valid_input_round_trips() {
        let original = AppSettings {
            font_family: "Custom Font".to_string(),
            font_size: 18.0,
            scrollback_lines: 42000,
            ..Default::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered = recover_settings_from_json(&json).unwrap();
        assert_eq!(recovered.font_family, "Custom Font");
        assert_eq!(recovered.font_size, 18.0);
        assert_eq!(recovered.scrollback_lines, 42000);
    }

    #[test]
    fn recover_clamps_out_of_range_numeric_fields() {
        // Fast path: otherwise-valid JSON, but numeric fields exceed their
        // allowed ranges. Recovery must clamp them exactly as the old
        // hand-rolled version did.
        let json = r#"{
            "font_size": 1000.0,
            "scrollback_lines": 999999999
        }"#;
        let recovered = recover_settings_from_json(json).unwrap();
        assert_eq!(recovered.font_size, 48.0);
        assert_eq!(recovered.scrollback_lines, 100_000);
    }

    #[test]
    fn recover_all_garbage_fields_returns_defaults() {
        // Valid JSON object, but every value has the wrong type.
        let json = r#"{
            "font_family": 123,
            "font_size": "huge",
            "cursor_blink": "yes",
            "scrollback_lines": [1, 2, 3]
        }"#;
        let recovered = recover_settings_from_json(json).unwrap();
        let defaults = AppSettings::default();
        assert_eq!(recovered.font_family, defaults.font_family);
        assert_eq!(recovered.font_size, defaults.font_size);
        assert_eq!(recovered.cursor_blink, defaults.cursor_blink);
        assert_eq!(recovered.scrollback_lines, defaults.scrollback_lines);
    }

    #[test]
    fn recover_rejects_non_object_root() {
        assert!(recover_settings_from_json("[1, 2, 3]").is_err());
        assert!(recover_settings_from_json("not json at all").is_err());
    }

    #[test]
    fn enabled_extensions_not_serialized_with_legacy_fields() {
        let mut settings = AppSettings::default();
        settings
            .enabled_extensions
            .insert("claude-code".to_string());
        let json = serde_json::to_string_pretty(&settings).unwrap();
        // Legacy bool fields should not appear in serialized output
        assert!(!json.contains("claude_code_integration"));
        assert!(!json.contains("codex_integration"));
        // enabled_extensions should be present
        assert!(json.contains("enabled_extensions"));
        assert!(json.contains("claude-code"));
    }

    // === Unified settings.json storage tests ===

    #[test]
    fn settings_round_trip_with_ai_and_sync() {
        let settings = AppSettings {
            ai_enabled: true,
            ai_models: vec![AiModelConfig::new(
                "Claude".into(),
                "https://api.anthropic.com".into(),
                "secret".into(),
                "claude-3-5-sonnet".into(),
            )],
            ai_default_model_id: Some("claude-3-5-sonnet".into()),
            sync: SyncSettings {
                enabled: true,
                provider: SyncProvider::WebDav,
                webdav: WebDavConfig {
                    server_url: "https://dav.example.com".into(),
                    username: "user".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            font_family: "Fira Code".into(),
            font_size: 16.0,
            ..Default::default()
        };

        let json = serde_json::to_string_pretty(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.ai_enabled, true);
        assert_eq!(restored.ai_models.len(), 1);
        assert_eq!(restored.ai_models[0].model_id, "claude-3-5-sonnet");
        assert_eq!(restored.sync.enabled, true);
        assert_eq!(restored.sync.webdav.server_url, "https://dav.example.com");
        assert_eq!(restored.font_family, "Fira Code");
        assert_eq!(restored.font_size, 16.0);
    }

    #[test]
    fn test_sync_settings_validate_configuration() {
        let mut sync = SyncSettings::default();
        // 1. 未启用
        assert_eq!(sync.validate_configuration(), Err(SyncConfigError::Disabled));

        // 2. 启用但服务器地址为空
        sync.enabled = true;
        assert_eq!(sync.validate_configuration(), Err(SyncConfigError::MissingServerUrl));

        // 3. 服务器地址缺少 http/https 协议
        sync.webdav.server_url = "dav.example.com/remote.php/webdav".into();
        assert_eq!(sync.validate_configuration(), Err(SyncConfigError::InvalidServerUrl));

        // 4. 正确配置
        sync.webdav.server_url = "https://dav.example.com/remote.php/webdav".into();
        assert!(sync.validate_configuration().is_ok());

        sync.webdav.server_url = "http://192.168.1.100:8080".into();
        assert!(sync.validate_configuration().is_ok());

        // 5. S3 验证
        sync.provider = SyncProvider::S3;
        assert_eq!(sync.validate_configuration(), Err(SyncConfigError::MissingS3Endpoint));

        sync.s3.endpoint = "s3.amazonaws.com".into();
        assert_eq!(sync.validate_configuration(), Err(SyncConfigError::InvalidS3Endpoint));

        sync.s3.endpoint = "https://s3.amazonaws.com".into();
        assert_eq!(sync.validate_configuration(), Err(SyncConfigError::MissingS3Bucket));

        sync.s3.bucket = "my-bucket".into();
        assert_eq!(sync.validate_configuration(), Err(SyncConfigError::MissingS3AccessKey));

        sync.s3.access_key_id = "AKIAEXAMPLE".into();
        assert!(sync.validate_configuration().is_ok());
    }

    #[test]
    fn test_close_behavior_deserialization_compatibility() {
        // 新格式
        let v: CloseBehavior = serde_json::from_str("\"minimize\"").unwrap();
        assert_eq!(v, CloseBehavior::Minimize);
        let v: CloseBehavior = serde_json::from_str("\"exit\"").unwrap();
        assert_eq!(v, CloseBehavior::Exit);

        // 旧别名兼容
        let v: CloseBehavior = serde_json::from_str("\"minimizetotray\"").unwrap();
        assert_eq!(v, CloseBehavior::Minimize);
        let v: CloseBehavior = serde_json::from_str("\"closetotray\"").unwrap();
        assert_eq!(v, CloseBehavior::Minimize);
        let v: CloseBehavior = serde_json::from_str("\"quit\"").unwrap();
        assert_eq!(v, CloseBehavior::Exit);
    }

    #[test]
    fn test_notification_settings_deserialization_compatibility() {
        let json = r#"{"enabled": true, "osc": true, "bell": false}"#;
        let n: NotificationSettings = serde_json::from_str(json).unwrap();
        assert_eq!(n.enabled, true);
        assert_eq!(n.osc, true);
        assert_eq!(n.bell, false);

        // 缺省字段容错
        let json = r#"{"enabled": true}"#;
        let n: NotificationSettings = serde_json::from_str(json).unwrap();
        assert_eq!(n.enabled, true);
        assert_eq!(n.osc, true);
        assert_eq!(n.bell, true);
    }

    #[test]
    fn test_confirm_close_tab_deserialization_compatibility() {
        // 缺省字段默认为 true
        let json = r#"{}"#;
        let recovered = recover_settings_from_json(json).unwrap();
        assert_eq!(recovered.confirm_close_tab, true);

        // 显式为 false 时正常保留
        let json = r#"{"confirm_close_tab": false}"#;
        let recovered = recover_settings_from_json(json).unwrap();
        assert_eq!(recovered.confirm_close_tab, false);
    }
}
