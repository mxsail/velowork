//! AI 记忆：在多次对话 / 技能调用之间保留轻量上下文。
//!
//! 目标：避免每次都把上千行终端输出塞给模型，改为只保留
//! 当前主机、当前目录、最近执行的命令、最近的错误、用户偏好等结构化摘要。
//! 记忆可持久化到磁盘（JSON），随 `AiClient` 生命周期加载 / 保存。

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// 记忆中保留的「最近」条目上限，避免无限增长。
const MAX_RECENT: usize = 30;

/// AI 运行时记忆。
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AiMemory {
    /// 当前操作的主机 / 会话（来自会话名 / 终端标题）。
    pub current_host: Option<String>,
    /// 当前工作目录。
    pub current_dir: Option<String>,
    /// 当前聚焦的会话 / 项目名（即「连接的是哪台服务器」）。
    pub current_project: Option<String>,
    /// 当前聚焦的终端 Tab 名称。
    pub current_tab: Option<String>,
    /// 当前 Workspace（即会话所属容器）。
    pub current_workspace: Option<String>,
    /// 当前终端中用户选中的文本（右键复制前的上下文）。
    pub selection: Option<String>,
    /// 当前打开的文件（若可探测；终端内编辑器场景多为 None）。
    pub open_file: Option<String>,
    /// 最近执行的命令（新者在后）。
    pub recent_commands: VecDeque<String>,
    /// 最近遇到的错误（新者在后）。
    pub recent_errors: VecDeque<String>,
    /// 用户偏好（key -> value），如 shell 类型、包管理器等。
    pub prefs: std::collections::HashMap<String, String>,
}

/// 跨线程共享的记忆（被 `ToolCtx` / `SkillCtx` / `AiClient` 持有）。
pub type SharedMemory = Mutex<AiMemory>;

impl AiMemory {
    /// 记录一条执行过的命令（去重相邻重复）。
    pub fn record_command(&mut self, cmd: &str) {
        let cmd = cmd.trim().to_string();
        if cmd.is_empty() {
            return;
        }
        if self.recent_commands.back().map(|c| c.as_str()) != Some(cmd.as_str()) {
            self.recent_commands.push_back(cmd);
        }
        while self.recent_commands.len() > MAX_RECENT {
            self.recent_commands.pop_front();
        }
    }

    /// 记录一条错误（用于诊断技能回溯）。
    pub fn record_error(&mut self, err: &str) {
        let err = err.trim().to_string();
        if err.is_empty() {
            return;
        }
        self.recent_errors.push_back(err);
        while self.recent_errors.len() > MAX_RECENT {
            self.recent_errors.pop_front();
        }
    }

    /// 记录当前主机 / 会话。
    pub fn set_host(&mut self, host: &str) {
        if !host.trim().is_empty() {
            self.current_host = Some(host.trim().to_string());
        }
    }

    /// 记录当前聚焦的会话 / 项目名。
    pub fn set_project(&mut self, project: &str) {
        if !project.trim().is_empty() {
            self.current_project = Some(project.trim().to_string());
        }
    }

    /// 记录当前聚焦的终端 Tab 名称。
    pub fn set_tab(&mut self, tab: &str) {
        if !tab.trim().is_empty() {
            self.current_tab = Some(tab.trim().to_string());
        }
    }

    /// 记录当前 Workspace（会话容器）。
    pub fn set_workspace(&mut self, workspace: &str) {
        if !workspace.trim().is_empty() {
            self.current_workspace = Some(workspace.trim().to_string());
        }
    }

    /// 记录当前终端中选中的文本（用户复制前的上下文）。
    pub fn set_selection(&mut self, selection: &str) {
        let s = selection.trim().to_string();
        if s.is_empty() {
            self.selection = None;
        } else {
            // 选中文本可能很长，截断以避免污染上下文。
            let char_limit = 4000;
            self.selection = Some(if s.chars().count() > char_limit {
                format!("{}…", s.chars().take(char_limit).collect::<String>())
            } else {
                s
            });
        }
    }

    /// 记录当前打开的文件。
    pub fn set_open_file(&mut self, path: &str) {
        if !path.trim().is_empty() {
            self.open_file = Some(path.trim().to_string());
        }
    }

    /// 记录当前工作目录。
    pub fn set_cwd(&mut self, cwd: &str) {
        if !cwd.trim().is_empty() {
            self.current_dir = Some(cwd.trim().to_string());
        }
    }

    /// 记录一条用户偏好。
    pub fn set_pref(&mut self, key: &str, value: &str) {
        self.prefs.insert(key.to_string(), value.to_string());
    }

    /// 将记忆格式化为可注入 system prompt 的上下文文本。
    pub fn to_context_string(&self) -> String {
        let mut s = String::new();
        if let Some(p) = &self.current_project {
            s.push_str(&format!("- Connected session: {}\n", p));
        }
        if let Some(h) = &self.current_host {
            s.push_str(&format!("- Current host: {}\n", h));
        }
        if let Some(w) = &self.current_workspace {
            s.push_str(&format!("- Workspace: {}\n", w));
        }
        if let Some(t) = &self.current_tab {
            s.push_str(&format!("- Active tab: {}\n", t));
        }
        if let Some(d) = &self.current_dir {
            s.push_str(&format!("- Current directory: {}\n", d));
        }
        if let Some(f) = &self.open_file {
            s.push_str(&format!("- Open file: {}\n", f));
        }
        if let Some(sel) = &self.selection {
            s.push_str(&format!("- Selected text:\n```\n{}\n```\n", sel));
        }
        if !self.recent_commands.is_empty() {
            s.push_str("- Recent commands:\n");
            for c in self.recent_commands.iter().rev().take(10) {
                s.push_str(&format!("  - {}\n", c));
            }
        }
        if !self.recent_errors.is_empty() {
            s.push_str("- Recent errors:\n");
            for e in self.recent_errors.iter().rev().take(10) {
                s.push_str(&format!("  - {}\n", e));
            }
        }
        if !self.prefs.is_empty() {
            s.push_str("- User preferences:\n");
            for (k, v) in &self.prefs {
                s.push_str(&format!("  - {}: {}\n", k, v));
            }
        }
        s
    }

    /// 从磁盘加载记忆；文件不存在或解析失败则返回空记忆。
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// 持久化记忆到磁盘（父目录不存在时自动创建）。
    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_caps_recent_commands() {
        let mut m = AiMemory::default();
        for i in 0..(MAX_RECENT + 5) {
            m.record_command(&format!("cmd {}", i));
        }
        assert_eq!(m.recent_commands.len(), MAX_RECENT);
        assert_eq!(m.recent_commands.back().unwrap(), "cmd 34");
        // 相邻重复不追加
        m.record_command("cmd 34");
        assert_eq!(m.recent_commands.len(), MAX_RECENT);
    }

    #[test]
    fn context_string_includes_host_and_prefs() {
        let mut m = AiMemory::default();
        m.set_host("web-01");
        m.set_pref("shell", "zsh");
        m.record_error("permission denied");
        let c = m.to_context_string();
        assert!(c.contains("web-01"));
        assert!(c.contains("shell: zsh"));
        assert!(c.contains("permission denied"));
    }

    #[test]
    fn live_context_fields_render() {
        let mut m = AiMemory::default();
        m.set_project("web-01");
        m.set_workspace("web-01");
        m.set_tab("shell-2");
        m.set_cwd("/var/www");
        m.set_selection("traceback: ...");
        let c = m.to_context_string();
        assert!(c.contains("Connected session: web-01"));
        assert!(c.contains("Workspace: web-01"));
        assert!(c.contains("Active tab: shell-2"));
        assert!(c.contains("/var/www"));
        assert!(c.contains("Selected text:"));
        assert!(c.contains("traceback: ..."));
    }

    #[test]
    fn selection_truncated_when_long() {
        let mut m = AiMemory::default();
        let big = "x".repeat(5000);
        m.set_selection(&big);
        let s = m.selection.as_ref().unwrap();
        assert_eq!(s.chars().count(), 4001);
        assert!(s.ends_with('…'));
        // 空选中清空
        m.set_selection("   ");
        assert!(m.selection.is_none());
    }
}
