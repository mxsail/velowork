//! 多轮对话历史上下文 Token 估算与自动压缩引擎。

use serde::{Deserialize, Serialize};
pub use velowork_workspace::settings::AiCompressionStrategy;

/// 通用简单消息接口，用于估算 Token 与按策略压缩上下文。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimpleChatMessage {
    pub is_user: bool,
    pub text: String,
    pub thinking: Option<String>,
    pub quote: Option<String>,
}

impl SimpleChatMessage {
    pub fn new(is_user: bool, text: impl Into<String>) -> Self {
        Self {
            is_user,
            text: text.into(),
            thinking: None,
            quote: None,
        }
    }

    pub fn with_thinking(mut self, thinking: impl Into<String>) -> Self {
        self.thinking = Some(thinking.into());
        self
    }

    pub fn with_quote(mut self, quote: impl Into<String>) -> Self {
        self.quote = Some(quote.into());
        self
    }

    /// 估算本条消息的 Token 使用量。
    pub fn estimate_tokens(&self) -> usize {
        let mut total = estimate_tokens(&self.text);
        if let Some(thinking) = &self.thinking {
            total += estimate_tokens(thinking);
        }
        if let Some(quote) = &self.quote {
            total += estimate_tokens(quote);
        }
        // 加上消息 header 开销 (~4 tokens)
        total + 4
    }
}

/// 快速中英文混合 Token 数估算。
///
/// 规则：
/// - CJK 字符（中日韩等全角）：约 1.5 字符 / token (char_count * 2 / 3 + 1)
/// - ASCII 字符（英文字母、数字、标点）：约 4 字符 / token
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let mut cjk_count = 0usize;
    let mut ascii_count = 0usize;

    for ch in text.chars() {
        if is_cjk_char(ch) {
            cjk_count += 1;
        } else {
            ascii_count += 1;
        }
    }

    let cjk_tokens = (cjk_count * 2 + 2) / 3;
    let ascii_tokens = (ascii_count + 3) / 4;
    (cjk_tokens + ascii_tokens).max(1)
}

fn is_cjk_char(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}' |
        '\u{3400}'..='\u{4DBF}' |
        '\u{20000}'..='\u{2A6DF}' |
        '\u{3000}'..='\u{303F}' |
        '\u{FF00}'..='\u{FFEF}'
    )
}

/// 估算多条消息的总 Token 数。
pub fn estimate_messages_tokens(messages: &[SimpleChatMessage]) -> usize {
    messages.iter().map(|m| m.estimate_tokens()).sum()
}

/// 对多轮对话历史消息按指定策略与参数实施上下文压缩。
///
/// - `strategy`: 压缩策略 (`Summarize` / `SlidingWindow` / `TruncateOldest`)
/// - `max_tokens`: 允许的最大上下文 Token 阈值
/// - `max_history_messages`: 允许保留的最大历史消息数
pub fn compress_chat_history(
    messages: &[SimpleChatMessage],
    strategy: AiCompressionStrategy,
    max_tokens: usize,
    max_history_messages: usize,
) -> Vec<SimpleChatMessage> {
    if messages.is_empty() {
        return Vec::new();
    }

    let total_tokens = estimate_messages_tokens(messages);
    let total_count = messages.len();

    // 如果未超过条数与 Token 限制，无需压缩
    if total_tokens <= max_tokens && total_count <= max_history_messages {
        return messages.to_vec();
    }

    match strategy {
        AiCompressionStrategy::SlidingWindow => {
            let keep_count = max_history_messages.max(2);
            if messages.len() <= keep_count {
                messages.to_vec()
            } else {
                messages[messages.len() - keep_count..].to_vec()
            }
        }
        AiCompressionStrategy::TruncateOldest => {
            let mut result = messages.to_vec();
            while result.len() > 1 && estimate_messages_tokens(&result) > max_tokens {
                result.remove(0);
            }
            result
        }
        AiCompressionStrategy::Summarize => {
            let keep_recent = (max_history_messages / 2).max(2).min(messages.len());
            let split_idx = messages.len().saturating_sub(keep_recent);

            if split_idx == 0 {
                return messages.to_vec();
            }

            let older = &messages[..split_idx];
            let recent = &messages[split_idx..];

            // 生成启发式摘要段落
            let mut summary_lines = Vec::new();
            summary_lines.push("[History Summary of previous turns]:".to_string());
            for m in older {
                let role = if m.is_user { "User" } else { "Assistant" };
                let mut snippet = m.text.replace('\n', " ");
                if snippet.len() > 100 {
                    snippet = format!("{}...", &snippet[..100]);
                }
                summary_lines.push(format!("- {}: {}", role, snippet));
            }

            let summary_text = summary_lines.join("\n");
            let summary_msg = SimpleChatMessage::new(false, summary_text);

            let mut out = Vec::with_capacity(recent.len() + 1);
            out.push(summary_msg);
            out.extend_from_slice(recent);

            // 若摘要 + 近期消息依然超出 max_tokens，则二次按 Token 截断最旧
            while out.len() > 2 && estimate_messages_tokens(&out) > max_tokens {
                out.remove(1); // 保留 idx 0 的摘要，移除紧接着最旧的近期消息
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        let text = "Hello world! 这是一个测试。";
        let tokens = estimate_tokens(text);
        assert!(tokens > 5 && tokens < 20);
    }

    #[test]
    fn test_sliding_window_compression() {
        let mut msgs = Vec::new();
        for i in 0..10 {
            msgs.push(SimpleChatMessage::new(i % 2 == 0, format!("Message {}", i)));
        }
        let compressed = compress_chat_history(&msgs, AiCompressionStrategy::SlidingWindow, 10000, 4);
        assert_eq!(compressed.len(), 4);
        assert_eq!(compressed.last().unwrap().text, "Message 9");
    }

    #[test]
    fn test_summarize_compression() {
        let mut msgs = Vec::new();
        for i in 0..10 {
            msgs.push(SimpleChatMessage::new(i % 2 == 0, format!("Turn message content {}", i)));
        }
        let compressed = compress_chat_history(&msgs, AiCompressionStrategy::Summarize, 10000, 4);
        assert!(compressed.len() <= 5);
        assert!(compressed[0].text.contains("[History Summary"));
    }
}
