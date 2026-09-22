//! Terminal inline AI interaction component.
//!
//! Provides a two-stage inline AI experience directly inside the terminal viewport:
//! 1. Floating Capsule Toolbar on text selection (explain, generate command, send to side panel, mini-ask).
//! 2. Inline Popover displaying streaming response, model switcher dropdown, command execution cards,
//!    follow-up conversation, and seamless escalation to the right AI assistant dock panel.

use gpui::*;
use std::collections::HashSet;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use velowork_ai::{
    classify_selection_intent, provider, render_inline_prompt, PromptScene, StreamChunk,
    TerminalContextSnapshot,
};
use velowork_i18n::i18n;
use velowork_markdown::{
    find_line_boundaries, find_word_boundaries, MarkdownSelectionEvent,
};
use velowork_ui::capsule_toolbar::{
    capsule_action_button, capsule_divider, capsule_icon_button, capsule_toolbar_container,
};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::icon_button::icon_button_sized;
use velowork_ui::motion::{ease_out_cubic, DURATION_PANEL};
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::select::{Select, SelectEvent, SelectOption, SelectPlacement, SelectState};
use velowork_ui::simple_input::{InputEvent, SimpleInput, SimpleInputState};
use velowork_ui::theme::theme;
use velowork_ui::tokens::{
    elevation_menu_shadow, ui_text_md, RADIUS_MD,
    SPACE_MD, SPACE_SM, SPACE_XS,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex, ControlSize};

use crate::views::ai::commands::extract_commands;
use crate::views::ai::message_view::{render_chat_message, ChatMessageCallbacks};
use crate::views::ai::types::ChatMessage;

/// Minimum dimensions for the resizable inline AI popover.
const MIN_POPOVER_WIDTH: f32 = 360.0;
const MIN_POPOVER_HEIGHT: f32 = 260.0;
const DEFAULT_INPUT_HEIGHT: f32 = 96.0;
const MIN_INPUT_HEIGHT: f32 = 72.0;
const MIN_CHAT_HEIGHT: f32 = 140.0;

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

/// Active dragging state when resizing the followup input card height.
#[derive(Clone, Copy, Debug)]
pub struct InputResizeDrag {
    pub start_mouse_y: Pixels,
    pub start_height: Pixels,
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
        project_id: String,
        quote: String,
        messages: Vec<ChatMessage>,
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
    pub input_height: Pixels,
    pub input_resize_drag: Option<InputResizeDrag>,
    pub messages: Vec<ChatMessage>,
    pub expanded_quotes: HashSet<usize>,
    pub copied_msg_index: Option<usize>,
    pub scroll_handle: ScrollHandle,
    pub focus_handle: FocusHandle,
    pub animation_frame: u64,
    pub cached_snapshot: Option<TerminalContextSnapshot>,
}

impl TerminalAiInline {
    /// Create a new inline AI floating toolbar.
    pub fn new_toolbar(
        terminal_id: String,
        project_id: String,
        position: Point<Pixels>,
        selection_text: String,
        overlay_registry: Option<Entity<OverlayRegistry>>,
        snapshot: Option<TerminalContextSnapshot>,
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
                .fill_height(true)
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
            input_height: px(DEFAULT_INPUT_HEIGHT),
            input_resize_drag: None,
            messages: Vec::new(),
            expanded_quotes: HashSet::new(),
            copied_msg_index: None,
            scroll_handle: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            animation_frame: 0,
            cached_snapshot: snapshot,
        }
    }

    /// Focus the input box appropriate for the current mode.
    pub fn focus_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            InlineAiMode::Toolbar => {
                self.toolbar_input.update(cx, |inp, cx| inp.focus(window, cx));
            }
            InlineAiMode::Popover => {
                self.followup_input.update(cx, |inp, cx| inp.focus(window, cx));
            }
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
        snapshot: Option<TerminalContextSnapshot>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut inline = Self::new_toolbar(
            terminal_id,
            project_id,
            position,
            selection_text.clone(),
            overlay_registry,
            snapshot,
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
                .placement(SelectPlacement::Above)
                .ghost(true)
                .size(ControlSize::Compact)
                .text_size(ui_text_md(cx));
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
        self.messages.push(ChatMessage::new_user(user_text.clone(), quote.clone(), Vec::new()));

        // Push Assistant Message
        self.reply_text.clear();
        self.extracted_commands.clear();
        self.is_streaming = true;

        self.messages.push(ChatMessage::new_assistant(String::new(), true));
        self.scroll_handle.scroll_to_bottom();

        // Prepare multi-turn messages for API (using unified api_content)
        let mut api_messages = Vec::new();
        for msg in &self.messages {
            if msg.streaming && msg.text.is_empty() {
                continue;
            }
            if msg.is_user {
                api_messages.push((msg.api_content(), true));
            } else if !msg.text.is_empty() {
                api_messages.push((msg.text.clone(), false));
            }
        }

        // Abort any active streaming task before initiating a new request.
        self._stream_task = None;
        self.stream_rx = None;

        let scene = if quote.is_some() {
            PromptScene::ErrorDiagnosis
        } else {
            PromptScene::CommandGen
        };

        if self.cached_snapshot.is_none() {
            self.cached_snapshot = Some(TerminalContextSnapshot {
                terminal_id: self.terminal_id.clone(),
                selected_text: quote.clone(),
                os: std::env::consts::OS.to_string(),
                shell: if cfg!(target_os = "windows") { "powershell".to_string() } else { "bash".to_string() },
                ..Default::default()
            });
        }

        let system_prompt = if let Some(ref snapshot) = self.cached_snapshot {
            render_inline_prompt(scene, snapshot)
        } else {
            String::new()
        };

        let rx = provider::stream_api_reply_with_system(
            &cfg.base_url,
            &cfg.api_key,
            &cfg.model_id,
            if system_prompt.is_empty() { None } else { Some(&system_prompt) },
            &api_messages,
        );
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

                        loop {
                            match rx.try_recv() {
                                Ok(chunk) => match chunk {
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
                                            last.streaming = false;
                                            if last.text.is_empty() {
                                                let err_str = format!("{}: {}", i18n!(cx, "ai_assistant.error"), i18n!(cx, "ai_assistant.empty_response"));
                                                this.error_message = Some(err_str.clone());
                                                last.text = err_str.clone();
                                                this.reply_text = err_str;
                                            }
                                        }
                                        done = true;
                                        updated = true;
                                        break;
                                    }
                                    StreamChunk::Error(err) => {
                                        let err_str = err.to_string();
                                        this.error_message = Some(err_str.clone());
                                        this.is_streaming = false;
                                        if let Some(last) = this.messages.last_mut().filter(|m| !m.is_user) {
                                            last.streaming = false;
                                            let full_err = format!("{}: {}", i18n!(cx, "ai_assistant.error"), err_str);
                                            last.text = full_err.clone();
                                            this.reply_text = full_err;
                                        }
                                        done = true;
                                        updated = true;
                                        break;
                                    }
                                    _ => {}
                                },
                                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                    this.is_streaming = false;
                                    if let Some(last) = this.messages.last_mut().filter(|m| !m.is_user) {
                                        last.streaming = false;
                                        if last.text.is_empty() {
                                            let err_str = format!("{}: {}", i18n!(cx, "ai_assistant.error"), i18n!(cx, "ai_assistant.network_interrupted"));
                                            this.error_message = Some(err_str.clone());
                                            last.text = err_str.clone();
                                            this.reply_text = err_str;
                                        }
                                    }
                                    done = true;
                                    updated = true;
                                    break;
                                }
                            }
                        }

                        if updated {
                            let text = this.reply_text.clone();
                            this.extracted_commands = extract_commands(&text);
                            this.scroll_handle.scroll_to_bottom();
                            cx.notify();
                        } else if this.is_streaming && this.reply_text.is_empty() {
                            // 思考等待中：每帧通知重绘以驱动圆点呼吸波浪动效
                            cx.notify();
                        }

                        if done {
                            this.scroll_handle.scroll_to_bottom();
                            let entity = cx.entity().clone();
                            cx.defer(move |cx| {
                                entity.update(cx, |this, _cx| {
                                    this.scroll_handle.scroll_to_bottom();
                                });
                            });
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


    pub fn trigger_explain(&mut self, cx: &mut Context<Self>) {
        let user_text = i18n!(cx, "terminal.ai_toolbar_explain");
        self.start_turn(user_text, Some(self.selection_text.clone()), cx);
    }

    pub fn trigger_search_in_browser(&mut self, cx: &mut Context<Self>) {
        let query = self.selection_text.trim();
        if !query.is_empty() {
            let settings = crate::settings::settings_entity(cx).read(cx).settings.clone();
            let url = if let Some(engine) = settings.search_engines.iter().find(|e| e.enabled) {
                engine.build_url(query)
            } else {
                velowork_workspace::settings::SearchEngineConfig::new(
                    "Google".to_string(),
                    "https://www.google.com/search?q=%s".to_string(),
                    String::new(),
                )
                .build_url(query)
            };
            cx.open_url(&url);
        }
        cx.emit(TerminalAiInlineEvent::Close);
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
            InlineAiMode::Toolbar => point(self.position.x, (self.position.y - px(42.0)).max(px(4.0))),
            InlineAiMode::Popover => point(self.position.x, self.position.y + px(4.0)),
        };

        let is_dragging = self.resize_drag.is_some() || self.input_resize_drag.is_some();
        let drag_overlay = if is_dragging {
            let cursor = if let Some(drag) = self.resize_drag {
                drag.edge.cursor_style()
            } else {
                CursorStyle::ResizeUpDown
            };
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

                                                    let max_input = (new_h - MIN_CHAT_HEIGHT).max(MIN_INPUT_HEIGHT);
                                                    if f32::from(this.input_height) > max_input {
                                                        this.input_height = px(max_input);
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
                                                } else if let Some(drag) = this.input_resize_drag {
                                                    let delta_y = f32::from(e.position.y - drag.start_mouse_y);
                                                    let max_input = (f32::from(this.popover_size.height) - MIN_CHAT_HEIGHT).max(MIN_INPUT_HEIGHT);
                                                    let new_h = (f32::from(drag.start_height) - delta_y).clamp(MIN_INPUT_HEIGHT, max_input);
                                                    if this.input_height != px(new_h) {
                                                        this.input_height = px(new_h);
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
                                                if this.resize_drag.take().is_some() || this.input_resize_drag.take().is_some() {
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
        let last_exit_code = self.cached_snapshot.as_ref().and_then(|s| {
            s.active_sessions
                .iter()
                .find(|sess| sess.terminal_id == self.terminal_id)
                .and_then(|sess| sess.last_exit_code)
        });

        let rec = classify_selection_intent(&self.selection_text, last_exit_code);

        let smart_icon = match rec.icon_name {
            "terminal" => AppIcon::Terminal,
            "external_link" => AppIcon::ExternalLink,
            "file_text" => AppIcon::File,
            "sparkle" => AppIcon::QuickCommand,
            _ => AppIcon::AiAssistant,
        };

        let smart_label = i18n!(cx, rec.label_key);
        let smart_tip: &'static str = Box::leak(i18n!(cx, rec.label_key).into_boxed_str());
        let prompt_text = rec.prompt_text.clone();

        let explain_tip: &'static str =
            Box::leak(i18n!(cx, "terminal.ai_toolbar_explain_tip").into_boxed_str());

        let search_tip: &'static str =
            Box::leak(i18n!(cx, "terminal.ai_toolbar_search_tip").into_boxed_str());

        let to_panel_tip: &'static str =
            Box::leak(i18n!(cx, "terminal.ai_toolbar_send_to_side_panel_tip").into_boxed_str());

        let close_tip: &'static str =
            Box::leak(i18n!(cx, "common.close").into_boxed_str());

        capsule_toolbar_container("terminal-ai-toolbar", cx)
            .child(
                capsule_action_button("ai-tb-smart-chip", smart_icon, smart_label, cx)
                    .bg(_p.surface_accent.opacity(0.12))
                    .text_color(_p.text_primary)
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(smart_tip)).into())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.start_turn(prompt_text.clone(), Some(this.selection_text.clone()), cx);
                    })),
            )
            .child(capsule_divider(cx))
            .child(
                capsule_icon_button("ai-tb-explain", AppIcon::AiAssistant, cx)
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(explain_tip)).into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.trigger_explain(cx);
                    })),
            )
            .child(
                capsule_icon_button("ai-tb-search", AppIcon::Search, cx)
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(search_tip)).into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.trigger_search_in_browser(cx);
                    })),
            )
            .child(
                capsule_icon_button("ai-tb-to-panel", AppIcon::ExternalLink, cx)
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(to_panel_tip)).into())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.emit(TerminalAiInlineEvent::ContinueInSidePanel {
                            project_id: this.project_id.clone(),
                            quote: this.selection_text.clone(),
                            messages: Vec::new(),
                        });
                        cx.emit(TerminalAiInlineEvent::Close);
                    })),
            )
            .child(capsule_divider(cx))
            .child(
                div()
                    .w(px(160.0))
                    .h(px(28.0))
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
                            .h(px(28.0))
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
                            .child(AppIcon::AiAssistant.size(px(14.0)).text_color(p.surface_accent))
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text_primary)
                                    .child(i18n!(cx, "ai_assistant.title")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(SPACE_XS)
                            .child(
                                icon_button_sized("btn-popover-to-panel", AppIcon::ExternalLink, 22.0, 13.0, &t)
                                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(to_panel_tip)).into())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.emit(TerminalAiInlineEvent::ContinueInSidePanel {
                                            project_id: this.project_id.clone(),
                                            quote: this.selection_text.clone(),
                                            messages: this.messages.clone(),
                                        });
                                        cx.emit(TerminalAiInlineEvent::Close);
                                    })),
                            )
                            .child(
                                icon_button_sized("btn-popover-close", AppIcon::Close, 22.0, 13.0, &t)
                                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(close_tip)).into())
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(TerminalAiInlineEvent::Close);
                                    })),
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
                                        .text_size(ui_text_md(cx))
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
                                        .text_size(ui_text_md(cx))
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
                                    .text_size(ui_text_md(cx))
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
                        let inline_for_ins = cx.entity().downgrade();
                        let inline_for_run = cx.entity().downgrade();
                        let inline_for_copy = cx.entity().downgrade();
                        let inline_for_sel = cx.entity().downgrade();
                        let inline_for_quote = cx.entity().downgrade();
                        let tid_ins = self.terminal_id.clone();
                        let tid_run = self.terminal_id.clone();

                        let callbacks = ChatMessageCallbacks {
                            on_copy_message: Some(Arc::new({
                                let w = inline_for_copy.clone();
                                move |msg_idx: usize, text: &str, _window: &mut Window, cx: &mut App| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
                                    let _ = w.update(cx, |this, cx| {
                                        this.copied_msg_index = Some(msg_idx);
                                        cx.notify();
                                        cx.spawn(async move |this: WeakEntity<TerminalAiInline>, cx| {
                                            smol::Timer::after(Duration::from_millis(2000)).await;
                                            let _ = this.update(cx, |this, cx| {
                                                this.copied_msg_index = None;
                                                cx.notify();
                                            });
                                        }).detach();
                                    });
                                }
                            })),
                            on_run_command: Some(Arc::new({
                                let w = inline_for_run;
                                let tid = tid_run;
                                move |cmd: &str, _window: &mut Window, cx: &mut App| {
                                    let _ = w.update(cx, |_this, cx| {
                                        cx.emit(TerminalAiInlineEvent::RunInTerminal {
                                            terminal_id: tid.clone(),
                                            command: cmd.to_string(),
                                        });
                                    });
                                }
                            })),
                            on_insert_command: Some(Arc::new({
                                let w = inline_for_ins;
                                let tid = tid_ins;
                                move |cmd: &str, _window: &mut Window, cx: &mut App| {
                                    let _ = w.update(cx, |_this, cx| {
                                        cx.emit(TerminalAiInlineEvent::InsertToTerminal {
                                            terminal_id: tid.clone(),
                                            command: cmd.to_string(),
                                        });
                                    });
                                }
                            })),
                            on_selection_event: Some(Arc::new({
                                let w = inline_for_sel;
                                let followup = self.followup_input.clone();
                                move |msg_idx: usize, ev: MarkdownSelectionEvent, window: &mut Window, cx: &mut App| {
                                    let _ = w.update(cx, |this, cx| {
                                        this.focus_handle.focus(window, cx);
                                        if let MarkdownSelectionEvent::Start { .. } = &ev {
                                            followup.update(cx, |inp, cx| inp.clear_selection(cx));
                                        }
                                        this.handle_selection_event(msg_idx, ev, cx);
                                    });
                                }
                            })),
                            on_context_menu: None,
                            on_toggle_quote: Some(Arc::new({
                                let w = inline_for_quote;
                                move |msg_idx: usize, _window: &mut Window, cx: &mut App| {
                                    let _ = w.update(cx, |this, cx| {
                                        if !this.expanded_quotes.remove(&msg_idx) {
                                            this.expanded_quotes.insert(msg_idx);
                                        }
                                        cx.notify();
                                    });
                                }
                            })),
                            on_edit_message: None,
                        };

                        let frame = self.animation_frame;
                        let copied_idx = self.copied_msg_index;
                        let sel_range = active_selection
                            .as_ref()
                            .map(|s| (s.start, s.end));
                        let sel_msg_idx = active_selection
                            .as_ref()
                            .map(|s| s.msg_index);

                        v_flex()
                            .w_full()
                            .min_w(px(0.0))
                            .gap(SPACE_MD)
                            .children(
                                self.messages.iter().enumerate().map(|(msg_idx, msg)| {
                                    let active_sel = if sel_msg_idx == Some(msg_idx) {
                                        sel_range
                                    } else {
                                        None
                                    };
                                    let is_quote_expanded = self.expanded_quotes.contains(&msg_idx);
                                    render_chat_message(
                                        msg,
                                        msg_idx,
                                        frame,
                                        copied_idx == Some(msg_idx),
                                        is_quote_expanded,
                                        active_sel,
                                        &callbacks,
                                        cx,
                                    )
                                })
                            )
                    })
            )
            // --- Footer: Followup Input ---
            .child(
                div()
                    .px(SPACE_XS)
                    .pb(SPACE_XS)
                    .pt(px(2.0))
                    .flex_shrink_0()
                    .bg(p.surface_overlay)
                    .rounded_b(RADIUS_MD)
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(self.input_height)
                            .rounded(RADIUS_MD)
                            .border_1()
                            .border_color(p.border_subtle)
                            .bg(p.surface_raised)
                            .p(px(4.0))
                            .flex()
                            .flex_col()
                            .justify_between()
                            // Top Splitter / Resize Handle for input card
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-4.0))
                                    .left(px(4.0))
                                    .right(px(4.0))
                                    .h(px(8.0))
                                    .cursor(CursorStyle::ResizeUpDown)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, e: &MouseDownEvent, _window, cx| {
                                            cx.stop_propagation();
                                            this.input_resize_drag = Some(InputResizeDrag {
                                                start_mouse_y: e.position.y,
                                                start_height: this.input_height,
                                            });
                                            cx.notify();
                                        }),
                                    ),
                            )
                            // Input Text Area
                            .child(
                                div()
                                    .flex_1()
                                    .min_h(px(28.0))
                                    .w_full()
                                    .overflow_hidden()
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
                                    .child(
                                        SimpleInput::new(&self.followup_input)
                                            .borderless(true)
                                            .fill_height()
                                            .text_size(ui_text_md(cx)),
                                    ),
                            )
                            // Bottom Controls Bar: Model selector on bottom-left (4px margin), Send on bottom-right
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .pt(px(2.0))
                                    .child(
                                        div()
                                            .w(px(140.0))
                                            .child(Select::new(&self.model_select)),
                                    )
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
