use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use crate::keybindings::Cancel;
use crate::views::components::modal_content;
use crate::views::panels::ai_assistant_panel::ChatAttachment;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::scrollable::{Scrollbar, ScrollbarShow};
use velowork_ui::tokens::*;
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex, ProgressRing};

pub enum AttachmentPreviewDialogEvent {
    Close,
}

pub struct AttachmentPreviewDialog {
    attachment: ChatAttachment,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    lines: Arc<Vec<SharedString>>,
    gutter_width: Pixels,
    is_truncated: bool,
    is_binary: bool,
    _watch_task: Option<Task<()>>,
}

impl EventEmitter<AttachmentPreviewDialogEvent> for AttachmentPreviewDialog {}

impl AttachmentPreviewDialog {
    pub fn new(mut attachment: ChatAttachment, cx: &mut Context<Self>) -> Self {
        const MAX_READ_BYTES: u64 = 2 * 1024 * 1024; // 2MB
        let is_image = attachment.is_image;
        let file_exists = attachment.path.exists();
        let mut is_truncated = false;
        let mut is_binary = false;
        let mut lines: Vec<SharedString> = Vec::new();

        if !is_image {
            if let Some(ref text) = attachment.text_content {
                lines = text.lines().map(SharedString::from).collect();
            } else if file_exists
                && let Ok(file) = std::fs::File::open(&attachment.path)
            {
                let metadata = file.metadata().ok();
                let file_len = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                if file_len > MAX_READ_BYTES {
                    is_truncated = true;
                }
                let mut buf = Vec::new();
                if file.take(MAX_READ_BYTES).read_to_end(&mut buf).is_ok() {
                    let check_len = buf.len().min(1024);
                    if buf[..check_len].contains(&0) {
                        is_binary = true;
                    } else {
                        let text = String::from_utf8_lossy(&buf);
                        lines = text.lines().map(SharedString::from).collect();
                        attachment.text_content = Some(text.into_owned());
                    }
                }
            }
        }

        let line_count = lines.len();
        let gutter_width = if line_count < 1000 {
            px(40.0)
        } else if line_count < 10000 {
            px(50.0)
        } else {
            px(60.0)
        };

        let mut watch_task = None;
        if is_image && !attachment.is_ready {
            let path = attachment.path.clone();
            watch_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
                for _ in 0..60 {
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                    if path.exists() {
                        let _ = this.update(cx, |this, cx| {
                            this.attachment.is_ready = true;
                            cx.notify();
                        });
                        break;
                    }
                }
            }));
        }

        Self {
            attachment,
            focus_handle: cx.focus_handle(),
            scroll_handle: UniformListScrollHandle::new(),
            lines: Arc::new(lines),
            gutter_width,
            is_truncated,
            is_binary,
            _watch_task: watch_task,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(AttachmentPreviewDialogEvent::Close);
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
        let p = SemanticPalette::from_context(cx);
        let focus_handle = self.focus_handle.clone();
        let att = &self.attachment;
        let is_image = att.is_image;
        let file_exists = att.path.exists();
        let title = att.name.clone();

        let meta_label = if is_image {
            if file_exists {
                std::fs::metadata(&att.path)
                    .ok()
                    .map(|m| format_file_size(m.len()))
            } else {
                None
            }
        } else if self.is_binary {
            Some(i18n!(cx, "ai.file_binary_label"))
        } else if self.lines.is_empty() {
            Some(i18n!(cx, "ai.file_empty"))
        } else {
            Some(i18n!(cx, "ai.file_line_count").replace("{count}", &self.lines.len().to_string()))
        };

        let close_tip = i18n!(cx, "common.action.close");

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
            // 36px 极简单行 Header：左侧文件名带图标，右侧独立关闭按钮
            .child(
                h_flex()
                    .h(px(36.0))
                    .w_full()
                    .flex_shrink_0()
                    .px(SPACE_MD)
                    .items_center()
                    .justify_between()
                    .bg(p.surface_card)
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .child(
                        h_flex()
                            .items_center()
                            .gap(SPACE_SM)
                            .flex_1()
                            .min_w_0()
                            .child(if is_image {
                                AppIcon::Image.size(px(14.0)).text_color(p.text_muted)
                            } else {
                                AppIcon::File.size(px(14.0)).text_color(p.text_muted)
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(ui_text_sm(cx))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text_primary)
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .id("att-preview-close-btn")
                            .flex_shrink_0()
                            .w(px(22.0))
                            .h(px(22.0))
                            .rounded(RADIUS_SM)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(p.surface_hover))
                            .tooltip(move |_, cx| {
                                cx.new(|_| Tooltip::new(close_tip.clone())).into()
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
                            .child(AppIcon::Close.size(px(12.0)).text_color(p.text_muted)),
                    ),
            )
            // 中间内容区：4px 精细边距，充满视口，内置右下角悬浮元信息与系统打开胶囊
            .child(
                div()
                    .w_full()
                    .p(SPACE_XS)
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(480.0))
                            .rounded(RADIUS_STD)
                            .bg(p.surface_card)
                            .border_1()
                            .border_color(p.border_subtle)
                            .overflow_hidden()
                            .child(if is_image {
                                if !att.is_ready {
                                    v_flex()
                                        .size_full()
                                        .items_center()
                                        .justify_center()
                                        .gap(SPACE_MD)
                                        .child(ProgressRing::new(0.5).size(px(24.0)).stroke_width(px(3.0)))
                                        .child(
                                            div()
                                                .text_size(ui_text_sm(cx))
                                                .text_color(p.text_muted)
                                                .child(i18n!(cx, "ai.preparing_preview")),
                                        )
                                        .into_any_element()
                                } else {
                                    v_flex()
                                        .size_full()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            img(att.path.clone())
                                                .size_full()
                                                .object_fit(ObjectFit::Contain),
                                        )
                                        .into_any_element()
                                }
                            } else if self.is_binary {
                                v_flex()
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .gap(SPACE_MD)
                                    .child(AppIcon::File.size(px(32.0)).text_color(p.text_muted))
                                    .child(
                                        div()
                                            .text_size(ui_text_sm(cx))
                                            .text_color(p.text_muted)
                                            .child(i18n!(cx, "ai.file_binary_not_supported")),
                                    )
                                    .into_any_element()
                            } else if self.lines.is_empty() {
                                v_flex()
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .text_size(ui_text_sm(cx))
                                            .text_color(p.text_muted)
                                            .child(i18n!(cx, "ai.file_empty")),
                                    )
                                    .into_any_element()
                            } else {
                                let lines = self.lines.clone();
                                let gutter_w = self.gutter_width;
                                let list = uniform_list(
                                    "att-preview-code-list",
                                    lines.len(),
                                    move |range, _window, cx| {
                                        let p = SemanticPalette::from_context(cx);
                                        let mono_font = mono_font_family(cx);
                                        let font_size = ui_text_md(cx);
                                        let gutter_font_size = ui_text_xs(cx);
                                        range
                                            .map(|i| {
                                                let line_num = (i + 1).to_string();
                                                let line_str = lines.get(i).cloned().unwrap_or_default();
                                                h_flex()
                                                    .w_full()
                                                    .h(px(24.0))
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .flex_shrink_0()
                                                            .w(gutter_w)
                                                            .h_full()
                                                            .flex()
                                                            .items_center()
                                                            .justify_end()
                                                            .pr(SPACE_SM)
                                                            .text_size(gutter_font_size)
                                                            .text_color(p.text_muted.opacity(0.6))
                                                            .child(line_num),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_shrink_0()
                                                            .w(px(1.0))
                                                            .h_full()
                                                            .bg(p.border_subtle),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .h_full()
                                                            .flex()
                                                            .items_center()
                                                            .pl(SPACE_SM)
                                                            .font_family(mono_font.clone())
                                                            .text_size(font_size)
                                                            .text_color(p.text_primary)
                                                            .whitespace_nowrap()
                                                            .overflow_hidden()
                                                            .child(line_str),
                                                    )
                                                    .into_any_element()
                                            })
                                            .collect::<Vec<_>>()
                                    },
                                )
                                .track_scroll(&self.scroll_handle)
                                .size_full();

                                v_flex()
                                    .size_full()
                                    .children(self.is_truncated.then(|| {
                                        h_flex()
                                            .w_full()
                                            .px(SPACE_MD)
                                            .py(SPACE_XS)
                                            .bg(p.status_warning.opacity(0.12))
                                            .border_b_1()
                                            .border_color(p.status_warning.opacity(0.3))
                                            .items_center()
                                            .gap(SPACE_SM)
                                            .child(AppIcon::Info.size(px(14.0)).text_color(p.status_warning))
                                            .child(
                                                div()
                                                    .text_size(ui_text_xs(cx))
                                                    .text_color(p.text_primary)
                                                    .child(i18n!(cx, "ai.file_size_exceeded_tip")),
                                            )
                                    }))
                                    .child(
                                        div()
                                            .relative()
                                            .flex_1()
                                            .min_h_0()
                                            .w_full()
                                            .overflow_hidden()
                                            .child(list)
                                            .child(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .right_0()
                                                    .bottom_0()
                                                    .w(px(8.0))
                                                    .child(
                                                        Scrollbar::vertical(&self.scroll_handle)
                                                            .id("att-preview-scrollbar")
                                                            .scrollbar_show(ScrollbarShow::Hover),
                                                    ),
                                            ),
                                    )
                                    .into_any_element()
                            })
                            // 经典桌面右下角区域：纯文字行数/大小（无背景）+ 独立的“在系统应用中打开”微按钮（4px 圆角）
                            .children((meta_label.is_some() || file_exists).then(|| {
                                h_flex()
                                    .occlude()
                                    .absolute()
                                    .bottom(px(8.0))
                                    .right(px(16.0))
                                    .items_center()
                                    .gap(SPACE_SM)
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .children(meta_label.map(|label| {
                                        div()
                                            .text_size(ui_text_xs(cx))
                                            .text_color(p.text_muted)
                                            .child(label)
                                    }))
                                    .children(file_exists.then(|| {
                                        h_flex()
                                            .id("att-preview-open-system-btn")
                                            .h(px(22.0))
                                            .px(px(6.0))
                                            .items_center()
                                            .gap(px(4.0))
                                            .rounded(RADIUS_SM)
                                            .bg(p.surface_card.opacity(0.85))
                                            .border_1()
                                            .border_color(p.border_subtle)
                                            .cursor_pointer()
                                            .text_color(p.text_muted)
                                            .hover(|s| s.bg(p.surface_hover).text_color(p.text_primary))
                                            .child(
                                                div()
                                                    .text_size(ui_text_xs(cx))
                                                    .child(i18n!(cx, "ai.open_in_system")),
                                            )
                                            .child(AppIcon::ExternalLink.size(px(11.0)))
                                            .on_click(cx.listener(|this, _, _, _| {
                                                this.open_in_system();
                                            }))
                                    }))
                            })),
                    ),
            )
    }
}
