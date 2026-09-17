//! LLM 接入层：OpenAI 兼容流式 + 本地规则回退。
//!
//! 本模块保持对 gpui / workspace 无依赖，可独立编译与单元测试。
//! 规则引擎只做关键词匹配并返回结构化 [`RuleReply`]，文案翻译与运行时读取
//! 由 [`crate::AiClient`] 负责。

use std::io::{BufRead, BufReader};
use std::sync::mpsc;

use serde_json::json;

use crate::runtime::AIError;

/// 当前支持的模型标识（本地规则引擎）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiModel {
    Local,
}

impl AiModel {
    pub fn all() -> &'static [AiModel] {
        &[AiModel::Local]
    }

    pub fn index(self) -> usize {
        AiModel::all().iter().position(|m| *m == self).unwrap_or(0)
    }
}

/// 流式响应分片。
#[derive(Clone, Debug)]
pub enum StreamChunk {
    Delta(String),
    /// 一轮推理结束时的工具调用集合（OpenAI 兼容 tool calling）。
    ToolCalls(Vec<ToolCall>),
    Done,
    Error(AIError),
}

/// LLM 连接配置（用于 Skill / Agent 内部回调模型）。
#[derive(Clone, Debug, Default)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model_id: String,
}

/// 暴露给 LLM 的工具描述（OpenAI 兼容 function 定义）。
#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema 对象，描述工具参数。
    pub parameters: serde_json::Value,
}

/// 一次工具调用（流式累积后的完整结果）。
#[derive(Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// 原始 JSON 参数字符串（跨多个 delta 累积而成）。
    pub arguments: String,
}

/// 本地规则引擎的决策结果（不含 i18n 文案，由调用方翻译）。
pub enum RuleReply {
    TerminalContent,
    ListSessions,
    SessionConfig(String),
    Exec(String),
    Script,
    Generic,
}

/// 本地规则回退 provider。
pub struct LocalRuleProvider;

impl LocalRuleProvider {
    /// 仅做关键词匹配，返回结构化决策；不读取运行时状态、不产生文案。
    pub fn decide(input: &str) -> RuleReply {
        let text = input.trim();
        let lower = text.to_lowercase();

        if lower.contains("读取终端")
            || lower.contains("当前终端")
            || lower.contains("屏幕")
            || lower.contains("读屏")
            || lower.contains("terminal content")
            || lower.contains("screen content")
            || lower.contains("read terminal")
        {
            return RuleReply::TerminalContent;
        }

        if lower.contains("会话列表")
            || lower.contains("列出会话")
            || lower.contains("有哪些会话")
            || lower.contains("所有连接")
            || lower.contains("list sessions")
            || lower.contains("list connections")
        {
            return RuleReply::ListSessions;
        }

        if lower.contains("配置")
            || lower.contains("config")
            || lower.contains("会话信息")
            || lower.contains("connection info")
        {
            let name = text
                .replace("配置", "")
                .replace("config", "")
                .replace("查看", "")
                .replace("会话", "")
                .replace("的", "")
                .replace("信息", "")
                .trim()
                .to_string();
            return RuleReply::SessionConfig(name);
        }

        if lower.contains("执行")
            || lower.contains("运行")
            || lower.contains("execute")
            || lower.contains("run")
        {
            if let Some(cmd) = extract_command(text) {
                return RuleReply::Exec(cmd);
            }
        }

        if lower.contains("脚本")
            || lower.contains("script")
            || lower.contains("自动化")
            || lower.contains("编写")
        {
            return RuleReply::Script;
        }

        RuleReply::Generic
    }
}

fn has_cjk(s: &str) -> bool {
    s.chars()
        .any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c))
}

fn extract_command(input: &str) -> Option<String> {
    for delim in ['`', '"', '\''] {
        if let Some(start) = input.find(delim) {
            let rest = &input[start + 1..];
            if let Some(end) = rest.find(delim) {
                let cmd = rest[..end].trim();
                if !cmd.is_empty() {
                    return Some(cmd.to_string());
                }
            }
        }
    }

    let verbs = [
        "请执行", "请运行", "帮我执行", "帮我运行", "帮忙执行", "执行", "运行", "execute", "run",
        "please execute", "please run",
    ];
    let mut s = input.trim();
    for v in verbs {
        if let Some(stripped) = s.strip_prefix(v) {
            s = stripped;
        }
    }
    let s = s.trim();
    if s.is_empty() || has_cjk(s) {
        None
    } else {
        Some(s.to_string())
    }
}

const BACKUP_SCRIPT: &str = r#"#!/usr/bin/env bash
set -euo pipefail
SRC="${1:-/var/www}"
DEST="${2:-/backups}"
ts="$(date +%Y%m%d-%H%M%S)"
tar -czf "$DEST/backup-$ts.tar.gz" "$SRC"
echo "Backed up $SRC -> $DEST/backup-$ts.tar.gz"
"#;

const DEPLOY_SCRIPT: &str = r#"#!/usr/bin/env bash
set -euo pipefail
cd "${1:-/srv/app}"
git pull --ff-only
docker compose build
docker compose up -d
echo "Deploy finished."
"#;

const MONITOR_SCRIPT: &str = r#"#!/usr/bin/env bash
echo "== CPU =="; top -bn1 | head -n 5
echo "== Memory =="; free -m
echo "== Disk =="; df -h
"#;

const CLEANUP_SCRIPT: &str = r#"#!/usr/bin/env bash
set -euo pipefail
TARGET="${1:-/tmp}"
find "$TARGET" -type f -mtime +7 -delete
echo "Cleaned files older than 7 days in $TARGET"
"#;

const GENERIC_SCRIPT: &str = r#"#!/usr/bin/env bash
set -euo pipefail
echo "Replace this with your automation logic."
"#;

/// 根据提示词生成脚本（纯数据，无 i18n）。
pub fn write_script(prompt: &str) -> String {
    let p = prompt.to_lowercase();
    let (lang, body) = if p.contains("backup") || p.contains("备份") {
        ("bash", BACKUP_SCRIPT)
    } else if p.contains("deploy") || p.contains("部署") {
        ("bash", DEPLOY_SCRIPT)
    } else if p.contains("monitor") || p.contains("监控") {
        ("bash", MONITOR_SCRIPT)
    } else if p.contains("clean") || p.contains("清理") {
        ("bash", CLEANUP_SCRIPT)
    } else {
        ("bash", GENERIC_SCRIPT)
    };
    format!("```{}\n{}\n```", lang, body)
}

/// 向 OpenAI 兼容端点发起流式请求的核心实现。
///
/// 在独立线程中阻塞读取 SSE 流，通过 [`mpsc::Receiver`] 逐片回传 [`StreamChunk`]。
/// 当 `tools` 非空时附加工具定义，并解析 `delta.tool_calls`，在流结束时以
/// [`StreamChunk::ToolCalls`] 一次性回传累积结果。
///
/// `http_timeout` — HTTP 请求整体超时（含 connect + read），默认 30s。
fn stream_api_raw(
    base_url: &str,
    api_key: &str,
    model_id: &str,
    messages: Vec<serde_json::Value>,
    tools: &[ToolSpec],
    http_timeout: std::time::Duration,
) -> mpsc::Receiver<StreamChunk> {
    let url = build_chat_completions_url(base_url);
    let api_key = api_key.to_string();
    let model_id = model_id.to_string();

    let mut body = json!({
        "model": model_id,
        "messages": messages,
        "stream": true,
        "max_tokens": 4096,
    });
    if !tools.is_empty() {
        let tool_vals: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        body["tools"] = json!(tool_vals);
    }

    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let client = match reqwest::blocking::Client::builder()
            .timeout(http_timeout)
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(StreamChunk::Error(AIError::Network {
                    message: e.to_string(),
                }));
                return;
            }
        };

        let mut req = client.post(&url).json(&body);
        if !api_key.is_empty() {
            req = req.bearer_auth(&api_key);
        }

        let resp = match req.send() {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(StreamChunk::Error(AIError::Network {
                    message: e.to_string(),
                }));
                return;
            }
        };

        let status = resp.status();
        if !status.is_success() {
            let code = status.as_u16();
            let err = match code {
                401 => AIError::Unauthorized {
                    message: format!("HTTP {code}"),
                },
                403 => AIError::Forbidden {
                    message: format!("HTTP {code}"),
                },
                429 => AIError::RateLimited {
                    message: format!("HTTP {code}"),
                    retry_after_secs: None,
                },
                s if s >= 500 => AIError::Server {
                    status: s,
                    message: format!("HTTP {code}"),
                },
                _ => AIError::ProviderError {
                    message: format!("HTTP {code}"),
                },
            };
            let _ = tx.send(StreamChunk::Error(err));
            return;
        }

        // 按 delta.tool_calls[].index 分组累积工具调用。
        let mut acc: Vec<ToolCall> = Vec::new();

        let reader = BufReader::new(resp);
        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };
            let line = line.trim();
            if line.is_empty() || !line.starts_with("data: ") {
                continue;
            }
            let data = &line[6..];
            if data == "[DONE]" {
                break;
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(delta) = json
                    .pointer("/choices/0/delta/content")
                    .and_then(|v| v.as_str())
                {
                    if !delta.is_empty() {
                        let _ = tx.send(StreamChunk::Delta(delta.to_string()));
                    }
                }
                if let Some(calls) = json
                    .pointer("/choices/0/delta/tool_calls")
                    .and_then(|v| v.as_array())
                {
                    for call in calls {
                        let idx = call
                            .get("index")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as usize;
                        if idx >= acc.len() {
                            acc.resize_with(idx + 1, || ToolCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                            });
                        }
                        if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                            if !id.is_empty() {
                                acc[idx].id = id.to_string();
                            }
                        }
                        if let Some(name) =
                            call.pointer("/function/name").and_then(|v| v.as_str())
                        {
                            if !name.is_empty() {
                                acc[idx].name = name.to_string();
                            }
                        }
                        if let Some(args) =
                            call.pointer("/function/arguments").and_then(|v| v.as_str())
                        {
                            acc[idx].arguments.push_str(args);
                        }
                    }
                }
            }
        }

        if !acc.is_empty() {
            let _ = tx.send(StreamChunk::ToolCalls(acc));
        }
        let _ = tx.send(StreamChunk::Done);
    });

    rx
}

/// 向 OpenAI 兼容的 chat completions 端点发起流式请求（纯文本，无工具）。
pub fn stream_api_reply(
    base_url: &str,
    api_key: &str,
    model_id: &str,
    messages: &[(String, bool)],
) -> mpsc::Receiver<StreamChunk> {
    stream_api_reply_with_system(base_url, api_key, model_id, None, messages)
}

/// 向 OpenAI 兼容的 chat completions 端点发起带 system prompt 的流式请求（纯文本，无工具）。
pub fn stream_api_reply_with_system(
    base_url: &str,
    api_key: &str,
    model_id: &str,
    system_prompt: Option<&str>,
    messages: &[(String, bool)],
) -> mpsc::Receiver<StreamChunk> {
    let mut api_messages: Vec<serde_json::Value> = Vec::new();
    if let Some(sys) = system_prompt {
        if !sys.trim().is_empty() {
            api_messages.push(json!({
                "role": "system",
                "content": sys,
            }));
        }
    }
    for (text, is_user) in messages {
        api_messages.push(json!({
            "role": if *is_user { "user" } else { "assistant" },
            "content": text,
        }));
    }
    stream_api_raw(base_url, api_key, model_id, api_messages, &[], std::time::Duration::from_secs(30))
}

/// 向 OpenAI 兼容端点发起带工具定义的流式请求（支持 tool calling）。
///
/// `messages` 为完整 OpenAI 格式的消息列表（含 system / user / assistant / tool）。
/// `http_timeout` — HTTP 请求整体超时，默认 30s。
pub fn stream_api_reply_with_tools(
    base_url: &str,
    api_key: &str,
    model_id: &str,
    messages: &[serde_json::Value],
    tools: &[ToolSpec],
    http_timeout: std::time::Duration,
) -> mpsc::Receiver<StreamChunk> {
    stream_api_raw(
        base_url,
        api_key,
        model_id,
        messages.to_vec(),
        tools,
        http_timeout,
    )
}

/// 流式补全（无工具）：对每个文本 delta 调用 `on_delta`，返回最终拼接文本。
///
/// 供 Skill 内部调用模型（诊断 / 生成命令 / 解释日志等），并可在 `on_delta`
/// 中把增量回传给上层做流式展示。
pub fn stream_complete(
    base_url: &str,
    api_key: &str,
    model_id: &str,
    messages: &[serde_json::Value],
    mut on_delta: impl FnMut(&str),
) -> Result<String, AIError> {
    let rx = stream_api_raw(base_url, api_key, model_id, messages.to_vec(), &[], std::time::Duration::from_secs(30));
    let mut out = String::new();
    for chunk in rx {
        match chunk {
            StreamChunk::Delta(t) => {
                out.push_str(&t);
                on_delta(&t);
            }
            StreamChunk::Error(e) => return Err(e),
            StreamChunk::Done | StreamChunk::ToolCalls(_) => {}
        }
    }
    Ok(out)
}

/// 构造规范的 `/chat/completions` 请求 URL。
///
/// 兼容用户填写的各种 Base URL 格式：
/// - `https://api.openai.com/v1` -> `https://api.openai.com/v1/chat/completions`
/// - `https://api.openai.com/v1/` -> `https://api.openai.com/v1/chat/completions`
/// - `https://api.openai.com/v1/chat/completions` -> `https://api.openai.com/v1/chat/completions`
/// - `https://api.openai.com/v1/chat/completions/` -> `https://api.openai.com/v1/chat/completions`
pub fn build_chat_completions_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{}/chat/completions", trimmed)
    }
}

/// 测试 LLM 端点连通性。
///
/// `base_url`: API Base URL（支持带或不带 `/chat/completions`）
/// `api_key`: API 密钥
/// `model_id`: 待测试的模型 ID（如 `gemini-1.5-flash`, `gpt-4o` 等，若为空则回退至 `gpt-3.5-turbo`）
/// `timeout_secs`: 请求超时秒数
pub fn test_llm_connection(
    base_url: &str,
    api_key: &str,
    model_id: &str,
    timeout_secs: u64,
) -> Result<(), String> {
    let url = build_chat_completions_url(base_url);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| e.to_string())?;

    let model = if model_id.trim().is_empty() {
        "gpt-3.5-turbo"
    } else {
        model_id.trim()
    };

    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 1,
    });

    let mut req = client.post(&url).json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }

    match req.send() {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                Ok(())
            } else {
                let text = resp.text().unwrap_or_default();
                let msg = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| v.get("error")?.get("message")?.as_str().map(String::from))
                    .unwrap_or_else(|| format!("HTTP {}", status));
                Err(msg)
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_chat_completions_url() {
        assert_eq!(
            build_chat_completions_url("https://generativelanguage.googleapis.com/v1beta/openai"),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(
            build_chat_completions_url("https://generativelanguage.googleapis.com/v1beta/openai/"),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(
            build_chat_completions_url(
                "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
            ),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(
            build_chat_completions_url(
                "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions/"
            ),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(
            build_chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
    }
}
