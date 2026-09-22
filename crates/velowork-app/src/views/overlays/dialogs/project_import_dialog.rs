//! Project Import Dialog
//!
//! Provides file browsing, password decryption, package inspection,
//! module selection, duplicate handling, and animated import execution.

use gpui::prelude::*;
use gpui::*;
use velowork_core::storage::database;
use velowork_core::theme::ThemeColors;
use velowork_i18n::i18n;
use velowork_ui::button::Button;
use velowork_ui::design::appearance::ControlSize;
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::h_flex;
use velowork_ui::icon::AppIcon;
use velowork_ui::input::{Input, InputState};
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::theme::{theme, with_alpha};
use velowork_ui::tokens::{
    RADIUS_MD, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS,
    ui_text_md, ui_text_sm,
};
use velowork_ui::v_flex;
use velowork_workspace::project_export::{
    ParsedProjectSummary, ProjectDuplicateStrategy, ProjectExportService,
};
use velowork_workspace::repositories::ai::{AiConversationRow, AiMessageRow, AiRepository};
use velowork_workspace::repositories::history::HistoryRepository;
use velowork_workspace::repositories::service_tree::ServiceTreeRepository;
use velowork_workspace::repositories::snippet::SnippetRepository;
use velowork_workspace::repositories::ssh_session_tree::SshSessionTreeRepository;
use velowork_workspace::repositories::tunnel_tree::TunnelTreeRepository;
use velowork_workspace::stores::{GlobalServiceStore, GlobalSessionStore, GlobalTunnelStore};
use velowork_workspace::state::{WindowId, Workspace};

use crate::keybindings::Cancel;
use crate::views::components::{modal_content, modal_header};

use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};

pub struct ProjectImportDialog {
    workspace: Entity<Workspace>,
    window_id: WindowId,
    focus_handle: FocusHandle,
    cancel_focus: FocusHandle,
    confirm_focus: FocusHandle,
    file_path_input: Option<Entity<InputState>>,
    password_input: Option<Entity<InputState>>,
    is_parsing: bool,
    is_importing: bool,
    parse_error: Option<String>,
    parsed_summary: Option<ParsedProjectSummary>,
    // Module checkboxes
    include_sessions: bool,
    include_ai_records: bool,
    include_tunnels: bool,
    include_services: bool,
    include_snippets: bool,
    include_session_history: bool,
    // Duplicate resolution
    duplicate_strategy: ProjectDuplicateStrategy,
    // Status message
    status_message: Option<String>,
    is_status_error: bool,
    scroll_handle: ScrollHandle,
    initial_focus_done: bool,
}

pub enum ProjectImportDialogEvent {
    Close,
    ProjectImported(String),
}

impl EventEmitter<ProjectImportDialogEvent> for ProjectImportDialog {}

impl ProjectImportDialog {
    pub fn new(
        workspace: Entity<Workspace>,
        window_id: WindowId,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            workspace,
            window_id,
            focus_handle: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            file_path_input: None,
            password_input: None,
            is_parsing: false,
            is_importing: false,
            parse_error: None,
            parsed_summary: None,
            include_sessions: true,
            include_ai_records: true,
            include_tunnels: true,
            include_services: true,
            include_snippets: true,
            include_session_history: true,
            duplicate_strategy: ProjectDuplicateStrategy::Rename,
            status_message: None,
            is_status_error: false,
            scroll_handle: ScrollHandle::new(),
            initial_focus_done: false,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        if !self.is_importing {
            cx.emit(ProjectImportDialogEvent::Close);
        }
    }

    fn browse_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_importing || self.is_parsing {
            return;
        }

        let prompt_opts = PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(i18n!(cx, "project.import.file_label").into()),
        };

        let paths_future = cx.prompt_for_paths(prompt_opts);
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(selected))) = paths_future.await
                && let Some(path) = selected.first().cloned()
            {
                let path_str = path.to_string_lossy().to_string();
                let _ = cx.update(|_window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(input) = &this.file_path_input {
                            input.update(cx, |st, cx| {
                                st.set_value(&path_str, cx);
                            });
                        }
                        this.parse_error = None;
                        this.parsed_summary = None;
                        cx.notify();
                    });
                });
            }
        })
        .detach();
    }

    fn parse_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_parsing || self.is_importing {
            return;
        }

        let path_str = self
            .file_path_input
            .as_ref()
            .map(|st| st.read(cx).value().trim().to_string())
            .unwrap_or_default();
        if path_str.is_empty() {
            self.parse_error = Some(i18n!(cx, "project.import.error_file_empty"));
            cx.notify();
            return;
        }

        let pwd_raw = self
            .password_input
            .as_ref()
            .map(|st| st.read(cx).value().trim().to_string())
            .unwrap_or_default();
        let password = if pwd_raw.is_empty() { None } else { Some(pwd_raw) };

        self.is_parsing = true;
        self.parse_error = None;
        self.parsed_summary = None;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let res = (|| -> Result<ParsedProjectSummary, String> {
                let bytes = std::fs::read(&path_str).map_err(|e| e.to_string())?;
                ProjectExportService::parse_package(&bytes, password.as_deref()).map_err(|e| {
                    let err_str = e.to_string();
                    if err_str.contains("PASSWORD_REQUIRED") {
                        "PASSWORD_REQUIRED".to_string()
                    } else if err_str.contains("PASSWORD_INCORRECT") {
                        "PASSWORD_INCORRECT".to_string()
                    } else {
                        err_str
                    }
                })
            })();

            let _ = this.update(cx, |this, cx| {
                this.is_parsing = false;
                match res {
                    Ok(summary) => {
                        this.parse_error = None;
                        this.include_sessions = true;
                        this.include_ai_records = summary.has_ai_records;
                        this.include_tunnels = summary.tunnel_count > 0;
                        this.include_services = summary.service_count > 0;
                        this.include_snippets = summary.snippet_count > 0 || summary.quick_command_count > 0;
                        this.include_session_history = summary.history_count > 0;
                        this.parsed_summary = Some(summary);
                    }
                    Err(code) => {
                        let msg = match code.as_str() {
                            "PASSWORD_REQUIRED" => i18n!(cx, "project.import.error_password_required"),
                            "PASSWORD_INCORRECT" => i18n!(cx, "project.import.error_password_incorrect"),
                            other => format!("{}: {}", i18n!(cx, "project.import.error_invalid_file"), other),
                        };
                        this.parse_error = Some(msg);
                        this.parsed_summary = None;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn execute_import(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_importing {
            return;
        }

        let Some(summary) = self.parsed_summary.clone() else {
            return;
        };

        self.is_importing = true;
        self.status_message = Some(i18n!(cx, "project.import.importing"));
        self.is_status_error = false;
        cx.notify();

        let pkg = summary.package;
        let mut target_name = pkg.project.name.trim().to_string();
        if target_name.is_empty() {
            target_name = "Imported Project".to_string();
        }

        let existing_projects = self.workspace.read(cx).projects().to_vec();
        let name_exists = existing_projects.iter().any(|p| p.name.trim().eq_ignore_ascii_case(&target_name));

        if name_exists {
            match self.duplicate_strategy {
                ProjectDuplicateStrategy::Skip => {
                    self.is_importing = false;
                    self.status_message = Some(format!("{}: {}", target_name, i18n!(cx, "project.import.strategy_skip")));
                    self.is_status_error = true;
                    cx.notify();
                    return;
                }
                ProjectDuplicateStrategy::Rename => {
                    let mut index = 1;
                    let mut new_name = format!("{} ({})", target_name, i18n!(cx, "project.copied_suffix"));
                    while existing_projects.iter().any(|p| p.name.trim().eq_ignore_ascii_case(&new_name)) {
                        index += 1;
                        new_name = format!("{} ({})", target_name, index);
                    }
                    target_name = new_name;
                }
                ProjectDuplicateStrategy::Overwrite => {
                    // Update existing
                }
            }
        }

        let window_id = self.window_id;
        let new_project_id = self.workspace.update(cx, |ws, cx| {
            let pid = ws.add_project(target_name.clone(), pkg.project.path.clone(), true, window_id, cx);
            ws.set_folder_color(&pid, pkg.project.folder_color, cx);
            ws.set_project_icon(&pid, pkg.project.icon.clone(), cx);
            ws.set_project_description(&pid, pkg.project.description.clone(), cx);
            ws.toggle_hidden(window_id, &pid, cx);
            pid
        });

        // 1. Sessions: update project_id on rows and insert into database
        if self.include_sessions && let Some(mut sessions) = pkg.sessions {
            if let Some(db) = database() {
                let repo = SshSessionTreeRepository::new(db);
                for row in &mut sessions {
                    row.project_id = Some(new_project_id.clone());
                }
                let _ = repo.insert_project_rows(Some(&new_project_id), &sessions);
            }
            if cx.has_global::<GlobalSessionStore>() {
                let store = cx.global::<GlobalSessionStore>().0.clone();
                store.update(cx, |s, cx| s.reload_from_disk(cx));
            }
        }

        // 2. AI records: insert into database under new_project_id
        if self.include_ai_records {
            if let Some(db) = database() {
                let repo = AiRepository::new(db);
                let now_iso = chrono::Utc::now().to_rfc3339();
                if let Some(conv_items) = pkg.ai_conversations {
                    for item in conv_items {
                        let new_conv_id = format!("conv_{}_{}", new_project_id, uuid::Uuid::new_v4());
                        let mut conv_row = item.conversation;
                        conv_row.id = new_conv_id.clone();
                        conv_row.project_id = Some(new_project_id.clone());
                        conv_row.updated_at = now_iso.clone();
                        let _ = repo.save_conversation(&conv_row);

                        for mut m in item.messages {
                            m.id = uuid::Uuid::new_v4().to_string();
                            m.conversation_id = new_conv_id.clone();
                            let _ = repo.save_message(&m);
                        }
                    }
                } else if let Some(records) = pkg.ai_records {
                    let conv_id = format!("conv_{}", new_project_id);
                    let conv_row = AiConversationRow {
                        id: conv_id.clone(),
                        profile_id: Some("default".into()),
                        project_id: Some(new_project_id.clone()),
                        title: Some("Imported Conversation".into()),
                        provider_id: None,
                        model: None,
                        status: "active".into(),
                        context_mode: "session".into(),
                        created_at: now_iso.clone(),
                        updated_at: now_iso.clone(),
                        revision: 1,
                        device_id: String::new(),
                    };
                    let _ = repo.save_conversation(&conv_row);

                    if let Ok(msgs) = serde_json::from_value::<Vec<AiMessageRow>>(records) {
                        for mut m in msgs {
                            m.id = uuid::Uuid::new_v4().to_string();
                            m.conversation_id = conv_id.clone();
                            let _ = repo.save_message(&m);
                        }
                    }
                }
            }
        }

        // 3. Tunnels: add to tunnels repository and reload store
        if self.include_tunnels && let Some(tunnels) = pkg.tunnels {
            if let Some(db) = database() {
                let repo = TunnelTreeRepository::new(db);
                if let Ok(mut nodes) = repo.load_tree() {
                    for mut t in tunnels {
                        t.project_id = Some(new_project_id.clone());
                        nodes.push(velowork_state::TunnelNode::Tunnel { profile: t });
                    }
                    let _ = repo.save_tree(&nodes);
                }
            }
            if cx.has_global::<GlobalTunnelStore>() {
                let store = cx.global::<GlobalTunnelStore>().0.clone();
                store.update(cx, |s, cx| s.reload_from_disk(cx));
            }
        }

        // 4. Services: add to service tree repository and reload store
        if self.include_services && let Some(services) = pkg.services {
            if let Some(db) = database() {
                let repo = ServiceTreeRepository::new(db);
                if let Ok(mut nodes) = repo.load_tree() {
                    for mut s in services {
                        s.project_id = Some(new_project_id.clone());
                        nodes.push(velowork_state::ServiceNode::Service { def: s });
                    }
                    let _ = repo.save_tree(&nodes);
                }
            }
            if cx.has_global::<GlobalServiceStore>() {
                let store = cx.global::<GlobalServiceStore>().0.clone();
                store.update(cx, |s, cx| s.reload_from_disk(cx));
            }
        }

        // 5. Quick Commands & Snippets
        if self.include_snippets {
            if let Some(qc_nodes) = pkg.quick_commands {
                let settings_ent = velowork_app_core::settings::settings_entity(cx);
                settings_ent.update(cx, |st, cx| {
                    let tree = st.settings.quick_commands_for_project_mut(Some(&new_project_id));
                    *tree = qc_nodes;
                    st.save_and_notify(cx);
                });
            }
            if let Some(snippets) = pkg.snippets {
                if let Some(db) = database() {
                    let repo = SnippetRepository::new(db);
                    for s in snippets {
                        let mut snippet = velowork_workspace::repositories::snippet::Snippet::new(&s.name, &s.content);
                        snippet.language = s.language;
                        snippet.tags = s.tags;
                        let _ = repo.insert(&snippet);
                    }
                }
            }
        }

        // 6. Session / Command History
        if self.include_session_history && let Some(history) = pkg.session_history {
            if let Some(db) = database() {
                let repo = HistoryRepository::new(db);
                for entry in history {
                    let _ = repo.record_project_command(&new_project_id, &entry.command, 5000, 0);
                }
            }
        }

        self.is_importing = false;
        cx.emit(ProjectImportDialogEvent::ProjectImported(new_project_id));
        cx.emit(ProjectImportDialogEvent::Close);
    }

    fn render_checkbox_item(
        &self,
        id: &'static str,
        label: String,
        count_desc: Option<String>,
        checked: bool,
        disabled: bool,
        icon: AppIcon,
        on_toggle: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        _t: &ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                            .text_color(p.text_secondary),
                    )
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .text_color(p.text_primary)
                            .child(label),
                    )
                    .when_some(count_desc, |el, desc| {
                        el.child(
                            div()
                                .text_size(ui_text_sm(cx))
                                .text_color(p.text_muted)
                                .child(desc),
                        )
                    }),
            )
            .child(
                div()
                    .w(px(18.0))
                    .h(px(18.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(if checked {
                        p.surface_accent
                    } else {
                        p.border_subtle
                    })
                    .bg(if checked {
                        p.surface_accent
                    } else {
                        gpui::transparent_black()
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(checked, |el| {
                        el.child(
                            AppIcon::Check
                                .size(px(12.0))
                                .text_color(p.text_on_accent),
                        )
                    }),
            )
    }

    fn render_strategy_button(
        &self,
        id: ElementId,
        label: String,
        strategy: ProjectDuplicateStrategy,
        _t: &ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = self.duplicate_strategy == strategy;
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        div()
            .id(id)
            .flex_1()
            .py(SPACE_SM)
            .px(SPACE_MD)
            .rounded(RADIUS_STD)
            .border_1()
            .cursor_pointer()
            .items_center()
            .justify_center()
            .flex()
            .when(is_selected, |el| {
                el.bg(p.surface_accent)
                    .border_color(p.surface_accent)
                    .text_color(p.text_on_accent)
            })
            .when(!is_selected, |el| {
                el.bg(p.surface_card)
                    .border_color(p.border_subtle)
                    .text_color(p.text_secondary)
                    .hover(|s| s.bg(p.surface_hover))
            })
            .text_size(ui_text_md(cx))
            .child(label)
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                this.duplicate_strategy = strategy;
                cx.notify();
            }))
    }
}

impl Render for ProjectImportDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);

        if self.file_path_input.is_none() {
            let file_ph = i18n!(cx, "project.import.file_placeholder");
            let input = cx.new(|cx| InputState::new(cx).placeholder(file_ph));
            self.file_path_input = Some(input);
        }
        let file_path_input = self.file_path_input.as_ref().unwrap();

        if !self.initial_focus_done {
            self.initial_focus_done = true;
            file_path_input.update(cx, |inp, cx| inp.focus(window, cx));
        }

        if self.password_input.is_none() {
            let pwd_ph = i18n!(cx, "project.import.password_placeholder");
            let input = cx.new(|cx| InputState::new(cx).placeholder(pwd_ph).masked(true));
            self.password_input = Some(input);
        }
        let password_input = self.password_input.as_ref().unwrap();

        let has_path = !file_path_input.read(cx).value().trim().is_empty();

        let focus_group = FocusGroup::new();
        focus_group.add(file_path_input.read(cx).focus_handle(cx));
        focus_group.add(password_input.read(cx).focus_handle(cx));
        focus_group.add(self.cancel_focus.clone());
        focus_group.add(self.confirm_focus.clone());

        let win_size = window.viewport_size();
        let card_w = px(520.0).min(win_size.width - px(48.0));
        let card_h = px(640.0).min(win_size.height - px(80.0)).max(px(240.0));

        let body_content = v_flex()
            .id(ElementId::Name("project-import-scroll".into()))
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
                                // File picker row
                                v_flex()
                                    .gap(SPACE_SM)
                                    .child(
                                        div()
                                            .text_size(ui_text_md(cx))
                                            .text_color(rgb(t.text_secondary))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(i18n!(cx, "project.import.file_label")),
                                    )
                                    .child(
                                        h_flex()
                                            .gap(SPACE_SM)
                                            .items_center()
                                            .child(
                                                div().flex_1().min_w_0().child(
                                                    Input::new(file_path_input).cleanable(true),
                                                ),
                                            )
                                            .child(
                                                Button::new("browse-vproj-btn", &t)
                                                    .size(ControlSize::Default)
                                                    .icon_left(AppIcon::Folder)
                                                    .tooltip(i18n!(cx, "project.import.browse_tooltip"))
                                                    .disabled(self.is_importing || self.is_parsing)
                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                        this.browse_file(window, cx);
                                                    })),
                                            ),
                                    ),
                            )
                            .child(
                                // Password row (optional)
                                v_flex()
                                    .gap(SPACE_SM)
                                    .child(
                                        div()
                                            .text_size(ui_text_md(cx))
                                            .text_color(rgb(t.text_secondary))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(i18n!(cx, "project.import.password_label")),
                                    )
                                    .child(
                                        div()
                                            .text_size(ui_text_sm(cx))
                                            .text_color(rgb(t.text_muted))
                                            .pb(SPACE_XS)
                                            .child(i18n!(cx, "project.import.password_hint")),
                                    )
                                    .child(Input::new(password_input).cleanable(true)),
                            )
                            .when_some(self.parse_error.clone(), |el, err| {
                                el.child(
                                    div()
                                        .px(SPACE_MD)
                                        .py(SPACE_SM)
                                        .rounded(RADIUS_STD)
                                        .bg(with_alpha(t.error, 0.15))
                                        .text_color(rgb(t.error))
                                        .text_size(ui_text_sm(cx))
                                        .child(err),
                                )
                            })
                            .child(
                                // Parse Button
                                h_flex()
                                    .justify_end()
                                    .child(
                                        Button::new("parse-vproj-btn", &t)
                                            .size(ControlSize::Default)
                                            .label(if self.is_parsing {
                                                i18n!(cx, "project.import.parsing")
                                            } else {
                                                i18n!(cx, "project.import.parse_btn")
                                            })
                                            .disabled(!has_path || self.is_parsing)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.parse_file(window, cx);
                                            })),
                                    ),
                            )
                            // Parsed details section
                            .when_some(self.parsed_summary.clone(), |el, summary| {
                                el.child(
                                    v_flex()
                                        .gap(SPACE_MD)
                                        .pt(SPACE_MD)
                                        .border_t_1()
                                        .border_color(p.border_subtle)
                                        .child(
                                            // Project preview card
                                            h_flex()
                                                .p(SPACE_MD)
                                                .gap(SPACE_MD)
                                                .items_center()
                                                .bg(p.surface_card)
                                                .rounded(RADIUS_MD)
                                                .border_1()
                                                .border_color(p.border_subtle)
                                                .child(
                                                    AppIcon::from_str(&summary.icon)
                                                        .unwrap_or(AppIcon::Folder)
                                                        .size(px(24.0))
                                                        .text_color(rgb(t.get_folder_color(summary.color))),
                                                )
                                                .child(
                                                    v_flex()
                                                        .flex_1()
                                                        .child(
                                                            div()
                                                                .text_size(ui_text_md(cx))
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .text_color(rgb(t.text_primary))
                                                                .child(summary.project_name),
                                                        )
                                                        .when(!summary.project_desc.is_empty(), |d| {
                                                            d.child(
                                                                div()
                                                                    .text_size(ui_text_sm(cx))
                                                                    .text_color(rgb(t.text_muted))
                                                                    .child(summary.project_desc),
                                                            )
                                                        }),
                                                ),
                                        )
                                        .child(
                                            // Module checkboxes
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
                                                    "import-mod-sessions",
                                                    i18n!(cx, "project.export.module_sessions"),
                                                    Some(format!("({} sessions)", summary.session_count)),
                                                    self.include_sessions,
                                                    true,
                                                    AppIcon::Terminal,
                                                    |_, _, _| {},
                                                    &t,
                                                    cx,
                                                ))
                                                .when(summary.has_ai_records, |list| {
                                                    let count_desc = if summary.ai_conversation_count > 0 {
                                                        Some(format!("({} conversations)", summary.ai_conversation_count))
                                                    } else {
                                                        None
                                                    };
                                                    list.child(self.render_checkbox_item(
                                                        "import-mod-ai",
                                                        i18n!(cx, "project.export.module_ai_records"),
                                                        count_desc,
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
                                                })
                                                .when(summary.tunnel_count > 0, |list| {
                                                    list.child(self.render_checkbox_item(
                                                        "import-mod-tunnels",
                                                        i18n!(cx, "project.export.module_tunnels"),
                                                        Some(format!("({} tunnels)", summary.tunnel_count)),
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
                                                })
                                                .when(summary.service_count > 0, |list| {
                                                    list.child(self.render_checkbox_item(
                                                        "import-mod-services",
                                                        i18n!(cx, "project.export.module_services"),
                                                        Some(format!("({} services)", summary.service_count)),
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
                                                })
                                                .when(summary.snippet_count > 0 || summary.quick_command_count > 0, |list| {
                                                    let count = summary.snippet_count + summary.quick_command_count;
                                                    list.child(self.render_checkbox_item(
                                                        "import-mod-snippets",
                                                        i18n!(cx, "project.export.module_snippets"),
                                                        Some(format!("({} commands)", count)),
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
                                                })
                                                .when(summary.history_count > 0, |list| {
                                                    list.child(self.render_checkbox_item(
                                                        "import-mod-history",
                                                        i18n!(cx, "project.export.module_session_history"),
                                                        Some(format!("({} records)", summary.history_count)),
                                                        self.include_session_history,
                                                        false,
                                                        AppIcon::Refresh,
                                                        |this, _, cx| {
                                                            this.include_session_history = !this.include_session_history;
                                                            cx.notify();
                                                        },
                                                        &t,
                                                        cx,
                                                    ))
                                                }),
                                        )
                                        .child(
                                            // Duplicate strategy radio
                                            v_flex()
                                                .gap(SPACE_SM)
                                                .child(
                                                    div()
                                                        .text_size(ui_text_md(cx))
                                                        .text_color(rgb(t.text_secondary))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .child(i18n!(cx, "project.import.duplicate_strategy")),
                                                )
                                                .child(
                                                    h_flex()
                                                        .gap(SPACE_SM)
                                                        .child(self.render_strategy_button(
                                                            "strategy-rename".into(),
                                                            i18n!(cx, "project.import.strategy_rename"),
                                                            ProjectDuplicateStrategy::Rename,
                                                            &t,
                                                            cx,
                                                        ))
                                                        .child(self.render_strategy_button(
                                                            "strategy-overwrite".into(),
                                                            i18n!(cx, "project.import.strategy_overwrite"),
                                                            ProjectDuplicateStrategy::Overwrite,
                                                            &t,
                                                            cx,
                                                        ))
                                                        .child(self.render_strategy_button(
                                                            "strategy-skip".into(),
                                                            i18n!(cx, "project.import.strategy_skip"),
                                                            ProjectDuplicateStrategy::Skip,
                                                            &t,
                                                            cx,
                                                        )),
                                                ),
                                        ),
                                )
                            })
                            .when_some(self.status_message.clone(), |el, msg| {
                                let color = if self.is_status_error { rgb(t.error) } else { rgb(t.success) };
                                el.child(
                                    div()
                                        .px(SPACE_MD)
                                        .py(SPACE_SM)
                                        .rounded(RADIUS_STD)
                                        .bg(with_alpha(if self.is_status_error { t.error } else { t.success }, 0.15))
                                        .text_color(color)
                                        .text_size(ui_text_sm(cx))
                                        .child(msg),
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

        modal_content("project-import-modal", cx)
            .relative()
            .w(card_w)
            .h(card_h)
            .overflow_hidden()
            .p(SPACE_XS)
            .track_focus(&self.focus_handle)
            .key_context("ProjectImportDialog")
            .tab_cycle(&focus_group)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| this.close(cx)))
            .focus_scope_on_click(&self.focus_handle)
            .child(modal_header(
                i18n!(cx, "project.import.title"),
                Some(i18n!(cx, "project.import.desc")),
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
                        Button::new("import-cancel-btn", &t)
                            .size(ControlSize::Default)
                            .label(i18n!(cx, "common.action.cancel"))
                            .focus_handle(&self.cancel_focus)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(
                        Button::new("import-execute-btn", &t)
                            .primary()
                            .size(ControlSize::Default)
                            .label(if self.is_importing {
                                i18n!(cx, "project.import.importing")
                            } else {
                                i18n!(cx, "project.import.import_btn")
                            })
                            .loading(self.is_importing)
                            .disabled(self.parsed_summary.is_none() || self.is_importing)
                            .focus_handle(&self.confirm_focus)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.execute_import(window, cx);
                            })),
                    ),
            )
    }
}
