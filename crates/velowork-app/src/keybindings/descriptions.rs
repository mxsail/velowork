use std::collections::HashMap;

use super::types::{ActionDescription, ActionScope};
use super::{
    AddTab, Cancel, CheckForUpdates, CloseSearch, CloseTerminal, Copy, CyclePanelNext,
    CyclePanelPrev, DuplicateChannel, DuplicateSession, EqualizeLayout, FocusBottomDock,
    FocusCenterDock, FocusDown, FocusLeft, FocusLeftDock, FocusNextTerminal, FocusPrevTerminal,
    FocusRight, FocusRightDock, FocusUp, InstallUpdate, JumpToNextPrompt, JumpToPreviousPrompt,
    LockApp, MinimizeTerminal, NewProject, NewSession, NewWindow, OpenSettingsFile, Paste, Quit,
    ReconnectTerminal, RenameActiveNode, ResetZoom, ScrollDown, ScrollUp, Search, SearchNext,
    SearchPrev, SendEscape, ShowAboutDialog, ShowAiAssistant, ShowAiSettings, ShowCommandPalette,
    ShowHelp, ShowHistoryPanel, ShowImportSessionDialog, ShowKeybindings, ShowLogConsole,
    ShowProfileManager, ShowProjectManageDialog, ShowQuickCommandsPanel, ShowServicesPanel,
    ShowSettings, ShowThemeSelector, ShowTunnelsPanel, ShowUpdateDialog, SplitHorizontal,
    SplitVertical, TerminalInlineAi, ToggleCommandsPanel, ToggleFullscreen, ToggleLeftDock, ToggleLeftDockAutoHide,
    TogglePaneSwitcher, ToggleRightDock, ToggleRightToolbar, ToggleSftpPanel, ZoomIn, ZoomOut,
};

/// Get human-readable descriptions for all actions
pub fn get_action_descriptions() -> HashMap<&'static str, ActionDescription> {
    let mut map = HashMap::new();

    // Global actions
    map.insert(
        "Quit",
        ActionDescription {
            id: "quit",
            name: "Quit",
            description: "Quit Velowork",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(Quit),
        },
    );
    map.insert(
        "LockApp",
        ActionDescription {
            id: "lock_app",
            name: "Lock Application",
            description: "Lock the application (requires master password to unlock)",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(LockApp),
        },
    );
    map.insert(
        "Cancel",
        ActionDescription {
            id: "cancel",
            name: "Cancel",
            description: "Close overlay, cancel rename, or dismiss",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(Cancel),
        },
    );
    map.insert(
        "SendEscape",
        ActionDescription {
            id: "send_escape",
            name: "Send Escape",
            description: "Send escape key to terminal",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: false,
            factory: || Box::new(SendEscape),
        },
    );

    // View / Dock actions
    map.insert(
        "ToggleLeftDock",
        ActionDescription {
            id: "toggle_left_dock",
            name: "Toggle Left Dock",
            description: "Show or hide the left dock panel",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ToggleLeftDock),
        },
    );
    map.insert(
        "ToggleRightDock",
        ActionDescription {
            id: "toggle_right_dock",
            name: "Toggle Right Dock",
            description: "Show or hide the right dock panel",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ToggleRightDock),
        },
    );
    map.insert(
        "ToggleRightToolbar",
        ActionDescription {
            id: "toggle_right_toolbar",
            name: "Toggle Right Toolbar",
            description: "Show or hide the right vertical toolbar strip",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ToggleRightToolbar),
        },
    );
    map.insert(
        "ToggleLeftDockAutoHide",
        ActionDescription {
            id: "toggle_left_dock_auto_hide",
            name: "Toggle Auto-Hide",
            description: "Enable or disable left dock auto-hide mode",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ToggleLeftDockAutoHide),
        },
    );
    map.insert(
        "ToggleCommandsPanel",
        ActionDescription {
            id: "toggle_commands_panel",
            name: "Toggle Commands Panel",
            description: "Show or hide the commands panel (falls back to command palette when no terminal is open)",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ToggleCommandsPanel),
        },
    );
    map.insert(
        "ToggleSftpPanel",
        ActionDescription {
            id: "toggle_sftp_panel",
            name: "Toggle SFTP Panel",
            description: "Show or hide the SFTP file panel for the active session",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ToggleSftpPanel),
        },
    );
    map.insert(
        "ShowTunnelsPanel",
        ActionDescription {
            id: "show_tunnels_panel",
            name: "Toggle Tunnels Panel",
            description: "Show or hide the SSH tunnels panel in the right dock",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowTunnelsPanel),
        },
    );
    map.insert(
        "ShowServicesPanel",
        ActionDescription {
            id: "show_services_panel",
            name: "Toggle Services Panel",
            description: "Show or hide the background services panel in the right dock",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowServicesPanel),
        },
    );
    map.insert(
        "ShowQuickCommandsPanel",
        ActionDescription {
            id: "show_quick_commands_panel",
            name: "Toggle Quick Commands Panel",
            description: "Show or hide the quick commands panel in the right dock",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowQuickCommandsPanel),
        },
    );
    map.insert(
        "ShowHistoryPanel",
        ActionDescription {
            id: "show_history_panel",
            name: "Toggle History Panel",
            description: "Show or hide the command history panel in the right dock",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowHistoryPanel),
        },
    );
    map.insert(
        "ShowAiAssistant",
        ActionDescription {
            id: "show_ai_assistant",
            name: "Show AI Assistant",
            description: "Show or hide the AI Assistant panel in the right dock",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowAiAssistant),
        },
    );
    map.insert(
        "TerminalInlineAi",
        ActionDescription {
            id: "terminal_inline_ai",
            name: "Terminal Inline AI",
            description: "Open inline AI assistant popover in the focused terminal",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(TerminalInlineAi),
        },
    );

    // Fullscreen actions
    map.insert(
        "ToggleFullscreen",
        ActionDescription {
            id: "toggle_fullscreen",
            name: "Toggle Fullscreen",
            description: "Toggle fullscreen mode for focused terminal",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(ToggleFullscreen),
        },
    );

    // Terminal pane actions
    map.insert(
        "SplitVertical",
        ActionDescription {
            id: "split_vertical",
            name: "Split Vertical",
            description: "Split the terminal vertically",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(SplitVertical),
        },
    );
    map.insert(
        "SplitHorizontal",
        ActionDescription {
            id: "split_horizontal",
            name: "Split Horizontal",
            description: "Split the terminal horizontally",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(SplitHorizontal),
        },
    );
    map.insert(
        "AddTab",
        ActionDescription {
            id: "add_tab",
            name: "Add Tab",
            description: "Add a new tab (creates tab group if needed)",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(AddTab),
        },
    );
    map.insert(
        "DuplicateSession",
        ActionDescription {
            id: "duplicate_session",
            name: "Duplicate Session",
            description: "Duplicate the current terminal session in a new tab",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(DuplicateSession),
        },
    );
    map.insert(
        "DuplicateChannel",
        ActionDescription {
            id: "duplicate_channel",
            name: "Duplicate Channel",
            description: "Duplicate SSH channel sharing connection in a new tab",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(DuplicateChannel),
        },
    );
    map.insert(
        "ReconnectTerminal",
        ActionDescription {
            id: "reconnect_terminal",
            name: "Reconnect Terminal",
            description: "Reconnect the active terminal session",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(ReconnectTerminal),
        },
    );
    map.insert(
        "CloseTerminal",
        ActionDescription {
            id: "close_terminal",
            name: "Close Terminal",
            description: "Close the current terminal",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(CloseTerminal),
        },
    );
    map.insert(
        "MinimizeTerminal",
        ActionDescription {
            id: "minimize_terminal",
            name: "Minimize Terminal",
            description: "Minimize/detach the terminal",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: false,
            factory: || Box::new(MinimizeTerminal),
        },
    );
    map.insert(
        "Copy",
        ActionDescription {
            id: "copy",
            name: "Copy",
            description: "Copy selected text",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: false,
            factory: || Box::new(Copy),
        },
    );
    map.insert(
        "Paste",
        ActionDescription {
            id: "paste",
            name: "Paste",
            description: "Paste from clipboard",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: false,
            factory: || Box::new(Paste),
        },
    );
    map.insert(
        "ScrollUp",
        ActionDescription {
            id: "scroll_up",
            name: "Scroll Up",
            description: "Scroll terminal output up",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: false,
            factory: || Box::new(ScrollUp),
        },
    );
    map.insert(
        "ScrollDown",
        ActionDescription {
            id: "scroll_down",
            name: "Scroll Down",
            description: "Scroll terminal output down",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: false,
            factory: || Box::new(ScrollDown),
        },
    );
    map.insert(
        "JumpToPreviousPrompt",
        ActionDescription {
            id: "jump_to_previous_prompt",
            name: "Jump to Previous Prompt",
            description: "Scroll to the previous shell prompt (OSC 133)",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(JumpToPreviousPrompt),
        },
    );
    map.insert(
        "JumpToNextPrompt",
        ActionDescription {
            id: "jump_to_next_prompt",
            name: "Jump to Next Prompt",
            description: "Scroll forward to the next shell prompt (OSC 133)",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(JumpToNextPrompt),
        },
    );

    // Zoom actions
    map.insert(
        "ZoomIn",
        ActionDescription {
            id: "zoom_in",
            name: "Zoom In",
            description: "Increase terminal font size",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(ZoomIn),
        },
    );
    map.insert(
        "ZoomOut",
        ActionDescription {
            id: "zoom_out",
            name: "Zoom Out",
            description: "Decrease terminal font size",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(ZoomOut),
        },
    );
    map.insert(
        "ResetZoom",
        ActionDescription {
            id: "reset_zoom",
            name: "Reset Zoom",
            description: "Reset terminal font size to default",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(ResetZoom),
        },
    );

    // Search actions
    map.insert(
        "Search",
        ActionDescription {
            id: "search",
            name: "Search",
            description: "Open search in terminal",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: true,
            factory: || Box::new(Search),
        },
    );
    map.insert(
        "SearchNext",
        ActionDescription {
            id: "search_next",
            name: "Search Next",
            description: "Find next search match",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: false,
            factory: || Box::new(SearchNext),
        },
    );
    map.insert(
        "SearchPrev",
        ActionDescription {
            id: "search_prev",
            name: "Search Previous",
            description: "Find previous search match",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: false,
            factory: || Box::new(SearchPrev),
        },
    );
    map.insert(
        "CloseSearch",
        ActionDescription {
            id: "close_search",
            name: "Close Search",
            description: "Close search panel",
            category: "Terminal",
            scope: ActionScope::Terminal,
            show_in_palette: false,
            factory: || Box::new(CloseSearch),
        },
    );

    // Navigation actions (Keybinding only / Context)
    map.insert(
        "FocusLeft",
        ActionDescription {
            id: "focus_left",
            name: "Focus Left",
            description: "Move focus to the left terminal",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(FocusLeft),
        },
    );
    map.insert(
        "FocusRight",
        ActionDescription {
            id: "focus_right",
            name: "Focus Right",
            description: "Move focus to the right terminal",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(FocusRight),
        },
    );
    map.insert(
        "FocusUp",
        ActionDescription {
            id: "focus_up",
            name: "Focus Up",
            description: "Move focus to the terminal above",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(FocusUp),
        },
    );
    map.insert(
        "FocusDown",
        ActionDescription {
            id: "focus_down",
            name: "Focus Down",
            description: "Move focus to the terminal below",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(FocusDown),
        },
    );
    map.insert(
        "FocusNextTerminal",
        ActionDescription {
            id: "focus_next_terminal",
            name: "Focus Next Terminal",
            description: "Move focus to the next terminal",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(FocusNextTerminal),
        },
    );
    map.insert(
        "FocusPrevTerminal",
        ActionDescription {
            id: "focus_prev_terminal",
            name: "Focus Previous Terminal",
            description: "Move focus to the previous terminal",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(FocusPrevTerminal),
        },
    );
    map.insert(
        "TogglePaneSwitcher",
        ActionDescription {
            id: "toggle_pane_switcher",
            name: "Display Panes",
            description: "Show numbered overlays on panes, press a digit to focus",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(TogglePaneSwitcher),
        },
    );

    // Project & Session actions
    map.insert(
        "NewProject",
        ActionDescription {
            id: "new_project",
            name: "New Project",
            description: "Create a new project",
            category: "Project",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(NewProject),
        },
    );
    map.insert(
        "NewSession",
        ActionDescription {
            id: "new_session",
            name: "New Session",
            description: "Create a new SSH session",
            category: "Session",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(NewSession),
        },
    );
    map.insert(
        "ShowProjectManageDialog",
        ActionDescription {
            id: "show_project_manage_dialog",
            name: "Project Manager",
            description: "Open project manager to organize projects",
            category: "Project",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowProjectManageDialog),
        },
    );
    map.insert(
        "ShowImportSessionDialog",
        ActionDescription {
            id: "show_import_session_dialog",
            name: "Import Sessions",
            description: "Import sessions from SSH Config, Xshell, MobaXterm, or FinalShell",
            category: "Project",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowImportSessionDialog),
        },
    );

    // Global dialogs & utilities
    map.insert(
        "ShowKeybindings",
        ActionDescription {
            id: "show_keybindings",
            name: "Show Keybindings",
            description: "Display keybinding help",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowKeybindings),
        },
    );
    map.insert(
        "ShowProfileManager",
        ActionDescription {
            id: "show_profile_manager",
            name: "Profile Manager",
            description: "Open profile manager to switch, create, or delete profiles",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowProfileManager),
        },
    );
    map.insert(
        "ShowThemeSelector",
        ActionDescription {
            id: "show_theme_selector",
            name: "Theme Selector",
            description: "Open theme selector to change appearance",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowThemeSelector),
        },
    );
    map.insert(
        "ShowCommandPalette",
        ActionDescription {
            id: "show_command_palette",
            name: "Command Palette",
            description: "Open command palette for quick access to all commands",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(ShowCommandPalette),
        },
    );
    map.insert(
        "ShowSettings",
        ActionDescription {
            id: "show_settings",
            name: "Settings",
            description: "Open settings panel",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowSettings),
        },
    );
    map.insert(
        "ShowAiSettings",
        ActionDescription {
            id: "show_ai_settings",
            name: "AI Settings",
            description: "Open settings panel to AI configuration",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowAiSettings),
        },
    );
    map.insert(
        "ShowUpdateDialog",
        ActionDescription {
            id: "show_update_dialog",
            name: "Update",
            description: "Open the update dialog",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowUpdateDialog),
        },
    );
    map.insert(
        "ShowHelp",
        ActionDescription {
            id: "show_help",
            name: "Help",
            description: "Open the help dialog",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowHelp),
        },
    );
    map.insert(
        "ShowAboutDialog",
        ActionDescription {
            id: "show_about_dialog",
            name: "About Velowork",
            description: "Open the about dialog",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowAboutDialog),
        },
    );
    map.insert(
        "OpenSettingsFile",
        ActionDescription {
            id: "open_settings_file",
            name: "Open Settings File",
            description: "Open settings JSON file in default editor",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(OpenSettingsFile),
        },
    );
    map.insert(
        "ShowLogConsole",
        ActionDescription {
            id: "show_log_console",
            name: "Log Console",
            description: "Live log viewer with runtime filter",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowLogConsole),
        },
    );
    map.insert(
        "CheckForUpdates",
        ActionDescription {
            id: "check_for_updates",
            name: "Check for Updates",
            description: "Check for a new version of Velowork",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(CheckForUpdates),
        },
    );
    map.insert(
        "InstallUpdate",
        ActionDescription {
            id: "install_update",
            name: "Install Update",
            description: "Install a downloaded update",
            category: "Global",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(InstallUpdate),
        },
    );

    // Layout & Window actions
    map.insert(
        "EqualizeLayout",
        ActionDescription {
            id: "equalize_layout",
            name: "Equalize Layout",
            description: "Equalize split pane sizes in the active terminal session",
            category: "Terminal",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(EqualizeLayout),
        },
    );
    map.insert(
        "NewWindow",
        ActionDescription {
            id: "new_window",
            name: "New Window",
            description: "Open an additional window onto the workspace",
            category: "Window",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(NewWindow),
        },
    );

    // Panel-scoped semantic action
    map.insert(
        "RenameActiveNode",
        ActionDescription {
            id: "rename_active_node",
            name: "Rename Node",
            description: "Rename the currently focused node (session/tunnel/service/quick command/sftp item)",
            category: "Panel",
            scope: ActionScope::Panel,
            show_in_palette: false,
            factory: || Box::new(RenameActiveNode),
        },
    );
    map.insert(
        "CyclePanelNext",
        ActionDescription {
            id: "cycle_panel_next",
            name: "Cycle Panel Next",
            description: "Switch to next dock panel",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(CyclePanelNext),
        },
    );
    map.insert(
        "CyclePanelPrev",
        ActionDescription {
            id: "cycle_panel_prev",
            name: "Cycle Panel Previous",
            description: "Switch to previous dock panel",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(CyclePanelPrev),
        },
    );
    map.insert(
        "FocusLeftDock",
        ActionDescription {
            id: "focus_left_dock",
            name: "Focus Left Dock",
            description: "Focus on the left dock",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(FocusLeftDock),
        },
    );
    map.insert(
        "FocusCenterDock",
        ActionDescription {
            id: "focus_center_dock",
            name: "Focus Center Dock",
            description: "Focus on the center dock",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(FocusCenterDock),
        },
    );
    map.insert(
        "FocusRightDock",
        ActionDescription {
            id: "focus_right_dock",
            name: "Focus Right Dock",
            description: "Focus on the right dock",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(FocusRightDock),
        },
    );
    map.insert(
        "FocusBottomDock",
        ActionDescription {
            id: "focus_bottom_dock",
            name: "Focus Bottom Dock",
            description: "Focus on the bottom dock",
            category: "Navigation",
            scope: ActionScope::Global,
            show_in_palette: false,
            factory: || Box::new(FocusBottomDock),
        },
    );

    map
}
