#![recursion_limit = "512"]
#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

//! Velowork terminal views crate.
//!
//! Contains custom GPUI elements for terminal rendering and the layout system
//! (split panes, tabs, terminal panes) used by the main application.

pub mod actions;
pub mod background_cache;
pub mod bg_processor;
pub mod elements;
pub mod layout;
pub mod overlays;
pub mod rendering;
pub mod shell_selector_overlay;
pub mod transfer_store;
pub mod welcome;

pub use background_cache::{
    init_terminal_background_cache, terminal_background_cache, terminal_background_element,
    terminal_background_element_with_corners, BackgroundImageError, GlobalTerminalBackgroundCache,
};

use velowork_core::api::ActionRequest;
use velowork_workspace::state::SplitDirection;

/// Trait for dispatching terminal actions (local or remote).
///
/// This abstracts the `ActionDispatcher` enum from the main application,
/// allowing the layout views to dispatch actions without knowing whether
/// the project is local or remote.
pub trait ActionDispatch: Clone + 'static {
    /// Dispatch a standard action.
    fn dispatch(&self, action: ActionRequest, cx: &mut gpui::App);

    /// Whether this dispatcher targets a remote project.
    fn is_remote(&self) -> bool;

    /// Split a terminal.
    fn split_terminal(
        &self,
        project_id: &str,
        layout_path: &[usize],
        direction: SplitDirection,
        cx: &mut gpui::App,
    );

    /// Add a tab.
    fn add_tab(
        &self,
        project_id: &str,
        layout_path: &[usize],
        in_group: bool,
        cx: &mut gpui::App,
    );

    /// Add a tab with a specific shell type.
    fn add_tab_with_shell(
        &self,
        project_id: &str,
        layout_path: &[usize],
        shell_type: velowork_core::shell::ShellType,
        in_group: bool,
        cx: &mut gpui::App,
    ) {
        let _ = shell_type;
        self.add_tab(project_id, layout_path, in_group, cx);
    }

    /// Upload a clipboard image pasted into a remote terminal.
    ///
    /// The temp file the terminal's process reads must live on the *server's*
    /// filesystem, not the client's. Remote dispatchers upload the bytes so the
    /// server materialises the file and bracketed-pastes its path. Local
    /// dispatchers don't override this — the caller writes a local temp file
    /// directly (see `TerminalPane::handle_paste`).
    fn upload_remote_paste_image(
        &self,
        terminal_id: &str,
        mime: &str,
        bytes: Vec<u8>,
        cx: &mut gpui::App,
    ) {
        let _ = (terminal_id, mime, bytes, cx);
    }
}

/// Settings namespace used in ExtensionSettingsStore.
const SETTINGS_ID: &str = "terminal";

/// Settings needed by terminal views.
///
/// Read/written through `ExtensionSettingsStore` so that changes flow through
/// the host app's persistence system automatically.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TerminalViewSettings {
    pub font_size: f32,
    pub line_height: f32,
    pub font_family: String,
    /// Base terminal font weight (Normal/Medium/Bold).
    #[serde(default = "default_font_weight")]
    pub font_weight: String,
    /// Base terminal font style (Normal/Italic).
    #[serde(default = "default_font_style")]
    pub font_style: String,
    pub cursor_style: velowork_workspace::settings::CursorShape,
    pub cursor_blink: bool,
    pub show_focused_border: bool,
    pub show_shell_selector: bool,
    pub idle_timeout_secs: u32,
    pub color_tinted_background: bool,
    pub file_opener: String,
    pub default_shell: velowork_terminal::shell_config::ShellType,
    /// When true, Ctrl+C copies the active selection (and clears it) instead of sending SIGINT.
    /// Ctrl+C without a selection always sends SIGINT.
    pub ctrl_c_copies_selection: bool,
    /// When true, line numbers are displayed in a gutter on the left of the terminal.
    pub show_line_numbers: bool,
    /// Mirror of `AppSettings::restore_terminals_on_startup`.
    /// When false (default), previously-open terminal sessions are NOT
    /// reconnected/reopened on app startup.
    #[serde(default = "default_restore_terminals_on_startup")]
    pub restore_terminals_on_startup: bool,
    /// Terminal background image path or URL. When set, the terminal content
    /// background renders transparent so the image shows through behind text.
    #[serde(default)]
    pub terminal_background_image: Option<String>,
    /// Apply a blur to the terminal background image to improve text readability.
    #[serde(default)]
    pub terminal_background_image_blur: bool,
    #[serde(default)]
    pub terminal_scrollbar_show: velowork_core::types::ScrollbarShow,
    #[serde(default = "default_color_scheme")]
    pub color_scheme: String,
    #[serde(default)]
    pub custom_terminal_color_schemes: Vec<velowork_core::theme::CustomTerminalColorScheme>,
    #[serde(default = "default_charset")]
    pub charset: String,
    #[serde(default = "default_term_type")]
    pub term_type: String,
    #[serde(default = "default_true")]
    pub shell_integration: bool,
    #[serde(default = "default_true")]
    pub bracketed_paste: bool,
    #[serde(default)]
    pub osc52_clipboard: bool,
    #[serde(default = "default_true")]
    pub true_color: bool,
    #[serde(default)]
    pub wrap_mode: velowork_workspace::settings::WrapMode,
    #[serde(default = "default_scrollback_lines")]
    pub scrollback_lines: u32,
    #[serde(default)]
    pub bell_style: velowork_core::types::BellStyle,
    #[serde(default = "default_bell_cooldown_ms")]
    pub bell_cooldown_ms: u32,
    #[serde(default = "default_word_selection_delimiters")]
    pub word_selection_delimiters: String,
    #[serde(default)]
    pub terminal_copy_on_select: bool,
    #[serde(default)]
    pub terminal_right_click_paste: bool,
    #[serde(default = "default_command_history_max_count")]
    pub command_history_max_count: usize,
    #[serde(default = "default_command_history_retention_days")]
    pub command_history_retention_days: u32,
    #[serde(default = "default_command_history_auto_completion")]
    pub command_history_auto_completion: bool,
    #[serde(default = "default_command_history_ignored_commands")]
    pub command_history_ignored_commands: Vec<String>,
    #[serde(default = "default_command_history_ignore_space")]
    pub command_history_ignore_space: bool,
    #[serde(default = "default_true")]
    pub ai_enabled: bool,
    #[serde(default = "default_true")]
    pub terminal_ai_floating_toolbar_enabled: bool,
    #[serde(default = "default_true")]
    pub terminal_ai_ghost_text_enabled: bool,
}

impl TerminalViewSettings {
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

fn default_bell_cooldown_ms() -> u32 {
    500
}

fn default_command_history_max_count() -> usize {
    1000
}

fn default_command_history_retention_days() -> u32 {
    30
}

fn default_command_history_auto_completion() -> bool {
    true
}

fn default_command_history_ignored_commands() -> Vec<String> {
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

fn default_command_history_ignore_space() -> bool {
    true
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

fn default_scrollback_lines() -> u32 {
    10000
}

fn default_word_selection_delimiters() -> String {
    r#"/,=+:;@#$&?%~|"'`(){}[]<>"#.to_string()
}

fn default_restore_terminals_on_startup() -> bool {
    false
}

fn default_font_weight() -> String {
    "Normal".to_string()
}

fn default_true() -> bool {
    true
}

fn default_font_style() -> String {
    "Normal".to_string()
}

// Read current terminal view settings from ExtensionSettingsStore.
pub fn terminal_view_settings(cx: &gpui::App) -> TerminalViewSettings {
    let store = cx.global::<velowork_extensions::ExtensionSettingsStore>();
    store
        .get(SETTINGS_ID, cx)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(|| TerminalViewSettings {
            font_size: 13.0,
            line_height: 1.3,
            font_family: "JetBrains Mono".to_string(),
            font_weight: default_font_weight(),
            font_style: default_font_style(),
            cursor_style: Default::default(),
            cursor_blink: false,
            bell_style: velowork_core::types::BellStyle::default(),
            bell_cooldown_ms: default_bell_cooldown_ms(),
            show_focused_border: false,
            show_shell_selector: false,
            idle_timeout_secs: 0,
            color_tinted_background: false,
            file_opener: String::new(),
            default_shell: velowork_terminal::shell_config::ShellType::Default,
            ctrl_c_copies_selection: false,
            show_line_numbers: false,
            restore_terminals_on_startup: false,
            terminal_background_image: None,
            terminal_background_image_blur: false,
            terminal_scrollbar_show: Default::default(),
            color_scheme: default_color_scheme(),
            custom_terminal_color_schemes: Vec::new(),
            charset: default_charset(),
            term_type: default_term_type(),
            wrap_mode: Default::default(),
            scrollback_lines: default_scrollback_lines(),
            word_selection_delimiters: default_word_selection_delimiters(),
            terminal_copy_on_select: false,
            terminal_right_click_paste: false,
            command_history_max_count: default_command_history_max_count(),
            command_history_retention_days: default_command_history_retention_days(),
            command_history_auto_completion: default_command_history_auto_completion(),
            command_history_ignored_commands: default_command_history_ignored_commands(),
            command_history_ignore_space: default_command_history_ignore_space(),
            shell_integration: true,
            bracketed_paste: true,
            osc52_clipboard: true,
            true_color: true,
            ai_enabled: true,
            terminal_ai_floating_toolbar_enabled: true,
            terminal_ai_ghost_text_enabled: true,
        })
}

/// Write terminal view settings to ExtensionSettingsStore.
pub fn set_terminal_view_settings(settings: &TerminalViewSettings, cx: &mut gpui::App) {
    if let Ok(value) = serde_json::to_value(settings) {
        velowork_extensions::ExtensionSettingsStore::update(SETTINGS_ID, value, cx);
    }
}

/// Callback type for registering content panes for dirty notification.
pub type RegisterContentPaneFn = Box<dyn Fn(String, gpui::WeakEntity<layout::terminal_pane::TerminalContent>) + Send + Sync>;

/// Global content pane registration function.
static REGISTER_CONTENT_PANE_FN: std::sync::OnceLock<RegisterContentPaneFn> = std::sync::OnceLock::new();

/// Set the global content pane registration function.
/// Called once by the main app at startup.
pub fn set_register_content_pane_fn(f: RegisterContentPaneFn) {
    let _ = REGISTER_CONTENT_PANE_FN.set(f);
}

/// Register a terminal content pane for direct dirty notification.
pub fn register_content_pane(
    terminal_id: String,
    content: gpui::WeakEntity<layout::terminal_pane::TerminalContent>,
) {
    if let Some(f) = REGISTER_CONTENT_PANE_FN.get() {
        f(terminal_id, content);
    }
}

/// Callback type for showing toast notifications.
pub type ToastErrorFn = Box<dyn Fn(String, &mut gpui::App) + Send + Sync>;

/// Global toast error function.
static TOAST_ERROR_FN: std::sync::OnceLock<ToastErrorFn> = std::sync::OnceLock::new();

/// Set the global toast error function.
pub fn set_toast_error_fn(f: ToastErrorFn) {
    let _ = TOAST_ERROR_FN.set(f);
}

/// Show an error toast notification.
pub fn toast_error(msg: String, cx: &mut gpui::App) {
    if let Some(f) = TOAST_ERROR_FN.get() {
        f(msg, cx);
    } else {
        log::error!("{}", msg);
    }
}
