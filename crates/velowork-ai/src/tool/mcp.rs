//! MCP（Model Context Protocol）适配器：通过 stdio 连接 MCP server，
//! 动态发现其 tools，包装为 [`Tool`] 实现注册进工具表。
//!
//! 采用 JSON-RPC 2.0 over newline-delimited stdio（MCP 标准传输）。每个 MCP server
//! 对应一个长驻进程；其 tools 通过共享的 `Arc<Mutex<McpClientInner>>` 复用同一连接，
//! 进程随最后一个引用释放而终止。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use gpui::App;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolCtx, ToolError};

/// 一个 MCP server 的启动配置。
#[derive(Clone, Debug)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// MCP server 暴露的单个工具描述（发现阶段得到）。
#[derive(Clone, Debug)]
pub struct McpToolSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// 构造一条 JSON-RPC 请求（已序列化为单行 JSON）。
fn jsonrpc_request(id: i64, method: &str, params: Value) -> String {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    serde_json::to_string(&msg).unwrap_or_default()
}

/// 解析一行 JSON-RPC 响应：
/// - 通知（无 `id`）或非目标 `id` → `Ok(None)`（跳过）。
/// - 目标 `id` 且含 `error` → `Err`。
/// - 目标 `id` 且含 `result` → `Ok(Some(result))`。
fn parse_response_line(line: &str, expected_id: i64) -> Result<Option<Value>, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("invalid JSON: {}", e))?;
    // 通知没有 id，直接跳过。
    let id = match v.get("id") {
        Some(id) => id,
        None => return Ok(None),
    };
    if id != &json!(expected_id) {
        return Ok(None);
    }
    if let Some(err) = v.get("error") {
        return Err(format!("MCP error: {}", err));
    }
    Ok(Some(v.get("result").cloned().unwrap_or(Value::Null)))
}

struct McpClientInner {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Drop for McpClientInner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// MCP stdio 客户端。多个 [`McpTool`] 通过共享的 `inner` 复用同一连接。
pub struct McpClient {
    inner: Arc<Mutex<McpClientInner>>,
    #[allow(dead_code)]
    server_name: String,
}

impl McpClient {
    /// 启动 MCP server 进程并完成 `initialize` 握手。
    pub fn start(config: &McpServerConfig) -> Result<Self, String> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        for (k, v) in &config.env {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn '{}': {}", config.command, e))?;
        let stdin = child.stdin.take().ok_or("MCP server has no stdin")?;
        let stdout = child.stdout.take().ok_or("MCP server has no stdout")?;

        let client = McpClient {
            inner: Arc::new(Mutex::new(McpClientInner {
                child,
                stdin,
                stdout: BufReader::new(stdout),
                next_id: 1,
            })),
            server_name: config.name.clone(),
        };

        let init_params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "velowork", "version": "0.1" }
        });
        client.request("initialize", init_params)?;
        // 通知 server 初始化完成（部分 server 需要）。
        let _ = client.notify("notifications/initialized", json!({}));
        Ok(client)
    }

    /// 发送一个请求并阻塞等待匹配 `id` 的响应。
    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        let id = inner.next_id;
        inner.next_id += 1;

        let line = jsonrpc_request(id, method, params);
        inner
            .stdin
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        inner.stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        inner.stdin.flush().map_err(|e| e.to_string())?;

        let mut buf = String::new();
        loop {
            buf.clear();
            let n = inner.stdout.read_line(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("MCP server closed the connection".into());
            }
            let trimmed = buf.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(result) = parse_response_line(trimmed, id)? {
                return Ok(result);
            }
        }
    }

    /// 发送一个通知（无需响应）。
    fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let line = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
        inner
            .stdin
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        inner.stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        inner.stdin.flush().map_err(|e| e.to_string())
    }

    /// 列出 server 暴露的全部工具。
    pub fn list_tools(&self) -> Result<Vec<McpToolSpec>, String> {
        let result = self.request("tools/list", json!({}))?;
        let arr = result
            .get("tools")
            .and_then(|t| t.as_array())
            .ok_or("MCP tools/list returned no 'tools' array")?;
        let mut specs = Vec::new();
        for t in arr {
            let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let description = t
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let schema = t
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
            specs.push(McpToolSpec {
                name,
                description,
                schema,
            });
        }
        Ok(specs)
    }

    /// 调用一个 MCP 工具，返回其文本输出。
    pub fn call_tool(&self, name: &str, args: Value) -> Result<String, String> {
        let result = self.request("tools/call", json!({ "name": name, "arguments": args }))?;
        let mut out = String::new();
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            for item in content {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
        if out.trim().is_empty() {
            out = serde_json::to_string(&result).unwrap_or_default();
        }
        Ok(out)
    }
}

/// 包装一个 MCP server 工具，使其可作为 [`Tool`] 注册。
///
/// 通过共享的 `inner` 连接调用远端工具；`name` / `description` / `schema` 缓存于本地。
#[derive(Clone)]
pub struct McpTool {
    inner: Arc<Mutex<McpClientInner>>,
    name: String,
    description: String,
    schema: Value,
}

impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn schema(&self) -> Value {
        self.schema.clone()
    }
    fn execute(&self, args: Value, _ctx: &ToolCtx, _cx: &App) -> Result<String, ToolError> {
        let client = McpClient {
            inner: self.inner.clone(),
            server_name: self.name.clone(),
        };
        client
            .call_tool(&self.name, args)
            .map_err(ToolError::Execution)
    }
}

/// 启动 MCP server 并发现其全部工具，返回可注册到 [`ToolRegistry`](crate::tool::ToolRegistry) 的工具列表。
///
/// 进程随返回的 [`McpTool`] 中最后一个被丢弃而终止。
pub fn discover_mcp_tools(config: &McpServerConfig) -> Result<Vec<McpTool>, String> {
    let client = McpClient::start(config)?;
    let specs = client.list_tools()?;
    let inner = client.inner.clone();
    let tools = specs
        .into_iter()
        .map(|spec| McpTool {
            inner: inner.clone(),
            name: spec.name,
            description: spec.description,
            schema: spec.schema,
        })
        .collect();
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serialization_shape() {
        let s = jsonrpc_request(3, "tools/list", json!({ "a": 1 }));
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 3);
        assert_eq!(v["method"], "tools/list");
        assert_eq!(v["params"]["a"], 1);
    }

    #[test]
    fn parse_notification_is_skipped() {
        // 通知没有 id → 跳过（Ok(None)）。
        let r = parse_response_line(
            r#"{"jsonrpc":"2.0","method":"notification","params":{}}"#,
            1,
        )
        .unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn parse_non_matching_id_is_skipped() {
        let r = parse_response_line(r#"{"jsonrpc":"2.0","id":99,"result":{"x":1}}"#, 1).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn parse_matching_result() {
        let r = parse_response_line(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#, 1).unwrap();
        assert_eq!(r, Some(json!({"ok": true})));
    }

    #[test]
    fn parse_error_returns_err() {
        let r = parse_response_line(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"boom"}}"#,
            1,
        );
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("boom"));
    }
}
