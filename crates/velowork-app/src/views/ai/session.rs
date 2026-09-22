//! GPUI Entity for managing an AI chat session, its streaming lifecycle, and messages.

use std::sync::Arc;
use std::time::Duration;
use gpui::{Context, Task, WeakEntity};
use parking_lot::Mutex;
use velowork_ai::provider::{self, StreamChunk};
use velowork_i18n::i18n;

use super::types::{ChatAttachment, ChatMessage};

pub struct AiChatSession {
    messages: Vec<ChatMessage>,
    is_streaming: bool,
    streaming_index: Option<usize>,
    error_message: Option<String>,
    stream_rx: Option<Arc<Mutex<std::sync::mpsc::Receiver<StreamChunk>>>>,
    _stream_task: Option<Task<()>>,
    animation_frame: u64,
    copied_msg_index: Option<usize>,
    _copied_task: Option<Task<()>>,
}

impl AiChatSession {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            messages: Vec::new(),
            is_streaming: false,
            streaming_index: None,
            error_message: None,
            stream_rx: None,
            _stream_task: None,
            animation_frame: 0,
            copied_msg_index: None,
            _copied_task: None,
        }
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        &mut self.messages
    }

    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    pub fn streaming_index(&self) -> Option<usize> {
        self.streaming_index
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    pub fn animation_frame(&self) -> u64 {
        self.animation_frame
    }

    pub fn copied_msg_index(&self) -> Option<usize> {
        self.copied_msg_index
    }

    pub fn set_copied_msg_index(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.copied_msg_index = Some(idx);
        cx.notify();

        self._copied_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            smol::Timer::after(Duration::from_millis(2000)).await;
            let _ = this.update(cx, |this, cx| {
                this.copied_msg_index = None;
                cx.notify();
            });
        }));
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.stop(cx);
        self.messages.clear();
        self.error_message = None;
        cx.notify();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if self.is_streaming {
            self.is_streaming = false;
            if let Some(idx) = self.streaming_index {
                if let Some(msg) = self.messages.get_mut(idx) {
                    msg.streaming = false;
                }
            }
        }
        self.stream_rx = None;
        self._stream_task = None;
        self.streaming_index = None;
        cx.notify();
    }

    pub fn push_user_message(
        &mut self,
        text: String,
        quote: Option<String>,
        attachments: Vec<ChatAttachment>,
        cx: &mut Context<Self>,
    ) {
        self.messages.push(ChatMessage::new_user(text, quote, attachments));
        cx.notify();
    }

    pub fn push_assistant_message(
        &mut self,
        text: String,
        streaming: bool,
        cx: &mut Context<Self>,
    ) -> usize {
        self.messages.push(ChatMessage::new_assistant(text, streaming));
        let idx = self.messages.len() - 1;
        cx.notify();
        idx
    }

    /// 发起多轮对话请求并启动 40ms Timer 非阻塞消费流
    pub fn start_turn(
        &mut self,
        base_url: &str,
        api_key: &str,
        model_id: &str,
        user_text: String,
        quote: Option<String>,
        attachments: Vec<ChatAttachment>,
        cx: &mut Context<Self>,
    ) {
        self.stop(cx);
        self.error_message = None;

        // 压入用户消息
        self.messages.push(ChatMessage::new_user(user_text, quote, attachments));

        // 压入助手消息占位
        self.messages.push(ChatMessage::new_assistant(String::new(), true));
        let assistant_idx = self.messages.len() - 1;
        self.streaming_index = Some(assistant_idx);
        self.is_streaming = true;

        // 构建全量多轮上下文
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

        let rx = provider::stream_api_reply(base_url, api_key, model_id, &api_messages);
        let rx_arc = Arc::new(Mutex::new(rx));
        self.stream_rx = Some(rx_arc);

        self._stream_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                smol::Timer::after(Duration::from_millis(40)).await;

                let finished = this
                    .update(cx, |this, cx| {
                        this.animation_frame = this.animation_frame.wrapping_add(1);
                        let rx_lock = this.stream_rx.clone();
                        let Some(ref rx_lock) = rx_lock else {
                            return true;
                        };
                        let rx = rx_lock.lock();

                        let mut done = false;
                        let mut updated = false;

                        loop {
                            match rx.try_recv() {
                                Ok(chunk) => match chunk {
                                    StreamChunk::Delta(delta) => {
                                        if let Some(idx) = this.streaming_index {
                                            if let Some(last) = this.messages.get_mut(idx) {
                                                last.text.push_str(&delta);
                                            }
                                        }
                                        updated = true;
                                    }
                                    StreamChunk::Done => {
                                        this.is_streaming = false;
                                        if let Some(idx) = this.streaming_index {
                                            if let Some(last) = this.messages.get_mut(idx) {
                                                last.streaming = false;
                                                if last.text.is_empty() {
                                                    let err_str = format!(
                                                        "{}: {}",
                                                        i18n!(cx, "ai.error"),
                                                        i18n!(cx, "ai.empty_response")
                                                    );
                                                    this.error_message = Some(err_str.clone());
                                                    last.text = err_str;
                                                }
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
                                        if let Some(idx) = this.streaming_index {
                                            if let Some(last) = this.messages.get_mut(idx) {
                                                last.streaming = false;
                                                let full_err = format!(
                                                    "{}: {}",
                                                    i18n!(cx, "ai.error"),
                                                    err_str
                                                );
                                                last.text = full_err;
                                            }
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
                                    if let Some(idx) = this.streaming_index {
                                        if let Some(last) = this.messages.get_mut(idx) {
                                            last.streaming = false;
                                            if last.text.is_empty() {
                                                let err_str = format!(
                                                    "{}: {}",
                                                    i18n!(cx, "ai.error"),
                                                    i18n!(cx, "ai.network_interrupted")
                                                );
                                                this.error_message = Some(err_str.clone());
                                                last.text = err_str;
                                            }
                                        }
                                    }
                                    done = true;
                                    updated = true;
                                    break;
                                }
                            }
                        }

                        if updated {
                            cx.notify();
                        } else if this.is_streaming {
                            // 思考等待中：每帧通知重绘以驱动圆点呼吸波浪动效
                            cx.notify();
                        }

                        if done {
                            this.stream_rx = None;
                            this.streaming_index = None;
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(true);

                if finished {
                    break;
                }
            }
        }));

        cx.notify();
    }
}
