use crate::keybindings::Cancel;
use velowork_ui::button::Button;
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use crate::theme::theme;
use crate::views::components::{modal_content, modal_header};
use velowork_ui::tokens::{RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_XL, ui_text, ui_text_md};
use gpui::*;
use velowork_ui::{h_flex, v_flex};
use velowork_ui::scrollable::ScrollableElement;
use velowork_i18n::i18n;

pub enum LogSavedDialogEvent {
    Close,
    OpenFile { path: std::path::PathBuf },
    OpenFolder { path: std::path::PathBuf },
}

pub struct LogSavedDialog {
    path: std::path::PathBuf,
    focus_handle: FocusHandle,
    close_focus: FocusHandle,
    file_focus: FocusHandle,
    folder_focus: FocusHandle,
}

impl EventEmitter<LogSavedDialogEvent> for LogSavedDialog {}

impl LogSavedDialog {
    pub fn new(_terminal_id: String, path: std::path::PathBuf, cx: &mut Context<Self>) -> Self {
        Self {
            path,
            focus_handle: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            file_focus: cx.focus_handle(),
            folder_focus: cx.focus_handle(),
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(LogSavedDialogEvent::Close);
    }

    fn open_file(&self, cx: &mut Context<Self>) {
        cx.emit(LogSavedDialogEvent::OpenFile { path: self.path.clone() });
        cx.emit(LogSavedDialogEvent::Close);
    }

    fn open_folder(&self, cx: &mut Context<Self>) {
        cx.emit(LogSavedDialogEvent::OpenFolder { path: self.path.clone() });
        cx.emit(LogSavedDialogEvent::Close);
    }
}

impl Render for LogSavedDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let focus_handle = self.focus_handle.clone();

        let path_str = self.path.to_string_lossy().to_string();

        let focus_group = FocusGroup::new();
        focus_group.add(self.close_focus.clone());
        focus_group.add(self.file_focus.clone());
        focus_group.add(self.folder_focus.clone());

        modal_content("log-saved-dialog-modal", cx)
            .w(px(400.0))
            .track_focus(&focus_handle)
            .key_context("LogSavedDialog")
            .tab_cycle(&focus_group)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.close(cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(modal_header(
                        i18n!(cx, "dialog.log_saved.title"),
                        None::<String>,
                        &t,
                        cx,
                        cx.listener(|this, _, _, cx| this.close(cx)),
                    ))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .px(px(24.0))
                            .py(px(20.0))
                            .gap(SPACE_XL)
                            .child(
                                div()
                                    .text_color(rgb(t.text_secondary))
                                    .text_size(ui_text(13.0, cx))
                                    .child(i18n!(cx, "dialog.log_saved.saved_to"))
                            )
                            .child(
                                div()
                                    .bg(p.surface_card)
                                    .border_1()
                                    .border_color(p.border_subtle)
                                    .p(px(10.0))
                                    .rounded(RADIUS_STD)
                                    .text_size(ui_text_md(cx))
                                    .text_color(p.text_primary)
                                    .child(path_str)
                            )
                    )
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
                                Button::new("close-btn", &t)
                                    .label(i18n!(cx, "common.close"))
                                    .focus_handle(&self.close_focus)
                                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                            )
                            .child(
                                Button::new("open-file-btn", &t)
                                    .label(i18n!(cx, "dialog.log_saved.open_file"))
                                    .focus_handle(&self.file_focus)
                                    .on_click(cx.listener(|this, _, _, cx| this.open_file(cx))),
                            )
                            .child(
                                Button::new("open-folder-btn", &t)
                                    .primary()
                                    .label(i18n!(cx, "dialog.log_saved.open_folder"))
                                    .focus_handle(&self.folder_focus)
                                    .on_click(cx.listener(|this, _, _, cx| this.open_folder(cx))),
                            ),
                    )
    }
}
