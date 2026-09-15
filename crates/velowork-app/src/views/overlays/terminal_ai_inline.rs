//! Terminal inline AI interaction component.
//!
//! Provides a two-stage inline AI experience directly inside the terminal viewport:
//! 1. Floating Capsule Toolbar on text selection (explain, generate command, send to side panel, mini-ask).
//! 2. Inline Popover displaying streaming response, model switcher dropdown, command execution cards,
//!    follow-up conversation, and seamless escalation to the right AI assistant dock panel.

use gpui::*;
use gpui::prelude::*;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use velowork_ai::{provider, StreamChunk};
use velowork_i18n::i18n;
use velowork_markdown::{
    find_line_boundaries, find_word_boundaries, MarkdownElement, MarkdownSelectionEvent,
};
use velowork_ui::capsule_toolbar::{
    capsule_divider, capsule_icon_button, capsule_toolbar_container,
};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::icon_button::icon_button_sized;
use velowork_ui::motion::{ease_out_cubic, DURATION_PANEL};
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::select::{Select, SelectEvent, SelectOption, SelectPlacement, SelectState};
use velowork_ui::simple_input::{InputEvent, SimpleInput, SimpleInputState};
use velowork_ui::theme::{theme, ThemeColors};
use velowork_ui::tokens::{
    elevation_menu_shadow, ui_text_md, RADIUS_LG, RADIUS_MD, RADIUS_SM, SPACE_MD,
    SPACE_SM, SPACE_XS,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex};

/// Minimum dimensions for the resizable inline AI popover.
const MIN_POPOVER_WIDTH: f32 = 360.0;
const MIN_POPOVER_HEIGHT: f32 = 260.0;

/// Edges and corners of the inline AI popover available for resizing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PopoverResizeEdge {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl PopoverResizeEdge {
    pub fn cursor_style(&self) -> CursorStyle {
        match self {
            Self::Top | Self::Bottom => CursorStyle::ResizeUpDown,
            Self::Left | Self::Right => CursorStyle::ResizeLeftRight,
            Self::TopLeft | Self::BottomRight => CursorStyle::ResizeUpLeftDownRight,
            Self::TopRight | Self::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
        }
    }
}

/// Active dragging state when resizing the inline AI popover.
#[derive(Clone, Copy, Debug)]
pub struct PopoverResizeDrag {
    pub edge: PopoverResizeEdge,
    pub start_mouse: Point<Pixels>,
    pub start_size: Size<Pixels>,
    pub start_pos: Point<Pixels>,
}

/// Mode of the inline AI view.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InlineAiMode {
    /// Capsule toolbar hovering above/below selected text.
    Toolbar,
    /// Inline popover window showing streaming AI response.
    Popover,
}

/// Active text selection in an inline AI assistant message.
#[derive(Clone, Debug)]
pub struct InlineChatSelection {
    pub msg_index: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Active mouse drag for text selection in an inline AI assistant message.
#[derive(Clone, Debug)]
pub struct InlineSelectionDrag {
    pub msg_index: usize,
    pub anchor: usize,
    pub plain_text: String,
}

/// A single turn in the inline popover chat history.
#[derive(Clone)]
pub struct InlineAiMessage {
    pub is_user: bool,
    pub text: String,
    pub quote: Option<String>,
    pub extracted_commands: Vec<String>,
    pub is_streaming: bool,
}

/// Events emitted by TerminalAiInline.
#[derive(Clone, Debug)]
pub enum TerminalAiInlineEvent {
    /// Insert a command into the terminal prompt without executing immediately.
    InsertToTerminal {
        terminal_id: String,
        command: String,
    },
    /// Send command directly to the terminal PTY and execute with newline.
    RunInTerminal {
        terminal_id: String,
        command: String,
    },
    /// Escalate conversation to the right AI assistant dock panel.
    ContinueInSidePanel {
        quote: String,
        reply: String,
    },
    /// Append a completed conversation turn to the project's session.
    AppendConversation {
        project_id: String,
        user_message: String,
        quote: Option<String>,
        assistant_reply: String,
    },
    /// Open the settings dialog to configure AI models.
    OpenSettings,
    /// Dismiss and close the inline AI view.
    Close,
}

impl EventEmitter<TerminalAiInlineEvent> for TerminalAiInline {}

pub struct TerminalAiInline {
    pub mode: InlineAiMode,
    pub terminal_id: String,
    pub project_id: String,
    pub position: Point<Pixels>,
    pub selection_text: String,
    pub reply_text: String,
    pub is_streaming: bool,
    pub error_message: Option<String>,
    pub extracted_commands: Vec<String>,
    pub model_select: Entity<SelectState<String>>,
    pub selected_model_id: Option<String>,
    pub toolbar_input: Entity<SimpleInputState>,
    pub followup_input: Entity<SimpleInputState>,
    pub active_selection: Option<InlineChatSelection>,
    pub selection_dragging: Option<InlineSelectionDrag>,
    pub overlay_registry: Option<Entity<OverlayRegistry>>,
    pub stream_rx: Option<Arc<parking_lot::Mutex<mpsc::Receiver<StreamChunk>>>>,
    pub _stream_task: Option<Task<()>>,
    pub enter_start: Instant,
    pub has_no_model: bool,
    pub popover_size: Size<Pixels>,
    pub resize_drag: Option<PopoverResizeDrag>,
    pub messages: Vec<InlineAiMessage>,
    pub copied_msg_index: Option<usize>,
    pub scroll_handle: ScrollHandle,
    pub focus_handle: FocusHandle,
    pub animation_frame: u64,
}

impl TerminalAiInline {
    /// Create a new inline AI floating toolbar.
    pub fn new_toolbar(
        terminal_id: String,
        project_id: String,
        position: Point<Pixels>,
        selection_text: String,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (model_select, selected_model_id, has_no_model) =
            Self::create_model_select(&overlay_registry, cx);

        let tb_ph = i18n!(cx, "terminal.ai_toolbar_ask_placeholder");
        let toolbar_input = cx.new(|cx| {
            SimpleInputState::new(cx)
                .placeholder(tb_ph)
                .submit_on_enter(true)
        });

        let fu_ph = i18n!(cx, "terminal.inline_ai_follow_up_placeholder");
        let followup_input = cx.new(|cx| {
            SimpleInputState::new(cx)
                .placeholder(fu_ph)
                .multiline()
                .multiline_rows(2)
                .submit_on_enter(true)
        });

        cx.subscribe(&toolbar_input, |this: &mut Self, _, event: &InputEvent, cx| {
            if *event == InputEvent::PressEnter {
                let query = this.toolbar_input.read(cx).value().trim().to_string();
                if !query.is_empty() {
                    let inp = this.toolbar_input.clone();
                    cx.defer(move |cx| {
                        inp.update(cx, |i, cx| i.set_value("", cx));
                    });
                    this.start_turn(query, Some(this.selection_text.clone()), cx);
                }
            }
        })
        .detach();

        cx.subscribe(&followup_input, |this: &mut Self, _, event: &InputEvent, cx| {
            if *event == InputEvent::PressEnter {
                this.submit_followup(cx);
            }
        })
        .detach();

        Self {
            mode: InlineAiMode::Toolbar,
            terminal_id,
            project_id,
            position,
            selection_text,
            reply_text: String::new(),
            is_streaming: false,
            error_message: None,
            extracted_commands: Vec::new(),
            model_select,
            selected_model_id,
            toolbar_input,
            followup_input,
            active_selection: None,
            selection_dragging: None,
            overlay_registry,
            stream_rx: None,
            _stream_task: None,
            enter_start: Instant::now(),
            has_no_model,
            popover_size: size(px(480.0), px(380.0)),
            resize_drag: None,
            messages: Vec::new(),
            copied_msg_index: None,
            scroll_handle: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            animation_frame: 0,
        }
    }

    /// Retrieve the currently selected plain text, if any.
    pub fn get_active_selection_text(&self) -> Option<String> {
        self.active_selection.as_ref().map(|s| s.text.clone())
    }

    /// Clear any active selection or in-progress selection drag.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.active_selection.is_some() || self.selection_dragging.is_some() {
            self.active_selection = None;
            self.selection_dragging = None;
            cx.notify();
        }
    }

    /// Handle markdown selection events (start, drag update, finish).
    pub fn handle_selection_event(
        &mut self,
        msg_idx: usize,
        event: MarkdownSelectionEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            MarkdownSelectionEvent::Start {
                offset,
                click_count,
                plain_text,
            } => {
                if click_count == 2 {
                    let (start, end) = find_word_boundaries(&plain_text, offset);
                    if start < end {
                        let selected_text: String =
                            plain_text.chars().skip(start).take(end - start).collect();
                        self.active_selection = Some(InlineChatSelection {
                            msg_index: msg_idx,
                            start,
                            end,
                            text: selected_text,
                        });
                        self.selection_dragging = None;
                        cx.notify();
                        return;
                    }
                } else if click_count >= 3 {
                    let (start, end) = find_line_boundaries(&plain_text, offset);
                    if start < end {
                        let selected_text: String =
                            plain_text.chars().skip(start).take(end - start).collect();
                        self.active_selection = Some(InlineChatSelection {
                            msg_index: msg_idx,
                            start,
                            end,
                            text: selected_text,
                        });
                        self.selection_dragging = None;
                        cx.notify();
                        return;
                    }
                }

                self.selection_dragging = Some(InlineSelectionDrag {
                    msg_index: msg_idx,
                    anchor: offset,
                    plain_text,
                });
                if self.active_selection.is_some() {
                    self.active_selection = None;
                    cx.notify();
                }
            }
            MarkdownSelectionEvent::Update { offset } => {
                let Some(ref drag) = self.selection_dragging else {
                    return;
                };
                if drag.msg_index != msg_idx {
                    return;
                }
                let anchor = drag.anchor;
                let start = anchor.min(offset);
                let end = anchor.max(offset);
                if start < end {
                    let selected_text: String =
                        drag.plain_text.chars().skip(start).take(end - start).collect();
                    let changed = match &self.active_selection {
                        Some(current) => {
                            current.msg_index != msg_idx
                                || current.start != start
                                || current.end != end
                                || current.text != selected_text
                        }
                        None => true,
                    };
                    if changed {
                        self.active_selection = Some(InlineChatSelection {
                            msg_index: msg_idx,
                            start,
                            end,
                            text: selected_text,
                        });
                        cx.notify();
                    }
                } else if self.active_selection.is_some() {
                    self.active_selection = None;
                    cx.notify();
                }
            }
            MarkdownSelectionEvent::End => {
                self.selection_dragging = None;
            }
        }
    }

    /// Create a new inline popover directly (e.g. triggered via Ctrl+K shortcut).
    pub fn new_popover(
        terminal_id: String,
        project_id: String,
        position: Point<Pixels>,
        selection_text: String,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut inline = Self::new_toolbar(
            terminal_id,
            project_id,
            position,
            selection_text.clone(),
            overlay_registry,
            cx,
        );
        inline.enter_start = Instant::now();
        inline.start_enter_animation(cx);
        inline.mode = InlineAiMode::Popover;
        if !selection_text.trim().is_empty() {
            inline.trigger_explain(cx);
        }
        inline.scroll_handle.scroll_to_bottom();
        inline
    }

    /// Drive a 60fps frame notification loop during the entrance animation duration.
    fn start_enter_animation(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let start = Instant::now();
            loop {
                let elapsed = start.elapsed();
                if elapsed >= DURATION_PANEL {
                    let _ = this.update(cx, |_this, cx| cx.notify());
                    break;
                }
                smol::Timer::after(Duration::from_millis(16)).await;
                if this.update(cx, |_this, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn create_model_select(
        overlay_registry: &Option<Entity<OverlayRegistry>>,
        cx: &mut Context<Self>,
    ) -> (Entity<SelectState<String>>, Option<String>, bool) {
        let settings = crate::settings::settings_entity(cx).read(cx).settings.clone();
        let models = settings.ai_models.clone();
        let has_no_model = models.is_empty();

        let default_id = settings
            .ai_default_model_id
            .clone()
            .or_else(|| models.first().map(|m| m.id.clone()));

        let reg = overlay_registry.clone();
        let options: Vec<SelectOption<String>> = models
            .iter()
            .map(|m| SelectOption::new(m.id.clone(), m.name.clone()))
            .collect();

        let selected = default_id.clone();
        let model_select = cx.new(|cx| {
            let mut s = SelectState::new(cx)
                .options(options)
                .selected(selected)
                .placeholder(i18n!(cx, "ai_assistant.model"))
                .placement(SelectPlacement::Below)
                .ghost(true);
            if let Some(r) = reg {
                s.set_overlay_registry(r);
            }
            s
        });

        cx.subscribe(&model_select, |this: &mut Self, _, event: &SelectEvent<String>, cx| {
            let SelectEvent::Change(opt_id) = event;
            this.selected_model_id = opt_id.clone();
            cx.notify();
        })
        .detach();

        (model_select, default_id, has_no_model)
    }

    /// Start a streaming prompt request to the AI model as part of a multi-turn conversation.
    pub fn start_turn(&mut self, user_text: String, quote: Option<String>, cx: &mut Context<Self>) {
        if self.mode != InlineAiMode::Popover {
            self.mode = InlineAiMode::Popover;
            self.enter_start = Instant::now();
            self.start_enter_animation(cx);
        }

        self.error_message = None;

        let settings = crate::settings::settings_entity(cx).read(cx).settings.clone();
        if settings.ai_models.is_empty() {
            self.has_no_model = true;
            self.is_streaming = false;
            self.error_message = Some(i18n!(cx, "terminal.inline_ai_no_model_configured"));
            cx.notify();
            return;
        }
        self.has_no_model = false;

        let selected_id = self
            .selected_model_id
            .clone()
            .or_else(|| settings.ai_default_model_id.clone())
            .or_else(|| settings.ai_models.first().map(|m| m.id.clone()))
            .unwrap_or_default();

        let model_cfg = settings
            .ai_models
            .iter()
            .find(|m| m.id == selected_id || m.model_id == selected_id)
            .cloned()
            .or_else(|| settings.ai_models.first().cloned());

        let Some(cfg) = model_cfg else {
            self.has_no_model = true;
            self.is_streaming = false;
            self.error_message = Some(i18n!(cx, "terminal.inline_ai_no_model_configured"));
            cx.notify();
            return;
        };

        // Push User Message
        self.messages.push(InlineAiMessage {
            is_user: true,
            text: user_text.clone(),
            quote: quote.clone(),
            extracted_commands: Vec::new(),
            is_streaming: false,
        });

        // Push Assistant Message
        self.reply_text.clear();
        self.extracted_commands.clear();
        self.is_streaming = true;

        self.messages.push(InlineAiMessage {
            is_user: false,
            text: String::new(),
            quote: None,
            extracted_commands: Vec::new(),
            is_streaming: true,
        });
        self.scroll_handle.scroll_to_bottom();

        // Prepare multi-turn messages for API
        let mut api_messages = Vec::new();
        for msg in &self.messages {
            if msg.is_streaming && msg.text.is_empty() {
                continue;
            }
            if msg.is_user {
                let content = if let Some(ref q) = msg.quote {
                    if !q.trim().is_empty() {
                        format!("终端选中文本：\n```\n{}\n```\n\n用户问题：{}", q, msg.text)
                    } else {
                        msg.text.clone()
                    }
                } else {
                    msg.text.clone()
                };
                api_messages.push((content, true));
            } else if !msg.text.is_empty() {
                api_messages.push((msg.text.clone(), false));
            }
        }

        // Abort any active streaming task before initiating a new request.
        self._stream_task = None;
        self.stream_rx = None;

        let rx = provider::stream_api_reply(&cfg.base_url, &cfg.api_key, &cfg.model_id, &api_messages);
        let rx_arc = Arc::new(parking_lot::Mutex::new(rx));
        self.stream_rx = Some(rx_arc);

        let user_text_for_done = user_text;
        let quote_for_done = quote;

        self._stream_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                smol::Timer::after(std::time::Duration::from_millis(40)).await;

                let finished = this
                    .update(cx, |this, cx| {
                        this.animation_frame = this.animation_frame.wrapping_add(1);
                        let Some(ref rx_lock) = this.stream_rx else {
                            return true;
                        };
                        let rx = rx_lock.lock();

                        let mut done = false;
                        let mut updated = false;

                        while let Ok(chunk) = rx.try_recv() {
                            match chunk {
                                StreamChunk::Delta(delta) => {
                                    this.reply_text.push_str(&delta);
                                    if let Some(last) = this.messages.last_mut().filter(|m| !m.is_user) {
                                        last.text.push_str(&delta);
                                    }
                                    updated = true;
                                }
                                StreamChunk::Done => {
                                    this.is_streaming = false;
                                    if let Some(last) = this.messages.last_mut().filter(|m| !m.is_user) {
                                        last.is_streaming = false;
                                    }
                                    done = true;
                                    updated = true;
                                    break;
                                }
                                StreamChunk::Error(err) => {
                                    this.error_message = Some(err.to_string());
                                    this.is_streaming = false;
                                    if let Some(last) = this.messages.last_mut().filter(|m| !m.is_user) {
                                        last.is_streaming = false;
                                    }
                                    done = true;
                                    updated = true;
                                    break;
                                }
                                _ => {}
                            }
                        }

                        if updated {
                            let text = this.reply_text.clone();
                            let cmds = Self::extract_commands(&text);
                            this.extracted_commands = cmds.clone();
                            if let Some(last) = this.messages.last_mut().filter(|m| !m.is_user) {
                                last.extracted_commands = cmds;
                            }
                            this.scroll_handle.scroll_to_bottom();
                            cx.notify();
                        } else if this.is_streaming && this.reply_text.is_empty() {
                            // 思考等待中：每帧通知重绘以驱动圆点呼吸波浪动效
                            cx.notify();
                        }

                        if done {
                            this.scroll_handle.scroll_to_bottom();
                            let final_reply = this.reply_text.clone();
                            cx.emit(TerminalAiInlineEvent::AppendConversation {
                                project_id: this.project_id.clone(),
                                user_message: user_text_for_done.clone(),
                                quote: quote_for_done.clone(),
                                assistant_reply: final_reply,
                            });
                        }

                        done
                    })
                    .unwrap_or(true);

                if finished {
                    break;
                }
            }
        }));

        cx.notify();
    }

    /// Start a streaming prompt request to the AI model.
    pub fn start_request(&mut self, prompt: String, cx: &mut Context<Self>) {
        self.start_turn(prompt, None, cx);
    }

    /// Stop current streaming generation if active.
    pub fn stop_streaming(&mut self, cx: &mut Context<Self>) {
        self.is_streaming = false;
        self.stream_rx = None;
        self._stream_task = None;
        cx.notify();
    }

    /// Extract bash/sh/shell command blocks or single executable lines from AI markdown output.
    fn extract_commands(text: &str) -> Vec<String> {
        let mut commands = Vec::new();
        let mut in_code_block = false;
        let mut is_shell_block = false;
        let mut current_block = String::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if in_code_block {
                    in_code_block = false;
                    if is_shell_block && !current_block.trim().is_empty() {
                        commands.push(current_block.trim().to_string());
                    }
                    current_block.clear();
                    is_shell_block = false;
                } else {
                    in_code_block = true;
                    let lang = trimmed.trim_start_matches('`').to_lowercase();
                    is_shell_block = lang.is_empty()
                        || lang.contains("bash")
                        || lang.contains("sh")
                        || lang.contains("shell")
                        || lang.contains("zsh");
                }
            } else if in_code_block && is_shell_block {
                if !current_block.is_empty() {
                    current_block.push('\n');
                }
                current_block.push_str(line);
            }
        }

        commands
    }

    pub fn trigger_explain(&mut self, cx: &mut Context<Self>) {
        let user_text = i18n!(cx, "terminal.ai_toolbar_explain");
        self.start_turn(user_text, Some(self.selection_text.clone()), cx);
    }

    pub fn trigger_generate_command(&mut self, cx: &mut Context<Self>) {
        let user_text = i18n!(cx, "terminal.ai_toolbar_gen_command");
        self.start_turn(user_text, Some(self.selection_text.clone()), cx);
    }

    pub fn submit_followup(&mut self, cx: &mut Context<Self>) {
        if self.is_streaming {
            return;
        }

        let text = self.followup_input.read(cx).value().to_string();
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return;
        }

        let inp = self.followup_input.clone();
        cx.defer(move |cx| {
            inp.update(cx, |inp, cx| {
                inp.set_value("", cx);
            });
        });

        self.start_turn(trimmed, None, cx);
    }
}

impl Render for TerminalAiInline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = SemanticPalette::from_context(cx);

        let content = match self.mode {
            InlineAiMode::Toolbar => self.render_toolbar(window, cx, &p).into_any_element(),
            InlineAiMode::Popover => self.render_popover(window, cx, &p).into_any_element(),
        };

        let pos = match self.mode {
            InlineAiMode::Toolbar => point(self.position.x, (self.position.y - px(38.0)).max(px(4.0))),
            InlineAiMode::Popover => point(self.position.x, self.position.y + px(4.0)),
        };

        let drag_overlay = if let Some(drag) = self.resize_drag {
            let cursor = drag.edge.cursor_style();
            let drag_entity = cx.entity().downgrade();
            Some(
                deferred(
                    anchored()
                        .position(point(px(0.0), px(0.0)))
                        .child(
                            canvas(
                                |_bounds, window, _cx| {
                                    window.insert_hitbox(
                                        Bounds::new(point(px(0.0), px(0.0)), window.viewport_size()),
                                        HitboxBehavior::BlockMouseExceptScroll,
                                    )
                                },
                                move |_bounds, hitbox, window, _cx| {
                                    window.set_cursor_style(cursor, &hitbox);
                                    let ent = drag_entity.clone();
                                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                                        if phase != DispatchPhase::Bubble {
                                            return;
                                        }
                                        if let Some(entity) = ent.upgrade() {
                                            entity.update(cx, |this, cx| {
                                                if let Some(drag) = this.resize_drag {
                                                    let viewport = window.viewport_size();
                                                    let start_w = f32::from(drag.start_size.width);
                                                    let start_h = f32::from(drag.start_size.height);
                                                    let start_x = f32::from(drag.start_pos.x);
                                                    let start_y = f32::from(drag.start_pos.y);

                                                    let max_w = (f32::from(viewport.width) - start_x - 4.0)
                                                        .max(MIN_POPOVER_WIDTH);
                                                    let max_h = (f32::from(viewport.height) - start_y - 4.0)
                                                        .max(MIN_POPOVER_HEIGHT);

                                                    let delta_x = f32::from(e.position.x - drag.start_mouse.x);
                                                    let delta_y = f32::from(e.position.y - drag.start_mouse.y);

                                                    let mut new_w = start_w;
                                                    let mut new_h = start_h;
                                                    let mut new_x = start_x;
                                                    let mut new_y = start_y;

                                                    match drag.edge {
                                                        PopoverResizeEdge::Right
                                                        | PopoverResizeEdge::TopRight
                                                        | PopoverResizeEdge::BottomRight => {
                                                            new_w = (start_w + delta_x).clamp(MIN_POPOVER_WIDTH, max_w);
                                                        }
                                                        PopoverResizeEdge::Left
                                                        | PopoverResizeEdge::TopLeft
                                                        | PopoverResizeEdge::BottomLeft => {
                                                            let max_left_w = (start_x + start_w - 4.0).max(MIN_POPOVER_WIDTH);
                                                            let raw_w = (start_w - delta_x).clamp(MIN_POPOVER_WIDTH, max_left_w);
                                                            let actual_dw = raw_w - start_w;
                                                            let clamped_x = (start_x - actual_dw).max(4.0);
                                                            let dw_applied = start_x - clamped_x;
                                                            new_w = start_w + dw_applied;
                                                            new_x = clamped_x;
                                                        }
                                                        _ => {}
                                                    }

                                                    match drag.edge {
                                                        PopoverResizeEdge::Bottom
                                                        | PopoverResizeEdge::BottomLeft
                                                        | PopoverResizeEdge::BottomRight => {
                                                            new_h = (start_h + delta_y).clamp(MIN_POPOVER_HEIGHT, max_h);
                                                        }
                                                        PopoverResizeEdge::Top
                                                        | PopoverResizeEdge::TopLeft
                                                        | PopoverResizeEdge::TopRight => {
                                                            let max_top_h = (start_y + start_h - 4.0).max(MIN_POPOVER_HEIGHT);
                                                            let raw_h = (start_h - delta_y).clamp(MIN_POPOVER_HEIGHT, max_top_h);
                                                            let actual_dh = raw_h - start_h;
                                                            let clamped_y = (start_y - actual_dh).max(4.0);
                                                            let dh_applied = start_y - clamped_y;
                                                            new_h = start_h + dh_applied;
                                                            new_y = clamped_y;
                                                        }
                                                        _ => {}
                                                    }

                                                    if this.popover_size.width != px(new_w)
                                                        || this.popover_size.height != px(new_h)
                                                        || this.position.x != px(new_x)
                                                        || this.position.y != px(new_y)
                                                    {
                                                        this.popover_size = size(px(new_w), px(new_h));
                                                        this.position = point(px(new_x), px(new_y));
                                                        cx.notify();
                                                    }
                                                }
                                            });
                                        }
                                    });

                                    let ent_up = drag_entity.clone();
                                    window.on_mouse_event(move |e: &MouseUpEvent, phase, _window, cx| {
                                        if phase != DispatchPhase::Bubble {
                                            return;
                                        }
                                        if e.button != MouseButton::Left {
                                            return;
                                        }
                                        if let Some(entity) = ent_up.upgrade() {
                                            entity.update(cx, |this, cx| {
                                                if this.resize_drag.take().is_some() {
                                                    cx.notify();
                                                }
                                            });
                                        }
                                    });
                                },
                            )
                            .w(window.viewport_size().width)
                            .h(window.viewport_size().height),
                        ),
                ),
            )
        } else {
            None
        };

        div()
            .children(drag_overlay)
            .child(
                deferred(
                    anchored()
                        .position(pos)
                        .anchor(gpui::Anchor::TopLeft)
                        .snap_to_window()
                        .child(content),
                ),
            )
    }
}

impl TerminalAiInline {
    fn render_toolbar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
        _p: &SemanticPalette,
    ) -> impl IntoElement {
        let explain_tip: &'static str =
            Box::leak(i18n!(cx, "terminal.ai_toolbar_explain_tip").into_boxed_str());

        let gen_cmd_tip: &'static str =
            Box::leak(i18n!(cx, "terminal.ai_toolbar_gen_command_tip").into_boxed_str());

        let to_panel_tip: &'static str =
            Box::leak(i18n!(cx, "terminal.ai_toolbar_send_to_side_panel_tip").into_boxed_str());

        let close_tip: &'static str =
            Box::leak(i18n!(cx, "common.close").into_boxed_str());

        let quote = self.selection_text.clone();

        capsule_toolbar_container("terminal-ai-toolbar", cx)
            .child(
                capsule_icon_button("ai-tb-explain", AppIcon::AiAssistant, cx)
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(explain_tip)).into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.trigger_explain(cx);
                    })),
            )
            .child(
                capsule_icon_button("ai-tb-gen-cmd", AppIcon::Terminal, cx)
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(gen_cmd_tip)).into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.trigger_generate_command(cx);
                    })),
            )
            .child(
                capsule_icon_button("ai-tb-to-panel", AppIcon::ExternalLink, cx)
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(to_panel_tip)).into())
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(TerminalAiInlineEvent::ContinueInSidePanel {
                            quote: quote.clone(),
                            reply: String::new(),
                        });
                        cx.emit(TerminalAiInlineEvent::Close);
                    })),
            )
            .child(capsule_divider(cx))
            .child(
                div()
                    .w(px(150.0))
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .on_key_down(cx.listener(|_, event: &KeyDownEvent, _window, cx| {
                        if event.keystroke.key.as_str() == "escape" {
                            cx.emit(TerminalAiInlineEvent::Close);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        SimpleInput::new(&self.toolbar_input)
                            .compact()
                            .h(px(22.0))
                            .text_size(ui_text_md(cx)),
                    ),
            )
            .child(
                capsule_icon_button("ai-tb-close", AppIcon::Close, cx)
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(close_tip)).into())
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(TerminalAiInlineEvent::Close);
                    })),
            )
    }

    fn render_resize_handle(
        &self,
        edge: PopoverResizeEdge,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let cursor = edge.cursor_style();
        let handle = div()
            .absolute()
            .cursor(cursor)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    this.resize_drag = Some(PopoverResizeDrag {
                        edge,
                        start_mouse: e.position,
                        start_size: this.popover_size,
                        start_pos: this.position,
                    });
                    cx.notify();
                }),
            );

        match edge {
            PopoverResizeEdge::Top => handle
                .top(px(0.0))
                .left(px(12.0))
                .right(px(12.0))
                .h(px(6.0)),
            PopoverResizeEdge::Bottom => handle
                .bottom(px(0.0))
                .left(px(12.0))
                .right(px(12.0))
                .h(px(6.0)),
            PopoverResizeEdge::Left => handle
                .left(px(0.0))
                .top(px(12.0))
                .bottom(px(12.0))
                .w(px(6.0)),
            PopoverResizeEdge::Right => handle
                .right(px(0.0))
                .top(px(12.0))
                .bottom(px(12.0))
                .w(px(6.0)),
            PopoverResizeEdge::TopLeft => handle
                .top(px(0.0))
                .left(px(0.0))
                .w(px(12.0))
                .h(px(12.0)),
            PopoverResizeEdge::TopRight => handle
                .top(px(0.0))
                .right(px(0.0))
                .w(px(12.0))
                .h(px(12.0)),
            PopoverResizeEdge::BottomLeft => handle
                .bottom(px(0.0))
                .left(px(0.0))
                .w(px(12.0))
                .h(px(12.0)),
            PopoverResizeEdge::BottomRight => handle
                .bottom(px(0.0))
                .right(px(0.0))
                .w(px(12.0))
                .h(px(12.0)),
        }
    }

    fn render_popover(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
        p: &SemanticPalette,
    ) -> impl IntoElement {
        let t = theme(cx);
        let elapsed = self.enter_start.elapsed();
        let t_norm = (elapsed.as_secs_f32() / DURATION_PANEL.as_secs_f32()).clamp(0.0, 1.0);
        let motion_progress = ease_out_cubic(t_norm);

        let last_user_msg = self.messages.iter().rfind(|m| m.is_user);
        let quote = last_user_msg
            .and_then(|m| m.quote.clone())
            .unwrap_or_else(|| self.selection_text.clone());
        let reply = self.reply_text.clone();

        let terminal_id = self.terminal_id.clone();
        let tid_for_run = self.terminal_id.clone();

        let to_panel_tip: &'static str =
            Box::leak(i18n!(cx, "terminal.inline_ai_continue_in_side_panel").into_boxed_str());
        let close_tip: &'static str =
            Box::leak(i18n!(cx, "terminal.inline_ai_close").into_boxed_str());

        let is_streaming = self.is_streaming;
        let btn_icon = if is_streaming {
            AppIcon::Stop
        } else {
            AppIcon::Send
        };
        let btn_id = if is_streaming {
            "btn-followup-stop"
        } else {
            "btn-followup-send"
        };
        let btn_tip: &'static str = if is_streaming {
            Box::leak(i18n!(cx, "ai_assistant.stop").into_boxed_str())
        } else {
            Box::leak(i18n!(cx, "ai_assistant.send").into_boxed_str())
        };

        div()
            .id("terminal-ai-popover")
            .occlude()
            .relative()
            .w(self.popover_size.width)
            .h(self.popover_size.height)
            .bg(p.surface_overlay)
            .border_1()
            .border_color(p.border_subtle)
            .rounded(RADIUS_MD)
            .shadow(elevation_menu_shadow())
            .flex()
            .flex_col()
            .overflow_hidden()
            .opacity(motion_progress)
            .key_context("TerminalAiInline")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                if cmd_or_ctrl && event.keystroke.key.eq_ignore_ascii_case("c") {
                    if let Some(text) = this.get_active_selection_text() {
                        if !text.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            if let Some(ref sel) = this.active_selection {
                                this.copied_msg_index = Some(sel.msg_index);
                                cx.spawn(async move |this: WeakEntity<Self>, cx| {
                                    smol::Timer::after(Duration::from_millis(2000)).await;
                                    let _ = this.update(cx, |this, cx| {
                                        this.copied_msg_index = None;
                                        cx.notify();
                                    });
                                }).detach();
                            }
                            cx.notify();
                            cx.stop_propagation();
                            return;
                        }
                    }
                }
                if event.keystroke.key.as_str() == "escape" {
                    cx.emit(TerminalAiInlineEvent::Close);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                this.focus_handle.focus(window, cx);
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, window, cx| {
                this.focus_handle.focus(window, cx);
                if let Some(text) = this.get_active_selection_text() {
                    if !text.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        if let Some(ref sel) = this.active_selection {
                            this.copied_msg_index = Some(sel.msg_index);
                            cx.spawn(async move |this: WeakEntity<Self>, cx| {
                                smol::Timer::after(Duration::from_millis(2000)).await;
                                let _ = this.update(cx, |this, cx| {
                                    this.copied_msg_index = None;
                                    cx.notify();
                                });
                            }).detach();
                        }
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                }
                cx.stop_propagation();
            }))
            .on_scroll_wheel(|_, _, cx| {
                cx.stop_propagation();
            })
            // --- Header ---
            .child(
                div()
                    .h(px(34.0))
                    .flex_shrink_0()
                    .px(SPACE_SM)
                    .bg(p.surface_header)
                    .rounded_t(RADIUS_MD)
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(SPACE_XS)
                            .child(AppIcon::AiAssistant.size(px(14.0)).text_color(p.text_primary))
                            .child(
                                div()
                                    .w(px(130.0))
                                    .child(Select::new(&self.model_select)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(SPACE_XS)
                            .child(
                                div()
                                    .id("btn-popover-to-panel")
                                    .cursor_pointer()
                                    .p(px(3.0))
                                    .rounded(RADIUS_SM)
                                    .hover(|s| s.bg(p.surface_hover))
                                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(to_panel_tip)).into())
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(TerminalAiInlineEvent::ContinueInSidePanel {
                                            quote: quote.clone(),
                                            reply: reply.clone(),
                                        });
                                        cx.emit(TerminalAiInlineEvent::Close);
                                    }))
                                    .child(
                                        AppIcon::ExternalLink
                                            .size(px(13.0))
                                            .text_color(p.text_secondary),
                                    ),
                            )
                            .child(
                                div()
                                    .id("btn-popover-close")
                                    .cursor_pointer()
                                    .p(px(3.0))
                                    .rounded(RADIUS_SM)
                                    .hover(|s| s.bg(p.surface_hover))
                                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(close_tip)).into())
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(TerminalAiInlineEvent::Close);
                                    }))
                                    .child(AppIcon::Close.size(px(13.0)).text_color(p.text_secondary)),
                            ),
                    ),
            )
            // --- Body ---
            .child(
                div()
                    .id("terminal-ai-popover-body")
                    .w_full()
                    .min_w(px(0.0))
                    .flex_1()
                    .min_h(px(80.0))
                    .p(SPACE_SM)
                    .overflow_x_hidden()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .child(
                        if self.has_no_model && self.messages.is_empty() {
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(SPACE_SM)
                                .py(px(16.0))
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(p.text_muted)
                                        .child(i18n!(cx, "terminal.inline_ai_no_model_configured")),
                                )
                                .child(
                                    div()
                                        .id("btn-popover-open-settings")
                                        .cursor_pointer()
                                        .px(SPACE_SM)
                                        .py(px(4.0))
                                        .rounded(RADIUS_MD)
                                        .bg(p.surface_accent)
                                        .text_size(px(11.5))
                                        .text_color(p.text_on_accent)
                                        .on_click(cx.listener(|_, _, _, cx| {
                                            cx.emit(TerminalAiInlineEvent::OpenSettings);
                                            cx.emit(TerminalAiInlineEvent::Close);
                                        }))
                                        .child(i18n!(cx, "terminal.inline_ai_open_settings")),
                                )
                                .into_any_element()
                        } else if let Some(ref err) = self.error_message {
                            if self.messages.is_empty() {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(SPACE_XS)
                                    .p(SPACE_SM)
                                    .rounded(RADIUS_MD)
                                    .bg(p.surface_danger)
                                    .text_size(px(12.0))
                                    .text_color(p.text_primary)
                                    .child(format!("Error: {}", err))
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            }
                        } else {
                            div().into_any_element()
                        },
                    )
                    .child({
                        let active_selection = self.active_selection.clone();
                        let inline_entity = cx.entity().clone();
                        v_flex()
                            .w_full()
                            .min_w(px(0.0))
                            .gap(SPACE_MD)
                            .children(
                                self.messages.iter().enumerate().map(|(msg_idx, msg)| {
                                    if msg.is_user {
                                        // User message: Antigravity unified card with embedded quote capsule
                                        let mut card_children: Vec<AnyElement> = Vec::new();
                                        if let Some(q) = msg.quote.as_ref().filter(|s| !s.trim().is_empty()) {
                                            let q_text = q.clone();
                                                card_children.push(
                                                    div()
                                                        .w_full()
                                                        .min_w(px(0.0))
                                                        .p(SPACE_XS)
                                                        .rounded(RADIUS_SM)
                                                        .bg(p.surface_raised)
                                                        .border_l_2()
                                                        .border_color(p.border_active)
                                                        .flex()
                                                        .flex_col()
                                                        .gap(px(2.0))
                                                        .child(
                                                            h_flex()
                                                                .items_center()
                                                                .gap(SPACE_XS)
                                                                .child(AppIcon::Terminal.size(px(11.0)).text_color(p.text_muted))
                                                                .child(
                                                                    div()
                                                                        .text_size(px(10.5))
                                                                        .text_color(p.text_muted)
                                                                        .child(i18n!(cx, "ai_assistant.quote"))
                                                                )
                                                        )
                                                        .child(
                                                            div()
                                                                .id(SharedString::from(format!("user-quote-{}", msg_idx)))
                                                                .w_full()
                                                                .min_w(px(0.0))
                                                                .max_h(px(72.0))
                                                                .overflow_y_scroll()
                                                                .text_size(px(11.0))
                                                                .text_color(p.text_secondary)
                                                                .child(q_text)
                                                        )
                                                        .into_any_element()
                                                );
                                        }
                                        card_children.push(
                                            div()
                                                .w_full()
                                                .min_w(px(0.0))
                                                .text_size(ui_text_md(cx))
                                                .text_color(p.text_primary)
                                                .child(msg.text.clone())
                                                .into_any_element()
                                        );

                                        div()
                                            .w_full()
                                            .min_w(px(0.0))
                                            .p(SPACE_SM)
                                            .rounded(RADIUS_LG)
                                            .bg(p.surface_card)
                                            .border_1()
                                            .border_color(p.border_subtle)
                                            .flex()
                                            .flex_col()
                                            .gap(SPACE_XS)
                                            .children(card_children)
                                            .into_any_element()
                                    } else {
                                        // Assistant reply: 100% transparent background, NO border, full width
                                        let cmd_tid_ins = terminal_id.clone();
                                        let cmd_tid_run = tid_for_run.clone();
                                        let copy_text = msg.text.clone();
                                        let is_copied = self.copied_msg_index == Some(msg_idx);
                                        let copy_label: &'static str = if is_copied {
                                            Box::leak(i18n!(cx, "ai_assistant.copied").into_boxed_str())
                                        } else {
                                            Box::leak(i18n!(cx, "common.copy").into_boxed_str())
                                        };

                                        div()
                                            .w_full()
                                            .min_w(px(0.0))
                                            .flex()
                                            .flex_col()
                                            .gap(SPACE_XS)
                                            .py(SPACE_XS)
                                            .when(msg.is_streaming && msg.text.is_empty(), |d| {
                                                d.child(loading_indicator(&t, cx, self.animation_frame))
                                            })
                                            .when(!msg.text.is_empty(), |d| {
                                                let active_sel = active_selection
                                                    .as_ref()
                                                    .filter(|s| s.msg_index == msg_idx);
                                                let sel_range = active_sel.map(|s| (s.start, s.end));
                                                let inline_for_sel = inline_entity.clone();
                                                let md_el = MarkdownElement::new(
                                                    ElementId::from(format!("inline-md-{}", msg_idx)),
                                                    &msg.text,
                                                )
                                                .selection(sel_range)
                                                .on_selection_event(move |ev, window, cx| {
                                                    inline_for_sel.update(cx, |this, cx| {
                                                        this.focus_handle.focus(window, cx);
                                                        if let MarkdownSelectionEvent::Start { .. } = &ev {
                                                            this.followup_input.update(cx, |inp, cx| inp.clear_selection(cx));
                                                        }
                                                        this.handle_selection_event(msg_idx, ev, cx);
                                                    });
                                                })
                                                .on_url_click(move |url, _window, cx| {
                                                    cx.open_url(url);
                                                });
                                                d.child(md_el)
                                            })
                                            .children(
                                                msg.extracted_commands.iter().enumerate().map(|(c_idx, cmd)| {
                                                    let cmd_to_insert = cmd.clone();
                                                    let cmd_to_run = cmd.clone();
                                                    let cmd_to_copy = cmd.clone();
                                                    let tid_ins = cmd_tid_ins.clone();
                                                    let tid_run = cmd_tid_run.clone();

                                                    let ins_tip: &'static str = Box::leak(
                                                        i18n!(cx, "terminal.inline_ai_insert_terminal_tip").into_boxed_str(),
                                                    );
                                                    let run_tip: &'static str = Box::leak(
                                                        i18n!(cx, "terminal.inline_ai_run_terminal_tip").into_boxed_str(),
                                                    );
                                                    let copy_tip: &'static str = Box::leak(
                                                        i18n!(cx, "terminal.inline_ai_copy_command").into_boxed_str(),
                                                    );

                                                    div()
                                                        .w_full()
                                                        .min_w(px(0.0))
                                                        .mt(SPACE_XS)
                                                        .p(SPACE_XS)
                                                        .bg(p.surface_raised)
                                                        .border_1()
                                                        .border_color(p.border_subtle)
                                                        .rounded(RADIUS_MD)
                                                        .flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .gap(SPACE_XS)
                                                        .child(
                                                            h_flex()
                                                                .items_center()
                                                                .gap(SPACE_XS)
                                                                .flex_1()
                                                                .min_w(px(0.0))
                                                                .overflow_hidden()
                                                                .child(AppIcon::Terminal.size(px(12.0)).text_color(p.text_muted))
                                                                .child(
                                                                    div()
                                                                        .flex_1()
                                                                        .min_w(px(0.0))
                                                                        .truncate()
                                                                        .text_size(px(11.0))
                                                                        .text_color(p.text_primary)
                                                                        .child(cmd.clone())
                                                                )
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_shrink_0()
                                                                .flex()
                                                                .items_center()
                                                                .gap(px(4.0))
                                                                .child(
                                                                    div()
                                                                        .id(SharedString::from(format!("cmd-ins-{}-{}", msg_idx, c_idx)))
                                                                        .cursor_pointer()
                                                                        .flex()
                                                                        .items_center()
                                                                        .gap(px(2.0))
                                                                        .px(px(6.0))
                                                                        .h(px(22.0))
                                                                        .rounded(RADIUS_SM)
                                                                        .bg(p.surface_hover)
                                                                        .hover(|s| s.bg(p.surface_selection))
                                                                        .text_size(px(11.0))
                                                                        .text_color(p.text_secondary)
                                                                        .tooltip(move |_, cx| cx.new(|_| Tooltip::new(ins_tip)).into())
                                                                        .on_click(cx.listener(move |_, _, _, cx| {
                                                                            cx.emit(TerminalAiInlineEvent::InsertToTerminal {
                                                                                terminal_id: tid_ins.clone(),
                                                                                command: cmd_to_insert.clone(),
                                                                            });
                                                                        }))
                                                                        .child(i18n!(cx, "terminal.inline_ai_insert_terminal")),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .id(SharedString::from(format!("cmd-run-{}-{}", msg_idx, c_idx)))
                                                                        .cursor_pointer()
                                                                        .flex()
                                                                        .items_center()
                                                                        .gap(px(2.0))
                                                                        .px(px(6.0))
                                                                        .h(px(22.0))
                                                                        .rounded(RADIUS_SM)
                                                                        .bg(p.surface_accent)
                                                                        .text_size(px(11.0))
                                                                        .text_color(p.text_on_accent)
                                                                        .tooltip(move |_, cx| cx.new(|_| Tooltip::new(run_tip)).into())
                                                                        .on_click(cx.listener(move |_, _, _, cx| {
                                                                            cx.emit(TerminalAiInlineEvent::RunInTerminal {
                                                                                terminal_id: tid_run.clone(),
                                                                                command: cmd_to_run.clone(),
                                                                            });
                                                                        }))
                                                                        .child(AppIcon::Play.size(px(10.0)).text_color(p.text_on_accent))
                                                                        .child(i18n!(cx, "terminal.inline_ai_run_terminal")),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .id(SharedString::from(format!("cmd-cpy-{}-{}", msg_idx, c_idx)))
                                                                        .cursor_pointer()
                                                                        .p(px(4.0))
                                                                        .rounded(RADIUS_SM)
                                                                        .hover(|s| s.bg(p.surface_hover))
                                                                        .tooltip(move |_, cx| cx.new(|_| Tooltip::new(copy_tip)).into())
                                                                        .on_click(cx.listener(move |_, _, _, cx| {
                                                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                                                cmd_to_copy.clone(),
                                                                            ));
                                                                        }))
                                                                        .child(
                                                                            AppIcon::Copy
                                                                                .size(px(12.0))
                                                                                .text_color(p.text_secondary),
                                                                        ),
                                                                ),
                                                        )
                                                })
                                            )
                                            .when(!msg.is_streaming && !msg.text.is_empty(), |d| {
                                                d.child(
                                                    h_flex()
                                                        .justify_start()
                                                        .pt(px(2.0))
                                                        .child(
                                                            div()
                                                                .id(SharedString::from(format!("copy-reply-{}", msg_idx)))
                                                                .cursor_pointer()
                                                                .flex()
                                                                .items_center()
                                                                .gap(px(3.0))
                                                                .px(px(6.0))
                                                                .h(px(20.0))
                                                                .rounded(RADIUS_SM)
                                                                .hover(|s| s.bg(p.surface_hover))
                                                                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(copy_label)).into())
                                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                                    cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                                                                    this.copied_msg_index = Some(msg_idx);
                                                                    cx.notify();
                                                                }))
                                                                .child(
                                                                    if is_copied {
                                                                        AppIcon::Check.size(px(11.0)).text_color(p.surface_accent)
                                                                    } else {
                                                                        AppIcon::Copy.size(px(11.0)).text_color(p.text_muted)
                                                                    }
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(px(10.5))
                                                                        .text_color(if is_copied { p.surface_accent } else { p.text_muted })
                                                                        .child(copy_label)
                                                                )
                                                        )
                                                )
                                            })
                                            .into_any_element()
                                    }
                                })
                            )
                    })
            )
            // --- Footer: Followup Input ---
            .child(
                div()
                    .p(SPACE_XS)
                    .flex_shrink_0()
                    .bg(p.surface_header)
                    .rounded_b(RADIUS_MD)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .child(
                        div()
                            .w_full()
                            .rounded(RADIUS_MD)
                            .border_1()
                            .border_color(p.border_subtle)
                            .bg(p.surface_card)
                            .p(SPACE_XS)
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .w_full()
                                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                                        if event.keystroke.key.as_str() == "escape" {
                                            cx.emit(TerminalAiInlineEvent::Close);
                                            cx.stop_propagation();
                                            return;
                                        }
                                        let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                                        if cmd_or_ctrl && event.keystroke.key.eq_ignore_ascii_case("c") {
                                            if let Some(text) = this.get_active_selection_text() {
                                                if !text.is_empty() {
                                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                                    if let Some(ref sel) = this.active_selection {
                                                        this.copied_msg_index = Some(sel.msg_index);
                                                        cx.spawn(async move |this: WeakEntity<Self>, cx| {
                                                            smol::Timer::after(Duration::from_millis(2000)).await;
                                                            let _ = this.update(cx, |this, cx| {
                                                                this.copied_msg_index = None;
                                                                cx.notify();
                                                            });
                                                        }).detach();
                                                    }
                                                    cx.notify();
                                                    cx.stop_propagation();
                                                    return;
                                                }
                                            }
                                        }
                                    }))
                                    .child(SimpleInput::new(&self.followup_input).borderless(true)),
                            )
                            .child(
                                h_flex()
                                    .justify_end()
                                    .pt(px(2.0))
                                    .child(
                                        icon_button_sized(
                                            btn_id,
                                            btn_icon,
                                            24.0,
                                            14.0,
                                            &t,
                                        )
                                        .tooltip(move |_, cx| cx.new(|_| Tooltip::new(btn_tip)).into())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if this.is_streaming {
                                                this.stop_streaming(cx);
                                            } else {
                                                this.submit_followup(cx);
                                            }
                                        })),
                                    ),
                            ),
                    ),
            )
            // --- Edge Resize Handles ---
            .child(self.render_resize_handle(PopoverResizeEdge::Top, cx))
            .child(self.render_resize_handle(PopoverResizeEdge::Bottom, cx))
            .child(self.render_resize_handle(PopoverResizeEdge::Left, cx))
            .child(self.render_resize_handle(PopoverResizeEdge::Right, cx))
            .child(self.render_resize_handle(PopoverResizeEdge::TopLeft, cx))
            .child(self.render_resize_handle(PopoverResizeEdge::TopRight, cx))
            .child(self.render_resize_handle(PopoverResizeEdge::BottomLeft, cx))
            .child(self.render_resize_handle(PopoverResizeEdge::BottomRight, cx))
    }
}

/// 加载状态指示器：三个错相位呼吸跳动的圆点 + 文案，直观表达「等待回复中」（与右侧 AI 助手面板保持一致）。
fn loading_indicator(t: &ThemeColors, cx: &App, frame: u64) -> impl IntoElement {
    let label = i18n!(cx, "ai_assistant.thinking");
    let dots = (0..3).map(|i| {
        // 每个圆点相位错开，形成波浪式呼吸效果。
        let phase = (frame + i * 10) % 30;
        let wave = (phase as f32 / 30.0 * std::f32::consts::PI * 2.0).sin();
        let opacity = 0.35 + 0.65 * ((wave + 1.0) / 2.0);
        div()
            .w(px(7.0))
            .h(px(7.0))
            .rounded(px(3.5))
            .bg(rgb(t.accent))
            .opacity(opacity)
    });
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .py(SPACE_SM)
        .child(h_flex().gap(px(5.0)).items_center().children(dots))
        .child(
            div()
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_muted))
                .child(label),
        )
}
