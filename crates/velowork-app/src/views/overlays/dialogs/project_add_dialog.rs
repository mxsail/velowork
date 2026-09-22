//! Add and edit project modal dialog overlay.

use crate::keybindings::Cancel;
use crate::theme::{theme, with_alpha};
use crate::ui::tokens::ui_text_md;
use crate::views::components::{labeled_input, modal_content};
use crate::workspace::state::{WindowId, Workspace};
use gpui::prelude::*;
use gpui::*;
use velowork_core::theme::FolderColor;
use velowork_i18n::i18n;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::dialog_actions::dialog_actions_extended;
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::h_flex;
use velowork_ui::icon::AppIcon;
use velowork_ui::input::{InputEvent, InputState, KeyInterceptResult, TextareaState};
use velowork_ui::scrollable::ScrollableElement;
use velowork_ui::tokens::{RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::v_flex;

/// Selectable project icons, matching the prototype's `.workspace-icon-btn`
/// row (`data-icon` values). Each name maps to `assets/icons/{name}.svg`.
pub const PROJECT_ICONS: &[AppIcon] = &[
    AppIcon::Monitor,
    AppIcon::Server,
    AppIcon::Code,
    AppIcon::Database,
    AppIcon::Cloud,
    AppIcon::Folder,
];

/// Selectable project colors, matching the prototype's `.workspace-color-btn`
/// swatches. Rendered through the theme system (`get_folder_color`) so nothing
/// is hardcoded, and persisted via `Workspace::set_folder_color`.
pub const PROJECT_COLORS: &[FolderColor] = &[
    FolderColor::Indigo,
    FolderColor::Green,
    FolderColor::Yellow,
    FolderColor::Red,
    FolderColor::Cyan,
    FolderColor::Purple,
    FolderColor::Orange,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectFormMode {
    Create,
    Edit { project_id: String },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingFocus {
    Name,
    IconGroup,
    ColorGroup,
    Description,
    Cancel,
    Confirm,
}

pub struct AddProjectDialog {
    workspace: Entity<Workspace>,
    window_id: WindowId,
    mode: ProjectFormMode,
    focus_handle: FocusHandle,
    icon_group_focus: FocusHandle,
    color_group_focus: FocusHandle,
    cancel_focus: FocusHandle,
    confirm_focus: FocusHandle,
    name_input: Entity<InputState>,
    desc_input: Entity<TextareaState>,
    _name_sub: Option<Subscription>,
    initial_focus_done: bool,
    pending_focus: Option<PendingFocus>,
    selected_icon: AppIcon,
    selected_color: FolderColor,
}

#[derive(Clone, Debug)]
pub enum AddProjectDialogEvent {
    Close,
    Saved { project_id: String, is_new: bool },
}

impl EventEmitter<AddProjectDialogEvent> for AddProjectDialog {}


impl AddProjectDialog {
    /// Create in default Create mode (backward compatible with OverlayManager).
    pub fn new(
        workspace: Entity<Workspace>,
        window_id: WindowId,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_create(workspace, window_id, cx)
    }

    /// Create in new project mode.
    pub fn new_create(
        workspace: Entity<Workspace>,
        window_id: WindowId,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_placeholder = i18n!(cx, "project.editor.name_placeholder");
        let name_input = cx.new(|cx| InputState::new(cx).placeholder(name_placeholder));
        let desc_placeholder = i18n!(cx, "project.editor.desc_placeholder");
        let desc_input = cx.new(|cx| {
            TextareaState::new(cx)
                .multiline()
                .placeholder(desc_placeholder)
        });

        Self::init(
            workspace,
            window_id,
            ProjectFormMode::Create,
            name_input,
            desc_input,
            AppIcon::Monitor,
            FolderColor::Indigo,
            cx,
        )
    }

    /// Create in edit existing project mode.
    pub fn new_edit(
        workspace: Entity<Workspace>,
        window_id: WindowId,
        project_id: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        let (name, icon_name, color, description) = workspace
            .read(cx)
            .project(project_id)
            .map(|p| {
                (
                    p.name.clone(),
                    p.icon.clone(),
                    p.folder_color,
                    p.description.clone(),
                )
            })
            .unwrap_or_default();

        let name_placeholder = i18n!(cx, "project.editor.name_placeholder");
        let name_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(name_placeholder)
                .default_value(&name)
        });
        let desc_placeholder = i18n!(cx, "project.editor.desc_placeholder");
        let desc_input = cx.new(|cx| {
            TextareaState::new(cx)
                .multiline()
                .placeholder(desc_placeholder)
                .default_value(&description)
        });

        let selected_icon = if icon_name.is_empty() {
            AppIcon::Monitor
        } else {
            AppIcon::from_str(&icon_name).unwrap_or(AppIcon::Monitor)
        };

        Self::init(
            workspace,
            window_id,
            ProjectFormMode::Edit {
                project_id: project_id.to_string(),
            },
            name_input,
            desc_input,
            selected_icon,
            color,
            cx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn init(
        workspace: Entity<Workspace>,
        window_id: WindowId,
        mode: ProjectFormMode,
        name_input: Entity<InputState>,
        desc_input: Entity<TextareaState>,
        selected_icon: AppIcon,
        selected_color: FolderColor,
        cx: &mut Context<Self>,
    ) -> Self {
        let dialog_entity = cx.entity().downgrade();
        name_input.update(cx, |inp, _cx| {
            inp.set_key_interceptor(move |event, _current_val, cx| {
                if event.keystroke.key.as_str() == "enter"
                    && let Some(dialog) = dialog_entity.upgrade()
                {
                    dialog.update(cx, |this, cx| {
                        this.pending_focus = Some(PendingFocus::IconGroup);
                        cx.notify();
                    });
                    return KeyInterceptResult::Handled;
                }
                KeyInterceptResult::Unhandled
            });
        });

        // Re-render immediately on input change to update validation & button state
        let name_sub = Some(cx.subscribe(&name_input, |_, _, _: &InputEvent, cx| {
            cx.notify();
        }));

        Self {
            workspace,
            window_id,
            mode,
            focus_handle: cx.focus_handle(),
            icon_group_focus: cx.focus_handle(),
            color_group_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            name_input,
            desc_input,
            _name_sub: name_sub,
            initial_focus_done: false,
            pending_focus: None,
            selected_icon,
            selected_color,
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(AddProjectDialogEvent::Close);
    }

    pub fn cycle_selected_icon(&mut self, forward: bool) {
        let current_idx = PROJECT_ICONS
            .iter()
            .position(|&i| i == self.selected_icon)
            .unwrap_or(0);
        let next_idx = if forward {
            (current_idx + 1) % PROJECT_ICONS.len()
        } else {
            (current_idx + PROJECT_ICONS.len() - 1) % PROJECT_ICONS.len()
        };
        self.selected_icon = PROJECT_ICONS[next_idx];
    }

    pub fn cycle_selected_color(&mut self, forward: bool) {
        let current_idx = PROJECT_COLORS
            .iter()
            .position(|&c| c == self.selected_color)
            .unwrap_or(0);
        let next_idx = if forward {
            (current_idx + 1) % PROJECT_COLORS.len()
        } else {
            (current_idx + PROJECT_COLORS.len() - 1) % PROJECT_COLORS.len()
        };
        self.selected_color = PROJECT_COLORS[next_idx];
    }

    pub fn is_name_duplicate(&self, name: &str, cx: &App) -> bool {
        let clean = name.trim();
        if clean.is_empty() {
            return false;
        }
        let lower = clean.to_lowercase();
        let current_id = match &self.mode {
            ProjectFormMode::Create => None,
            ProjectFormMode::Edit { project_id } => Some(project_id.as_str()),
        };
        self.workspace.read(cx).projects().iter().any(|p| {
            if let Some(cid) = current_id
                && p.id == cid
            {
                return false;
            }
            p.name.trim().to_lowercase() == lower
        })
    }

    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).text().trim().to_string();
        if name.is_empty() || self.is_name_duplicate(&name, cx) {
            return;
        }

        let description = self.desc_input.read(cx).text().to_string();
        let icon = self.selected_icon.name().to_string();
        let color = self.selected_color;
        let window_id = self.window_id;

        match &self.mode {
            ProjectFormMode::Create => {
                let pid = self.workspace.update(cx, |ws, cx| {
                    let project_id = ws.add_project(name, String::new(), true, window_id, cx);
                    ws.set_folder_color(&project_id, color, cx);
                    ws.set_project_icon(&project_id, icon, cx);
                    ws.set_project_description(&project_id, description, cx);
                    project_id
                });
                cx.emit(AddProjectDialogEvent::Saved {
                    project_id: pid,
                    is_new: true,
                });
            }
            ProjectFormMode::Edit { project_id } => {
                let pid = project_id.clone();
                self.workspace.update(cx, |ws, cx| {
                    ws.rename_project(&pid, name, cx);
                    ws.set_folder_color(&pid, color, cx);
                    ws.set_project_icon(&pid, icon, cx);
                    ws.set_project_description(&pid, description, cx);
                });
                cx.emit(AddProjectDialogEvent::Saved {
                    project_id: pid,
                    is_new: false,
                });
            }
        }
    }

    /// Icon picker row: single-select buttons with direction-key navigation and focus ring.
    fn render_icon_picker(
        &self,
        is_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);

        h_flex()
            .id("project-icon-picker-group")
            .track_focus(&self.icon_group_focus)
            .gap(SPACE_MD)
            .flex_wrap()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                match event.keystroke.key.as_str() {
                    "left" | "up" => {
                        this.cycle_selected_icon(false);
                        cx.notify();
                        cx.stop_propagation();
                    }
                    "right" | "down" => {
                        this.cycle_selected_icon(true);
                        cx.notify();
                        cx.stop_propagation();
                    }
                    "enter" => {
                        this.pending_focus = Some(PendingFocus::ColorGroup);
                        cx.notify();
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .children(PROJECT_ICONS.iter().map(|&icon| {
                let is_active = self.selected_icon == icon;
                let icon_name = icon;
                let icon_color = if is_active {
                    t.border_active
                } else {
                    t.text_muted
                };

                let mut el = div()
                    .id(ElementId::Name(
                        format!("project-icon-{}", icon.name()).into(),
                    ))
                    .w(px(36.0))
                    .h(px(36.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(RADIUS_STD)
                    .border_1()
                    .cursor_pointer();

                if is_active {
                    el = el
                        .border_color(p.border_active)
                        .bg(p.surface_card)
                        .when(is_focused, |s| {
                            s.shadow(vec![BoxShadow {
                                color: with_alpha(t.border_active, 0.35),
                                offset: point(px(0.0), px(0.0)),
                                blur_radius: px(3.0),
                                spread_radius: px(1.5),
                                inset: false,
                            }])
                        });
                } else {
                    el = el
                        .border_color(p.border_subtle)
                        .hover(|s| {
                            s.bg(p.surface_hover)
                                .border_color(p.surface_accent.opacity(0.6))
                        });
                }

                el.child(
                    icon.size(px(18.0))
                        .text_color(rgb(icon_color)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _window, cx| {
                        this.selected_icon = icon_name;
                        this.pending_focus = Some(PendingFocus::IconGroup);
                        cx.notify();
                    }),
                )
            }))
    }

    /// Color picker row: circular swatches with direction-key navigation and focus ring.
    fn render_color_picker(
        &self,
        is_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);

        h_flex()
            .id("project-color-picker-group")
            .track_focus(&self.color_group_focus)
            .gap(SPACE_MD)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                match event.keystroke.key.as_str() {
                    "left" | "up" => {
                        this.cycle_selected_color(false);
                        cx.notify();
                        cx.stop_propagation();
                    }
                    "right" | "down" => {
                        this.cycle_selected_color(true);
                        cx.notify();
                        cx.stop_propagation();
                    }
                    "enter" => {
                        this.pending_focus = Some(PendingFocus::Description);
                        cx.notify();
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .children(PROJECT_COLORS.iter().map(|&color| {
                let is_active = self.selected_color == color;
                let hex = t.get_folder_color(color);

                let mut el = div()
                    .id(ElementId::Name(
                        format!("project-color-{:?}", color).into(),
                    ))
                    .w(px(28.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .border_2();

                if is_active {
                    el = el
                        .border_color(p.border_active)
                        .when(is_focused, |s| {
                            s.shadow(vec![BoxShadow {
                                color: with_alpha(t.border_active, 0.35),
                                offset: point(px(0.0), px(0.0)),
                                blur_radius: px(3.0),
                                spread_radius: px(1.5),
                                inset: false,
                            }])
                        });
                } else {
                    el = el
                        .border_color(rgba(0x00000000))
                        .hover(|s| s.border_color(p.border_subtle));
                }

                el.child(
                    div()
                        .w(px(18.0))
                        .h(px(18.0))
                        .rounded_full()
                        .bg(rgb(hex)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _window, cx| {
                        this.selected_color = color;
                        this.pending_focus = Some(PendingFocus::ColorGroup);
                        cx.notify();
                    }),
                )
            }))
    }
}

impl Render for AddProjectDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let focus_handle = self.focus_handle.clone();

        if !self.initial_focus_done {
            self.initial_focus_done = true;
            self.name_input.update(cx, |input, cx| {
                input.focus(window, cx);
            });
        } else if let Some(pending) = self.pending_focus.take() {
            match pending {
                PendingFocus::Name => {
                    self.name_input.update(cx, |inp, cx| inp.focus(window, cx));
                }
                PendingFocus::IconGroup => {
                    window.focus(&self.icon_group_focus, cx);
                }
                PendingFocus::ColorGroup => {
                    window.focus(&self.color_group_focus, cx);
                }
                PendingFocus::Description => {
                    self.desc_input.update(cx, |inp, cx| inp.focus(window, cx));
                }
                PendingFocus::Cancel => {
                    window.focus(&self.cancel_focus, cx);
                }
                PendingFocus::Confirm => {
                    window.focus(&self.confirm_focus, cx);
                }
            }
        }

        let is_icon_focused = self.icon_group_focus.is_focused(window);
        let is_color_focused = self.color_group_focus.is_focused(window);

        let current_name = self.name_input.read(cx).text().to_string();
        let clean_name = current_name.trim();
        let is_empty_name = clean_name.is_empty();
        let is_duplicate = !is_empty_name && self.is_name_duplicate(clean_name, cx);
        let confirm_disabled = is_empty_name || is_duplicate;

        let is_edit = matches!(self.mode, ProjectFormMode::Edit { .. });
        let title_text = if is_edit {
            i18n!(cx, "project.manage.edit_project")
        } else {
            i18n!(cx, "project.editor.title")
        };
        let confirm_label = if is_edit {
            i18n!(cx, "common.action.save")
        } else {
            i18n!(cx, "project.editor.add")
        };

        let focus_group = FocusGroup::new();
        focus_group.add(self.name_input.read(cx).focus_handle(cx));
        focus_group.add(self.icon_group_focus.clone());
        focus_group.add(self.color_group_focus.clone());
        focus_group.add(self.desc_input.read(cx).focus_handle(cx));
        focus_group.add(self.cancel_focus.clone());
        focus_group.add(self.confirm_focus.clone());

        let close_tip = i18n!(cx, "common.action.close");

        modal_content("add-project-modal", cx)
            .w(px(460.0))
            .track_focus(&focus_handle)
            .key_context("AddProjectDialog")
            .tab_cycle(&focus_group)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.close(cx);
            }))
            .focus_scope_on_click(&focus_handle)
                    .child(
                        div()
                            .id("add-project-header")
                            .flex()
                            .items_center()
                            .justify_between()
                            .px(SPACE_XL)
                            .py(SPACE_LG)
                            .border_b_1()
                            .border_color(p.border_subtle)
                            .child(
                                h_flex()
                                    .gap(SPACE_MD)
                                    .items_center()
                                    .child(
                                        if is_edit {
                                            AppIcon::Edit
                                                .size(px(18.0))
                                                .text_color(p.border_active)
                                        } else {
                                            AppIcon::Plus
                                                .size(px(18.0))
                                                .text_color(p.border_active)
                                        },
                                    )
                                    .child(
                                        div()
                                            .text_size(ui_text_md(cx))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.text_primary)
                                            .child(title_text),
                                    ),
                            )
                            .child(
                                div()
                                    .id("add-project-close")
                                    .cursor_pointer()
                                    .w(px(28.0))
                                    .h(px(28.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(RADIUS_STD)
                                    .hover(|s| s.bg(p.surface_hover))
                                    .text_color(p.text_secondary)
                                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(close_tip.clone())).into())
                                    .child(AppIcon::Close.size(px(16.0)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| this.close(cx)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .p(SPACE_XL)
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .gap(SPACE_LG)
                            // 1. Name (text input, required, duplicate check)
                            .child(
                                v_flex()
                                    .gap(SPACE_XS)
                                    .child(
                                        labeled_input(i18n!(cx, "project.editor.name"), &t, cx).child(
                                            div().flex_1().child(
                                                velowork_ui::Input::new(&self.name_input)
                                                    .cleanable(true)
                                                    .when(is_duplicate, |inp| {
                                                        inp.border_color(rgb(t.error))
                                                    }),
                                            ),
                                        ),
                                    )
                                    .when(is_duplicate, |c| {
                                        c.child(
                                            div()
                                                .px(SPACE_SM)
                                                .text_size(ui_text_md(cx))
                                                .text_color(rgb(t.error))
                                                .child(i18n!(cx, "project.duplicate_name_error")),
                                        )
                                    }),
                            )
                            // 2. Icon (direction key picker)
                            .child(
                                labeled_input(i18n!(cx, "project.editor.icon"), &t, cx)
                                    .child(self.render_icon_picker(is_icon_focused, cx)),
                            )
                            // 3. Color (direction key swatches)
                            .child(
                                labeled_input(i18n!(cx, "project.editor.color"), &t, cx)
                                    .child(self.render_color_picker(is_color_focused, cx)),
                            )
                            // 4. Description (multi-line textarea)
                            .child(
                                labeled_input(i18n!(cx, "project.editor.description"), &t, cx).child(
                                    div().flex_1().child(
                                        velowork_ui::Input::new(&self.desc_input)
                                            .fill_height()
                                            .h(px(80.0)),
                                    ),
                                ),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(48.0))
                            .flex_shrink_0()
                            .items_center()
                            .justify_end()
                            .px(SPACE_LG)
                            .border_t_1()
                            .border_color(p.border_subtle)
                            .child(dialog_actions_extended(
                                i18n!(cx, "common.action.cancel"),
                                cx.listener(|this, _, _window, cx| {
                                    this.close(cx);
                                }),
                                &self.cancel_focus,
                                confirm_label,
                                cx.listener(|this, _, window, cx| {
                                    this.save(window, cx);
                                }),
                                &self.confirm_focus,
                                true,
                                false,
                                false,
                                confirm_disabled,
                                &t,
                            )),
                    )
    }
}

impl_focusable!(AddProjectDialog);

#[cfg(test)]
mod tests {
    use super::*;
    use velowork_workspace::init_settings;
    use velowork_workspace::state::{WindowState, WorkspaceData, ProjectData};
    use std::collections::HashMap;

    fn empty_workspace() -> WorkspaceData {
        WorkspaceData {
            version: 1,
            projects: Vec::new(),
            project_order: Vec::new(),
            folders: Vec::new(),
            service_panel_heights: HashMap::new(),
            main_window: WindowState::default(),
            extra_windows: Vec::new(),
        }
    }

    #[gpui::test]
    async fn test_add_project_dialog_create_mode_defaults(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            init_settings(cx);
            velowork_i18n::init_locale(velowork_i18n::Locale::Zh, cx);
            let window_id = WindowId::Main;
            let workspace = cx.new(|_| Workspace::new(empty_workspace()));
            let dialog = cx.new(|cx| AddProjectDialog::new_create(workspace, window_id, cx));
            let d = dialog.read(cx);
            assert_eq!(d.mode, ProjectFormMode::Create);
            assert_eq!(d.selected_icon, AppIcon::Monitor);
            assert_eq!(d.selected_color, FolderColor::Indigo);
            assert_eq!(d.name_input.read(cx).text(), "");
        });
    }

    #[gpui::test]
    async fn test_add_project_dialog_edit_mode_prefill(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            init_settings(cx);
            velowork_i18n::init_locale(velowork_i18n::Locale::Zh, cx);
            let window_id = WindowId::Main;
            let mut ws_data = empty_workspace();
            ws_data.projects.push(ProjectData {
                id: "proj-1".to_string(),
                name: "My Project".to_string(),
                icon: "database".to_string(),
                folder_color: FolderColor::Green,
                description: "Test description".to_string(),
                ..Default::default()
            });
            ws_data.project_order.push("proj-1".to_string());
            let workspace = cx.new(|_| Workspace::new(ws_data));

            let dialog = cx.new(|cx| AddProjectDialog::new_edit(workspace, window_id, "proj-1", cx));
            let d = dialog.read(cx);
            assert_eq!(d.mode, ProjectFormMode::Edit { project_id: "proj-1".to_string() });
            assert_eq!(d.selected_icon, AppIcon::Database);
            assert_eq!(d.selected_color, FolderColor::Green);
            assert_eq!(d.name_input.read(cx).text(), "My Project");
            assert_eq!(d.desc_input.read(cx).text(), "Test description");
        });
    }

    #[gpui::test]
    async fn test_add_project_dialog_duplicate_name_detection(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            init_settings(cx);
            velowork_i18n::init_locale(velowork_i18n::Locale::Zh, cx);
            let window_id = WindowId::Main;
            let mut ws_data = empty_workspace();
            ws_data.projects.push(ProjectData {
                id: "proj-1".to_string(),
                name: "Existing Project".to_string(),
                ..Default::default()
            });
            ws_data.project_order.push("proj-1".to_string());
            let workspace = cx.new(|_| Workspace::new(ws_data));

            // Create mode:
            let create_dlg = cx.new(|cx| AddProjectDialog::new_create(workspace.clone(), window_id, cx));
            assert!(create_dlg.read(cx).is_name_duplicate("Existing Project", cx));
            assert!(create_dlg.read(cx).is_name_duplicate("  existing project  ", cx));
            assert!(!create_dlg.read(cx).is_name_duplicate("Another Project", cx));
            assert!(!create_dlg.read(cx).is_name_duplicate("", cx));

            // Edit mode for proj-1: keeping own name is NOT duplicate:
            let edit_dlg = cx.new(|cx| AddProjectDialog::new_edit(workspace, window_id, "proj-1", cx));
            assert!(!edit_dlg.read(cx).is_name_duplicate("Existing Project", cx));
            assert!(!edit_dlg.read(cx).is_name_duplicate("Existing Project ", cx));
        });
    }

    #[gpui::test]
    async fn test_add_project_dialog_keyboard_cycling(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            init_settings(cx);
            velowork_i18n::init_locale(velowork_i18n::Locale::Zh, cx);
            let window_id = WindowId::Main;
            let workspace = cx.new(|_| Workspace::new(empty_workspace()));
            let dialog = cx.new(|cx| AddProjectDialog::new_create(workspace, window_id, cx));

            // Test icon cycle forward and backward
            dialog.update(cx, |d, _| {
                assert_eq!(d.selected_icon, AppIcon::Monitor);
                d.cycle_selected_icon(true);
                assert_eq!(d.selected_icon, PROJECT_ICONS[1]);
                d.cycle_selected_icon(false);
                assert_eq!(d.selected_icon, PROJECT_ICONS[0]);
                d.cycle_selected_icon(false);
                assert_eq!(d.selected_icon, PROJECT_ICONS[PROJECT_ICONS.len() - 1]);
            });

            // Test color cycle forward and backward
            dialog.update(cx, |d, _| {
                assert_eq!(d.selected_color, FolderColor::Indigo);
                d.cycle_selected_color(true);
                assert_eq!(d.selected_color, PROJECT_COLORS[1]);
                d.cycle_selected_color(false);
                assert_eq!(d.selected_color, PROJECT_COLORS[0]);
                d.cycle_selected_color(false);
                assert_eq!(d.selected_color, PROJECT_COLORS[PROJECT_COLORS.len() - 1]);
            });
        });
    }
}
