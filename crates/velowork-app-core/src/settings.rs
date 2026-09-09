//! Global observable settings module
//!
//! Provides app-wide access to settings through the GlobalSettings global.
//! Settings are automatically persisted to disk with debouncing.

use crate::settings_persister::SettingsPersister;
use crate::workspace::persistence::{AppSettings, get_settings_path, load_settings, save_settings};
use velowork_workspace::settings::{CustomTitlebarPosition, CustomTitlebarPreset, TitlebarStyle};
use gpui::*;
use velowork_terminal::session_backend::SessionBackend;
use velowork_terminal::shell_config::ShellType;
use velowork_theme::ColorTheme;
use velowork_workspace::sync::notify_config_changed;

/// Global settings wrapper for app-wide access
#[derive(Clone)]
pub struct GlobalSettings(pub Entity<SettingsState>);

impl Global for GlobalSettings {}

/// Settings state that can be observed and updated
pub struct SettingsState {
    pub settings: AppSettings,
    persister: SettingsPersister,
}

/// Macro to generate setter methods with clamping and auto-save
macro_rules! setting_setter {
    // For f32 values with min/max clamping
    ($fn_name:ident, $field:ident, f32, $min:expr, $max:expr) => {
        pub fn $fn_name(&mut self, value: f32, cx: &mut Context<Self>) {
            let clamped = value.clamp($min, $max);
            if (self.settings.$field - clamped).abs() < f32::EPSILON {
                return;
            }
            self.settings.$field = clamped;
            self.save_and_notify(cx);
        }
    };
    // For u32 values with min/max clamping
    ($fn_name:ident, $field:ident, u32, $min:expr, $max:expr) => {
        pub fn $fn_name(&mut self, value: u32, cx: &mut Context<Self>) {
            let clamped = value.clamp($min, $max);
            if self.settings.$field == clamped {
                return;
            }
            self.settings.$field = clamped;
            self.save_and_notify(cx);
        }
    };
    // For bool values (no clamping)
    ($fn_name:ident, $field:ident, bool) => {
        pub fn $fn_name(&mut self, value: bool, cx: &mut Context<Self>) {
            if self.settings.$field == value {
                return;
            }
            self.settings.$field = value;
            self.save_and_notify(cx);
        }
    };
    // For String values (no clamping)
    ($fn_name:ident, $field:ident, String) => {
        pub fn $fn_name(&mut self, value: String, cx: &mut Context<Self>) {
            if self.settings.$field == value {
                return;
            }
            self.settings.$field = value;
            self.save_and_notify(cx);
        }
    };
    // For arbitrary enum/struct types (no clamping)
    ($fn_name:ident, $field:ident, $type:ty) => {
        pub fn $fn_name(&mut self, value: $type, cx: &mut Context<Self>) {
            if self.settings.$field == value {
                return;
            }
            self.settings.$field = value;
            self.save_and_notify(cx);
        }
    };
}

impl SettingsState {
    pub fn new(settings: AppSettings) -> Self {
        let persister = SettingsPersister::spawn(&settings);
        Self {
            settings,
            persister,
        }
    }

    pub fn get(&self) -> &AppSettings {
        &self.settings
    }

    // Generate all setters using the macro
    setting_setter!(set_font_size, font_size, f32, 8.0, 48.0);
    setting_setter!(set_font_family, font_family, String);
    setting_setter!(set_font_weight, font_weight, String);
    setting_setter!(set_font_style, font_style, String);
    setting_setter!(set_line_height, line_height, f32, 1.0, 3.0);
    setting_setter!(set_ui_font_size, ui_font_size, f32, 8.0, 24.0);
    setting_setter!(set_ui_scale, ui_scale, f32, 80.0, 200.0);
    /// Set the UI density level (Compact, Default, Comfortable)
    pub fn set_ui_density(
        &mut self,
        value: velowork_core::theme::UiDensity,
        cx: &mut Context<Self>,
    ) {
        self.settings.ui_density = value;
        self.save_and_notify(cx);
    }
    setting_setter!(set_mono_font_family, mono_font_family, String);
    setting_setter!(set_markdown_font_family, markdown_font_family, String);
    setting_setter!(set_file_font_size, file_font_size, f32, 8.0, 24.0);
    /// Set the cursor style (Block, Bar, Underline)
    pub fn set_cursor_style(
        &mut self,
        value: crate::workspace::settings::CursorShape,
        cx: &mut Context<Self>,
    ) {
        self.settings.cursor_style = value;
        self.save_and_notify(cx);
    }

    /// Set the UI language locale and refresh all windows
    pub fn set_locale(
        &mut self,
        value: crate::workspace::settings::Locale,
        cx: &mut Context<Self>,
    ) {
        if self.settings.locale == value {
            return;
        }
        self.settings.locale = value;
        self.save_and_notify(cx);
        velowork_i18n::set_locale(value, cx);
    }

    /// Set the terminal scrollbar display mode
    pub fn set_terminal_scrollbar_show(
        &mut self,
        value: velowork_core::types::ScrollbarShow,
        cx: &mut Context<Self>,
    ) {
        self.settings.terminal_scrollbar_show = value;
        self.save_and_notify(cx);
    }

    setting_setter!(set_cursor_blink, cursor_blink, bool);
    /// Set the terminal bell style (Visual, Audible, Both, Disabled)
    pub fn set_bell_style(
        &mut self,
        value: velowork_core::types::BellStyle,
        cx: &mut Context<Self>,
    ) {
        if self.settings.bell_style == value {
            return;
        }
        self.settings.bell_style = value;
        self.save_and_notify(cx);
    }
    setting_setter!(set_bell_cooldown_ms, bell_cooldown_ms, u32, 0, 5000);
    setting_setter!(set_scrollback_lines, scrollback_lines, u32, 100, 100000);
    setting_setter!(
        set_terminal_close_grace_secs,
        terminal_close_grace_secs,
        u32,
        0,
        60
    );
    setting_setter!(set_show_focused_border, show_focused_border, bool);
    setting_setter!(set_titlebar_style, titlebar_style, TitlebarStyle);
    setting_setter!(set_titlebar_preset, titlebar_preset, CustomTitlebarPreset);
    setting_setter!(set_titlebar_position, titlebar_position, CustomTitlebarPosition);
    setting_setter!(set_titlebar_height, titlebar_height, f32, 20.0, 60.0);
    setting_setter!(set_window_control_button_gap, window_control_button_gap, f32, 0.0, 32.0);
    setting_setter!(set_window_control_margin, window_control_margin, f32, 0.0, 48.0);
    setting_setter!(set_window_corner_radius, window_corner_radius, f32, 0.0, 32.0);
    setting_setter!(set_window_control_icon_size, window_control_icon_size, f32, 8.0, 32.0);
    setting_setter!(set_color_tinted_background, color_tinted_background, bool);
    setting_setter!(set_enable_animations, enable_animations, bool);
    setting_setter!(
        set_detached_overlays_by_default,
        detached_overlays_by_default,
        bool
    );

    /// Persist the most recent detached overlay window bounds.
    pub fn set_detached_overlay_bounds(
        &mut self,
        bounds: crate::workspace::settings::DetachedWindowBounds,
        cx: &mut Context<Self>,
    ) {
        self.settings.detached_overlay_bounds = Some(bounds);
        self.save_and_notify(cx);
    }

    setting_setter!(set_monitor_popup_width, monitor_popup_width, f32, 400.0, 2000.0);
    setting_setter!(set_monitor_popup_height, monitor_popup_height, f32, 300.0, 2000.0);

    /// Set both monitor popup dimensions at once and persist.
    pub fn set_monitor_popup_size(&mut self, width: f32, height: f32, cx: &mut Context<Self>) {
        let clamped_w = width.clamp(400.0, 2000.0);
        let clamped_h = height.clamp(300.0, 2000.0);
        if (self.settings.monitor_popup_width - clamped_w).abs() < f32::EPSILON
            && (self.settings.monitor_popup_height - clamped_h).abs() < f32::EPSILON
        {
            return;
        }
        self.settings.monitor_popup_width = clamped_w;
        self.settings.monitor_popup_height = clamped_h;
        self.save_and_notify(cx);
    }

    setting_setter!(set_min_column_width, min_column_width, f32, 100.0, 2000.0);
    setting_setter!(set_show_shell_selector, show_shell_selector, bool);
    setting_setter!(
        set_terminal_ctrl_c_copies_selection,
        terminal_ctrl_c_copies_selection,
        bool
    );
    setting_setter!(set_show_line_numbers, show_line_numbers, bool);
    // Toggle whether previously-open terminal sessions are reconnected/reopened on app startup.
    setting_setter!(
        set_restore_terminals_on_startup,
        restore_terminals_on_startup,
        bool
    );
    setting_setter!(
        set_word_selection_delimiters,
        word_selection_delimiters,
        String
    );
    setting_setter!(set_terminal_copy_on_select, terminal_copy_on_select, bool);
    setting_setter!(
        set_terminal_right_click_paste,
        terminal_right_click_paste,
        bool
    );

    /// Master switch for native desktop notifications (opt-in).
    pub fn set_notifications_enabled(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.notifications.enabled = value;
        self.save_and_notify(cx);
    }
    /// Toggle notifications for OSC 9 / OSC 777 terminal alerts.
    pub fn set_notifications_osc(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.notifications.osc = value;
        self.save_and_notify(cx);
    }
    /// Toggle notifications for the terminal bell.
    pub fn set_notifications_bell(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.notifications.bell = value;
        self.save_and_notify(cx);
    }

    /// Set file finder "show ignored" preference (persisted default for future opens).
    pub fn set_file_finder_show_ignored(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.file_finder.show_ignored = value;
        self.save_and_notify(cx);
    }
    setting_setter!(set_idle_timeout_secs, idle_timeout_secs, u32, 0, 300);
    /// Set the default shell type for new terminals
    pub fn set_default_shell(&mut self, value: ShellType, cx: &mut Context<Self>) {
        self.settings.default_shell = value;
        self.save_and_notify(cx);
    }

    /// Set the session backend for terminal persistence
    pub fn set_session_backend(&mut self, value: SessionBackend, cx: &mut Context<Self>) {
        self.settings.session_backend = value;
        self.save_and_notify(cx);
    }

    // ============================================================
    // Command History settings
    // ============================================================
    setting_setter!(set_command_history_max_count, command_history_max_count, usize);
    setting_setter!(set_command_history_retention_days, command_history_retention_days, u32);
    setting_setter!(set_command_history_auto_completion, command_history_auto_completion, bool);
    setting_setter!(set_command_history_ignore_space, command_history_ignore_space, bool);
    pub fn set_command_history_ignored_commands(&mut self, value: Vec<String>, cx: &mut Context<Self>) {
        self.settings.command_history_ignored_commands = value;
        self.save_and_notify(cx);
    }

    // ============================================================
    // AI Assistant settings
    // ============================================================

    /// Enable/disable the AI assistant feature
    pub fn set_ai_enabled(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.ai_enabled = value;
        self.save_and_notify(cx);
    }

    /// Set AI context auto-compression enabled state
    pub fn set_ai_auto_compress(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.ai_auto_compress = value;
        self.save_and_notify(cx);
    }

    /// Set AI context compression strategy
    pub fn set_ai_compression_strategy(
        &mut self,
        strategy: crate::workspace::settings::AiCompressionStrategy,
        cx: &mut Context<Self>,
    ) {
        self.settings.ai_compression_strategy = strategy;
        self.save_and_notify(cx);
    }

    /// Set max context tokens threshold
    pub fn set_ai_max_context_tokens(&mut self, value: usize, cx: &mut Context<Self>) {
        self.settings.ai_max_context_tokens = value;
        self.save_and_notify(cx);
    }

    /// Set max retained history messages count
    pub fn set_ai_max_history_messages(&mut self, value: usize, cx: &mut Context<Self>) {
        self.settings.ai_max_history_messages = value;
        self.save_and_notify(cx);
    }

    /// Enable/disable a specific AI skill
    pub fn set_ai_skill_enabled(&mut self, skill_name: &str, enabled: bool, cx: &mut Context<Self>) {
        if enabled {
            self.settings.ai_disabled_skills.retain(|s| s != skill_name);
        } else if !self.settings.ai_disabled_skills.iter().any(|s| s == skill_name) {
            self.settings.ai_disabled_skills.push(skill_name.to_string());
        }
        self.save_and_notify(cx);
    }

    /// Add a new AI model configuration
    pub fn add_ai_model(
        &mut self,
        model: crate::workspace::settings::AiModelConfig,
        cx: &mut Context<Self>,
    ) {
        self.settings.ai_models.push(model);
        self.save_and_notify(cx);
    }

    /// Update an existing AI model configuration by ID
    pub fn update_ai_model(
        &mut self,
        model: crate::workspace::settings::AiModelConfig,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self
            .settings
            .ai_models
            .iter_mut()
            .find(|m| m.id == model.id)
        {
            *existing = model;
            self.save_and_notify(cx);
        }
    }

    /// Remove an AI model configuration by ID
    pub fn remove_ai_model(&mut self, model_id: &str, cx: &mut Context<Self>) {
        self.settings.ai_models.retain(|m| m.id != model_id);
        if self.settings.ai_default_model_id.as_deref() == Some(model_id) {
            self.settings.ai_default_model_id = None;
        }
        self.save_and_notify(cx);
    }

    /// Toggle an AI model's enabled state
    pub fn toggle_ai_model(&mut self, model_id: &str, enabled: bool, cx: &mut Context<Self>) {
        if let Some(model) = self
            .settings
            .ai_models
            .iter_mut()
            .find(|m| m.id == model_id)
        {
            model.enabled = enabled;
            self.save_and_notify(cx);
        }
    }

    /// Add a new search engine configuration.
    pub fn add_search_engine(
        &mut self,
        engine: crate::workspace::settings::SearchEngineConfig,
        cx: &mut Context<Self>,
    ) {
        self.settings.search_engines.push(engine);
        self.save_and_notify(cx);
    }

    /// Update an existing search engine configuration by ID.
    pub fn update_search_engine(
        &mut self,
        engine: crate::workspace::settings::SearchEngineConfig,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self
            .settings
            .search_engines
            .iter_mut()
            .find(|e| e.id == engine.id)
        {
            *existing = engine;
            self.save_and_notify(cx);
        }
    }

    /// Remove a search engine configuration by ID.
    pub fn remove_search_engine(&mut self, engine_id: &str, cx: &mut Context<Self>) {
        self.settings
            .search_engines
            .retain(|e| e.id != engine_id);
        self.save_and_notify(cx);
    }

    /// Toggle a search engine's enabled state.
    pub fn toggle_search_engine(&mut self, engine_id: &str, enabled: bool, cx: &mut Context<Self>) {
        if let Some(engine) = self
            .settings
            .search_engines
            .iter_mut()
            .find(|e| e.id == engine_id)
        {
            engine.enabled = enabled;
            self.save_and_notify(cx);
        }
    }

    /// Set the default AI model for new conversations
    pub fn set_ai_default_model(&mut self, model_id: Option<String>, cx: &mut Context<Self>) {
        self.settings.ai_default_model_id = model_id;
        self.save_and_notify(cx);
    }

    setting_setter!(set_ai_temperature, ai_temperature, f32, 0.0, 2.0);
    setting_setter!(set_ai_max_tokens, ai_max_tokens, u32, 1, 128000);

    /// Set per-extension settings blob (opaque JSON value).
    pub fn set_extension_setting(
        &mut self,
        extension_id: &str,
        value: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        self.settings
            .extension_settings
            .insert(extension_id.to_string(), value);
        self.save_and_notify(cx);
    }

    /// Enable or disable an extension by ID.
    pub fn set_extension_enabled(
        &mut self,
        extension_id: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if enabled {
            self.settings
                .enabled_extensions
                .insert(extension_id.to_string());
        } else {
            self.settings.enabled_extensions.remove(extension_id);
        }
        self.save_and_notify(cx);
    }

    /// Set sidebar open state
    pub fn set_sidebar_open(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.sidebar.is_open = value;
        self.save_and_notify(cx);
    }

    /// Set sidebar auto-hide mode
    pub fn set_sidebar_auto_hide(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.sidebar.auto_hide = value;
        self.save_and_notify(cx);
    }

    /// Set sidebar width (clamped to min/max bounds)
    pub fn set_sidebar_width(&mut self, value: f32, cx: &mut Context<Self>) {
        use crate::workspace::persistence::{MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH};
        self.settings.sidebar.width = value.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        self.save_and_notify(cx);
    }

    /// Set right sidebar open state
    pub fn set_right_sidebar_open(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.right_sidebar.is_open = value;
        self.save_and_notify(cx);
    }

    /// Set right sidebar auto-hide mode
    pub fn set_right_sidebar_auto_hide(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.right_sidebar.auto_hide = value;
        self.save_and_notify(cx);
    }

    /// Set right sidebar width (clamped to bounds)
    pub fn set_right_sidebar_width(&mut self, value: f32, cx: &mut Context<Self>) {
        use crate::workspace::persistence::{MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH};
        self.settings.right_sidebar.width = value.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        self.save_and_notify(cx);
    }

    /// Set the color theme (palette) used when the active appearance is dark.
    pub fn set_dark_color_theme(&mut self, value: ColorTheme, cx: &mut Context<Self>) {
        self.settings.dark_color_theme = value;
        if value != ColorTheme::Custom {
            self.settings.custom_theme_id = None;
        }
        self.save_and_notify(cx);
    }

    /// Set the color theme (palette) used when the active appearance is light.
    pub fn set_light_color_theme(&mut self, value: ColorTheme, cx: &mut Context<Self>) {
        self.settings.light_color_theme = value;
        if value != ColorTheme::Custom {
            self.settings.custom_theme_id = None;
        }
        self.save_and_notify(cx);
    }

    /// Set the custom theme ID (file stem, e.g. "example-theme").
    pub fn set_custom_theme_id(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.settings.custom_theme_id = id;
        self.save_and_notify(cx);
    }

    /// Set the file opener command
    pub fn set_file_opener(&mut self, value: String, cx: &mut Context<Self>) {
        self.settings.file_opener = value;
        self.save_and_notify(cx);
    }

    // ============================================================
    // New setters for Settings Dialog (Phase 1)
    // ============================================================

    // General settings
    setting_setter!(set_start_on_boot, start_on_boot, bool);
    setting_setter!(set_auto_check_updates, auto_check_updates, bool);

    /// Set close window behavior
    pub fn set_close_behavior(
        &mut self,
        value: crate::workspace::settings::CloseBehavior,
        cx: &mut Context<Self>,
    ) {
        self.settings.close_behavior = value;
        self.save_and_notify(cx);
    }

    /// Set proxy mode
    pub fn set_proxy_mode(
        &mut self,
        value: crate::workspace::settings::ProxyMode,
        cx: &mut Context<Self>,
    ) {
        self.settings.proxy_mode = value;
        self.save_and_notify(cx);
    }

    setting_setter!(set_proxy_host, proxy_host, String);

    /// Set proxy port
    pub fn set_proxy_port(&mut self, value: u16, cx: &mut Context<Self>) {
        self.settings.proxy_port = value.max(1);
        self.save_and_notify(cx);
    }

    // Appearance settings
    /// Set color theme
    pub fn set_color_theme(
        &mut self,
        value: crate::workspace::settings::ColorSchema,
        cx: &mut Context<Self>,
    ) {
        self.settings.color_schema = value;
        self.save_and_notify(cx);
    }


    setting_setter!(set_bg_opacity, bg_opacity, f32, 0.1, 1.0);

    /// Set tab width mode
    pub fn set_tab_width_mode(
        &mut self,
        value: crate::workspace::settings::TabWidthMode,
        cx: &mut Context<Self>,
    ) {
        self.settings.tab_width_mode = value;
        self.save_and_notify(cx);
    }

    setting_setter!(set_enable_tab_preview, enable_tab_preview, bool);

    // Font settings
    setting_setter!(set_ui_font_family, ui_font_family, String);

    // Terminal settings
    setting_setter!(set_color_scheme, color_scheme, String);
    setting_setter!(set_charset, charset, String);
    setting_setter!(set_term_type, term_type, String);
    setting_setter!(set_shell_integration, shell_integration, bool);
    setting_setter!(set_bracketed_paste, bracketed_paste, bool);
    setting_setter!(set_osc52_clipboard, osc52_clipboard, bool);
    setting_setter!(set_true_color, true_color, bool);

    /// Set custom terminal color schemes
    pub fn set_custom_terminal_color_schemes(
        &mut self,
        schemes: Vec<velowork_core::theme::CustomTerminalColorScheme>,
        cx: &mut Context<Self>,
    ) {
        self.settings.custom_terminal_color_schemes = schemes;
        self.save_and_notify(cx);
    }

    /// Save (insert or update) a custom terminal color scheme
    pub fn save_custom_terminal_color_scheme(
        &mut self,
        scheme: velowork_core::theme::CustomTerminalColorScheme,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self
            .settings
            .custom_terminal_color_schemes
            .iter_mut()
            .find(|s| s.id == scheme.id)
        {
            *existing = scheme;
        } else {
            self.settings.custom_terminal_color_schemes.push(scheme);
        }
        self.save_and_notify(cx);
    }

    /// Delete a custom terminal color scheme by id
    pub fn delete_custom_terminal_color_scheme(
        &mut self,
        scheme_id: &str,
        cx: &mut Context<Self>,
    ) {
        self.settings
            .custom_terminal_color_schemes
            .retain(|s| s.id != scheme_id);
        self.save_and_notify(cx);
    }

    /// Set terminal background image path/URL. An empty string clears it (None).
    pub fn set_terminal_background_image(&mut self, value: Option<String>, cx: &mut Context<Self>) {
        self.settings.terminal_background_image = value.filter(|s| !s.trim().is_empty());
        self.save_and_notify(cx);
    }

    /// Enable/disable blurring of the terminal background image (improves text readability).
    pub fn set_terminal_background_image_blur(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.terminal_background_image_blur = value;
        self.save_and_notify(cx);
    }

    /// Set terminal wrap mode
    pub fn set_wrap_mode(
        &mut self,
        value: crate::workspace::settings::WrapMode,
        cx: &mut Context<Self>,
    ) {
        self.settings.wrap_mode = value;
        self.save_and_notify(cx);
    }

    // File manager settings
    setting_setter!(set_show_hidden_files, show_hidden_files, bool);
    setting_setter!(set_sftp_default_file_mode, sftp_default_file_mode, String);
    setting_setter!(set_sftp_default_dir_mode, sftp_default_dir_mode, String);

    /// Set file sort order
    pub fn set_file_sort_by(
        &mut self,
        value: crate::workspace::settings::FileSortBy,
        cx: &mut Context<Self>,
    ) {
        self.settings.file_sort_by = value;
        self.save_and_notify(cx);
    }

    setting_setter!(set_alternating_row_bg, alternating_row_bg, bool);

    // Security settings
    /// Enable/disable credential encryption
    pub fn set_encryption_enabled(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.security.encryption_enabled = value;
        self.save_and_notify(cx);
    }

    /// Set password timeout in seconds
    pub fn set_password_timeout_secs(&mut self, value: u32, cx: &mut Context<Self>) {
        self.settings.security.password_timeout_secs = value.min(86400);
        self.save_and_notify(cx);
    }

    /// Mirror of the active security mode (`"standard"` / `"enhanced"` / `"maximum"`).
    pub fn set_security_mode(&mut self, value: String, cx: &mut Context<Self>) {
        self.settings.security.security_mode = value;
        self.save_and_notify(cx);
    }

    /// Mirror of whether a master password is configured.
    pub fn set_master_password_set(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.security.master_password_set = value;
        self.save_and_notify(cx);
    }

    // Sync settings
    /// Enable/disable sync
    pub fn set_sync_enabled(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.settings.sync.enabled == value {
            return;
        }
        self.settings.sync.enabled = value;
        self.save_and_notify(cx);
    }

    /// Set sync provider
    pub fn set_sync_provider(
        &mut self,
        value: crate::workspace::settings::SyncProvider,
        cx: &mut Context<Self>,
    ) {
        if self.settings.sync.provider == value {
            return;
        }
        self.settings.sync.provider = value;
        self.save_and_notify(cx);
    }

    /// Set WebDAV server URL
    pub fn set_webdav_server_url(&mut self, value: String, cx: &mut Context<Self>) {
        if self.settings.sync.webdav.server_url == value {
            return;
        }
        self.settings.sync.webdav.server_url = value;
        self.save_and_notify(cx);
    }

    /// Set WebDAV username
    pub fn set_webdav_username(&mut self, value: String, cx: &mut Context<Self>) {
        if self.settings.sync.webdav.username == value {
            return;
        }
        self.settings.sync.webdav.username = value;
        self.save_and_notify(cx);
    }

    /// Set WebDAV remote path
    pub fn set_webdav_remote_path(&mut self, value: String, cx: &mut Context<Self>) {
        if self.settings.sync.webdav.remote_path == value {
            return;
        }
        self.settings.sync.webdav.remote_path = value;
        self.save_and_notify(cx);
    }

    /// 标记 WebDAV 密码是否已持久化到系统密钥库（真实密码不进 settings.json）。
    pub fn set_webdav_password_stored(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.settings.sync.webdav.password_stored == value {
            return;
        }
        self.settings.sync.webdav.password_stored = value;
        self.save_and_notify(cx);
    }

    /// Enable/disable auto-sync
    pub fn set_auto_sync(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.settings.sync.auto_sync == value {
            return;
        }
        self.settings.sync.auto_sync = value;
        self.save_and_notify(cx);
    }

    /// Set sync interval in seconds
    pub fn set_sync_interval_secs(&mut self, value: u32, cx: &mut Context<Self>) {
        let val = value.max(60);
        if self.settings.sync.sync_interval_secs == val {
            return;
        }
        self.settings.sync.sync_interval_secs = val;
        self.save_and_notify(cx);
    }

    /// 设置本地与云端配置冲突时的处理策略
    pub fn set_sync_conflict_strategy(
        &mut self,
        value: crate::workspace::settings::SyncConflictStrategy,
        cx: &mut Context<Self>,
    ) {
        if self.settings.sync.conflict_strategy == value {
            return;
        }
        self.settings.sync.conflict_strategy = value;
        self.save_and_notify(cx);
    }

    /// 设置同步数据范围
    pub fn set_sync_data_scope(
        &mut self,
        scope: crate::workspace::settings::SyncDataScope,
        cx: &mut Context<Self>,
    ) {
        self.settings.sync.data_scope = scope;
        self.save_and_notify(cx);
    }

    /// 单项切换同步范围
    pub fn set_sync_scope_item(
        &mut self,
        field: &str,
        value: bool,
        cx: &mut Context<Self>,
    ) {
        match field {
            "sessions" => self.settings.sync.data_scope.sessions = value,
            "tunnels" => self.settings.sync.data_scope.tunnels = value,
            "services" => self.settings.sync.data_scope.services = value,
            "quick_commands" => self.settings.sync.data_scope.quick_commands = value,
            "ai_chat" => self.settings.sync.data_scope.ai_chat = value,
            "command_history" => self.settings.sync.data_scope.command_history = value,
            "settings" => self.settings.sync.data_scope.settings = value,
            "themes" => self.settings.sync.data_scope.themes = value,
            "credentials" => self.settings.sync.data_scope.credentials = value,
            _ => return,
        }
        self.save_and_notify(cx);
    }

    /// 一键全选 / 全不选同步范围
    pub fn set_sync_data_scope_all(&mut self, value: bool, cx: &mut Context<Self>) {
        self.settings.sync.data_scope.sessions = value;
        self.settings.sync.data_scope.tunnels = value;
        self.settings.sync.data_scope.services = value;
        self.settings.sync.data_scope.quick_commands = value;
        self.settings.sync.data_scope.ai_chat = value;
        self.settings.sync.data_scope.command_history = value;
        self.settings.sync.data_scope.settings = value;
        self.settings.sync.data_scope.themes = value;
        self.settings.sync.data_scope.credentials = value;
        self.save_and_notify(cx);
    }

    /// 记录上次成功同步的时间（None 表示清空）。
    pub fn set_last_sync_at(
        &mut self,
        value: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.settings.sync.last_sync_at = value;
        self.save_and_notify(cx);
    }

    /// 从磁盘重新加载设置（用于「从云端恢复」后刷新运行中的设置实体）。
    /// 静默保存，不触发自动同步信号。
    pub fn reload_settings(&mut self, cx: &mut Context<Self>) {
        self.settings = crate::workspace::settings::load_settings();
        self.persister.send_save(self.settings.clone());
        cx.notify();
    }

    /// Synchronously flush any pending settings save (called on quit)
    pub fn flush_pending_save(&self) {
        self.persister.flush_sync();
    }

    /// Save and notify - common logic for all setters.
    /// Public so that the ExtensionSettingsStore setter callback can trigger persistence.
    pub fn save_and_notify(&mut self, cx: &mut Context<Self>) {
        self.persister.send_save(self.settings.clone());
        // 任意配置变更都唤醒后台自动同步引擎（「配置修改即触发同步」）。
        notify_config_changed();
        cx.notify();
    }
}

/// Get the global settings entity
pub fn settings_entity(cx: &App) -> Entity<SettingsState> {
    cx.global::<GlobalSettings>().0.clone()
}

/// Get a copy of the current settings
pub fn settings(cx: &App) -> AppSettings {
    settings_entity(cx).read(cx).settings.clone()
}

/// Resolve the window-surface alpha a window must request *at creation* so that
/// custom-titlebar rounded corners (and translucent backgrounds) are drawn
/// without sharp square tips on compositors such as KDE Plasma on Wayland.
///
/// Those compositors negotiate the window's alpha channel when the surface is
/// first created and ignore later `set_background_appearance` mutations, so the
/// correct appearance has to be set on `WindowOptions` up front. This mirrors
/// the runtime logic in `velowork_app::simple_root::SimpleRoot::render`. A
/// freshly opened window is never maximized, so the maximize guard applied at
/// runtime is intentionally omitted here.
pub fn window_background_appearance(settings: &AppSettings) -> gpui::WindowBackgroundAppearance {
    if settings.bg_opacity < 1.0 {
        gpui::WindowBackgroundAppearance::Blurred
    } else if settings.titlebar_style == TitlebarStyle::Custom && settings.window_corner_radius > 0.0
    {
        gpui::WindowBackgroundAppearance::Transparent
    } else {
        gpui::WindowBackgroundAppearance::Opaque
    }
}

/// Whether the given window currently shows rounded corners (client-side
/// decoration).
///
/// Mirrors the runtime guard used in `SimpleRoot::render` and
/// `WindowView::render`: rounded corners only apply with the custom titlebar,
/// when the window is neither maximized nor fullscreen, and when a positive
/// corner radius is configured. Use this for full-window dimming masks so they
/// clip their own background to the rounded window shape instead of painting
/// square dark corners over the (Transparent) window surface.
pub fn has_rounded_window_corners(window: &Window, cx: &App) -> bool {
    let settings = settings_entity(cx).read(cx);
    settings.settings.titlebar_style == TitlebarStyle::Custom
        && !window.is_maximized()
        && !window.is_fullscreen()
        && settings.settings.window_corner_radius > 0.0
}

/// Open the settings file in the default editor
pub fn open_settings_file() {
    let path = get_settings_path();

    if !path.exists() {
        let settings = load_settings();
        if let Err(e) = save_settings(&settings) {
            log::error!("[settings] Failed to write settings file before opening | path={} | error: {:#}", path.display(), e);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let _ = velowork_core::process::spawn_and_reap(
            velowork_core::process::command("open").arg("-t").arg(&path),
        );
    }

    #[cfg(target_os = "linux")]
    {
        let _ = velowork_core::process::spawn_and_reap(
            velowork_core::process::command("xdg-open").arg(&path),
        );
    }

    #[cfg(target_os = "windows")]
    {
        let _ = velowork_core::process::spawn_and_reap(
            velowork_core::process::command("notepad").arg(&path),
        );
    }
}

/// Initialize global settings - call this at app startup
pub fn init_settings(cx: &mut App) -> Entity<SettingsState> {
    let settings = load_settings();
    let entity = cx.new(|_cx| SettingsState::new(settings));
    cx.set_global(GlobalSettings(entity.clone()));
    entity
}
