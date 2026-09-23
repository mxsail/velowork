use crate::keybindings::Cancel;
use crate::theme::theme;
use crate::views::components::{modal_content, modal_header};
use crate::views::panels::ai_assistant_panel::ChatAttachment;
use gpui::prelude::FluentBuilder;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::button::Button;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::scrollable::ScrollableElement;
use velowork_ui::tokens::*;
use velowork_ui::{h_flex, v_flex, ProgressRing};

pub enum AttachmentPreviewDialogEvent {
    Close,
}

pub struct AttachmentPreviewDialog {
    attachment: ChatAttachment,
    focus_handle: FocusHandle,
    copied: bool,
    _copy_timer: Option<Task<()>>,
}

impl EventEmitter<AttachmentPreviewDialogEvent> for AttachmentPreviewDialog {}

impl AttachmentPreviewDialog {
    pub fn new(attachment: ChatAttachment, cx: &mut Context<Self>) -> Self {
        Self {
            attachment,
            focus_handle: cx.focus_handle(),
            copied: false,
            _copy_timer: None,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(AttachmentPreviewDialogEvent::Close);
    }

    fn copy_content(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = true;
        cx.notify();

        self._copy_timer = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                this._copy_timer = None;
                cx.notify();
            });
        }));
    }

    fn open_in_system(&self) {
        if self.attachment.path.exists() {
            velowork_core::process::open_file(&self.attachment.path);
        }
    }
}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

impl Render for AttachmentPreviewDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let focus_handle = self.focus_handle.clone();
        let att = &self.attachment;
        let is_image = att.is_image;
        let file_exists = att.path.exists();

        let title = att.name.clone();
        let subtitle = if is_image {
            if file_exists {
                std::fs::metadata(&att.path)
                    .ok()
                    .map(|m| format_file_size(m.len()))
            } else {
                None
            }
        } else {
            att.text_content
                .as_ref()
                .map(|c| format!("{} 字符", c.chars().count()))
        };

        modal_content("attachment-preview-dialog-modal", cx)
            .w(px(720.0))
            .max_w(px(800.0))
            .track_focus(&focus_handle)
            .key_context("AttachmentPreviewDialog")
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.close(cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(modal_header(
                title,
                subtitle,
                &t,
                cx,
                cx.listener(|this, _, _, cx| this.close(cx)),
            ))
            .child(
                v_flex()
                    .w_full()
                    .max_h(px(520.0))
                    .min_h_0()
                    .overflow_hidden()
                    .p(SPACE_LG)
                    .child(if is_image {
                        if !att.is_ready && !file_exists {
                            v_flex()
                                .size_full()
                                .min_h(px(200.0))
                                .items_center()
                                .justify_center()
                                .gap(SPACE_MD)
                                .child(ProgressRing::new(0.5).size(px(24.0)).stroke_width(px(3.0)))
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(p.text_muted)
                                        .child("正在准备图片预览…"),
                                )
                                .into_any_element()
                        } else {
                            v_flex()
                                .w_full()
                                .items_center()
                                .justify_center()
                                .rounded(RADIUS_STD)
                                .bg(p.surface_card)
                                .border_1()
                                .border_color(p.border_subtle)
                                .overflow_hidden()
                                .child(
                                    img(att.path.clone())
                                        .max_w_full()
                                        .max_h(px(460.0))
                                        .object_fit(ObjectFit::Contain),
                                )
                                .into_any_element()
                        }
                    } else {
                        // 文本 / 代码预览
                        let raw_content = att.text_content.clone().unwrap_or_default();
                        let char_count = raw_content.chars().count();
                        const MAX_PREVIEW_CHARS: usize = 10_000;
                        let is_truncated = char_count > MAX_PREVIEW_CHARS;
                        let display_content = if is_truncated {
                            raw_content.chars().take(MAX_PREVIEW_CHARS).collect::<String>()
                        } else {
                            raw_content
                        };

                        v_flex()
                            .w_full()
                            .max_h(px(460.0))
                            .min_h_0()
                            .rounded(RADIUS_STD)
                            .bg(p.surface_card)
                            .border_1()
                            .border_color(p.border_subtle)
                            .overflow_hidden()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scrollbar()
                                    .p(SPACE_MD)
                                    .child(
                                        div()
                                            .font_family(mono_font_family(cx))
                                            .text_size(ui_text_xs(cx))
                                            .text_color(p.text_primary)
                                            .line_height(relative(1.5))
                                            .child(display_content),
                                    ),
                            )
                            .when(is_truncated, |d| {
                                d.child(
                                    div()
                                        .px(SPACE_MD)
                                        .py(SPACE_XS)
                                        .bg(p.surface_hover)
                                        .border_t_1()
                                        .border_color(p.border_subtle)
                                        .text_size(ui_text_xs(cx))
                                        .text_color(p.text_muted)
                                        .child(format!("文本较长，当前仅预览前 {} 字符", MAX_PREVIEW_CHARS)),
                                )
                            })
                            .into_any_element()
                    }),
            )
            .child(
                h_flex()
                    .h(px(48.0))
                    .flex_shrink_0()
                    .px(SPACE_LG)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .items_center()
                            .gap(SPACE_SM)
                            .children(if file_exists {
                                Some(
                                    Button::new("open-system-btn", &t)
                                        .icon_left(AppIcon::ExternalLink)
                                        .label(i18n!(cx, "ai.open_in_system"))
                                        .on_click(cx.listener(|this, _, _, _| {
                                            this.open_in_system();
                                        })),
                                )
                            } else {
                                None
                            })
                            .children(if !is_image || !file_exists {
                                let content_to_copy = att.text_content.clone().unwrap_or_default();
                                if !content_to_copy.is_empty() {
                                    let is_copied = self.copied;
                                    Some(
                                        Button::new("copy-content-btn", &t)
                                            .icon_left(if is_copied {
                                                AppIcon::Check
                                            } else {
                                                AppIcon::Copy
                                            })
                                            .label(if is_copied {
                                                i18n!(cx, "ai.copied")
                                            } else {
                                                i18n!(cx, "ai.copy_content")
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.copy_content(content_to_copy.clone(), cx);
                                            })),
                                    )
                                } else {
                                    None
                                }
                            } else {
                                None
                            }),
                    )
                    .child(
                        Button::new("close-preview-btn", &t)
                            .label(i18n!(cx, "common.action.close"))
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
            )
    }
}
