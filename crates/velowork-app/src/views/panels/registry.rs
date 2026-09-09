use gpui::Entity;
use velowork_terminal::TerminalsRegistry;
use velowork_ui::dock::RightToolbarRegistry;
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_workspace::focus::FocusManager;
use crate::views::overlays::overlay_manager::OverlayManager;
use velowork_workspace::state::Workspace;


/// Creation context holding application-level references required to instantiate panels.
pub struct AppPanelCreationContext {
    pub workspace: Entity<Workspace>,
    pub focus_manager: Entity<FocusManager>,
    pub terminals: TerminalsRegistry,
    pub overlay_manager: Entity<OverlayManager>,
    pub overlay_registry: Entity<OverlayRegistry>,
}

/// Register all default right toolbar panels.
pub fn create_default_right_toolbar_registry() -> RightToolbarRegistry {
    let mut registry = RightToolbarRegistry::new();
    super::ai_assistant_panel::register_toolbar_panel(&mut registry);
    super::quick_commands_panel::register_toolbar_panel(&mut registry);
    super::command_history_panel::register_toolbar_panel(&mut registry);
    super::tunnels_panel::register_toolbar_panel(&mut registry);
    super::service_monitor_panel::register_toolbar_panel(&mut registry);
    registry
}
