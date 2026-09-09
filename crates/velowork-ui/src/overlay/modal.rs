//! Standard modal chrome: header, body, footer and action buttons.
//!
//! # The contract (code *is* the spec)
//!
//! Every container-surface overlay (`Modal`, `Wizard`, `Drawer`, `Inspector`)
//! is composed of exactly three slots, in this order:
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │ [icon] Title            [badges] [toolbar] [⤢] [✕]       │  ModalHeader
//! ├──────────────────────────────────────────────────────────┤
//! │ scrollable content                                       │  modal_body
//! ├──────────────────────────────────────────────────────────┤
//! │ status text                      [Cancel] [Confirm]      │  ModalFooter
//! └──────────────────────────────────────────────────────────┘
//! ```
//!
//! Rules enforced here so no dialog has to re-decide them:
//!
//! - **Title** is `TEXT_MD` / semibold / `text_primary`; **subtitle** is
//!   `TEXT_XS` / `text_muted`; **body** is `TEXT_SM`.
//! - The **✕** and **detach** buttons are icon-only and therefore *always*
//!   carry a translated tooltip (`dialog.close` / `overlay.detach` /
//!   `overlay.redock`), per the project's icon-button rule.
//! - The **primary action is right-most**; destructive actions render in the
//!   danger variant.
//! - Nothing here hardcodes a pixel value: spacing/type/radius come from
//!   `tokens`, colors from [`SemanticPalette`].
//!
//! Controllers only get `&self` while rendering, so all interaction is
//! expressed as plain `Fn(&mut Window, &mut App)` callbacks (typically
//! [`OverlayView::close_callback`] or a `WeakEntity::update`).

use crate::behavior::{HoverBehavior, StatefulElementBehaviorExt};
use crate::button::Button;
use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::styled::{h_flex, v_flex};
use crate::theme::{ThemeColors, bg_opacity, surface_bg, theme, with_alpha};
use crate::tokens::{
    RADIUS_LG, RADIUS_STD, SPACE_LG, SPACE_XL, ui_icon_sm, ui_icon_std, ui_space_lg, ui_space_md,
    ui_space_sm, ui_space_xs, ui_text_md, ui_text_ms, ui_text_sm, ui_text_xl, ui_text_xs,
};
use gpui::prelude::*;
use gpui::*;
use std::rc::Rc;
use velowork_i18n::i18n;

/// A callback usable from inside `render_content`, which only has `&self`.
pub type OverlayAction = Rc<dyn Fn(&mut Window, &mut App)>;

// =============================================================================
// Badges
// =============================================================================

/// Semantic tone of a header badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BadgeTone {
    /// Neutral metadata (e.g. "SSH", "v2").
    #[default]
    Neutral,
    /// Informational (e.g. "Beta").
    Info,
    /// Positive (e.g. "Connected", "Auto Saved").
    Success,
    /// Needs attention (e.g. "Unsaved").
    Warning,
    /// Destructive / failing (e.g. "Disconnected").
    Danger,
}

impl BadgeTone {
    fn color(self, p: &SemanticPalette) -> Hsla {
        match self {
            BadgeTone::Neutral => p.text_muted,
            BadgeTone::Info => p.status_info,
            BadgeTone::Success => p.status_success,
            BadgeTone::Warning => p.status_warning,
            BadgeTone::Danger => p.status_error,
        }
    }
}

/// A small, tinted label shown next to the modal title.
#[derive(Clone, Debug)]
pub struct ModalBadge {
    label: SharedString,
    tone: BadgeTone,
}

impl ModalBadge {
    /// Neutral badge with `label` (translate at the call site).
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            tone: BadgeTone::Neutral,
        }
    }

    /// Set the semantic tone.
    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self
    }

    fn render(self, cx: &App) -> Div {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let color = self.tone.color(&p);
        div()
            .px(ui_space_xs(cx))
            .rounded(RADIUS_STD)
            .border_1()
            .border_color(color.opacity(0.35))
            .text_size(ui_text_xs(cx))
            .text_color(color)
            .child(self.label)
    }
}

// =============================================================================
// Header
// =============================================================================

/// Which window affordance the header should offer.
///
/// `Presentation::Detached` overlays show "dock back", inline ones that *can*
/// be detached show "pop out"; overlays that must stay inline show nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DetachAffordance {
    /// No pop-out control (default).
    #[default]
    None,
    /// Currently inline; offer "pop out to a new window".
    Detach,
    /// Currently in its own window; offer "dock back".
    Redock,
}

/// The standard modal header.
///
/// Build it inside `render_content`, then `.render(cx)`.
pub struct ModalHeader {
    title: SharedString,
    subtitle: Option<SharedString>,
    icon: Option<AppIcon>,
    badges: Vec<ModalBadge>,
    toolbar: Vec<AnyElement>,
    show_close: bool,
    detach: DetachAffordance,
    on_close: Option<OverlayAction>,
    on_detach: Option<OverlayAction>,
}

impl ModalHeader {
    /// Header with a title. Pass an already-translated string.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            icon: None,
            badges: Vec::new(),
            toolbar: Vec::new(),
            show_close: true,
            detach: DetachAffordance::None,
            on_close: None,
            on_detach: None,
        }
    }

    /// Secondary line under the title.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Leading icon.
    pub fn icon(mut self, icon: AppIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Append a badge next to the title.
    pub fn badge(mut self, badge: ModalBadge) -> Self {
        self.badges.push(badge);
        self
    }

    /// Append a custom element to the right-hand toolbar (before ⤢ / ✕).
    pub fn toolbar(mut self, element: impl IntoElement) -> Self {
        self.toolbar.push(element.into_any_element());
        self
    }

    /// Hide the ✕ button (e.g. a blocking wizard step).
    pub fn hide_close(mut self) -> Self {
        self.show_close = false;
        self
    }

    /// Wire the ✕ button.
    pub fn close_with(mut self, on_close: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(on_close));
        self
    }

    /// Offer a pop-out / dock-back button.
    pub fn detachable(
        mut self,
        affordance: DetachAffordance,
        on_detach: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.detach = affordance;
        self.on_detach = Some(Rc::new(on_detach));
        self
    }

    /// Render the header row.
    pub fn render(self, cx: &App) -> Div {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        let mut titles = v_flex().gap(ui_space_xs(cx)).child(
            div()
                .text_size(ui_text_md(cx))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.text_primary)
                .child(self.title),
        );
        if let Some(subtitle) = self.subtitle {
            titles = titles.child(
                div()
                    .text_size(ui_text_xs(cx))
                    .text_color(p.text_muted)
                    .child(subtitle),
            );
        }

        let mut left = h_flex().gap(ui_space_sm(cx)).items_center();
        if let Some(icon) = self.icon {
            left = left.child(icon.size(ui_icon_std(cx)).text_color(p.text_secondary));
        }
        left = left.child(titles);
        for badge in self.badges {
            left = left.child(badge.render(cx));
        }

        let mut right = h_flex().gap(ui_space_xs(cx)).items_center();
        for element in self.toolbar {
            right = right.child(element);
        }
        if self.detach != DetachAffordance::None {
            if let Some(on_detach) = self.on_detach {
                let (icon, tooltip) = match self.detach {
                    DetachAffordance::Redock => (AppIcon::Attach, i18n!(cx, "overlay.redock")),
                    _ => (AppIcon::Detach, i18n!(cx, "overlay.detach")),
                };
                right = right.child(chrome_button(
                    "overlay-header-detach",
                    icon,
                    tooltip,
                    on_detach,
                    cx,
                ));
            }
        }
        if self.show_close {
            if let Some(on_close) = self.on_close {
                right = right.child(chrome_button(
                    "overlay-header-close",
                    AppIcon::Close,
                    i18n!(cx, "common.close"),
                    on_close,
                    cx,
                ));
            }
        }

        h_flex()
            .w_full()
            .items_start()
            .justify_between()
            .gap(ui_space_md(cx))
            .px(ui_space_lg(cx))
            .py(ui_space_md(cx))
            .bg(p.surface_card)
            .rounded_t(RADIUS_LG)
            .border_b_1()
            .border_color(p.border_subtle)
            .child(left)
            .child(right)
    }
}

/// Square icon-only chrome button (✕ / pop-out / month arrows). Always tooltipped.
///
/// Shared by every overlay that needs an icon-only affordance, because the
/// project rule is *icon buttons must have a tooltip* — having one helper makes
/// that impossible to forget.
pub fn chrome_button(
    id: impl Into<ElementId>,
    icon: AppIcon,
    tooltip: String,
    on_click: OverlayAction,
    cx: &App,
) -> Stateful<Div> {
    use crate::behavior::{HoverBehavior, StatefulElementBehaviorExt};
    let t = theme(cx);
    let p = SemanticPalette::from_theme(&t);
    // Hit area = icon + one spacing step on each side; no magic numbers.
    let box_size = ui_icon_sm(cx) + ui_space_sm(cx) * 2.0;

    div()
        .id(id)
        .w(box_size)
        .h(box_size)
        .flex()
        .items_center()
        .justify_center()
        .rounded(RADIUS_STD)
        .cursor_pointer()
        .stateful_behavior(HoverBehavior {
            hover_bg: rgb(t.bg_hover).into(),
            ..Default::default()
        })
        .tooltip(move |_, cx| {
            let tip = tooltip.clone();
            cx.new(|_| crate::Tooltip::new(tip)).into()
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(icon.size(ui_icon_sm(cx)).text_color(p.text_secondary))
}

// =============================================================================
// Body
// =============================================================================

/// Scrollable modal body with the standard padding.
///
/// `id` must be unique within the window (GPUI needs it to track scroll state).
pub fn modal_body(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(ui_space_lg(cx))
        .py(ui_space_md(cx))
        .text_size(ui_text_sm(cx))
}

// =============================================================================
// Actions + Footer
// =============================================================================

/// Visual weight of a footer action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ActionKind {
    /// Neutral / dismissive (Cancel, Back).
    #[default]
    Secondary,
    /// The single affirmative action; rendered right-most.
    Primary,
    /// Destructive affirmative action (Delete).
    Danger,
}

/// One footer button.
pub struct ModalAction {
    id: ElementId,
    label: SharedString,
    kind: ActionKind,
    disabled: bool,
    tooltip: Option<SharedString>,
    on_click: OverlayAction,
}

impl ModalAction {
    /// A secondary action.
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: ActionKind::Secondary,
            disabled: false,
            tooltip: None,
            on_click: Rc::new(on_click),
        }
    }

    /// Mark as the affirmative action.
    pub fn primary(mut self) -> Self {
        self.kind = ActionKind::Primary;
        self
    }

    /// Mark as destructive.
    pub fn danger(mut self) -> Self {
        self.kind = ActionKind::Danger;
        self
    }

    /// Disable (e.g. failing validation).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Explain *why* it is disabled, or add a shortcut hint.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    fn render(self, cx: &App) -> Button {
        let t = theme(cx);
        let on_click = self.on_click;
        let mut button = Button::new(self.id, &t)
            .label(self.label)
            .disabled(self.disabled)
            .on_click(move |_, window, cx| on_click(window, cx));
        button = match self.kind {
            ActionKind::Primary => button.primary(),
            ActionKind::Danger => button.primary().danger(true),
            ActionKind::Secondary => button,
        };
        if let Some(tooltip) = self.tooltip {
            button = button.tooltip(tooltip);
        }
        button
    }
}

/// The right-aligned row of footer buttons.
///
/// Order is preserved, so push dismissive actions first and the primary last.
#[derive(Default)]
pub struct ModalActions {
    actions: Vec<ModalAction>,
}

impl ModalActions {
    /// Empty action row.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an action.
    pub fn push(mut self, action: ModalAction) -> Self {
        self.actions.push(action);
        self
    }

    /// The canonical `[Cancel] [Confirm]` pair used by most dialogs.
    pub fn cancel_confirm(
        confirm_label: impl Into<SharedString>,
        on_cancel: impl Fn(&mut Window, &mut App) + 'static,
        on_confirm: impl Fn(&mut Window, &mut App) + 'static,
        cx: &App,
    ) -> Self {
        Self::new()
            .push(ModalAction::new(
                "overlay-action-cancel",
                i18n!(cx, "common.cancel"),
                on_cancel,
            ))
            .push(
                ModalAction::new("overlay-action-confirm", confirm_label.into(), on_confirm)
                    .primary(),
            )
    }

    /// Whether any action is present.
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Render the row.
    pub fn render(self, cx: &App) -> Div {
        let mut row = h_flex().gap(ui_space_sm(cx)).items_center();
        for action in self.actions {
            row = row.child(action.render(cx));
        }
        row
    }
}

/// The standard modal footer: status on the left, actions on the right.
#[derive(Default)]
pub struct ModalFooter {
    status: Option<AnyElement>,
    status_text: Option<SharedString>,
    status_tone: BadgeTone,
    actions: ModalActions,
}

impl ModalFooter {
    /// Footer with the given actions.
    pub fn new(actions: ModalActions) -> Self {
        Self {
            actions,
            ..Default::default()
        }
    }

    /// Short status line (validation summary, "Modified", "Auto Saved"…).
    pub fn status(mut self, text: impl Into<SharedString>, tone: BadgeTone) -> Self {
        self.status_text = Some(text.into());
        self.status_tone = tone;
        self
    }

    /// Fully custom status area, replacing [`ModalFooter::status`].
    pub fn status_element(mut self, element: impl IntoElement) -> Self {
        self.status = Some(element.into_any_element());
        self
    }

    /// Render the footer, or nothing when it would be empty.
    pub fn render(self, cx: &App) -> Option<Div> {
        if self.status.is_none() && self.status_text.is_none() && self.actions.is_empty() {
            return None;
        }
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        let status_area = match (self.status, self.status_text) {
            (Some(element), _) => div().child(element),
            (None, Some(text)) => div()
                .text_size(ui_text_xs(cx))
                .text_color(self.status_tone.color(&p))
                .child(text),
            _ => div(),
        };

        Some(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap(ui_space_md(cx))
                .px(ui_space_lg(cx))
                .py(ui_space_md(cx))
                .bg(p.surface_card)
                .rounded_b(RADIUS_LG)
                .border_t_1()
                .border_color(p.border_subtle)
                .child(status_area)
                .child(self.actions.render(cx)),
        )
    }
}

/// Assemble header + body + optional footer into the content passed to the shell.
///
/// The shell supplies the card chrome (background, border, radius, shadow,
/// width, backdrop), so this only lays the three slots out vertically.
pub fn modal_frame(header: Div, body: impl IntoElement, footer: Option<Div>) -> Div {
    let mut frame = v_flex().w_full().min_h_0().child(header).child(body);
    if let Some(footer) = footer {
        frame = frame.child(footer);
    }
    frame
}

/// Window corner radius (in px) that full-window dimming masks should clip to,
/// so modal backdrops don't paint square dark tips over the (Transparent)
/// window surface. GPUI's `overflow_hidden` only clips *overflowing*
/// (rectangular) content and never the rounded corner shape, so each mask must
/// round its own background — this global carries the active radius (0.0 when
/// rounded corners are disabled) from the root window view into `modal_backdrop`.
#[derive(Clone, Copy, Default)]
pub struct WindowCornerRadius(pub f32);

impl Global for WindowCornerRadius {}

/// Create a fullscreen overlay that fills the entire window.
///
/// Used for content-heavy views (diff viewer, file viewer) that benefit
/// from maximum screen real estate. No backdrop, no rounded corners.
pub fn fullscreen_overlay(id: impl Into<SharedString>, t: &ThemeColors) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    div()
        .id(ElementId::Name(id.into()))
        .occlude()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .bg(p.surface_base)
        .flex()
        .flex_col()
}

/// Like `fullscreen_overlay`, but sized via `size_full()` instead of
/// absolute positioning. Use when the overlay is hosted inside a parent
/// that owns the layout (e.g. a detached window's content area), where
/// absolute positioning does not interact correctly with flex sizing.
pub fn fullscreen_panel(id: impl Into<SharedString>, t: &ThemeColors) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    div()
        .id(ElementId::Name(id.into()))
        .occlude()
        .size_full()
        .bg(p.surface_base)
        .flex()
        .flex_col()
}

/// A spacer that always fills remaining flex space. When `enabled` is true
/// the spacer also acts as a drag handle for window move, mirroring the
/// main app titlebar: `WindowControlArea::Drag` (HTCAPTION on Windows,
/// platform-native drag on macOS) plus a Linux mouse-move fallback since
/// `WindowControlArea::Drag` is a no-op there.
pub fn window_drag_spacer(enabled: bool) -> Stateful<Div> {
    div()
        .id("window-drag-spacer")
        .flex_1()
        .h_full()
        .when(enabled, |d| {
            d.window_control_area(WindowControlArea::Drag)
                .when(cfg!(target_os = "linux"), |d| {
                    d.on_mouse_down(MouseButton::Left, |_, window, _cx| {
                        window.start_window_move();
                    })
                })
        })
}

/// Whether a detached overlay window should draw its own min/max chrome.
/// Mirrors the rule used by the main app titlebar so detached windows stay
/// consistent: always on Windows, never on macOS (native traffic lights),
/// runtime-determined on Linux.
pub fn detached_needs_controls(window: &Window) -> bool {
    if cfg!(target_os = "windows") {
        true
    } else if cfg!(target_os = "macos") {
        false
    } else {
        matches!(window.window_decorations(), Decorations::Client { .. })
    }
}

/// Render minimize + maximize buttons for a detached overlay window.
/// Returns an empty container when `needs_controls` is false (the OS draws
/// the controls itself, e.g. macOS server-side decorations).
///
/// The close button is intentionally omitted — the host overlay already has
/// its own close button which closes the detached window via `Close` event.
pub fn window_min_max_controls(
    needs_controls: bool,
    is_maximized: bool,
    t: &ThemeColors,
    cx: &App,
) -> Div {
    let t = *t;
    div().when(needs_controls, move |d| {
        d.child(
            h_flex()
                .gap(px(2.0))
                .child(window_chrome_button(
                    "dw-min",
                    "\u{2014}",
                    WindowControlArea::Min,
                    &t,
                    cx,
                    |window| window.minimize_window(),
                ))
                .child(window_chrome_button(
                    "dw-max",
                    if is_maximized { "\u{2750}" } else { "\u{25A1}" },
                    WindowControlArea::Max,
                    &t,
                    cx,
                    |window| window.zoom_window(),
                )),
        )
    })
}

/// Build a single chrome button. On Windows we mark the area with
/// `WindowControlArea` so the OS handles the click natively (matches the
/// main titlebar's behavior); on other platforms we wire a normal click
/// handler.
fn window_chrome_button(
    id: &'static str,
    label: &'static str,
    area: WindowControlArea,
    t: &ThemeColors,
    cx: &App,
    on_activate: fn(&mut Window),
) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    let use_native = cfg!(target_os = "windows");
    div()
        .id(id)
        .cursor_pointer()
        .w(px(28.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(RADIUS_STD)
        .text_size(ui_text_md(cx))
        .text_color(p.text_secondary)
        .stateful_behavior(HoverBehavior {
            hover_bg: p.surface_hover,
            ..Default::default()
        })
        .child(label)
        .when(use_native, |d| d.occlude().window_control_area(area))
        .when(!use_native, |d| {
            d.on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                on_activate(window);
            })
        })
}

/// Create a modal backdrop with click-to-close functionality.
///
/// Returns a positioned div that covers the screen with a semi-transparent overlay.
/// The backdrop handles clicks to close the modal.
///
/// # Example
///
/// ```rust,ignore
/// modal_backdrop("my-modal-backdrop", &t)
///     .items_center() // or .items_start().pt(px(80.0)) for top positioning
///     .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close(cx)))
///     .child(modal_content("my-modal", cx).child(...))
/// ```
pub fn modal_backdrop(id: impl Into<SharedString>, _t: &ThemeColors, cx: &App) -> Stateful<Div> {
    let corner_radius = cx.global::<WindowCornerRadius>().0;

    div()
        .id(ElementId::Name(id.into()))
        .occlude()
        .absolute()
        .inset_0()
        .size_full()
        .p(SPACE_LG)
        .bg(with_alpha(0x000000, 0.35 * bg_opacity(cx)))
        // Clip the dimming mask to the window's rounded corners so it doesn't
        // paint square dark tips over the (Transparent) window surface.
        .when(corner_radius > 0.0, |d| {
            d.rounded(px(corner_radius))
        })
        .flex()
        .items_center()
        .justify_center()
}

/// Create a modal content container with standard styling.
///
/// Returns a styled div with background, border, shadow, and rounded corners.
/// Includes a mouse handler that prevents clicks from propagating to the backdrop.
///
/// The card background uses `surface_bg` so it respects the global
/// `bg_opacity` setting — when the user lowers transparency, every modal that
/// builds its content through this helper becomes translucent (showing the
/// surface behind it) instead of staying fully opaque.
pub fn modal_content(id: impl Into<SharedString>, cx: &App) -> Stateful<Div> {
    let t = theme(cx);
    let p = SemanticPalette::from_context(cx);
    div()
        .id(ElementId::Name(id.into()))
        .bg(surface_bg(t.bg_primary, cx))
        .text_color(p.text_primary)
        .rounded(RADIUS_LG)
        .border_1()
        .border_color(p.border_subtle)
        .shadow_xl()
        .flex()
        .flex_col()
        .max_w_full()
        .max_h_full()
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        // Prevent clicks on content from closing modal
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
}

/// Create a modal header with title, optional subtitle, and close button.
pub fn modal_header<F>(
    title: impl Into<SharedString>,
    subtitle: Option<impl Into<SharedString>>,
    _t: &ThemeColors,
    cx: &App,
    on_close: F,
) -> Stateful<Div>
where
    F: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
{
    let p = SemanticPalette::from_context(cx);
    let title = title.into();
    let subtitle = subtitle.map(|s| s.into());

    let mut title_section = v_flex().gap(px(2.0)).child(
        div()
            .text_size(ui_text_md(cx))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.text_primary)
            .child(title),
    );

    if let Some(subtitle) = subtitle {
        title_section = title_section.child(
            div()
                .text_size(ui_text_ms(cx))
                .text_color(p.text_muted)
                .child(subtitle),
        );
    }

    div()
        .id("modal-header")
        .px(SPACE_XL)
        .py(SPACE_LG)
        .flex()
        .items_center()
        .justify_between()
        .bg(p.surface_card)
        .rounded_t(RADIUS_LG)
        .border_b_1()
        .border_color(p.border_subtle)
        .child(title_section)
        .child(
            div()
                .id("modal-close-btn")
                .cursor_pointer()
                .w(px(28.0))
                .h(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(RADIUS_STD)
                .stateful_behavior(HoverBehavior {
                    hover_bg: p.surface_hover,
                    ..Default::default()
                })
                .text_size(ui_text_xl(cx))
                .text_color(p.text_secondary)
                .child("✕")
                .on_mouse_down(MouseButton::Left, on_close),
        )
}
