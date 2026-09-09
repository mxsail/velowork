//! Shared header action buttons for terminal panes and tab groups.
//!
//! This module provides reusable button definitions to ensure consistency
//! between regular terminal mode and tab group mode.

use crate::design::appearance::{ControlAppearance, ControlSize, ControlVariant};
use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::theme::ThemeColors;
use crate::tooltip::Tooltip;
use gpui::*;
use velowork_i18n::i18n;

/// All available header actions for terminal panes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderAction {
    SplitVertical,
    SplitHorizontal,
    AddTab,
    Minimize,
    ExportBuffer,
    Fullscreen,
    Detach,
    Close,
    ZoomPrev,
    ZoomNext,
    ExitZoom,
    Sftp,
    CommandPalette,
    Search,
    MoreMenu,
    TabList,
}

impl HeaderAction {
    /// Returns the canonical [`AppIcon`] for this action.
    pub fn app_icon(&self) -> AppIcon {
        match self {
            HeaderAction::SplitVertical => AppIcon::SplitVertical,
            HeaderAction::SplitHorizontal => AppIcon::SplitHorizontal,
            HeaderAction::AddTab => AppIcon::Plus,
            HeaderAction::Minimize => AppIcon::Minimize,
            HeaderAction::ExportBuffer => AppIcon::Copy,
            HeaderAction::Fullscreen => AppIcon::Fullscreen,
            HeaderAction::Detach => AppIcon::Detach,
            HeaderAction::Close => AppIcon::Close,
            HeaderAction::ZoomPrev => AppIcon::ChevronLeft,
            HeaderAction::ZoomNext => AppIcon::ChevronRight,
            HeaderAction::ExitZoom => AppIcon::FullscreenExit,
            HeaderAction::Sftp => AppIcon::Folder,
            HeaderAction::CommandPalette => AppIcon::Keyboard,
            HeaderAction::Search => AppIcon::Search,
            HeaderAction::MoreMenu => AppIcon::MoreMenu,
            HeaderAction::TabList => AppIcon::Layers2,
        }
    }

    /// Returns the i18n key for the default tooltip text of this action.
    ///
    /// The actual translation happens in [`header_button_base`] via the `i18n!`
    /// macro. Callers that pass a `tooltip_override` supply an already-translated
    /// [`SharedString`] and bypass this key.
    pub fn tooltip(&self) -> &'static str {
        match self {
            HeaderAction::SplitVertical => "terminal.split_vertical",
            HeaderAction::SplitHorizontal => "terminal.split_horizontal",
            HeaderAction::AddTab => "terminal.add_tab",
            HeaderAction::Minimize => "terminal.minimize",
            HeaderAction::ExportBuffer => "terminal.export_buffer",
            HeaderAction::Fullscreen => "terminal.fullscreen",
            HeaderAction::Detach => "terminal.detach",
            HeaderAction::Close => "terminal.close",
            HeaderAction::ZoomPrev => "terminal.zoom_prev",
            HeaderAction::ZoomNext => "terminal.zoom_next",
            HeaderAction::ExitZoom => "terminal.exit_zoom",
            HeaderAction::Sftp => "terminal.sftp",
            HeaderAction::CommandPalette => "terminal.command_palette",
            HeaderAction::Search => "terminal.search",
            HeaderAction::MoreMenu => "common.more",
            HeaderAction::TabList => "terminal.tab_list",
        }
    }

    /// Returns true if this is a close/exit action (for red hover styling).
    pub fn is_close(&self) -> bool {
        matches!(self, HeaderAction::Close | HeaderAction::ExitZoom)
    }

    /// Returns the element ID prefix for this action.
    pub fn id_prefix(&self) -> &'static str {
        match self {
            HeaderAction::SplitVertical => "split-vertical-btn",
            HeaderAction::SplitHorizontal => "split-horizontal-btn",
            HeaderAction::AddTab => "add-tab-btn",
            HeaderAction::Minimize => "minimize-btn",
            HeaderAction::ExportBuffer => "export-buffer-btn",
            HeaderAction::Fullscreen => "fullscreen-btn",
            HeaderAction::Detach => "detach-btn",
            HeaderAction::Close => "close-btn",
            HeaderAction::ZoomPrev => "zoom-prev-btn",
            HeaderAction::ZoomNext => "zoom-next-btn",
            HeaderAction::ExitZoom => "zoom-exit-btn",
            HeaderAction::Sftp => "sftp-btn",
            HeaderAction::CommandPalette => "cmd-palette-btn",
            HeaderAction::Search => "search-btn",
            HeaderAction::MoreMenu => "more-action-btn",
            HeaderAction::TabList => "tab-list-btn",
        }
    }
}

/// Renders a header button base element without click handler.
/// The caller should attach `.on_click()` to handle the action.
///
/// Dimensions, padding, font scale, and colors are automatically derived from
/// [`ControlAppearance`] using [`ControlSize::Compact`].
///
/// # Arguments
/// * `action` - The action this button represents
/// * `id_suffix` - Unique suffix for the element ID
/// * `t` - Theme reference
/// * `tooltip_override` - Optional tooltip text override (e.g., "Close Tab" instead of "Close")
/// * `gpui_action` - Optional GPUI action for keybinding display in tooltips
pub fn header_button_base(
    action: HeaderAction,
    id_suffix: &str,
    t: &ThemeColors,
    tooltip_override: Option<SharedString>,
    gpui_action: Option<Box<dyn Action>>,
    cx: &App,
) -> Stateful<Div> {
    let default_tooltip_key = action.tooltip();

    let palette = SemanticPalette::from_context(cx);
    let variant = if action.is_close() {
        ControlVariant::Danger
    } else {
        ControlVariant::Ghost
    };
    let appearance = ControlAppearance::resolve(
        ControlSize::Compact,
        variant,
        &palette,
        crate::tokens::get_ui_density(cx),
        crate::tokens::ui_text_scale(cx),
    );

    let hover_bg = if action.is_close() {
        rgba(0xf14c4c99).into()
    } else {
        appearance.bg_hover
    };

    let group_id = SharedString::from(format!("{}-{}", action.id_prefix(), id_suffix));
    let _is_close = action.is_close();

    let icon_el = action
        .app_icon()
        .size(appearance.icon_size)
        .text_color(rgb(t.text_secondary))
        .group_hover(group_id.clone(), |s| s.text_color(palette.text_primary));

    let base = div()
        .id(group_id.clone())
        .group(group_id)
        .cursor_pointer()
        .w(appearance.height)
        .h(appearance.height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(appearance.radius)
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .hover(|s| s.bg(hover_bg))
        .child(icon_el);

    base.tooltip(move |_, cx| {
        let tooltip_text = match tooltip_override {
            Some(ref override_text) => override_text.to_string(),
            None => i18n!(cx, default_tooltip_key),
        };
        let mut tooltip = Tooltip::new(tooltip_text);
        if let Some(ref action) = gpui_action {
            tooltip = tooltip.action(action.as_ref(), None);
        }
        cx.new(|_| tooltip).into()
    })
}
