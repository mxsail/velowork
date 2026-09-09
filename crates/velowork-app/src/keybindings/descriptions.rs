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
    SplitVertical, ToggleCommandsPanel, ToggleFullscreen, ToggleLeftDock, ToggleLeftDockAutoHide,
    TogglePaneSwitcher, ToggleRightDock, ToggleSftpPanel, ZoomIn, ZoomOut,
};

/// Get human-readable descriptions for all actions
pub fn get_action_descriptions() -> HashMap<&'static str, ActionDescription> {
    let mut map = HashMap::new();

    // Global actions
    map.insert(
        "Quit",
        ActionDescription {
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
            name: "Toggle Right Dock",
            description: "Show or hide the right dock panel",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ToggleRightDock),
        },
    );
    map.insert(
        "ToggleLeftDockAutoHide",
        ActionDescription {
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
            name: "Show AI Assistant",
            description: "Show or hide the AI Assistant panel in the right dock",
            category: "View",
            scope: ActionScope::Global,
            show_in_palette: true,
            factory: || Box::new(ShowAiAssistant),
        },
    );

    // Fullscreen actions
    map.insert(
        "ToggleFullscreen",
        ActionDescription {
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
