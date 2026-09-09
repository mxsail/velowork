//! 服务监控采集引擎。
//!
//! 复用聚焦终端已建立的 russh 会话（`SshSessionHandle`，不二次拨号），
//! 将多个服务的存活命令合并为单个脚本一次 exec，以单 `BEGIN` + base64(JSON 数组) +
//! `END` 标记返回所有状态。引擎本身为纯逻辑 + 同步 probe，周期性调度由上层
//! （app 层 status_bar）通过 `cx.spawn`/`cx.background_spawn` 驱动，便于与 GPUI
//! 事件循环集成。

use crate::pty_manager::get_tokio_runtime;
use gpui::*;
use std::collections::HashMap;
use velowork_state::{ServiceDefinition, ServiceId, ServiceOp, ServiceRuntimeState, ServiceStatus};

/// 复用 ssh_monitor 暴露的共享会话句柄别名。
pub use crate::ssh_monitor::SshSessionHandle;

/// 探活脚本包裹标记。
const MARKER_BEGIN: &str = "__VELO_SERVICE_BEGIN__";
const MARKER_END: &str = "__VELO_SERVICE_END__";

/// 一次探活得到的单服务结果。
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceProbeResult {
    pub service_id: ServiceId,
    pub status: ServiceStatus,
    pub message: Option<String>,
}

/// 单个服务探活请求的入参（仅含生存所需字段，避免持有整个 Definition）。
#[derive(Clone, Debug)]
pub struct ProbeTarget {
    pub id: ServiceId,
    pub alive_command: String,
}

/// 后台采集引擎状态（Entity）。不持久化，运行态仅内存。
pub struct ServiceMonitorEngine {
    /// 当前激活终端关联会话的 SSH 句柄（运行时，非持久化）。
    session: Option<SshSessionHandle>,
    /// 当前目标会话 ID（与 session 对应，用于状态归属）。
    session_id: Option<String>,
    /// 需要采集的服务（已按当前会话过滤）。
    services: Vec<ServiceDefinition>,
    /// 是否开启采集（无监控项/未聚焦 SSH 终端时为 false）。
    enabled: bool,
    /// 各服务运行态。
    states: HashMap<ServiceId, ServiceRuntimeState>,
    /// 单调递增版本号，防止旧的 probe 结果覆盖新结果。
    revision: u64,
}

/// 全局句柄。
pub struct GlobalServiceMonitorEngine(pub Entity<ServiceMonitorEngine>);
impl Global for GlobalServiceMonitorEngine {}

impl ServiceMonitorEngine {
    pub fn new() -> Self {
        Self {
            session: None,
            session_id: None,
            services: Vec::new(),
            enabled: false,
            states: HashMap::new(),
            revision: 0,
        }
    }

    /// 设置采集目标：当前激活会话 ID 与关联服务列表。
    pub fn set_target(
        &mut self,
        session_id: Option<String>,
        services: Vec<ServiceDefinition>,
        cx: &mut Context<Self>,
    ) {
        let session_changed = self.session_id != session_id;
        self.session_id = session_id.clone();
        self.services = services;
        self.enabled = session_id.is_some() && !self.services.is_empty();

        if session_changed {
            self.states.clear();
        } else {
            let current_ids: std::collections::HashSet<_> =
                self.services.iter().map(|s| s.id.clone()).collect();
            self.states.retain(|id, _| current_ids.contains(id));
        }

        for s in &self.services {
            if !self.states.contains_key(&s.id) {
                self.revision += 1;
                self.states.insert(
                    s.id.clone(),
                    ServiceRuntimeState {
                        service_id: s.id.clone(),
                        session_id: session_id.clone().unwrap_or_default(),
                        status: ServiceStatus::NotChecked,
                        last_check: 0,
                        message: None,
                        revision: self.revision,
                    },
                );
            }
        }
        cx.notify();
    }

    pub fn services(&self) -> &[ServiceDefinition] {
        &self.services
    }

    /// 注入当前聚焦终端的 SSH 会话句柄（用于执行探活/命令）。
    pub fn set_session(&mut self, handle: SshSessionHandle) {
        self.session = Some(handle);
    }

    /// 清空会话句柄（终端失焦/关闭）。
    pub fn clear_session(&mut self, cx: &mut Context<Self>) {
        self.session = None;
        self.enabled = false;
        cx.notify();
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled && self.session.is_some()
    }

    pub fn states(&self) -> &HashMap<ServiceId, ServiceRuntimeState> {
        &self.states
    }

    /// 返回指定服务的运行态（供 UI 读取状态点色彩）。
    pub fn runtime_for(&self, id: &str) -> Option<&ServiceRuntimeState> {
        self.states.get(id)
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// 返回探活所需的快照（services + session），供上层在后台线程执行。
    pub fn snapshot_for_probe(
        &self,
    ) -> (
        Option<String>,
        Vec<ServiceDefinition>,
        Option<SshSessionHandle>,
    ) {
        (
            self.session_id.clone(),
            self.services.clone(),
            self.session.clone(),
        )
    }

    /// 同步执行一次合并探活（在后台线程调用，内部 block_on russh）。
    pub fn probe(
        services: &[ServiceDefinition],
        session: &SshSessionHandle,
    ) -> Vec<ServiceProbeResult> {
        let targets: Vec<ProbeTarget> = services
            .iter()
            .filter(|s| s.monitor_enabled)
            .filter_map(|s| {
                s.effective_alive_command()
                    .map(|alive_command| ProbeTarget {
                        id: s.id.clone(),
                        alive_command,
                    })
            })
            .collect();

        if targets.is_empty() {
            return Vec::new();
        }

        let script = build_probe_script(&targets);
        let raw = exec_script(session, &script);
        parse_probe_output(&raw, &targets)
    }

    /// 应用探活结果到运行态（在主线程调用）。
    pub fn apply_results(&mut self, results: Vec<ServiceProbeResult>, cx: &mut Context<Self>) {
        for r in results {
            self.revision += 1;
            let entry =
                self.states
                    .entry(r.service_id.clone())
                    .or_insert_with(|| ServiceRuntimeState {
                        service_id: r.service_id.clone(),
                        session_id: self.session_id.clone().unwrap_or_default(),
                        status: ServiceStatus::NotChecked,
                        last_check: 0,
                        message: None,
                        revision: 0,
                    });
            // 仅当结果比现有状态更新时写入（防止乱序覆盖）。
            if r.status != ServiceStatus::Unknown || entry.status == ServiceStatus::NotChecked {
                entry.status = r.status;
                entry.message = r.message;
            }
            entry.last_check = now_secs();
            entry.revision = self.revision;
        }
        cx.notify();
    }

    /// 同步执行一次启停/重启命令（后台线程调用）。
    pub fn exec_command(
        def: &ServiceDefinition,
        op: ServiceOp,
        session: &SshSessionHandle,
    ) -> Result<String, String> {
        let cmd = match op {
            ServiceOp::Start => def.effective_start_command(),
            ServiceOp::Stop => def.effective_stop_command(),
            ServiceOp::Restart => def.effective_restart_command(),
        };
        let cmd = cmd.ok_or_else(|| match op {
            ServiceOp::Start => "缺少启动命令".to_string(),
            ServiceOp::Stop => "缺少停止命令".to_string(),
            ServiceOp::Restart => "缺少重启命令".to_string(),
        })?;
        let script = wrap_workdir(&cmd, def.workdir.as_deref());
        let res = exec_script_detailed(session, &script)?;

        let stderr_clean = res.stderr.trim();
        let stdout_clean = res.stdout.trim();
        let combined_output = format!("{}\n{}", stdout_clean, stderr_clean);

        // 提权与密码交互特征识别
        let is_permission_or_auth_issue = {
            let lower = combined_output.to_lowercase();
            lower.contains("interactive authentication required")
                || lower.contains("a terminal is required")
                || lower.contains("no tty present")
                || lower.contains("a password is required")
                || lower.contains("password:")
                || lower.contains("permission denied")
                || lower.contains("operation not permitted")
                || lower.contains("access denied")
                || combined_output.contains("权限不够")
                || combined_output.contains("需要 root 权限")
                || combined_output.contains("身份验证")
        };

        let exit_code = res.exit_status;
        let is_failed = match exit_code {
            Some(code) => code != 0,
            None => !stderr_clean.is_empty() || is_permission_or_auth_issue,
        };

        if is_failed {
            if is_permission_or_auth_issue {
                let detail = if !stderr_clean.is_empty() {
                    stderr_clean.to_string()
                } else if !stdout_clean.is_empty() {
                    stdout_clean.to_string()
                } else {
                    "需要提权密码".to_string()
                };
                Err(format!("{}（请在终端中执行以输入密码）", detail))
            } else {
                let detail = if !stderr_clean.is_empty() {
                    stderr_clean.to_string()
                } else if !stdout_clean.is_empty() {
                    stdout_clean.to_string()
                } else if let Some(code) = exit_code {
                    format!("退出码: {}", code)
                } else {
                    "命令异常中断".to_string()
                };
                Err(detail)
            }
        } else {
            Ok(res.stdout)
        }
    }
}

/// 当前秒级时间戳。
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 若设置了工作目录，包裹 `cd <workdir> && <cmd>`。
fn wrap_workdir(cmd: &str, workdir: Option<&str>) -> String {
    let path_export =
        "export PATH=\"/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$PATH\"";
    match workdir {
        Some(wd) if !wd.trim().is_empty() => format!("{} && cd {:?} && {}", path_export, wd, cmd),
        _ => format!("{} && {}", path_export, cmd),
    }
}

/// 构造合并探活脚本。
///
/// 对每个目标把 `alive_command` 用 base64 包裹，
/// 远端循环解码并以 `sh -c` 执行，收集每条退出码组装为 JSON 数组，
/// 再将 JSON 数组在远端整体 base64 编码，以 `BEGIN` / `END` 标记输出。
pub fn build_probe_script(targets: &[ProbeTarget]) -> String {
    let mut pairs: Vec<String> = Vec::new();
    for t in targets {
        let b64 = base64_encode(t.alive_command.as_bytes());
        pairs.push(format!("{}:{}", t.id, b64));
    }
    let joined = pairs.join(" ");
    format!(
        "export PATH=\"/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$PATH\"\n\
        if printf 'MQ==' | (base64 -d 2>/dev/null) >/dev/null; then _b64d() {{ base64 -d; }}; \
        elif printf 'MQ==' | (base64 -D 2>/dev/null) >/dev/null; then _b64d() {{ base64 -D; }}; \
        elif printf 'MQ==' | (base64 --decode 2>/dev/null) >/dev/null; then _b64d() {{ base64 --decode; }}; \
        elif printf 'MQ==' | (openssl base64 -d 2>/dev/null) >/dev/null; then _b64d() {{ openssl base64 -d; }}; \
        else _b64d() {{ cat; }}; fi\n\
        if printf '1' | (base64 2>/dev/null) >/dev/null; then _b64e() {{ base64 | tr -d '\\r\\n'; }}; \
        elif printf '1' | (openssl base64 2>/dev/null) >/dev/null; then _b64e() {{ openssl base64 | tr -d '\\r\\n'; }}; \
        else _b64e() {{ cat; }}; fi\n\
        echo {begin}\n\
        json=$(first=1; printf '['; for pair in {pairs}; do id=\"${{pair%%:*}}\"; b64=\"${{pair#*:}}\"; \
        printf '%s' \"$b64\" | _b64d 2>/dev/null | sh >/dev/null 2>&1; rc=$?; \
        st=$([ $rc -eq 0 ] && echo running || echo stopped); \
        [ $first -eq 1 ] && first=0 || printf ','; \
        printf '{{\"id\":\"%s\",\"status\":\"%s\"}}' \"$id\" \"$st\"; done; printf ']')\n\
        printf '%s' \"$json\" | _b64e\necho\necho {end}",
        begin = MARKER_BEGIN,
        end = MARKER_END,
        pairs = joined,
    )
}

/// 解析探活输出：取 BEGIN/END 间内容，base64 decode 得到 JSON 数组 `[{id,status}]`，
/// 映射为 `ServiceStatus`。
pub fn parse_probe_output(raw: &str, _targets: &[ProbeTarget]) -> Vec<ServiceProbeResult> {
    let start = match raw.find(MARKER_BEGIN) {
        Some(i) => i + MARKER_BEGIN.len(),
        None => return Vec::new(),
    };
    let rest = &raw[start..];
    let end = match rest.find(MARKER_END) {
        Some(i) => i,
        None => return Vec::new(),
    };
    let b64 = rest[..end].trim();
    if b64.is_empty() {
        return Vec::new();
    }
    let decoded = match base64_decode(b64) {
        Some(d) => d,
        None => return Vec::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_slice(&decoded) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match parsed.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut results = Vec::new();
    for item in arr {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = match item.get("status").and_then(|v| v.as_str()) {
            Some("running") => ServiceStatus::Running,
            Some("stopped") => ServiceStatus::Stopped,
            _ => ServiceStatus::Unknown,
        };
        results.push(ServiceProbeResult {
            service_id: id,
            status,
            message: None,
        });
    }
    results
}

/// 执行结果（包含退出码、标准输出和标准错误）。
#[derive(Debug, Clone, Default)]
pub struct ExecResult {
    pub exit_status: Option<u32>,
    pub stdout: String,
    pub stderr: String,
}

/// 在 russh 会话上执行脚本并回收退出状态与全部输出。
pub fn exec_script_detailed(
    session: &SshSessionHandle,
    script: &str,
) -> Result<ExecResult, String> {
    let session = session.clone();
    let script = script.to_string();
    let rt = get_tokio_runtime();
    let handle = rt.spawn(async move {
        let mut channel = match session.channel_open_session().await {
            Ok(c) => c,
            Err(e) => return Err(format!("ssh channel open failed: {}", e)),
        };
        if let Err(e) = channel.exec(false, script.as_str()).await {
            return Err(format!("ssh exec failed: {}", e));
        }
        let mut res = ExecResult::default();
        loop {
            match channel.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    res.stdout.push_str(&String::from_utf8_lossy(&data));
                }
                Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                    res.stderr.push_str(&String::from_utf8_lossy(&data));
                }
                Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                    res.exit_status = Some(exit_status);
                }
                Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                Some(_) => {}
            }
        }
        Ok::<_, String>(res)
    });
    match rt.block_on(handle) {
        Ok(Ok(res)) => Ok(res),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("task panicked: {}", e)),
    }
}

/// 在 russh 会话上执行脚本并回收全部输出（阻塞当前线程）。
fn exec_script(session: &SshSessionHandle, script: &str) -> String {
    exec_script_detailed(session, script)
        .map(|r| r.stdout)
        .unwrap_or_else(|e| format!("__VELO_EXEC_ERROR__{}", e))
}

/// 简单 base64 编码（标准字母表）。
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(CHARS[((n >> 18) & 63) as usize] as char);
        out.push(CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

impl Default for ServiceMonitorEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// 简单 base64 解码（标准字母表，忽略空白与 '='）。
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (i, &c) in CHARS.iter().enumerate() {
        lookup[c as usize] = i as u8;
    }
    let clean: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .filter(|&b| b != b'=')
        .collect();
    if clean.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::new();
    for chunk in clean.chunks(4) {
        let mut buf = [0u8; 4];
        for (i, &b) in chunk.iter().enumerate() {
            let v = lookup[b as usize];
            if v == 255 {
                return None;
            }
            buf[i] = v;
        }
        let n = ((buf[0] as u32) << 18)
            | ((buf[1] as u32) << 12)
            | ((buf[2] as u32) << 6)
            | (buf[3] as u32);
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}
