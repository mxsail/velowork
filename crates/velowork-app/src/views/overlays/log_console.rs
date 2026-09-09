use crate::keybindings::Cancel;
use crate::logging::{self, LogLine};
use crate::theme::theme;
use crate::ui::tokens::{mono_font_family, ui_text, ui_text_ms};
use velowork_ui::tokens::{SPACE_XS, SPACE_SM, SPACE_MD, SPACE_XL, RADIUS_STD, ICON_MD, RADIUS_MD};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::*;
use velowork_ui::{h_flex, v_flex};
use velowork_ui::badge::keyboard_hints_footer;
use velowork_ui::input::InputState;
use velowork_core::theme::ThemeColors;
use velowork_ui::overlay::{modal_content, modal_header};
use std::time::Duration;
use velowork_i18n::i18n;

/// Cap on lines mirrored locally for rendering (the ring itself also caps).
const DISPLAY_CAP: usize = 10_000;
/// How often the console pulls new lines from the ring.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Which input field is active.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveField {
    Capture,
    Filter,
}

pub enum LogConsoleEvent {
    Close,
}

impl velowork_ui::overlay::CloseEvent for LogConsoleEvent {
    fn is_close(&self) -> bool {
        matches!(self, Self::Close)
    }
}

pub struct LogConsole {
    focus_handle: FocusHandle,
    lines: Vec<LogLine>,
    /// Next seq we haven't pulled yet.
    cursor: u64,
    prefill_capture: String,
    /// Editable capture directive (applied to the logger on Enter).
    capture_input: Option<Entity<InputState>>,
    /// Live display substring filter (matches target + message).
    filter_input: Option<Entity<InputState>>,
    /// Minimum severity to display (`Trace` = show everything).
    min_level: log::Level,
    active: ActiveField,
    auto_scroll: bool,
    scroll: UniformListScrollHandle,
    /// Set when new lines arrived and we should stick to the bottom.
    pending_scroll: bool,
    initial_focus_done: bool,
}

impl LogConsole {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (lines, cursor, capture_str) = logging::hub()
            .map(|h| (h.snapshot_since(0), h.next_seq(), h.directives()))
            .unwrap_or_else(|| (Vec::new(), 0, logging::DEFAULT_CAPTURE.to_string()));

        // Poll the ring for new lines while the console is open.
        cx.spawn(async move |this: WeakEntity<LogConsole>, cx| {
            loop {
                smol::Timer::after(POLL_INTERVAL).await;
                let alive = this
                    .update(cx, |this, cx| this.pull_new(cx))
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            lines,
            cursor,
            prefill_capture: capture_str,
            capture_input: None,
            filter_input: None,
            min_level: log::Level::Trace,
            active: ActiveField::Filter,
            auto_scroll: true,
            scroll: UniformListScrollHandle::new(),
            pending_scroll: true,
            initial_focus_done: false,
        }
    }

    /// Append any lines captured since our cursor, capping the local mirror.
    fn pull_new(&mut self, cx: &mut Context<Self>) {
        let Some(hub) = logging::hub() else { return };
        if hub.next_seq() == self.cursor {
            return;
        }
        let mut fresh = hub.snapshot_since(self.cursor);
        if fresh.is_empty() {
            return;
        }
        self.cursor = hub.next_seq();
        self.lines.append(&mut fresh);
        if self.lines.len() > DISPLAY_CAP {
            let overflow = self.lines.len() - DISPLAY_CAP;
            self.lines.drain(0..overflow);
        }
        if self.auto_scroll {
            self.pending_scroll = true;
        }
        cx.notify();
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(LogConsoleEvent::Close);
    }

    fn apply_capture(&mut self, cx: &mut Context<Self>) {
        let text = self
            .capture_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_default();
        if let Some(hub) = logging::hub() {
            hub.set_capture_filter(&text);
        }
        cx.notify();
    }

    fn set_level(&mut self, level: log::Level, cx: &mut Context<Self>) {
        self.min_level = level;
        cx.notify();
    }

    /// Lines passing the display filter (severity + substring on target+msg).
    fn visible(&self, cx: &App) -> Vec<LogLine> {
        let needle = self
            .filter_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string().to_ascii_lowercase())
            .unwrap_or_default();
        self.lines
            .iter()
            .filter(|l| l.level <= self.min_level)
            .filter(|l| {
                needle.is_empty()
                    || l.message.to_ascii_lowercase().contains(&needle)
                    || l.target.to_ascii_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    }
}

impl EventEmitter<LogConsoleEvent> for LogConsole {}

fn level_color(level: log::Level, t: &ThemeColors) -> u32 {
    match level {
        log::Level::Error => t.error,
        log::Level::Warn => t.warning,
        log::Level::Info => t.success,
        log::Level::Debug => t.text_secondary,
        log::Level::Trace => t.text_muted,
    }
}

fn level_label(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "ERR",
        log::Level::Warn => "WRN",
        log::Level::Info => "INF",
        log::Level::Debug => "DBG",
        log::Level::Trace => "TRC",
    }
}

/// One log line as a plain, paste-friendly string (mirrors the row layout).
fn format_line_for_copy(line: &LogLine) -> String {
    format!(
        "{} {} {} {}",
        format_time(&line.timestamp),
        level_label(line.level),
        line.target,
        line.message
    )
}

fn format_time(ts: &time::OffsetDateTime) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ts.hour(),
        ts.minute(),
        ts.second(),
        ts.millisecond()
    )
}

impl Render for LogConsole {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let focus_handle = self.focus_handle.clone();

        if self.capture_input.is_none() {
            let placeholder = "velowork::cmd=trace,info";
            let default_val = self.prefill_capture.clone();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(placeholder)
                    .default_value(&default_val)
            });
            self.capture_input = Some(input);
        }

        if self.filter_input.is_none() {
            let placeholder = "substring (target or message)";
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(placeholder)
            });
            let input_clone = input.clone();
            cx.subscribe(&input_clone, |_, _, _: &velowork_ui::input::InputEvent, cx| {
                cx.notify();
            })
            .detach();
            self.filter_input = Some(input);
        }

        // Focus the filter input on opening, not on every subsequent re-render.
        if !self.initial_focus_done {
            self.initial_focus_done = true;
            if let Some(filter_input) = self.filter_input.as_ref() {
                filter_input.update(cx, |input, cx| {
                    input.focus(window, cx);
                });
            }
        }

        let visible = self.visible(cx);
        let total = self.lines.len();
        let shown = visible.len();

        if self.pending_scroll && shown > 0 {
            self.scroll.scroll_to_item(shown - 1, ScrollStrategy::Top);
            self.pending_scroll = false;
        }

        modal_content("log-console-modal", cx)
            .w(px(900.0))
            .h(px(620.0))
            .track_focus(&focus_handle)
            .key_context("LogConsole")
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| this.close(cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.as_str() == "tab" {
                    cx.stop_propagation();
                    this.active = match this.active {
                        ActiveField::Capture => ActiveField::Filter,
                        ActiveField::Filter => ActiveField::Capture,
                    };
                    match this.active {
                        ActiveField::Capture => {
                            if let Some(inp) = this.capture_input.as_ref() {
                                inp.update(cx, |s, cx| s.focus(window, cx));
                            }
                        }
                        ActiveField::Filter => {
                            if let Some(inp) = this.filter_input.as_ref() {
                                inp.update(cx, |s, cx| s.focus(window, cx));
                            }
                        }
                    }
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(modal_header(
                i18n!(cx, "log.title"),
                Some(velowork_i18n::t_fmt(&*cx, "log.shown_buffered", &[("shown", &shown.to_string()), ("total", &total.to_string())])),
                &t,
                cx,
                cx.listener(|this, _, _window, cx| this.close(cx)),
            ))
            .child(self.render_toolbar(&t, cx))
            .child(self.render_list(visible, &t, cx))
            .child({
                let hints: Vec<(String, String)> = vec![
                    ("Tab".to_string(), i18n!(cx, "log.hint_switch_field")),
                    ("Enter".to_string(), i18n!(cx, "log.hint_apply_capture")),
                    ("Esc".to_string(), i18n!(cx, "log.hint_close")),
                ];
                let hint_refs: Vec<(&str, &str)> = hints.iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                keyboard_hints_footer(&hint_refs, &t, cx)
            })
    }
}

impl LogConsole {
    fn render_toolbar(&self, t: &ThemeColors, cx: &mut Context<Self>) -> impl IntoElement {
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        v_flex()
            .px(SPACE_XL)
            .py(px(10.0))
            .gap(SPACE_MD)
            .border_b_1()
            .border_color(p.border_subtle)
            // Capture directive row (the runtime switch).
            .child(
                h_flex()
                    .items_center()
                    .gap(SPACE_MD)
                    .child(field_label(i18n!(cx, "log.capture").as_str(), t, cx))
                    .child(
                        div()
                            .id("log-capture-input-container")
                            .key_context("LogCaptureInput")
                            .flex_1()
                            .h(px(28.0))
                            .when_some(self.capture_input.as_ref(), |this, inp| {
                                this.child(velowork_ui::Input::new(inp).cleanable(true))
                            })
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                                if event.keystroke.key.as_str() == "enter" {
                                    cx.stop_propagation();
                                    this.apply_capture(cx);
                                }
                            })),
                    ),
            )
            // Display filter + severity chips + controls.
            .child(
                h_flex()
                    .items_center()
                    .gap(SPACE_MD)
                    .child(field_label(i18n!(cx, "common.filter").as_str(), t, cx))
                    .child(
                        div()
                            .id("log-filter-input-container")
                            .key_context("LogFilterInput")
                            .flex_1()
                            .h(px(28.0))
                            .when_some(self.filter_input.as_ref(), |this, inp| {
                                this.child(velowork_ui::Input::new(inp).cleanable(true))
                            }),
                    )
                    .child(
                        h_flex()
                            .gap(SPACE_XS)
                            .children([
                                log::Level::Error,
                                log::Level::Warn,
                                log::Level::Info,
                                log::Level::Debug,
                                log::Level::Trace,
                            ]
                            .iter()
                            .map(|&level| {
                                let active = level <= self.min_level;
                                let color = level_color(level, t);
                                div()
                                    .id(ElementId::Name(format!("log-level-chip-{:?}", level).into()))
                                    .cursor_pointer()
                                    .px(SPACE_SM)
                                    .py(px(2.0))
                                    .rounded(RADIUS_STD)
                                    .text_size(ui_text_ms(cx))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .bg(if active { rgb(color) } else { rgb(t.bg_secondary) })
                                    .text_color(if active { rgb(t.bg_panel) } else { rgb(t.text_muted) })
                                    .child(level_label(level))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .on_click(cx.listener(move |this, _, _window, cx| this.set_level(level, cx)))
                            })),
                    )
                    .child(toggle_chip(
                        "autoscroll",
                        self.auto_scroll,
                        t,
                        cx,
                        cx.listener(|this, _, _w, cx| {
                            this.auto_scroll = !this.auto_scroll;
                            this.pending_scroll = this.auto_scroll;
                            cx.notify();
                        }),
                    ))
                    .child(toggle_chip(
                        "copy",
                        false,
                        t,
                        cx,
                        cx.listener(|this, _, _w, cx| {
                            let text = this
                                .visible(cx)
                                .iter()
                                .map(format_line_for_copy)
                                .collect::<Vec<_>>()
                                .join("\n");
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }),
                    ))
                    .child(toggle_chip(
                        "clear",
                        false,
                        t,
                        cx,
                        cx.listener(|this, _, _w, cx| {
                            if let Some(hub) = logging::hub() {
                                hub.clear();
                            }
                            this.lines.clear();
                            if let Some(hub) = logging::hub() {
                                this.cursor = hub.next_seq();
                            }
                            cx.notify();
                        }),
                    )),
            )
    }

    fn render_list(
        &self,
        visible: Vec<LogLine>,
        t: &ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = *t;
        if visible.is_empty() {
            return v_flex()
                .id("log-console-empty")
                .flex_1()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(ui_text(13.0, cx))
                        .text_color(rgb(t.text_muted))
                        .child(i18n!(cx, "log.no_matches")),
                )
                .into_any_element();
        }

        let copy_tip = i18n!(cx, "common.copy");
        let count = visible.len();
        uniform_list(
            "log-console-list",
            count,
            move |range, _window, cx| {
                range
                    .map(|i| render_row(&visible[i], &copy_tip, &t, cx).into_any_element())
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .into_any_element()
    }
}

fn render_row(
    line: &LogLine,
    copy_tip: &str,
    t: &ThemeColors,
    cx: &App,
) -> impl IntoElement {
    let p = SemanticPalette::from_theme(t);
    let group_id: SharedString = format!("log-row-{}", line.seq).into();
    let copy_id: SharedString = format!("log-copy-{}", line.seq).into();
    let copy_text = line.message.clone();
    let copy_tip = copy_tip.to_string();

    // 动态根据设置系统中的字体缩放计算单行行高、双行高度及总行高，避免字体调大时裁切
    let font_size = ui_text_ms(cx);
    let line_h = font_size * 1.5;
    let text_box_h = line_h * 2.0;
    let row_h = text_box_h + px(12.0);

    h_flex()
        .id(ElementId::Name(group_id.clone()))
        .group(group_id.clone())
        .w_full()
        .h(row_h)
        .px(SPACE_XL)
        .py(px(6.0))
        .gap(SPACE_MD)
        .items_center()
        .border_b_1()
        .border_color(p.border_subtle)
        .font_family(mono_font_family(cx))
        .text_size(font_size)
        .hover(|s| s.bg(p.surface_hover))
        // 1. 加宽时间戳列：120px 宽度，单行不换行
        .child(
            div()
                .flex_shrink_0()
                .w(px(120.0))
                .whitespace_nowrap()
                .text_color(p.text_muted)
                .child(format_time(&line.timestamp)),
        )
        // 2. 日志级别列 (ERR, WRN, INF, DBG, TRC)
        .child(
            div()
                .flex_shrink_0()
                .w(px(34.0))
                .whitespace_nowrap()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(level_color(line.level, t)))
                .child(level_label(line.level)),
        )
        // 3. Target 模块列：170px 宽度，动态双行文本高度（随字体设置同比例缩放）
        .child(
            div()
                .flex_shrink_0()
                .w(px(170.0))
                .h(text_box_h)
                .line_height(line_h)
                .overflow_hidden()
                .text_color(p.text_secondary)
                .whitespace_normal()
                .child(line.target.clone()),
        )
        // 4. Message 日志消息列：动态双行文本高度（随字体设置同比例缩放）
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(text_box_h)
                .line_height(line_h)
                .overflow_hidden()
                .text_color(p.text_primary)
                .whitespace_normal()
                .child(line.message.clone()),
        )
        // 5. 右侧边缘固定居中的放大版复制按钮 (30px x 30px)
        .child(
            div()
                .id(ElementId::Name(copy_id))
                .flex_shrink_0()
                .opacity(0.0)
                .group_hover(group_id, |s| s.opacity(1.0))
                .cursor_pointer()
                .w(px(30.0))
                .h(px(30.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(RADIUS_MD)
                .bg(p.surface_raised)
                .border_1()
                .border_color(p.border_subtle)
                .shadow_md()
                .hover(|s| s.bg(p.surface_hover).border_color(p.border_active))
                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(copy_tip.clone())).into())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                })
                .child(
                    AppIcon::Copy
                        .size(ICON_MD)
                        .text_color(p.text_secondary),
                ),
        )
}

fn field_label(label: &str, t: &ThemeColors, cx: &App) -> impl IntoElement {
    div()
        .w(px(52.0))
        .flex_shrink_0()
        .text_size(ui_text_ms(cx))
        .text_color(rgb(t.text_muted))
        .child(label.to_string())
}

fn toggle_chip<F>(
    label: &'static str,
    on: bool,
    t: &ThemeColors,
    cx: &App,
    handler: F,
) -> impl IntoElement
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    div()
        .id(ElementId::Name(label.into()))
        .px(SPACE_SM)
        .py(px(2.0))
        .rounded(RADIUS_STD)
        .cursor_pointer()
        .flex_shrink_0()
        .text_size(ui_text_ms(cx))
        .text_color(rgb(if on { t.text_primary } else { t.text_muted }))
        .bg(rgb(if on { t.bg_hover } else { t.bg_secondary }))
        .border_1()
        .border_color(rgb(if on { t.border_active } else { t.border }))
        .child(label)
        .on_click(handler)
}

velowork_ui::impl_focusable!(LogConsole);
