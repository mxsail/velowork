//! Overlay management utilities and OverlayManager Entity.
//!
//! Provides traits, helpers, and a centralized manager for modal overlay components
//! with consistent toggle and close behavior.

use gpui::*;
use std::sync::Arc;
use velowork_i18n::i18n;

use crate::settings::settings_entity;


use crate::terminal::shell_config::ShellType;
use crate::views::overlays::pickers::command_palette::{CommandPalette, CommandPaletteEvent};
use crate::views::overlays::keybindings_help::{KeybindingsHelp, KeybindingsHelpEvent};
use crate::views::overlays::dialogs::project_add_dialog::{AddProjectDialog, AddProjectDialogEvent};
use crate::views::overlays::dialogs::project_manage_dialog::{ManageProjectsDialog, ManageProjectsDialogEvent};
use crate::views::overlays::dialogs::import_session_dialog::{ImportSessionsDialog, ImportSessionsDialogEvent};
use crate::views::overlays::menus::project_context_menu::{ContextMenu, ContextMenuEvent};
use crate::views::overlays::menus::folder_context_menu::{FolderContextMenu, FolderContextMenuEvent};
use crate::views::overlays::{ShellSelectorOverlay, ShellSelectorOverlayEvent};
use crate::views::overlays::settings::profile_manager::{ProfileManager, ProfileManagerEvent};
use crate::views::overlays::settings::settings_panel::{SettingsCategory, SettingsPanel, SettingsPanelEvent};
use crate::views::overlays::dialogs::terminal_color_scheme_dialog::{
    TerminalColorSchemeDialog, TerminalColorSchemeDialogEvent,
};
use crate::views::overlays::dialogs::update_dialog::{UpdateDialog, UpdateDialogEvent};
use crate::views::overlays::dialogs::about_dialog::{AboutDialog, AboutDialogEvent};
use crate::views::overlays::dialogs::help_dialog::{HelpDialog, HelpDialogEvent};
use crate::views::overlays::pickers::theme_selector::{ThemeSelector, ThemeSelectorEvent};
use crate::views::overlays::dialogs::log_record_dialog::{LogRecordDialog, LogRecordDialogEvent};
use crate::views::overlays::dialogs::log_saved_dialog::{LogSavedDialog, LogSavedDialogEvent};
use crate::views::overlays::tab_context_menu::{TabContextMenu, TabContextMenuEvent};
use velowork_views_terminal::overlays::terminal_context_menu::{open_terminal_context_menu, TerminalContextMenuEvent};
use velowork_ui::menu::PopupMenu;
use crate::views::overlays::log_console::{LogConsole, LogConsoleEvent};
use crate::views::overlays::rename_directory_dialog::{RenameDirectoryDialog, RenameDirectoryDialogEvent};
use crate::views::overlays::dialogs::quick_command_dialog::{
    QuickCommandDialog, QuickCommandDialogEvent, QuickCommandDialogMode,
};
use crate::views::overlays::dialogs::tunnel_dialog::{
    TunnelDialog, TunnelDialogEvent, TunnelDialogMode,
};
use crate::views::overlays::dialogs::service_dialog::{
    ServiceDialog, ServiceDialogEvent, ServiceDialogMode,
};
use crate::views::overlays::menus::tunnel_context_menu::{
    open_tunnel_context_menu, TunnelContextMenuEvent, TunnelMenuRequest,
};
use crate::views::overlays::menus::service_context_menu::{
    open_service_context_menu, ServiceContextMenuEvent, ServiceMenuRequest,
};
use crate::views::overlays::dialogs::quick_command_var_dialog::{
    QuickCommandVarDialog, QuickCommandVarDialogEvent,
};
use crate::views::overlays::menus::quick_command_context_menu::{
    open_quick_command_context_menu, QuickCommandContextMenuEvent, QuickCommandMenuRequest,
};
use crate::views::overlays::transfer_popup::{TransferPopup, TransferPopupEvent};
use crate::views::overlays::terminal_ai_inline::{TerminalAiInline, TerminalAiInlineEvent};
use velowork_ui::confirm_dialog::{ConfirmDialog, ConfirmDialogEvent};
use velowork_ui::{AnimatedModal, AnimatedModalEvent};
use velowork_state::{ServiceDefinition, ServiceNode, TunnelNode};
use velowork_workspace::quick_commands::{
    new_quick_command_id, qc_find_node_mut, qc_find_node_ref, qc_insert_node, qc_parent_id_of,
    qc_remove_node, QuickCommandNode, QuickCommandVar,
};
use velowork_workspace::services::{
    new_service_id, service_parent_id_of, service_unique_duplicate_name,
};
use velowork_workspace::stores::{GlobalServiceStore, GlobalTunnelStore};
use velowork_workspace::tunnels::{
    new_tunnel_id, tunnel_find_node_ref, tunnel_parent_id_of, tunnel_unique_duplicate_name,
};
use velowork_terminal::GlobalTunnelEngine;
use velowork_terminal::TerminalsRegistry;
use velowork_views_terminal::transfer_store::GlobalTransferStore;
use crate::workspace::request_broker::RequestBroker;
use crate::workspace::requests::SidebarRequest;
use crate::workspace::state::{WindowId, Workspace};

// Re-export generic overlay utilities from velowork-ui
pub use velowork_ui::overlay::{CloseEvent, OverlaySlot};
pub use velowork_ui::overlay_registry::{OverlayId, OverlayInfo, OverlayRegistry};
pub use velowork_ui::{open_overlay, toggle_overlay};

// CloseEvent impls for overlay events defined in src/ (local types)

impl CloseEvent for AddProjectDialogEvent {
    fn is_close(&self) -> bool {
        matches!(self, Self::Close | Self::Saved { .. })
    }
}
impl CloseEvent for ImportSessionsDialogEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}
impl CloseEvent for ManageProjectsDialogEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}
impl CloseEvent for KeybindingsHelpEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}
impl CloseEvent for ThemeSelectorEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}
impl CloseEvent for CommandPaletteEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}
impl CloseEvent for SettingsPanelEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}
impl CloseEvent for HelpDialogEvent {
    fn is_close(&self) -> bool {
        matches!(self, Self::Close)
    }
}

impl CloseEvent for UpdateDialogEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}
impl CloseEvent for TunnelDialogEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}
impl CloseEvent for ServiceDialogEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}

// ============================================================================
// OverlayManager Entity
// ============================================================================

/// Events emitted by OverlayManager that require handling by WindowView.
///
/// These events are forwarded from individual overlays when they require
/// actions that need access to WindowView's state (terminals, PTY manager, etc.)
#[derive(Clone)]
pub enum OverlayManagerEvent {
    /// Shell selector selected a shell for a terminal
    ShellSelected {
        shell_type: ShellType,
        project_id: String,
        terminal_id: String,
    },

    /// Context menu: Add terminal to project
    AddTerminal { project_id: String },

    /// Context menu: Rename project
    RenameProject { project_id: String, project_name: String },

    /// Context menu: Rename directory on disk
    RenameDirectory { project_id: String, project_path: String },

    /// Context menu: Delete project
    DeleteProject { project_id: String },

    /// Color picker: project color was changed (for remote sync)
    ProjectColorChanged { project_id: String, color: velowork_core::theme::FolderColor },

    /// Context menu: Focus parent project of a worktree
    FocusParent { project_id: String },

    /// Project switcher: Focus a specific project
    FocusProject(String),

    /// Project switcher: jump into an open project's first terminal (Tab),
    /// switching windows if needed, without changing the layout.
    JumpToProject(String),

    /// Project switcher: Toggle project overview visibility
    ToggleProjectVisibility(String),

    /// Terminal context menu: copy
    TerminalCopy { terminal_id: String },
    /// Terminal context menu: paste
    TerminalPaste { terminal_id: String },
    /// Terminal context menu: clear
    TerminalClear { terminal_id: String },
    /// Terminal context menu: select all
    TerminalSelectAll { terminal_id: String },
    /// Terminal context menu: split
    TerminalSplit { project_id: String, layout_path: Vec<usize>, direction: crate::workspace::state::SplitDirection },
    /// Terminal context menu: close terminal
    TerminalClose { project_id: String, terminal_id: String },
    TerminalExportSelected { terminal_id: String },
    TerminalExportAll { terminal_id: String },
    TerminalLogStart {
        terminal_id: String,
        filename: String,
        append_mode: bool,
        auto_save_interval: usize,
    },
    ShowLogRecordDialog { terminal_id: String },
    ShowLogSavedDialog {
        terminal_id: String,
        path: std::path::PathBuf,
    },
    TerminalLogOpenFileWithPath {
        path: std::path::PathBuf,
    },
    TerminalLogOpenFolderWithPath {
        path: std::path::PathBuf,
    },
    TerminalLogPause { terminal_id: String },
    TerminalLogResume { terminal_id: String },
    TerminalLogStop { terminal_id: String },
    TerminalLogOpenFile { terminal_id: String },
    TerminalLogOpenFolder { terminal_id: String },
    TerminalAIInterpret { terminal_id: String, text: String },
    TerminalFind { terminal_id: String },
    TerminalWebSearch { terminal_id: String },
    TerminalSearchWithEngine { terminal_id: String, url: String },
    TerminalToggleWordWrap,
    TerminalToggleLineNumbers,
    TerminalClearScrollback { terminal_id: String },
    TerminalClearAll { terminal_id: String },
    TerminalEditConfig { terminal_id: String },
    TerminalZmodemUpload { terminal_id: String },

    /// Tab context menu: duplicate session
    TabDuplicateSession { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    /// Tab context menu: duplicate channel
    TabDuplicateChannel { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    /// Tab context menu: reconnect session
    TabReconnect { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    /// Tab context menu: split horizontal
    TabSplitHorizontal { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    /// Tab context menu: split vertical
    TabSplitVertical { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    /// Tab context menu: close tab
    TabClose { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    /// Tab context menu: close other tabs
    TabCloseOthers { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    /// Tab context menu: close tabs to the right
    TabCloseToRight { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    /// Tab context menu: close inactive tabs
    TabCloseInactive { project_id: String, layout_path: Vec<usize> },
    /// Tab context menu: open session settings
    TabSessionSettings { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    /// Tab context menu: toggle minimize terminal
    TabToggleMinimize { project_id: String, layout_path: Vec<usize>, tab_index: usize },

    /// Profile manager: switch to a different profile (triggers relaunch)
    SwitchProfile(String),

    /// A modal overlay was closed (signals WindowView to restore terminal/root focus)
    ModalClosed,

    /// Inline AI toolbar / popover event
    TerminalAiInline(TerminalAiInlineEvent),
}

#[derive(Clone)]
pub(crate) struct ModalStackEntry {
    pub(crate) animated: Entity<AnimatedModal>,
    pub(crate) view: AnyView,
    pub(crate) modal_type_id: Option<std::any::TypeId>,
    pub(crate) origin_focus_handle: Option<FocusHandle>,
    pub(crate) panel_container_handle: Option<FocusHandle>,
}

/// Centralized overlay manager that handles all modal overlays.
pub struct OverlayManager {
    /// Identifies which window-scoped slot on the shared `Workspace` this
    /// overlay manager addresses.
    pub(crate) window_id: WindowId,
    pub(crate) window_handle: Option<AnyWindowHandle>,
    pub(crate) workspace: Entity<Workspace>,
    pub(crate) focus_manager: Entity<crate::workspace::focus::FocusManager>,
    request_broker: Entity<RequestBroker>,

    /// Central registry of all interactive overlay surfaces (menus, popovers,
    /// tooltips, etc.) tracked for window-level click-outside dismissal.
    /// The `WindowView` routes `MouseDown` here so no dock/panel needs to
    /// know about overlays.
    overlay_registry: Entity<OverlayRegistry>,

    /// 模态弹窗 LIFO 栈：支持多层嵌套弹窗、子确认框与原路精确归还
    modal_stack: Vec<ModalStackEntry>,

    /// 暂存刚关闭弹窗的发起源焦点句柄（供 WindowView::render 三级降级自愈使用）
    pending_restore_origin: Option<FocusHandle>,

    /// 暂存刚关闭弹窗的发起面板容器句柄（供已删节点降级平移使用）
    pending_restore_panel: Option<FocusHandle>,

    /// The single active modal overlay (only one can be open at a time).
    active_modal: Option<AnyView>,
    active_animated_modal: Option<Entity<AnimatedModal>>,

    /// TypeId of the active modal for toggle detection.
    modal_type_id: Option<std::any::TypeId>,

    // Context menus remain separate (positioned popups, not full-screen modals)
    context_menu: OverlaySlot<ContextMenu>,
    folder_context_menu: OverlaySlot<FolderContextMenu>,
    terminal_context_menu: OverlaySlot<PopupMenu>,
    tab_context_menu: OverlaySlot<TabContextMenu>,
    quick_command_context_menu: OverlaySlot<PopupMenu>,
    tunnel_context_menu: OverlaySlot<PopupMenu>,
    service_context_menu: OverlaySlot<PopupMenu>,
    pub(crate) confirm_dialog: Option<Entity<ConfirmDialog>>,

    /// Latest user click origin with timestamp for global origin-aware modal animation.
    last_click_origin: Option<(Point<Pixels>, std::time::Instant)>,

    /// Most recent terminal context (terminal_id, project_id, layout_path) for pane targeting.
    last_terminal_context: Option<(String, String, Vec<usize>)>,

    /// Transfer manager popup (anchored above the status-bar transfer button).
    transfer_popup: OverlaySlot<TransferPopup>,

    /// Terminal inline AI toolbar / popover.
    pub(crate) terminal_ai_inline: OverlaySlot<TerminalAiInline>,

    /// OS window handle of the detached settings panel window (if open).
    /// Used to prevent opening multiple settings windows — if the handle is
    /// still alive we activate the existing window instead of spawning a new one.
    settings_window_handle: Option<AnyWindowHandle>,
    settings_panel_entity: Option<Entity<SettingsPanel>>,

    /// OS window handle of the detached log console window (if open).
    log_console_window_handle: Option<AnyWindowHandle>,
    log_console_entity: Option<Entity<LogConsole>>,

    pub(crate) on_qc_create_folder: Option<std::sync::Arc<dyn Fn(Option<String>, &mut Context<OverlayManager>) + Send + Sync>>,
    pub(crate) on_qc_rename_folder: Option<std::sync::Arc<dyn Fn(String, String, &mut Context<OverlayManager>) + Send + Sync>>,
    pub(crate) on_qc_rename_command: Option<std::sync::Arc<dyn Fn(String, String, &mut Context<OverlayManager>) + Send + Sync>>,
    pub(crate) on_qc_created: Option<std::sync::Arc<dyn Fn(String, &mut Context<OverlayManager>) + Send + Sync>>,
    pub(crate) on_tunnel_create_folder: Option<std::sync::Arc<dyn Fn(Option<String>, &mut Context<OverlayManager>) + Send + Sync>>,
    pub(crate) on_tunnel_rename_folder: Option<std::sync::Arc<dyn Fn(String, String, &mut Context<OverlayManager>) + Send + Sync>>,
    pub(crate) on_tunnel_rename: Option<std::sync::Arc<dyn Fn(String, String, &mut Context<OverlayManager>) + Send + Sync>>,
    pub(crate) on_service_create_folder: Option<std::sync::Arc<dyn Fn(Option<String>, &mut Context<OverlayManager>) + Send + Sync>>,
    pub(crate) on_service_rename_folder: Option<std::sync::Arc<dyn Fn(String, String, &mut Context<OverlayManager>) + Send + Sync>>,
    pub(crate) on_service_rename: Option<std::sync::Arc<dyn Fn(String, String, &mut Context<OverlayManager>) + Send + Sync>>,
    pub(crate) on_open_commands_with: Option<std::sync::Arc<dyn Fn(String, &mut Context<OverlayManager>) + Send + Sync>>,
}

impl OverlayManager {
    /// Create a new OverlayManager.
    pub fn new(
        window_id: WindowId,
        window_handle: Option<AnyWindowHandle>,
        workspace: Entity<Workspace>,
        focus_manager: Entity<crate::workspace::focus::FocusManager>,
        request_broker: Entity<RequestBroker>,
        overlay_registry: Entity<OverlayRegistry>,
    ) -> Self {
        Self {
            window_id,
            window_handle,
            workspace,
            focus_manager,
            request_broker,
            overlay_registry,
            modal_stack: Vec::new(),
            pending_restore_origin: None,
            pending_restore_panel: None,
            active_modal: None,
            active_animated_modal: None,
            modal_type_id: None,
            on_qc_create_folder: None,
            on_qc_rename_folder: None,
            on_qc_rename_command: None,
            on_qc_created: None,
            on_tunnel_create_folder: None,
            on_tunnel_rename_folder: None,
            on_tunnel_rename: None,
            on_service_create_folder: None,
            on_service_rename_folder: None,
            on_service_rename: None,
            on_open_commands_with: None,
            context_menu: OverlaySlot::new(),
            folder_context_menu: OverlaySlot::new(),
            terminal_context_menu: OverlaySlot::new(),
            tab_context_menu: OverlaySlot::new(),
            quick_command_context_menu: OverlaySlot::new(),
            tunnel_context_menu: OverlaySlot::new(),
            service_context_menu: OverlaySlot::new(),
            confirm_dialog: None,
            last_click_origin: None,
            last_terminal_context: None,
            transfer_popup: OverlaySlot::new(),
            terminal_ai_inline: OverlaySlot::new(),
            settings_window_handle: None,
            settings_panel_entity: None,
            log_console_window_handle: None,
            log_console_entity: None,
        }
    }

    /// Identifies which window-scoped slot on the shared `Workspace` this
    /// overlay manager addresses. Always `WindowId::Main` today (single-window
    /// runtime); slice 05 spawns extras that mint distinct `WindowId::Extra(uuid)`s.
    /// Field is read directly within the impl via `self.window_id` once readers
    /// land; this public getter exists for external callers (e.g. the slice 05
    /// spawn flow on `Velowork`) that need to address window-scoped state on
    /// `Workspace` in the same window this overlay manager inhabits.
    /// `#[allow(dead_code)]` because no caller reads it yet -- rustc tracks
    /// fields and methods separately, so the field being used by the ctor does
    /// NOT mark the getter as used.
    #[allow(dead_code)]
    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// Set or update the main window handle for focus dispatching.
    pub fn set_window_handle(&mut self, handle: AnyWindowHandle) {
        self.window_handle = Some(handle);
    }

    /// Access the window-scoped `OverlayRegistry`.
    pub fn overlay_registry(&self) -> Entity<OverlayRegistry> {
        self.overlay_registry.clone()
    }

    pub(crate) fn active_project_id(&self, cx: &App) -> Option<String> {
        self.focus_manager.read(cx).active_project_id().cloned()
    }

    // ========================================================================
    // Modal management helpers (Focus Origin Stack & Cascading Modals)
    // ========================================================================

    /// Close the active (topmost) modal with smooth leave animation, restoring focus when finished.
    pub fn close_modal(&mut self, cx: &mut Context<Self>) {
        if let Some(am) = self.active_animated_modal.clone() {
            am.update(cx, |modal, cx| {
                modal.request_close(cx);
            });
        } else if let Some(last) = self.modal_stack.last() {
            let am = last.animated.clone();
            am.update(cx, |modal, cx| {
                modal.request_close(cx);
            });
        } else {
            self.finish_modal_closed(cx);
        }
    }

    /// Close the active (topmost) modal with smooth Dynamic Island morph exit towards `target_bounds`.
    pub fn close_modal_with_morph(&mut self, target_bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        self.close_modal_with_morph_content(target_bounds, None, cx);
    }

    /// Close the active (topmost) modal with smooth Dynamic Island morph exit towards `target_bounds`
    /// and optional destination content for cross-dissolving preview.
    pub fn close_modal_with_morph_content(
        &mut self,
        target_bounds: Bounds<Pixels>,
        destination_content: Option<AnyView>,
        cx: &mut Context<Self>,
    ) {
        if let Some(am) = self.active_animated_modal.clone() {
            am.update(cx, |modal, cx| {
                modal.start_morph_exit_with_content(target_bounds, destination_content, cx);
            });
        } else if let Some(last) = self.modal_stack.last() {
            let am = last.animated.clone();
            am.update(cx, |modal, cx| {
                modal.start_morph_exit_with_content(target_bounds, destination_content, cx);
            });
        } else {
            self.finish_modal_closed(cx);
        }
    }

    /// Complete modal dismissal for a specific animated modal instance, updating stack
    /// and queuing focus return targets.
    pub(crate) fn finish_modal_closed_for(&mut self, anim_entity_id: EntityId, cx: &mut Context<Self>) {
        let removed_entry = if let Some(pos) = self
            .modal_stack
            .iter()
            .position(|e| e.animated.entity_id() == anim_entity_id)
        {
            Some(self.modal_stack.remove(pos))
        } else {
            self.modal_stack.pop()
        };

        if let Some(top) = self.modal_stack.last() {
            self.active_modal = Some(top.view.clone());
            self.active_animated_modal = Some(top.animated.clone());
            self.modal_type_id = top.modal_type_id;
        } else {
            self.active_modal = None;
            self.active_animated_modal = None;
            self.modal_type_id = None;
        }

        if let Some(entry) = removed_entry {
            self.pending_restore_origin = entry.origin_focus_handle;
            self.pending_restore_panel = entry.panel_container_handle;
        }

        // Clear any project-panel hover highlight published by the Switch Project overlay.
        crate::views::overlays::project_hover::set_hovered_project(None, cx);

        // If modal stack is empty and no dock/origin was specified, restore focused terminal state
        if self.modal_stack.is_empty()
            && self.pending_restore_origin.is_none()
            && self.pending_restore_panel.is_none()
        {
            let workspace = self.workspace.clone();
            self.focus_manager.update(cx, |fm, cx| {
                workspace.update(cx, |ws, cx| ws.restore_focused_terminal(fm, cx));
                cx.notify();
            });
        }

        cx.emit(OverlayManagerEvent::ModalClosed);
        cx.notify();
    }

    /// Complete topmost modal dismissal.
    fn finish_modal_closed(&mut self, cx: &mut Context<Self>) {
        if let Some(top) = self.modal_stack.last() {
            let id = top.animated.entity_id();
            self.finish_modal_closed_for(id, cx);
        } else if self.active_modal.is_some() || self.active_animated_modal.is_some() {
            self.active_modal = None;
            self.active_animated_modal = None;
            self.modal_type_id = None;
            cx.emit(OverlayManagerEvent::ModalClosed);
            cx.notify();
        }
    }

    /// Take pending restore origin focus handle.
    pub fn take_pending_restore_origin(&mut self) -> Option<FocusHandle> {
        self.pending_restore_origin.take()
    }

    /// Take pending restore panel container focus handle.
    pub fn take_pending_restore_panel(&mut self) -> Option<FocusHandle> {
        self.pending_restore_panel.take()
    }

    /// Active modal focus handle (if modal is currently active).
    pub fn active_modal_focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        self.active_animated_modal
            .as_ref()
            .map(|am| am.read(cx).focus_handle().clone())
    }

    /// Immediate close without leave animation (used when immediately replacing with another modal).
    #[allow(dead_code)]
    fn close_modal_immediate(&mut self, cx: &mut Context<Self>) {
        self.finish_modal_closed(cx);
    }

    /// Check if a modal is currently open.
    pub fn has_modal(&self) -> bool {
        !self.modal_stack.is_empty() || self.active_modal.is_some()
    }

    /// Check whether the currently active modal originated from or belongs to the specified panel.
    pub fn active_modal_belongs_to(&self, panel_handle: &FocusHandle) -> bool {
        self.modal_stack.iter().rev().any(|entry| {
            entry.panel_container_handle.as_ref() == Some(panel_handle)
                || entry.origin_focus_handle.as_ref() == Some(panel_handle)
        })
    }

    /// Check if the active modal is of a specific type.
    fn is_modal<T: 'static>(&self) -> bool {
        self.modal_type_id == Some(std::any::TypeId::of::<T>())
    }

    /// Record the latest user click position for origin-aware modal emergence.
    pub fn record_click_origin(&mut self, origin: Point<Pixels>) {
        self.last_click_origin = Some((origin, std::time::Instant::now()));
    }

    /// Consume the recent click position if it occurred within 500ms.
    pub fn consume_click_origin(&mut self) -> Option<Point<Pixels>> {
        if let Some((pos, time)) = self.last_click_origin.take() {
            if time.elapsed() <= std::time::Duration::from_millis(500) {
                return Some(pos);
            }
        }
        None
    }

    /// Open a modal with explicit origin and panel container FocusHandles for
    /// precise origin-anchored return and fallback.
    pub fn open_modal_with_origin<T: Render + 'static>(
        &mut self,
        entity: Entity<T>,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        self.open_modal_with_click_origin(entity, origin, panel_container, None, cx);
    }

    /// Open a modal with explicit origin focus handles and optional explicit click origin coordinate.
    pub fn open_modal_with_click_origin<T: Render + 'static>(
        &mut self,
        entity: Entity<T>,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        explicit_click_origin: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        let enable_animations = crate::settings::settings_entity(cx)
            .read(cx)
            .settings
            .enable_animations;

        let is_top_aligned = std::any::TypeId::of::<T>() == std::any::TypeId::of::<CommandPalette>()
            || std::any::TypeId::of::<T>() == std::any::TypeId::of::<ShellSelectorOverlay>();

        // Priority: explicit click origin -> recent recorded click within 500ms -> None (keyboard fallback)
        let click_origin = explicit_click_origin.or_else(|| self.consume_click_origin());

        let animated = cx.new(|cx| {
            let mut modal = AnimatedModal::new_with_origin(entity.into(), click_origin, cx)
                .with_animations(enable_animations, cx);
            if is_top_aligned {
                modal = modal.align_top(px(80.0));
            }
            modal
        });

        let anim_id = animated.entity_id();
        cx.subscribe(&animated, move |this, _, event: &AnimatedModalEvent, cx| match event {
            AnimatedModalEvent::Dismissed => {
                this.finish_modal_closed_for(anim_id, cx);
            }
            AnimatedModalEvent::Closing => {}
        })
        .detach();

        let entry = ModalStackEntry {
            animated: animated.clone(),
            view: animated.clone().into(),
            modal_type_id: Some(std::any::TypeId::of::<T>()),
            origin_focus_handle: origin,
            panel_container_handle: panel_container,
        };

        self.active_modal = Some(entry.view.clone());
        self.active_animated_modal = Some(animated);
        self.modal_type_id = entry.modal_type_id;
        self.modal_stack.push(entry);

        let workspace = self.workspace.clone();
        self.focus_manager.update(cx, |fm, cx| {
            workspace.update(cx, |ws, cx| ws.clear_focused_terminal(fm, cx));
            cx.notify();
        });
        cx.notify();
    }

    /// Open a modal, automatically wrapping it in `AnimatedModal` to provide
    /// spring bounce overshoot enter and smooth leave animations.
    ///
    /// Automatically clears terminal focus so keyboard input goes to the modal.
    pub fn open_modal<T: Render + 'static>(&mut self, entity: Entity<T>, cx: &mut Context<Self>) {
        self.open_modal_with_origin(entity, None, None, cx);
    }

    /// Open a modal with a specified initial focus target (e.g. close button or specific control),
    /// automatically clearing terminal focus and granting physical window focus to the target.
    pub fn open_modal_with_focus<T: Render + 'static>(
        &mut self,
        entity: Entity<T>,
        focus_handle: FocusHandle,
        cx: &mut Context<Self>,
    ) {
        self.open_modal_with_focus_and_window(entity, focus_handle, None, cx);
    }

    /// Open a modal with a specified initial focus target and an optional Window reference
    /// for synchronous zero-delay physical focus dispatch.
    pub fn open_modal_with_focus_and_window<T: Render + 'static>(
        &mut self,
        entity: Entity<T>,
        focus_handle: FocusHandle,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.open_modal(entity, cx);
        if let Some(window) = window {
            window.focus(&focus_handle, cx);
        } else if let Some(wh) = self.window_handle {
            cx.defer(move |cx| {
                let _ = wh.update(cx, |_, window, cx| {
                    window.focus(&focus_handle, cx);
                });
            });
        }
    }

    /// Open and expand the bottom Commands panel with prefilled command text.
    pub fn open_commands_panel_with(&self, cmd: &str, cx: &mut Context<Self>) {
        if let Some(cb) = &self.on_open_commands_with {
            cb(cmd.to_string(), cx);
        }
    }

    /// Get the active modal for rendering.
    pub fn render_modal(&self) -> Option<AnyView> {
        self.modal_stack
            .last()
            .map(|e| e.view.clone())
            .or_else(|| self.active_modal.clone())
    }

    /// Get all active modals for rendering in stack order (bottom to top).
    pub fn render_modals(&self) -> Vec<AnyView> {
        if !self.modal_stack.is_empty() {
            self.modal_stack.iter().map(|e| e.view.clone()).collect()
        } else if let Some(m) = self.active_modal.clone() {
            vec![m]
        } else {
            Vec::new()
        }
    }

    /// Check if the active modal is a ConfirmDialog (e.g. quit confirmation).
    pub fn is_confirm_dialog_active(&self) -> bool {
        self.is_modal::<velowork_ui::confirm_dialog::ConfirmDialog>()
    }

    /// Get the active ConfirmDialog modal for rendering on top of the lock screen if one is active.
    pub fn render_confirm_dialog_modal(&self) -> Option<AnyView> {
        if self.is_confirm_dialog_active() {
            self.render_modal()
        } else {
            None
        }
    }

    // ========================================================================
    // Centralized overlay registry (window-level click-outside dismissal)
    // ========================================================================

    /// Register an overlay surface (menu, popover, tooltip, ...) for
    /// window-level click-outside dismissal. The `close` callback is
    /// invoked when a `MouseDown` lands outside the overlay's bounds.
    pub fn register_overlay(
        &mut self,
        info: OverlayInfo,
        close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
        cx: &mut Context<Self>,
    ) {
        self.overlay_registry.update(cx, |r, _cx| r.register(info, close));
    }

    /// Remove a registered overlay by id.
    pub fn unregister_overlay(&mut self, id: &OverlayId, cx: &mut Context<Self>) {
        self.overlay_registry.update(cx, |r, _| r.unregister(id));
    }

    /// Update a registered overlay's on-screen bounds.
    pub fn set_overlay_bounds(
        &mut self,
        id: &OverlayId,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.overlay_registry.update(cx, |r, _| r.set_bounds(id, bounds));
    }

    /// Window-level `MouseDown` entry point. Delegates to the registry,
    /// which closes every `ClickOutside` overlay whose bounds do not
    /// contain `point`. Called by `WindowView` for every left `MouseDown`.
    pub fn handle_overlay_mouse_down(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay_registry
            .update(cx, |r, cx| r.handle_mouse_down(point, window, cx));
    }

    // ========================================================================
    // Context menu visibility checks (kept separate)
    // ========================================================================

    /// Close all context menu slots (mutual exclusion).
    fn close_all_context_menus(&mut self) {
        self.context_menu.close();
        self.folder_context_menu.close();
        self.terminal_context_menu.close();
        self.tab_context_menu.close();
        self.quick_command_context_menu.close();
        self.tunnel_context_menu.close();
        self.service_context_menu.close();
        self.transfer_popup.close();
        self.terminal_ai_inline.close();
    }

    /// Force-close every interactive floating surface currently open in this
    /// window: modals, context menus, the color-picker popover, the transfer
    /// popup, and every surface registered in the global `OverlayRegistry`
    /// (dropdown panels such as the "forward type" select, popovers, etc.).
    ///
    /// Used when the app auto-locks: any dropdown/overlay that was left open
    /// behind the lock screen must be dismissed so it can neither leak
    /// information nor leave a visual remnant. The lock screen itself is *not*
    /// part of `OverlayRegistry`, so it is unaffected. The underlying editor
    /// state (e.g. the lock-screen-settings edit dialog) is preserved because
    /// it lives in the main window subtree, not in a floating surface.
    /// Close all detached standalone OS windows (e.g. settings panel, log console).
    /// Safely cleans up window handles and associated entities to prevent credential leakage on lock.
    pub fn close_detached_windows(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window_handle.take() {
            let _ = handle.update(cx, |_, window, _| {
                window.remove_window();
            });
            self.settings_panel_entity = None;
        }
        if let Some(handle) = self.log_console_window_handle.take() {
            let _ = handle.update(cx, |_, window, _| {
                window.remove_window();
            });
            self.log_console_entity = None;
        }
    }

    pub fn close_all_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Do NOT close active modal: the user's edit state (e.g. TunnelDialog, Settings)
        // is preserved under the lock screen and restored upon unlock.
        self.close_all_context_menus();
        self.close_detached_windows(cx);

        // Dismiss every floating surface registered in the global registry
        // (select dropdowns, popovers, ...).
        self.overlay_registry
            .update(cx, |registry, cx| {
                registry.close_all(window, cx);
            });
    }

    /// Check if context menu is open.
    pub fn has_context_menu(&self) -> bool {
        self.context_menu.is_open()
    }

    /// Check if folder context menu is open.
    pub fn has_folder_context_menu(&self) -> bool {
        self.folder_context_menu.is_open()
    }

    /// Check if terminal context menu is open.
    pub fn has_terminal_context_menu(&self) -> bool {
        self.terminal_context_menu.is_open()
    }

    /// Check if tab context menu is open.
    pub fn has_tab_context_menu(&self) -> bool {
        self.tab_context_menu.is_open()
    }

    /// Check if quick-command context menu is open.
    pub fn has_quick_command_context_menu(&self) -> bool {
        self.quick_command_context_menu.is_open()
    }

    // ========================================================================
    // Simple toggle overlays
    // ========================================================================

    /// Toggle add project dialog overlay.
    pub fn toggle_add_project_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        let window_id = self.window_id;
        toggle_overlay!(self, cx, AddProjectDialog, AddProjectDialogEvent, |cx| {
            AddProjectDialog::new(workspace, window_id, cx)
        });
    }

    /// Toggle import sessions dialog overlay.
    pub fn toggle_import_sessions_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        let window_id = self.window_id;
        let focus_manager = self.focus_manager.clone();
        toggle_overlay!(self, cx, ImportSessionsDialog, ImportSessionsDialogEvent, |cx| {
            ImportSessionsDialog::new(workspace, window_id, focus_manager, cx)
        });
    }

    /// Toggle manage projects dialog overlay.
    pub fn toggle_manage_projects_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        let window_id = self.window_id;
        let focus_manager = self.focus_manager.clone();
        let overlay_manager = cx.entity();
        toggle_overlay!(self, cx, ManageProjectsDialog, ManageProjectsDialogEvent, |cx| {
            ManageProjectsDialog::new(workspace, window_id, focus_manager, overlay_manager, cx)
        });
    }

    /// Toggle keybindings help overlay.
    pub fn toggle_keybindings_help(&mut self, cx: &mut Context<Self>) {
        if self.is_modal::<KeybindingsHelp>() {
            self.close_modal(cx);
        } else {
            let entity = cx.new(KeybindingsHelp::new);
            cx.subscribe(&entity, |this, _, event: &KeybindingsHelpEvent, cx| {
                match event {
                    KeybindingsHelpEvent::Close => {
                        this.close_modal(cx);
                    }
                    KeybindingsHelpEvent::ReloadBindings => {
                        crate::keybindings::reload_keybindings(cx);
                    }
                }
            }).detach();
            self.open_modal(entity, cx);
        }
    }

    /// Toggle theme selector overlay.
    pub fn toggle_theme_selector(&mut self, cx: &mut Context<Self>) {
        self.toggle_theme_selector_with_window(None, cx);
    }

    /// Toggle theme selector overlay with optional Window reference for immediate focus.
    pub fn toggle_theme_selector_with_window(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if self.is_modal::<ThemeSelector>() {
            self.close_modal(cx);
        } else {
            let entity = cx.new(ThemeSelector::new);
            let focus = entity.read(cx).focus_handle(cx);
            cx.subscribe(&entity, |this, _, event: &ThemeSelectorEvent, cx| {
                if event.is_close() {
                    this.close_modal(cx);
                }
            })
            .detach();
            self.open_modal_with_focus_and_window(entity, focus, window, cx);
        }
    }

    /// Toggle command palette overlay.
    pub fn toggle_command_palette(&mut self, cx: &mut Context<Self>) {
        let ws = self.workspace.clone();
        let fm = self.focus_manager.clone();
        let window_id = self.window_id;
        toggle_overlay!(self, cx, CommandPalette, CommandPaletteEvent, |cx| {
            CommandPalette::new(ws, fm, window_id, cx)
        });
    }

    /// Toggle settings panel in a standalone window.
    ///
    /// If a settings window is already open, activates (raises) it instead of
    /// opening a duplicate. The tracked handle is cleared when the window is
    /// closed by the user.
    pub fn toggle_settings_panel(&mut self, cx: &mut Context<Self>) {
        self.open_settings_panel_to(None, cx);
    }

    /// Open settings panel in a standalone window and navigate to a category.
    ///
    /// If a settings window is already open, activates (raises) it and navigates
    /// to the requested category instead of opening a duplicate.
    pub fn open_settings_panel_to(&mut self, category: Option<SettingsCategory>, cx: &mut Context<Self>) {
        // If we already have a settings window handle, try to activate it and navigate.
        if let Some(handle) = self.settings_window_handle {
            let cat = category.clone();
            let panel_entity = self.settings_panel_entity.clone();
            let alive = handle.update(cx, |_, window, cx| {
                window.activate_window();
                if let Some(cat) = cat
                    && let Some(panel) = panel_entity {
                    panel.update(cx, |p, cx| {
                        p.nav_to_category(cat, cx);
                    });
                }
                window.refresh();
            });
            if alive.is_ok() {
                // Window is still open — brought it to front and navigated.
                return;
            }
            // Window was closed externally; clear the stale handle.
            self.settings_window_handle = None;
            self.settings_panel_entity = None;
        }

        let workspace = self.workspace.clone();
        let title = i18n!(cx, "settings.title");
        let initial_category = category;
        let panel_slot: std::sync::Arc<parking_lot::Mutex<Option<gpui::Entity<SettingsPanel>>>> =
            std::sync::Arc::new(parking_lot::Mutex::new(None));
        let panel_slot_capture = panel_slot.clone();
        let panel_slot_close = panel_slot.clone();

        let handle = crate::app::open_detached_overlay::<SettingsPanel, SettingsPanelEvent>(
            title,
            move |window, registry, cx| {
                let entity = cx.new(|cx| {
                    let mut panel = SettingsPanel::new(workspace, window, cx);
                    panel.set_overlay_registry(registry, cx);
                    if let Some(cat) = initial_category {
                        panel.nav_to_category(cat, cx);
                    }
                    panel
                });
                *panel_slot_capture.lock() = Some(entity.clone());
                entity
            },
            crate::app::DetachedOverlayOptions {
                size: gpui::size(gpui::px(960.0), gpui::px(700.0)),
                min_size: gpui::size(gpui::px(600.0), gpui::px(450.0)),
                on_close: Some(std::sync::Arc::new(move |_window, cx| {
                    if let Some(panel) = panel_slot_close.lock().as_ref() {
                        panel.update(cx, |this, cx| {
                            this.flush_inputs_to_settings(cx);
                        });
                    }
                })),
                hide_titlebar: false,
            },
            cx,
        );
        self.settings_panel_entity = panel_slot.lock().clone();
        self.settings_window_handle = handle;
    }

    /// Toggle the standalone update dialog overlay.
    pub fn toggle_update_dialog(&mut self, cx: &mut Context<Self>) {
        self.toggle_update_dialog_with_window(None, cx);
    }

    /// Toggle the standalone update dialog overlay with an optional Window reference.
    pub fn toggle_update_dialog_with_window(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if self.is_modal::<UpdateDialog>() {
            self.close_modal(cx);
        } else {
            let entity = cx.new(UpdateDialog::new);
            let focus = entity.read(cx).focus_handle();
            cx.subscribe(&entity, |this, _, event: &UpdateDialogEvent, cx| {
                if event.is_close() {
                    this.close_modal(cx);
                }
            })
            .detach();
            self.open_modal_with_focus_and_window(entity, focus, window, cx);
        }
    }

    /// Toggle the standalone help dialog overlay.
    pub fn toggle_help_dialog(&mut self, cx: &mut Context<Self>) {
        self.toggle_help_dialog_with_window(None, cx);
    }

    /// Toggle the standalone help dialog overlay with an optional Window reference.
    pub fn toggle_help_dialog_with_window(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if self.is_modal::<HelpDialog>() {
            self.close_modal(cx);
        } else {
            let entity = cx.new(HelpDialog::new);
            let focus = entity.read(cx).close_button_focus();
            cx.subscribe(&entity, |this, _, event: &HelpDialogEvent, cx| {
                if event.is_close() {
                    this.close_modal(cx);
                }
            })
            .detach();
            self.open_modal_with_focus_and_window(entity, focus, window, cx);
        }
    }

    /// Toggle the standalone about dialog overlay.
    pub fn toggle_about_dialog(&mut self, cx: &mut Context<Self>) {
        self.toggle_about_dialog_with_window(None, cx);
    }

    /// Toggle the standalone about dialog overlay with an optional Window reference.
    pub fn toggle_about_dialog_with_window(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if self.is_modal::<AboutDialog>() {
            self.close_modal(cx);
        } else {
            let entity = cx.new(AboutDialog::new);
            let focus = entity.read(cx).close_button_focus();
            cx.subscribe(&entity, |this, _, event: &AboutDialogEvent, cx| {
                match event {
                    AboutDialogEvent::Close => {
                        this.close_modal(cx);
                    }
                    AboutDialogEvent::OpenUpdateDialog => {
                        this.close_modal(cx);
                        this.toggle_update_dialog(cx);
                    }
                }
            }).detach();
            self.open_modal_with_focus_and_window(entity, focus, window, cx);
        }
    }

    /// Toggle the terminal color scheme manager dialog overlay.
    pub fn toggle_terminal_color_scheme_dialog(&mut self, cx: &mut Context<Self>) {
        toggle_overlay!(
            self,
            cx,
            TerminalColorSchemeDialog,
            TerminalColorSchemeDialogEvent,
            TerminalColorSchemeDialog::new
        );
    }

    /// Toggle the log console overlay (live in-app log viewer as a detached window).
    pub fn toggle_log_console(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.log_console_window_handle {
            let alive = handle.update(cx, |_, window, _cx| {
                window.activate_window();
                window.refresh();
            });
            if alive.is_ok() {
                return;
            }
            self.log_console_window_handle = None;
            self.log_console_entity = None;
        }

        let title = i18n!(cx, "log.title");
        let panel_slot = std::rc::Rc::new(std::cell::RefCell::new(None));
        let panel_slot_capture = panel_slot.clone();

        let handle = crate::app::open_detached_overlay::<LogConsole, LogConsoleEvent>(
            title,
            move |_window, _registry, cx| {
                let entity = cx.new(LogConsole::new);
                *panel_slot_capture.borrow_mut() = Some(entity.clone());
                entity
            },
            crate::app::DetachedOverlayOptions {
                size: gpui::size(gpui::px(1000.0), gpui::px(680.0)),
                min_size: gpui::size(gpui::px(600.0), gpui::px(420.0)),
                on_close: None,
                hide_titlebar: false,
            },
            cx,
        );
        self.log_console_entity = panel_slot.borrow().clone();
        self.log_console_window_handle = handle;
    }


    // ========================================================================
    // Profile manager (switch / create / delete)
    // ========================================================================

    /// Toggle profile manager overlay.
    pub fn toggle_profile_manager(&mut self, cx: &mut Context<Self>) {
        if self.is_modal::<ProfileManager>() {
            self.close_modal(cx);
        } else {
            let manager = cx.new(ProfileManager::new);
            cx.subscribe(&manager, |this, _, event: &ProfileManagerEvent, cx| {
                match event {
                    ProfileManagerEvent::Close => {
                        this.close_modal(cx);
                    }
                    ProfileManagerEvent::SwitchProfile(id) => {
                        cx.emit(OverlayManagerEvent::SwitchProfile(id.clone()));
                        this.close_modal(cx);
                    }
                }
            })
            .detach();
            self.open_modal(manager, cx);
        }
    }

    // ========================================================================
    // Shell selector (parametric)
    // ========================================================================

    /// Show shell selector overlay for a terminal.
    pub fn show_shell_selector(
        &mut self,
        current_shell: ShellType,
        project_id: String,
        terminal_id: String,
        cx: &mut Context<Self>,
    ) {
        let context = Some((project_id.clone(), terminal_id.clone()));
        let entity = cx.new(|cx| ShellSelectorOverlay::new(current_shell, context, cx));
        cx.subscribe(&entity, move |this, _, event: &ShellSelectorOverlayEvent, cx| {
            match event {
                ShellSelectorOverlayEvent::Close => {
                    this.close_modal(cx);
                }
                ShellSelectorOverlayEvent::ShellSelected { shell_type, context } => {
                    if let Some((project_id, terminal_id)) = context {
                        cx.emit(OverlayManagerEvent::ShellSelected {
                            shell_type: shell_type.clone(),
                            project_id: project_id.clone(),
                            terminal_id: terminal_id.clone(),
                        });
                    }
                    this.close_modal(cx);
                }
            }
        }).detach();
        self.open_modal(entity, cx);
    }

    // ========================================================================
    // Rename directory dialog (parametric)
    // ========================================================================

    /// Show rename directory dialog for a project.
    pub fn show_rename_directory_dialog(
        &mut self,
        project_id: String,
        project_path: String,
        cx: &mut Context<Self>,
    ) {
        let entity = cx.new(|cx| RenameDirectoryDialog::new(project_id, project_path, cx));
        cx.subscribe(&entity, |this, _, event: &RenameDirectoryDialogEvent, cx| {
            match event {
                RenameDirectoryDialogEvent::Close => {
                    this.close_modal(cx);
                }
                RenameDirectoryDialogEvent::Renamed { project_id, new_path, new_name } => {
                    this.close_modal(cx);
                    this.workspace.update(cx, |ws, cx| {
                        ws.rename_project_directory(project_id, new_path.clone(), new_name.clone(), cx);
                    });
                }
            }
        }).detach();
        self.open_modal(entity, cx);
    }

    // ========================================================================
    // Context menu (parametric - remains as separate OverlaySlot)
    // ========================================================================

    /// Show context menu for a project.
    pub fn show_context_menu(
        &mut self,
        project_id: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.close_modal(cx);
        self.close_all_context_menus();

        let ws = self.workspace.read(cx);
        let project = ws.project(&project_id);
        let project_name = project.map(|p| p.name.clone()).unwrap_or_default();
        let project_path = project.map(|p| p.path.clone()).unwrap_or_default();
        let is_pinned = project.map(|p| p.pinned).unwrap_or(false);
        let extras_exist = !ws.data().extra_windows.is_empty();
        let is_hidden_in_window = ws
            .data()
            .window(self.window_id)
            .map(|w| w.hidden_project_ids.contains(&project_id))
            .unwrap_or(false);
        let menu = cx.new(|cx| {
            ContextMenu::new(
                project_id,
                position,
                project_name,
                project_path,
                is_pinned,
                extras_exist,
                is_hidden_in_window,
                cx,
            )
        });

        cx.subscribe(&menu, |this, _, event: &ContextMenuEvent, cx| {
            match event {
                ContextMenuEvent::Close => {
                    this.hide_context_menu(cx);
                }
                ContextMenuEvent::AddTerminal { project_id } => {
                    this.hide_context_menu(cx);
                    cx.emit(OverlayManagerEvent::AddTerminal {
                        project_id: project_id.clone(),
                    });
                }
                ContextMenuEvent::RenameProject { project_id, project_name } => {
                    this.hide_context_menu(cx);
                    cx.emit(OverlayManagerEvent::RenameProject {
                        project_id: project_id.clone(),
                        project_name: project_name.clone(),
                    });
                }
                ContextMenuEvent::RenameDirectory { project_id, project_path } => {
                    this.hide_context_menu(cx);
                    cx.emit(OverlayManagerEvent::RenameDirectory {
                        project_id: project_id.clone(),
                        project_path: project_path.clone(),
                    });
                }
                ContextMenuEvent::DeleteProject { project_id } => {
                    this.hide_context_menu(cx);
                    cx.emit(OverlayManagerEvent::DeleteProject {
                        project_id: project_id.clone(),
                    });
                }
                ContextMenuEvent::FocusParent { project_id } => {
                    this.hide_context_menu(cx);
                    cx.emit(OverlayManagerEvent::FocusParent {
                        project_id: project_id.clone(),
                    });
                }
                ContextMenuEvent::CopyPath { .. } => {
                    // Path already copied to clipboard in the handler
                    this.hide_context_menu(cx);
                }
                ContextMenuEvent::FocusProject { project_id } => {
                    this.hide_context_menu(cx);
                    cx.emit(OverlayManagerEvent::FocusProject(project_id.clone()));
                }
                ContextMenuEvent::HideProject { project_id } => {
                    this.hide_context_menu(cx);
                    cx.emit(OverlayManagerEvent::ToggleProjectVisibility(project_id.clone()));
                }
                ContextMenuEvent::TogglePinned { project_id } => {
                    this.hide_context_menu(cx);
                    this.workspace.update(cx, |ws, cx| {
                        ws.toggle_project_pinned(project_id, cx);
                    });
                }
            }
        })
        .detach();

        self.context_menu.set(menu);
        cx.notify();
    }

    /// Hide context menu.
    pub fn hide_context_menu(&mut self, cx: &mut Context<Self>) {
        self.context_menu.close();
        cx.notify();
    }

    /// Show folder context menu.
    pub fn show_folder_context_menu(
        &mut self,
        folder_id: String,
        folder_name: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.close_modal(cx);
        self.close_all_context_menus();

        let ws = self.workspace.read(cx);
        let folder = ws.folder(&folder_id);
        let project_count = folder.map(|f| f.project_ids.len()).unwrap_or(0);
        let is_active_filter = ws.active_folder_filter(self.window_id) == Some(&folder_id);
        let menu = cx.new(|cx| {
            FolderContextMenu::new(
                folder_id,
                folder_name,
                position,
                project_count,
                is_active_filter,
                cx,
            )
        });

        cx.subscribe(&menu, |this, _, event: &FolderContextMenuEvent, cx| {
            match event {
                FolderContextMenuEvent::Close => {
                    this.hide_folder_context_menu(cx);
                }
                FolderContextMenuEvent::RenameFolder { folder_id, folder_name } => {
                    this.hide_folder_context_menu(cx);
                    this.request_broker.update(cx, |broker, cx| {
                        broker.push_sidebar_request(SidebarRequest::RenameFolder {
                            folder_id: folder_id.clone(),
                            folder_name: folder_name.clone(),
                        }, cx);
                    });
                }
                FolderContextMenuEvent::DeleteFolder { folder_id } => {
                    this.hide_folder_context_menu(cx);
                    this.workspace.update(cx, |ws, cx| {
                        ws.delete_folder(folder_id, cx);
                    });
                }
                FolderContextMenuEvent::FilterToFolder { folder_id } => {
                    this.hide_folder_context_menu(cx);
                    let window_id = this.window_id;
                    let workspace = this.workspace.clone();
                    let fid = folder_id.clone();
                    this.focus_manager.update(cx, |fm, cx| {
                        workspace.update(cx, |ws, cx| {
                            ws.toggle_folder_focus(fm, window_id, &fid, cx);
                        });
                        cx.notify();
                    });
                }
            }
        })
        .detach();

        self.folder_context_menu.set(menu);
        cx.notify();
    }

    /// Hide folder context menu.
    pub fn hide_folder_context_menu(&mut self, cx: &mut Context<Self>) {
        self.folder_context_menu.close();
        cx.notify();
    }

    // ========================================================================
    // Terminal context menu (positioned popup)
    // ========================================================================

    /// Show terminal context menu.
    // GPUI overlay setup: params are position/context inputs, not a group.
    #[allow(clippy::too_many_arguments)]
    pub fn show_terminal_context_menu(
        &mut self,
        terminal_id: String,
        project_id: String,
        layout_path: Vec<usize>,
        position: gpui::Point<gpui::Pixels>,
        has_selection: bool,
        selection: String,
        link_url: Option<String>,
        is_recording: bool,
        is_recording_paused: bool,
        word_wrap_enabled: bool,
        line_numbers_enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.close_modal(cx);
        self.close_all_context_menus();
        self.dismiss_terminal_ai_inline(cx);
        self.last_terminal_context = Some((terminal_id.clone(), project_id.clone(), layout_path.clone()));

        let settings = settings_entity(cx).read(cx).settings.clone();
        let ai_enabled = settings.ai_enabled;
        let search_engines: Vec<velowork_workspace::settings::SearchEngineConfig> = settings
            .search_engines
            .iter()
            .filter(|e| e.enabled)
            .cloned()
            .collect();

        let this_weak = cx.entity().downgrade();
        let menu = open_terminal_context_menu(
            terminal_id,
            project_id,
            layout_path,
            position,
            has_selection,
            selection,
            link_url,
            is_recording,
            is_recording_paused,
            word_wrap_enabled,
            line_numbers_enabled,
            ai_enabled,
            search_engines,
            Some(self.overlay_registry.clone()),
            move |event, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| {
                        match event {
                            TerminalContextMenuEvent::Close => {
                                this.hide_terminal_context_menu(cx);
                            }
                            TerminalContextMenuEvent::Copy { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalCopy { terminal_id });
                            }
                            TerminalContextMenuEvent::Paste { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalPaste { terminal_id });
                            }
                            TerminalContextMenuEvent::Clear { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalClear { terminal_id });
                            }
                            TerminalContextMenuEvent::SelectAll { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalSelectAll { terminal_id });
                            }
                            TerminalContextMenuEvent::Split { project_id, layout_path, direction } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalSplit {
                                    project_id,
                                    layout_path,
                                    direction,
                                });
                            }
                            TerminalContextMenuEvent::CloseTerminal { project_id, terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalClose {
                                    project_id,
                                    terminal_id,
                                });
                            }
                            TerminalContextMenuEvent::OpenLink { url } => {
                                this.hide_terminal_context_menu(cx);
                                crate::views::layout::terminal_pane::url_detector::UrlDetector::open_url(&url);
                            }
                            TerminalContextMenuEvent::CopyLink { url } => {
                                this.hide_terminal_context_menu(cx);
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(url));
                            }
                            TerminalContextMenuEvent::ExportSelected { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalExportSelected { terminal_id });
                            }
                            TerminalContextMenuEvent::ExportAll { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalExportAll { terminal_id });
                            }
                            TerminalContextMenuEvent::ShowLogRecordDialog { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::ShowLogRecordDialog { terminal_id });
                            }
                            TerminalContextMenuEvent::LogPause { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalLogPause { terminal_id });
                            }
                            TerminalContextMenuEvent::LogResume { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalLogResume { terminal_id });
                            }
                            TerminalContextMenuEvent::LogStop { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalLogStop { terminal_id });
                            }
                            TerminalContextMenuEvent::LogOpenFile { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalLogOpenFile { terminal_id });
                            }
                            TerminalContextMenuEvent::LogOpenFolder { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalLogOpenFolder { terminal_id });
                            }
                            TerminalContextMenuEvent::AIInterpret { terminal_id, text } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalAIInterpret {
                                    terminal_id,
                                    text,
                                });
                            }
                            TerminalContextMenuEvent::Find { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalFind { terminal_id });
                            }
                            TerminalContextMenuEvent::WebSearch { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalWebSearch { terminal_id });
                            }
                            TerminalContextMenuEvent::SearchWithEngine { terminal_id: _, url } => {
                                this.hide_terminal_context_menu(cx);
                                crate::views::layout::terminal_pane::url_detector::UrlDetector::open_url(&url);
                            }
                            TerminalContextMenuEvent::ToggleWordWrap => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalToggleWordWrap);
                            }
                            TerminalContextMenuEvent::ToggleLineNumbers => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalToggleLineNumbers);
                            }
                            TerminalContextMenuEvent::ClearScrollback { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalClearScrollback { terminal_id });
                            }
                            TerminalContextMenuEvent::ClearAll { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalClearAll { terminal_id });
                            }
                            TerminalContextMenuEvent::EditConfig { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalEditConfig { terminal_id });
                            }
                            TerminalContextMenuEvent::ZmodemUpload { terminal_id } => {
                                this.hide_terminal_context_menu(cx);
                                cx.emit(OverlayManagerEvent::TerminalZmodemUpload { terminal_id });
                            }
                        }
                    });
                }
            },
            cx,
        );

        self.terminal_context_menu.set(menu);
        cx.notify();
    }

    /// Hide terminal context menu.
    pub fn hide_terminal_context_menu(&mut self, cx: &mut Context<Self>) {
        self.terminal_context_menu.close();
        cx.notify();
    }

    /// Get terminal context menu entity for rendering.
    pub fn render_terminal_context_menu(&self) -> Option<Entity<PopupMenu>> {
        self.terminal_context_menu.render()
    }

    // ========================================================================
    // Terminal AI Inline (Floating Toolbar & Popover)
    // ========================================================================

    pub fn show_terminal_ai_floating_toolbar(
        &mut self,
        terminal_id: String,
        project_id: String,
        position: Point<Pixels>,
        selection_text: String,
        snapshot: Option<velowork_ai::TerminalContextSnapshot>,
        cx: &mut Context<Self>,
    ) {
        let settings = settings_entity(cx).read(cx).settings.clone();
        if !settings.ai_enabled || !settings.terminal_ai_floating_toolbar_enabled {
            return;
        }

        let reg = Some(self.overlay_registry.clone());
        let view = cx.new(|cx| {
            TerminalAiInline::new_toolbar(
                terminal_id,
                project_id,
                position,
                selection_text,
                reg,
                snapshot,
                cx,
            )
        });

        self.subscribe_terminal_ai_inline(&view, cx);
        self.terminal_ai_inline.set(view);
        cx.notify();
    }

    pub fn show_terminal_ai_popover(
        &mut self,
        terminal_id: String,
        project_id: String,
        position: Point<Pixels>,
        selection_text: String,
        snapshot: Option<velowork_ai::TerminalContextSnapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = settings_entity(cx).read(cx).settings.clone();
        if !settings.ai_enabled {
            return;
        }

        if self.terminal_ai_inline.is_open() && selection_text.trim().is_empty() {
            if let Some(view) = self.terminal_ai_inline.render() {
                if view.read(cx).terminal_id == terminal_id {
                    view.update(cx, |this, cx| {
                        this.focus_input(window, cx);
                    });
                    cx.notify();
                    return;
                }
            }
        }

        let reg = Some(self.overlay_registry.clone());
        let view = cx.new(|cx| {
            TerminalAiInline::new_popover(
                terminal_id,
                project_id,
                position,
                selection_text,
                reg,
                snapshot,
                cx,
            )
        });

        self.subscribe_terminal_ai_inline(&view, cx);
        self.terminal_ai_inline.set(view.clone());
        view.update(cx, |this, cx| {
            this.focus_input(window, cx);
        });
        cx.notify();
    }

    fn subscribe_terminal_ai_inline(
        &mut self,
        view: &Entity<TerminalAiInline>,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(view, |this: &mut Self, _, event: &TerminalAiInlineEvent, cx| {
            match event {
                TerminalAiInlineEvent::Close => {
                    this.terminal_ai_inline.close();
                    cx.notify();
                }
                _ => {
                    cx.emit(OverlayManagerEvent::TerminalAiInline(event.clone()));
                }
            }
        })
        .detach();
    }

    pub fn dismiss_terminal_ai_inline(&mut self, cx: &mut Context<Self>) {
        if self.terminal_ai_inline.is_open() {
            self.terminal_ai_inline.close();
            cx.notify();
        }
    }

    pub fn has_terminal_ai_inline(&self) -> bool {
        self.terminal_ai_inline.is_open()
    }

    pub fn render_terminal_ai_inline(&self) -> Option<Entity<TerminalAiInline>> {
        self.terminal_ai_inline.render()
    }

    // ========================================================================
    // Tab context menu (positioned popup)
    // ========================================================================

    /// Show tab context menu.
    #[allow(clippy::too_many_arguments)]
    pub fn show_tab_context_menu(
        &mut self,
        tab_index: usize,
        num_tabs: usize,
        project_id: String,
        layout_path: Vec<usize>,
        position: gpui::Point<gpui::Pixels>,
        is_ssh: bool,
        is_minimized: bool,
        minimize_shortcut: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.close_modal(cx);
        self.close_all_context_menus();

        let menu = cx.new(|cx| {
            TabContextMenu::new(
                tab_index,
                num_tabs,
                project_id,
                layout_path,
                position,
                is_ssh,
                is_minimized,
                minimize_shortcut,
                cx,
            )
        });

        cx.subscribe(&menu, |this, _, event: &TabContextMenuEvent, cx| {
            match event {
                TabContextMenuEvent::Close => {
                    this.hide_tab_context_menu(cx);
                }
                TabContextMenuEvent::ToggleMinimize { project_id, layout_path, tab_index } => {
                    this.hide_tab_context_menu(cx);
                    cx.emit(OverlayManagerEvent::TabToggleMinimize {
                        project_id: project_id.clone(),
                        layout_path: layout_path.clone(),
                        tab_index: *tab_index,
                    });
                }
                TabContextMenuEvent::DuplicateSession { project_id, layout_path, tab_index } => {
                    this.hide_tab_context_menu(cx);
                    cx.emit(OverlayManagerEvent::TabDuplicateSession {
                        project_id: project_id.clone(),
                        layout_path: layout_path.clone(),
                        tab_index: *tab_index,
                    });
                }
                TabContextMenuEvent::DuplicateChannel { project_id, layout_path, tab_index } => {
                    this.hide_tab_context_menu(cx);
                    cx.emit(OverlayManagerEvent::TabDuplicateChannel {
                        project_id: project_id.clone(),
                        layout_path: layout_path.clone(),
                        tab_index: *tab_index,
                    });
                }
                TabContextMenuEvent::Reconnect { project_id, layout_path, tab_index } => {
                    this.hide_tab_context_menu(cx);
                    cx.emit(OverlayManagerEvent::TabReconnect {
                        project_id: project_id.clone(),
                        layout_path: layout_path.clone(),
                        tab_index: *tab_index,
                    });
                }
                TabContextMenuEvent::SplitHorizontal { project_id, layout_path, tab_index } => {
                    this.hide_tab_context_menu(cx);
                    cx.emit(OverlayManagerEvent::TabSplitHorizontal {
                        project_id: project_id.clone(),
                        layout_path: layout_path.clone(),
                        tab_index: *tab_index,
                    });
                }
                TabContextMenuEvent::SplitVertical { project_id, layout_path, tab_index } => {
                    this.hide_tab_context_menu(cx);
                    cx.emit(OverlayManagerEvent::TabSplitVertical {
                        project_id: project_id.clone(),
                        layout_path: layout_path.clone(),
                        tab_index: *tab_index,
                    });
                }
                TabContextMenuEvent::CloseTab { project_id, layout_path, tab_index } => {
                    this.hide_tab_context_menu(cx);
                    cx.emit(OverlayManagerEvent::TabClose {
                        project_id: project_id.clone(),
                        layout_path: layout_path.clone(),
                        tab_index: *tab_index,
                    });
                }
                TabContextMenuEvent::CloseOtherTabs { project_id, layout_path, tab_index } => {
                    this.hide_tab_context_menu(cx);
                    this.request_tab_close_others_confirm(project_id.clone(), layout_path.clone(), *tab_index, cx);
                }
                TabContextMenuEvent::CloseTabsToRight { project_id, layout_path, tab_index } => {
                    this.hide_tab_context_menu(cx);
                    this.request_tab_close_to_right_confirm(project_id.clone(), layout_path.clone(), *tab_index, cx);
                }
                TabContextMenuEvent::CloseInactiveTabs { project_id, layout_path } => {
                    this.hide_tab_context_menu(cx);
                    this.request_tab_close_inactive_confirm(project_id.clone(), layout_path.clone(), cx);
                }
                TabContextMenuEvent::SessionSettings { project_id, layout_path, tab_index } => {
                    this.hide_tab_context_menu(cx);
                    cx.emit(OverlayManagerEvent::TabSessionSettings {
                        project_id: project_id.clone(),
                        layout_path: layout_path.clone(),
                        tab_index: *tab_index,
                    });
                }
            }
        })
        .detach();

        self.tab_context_menu.set(menu);
        cx.notify();
    }

    /// Hide tab context menu.
    pub fn hide_tab_context_menu(&mut self, cx: &mut Context<Self>) {
        self.tab_context_menu.close();
        cx.notify();
    }

    /// Get tab context menu entity for rendering.
    pub fn render_tab_context_menu(&self) -> Option<Entity<TabContextMenu>> {
        self.tab_context_menu.render()
    }

    /// Open confirmation dialog before closing a terminal tab view.
    pub fn request_terminal_close_confirm(
        &mut self,
        _project_id: String,
        _terminal_id: String,
        on_confirm: impl FnOnce(&mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                i18n!(cx, "terminal.confirm_close_title"),
                i18n!(cx, "terminal.confirm_close_current_msg"),
                i18n!(cx, "terminal.close_current_view"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(self.overlay_registry.clone()),
                "tab-close-confirm",
            )
            .checkbox(i18n!(cx, "common.dont_ask_again"), false)
            .default_button(velowork_ui::confirm_dialog::ConfirmDialogButton::Confirm)
        });

        let mut on_confirm = Some(on_confirm);
        cx.subscribe(&dialog, move |this, _dialog, event, cx| {
            if let ConfirmDialogEvent::Confirmed { checkbox_checked } = event {
                if *checkbox_checked {
                    crate::settings::settings_entity(cx).update(cx, |s, cx| {
                        s.set_confirm_close_tab(false, cx);
                    });
                }
                if let Some(callback) = on_confirm.take() {
                    callback(cx);
                }
            }
            this.close_modal(cx);
        })
        .detach();

        self.open_modal(dialog, cx);
    }

    /// Open confirmation dialog before closing other tab views.
    pub fn request_tab_close_others_confirm(
        &mut self,
        project_id: String,
        layout_path: Vec<usize>,
        tab_index: usize,
        cx: &mut Context<Self>,
    ) {
        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                i18n!(cx, "terminal.confirm_close_title"),
                i18n!(cx, "terminal.confirm_close_other_msg"),
                i18n!(cx, "terminal.close_other_views"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(self.overlay_registry.clone()),
                "tab-close-others-confirm",
            )
        });

        cx.subscribe(&dialog, move |this, _dialog, event, cx| {
            if matches!(event, ConfirmDialogEvent::Confirmed { .. }) {
                cx.emit(OverlayManagerEvent::TabCloseOthers {
                    project_id: project_id.clone(),
                    layout_path: layout_path.clone(),
                    tab_index,
                });
            }
            this.close_modal(cx);
        })
        .detach();

        self.open_modal(dialog, cx);
    }

    /// Open confirmation dialog before closing tab views to the right.
    pub fn request_tab_close_to_right_confirm(
        &mut self,
        project_id: String,
        layout_path: Vec<usize>,
        tab_index: usize,
        cx: &mut Context<Self>,
    ) {
        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                i18n!(cx, "terminal.confirm_close_title"),
                i18n!(cx, "terminal.confirm_close_right_msg"),
                i18n!(cx, "terminal.close_right_views"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(self.overlay_registry.clone()),
                "tab-close-to-right-confirm",
            )
        });

        cx.subscribe(&dialog, move |this, _dialog, event, cx| {
            if matches!(event, ConfirmDialogEvent::Confirmed { .. }) {
                cx.emit(OverlayManagerEvent::TabCloseToRight {
                    project_id: project_id.clone(),
                    layout_path: layout_path.clone(),
                    tab_index,
                });
            }
            this.close_modal(cx);
        })
        .detach();

        self.open_modal(dialog, cx);
    }

    /// Open confirmation dialog before closing inactive tab views.
    pub fn request_tab_close_inactive_confirm(
        &mut self,
        project_id: String,
        layout_path: Vec<usize>,
        cx: &mut Context<Self>,
    ) {
        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                i18n!(cx, "terminal.confirm_close_title"),
                i18n!(cx, "terminal.confirm_close_inactive_msg"),
                i18n!(cx, "terminal.close_inactive_views"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(self.overlay_registry.clone()),
                "tab-close-inactive-confirm",
            )
        });

        cx.subscribe(&dialog, move |this, _dialog, event, cx| {
            if matches!(event, ConfirmDialogEvent::Confirmed { .. }) {
                cx.emit(OverlayManagerEvent::TabCloseInactive {
                    project_id: project_id.clone(),
                    layout_path: layout_path.clone(),
                });
            }
            this.close_modal(cx);
        })
        .detach();

        self.open_modal(dialog, cx);
    }

    // ========================================================================
    // Quick-command tree context menu + dialogs (positioned popup + modals)
    // ========================================================================

    /// Show the quick-command context menu (right-click on the tree).
    pub fn show_quick_command_context_menu(
        &mut self,
        request: QuickCommandMenuRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_modal(cx);
        self.close_all_context_menus();

        let this_weak = cx.entity().downgrade();
        let menu = open_quick_command_context_menu(
            request,
            Some(self.overlay_registry.clone()),
            move |event, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| match event {
                        QuickCommandContextMenuEvent::Close => {
                            this.hide_quick_command_context_menu(cx);
                        }
                        QuickCommandContextMenuEvent::NewCommand { parent_id } => {
                            this.hide_quick_command_context_menu(cx);
                            this.show_quick_command_dialog(
                                QuickCommandDialogMode::CreateCommand {
                                    parent_id: parent_id.clone(),
                                    initial_content: None,
                                },
                                cx,
                            );
                        }
                        QuickCommandContextMenuEvent::NewFolder { parent_id } => {
                            this.hide_quick_command_context_menu(cx);
                            if let Some(cb) = &this.on_qc_create_folder {
                                cb(parent_id.clone(), cx);
                            } else {
                                this.show_quick_command_dialog(
                                    QuickCommandDialogMode::CreateFolder {
                                        parent_id: parent_id.clone(),
                                    },
                                    cx,
                                );
                            }
                        }
                        QuickCommandContextMenuEvent::Edit { node } => {
                            this.hide_quick_command_context_menu(cx);
                            let node = node.clone();
                            if node.is_folder() {
                                if let QuickCommandNode::Folder { id, name, .. } = &node {
                                    if let Some(cb) = &this.on_qc_rename_folder {
                                        cb(id.clone(), name.clone(), cx);
                                    } else {
                                        this.show_quick_command_dialog(
                                            QuickCommandDialogMode::EditFolder { node },
                                            cx,
                                        );
                                    }
                                }
                            } else {
                                this.show_quick_command_dialog(
                                    QuickCommandDialogMode::EditCommand { node },
                                    cx,
                                );
                            }
                        }
                        QuickCommandContextMenuEvent::Rename { node } => {
                            this.hide_quick_command_context_menu(cx);
                            let node = node.clone();
                            if node.is_folder() {
                                if let QuickCommandNode::Folder { id, name, .. } = &node {
                                    if let Some(cb) = &this.on_qc_rename_folder {
                                        cb(id.clone(), name.clone(), cx);
                                    } else {
                                        this.show_quick_command_dialog(
                                            QuickCommandDialogMode::EditFolder { node },
                                            cx,
                                        );
                                    }
                                }
                            } else {
                                if let QuickCommandNode::Command { id, name, .. } = &node {
                                    if let Some(cb) = &this.on_qc_rename_command {
                                        cb(id.clone(), name.clone(), cx);
                                    } else {
                                        this.show_quick_command_dialog(
                                            QuickCommandDialogMode::EditCommand { node },
                                            cx,
                                        );
                                    }
                                }
                            }
                        }
                        QuickCommandContextMenuEvent::Duplicate { command_id } => {
                            this.hide_quick_command_context_menu(cx);
                            let cid = command_id.clone();
                            let active_pid = this.active_project_id(cx);
                            let settings_entity = crate::settings::settings_entity(cx);
                            let mut new_clone_id = None;
                            settings_entity.update(cx, |s, cx| {
                                let tree = s.settings.quick_commands_for_project_mut(active_pid.as_deref());
                                let parent = qc_parent_id_of(
                                    tree,
                                    &cid,
                                )
                                .flatten();
                                let clone = if let Some(
                                    QuickCommandNode::Command {
                                        name,
                                        command,
                                        variables,
                                        ..
                                    },
                                ) = qc_find_node_mut(
                                    tree,
                                    &cid,
                                ) {
                                    Some(QuickCommandNode::Command {
                                        id: new_quick_command_id(),
                                        name: format!("{} (副本)", name),
                                        command: command.clone(),
                                        variables: variables.clone(),
                                    })
                                } else {
                                    None
                                };
                                if let Some(clone) = clone {
                                    let clone_id = clone.id().to_string();
                                    new_clone_id = Some(clone_id);
                                    if let Some(ref p_id) = parent {
                                        if let Some(pnode) = qc_find_node_mut(tree, p_id) {
                                            if let QuickCommandNode::Folder { expanded, .. } = pnode {
                                                *expanded = true;
                                            }
                                        }
                                    }
                                    qc_insert_node(
                                        tree,
                                        parent.as_deref(),
                                        clone,
                                    );
                                    s.save_and_notify(cx);
                                }
                            });
                            if let Some(clone_id) = new_clone_id {
                                if let Some(cb) = &this.on_qc_created {
                                    cb(clone_id, cx);
                                }
                            }
                        }
                        QuickCommandContextMenuEvent::Delete { ids } => {
                            this.hide_quick_command_context_menu(cx);
                            this.request_quick_command_delete_confirm(ids.clone(), cx);
                        }
                    });
                }
            },
            window,
            cx,
        );

        self.quick_command_context_menu.set(menu);
        cx.notify();
    }

    /// Hide quick-command context menu.
    pub fn hide_quick_command_context_menu(&mut self, cx: &mut Context<Self>) {
        self.quick_command_context_menu.close();
        cx.notify();
    }

    // ─── Quick-command delete confirmation (弹窗确认删除，操作不可逆) ────────

    /// Open a confirmation dialog before deleting one or more quick-command
    /// nodes. The actual deletion happens only after the user confirms.
    pub fn request_quick_command_delete_confirm(
        &mut self,
        ids: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        self.request_quick_command_delete_confirm_with_origin(ids, None, None, cx);
    }

    /// Open confirmation dialog for deleting quick-commands with explicit origin.
    pub fn request_quick_command_delete_confirm_with_origin(
        &mut self,
        ids: Vec<String>,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            return;
        }

        let active_pid = self.active_project_id(cx);
        let (title, message) = {
            let settings_entity = crate::settings::settings_entity(cx);
            let s = settings_entity.read(cx);
            let tree = s
                .settings
                .quick_commands_for_project(active_pid.as_deref());
            if ids.len() == 1 {
                let name = qc_find_node_ref(tree, &ids[0])
                    .map(|n| n.name().to_string())
                    .unwrap_or_default();
                (
                    i18n!(cx, "quick_commands.delete_title"),
                    i18n!(cx, "quick_commands.delete_confirm").replace("{name}", &name),
                )
            } else {
                (
                    i18n!(cx, "quick_commands.delete_title"),
                    i18n!(cx, "quick_commands.delete_confirm_multi")
                        .replace("{count}", &ids.len().to_string()),
                )
            }
        };

        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                title,
                message,
                i18n!(cx, "common.action.delete"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(self.overlay_registry.clone()),
                "quick-command-delete-confirm",
            )
        });

        cx.subscribe(&dialog, {
            let ids = ids.clone();
            move |this, _dialog, event, cx| {
                if matches!(event, ConfirmDialogEvent::Confirmed { .. }) {
                    let ids = ids.clone();
                    let active_pid = this.active_project_id(cx);
                    let settings_entity = crate::settings::settings_entity(cx);
                    settings_entity.update(cx, |s, cx| {
                        let tree =
                            s.settings
                                .quick_commands_for_project_mut(active_pid.as_deref());
                        for id in &ids {
                            qc_remove_node(tree, id);
                        }
                        s.save_and_notify(cx);
                    });
                }
                this.close_modal(cx);
            }
        })
        .detach();

        self.open_modal_with_origin(dialog, origin, panel_container, cx);
    }

    /// Get quick-command context menu entity for rendering.
    pub fn render_quick_command_context_menu(
        &self,
    ) -> Option<Entity<PopupMenu>> {
        self.quick_command_context_menu.render()
    }

    /// Open the quick-command edit/create dialog.
    pub fn show_quick_command_dialog(
        &mut self,
        mode: QuickCommandDialogMode,
        cx: &mut Context<Self>,
    ) {
        self.show_quick_command_dialog_with_origin(mode, None, None, cx);
    }

    /// Open the quick-command edit/create dialog with explicit origin.
    pub fn show_quick_command_dialog_with_origin(
        &mut self,
        mode: QuickCommandDialogMode,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        self.close_all_context_menus();
        let project_id = self.active_project_id(cx);

        let entity = cx.new(|cx| {
            QuickCommandDialog::new(
                mode,
                project_id,
                Some(self.overlay_registry.clone()),
                cx,
            )
        });
        entity.update(cx, |this, cx| this.setup_selects(cx));
        cx.subscribe(&entity, |this, _, event: &QuickCommandDialogEvent, cx| {
            match event {
                QuickCommandDialogEvent::Close => {
                    this.close_modal(cx);
                }
                QuickCommandDialogEvent::Created { id } => {
                    if let Some(cb) = &this.on_qc_created {
                        cb(id.clone(), cx);
                    }
                    this.close_modal(cx);
                }
            }
        })
        .detach();

        self.open_modal_with_origin(entity, origin, panel_container, cx);
    }

    /// Open the quick-command variable-input dialog.
    pub fn show_quick_command_var_dialog(
        &mut self,
        focus_manager: Entity<crate::workspace::focus::FocusManager>,
        workspace: Entity<crate::workspace::state::Workspace>,
        terminals: TerminalsRegistry,
        command_name: String,
        template: String,
        variables: Vec<QuickCommandVar>,
        cx: &mut Context<Self>,
    ) {
        self.show_quick_command_var_dialog_with_origin(
            focus_manager,
            workspace,
            terminals,
            command_name,
            template,
            variables,
            None,
            None,
            cx,
        );
    }

    /// Open the quick-command variable-input dialog with explicit origin.
    pub fn show_quick_command_var_dialog_with_origin(
        &mut self,
        focus_manager: Entity<crate::workspace::focus::FocusManager>,
        workspace: Entity<crate::workspace::state::Workspace>,
        terminals: TerminalsRegistry,
        command_name: String,
        template: String,
        variables: Vec<QuickCommandVar>,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        self.close_all_context_menus();

        let entity = cx.new(|cx| {
            QuickCommandVarDialog::new(
                focus_manager,
                workspace,
                terminals,
                command_name,
                template,
                variables,
                cx,
            )
        });
        cx.subscribe(&entity, |this, _, event: &QuickCommandVarDialogEvent, cx| {
            match event {
                QuickCommandVarDialogEvent::Close => {
                    this.close_modal(cx);
                }
                QuickCommandVarDialogEvent::Executed => {
                    if let Some(entry) = this.modal_stack.last_mut() {
                        entry.origin_focus_handle = None;
                        entry.panel_container_handle = None;
                    }
                    this.close_modal(cx);
                }
            }
        })
        .detach();

        self.open_modal_with_origin(entity, origin, panel_container, cx);
    }

    // ========================================================================
    // Transfer manager popup (positioned popup)
    // ========================================================================

    /// Check if the transfer manager popup is open.
    pub fn has_transfer_popup(&self) -> bool {
        self.transfer_popup.is_open()
    }

    /// Toggle the transfer manager popup. When opening, `anchor` is the
    /// top-right point of the status-bar trigger button (window coordinates);
    /// the popup anchors itself `BottomRight` there so it sits above the button.
    pub fn toggle_transfer_popup(&mut self, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        if self.transfer_popup.is_open() {
            self.hide_transfer_popup(cx);
        } else {
            self.close_modal(cx);
            self.close_all_context_menus();
            let store = cx.global::<GlobalTransferStore>().0.clone();
            let popup = cx.new(|cx| TransferPopup::new(store, anchor, cx));
            cx.subscribe(&popup, |this, _, _event: &TransferPopupEvent, cx| {
                this.hide_transfer_popup(cx);
            })
            .detach();
            self.transfer_popup.set(popup);
            cx.notify();
        }
    }

    /// Hide the transfer manager popup.
    pub fn hide_transfer_popup(&mut self, cx: &mut Context<Self>) {
        self.transfer_popup.close();
        cx.notify();
    }

    /// Get the transfer manager popup entity for rendering.
    pub fn render_transfer_popup(&self) -> Option<Entity<TransferPopup>> {
        self.transfer_popup.render()
    }

    /// Find the screen bounds of a terminal pane by terminal_id.
    pub fn find_terminal_pane_bounds(&self, terminal_id: &str, cx: &App) -> Option<Bounds<Pixels>> {
        let pane_map = velowork_views_terminal::layout::navigation::get_pane_map(self.window_id);

        // 1. Check if we have context from the most recent terminal right-click menu matching this terminal_id:
        if let Some((ref tid, ref pid, ref path)) = self.last_terminal_context {
            if tid == terminal_id {
                if let Some(pane) = pane_map.find_pane(pid, path) {
                    return Some(pane.bounds);
                }
            }
        }

        // 2. Look up the terminal's project and path from workspace layout tree:
        let ws = self.workspace.read(cx);
        if let Some((project_id, path)) = ws.projects().iter().find_map(|p| {
            p.layout.as_ref().and_then(|l| l.find_terminal_path(terminal_id).map(|path| (p.id.clone(), path)))
        }) {
            if let Some(pane) = pane_map.find_pane(&project_id, &path) {
                return Some(pane.bounds);
            }
        }

        // 3. Fallback to any pane registered for this window:
        pane_map.all_panes().first().map(|p| p.bounds)
    }

    /// Compute the target bounds of the floating recording capsule in window coordinates.
    pub fn compute_capsule_target_bounds(&self, terminal_id: &str, cx: &App) -> Bounds<Pixels> {
        let toolbar_w = px(304.0);
        let toolbar_h = px(42.0);
        if let Some(pane_bounds) = self.find_terminal_pane_bounds(terminal_id, cx) {
            let capsule_x = pane_bounds.origin.x + (pane_bounds.size.width - toolbar_w) / 2.0;
            // The floating toolbar is rendered at .top(SPACE_XS) inside terminal-pane-main.
            // SPACE_XS is 4px from the top edge of the terminal pane.
            let capsule_y = pane_bounds.origin.y + velowork_ui::tokens::SPACE_XS;
            Bounds::new(Point::new(capsule_x, capsule_y), Size::new(toolbar_w, toolbar_h))
        } else {
            let fallback_x = px(400.0);
            let fallback_y = px(48.0);
            Bounds::new(Point::new(fallback_x, fallback_y), Size::new(toolbar_w, toolbar_h))
        }
    }

    /// Show log record dialog.
    pub fn show_log_record_dialog(&mut self, terminal_id: String, cx: &mut Context<Self>) {
        let dialog = cx.new(|cx| {
            LogRecordDialog::new(terminal_id, cx)
        });
        cx.subscribe(&dialog, |this, _, event: &LogRecordDialogEvent, cx| {
            match event {
                LogRecordDialogEvent::Close => {
                    this.close_modal(cx);
                }
                LogRecordDialogEvent::StartRecording { terminal_id, filename, append_mode, auto_save_interval } => {
                    let target_bounds = this.compute_capsule_target_bounds(&terminal_id, cx);
                    cx.emit(OverlayManagerEvent::TerminalLogStart {
                        terminal_id: terminal_id.clone(),
                        filename: filename.clone(),
                        append_mode: *append_mode,
                        auto_save_interval: *auto_save_interval,
                    });
                    let preview = cx.new(|_| crate::views::overlays::dialogs::log_record_dialog::LogToolbarPreview);
                    this.close_modal_with_morph_content(target_bounds, Some(preview.into()), cx);
                }
            }
        })
        .detach();
        self.open_modal(dialog, cx);
    }

    /// Show log saved dialog.
    pub fn show_log_saved_dialog(
        &mut self,
        terminal_id: String,
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let dialog = cx.new(|cx| {
            LogSavedDialog::new(terminal_id, path, cx)
        });
        cx.subscribe(&dialog, |this, _, event: &LogSavedDialogEvent, cx| {
            match event {
                LogSavedDialogEvent::Close => {
                    this.close_modal(cx);
                }
                LogSavedDialogEvent::OpenFile { path } => {
                    cx.emit(OverlayManagerEvent::TerminalLogOpenFileWithPath {
                        path: path.clone(),
                    });
                    this.close_modal(cx);
                }
                LogSavedDialogEvent::OpenFolder { path } => {
                    cx.emit(OverlayManagerEvent::TerminalLogOpenFolderWithPath {
                        path: path.clone(),
                    });
                    this.close_modal(cx);
                }
            }
        })
        .detach();
        self.open_modal(dialog, cx);
    }

    // ========================================================================
    // Render helpers (context menus only - modal uses render_modal())
    // ========================================================================

    /// Get context menu entity for rendering.
    pub fn render_context_menu(&self) -> Option<Entity<ContextMenu>> {
        self.context_menu.render()
    }

    /// Get folder context menu entity for rendering.
    pub fn render_folder_context_menu(&self) -> Option<Entity<FolderContextMenu>> {
        self.folder_context_menu.render()
    }

    // ========================================================================
    // Tunnel tree context menu + dialogs (positioned popup + modals)
    // ========================================================================

    /// Check if the tunnel context menu is open.
    pub fn has_tunnel_context_menu(&self) -> bool {
        self.tunnel_context_menu.is_open()
    }

    /// Show the tunnel tree context menu (right-click on the tree).
    pub fn show_tunnel_context_menu(
        &mut self,
        request: TunnelMenuRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_modal(cx);
        self.close_all_context_menus();

        let this_weak = cx.entity().downgrade();
        let menu = open_tunnel_context_menu(
            request,
            Some(self.overlay_registry.clone()),
            move |event, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| match event {
                        TunnelContextMenuEvent::Close => {
                            this.hide_tunnel_context_menu(cx);
                        }
                        TunnelContextMenuEvent::NewTunnel { parent_id } => {
                            this.hide_tunnel_context_menu(cx);
                            let active_pid = this.active_project_id(cx);
                            this.show_tunnel_dialog(
                                TunnelDialogMode::Create {
                                    parent_id: parent_id.clone(),
                                    project_id: active_pid,
                                },
                                cx,
                            );
                        }
                        TunnelContextMenuEvent::NewFolder { parent_id } => {
                            this.hide_tunnel_context_menu(cx);
                            if let Some(cb) = &this.on_tunnel_create_folder {
                                cb(parent_id.clone(), cx);
                            }
                        }
                        TunnelContextMenuEvent::Edit { node } => {
                            this.hide_tunnel_context_menu(cx);
                            let profile = match node {
                                TunnelNode::Tunnel { profile } => profile.clone(),
                                _ => return,
                            };
                            this.show_tunnel_dialog(TunnelDialogMode::Edit { profile }, cx);
                        }
                        TunnelContextMenuEvent::Rename { node } => {
                            this.hide_tunnel_context_menu(cx);
                            match node {
                                TunnelNode::Folder { id, name, .. } => {
                                    if let Some(cb) = &this.on_tunnel_rename_folder {
                                        cb(id.clone(), name.clone(), cx);
                                    }
                                }
                                TunnelNode::Tunnel { profile } => {
                                    if let Some(cb) = &this.on_tunnel_rename {
                                        cb(profile.id.clone(), profile.name.clone(), cx);
                                    } else {
                                        this.show_tunnel_dialog(
                                            TunnelDialogMode::Edit {
                                                profile: profile.clone(),
                                            },
                                            cx,
                                        );
                                    }
                                }
                            }
                        }
                        TunnelContextMenuEvent::Copy { node } => {
                            this.hide_tunnel_context_menu(cx);
                            if let TunnelNode::Tunnel { profile } = node {
                                if let Some(store) = cx.try_global::<GlobalTunnelStore>() {
                                    let store_entity = store.0.clone();
                                    store_entity.update(cx, |s, cx| {
                                        let nodes = s.nodes().to_vec();
                                        let parent = tunnel_parent_id_of(&nodes, &profile.id).flatten();
                                        let new_name = tunnel_unique_duplicate_name(
                                            &nodes,
                                            parent.as_deref(),
                                            &profile.name,
                                        );
                                        let mut new_profile = profile.clone();
                                        new_profile.id = new_tunnel_id();
                                        new_profile.name = new_name;
                                        s.add_tunnel_to(new_profile, parent.as_deref(), cx);
                                    });
                                }
                            }
                        }
                        TunnelContextMenuEvent::Delete { ids } => {
                            this.hide_tunnel_context_menu(cx);
                            this.request_tunnel_delete_confirm(ids.clone(), cx);
                        }
                    });
                }
            },
            window,
            cx,
        );

        self.tunnel_context_menu.set(menu);
        cx.notify();
    }

    /// Hide tunnel context menu.
    pub fn hide_tunnel_context_menu(&mut self, cx: &mut Context<Self>) {
        self.tunnel_context_menu.close();
        cx.notify();
    }

    /// Get tunnel context menu entity for rendering.
    pub fn render_tunnel_context_menu(&self) -> Option<Entity<PopupMenu>> {
        self.tunnel_context_menu.render()
    }

    /// Check if the service context menu is open.
    pub fn has_service_context_menu(&self) -> bool {
        self.service_context_menu.is_open()
    }

    /// Show the service tree context menu (right-click on the tree).
    pub fn show_service_context_menu(
        &mut self,
        request: ServiceMenuRequest,
        terminals: TerminalsRegistry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_service_context_menu_with_origin(request, terminals, None, window, cx);
    }

    /// Show the service tree context menu with explicit origin.
    pub fn show_service_context_menu_with_origin(
        &mut self,
        request: ServiceMenuRequest,
        terminals: TerminalsRegistry,
        origin: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_modal(cx);
        self.close_all_context_menus();

        let this_weak = cx.entity().downgrade();
        let origin_clone = origin.clone();
        let menu = open_service_context_menu(
            request,
            Some(self.overlay_registry.clone()),
            move |event, cx| {
                if let Some(this) = this_weak.upgrade() {
                    let terms = terminals.clone();
                    let origin = origin_clone.clone();
                    this.update(cx, |this, cx| match event {
                        ServiceContextMenuEvent::Close => {
                            this.hide_service_context_menu(cx);
                        }
                        ServiceContextMenuEvent::Op { node, op } => {
                            this.hide_service_context_menu(cx);
                            if let ServiceNode::Service { def } = node {
                                if let Some(cmd) = match op {
                                    velowork_state::ServiceOp::Start => def.effective_start_command(),
                                    velowork_state::ServiceOp::Stop => def.effective_stop_command(),
                                    velowork_state::ServiceOp::Restart => def.effective_restart_command(),
                                } {
                                    let focus_mgr = this.focus_manager.clone();
                                    let ws = this.workspace.clone();
                                    crate::views::panels::quick_commands_panel::send_command_to_focused_terminal(
                                        &focus_mgr,
                                        &ws,
                                        &terms,
                                        &cmd,
                                        cx,
                                    );
                                }
                            }
                        }
                        ServiceContextMenuEvent::SendToTerminal { node } => {
                            this.hide_service_context_menu(cx);
                            if let ServiceNode::Service { def } = node {
                                if let Some(cmd) = def.effective_start_command().or_else(|| def.effective_alive_command()) {
                                    let focus_mgr = this.focus_manager.clone();
                                    let ws = this.workspace.clone();
                                    crate::views::panels::quick_commands_panel::send_command_to_focused_terminal(
                                        &focus_mgr,
                                        &ws,
                                        &terms,
                                        &cmd,
                                        cx,
                                    );
                                }
                            }
                        }
                        ServiceContextMenuEvent::NewService { parent_id } => {
                            this.hide_service_context_menu(cx);
                            this.show_service_dialog_with_origin(
                                ServiceDialogMode::Create { parent_id: parent_id.clone() },
                                None,
                                origin.clone(),
                                origin,
                                cx,
                            );
                        }
                        ServiceContextMenuEvent::NewFolder { parent_id } => {
                            this.hide_service_context_menu(cx);
                            if let Some(cb) = &this.on_service_create_folder {
                                cb(parent_id.clone(), cx);
                            }
                        }
                        ServiceContextMenuEvent::Edit { node } => {
                            this.hide_service_context_menu(cx);
                            if let ServiceNode::Service { def } = node {
                                this.show_service_dialog_with_origin(
                                    ServiceDialogMode::Edit,
                                    Some(def),
                                    origin.clone(),
                                    origin,
                                    cx,
                                );
                            }
                        }
                        ServiceContextMenuEvent::Rename { node } => {
                            this.hide_service_context_menu(cx);
                            match node {
                                ServiceNode::Folder { id, name, .. } => {
                                    if let Some(cb) = &this.on_service_rename_folder {
                                        cb(id.clone(), name.clone(), cx);
                                    }
                                }
                                ServiceNode::Service { def } => {
                                    if let Some(cb) = &this.on_service_rename {
                                        cb(def.id.clone(), def.name.clone(), cx);
                                    } else {
                                        this.show_service_dialog_with_origin(
                                            ServiceDialogMode::Edit,
                                            Some(def),
                                            origin.clone(),
                                            origin,
                                            cx,
                                        );
                                    }
                                }
                            }
                        }
                        ServiceContextMenuEvent::Copy { node } => {
                            this.hide_service_context_menu(cx);
                            if let ServiceNode::Service { def } = node {
                                if let Some(store) = cx.try_global::<GlobalServiceStore>() {
                                    let store_entity = store.0.clone();
                                    store_entity.update(cx, |s, cx| {
                                        let nodes = s.nodes().to_vec();
                                        let parent = service_parent_id_of(&nodes, &def.id).flatten();
                                        let new_name = service_unique_duplicate_name(
                                            &nodes,
                                            parent.as_deref(),
                                            &def.name,
                                        );
                                        let mut new_def = def.clone();
                                        new_def.id = new_service_id();
                                        new_def.name = new_name;
                                        s.upsert(new_def, cx);
                                    });
                                }
                            }
                        }
                        ServiceContextMenuEvent::Delete { ids } => {
                            this.hide_service_context_menu(cx);
                            this.request_service_delete_confirm_with_origin(
                                ids,
                                origin.clone(),
                                origin,
                                cx,
                            );
                        }
                    });
                }
            },
            window,
            cx,
        );

        let origin_clone_for_menu = origin.clone();
        menu.update(cx, |m, _| {
            m.set_previous_focus(origin_clone_for_menu);
        });

        self.service_context_menu.set(menu);
        cx.notify();
    }

    /// Hide service context menu.
    pub fn hide_service_context_menu(&mut self, cx: &mut Context<Self>) {
        self.service_context_menu.close();
        cx.notify();
    }

    /// Get service context menu entity for rendering.
    pub fn render_service_context_menu(&self) -> Option<Entity<PopupMenu>> {
        self.service_context_menu.render()
    }

    /// Request confirmation dialog for service deletion.
    pub fn request_service_delete_confirm(
        &mut self,
        ids: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        self.request_service_delete_confirm_with_origin(ids, None, None, cx);
    }

    /// Request confirmation dialog for service deletion with explicit origin.
    pub fn request_service_delete_confirm_with_origin(
        &mut self,
        ids: Vec<String>,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            return;
        }
        let title = i18n!(cx, "service.confirm_delete");
        let name = if let Some(store) = cx.try_global::<GlobalServiceStore>() {
            let nodes = store.0.read(cx).nodes().to_vec();
            let mut names: Vec<String> = nodes
                .iter()
                .filter(|n| ids.iter().any(|i| i.as_str() == n.id()))
                .map(|n| n.name().to_string())
                .collect();
            names.sort();
            if names.is_empty() {
                ids.join(", ")
            } else if names.len() == 1 {
                names[0].clone()
            } else {
                format!("{}...", names[0])
            }
        } else {
            ids.join(", ")
        };
        let body = i18n!(cx, "service.confirm_delete_msg").replace("{name}", &name);
        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                title,
                body,
                i18n!(cx, "common.action.delete"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(self.overlay_registry.clone()),
                "service-delete-confirm",
            )
        });
        let ids_clone = ids.clone();
        cx.subscribe(&dialog, move |this, _, event: &ConfirmDialogEvent, cx| match event {
            ConfirmDialogEvent::Confirmed { .. } => {
                if let Some(store) = cx.try_global::<GlobalServiceStore>() {
                    let store_entity = store.0.clone();
                    store_entity.update(cx, |s, cx| s.remove_nodes(&ids_clone, cx));
                }
                this.close_modal(cx);
            }
            ConfirmDialogEvent::Cancelled => {
                this.close_modal(cx);
            }
        })
        .detach();
        self.open_modal_with_origin(dialog, origin, panel_container, cx);
    }

    /// Open the tunnel create/edit dialog with explicit origin.
    pub fn show_tunnel_dialog_with_origin(
        &mut self,
        mode: TunnelDialogMode,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        self.close_all_context_menus();

        let mode = match mode {
            TunnelDialogMode::Create { parent_id, project_id } => TunnelDialogMode::Create {
                parent_id,
                project_id: project_id.or_else(|| self.active_project_id(cx)),
            },
            other => other,
        };

        let entity = cx.new(|cx| TunnelDialog::new(mode, Some(self.overlay_registry.clone()), cx));
        entity.update(cx, |this, cx| this.setup_selects(cx));
        cx.subscribe(&entity, |this, _, event: &TunnelDialogEvent, cx| {
            match event {
                TunnelDialogEvent::Close => {
                    this.close_modal(cx);
                }
                TunnelDialogEvent::Saved { .. } => {
                    this.close_modal(cx);
                }
            }
        })
        .detach();

        self.open_modal_with_origin(entity, origin, panel_container, cx);
    }

    /// Open the tunnel create/edit dialog.
    pub fn show_tunnel_dialog(&mut self, mode: TunnelDialogMode, cx: &mut Context<Self>) {
        self.show_tunnel_dialog_with_origin(mode, None, None, cx);
    }

    /// 打开服务监控配置对话框（新增/编辑单个服务）。
    pub fn show_service_dialog(
        &mut self,
        mode: ServiceDialogMode,
        initial: Option<velowork_state::ServiceDefinition>,
        cx: &mut Context<Self>,
    ) {
        self.show_service_dialog_with_origin(mode, initial, None, None, cx);
    }

    /// 打开服务监控配置对话框（新增/编辑单个服务）并显式锚定发起源。
    pub fn show_service_dialog_with_origin(
        &mut self,
        mode: ServiceDialogMode,
        mut initial: Option<velowork_state::ServiceDefinition>,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        self.close_all_context_menus();

        let active_pid = self.active_project_id(cx);
        if let ServiceDialogMode::Create { .. } = mode {
            // 始终确保新建服务归属当前激活项目，避免 project_id 为空导致跨项目可见。
            let mut def = initial.unwrap_or_else(ServiceDefinition::default);
            if def.project_id.is_none() {
                def.project_id = active_pid;
            }
            initial = Some(def);
        }

        let entity = cx.new(|cx| ServiceDialog::new(mode, initial, Some(self.overlay_registry.clone()), cx));
        entity.update(cx, |this, cx| {
            this.setup_selects(cx);
            this.refresh_selects(cx);
        });
        cx.subscribe(&entity, |this, _, event: &ServiceDialogEvent, cx| match event {
            ServiceDialogEvent::Close => {
                this.close_modal(cx);
            }
            ServiceDialogEvent::Saved { def } => {
                let store = cx.try_global::<GlobalServiceStore>().map(|s| s.0.clone());
                if let Some(store) = store {
                    store.update(cx, |s, cx| s.upsert(def.clone(), cx));
                }
                this.close_modal(cx);
            }
        })
        .detach();

        self.open_modal_with_origin(entity, origin, panel_container, cx);
    }

    /// 服务操作执行前的二次确认弹框（受 ServiceCommandPolicy 约束）。
    pub fn request_service_action_confirm<F>(
        &mut self,
        service_name: String,
        op_label: String,
        cmd: String,
        cx: &mut Context<Self>,
        on_confirm: F,
    ) where
        F: FnOnce(&mut App) + Send + Sync + 'static,
    {
        self.close_modal(cx);
        self.close_all_context_menus();

        let title = i18n!(cx, "service.confirm_title");
        let body = format!("{}: {} ({})", op_label, service_name, cmd);
        let overlay_registry = self.overlay_registry.clone();

        let entity = cx.new(|cx| {
            velowork_ui::confirm_dialog::ConfirmDialog::new(
                cx,
                title,
                body,
                i18n!(cx, "common.action.confirm"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(overlay_registry),
                "service-confirm-dialog",
            )
        });

        let mut on_confirm = Some(on_confirm);
        cx.subscribe(&entity, move |this, _, event: &velowork_ui::confirm_dialog::ConfirmDialogEvent, cx| match event {
            velowork_ui::confirm_dialog::ConfirmDialogEvent::Confirmed { .. } => {
                if let Some(f) = on_confirm.take() {
                    cx.spawn(async move |_this, cx| {
                        let _ = cx.update(f);
                    }).detach();
                }
                this.close_modal(cx);
            }
            velowork_ui::confirm_dialog::ConfirmDialogEvent::Cancelled => {
                this.close_modal(cx);
            }
        })
        .detach();

        self.open_modal(entity, cx);
    }

    /// 打开退出应用前的二次确认弹框（当存在活跃会话连接时触发）。
    pub fn open_confirm_quit_dialog<F>(
        &mut self,
        session_count: usize,
        terminal_count: usize,
        cx: &mut Context<Self>,
        on_confirm: F,
    ) where
        F: FnOnce(&mut App) + Send + Sync + 'static,
    {
        self.close_modal(cx);
        self.close_all_context_menus();

        let title = i18n!(cx, "dialog.confirm_quit.title");
        let body = if session_count > 0 {
            // Active SSH sessions take priority in messaging
            if session_count > 1 {
                let count_str = session_count.to_string();
                velowork_i18n::t_fmt(cx, "dialog.confirm_quit.message_with_count", &[("count", &count_str)])
            } else {
                i18n!(cx, "dialog.confirm_quit.message")
            }
        } else {
            // Only terminal tabs open, no active sessions
            if terminal_count > 1 {
                let count_str = terminal_count.to_string();
                velowork_i18n::t_fmt(cx, "dialog.confirm_quit.message_tabs_with_count", &[("count", &count_str)])
            } else {
                i18n!(cx, "dialog.confirm_quit.message_tabs")
            }
        };
        let overlay_registry = self.overlay_registry.clone();

        let entity = cx.new(|cx| {
            velowork_ui::confirm_dialog::ConfirmDialog::new(
                cx,
                title,
                body,
                i18n!(cx, "dialog.confirm_quit.confirm"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(overlay_registry),
                "confirm-quit-dialog",
            )
        });

        let mut on_confirm = Some(on_confirm);
        cx.subscribe(&entity, move |this, _, event: &velowork_ui::confirm_dialog::ConfirmDialogEvent, cx| match event {
            velowork_ui::confirm_dialog::ConfirmDialogEvent::Confirmed { .. } => {
                if let Some(f) = on_confirm.take() {
                    cx.spawn(async move |_this, cx| {
                        let _ = cx.update(f);
                    }).detach();
                }
                this.close_modal(cx);
            }
            velowork_ui::confirm_dialog::ConfirmDialogEvent::Cancelled => {
                this.close_modal(cx);
            }
        })
        .detach();

        self.open_modal(entity, cx);
    }

    /// Open a confirmation dialog before deleting one or more tunnel nodes.
    pub fn request_tunnel_delete_confirm(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        self.request_tunnel_delete_confirm_with_origin(ids, None, None, cx);
    }

    /// Open a confirmation dialog before deleting one or more tunnel nodes with explicit origin.
    pub fn request_tunnel_delete_confirm_with_origin(
        &mut self,
        ids: Vec<String>,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            return;
        }

        let nodes = tunnel_nodes_snapshot(cx);
        let title = i18n!(cx, "tunnel.delete_title");
        let message = if ids.len() == 1 {
            let name = tunnel_find_node_ref(&nodes, &ids[0])
                .map(|n| n.name().to_string())
                .unwrap_or_default();
            i18n!(cx, "tunnel.delete_confirm").replace("{name}", &name)
        } else {
            i18n!(cx, "tunnel.delete_confirm_multi")
                .replace("{count}", &ids.len().to_string())
        };

        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                title,
                message,
                i18n!(cx, "common.action.delete"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(self.overlay_registry.clone()),
                "tunnel-delete-confirm",
            )
        });

        cx.subscribe(&dialog, {
            let ids = ids.clone();
            move |this, _dialog, event, cx| {
                if matches!(event, ConfirmDialogEvent::Confirmed { .. }) {
                    let ids = ids.clone();
                    let engine = cx
                        .try_global::<GlobalTunnelEngine>()
                        .map(|e| e.0.clone());
                    if let Some(store) = cx.try_global::<GlobalTunnelStore>() {
                        let store_entity = store.0.clone();
                        store_entity.update(cx, |s, cx| {
                            for id in &ids {
                                let removed = s.remove_node(id, cx);
                                if let Some(engine) = &engine {
                                    for tid in removed {
                                        engine.stop_tunnel(&tid);
                                    }
                                }
                            }
                        });
                    }
                }
                this.close_modal(cx);
            }
        })
        .detach();

        self.open_modal_with_origin(dialog, origin, panel_container, cx);
    }

    /// Open a confirmation dialog before deleting a session or folder node with explicit origin.
    pub fn request_session_delete_confirm_with_origin(
        &mut self,
        node_id: String,
        node_label: String,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        on_confirmed: Option<std::sync::Arc<dyn Fn(&str, &mut Context<OverlayManager>) + Send + Sync>>,
        cx: &mut Context<Self>,
    ) {
        let message = i18n!(cx, "session.delete_confirm").replace("{name}", &node_label);
        let title = i18n!(cx, "session.delete_title");
        let overlay_registry = self.overlay_registry.clone();

        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                title,
                message,
                i18n!(cx, "common.action.delete"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(overlay_registry),
                "session-delete-confirm",
            )
        });

        let active_pid = self.active_project_id(cx);
        cx.subscribe(&dialog, {
            let node_id = node_id.clone();
            move |this, _dialog, event, cx| {
                if matches!(event, ConfirmDialogEvent::Confirmed { .. }) {
                    if let Some(store) = cx.try_global::<velowork_workspace::stores::GlobalSessionStore>() {
                        let store_entity = store.0.clone();
                        store_entity.update(cx, |s, cx| {
                            s.delete_node_for_project(active_pid.as_deref(), &node_id, cx);
                        });
                    }
                    if let Some(cb) = &on_confirmed {
                        cb(&node_id, cx);
                    }
                }
                this.close_modal(cx);
            }
        })
        .detach();

        self.open_modal_with_origin(dialog, origin, panel_container, cx);
    }

    /// Open confirmation dialog for deleting a command history entry with origin anchoring.
    pub fn show_command_history_delete_confirm_with_origin(
        &mut self,
        command: String,
        origin: Option<FocusHandle>,
        panel_container: Option<FocusHandle>,
        on_confirm: impl FnOnce(&mut App) + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let title = i18n!(cx, "command_history.delete_confirm_title");
        let body = format!("{}\n\n{}", i18n!(cx, "command_history.delete_confirm_message"), command);
        let overlay_registry = self.overlay_registry.clone();

        let entity = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                title,
                body,
                i18n!(cx, "common.action.confirm"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(overlay_registry),
                "history-delete-confirm-dialog",
            )
        });

        let mut on_confirm = Some(on_confirm);
        cx.subscribe(&entity, move |this, _, event: &ConfirmDialogEvent, cx| match event {
            ConfirmDialogEvent::Confirmed { .. } => {
                if let Some(f) = on_confirm.take() {
                    cx.spawn(async move |_this, cx| {
                        let _ = cx.update(f);
                    }).detach();
                }
                this.close_modal(cx);
            }
            ConfirmDialogEvent::Cancelled => {
                this.close_modal(cx);
            }
        })
        .detach();

        self.open_modal_with_origin(entity, origin, panel_container, cx);
    }

    /// Open global confirmation dialog for deleting a command history entry.
    pub fn show_command_history_delete_confirm(
        &mut self,
        command: String,
        on_confirm: impl FnOnce(&mut App) + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.show_command_history_delete_confirm_with_origin(command, None, None, on_confirm, cx);
    }

    /// Open global confirmation dialog for clearing all command history entries for the project.
    pub fn show_command_history_clear_confirm(
        &mut self,
        on_confirm: impl FnOnce(&mut App) + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let title = i18n!(cx, "command_history.clear_confirm_title");
        let body = i18n!(cx, "command_history.clear_confirm_message");
        let overlay_registry = self.overlay_registry.clone();

        let entity = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                title,
                body,
                i18n!(cx, "common.action.confirm"),
                i18n!(cx, "common.action.cancel"),
                true,
                Some(overlay_registry),
                "history-clear-confirm-dialog",
            )
        });

        let mut on_confirm = Some(on_confirm);
        cx.subscribe(&entity, move |this, _, event: &ConfirmDialogEvent, cx| match event {
            ConfirmDialogEvent::Confirmed { .. } => {
                if let Some(f) = on_confirm.take() {
                    cx.spawn(async move |_this, cx| {
                        let _ = cx.update(f);
                    }).detach();
                }
                this.close_modal(cx);
            }
            ConfirmDialogEvent::Cancelled => {
                this.close_modal(cx);
            }
        })
        .detach();

        self.open_modal(entity, cx);
    }
}

fn tunnel_nodes_snapshot(cx: &App) -> Vec<TunnelNode> {
    if let Some(store) = cx.try_global::<GlobalTunnelStore>() {
        store.0.read(cx).nodes().to_vec()
    } else {
        Vec::new()
    }
}

impl EventEmitter<OverlayManagerEvent> for OverlayManager {}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn test_close_detached_windows_idempotent(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            velowork_workspace::init_settings(cx);
            let ws = cx.new(|cx| crate::workspace::state::Workspace::new(cx));
            let om = cx.new(|cx| OverlayManager::new(ws, 1, cx));

            om.update(cx, |this, cx| {
                assert!(this.settings_window_handle.is_none());
                assert!(this.log_console_window_handle.is_none());

                // Closing when already None is a safe no-op
                this.close_detached_windows(cx);
                assert!(this.settings_window_handle.is_none());
                assert!(this.log_console_window_handle.is_none());

                // Confirm dialog check is false when no modal is active
                assert!(!this.is_confirm_dialog_active());
                assert!(this.render_confirm_dialog_modal().is_none());
            });
        });
    }
}
