use crate::icon::AppIcon;
use gpui::*;
use std::sync::Arc;

/// Data entry model for items rendered inside `PopupMenu`.
#[derive(Clone)]
pub enum PopupMenuItem {
    /// Standard menu item with optional icon, shortcut, check state, and action.
    Item(MenuItemData),
    /// Submenu containing child items, expanded on hover or Right arrow key.
    Submenu(SubmenuData),
    /// Visual separator line between menu sections.
    Separator,
    /// Non-interactive group label or section header.
    Label(SharedString),
    /// Custom element rendered directly inside the menu surface.
    Custom(Arc<dyn Fn(&mut Window, &mut App) -> AnyElement + Send + Sync>),
}

#[derive(Clone)]
pub struct MenuItemData {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: Option<AppIcon>,
    pub shortcut: Option<SharedString>,
    pub checked: Option<bool>,
    pub disabled: bool,
    pub text_color: Option<u32>,
    pub icon_color: Option<u32>,
    pub action: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
}

#[derive(Clone)]
pub struct SubmenuData {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: Option<AppIcon>,
    pub shortcut: Option<SharedString>,
    pub disabled: bool,
    pub text_color: Option<u32>,
    pub icon_color: Option<u32>,
    pub items: Vec<PopupMenuItem>,
}

impl PopupMenuItem {
    /// Create a standard menu item with a label and click action callback.
    pub fn item(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        action: impl Fn(&mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        Self::Item(MenuItemData {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            checked: None,
            disabled: false,
            text_color: None,
            icon_color: None,
            action: Arc::new(action),
        })
    }

    /// Create a submenu item with child items.
    pub fn submenu(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        items: Vec<PopupMenuItem>,
    ) -> Self {
        Self::Submenu(SubmenuData {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            disabled: false,
            text_color: None,
            icon_color: None,
            items,
        })
    }

    /// Create a visual separator line.
    pub fn separator() -> Self {
        Self::Separator
    }

    /// Create a section header label.
    pub fn label(text: impl Into<SharedString>) -> Self {
        Self::Label(text.into())
    }

    /// Create a menu entry with a custom element builder.
    pub fn custom(
        builder: impl Fn(&mut Window, &mut App) -> AnyElement + Send + Sync + 'static,
    ) -> Self {
        Self::Custom(Arc::new(builder))
    }

    /// Attach an icon to this menu item or submenu.
    pub fn icon(mut self, icon: AppIcon) -> Self {
        match &mut self {
            Self::Item(data) => data.icon = Some(icon),
            Self::Submenu(data) => data.icon = Some(icon),
            _ => {}
        }
        self
    }

    /// Attach a keyboard shortcut text to this menu item or submenu.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        let s = shortcut.into();
        match &mut self {
            Self::Item(data) => data.shortcut = Some(s),
            Self::Submenu(data) => data.shortcut = Some(s),
            _ => {}
        }
        self
    }

    /// Set checkbox state for this menu item.
    pub fn checked(mut self, checked: bool) -> Self {
        if let Self::Item(data) = &mut self {
            data.checked = Some(checked);
        }
        self
    }

    /// Set disabled state for this menu item or submenu.
    pub fn disabled(mut self, disabled: bool) -> Self {
        match &mut self {
            Self::Item(data) => data.disabled = disabled,
            Self::Submenu(data) => data.disabled = disabled,
            _ => {}
        }
        self
    }

    /// Custom text and icon color.
    pub fn text_color(mut self, color: u32) -> Self {
        match &mut self {
            Self::Item(data) => data.text_color = Some(color),
            Self::Submenu(data) => data.text_color = Some(color),
            _ => {}
        }
        self
    }

    /// Custom icon color.
    pub fn icon_color(mut self, color: u32) -> Self {
        match &mut self {
            Self::Item(data) => data.icon_color = Some(color),
            Self::Submenu(data) => data.icon_color = Some(color),
            _ => {}
        }
        self
    }
}
