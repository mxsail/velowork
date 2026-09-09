//! Import external sessions dialog overlay.

use std::path::PathBuf;

use gpui::prelude::FluentBuilder;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::button::Button;
use velowork_ui::design::appearance::ControlSize;
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::h_flex;
use velowork_ui::icon::AppIcon;
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::select::{SelectOption, SelectState};
use velowork_ui::simple_input::{SimpleInput, SimpleInputState};
use velowork_ui::theme::{theme, with_alpha};
use velowork_ui::tokens::{
    RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS,
    ui_text_md, ui_text_sm,
};
use velowork_workspace::importer::{
    DuplicateStrategy, ImportContext, ImportResult, ImporterRegistry,
    import_sessions_into_store,
};
use velowork_workspace::state::{WindowId, Workspace};
use velowork_workspace::stores::GlobalSessionStore;

use crate::keybindings::Cancel;
use crate::views::components::modal_content;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportFormatOption {
    Auto,
    Xshell,
    MobaXterm,
    WindTerm,
    FinalShell,
}

impl ImportFormatOption {
    pub fn all() -> &'static [ImportFormatOption] {
        &[
            ImportFormatOption::Auto,
            ImportFormatOption::Xshell,
            ImportFormatOption::MobaXterm,
            ImportFormatOption::WindTerm,
            ImportFormatOption::FinalShell,
        ]
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Xshell => "xshell",
            Self::MobaXterm => "mobaxterm",
            Self::WindTerm => "windterm",
            Self::FinalShell => "finalshell",
        }
    }

    pub fn label(&self, cx: &App) -> String {
        match self {
            Self::Auto => i18n!(cx, "import_session.format_auto"),
            Self::Xshell => i18n!(cx, "import_session.format_xshell"),
            Self::MobaXterm => i18n!(cx, "import_session.format_mobaxterm"),
            Self::WindTerm => i18n!(cx, "import_session.format_windterm"),
            Self::FinalShell => i18n!(cx, "import_session.format_finalshell"),
        }
    }

    pub fn icon(&self) -> AppIcon {
        match self {
            Self::Auto => AppIcon::Refresh,
            Self::Xshell => AppIcon::Terminal,
            Self::MobaXterm => AppIcon::Server,
            Self::WindTerm => AppIcon::Code,
            Self::FinalShell => AppIcon::Folder,
        }
    }
}

pub struct ImportSessionsDialog {
    _workspace: Entity<Workspace>,
    focus_manager: Entity<velowork_workspace::focus::FocusManager>,
    _window_id: WindowId,
    focus_handle: FocusHandle,
    cancel_focus: FocusHandle,
    confirm_focus: FocusHandle,
    selected_format: ImportFormatOption,
    format_select: Entity<SelectState<ImportFormatOption>>,
    path_input: Entity<SimpleInputState>,
    password_input: Entity<SimpleInputState>,
    duplicate_strategy: DuplicateStrategy,
    strategy_select: Entity<SelectState<DuplicateStrategy>>,
    registry: ImporterRegistry,
    status_message: Option<String>,
    is_error: bool,
    is_importing: bool,
    last_result: Option<ImportResult>,
    scroll_handle: ScrollHandle,
    initial_focus_done: bool,
}

pub enum ImportSessionsDialogEvent {
    Close,
}

impl EventEmitter<ImportSessionsDialogEvent> for ImportSessionsDialog {}

impl ImportSessionsDialog {
    pub fn new(
        _workspace: Entity<Workspace>,
        window_id: WindowId,
        focus_manager: Entity<velowork_workspace::focus::FocusManager>,
        cx: &mut Context<Self>,
    ) -> Self {
        let path_ph = i18n!(cx, "import_session.path_placeholder");
        let path_input = cx.new(|cx| SimpleInputState::new(cx).placeholder(path_ph));

        let pwd_ph = i18n!(cx, "import_session.password_placeholder");
        let password_input = cx.new(|cx| SimpleInputState::new(cx).placeholder(pwd_ph).password());

        let format_options: Vec<SelectOption<ImportFormatOption>> = ImportFormatOption::all()
            .iter()
            .map(|opt| {
                SelectOption::new(opt.clone(), opt.label(cx))
                    .icon(opt.icon())
            })
            .collect();

        let format_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(format_options)
                .selected(Some(ImportFormatOption::Auto))
        });

        cx.subscribe(&format_select, |this, _, event: &velowork_ui::select::SelectEvent<ImportFormatOption>, cx| {
            if let velowork_ui::select::SelectEvent::Change(Some(val)) = event {
                this.selected_format = val.clone();
                this.status_message = None;
                this.is_error = false;
                cx.notify();
            }
        })
        .detach();

        let strategy_options: Vec<SelectOption<DuplicateStrategy>> = DuplicateStrategy::all()
            .iter()
            .map(|opt| {
                let label = match opt {
                    DuplicateStrategy::Overwrite => i18n!(cx, "import_session.duplicate_overwrite"),
                    DuplicateStrategy::Rename => i18n!(cx, "import_session.duplicate_rename"),
                    DuplicateStrategy::Skip => i18n!(cx, "import_session.duplicate_skip"),
                };
                let icon = match opt {
                    DuplicateStrategy::Overwrite => AppIcon::Refresh,
                    DuplicateStrategy::Rename => AppIcon::Edit,
                    DuplicateStrategy::Skip => AppIcon::Close,
                };
                SelectOption::new(*opt, label).icon(icon)
            })
            .collect();

        let strategy_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(strategy_options)
                .selected(Some(DuplicateStrategy::Overwrite))
        });

        cx.subscribe(&strategy_select, |this, _, event: &velowork_ui::select::SelectEvent<DuplicateStrategy>, cx| {
            if let velowork_ui::select::SelectEvent::Change(Some(val)) = event {
                this.duplicate_strategy = *val;
                this.status_message = None;
                this.is_error = false;
                cx.notify();
            }
        })
        .detach();

        Self {
            _workspace,
            focus_manager,
            _window_id: window_id,
            focus_handle: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            selected_format: ImportFormatOption::Auto,
            format_select,
            path_input,
            password_input,
            duplicate_strategy: DuplicateStrategy::Overwrite,
            strategy_select,
            registry: ImporterRegistry::default_registry(),
            status_message: None,
            is_error: false,
            is_importing: false,
            last_result: None,
            scroll_handle: ScrollHandle::new(),
            initial_focus_done: false,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        if !self.is_importing {
            cx.emit(ImportSessionsDialogEvent::Close);
        }
    }

    fn open_file_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_importing {
            return;
        }

        let is_dir_only = self.selected_format == ImportFormatOption::FinalShell;
        let prompt_title = i18n!(cx, "import_session.browse_tooltip");
        let prompt_opts = PathPromptOptions {
            files: !is_dir_only,
            directories: is_dir_only,
            multiple: false,
            prompt: Some(prompt_title.into()),
        };

        let paths_future = cx.prompt_for_paths(prompt_opts);
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(selected))) = paths_future.await
                && let Some(path) = selected.first().cloned()
            {
                let path_str = path.to_string_lossy().to_string();
                let _ = this.update(cx, |this, cx| {
                    this.path_input.update(cx, |st, cx| {
                        st.set_value(path_str, cx);
                    });
                    this.status_message = None;
                    this.is_error = false;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn execute_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_importing {
            return;
        }

        self.status_message = None;
        self.is_error = false;
        self.last_result = None;

        let raw_path = self.path_input.read(cx).value().trim().to_string();

        if raw_path.is_empty() {
            self.status_message = Some(i18n!(cx, "import_session.path_empty"));
            self.is_error = true;
            cx.notify();
            return;
        }

        let path = PathBuf::from(&raw_path);
        if !path.exists() {
            self.status_message = Some(format!("Path does not exist: {}", path.display()));
            self.is_error = true;
            cx.notify();
            return;
        }

        let master_password = {
            let pwd = self.password_input.read(cx).value().trim().to_string();
            if pwd.is_empty() { None } else { Some(pwd) }
        };

        let format_id = self.selected_format.id().to_string();
        let registry = self.registry.clone();

        self.is_importing = true;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let parse_res = cx.background_executor().spawn(async move {
                let mut ctx = ImportContext::new(&path);
                if let Some(pwd) = master_password {
                    ctx = ctx.with_master_password(pwd);
                }
                registry.parse(Some(&format_id), &ctx)
            }).await;

            let _ = this.update(cx, |this, cx| {
                this.is_importing = false;
                match parse_res {
                    Ok(imported_sessions) => {
                        if imported_sessions.is_empty() {
                            this.status_message = Some("No valid SSH sessions found in the provided source.".to_string());
                            this.is_error = true;
                        } else if cx.has_global::<GlobalSessionStore>() {
                            let target_project_id = this.focus_manager.read(cx).active_project_id().cloned();
                            let session_store = cx.global::<GlobalSessionStore>().0.clone();
                            let duplicate_strategy = this.duplicate_strategy;
                            let result = session_store.update(cx, |store, cx| {
                                import_sessions_into_store(
                                    store,
                                    target_project_id.as_deref(),
                                    imported_sessions,
                                    duplicate_strategy,
                                    cx,
                                )
                            });

                            let summary_tmpl = i18n!(cx, "import_session.success_summary");
                            let summary = summary_tmpl
                                .replace("{imported}", &result.imported_sessions.to_string())
                                .replace("{overwritten}", &result.overwritten_sessions.to_string())
                                .replace("{renamed}", &result.renamed_sessions.to_string())
                                .replace("{skipped}", &result.skipped_sessions.to_string())
                                .replace("{folders}", &result.created_folders.to_string());

                            this.status_message = Some(summary);
                            this.is_error = false;
                            this.last_result = Some(result);
                        } else {
                            this.status_message = Some("Internal error: SessionStore is not available.".to_string());
                            this.is_error = true;
                        }
                    }
                    Err(e) => {
                        let err_details = format!("{:#}", e);
                        let msg = i18n!(cx, "import_session.failed");
                        this.status_message = Some(msg.replace("{error}", &err_details));
                        this.is_error = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for ImportSessionsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let focus_handle = self.focus_handle.clone();

        if !self.initial_focus_done {
            self.initial_focus_done = true;
            self.format_select.update(cx, |sel, cx| sel.focus(window, cx));
        }

        let show_password_field = matches!(
            self.selected_format,
            ImportFormatOption::Auto | ImportFormatOption::WindTerm
        );

        let focus_group = FocusGroup::new();
        focus_group.add(self.format_select.read(cx).focus_handle().clone());
        focus_group.add(self.path_input.read(cx).focus_handle(cx));
        if show_password_field {
            focus_group.add(self.password_input.read(cx).focus_handle(cx));
        }
        focus_group.add(self.strategy_select.read(cx).focus_handle().clone());
        focus_group.add(self.cancel_focus.clone());
        focus_group.add(self.confirm_focus.clone());

        let win_size = window.viewport_size();
        let card_w = px(520.0).min(win_size.width - px(48.0));
        let card_h = px(600.0).min(win_size.height - px(80.0)).max(px(240.0));

        let header = div()
            .id("import-sessions-header")
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
                        AppIcon::FolderInput
                            .size(px(18.0))
                            .text_color(rgb(t.border_active)),
                    )
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text_primary))
                            .child(i18n!(cx, "import_session.title")),
                    ),
            )
            .child(
                div()
                    .id("import-sessions-close")
                    .when(!self.is_importing, |d| d.cursor_pointer())
                    .w(px(28.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(RADIUS_STD)
                    .when(!self.is_importing, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
                    .text_color(if self.is_importing { rgb(t.text_muted) } else { rgb(t.text_secondary) })
                    .child(AppIcon::Close.size(px(16.0)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.close(cx)),
                    ),
            );

        let body_content = div()
            .id(ElementId::Name("import-sessions-scroll".into()))
            .relative()
            .p(SPACE_XL)
            .flex()
            .flex_col()
            .gap(SPACE_LG)
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .min_h(px(0.0))
            .w_full()
                            // 1. Format Selection (Vertical Stack)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(SPACE_XS)
                                    .child(
                                        div()
                                            .text_size(ui_text_md(cx))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(t.text_secondary))
                                            .child(i18n!(cx, "import_session.format_label")),
                                    )
                                    .child(self.format_select.clone()),
                            )
                            // 2. File Path (Vertical Stack with Browse Button)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(SPACE_XS)
                                    .child(
                                        div()
                                            .text_size(ui_text_md(cx))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(t.text_secondary))
                                            .child(i18n!(cx, "import_session.path_label")),
                                    )
                                    .child(
                                        h_flex()
                                            .gap(SPACE_SM)
                                            .items_center()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .child(SimpleInput::new(&self.path_input)),
                                            )
                                            .child(
                                                Button::new("import-file-browse-btn", &t)
                                                    .size(ControlSize::Default)
                                                    .icon_left(AppIcon::Folder)
                                                    .tooltip(i18n!(cx, "import_session.browse_tooltip"))
                                                    .disabled(self.is_importing)
                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                        this.open_file_picker(window, cx);
                                                    })),
                                            ),
                                    ),
                            )
                            // 3. Password (optional, for WindTerm)
                            .when(show_password_field, |d| {
                                d.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(SPACE_XS)
                                        .child(
                                            div()
                                                .text_size(ui_text_md(cx))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(rgb(t.text_secondary))
                                                .child(i18n!(cx, "import_session.password_label")),
                                        )
                                        .child(SimpleInput::new(&self.password_input)),
                                )
                            })
                            // 4. Duplicate Name Strategy Selection (Vertical Stack)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(SPACE_XS)
                                    .child(
                                        div()
                                            .text_size(ui_text_md(cx))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(t.text_secondary))
                                            .child(i18n!(cx, "import_session.duplicate_strategy_label")),
                                    )
                                    .child(self.strategy_select.clone()),
                            )
                            // 5. Status / Result Message (Clear Contrast Banner)
                            .when_some(self.status_message.as_ref(), |d, msg| {
                                let is_err = self.is_error;
                                d.child(
                                    div()
                                        .px(SPACE_MD)
                                        .py(SPACE_SM)
                                        .rounded(RADIUS_STD)
                                        .border_1()
                                        .when(is_err, |s| {
                                            s.bg(with_alpha(t.error, 0.12))
                                                .border_color(with_alpha(t.error, 0.35))
                                                .text_color(rgb(t.error))
                                        })
                                        .when(!is_err, |s| {
                                            s.bg(with_alpha(t.success, 0.12))
                                                .border_color(with_alpha(t.success, 0.35))
                                                .text_color(rgb(t.success))
                                        })
                                        .text_size(ui_text_sm(cx))
                                        .child(msg.clone())
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

        modal_content("import-sessions-modal", cx)
            .relative()
            .w(card_w)
            .h(card_h)
            .overflow_hidden()
            .track_focus(&focus_handle)
            .key_context("ImportSessionsDialog")
            .tab_cycle(&focus_group)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.close(cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .focus_scope_on_click(&self.focus_handle)
                    .child(header)
                    .child(body_container)
                    .child(
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
                                    .label(i18n!(cx, "common.cancel"))
                                    .disabled(self.is_importing)
                                    .focus_handle(&self.cancel_focus)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.close(cx);
                                    })),
                            )
                            .child(
                                Button::new("import-confirm-btn", &t)
                                    .primary()
                                    .loading(self.is_importing)
                                    .disabled(self.is_importing)
                                    .label(if self.is_importing {
                                        i18n!(cx, "import_session.btn_importing")
                                    } else {
                                        i18n!(cx, "import_session.btn_import")
                                    })
                                    .focus_handle(&self.confirm_focus)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.execute_import(window, cx);
                                    })),
                            ),
                    )
    }
}

impl_focusable!(ImportSessionsDialog);
