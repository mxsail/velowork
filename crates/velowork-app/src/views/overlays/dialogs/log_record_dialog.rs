//! Log recording dialog modal.

use crate::keybindings::Cancel;
use velowork_ui::icon::AppIcon;
use crate::theme::theme;
use crate::views::components::{
    button, labeled_input, modal_content,
    modal_header,
};
use velowork_ui::button::button_primary;
use velowork_ui::input::InputState;
use velowork_ui::radio::{RadioGroup, RadioMode, RadioOption};
use velowork_ui::select::{Select, SelectOption, SelectState};
use velowork_ui::tokens::{SPACE_LG, SPACE_MD, SPACE_XL};
use gpui::prelude::*;
use gpui::*;
use velowork_ui::{h_flex, v_flex};
use velowork_ui::scrollable::ScrollableElement;
use velowork_i18n::i18n;

pub enum LogRecordDialogEvent {
    Close,
    StartRecording {
        terminal_id: String,
        filename: String,
        append_mode: bool,
        auto_save_interval: usize,
    },
}

use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};

pub struct LogRecordDialog {
    terminal_id: String,
    focus_handle: FocusHandle,
    mode_focus: FocusHandle,
    cancel_focus: FocusHandle,
    confirm_focus: FocusHandle,
    filename_input: Entity<InputState>,
    initial_focus_done: bool,
    append_mode: bool,
    interval_select: Entity<SelectState<usize>>,
}

impl EventEmitter<LogRecordDialogEvent> for LogRecordDialog {}

impl LogRecordDialog {
    pub fn new(terminal_id: String, cx: &mut Context<Self>) -> Self {
        let today = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
        let default_filename = format!("terminal-log-{}.log", today);

        let filename_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("terminal-log.log")
                .default_value(&default_filename)
        });

        let interval_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new(0, i18n!(cx, "dialog.log.interval_realtime")),
                    SelectOption::new(5, i18n!(cx, "dialog.log.interval_5s")),
                    SelectOption::new(10, i18n!(cx, "dialog.log.interval_10s")),
                    SelectOption::new(30, i18n!(cx, "dialog.log.interval_30s")),
                    SelectOption::new(60, i18n!(cx, "dialog.log.interval_60s")),
                ])
                .selected(Some(0))
        });

        Self {
            terminal_id,
            focus_handle: cx.focus_handle(),
            mode_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            filename_input,
            initial_focus_done: false,
            append_mode: false,
            interval_select,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(LogRecordDialogEvent::Close);
    }

    fn start_recording(&self, cx: &mut Context<Self>) {
        let filename = self.filename_input.read(cx).text().to_string();
        let filename = if filename.trim().is_empty() {
            "terminal-log.log".to_string()
        } else {
            filename
        };
        let auto_save_interval = self
            .interval_select
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or(0);
        cx.emit(LogRecordDialogEvent::StartRecording {
            terminal_id: self.terminal_id.clone(),
            filename,
            append_mode: self.append_mode,
            auto_save_interval,
        });
    }
}

impl Render for LogRecordDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let focus_handle = self.focus_handle.clone();

        if !self.initial_focus_done {
            self.initial_focus_done = true;
            self.filename_input.update(cx, |inp, cx| {
                inp.focus(window, cx);
            });
        }

        let focus_group = FocusGroup::new();
        focus_group.add(self.filename_input.read(cx).focus_handle(cx));
        focus_group.add(self.mode_focus.clone());
        focus_group.add(self.interval_select.read(cx).focus_handle().clone());
        focus_group.add(self.cancel_focus.clone());
        focus_group.add(self.confirm_focus.clone());

        modal_content("log-record-dialog-modal", cx)
            .w(px(400.0))
            .track_focus(&focus_handle)
            .key_context("LogRecordDialog")
            .tab_cycle(&focus_group)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.close(cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(modal_header(
                        i18n!(cx, "dialog.log.title"),
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
                            .px(SPACE_XL)
                            .py(SPACE_LG)
                            .gap(SPACE_XL)
                            .child(
                                labeled_input(&i18n!(cx, "dialog.log.filename"), &t, cx).child(
                                    div().flex_1().child(velowork_ui::Input::new(&self.filename_input).cleanable(true)),
                                ),
                            )
                            .child(
                                labeled_input(&i18n!(cx, "dialog.log.mode"), &t, cx).child(
                                    RadioGroup::new("log-record-mode")
                                        .mode(RadioMode::Button)
                                        .focus(&self.mode_focus)
                                        .selected(Some(self.append_mode))
                                        .options(vec![
                                            RadioOption::new(false, i18n!(cx, "dialog.log.overwrite")),
                                            RadioOption::new(true, i18n!(cx, "dialog.log.append")),
                                        ])
                                        .on_change({
                                            let entity = cx.entity();
                                            move |append_mode: &bool, _, cx| {
                                                let val = *append_mode;
                                                entity.update(cx, |this, cx| {
                                                    this.append_mode = val;
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                ),
                            )
                            .child(
                                labeled_input(&i18n!(cx, "dialog.log.interval"), &t, cx).child(
                                    div().flex_1().child(Select::new(&self.interval_select))
                                )
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
                                button("cancel-btn", &i18n!(cx, "common.cancel"), &t)
                                    .focus_handle(&self.cancel_focus)
                                    .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
                            )
                            .child(
                                button_primary("start-btn", i18n!(cx, "dialog.log.start"), &t)
                                    .icon_left(AppIcon::Terminal)
                                    .focus_handle(&self.confirm_focus)
                                    .on_click(cx.listener(|this, _, _, cx| this.start_recording(cx)))
                            )
                    )
    }
}

/// 1:1 录制工具条高保真预览组件，用于弹窗收缩飞行动画后半程（110ms-250ms）内交叉淡入。
pub struct LogToolbarPreview;

impl Render for LogToolbarPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use velowork_ui::tokens::{ICON_STD, RADIUS_MD, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, ui_text_md, ui_text_ms};
        use velowork_ui::theme::{theme, with_alpha};
        use velowork_ui::design::semantic::SemanticPalette;

        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let status_label = i18n!(cx, "toolbar.log.recording");

        let dot_indicator = div()
            .w(px(14.0))
            .h(px(14.0))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(with_alpha(t.error, 0.28))
            .child(
                div()
                    .w(px(8.0))
                    .h(px(8.0))
                    .rounded_full()
                    .bg(p.status_error)
                    .opacity(0.85),
            );

        div()
            .w(px(304.0))
            .h(px(42.0))
            .flex()
            .items_center()
            .justify_between()
            .px(SPACE_LG)
            .py(SPACE_SM)
            .gap(SPACE_MD)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(SPACE_MD)
                    .child(dot_indicator)
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text_primary)
                            .child(status_label),
                    )
                    .child(
                        div()
                            .text_size(ui_text_ms(cx))
                            .font_family("JetBrains Mono")
                            .text_color(p.text_muted)
                            .px(SPACE_SM)
                            .py(px(2.0))
                            .bg(with_alpha(t.bg_hover, if t.is_dark() { 0.55 } else { 0.85 }))
                            .rounded(RADIUS_MD)
                            .child("00:00:00"),
                    ),
            )
            .child(
                div()
                    .w(px(1.0))
                    .h(px(16.0))
                    .bg(p.border_subtle),
            )
            .child(
                h_flex()
                    .gap(SPACE_SM)
                    .items_center()
                    .child(
                        div()
                            .w(px(24.0))
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_STD)
                            .child(AppIcon::Pause.size(ICON_STD).text_color(p.text_secondary)),
                    )
                    .child(
                        div()
                            .w(px(24.0))
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_STD)
                            .child(AppIcon::Stop.size(ICON_STD).text_color(p.status_error)),
                    )
                    .child(
                        div()
                            .w(px(24.0))
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_STD)
                            .child(AppIcon::Close.size(ICON_STD).text_color(p.text_secondary)),
                    ),
            )
    }
}
