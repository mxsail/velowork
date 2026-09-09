use gpui::{AnyElement, Entity, Hsla};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::icon::AppIcon;
use crate::input::InputState;

/// Unique identifier for a business panel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PanelId(pub String);

impl From<&str> for PanelId {
    fn from(s: &str) -> Self {
        PanelId(s.to_string())
    }
}

impl From<String> for PanelId {
    fn from(s: String) -> Self {
        PanelId(s)
    }
}

/// Identifies the kind of business logic a panel carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelKind {
    Terminal,
    Files,
    Database,
    Monitor,
    Sftp,
    Custom,
}

pub use velowork_core::types::DockPosition;

/// Semantic accent color used to tint a toolbar `IconButton` so its state is
/// obvious at a glance (e.g. green = running, yellow = paused, red = stopped).
/// Mapped to the theme's `success` / `warning` / `error` palette so it adapts
/// to light / dark / high-contrast themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccentColor {
    Success,
    Warning,
    Error,
}

/// Represents the visual notification status on a panel tab.
#[derive(Debug, Clone)]
pub enum PanelBadge {
    /// A numeric count (e.g. 3 unread messages or processes).
    Count(usize),
    /// Descriptive text (e.g. "Sync", "Error").
    Text(String),
    /// A status colored indicator dot.
    StatusDot(Hsla),
}

/// Metadata describing capabilities and configurations of a Panel.
#[derive(Debug, Clone)]
pub struct PanelInfo {
    pub id: PanelId,
    pub title: String,
    pub icon: AppIcon,
    pub kind: PanelKind,
    pub closable: bool,
    pub detachable: bool,
    pub maximizable: bool,
    pub resizable: bool,
    pub persistent: bool,
    pub badge: Option<PanelBadge>,
    pub loading: bool,
    pub error: Option<String>,
}

impl PanelInfo {
    pub fn new(id: impl Into<PanelId>, title: impl Into<String>, icon: AppIcon, kind: PanelKind) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon,
            kind,
            closable: true,
            detachable: true,
            maximizable: true,
            resizable: true,
            persistent: false,
            badge: None,
            loading: false,
            error: None,
        }
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }
}

/// Controls how the dock header is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockHeaderMode {
    /// Auto: Shows standard title when <= 1 tab, shows Multi-Tab bar when >= 2 tabs.
    #[default]
    Auto,
    /// TitleOnly: Always render single-panel title bar (for sidebars driven by an external activity bar).
    TitleOnly,
    /// Tabs: Always render multi-tab bar (for multi-task bottom dock).
    Tabs,
}

/// Custom button registered by a business panel to be rendered in the header's right action area.
#[derive(Clone)]
pub struct PanelAction {
    pub icon: AppIcon,
    pub tooltip: String,
    pub callback: Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + Send + Sync>,
}

impl std::fmt::Debug for PanelAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanelAction")
            .field("icon", &self.icon)
            .field("tooltip", &self.tooltip)
            .finish()
    }
}

// ── Toolbar-extension slot types ─────────────────────────────────────────────

/// Each business panel can return a flat list of toolbar items via
/// [`super::panel::Panel::toolbar_elements`]. The [`super::panel::DockPanel`]
/// renders them inline between the tab row and the "more" menu button,
/// letting panels put rich controls (play / pause, repeat input, address
/// bar, …) directly in the header area instead of pushing them into the
/// body.
pub enum ToolbarItem {
    /// An icon-only button with tooltip.
    IconButton {
        icon: AppIcon,
        tooltip: String,
        enabled: bool,
        on_click: Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + Send + Sync>,
        /// Optional semantic accent. When set, the icon is tinted with the
        /// accent color while `active` (or while hovered), giving clear status
        /// feedback for run / pause / stop style controls.
        accent: Option<AccentColor>,
        /// Whether the button currently represents the "active" state. Drives
        /// the persistent accent tint; hover always previews the accent.
        active: bool,
    },
    /// A vertical separator line.
    Separator,
    /// A plain text label.
    Label(String),
    /// A single-line text input (SimpleInputState entity).
    TextInput {
        entity: Entity<InputState>,
        width_px: f32,
        /// Optional non-editable unit suffix rendered after the input box
        /// (e.g. `"s"` for a seconds field). Purely a display hint.
        suffix: Option<String>,
        /// Optional callback fired when the user presses Enter inside the
        /// input (e.g. to submit a path in the SFTP address bar). If `None`
        /// the input is purely passive.
        on_enter: Option<Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + Send + Sync>>,
    },
    /// A fully custom element rendered as-is in the toolbar. Business panels
    /// build it themselves (e.g. a `dropdown_button` trigger) so the dock does
    /// not need a bespoke variant per control. The panel is responsible for
    /// wiring up toggle / bounds-tracking on the element it provides.
    Custom(AnyElement),
}

impl ToolbarItem {
    /// Return the intrinsic estimated width in pixels for header layout budgeting.
    pub fn estimated_width(&self) -> f32 {
        match self {
            ToolbarItem::IconButton { .. } => 28.0,
            ToolbarItem::Separator => 9.0,
            ToolbarItem::Label(lbl) => lbl.chars().count() as f32 * 7.5 + 8.0,
            ToolbarItem::TextInput { width_px, .. } => *width_px + 16.0,
            ToolbarItem::Custom(_) => 100.0,
        }
    }
}

impl std::fmt::Debug for ToolbarItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IconButton { icon, tooltip, enabled, .. } => f
                .debug_struct("IconButton")
                .field("icon", icon)
                .field("tooltip", tooltip)
                .field("enabled", enabled)
                .finish(),
            Self::Separator => f.debug_struct("Separator").finish(),
            Self::Label(s) => f.debug_tuple("Label").field(s).finish(),
            Self::TextInput { width_px, suffix, .. } => f
                .debug_struct("TextInput")
                .field("width_px", width_px)
                .field("suffix", suffix)
                .finish(),
            Self::Custom(_) => f.debug_struct("Custom").finish(),
        }
    }
}

/// Display mode for the DockPanel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelMode {
    /// Rendered docked alongside other panes.
    Normal,
    /// Maximized within the docking region (hides other dock slots).
    Maximized,
    /// Floated as a separate view layer or native window.
    Detached,
    /// Fullscreen covering all sidebars, statusbar, and other layout regions.
    Fullscreen,
}

/// Collapse state of a docked region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelCollapseState {
    /// Fully visible.
    Normal,
    /// Collapsed to its header only.
    Collapsed,
    /// Completely hidden.
    Hidden,
}

/// The resizable edges of a DockPanel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Lifecycle events dispatched to business panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelLifecycleEvent {
    Open,
    Close,
    Focus,
    Blur,
    Show,
    Hide,
    Resize,
    TabChanged,
    Detach,
    Attach,
}

/// Holds state for an active drag-resize operation.
#[derive(Debug, Clone, Copy)]
pub struct ResizeDragState {
    pub edge: ResizeEdge,
    pub start_mouse_pos: gpui::Point<gpui::Pixels>,
    pub start_size: f32,
}

/// Describes a panel that can be toggled into a [`super::panel::DockPanel`]
/// through its "more" menu. Only metadata is stored here so `velowork-ui`
/// stays decoupled from concrete panel implementations living in `velowork-app`.
///
/// The actual panel instance is produced on demand by the
/// `DockPanel::on_request_panel` callback supplied by the app layer.
#[derive(Clone)]
pub struct PanelProvider {
    /// Stable id, must match the `PanelInfo::id` of the produced panel
    /// (e.g. `"quick_commands"`, `"ai_assistant"`).
    pub id: String,
    /// Human-readable label shown in the picker (already localized).
    pub title: String,
    /// Icon shown next to the label.
    pub icon: AppIcon,
}

