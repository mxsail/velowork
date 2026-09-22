//! Project Export Dialog
//!
//! Provides module selection and optional password protection for single project export.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::*;
use velowork_core::theme::ThemeColors;
use velowork_i18n::i18n;
use velowork_ui::button::Button;
use velowork_ui::design::appearance::ControlSize;
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::h_flex;
use velowork_ui::icon::AppIcon;
use velowork_ui::input::{Input, InputContentType, InputState};
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::theme::{theme, with_alpha};
use velowork_ui::tokens::{
    RADIUS_MD, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS,
    ui_text_md, ui_text_sm,
};
use velowork_ui::v_flex;
use velowork_workspace::project_export::{ProjectExportOptions, ProjectExportService};
use velowork_workspace::state::Workspace;

use crate::keybindings::Cancel;
use crate::views::components::{modal_content, modal_header};

use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};

pub struct ProjectExportDialog {
    workspace: Entity<Workspace>,
    project_id: String,
    focus_handle: FocusHandle,
    cancel_focus: FocusHandle,
    confirm_focus: FocusHandle,
    password_input: Option<Entity<InputState>>,
    include_sessions: bool,
    include_ai_records: bool,
    include_tunnels: bool,
    include_services: bool,
    include_snippets: bool,
    include_session_history: bool,
    is_exporting: bool,
    status_message: Option<String>,
    is_error: bool,
    scroll_handle: ScrollHandle,
    initial_focus_done: bool,
}

pub enum ProjectExportDialogEvent {
    Close,
}

impl EventEmitter<ProjectExportDialogEvent> for ProjectExportDialog {}

impl ProjectExportDialog {
    pub fn new(
        workspace: Entity<Workspace>,
        project_id: String,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            workspace,
            project_id,
            focus_handle: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            password_input: None,
            include_sessions: true,
            include_ai_records: true,
            include_tunnels: true,
            include_services: true,
            include_snippets: true,
            include_session_history: true,
            is_exporting: false,
            status_message: None,
            is_error: false,
            scroll_handle: ScrollHandle::new(),
            initial_focus_done: false,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        if !self.is_exporting {
            cx.emit(ProjectExportDialogEvent::Close);
        }
    }

    fn execute_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_exporting {
            return;
        }

        let project = self
            .workspace
            .read(cx)
            .project(&self.project_id)
            .cloned();

        let Some(project) = project else {
            self.status_message = Some(i18n!(cx, "project.not_found"));
            self.is_error = true;
            cx.notify();
            return;
        };

        let options = ProjectExportOptions {
            include_sessions: true,
            include_ai_records: self.include_ai_records,
            include_tunnels: self.include_tunnels,
            include_services: self.include_services,
            include_snippets: self.include_snippets,
            include_session_history: self.include_session_history,
        };

        let pwd_raw = self
            .password_input
            .as_ref()
            .map(|inp| inp.read(cx).value().trim().to_string())
            .unwrap_or_default();
        let password = if pwd_raw.is_empty() {
            None
        } else {
            Some(pwd_raw)
        };

        let quick_commands = velowork_app_core::settings::settings_entity(cx)
            .read(cx)
            .settings
            .quick_commands_for_project(Some(&self.project_id))
            .to_vec();

        let safe_name = project.name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let default_file_name = format!("{}.vproj", if safe_name.is_empty() { "project" } else { &safe_name });
        let default_path = PathBuf::from(&default_file_name);

        let prompt_future = cx.prompt_for_new_path(&default_path, None);

        self.is_exporting = true;
        self.status_message = Some(i18n!(cx, "project.export.exporting"));
        self.is_error = false;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let res = prompt_future.await;
            match res {
                Ok(Ok(Some(save_path))) => {
                    let export_res = (|| -> anyhow::Result<()> {
                        let bytes = ProjectExportService::export_to_bytes(
                            &project,
                            &options,
                            password.as_deref(),
                            None,
                            Some(&quick_commands),
                        )?;
                        std::fs::write(&save_path, bytes)?;
                        Ok(())
                    })();

                    let _ = this.update(cx, |this, cx| {
                        this.is_exporting = false;
                        match export_res {
                            Ok(()) => {
                                this.status_message = Some(i18n!(cx, "project.export.success"));
                                this.is_error = false;
                                cx.notify();
                                this.close(cx);
                            }
                            Err(e) => {
                                this.status_message = Some(format!("{}: {}", i18n!(cx, "project.export.export_btn"), e));
                                this.is_error = true;
                                cx.notify();
                            }
                        }
                    });
                }
                _ => {
                    let _ = this.update(cx, |this, cx| {
                        this.is_exporting = false;
                        this.status_message = None;
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn render_checkbox_item(
        &self,
        id: &'static str,
        label: String,
        checked: bool,
        disabled: bool,
        icon: AppIcon,
        on_toggle: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static + Send + Sync,
        t: &ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let border_color = if checked {
            rgb(t.accent)
        } else {
            rgb(t.border)
        };
        let bg_color = if checked {
            rgb(t.accent)
        } else {
            rgb(t.bg_secondary)
        };

        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        h_flex()
            .id(id)
            .w_full()
            .py(SPACE_SM)
            .px(SPACE_MD)
            .rounded(RADIUS_STD)
            .items_center()
            .justify_between()
            .border_1()
            .border_color(p.border_subtle)
            .bg(p.surface_card)
            .when(!disabled, |el| {
                el.cursor_pointer()
                    .hover(|s| s.bg(p.surface_hover))
            })
            .when(disabled, |el| el.opacity(0.8))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| {
                if !disabled {
                    on_toggle(this, window, cx);
                }
            }))
            .child(
                h_flex()
                    .gap(SPACE_MD)
                    .items_center()
                    .child(
                        icon.size(px(16.0))
                            .text_color(rgb(t.text_secondary)),
                    )
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .text_color(rgb(t.text_primary))
                            .child(label),
                    ),
            )
            .child(
                div()
                    .w(px(18.0))
                    .h(px(18.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(border_color)
                    .bg(bg_color)
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(checked, |el| {
                        el.child(
                            AppIcon::Check
                                .size(px(12.0))
                                .text_color(rgb(0xffffff)),
                        )
                    }),
            )
    }
}

impl Render for ProjectExportDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let project = self.workspace.read(cx).project(&self.project_id).cloned();
        let project_name = project.as_ref().map(|p| p.name.clone()).unwrap_or_default();
        let project_icon = project.as_ref().map(|p| p.icon.clone()).unwrap_or_default();
        let project_color = project.as_ref().map(|p| p.folder_color).unwrap_or(velowork_core::theme::FolderColor::Indigo);

        if self.password_input.is_none() {
            let pwd_ph = i18n!(cx, "project.export.password_placeholder");
            let input = cx.new(|cx| InputState::new(cx).placeholder(pwd_ph).masked(true));
            self.password_input = Some(input);
        }
        let password_input = self.password_input.as_ref().unwrap();

        if !self.initial_focus_done {
            self.initial_focus_done = true;
            password_input.update(cx, |inp, cx| inp.focus(window, cx));
        }

        let focus_group = FocusGroup::new();
        focus_group.add(password_input.read(cx).focus_handle(cx));
        focus_group.add(self.cancel_focus.clone());
        focus_group.add(self.confirm_focus.clone());

        let win_size = window.viewport_size();
        let card_w = px(480.0).min(win_size.width - px(48.0));
        let card_h = px(580.0).min(win_size.height - px(80.0)).max(px(240.0));

        let body_content = v_flex()
            .id(ElementId::Name("project-export-scroll".into()))
            .relative()
            .flex()
            .flex_col()
            .p(px(20.0))
            .gap(SPACE_LG)
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .min_h(px(0.0))
            .w_full()
                            .child(
                                // Project info card
                                h_flex()
                                    .p(SPACE_MD)
                                    .gap(SPACE_MD)
                                    .items_center()
                                    .bg(p.surface_card)
                                    .rounded(RADIUS_MD)
                                    .border_1()
                                    .border_color(p.border_subtle)
                                    .child(
                                        AppIcon::from_str(&project_icon)
                                            .unwrap_or(AppIcon::Folder)
                                            .size(px(22.0))
                                            .text_color(rgb(t.get_folder_color(project_color))),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .text_size(ui_text_md(cx))
                                                    .text_color(rgb(t.text_primary))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .truncate()
                                                    .child(project_name),
                                            ),
                                    ),
                            )
                            .child(
                                // Module selection list
                                v_flex()
                                    .gap(SPACE_SM)
                                    .child(
                                        div()
                                            .text_size(ui_text_md(cx))
                                            .text_color(rgb(t.text_secondary))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(i18n!(cx, "project.import.modules_to_import")),
                                    )
                                    .child(self.render_checkbox_item(
                                        "export-mod-sessions",
                                        i18n!(cx, "project.export.module_sessions"),
                                        self.include_sessions,
                                        true, // mandatory
                                        AppIcon::Terminal,
                                        |_, _, _| {},
                                        &t,
                                        cx,
                                    ))
                                    .child(self.render_checkbox_item(
                                        "export-mod-ai",
                                        i18n!(cx, "project.export.module_ai_records"),
                                        self.include_ai_records,
                                        false,
                                        AppIcon::AiAssistant,
                                        |this, _, cx| {
                                            this.include_ai_records = !this.include_ai_records;
                                            cx.notify();
                                        },
                                        &t,
                                        cx,
                                    ))
                                    .child(self.render_checkbox_item(
                                        "export-mod-tunnels",
                                        i18n!(cx, "project.export.module_tunnels"),
                                        self.include_tunnels,
                                        false,
                                        AppIcon::Download,
                                        |this, _, cx| {
                                            this.include_tunnels = !this.include_tunnels;
                                            cx.notify();
                                        },
                                        &t,
                                        cx,
                                    ))
                                    .child(self.render_checkbox_item(
                                        "export-mod-services",
                                        i18n!(cx, "project.export.module_services"),
                                        self.include_services,
                                        false,
                                        AppIcon::SquareActivity,
                                        |this, _, cx| {
                                            this.include_services = !this.include_services;
                                            cx.notify();
                                        },
                                        &t,
                                        cx,
                                    ))
                                    .child(self.render_checkbox_item(
                                        "export-mod-snippets",
                                        i18n!(cx, "project.export.module_snippets"),
                                        self.include_snippets,
                                        false,
                                        AppIcon::QuickCommand,
                                        |this, _, cx| {
                                            this.include_snippets = !this.include_snippets;
                                            cx.notify();
                                        },
                                        &t,
                                        cx,
                                    ))
                                    .child(self.render_checkbox_item(
                                        "export-mod-history",
                                        i18n!(cx, "project.export.module_session_history"),
                                        self.include_session_history,
                                        false,
                                        AppIcon::Refresh,
                                        |this, _, cx| {
                                            this.include_session_history = !this.include_session_history;
                                            cx.notify();
                                        },
                                        &t,
                                        cx,
                                    )),
                            )
                            .child(
                                // Password protection (optional)
                                v_flex()
                                    .gap(SPACE_SM)
                                    .child(
                                        div()
                                            .text_size(ui_text_md(cx))
                                            .text_color(rgb(t.text_secondary))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(i18n!(cx, "project.export.password_label")),
                                    )
                                    .child(
                                        Input::new(password_input)
                                            .content_type(InputContentType::Password)
                                            .cleanable(true)
                                            .mask_toggle(true),
                                    ),
                            )
                            .when_some(self.status_message.clone(), |el, msg| {
                                let color = if self.is_error { rgb(t.error) } else { rgb(t.success) };
                                el.child(
                                    div()
                                        .px(SPACE_MD)
                                        .py(SPACE_SM)
                                        .rounded(RADIUS_STD)
                                        .bg(with_alpha(if self.is_error { t.error } else { t.success }, 0.15))
                                        .text_color(color)
                                        .text_size(ui_text_sm(cx))
                                        .child(msg)
                                )
                            });

        let body_container = div()
            .relative()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_hidden()
            .child(body_content.h_full())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .left_0()
                    .child(Scrollbar::vertical(&self.scroll_handle)),
            );

        modal_content("project-export-modal", cx)
            .relative()
            .w(card_w)
            .h(card_h)
            .overflow_hidden()
            .p(SPACE_XS)
            .track_focus(&self.focus_handle)
            .key_context("ProjectExportDialog")
            .tab_cycle(&focus_group)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| this.close(cx)))
            .focus_scope_on_click(&self.focus_handle)
            .child(modal_header(
                i18n!(cx, "project.export.title"),
                Some(i18n!(cx, "project.export.desc")),
                &t,
                cx,
                cx.listener(|this: &mut Self, _, _, cx| this.close(cx)),
            ))
            .child(body_container)
            .child(
                // Footer actions
                h_flex()
                    .h(px(48.0))
                    .flex_shrink_0()
                    .px(SPACE_LG)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .items_center()
                    .justify_end()
                    .gap(SPACE_MD)
                    .child(
                        Button::new("export-cancel-btn", &t)
                            .size(ControlSize::Default)
                            .label(i18n!(cx, "common.action.cancel"))
                            .focus_handle(&self.cancel_focus)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(
                        Button::new("export-execute-btn", &t)
                            .primary()
                            .size(ControlSize::Default)
                            .label(if self.is_exporting {
                                i18n!(cx, "project.export.exporting")
                            } else {
                                i18n!(cx, "project.export.export_btn")
                            })
                            .disabled(self.is_exporting)
                            .focus_handle(&self.confirm_focus)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.execute_export(window, cx);
                            })),
                    ),
            )
    }
}
