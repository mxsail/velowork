use crate::keybindings::{Cancel, RenameActiveNode};
use crate::theme::{surface_bg_t, theme, with_alpha};
use crate::ui::tokens::{
    ui_text_md, ui_text_sm, ICON_SM, ICON_STD, RADIUS_STD,
    SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, SPACE_XL,
};
use crate::views::components::{
    dropdown_anchored_below, dropdown_overlay,
    modal_content,
};
use crate::views::overlays::dialogs::project_add_dialog::{
    AddProjectDialog, AddProjectDialogEvent,
};
use crate::views::overlays::dialogs::project_export_dialog::{
    ProjectExportDialog, ProjectExportDialogEvent,
};
use crate::views::overlays::dialogs::project_import_dialog::{
    ProjectImportDialog, ProjectImportDialogEvent,
};
use crate::views::overlays::overlay_manager::OverlayManager;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, BoxShadow, ClickEvent, Context, ElementId, Entity, EventEmitter,
    FocusHandle, FontWeight, Hsla, IntoElement, KeyDownEvent, MouseButton, Pixels, Render,
    Subscription, UniformListScrollHandle, Window, canvas, deferred, div, point, px, rgb, size,
};
use std::collections::HashMap;
use std::sync::Arc;
use velowork_core::theme::FolderColor;
use velowork_i18n::i18n;
use velowork_ui::button::Button;
use velowork_ui::design::appearance::ControlSize;
use velowork_ui::h_flex;
use velowork_ui::icon::AppIcon;
use velowork_ui::input::{InputEvent, InputState, KeyInterceptResult};
use velowork_ui::overlay_registry::{ClosePolicy, OverlayInfo};
use velowork_ui::menu::{menu_item, menu_item_disabled, menu_item_with_color};
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::v_flex;
use velowork_ui::{scroll_to_row, virtual_list};
use velowork_workspace::focus::FocusManager;
use velowork_workspace::state::{ProjectData, WindowId, Workspace};

type CloseFn = Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingFocus {
    SearchInput,
    ImportButton,
    NewButton,
    DeleteCancel,
    DeleteConfirm,
}

pub struct ManageProjectsDialog {
    workspace: Entity<Workspace>,
    window_id: WindowId,
    focus_manager: Entity<FocusManager>,
    focus_handle: FocusHandle,
    overlay_manager: Entity<OverlayManager>,
    rename_project_id: Option<String>,
    action_menu_project_id: Option<String>,
    more_button_bounds: HashMap<String, Bounds<Pixels>>,
    initial_focus_done: bool,
    delete_confirming_id: Option<String>,
    rename_input: Option<Entity<InputState>>,
    rename_sub: Option<Subscription>,
    scroll_handle: UniformListScrollHandle,
    search_input: Option<Entity<InputState>>,
    search_sub: Option<Subscription>,
    query: String,
    filtered_indices: Vec<usize>,
    selected_filtered_idx: usize,
    import_button_focus: FocusHandle,
    new_button_focus: FocusHandle,
    delete_cancel_focus: FocusHandle,
    delete_confirm_focus: FocusHandle,
    pending_focus: Option<PendingFocus>,
}

pub enum ManageProjectsDialogEvent {
    Close,
}

impl EventEmitter<ManageProjectsDialogEvent> for ManageProjectsDialog {}

impl ManageProjectsDialog {
    pub fn new(
        workspace: Entity<Workspace>,
        window_id: WindowId,
        focus_manager: Entity<FocusManager>,
        overlay_manager: Entity<OverlayManager>,
        cx: &mut Context<Self>,
    ) -> Self {
        let project_count = workspace.read(cx).projects().len();
        let filtered_indices: Vec<usize> = (0..project_count).collect();
        let selected_filtered_idx = focus_manager
            .read(cx)
            .focused_project_id()
            .and_then(|fid| {
                workspace
                    .read(cx)
                    .projects()
                    .iter()
                    .position(|p| p.id == *fid)
            })
            .unwrap_or(0);

        let mut this = Self {
            workspace,
            window_id,
            focus_manager,
            focus_handle: cx.focus_handle(),
            overlay_manager,
            rename_project_id: None,
            action_menu_project_id: None,
            more_button_bounds: HashMap::new(),
            initial_focus_done: false,
            delete_confirming_id: None,
            rename_input: None,
            rename_sub: None,
            scroll_handle: UniformListScrollHandle::new(),
            search_input: None,
            search_sub: None,
            query: String::new(),
            filtered_indices,
            selected_filtered_idx,
            import_button_focus: cx.focus_handle(),
            new_button_focus: cx.focus_handle(),
            delete_cancel_focus: cx.focus_handle(),
            delete_confirm_focus: cx.focus_handle(),
            pending_focus: Some(PendingFocus::SearchInput),
        };
        this.ensure_search_input(cx);
        this
    }

    fn trigger_delete_on_selected(&mut self, cx: &mut Context<Self>) {
        if self.delete_confirming_id.is_some() {
            return;
        }
        if self.workspace.read(cx).projects().len() <= 1 {
            return;
        }
        if let Some(pid) = self.current_selected_project_id(cx) {
            self.delete_confirming_id = Some(pid);
            self.pending_focus = Some(PendingFocus::DeleteCancel);
            cx.notify();
        }
    }

    fn ensure_search_input(&mut self, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(input) = self.search_input.clone() {
            return input;
        }
        let placeholder = i18n!(cx, "project.manage.search_placeholder");
        let input = cx.new(|cx| {
            InputState::new(cx).placeholder(placeholder)
        });

        let dialog_entity = cx.entity().downgrade();
        input.update(cx, |inp, _cx| {
            inp.set_key_interceptor(move |event, current_val, cx| {
                if event.keystroke.key.as_str() != "delete" || !current_val.is_empty() {
                    return KeyInterceptResult::Unhandled;
                }
                if let Some(dialog) = dialog_entity.upgrade() {
                    dialog.update(cx, |this, cx| {
                        this.trigger_delete_on_selected(cx);
                    });
                    return KeyInterceptResult::Handled;
                }
                KeyInterceptResult::Unhandled
            });
        });

        let input_clone = input.clone();
        self.search_sub = Some(cx.subscribe(&input_clone, |this: &mut Self, _, _: &InputEvent, cx| {
            if let Some(inp) = this.search_input.as_ref() {
                let new_query = inp.read(cx).text().to_string();
                if this.query != new_query {
                    this.query = new_query;
                    this.selected_filtered_idx = 0;
                    this.recompute_filtered(cx);
                    scroll_to_row(&this.scroll_handle, 0);
                    cx.notify();
                }
            }
        }));
        self.search_input = Some(input.clone());
        input
    }

    fn dismiss(&self, cx: &mut Context<Self>) {
        cx.emit(ManageProjectsDialogEvent::Close);
    }

    fn confirm_and_switch(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let current = self.focus_manager.read(cx).active_project_id().cloned();
        if current.as_deref() != Some(project_id) {
            let pid = project_id.to_string();
            self.focus_manager.update(cx, |fm, cx_fm| {
                self.workspace.update(cx_fm, |ws, cx_ws| {
                    ws.set_focused_project(fm, Some(pid), self.window_id, cx_ws);
                });
                cx_fm.notify();
            });
        }
        cx.emit(ManageProjectsDialogEvent::Close);
    }

    fn recompute_filtered(&mut self, cx: &App) {
        let projects = self.workspace.read(cx).projects();
        self.filtered_indices = ranked_project_filter(projects, &self.query);
        if self.filtered_indices.is_empty() {
            self.selected_filtered_idx = 0;
        } else if self.selected_filtered_idx >= self.filtered_indices.len() {
            self.selected_filtered_idx = self.filtered_indices.len().saturating_sub(1);
        }
    }

    fn current_selected_project_id(&self, cx: &App) -> Option<String> {
        let raw_idx = *self.filtered_indices.get(self.selected_filtered_idx)?;
        self.workspace.read(cx).projects().get(raw_idx).map(|p| p.id.clone())
    }

    fn open_create_dialog(&mut self, cx: &mut Context<Self>) {
        self.action_menu_project_id = None;
        let ws = self.workspace.clone();
        let win_id = self.window_id;
        let dlg = cx.new(|cx| AddProjectDialog::new_create(ws, win_id, cx));
        let om = self.overlay_manager.clone();
        cx.subscribe(&dlg, move |this, _, event: &AddProjectDialogEvent, cx| {
            match event {
                AddProjectDialogEvent::Close => {
                    om.update(cx, |om, cx| om.close_modal(cx));
                    this.pending_focus = Some(PendingFocus::SearchInput);
                    cx.notify();
                }
                AddProjectDialogEvent::Saved { project_id, .. } => {
                    om.update(cx, |om, cx| om.close_modal(cx));
                    let pid = project_id.clone();
                    this.recompute_filtered(cx);
                    if let Some(pos) = this.filtered_indices.iter().position(|&raw_idx| {
                        this.workspace.read(cx).projects().get(raw_idx).is_some_and(|p| p.id == pid)
                    }) {
                        this.selected_filtered_idx = pos;
                        scroll_to_row(&this.scroll_handle, pos);
                    }
                    this.pending_focus = Some(PendingFocus::SearchInput);
                    cx.notify();
                }
            }
        })
        .detach();
        let origin_focus = self.new_button_focus.clone();
        self.overlay_manager.update(cx, |om, cx| {
            om.open_modal_with_origin(dlg, Some(origin_focus), None, cx);
        });
    }

    fn open_edit_dialog(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.action_menu_project_id = None;
        let ws = self.workspace.clone();
        let win_id = self.window_id;
        let pid = project_id.to_string();
        let dlg = cx.new(|cx| AddProjectDialog::new_edit(ws, win_id, &pid, cx));
        let om = self.overlay_manager.clone();
        cx.subscribe(&dlg, move |this, _, event: &AddProjectDialogEvent, cx| {
            match event {
                AddProjectDialogEvent::Close => {
                    om.update(cx, |om, cx| om.close_modal(cx));
                    this.pending_focus = Some(PendingFocus::SearchInput);
                    cx.notify();
                }
                AddProjectDialogEvent::Saved { project_id, .. } => {
                    om.update(cx, |om, cx| om.close_modal(cx));
                    let pid = project_id.clone();
                    this.recompute_filtered(cx);
                    if let Some(pos) = this.filtered_indices.iter().position(|&raw_idx| {
                        this.workspace.read(cx).projects().get(raw_idx).is_some_and(|p| p.id == pid)
                    }) {
                        this.selected_filtered_idx = pos;
                        scroll_to_row(&this.scroll_handle, pos);
                    }
                    this.pending_focus = Some(PendingFocus::SearchInput);
                    cx.notify();
                }
            }
        })
        .detach();
        self.overlay_manager.update(cx, |om, cx| {
            om.open_modal(dlg, cx);
        });
    }

    fn duplicate_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let (name, icon, color, description) = self
            .workspace
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
        if name.is_empty() {
            return;
        }
        let suffix = i18n!(cx, "project.copied_suffix");
        let new_name = format!("{} ({})", name, suffix);
        let window_id = self.window_id;
        let new_icon = if icon.is_empty() {
            "monitor".to_string()
        } else {
            icon
        };
        self.workspace.update(cx, |ws, cx| {
            let id = ws.add_project(new_name, String::new(), false, window_id, cx);
            ws.set_folder_color(&id, color, cx);
            ws.set_project_icon(&id, new_icon, cx);
            ws.set_project_description(&id, description, cx);
            ws.toggle_hidden(window_id, &id, cx);
        });
        self.action_menu_project_id = None;
        self.recompute_filtered(cx);
        self.pending_focus = Some(PendingFocus::SearchInput);
        cx.notify();
    }

    fn delete_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if self.workspace.read(cx).data.projects.len() <= 1 {
            self.delete_confirming_id = None;
            self.rename_project_id = None;
            self.action_menu_project_id = None;
            self.pending_focus = Some(PendingFocus::SearchInput);
            cx.notify();
            return;
        }
        let pid = project_id.to_string();
        let fm = self.focus_manager.clone();
        let ws = self.workspace.clone();
        fm.update(cx, |fm, cx| {
            ws.update(cx, |ws, cx| {
                ws.delete_project(fm, &pid, cx);
            });
        });
        self.delete_confirming_id = None;
        self.rename_project_id = None;
        self.action_menu_project_id = None;
        self.recompute_filtered(cx);
        self.pending_focus = Some(PendingFocus::SearchInput);
        cx.notify();
    }

    fn start_rename(&mut self, project_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .workspace
            .read(cx)
            .project(project_id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let placeholder = i18n!(cx, "dock.project_name_placeholder");
        let input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(placeholder)
                .default_value(&name)
        });
        input.update(cx, |inp, cx| inp.focus(window, cx));
        self.rename_input = Some(input);
        self.rename_project_id = Some(project_id.to_string());
        self.rename_sub = None;
        if let Some(pos) = self.filtered_indices.iter().position(|&raw_idx| {
            self.workspace.read(cx).projects().get(raw_idx).is_some_and(|p| p.id == project_id)
        }) {
            self.selected_filtered_idx = pos;
        }
        self.action_menu_project_id = None;
        cx.notify();
    }

    fn is_project_name_duplicate(&self, except_id: &str, name: &str, cx: &App) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let lower = name.to_lowercase();
        self.workspace
            .read(cx)
            .projects()
            .iter()
            .any(|p| p.id != except_id && p.name.trim().to_lowercase() == lower)
    }

    fn finish_rename(&mut self, cx: &mut Context<Self>) {
        let commit = if let (Some(input), Some(pid)) =
            (self.rename_input.as_ref(), self.rename_project_id.clone())
        {
            let val = input.read(cx).text().to_string().trim().to_string();
            let dup = self.is_project_name_duplicate(&pid, &val, cx);
            !val.is_empty() && !dup
        } else {
            false
        };
        if commit {
            let pid = self.rename_project_id.clone().unwrap();
            let val = self
                .rename_input
                .as_ref()
                .unwrap()
                .read(cx)
                .text()
                .to_string()
                .trim()
                .to_string();
            self.workspace.update(cx, |ws, cx| {
                ws.rename_project(&pid, val, cx);
            });
        }
        self.rename_input = None;
        self.rename_project_id = None;
        self.rename_sub = None;
        self.recompute_filtered(cx);
        self.pending_focus = Some(PendingFocus::SearchInput);
        cx.notify();
    }

    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.rename_input = None;
        self.rename_project_id = None;
        self.rename_sub = None;
        self.pending_focus = Some(PendingFocus::SearchInput);
        cx.notify();
    }

    fn open_export_dialog(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.action_menu_project_id = None;
        let ws = self.workspace.clone();
        let pid = project_id.to_string();
        let dlg = cx.new(|cx| ProjectExportDialog::new(ws, pid, cx));
        let om = self.overlay_manager.clone();
        cx.subscribe(&dlg, move |this, _, _: &ProjectExportDialogEvent, cx| {
            om.update(cx, |om, cx| om.close_modal(cx));
            this.pending_focus = Some(PendingFocus::SearchInput);
            cx.notify();
        })
        .detach();
        self.overlay_manager.update(cx, |om, cx| {
            om.open_modal(dlg, cx);
        });
    }

    fn open_import_dialog(&mut self, cx: &mut Context<Self>) {
        self.action_menu_project_id = None;
        let ws = self.workspace.clone();
        let win_id = self.window_id;
        let dlg = cx.new(|cx| ProjectImportDialog::new(ws, win_id, cx));
        let om = self.overlay_manager.clone();
        cx.subscribe(&dlg, move |this, _, event: &ProjectImportDialogEvent, cx| {
            match event {
                ProjectImportDialogEvent::Close => {
                    om.update(cx, |om, cx| om.close_modal(cx));
                    this.pending_focus = Some(PendingFocus::SearchInput);
                    cx.notify();
                }
                ProjectImportDialogEvent::ProjectImported(pid) => {
                    om.update(cx, |om, cx| om.close_modal(cx));
                    this.query.clear();
                    if let Some(inp) = this.search_input.as_ref() {
                        inp.update(cx, |i, cx| i.set_value("", cx));
                    }
                    this.recompute_filtered(cx);
                    if let Some(pos) = this.filtered_indices.iter().position(|&raw_idx| {
                        this.workspace.read(cx).projects().get(raw_idx).is_some_and(|p| p.id == *pid)
                    }) {
                        this.selected_filtered_idx = pos;
                        scroll_to_row(&this.scroll_handle, pos);
                    }
                    this.pending_focus = Some(PendingFocus::SearchInput);
                    cx.notify();
                }
            }
        })
        .detach();
        let origin_focus = self.import_button_focus.clone();
        self.overlay_manager.update(cx, |om, cx| {
            om.open_modal_with_origin(dlg, Some(origin_focus), None, cx);
        });
    }


    fn render_list_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        self.ensure_search_input(cx);

        let filtered_len = self.filtered_indices.len();
        let total_count = self.workspace.read(cx).projects().len();
        let row_h = window.line_height() + SPACE_XL;

        let list_el: AnyElement = if total_count == 0 {
            div()
                .w_full()
                .py(SPACE_LG)
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_muted))
                .child(i18n!(cx, "project.no_projects"))
                .into_any_element()
        } else if filtered_len == 0 {
            div()
                .w_full()
                .py(SPACE_LG)
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_muted))
                .child(i18n!(cx, "common.state.no_results"))
                .into_any_element()
        } else {
            virtual_list(
                "mp-project-list",
                cx.entity(),
                &self.scroll_handle,
                filtered_len,
                move |this, range, window, cx| {
                    let projects = this.workspace.read(cx).projects().to_vec();
                    range
                        .filter_map(|filtered_idx| {
                            let raw_idx = *this.filtered_indices.get(filtered_idx)?;
                            let p = projects.get(raw_idx)?;
                            let (id, name, desc, icon, color) = (
                                p.id.clone(),
                                p.name.clone(),
                                p.description.clone(),
                                p.icon.clone(),
                                p.folder_color,
                            );
                            Some(this.render_project_row(
                                filtered_idx, id, name, desc, icon, color, row_h, window, cx,
                            ))
                        })
                        .collect()
                },
            )
            .into_any_element()
        };

        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);

        let count_text = if self.query.is_empty() {
            format!("{}", total_count)
        } else {
            format!("{} / {}", filtered_len, total_count)
        };

        v_flex()
            .id("manage-projects-list-view")
            .w_full()
            .h_full()
            .child(
                // Header Toolbar
                h_flex()
                    .w_full()
                    .px(px(20.0))
                    .py(SPACE_LG)
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .items_center()
                    .child(
                        h_flex()
                            .gap(SPACE_MD)
                            .items_center()
                            .child(
                                AppIcon::Folder
                                    .size(px(20.0))
                                    .text_color(p.surface_accent),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text_primary)
                                    .child(i18n!(cx, "project.manage.title")),
                            )
                            .child(
                                div()
                                    .px(SPACE_SM)
                                    .py(px(2.0))
                                    .rounded_full()
                                    .bg(p.surface_card)
                                    .border_1()
                                    .border_color(p.border_subtle)
                                    .text_size(ui_text_sm(cx))
                                    .text_color(p.text_secondary)
                                    .child(count_text),
                            ),
                    ),
            )
            .child(
                // Search Input Toolbar
                div()
                    .w_full()
                    .px(px(20.0))
                    .pt(SPACE_MD)
                    .pb(SPACE_SM)
                    .when_some(self.search_input.as_ref(), |this, inp| {
                        this.child(velowork_ui::Input::new(inp).cleanable(true))
                    }),
            )
            .child(
                v_flex()
                    .id("mp-list-container")
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .px(px(20.0))
                    .py(SPACE_SM)
                    .child(list_el),
            )
            .child(
                // Bottom Fixed Actions Toolbar
                h_flex()
                    .w_full()
                    .px(px(20.0))
                    .py(SPACE_MD)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .justify_end()
                    .gap(SPACE_MD)
                    .items_center()
                    .child(
                        Button::new("mp-bottom-import-btn", &t)
                            .focus_handle(&self.import_button_focus)
                            .size(ControlSize::Default)
                            .icon_left(AppIcon::FolderInput)
                            .label(i18n!(cx, "project.import_project"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_import_dialog(cx);
                            })),
                    )
                    .child(
                        Button::new("mp-bottom-new-btn", &t)
                            .focus_handle(&self.new_button_focus)
                            .primary()
                            .size(ControlSize::Default)
                            .icon_left(AppIcon::Plus)
                            .label(i18n!(cx, "project.new_project"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_create_dialog(cx);
                            })),
                    ),
            )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_project_row(
        &mut self,
        idx: usize,
        project_id: String,
        project_name: String,
        project_desc: String,
        project_icon: String,
        project_color: FolderColor,
        row_h: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let om = self.overlay_manager.clone();
        let menu_entity = cx.entity().downgrade();

        let is_delete_confirm = self.delete_confirming_id.as_deref() == Some(&project_id);
        let is_rename =
            self.rename_input.is_some() && self.rename_project_id.as_deref() == Some(&project_id);
        let is_selected = idx == self.selected_filtered_idx;

        let (is_duplicate, _border_color) = if is_rename {
            let input_entity = self.rename_input.clone().unwrap();
            let focus_handle = input_entity.read(cx).focus_handle(cx);
            if self.rename_sub.is_none() {
                window.focus(&focus_handle, cx);
                self.rename_sub =
                    Some(cx.on_blur(&focus_handle, window, move |this, window, cx| {
                        this.finish_rename(cx);
                        if this.rename_input.is_none() {
                            window.focus(&this.focus_handle, cx);
                        }
                    }));
            }
            let current_val = input_entity.read(cx).text().to_string().trim().to_string();
            let dup = self.rename_project_id.as_ref().is_some_and(|pid| {
                self.is_project_name_duplicate(pid, &current_val, cx)
            });
            let bc = if dup { rgb(t.error) } else { rgb(t.border_active) };
            (dup, bc)
        } else {
            (false, rgb(t.border_active))
        };

        let name_cell = if is_rename {
            let input_entity = self.rename_input.clone().unwrap();
            div()
                .flex_1()
                .min_w_0()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _, _, cx| cx.stop_propagation()),
                )
                .child(
                    velowork_ui::Input::new(&input_entity)
                        .cleanable(false)
                        .when(is_duplicate, |inp| inp.border_color(rgb(t.error))),
                )
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter" {
                        if !is_duplicate {
                            this.finish_rename(cx);
                            window.focus(&this.focus_handle, cx);
                        }
                        cx.stop_propagation();
                    } else if event.keystroke.key == "escape" {
                        this.cancel_rename(cx);
                        window.focus(&this.focus_handle, cx);
                        cx.stop_propagation();
                    }
                }))
                .into_any_element()
        } else {
            let full_name = project_name.clone();
            h_flex()
                .flex_1()
                .min_w_0()
                .gap(SPACE_SM)
                .items_center()
                .child(
                    div()
                        .id(ElementId::Name(
                            format!("manage-project-name-{}", project_id).into(),
                        ))
                        .tooltip(move |_, cx| {
                            let __tip = full_name.clone();
                            cx.new(|_| Tooltip::new(__tip)).into()
                        })
                        .text_size(ui_text_md(cx))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(t.text_primary))
                        .truncate()
                        .child(project_name.clone()),
                )
                .when(!project_desc.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_muted))
                            .truncate()
                            .child(format!("- {}", project_desc)),
                    )
                })
                .into_any_element()
        };

        let action_cell = if is_delete_confirm {
            let is_confirm_focused = self.delete_confirm_focus.is_focused(window);
            let is_cancel_focused = self.delete_cancel_focus.is_focused(window);
            h_flex()
                .gap(SPACE_SM)
                .items_center()
                .child(
                    div()
                        .id("mp-del-confirm")
                        .track_focus(&self.delete_confirm_focus)
                        .cursor_pointer()
                        .h(px(24.0))
                        .px(ICON_SM)
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(RADIUS_STD)
                        .text_color(rgb(t.error))
                        .hover(|s| s.bg(rgb(t.error)).text_color(rgb(t.text_primary)))
                        .when(is_confirm_focused, |d| {
                            d.border_1()
                                .border_color(rgb(t.error))
                                .shadow(vec![BoxShadow {
                                    color: Hsla { a: 0.35, ..p.surface_danger },
                                    offset: point(px(0.0), px(0.0)),
                                    blur_radius: px(3.0),
                                    spread_radius: px(1.5),
                                    inset: false,
                                }])
                        })
                        .text_size(ui_text_md(cx))
                        .child(i18n!(cx, "common.action.confirm"))
                        .on_mouse_down(MouseButton::Left, cx.listener({
                            let pid = project_id.clone();
                            move |this, _, _, cx| this.delete_project(&pid, cx)
                        })),
                )
                .child(
                    div()
                        .id("mp-del-close")
                        .track_focus(&self.delete_cancel_focus)
                        .cursor_pointer()
                        .h(px(24.0))
                        .px(ICON_SM)
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(RADIUS_STD)
                        .border_1()
                        .border_color(p.border_subtle)
                        .text_color(p.text_muted)
                        .hover(|s| s.bg(p.surface_hover).text_color(p.text_primary))
                        .when(is_cancel_focused, |d| {
                            d.border_color(p.border_active)
                                .shadow(vec![BoxShadow {
                                    color: Hsla { a: 0.35, ..p.border_active },
                                    offset: point(px(0.0), px(0.0)),
                                    blur_radius: px(3.0),
                                    spread_radius: px(1.5),
                                    inset: false,
                                }])
                        })
                        .text_size(ui_text_md(cx))
                        .child(i18n!(cx, "common.action.cancel"))
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                            this.delete_confirming_id = None;
                            this.pending_focus = Some(PendingFocus::SearchInput);
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            div()
                .id(ElementId::Name(
                    format!("project-more-{}", project_id).into(),
                ))
                .relative()
                .cursor_pointer()
                .w(px(26.0))
                .h(px(26.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(RADIUS_STD)
                .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                .child(
                    AppIcon::MoreMenu
                        .size(ICON_STD)
                        .text_color(rgb(t.text_muted)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener({
                        let pid = project_id.clone();
                        let om = om.clone();
                        move |this, _event, _window, cx| {
                            let was_open =
                                this.action_menu_project_id.as_deref() == Some(pid.as_str());
                            if was_open {
                                om.update(cx, |om, ctx| {
                                    om.unregister_overlay(&"mp-action-menu".into(), ctx);
                                });
                                this.action_menu_project_id = None;
                            } else {
                                this.action_menu_project_id = Some(pid.clone());
                                let init_bounds = this
                                    .more_button_bounds
                                    .get(&pid)
                                    .cloned()
                                    .map(|b| Bounds::new(b.origin, size(px(160.0), px(200.0))))
                                    .unwrap_or(Bounds::new(
                                        point(px(0.0), px(0.0)),
                                        size(px(160.0), px(200.0)),
                                    ));
                                let dialog_entity = cx.entity().downgrade();
                                let close_fn: CloseFn =
                                    Arc::new(move |_, cx| {
                                        if let Some(entity) = dialog_entity.upgrade() {
                                            entity.update(cx, |this, _| {
                                                this.action_menu_project_id = None;
                                            });
                                        }
                                    });
                                om.update(cx, |om, ctx| {
                                    om.register_overlay(
                                        OverlayInfo {
                                            id: "mp-action-menu".into(),
                                            bounds: init_bounds,
                                            secondary_bounds: None,
                                            close_policy: ClosePolicy::ClickOutside,
                                            z_index: 1000,
                                        },
                                        close_fn,
                                        ctx,
                                    );
                                });
                            }
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }),
                )
                .child(
                    canvas(
                        {
                            let e = menu_entity.clone();
                            let pid = project_id.clone();
                            move |bounds, _, cx| {
                                if let Some(e) = e.upgrade() {
                                    e.update(cx, |this, _| {
                                        this.more_button_bounds.insert(pid.clone(), bounds);
                                    });
                                }
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .into_any_element()
        };

        let row_click_idx = idx;

        h_flex()
            .id(ElementId::Name(
                format!("manage-project-{}", project_id).into(),
            ))
            .relative()
            .w_full()
            .h(row_h)
            .px(SPACE_MD)
            .rounded(RADIUS_STD)
            .items_center()
            .justify_between()
            .border_1()
            .border_color(with_alpha(0x000000, 0.0))
            .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
            .when(is_selected && !is_rename, |d| {
                d.bg(surface_bg_t(t.bg_hover, &t))
                    .border_color(rgb(t.border_active))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _window, cx| {
                    this.pending_focus = Some(PendingFocus::SearchInput);
                    cx.notify();
                }),
            )
            .on_click(cx.listener({
                let pid = project_id.clone();
                move |this, event: &ClickEvent, _window, cx| {
                    this.selected_filtered_idx = row_click_idx;
                    if event.click_count() >= 2 {
                        this.confirm_and_switch(&pid, cx);
                    } else {
                        cx.notify();
                    }
                }
            }))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(SPACE_MD)
                    .items_center()
                    .child(
                        AppIcon::from_str(&project_icon)
                            .unwrap_or(AppIcon::Folder)
                            .size(px(18.0))
                            .text_color(rgb(t.get_folder_color(project_color))),
                    )
                    .child(name_cell),
            )
            .child(action_cell)
            .when(is_rename && is_duplicate, |row| {
                row.child(
                    deferred(
                        div()
                            .absolute()
                            .top(row_h)
                            .left(px(34.0))
                            .px(SPACE_MD)
                            .py(SPACE_XS)
                            .bg(surface_bg_t(t.bg_panel, &t))
                            .border_1()
                            .border_color(rgb(t.error))
                            .rounded(RADIUS_STD)
                            .shadow_md()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.error))
                            .whitespace_nowrap()
                            .child(i18n!(cx, "project.duplicate_name_error")),
                    )
                    .with_priority(1),
                )
            })
            .into_any_element()
    }

}

impl Render for ManageProjectsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let focus_handle = self.focus_handle.clone();

        self.ensure_search_input(cx);

        if !self.initial_focus_done {
            self.initial_focus_done = true;
            if let Some(search_input) = self.search_input.as_ref() {
                search_input.update(cx, |inp, cx| inp.focus(window, cx));
            }
        } else if let Some(pending) = self.pending_focus.take() {
            match pending {
                PendingFocus::SearchInput => {
                    if let Some(search_input) = self.search_input.as_ref() {
                        search_input.update(cx, |inp, cx| inp.focus(window, cx));
                    }
                }
                PendingFocus::ImportButton => {
                    window.focus(&self.import_button_focus, cx);
                }
                PendingFocus::NewButton => {
                    window.focus(&self.new_button_focus, cx);
                }
                PendingFocus::DeleteCancel => {
                    window.focus(&self.delete_cancel_focus, cx);
                }
                PendingFocus::DeleteConfirm => {
                    window.focus(&self.delete_confirm_focus, cx);
                }
            }
        }

        let om = self.overlay_manager.clone();
        let action_menu_element = if let Some(pid) = self.action_menu_project_id.clone() {
            let trigger = self
                .more_button_bounds
                .get(&pid)
                .cloned()
                .unwrap_or(Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(24.0), px(24.0)),
                ));
            let mut menu_bounds = trigger;
            menu_bounds.size.width = px(160.0);
            let pid_for_menu = pid;
            let only_one = self.workspace.read(cx).data.projects.len() <= 1;

            let menu = dropdown_overlay("mp-action-menu", &t, cx)
                .child(
                    canvas(
                        {
                            let om = om.clone();
                            move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
                                om.update(cx, |om, ctx| {
                                    om.set_overlay_bounds(&"mp-action-menu".into(), bounds, ctx);
                                });
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .child(
                    menu_item(
                        "mp-menu-rename",
                        AppIcon::Edit,
                        i18n!(cx, "common.action.rename"),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener({
                        let p = pid_for_menu.clone();
                        let om = om.clone();
                        move |this, _, window, cx| {
                            om.update(cx, |om, ctx| {
                                om.unregister_overlay(&"mp-action-menu".into(), ctx)
                            });
                            this.action_menu_project_id = None;
                            this.start_rename(&p, window, cx);
                        }
                    })),
                )
                .child(
                    menu_item(
                        "mp-menu-edit",
                        AppIcon::Settings,
                        i18n!(cx, "project.edit_project"),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener({
                        let p = pid_for_menu.clone();
                        let om = om.clone();
                        move |this, _, _, cx| {
                            om.update(cx, |om, ctx| {
                                om.unregister_overlay(&"mp-action-menu".into(), ctx)
                            });
                            this.action_menu_project_id = None;
                            this.open_edit_dialog(&p, cx);
                        }
                    })),
                )
                .child(
                    menu_item(
                        "mp-menu-duplicate",
                        AppIcon::Duplicate,
                        i18n!(cx, "project.copy_project"),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener({
                        let p = pid_for_menu.clone();
                        let om = om.clone();
                        move |this, _, _, cx| {
                            om.update(cx, |om, ctx| {
                                om.unregister_overlay(&"mp-action-menu".into(), ctx)
                            });
                            this.action_menu_project_id = None;
                            this.duplicate_project(&p, cx);
                        }
                    })),
                )
                .child(
                    menu_item(
                        "mp-menu-export",
                        AppIcon::FolderOutput,
                        i18n!(cx, "project.export_project"),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener({
                        let p = pid_for_menu.clone();
                        let om = om.clone();
                        move |this, _, _, cx| {
                            om.update(cx, |om, ctx| {
                                om.unregister_overlay(&"mp-action-menu".into(), ctx)
                            });
                            this.action_menu_project_id = None;
                            this.open_export_dialog(&p, cx);
                        }
                    })),
                )
                .child(if only_one {
                    menu_item_disabled(
                        "mp-menu-delete",
                        AppIcon::Trash,
                        i18n!(cx, "common.action.delete"),
                        &t,
                        cx,
                    )
                } else {
                    menu_item_with_color(
                        "mp-menu-delete",
                        AppIcon::Trash,
                        i18n!(cx, "common.action.delete"),
                        t.error,
                        t.error,
                        &t,
                        cx,
                    )
                    .on_click(cx.listener({
                        let p = pid_for_menu.clone();
                        let om = om.clone();
                        move |this, _, _, cx| {
                            om.update(cx, |om, ctx| {
                                om.unregister_overlay(&"mp-action-menu".into(), ctx)
                            });
                            this.action_menu_project_id = None;
                            this.delete_confirming_id = Some(p.clone());
                            cx.notify();
                        }
                    }))
                });
            Some(dropdown_anchored_below(menu_bounds, menu))
        } else {
            None
        };

        let focus_group = FocusGroup::new();
        if self.delete_confirming_id.is_some() {
            focus_group.add(self.delete_cancel_focus.clone());
            focus_group.add(self.delete_confirm_focus.clone());
        } else {
            if let Some(search_input) = self.search_input.as_ref() {
                focus_group.add(search_input.read(cx).focus_handle(cx));
            }
            focus_group.add(self.import_button_focus.clone());
            focus_group.add(self.new_button_focus.clone());
        }

        div()
            .child(
                modal_content("manage-projects-modal", cx)
                    .w(px(580.0))
                    .h(px(520.0))
                    .track_focus(&focus_handle)
                    .tab_cycle(&focus_group)
                    .key_context("ManageProjectsDialog")
                    .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                if this.action_menu_project_id.is_some() {
                    this.action_menu_project_id = None;
                    this.pending_focus = Some(PendingFocus::SearchInput);
                    cx.notify();
                    return;
                }
                if this.rename_input.is_some() {
                    this.cancel_rename(cx);
                    return;
                }
                if this.delete_confirming_id.is_some() {
                    this.delete_confirming_id = None;
                    this.pending_focus = Some(PendingFocus::SearchInput);
                    cx.notify();
                    return;
                }
                this.dismiss(cx);
            }))
            .on_action(cx.listener(|this, _: &RenameActiveNode, window, cx| {
                if this.rename_input.is_some() || this.action_menu_project_id.is_some() {
                    return;
                }
                if let Some(pid) = this.current_selected_project_id(cx) {
                    this.start_rename(&pid, window, cx);
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.rename_input.is_some() || this.action_menu_project_id.is_some() {
                    return;
                }

                // If in delete confirmation card:
                if this.delete_confirming_id.is_some() {
                    match event.keystroke.key.as_str() {
                        "left" | "right" => {
                            if this.delete_cancel_focus.is_focused(window) {
                                this.pending_focus = Some(PendingFocus::DeleteConfirm);
                            } else {
                                this.pending_focus = Some(PendingFocus::DeleteCancel);
                            }
                            cx.notify();
                            cx.stop_propagation();
                        }
                        "escape" => {
                            this.delete_confirming_id = None;
                            this.pending_focus = Some(PendingFocus::SearchInput);
                            cx.notify();
                            cx.stop_propagation();
                        }
                        _ => {}
                    }
                    return;
                }

                // If on bottom buttons:
                if this.import_button_focus.is_focused(window) {
                    match event.keystroke.key.as_str() {
                        "right" => {
                            this.pending_focus = Some(PendingFocus::NewButton);
                            cx.notify();
                            cx.stop_propagation();
                        }
                        "left" | "up" => {
                            this.pending_focus = Some(PendingFocus::SearchInput);
                            cx.notify();
                            cx.stop_propagation();
                        }
                        "escape" => {
                            this.dismiss(cx);
                            cx.stop_propagation();
                        }
                        _ => {}
                    }
                    return;
                }
                if this.new_button_focus.is_focused(window) {
                    match event.keystroke.key.as_str() {
                        "left" => {
                            this.pending_focus = Some(PendingFocus::ImportButton);
                            cx.notify();
                            cx.stop_propagation();
                        }
                        "right" | "up" => {
                            this.pending_focus = Some(PendingFocus::SearchInput);
                            cx.notify();
                            cx.stop_propagation();
                        }
                        "escape" => {
                            this.dismiss(cx);
                            cx.stop_propagation();
                        }
                        _ => {}
                    }
                    return;
                }

                // Normal list / search input:
                let count = this.filtered_indices.len();
                match event.keystroke.key.as_str() {
                    "down" => {
                        if count > 0 && this.selected_filtered_idx + 1 < count {
                            this.selected_filtered_idx += 1;
                            scroll_to_row(&this.scroll_handle, this.selected_filtered_idx);
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }
                    "up" => {
                        if this.selected_filtered_idx > 0 {
                            this.selected_filtered_idx -= 1;
                            scroll_to_row(&this.scroll_handle, this.selected_filtered_idx);
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }
                    "enter" => {
                        if let Some(pid) = this.current_selected_project_id(cx) {
                            this.confirm_and_switch(&pid, cx);
                        }
                        cx.stop_propagation();
                    }
                    "delete" => {
                        let is_input_empty = this
                            .search_input
                            .as_ref()
                            .is_none_or(|inp| inp.read(cx).text().is_empty());
                        if is_input_empty {
                            this.trigger_delete_on_selected(cx);
                            cx.stop_propagation();
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_down(
                        MouseButton::Left,
                        cx.listener({
                            let om = self.overlay_manager.clone();
                            move |this, _, _window, cx| {
                                cx.stop_propagation();
                                if this.action_menu_project_id.is_some() {
                                    om.update(cx, |om, ctx| {
                                        om.unregister_overlay(
                                            &"mp-action-menu".into(),
                                            ctx,
                                        );
                                    });
                                    this.action_menu_project_id = None;
                                    cx.notify();
                                }
                            }
                        }),
                    )
                    .child(self.render_list_view(window, cx).into_any_element())
                    .child(
                        div()
                            .id("manage-projects-close")
                            .absolute()
                            .top(SPACE_MD)
                            .right(SPACE_MD)
                            .cursor_pointer()
                            .w(px(28.0))
                            .h(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_STD)
                            .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                            .child(
                                AppIcon::Close
                                    .size(px(16.0))
                                    .text_color(rgb(t.text_secondary)),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.dismiss(cx)),
                            ),
                    ),
            )
            .when_some(action_menu_element, |d, o| d.child(o))
    }
}

impl_focusable!(ManageProjectsDialog);

// ========================================================================
// Fuzzy project ranking and matching
// ========================================================================

fn ranked_project_filter(items: &[ProjectData], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..items.len()).collect();
    }

    let query_lower = query.to_lowercase();
    let mut scored: Vec<(usize, i32)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, project)| {
            project_match_score(project, &query_lower).map(|score| (index, score))
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scored.into_iter().map(|(index, _)| index).collect()
}

fn project_match_score(project: &ProjectData, query: &str) -> Option<i32> {
    let name = project.name.to_lowercase();
    let path = project.path.to_lowercase();
    let base_dir = std::path::Path::new(&project.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_lowercase();

    let primary_score = [
        text_match_score(&name, query, 700, 420, 220),
        if base_dir == name {
            None
        } else {
            text_match_score(&base_dir, query, 650, 380, 200)
        },
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0);

    let best_segment_score = path
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .enumerate()
        .filter_map(|(depth, segment)| {
            text_match_score(segment, query, 240, 140, 70).map(|score| {
                let nested_bonus = ((depth as i32 + 1) * 18).min(90);
                let leaf_bonus = if segment == base_dir { 40 } else { 0 };
                score + nested_bonus + leaf_bonus
            })
        })
        .max()
        .unwrap_or(0);

    let path_score = if path.contains(query) {
        let tail_bias = path
            .rfind(query)
            .map(|index| ((index as i32) * 40) / path.len().max(1) as i32)
            .unwrap_or(0);
        30 + tail_bias
    } else {
        0
    };

    let total_score = primary_score + best_segment_score + path_score;
    (total_score > 0).then_some(total_score)
}

fn text_match_score(
    text: &str,
    query: &str,
    exact_bonus: i32,
    prefix_bonus: i32,
    contains_bonus: i32,
) -> Option<i32> {
    if !text.contains(query) {
        return None;
    }

    let closeness_bonus = (24 - text.len().saturating_sub(query.len()) as i32).max(0);
    let score = if text == query {
        exact_bonus
    } else if text.starts_with(query) {
        prefix_bonus
    } else {
        contains_bonus
    } + closeness_bonus;

    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::init_settings;
    use crate::terminal::shell_config::ShellType;
    use crate::theme::FolderColor;
    use crate::workspace::state::{LayoutNode, ProjectData, WindowState, WorkspaceData};
    use std::collections::HashMap;

    fn make_project(name: &str, path: &str) -> ProjectData {
        ProjectData {
            id: name.to_string(),
            name: name.to_string(),
            path: path.to_string(),
            layout: None::<LayoutNode>,
            terminal_names: HashMap::new(),
            hidden_terminals: HashMap::new(),
            folder_color: FolderColor::default(),
            is_remote: false,
            connection_id: None,
            service_terminals: HashMap::new(),
            default_shell: None::<ShellType>,
            pinned: false,
            last_activity_at: None,
            ..Default::default()
        }
    }

    #[test]
    fn prefers_project_name_over_generic_path_match() {
        let target = make_project("roj", "/home/matej21/projects/oss/roj");
        let generic = make_project("alpha", "/home/matej21/projects/oss/projects-alpha");

        let target_score = project_match_score(&target, "roj").unwrap();
        let generic_score = project_match_score(&generic, "roj").unwrap();

        assert!(target_score > generic_score);
    }

    #[test]
    fn prefers_more_nested_segment_matches() {
        let nested = make_project("alpha", "/home/matej21/projects/oss/roj");
        let shallow = make_project("alpha", "/roj/worktrees/demo");

        let nested_score = project_match_score(&nested, "roj").unwrap();
        let shallow_score = project_match_score(&shallow, "roj").unwrap();

        assert!(nested_score > shallow_score);
    }

    #[test]
    fn ranked_filter_sorts_best_match_first() {
        let items = vec![
            make_project("alpha", "/home/matej21/projects/oss/projects-alpha"),
            make_project("roj", "/home/matej21/projects/oss/roj"),
            make_project("beta", "/home/matej21/projects/oss/other"),
        ];

        let filtered = ranked_project_filter(&items, "roj");

        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0], 1);
        assert_eq!(filtered[1], 0);
    }

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
    async fn test_project_manage_dialog_search_input_init(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            init_settings(cx);
            velowork_i18n::init_locale(velowork_i18n::Locale::Zh, cx);
            let window_id = WindowId::Main;
            let workspace = cx.new(|_| Workspace::new(empty_workspace()));
            let focus_manager = cx.new(|_| FocusManager::new());
            let request_broker = cx.new(|_| velowork_workspace::request_broker::RequestBroker::new());
            let overlay_registry = cx.new(|_| velowork_ui::overlay_registry::OverlayRegistry::new());
            let overlay_manager = cx.new(|_| {
                OverlayManager::new(
                    window_id,
                    None,
                    workspace.clone(),
                    focus_manager.clone(),
                    request_broker,
                    overlay_registry,
                )
            });
            let dialog = cx.new(|cx| {
                ManageProjectsDialog::new(workspace, window_id, focus_manager, overlay_manager, cx)
            });
            assert!(dialog.read(cx).search_input.is_some());
            assert!(!dialog.read(cx).initial_focus_done);
            assert_eq!(dialog.read(cx).pending_focus, Some(PendingFocus::SearchInput));
        });
    }
}
