use gpui::Action;
use serde::{Deserialize, Serialize};

/// Represents a single keybinding configuration
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeybindingEntry {
    /// The keystroke string (e.g., "cmd-b", "ctrl-shift-d")
    pub keystroke: String,
    /// Optional context for the keybinding (e.g., "TerminalPane")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Optional VSCode style `when` clause (e.g. "terminalFocus")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// Command identifier (e.g. "workbench.view.explorer")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Whether this binding is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl KeybindingEntry {
    pub fn new(keystroke: impl Into<String>, context: Option<&str>) -> Self {
        Self {
            keystroke: keystroke.into(),
            context: context.map(String::from),
            when: context.map(String::from),
            command: None,
            enabled: true,
        }
    }

    pub fn with_when(mut self, when: impl Into<String>) -> Self {
        let w = when.into();
        self.when = Some(w.clone());
        self.context = Some(w);
        self
    }

    pub fn with_command(mut self, cmd: impl Into<String>) -> Self {
        self.command = Some(cmd.into());
        self
    }
}

/// Represents a conflict between two keybindings
#[derive(Clone, Debug)]
pub struct KeybindingConflict {
    pub keystroke: String,
    pub context: Option<String>,
    pub action1: String,
    pub action2: String,
    /// 冲突分类：Hard（错误）、Override（合法覆盖）或 ChordPrefix（前缀遮挡）。
    pub kind: ConflictKind,
    /// 仅 ChordPrefix 冲突时有值，记录被遮挡的完整和弦 keystroke。
    pub chord_keystroke: Option<String>,
}

impl std::fmt::Display for KeybindingConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ctx = self
            .context
            .as_ref()
            .map(|c| format!(" (in {})", c))
            .unwrap_or_default();
        match self.kind {
            ConflictKind::Hard => write!(
                f,
                "'{}'{} HARD conflict: {} vs {}",
                self.keystroke, ctx, self.action1, self.action2
            ),
            ConflictKind::Override => write!(
                f,
                "'{}'{} context override (legal): {} vs {}",
                self.keystroke, ctx, self.action1, self.action2
            ),
            ConflictKind::ChordPrefix => write!(
                f,
                "'{}'{} chord-prefix conflict: {} (single) blocks {} (chord: {})",
                self.keystroke,
                ctx,
                self.action1,
                self.action2,
                self.chord_keystroke.as_deref().unwrap_or("?")
            ),
        }
    }
}

/// 语义作用域：仅用于分类、文档与静态冲突分析，**不参与运行时分发**。
///
/// 运行时真正决定快捷键命中哪个 Action 的是 GPUI 的 KeyContext（即
/// `KeybindingEntry.context`，如 `None`/Global、`Sidebar`、`Editor`、`TerminalPane`）。
/// 本枚举刻意不实现 `Serialize`/`Deserialize`，仅为编辑器内静态元数据，
/// 避免引入序列化兼容负担或被误当作运行时路由使用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionScope {
    /// 应用内全局可达的快捷键（与具体焦点面板无关）。
    Global,
    /// 面板内语义（如重命名节点），通常绑定到特定面板的 KeyContext。
    Panel,
    /// 终端面板内的快捷键。
    Terminal,
}

/// 冲突分类：仅区分"真正错误"与"合法覆盖"，不做不可靠的可达性推断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// 同一 KeyContext 同一 keystroke 绑定到不同 action，属于错误。
    Hard,
    /// 不同 KeyContext 同一 keystroke，属于合法 context override，不报错。
    Override,
    /// 单键绑定是和弦绑定的前缀，会导致和弦永远无法触发。
    ChordPrefix,
}

/// Human-readable description of an action
#[derive(Clone)]
pub struct ActionDescription {
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    /// 语义作用域，仅用于分类与文档，不参与运行时分发。
    pub scope: ActionScope,
    /// 是否在命令面板（Command Palette）中展示。微观按键（如 Esc, Copy, Paste, Scroll 等）设为 false。
    pub show_in_palette: bool,
    /// Factory to create a boxed Action for dispatch
    pub factory: fn() -> Box<dyn Action>,
}
