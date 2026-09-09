//! `SessionDialogModel`：领域态（`config`/`baseline`）与瞬态（`ui`）显式分离，
//! 聚合输入收进 `inputs`。保存只持久化 `config`。
//!
//! Summary / ChangeSet 为**事件驱动缓存**：由变更事件调用 [`SessionDialogModel::recompute`]
//! 写入 `ui.summaries` / `ui.change_set`；render 只读缓存。

use std::collections::{HashMap, HashSet};

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{point, px, App, AppContext, Bounds, Context, Entity, EntityId, FocusHandle, Focusable, ListAlignment, ListState, Pixels, ScrollHandle, SharedString, WeakEntity, Window};
use velowork_state::{
    IconColor, KeepAliveStrategy, ProxyType, SshAuthType, SshSession, StrictHostKey,
};
use velowork_ui::tokens::SPACE_CARD_GAP;
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::select::{SelectEvent, SelectOption, SelectState};
use crate::views::overlays::dialogs::session_dialog::render::dropdown_option_list;
use velowork_ui::input::InputState;

use super::changeset::{recompute_change_set, ChangeSet};
use super::inputs::SessionDialogInputs;
use super::parser::ConnectionParserPipeline;
use super::section::{visible_sections, SshSection, BUILTIN_SECTIONS};
use super::validation::{validate_value, FieldId, ValidationState};
use crate::views::panels::session_panel::SessionPanel;

/// SSH 算法类别（用于分类 Tab 切换）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlgorithmCategory {
    #[default]
    Kex,
    Cipher,
    Mac,
    HostKey,
}

impl AlgorithmCategory {
    pub const ALL: &'static [Self] = &[Self::Kex, Self::Cipher, Self::Mac, Self::HostKey];

    pub const fn key(&self) -> &'static str {
        match self {
            Self::Kex => "kex",
            Self::Cipher => "cipher",
            Self::Mac => "mac",
            Self::HostKey => "hostkey",
        }
    }

    pub const fn title_key(&self) -> &'static str {
        match self {
            Self::Kex => "ssh.tab.russh_kex",
            Self::Cipher => "ssh.tab.russh_cipher",
            Self::Mac => "ssh.tab.russh_mac",
            Self::HostKey => "ssh.tab.russh_hostkey",
        }
    }

    pub const fn desc_key(&self) -> &'static str {
        match self {
            Self::Kex => "ssh.advanced.kex_desc",
            Self::Cipher => "ssh.advanced.cipher_desc",
            Self::Mac => "ssh.advanced.mac_desc",
            Self::HostKey => "ssh.advanced.hostkey_desc",
        }
    }
}

/// 对话框瞬态 UI 状态（不持久化）。
pub struct SessionDialogUiState {
    pub expanded_sections: HashSet<SshSection>,
    pub active_section: SshSection,
    pub nav_search: String,
    pub validation: ValidationState,
    pub change_set: ChangeSet,
    pub hover: Option<SshSection>,
    /// 算法高级折叠（Automatic 时的 ▼ Advanced）。
    pub algorithms_advanced_open: bool,
    /// 算法分类 Tab（密钥交换 / 加密 / MAC / 主机密钥）。
    pub algorithm_category: AlgorithmCategory,
    /// 当前打开的下拉标识（None = 无）。
    pub dropdown_open: Option<&'static str>,
    /// 各下拉触发按钮的窗口绝对包围盒（按 key 分别存储，用于 overlay 定位）。
    pub dropdown_bounds: HashMap<String, Bounds<Pixels>>,
    /// 所属面板实体 id（用于 notify 重渲染）。
    pub panel_id: Option<EntityId>,
    /// 所属面板实体弱引用（供下拉 overlay 的全局 ClickOutside 关闭回调使用）。
    pub panel_weak: Option<WeakEntity<SessionPanel>>,
    /// 右侧卡片区滚动句柄（用于导航点击后自动滚动到目标卡片）。
    pub scroll_handle: ScrollHandle,
    /// 右侧卡片区 GPUI ListState（用于 ListOffset 定位精准滚动）。
    pub list_state: ListState,
    /// 导航触发的待滚动目标 section（下帧渲染后清除）。
    pub pending_scroll: Cell<Option<SshSection>>,
    /// Tab 焦点切换触发的待自适应视口滚动句柄（视口未完成首次布局时暂存）
    pub pending_scroll_focus_handle: Cell<Option<FocusHandle>>,
    /// 各 section 卡片动态布局高度缓存（基于 canvas 测量，用于精准算距贴顶）
    pub card_heights: Rc<RefCell<HashMap<SshSection, f32>>>,
    /// 首次渲染时自动聚焦标记（防止后续渲染帧重复抢夺焦点）。
    pub auto_focused: bool,
    /// 是否处于新建所属目录编辑态
    pub creating_parent_folder: bool,
    /// 新建所属目录输入框实体
    pub parent_folder_input: Option<Entity<InputState>>,
}

impl Default for SessionDialogUiState {
    fn default() -> Self {
        let mut expanded = HashSet::new();
        for d in BUILTIN_SECTIONS {
            if d.default_expand {
                expanded.insert(d.id);
            }
        }
        Self {
            expanded_sections: expanded,
            active_section: SshSection::Basic,
            nav_search: String::new(),
            validation: ValidationState::new(),
            change_set: ChangeSet::default(),
            hover: None,
            algorithms_advanced_open: false,
            algorithm_category: AlgorithmCategory::Kex,
            dropdown_open: None,
            dropdown_bounds: HashMap::new(),
            panel_id: None,
            panel_weak: None,
            scroll_handle: ScrollHandle::new(),
            list_state: ListState::new(BUILTIN_SECTIONS.len(), ListAlignment::Top, px(1000.0)),
            pending_scroll: Cell::new(None),
            pending_scroll_focus_handle: Cell::new(None),
            card_heights: Rc::new(RefCell::new(HashMap::new())),
            auto_focused: false,
            creating_parent_folder: false,
            parent_folder_input: None,
        }
    }
}

/// 对话框数据模型。
pub struct SessionDialogModel {
    pub config: SshSession,
    pub baseline: SshSession,
    pub inputs: SessionDialogInputs,
    pub ui: SessionDialogUiState,
    /// 编辑态会话 id（None = 新建）。
    pub editing_id: Option<String>,
    /// 下拉选择组件集合（替代原 dropdown_field 手写下拉）。
    pub selects: SessionDialogSelects,
    /// 键盘导航焦点句柄集合（Tab / Shift+Tab 顺序与空格/回车激活）。
    pub focus: SessionDialogFocus,
}

/// 新建会话弹窗的可键盘聚焦元素焦点句柄集合。
///
/// 所有句柄在 [`SessionDialogModel::new`] 中创建并保持稳定，
/// 以保证 Tab 顺序固定且焦点环可正确显示。
pub struct SessionDialogFocus {
    /// 左侧导航栏各分组按钮。
    pub nav: HashMap<SshSection, FocusHandle>,
    /// 右侧每个卡片的头部（点击展开/收起）。
    pub card_header: HashMap<SshSection, FocusHandle>,
    /// Connection：启用 SFTP / 监控。
    pub sftp: FocusHandle,
    pub monitor: FocusHandle,
    /// Connection：监控子项 CPU/内存/磁盘。
    pub monitor_cpu: FocusHandle,
    pub monitor_mem: FocusHandle,
    pub monitor_disk: FocusHandle,
    /// Authentication：认证方式单选按钮。
    pub auth_method: FocusHandle,
    /// Terminal：光标样式、光标闪烁、铃声类型 RadioGroup。
    pub cursor_shape: FocusHandle,
    pub cursor_blink: FocusHandle,
    pub bell_style: FocusHandle,
    /// Terminal：四项增强选项（Shell集成、括号粘贴、OSC52、真彩色）。
    pub term_shell_integration: FocusHandle,
    pub term_bracketed_paste: FocusHandle,
    pub term_osc52: FocusHandle,
    pub term_true_color: FocusHandle,
    /// Network：TCP_NODELAY 开关。
    pub net_tcp_nodelay: FocusHandle,
    /// Network：启用 X11 转发开关。
    pub net_x11_enable: FocusHandle,
    /// Network：启用 SSH Agent 转发开关。
    pub net_agent_fwd_enable: FocusHandle,
    /// Network：启用代理开关。
    pub proxy: FocusHandle,
    /// Advanced：算法自动开关。
    pub adv_algo_auto: FocusHandle,
    /// Advanced：高级算法展开按钮。
    pub algo_advanced: FocusHandle,
    /// Serial：引脚与通信开关。
    pub serial_dtr: FocusHandle,
    pub serial_rts: FocusHandle,
    pub serial_local_echo: FocusHandle,
    pub serial_timestamps: FocusHandle,
    pub serial_auto_reconnect: FocusHandle,
    /// 顶部/底部操作栏按钮。
    pub test: FocusHandle,
    pub cancel: FocusHandle,
    pub save: FocusHandle,
    /// 目录选择区：新增目录按钮、确认与取消按钮。
    pub new_parent_folder: FocusHandle,
    pub dir_confirm: FocusHandle,
    pub dir_cancel: FocusHandle,
}

/// 下拉选择组件集合。每个字段对应会话配置中的一个枚举/列表型字段，
/// 值统一以 `SharedString` 承载，与原 dropdown 的 value 约定（枚举用 `{:?}`）保持一致。
pub struct SessionDialogSelects {
    pub proxy_type: Entity<SelectState<SharedString>>,
    pub jump_session: Entity<SelectState<SharedString>>,
    pub terminal_type: Entity<SelectState<SharedString>>,
    pub charset: Entity<SelectState<SharedString>>,
    pub font_family: Entity<SelectState<SharedString>>,
    pub color_scheme: Entity<SelectState<SharedString>>,
    pub shell_integration: Entity<SelectState<SharedString>>,
    pub bracketed_paste: Entity<SelectState<SharedString>>,
    pub osc52_clipboard: Entity<SelectState<SharedString>>,
    pub true_color: Entity<SelectState<SharedString>>,
    pub compression: Entity<SelectState<SharedString>>,
    pub strict_host: Entity<SelectState<SharedString>>,
    pub parent_folder: Entity<SelectState<SharedString>>,
    pub serial_baud_rate: Entity<SelectState<SharedString>>,
    pub serial_data_bits: Entity<SelectState<SharedString>>,
    pub serial_stop_bits: Entity<SelectState<SharedString>>,
    pub serial_parity: Entity<SelectState<SharedString>>,
    pub serial_flow_control: Entity<SelectState<SharedString>>,
    pub serial_display_mode: Entity<SelectState<SharedString>>,
    pub serial_line_ending: Entity<SelectState<SharedString>>,
    pub cursor_shape: Entity<SelectState<SharedString>>,
    pub cursor_blink: Entity<SelectState<SharedString>>,
    pub telnet_encoding: Entity<SelectState<SharedString>>,
    pub local_shell: Entity<SelectState<SharedString>>,
}

impl SessionDialogSelects {
    /// 创建全部 SelectState 实体（占位，options/selected 由 `setup` 填充）。
    pub fn new<T: 'static>(cx: &mut Context<T>) -> Self {
        let font_family = cx.new(|cx| SelectState::new(cx).searchable(true).virtual_scroll(true));
        let mut mk = || cx.new(|cx| SelectState::new(cx));
        Self {
            proxy_type: mk(),
            jump_session: mk(),
            terminal_type: mk(),
            charset: mk(),
            font_family,
            color_scheme: mk(),
            shell_integration: mk(),
            bracketed_paste: mk(),
            osc52_clipboard: mk(),
            true_color: mk(),
            compression: mk(),
            strict_host: mk(),
            parent_folder: mk(),
            serial_baud_rate: mk(),
            serial_data_bits: mk(),
            serial_stop_bits: mk(),
            serial_parity: mk(),
            serial_flow_control: mk(),
            serial_display_mode: mk(),
            serial_line_ending: mk(),
            cursor_shape: mk(),
            cursor_blink: mk(),
            telnet_encoding: mk(),
            local_shell: mk(),
        }
    }

    /// 按 key 取得对应 SelectState 实体。
    pub fn get(&self, key: &str) -> Option<&Entity<SelectState<SharedString>>> {
        match key {
            SELECT_KEY_PARENT_FOLDER => Some(&self.parent_folder),
            SELECT_KEY_PROXY_TYPE => Some(&self.proxy_type),
            SELECT_KEY_JUMP_SESSION => Some(&self.jump_session),
            SELECT_KEY_TERMINAL_TYPE => Some(&self.terminal_type),
            SELECT_KEY_CHARSET => Some(&self.charset),
            SELECT_KEY_FONT_FAMILY => Some(&self.font_family),
            SELECT_KEY_COLOR_SCHEME => Some(&self.color_scheme),
            SELECT_KEY_SHELL_INTEGRATION => Some(&self.shell_integration),
            SELECT_KEY_BRACKETED_PASTE => Some(&self.bracketed_paste),
            SELECT_KEY_OSC52_CLIPBOARD => Some(&self.osc52_clipboard),
            SELECT_KEY_TRUE_COLOR => Some(&self.true_color),
            SELECT_KEY_COMPRESSION => Some(&self.compression),
            SELECT_KEY_STRICT_HOST => Some(&self.strict_host),
            SELECT_KEY_SERIAL_BAUD => Some(&self.serial_baud_rate),
            SELECT_KEY_SERIAL_DATA_BITS => Some(&self.serial_data_bits),
            SELECT_KEY_SERIAL_STOP_BITS => Some(&self.serial_stop_bits),
            SELECT_KEY_SERIAL_PARITY => Some(&self.serial_parity),
            SELECT_KEY_SERIAL_FLOW_CONTROL => Some(&self.serial_flow_control),
            SELECT_KEY_SERIAL_DISPLAY_MODE => Some(&self.serial_display_mode),
            SELECT_KEY_SERIAL_LINE_ENDING => Some(&self.serial_line_ending),
            SELECT_KEY_CURSOR_SHAPE => Some(&self.cursor_shape),
            SELECT_KEY_CURSOR_BLINK => Some(&self.cursor_blink),
            SELECT_KEY_TELNET_ENCODING => Some(&self.telnet_encoding),
            SELECT_KEY_LOCAL_SHELL => Some(&self.local_shell),
            _ => None,
        }
    }

    /// 为全部 SelectState 注入 OverlayRegistry 句柄，支持点击外部区域自动收起。
    pub fn set_overlay_registry(&self, reg: Entity<OverlayRegistry>, cx: &mut App) {
        self.proxy_type.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.jump_session.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.terminal_type.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.charset.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.font_family.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.color_scheme.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.shell_integration.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.bracketed_paste.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.osc52_clipboard.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.true_color.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.compression.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.strict_host.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.parent_folder.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.serial_baud_rate.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.serial_data_bits.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.serial_stop_bits.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.serial_parity.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.serial_flow_control.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.serial_display_mode.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.serial_line_ending.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.cursor_shape.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.cursor_blink.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.telnet_encoding.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.local_shell.update(cx, |s, _| s.set_overlay_registry(reg.clone()));
    }
}

/// 各下拉的标识键（用于同步 config 与 select 值）。
pub const SELECT_KEY_PARENT_FOLDER: &str = "parent_folder";
pub const SELECT_KEY_PROXY_TYPE: &str = "proxy_type";
pub const SELECT_KEY_JUMP_SESSION: &str = "jump_session";
pub const SELECT_KEY_TERMINAL_TYPE: &str = "terminal_type";
pub const SELECT_KEY_CHARSET: &str = "charset";
pub const SELECT_KEY_FONT_FAMILY: &str = "font_family";
pub const SELECT_KEY_COLOR_SCHEME: &str = "color_scheme";
pub const SELECT_KEY_SHELL_INTEGRATION: &str = "shell_integration";
pub const SELECT_KEY_BRACKETED_PASTE: &str = "bracketed_paste";
pub const SELECT_KEY_OSC52_CLIPBOARD: &str = "osc52_clipboard";
pub const SELECT_KEY_TRUE_COLOR: &str = "true_color";
pub const SELECT_KEY_COMPRESSION: &str = "compression";
pub const SELECT_KEY_STRICT_HOST: &str = "strict_host";
pub const SELECT_KEY_SERIAL_BAUD: &str = "serial_baud";
pub const SELECT_KEY_SERIAL_DATA_BITS: &str = "serial_data_bits";
pub const SELECT_KEY_SERIAL_STOP_BITS: &str = "serial_stop_bits";
pub const SELECT_KEY_SERIAL_PARITY: &str = "serial_parity";
pub const SELECT_KEY_SERIAL_FLOW_CONTROL: &str = "serial_flow_control";
pub const SELECT_KEY_SERIAL_DISPLAY_MODE: &str = "serial_display_mode";
pub const SELECT_KEY_SERIAL_LINE_ENDING: &str = "serial_line_ending";
pub const SELECT_KEY_CURSOR_SHAPE: &str = "cursor_shape";
pub const SELECT_KEY_CURSOR_BLINK: &str = "cursor_blink";
pub const SELECT_KEY_TELNET_ENCODING: &str = "telnet_encoding";
pub const SELECT_KEY_LOCAL_SHELL: &str = "local_shell";

impl SessionDialogModel {
    /// 新建（空白）对话框模型。
    pub fn new<T: 'static>(window: &mut Window, cx: &mut Context<T>) -> Self {
        let inputs = SessionDialogInputs::new(window, cx);
        let config = default_config();
        let mut model = Self {
            baseline: config.clone(),
            config,
            inputs,
            ui: SessionDialogUiState::default(),
            editing_id: None,
            selects: SessionDialogSelects::new(cx),
            focus: SessionDialogFocus {
                nav: BUILTIN_SECTIONS.iter().map(|d| (d.id, cx.focus_handle())).collect(),
                card_header: BUILTIN_SECTIONS.iter().map(|d| (d.id, cx.focus_handle())).collect(),
                sftp: cx.focus_handle(),
                monitor: cx.focus_handle(),
                monitor_cpu: cx.focus_handle(),
                monitor_mem: cx.focus_handle(),
                monitor_disk: cx.focus_handle(),
                auth_method: cx.focus_handle(),
                cursor_shape: cx.focus_handle(),
                cursor_blink: cx.focus_handle(),
                bell_style: cx.focus_handle(),
                term_shell_integration: cx.focus_handle(),
                term_bracketed_paste: cx.focus_handle(),
                term_osc52: cx.focus_handle(),
                term_true_color: cx.focus_handle(),
                net_tcp_nodelay: cx.focus_handle(),
                net_x11_enable: cx.focus_handle(),
                net_agent_fwd_enable: cx.focus_handle(),
                proxy: cx.focus_handle(),
                adv_algo_auto: cx.focus_handle(),
                algo_advanced: cx.focus_handle(),
                serial_dtr: cx.focus_handle(),
                serial_rts: cx.focus_handle(),
                serial_local_echo: cx.focus_handle(),
                serial_timestamps: cx.focus_handle(),
                serial_auto_reconnect: cx.focus_handle(),
                test: cx.focus_handle(),
                cancel: cx.focus_handle(),
                save: cx.focus_handle(),
                new_parent_folder: cx.focus_handle(),
                dir_confirm: cx.focus_handle(),
                dir_cancel: cx.focus_handle(),
            },
        };
        model.apply_config_to_inputs(window, cx);
        model.recompute();
        model
    }

    /// 载入既有会话进行编辑：设置 config + baseline，并回填输入。
    pub fn load(&mut self, session: SshSession, window: &mut Window, cx: &mut App) {
        self.editing_id = Some(session.id.clone());
        self.config = session.clone();
        self.baseline = session;
        self.apply_config_to_inputs(window, cx);
        self.ui.validation.clear();
        self.recompute();
    }

    /// 重置为新建空白态。
    pub fn reset(&mut self, window: &mut Window, cx: &mut App) {
        self.editing_id = None;
        self.config = default_config();
        self.baseline = self.config.clone();
        self.apply_config_to_inputs(window, cx);
        self.ui = SessionDialogUiState::default();
        self.recompute();
    }

    /// 是否有未保存修改。
    pub fn is_dirty(&self) -> bool {
        self.config != self.baseline
    }

    /// 是否可保存（无 Error）。
    pub fn is_valid(&self) -> bool {
        !self.ui.validation.has_error()
    }

    /// 事件驱动重算 ChangeSet（缓存进 ui）。
    pub fn recompute(&mut self) {
        self.ui.change_set = recompute_change_set(&self.config, &self.baseline);
    }

    /// 从单个文本输入同步进 config（轻量按键同步，避免全量遍历）。
    pub fn sync_field_from_input(&mut self, fid: FieldId, cx: &App) {
        let read = |e: &Entity<InputState>| e.read(cx).text().to_string().trim().to_string();
        match fid {
            FieldId::Name => self.config.name = read(&self.inputs.name),
            FieldId::Host => self.config.host = read(&self.inputs.host),
            FieldId::Username => self.config.username = read(&self.inputs.username),
            FieldId::Port => self.config.port = read(&self.inputs.port).parse().unwrap_or(self.config.port),
            FieldId::SerialPort => {
                let v = read(&self.inputs.serial_port);
                self.config.serial_port = if v.is_empty() { None } else { Some(v) };
            }
            FieldId::TelnetHost => {
                let v = read(&self.inputs.telnet_host);
                self.config.telnet_host = if v.is_empty() { None } else { Some(v) };
            }
            FieldId::TelnetPort => {
                self.config.telnet_port = read(&self.inputs.telnet_port).parse().unwrap_or(self.config.telnet_port);
            }
            FieldId::ConnectionTimeout => {
                self.config.connection_timeout = read(&self.inputs.connection_timeout)
                    .parse()
                    .unwrap_or(self.config.connection_timeout);
            }
            FieldId::KeepaliveInterval => {
                self.config.keep_alive_interval = read(&self.inputs.keepalive_interval)
                    .parse()
                    .unwrap_or(self.config.keep_alive_interval);
            }
            FieldId::KeepaliveMax => {
                self.config.keep_alive_max = read(&self.inputs.keepalive_max)
                    .parse()
                    .unwrap_or(self.config.keep_alive_max);
            }
            FieldId::IdleTimeout => {
                self.config.idle_disconnect_timeout = read(&self.inputs.idle_disconnect)
                    .parse()
                    .unwrap_or(self.config.idle_disconnect_timeout);
            }
            FieldId::RekeyTime => {
                self.config.rekey_time = read(&self.inputs.rekey_time).parse().unwrap_or(self.config.rekey_time);
            }
            FieldId::GexMin => {
                self.config.gex_min = read(&self.inputs.gex_min).parse().unwrap_or(self.config.gex_min);
            }
            FieldId::GexPreferred => {
                self.config.gex_preferred = read(&self.inputs.gex_preferred).parse().unwrap_or(self.config.gex_preferred);
            }
            FieldId::GexMax => {
                self.config.gex_max = read(&self.inputs.gex_max).parse().unwrap_or(self.config.gex_max);
            }
            FieldId::MaxPackets => {
                self.config.max_packets = read(&self.inputs.max_packets).parse().unwrap_or(self.config.max_packets);
            }
            FieldId::RecvWindow => {
                self.config.recv_window = read(&self.inputs.recv_window).parse().unwrap_or(self.config.recv_window);
            }
            FieldId::Scrollback => {
                let scrollback = read(&self.inputs.scrollback);
                self.config.terminal.scrollback_lines = if scrollback.is_empty() { None } else { scrollback.parse().ok() };
            }
            FieldId::Password => {
                if let SshAuthType::Password { password } = &mut self.config.auth_type {
                    let v = read(&self.inputs.password);
                    *password = if v.is_empty() { None } else { Some(v) };
                }
            }
            FieldId::KeyPath => {
                if let SshAuthType::PrivateKey { key_path, .. } = &mut self.config.auth_type {
                    *key_path = read(&self.inputs.key_path);
                }
            }
            FieldId::Passphrase => {
                if let SshAuthType::PrivateKey { passphrase, .. } = &mut self.config.auth_type {
                    let v = read(&self.inputs.passphrase);
                    *passphrase = if v.is_empty() { None } else { Some(v) };
                }
            }
            FieldId::StartupCommand => {
                let v = read(&self.inputs.startup_command);
                self.config.startup_command = if v.is_empty() { None } else { Some(v) };
            }
            FieldId::Notes => {
                let v = read(&self.inputs.notes);
                self.config.notes = if v.is_empty() { None } else { Some(v) };
            }
            FieldId::WordSeparators => {
                let v = read(&self.inputs.word_separators);
                self.config.terminal.word_separators = if v.is_empty() { None } else { Some(v) };
            }
            _ => {}
        }
    }

    /// 从文本输入同步进 config（数值/字符串字段），随后应 recompute。
    pub fn sync_config_from_inputs(&mut self, cx: &App) {
        let read = |e: &Entity<InputState>| e.read(cx).text().to_string().trim().to_string();

        self.config.name = read(&self.inputs.name);
        self.config.host = read(&self.inputs.host);
        self.config.username = read(&self.inputs.username);
        self.config.port = read(&self.inputs.port).parse().unwrap_or(self.config.port);
        self.config.serial_port = {
            let v = read(&self.inputs.serial_port);
            if v.is_empty() { None } else { Some(v) }
        };
        self.config.telnet_host = {
            let v = read(&self.inputs.telnet_host);
            if v.is_empty() { None } else { Some(v) }
        };
        self.config.telnet_port = read(&self.inputs.telnet_port).parse().unwrap_or(self.config.telnet_port);
        self.config.connection_timeout = read(&self.inputs.connection_timeout)
            .parse()
            .unwrap_or(self.config.connection_timeout);
        self.config.keep_alive_interval = read(&self.inputs.keepalive_interval)
            .parse()
            .unwrap_or(self.config.keep_alive_interval);
        self.config.keep_alive_max = read(&self.inputs.keepalive_max)
            .parse()
            .unwrap_or(self.config.keep_alive_max);
        self.config.idle_disconnect_timeout = read(&self.inputs.idle_disconnect)
            .parse()
            .unwrap_or(self.config.idle_disconnect_timeout);
        self.config.rekey_time =
            read(&self.inputs.rekey_time).parse().unwrap_or(self.config.rekey_time);
        self.config.gex_min =
            read(&self.inputs.gex_min).parse().unwrap_or(self.config.gex_min);
        self.config.gex_preferred = read(&self.inputs.gex_preferred)
            .parse()
            .unwrap_or(self.config.gex_preferred);
        self.config.gex_max =
            read(&self.inputs.gex_max).parse().unwrap_or(self.config.gex_max);
        self.config.max_packets = read(&self.inputs.max_packets)
            .parse()
            .unwrap_or(self.config.max_packets);
        self.config.recv_window = read(&self.inputs.recv_window)
            .parse()
            .unwrap_or(self.config.recv_window);

        let scrollback = read(&self.inputs.scrollback);
        self.config.terminal.scrollback_lines =
            if scrollback.is_empty() { None } else { scrollback.parse().ok() };

        let bell_cooldown = read(&self.inputs.bell_cooldown_ms);
        self.config.terminal.bell_cooldown_ms =
            if bell_cooldown.is_empty() { None } else { bell_cooldown.parse().ok() };

        let opt = |s: String| if s.is_empty() { None } else { Some(s) };
        self.config.terminal.word_separators = opt(read(&self.inputs.word_separators));
        self.config.terminal.font_size = read(&self.inputs.font_size).parse().ok();
        self.config.local_cwd = opt(read(&self.inputs.local_cwd));

        // 认证相关随 auth_type 同步
        match &mut self.config.auth_type {
            SshAuthType::Password { password } => {
                let v = read(&self.inputs.password);
                *password = if v.is_empty() { None } else { Some(v) };
            }
            SshAuthType::PrivateKey { key_path, passphrase } => {
                *key_path = read(&self.inputs.key_path);
                let v = read(&self.inputs.passphrase);
                *passphrase = if v.is_empty() { None } else { Some(v) };
            }
            SshAuthType::SshAgent { socket_path } => {
                let v = read(&self.inputs.agent_socket_path);
                *socket_path = if v.is_empty() { None } else { Some(v) };
            }
            _ => {}
        }

        // 代理字段（仅在启用代理时有意义，但同步无害）
        if self.config.proxy_type != ProxyType::None {
            self.config.proxy_host = opt(read(&self.inputs.proxy_host));
            self.config.proxy_port = read(&self.inputs.proxy_port).parse().ok();
            self.config.proxy_username = opt(read(&self.inputs.proxy_username));
            self.config.proxy_password = opt(read(&self.inputs.proxy_password));
        } else {
            self.config.proxy_host = None;
            self.config.proxy_port = None;
            self.config.proxy_username = None;
            self.config.proxy_password = None;
        }

        self.config.totp_secret = opt(read(&self.inputs.totp_secret));
        self.config.x11_display = opt(read(&self.inputs.x11_display));
        self.config.startup_command = opt(read(&self.inputs.startup_command));
        self.config.tags = opt(read(&self.inputs.tags));
        self.config.notes = opt(read(&self.inputs.notes));
    }

    /// 把 config 写回全部输入（load / template / advisor / undo 后调用）。
    pub fn apply_config_to_inputs(&self, window: &mut Window, cx: &mut App) {
        let set = |e: &Entity<InputState>, v: &str, _window: &mut Window, cx: &mut App| {
            e.update(cx, |s, cx| s.set_value(v, cx));
        };
        set(&self.inputs.name, &self.config.name, window, cx);
        set(&self.inputs.host, &self.config.host, window, cx);
        set(&self.inputs.username, &self.config.username, window, cx);
        set(&self.inputs.port, &self.config.port.to_string(), window, cx);
        set(&self.inputs.serial_port, self.config.serial_port.as_deref().unwrap_or(""), window, cx);
        set(&self.inputs.telnet_host, self.config.telnet_host.as_deref().unwrap_or(""), window, cx);
        set(&self.inputs.telnet_port, &self.config.telnet_port.to_string(), window, cx);
        set(&self.inputs.connection_timeout, &self.config.connection_timeout.to_string(), window, cx);
        set(&self.inputs.keepalive_interval, &self.config.keep_alive_interval.to_string(), window, cx);
        set(&self.inputs.keepalive_max, &self.config.keep_alive_max.to_string(), window, cx);
        set(&self.inputs.idle_disconnect, &self.config.idle_disconnect_timeout.to_string(), window, cx);
        set(&self.inputs.rekey_time, &self.config.rekey_time.to_string(), window, cx);
        set(&self.inputs.gex_min, &self.config.gex_min.to_string(), window, cx);
        set(
            &self.inputs.gex_preferred,
            &self.config.gex_preferred.to_string(),
            window,
            cx,
        );
        set(&self.inputs.gex_max, &self.config.gex_max.to_string(), window, cx);
        set(&self.inputs.max_packets, &self.config.max_packets.to_string(), window, cx);
        set(&self.inputs.recv_window, &self.config.recv_window.to_string(), window, cx);
        set(
            &self.inputs.scrollback,
            &self.config.terminal.scrollback_lines.map(|v| v.to_string()).unwrap_or_default(),
            window,
            cx,
        );
        set(
            &self.inputs.bell_cooldown_ms,
            &self.config.terminal.bell_cooldown_ms.map(|v| v.to_string()).unwrap_or_default(),
            window,
            cx,
        );
        set(
            &self.inputs.word_separators,
            self.config.terminal.word_separators.as_deref().unwrap_or(""),
            window,
            cx,
        );
        set(
            &self.inputs.font_size,
            &self.config.terminal.font_size.map(|v| v.to_string()).unwrap_or_default(),
            window,
            cx,
        );
        set(
            &self.inputs.local_cwd,
            self.config.local_cwd.as_deref().unwrap_or(""),
            window,
            cx,
        );
        set(&self.inputs.totp_secret, self.config.totp_secret.as_deref().unwrap_or(""), window, cx);
        set(&self.inputs.x11_display, self.config.x11_display.as_deref().unwrap_or(""), window, cx);
        set(&self.inputs.startup_command, self.config.startup_command.as_deref().unwrap_or(""), window, cx);
        set(&self.inputs.tags, self.config.tags.as_deref().unwrap_or(""), window, cx);
        set(&self.inputs.notes, self.config.notes.as_deref().unwrap_or(""), window, cx);
        set(&self.inputs.proxy_host, self.config.proxy_host.as_deref().unwrap_or(""), window, cx);
        set(
            &self.inputs.proxy_port,
            &self.config.proxy_port.map(|v| v.to_string()).unwrap_or_default(),
            window,
            cx,
        );
        set(&self.inputs.proxy_username, self.config.proxy_username.as_deref().unwrap_or(""), window, cx);
        set(&self.inputs.proxy_password, self.config.proxy_password.as_deref().unwrap_or(""), window, cx);

        match &self.config.auth_type {
            SshAuthType::Password { password } => {
                set(&self.inputs.password, password.as_deref().unwrap_or(""), window, cx);
            }
            SshAuthType::PrivateKey { key_path, passphrase } => {
                set(&self.inputs.key_path, key_path, window, cx);
                set(&self.inputs.passphrase, passphrase.as_deref().unwrap_or(""), window, cx);
            }
            SshAuthType::SshAgent { socket_path } => {
                set(&self.inputs.agent_socket_path, socket_path.as_deref().unwrap_or(""), window, cx);
            }
            _ => {}
        }
    }

    /// 单字段校验（写入 ui.validation）。
    pub fn validate_field(&mut self, id: FieldId, cx: &App) {
        if !id.is_applicable(self.config.protocol) {
            self.ui.validation.set(id, None);
            return;
        }
        let raw = self.input_for(id).map(|e| e.read(cx).text().to_string());
        let result = raw.and_then(|v| validate_value(id, &v, cx));
        self.ui.validation.set(id, result);
    }

    /// 全字段校验，返回首个 Error 字段（若有）。
    pub fn validate_all(&mut self, cx: &App) -> Option<FieldId> {
        for &id in FieldId::ALL {
            self.validate_field(id, cx);
        }
        self.ui.validation.first_error_field()
    }

    /// Host 智能解析：回填 username/port（仅覆盖能解析出的部分）。
    pub fn parse_host_and_backfill(&mut self, _window: &mut Window, cx: &mut App) -> bool {
        let raw = self.inputs.host.read(cx).text().to_string();
        let pipeline = ConnectionParserPipeline::with_builtins();
        let mut backfilled = false;
        if let Some(target) = pipeline.parse(&raw) {
            // 仅当解析出 user@host 结构（即包含分隔符）时才回填，避免纯 host 覆盖
            if let Some(host) = target.host {
                if raw.contains('@') || raw.contains(' ') || raw.contains("://") {
                    self.inputs.host.update(cx, |s, cx| s.set_value(&host, cx));
                    backfilled = true;
                }
            }
            if let Some(user) = target.username {
                self.inputs.username.update(cx, |s, cx| s.set_value(&user, cx));
                backfilled = true;
            }
            if let Some(port) = target.port {
                self.inputs.port.update(cx, |s, cx| s.set_value(&port.to_string(), cx));
                backfilled = true;
            }
        }
        backfilled
    }

    /// FieldId → 对应输入实体（无对应则 None，例如枚举/开关类字段）。
    pub fn input_for(&self, id: FieldId) -> Option<&Entity<InputState>> {
        Some(match id {
            FieldId::Name => &self.inputs.name,
            FieldId::Host => &self.inputs.host,
            FieldId::Port => &self.inputs.port,
            FieldId::SerialPort => &self.inputs.serial_port,
            FieldId::TelnetHost => &self.inputs.telnet_host,
            FieldId::TelnetPort => &self.inputs.telnet_port,
            FieldId::Username => &self.inputs.username,
            FieldId::Password => &self.inputs.password,
            FieldId::KeyPath => &self.inputs.key_path,
            FieldId::Passphrase => &self.inputs.passphrase,
            FieldId::ConnectionTimeout => &self.inputs.connection_timeout,
            FieldId::KeepaliveInterval => &self.inputs.keepalive_interval,
            FieldId::KeepaliveMax => &self.inputs.keepalive_max,
            FieldId::IdleTimeout => &self.inputs.idle_disconnect,
            FieldId::ProxyHost => &self.inputs.proxy_host,
            FieldId::ProxyPort => &self.inputs.proxy_port,
            FieldId::Scrollback => &self.inputs.scrollback,
            FieldId::RekeyTime => &self.inputs.rekey_time,
            FieldId::GexMin => &self.inputs.gex_min,
            FieldId::GexPreferred => &self.inputs.gex_preferred,
            FieldId::GexMax => &self.inputs.gex_max,
            FieldId::MaxPackets => &self.inputs.max_packets,
            FieldId::RecvWindow => &self.inputs.recv_window,
            FieldId::StartupCommand => &self.inputs.startup_command,
            FieldId::ProxyUsername => &self.inputs.proxy_username,
            FieldId::ProxyPassword => &self.inputs.proxy_password,
            FieldId::Tags => &self.inputs.tags,
            FieldId::Notes => &self.inputs.notes,
            FieldId::WordSeparators => &self.inputs.word_separators,
            // 无文本输入的字段（枚举/开关/dropdown 驱动）
            FieldId::ChannelBuffer => return None,
        })
    }

    /// 构造用于持久化的 `SshSession`（补齐 id）。
    pub fn to_session(&self, id: String) -> SshSession {
        let mut s = self.config.clone();
        s.id = id;
        s.save_credentials = true;
        s
    }

    /// 展开/折叠分组。
    pub fn toggle_section(&mut self, section: SshSection) {
        if self.ui.expanded_sections.contains(&section) {
            self.ui.expanded_sections.remove(&section);
        } else {
            self.ui.expanded_sections.insert(section);
        }
    }

    pub fn is_expanded(&self, section: SshSection) -> bool {
        self.ui.expanded_sections.contains(&section)
    }

    /// 将所有下拉的「选项 + 当前选中值」同步到对应的 SelectState 实体。
    ///
    /// `active_project_id` 用于构建父文件夹 / 跳转会话等依赖项目上下文的选项。
    /// 此方法与 `dropdown_field` 共用 `dropdown_option_list`，避免选项定义重复。
    pub fn sync_selects(&self, active_project_id: &Option<String>, cx: &mut App) {
        let keys: [(&'static str, &Entity<SelectState<SharedString>>); 24] = [
            (SELECT_KEY_PARENT_FOLDER, &self.selects.parent_folder),
            (SELECT_KEY_PROXY_TYPE, &self.selects.proxy_type),
            (SELECT_KEY_JUMP_SESSION, &self.selects.jump_session),
            (SELECT_KEY_TERMINAL_TYPE, &self.selects.terminal_type),
            (SELECT_KEY_CHARSET, &self.selects.charset),
            (SELECT_KEY_FONT_FAMILY, &self.selects.font_family),
            (SELECT_KEY_COLOR_SCHEME, &self.selects.color_scheme),
            (SELECT_KEY_SHELL_INTEGRATION, &self.selects.shell_integration),
            (SELECT_KEY_BRACKETED_PASTE, &self.selects.bracketed_paste),
            (SELECT_KEY_OSC52_CLIPBOARD, &self.selects.osc52_clipboard),
            (SELECT_KEY_TRUE_COLOR, &self.selects.true_color),
            (SELECT_KEY_COMPRESSION, &self.selects.compression),
            (SELECT_KEY_STRICT_HOST, &self.selects.strict_host),
            (SELECT_KEY_SERIAL_BAUD, &self.selects.serial_baud_rate),
            (SELECT_KEY_SERIAL_DATA_BITS, &self.selects.serial_data_bits),
            (SELECT_KEY_SERIAL_STOP_BITS, &self.selects.serial_stop_bits),
            (SELECT_KEY_SERIAL_PARITY, &self.selects.serial_parity),
            (SELECT_KEY_SERIAL_FLOW_CONTROL, &self.selects.serial_flow_control),
            (SELECT_KEY_SERIAL_DISPLAY_MODE, &self.selects.serial_display_mode),
            (SELECT_KEY_SERIAL_LINE_ENDING, &self.selects.serial_line_ending),
            (SELECT_KEY_CURSOR_SHAPE, &self.selects.cursor_shape),
            (SELECT_KEY_CURSOR_BLINK, &self.selects.cursor_blink),
            (SELECT_KEY_TELNET_ENCODING, &self.selects.telnet_encoding),
            (SELECT_KEY_LOCAL_SHELL, &self.selects.local_shell),
        ];
        for (key, state) in keys {
            let raw = dropdown_option_list(key, self, active_project_id, cx);
            let mut options: Vec<SelectOption<SharedString>> = Vec::with_capacity(raw.len());
            let mut selected: Option<SharedString> = None;
            for (label, value, is_selected) in raw {
                let value = SharedString::from(value);
                options.push(SelectOption::new(value.clone(), label));
                if is_selected {
                    selected = Some(value);
                }
            }
            state.update(cx, |s, cx| {
                s.set_options(options, cx);
                s.set_selected_value(selected, cx);
            });
        }
    }

    /// 为全部 Select 组件注入 OverlayRegistry 句柄。
    pub fn set_overlay_registry(&self, reg: Entity<OverlayRegistry>, cx: &mut App) {
        self.selects.set_overlay_registry(reg, cx);
    }

    /// 订阅全部 Select 组件的变更事件，写回会话配置。
    ///
    /// 必须在持有 `SessionPanel` 实体的上下文中调用（订阅回调需要 `panel` 才能写回 config）。
    /// 订阅在 panel / select 实体存活期间保持有效。
    pub fn subscribe_selects(&self, panel: Entity<SessionPanel>, cx: &mut Context<SessionPanel>) {
        let keys: [(&'static str, &Entity<SelectState<SharedString>>); 24] = [
            (SELECT_KEY_PARENT_FOLDER, &self.selects.parent_folder),
            (SELECT_KEY_PROXY_TYPE, &self.selects.proxy_type),
            (SELECT_KEY_JUMP_SESSION, &self.selects.jump_session),
            (SELECT_KEY_TERMINAL_TYPE, &self.selects.terminal_type),
            (SELECT_KEY_CHARSET, &self.selects.charset),
            (SELECT_KEY_FONT_FAMILY, &self.selects.font_family),
            (SELECT_KEY_COLOR_SCHEME, &self.selects.color_scheme),
            (SELECT_KEY_SHELL_INTEGRATION, &self.selects.shell_integration),
            (SELECT_KEY_BRACKETED_PASTE, &self.selects.bracketed_paste),
            (SELECT_KEY_OSC52_CLIPBOARD, &self.selects.osc52_clipboard),
            (SELECT_KEY_TRUE_COLOR, &self.selects.true_color),
            (SELECT_KEY_COMPRESSION, &self.selects.compression),
            (SELECT_KEY_STRICT_HOST, &self.selects.strict_host),
            (SELECT_KEY_SERIAL_BAUD, &self.selects.serial_baud_rate),
            (SELECT_KEY_SERIAL_DATA_BITS, &self.selects.serial_data_bits),
            (SELECT_KEY_SERIAL_STOP_BITS, &self.selects.serial_stop_bits),
            (SELECT_KEY_SERIAL_PARITY, &self.selects.serial_parity),
            (SELECT_KEY_SERIAL_FLOW_CONTROL, &self.selects.serial_flow_control),
            (SELECT_KEY_SERIAL_DISPLAY_MODE, &self.selects.serial_display_mode),
            (SELECT_KEY_SERIAL_LINE_ENDING, &self.selects.serial_line_ending),
            (SELECT_KEY_CURSOR_SHAPE, &self.selects.cursor_shape),
            (SELECT_KEY_CURSOR_BLINK, &self.selects.cursor_blink),
            (SELECT_KEY_TELNET_ENCODING, &self.selects.telnet_encoding),
            (SELECT_KEY_LOCAL_SHELL, &self.selects.local_shell),
        ];
        for (key, state) in keys {
            let p = panel.clone();
            let k = key.to_string();
            cx.subscribe(state, move |_panel, _st, ev: &SelectEvent<SharedString>, cxx| {
                let SelectEvent::Change(v) = ev;
                // 事件在 SelectState 实体更新期间同步派发，此刻 SessionPanel 可能正被外层
                // update 锁住。把对 SessionPanel 的更新推迟到当前事务结束后再执行，避免重入 panic。
                let value = v.clone().map(|s| s.to_string()).unwrap_or_default();
                let k = k.clone();
                let p = p.clone();
                cxx.defer(move |cx: &mut App| {
                    p.update(cx, |this, cxx| {
                        this.dialog_select_dropdown(&k, value, cxx);
                        this.refresh_session_dialog_selects(cxx);
                    });
                });
            })
            .detach();
        }
    }

    /// 取得所有可键盘聚焦元素的 FocusHandle，按「层次化焦点树」顺序供 Tab / Shift+Tab 导航：
    ///
    /// 1. 顶部搜索框
    /// 2. 左侧导航列 8 个分组按钮（进入后可按 ↑/↓ 方向键切换）
    /// 3. 右侧各卡片：每个分组先「卡片头部」再「卡片内容」
    /// 4. 顶部与底部操作按钮（测试 / 取消 / 保存）
    ///
    /// 顺序与 [`render_session_dialog`] 的视觉布局保持一致，避免 Tab 跳序。
    pub fn focus_handles(&self, cx: &App) -> Vec<FocusHandle> {
        let mut handles = Vec::new();

        // ① 顶部搜索框
        handles.push(self.inputs.nav_search.focus_handle(cx));

        // ② 左侧导航当前活动分组选项卡（进入后通过 ↑/↓ 方向键在分组间切换并滚动）
        if let Some(fh) = self.focus.nav.get(&self.ui.active_section) {
            handles.push(fh.clone());
        }

        // ③ 右侧各卡片内容（严格按可见分组渲染顺序）
        let visible = visible_sections(self.config.protocol);
        for sec in visible {
            self.section_focus_handles(*sec, cx, &mut handles);
        }

        // ④ 底部操作按钮（非 SSH 协议不渲染测试按钮）
        if self.config.protocol == velowork_state::SessionProtocol::Ssh {
            handles.push(self.focus.test.clone());
        }
        handles.push(self.focus.cancel.clone());
        handles.push(self.focus.save.clone());

        handles
    }

    /// 在对话框所有可聚焦元素之间按 ARIA 模式流转焦点，并在切到可视范围之外的分组时自动展开并滚动定位。
    pub fn cycle_focus(&mut self, is_shift: bool, window: &mut Window, cx: &mut App) -> bool {
        // 1. 若当前焦点在左侧任意导航项上
        let is_any_nav_focused = self.focus.nav.values().any(|h| h.is_focused(window));
        if is_any_nav_focused {
            if is_shift {
                // Shift+Tab：从左侧导航退回顶部搜索框
                let search_fh = self.inputs.nav_search.focus_handle(cx);
                window.focus(&search_fh, cx);
                return true;
            } else {
                // 正向 Tab：直接切入当前活动分类内部的首个可聚焦表单控件
                let mut cat_handles = Vec::new();
                self.section_focus_handles(self.ui.active_section, cx, &mut cat_handles);
                if let Some(first) = cat_handles.first() {
                    window.focus(first, cx);
                    self.scroll_handle_into_view(first, cx);
                    return true;
                }
            }
        }

        // 2. 若当前焦点在搜索框且为正向 Tab：直达当前活动分类导航项
        if self.inputs.nav_search.focus_handle(cx).is_focused(window)
            && !is_shift
            && let Some(active_nav) = self.focus.nav.get(&self.ui.active_section)
        {
            window.focus(active_nav, cx);
            return true;
        }

        // 3. 若当前处于当前活动分类的首个表单控件且为 Shift+Tab：回弹至左侧活动导航项
        if is_shift {
            let mut cat_handles = Vec::new();
            self.section_focus_handles(self.ui.active_section, cx, &mut cat_handles);
            if let Some(first) = cat_handles.first()
                && first.is_focused(window)
                && let Some(active_nav) = self.focus.nav.get(&self.ui.active_section)
            {
                window.focus(active_nav, cx);
                return true;
            }
        }

        let handles = self.focus_handles(cx);
        if handles.is_empty() {
            return false;
        }

        let current_idx = handles.iter().position(|h| h.is_focused(window));
        let next_idx = match current_idx {
            Some(idx) => {
                if is_shift {
                    (idx + handles.len() - 1) % handles.len()
                } else {
                    (idx + 1) % handles.len()
                }
            }
            None => {
                if is_shift {
                    handles.len() - 1
                } else {
                    0
                }
            }
        };

        let next_handle = &handles[next_idx];
        window.focus(next_handle, cx);

        self.scroll_handle_into_view(next_handle, cx);

        true
    }

    /// 基础信息分组焦点句柄（按协议拆分子表单对应）
    fn basic_focus_handles(&self, cx: &App, out: &mut Vec<FocusHandle>) {
        out.push(self.inputs.name.focus_handle(cx));
        if self.ui.parent_folder_input.is_none() {
            out.push(self.selects.parent_folder.read(cx).focus_handle().clone());
            out.push(self.focus.new_parent_folder.clone());
        }

        match self.config.protocol {
            velowork_state::SessionProtocol::Ssh => {}
            velowork_state::SessionProtocol::Serial => {
                out.push(self.inputs.serial_port.focus_handle(cx));
                out.push(self.selects.serial_baud_rate.read(cx).focus_handle().clone());
                out.push(self.selects.serial_data_bits.read(cx).focus_handle().clone());
                out.push(self.selects.serial_stop_bits.read(cx).focus_handle().clone());
                out.push(self.selects.serial_parity.read(cx).focus_handle().clone());
                out.push(self.selects.serial_flow_control.read(cx).focus_handle().clone());
                out.push(self.focus.serial_dtr.clone());
                out.push(self.focus.serial_rts.clone());
                out.push(self.selects.serial_display_mode.read(cx).focus_handle().clone());
                out.push(self.selects.serial_line_ending.read(cx).focus_handle().clone());
                out.push(self.focus.serial_local_echo.clone());
                out.push(self.focus.serial_timestamps.clone());
                out.push(self.focus.serial_auto_reconnect.clone());
            }
            velowork_state::SessionProtocol::Telnet => {
                out.push(self.inputs.telnet_host.focus_handle(cx));
                out.push(self.inputs.telnet_port.focus_handle(cx));
            }
            velowork_state::SessionProtocol::Local => {
                out.push(self.selects.local_shell.read(cx).focus_handle().clone());
                out.push(self.inputs.local_cwd.focus_handle(cx));
            }
        }
        out.push(self.inputs.startup_command.focus_handle(cx));
    }

    /// 终端设置分组焦点句柄（严格对齐真实视觉从上到下顺序）
    fn terminal_focus_handles(&self, cx: &App, out: &mut Vec<FocusHandle>) {
        out.push(self.selects.font_family.read(cx).focus_handle().clone());
        out.push(self.inputs.font_size.focus_handle(cx));
        out.push(self.selects.color_scheme.read(cx).focus_handle().clone());
        out.push(self.selects.charset.read(cx).focus_handle().clone());

        let is_serial = self.config.protocol == velowork_state::SessionProtocol::Serial;
        if !is_serial {
            out.push(self.selects.terminal_type.read(cx).focus_handle().clone());
        }
        out.push(self.inputs.scrollback.focus_handle(cx));

        out.push(self.focus.cursor_shape.clone());
        out.push(self.focus.cursor_blink.clone());
        out.push(self.focus.bell_style.clone());
        out.push(self.inputs.bell_cooldown_ms.focus_handle(cx));
        out.push(self.inputs.word_separators.focus_handle(cx));

        if !is_serial {
            out.push(self.focus.term_shell_integration.clone());
            out.push(self.focus.term_bracketed_paste.clone());
            out.push(self.focus.term_osc52.clone());
            out.push(self.focus.term_true_color.clone());
        }
    }

    /// 连接设置分组焦点句柄（SSH 专属）
    fn connection_focus_handles(&self, cx: &App, out: &mut Vec<FocusHandle>) {
        out.push(self.inputs.host.focus_handle(cx));
        out.push(self.inputs.port.focus_handle(cx));
        out.push(self.inputs.connection_timeout.focus_handle(cx));
        out.push(self.focus.sftp.clone());
        out.push(self.focus.monitor.clone());
        if self.config.enable_monitor {
            out.push(self.focus.monitor_cpu.clone());
            out.push(self.focus.monitor_mem.clone());
            out.push(self.focus.monitor_disk.clone());
        }
        if self.config.proxy_type == ProxyType::None {
            out.push(self.focus.proxy.clone());
        } else {
            out.push(self.selects.proxy_type.read(cx).focus_handle().clone());
            if self.config.proxy_type == ProxyType::Jump {
                out.push(self.selects.jump_session.read(cx).focus_handle().clone());
            } else {
                out.push(self.inputs.proxy_host.focus_handle(cx));
                out.push(self.inputs.proxy_port.focus_handle(cx));
                out.push(self.inputs.proxy_username.focus_handle(cx));
                out.push(self.inputs.proxy_password.focus_handle(cx));
            }
        }
    }

    /// 认证设置分组焦点句柄（SSH 专属）
    fn authentication_focus_handles(&self, cx: &App, out: &mut Vec<FocusHandle>) {
        out.push(self.inputs.username.focus_handle(cx));
        out.push(self.focus.auth_method.clone());
        match &self.config.auth_type {
            SshAuthType::Password { .. } => {
                out.push(self.inputs.password.focus_handle(cx));
            }
            SshAuthType::PrivateKey { .. } => {
                out.push(self.inputs.key_path.focus_handle(cx));
                out.push(self.inputs.passphrase.focus_handle(cx));
            }
            SshAuthType::SshAgent { .. } => {
                out.push(self.inputs.agent_socket_path.focus_handle(cx));
            }
            _ => {}
        }
        out.push(self.inputs.totp_secret.focus_handle(cx));
    }

    /// 网络设置分组焦点句柄（SSH 专属）
    fn network_focus_handles(&self, cx: &App, out: &mut Vec<FocusHandle>) {
        out.push(self.inputs.keepalive_interval.focus_handle(cx));
        out.push(self.inputs.keepalive_max.focus_handle(cx));
        out.push(self.inputs.idle_disconnect.focus_handle(cx));
        out.push(self.focus.net_tcp_nodelay.clone());
        out.push(self.focus.net_x11_enable.clone());
        if self.config.enable_x11_forwarding {
            out.push(self.inputs.x11_display.focus_handle(cx));
        }
        out.push(self.focus.net_agent_fwd_enable.clone());
        out.push(self.selects.compression.read(cx).focus_handle().clone());
        out.push(self.inputs.max_packets.focus_handle(cx));
        out.push(self.inputs.recv_window.focus_handle(cx));
    }

    /// 安全设置分组焦点句柄（SSH 专属）
    fn security_focus_handles(&self, cx: &App, out: &mut Vec<FocusHandle>) {
        out.push(self.focus.adv_algo_auto.clone());
        if self.config.algorithms_automatic {
            out.push(self.focus.algo_advanced.clone());
        }
        out.push(self.selects.strict_host.read(cx).focus_handle().clone());
    }

    /// 将单个分组卡片内「当前渲染」的可聚焦元素追加到 `out`。
    pub(super) fn section_focus_handles(
        &self,
        section: SshSection,
        cx: &App,
        out: &mut Vec<FocusHandle>,
    ) {
        match section {
            SshSection::Basic => self.basic_focus_handles(cx, out),
            SshSection::Connection => self.connection_focus_handles(cx, out),
            SshSection::Authentication => self.authentication_focus_handles(cx, out),
            SshSection::Terminal => self.terminal_focus_handles(cx, out),
            SshSection::Network => self.network_focus_handles(cx, out),
            SshSection::Security => self.security_focus_handles(cx, out),
            SshSection::Advanced => {
                out.push(self.inputs.rekey_time.focus_handle(cx));
                out.push(self.inputs.gex_min.focus_handle(cx));
                out.push(self.inputs.gex_preferred.focus_handle(cx));
                out.push(self.inputs.gex_max.focus_handle(cx));
            }
            SshSection::Notes => {
                out.push(self.inputs.tags.focus_handle(cx));
                out.push(self.inputs.notes.focus_handle(cx));
            }
        }
    }

    /// 寻找包含指定 FocusHandle 的 SshSection 分组（如有）
    pub fn section_for_focus_handle(&self, target_handle: &FocusHandle, cx: &App) -> Option<SshSection> {
        let visible = visible_sections(self.config.protocol);
        for section in visible {
            if let Some(fh) = self.focus.card_header.get(section) {
                if fh == target_handle {
                    return Some(*section);
                }
            }
            if let Some(fh) = self.focus.nav.get(section) {
                if fh == target_handle {
                    return Some(*section);
                }
            }
            let mut handles = Vec::new();
            self.section_focus_handles(*section, cx, &mut handles);
            if handles.iter().any(|h| h == target_handle) {
                return Some(*section);
            }
        }
        None
    }

    /// 获取当前获得键盘焦点且在可视视口内的 SshSection 分组（如有）：
    /// 用于「焦点驱动高亮」，当 Tab 或交互使某卡片内控件获焦时，
    /// 左侧导航立即高亮该卡片，避免因无需滚动而被 Scroll Spy 反向覆写。
    pub fn focused_section(&self, window: &Window, cx: &App) -> Option<SshSection> {
        let visible = visible_sections(self.config.protocol);
        for &sec in visible {
            // 1. 导航项获焦：恒定属于该 section
            if let Some(fh) = self.focus.nav.get(&sec)
                && fh.is_focused(window)
            {
                return Some(sec);
            }
            // 2. 卡片内部表单控件获焦
            let mut handles = Vec::new();
            self.section_focus_handles(sec, cx, &mut handles);
            for h in &handles {
                if h.is_focused(window) {
                    let (top, bottom) = self.handle_y_range_in_section(sec, h, cx);
                    let sec_top = self.section_top_offset(sec);
                    let item_top = sec_top + top;
                    let item_bottom = sec_top + bottom;
                    let cur_scroll_y = -f32::from(self.ui.scroll_handle.offset().y);
                    let vp_h = f32::from(self.ui.scroll_handle.bounds().size.height);
                    if vp_h > 0.0 {
                        // 若控件已被完全滚出视口顶部或底部，让位给 Scroll Spy
                        if item_bottom < cur_scroll_y || item_top > cur_scroll_y + vp_h {
                            return None;
                        }
                    }
                    return Some(sec);
                }
            }
        }
        None
    }

    /// 计算指定分组卡片在右侧滚动容器内容空间的顶部 Y 偏移量
    pub fn section_top_offset(&self, target: SshSection) -> f32 {
        let visible_sec_list = visible_sections(self.config.protocol);
        let gap = f32::from(SPACE_CARD_GAP);
        let heights = self.ui.card_heights.borrow();
        let mut y_acc = 0.0;
        for &sec in visible_sec_list {
            if sec == target {
                break;
            }
            let is_expanded = self.is_expanded(sec);
            let card_h = heights.get(&sec).copied().unwrap_or_else(|| {
                if is_expanded {
                    default_expanded_height(sec, self.config.protocol)
                } else {
                    52.0
                }
            });
            y_acc += card_h + gap;
        }
        y_acc
    }

    /// 计算指定分组中某一焦点控件在该卡片内部的相对垂直范围 [top, bottom]
    pub fn handle_y_range_in_section(
        &self,
        sec: SshSection,
        handle: &FocusHandle,
        cx: &App,
    ) -> (f32, f32) {
        let row_idx = self.handle_row_index_in_section(sec, handle, cx);
        let row_top = 40.0 + row_idx as f32 * 62.0;
        let row_bottom = row_top + 52.0;
        (row_top, row_bottom)
    }

    /// 根据焦点控件在分组卡片中的视觉排列，解析其行索引
    fn handle_row_index_in_section(&self, sec: SshSection, handle: &FocusHandle, cx: &App) -> usize {
        match sec {
            SshSection::Basic => {
                if handle == &self.inputs.name.focus_handle(cx) {
                    return 0;
                }
                let folder_handle = if let Some(ref input) = self.ui.parent_folder_input {
                    input.focus_handle(cx)
                } else {
                    self.selects.parent_folder.read(cx).focus_handle().clone()
                };
                if handle == &folder_handle {
                    return 1;
                }
                match self.config.protocol {
                    velowork_state::SessionProtocol::Ssh => 2,
                    velowork_state::SessionProtocol::Serial => {
                        if handle == &self.inputs.serial_port.focus_handle(cx) {
                            2
                        } else if handle == self.selects.serial_baud_rate.read(cx).focus_handle() {
                            3
                        } else if handle == self.selects.serial_data_bits.read(cx).focus_handle() {
                            4
                        } else if handle == self.selects.serial_stop_bits.read(cx).focus_handle() {
                            5
                        } else if handle == self.selects.serial_parity.read(cx).focus_handle() {
                            6
                        } else if handle == self.selects.serial_flow_control.read(cx).focus_handle() {
                            7
                        } else if handle == &self.focus.serial_dtr || handle == &self.focus.serial_rts {
                            8
                        } else if handle == self.selects.serial_display_mode.read(cx).focus_handle() {
                            9
                        } else if handle == self.selects.serial_line_ending.read(cx).focus_handle() {
                            10
                        } else if handle == &self.focus.serial_local_echo
                            || handle == &self.focus.serial_timestamps
                            || handle == &self.focus.serial_auto_reconnect
                        {
                            11
                        } else {
                            12
                        }
                    }
                    velowork_state::SessionProtocol::Telnet => {
                        if handle == &self.inputs.telnet_host.focus_handle(cx)
                            || handle == &self.inputs.telnet_port.focus_handle(cx)
                        {
                            2
                        } else {
                            3
                        }
                    }
                    velowork_state::SessionProtocol::Local => {
                        if handle == self.selects.local_shell.read(cx).focus_handle() {
                            2
                        } else if handle == &self.inputs.local_cwd.focus_handle(cx) {
                            3
                        } else {
                            4
                        }
                    }
                }
            }
            SshSection::Connection => {
                if handle == &self.inputs.host.focus_handle(cx)
                    || handle == &self.inputs.port.focus_handle(cx)
                {
                    0
                } else if handle == &self.inputs.connection_timeout.focus_handle(cx) {
                    1
                } else if handle == &self.focus.sftp {
                    2
                } else if handle == &self.focus.monitor {
                    3
                } else if handle == &self.focus.monitor_cpu
                    || handle == &self.focus.monitor_mem
                    || handle == &self.focus.monitor_disk
                {
                    4
                } else {
                    5
                }
            }
            SshSection::Authentication => {
                if handle == &self.inputs.username.focus_handle(cx) {
                    0
                } else if handle == &self.focus.auth_method {
                    1
                } else if handle == &self.inputs.password.focus_handle(cx)
                    || handle == &self.inputs.key_path.focus_handle(cx)
                    || handle == &self.inputs.passphrase.focus_handle(cx)
                    || handle == &self.inputs.agent_socket_path.focus_handle(cx)
                {
                    2
                } else {
                    3
                }
            }
            SshSection::Terminal => {
                if handle == self.selects.font_family.read(cx).focus_handle()
                    || handle == &self.inputs.font_size.focus_handle(cx)
                {
                    0
                } else if handle == self.selects.color_scheme.read(cx).focus_handle()
                    || handle == self.selects.charset.read(cx).focus_handle()
                {
                    1
                } else if handle == self.selects.terminal_type.read(cx).focus_handle()
                    || handle == &self.inputs.scrollback.focus_handle(cx)
                {
                    2
                } else if handle == &self.focus.cursor_shape {
                    3
                } else if handle == &self.focus.cursor_blink {
                    4
                } else if handle == &self.focus.bell_style {
                    5
                } else if handle == &self.inputs.bell_cooldown_ms.focus_handle(cx) {
                    6
                } else if handle == &self.inputs.word_separators.focus_handle(cx) {
                    7
                } else if handle == &self.focus.term_shell_integration {
                    8
                } else if handle == &self.focus.term_bracketed_paste {
                    9
                } else if handle == &self.focus.term_osc52 {
                    10
                } else if handle == &self.focus.term_true_color {
                    11
                } else {
                    0
                }
            }
            SshSection::Network => {
                if handle == &self.inputs.keepalive_interval.focus_handle(cx)
                    || handle == &self.inputs.keepalive_max.focus_handle(cx)
                    || handle == &self.inputs.idle_disconnect.focus_handle(cx)
                {
                    0
                } else if handle == &self.focus.net_tcp_nodelay {
                    1
                } else if handle == &self.focus.net_x11_enable
                    || handle == &self.inputs.x11_display.focus_handle(cx)
                {
                    2
                } else if handle == &self.focus.net_agent_fwd_enable {
                    3
                } else if handle == self.selects.compression.read(cx).focus_handle() {
                    4
                } else {
                    5
                }
            }
            SshSection::Security => {
                if handle == &self.focus.adv_algo_auto {
                    0
                } else if handle == &self.focus.algo_advanced {
                    1
                } else {
                    2
                }
            }
            SshSection::Advanced => {
                if handle == &self.inputs.rekey_time.focus_handle(cx) {
                    0
                } else {
                    1
                }
            }
            SshSection::Notes => {
                if handle == &self.inputs.tags.focus_handle(cx) {
                    0
                } else {
                    1
                }
            }
        }
    }

    /// 视口自适应平滑滚动（Scroll Into View with 40px Padding）：
    /// 当 Tab 或交互导致某控件获焦时，若其处于视口之外或距离边缘过近，自动滚动至视口并保留 40px 呼吸边距。
    pub fn scroll_handle_into_view(&mut self, handle: &FocusHandle, cx: &App) {
        let Some(sec) = self.section_for_focus_handle(handle, cx) else {
            return;
        };

        self.ui.expanded_sections.insert(sec);
        self.ui.active_section = sec;

        // 若该焦点句柄即为左侧导航项本身，直接触发整卡精准贴顶滚动
        if let Some(nav_fh) = self.focus.nav.get(&sec)
            && nav_fh == handle
        {
            self.ui.pending_scroll.set(Some(sec));
            return;
        }

        let (in_card_top, in_card_bottom) = self.handle_y_range_in_section(sec, handle, cx);
        let sec_top = self.section_top_offset(sec);
        let item_top = sec_top + in_card_top;
        let item_bottom = sec_top + in_card_bottom;

        let viewport_h = f32::from(self.ui.scroll_handle.bounds().size.height);
        if viewport_h <= 0.0 {
            // 视口尚未完成首次布局时，暂存待滚动句柄，在 render 中生效
            self.ui.pending_scroll_focus_handle.set(Some(handle.clone()));
            return;
        }

        let cur_scroll_y = -f32::from(self.ui.scroll_handle.offset().y);
        let max_offset_y = f32::from(self.ui.scroll_handle.max_offset().y);

        let pad = 40.0;
        let vis_top = item_top - cur_scroll_y;
        let vis_bottom = item_bottom - cur_scroll_y;

        let target_scroll_y = if vis_bottom > viewport_h - pad {
            // 控件底部超出或靠近下边缘：向下滚动使其底部保留 40px 内边距
            let needed = item_bottom - (viewport_h - pad);
            // 优先保证顶部可见（避免超高控件顶部被顶出视口）
            needed.max(item_top - pad)
        } else if vis_top < pad {
            // 控件顶部超出或靠近上边缘：向上滚动使其顶部保留 40px 内边距
            item_top - pad
        } else {
            // 已在舒适视口内，无需滚动
            return;
        };

        let clamped_scroll_y = target_scroll_y.clamp(0.0, max_offset_y.max(0.0));
        self.ui.scroll_handle.set_offset(point(px(0.0), px(-clamped_scroll_y)));
    }
}

/// 针对不同会话协议与卡片分组的预估展开高度（用于 canvas 测量前的贴顶与焦点滚动估算）
pub fn default_expanded_height(section: SshSection, protocol: velowork_state::SessionProtocol) -> f32 {
    match section {
        SshSection::Basic => {
            if protocol == velowork_state::SessionProtocol::Serial {
                800.0
            } else if protocol == velowork_state::SessionProtocol::Local {
                320.0
            } else {
                280.0
            }
        }
        SshSection::Connection => 380.0,
        SshSection::Authentication => 360.0,
        SshSection::Terminal => {
            if protocol == velowork_state::SessionProtocol::Serial {
                580.0
            } else {
                780.0
            }
        }
        SshSection::Network => 380.0,
        SshSection::Security => 400.0,
        SshSection::Advanced => 350.0,
        SshSection::Notes => 220.0,
    }
}

/// 新建会话的默认配置（沿用原 reset_session_fields 的默认算法列表）。
fn default_config() -> SshSession {
    SshSession {
        port: 22,
        auth_type: SshAuthType::Password { password: None },
        icon_color: IconColor::default(),
        save_credentials: false,
        totp_secret: None,
        connection_timeout: 30,
        keep_alive_interval: 30,
        keep_alive_max: 3,
        idle_disconnect_timeout: 0,
        keepalive_strategy: KeepAliveStrategy::Always,
        tcp_nodelay: true,
        channel_buffer_size: 100,
        proxy_type: ProxyType::None,
        enable_sftp: true,
        enable_x11_forwarding: false,
        x11_display: None,
        monitor_cpu: true,
        monitor_mem: true,
        compression: velowork_state::CompressionType::default(),
        strict_host_key: StrictHostKey::default(),
        max_packets: 32768,
        recv_window: 2097152,
        gex_min: 2048,
        gex_preferred: 4096,
        gex_max: 8192,
        rekey_time: 3600,
        algorithms_automatic: true,
        terminal: velowork_state::SessionTerminalOptions {
            shell_integration: Some(false),
            osc52_clipboard: Some(false),
            bracketed_paste: Some(true),
            true_color: Some(true),
            term_type: Some("xterm-256color".to_string()),
            charset: Some("UTF-8".to_string()),
            ..Default::default()
        },
        kex_algorithms: default_algorithms_for_cat("kex"),
        cipher_algorithms: default_algorithms_for_cat("cipher"),
        mac_algorithms: default_algorithms_for_cat("mac"),
        hostkey_algorithms: default_algorithms_for_cat("hostkey"),
        ..Default::default()
    }
}

/// 各算法类别的候选全集（标准 SSH 算法名）。
pub fn algo_candidates(cat: &str) -> &'static [&'static str] {
    match cat {
        "kex" => &[
            "curve25519-sha256",
            "curve25519-sha256@libssh.org",
            "mlkem768x25519-sha256",
            "ecdh-sha2-nistp256",
            "ecdh-sha2-nistp384",
            "ecdh-sha2-nistp521",
            "diffie-hellman-group-exchange-sha256",
            "diffie-hellman-group-exchange-sha1",
            "diffie-hellman-group14-sha256",
            "diffie-hellman-group14-sha1",
            "diffie-hellman-group15-sha512",
            "diffie-hellman-group16-sha512",
            "diffie-hellman-group17-sha512",
            "diffie-hellman-group18-sha512",
            "diffie-hellman-group1-sha1",
        ],
        "cipher" => &[
            "chacha20-poly1305@openssh.com",
            "aes256-gcm@openssh.com",
            "aes128-gcm@openssh.com",
            "aes256-ctr",
            "aes192-ctr",
            "aes128-ctr",
            "aes256-cbc",
            "aes192-cbc",
            "aes128-cbc",
            "3des-cbc",
        ],
        "mac" => &[
            "hmac-sha2-256-etm@openssh.com",
            "hmac-sha2-512-etm@openssh.com",
            "hmac-sha2-256",
            "hmac-sha2-512",
            "hmac-sha1-etm@openssh.com",
            "hmac-sha1",
        ],
        "hostkey" => &[
            "ssh-ed25519",
            "rsa-sha2-512",
            "rsa-sha2-256",
            "ssh-rsa",
            "ecdsa-sha2-nistp256",
            "ecdsa-sha2-nistp384",
            "ecdsa-sha2-nistp521",
        ],
        _ => &[],
    }
}

/// 各算法类别的默认排序与勾选列表。
pub fn default_algorithms_for_cat(cat: &str) -> Vec<String> {
    match cat {
        "kex" => vec![
            "curve25519-sha256".into(),
            "ecdh-sha2-nistp256".into(),
            "ecdh-sha2-nistp384".into(),
            "ecdh-sha2-nistp521".into(),
            "diffie-hellman-group14-sha256".into(),
            "diffie-hellman-group16-sha512".into(),
            "diffie-hellman-group18-sha512".into(),
        ],
        "cipher" => vec![
            "chacha20-poly1305@openssh.com".into(),
            "aes256-gcm@openssh.com".into(),
            "aes128-gcm@openssh.com".into(),
            "aes256-ctr".into(),
            "aes192-ctr".into(),
            "aes128-ctr".into(),
        ],
        "mac" => vec![
            "hmac-sha2-256-etm@openssh.com".into(),
            "hmac-sha2-512-etm@openssh.com".into(),
            "hmac-sha2-256".into(),
            "hmac-sha2-512".into(),
        ],
        "hostkey" => vec![
            "ssh-ed25519".into(),
            "rsa-sha2-512".into(),
            "rsa-sha2-256".into(),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use velowork_i18n::{init_locale, t, Locale};

    #[gpui::test]
    fn test_algorithm_category_properties(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init_locale(Locale::Zh, cx);
            for cat in AlgorithmCategory::ALL {
                assert!(!cat.key().is_empty());
                assert!(!cat.title_key().is_empty());
                assert!(!cat.desc_key().is_empty());

                let zh_title = t(cx, cat.title_key());
                let zh_desc = t(cx, cat.desc_key());
                assert_ne!(zh_title, cat.title_key(), "title_key {} missing in zh.json", cat.title_key());
                assert_ne!(zh_desc, cat.desc_key(), "desc_key {} missing in zh.json", cat.desc_key());
            }

            init_locale(Locale::En, cx);
            for cat in AlgorithmCategory::ALL {
                let en_title = t(cx, cat.title_key());
                let en_desc = t(cx, cat.desc_key());
                assert_ne!(en_title, cat.title_key(), "title_key {} missing in en.json", cat.title_key());
                assert_ne!(en_desc, cat.desc_key(), "desc_key {} missing in en.json", cat.desc_key());

                let candidates = algo_candidates(cat.key());
                let defaults = default_algorithms_for_cat(cat.key());
                assert!(!candidates.is_empty());
                assert!(!defaults.is_empty());

                for d in &defaults {
                    assert!(candidates.contains(&d.as_str()), "default algo {d} must be in candidates");
                }
            }
        });
    }

    #[gpui::test]
    fn test_protocol_validation_isolation(cx: &mut TestAppContext) {
        let _window = cx.open_window(gpui::size(px(800.0), px(600.0)), |window, cx| {
            init_locale(Locale::En, cx);

            // 1. Default protocol is SSH. With empty inputs, validate_all should fail on Name or Host.
            let mut m_ssh = SessionDialogModel::new(window, cx);
            let err = m_ssh.validate_all(cx);
            assert!(err.is_some());
            assert!(matches!(err, Some(FieldId::Name) | Some(FieldId::Host)));

            // 2. Telnet protocol dialog.
            let mut m_telnet = SessionDialogModel::new(window, cx);
            m_telnet.config.protocol = velowork_state::SessionProtocol::Telnet;
            m_telnet.baseline.protocol = velowork_state::SessionProtocol::Telnet;
            // Non-applicable SSH Host/Port should not produce error
            assert!(m_telnet.ui.validation.get(FieldId::Host).is_none());
            assert!(m_telnet.ui.validation.get(FieldId::Port).is_none());

            // Provide valid Telnet data
            m_telnet.inputs.name.update(cx, |s, cx| s.set_value("My Telnet", cx));
            m_telnet.inputs.telnet_host.update(cx, |s, cx| s.set_value("192.168.1.1", cx));
            m_telnet.sync_config_from_inputs(cx);
            let err = m_telnet.validate_all(cx);
            assert_eq!(err, None, "Telnet with valid name and telnet_host should pass validation");
            assert!(m_telnet.is_valid());

            // 3. Serial protocol dialog.
            let mut m_serial = SessionDialogModel::new(window, cx);
            m_serial.config.protocol = velowork_state::SessionProtocol::Serial;
            m_serial.baseline.protocol = velowork_state::SessionProtocol::Serial;
            // Telnet fields should not produce error
            assert!(m_serial.ui.validation.get(FieldId::TelnetHost).is_none());
            // SerialPort is empty, so it should fail on SerialPort
            m_serial.inputs.name.update(cx, |s, cx| s.set_value("My Serial", cx));
            m_serial.sync_config_from_inputs(cx);
            let err = m_serial.validate_all(cx);
            assert_eq!(err, Some(FieldId::SerialPort));

            // Provide Serial port
            m_serial.inputs.serial_port.update(cx, |s, cx| s.set_value("COM1", cx));
            m_serial.sync_config_from_inputs(cx);
            let err = m_serial.validate_all(cx);
            assert_eq!(err, None, "Serial with valid name and serial_port should pass validation");
            assert!(m_serial.is_valid());

            // 4. Local protocol dialog.
            let mut m_local = SessionDialogModel::new(window, cx);
            m_local.config.protocol = velowork_state::SessionProtocol::Local;
            m_local.baseline.protocol = velowork_state::SessionProtocol::Local;
            m_local.inputs.name.update(cx, |s, cx| s.set_value("My Local", cx));
            m_local.sync_config_from_inputs(cx);
            let err = m_local.validate_all(cx);
            assert_eq!(err, None, "Local with valid name should pass validation");
            assert!(m_local.is_valid());

            gpui::Empty
        });
    }

    #[gpui::test]
    fn test_session_dialog_title_formatting(cx: &mut TestAppContext) {
        use crate::views::overlays::dialogs::session_dialog::session_dialog_title;
        use velowork_state::SessionProtocol;

        cx.update(|cx| {
            // Test Chinese locale
            init_locale(Locale::Zh, cx);
            assert_eq!(session_dialog_title(false, SessionProtocol::Ssh, cx), "新建 SSH 会话");
            assert_eq!(session_dialog_title(false, SessionProtocol::Serial, cx), "新建串口会话");
            assert_eq!(session_dialog_title(false, SessionProtocol::Telnet, cx), "新建 Telnet 会话");
            assert_eq!(session_dialog_title(false, SessionProtocol::Local, cx), "新建本地终端会话");

            assert_eq!(session_dialog_title(true, SessionProtocol::Ssh, cx), "编辑 SSH 会话");
            assert_eq!(session_dialog_title(true, SessionProtocol::Serial, cx), "编辑串口会话");
            assert_eq!(session_dialog_title(true, SessionProtocol::Telnet, cx), "编辑 Telnet 会话");
            assert_eq!(session_dialog_title(true, SessionProtocol::Local, cx), "编辑本地终端会话");

            // Test English locale
            init_locale(Locale::En, cx);
            assert_eq!(session_dialog_title(false, SessionProtocol::Ssh, cx), "New SSH Session");
            assert_eq!(session_dialog_title(false, SessionProtocol::Serial, cx), "New Serial Session");
            assert_eq!(session_dialog_title(false, SessionProtocol::Telnet, cx), "New Telnet Session");
            assert_eq!(session_dialog_title(false, SessionProtocol::Local, cx), "New Local Terminal Session");

            assert_eq!(session_dialog_title(true, SessionProtocol::Ssh, cx), "Edit SSH Session");
            assert_eq!(session_dialog_title(true, SessionProtocol::Serial, cx), "Edit Serial Session");
            assert_eq!(session_dialog_title(true, SessionProtocol::Telnet, cx), "Edit Telnet Session");
            assert_eq!(session_dialog_title(true, SessionProtocol::Local, cx), "Edit Local Terminal Session");
        });
    }

    #[gpui::test]
    fn test_dialog_state_sizes(cx: &mut TestAppContext) {
        let _ = cx.open_window(gpui::size(px(1920.0), px(1080.0)), |window, cx| {
            init_locale(Locale::En, cx);
            let win_size = gpui::size(px(1920.0), px(1080.0));

            let mut m_telnet = SessionDialogModel::new(window, cx);
            m_telnet.config.protocol = velowork_state::SessionProtocol::Telnet;
            let telnet_state = crate::views::overlays::dialogs::session_dialog::SessionDialogState::Session {
                model: Box::new(m_telnet),
            };
            let size_telnet = telnet_state.preferred_size(win_size);
            assert_eq!(size_telnet.width, px(820.0));
            assert_eq!(size_telnet.height, px(560.0));

            let mut m_ssh = SessionDialogModel::new(window, cx);
            m_ssh.config.protocol = velowork_state::SessionProtocol::Ssh;
            let ssh_state = crate::views::overlays::dialogs::session_dialog::SessionDialogState::Session {
                model: Box::new(m_ssh),
            };
            let size_ssh = ssh_state.preferred_size(win_size);
            assert_eq!(size_ssh.width, px(900.0));
            assert_eq!(size_ssh.height, px(680.0));

            gpui::Empty
        });
    }

    #[gpui::test]
    fn test_serial_and_terminal_advanced_fields(cx: &mut TestAppContext) {
        let _ = cx.open_window(gpui::size(px(800.0), px(600.0)), |window, cx| {
            init_locale(Locale::En, cx);
            let mut model = SessionDialogModel::new(window, cx);

            // Verify defaults
            assert_eq!(model.config.serial_display_mode, "text");
            assert_eq!(model.config.serial_line_ending, "crlf");
            assert!(!model.config.serial_local_echo);
            assert!(!model.config.serial_timestamps);
            assert!(!model.config.serial_auto_reconnect);
            assert_eq!(model.config.terminal.cursor_shape, None);
            assert_eq!(model.config.terminal.cursor_blink, None);
            assert_eq!(model.config.terminal.word_separators, None);

            // Test applying and syncing word separators
            model.inputs.word_separators.update(cx, |s, cx| s.set_value("/;:", cx));
            model.sync_config_from_inputs(cx);
            assert_eq!(model.config.terminal.word_separators.as_deref(), Some("/;:"));

            // Test loading session with customized serial & terminal appearance
            let mut session = model.config.clone();
            session.serial_display_mode = "hex".to_string();
            session.serial_line_ending = "lf".to_string();
            session.serial_local_echo = true;
            session.serial_timestamps = true;
            session.serial_auto_reconnect = true;
            session.terminal.cursor_shape = Some(velowork_core::types::CursorShape::Bar);
            session.terminal.cursor_blink = Some(true);
            session.terminal.word_separators = Some("[]{}".to_string());

            model.load(session, window, cx);
            assert_eq!(model.config.serial_display_mode, "hex");
            assert_eq!(model.config.serial_line_ending, "lf");
            assert!(model.config.serial_local_echo);
            assert!(model.config.serial_timestamps);
            assert!(model.config.serial_auto_reconnect);
            assert_eq!(model.config.terminal.cursor_shape, Some(velowork_core::types::CursorShape::Bar));
            assert_eq!(model.config.terminal.cursor_blink, Some(true));
            assert_eq!(model.inputs.word_separators.read(cx).text(), "[]{}");

            gpui::Empty
        });
    }

    #[gpui::test]
    fn test_key_path_picker_sync(cx: &mut TestAppContext) {
        let _ = cx.open_window(gpui::size(px(800.0), px(600.0)), |window, cx| {
            init_locale(Locale::En, cx);
            let mut model = SessionDialogModel::new(window, cx);

            model.config.auth_type = SshAuthType::PrivateKey {
                key_path: String::new(),
                passphrase: None,
            };
            model.inputs.key_path.update(cx, |s, cx| s.set_value("/home/user/.ssh/id_ed25519", cx));
            model.sync_config_from_inputs(cx);

            if let SshAuthType::PrivateKey { key_path, .. } = &model.config.auth_type {
                assert_eq!(key_path, "/home/user/.ssh/id_ed25519");
            } else {
                panic!("auth_type should be PrivateKey");
            }

            gpui::Empty
        });
    }

    #[gpui::test]
    fn test_auth_input_placeholders(cx: &mut TestAppContext) {
        let _ = cx.open_window(gpui::size(px(800.0), px(600.0)), |window, cx| {
            init_locale(Locale::Zh, cx);
            let model = SessionDialogModel::new(window, cx);

            assert_eq!(model.inputs.key_path.read(cx).get_placeholder(), "~/.ssh/id_rsa");
            assert_eq!(model.inputs.passphrase.read(cx).get_placeholder(), "输入密码（可选）");
            assert_eq!(model.inputs.agent_socket_path.read(cx).get_placeholder(), "留空使用系统默认（$SSH_AUTH_SOCK / 命名管道 / Pageant）");

            gpui::Empty
        });
    }

    #[gpui::test]
    fn test_terminal_focus_handles_and_row_mapping(cx: &mut TestAppContext) {
        let _ = cx.open_window(gpui::size(px(800.0), px(600.0)), |window, cx| {
            init_locale(Locale::Zh, cx);
            let model = SessionDialogModel::new(window, cx);

            let mut term_handles = Vec::new();
            model.terminal_focus_handles(cx, &mut term_handles);
            assert_eq!(term_handles.len(), 15, "SSH terminal section should have 15 focus handles");

            // Verify that all 15 handles map to strictly non-decreasing row indices between 0 and 11
            let mut prev_row = 0;
            for (idx, handle) in term_handles.iter().enumerate() {
                let row = model.handle_row_index_in_section(SshSection::Terminal, handle, cx);
                assert!(
                    row >= prev_row,
                    "handle index {} with row {} should not be smaller than previous row {}",
                    idx,
                    row,
                    prev_row
                );
                assert!(row <= 11, "handle index {} with row {} should be <= 11", idx, row);
                let (top, bottom) = model.handle_y_range_in_section(SshSection::Terminal, handle, cx);
                assert!(top < bottom);
                assert_eq!(bottom - top, 52.0);
                prev_row = row;
            }

            // Verify first and last rows
            assert_eq!(model.handle_row_index_in_section(SshSection::Terminal, &term_handles[0], cx), 0);
            assert_eq!(model.handle_row_index_in_section(SshSection::Terminal, &term_handles[14], cx), 11);

            gpui::Empty
        });
    }

    #[gpui::test]
    fn test_section_top_offset_accumulation(cx: &mut TestAppContext) {
        let _ = cx.open_window(gpui::size(px(800.0), px(600.0)), |window, cx| {
            init_locale(Locale::Zh, cx);
            let model = SessionDialogModel::new(window, cx);

            let top_basic = model.section_top_offset(SshSection::Basic);
            assert_eq!(top_basic, 0.0, "Basic should start at offset 0");

            let top_conn = model.section_top_offset(SshSection::Connection);
            assert!(top_conn > top_basic, "Connection section must start below Basic");

            let top_term = model.section_top_offset(SshSection::Terminal);
            assert!(top_term > top_conn, "Terminal section must start below Connection");

            let top_notes = model.section_top_offset(SshSection::Notes);
            assert!(top_notes > top_term, "Notes section must start below Terminal");

            gpui::Empty
        });
    }

    #[gpui::test]
    fn test_scroll_handle_into_view_margin(cx: &mut TestAppContext) {
        let _ = cx.open_window(gpui::size(px(800.0), px(600.0)), |window, cx| {
            init_locale(Locale::Zh, cx);
            let mut model = SessionDialogModel::new(window, cx);

            let mut term_handles = Vec::new();
            model.terminal_focus_handles(cx, &mut term_handles);

            // 1. Initial state before layout: pending_scroll_focus_handle is buffered
            let first_handle = &term_handles[0];
            model.scroll_handle_into_view(first_handle, cx);
            assert_eq!(
                model.ui.pending_scroll_focus_handle.get().as_ref(),
                Some(first_handle),
                "Before viewport layout, pending focus handle must be queued"
            );

            // 2. Clear pending and verify section tracking
            assert_eq!(model.ui.active_section, SshSection::Terminal);
            assert!(model.ui.expanded_sections.contains(&SshSection::Terminal));

            gpui::Empty
        });
    }

    #[gpui::test]
    fn test_aria_cycle_focus(cx: &mut TestAppContext) {
        let _ = cx.open_window(gpui::size(px(800.0), px(600.0)), |window, cx| {
            init_locale(Locale::Zh, cx);
            let mut model = SessionDialogModel::new(window, cx);

            // 1. Initial state: focus search input
            let search_handle = model.inputs.nav_search.focus_handle(cx);
            window.focus(&search_handle, cx);
            assert!(search_handle.is_focused(window));

            // 2. Tab from search input -> enters active nav tab (Basic)
            assert!(model.cycle_focus(false, window, cx));
            let basic_nav = model.focus.nav.get(&SshSection::Basic).unwrap();
            assert!(basic_nav.is_focused(window), "Tab from search input must focus active nav tab");

            // 3. Tab from nav tab -> enters first input field of Basic (name)
            assert!(model.cycle_focus(false, window, cx));
            let name_handle = model.inputs.name.focus_handle(cx);
            assert!(name_handle.is_focused(window), "Tab from active nav tab must enter first card input (name)");

            // 4. Shift+Tab from first card input -> returns to active nav tab
            assert!(model.cycle_focus(true, window, cx));
            assert!(basic_nav.is_focused(window), "Shift+Tab from first card input must return to active nav tab");

            // 5. Shift+Tab from nav tab -> returns to search input
            assert!(model.cycle_focus(true, window, cx));
            assert!(search_handle.is_focused(window), "Shift+Tab from active nav tab must return to search input");

            // 6. Switch active section to Connection and verify Tab lands on Connection's host input
            model.ui.active_section = SshSection::Connection;
            let conn_nav = model.focus.nav.get(&SshSection::Connection).unwrap();
            window.focus(conn_nav, cx);
            assert!(conn_nav.is_focused(window));

            assert!(model.cycle_focus(false, window, cx));
            let host_handle = model.inputs.host.focus_handle(cx);
            assert!(host_handle.is_focused(window), "Tab from Connection nav tab must enter host input");

            // 7. Shift+Tab from host input -> returns to Connection nav tab
            assert!(model.cycle_focus(true, window, cx));
            assert!(conn_nav.is_focused(window), "Shift+Tab from host input must return to Connection nav tab");

            gpui::Empty
        });
    }
}

