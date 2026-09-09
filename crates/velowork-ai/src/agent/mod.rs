//! 编排层：Tool-Calling / ReAct 循环。
//!
//! ```text
//! 用户意图 → prompt 组装 → provider 推理 → 解析 tool_call
//!         → runtime 门禁校验（经 ToolCtx.permission）→ tool 执行
//!         → 结果回填上下文 → 下一轮 → 直到 finish
//! ```
//!
//! 本模块依赖 gpui（工具执行需 `&App`），并组合 provider / tool / runtime。

use std::sync::mpsc;

use gpui::App;
use serde_json::{json, Value};

use std::time::Duration;

use crate::provider::{stream_api_reply_with_tools, StreamChunk, ToolCall, ToolSpec};
use crate::runtime::{AIError, RequestState, RetryConfig, StreamEvent};
use crate::tool::{ToolCtx, ToolError, ToolRegistry};

/// Agent 循环向调用方回传的事件（用于 UI 流式展示 / 日志）。
#[derive(Clone, Debug)]
pub enum AgentEvent {
    /// 模型输出的思考 / 回复文本片段。
    Thought(String),
    /// 模型决定调用工具。
    ToolUse { name: String, arguments: String },
    /// 工具执行结果。
    ToolResult(String),
    /// 整轮结束。
    Done,
    /// 出错。
    Error(String),
}

/// agent 循环中允许连续重复的相同工具调用轮数上限。
///
/// 若模型连续 `AGENT_REPEAT_LIMIT` 轮产出的工具调用集合完全相同，
/// 说明已陷入重复调用循环（拿不到新信息、也无法给出最终答复），
/// 此时提前终止并报告明确错误，避免无意义地耗尽 `max_rounds`。
const AGENT_REPEAT_LIMIT: usize = 3;

/// 将工具注册表转换为 OpenAI 兼容的 [`ToolSpec`] 列表。
pub fn tool_specs(registry: &ToolRegistry) -> Vec<ToolSpec> {
    registry
        .list()
        .iter()
        .map(|t| ToolSpec {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.schema(),
        })
        .collect()
}

/// 运行一轮 agent 对话（ReAct 循环），返回模型最终回复文本。
///
/// `initial_messages` 为已包含 system / user 的消息列表（OpenAI 格式 [`Value`]）。
/// 工具执行需要 `&App`，因此本函数在持有 `&App` 的线程上同步执行；每个 HTTP 往返
/// 在独立线程中流式读取，主线程通过 `recv` 消费（与现有聊天面板一致）。
///
/// `event_sink` 可选：若提供，循环会回传 [`AgentEvent`] 供调用方流式展示。
pub fn run_agent_turn(
    base_url: &str,
    api_key: &str,
    model_id: &str,
    initial_messages: &[Value],
    registry: &ToolRegistry,
    ctx: &ToolCtx,
    cx: &App,
    max_rounds: usize,
    event_sink: Option<mpsc::Sender<AgentEvent>>,
) -> Result<String, AIError> {
    let specs = tool_specs(registry);
    let mut messages: Vec<Value> = initial_messages.to_vec();

    for _round in 0..max_rounds {
        let rx = stream_api_reply_with_tools(base_url, api_key, model_id, &messages, &specs, std::time::Duration::from_secs(30));

        let mut content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut errored: Option<AIError> = None;

        for chunk in rx {
            match chunk {
                StreamChunk::Delta(text) => {
                    content.push_str(&text);
                    if let Some(sink) = &event_sink {
                        let _ = sink.send(AgentEvent::Thought(text));
                    }
                }
                StreamChunk::ToolCalls(calls) => {
                    tool_calls = calls;
                }
                StreamChunk::Error(e) => {
                    errored = Some(e);
                    break;
                }
                StreamChunk::Done => break,
            }
        }

        if let Some(e) = errored {
            if let Some(sink) = &event_sink {
                let _ = sink.send(AgentEvent::Error(e.to_string()));
                let _ = sink.send(AgentEvent::Done);
            }
            return Err(e);
        }

        // 无工具调用 → 最终回复。
        if tool_calls.is_empty() {
            if let Some(sink) = &event_sink {
                let _ = sink.send(AgentEvent::Done);
            }
            return Ok(content);
        }

        // 回填 assistant 消息（含 tool_calls）。
        let mut assistant = json!({
            "role": "assistant",
            "content": if content.is_empty() { Value::Null } else { Value::String(content.clone()) },
        });
        let calls_json: Vec<Value> = tool_calls
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments }
                })
            })
            .collect();
        assistant["tool_calls"] = json!(calls_json);
        messages.push(assistant);

        // 执行每个工具调用，回填 tool 消息。
        for call in &tool_calls {
            let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
            let result = registry.call(&call.name, args, ctx, cx);
            let result_text = match &result {
                Ok(s) => s.clone(),
                Err(e) => e.to_string(),
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": result_text,
            }));
            if let Some(sink) = &event_sink {
                let _ = sink.send(AgentEvent::ToolUse {
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                });
                let _ = sink.send(AgentEvent::ToolResult(result_text));
            }
        }
    }

    let err = AIError::Other {
        message: "agent turn exceeded max rounds".into(),
    };
    if let Some(sink) = &event_sink {
        let _ = sink.send(AgentEvent::Error(err.to_string()));
        let _ = sink.send(AgentEvent::Done);
    }
    Err(err)
}

pub fn run_agent_turn_with_tool_channel(
    base_url: &str,
    api_key: &str,
    model_id: &str,
    initial_messages: &[Value],
    registry: &ToolRegistry,
    tool_tx: &mpsc::Sender<(String, Value, mpsc::Sender<Result<String, ToolError>>)>,
    max_rounds: usize,
    event_sink: Option<mpsc::Sender<StreamEvent>>,
) {
    let specs = tool_specs(registry);
    let mut messages: Vec<Value> = initial_messages.to_vec();
    let retry_cfg = RetryConfig::default();

    // 重复工具调用检测状态（跨轮次保持）。
    let mut repeat_streak: u32 = 0;
    let mut last_calls: Option<Vec<(String, String)>> = None;

    for _round in 0..max_rounds {
        let mut api_retries: u32 = 0;
        let (content, tool_calls) = loop {
            let rx = stream_api_reply_with_tools(
                base_url,
                api_key,
                model_id,
                &messages,
                &specs,
                Duration::from_secs(30),
            );

            let mut content = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut errored: Option<AIError> = None;

            for chunk in rx {
                match chunk {
                    StreamChunk::Delta(text) => {
                        content.push_str(&text);
                        if let Some(sink) = &event_sink {
                            let _ = sink.send(StreamEvent::Token(text));
                        }
                    }
                    StreamChunk::ToolCalls(calls) => {
                        tool_calls = calls;
                    }
                    StreamChunk::Error(e) => {
                        errored = Some(e);
                        break;
                    }
                    StreamChunk::Done => break,
                }
            }

            match errored {
                Some(e) => {
                    // 可重试且未达上限 → 指数退避后重试。
                    if let Some(delay) = retry_cfg.should_retry(&e, api_retries) {
                        api_retries += 1;
                        std::thread::sleep(delay);
                        continue;
                    }
                    // 不可重试或已达上限 → 失败。
                    if let Some(sink) = &event_sink {
                        let _ = sink.send(StreamEvent::StateChange(RequestState::Failed {
                            error: e,
                        }));
                    }
                    return;
                }
                None => break (content, tool_calls),
            }
        };

        // 重复工具调用防护：若模型连续多轮产出的工具调用集合完全相同，
        // 说明陷入死循环（拿不到新信息、也无法给出最终答复），提前终止。
        let current_sig: Vec<(String, String)> = tool_calls
            .iter()
            .map(|c| (c.name.clone(), c.arguments.clone()))
            .collect();
        if let Some(prev) = &last_calls {
            if *prev == current_sig {
                repeat_streak += 1;
            } else {
                repeat_streak = 0;
            }
        } else {
            repeat_streak = 0;
        }
        last_calls = Some(current_sig);
        if repeat_streak >= AGENT_REPEAT_LIMIT as u32 {
            if let Some(sink) = &event_sink {
                let _ = sink.send(StreamEvent::StateChange(RequestState::Failed {
                    error: AIError::Other {
                        message: "agent 陷入重复的工具调用循环，已停止（请调整问题或补充上下文）".into(),
                    },
                }));
            }
            return;
        }

        if tool_calls.is_empty() {
            if let Some(sink) = &event_sink {
                let _ = sink.send(StreamEvent::StateChange(RequestState::Completed));
            }
            return;
        }

        let mut assistant = json!({
            "role": "assistant",
            "content": if content.is_empty() { Value::Null } else { Value::String(content.clone()) },
        });
        let calls_json: Vec<Value> = tool_calls
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments }
                })
            })
            .collect();
        assistant["tool_calls"] = json!(calls_json);
        messages.push(assistant);

        for call in &tool_calls {
            let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);

            let (result_tx, result_rx) = mpsc::channel();
            if let Err(_) = tool_tx.send((call.name.clone(), args, result_tx)) {
                if let Some(sink) = &event_sink {
                    let _ = sink.send(StreamEvent::StateChange(RequestState::Failed {
                        error: AIError::Other {
                            message: "tool channel disconnected".into(),
                        },
                    }));
                }
                return;
            }

            let result = match result_rx.recv() {
                Ok(r) => r,
                Err(_) => {
                    if let Some(sink) = &event_sink {
                        let _ = sink.send(StreamEvent::StateChange(RequestState::Failed {
                            error: AIError::Other {
                                message: "tool result channel disconnected".into(),
                            },
                        }));
                    }
                    return;
                }
            };

            let result_text = match &result {
                Ok(s) => s.clone(),
                Err(e) => e.to_string(),
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": result_text,
            }));
            if let Some(sink) = &event_sink {
                let _ = sink.send(StreamEvent::ToolRequest {
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                });
                let _ = sink.send(StreamEvent::ToolResult(result_text));
            }
        }
    }

    if let Some(sink) = &event_sink {
        let _ = sink.send(StreamEvent::StateChange(RequestState::Failed {
            error: AIError::Other {
                message: "agent turn exceeded max rounds".into(),
            },
        }));
    }
}
