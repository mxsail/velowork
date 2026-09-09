use std::any::Any;
use std::sync::Arc;
use gpui::{App, Window};
use velowork_i18n::i18n;

use crate::icon::AppIcon;
use super::panel::AnyPanel;
use super::types::PanelProvider;

/// Type-erased context passed to panel factory closures to instantiate panel views.
#[derive(Clone)]
pub struct PanelCreationContext {
    data: Arc<dyn Any + Send + Sync>,
}

impl PanelCreationContext {
    pub fn new<T: Send + Sync + 'static>(data: T) -> Self {
        Self {
            data: Arc::new(data),
        }
    }

    pub fn downcast_ref<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.data.downcast_ref::<T>()
    }
}

/// Specification for a right toolbar panel registration.
#[derive(Clone)]
pub struct ToolbarPanelSpec {
    /// Unique identifier for the panel (e.g., "ai_assistant", "quick_commands", "tunnels", "services").
    pub id: String,
    /// Icon displayed on the right toolbar.
    pub icon: AppIcon,
    /// Translation key for i18n label/tooltip (e.g., "dock.panel.ai_assistant").
    pub title_key: String,
    /// Display order on the toolbar (lower numbers appear first).
    pub order: i32,
    /// Dynamic predicate function determining if the toolbar button should be visible.
    /// Evaluated against `&App` context so it can read settings or internal state.
    pub is_visible: Arc<dyn Fn(&App) -> bool + Send + Sync>,
    /// Factory closure to create the panel view on demand.
    pub factory: Arc<dyn Fn(&PanelCreationContext, &mut Window, &mut App) -> AnyPanel + Send + Sync>,
}

/// Registry holding all registered toolbar panels.
#[derive(Default, Clone)]
pub struct RightToolbarRegistry {
    panels: Vec<ToolbarPanelSpec>,
}

impl RightToolbarRegistry {
    pub fn new() -> Self {
        Self { panels: Vec::new() }
    }

    /// Register a new panel to the right toolbar.
    pub fn register(&mut self, spec: ToolbarPanelSpec) {
        self.panels.retain(|p| p.id != spec.id);
        self.panels.push(spec);
        self.panels.sort_by_key(|p| p.order);
    }

    /// Get all visible panels based on their individual `is_visible` predicate.
    pub fn visible_panels(&self, cx: &App) -> Vec<&ToolbarPanelSpec> {
        self.panels
            .iter()
            .filter(|spec| (spec.is_visible)(cx))
            .collect()
    }

    /// Check if a specific panel is registered and currently visible.
    pub fn is_panel_visible(&self, id: &str, cx: &App) -> bool {
        self.panels
            .iter()
            .find(|p| p.id == id)
            .map_or(false, |spec| (spec.is_visible)(cx))
    }

    /// Build PanelProviders for dock "more" menu based on currently visible registered panels.
    pub fn panel_providers(&self, cx: &App) -> Vec<PanelProvider> {
        self.visible_panels(cx)
            .into_iter()
            .map(|spec| PanelProvider {
                id: spec.id.clone(),
                title: i18n!(cx, &spec.title_key),
                icon: spec.icon,
            })
            .collect()
    }

    /// Create a panel instance by ID.
    pub fn create_panel(
        &self,
        id: &str,
        ctx: &PanelCreationContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyPanel> {
        self.panels
            .iter()
            .find(|p| p.id == id)
            .map(|spec| (spec.factory)(ctx, window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_panel_creation_context_downcast() {
        struct DummyData {
            name: String,
        }
        let ctx = PanelCreationContext::new(DummyData {
            name: "test".to_string(),
        });
        assert_eq!(ctx.downcast_ref::<DummyData>().unwrap().name, "test");
        assert!(ctx.downcast_ref::<String>().is_none());
    }

    #[gpui::test]
    fn test_registry_ordering_and_filtering(cx: &mut gpui::TestAppContext) {
        let ai_enabled = Arc::new(AtomicBool::new(false));
        let ai_enabled_clone = ai_enabled.clone();

        let mut registry = RightToolbarRegistry::new();

        registry.register(ToolbarPanelSpec {
            id: "quick_commands".to_string(),
            icon: AppIcon::QuickCommand,
            title_key: "dock.panel.quick_commands".to_string(),
            order: 20,
            is_visible: Arc::new(|_| true),
            factory: Arc::new(|_, _, _| panic!("not invoked")),
        });

        registry.register(ToolbarPanelSpec {
            id: "ai_assistant".to_string(),
            icon: AppIcon::AiAssistant,
            title_key: "dock.panel.ai_assistant".to_string(),
            order: 10,
            is_visible: Arc::new(move |_| ai_enabled_clone.load(Ordering::Relaxed)),
            factory: Arc::new(|_, _, _| panic!("not invoked")),
        });

        // Verify sorted by order
        assert_eq!(registry.panels.len(), 2);
        assert_eq!(registry.panels[0].id, "ai_assistant"); // order 10
        assert_eq!(registry.panels[1].id, "quick_commands"); // order 20

        cx.update(|cx| {
            // When ai_enabled is false
            ai_enabled.store(false, Ordering::Relaxed);
            let visible = registry.visible_panels(cx);
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].id, "quick_commands");
            assert!(!registry.is_panel_visible("ai_assistant", cx));
            assert!(registry.is_panel_visible("quick_commands", cx));

            // When ai_enabled becomes true
            ai_enabled.store(true, Ordering::Relaxed);
            let visible = registry.visible_panels(cx);
            assert_eq!(visible.len(), 2);
            assert_eq!(visible[0].id, "ai_assistant");
            assert_eq!(visible[1].id, "quick_commands");
            assert!(registry.is_panel_visible("ai_assistant", cx));
            assert!(registry.is_panel_visible("quick_commands", cx));
        });
    }
}


