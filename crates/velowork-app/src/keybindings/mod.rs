mod config;
mod descriptions;
mod types;

use gpui::*;
use parking_lot::RwLock;
use velowork_i18n::i18n;

pub use config::{
    get_keybindings_path, load_keybindings, save_keybindings,
    KeybindingConfig,
};
pub use descriptions::get_action_descriptions;
#[allow(unused_imports)]
pub use types::{ActionDescription, ConflictKind, KeybindingConflict, KeybindingEntry};

// App-level actions (handled by window view, overlay manager, sidebar)
actions!(
    velowork,
    [
        Quit,
        About,
        ReloadKeybindings,
        ToggleLeftDock,
        ToggleRightDock,
        ToggleRightToolbar,
        ToggleLeftDockAutoHide,
        NewProject,
        CloseWindow,
        ScrollUp,
        ScrollDown,
        ShowKeybindings,
        ShowThemeSelector,
        ShowCommandPalette,
        ShowSettings,
        ShowUpdateDialog,
        ShowAboutDialog,
        ShowHelp,
        OpenSettingsFile,
        CheckForUpdates,
        InstallUpdate,
        TogglePaneSwitcher,
        ShowLogConsole,
        EqualizeLayout,
        ShowProfileManager,
        NewWindow,
        ToggleSftpPanel,
        ToggleCommandsPanel,
        FocusLeftDock,
        FocusCenterDock,
        FocusRightDock,
        FocusBottomDock,
        CyclePanelNext,
        CyclePanelPrev,
        LockApp,
        ShowTunnelsPanel,
        ShowServicesPanel,
        ShowQuickCommandsPanel,
        ShowHistoryPanel,
        ShowAiAssistant,
        ShowAiSettings,
        // 会话与项目管理 Action
        NewSession,
        ShowProjectManageDialog,
        ShowImportSessionDialog,
        // 面板内语义 Action：仅在具体面板 Entity 上注册 on_action，不注册为全局 Action。
        RenameActiveNode,
    ]
);

// Terminal-specific actions (defined in velowork-views-terminal crate)
pub use velowork_views_terminal::actions::{
    SendEscape, SplitVertical, SplitHorizontal, AddTab, CloseTerminal,
    MinimizeTerminal, FocusNextTerminal, FocusPrevTerminal,
    FocusLeft, FocusRight, FocusUp, FocusDown,
    Copy, Paste, Search, SearchNext, SearchPrev, CloseSearch,
    SendTab, SendBacktab, ZoomIn, ZoomOut, ResetZoom,
    ToggleFullscreen,
    JumpToPreviousPrompt, JumpToNextPrompt,
    DuplicateSession, DuplicateChannel, ReconnectTerminal,
    TerminalInlineAi,
};

// Generic cancel action for overlays is `velowork_ui::Cancel`.
pub use velowork_ui::Cancel;

/// Global keybinding configuration (thread-safe)
static KEYBINDING_CONFIG: RwLock<Option<KeybindingConfig>> = RwLock::new(None);

/// Get a read guard to the current keybinding configuration
///
/// Returns a guard that dereferences to KeybindingConfig.
/// The guard must be held for the duration of access.
pub fn get_config() -> impl std::ops::Deref<Target = KeybindingConfig> {
    parking_lot::RwLockReadGuard::map(KEYBINDING_CONFIG.read(), |opt| {
        #[allow(
            clippy::expect_used,
            reason = "init_keybindings() runs at startup before any caller reaches get_config()"
        )]
        opt.as_ref().expect("Keybinding config not initialized")
    })
}

/// Reset keybindings to defaults and save
pub fn reset_to_defaults() -> anyhow::Result<()> {
    let config = KeybindingConfig::defaults();
    save_keybindings(&config)?;
    *KEYBINDING_CONFIG.write() = Some(config);
    Ok(())
}

/// Convert a GPUI Keystroke to the config string format (e.g., "cmd-shift-d")
/// GPUI's unparse() uses "super-" on Linux for the platform modifier,
/// but our config format uses "cmd-" for cross-platform consistency.
pub fn keystroke_to_config_string(keystroke: &gpui::Keystroke) -> String {
    let unparsed = keystroke.unparse();
    // Normalize platform modifier names to "cmd-" for config consistency
    unparsed
        .replace("super-", "cmd-")
        .replace("win-", "cmd-")
}

/// Reload keybindings: update global config, save to disk, and re-register with GPUI.
/// Call this after modifying the config via get_config_mut() or update_config().
pub fn reload_keybindings(cx: &mut App) {
    let config = {
        KEYBINDING_CONFIG.read().as_ref().cloned().unwrap_or_default()
    };

    // Save to disk
    if let Err(e) = save_keybindings(&config) {
        log::error!("[keybindings] Failed to save keybindings | error: {:#}", e);
    }

    // Clear existing bindings and re-register everything
    cx.clear_key_bindings();
    register_bindings_from_config(cx, &config);

    // Re-register essential non-overridable bindings
    cx.bind_keys([
        KeyBinding::new("tab", SendTab, Some("TerminalPane")),
        KeyBinding::new("shift-tab", SendBacktab, Some("TerminalPane")),
    ]);

    cx.bind_keys([
        KeyBinding::new("escape", Cancel, None),
        KeyBinding::new("escape", SendEscape, Some("TerminalPane")),
        KeyBinding::new("escape", CloseSearch, Some("SearchBar")),
        KeyBinding::new("escape", velowork_views_terminal::actions::Cancel, Some("TerminalRename")),
        KeyBinding::new("escape", velowork_ui::Cancel, Some("ContextMenu")),
        KeyBinding::new("escape", velowork_ui::Cancel, Some("FolderContextMenu")),
        KeyBinding::new("escape", velowork_ui::Cancel, Some("RenameDirectoryDialog")),
        KeyBinding::new("escape", velowork_views_terminal::actions::Cancel, Some("ShellSelectorOverlay")),
    ]);
}

/// Get a mutable reference to the global keybinding configuration.
/// After modifying, call reload_keybindings(cx) to apply changes.
pub fn update_config(f: impl FnOnce(&mut KeybindingConfig)) {
    let mut guard = KEYBINDING_CONFIG.write();
    if let Some(config) = guard.as_mut() {
        f(config);
    }
}

/// Register keybindings for the application from configuration
pub fn register_keybindings(cx: &mut App) {
    // Load configuration
    let config = load_keybindings();

    // Check for conflicts and warn
    let conflicts = config.detect_conflicts();
    for conflict in &conflicts {
        log::warn!("[keybindings] Keybinding conflict detected: {}", conflict);
    }

    // Store config globally (thread-safe)
    *KEYBINDING_CONFIG.write() = Some(config.clone());

    // Register bindings from config
    register_bindings_from_config(cx, &config);

    // Register essential terminal keybindings that should not be overridden
    // Tab/Shift+Tab must be captured to prevent GPUI's focus navigation from consuming them
    cx.bind_keys([
        KeyBinding::new("tab", SendTab, Some("TerminalPane")),
        KeyBinding::new("shift-tab", SendBacktab, Some("TerminalPane")),
    ]);

    // Register escape keybindings with context-based precedence:
    //   Global:             escape → Cancel        (overlays, sidebar rename)
    //   TerminalPane:       escape → SendEscape    (send 0x1b to PTY)
    //   SearchBar:          escape → CloseSearch   (close search, deeper than TerminalPane)
    //   TerminalRename:     escape → Cancel        (cancel rename, deeper than TerminalPane)
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, None),
        KeyBinding::new("escape", SendEscape, Some("TerminalPane")),
        KeyBinding::new("escape", CloseSearch, Some("SearchBar")),
        // Terminal rename uses the crate's Cancel action
        KeyBinding::new("escape", velowork_views_terminal::actions::Cancel, Some("TerminalRename")),
        // velowork-ui crate Cancel actions for context menus
        KeyBinding::new("escape", velowork_ui::Cancel, Some("ContextMenu")),
        KeyBinding::new("escape", velowork_ui::Cancel, Some("FolderContextMenu")),
        KeyBinding::new("escape", velowork_ui::Cancel, Some("RenameDirectoryDialog")),
        // velowork-views-terminal crate Cancel for shell selector
        KeyBinding::new("escape", velowork_views_terminal::actions::Cancel, Some("ShellSelectorOverlay")),
    ]);

    cx.set_global(velowork_views_terminal::welcome::GlobalShortcutProvider(
        std::sync::Arc::new(shortcut_for_action),
    ));
}

/// Register keybindings from a configuration
fn register_bindings_from_config(cx: &mut App, config: &KeybindingConfig) {
    // Collect all keybindings
    let mut bindings: Vec<KeyBinding> = Vec::new();

    for (action, entries) in &config.bindings {
        for entry in entries {
            if !entry.enabled {
                continue;
            }

            let context = entry.context.as_deref();

            // Map action name to action type
            if let Some(binding) = create_keybinding(action, &entry.keystroke, context) {
                bindings.push(binding);
            }
        }
    }

    // Register all bindings
    cx.bind_keys(bindings);
}

/// Create a KeyBinding from action name, keystroke, and context
fn create_keybinding(action: &str, keystroke: &str, context: Option<&str>) -> Option<KeyBinding> {
    // Map action names to actual actions
    match action {
        "Quit" => Some(KeyBinding::new(keystroke, Quit, context)),
        "Cancel" => Some(KeyBinding::new(keystroke, Cancel, context)),
        "RenameActiveNode" => Some(KeyBinding::new(keystroke, RenameActiveNode, context)),
        "SendEscape" => Some(KeyBinding::new(keystroke, SendEscape, context)),
        "ToggleLeftDock" => Some(KeyBinding::new(keystroke, ToggleLeftDock, context)),
        "ToggleRightDock" => Some(KeyBinding::new(keystroke, ToggleRightDock, context)),
        "ToggleRightToolbar" => Some(KeyBinding::new(keystroke, ToggleRightToolbar, context)),
        "ToggleLeftDockAutoHide" => Some(KeyBinding::new(keystroke, ToggleLeftDockAutoHide, context)),
        "FocusLeftDock" => Some(KeyBinding::new(keystroke, FocusLeftDock, context)),
        "ToggleFullscreen" => Some(KeyBinding::new(keystroke, ToggleFullscreen, context)),
        "SplitVertical" => Some(KeyBinding::new(keystroke, SplitVertical, context)),
        "SplitHorizontal" => Some(KeyBinding::new(keystroke, SplitHorizontal, context)),
        "AddTab" => Some(KeyBinding::new(keystroke, AddTab, context)),
        "DuplicateSession" => Some(KeyBinding::new(keystroke, DuplicateSession, context)),
        "DuplicateChannel" => Some(KeyBinding::new(keystroke, DuplicateChannel, context)),
        "ReconnectTerminal" => Some(KeyBinding::new(keystroke, ReconnectTerminal, context)),
        "CloseTerminal" => Some(KeyBinding::new(keystroke, CloseTerminal, context)),
        "MinimizeTerminal" => Some(KeyBinding::new(keystroke, MinimizeTerminal, context)),
        "FocusNextTerminal" => Some(KeyBinding::new(keystroke, FocusNextTerminal, context)),
        "FocusPrevTerminal" => Some(KeyBinding::new(keystroke, FocusPrevTerminal, context)),
        "FocusLeft" => Some(KeyBinding::new(keystroke, FocusLeft, context)),
        "FocusRight" => Some(KeyBinding::new(keystroke, FocusRight, context)),
        "FocusUp" => Some(KeyBinding::new(keystroke, FocusUp, context)),
        "FocusDown" => Some(KeyBinding::new(keystroke, FocusDown, context)),
        "NewProject" => Some(KeyBinding::new(keystroke, NewProject, context)),
        "Copy" => Some(KeyBinding::new(keystroke, Copy, context)),
        "Paste" => Some(KeyBinding::new(keystroke, Paste, context)),
        "ScrollUp" => Some(KeyBinding::new(keystroke, ScrollUp, context)),
        "ScrollDown" => Some(KeyBinding::new(keystroke, ScrollDown, context)),
        "Search" => Some(KeyBinding::new(keystroke, Search, context)),
        "SearchNext" => Some(KeyBinding::new(keystroke, SearchNext, context)),
        "SearchPrev" => Some(KeyBinding::new(keystroke, SearchPrev, context)),
        "JumpToPreviousPrompt" => Some(KeyBinding::new(keystroke, JumpToPreviousPrompt, context)),
        "JumpToNextPrompt" => Some(KeyBinding::new(keystroke, JumpToNextPrompt, context)),
        "CloseSearch" => Some(KeyBinding::new(keystroke, CloseSearch, context)),
        "ShowKeybindings" => Some(KeyBinding::new(keystroke, ShowKeybindings, context)),
        "ShowThemeSelector" => Some(KeyBinding::new(keystroke, ShowThemeSelector, context)),
        "ShowCommandPalette" => Some(KeyBinding::new(keystroke, ShowCommandPalette, context)),
        "ShowSettings" => Some(KeyBinding::new(keystroke, ShowSettings, context)),
        "ShowAiSettings" => Some(KeyBinding::new(keystroke, ShowAiSettings, context)),
        "OpenSettingsFile" => Some(KeyBinding::new(keystroke, OpenSettingsFile, context)),
        "ShowProjectSwitcher" => Some(KeyBinding::new(keystroke, ShowProjectManageDialog, context)),
        "CheckForUpdates" => Some(KeyBinding::new(keystroke, CheckForUpdates, context)),
        "InstallUpdate" => Some(KeyBinding::new(keystroke, InstallUpdate, context)),
        "TogglePaneSwitcher" => Some(KeyBinding::new(keystroke, TogglePaneSwitcher, context)),
        "EqualizeLayout" => Some(KeyBinding::new(keystroke, EqualizeLayout, context)),
        "ShowProfileManager" => Some(KeyBinding::new(keystroke, ShowProfileManager, context)),
        "ShowLogConsole" => Some(KeyBinding::new(keystroke, ShowLogConsole, context)),
        "NewWindow" => Some(KeyBinding::new(keystroke, NewWindow, context)),
        "FocusCenterDock" => Some(KeyBinding::new(keystroke, FocusCenterDock, context)),
        "FocusRightDock" => Some(KeyBinding::new(keystroke, FocusRightDock, context)),
        "FocusBottomDock" => Some(KeyBinding::new(keystroke, FocusBottomDock, context)),
        "CyclePanelNext" => Some(KeyBinding::new(keystroke, CyclePanelNext, context)),
        "CyclePanelPrev" => Some(KeyBinding::new(keystroke, CyclePanelPrev, context)),
        "LockApp" => Some(KeyBinding::new(keystroke, LockApp, context)),
        "ShowTunnelsPanel" => Some(KeyBinding::new(keystroke, ShowTunnelsPanel, context)),
        "ShowServicesPanel" => Some(KeyBinding::new(keystroke, ShowServicesPanel, context)),
        "ShowQuickCommandsPanel" => Some(KeyBinding::new(keystroke, ShowQuickCommandsPanel, context)),
        "ShowHistoryPanel" => Some(KeyBinding::new(keystroke, ShowHistoryPanel, context)),
        "ShowAiAssistant" => Some(KeyBinding::new(keystroke, ShowAiAssistant, context)),
        "TerminalInlineAi" => Some(KeyBinding::new(keystroke, TerminalInlineAi, context)),
        "ToggleSftpPanel" => Some(KeyBinding::new(keystroke, ToggleSftpPanel, context)),
        "ToggleCommandsPanel" => Some(KeyBinding::new(keystroke, ToggleCommandsPanel, context)),
        "NewSession" => Some(KeyBinding::new(keystroke, NewSession, context)),
        "ShowImportSessionDialog" => Some(KeyBinding::new(keystroke, ShowImportSessionDialog, context)),
        "ShowProjectManageDialog" => Some(KeyBinding::new(keystroke, ShowProjectManageDialog, context)),
        "ShowAboutDialog" => Some(KeyBinding::new(keystroke, ShowAboutDialog, context)),
        "ShowHelp" => Some(KeyBinding::new(keystroke, ShowHelp, context)),
        "CloseWindow" => Some(KeyBinding::new(keystroke, CloseWindow, context)),
        "ZoomIn" => Some(KeyBinding::new(keystroke, ZoomIn, context)),
        "ZoomOut" => Some(KeyBinding::new(keystroke, ZoomOut, context)),
        "ResetZoom" => Some(KeyBinding::new(keystroke, ResetZoom, context)),
        _ => {
            log::warn!("Unknown action in keybinding config: {}", action);
            None
        }
    }
}

/// Helper to check if a specific keybinding entry is customized relative to defaults.
pub fn is_entry_customized(
    action: &str,
    entry_index: usize,
    entry: &KeybindingEntry,
    defaults: &KeybindingConfig,
) -> bool {
    if let Some(default_entries) = defaults.bindings.get(action) {
        if let Some(default_entry) = default_entries.get(entry_index) {
            return entry != default_entry;
        }
    }
    true
}

/// Helper to check if an entry belongs to the current platform.
/// Standard default macOS entries (`cmd-...`) are hidden on Linux/Windows.
/// Standard default Linux/Windows entries (`ctrl-...` without `cmd-...`) are hidden on macOS.
/// Any customized entry (modified by the user on this platform) is always shown.
pub fn is_entry_matching_platform(
    entry: &KeybindingEntry,
    is_custom: bool,
) -> bool {
    if is_custom || entry.keystroke == "unset" || entry.keystroke.is_empty() {
        return true;
    }
    let ks = entry.keystroke.to_lowercase();
    if cfg!(target_os = "macos") {
        if ks.contains("ctrl-") && !ks.contains("cmd-") {
            return false;
        }
    } else if ks.contains("cmd-") {
        return false;
    }
    true
}

/// Helper to convert CamelCase or identifier to snake_case id (e.g. "ToggleLeftDock" -> "toggle_left_dock")
pub fn action_to_id(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut chars = name.chars().peekable();
    let mut prev_is_upper = false;
    let mut prev_is_underscore = false;

    while let Some(c) = chars.next() {
        if c == '/' || c == '-' || c == ' ' {
            if !prev_is_underscore && !out.is_empty() {
                out.push('_');
                prev_is_underscore = true;
            }
            prev_is_upper = false;
            continue;
        }
        if c.is_uppercase() {
            let next_is_lower = chars.peek().map_or(false, |n| n.is_lowercase());
            if !out.is_empty() && !prev_is_underscore && (!prev_is_upper || next_is_lower) {
                out.push('_');
            }
            out.extend(c.to_lowercase());
            prev_is_upper = true;
            prev_is_underscore = false;
        } else {
            out.push(c);
            prev_is_upper = false;
            prev_is_underscore = c == '_';
        }
    }
    out
}

/// Safely translate action name without leaking raw i18n keys
pub fn translate_action_name(raw_name: &str, action_key: &str, cx: &App) -> String {
    // 1. Try commands.<id>.label where id is derived from action_key or raw_name
    let id_from_key = action_to_id(action_key);
    let key1 = format!("commands.{}.label", id_from_key);
    let trans1 = i18n!(cx, key1.as_str());
    if trans1 != key1 && !trans1.is_empty() {
        return trans1;
    }

    let id_from_name = action_to_id(raw_name);
    if id_from_name != id_from_key {
        let key2 = format!("commands.{}.label", id_from_name);
        let trans2 = i18n!(cx, key2.as_str());
        if trans2 != key2 && !trans2.is_empty() {
            return trans2;
        }
    }

    // 2. Backward compatibility fallback: commands.<raw_name> or commands.<action_key>
    let legacy_key1 = format!("commands.{}", raw_name);
    let trans_legacy1 = i18n!(cx, legacy_key1.as_str());
    if trans_legacy1 != legacy_key1 && !trans_legacy1.is_empty() {
        return trans_legacy1;
    }

    let legacy_key2 = format!("commands.{}", action_key);
    let trans_legacy2 = i18n!(cx, legacy_key2.as_str());
    if trans_legacy2 != legacy_key2 && !trans_legacy2.is_empty() {
        return trans_legacy2;
    }

    raw_name.to_string()
}

/// Safely translate action description without leaking raw i18n keys
pub fn translate_action_desc(raw_desc: &str, cx: &App) -> String {
    if raw_desc.is_empty() {
        return String::new();
    }
    // If passed a snake_case id
    let key = format!("commands.{}.description", raw_desc);
    let trans = i18n!(cx, key.as_str());
    if trans != key && !trans.is_empty() {
        return trans;
    }

    // Backward compatibility fallback: commands.<raw_desc>
    let legacy_key = format!("commands.{}", raw_desc);
    let trans_legacy = i18n!(cx, legacy_key.as_str());
    if trans_legacy != legacy_key && !trans_legacy.is_empty() {
        trans_legacy
    } else {
        raw_desc.to_string()
    }
}

/// Safely translate action description when command id is known
pub fn translate_action_desc_for_id(action_id: &str, raw_desc: &str, cx: &App) -> String {
    if !action_id.is_empty() {
        let key = format!("commands.{}.description", action_id);
        let trans = i18n!(cx, key.as_str());
        if trans != key && !trans.is_empty() {
            return trans;
        }
    }
    translate_action_desc(raw_desc, cx)
}

/// Helper to get i18n display name for categories.
pub fn translate_category(category: &str, cx: &App) -> String {
    let cat_id = category.to_lowercase();
    let cat_key = format!("commands.category.{}", cat_id);
    let trans = i18n!(cx, cat_key.as_str());
    if trans != cat_key && !trans.is_empty() {
        return trans;
    }

    match cat_id.as_str() {
        "all" => i18n!(cx, "keybindings.tab_all"),
        "global" => i18n!(cx, "keybindings.tab_global"),
        "terminal" => i18n!(cx, "keybindings.tab_terminal"),
        "navigation" => i18n!(cx, "keybindings.tab_navigation"),
        "view" => i18n!(cx, "keybindings.tab_view"),
        "search" => i18n!(cx, "keybindings.tab_search"),
        "fullscreen" => i18n!(cx, "keybindings.tab_fullscreen"),
        "project" => i18n!(cx, "keybindings.tab_project"),
        "other" => i18n!(cx, "keybindings.tab_other"),
        _ => {
            let cat_key = format!("commands.cat.{}", category);
            let trans = i18n!(cx, cat_key.as_str());
            if trans != cat_key && !trans.is_empty() {
                trans
            } else {
                category.to_string()
            }
        }
    }
}

/// The shortcut currently bound to `action`, dynamically retrieved from
/// the current runtime `KeybindingConfig` and formatted for display.
///
/// Returns `None` if the action is unbound, disabled, or if config is not initialized.
pub fn shortcut_for_action(action: &str) -> Option<String> {
    let guard = KEYBINDING_CONFIG.read();
    let config = guard.as_ref()?;
    let defaults = KeybindingConfig::defaults();
    let entries = config.bindings.get(action)?;
    let chosen = entries
        .iter()
        .enumerate()
        .find(|(idx, e)| {
            e.enabled && is_entry_matching_platform(e, is_entry_customized(action, *idx, e, &defaults))
        })
        .map(|(_, e)| e)
        .or_else(|| entries.iter().find(|e| e.enabled))?;

    if chosen.keystroke == "unset" || chosen.keystroke.is_empty() {
        None
    } else {
        Some(format_keystroke(&chosen.keystroke))
    }
}

/// The shortcut currently bound to `action`, dynamically retrieved from
/// the current runtime `KeybindingConfig` and split into individual key
/// strings suitable for badge rendering (e.g. `["Ctrl", "Shift", "P"]` or `["⌘", "⇧", "P"]`).
///
/// Returns `None` if the action is unbound, disabled, or if config is not initialized.
pub fn shortcut_keys_for_action(action: &str) -> Option<Vec<String>> {
    let guard = KEYBINDING_CONFIG.read();
    let config = guard.as_ref()?;
    let defaults = KeybindingConfig::defaults();
    let entries = config.bindings.get(action)?;
    let chosen = entries
        .iter()
        .enumerate()
        .find(|(idx, e)| {
            e.enabled && is_entry_matching_platform(e, is_entry_customized(action, *idx, e, &defaults))
        })
        .map(|(_, e)| e)
        .or_else(|| entries.iter().find(|e| e.enabled))?;

    if chosen.keystroke == "unset" || chosen.keystroke.is_empty() {
        None
    } else {
        Some(format_keystroke_to_parts(&chosen.keystroke))
    }
}

/// Format a keystroke for display (convert to human-readable format).
pub fn format_keystroke(keystroke: &str) -> String {
    if keystroke.contains(' ') {
        return keystroke
            .split_whitespace()
            .map(format_single_keystroke)
            .collect::<Vec<_>>()
            .join(" ");
    }
    format_single_keystroke(keystroke)
}

/// Format a keystroke into individual key strings for badge rendering.
pub fn format_keystroke_to_parts(keystroke: &str) -> Vec<String> {
    if keystroke.contains(' ') {
        return keystroke
            .split_whitespace()
            .flat_map(format_single_keystroke_parts)
            .collect();
    }
    format_single_keystroke_parts(keystroke)
}

/// Split and format a single keystroke into its component key names.
pub fn format_single_keystroke_parts(k: &str) -> Vec<String> {
    if cfg!(target_os = "macos") {
        k.split('-')
            .map(|part| match part.to_lowercase().as_str() {
                "cmd" => "⌘".to_string(),
                "shift" => "⇧".to_string(),
                "alt" => "⌥".to_string(),
                "ctrl" => "⌃".to_string(),
                "escape" => "Esc".to_string(),
                "pageup" => "PgUp".to_string(),
                "pagedown" => "PgDn".to_string(),
                "enter" => "Enter".to_string(),
                "space" => "Space".to_string(),
                "tab" => "Tab".to_string(),
                "left" => "←".to_string(),
                "right" => "→".to_string(),
                "up" => "↑".to_string(),
                "down" => "↓".to_string(),
                p if p.starts_with('f') && p.len() > 1 && p[1..].chars().all(|c| c.is_ascii_digit()) => {
                    p.to_uppercase()
                }
                other => {
                    if other.len() == 1 {
                        other.to_uppercase()
                    } else {
                        let mut chars = other.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                        }
                    }
                }
            })
            .collect()
    } else {
        k.split('-')
            .map(|part| match part.to_lowercase().as_str() {
                "ctrl" => "Ctrl".to_string(),
                "shift" => "Shift".to_string(),
                "alt" => "Alt".to_string(),
                "cmd" | "super" | "win" => "Super".to_string(),
                "escape" => "Esc".to_string(),
                "pageup" => "PgUp".to_string(),
                "pagedown" => "PgDn".to_string(),
                "enter" => "Enter".to_string(),
                "space" => "Space".to_string(),
                "tab" => "Tab".to_string(),
                "left" => "←".to_string(),
                "right" => "→".to_string(),
                "up" => "↑".to_string(),
                "down" => "↓".to_string(),
                p if p.starts_with('f') && p.len() > 1 && p[1..].chars().all(|c| c.is_ascii_digit()) => {
                    p.to_uppercase()
                }
                other => {
                    if other.len() == 1 {
                        other.to_uppercase()
                    } else {
                        let mut chars = other.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                        }
                    }
                }
            })
            .collect()
    }
}

fn format_single_keystroke(k: &str) -> String {
    let parts = format_single_keystroke_parts(k);
    if cfg!(target_os = "macos") {
        parts.join("")
    } else {
        parts.join("+")
    }
}

