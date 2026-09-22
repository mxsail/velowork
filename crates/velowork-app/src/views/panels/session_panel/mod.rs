use gpui::SharedString;
use velowork_ui::InputFocusRingExt;
use velowork_ui::icon::AppIcon;
use velowork_ui::icon::folder_tree_icon;
use velowork_ui::input::{Input, InputState};
use velowork_ui::simple_input::{InputFocusedEvent, SimpleInput, SimpleInputState};
use velowork_ui::theme::{ThemeColors, surface_bg, surface_bg_t, theme, with_alpha};
use velowork_ui::tokens::{
    ui_space_lg, ui_space_md, ui_space_sm, ui_space_tree_indent, ui_space_xs, ICON_SM, ICON_STD,
    RADIUS_SM, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL,
    SPACE_XS, ui_icon_std_ts, ui_text, ui_text_md, ui_text_ms, ui_text_scale, ui_text_sm,
    ui_text_xs,
};

use crate::keybindings::{
    About, Cancel, RenameActiveNode, ShowHelp, ShowKeybindings, ShowSettings,
};
use crate::settings::settings_entity;
use gpui::prelude::*;
use gpui::*;

use std::sync::Arc;
use velowork_ui::SemanticPalette;
use velowork_ui::tree::{expandable_file_row, expandable_folder_row, tree_row_appearance};

/// 会话树行高（纯几何，与主题无关）：由设计系统 Compact 档解析，
/// 与 `expandable_folder_row` / `expandable_file_row` 的实际行高保持一致。
fn session_row_height(cx: &App) -> Pixels {
    velowork_ui::tree_row_height(cx)
}
use velowork_i18n::i18n;
use velowork_state::{
    CompressionType, IconColor, KeepAliveStrategy, ProxyType, SessionTreeNode, SshAuthType,
    SshSession, StrictHostKey, WindowId as WorkspaceWindowId,
};
use velowork_ui::button::{button, button_primary};
use velowork_ui::h_flex;
use velowork_ui::menu::{ContextMenu, PopupMenu, PopupMenuItem};
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::scrollable::{ScrollableElement, Scrollbar};
use velowork_ui::tooltip::{Tooltip, TooltipDirection};
use velowork_workspace::focus::FocusManager;
use velowork_workspace::request_broker::RequestBroker;
use velowork_workspace::requests::OverlayRequest;
use velowork_workspace::state::Workspace;

use crate::views::overlays::dialogs::session_dialog::{
    SessionDialogState,
    model::SessionDialogModel,
    render::TerminalToggle,
    section::SshSection,
    validation::{FieldId, ValidationResult},
};

pub mod types;
pub use types::*;

pub struct SessionPanel {
    _window_id: WorkspaceWindowId,
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,
    focus_handle: FocusHandle,
    /// 会话树滚动容器句柄，用于键盘导航时把焦点行滚动进可视区。
    tree_scroll_handle: ScrollHandle,
    pub(crate) active_dialog: Option<SessionPanelDialog>,
    /// 新版「新建/编辑会话」多态对话框状态（取代旧的分页表单字段）。
    pub(crate) session_dialog: Option<SessionDialogState>,
    /// 对话框动效状态（进场淡入+源点扩散，出场加速收缩）。
    pub(crate) dialog_motion_state: velowork_ui::motion::ModalMotionState,
    pub(crate) dialog_anim_task: Option<Task<()>>,
    pub(crate) dialog_origin: Option<Point<Pixels>>,
    pub(crate) overlay_manager: Option<Entity<crate::views::overlays::OverlayManager>>,
    /// 当前对话框所属面板实体 id（用于 notify 重渲染）。
    pub(crate) dialog_panel_id: Option<EntityId>,
    pub(crate) focus_dialog_inputs: std::cell::Cell<bool>,
    pub(crate) inline_folder: Option<InlineFolderState>,
    pub(crate) inline_folder_sub: Option<Subscription>,
    pub(crate) inline_session: Option<InlineSessionRename>,
    pub(crate) inline_session_sub: Option<Subscription>,

    folder_name_input: Entity<SimpleInputState>,

    // SSH dialog tab state
    pub(super) ssh_dialog_tab: SshDialogTab,

    // SSH session fields — General
    session_name_input: Entity<SimpleInputState>,
    pub(super) session_icon_color: IconColor,
    pub(super) session_parent_folder_id: Option<String>,
    pub(super) session_startup_command_input: Entity<SimpleInputState>,

    // SSH session fields — Authentication
    session_username_input: Entity<SimpleInputState>,
    session_auth_type: SshAuthType,
    session_password_input: Entity<SimpleInputState>,
    session_key_path_input: Entity<SimpleInputState>,
    session_passphrase_input: Entity<SimpleInputState>,

    // SSH session fields — Connection
    session_host_input: Entity<SimpleInputState>,
    session_port_input: Entity<SimpleInputState>,
    pub(super) session_timeout_input: Entity<SimpleInputState>,
    pub(super) session_keepalive_input: Entity<SimpleInputState>,
    pub(super) session_keepalive_max: u32,
    pub(super) session_idle_disconnect_input: Entity<SimpleInputState>,
    pub(super) session_keepalive_strategy: KeepAliveStrategy,
    pub(super) session_tcp_nodelay: bool,
    pub(super) session_channel_buffer_size: u32,
    pub(super) session_proxy_type: ProxyType,
    pub(super) session_proxy_host_input: Entity<SimpleInputState>,
    pub(super) session_proxy_port_input: Entity<SimpleInputState>,
    pub(super) session_proxy_username_input: Entity<SimpleInputState>,
    pub(super) session_proxy_password_input: Entity<SimpleInputState>,
    pub(super) session_enable_sftp: bool,
    pub(super) session_enable_monitor: bool,
    pub(super) session_monitor_cpu: bool,
    pub(super) session_monitor_mem: bool,
    pub(super) session_monitor_disk: bool,

    // SSH session fields — Terminal
    pub(super) session_terminal_type: Option<String>,
    pub(super) session_charset: Option<String>,
    pub(super) session_scrollback_input: Entity<SimpleInputState>,

    // SSH session fields — Advanced
    pub(super) session_compression: CompressionType,
    pub(super) session_strict_host_key: StrictHostKey,
    pub(super) session_max_packets: u32,
    pub(super) session_recv_window: u32,
    pub(super) session_gex_min: u32,
    pub(super) session_gex_preferred: u32,
    pub(super) session_gex_max: u32,

    // SSH session fields — Algorithms
    pub(super) session_kex_algorithms: Vec<String>,
    pub(super) session_cipher_algorithms: Vec<String>,
    pub(super) session_mac_algorithms: Vec<String>,
    pub(super) session_hostkey_algorithms: Vec<String>,

    // SSH session fields — Notes
    pub(super) session_tags_input: Entity<SimpleInputState>,
    pub(super) session_notes_input: Entity<SimpleInputState>,

    // Resizable text area heights (startup command, notes)
    pub(super) startup_command_height: Option<Pixels>,
    pub(super) notes_height: Option<Pixels>,
    resize_dragging: Option<ResizeDragState>,

    // Test connection state
    pub(super) test_connection_status: TestConnectionStatus,

    // Validation error message
    pub(super) validation_error: Option<String>,

    // Dropdown states for advanced options
    pub(super) ssh_compression_dropdown_open: bool,
    pub(super) ssh_strict_host_key_dropdown_open: bool,
    pub(super) ssh_max_packets_dropdown_open: bool,
    pub(super) ssh_recv_window_dropdown_open: bool,
    pub(super) ssh_auth_method_dropdown_open: bool,
    pub(super) ssh_proxy_type_dropdown_open: bool,
    pub(super) ssh_terminal_type_dropdown_open: bool,
    pub(super) ssh_charset_dropdown_open: bool,
    pub(super) ssh_icon_color_dropdown_open: bool,
    pub(super) ssh_folder_dropdown_open: bool,
    pub(super) ssh_keep_alive_max_dropdown_open: bool,
    pub(super) ssh_channel_buffer_dropdown_open: bool,
    pub(super) ssh_gex_min_dropdown_open: bool,
    pub(super) ssh_gex_preferred_dropdown_open: bool,
    pub(super) ssh_gex_max_dropdown_open: bool,

    // Dropdown bounds for overlay positioning
    compression_dropdown_bounds: Option<Bounds<Pixels>>,
    strict_host_key_dropdown_bounds: Option<Bounds<Pixels>>,
    max_packets_dropdown_bounds: Option<Bounds<Pixels>>,
    recv_window_dropdown_bounds: Option<Bounds<Pixels>>,
    auth_method_dropdown_bounds: Option<Bounds<Pixels>>,
    proxy_type_dropdown_bounds: Option<Bounds<Pixels>>,
    terminal_type_dropdown_bounds: Option<Bounds<Pixels>>,
    charset_dropdown_bounds: Option<Bounds<Pixels>>,
    icon_color_dropdown_bounds: Option<Bounds<Pixels>>,
    folder_dropdown_bounds: Option<Bounds<Pixels>>,
    keep_alive_max_dropdown_bounds: Option<Bounds<Pixels>>,
    channel_buffer_dropdown_bounds: Option<Bounds<Pixels>>,
    gex_min_dropdown_bounds: Option<Bounds<Pixels>>,
    gex_preferred_dropdown_bounds: Option<Bounds<Pixels>>,
    gex_max_dropdown_bounds: Option<Bounds<Pixels>>,

    search_input: Option<Entity<InputState>>,
    show_search: bool,
    request_broker: Entity<RequestBroker>,

    /// Multi-selection: set of currently selected node ids (folders and sessions).
    selected_node_ids: std::collections::HashSet<String>,
    /// Anchor node for Shift-range selection (last node selected without Shift).
    selection_anchor: Option<String>,
    /// 键盘导航用的焦点索引（基于 `visible_order`）。相比仅靠 id 用
    /// `position` 查找索引，显式维护索引在存在重复 id 时也不会跳错节点。
    focused_index: Option<usize>,
    /// Flattened ids of currently visible nodes in display order (folders + sessions),
    /// updated on every render. Used to compute Shift-range selection and open order.
    visible_order: Vec<String>,
    /// Screen bounds of currently selected nodes (keyed by node id), captured during paint
    /// to anchor origin-aware confirmation modal animations when triggered via keyboard (e.g. Delete).
    selected_node_bounds: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Bounds<Pixels>>>>,
    project_selector_bounds: Bounds<Pixels>,
    settings_button_bounds: Bounds<Pixels>,
    add_session_button_bounds: Bounds<Pixels>,
    active_menu: Option<Entity<PopupMenu>>,
    menu_blur_subscription: Option<gpui::Subscription>,
    /// 记录底部菜单最近一次被关闭的 (overlay_id, 时刻)。
    /// 当菜单已打开时再次点击对应触发按钮，window 级的 click-outside
    /// 会先在 MouseDown 阶段关闭菜单（此时记录该值），随后按钮的
    /// on_click（MouseUp 阶段）才触发。open 处理函数据此判定“本次点击
    /// 应视为收起”，避免误判为重新打开。
    menu_toggle_guard: Option<(SharedString, std::time::Instant)>,
    /// Registry the footer menus register with for centralized
    /// click-outside dismissal. Injected after construction (the
    /// `OverlayRegistry` is created after this panel in `WindowView::new`).
    overlay_registry: Option<Entity<OverlayRegistry>>,
    context_menu: Option<Entity<PopupMenu>>,
    /// 弹窗打开前的原物理焦点句柄，用于 Esc/取消/关闭时精准回溯历史焦点
    dialog_previous_focus: Option<FocusHandle>,
}

fn convert_session_tree_nodes(
    nodes: &[SessionTreeNode],
) -> Vec<velowork_ui::TreeNodeData<String, SessionTreeNode>> {
    nodes
        .iter()
        .map(|node| match node {
            SessionTreeNode::Folder {
                id, name, children, ..
            } => velowork_ui::TreeNodeData::new(id.clone(), name.clone(), true)
                .with_children(convert_session_tree_nodes(children))
                .with_payload(node.clone()),
            SessionTreeNode::Session { session } => {
                velowork_ui::TreeNodeData::new(session.id.clone(), session.name.clone(), false)
                    .with_payload(node.clone())
            }
        })
        .collect()
}

fn collect_session_expanded_keys(
    nodes: &[SessionTreeNode],
    out: &mut std::collections::HashSet<String>,
) {
    for node in nodes {
        if let SessionTreeNode::Folder {
            id,
            children,
            is_collapsed,
            ..
        } = node
        {
            if !is_collapsed {
                out.insert(id.clone());
                collect_session_expanded_keys(children, out);
            }
        }
    }
}

impl EventEmitter<SessionPanelEvent> for SessionPanel {}

impl SessionPanel {
    pub fn new(
        window_id: WorkspaceWindowId,
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        request_broker: Entity<RequestBroker>,
        cx: &mut Context<Self>,
    ) -> Self {
        let folder_name_input =
            cx.new(|cx| SimpleInputState::new(cx).placeholder("Folder name..."));
        let session_name_input =
            cx.new(|cx| SimpleInputState::new(cx).placeholder("Connection name"));
        let session_host_input =
            cx.new(|cx| SimpleInputState::new(cx).placeholder("192.168.1.100 or example.com"));
        let session_port_input = cx.new(|cx| {
            let mut s = SimpleInputState::new(cx);
            s.set_value("22", cx);
            s.placeholder("22")
        });
        let session_username_input = cx.new(|cx| SimpleInputState::new(cx).placeholder("root"));
        let session_password_input =
            cx.new(|cx| SimpleInputState::new(cx).placeholder("Password").password());
        let session_key_path_input =
            cx.new(|cx| SimpleInputState::new(cx).placeholder("~/.ssh/id_rsa"));
        let session_passphrase_input = cx.new(|cx| {
            SimpleInputState::new(cx)
                .placeholder("Passphrase (optional)")
                .password()
        });
        let session_timeout_input = cx.new(|cx| {
            let mut s = SimpleInputState::new(cx);
            s.set_value("30", cx);
            s.placeholder("30")
        });
        let session_keepalive_input = cx.new(|cx| {
            let mut s = SimpleInputState::new(cx);
            s.set_value("30", cx);
            s.placeholder("30")
        });
        let session_idle_disconnect_input = cx.new(|cx| {
            let mut s = SimpleInputState::new(cx);
            s.set_value("0", cx);
            s.placeholder("0 (disabled)")
        });
        let session_proxy_host_input =
            cx.new(|cx| SimpleInputState::new(cx).placeholder("127.0.0.1"));
        let session_proxy_port_input = cx.new(|cx| SimpleInputState::new(cx).placeholder("1080"));
        let session_proxy_username_input =
            cx.new(|cx| SimpleInputState::new(cx).placeholder("Username (optional)"));
        let session_proxy_password_input = cx.new(|cx| {
            SimpleInputState::new(cx)
                .placeholder("Password (optional)")
                .password()
        });
        let session_scrollback_input = cx.new(|cx| {
            let mut s = SimpleInputState::new(cx);
            s.set_value("10000", cx);
            s.placeholder("10000")
        });
        let session_tags_input =
            cx.new(|cx| SimpleInputState::new(cx).placeholder("production, web, nginx"));
        let session_notes_input = cx.new(|cx| {
            SimpleInputState::new(cx)
                .multiline()
                .placeholder("Notes about this connection...")
        });
        let session_startup_command_input = cx.new(|cx| {
            SimpleInputState::new(cx)
                .multiline()
                .placeholder("Startup commands (optional)")
        });

        let this = Self {
            _window_id: window_id,
            workspace,
            focus_manager: focus_manager.clone(),
            focus_handle: cx.focus_handle(),
            tree_scroll_handle: ScrollHandle::new(),
            active_dialog: None,
            session_dialog: None,
            dialog_panel_id: None,
            focus_dialog_inputs: std::cell::Cell::new(false),
            inline_folder: None,
            inline_folder_sub: None,
            inline_session: None,
            inline_session_sub: None,
            folder_name_input,
            ssh_dialog_tab: SshDialogTab::General,
            session_name_input,
            session_icon_color: IconColor::default(),
            session_parent_folder_id: None,
            session_startup_command_input,
            session_username_input,
            session_auth_type: SshAuthType::Password { password: None },
            session_password_input,
            session_key_path_input,
            session_passphrase_input,
            session_host_input,
            session_port_input,
            session_timeout_input,
            session_keepalive_input,
            session_idle_disconnect_input,
            session_keepalive_strategy: KeepAliveStrategy::Always,
            session_keepalive_max: 3,
            session_tcp_nodelay: true,
            session_channel_buffer_size: 100,
            session_proxy_type: ProxyType::None,
            session_proxy_host_input,
            session_proxy_port_input,
            session_proxy_username_input,
            session_proxy_password_input,
            session_enable_sftp: true,
            session_enable_monitor: false,
            session_monitor_cpu: true,
            session_monitor_mem: true,
            session_monitor_disk: false,
            session_terminal_type: None,
            session_charset: None,
            session_scrollback_input,
            session_compression: CompressionType::default(),
            session_strict_host_key: StrictHostKey::default(),
            session_max_packets: 32768,
            session_recv_window: 2097152,
            session_gex_min: 2048,
            session_gex_preferred: 4096,
            session_gex_max: 8192,
            session_kex_algorithms: vec![
                "curve25519-sha256".into(),
                "ecdh-sha2-nistp256".into(),
                "ecdh-sha2-nistp384".into(),
                "ecdh-sha2-nistp521".into(),
                "diffie-hellman-group14-sha256".into(),
                "diffie-hellman-group16-sha512".into(),
                "diffie-hellman-group18-sha512".into(),
            ],
            session_cipher_algorithms: vec![
                "chacha20-poly1305@openssh.com".into(),
                "aes256-gcm@openssh.com".into(),
                "aes128-gcm@openssh.com".into(),
                "aes256-ctr".into(),
                "aes192-ctr".into(),
                "aes128-ctr".into(),
            ],
            session_mac_algorithms: vec![
                "hmac-sha2-256-etm@openssh.com".into(),
                "hmac-sha2-512-etm@openssh.com".into(),
                "hmac-sha2-256".into(),
                "hmac-sha2-512".into(),
                "umac-128-etm@openssh.com".into(),
            ],
            session_hostkey_algorithms: vec![
                "ssh-ed25519".into(),
                "rsa-sha2-512".into(),
                "rsa-sha2-256".into(),
            ],
            session_tags_input,
            session_notes_input,
            startup_command_height: None,
            notes_height: None,
            resize_dragging: None,
            test_connection_status: TestConnectionStatus::Idle,
            validation_error: None,
            ssh_compression_dropdown_open: false,
            ssh_strict_host_key_dropdown_open: false,
            ssh_max_packets_dropdown_open: false,
            ssh_recv_window_dropdown_open: false,
            ssh_auth_method_dropdown_open: false,
            ssh_proxy_type_dropdown_open: false,
            ssh_terminal_type_dropdown_open: false,
            ssh_charset_dropdown_open: false,
            ssh_icon_color_dropdown_open: false,
            ssh_folder_dropdown_open: false,
            ssh_keep_alive_max_dropdown_open: false,
            ssh_channel_buffer_dropdown_open: false,
            ssh_gex_min_dropdown_open: false,
            ssh_gex_preferred_dropdown_open: false,
            ssh_gex_max_dropdown_open: false,
            compression_dropdown_bounds: None,
            strict_host_key_dropdown_bounds: None,
            max_packets_dropdown_bounds: None,
            recv_window_dropdown_bounds: None,
            auth_method_dropdown_bounds: None,
            proxy_type_dropdown_bounds: None,
            terminal_type_dropdown_bounds: None,
            charset_dropdown_bounds: None,
            icon_color_dropdown_bounds: None,
            folder_dropdown_bounds: None,
            keep_alive_max_dropdown_bounds: None,
            channel_buffer_dropdown_bounds: None,
            gex_min_dropdown_bounds: None,
            gex_preferred_dropdown_bounds: None,
            gex_max_dropdown_bounds: None,
            search_input: None,
            show_search: false,
            request_broker,
            selected_node_ids: std::collections::HashSet::new(),
            selection_anchor: None,
            focused_index: None,
            visible_order: Vec::new(),
            selected_node_bounds: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
            project_selector_bounds: Bounds::default(),
            settings_button_bounds: Bounds::default(),
            add_session_button_bounds: Bounds::default(),
            active_menu: None,
            overlay_registry: None,
            overlay_manager: None,
            dialog_origin: None,
            dialog_motion_state: velowork_ui::motion::ModalMotionState::default(),
            dialog_anim_task: None,
            menu_blur_subscription: None,
            menu_toggle_guard: None,
            context_menu: None,
            dialog_previous_focus: None,
        };

        let inputs = vec![
            this.folder_name_input.clone(),
            this.session_name_input.clone(),
            this.session_startup_command_input.clone(),
            this.session_username_input.clone(),
            this.session_password_input.clone(),
            this.session_key_path_input.clone(),
            this.session_passphrase_input.clone(),
            this.session_host_input.clone(),
            this.session_port_input.clone(),
            this.session_timeout_input.clone(),
            this.session_keepalive_input.clone(),
            this.session_proxy_host_input.clone(),
            this.session_proxy_port_input.clone(),
            this.session_proxy_username_input.clone(),
            this.session_proxy_password_input.clone(),
            this.session_scrollback_input.clone(),
            this.session_tags_input.clone(),
            this.session_notes_input.clone(),
        ];

        for input in inputs {
            cx.subscribe(&input, |this, _, _: &InputFocusedEvent, cx| {
                this.focus_manager.update(cx, |fm, cx| {
                    this.workspace.update(cx, |ws, cx| {
                        ws.clear_focused_terminal(fm, cx);
                    });
                });
                cx.notify();
            })
            .detach();
        }

        cx.observe(&focus_manager, |_, _, cx| {
            cx.notify();
        })
        .detach();

        cx.observe(&this.workspace, |_, _, cx| {
            cx.notify();
        })
        .detach();

        // Re-render when SSH connection state changes (connect / disconnect) so
        // session-tree icons update immediately — e.g. after the last terminal
        // tab for a session is closed the icon reverts to its default color.
        let connection_store = cx
            .global::<velowork_workspace::stores::GlobalConnectionStore>()
            .0
            .clone();
        cx.subscribe(
            &connection_store,
            |_, _, _: &velowork_workspace::stores::ConnectionEvent, cx| {
                cx.notify();
            },
        )
        .detach();

        // 重新渲染当全盘 SessionStore 发生变化时（新增/改名/删除/树替换）
        if cx.has_global::<velowork_workspace::stores::GlobalSessionStore>() {
            let session_store = cx
                .global::<velowork_workspace::stores::GlobalSessionStore>()
                .0
                .clone();
            cx.subscribe(
                &session_store,
                |_, _, _: &velowork_workspace::stores::SessionEvent, cx| {
                    cx.notify();
                },
            )
            .detach();
        }

        // 重新渲染当 App 设置项发生变化时（侧边栏配置/字体/色彩主题/同步等）
        cx.observe(&settings_entity(cx), |_, _, cx| {
            cx.notify();
        })
        .detach();

        this
    }

    fn select_project(&self, project_id: Option<String>, cx: &mut Context<Self>) {
        let focus_manager = self.focus_manager.clone();
        let workspace = self.workspace.clone();
        let window_id = self._window_id;
        focus_manager.update(cx, |fm, cx| {
            workspace.update(cx, |ws, cx| {
                ws.set_focused_project(fm, project_id.clone(), window_id, cx);
                // 选中一个具体项目时，确保它在当前窗口可见（取消隐藏），
                // 否则从 footer 菜单点击处于隐藏态的项目不会有任何视觉反馈。
                if let Some(ref pid) = project_id {
                    let is_hidden = ws
                        .data
                        .window(window_id)
                        .map(|w| w.hidden_project_ids.contains(pid))
                        .unwrap_or(false);
                    if is_hidden {
                        ws.toggle_hidden(window_id, pid, cx);
                    }
                }
            });
            cx.notify();
        });
    }

    fn open_manage_projects_dialog(&self, cx: &mut Context<Self>) {
        self.request_broker.update(cx, |broker, cx| {
            broker.push_overlay_request(OverlayRequest::ManageProjectsDialog, cx);
        });
    }

    pub fn open_import_sessions_dialog(&self, cx: &mut Context<Self>) {
        self.request_broker.update(cx, |broker, cx| {
            broker.push_overlay_request(OverlayRequest::ImportSessionsDialog, cx);
        });
    }

    fn set_all_folders_collapsed(&self, collapsed: bool, cx: &mut Context<Self>) {
        let store = cx
            .global::<velowork_workspace::stores::GlobalSessionStore>()
            .0
            .clone();
        let active_pid = self.active_project_id(cx);
        store.update(cx, |store, cx| {
            store.set_all_folders_collapsed_for_project(active_pid.as_deref(), collapsed, cx);
        });
    }

    fn toggle_folder(&self, folder_id: &str, cx: &mut Context<Self>) {
        let store = cx
            .global::<velowork_workspace::stores::GlobalSessionStore>()
            .0
            .clone();
        let active_pid = self.active_project_id(cx);
        store.update(cx, |store, cx| {
            store.toggle_folder_collapsed_for_project(active_pid.as_deref(), folder_id, cx);
        });
        cx.notify();
    }

    fn render_tree_node(
        &mut self,
        nodes: &[SessionTreeNode],
        t: &ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        self.selected_node_bounds.borrow_mut().clear();
        let tree_data = convert_session_tree_nodes(nodes);
        let mut expanded_keys = std::collections::HashSet::new();
        collect_session_expanded_keys(nodes, &mut expanded_keys);

        let is_panel_active = self.is_selection_active(window, cx);
        let active_selected_keys = if is_panel_active {
            self.selected_node_ids.clone()
        } else {
            std::collections::HashSet::new()
        };

        let t_color = t.clone();
        let tree_widget = velowork_ui::tree::<Self, String, SessionTreeNode>("session-tree")
            .nodes(tree_data)
            .expanded_keys(expanded_keys)
            .selected_keys(active_selected_keys)
            .render_before_children({
                let t_color = t_color.clone();
                move |this, parent_key, depth, window, cx| {
                    let parent_id = parent_key.map(|k| k.as_str());
                    let is_creating_here = match &this.inline_folder {
                        Some(InlineFolderState::Creating {
                            parent_id: target_pid,
                            ..
                        }) => target_pid.as_deref() == parent_id,
                        _ => false,
                    };
                    if is_creating_here {
                        return Some(this.render_inline_folder_input_row(
                            "session-inline-folder-create",
                            false,
                            depth,
                            &t_color,
                            window,
                            cx,
                        ));
                    }
                    None
                }
            })
            .render_node({
                let t_color = t_color.clone();
                move |this, tree_ctx, window, cx| {
                    let node = match tree_ctx.node.payload.as_ref() {
                        Some(n) => n,
                        None => return div().into_any_element(),
                    };

                    match node {
                        SessionTreeNode::Folder {
                            id,
                            name,
                            children,
                            is_collapsed,
                        } => {
                            let is_renaming_this = match &this.inline_folder {
                                Some(InlineFolderState::Renaming { id: target_id, .. }) => {
                                    target_id == id
                                }
                                _ => false,
                            };
                            if is_renaming_this {
                                return this.render_inline_folder_input_row(
                                    id,
                                    !is_collapsed,
                                    tree_ctx.depth,
                                    &t_color,
                                    window,
                                    cx,
                                );
                            }

                            let mut elements = Vec::new();
                            this.render_folder_row(
                                &mut elements,
                                id,
                                name,
                                children,
                                *is_collapsed,
                                tree_ctx.depth,
                                0,
                                tree_ctx.parent_key.map(|k| k.as_str()),
                                &t_color,
                                tree_ctx.is_selected,
                                cx,
                            );
                            elements.pop().unwrap_or_else(|| div().into_any_element())
                        }
                        SessionTreeNode::Session { session } => {
                            let is_renaming_this = match &this.inline_session {
                                Some(state) => state.id == session.id,
                                None => false,
                            };
                            if is_renaming_this {
                                let has_tab =
                                    this.workspace.read(cx).data().has_session_tab(&session.id);
                                let connection_store = cx
                                    .global::<velowork_workspace::stores::GlobalConnectionStore>()
                                    .0
                                    .read(cx);
                                let is_connected = connection_store.is_connected(&session.id);
                                let icon_color = if has_tab {
                                    if is_connected {
                                        Some(t_color.success)
                                    } else {
                                        Some(t_color.error)
                                    }
                                } else {
                                    session.icon_color.color_hex()
                                };
                                return this.render_inline_session_input_row(
                                    tree_ctx.depth,
                                    session_icon(session),
                                    icon_color,
                                    is_connected,
                                    &t_color,
                                    window,
                                    cx,
                                );
                            }

                            let mut elements = Vec::new();
                            this.render_session_row(
                                &mut elements,
                                session,
                                tree_ctx.depth,
                                0,
                                tree_ctx.parent_key.map(|k| k.as_str()),
                                &t_color,
                                tree_ctx.is_selected,
                                cx,
                            );
                            elements.pop().unwrap_or_else(|| div().into_any_element())
                        }
                    }
                }
            });

        vec![tree_widget.render(self, window, cx)]
    }

    fn render_inline_folder_input_row(
        &mut self,
        // Stable node id of the folder being created/renamed. Passed through to
        // `folder_tree_icon` so the inline row's icon is the *same* element (and
        // therefore keeps the exact same open/closed state) as the folder row it
        // replaces — a rename must never reset or swap the icon.
        folder_id: &str,
        // True when the folder is expanded, so the inline row shows the open
        // glyph (matching the pre-rename row) instead of forcing it closed.
        is_expanded: bool,
        depth: usize,
        t: &ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_ent = if let Some(ref mut state) = self.inline_folder {
            match state {
                InlineFolderState::Creating { input, .. } => {
                    if input.is_none() {
                        let ph = i18n!(cx, "dock.folder_name_placeholder");
                        let i = cx.new(|cx| InputState::new(cx).placeholder(ph));
                        *input = Some(i);
                    }
                    input.clone()
                }
                InlineFolderState::Renaming {
                    input,
                    original_name,
                    ..
                } => {
                    if input.is_none() {
                        let name = original_name.clone();
                        let i = cx.new(|cx| InputState::new(cx).default_value(name));
                        *input = Some(i);
                    }
                    input.clone()
                }
            }
        } else {
            None
        };

        let Some(input) = input_ent else {
            return div().into_any_element();
        };

        let indent = px(depth as f32 * f32::from(ui_space_tree_indent(cx)));
        let focus_handle = input.focus_handle(cx);

        if self.inline_folder_sub.is_none() {
            window.focus(&focus_handle, cx);
            self.inline_folder_sub =
                Some(cx.on_blur(&focus_handle, window, move |this, window, cx| {
                    this.commit_inline_folder(cx);
                    if this.inline_folder.is_none() {
                        window.focus(&this.focus_handle, cx);
                    }
                }));
        }

        let current_val = input.read(cx).text().to_string().trim().to_string();
        let (parent_id, except_id) = match &self.inline_folder {
            Some(InlineFolderState::Creating { parent_id, .. }) => (parent_id.clone(), None),
            Some(InlineFolderState::Renaming { id, .. }) => {
                let store = cx
                    .global::<velowork_workspace::stores::GlobalSessionStore>()
                    .0
                    .read(cx);
                let parent_id = velowork_workspace::stores::SessionStore::find_parent_id(
                    self.active_tree(store, cx),
                    id,
                );
                (parent_id, Some(id.clone()))
            }
            None => (None, None),
        };

        let is_duplicate = if !current_val.is_empty() {
            let store = cx
                .global::<velowork_workspace::stores::GlobalSessionStore>()
                .0
                .read(cx);
            velowork_workspace::stores::SessionStore::folder_name_exists_in_parent(
                self.active_tree(store, cx),
                parent_id.as_deref(),
                &current_val,
                except_id.as_deref(),
            )
        } else {
            false
        };

        let ap = tree_row_appearance(t, cx);
        let icon_size = ap.icon_size;

        div()
            .id("session-inline-folder-container")
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .id("session-inline-folder-row")
                    .flex()
                    .items_center()
                    .h(ap.height)
                    .pl(indent + ui_space_sm(cx))
                    .pr(ui_space_lg(cx))
                    .child(
                        AppIcon::ChevronRight
                            .size(icon_size)
                            .text_color(rgb(t.text_muted))
                            .mr(ui_space_xs(cx))
                            .flex_shrink_0(),
                    )
                    .child(div().mr(ui_space_xs(cx)).child(folder_tree_icon(
                        folder_id,
                        is_expanded,
                        depth,
                        icon_size,
                        t,
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(velowork_ui::Input::new(&input))
                            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                if e.keystroke.key == "enter" {
                                    if !is_duplicate {
                                        this.commit_inline_folder(cx);
                                    }
                                    window.focus(&this.focus_handle, cx);
                                    cx.stop_propagation();
                                } else if e.keystroke.key == "escape" {
                                    this.cancel_inline_folder(cx);
                                    window.focus(&this.focus_handle, cx);
                                    cx.stop_propagation();
                                }
                            })),
                    ),
            )
            .when(is_duplicate, |container| {
                container.child(
                    div()
                        .my(px(2.0))
                        .ml(indent + px(40.0))
                        .mr(ui_space_lg(cx))
                        .px(ui_space_md(cx))
                        .py(ui_space_xs(cx))
                        .bg(surface_bg_t(t.bg_secondary, &t))
                        .border_1()
                        .border_color(rgb(t.error))
                        .rounded(RADIUS_STD)
                        .child(
                            div()
                                .text_size(ui_text(11.0, cx))
                                .text_color(rgb(t.error))
                                .child(i18n!(cx, "dock.duplicate_folder_error")),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_inline_session_input_row(
        &mut self,
        depth: usize,
        icon: AppIcon,
        // Connection-derived icon colour, identical to the normal session row.
        icon_color: Option<u32>,
        // Whether the session is connected — drives the left-edge accent bar,
        // matching the normal session row's `is_open` flag.
        is_connected: bool,
        t: &ThemeColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_ent = if let Some(ref mut state) = self.inline_session {
            if state.input.is_none() {
                let name = state.original_name.clone();
                let i = cx.new(|cx| InputState::new(cx).default_value(name));
                state.input = Some(i);
            }
            state.input.clone()
        } else {
            None
        };

        let Some(input) = input_ent else {
            return div().into_any_element();
        };

        let indent = px(depth as f32 * f32::from(ui_space_tree_indent(cx)));
        let ap = tree_row_appearance(t, cx);
        let icon_size = ap.icon_size;
        let focus_handle = input.focus_handle(cx);

        if self.inline_session_sub.is_none() {
            window.focus(&focus_handle, cx);
            self.inline_session_sub =
                Some(cx.on_blur(&focus_handle, window, move |this, window, cx| {
                    this.commit_inline_session(cx);
                    if this.inline_session.is_none() {
                        window.focus(&this.focus_handle, cx);
                    }
                }));
        }

        div()
            .id("session-inline-session-container")
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .id("session-inline-session-row")
                    .relative()
                    .flex()
                    .items_center()
                    .h(ap.height)
                    .pl(indent + ui_space_sm(cx))
                    .pr(ui_space_lg(cx))
                    .when(is_connected, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left_0()
                                .top(px(5.0))
                                .bottom(px(5.0))
                                .w(px(2.0))
                                .bg(rgb(t.border_active))
                                .rounded_r(px(1.0)),
                        )
                    })
                    .child(div().w(icon_size).h(icon_size).mr(px(4.0)).flex_shrink_0())
                    .child(
                        icon.size(icon_size)
                            .text_color(rgb(icon_color.unwrap_or(t.text_secondary)))
                            .mr(px(4.0))
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(velowork_ui::Input::new(&input))
                            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                if e.keystroke.key == "enter" {
                                    this.commit_inline_session(cx);
                                    window.focus(&this.focus_handle, cx);
                                    cx.stop_propagation();
                                } else if e.keystroke.key == "escape" {
                                    this.cancel_inline_session(cx);
                                    window.focus(&this.focus_handle, cx);
                                    cx.stop_propagation();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_folder_row(
        &self,
        elements: &mut Vec<AnyElement>,
        folder_id: &str,
        folder_name: &str,
        _children: &[SessionTreeNode],
        is_collapsed: bool,
        depth: usize,
        _index: usize,
        _parent_id: Option<&str>,
        t: &ThemeColors,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) {
        let is_expanded = !is_collapsed;
        let _indent = depth as f32 * 14.0;
        let fid = folder_id.to_string();
        let fid_ctx = folder_id.to_string();
        let fname = folder_name.to_string();
        let fname_drag = folder_name.to_string();
        let fid_drag = folder_id.to_string();
        let _fname_drop = folder_name.to_string();
        let fid_drop = folder_id.to_string();
        let t_border = t.border_active;
        let t_bg = t.bg_selection;

        elements.push(
            expandable_folder_row(
                folder_id,
                folder_name,
                depth,
                is_expanded,
                is_selected,
                t,
                cx,
            )
            .id(ElementId::Name(
                format!("session-folder-{}", folder_id).into(),
            ))
            .rounded(RADIUS_STD)
            .border_1()
            .border_color(with_alpha(0x00000000, 0.0))
            .when(is_selected, |d| {
                let fid = folder_id.to_string();
                let bounds_map = self.selected_node_bounds.clone();
                d.bg(surface_bg(t.bg_selection, cx))
                    .border_color(rgb(t.border_active))
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                bounds_map.borrow_mut().insert(fid, bounds);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
            })
            // Drag source: this folder can be dragged
            .on_drag(
                SessionTreeFolderDrag {
                    folder_id: fid_drag.clone(),
                    folder_name: fname_drag.clone(),
                },
                move |drag, position, _window, cx| {
                    let cursor_y = position.y;
                    let cursor_x = position.x;
                    cx.new(move |_| SessionTreeFolderDragView {
                        name: drag.folder_name.clone(),
                        cursor_offset_x: cursor_x,
                        cursor_offset_y: cursor_y,
                    })
                },
            )
            // Drop target: session dragged over this folder → highlight (drop into)
            .drag_over::<SessionTreeSessionDrag>(move |style, _, _, _| style.bg(rgb(t_bg)))
            .on_drop(cx.listener({
                let folder_id = fid_drop.clone();
                move |this, drag: &SessionTreeSessionDrag, _window, cx| {
                    let store = cx
                        .global::<velowork_workspace::stores::GlobalSessionStore>()
                        .0
                        .clone();
                    let active_pid = this.active_project_id(cx);
                    store.update(cx, |store, cx| {
                        // Move session into this folder (append to end)
                        let child_count = store
                            .tree_for_project(active_pid.as_deref())
                            .iter()
                            .find(|n| n.id() == &folder_id)
                            .and_then(|n| {
                                if let SessionTreeNode::Folder { children, .. } = n {
                                    Some(children.len())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        store.move_node_for_project(
                            active_pid.as_deref(),
                            &drag.session_id,
                            Some(&folder_id),
                            child_count,
                            cx,
                        );
                    });
                }
            }))
            // Drop target: folder dragged over this folder → insertion line above (reorder)
            .drag_over::<SessionTreeFolderDrag>(move |style, _, _, _| {
                style.border_t_2().border_color(rgb(t_border))
            })
            .on_drop(cx.listener({
                let folder_id = fid_drop.clone();
                move |this, drag: &SessionTreeFolderDrag, _window, cx| {
                    if drag.folder_id != folder_id {
                        let store = cx
                            .global::<velowork_workspace::stores::GlobalSessionStore>()
                            .0
                            .clone();
                        // Find the index of this folder in its parent
                        let target_index = {
                            let store_ref = store.read(cx);
                            Self::find_node_index_in_parent(
                                this.active_tree(store_ref, cx),
                                &folder_id,
                            )
                        };
                        if let Some(idx) = target_index {
                            let parent_id = {
                                let store_ref = store.read(cx);
                                velowork_workspace::stores::SessionStore::find_parent_id(
                                    this.active_tree(store_ref, cx),
                                    &folder_id,
                                )
                            };
                            let active_pid = this.active_project_id(cx);
                            store.update(cx, |store, cx| {
                                store.move_node_for_project(
                                    active_pid.as_deref(),
                                    &drag.folder_id,
                                    parent_id.as_deref(),
                                    idx,
                                    cx,
                                );
                            });
                        }
                    }
                }
            }))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.handle_node_click(&fid, true, event, window, cx);
                // Prevent the tree-container click from clearing the selection.
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_tree_context_menu(
                        event.position,
                        fid_ctx.clone(),
                        fname.clone(),
                        true,
                        false,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .into_any_element(),
        );
    }

    fn render_session_row(
        &self,
        elements: &mut Vec<AnyElement>,
        session: &SshSession,
        depth: usize,
        _index: usize,
        parent_id: Option<&str>,
        t: &ThemeColors,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) {
        let has_tab = self.workspace.read(cx).data().has_session_tab(&session.id);
        let connection_store = cx
            .global::<velowork_workspace::stores::GlobalConnectionStore>()
            .0
            .read(cx);
        let is_connected = connection_store.is_connected(&session.id);
        // Icon color follows the session's tab connection state:
        // * has tabs and at least one connected → green (success)
        // * has tabs but all dropped / failed   → red (error)
        // * no tabs in workspace                → default neutral color
        let icon_color = if has_tab {
            if is_connected {
                Some(t.success)
            } else {
                Some(t.error)
            }
        } else {
            session.icon_color.color_hex()
        };
        let subtext = session_subtext(session);
        let subtext_opt = if subtext.is_empty() {
            None
        } else {
            Some(subtext.as_str())
        };
        let session_info = session_tooltip(session, cx);
        let sid = session.id.clone();
        let sid_ctx = session.id.clone();
        let sname = session.name.clone();
        let sid_drag = session.id.clone();
        let sname_drag = session.name.clone();
        let sid_drop = session.id.clone();
        let parent_id_owned = parent_id.map(|s| s.to_string());
        let t_border = t.border_active;

        elements.push(
            expandable_file_row(
                &session.name,
                subtext_opt,
                depth,
                None,
                Some(session_icon(session)),
                icon_color,
                is_connected,
                is_selected,
                t,
                cx,
            )
            .id(ElementId::Name(
                format!("session-row-{}", session.id).into(),
            ))
            .rounded(RADIUS_STD)
            .border_1()
            .border_color(with_alpha(0x00000000, 0.0))
            .when(is_selected, |d| {
                let sid = session.id.clone();
                let bounds_map = self.selected_node_bounds.clone();
                d.bg(surface_bg(t.bg_selection, cx))
                    .border_color(rgb(t.border_active))
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                bounds_map.borrow_mut().insert(sid, bounds);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
            })
            // Drag source: this session can be dragged
            .on_drag(
                SessionTreeSessionDrag {
                    session_id: sid_drag.clone(),
                    session_name: sname_drag.clone(),
                },
                move |drag, position, _window, cx| {
                    let cursor_y = position.y;
                    let cursor_x = position.x;
                    cx.new(move |_| SessionTreeSessionDragView {
                        name: drag.session_name.clone(),
                        cursor_offset_x: cursor_x,
                        cursor_offset_y: cursor_y,
                    })
                },
            )
            // Drop target: session dragged over this session → insertion line above (reorder)
            .drag_over::<SessionTreeSessionDrag>(move |style, _, _, _| {
                style.border_t_2().border_color(rgb(t_border))
            })
            .on_drop(cx.listener({
                let session_id = sid_drop.clone();
                let parent_id = parent_id_owned.clone();
                move |this, drag: &SessionTreeSessionDrag, _window, cx| {
                    if drag.session_id != session_id {
                        let store = cx
                            .global::<velowork_workspace::stores::GlobalSessionStore>()
                            .0
                            .clone();
                        // Find the index of this session in its parent
                        let target_index = {
                            let store_ref = store.read(cx);
                            Self::find_node_index_in_parent(
                                this.active_tree(store_ref, cx),
                                &session_id,
                            )
                        };
                        if let Some(idx) = target_index {
                            let active_pid = this.active_project_id(cx);
                            store.update(cx, |store, cx| {
                                store.move_node_for_project(
                                    active_pid.as_deref(),
                                    &drag.session_id,
                                    parent_id.as_deref(),
                                    idx,
                                    cx,
                                );
                            });
                        }
                    }
                }
            }))
            // Drop target: folder dragged over this session → insertion line below (reorder folder after)
            .drag_over::<SessionTreeFolderDrag>(move |style, _, _, _| {
                style.border_b_2().border_color(rgb(t_border))
            })
            .on_drop(cx.listener({
                let session_id = sid_drop.clone();
                let parent_id = parent_id_owned.clone();
                move |this, drag: &SessionTreeFolderDrag, _window, cx| {
                    if drag.folder_id != session_id {
                        let store = cx
                            .global::<velowork_workspace::stores::GlobalSessionStore>()
                            .0
                            .clone();
                        // Insert folder after this session
                        let target_index = {
                            let store_ref = store.read(cx);
                            Self::find_node_index_in_parent(
                                this.active_tree(store_ref, cx),
                                &session_id,
                            )
                            .map(|i| i + 1)
                        };
                        if let Some(idx) = target_index {
                            let active_pid = this.active_project_id(cx);
                            store.update(cx, |store, cx| {
                                store.move_node_for_project(
                                    active_pid.as_deref(),
                                    &drag.folder_id,
                                    parent_id.as_deref(),
                                    idx,
                                    cx,
                                );
                            });
                        }
                    }
                }
            }))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                // Single/Ctrl/Shift click = select; plain double-click = connect.
                this.handle_node_click(&sid, false, event, window, cx);
                // Prevent the tree-container click from clearing the selection.
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_tree_context_menu(
                        event.position,
                        sid_ctx.clone(),
                        sname.clone(),
                        false,
                        false,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .tooltip(move |_, cx| {
                let __tip = session_info.clone();
                cx.new(|_| Tooltip::new(__tip)).into()
            })
            .into_any_element(),
        );
    }

    pub(crate) fn active_project_id(&self, cx: &App) -> Option<String> {
        let raw = self.focus_manager.read(cx).active_project_id().cloned();
        if let Some(ref pid) = raw {
            if self.workspace.read(cx).project(pid).is_some() {
                return raw;
            }
        }
        self.workspace.read(cx).projects().first().map(|p| p.id.clone())
    }

    fn active_tree<'a>(
        &self,
        store: &'a velowork_workspace::stores::SessionStore,
        cx: &App,
    ) -> &'a [SessionTreeNode] {
        let pid = self.active_project_id(cx);
        store.tree_for_project(pid.as_deref())
    }

    fn get_selected_folder_id(&self, cx: &App) -> Option<String> {
        let store = cx
            .global::<velowork_workspace::stores::GlobalSessionStore>()
            .0
            .read(cx);
        let active_pid = self.active_project_id(cx);
        let tree = store.tree_for_project(active_pid.as_deref());

        for id in &self.selected_node_ids {
            if Self::is_folder_node(tree, id) {
                return Some(id.clone());
            }
        }
        None
    }

    fn is_folder_node(nodes: &[SessionTreeNode], target_id: &str) -> bool {
        for node in nodes {
            match node {
                SessionTreeNode::Folder { id, children, .. } => {
                    if id == target_id {
                        return true;
                    }
                    if Self::is_folder_node(children, target_id) {
                        return true;
                    }
                }
                SessionTreeNode::Session { .. } => {}
            }
        }
        false
    }

    pub fn inline_create_folder(
        &mut self,
        parent_id: Option<String>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parent_id = parent_id.or_else(|| self.get_selected_folder_id(cx));
        let active_pid = self.active_project_id(cx);

        if let Some(pid) = &parent_id {
            let pid = pid.clone();
            let store = cx
                .global::<velowork_workspace::stores::GlobalSessionStore>()
                .0
                .clone();
            let pid_clone = active_pid.clone();
            store.update(cx, |s, cx| {
                s.set_folder_collapsed_for_project(pid_clone.as_deref(), &pid, false, cx);
            });
        }

        self.inline_folder_sub = None;
        self.inline_folder = Some(InlineFolderState::Creating {
            parent_id,
            input: None,
        });
        cx.notify();
    }

    pub fn inline_rename_folder(
        &mut self,
        id: String,
        name: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inline_folder_sub = None;
        self.inline_folder = Some(InlineFolderState::Renaming {
            id,
            original_name: name,
            input: None,
        });
        cx.notify();
    }

    pub fn cancel_inline_folder(&mut self, cx: &mut Context<Self>) {
        if self.inline_folder.is_some() {
            self.inline_folder = None;
            self.inline_folder_sub = None;
            cx.notify();
        }
    }

    pub fn commit_inline_folder(&mut self, cx: &mut Context<Self>) {
        self.inline_folder_sub = None;
        if let Some(state) = self.inline_folder.take() {
            match state {
                InlineFolderState::Creating { parent_id, input } => {
                    let val = input
                        .as_ref()
                        .map(|i| i.read(cx).text().to_string().trim().to_string())
                        .unwrap_or_default();
                    if !val.is_empty() {
                        let store = cx
                            .global::<velowork_workspace::stores::GlobalSessionStore>()
                            .0
                            .clone();
                        let active_pid = self.active_project_id(cx);
                        let is_dup = {
                            let store_ref = store.read(cx);
                            velowork_workspace::stores::SessionStore::folder_name_exists_in_parent(
                                store_ref.tree_for_project(active_pid.as_deref()),
                                parent_id.as_deref(),
                                &val,
                                None,
                            )
                        };
                        if is_dup {
                            self.inline_folder =
                                Some(InlineFolderState::Creating { parent_id, input });
                            cx.notify();
                            return;
                        }
                        let pid = active_pid.clone();
                        let parent_id_clone = parent_id.clone();
                        let created_id = store.update(cx, |s, cx| {
                            if let Some(ref p_id) = parent_id_clone {
                                s.set_folder_collapsed_for_project(pid.as_deref(), p_id, false, cx);
                            }
                            s.add_folder_for_project(
                                pid.as_deref(),
                                parent_id_clone.as_deref(),
                                val,
                                cx,
                            )
                        });
                        if let Some(new_id) = created_id {
                            self.selected_node_ids.clear();
                            self.selected_node_ids.insert(new_id.clone());
                            self.selection_anchor = Some(new_id);
                        }
                    }
                }
                InlineFolderState::Renaming {
                    id,
                    original_name,
                    input,
                } => {
                    let val = input
                        .as_ref()
                        .map(|i| i.read(cx).text().to_string().trim().to_string())
                        .unwrap_or_default();
                    if !val.is_empty() && val != original_name {
                        let store = cx
                            .global::<velowork_workspace::stores::GlobalSessionStore>()
                            .0
                            .clone();
                        let active_pid = self.active_project_id(cx);
                        let (parent_id, is_dup) = {
                            let store_ref = store.read(cx);
                            let tree = store_ref.tree_for_project(active_pid.as_deref());
                            let pid =
                                velowork_workspace::stores::SessionStore::find_parent_id(tree, &id);
                            let dup = velowork_workspace::stores::SessionStore::folder_name_exists_in_parent(
                                tree,
                                pid.as_deref(),
                                &val,
                                Some(&id),
                            );
                            (pid, dup)
                        };
                        if is_dup {
                            self.inline_folder = Some(InlineFolderState::Renaming {
                                id,
                                original_name,
                                input,
                            });
                            cx.notify();
                            return;
                        }
                        let _ = parent_id;
                        store.update(cx, |s, cx| {
                            s.rename_node_for_project(active_pid.as_deref(), &id, &val, cx);
                        });
                    }
                }
            }
            cx.notify();
        }
    }

    // ─── Inline session rename (行内重命名会话配置) ──────────────────────────

    pub fn inline_rename_session(
        &mut self,
        id: String,
        name: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inline_session_sub = None;
        self.inline_session = Some(InlineSessionRename {
            id,
            original_name: name,
            input: None,
        });
        cx.notify();
    }

    pub fn cancel_inline_session(&mut self, cx: &mut Context<Self>) {
        if self.inline_session.is_some() {
            self.inline_session = None;
            self.inline_session_sub = None;
            cx.notify();
        }
    }

    pub fn commit_inline_session(&mut self, cx: &mut Context<Self>) {
        self.inline_session_sub = None;
        if let Some(state) = self.inline_session.take() {
            let val = state
                .input
                .as_ref()
                .map(|i| i.read(cx).text().to_string().trim().to_string())
                .unwrap_or_default();
            if !val.is_empty() && val != state.original_name {
                let store = cx
                    .global::<velowork_workspace::stores::GlobalSessionStore>()
                    .0
                    .clone();
                let active_pid = self.active_project_id(cx);
                let dup = store.read(cx).tree_for_project(active_pid.as_deref());
                let parent_id =
                    velowork_workspace::stores::SessionStore::find_parent_id(dup, &state.id);
                if velowork_workspace::stores::SessionStore::node_name_exists_in_parent(
                    dup,
                    parent_id.as_deref(),
                    &val,
                    Some(&state.id),
                ) {
                    // 同目录已存在同名节点，保留输入框让用户输入其他名称。
                    self.inline_session = Some(InlineSessionRename {
                        id: state.id,
                        original_name: state.original_name,
                        input: state.input,
                    });
                    cx.notify();
                    return;
                }
                store.update(cx, |s, cx| {
                    s.rename_node_for_project(active_pid.as_deref(), &state.id, &val, cx);
                });
            }
            cx.notify();
        }
    }

    /// 同一目录下（含目录、会话、渠道）是否已存在同名节点。用于会话新建/编辑/
    /// 重命名的重名校验；`except_id` 排除节点自身（编辑/重命名场景）。
    fn session_name_conflicts(
        &self,
        cx: &App,
        parent_id: Option<&str>,
        name: &str,
        except_id: Option<&str>,
    ) -> bool {
        let store = cx
            .global::<velowork_workspace::stores::GlobalSessionStore>()
            .0
            .clone();
        let pid = self.active_project_id(cx);
        let tree = store.read(cx).tree_for_project(pid.as_deref());
        velowork_workspace::stores::SessionStore::node_name_exists_in_parent(
            tree, parent_id, name, except_id,
        )
    }

    // ─── Delete confirmation (弹窗确认删除，操作不可逆) ──────────────────────

    /// Open a confirmation dialog before deleting a folder or session node.
    /// The actual deletion happens only after the user confirms.
    pub fn request_delete_confirm(
        &mut self,
        node_id: String,
        node_label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let origin = Some(self.focus_handle.clone());
        let this_weak = cx.entity().downgrade();
        let on_confirmed = std::sync::Arc::new(
            move |deleted_id: &str, cx: &mut Context<crate::views::overlays::OverlayManager>| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| {
                        let deleted_pos = this.visible_order.iter().position(|id| id == deleted_id);
                        this.selected_node_ids.remove(deleted_id);
                        let remaining_visible: Vec<String> = this
                            .visible_order
                            .iter()
                            .filter(|id| id.as_str() != deleted_id)
                            .cloned()
                            .collect();
                        if let Some(idx) = deleted_pos {
                            if !remaining_visible.is_empty() {
                                let next_idx = if idx < remaining_visible.len() {
                                    idx
                                } else {
                                    remaining_visible.len() - 1
                                };
                                if let Some(next_id) = remaining_visible.get(next_idx).cloned() {
                                    this.selected_node_ids.clear();
                                    this.selected_node_ids.insert(next_id.clone());
                                    this.selection_anchor = Some(next_id.clone());
                                    this.focused_index = Some(next_idx);
                                    this.scroll_node_into_view(&next_id, cx);
                                }
                            } else {
                                this.focused_index = None;
                                this.selection_anchor = None;
                            }
                        }
                        this.focus_manager.update(cx, |fm, _| {
                            fm.request_focus(velowork_workspace::focus::FocusLayer::SessionTree);
                        });
                        cx.notify();
                    });
                }
            },
        );

        if let Some(om) = &self.overlay_manager {
            let origin_clone = origin.clone();
            let click_origin = self
                .selected_node_bounds
                .borrow()
                .get(&node_id)
                .map(|b| b.center())
                .or_else(|| {
                    self.selected_node_bounds
                        .borrow()
                        .values()
                        .next()
                        .map(|b| b.center())
                });
            om.update(cx, |om, cx| {
                if let Some(pt) = click_origin {
                    om.record_click_origin(pt);
                }
                om.request_session_delete_confirm_with_origin(
                    node_id,
                    node_label,
                    origin_clone.clone(),
                    origin_clone,
                    Some(on_confirmed),
                    cx,
                );
            });
        }
    }

    pub fn open_add_folder_dialog(
        &mut self,
        parent_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inline_create_folder(parent_id, window, cx);
    }

    pub fn open_edit_folder_dialog(
        &mut self,
        id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inline_rename_folder(id, name, window, cx);
    }

    pub fn open_add_session_dialog(&mut self, parent_id: Option<String>, cx: &mut Context<Self>) {
        self.open_add_session_dialog_at(
            parent_id,
            None,
            Some(velowork_state::SessionProtocol::Ssh),
            cx,
        );
    }

    pub fn open_add_session_with_protocol(
        &mut self,
        protocol: velowork_state::SessionProtocol,
        parent_id: Option<String>,
        origin: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        self.open_add_session_dialog_at(parent_id, origin, Some(protocol), cx);
    }

    pub fn open_add_session_dialog_at(
        &mut self,
        parent_id: Option<String>,
        origin: Option<Point<Pixels>>,
        protocol: Option<velowork_state::SessionProtocol>,
        cx: &mut Context<Self>,
    ) {
        let parent_id = parent_id.or_else(|| self.get_selected_folder_id(cx));
        self.session_parent_folder_id = parent_id.clone();
        self.active_dialog = Some(SessionPanelDialog::AddSession {
            parent_id: self.session_parent_folder_id.clone(),
            protocol,
        });
        self.session_dialog = None;
        self.test_connection_status = TestConnectionStatus::Idle;
        self.focus_dialog_inputs.set(true);
        self.focus_manager.update(cx, |fm, _| fm.enter_modal());
        self.dialog_origin = origin;
        self.dialog_motion_state = velowork_ui::motion::ModalMotionState::new_opening(origin);
        cx.notify();
    }

    pub fn open_edit_session_dialog(&mut self, session: SshSession, cx: &mut Context<Self>) {
        self.open_edit_session_dialog_at(session, None, cx);
    }

    pub fn open_edit_session_dialog_at(
        &mut self,
        session: SshSession,
        origin: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        self.session_parent_folder_id = session.parent_folder_id.clone();
        self.active_dialog = Some(SessionPanelDialog::EditSession { session });
        self.session_dialog = None;
        self.test_connection_status = TestConnectionStatus::Idle;
        self.focus_dialog_inputs.set(true);
        self.focus_manager.update(cx, |fm, _| fm.enter_modal());
        self.dialog_origin = origin;
        self.dialog_motion_state = velowork_ui::motion::ModalMotionState::new_opening(origin);
        cx.notify();
    }

    fn start_dialog_enter_animation(
        &mut self,
        origin: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        let enable_animations = crate::settings::settings_entity(cx)
            .read(cx)
            .settings
            .enable_animations;
        if enable_animations {
            self.dialog_motion_state = velowork_ui::motion::ModalMotionState::new_opening(origin);
            let total_dur = velowork_ui::motion::DURATION_MODAL_ENTER;
            self.dialog_anim_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
                let start = std::time::Instant::now();
                loop {
                    let elapsed = start.elapsed();
                    let progress = (elapsed.as_secs_f32() / total_dur.as_secs_f32()).min(1.0);
                    let res = this.update(cx, |this, cx| {
                        this.dialog_motion_state.progress = progress;
                        cx.notify();
                    });
                    if res.is_err() || progress >= 1.0 {
                        break;
                    }
                    smol::Timer::after(std::time::Duration::from_millis(8)).await;
                }
                let _ = this.update(cx, |this, cx| {
                    this.dialog_motion_state.progress = 1.0;
                    this.dialog_anim_task = None;
                    cx.notify();
                });
            }));
        } else {
            self.dialog_motion_state = velowork_ui::motion::ModalMotionState::default();
            self.dialog_anim_task = None;
        }
    }

    fn close_all_dropdowns(&mut self) {
        self.ssh_compression_dropdown_open = false;
        self.ssh_strict_host_key_dropdown_open = false;
        self.ssh_max_packets_dropdown_open = false;
        self.ssh_recv_window_dropdown_open = false;
        self.ssh_auth_method_dropdown_open = false;
        self.ssh_proxy_type_dropdown_open = false;
        self.ssh_terminal_type_dropdown_open = false;
        self.ssh_charset_dropdown_open = false;
        self.ssh_icon_color_dropdown_open = false;
        self.ssh_folder_dropdown_open = false;
        self.ssh_keep_alive_max_dropdown_open = false;
        self.ssh_channel_buffer_dropdown_open = false;
        self.ssh_gex_min_dropdown_open = false;
        self.ssh_gex_preferred_dropdown_open = false;
        self.ssh_gex_max_dropdown_open = false;
    }

    fn close_all_dropdowns_except(&mut self, except: &str) {
        if except != "proxy_type" {
            self.ssh_proxy_type_dropdown_open = false;
        }
        if except != "auth_method" {
            self.ssh_auth_method_dropdown_open = false;
        }
        if except != "icon_color" {
            self.ssh_icon_color_dropdown_open = false;
        }
        if except != "terminal_type" {
            self.ssh_terminal_type_dropdown_open = false;
        }
        if except != "charset" {
            self.ssh_charset_dropdown_open = false;
        }
        if except != "compression" {
            self.ssh_compression_dropdown_open = false;
        }
        if except != "strict_host_key" {
            self.ssh_strict_host_key_dropdown_open = false;
        }
        if except != "max_packets" {
            self.ssh_max_packets_dropdown_open = false;
        }
        if except != "recv_window" {
            self.ssh_recv_window_dropdown_open = false;
        }
        if except != "folder" {
            self.ssh_folder_dropdown_open = false;
        }
        if except != "keepalive_max" {
            self.ssh_keep_alive_max_dropdown_open = false;
        }
        if except != "channel_buffer" {
            self.ssh_channel_buffer_dropdown_open = false;
        }
        if except != "gex_min" {
            self.ssh_gex_min_dropdown_open = false;
        }
        if except != "gex_preferred" {
            self.ssh_gex_preferred_dropdown_open = false;
        }
        if except != "gex_max" {
            self.ssh_gex_max_dropdown_open = false;
        }
    }

    fn build_ssh_session(&self, id: String, cx: &App) -> SshSession {
        let name = self.session_name_input.read(cx).value().trim().to_string();
        let host = self.session_host_input.read(cx).value().trim().to_string();
        let username = self
            .session_username_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let port_str = self.session_port_input.read(cx).value().trim().to_string();
        let port = port_str.parse::<u16>().unwrap_or(22);

        let auth_type = match &self.session_auth_type {
            SshAuthType::Password { .. } => {
                let password = self
                    .session_password_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                SshAuthType::Password {
                    password: if password.is_empty() {
                        None
                    } else {
                        Some(password)
                    },
                }
            }
            SshAuthType::PrivateKey { .. } => {
                let key_path = self
                    .session_key_path_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let passphrase = self
                    .session_passphrase_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                SshAuthType::PrivateKey {
                    key_path,
                    passphrase: if passphrase.is_empty() {
                        None
                    } else {
                        Some(passphrase)
                    },
                }
            }
            SshAuthType::KeyboardInteractive => SshAuthType::KeyboardInteractive,
            SshAuthType::SshAgent { socket_path } => SshAuthType::SshAgent {
                socket_path: socket_path.clone(),
            },
        };

        let timeout_str = self
            .session_timeout_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let connection_timeout = timeout_str.parse::<u32>().unwrap_or(30);
        let keepalive_str = self
            .session_keepalive_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let keep_alive_interval = keepalive_str.parse::<u32>().unwrap_or(60);
        let idle_disconnect_str = self
            .session_idle_disconnect_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let idle_disconnect_timeout = idle_disconnect_str.parse::<u32>().unwrap_or(0);

        let proxy_host = if self.session_proxy_type != ProxyType::None {
            let h = self
                .session_proxy_host_input
                .read(cx)
                .value()
                .trim()
                .to_string();
            if h.is_empty() { None } else { Some(h) }
        } else {
            None
        };

        let proxy_port = if self.session_proxy_type != ProxyType::None {
            let p = self
                .session_proxy_port_input
                .read(cx)
                .value()
                .trim()
                .to_string();
            p.parse::<u16>().ok()
        } else {
            None
        };

        let proxy_username = if self.session_proxy_type != ProxyType::None {
            let u = self
                .session_proxy_username_input
                .read(cx)
                .value()
                .trim()
                .to_string();
            if u.is_empty() { None } else { Some(u) }
        } else {
            None
        };

        let proxy_password = if self.session_proxy_type != ProxyType::None {
            let p = self
                .session_proxy_password_input
                .read(cx)
                .value()
                .trim()
                .to_string();
            if p.is_empty() { None } else { Some(p) }
        } else {
            None
        };

        let scrollback_str = self
            .session_scrollback_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let scrollback_lines = if scrollback_str.is_empty() {
            None
        } else {
            scrollback_str.parse::<u32>().ok()
        };

        let tags = {
            let v = self.session_tags_input.read(cx).value().trim().to_string();
            if v.is_empty() { None } else { Some(v) }
        };
        let notes = {
            let v = self.session_notes_input.read(cx).value().trim().to_string();
            if v.is_empty() { None } else { Some(v) }
        };

        SshSession {
            id,
            name,
            parent_folder_id: self.session_parent_folder_id.clone(),
            startup_command: {
                let v = self
                    .session_startup_command_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if v.is_empty() { None } else { Some(v) }
            },
            host,
            port,
            username,
            auth_type,
            icon_color: self.session_icon_color,
            save_credentials: true,
            connection_timeout,
            keep_alive_interval,
            keep_alive_max: self.session_keepalive_max,
            idle_disconnect_timeout,
            keepalive_strategy: self.session_keepalive_strategy,
            tcp_nodelay: self.session_tcp_nodelay,
            channel_buffer_size: self.session_channel_buffer_size,
            proxy_type: self.session_proxy_type,
            proxy_host,
            proxy_port,
            proxy_username,
            proxy_password,
            enable_sftp: self.session_enable_sftp,
            enable_monitor: self.session_enable_monitor,
            monitor_cpu: self.session_monitor_cpu,
            monitor_mem: self.session_monitor_mem,
            terminal: velowork_state::SessionTerminalOptions {
                term_type: self.session_terminal_type.clone(),
                charset: self.session_charset.clone(),
                scrollback_lines,
                ..Default::default()
            },
            compression: self.session_compression,
            strict_host_key: self.session_strict_host_key,
            max_packets: self.session_max_packets,
            recv_window: self.session_recv_window,
            gex_min: self.session_gex_min,
            gex_preferred: self.session_gex_preferred,
            gex_max: self.session_gex_max,
            kex_algorithms: self.session_kex_algorithms.clone(),
            cipher_algorithms: self.session_cipher_algorithms.clone(),
            mac_algorithms: self.session_mac_algorithms.clone(),
            hostkey_algorithms: self.session_hostkey_algorithms.clone(),
            tags,
            notes,
            ..Default::default()
        }
    }

    fn submit_dialog(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let dialog = match &self.active_dialog {
            Some(d) => d.clone(),
            None => return,
        };

        let active_pid = self.active_project_id(cx);

        match dialog {
            SessionPanelDialog::AddFolder { parent_id } => {
                let name = self.folder_name_input.read(cx).value().trim().to_string();
                if !name.is_empty() {
                    let store = cx
                        .global::<velowork_workspace::stores::GlobalSessionStore>()
                        .0
                        .clone();
                    let pid = active_pid.clone();
                    let parent_id_clone = parent_id.clone();
                    let created_id = store.update(cx, |store, cx| {
                        if let Some(ref p_id) = parent_id_clone {
                            store.set_folder_collapsed_for_project(pid.as_deref(), p_id, false, cx);
                        }
                        store.add_folder_for_project(
                            pid.as_deref(),
                            parent_id_clone.as_deref(),
                            name,
                            cx,
                        )
                    });
                    if let Some(new_id) = created_id {
                        self.selected_node_ids.clear();
                        self.selected_node_ids.insert(new_id.clone());
                        self.selection_anchor = Some(new_id);
                    }
                }
            }
            SessionPanelDialog::EditFolder { id, .. } => {
                let name = self.folder_name_input.read(cx).value().trim().to_string();
                if !name.is_empty() {
                    let store = cx
                        .global::<velowork_workspace::stores::GlobalSessionStore>()
                        .0
                        .clone();
                    let pid = active_pid.clone();
                    store.update(cx, |store, cx| {
                        store.rename_node_for_project(pid.as_deref(), &id, &name, cx);
                    });
                }
            }
            SessionPanelDialog::AddSession { .. } => {
                let name = self.session_name_input.read(cx).value().trim().to_string();
                let host = self.session_host_input.read(cx).value().trim().to_string();
                let username = self
                    .session_username_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();

                if name.is_empty() || host.is_empty() || username.is_empty() {
                    let mut missing = Vec::new();
                    if name.is_empty() {
                        missing.push(i18n!(cx, "ssh.general.name"));
                    }
                    if host.is_empty() {
                        missing.push(i18n!(cx, "ssh.connection.host"));
                    }
                    if username.is_empty() {
                        missing.push(i18n!(cx, "ssh.auth.username"));
                    }
                    self.validation_error = Some(format!(
                        "{}: {}",
                        i18n!(cx, "ssh.validation.required"),
                        missing.join(", ")
                    ));
                    cx.notify();
                    return;
                }

                let target_parent_id = self.session_parent_folder_id.clone();
                if self.session_name_conflicts(cx, target_parent_id.as_deref(), &name, None) {
                    self.validation_error =
                        Some(i18n!(cx, "sftp.dialog.name_exists").replace("{name}", &name));
                    cx.notify();
                    return;
                }

                let session = self.build_ssh_session(uuid::Uuid::new_v4().to_string(), cx);
                let store = cx
                    .global::<velowork_workspace::stores::GlobalSessionStore>()
                    .0
                    .clone();
                let pid = active_pid.clone();
                let session_id = session.id.clone();
                store.update(cx, |store, cx| {
                    if let Some(ref p_id) = target_parent_id {
                        store.set_folder_collapsed_for_project(pid.as_deref(), p_id, false, cx);
                    }
                    store.add_session_for_project(
                        pid.as_deref(),
                        target_parent_id.as_deref(),
                        session,
                        cx,
                    );
                });
                self.selected_node_ids.clear();
                self.selected_node_ids.insert(session_id.clone());
                self.selection_anchor = Some(session_id);
            }
            SessionPanelDialog::EditSession { session } => {
                let name = self.session_name_input.read(cx).value().trim().to_string();
                let host = self.session_host_input.read(cx).value().trim().to_string();
                let username = self
                    .session_username_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();

                if name.is_empty() || host.is_empty() || username.is_empty() {
                    let mut missing = Vec::new();
                    if name.is_empty() {
                        missing.push(i18n!(cx, "ssh.general.name"));
                    }
                    if host.is_empty() {
                        missing.push(i18n!(cx, "ssh.connection.host"));
                    }
                    if username.is_empty() {
                        missing.push(i18n!(cx, "ssh.auth.username"));
                    }
                    self.validation_error = Some(format!(
                        "{}: {}",
                        i18n!(cx, "ssh.validation.required"),
                        missing.join(", ")
                    ));
                    cx.notify();
                    return;
                }

                // 编辑时排除节点自身后，校验同目录是否已有同名节点。
                let store = cx
                    .global::<velowork_workspace::stores::GlobalSessionStore>()
                    .0
                    .clone();
                let pid = active_pid.clone();
                let tree = store.read(cx).tree_for_project(pid.as_deref());
                let parent_id =
                    velowork_workspace::stores::SessionStore::find_parent_id(tree, &session.id);
                if self.session_name_conflicts(cx, parent_id.as_deref(), &name, Some(&session.id)) {
                    self.validation_error =
                        Some(i18n!(cx, "sftp.dialog.name_exists").replace("{name}", &name));
                    cx.notify();
                    return;
                }

                let updated = self.build_ssh_session(session.id, cx);
                store.update(cx, |store, cx| {
                    store.update_session_for_project(pid.as_deref(), updated, cx);
                });
            }
        }

        self.active_dialog = None;
        if let Some(window) = window {
            self.focus_session_tree(window, cx);
        }
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::SessionTree);
        });
        cx.notify();
    }

    fn submit_and_connect(&mut self, mut window: Option<&mut Window>, cx: &mut Context<Self>) {
        let dialog = match &self.active_dialog {
            Some(d) => d.clone(),
            None => return,
        };

        let active_pid = self.active_project_id(cx);

        match dialog {
            SessionPanelDialog::AddSession { .. } => {
                let name = self.session_name_input.read(cx).value().trim().to_string();
                let host = self.session_host_input.read(cx).value().trim().to_string();
                let username = self
                    .session_username_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();

                if name.is_empty() || host.is_empty() || username.is_empty() {
                    let mut missing = Vec::new();
                    if name.is_empty() {
                        missing.push(i18n!(cx, "ssh.general.name"));
                    }
                    if host.is_empty() {
                        missing.push(i18n!(cx, "ssh.connection.host"));
                    }
                    if username.is_empty() {
                        missing.push(i18n!(cx, "ssh.auth.username"));
                    }
                    self.validation_error = Some(format!(
                        "{}: {}",
                        i18n!(cx, "ssh.validation.required"),
                        missing.join(", ")
                    ));
                    cx.notify();
                    return;
                }

                let target_parent_id = self.session_parent_folder_id.clone();
                if self.session_name_conflicts(cx, target_parent_id.as_deref(), &name, None) {
                    self.validation_error =
                        Some(i18n!(cx, "sftp.dialog.name_exists").replace("{name}", &name));
                    cx.notify();
                    return;
                }

                let session = self.build_ssh_session(uuid::Uuid::new_v4().to_string(), cx);
                let session_clone = session.clone();
                let store = cx
                    .global::<velowork_workspace::stores::GlobalSessionStore>()
                    .0
                    .clone();
                let pid = active_pid.clone();
                let session_id = session.id.clone();
                store.update(cx, |store, cx| {
                    if let Some(ref p_id) = target_parent_id {
                        store.set_folder_collapsed_for_project(pid.as_deref(), p_id, false, cx);
                    }
                    store.add_session_for_project(
                        pid.as_deref(),
                        target_parent_id.as_deref(),
                        session,
                        cx,
                    );
                });
                self.selected_node_ids.clear();
                self.selected_node_ids.insert(session_id.clone());
                self.selection_anchor = Some(session_id);
                // Connect to the session
                self.connect_ssh(&session_clone, window.as_deref_mut(), cx);
            }
            SessionPanelDialog::EditSession { session } => {
                let name = self.session_name_input.read(cx).value().trim().to_string();
                let host = self.session_host_input.read(cx).value().trim().to_string();
                let username = self
                    .session_username_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();

                if name.is_empty() || host.is_empty() || username.is_empty() {
                    let mut missing = Vec::new();
                    if name.is_empty() {
                        missing.push(i18n!(cx, "ssh.general.name"));
                    }
                    if host.is_empty() {
                        missing.push(i18n!(cx, "ssh.connection.host"));
                    }
                    if username.is_empty() {
                        missing.push(i18n!(cx, "ssh.auth.username"));
                    }
                    self.validation_error = Some(format!(
                        "{}: {}",
                        i18n!(cx, "ssh.validation.required"),
                        missing.join(", ")
                    ));
                    cx.notify();
                    return;
                }

                let store = cx
                    .global::<velowork_workspace::stores::GlobalSessionStore>()
                    .0
                    .clone();
                let pid = active_pid.clone();
                let tree = store.read(cx).tree_for_project(pid.as_deref());
                let parent_id =
                    velowork_workspace::stores::SessionStore::find_parent_id(tree, &session.id);
                if self.session_name_conflicts(cx, parent_id.as_deref(), &name, Some(&session.id)) {
                    self.validation_error =
                        Some(i18n!(cx, "sftp.dialog.name_exists").replace("{name}", &name));
                    cx.notify();
                    return;
                }

                let updated = self.build_ssh_session(session.id, cx);
                let updated_clone = updated.clone();
                store.update(cx, |store, cx| {
                    store.update_session_for_project(pid.as_deref(), updated, cx);
                });
                // Connect to the session
                self.connect_ssh(&updated_clone, window.as_deref_mut(), cx);
            }
            _ => {
                // For folder dialogs, just submit
                self.submit_dialog(window, cx);
                return;
            }
        }

        self.active_dialog = None;
        if let Some(window) = window {
            self.focus_session_tree(window, cx);
        }
        cx.notify();
    }

    fn cancel_dialog(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.active_dialog = None;
        self.close_all_dropdowns();
        if let Some(window) = window {
            let mut restored = false;
            if let Some(prev) = self.dialog_previous_focus.take() {
                window.focus(&prev, cx);
                restored = window.focused(cx).is_some();
            }
            if !restored {
                self.focus_session_tree(window, cx);
            }
        }
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::SessionTree);
        });
        cx.notify();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        match key {
            "tab" => {
                cx.stop_propagation();
                let is_shift = event.keystroke.modifiers.shift;

                if let Some(dialog) = &self.active_dialog {
                    match dialog {
                        SessionPanelDialog::AddFolder { .. }
                        | SessionPanelDialog::EditFolder { .. } => {}
                        SessionPanelDialog::AddSession { .. }
                        | SessionPanelDialog::EditSession { .. } => {
                            let focus_handles = if let Some(model) = self.ssh_dialog() {
                                model.focus_handles(cx)
                            } else {
                                self.current_tab_inputs(cx)
                                    .iter()
                                    .map(|input| input.read(cx).focus_handle(cx))
                                    .collect()
                            };

                            let current_idx =
                                focus_handles.iter().position(|fh| fh.is_focused(window));

                            let next_handle = if let Some(idx) = current_idx {
                                let next_idx = if is_shift {
                                    (idx + focus_handles.len() - 1) % focus_handles.len()
                                } else {
                                    (idx + 1) % focus_handles.len()
                                };
                                Some(&focus_handles[next_idx])
                            } else if !focus_handles.is_empty() {
                                Some(&focus_handles[0])
                            } else {
                                None
                            };

                            if let Some(target) = next_handle {
                                window.focus(target, cx);
                                if let Some(model) = self.ssh_dialog_mut() {
                                    if let Some(sec) = model.section_for_focus_handle(target, cx) {
                                        model.ui.expanded_sections.insert(sec);
                                        model.ui.active_section = sec;
                                        model.ui.pending_scroll.set(Some(sec));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "escape" => {
                cx.stop_propagation();
                self.cancel_dialog(Some(window), cx);
            }
            "enter" => {
                // Submit on Enter for folder dialogs
                if let Some(dialog) = &self.active_dialog {
                    match dialog {
                        SessionPanelDialog::AddFolder { .. }
                        | SessionPanelDialog::EditFolder { .. } => {
                            cx.stop_propagation();
                            self.submit_dialog(Some(window), cx);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn current_tab_inputs(&self, _cx: &App) -> Vec<Entity<SimpleInputState>> {
        match self.ssh_dialog_tab {
            SshDialogTab::General => {
                vec![
                    self.session_name_input.clone(),
                    self.session_host_input.clone(),
                    self.session_port_input.clone(),
                    self.session_tags_input.clone(),
                    self.session_startup_command_input.clone(),
                    self.session_notes_input.clone(),
                ]
            }
            SshDialogTab::Authentication => {
                let mut inputs = vec![self.session_username_input.clone()];
                match &self.session_auth_type {
                    SshAuthType::Password { .. } => {
                        inputs.push(self.session_password_input.clone())
                    }
                    SshAuthType::PrivateKey { .. } => {
                        inputs.push(self.session_key_path_input.clone());
                        inputs.push(self.session_passphrase_input.clone());
                    }
                    _ => {}
                }
                inputs
            }
            SshDialogTab::Connection => {
                let mut inputs = vec![
                    self.session_timeout_input.clone(),
                    self.session_keepalive_input.clone(),
                ];
                if self.session_proxy_type != ProxyType::None {
                    inputs.push(self.session_proxy_host_input.clone());
                    inputs.push(self.session_proxy_port_input.clone());
                    inputs.push(self.session_proxy_username_input.clone());
                    inputs.push(self.session_proxy_password_input.clone());
                }
                inputs
            }
            SshDialogTab::Terminal => {
                vec![self.session_scrollback_input.clone()]
            }
            SshDialogTab::Advanced
            | SshDialogTab::Kex
            | SshDialogTab::Cipher
            | SshDialogTab::Mac
            | SshDialogTab::Hostkey => {
                vec![]
            }
        }
    }

    fn connect_ssh(
        &self,
        session: &SshSession,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        log::debug!(
            "[session_panel:connect_ssh] session_id={} name={} protocol={:?}",
            session.id, session.name, session.protocol
        );
        let shell = match session.protocol {
            velowork_state::SessionProtocol::Serial => {
                let port = session.serial_port.as_deref().unwrap_or("");
                let baud = session.serial_baud_rate.to_string();
                velowork_terminal::shell_config::ShellType::Custom {
                    path: "serial".to_string(),
                    args: vec![
                        "--id".to_string(),
                        session.id.clone(),
                        "--port".to_string(),
                        port.to_string(),
                        "--baud".to_string(),
                        baud,
                    ],
                }
            }
            velowork_state::SessionProtocol::Telnet => {
                let host = session
                    .telnet_host
                    .as_deref()
                    .unwrap_or(session.host.as_str());
                let port = if session.telnet_port > 0 {
                    session.telnet_port
                } else {
                    23
                };
                velowork_terminal::shell_config::ShellType::Custom {
                    path: "telnet".to_string(),
                    args: vec![
                        "--id".to_string(),
                        session.id.clone(),
                        "--host".to_string(),
                        host.to_string(),
                        "--port".to_string(),
                        port.to_string(),
                    ],
                }
            }
            velowork_state::SessionProtocol::Local => {
                let mut args = vec!["--id".to_string(), session.id.clone()];
                if let Some(shell_str) = &session.local_shell {
                    args.push("--shell".to_string());
                    args.push(shell_str.clone());
                }
                velowork_terminal::shell_config::ShellType::Custom {
                    path: "local".to_string(),
                    args,
                }
            }
            velowork_state::SessionProtocol::Ssh => {
                let username = &session.username;
                let host = &session.host;
                let port = session.port;

                let mut args = Vec::new();
                if let SshAuthType::PrivateKey { key_path, .. } = &session.auth_type {
                    if !key_path.is_empty() {
                        args.push("-i".to_string());
                        args.push(key_path.clone());
                    }
                }
                args.push("-p".to_string());
                args.push(port.to_string());
                args.push("--id".to_string());
                args.push(session.id.clone());
                args.push(format!("{}@{}", username, host));

                velowork_terminal::shell_config::ShellType::Custom {
                    path: "ssh".to_string(),
                    args,
                }
            }
        };

        let project_id = self
            .focus_manager
            .read(cx)
            .focused_project_id()
            .cloned()
            .or_else(|| {
                self.workspace
                    .read(cx)
                    .data()
                    .projects
                    .iter()
                    .next()
                    .map(|p| p.id.clone())
            });

        if let Some(project_id) = project_id {
            let focus_manager = self.focus_manager.clone();
            let workspace = self.workspace.clone();
            let session_id = session.id.clone();
            let window_id = self._window_id;

            let mut target_path = None;

            focus_manager.update(cx, |fm, cx| {
                workspace.update(cx, |ws, cx| {
                    let conn_store = cx
                        .global::<velowork_workspace::stores::GlobalConnectionStore>()
                        .0
                        .clone();
                    conn_store.update(cx, |store, cx| {
                        store.mark_connected(&session_id, cx);
                    });
                    let has_layout = ws
                        .project(&project_id)
                        .map(|p| p.layout.is_some())
                        .unwrap_or(false);
                    if has_layout {
                        let path = fm
                            .focused_terminal_state()
                            .filter(|state| state.project_id == project_id)
                            .map(|state| state.layout_path.clone())
                            .unwrap_or_else(Vec::new);
                        // If the currently focused pane is a Welcome placeholder,
                        // replace it in-place rather than adding a new Tab sibling.
                        let focused_is_welcome = ws
                            .get_terminal_shell(&project_id, &path)
                            .map(|st| st == velowork_terminal::shell_config::ShellType::Welcome)
                            .unwrap_or(false);
                        if focused_is_welcome {
                            ws.replace_terminal_shell(&project_id, &path, shell.clone(), cx);
                            ws.set_focused_terminal(fm, project_id.clone(), path.clone(), cx);
                        } else {
                            ws.add_tab_with_shell(fm, &project_id, &path, shell.clone(), cx);
                        }
                    } else {
                        ws.add_terminal_with_shell(fm, &project_id, shell.clone(), cx);
                    }
                    let new_path = fm
                        .focused_terminal_state()
                        .filter(|state| state.project_id == project_id)
                        .map(|state| state.layout_path.clone());
                    if let Some(path) = new_path {
                        target_path = Some(path);
                    }
                });
            });

            cx.emit(SessionPanelEvent::SpawnTerminals {
                project_id: project_id.clone(),
            });

            if let Some(window) = window {
                if let Some(path) = target_path {
                    let project_id = project_id.clone();
                    Self::schedule_focus_pane(window, window_id, project_id, path, 10, cx);
                }
            }
        }
    }

    fn schedule_focus_pane(
        window: &mut Window,
        window_id: WorkspaceWindowId,
        project_id: String,
        path: Vec<usize>,
        retries_left: usize,
        cx: &mut App,
    ) {
        let pane_map = velowork_views_terminal::layout::navigation::get_pane_map(window_id);
        if let Some(pane) = pane_map.find_pane(&project_id, &path) {
            if let Some(ref fh) = pane.focus_handle {
                window.focus(fh, cx);
                return;
            }
        }
        if retries_left > 0 {
            window.on_next_frame(move |window, cx| {
                Self::schedule_focus_pane(
                    window,
                    window_id,
                    project_id,
                    path,
                    retries_left - 1,
                    cx,
                );
            });
        } else {
            log::warn!(
                "schedule_focus_pane exhausted retries for project {} at path {:?}",
                project_id,
                path
            );
        }
    }

    /// Inject the window-level `OverlayRegistry` so the footer menus register
    /// for centralized click-outside dismissal. Called once from
    /// `WindowView::new` after the registry is created.
    pub fn set_overlay_registry(&mut self, reg: Entity<OverlayRegistry>) {
        self.overlay_registry = Some(reg);
    }

    /// Inject the window-level `OverlayManager` for unified dialog/modal tracking.
    pub fn set_overlay_manager(
        &mut self,
        om: Entity<crate::views::overlays::OverlayManager>,
        cx: &mut Context<Self>,
    ) {
        cx.observe(&om, |_, _, cx| cx.notify()).detach();
        self.overlay_manager = Some(om);
    }

    pub fn open_add_session_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_menu.is_some() || self.active_dialog.is_some() {
            return;
        }

        let this_weak = cx.entity().downgrade();
        let this_weak_close = this_weak.clone();
        let registry = self.overlay_registry.clone();
        let bounds = self.add_session_button_bounds;

        let w_ssh = this_weak.clone();
        let w_ser = this_weak.clone();
        let w_tel = this_weak.clone();
        let w_loc = this_weak.clone();
        let origin = Some(bounds.center());

        let menu_items = vec![
            PopupMenuItem::item(
                "new-session-ssh",
                i18n!(cx, "ssh.protocol.ssh"),
                move |_, cx| {
                    if let Some(this) = w_ssh.upgrade() {
                        this.update(cx, |this, cx| {
                            this.open_add_session_with_protocol(
                                velowork_state::SessionProtocol::Ssh,
                                None,
                                origin,
                                cx,
                            );
                        });
                    }
                },
            )
            .icon(session_protocol_icon(velowork_state::SessionProtocol::Ssh)),
            PopupMenuItem::item(
                "new-session-serial",
                i18n!(cx, "ssh.protocol.serial"),
                move |_, cx| {
                    if let Some(this) = w_ser.upgrade() {
                        this.update(cx, |this, cx| {
                            this.open_add_session_with_protocol(
                                velowork_state::SessionProtocol::Serial,
                                None,
                                origin,
                                cx,
                            );
                        });
                    }
                },
            )
            .icon(session_protocol_icon(
                velowork_state::SessionProtocol::Serial,
            )),
            PopupMenuItem::item(
                "new-session-telnet",
                i18n!(cx, "ssh.protocol.telnet"),
                move |_, cx| {
                    if let Some(this) = w_tel.upgrade() {
                        this.update(cx, |this, cx| {
                            this.open_add_session_with_protocol(
                                velowork_state::SessionProtocol::Telnet,
                                None,
                                origin,
                                cx,
                            );
                        });
                    }
                },
            )
            .icon(session_protocol_icon(
                velowork_state::SessionProtocol::Telnet,
            )),
            PopupMenuItem::item(
                "new-session-local",
                i18n!(cx, "ssh.protocol.local"),
                move |_, cx| {
                    if let Some(this) = w_loc.upgrade() {
                        this.update(cx, |this, cx| {
                            this.open_add_session_with_protocol(
                                velowork_state::SessionProtocol::Local,
                                None,
                                origin,
                                cx,
                            );
                        });
                    }
                },
            )
            .icon(session_protocol_icon(
                velowork_state::SessionProtocol::Local,
            )),
        ];

        let menu = cx.new(|cx| {
            velowork_ui::menu::PopupMenu::new(cx, menu_items, bounds.origin, registry, None)
                .trigger_bounds(bounds)
                .direction(velowork_ui::menu::PopupMenuDirection::Below)
                .min_width(px(160.0))
                .auto_close_on_hover_out(true)
        });

        let menu_id = menu.entity_id();
        let this_weak_close_app = this_weak_close.clone();
        let on_close_app: Arc<dyn Fn(&mut App) + Send + Sync> = Arc::new(move |cx| {
            if let Some(this) = this_weak_close_app.upgrade() {
                this.update(cx, |this, cx| {
                    if this.active_menu.as_ref().map(|m| m.entity_id()) == Some(menu_id) {
                        this.active_menu = None;
                        cx.notify();
                    }
                });
            }
        });
        let on_close_app_for_win = on_close_app.clone();
        let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
            on_close_app_for_win(cx);
        });
        menu.update(cx, |m, _| {
            m.set_on_close(Some(on_close));
            m.set_on_close_app(Some(on_close_app));
        });

        let focus_handle = menu.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);

        self.active_menu = Some(menu);
        cx.notify();
    }

    fn open_project_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 0. 收起判定：若项目菜单刚被 window 级 click-outside 因“点击本按钮”
        //    而关闭（MouseDown 阶段），本次应视为收起，不再重新打开。
        if let Some((id, at)) = self.menu_toggle_guard.take() {
            if id == "session-project-menu" && at.elapsed() < std::time::Duration::from_millis(350)
            {
                return;
            }
        }

        // 1. 若当前处于打开状态，直接关闭并退出（实现再次点击收起）
        if let Some(menu) = self.active_menu.take() {
            menu.update(cx, |m, cx| {
                m.set_on_close(None);
                m.close(window, cx);
            });
            self.menu_blur_subscription = None;
            cx.notify();
            return;
        }

        let t = theme(cx);
        let this_weak = cx.entity().downgrade();
        let projects = self.workspace.read(cx).data().projects.clone();
        let mut menu_items = Vec::new();

        let active_project_id = self.active_project_id(cx);
        for project in projects.iter() {
            let project_id = project.id.clone();
            let this_weak_item = this_weak.clone();
            let icon = AppIcon::from_str(&project.icon).unwrap_or(AppIcon::Folder);
            let is_active = active_project_id.as_deref() == Some(&project_id);
            let icon_color = t.get_folder_color(project.folder_color);

            menu_items.push(
                PopupMenuItem::item(
                    format!("project-{}", project_id),
                    project.name.clone(),
                    move |_, cx| {
                        if let Some(this) = this_weak_item.upgrade() {
                            let pid = project_id.clone();
                            this.update(cx, |this, cx| {
                                this.select_project(Some(pid), cx);
                            });
                        }
                    },
                )
                .icon(icon)
                .icon_color(icon_color)
                .checked(is_active),
            );
        }

        menu_items.push(PopupMenuItem::separator());

        let this_weak_manage = this_weak.clone();
        let manage_projects_label = i18n!(cx, "project.manage.label");
        menu_items.push(
            PopupMenuItem::item("manage-projects", manage_projects_label, move |_, cx| {
                if let Some(this) = this_weak_manage.upgrade() {
                    this.update(cx, |this, cx| {
                        this.open_manage_projects_dialog(cx);
                    });
                }
            })
            .icon(AppIcon::Folder),
        );

        // project_selector_bounds 是 content-box（图标位置，不含 trigger 的 SPACE_MD 左右内边距），
        // 这里转换为 trigger 的 border-box，使菜单左右缘与整块 hover 高亮背景对齐。
        let bounds = self.project_selector_bounds;
        let adjusted_bounds = Bounds {
            origin: point(bounds.origin.x - SPACE_MD, bounds.origin.y - SPACE_XS),
            size: Size {
                width: bounds.size.width + SPACE_MD * 2.0,
                height: bounds.size.height + SPACE_XS,
            },
        };
        let _this_weak_close = this_weak.clone();
        let registry = self.overlay_registry.clone();

        let menu = cx.new(|cx| {
            velowork_ui::menu::PopupMenu::new(
                cx,
                menu_items,
                adjusted_bounds.origin,
                registry,
                None,
            )
            .trigger_bounds(adjusted_bounds)
            .direction(velowork_ui::menu::PopupMenuDirection::Above)
            .min_width(adjusted_bounds.size.width)
        });

        let menu_id = menu.entity_id();
        let this_weak_close = this_weak.clone();
        let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
            if let Some(this) = this_weak_close.upgrade() {
                this.update(cx, |this, cx| {
                    if this.active_menu.as_ref().map(|m| m.entity_id()) == Some(menu_id) {
                        this.active_menu = None;
                        this.menu_toggle_guard = Some((
                            SharedString::from("session-project-menu"),
                            std::time::Instant::now(),
                        ));
                        cx.notify();
                    }
                });
            }
        });
        menu.update(cx, |m, _| m.set_on_close(Some(on_close)));

        let focus_handle = menu.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);

        self.active_menu = Some(menu);
        cx.notify();
    }

    fn open_settings_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 0. 收起判定：若设置菜单刚被 window 级 click-outside 因“点击本按钮”
        //    而关闭（MouseDown 阶段），本次应视为收起，不再重新打开。
        if let Some((id, at)) = self.menu_toggle_guard.take() {
            if id == "session-settings-menu" && at.elapsed() < std::time::Duration::from_millis(350)
            {
                return;
            }
        }

        // 1. 若当前处于打开状态，直接关闭并退出（实现再次点击收起）
        if let Some(menu) = self.active_menu.take() {
            menu.update(cx, |m, cx| {
                m.set_on_close(None);
                m.close(window, cx);
            });
            self.menu_blur_subscription = None;
            cx.notify();
            return;
        }

        let this_weak = cx.entity().downgrade();
        let mut menu_items = Vec::new();

        // 当前主题与语言，用于子菜单高亮
        let settings = crate::settings::settings_entity(cx)
            .read(cx)
            .settings
            .clone();
        let current_schema = settings.color_schema;
        let current_locale = settings.locale;

        // —— 主题子菜单（深色 / 浅色 / 系统默认）——
        let theme_entries: Vec<PopupMenuItem> =
            velowork_workspace::settings::ColorSchema::all_variants()
                .iter()
                .map(|schema| {
                    let schema_val = *schema;
                    let is_current = schema_val == current_schema;
                    let label = i18n!(cx, schema_val.translation_key());
                    PopupMenuItem::item(
                        format!("theme-{}", schema_val.translation_key()),
                        label,
                        move |_, cx| {
                            crate::settings::settings_entity(cx)
                                .update(cx, |state, cx| state.set_color_theme(schema_val, cx));
                        },
                    )
                    .checked(is_current)
                })
                .collect();

        let theme_label = i18n!(cx, "menu.appearance");
        menu_items.push(
            PopupMenuItem::submenu("settings-theme", theme_label, theme_entries)
                .icon(AppIcon::Monitor),
        );

        // —— 语言子菜单（English / 中文 …）——
        let lang_entries: Vec<PopupMenuItem> = velowork_workspace::settings::Locale::all_variants()
            .iter()
            .map(|locale| {
                let locale_val = *locale;
                let is_current = locale_val == current_locale;
                PopupMenuItem::item(
                    format!("lang-{}", locale_val.code()),
                    locale_val.display_name().to_string(),
                    move |_, cx| {
                        crate::settings::settings_entity(cx)
                            .update(cx, |state, cx| state.set_locale(locale_val, cx));
                    },
                )
                .checked(is_current)
            })
            .collect();

        let lang_label = i18n!(cx, "menu.language");
        menu_items.push(
            PopupMenuItem::submenu("settings-language", lang_label, lang_entries)
                .icon(AppIcon::Globe),
        );

        menu_items.push(PopupMenuItem::separator());

        let shortcuts_label = i18n!(cx, "menu.shortcuts");
        menu_items.push(
            PopupMenuItem::item("keyboard-shortcuts", shortcuts_label, move |window, cx| {
                window.dispatch_action(Box::new(ShowKeybindings), cx);
            })
            .icon(AppIcon::Keyboard)
            .shortcut("Ctrl+K Ctrl+S"),
        );

        let help_label = i18n!(cx, "menu.help");
        menu_items.push(
            PopupMenuItem::item("open-help", help_label, move |window, cx| {
                window.dispatch_action(Box::new(ShowHelp), cx);
            })
            .icon(AppIcon::Help),
        );

        menu_items.push(PopupMenuItem::separator());

        let update_label = i18n!(cx, "menu.about");
        menu_items.push(
            PopupMenuItem::item("about", update_label, move |window, cx| {
                window.dispatch_action(Box::new(About), cx);
            })
            .icon(AppIcon::Info),
        );

        let settings_label = i18n!(cx, "menu.settings");
        menu_items.push(
            PopupMenuItem::item("open-settings", settings_label, move |window, cx| {
                window.dispatch_action(Box::new(ShowSettings), cx);
            })
            .icon(AppIcon::Settings)
            .shortcut("Ctrl+,"),
        );

        let bounds = self.settings_button_bounds;
        let this_weak_close = this_weak.clone();
        let registry = self.overlay_registry.clone();

        let menu = cx.new(|cx| {
            velowork_ui::menu::PopupMenu::new(cx, menu_items, bounds.origin, registry, None)
                .trigger_bounds(bounds)
                .direction(velowork_ui::menu::PopupMenuDirection::Above)
                .min_width(px(210.0))
        });

        let menu_id = menu.entity_id();
        let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
            if let Some(this) = this_weak_close.upgrade() {
                this.update(cx, |this, cx| {
                    if this.active_menu.as_ref().map(|m| m.entity_id()) == Some(menu_id) {
                        this.active_menu = None;
                        this.menu_toggle_guard = Some((
                            SharedString::from("session-settings-menu"),
                            std::time::Instant::now(),
                        ));
                        cx.notify();
                    }
                });
            }
        });
        menu.update(cx, |m, _| m.set_on_close(Some(on_close)));

        let focus_handle = menu.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);

        self.active_menu = Some(menu);
        cx.notify();
    }

    fn open_tree_context_menu(
        &mut self,
        position: Point<Pixels>,
        node_id: String,
        label: String,
        is_folder: bool,
        is_blank: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ctx) = self.context_menu.take() {
            ctx.update(cx, |m, cx| {
                m.set_on_close(None);
                m.close(window, cx);
            });
        }

        if !is_blank && !self.selected_node_ids.contains(&node_id) {
            self.selected_node_ids.clear();
            self.selected_node_ids.insert(node_id.clone());
            self.selection_anchor = Some(node_id.clone());
            window.focus(&self.focus_handle, cx);
        }

        let mut items = Vec::new();
        let this_weak = cx.entity().downgrade();
        let t = theme(cx);
        let origin = Some(position);

        if is_blank {
            let has_session_sel = self.selection_has_session(cx);
            let w1_ssh = this_weak.clone();
            let w1_ser = this_weak.clone();
            let w1_tel = this_weak.clone();
            let w1_loc = this_weak.clone();
            items.push(
                PopupMenuItem::submenu(
                    "tree-ctx-new-conn",
                    i18n!(cx, "dock.add_connection"),
                    vec![
                        PopupMenuItem::item(
                            "new-conn-ssh",
                            i18n!(cx, "ssh.protocol.ssh"),
                            move |_, cx| {
                                if let Some(this) = w1_ssh.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.open_add_session_with_protocol(
                                            velowork_state::SessionProtocol::Ssh,
                                            None,
                                            origin,
                                            cx,
                                        );
                                    });
                                }
                            },
                        )
                        .icon(session_protocol_icon(velowork_state::SessionProtocol::Ssh)),
                        PopupMenuItem::item(
                            "new-conn-serial",
                            i18n!(cx, "ssh.protocol.serial"),
                            move |_, cx| {
                                if let Some(this) = w1_ser.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.open_add_session_with_protocol(
                                            velowork_state::SessionProtocol::Serial,
                                            None,
                                            origin,
                                            cx,
                                        );
                                    });
                                }
                            },
                        )
                        .icon(session_protocol_icon(
                            velowork_state::SessionProtocol::Serial,
                        )),
                        PopupMenuItem::item(
                            "new-conn-telnet",
                            i18n!(cx, "ssh.protocol.telnet"),
                            move |_, cx| {
                                if let Some(this) = w1_tel.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.open_add_session_with_protocol(
                                            velowork_state::SessionProtocol::Telnet,
                                            None,
                                            origin,
                                            cx,
                                        );
                                    });
                                }
                            },
                        )
                        .icon(session_protocol_icon(
                            velowork_state::SessionProtocol::Telnet,
                        )),
                        PopupMenuItem::item(
                            "new-conn-local",
                            i18n!(cx, "ssh.protocol.local"),
                            move |_, cx| {
                                if let Some(this) = w1_loc.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.open_add_session_with_protocol(
                                            velowork_state::SessionProtocol::Local,
                                            None,
                                            origin,
                                            cx,
                                        );
                                    });
                                }
                            },
                        )
                        .icon(session_protocol_icon(
                            velowork_state::SessionProtocol::Local,
                        )),
                    ],
                )
                .icon(AppIcon::Plus),
            );

            let w2 = this_weak.clone();
            items.push(
                PopupMenuItem::item(
                    "tree-ctx-new-folder",
                    i18n!(cx, "common.new_folder"),
                    move |window, cx| {
                        if let Some(this) = w2.upgrade() {
                            this.update(cx, |this, cx| this.inline_create_folder(None, window, cx));
                        }
                    },
                )
                .icon(AppIcon::NewFolder),
            );

            let w_imp = this_weak.clone();
            items.push(
                PopupMenuItem::item(
                    "tree-ctx-import-sessions",
                    i18n!(cx, "session.import.menu_item"),
                    move |_, cx| {
                        if let Some(this) = w_imp.upgrade() {
                            this.update(cx, |this, cx| this.open_import_sessions_dialog(cx));
                        }
                    },
                )
                .icon(AppIcon::FolderInput),
            );

            if has_session_sel {
                items.push(PopupMenuItem::separator());
                let w3 = this_weak.clone();
                items.push(
                    PopupMenuItem::item(
                        "tree-ctx-connect",
                        i18n!(cx, "common.action.connect"),
                        move |window, cx| {
                            if let Some(this) = w3.upgrade() {
                                this.update(cx, |this, cx| {
                                    this.open_selected_nodes(Some(window), cx)
                                });
                            }
                        },
                    )
                    .icon(AppIcon::Link),
                );
            }
        } else {
            if !is_folder {
                let sid = node_id.clone();
                let w_conn = this_weak.clone();
                items.push(
                    PopupMenuItem::item(
                        "tree-ctx-connect",
                        i18n!(cx, "common.action.connect"),
                        move |window, cx| {
                            if let Some(this) = w_conn.upgrade() {
                                let sid = sid.clone();
                                this.update(cx, |this, cx| {
                                    if !this.selected_node_ids.is_empty()
                                        && this.selected_node_ids.contains(&sid)
                                    {
                                        this.open_selected_nodes(Some(window), cx);
                                    } else {
                                        let session = {
                                            let store = cx
                                                .global::<velowork_workspace::stores::GlobalSessionStore>()
                                                .0
                                                .read(cx);
                                            Self::find_session_by_id(
                                                this.active_tree(store, cx),
                                                &sid,
                                            )
                                            .cloned()
                                        };
                                        if let Some(session) = session {
                                            this.connect_ssh(&session, Some(window), cx);
                                        }
                                    }
                                });
                            }
                        },
                    )
                    .icon(AppIcon::Link),
                );
                items.push(PopupMenuItem::separator());
            }

            if is_folder {
                let fid_ssh = node_id.clone();
                let fid_ser = node_id.clone();
                let fid_tel = node_id.clone();
                let fid_loc = node_id.clone();
                let fid2 = node_id.clone();
                let w_f_ssh = this_weak.clone();
                let w_f_ser = this_weak.clone();
                let w_f_tel = this_weak.clone();
                let w_f_loc = this_weak.clone();
                let w_f2 = this_weak.clone();
                items.push(
                    PopupMenuItem::submenu(
                        "tree-ctx-new-conn",
                        i18n!(cx, "dock.add_connection"),
                        vec![
                            PopupMenuItem::item(
                                "new-folder-session-ssh",
                                i18n!(cx, "ssh.protocol.ssh"),
                                move |_, cx| {
                                    if let Some(this) = w_f_ssh.upgrade() {
                                        let fid = fid_ssh.clone();
                                        this.update(cx, |this, cx| {
                                            this.open_add_session_with_protocol(
                                                velowork_state::SessionProtocol::Ssh,
                                                Some(fid),
                                                origin,
                                                cx,
                                            );
                                        });
                                    }
                                },
                            )
                            .icon(session_protocol_icon(velowork_state::SessionProtocol::Ssh)),
                            PopupMenuItem::item(
                                "new-folder-session-serial",
                                i18n!(cx, "ssh.protocol.serial"),
                                move |_, cx| {
                                    if let Some(this) = w_f_ser.upgrade() {
                                        let fid = fid_ser.clone();
                                        this.update(cx, |this, cx| {
                                            this.open_add_session_with_protocol(
                                                velowork_state::SessionProtocol::Serial,
                                                Some(fid),
                                                origin,
                                                cx,
                                            );
                                        });
                                    }
                                },
                            )
                            .icon(session_protocol_icon(
                                velowork_state::SessionProtocol::Serial,
                            )),
                            PopupMenuItem::item(
                                "new-folder-session-telnet",
                                i18n!(cx, "ssh.protocol.telnet"),
                                move |_, cx| {
                                    if let Some(this) = w_f_tel.upgrade() {
                                        let fid = fid_tel.clone();
                                        this.update(cx, |this, cx| {
                                            this.open_add_session_with_protocol(
                                                velowork_state::SessionProtocol::Telnet,
                                                Some(fid),
                                                origin,
                                                cx,
                                            );
                                        });
                                    }
                                },
                            )
                            .icon(session_protocol_icon(
                                velowork_state::SessionProtocol::Telnet,
                            )),
                            PopupMenuItem::item(
                                "new-folder-session-local",
                                i18n!(cx, "ssh.protocol.local"),
                                move |_, cx| {
                                    if let Some(this) = w_f_loc.upgrade() {
                                        let fid = fid_loc.clone();
                                        this.update(cx, |this, cx| {
                                            this.open_add_session_with_protocol(
                                                velowork_state::SessionProtocol::Local,
                                                Some(fid),
                                                origin,
                                                cx,
                                            );
                                        });
                                    }
                                },
                            )
                            .icon(session_protocol_icon(
                                velowork_state::SessionProtocol::Local,
                            )),
                        ],
                    )
                    .icon(AppIcon::Plus),
                );
                items.push(
                    PopupMenuItem::item(
                        "tree-ctx-new-folder",
                        i18n!(cx, "common.new_folder"),
                        move |window, cx| {
                            if let Some(this) = w_f2.upgrade() {
                                let fid = fid2.clone();
                                this.update(cx, |this, cx| {
                                    this.inline_create_folder(Some(fid), window, cx)
                                });
                            }
                        },
                    )
                    .icon(AppIcon::NewFolder),
                );
                items.push(PopupMenuItem::separator());
            } else {
                let sid = node_id.clone();
                let sname = label.clone();
                let sid_dup = node_id.clone();
                let w_ren = this_weak.clone();
                let w_dup = this_weak.clone();
                items.push(
                    PopupMenuItem::item(
                        "tree-ctx-rename-session",
                        i18n!(cx, "common.action.rename"),
                        move |window, cx| {
                            if let Some(this) = w_ren.upgrade() {
                                let sid = sid.clone();
                                let sname = sname.clone();
                                this.update(cx, |this, cx| {
                                    this.inline_rename_session(sid, sname, window, cx)
                                });
                            }
                        },
                    )
                    .icon(AppIcon::Edit),
                );
                items.push(
                    PopupMenuItem::item(
                        "tree-ctx-duplicate",
                        i18n!(cx, "common.action.duplicate"),
                        move |window, cx| {
                            if let Some(this) = w_dup.upgrade() {
                                let sid = sid_dup.clone();
                                this.update(cx, |this, cx| {
                                    let store = cx
                                        .global::<velowork_workspace::stores::GlobalSessionStore>()
                                        .0
                                        .clone();
                                    let pid = this.active_project_id(cx);
                                    let new_id = store.update(cx, |store, cx| {
                                        store.duplicate_session_for_project(
                                            pid.as_deref(),
                                            &sid,
                                            cx,
                                        )
                                    });
                                    if let Some(new_id) = new_id {
                                        this.selected_node_ids.clear();
                                        this.selected_node_ids.insert(new_id.clone());
                                        this.selection_anchor = Some(new_id.clone());
                                        this.scroll_node_into_view(&new_id, cx);
                                    }
                                    this.focus_session_tree(window, cx);
                                    this.focus_manager.update(cx, |fm, _| {
                                        fm.request_focus(
                                            velowork_workspace::focus::FocusLayer::SessionTree,
                                        );
                                    });
                                });
                            }
                        },
                    )
                    .icon(AppIcon::Copy),
                );
                items.push(PopupMenuItem::separator());
            }

            let nid = node_id.clone();
            let nlabel = label.clone();
            let w_edit = this_weak.clone();
            items.push(
                PopupMenuItem::item(
                    "tree-ctx-edit",
                    i18n!(
                        cx,
                        if is_folder {
                            "common.action.rename"
                        } else {
                            "common.action.edit"
                        }
                    ),
                    move |window, cx| {
                        if let Some(this) = w_edit.upgrade() {
                            let nid = nid.clone();
                            let nlabel = nlabel.clone();
                            this.update(cx, |this, cx| {
                                if is_folder {
                                    this.inline_rename_folder(nid, nlabel, window, cx);
                                } else {
                                    let store = cx
                                        .global::<velowork_workspace::stores::GlobalSessionStore>()
                                        .0
                                        .read(cx);
                                    if let Some(session) =
                                        Self::find_session_by_id(this.active_tree(store, cx), &nid)
                                    {
                                        this.open_edit_session_dialog_at(session.clone(), origin, cx);
                                    }
                                }
                            });
                        }
                    },
                )
                .icon(AppIcon::Edit),
            );

            let del_id = node_id.clone();
            let del_label = label.clone();
            let w_del = this_weak.clone();
            items.push(
                PopupMenuItem::item(
                    "tree-ctx-delete",
                    i18n!(cx, "common.action.delete"),
                    move |window, cx| {
                        if let Some(this) = w_del.upgrade() {
                            let del_id = del_id.clone();
                            let del_label = del_label.clone();
                            this.update(cx, |this, cx| {
                                this.request_delete_confirm(del_id, del_label, window, cx)
                            });
                        }
                    },
                )
                .icon(AppIcon::Trash)
                .text_color(t.error),
            );
        }

        let origin = self.focus_handle.clone();
        let menu = ContextMenu::open_with_origin(
            position,
            items,
            self.overlay_registry.clone(),
            None,
            Some(origin),
            window,
            cx,
        );

        let menu_id = menu.entity_id();
        let this_weak_close = this_weak.clone();
        let on_close = Arc::new(move |window: &mut Window, cx: &mut App| {
            if let Some(this) = this_weak_close.upgrade() {
                this.update(cx, |this, cx| {
                    if this.context_menu.as_ref().map(|m| m.entity_id()) == Some(menu_id) {
                        this.context_menu = None;
                        if window.focused(cx).is_none() {
                            this.focus_session_tree(window, cx);
                        }
                        cx.notify();
                    }
                });
            }
        });
        menu.update(cx, |m, _| m.set_on_close(Some(on_close)));

        self.context_menu = Some(menu);
        cx.notify();
    }

    // ─── Multi-selection helpers ─────────────────────────────────────────────

    /// Collect the ids of all currently visible nodes in display order.
    /// Mirrors `render_tree_node` traversal: a folder id is emitted, then its
    /// children are emitted only when the folder is expanded.
    fn collect_visible_ids(nodes: &[SessionTreeNode], out: &mut Vec<String>) {
        let mut ordered: Vec<&SessionTreeNode> = nodes.iter().collect();
        ordered.sort_by(|a, b| {
            let a_folder = matches!(a, SessionTreeNode::Folder { .. });
            let b_folder = matches!(b, SessionTreeNode::Folder { .. });
            b_folder.cmp(&a_folder)
        });
        for node in ordered {
            match node {
                SessionTreeNode::Folder {
                    id,
                    children,
                    is_collapsed,
                    ..
                } => {
                    out.push(id.clone());
                    if !is_collapsed {
                        Self::collect_visible_ids(children, out);
                    }
                }
                SessionTreeNode::Session { session } => {
                    out.push(session.id.clone());
                }
            }
        }
    }

    /// Recursively collect every session under the given node list.
    fn collect_all_sessions(nodes: &[SessionTreeNode], out: &mut Vec<SshSession>) {
        for node in nodes {
            match node {
                SessionTreeNode::Session { session } => out.push(session.clone()),
                SessionTreeNode::Folder { children, .. } => {
                    Self::collect_all_sessions(children, out)
                }
            }
        }
    }

    /// Collect sessions represented by a node id: a session id yields that session;
    /// a folder id yields all sessions nested within it (recursively).
    fn collect_sessions_for_id(nodes: &[SessionTreeNode], id: &str, out: &mut Vec<SshSession>) {
        for node in nodes {
            match node {
                SessionTreeNode::Session { session } => {
                    if session.id == id {
                        out.push(session.clone());
                    }
                }
                SessionTreeNode::Folder {
                    id: fid, children, ..
                } => {
                    if fid == id {
                        Self::collect_all_sessions(children, out);
                    } else {
                        Self::collect_sessions_for_id(children, id, out);
                    }
                }
            }
        }
    }

    /// Clear the current multi-selection, if any.
    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selected_node_ids.is_empty() || self.selection_anchor.is_some() {
            self.selected_node_ids.clear();
            self.selection_anchor = None;
            self.focused_index = None;
            cx.notify();
        }
    }

    /// Whether the current multi-selection contains at least one session node
    /// (as opposed to only folders). Used to decide whether to surface a batch
    /// "连接" action in the blank-area context menu.
    fn selection_has_session(&self, cx: &App) -> bool {
        if self.selected_node_ids.is_empty() {
            return false;
        }
        let store = cx
            .global::<velowork_workspace::stores::GlobalSessionStore>()
            .0
            .read(cx);
        let tree = self.active_tree(store, cx);
        self.selected_node_ids
            .iter()
            .any(|id| Self::find_session_by_id(tree, id).is_some())
    }

    /// Central click handler for a tree node (folder or session).
    ///
    /// Behavior:
    /// - Shift+click: select the contiguous range between the anchor and this node
    ///   (in visible order), keeping the anchor.
    /// - Ctrl/Cmd+click: toggle this node in the selection and make it the anchor.
    /// - Plain click on a folder: select only this folder and toggle its collapse.
    /// - Plain double-click on a session: open the SSH connection.
    /// - Plain single-click on a session: select only this session.
    fn handle_node_click(
        &mut self,
        node_id: &str,
        is_folder: bool,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mods = event.modifiers();
        let ctrl = mods.control || mods.platform;
        let shift = mods.shift;

        // Ensure the panel is focused so keyboard shortcuts (Enter/Escape) apply.
        window.focus(&self.focus_handle, cx);
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::SessionTree);
        });

        // 点击节点后，键盘导航焦点应跟随到被点击的行。
        let clicked_index = self.visible_order.iter().position(|x| x == node_id);

        if shift {
            let anchor = self.selection_anchor.clone();
            let range: Option<Vec<String>> = anchor.and_then(|anchor| {
                let ai = self.visible_order.iter().position(|id| id == &anchor)?;
                let ti = self.visible_order.iter().position(|id| id == node_id)?;
                let (lo, hi) = if ai <= ti { (ai, ti) } else { (ti, ai) };
                Some(self.visible_order[lo..=hi].to_vec())
            });
            match range {
                Some(ids) => {
                    self.selected_node_ids = ids.into_iter().collect();
                    // Anchor stays put so the range can be re-adjusted.
                }
                None => {
                    self.selected_node_ids.clear();
                    self.selected_node_ids.insert(node_id.to_string());
                    self.selection_anchor = Some(node_id.to_string());
                }
            }
            self.focused_index = clicked_index;
            cx.notify();
            return;
        }

        if ctrl {
            if self.selected_node_ids.contains(node_id) {
                self.selected_node_ids.remove(node_id);
            } else {
                self.selected_node_ids.insert(node_id.to_string());
            }
            self.selection_anchor = Some(node_id.to_string());
            self.focused_index = clicked_index;
            cx.notify();
            return;
        }

        // Plain click (no modifiers).
        if is_folder {
            self.selected_node_ids.clear();
            self.selected_node_ids.insert(node_id.to_string());
            self.selection_anchor = Some(node_id.to_string());
            self.focused_index = clicked_index;
            // toggle_folder calls cx.notify() internally.
            self.toggle_folder(node_id, cx);
            return;
        }

        // Session, no modifiers.
        if event.click_count() >= 2 {
            let session = {
                let store = cx
                    .global::<velowork_workspace::stores::GlobalSessionStore>()
                    .0
                    .read(cx);
                Self::find_session_by_id(self.active_tree(store, cx), node_id).cloned()
            };
            if let Some(s) = session {
                self.connect_ssh(&s, Some(window), cx);
            }
        } else {
            self.selected_node_ids.clear();
            self.selected_node_ids.insert(node_id.to_string());
            self.selection_anchor = Some(node_id.to_string());
            self.focused_index = clicked_index;
            cx.notify();
        }
    }

    /// Open all sessions represented by the current multi-selection.
    /// Folders expand to all nested sessions; duplicates are opened once.
    /// 当前“键盘焦点”节点：优先返回 Shift 选区锚点，否则返回任一选中项。
    /// 方向键导航与空格激活都以此为操作对象。
    fn focused_node_id(&self) -> Option<String> {
        self.selection_anchor
            .clone()
            .or_else(|| self.selected_node_ids.iter().next().cloned())
    }

    /// 当前键盘焦点的索引（基于 `visible_order`）。
    /// 优先复用显式维护的 `focused_index`；仅当该索引因树结构变化而错位
    /// （索引越界或其指向的 id 已不是当前焦点节点）时，才回退到按 id 查找。
    /// 这样即使在 `visible_order` 存在重复 id 的情况下，连续方向键导航也不会跳错节点。
    fn current_focus_index(&self) -> Option<usize> {
        if let Some(i) = self.focused_index {
            if i < self.visible_order.len() {
                if let Some(fid) = self.focused_node_id() {
                    if self.visible_order[i] == fid {
                        return Some(i);
                    }
                }
            }
        }
        self.focused_node_id()
            .and_then(|id| self.visible_order.iter().position(|x| x == &id))
    }

    /// Check whether the session tree or any of its children is currently focused
    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.focus_handle.contains_focused(window, cx)
    }

    /// Return reference to session tree focus handle
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Check whether selection highlighting in the session tree should be active.
    /// Active when the panel (or its children) contains focus, or when a context menu,
    /// dialog, or panel-originating modal is currently open.
    pub fn is_selection_active(&self, window: &Window, cx: &App) -> bool {
        self.focus_handle.contains_focused(window, cx)
            || self.context_menu.is_some()
            || self.active_dialog.is_some()
            || self.active_menu.is_some()
            || self
                .overlay_manager
                .as_ref()
                .is_some_and(|om| om.read(cx).active_modal_belongs_to(&self.focus_handle))
    }

    /// 激活会话树焦点并确保有高亮选中的节点，将其滚动到可视区。
    pub fn focus_session_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        if self.selected_node_ids.is_empty() {
            if let Some(first_id) = self.visible_order.first().cloned() {
                self.selected_node_ids.insert(first_id.clone());
                self.selection_anchor = Some(first_id.clone());
                self.focused_index = Some(0);
                self.scroll_node_into_view(&first_id, cx);
            }
        } else if let Some(id) = self.selected_node_ids.iter().next().cloned() {
            self.scroll_node_into_view(&id, cx);
        }
        cx.notify();
    }

    /// 方向键移动焦点（同时把单选选中移动到该节点）。
    /// `delta > 0` 向下，`delta < 0` 向上。要求已有选中节点，否则无操作。
    /// 基于显式索引导航，免疫 `visible_order` 中的重复 id。
    fn move_focus(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.selected_node_ids.is_empty() || self.visible_order.is_empty() {
            return;
        }
        let idx = match self.current_focus_index() {
            Some(i) => i,
            // 焦点索引无法确定（如选中项不在可见列表）时，从第一个可见项开始。
            None => 0,
        };
        let new_idx = if delta > 0 {
            (idx + delta as usize).min(self.visible_order.len() - 1)
        } else {
            idx.saturating_sub((-delta) as usize)
        };
        let new_id = self.visible_order[new_idx].clone();
        self.selected_node_ids.clear();
        self.selected_node_ids.insert(new_id.clone());
        self.selection_anchor = Some(new_id.clone());
        self.focused_index = Some(new_idx);
        // 把刚移动到焦点的行滚动进可视区（仅在不完全可见时滚动，做最小位移）。
        self.scroll_node_into_view(&new_id, cx);
        cx.notify();
    }

    /// 将指定节点行滚动进会话树可视区。
    /// 行高由设计系统 Compact 档统一解析，滚动容器内边距上侧 6px，因此节点 i
    /// 的内容纵坐标 top = 6 + i*row_h、bottom = 6 + (i+1)*row_h。仅在行未完全
    /// 可见时才滚动，并保持最小位移（尽量不跳动）。
    fn scroll_node_into_view(&self, id: &str, cx: &App) {
        let Some(idx) = self.visible_order.iter().position(|x| x == id) else {
            return;
        };
        let row_h: Pixels = session_row_height(cx);
        let pad_top: Pixels = SPACE_SM;
        let top = pad_top + row_h * idx as f32;
        let bottom = top + row_h;

        let viewport_h = self.tree_scroll_handle.bounds().size.height;
        if viewport_h == px(0.0) {
            return; // 尚未完成布局
        }
        let cur = self.tree_scroll_handle.offset().y;
        // 视口坐标下的行上下边界
        let row_top_vp = top + cur;
        let row_bottom_vp = bottom + cur;

        let mut new_off = if row_top_vp < px(0.0) {
            // 行在视口上方 → 滚动到顶部对齐
            -top
        } else if row_bottom_vp > viewport_h {
            // 行在视口下方 → 滚动使其底部对齐视口底部
            viewport_h - bottom
        } else {
            return; // 已完全可见，无需滚动
        };
        // 限制在合法滚动范围内
        let max_off = self.tree_scroll_handle.max_offset().y;
        new_off = new_off.max(-max_off).min(px(0.0));
        self.tree_scroll_handle
            .set_offset(Point::new(px(0.0), new_off));
    }

    /// 空格：对当前键盘焦点节点执行“鼠标左键单击”的等价操作——
    /// 文件夹展开/收起，会话单选。要求已有选中节点，否则无操作。
    fn activate_focused(&mut self, cx: &mut Context<Self>) {
        if self.selected_node_ids.is_empty() {
            return;
        }
        let id = match self.focused_node_id() {
            Some(id) => id,
            None => return,
        };
        let store = cx
            .global::<velowork_workspace::stores::GlobalSessionStore>()
            .0
            .read(cx);
        let tree = self.active_tree(store, cx);
        self.selected_node_ids.clear();
        self.selected_node_ids.insert(id.clone());
        self.selection_anchor = Some(id.clone());
        self.focused_index = self.current_focus_index();
        if Self::is_folder_node(tree, &id) {
            // 等价于鼠标单击文件夹：收起/展开。
            self.toggle_folder(&id, cx);
        } else {
            cx.notify();
        }
    }

    fn open_selected_nodes(&mut self, mut window: Option<&mut Window>, cx: &mut Context<Self>) {
        // Preserve visible order for a deterministic open sequence.
        let ordered_ids: Vec<String> = if self.visible_order.is_empty() {
            self.selected_node_ids.iter().cloned().collect()
        } else {
            self.visible_order
                .iter()
                .filter(|id| self.selected_node_ids.contains(*id))
                .cloned()
                .collect()
        };

        let sessions = {
            let store = cx
                .global::<velowork_workspace::stores::GlobalSessionStore>()
                .0
                .read(cx);
            let tree = self.active_tree(store, cx);
            let mut collected: Vec<SshSession> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for id in &ordered_ids {
                let mut tmp = Vec::new();
                Self::collect_sessions_for_id(tree, id, &mut tmp);
                for s in tmp {
                    if seen.insert(s.id.clone()) {
                        collected.push(s);
                    }
                }
            }
            collected
        };

        for s in &sessions {
            self.connect_ssh(s, window.as_deref_mut(), cx);
        }
    }

    fn render_tree_context_menu(&self, _cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        Some(menu.clone().into_any_element())
    }

    pub(crate) fn test_connection(&mut self, cx: &mut Context<Self>) {
        let (host, port, username, timeout, auth_type, session_config) =
            if let Some(m) = self.ssh_dialog_mut() {
                m.sync_config_from_inputs(cx);
                let host = m.inputs.host.read(cx).value().trim().to_string();
                let port = m
                    .inputs
                    .port
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u16>()
                    .unwrap_or(22);
                let username = m.inputs.username.read(cx).value().trim().to_string();
                let timeout = m
                    .inputs
                    .connection_timeout
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u32>()
                    .unwrap_or(30);

                let auth_type = match &m.config.auth_type {
                    SshAuthType::Password { .. } => {
                        let pwd = m.inputs.password.read(cx).value().trim().to_string();
                        SshAuthType::Password {
                            password: if pwd.is_empty() { None } else { Some(pwd) },
                        }
                    }
                    SshAuthType::PrivateKey { .. } => {
                        let kp = m.inputs.key_path.read(cx).value().trim().to_string();
                        let pp = m.inputs.passphrase.read(cx).value().trim().to_string();
                        SshAuthType::PrivateKey {
                            key_path: kp,
                            passphrase: if pp.is_empty() { None } else { Some(pp) },
                        }
                    }
                    SshAuthType::KeyboardInteractive => SshAuthType::KeyboardInteractive,
                    SshAuthType::SshAgent { .. } => SshAuthType::SshAgent {
                        socket_path: {
                            let v = m
                                .inputs
                                .agent_socket_path
                                .read(cx)
                                .value()
                                .trim()
                                .to_string();
                            if v.is_empty() { None } else { Some(v) }
                        },
                    },
                };
                (
                    host,
                    port,
                    username,
                    timeout,
                    auth_type,
                    Some(m.config.clone()),
                )
            } else {
                let host = self.session_host_input.read(cx).value().trim().to_string();
                let port = self
                    .session_port_input
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u16>()
                    .unwrap_or(22);
                let username = self
                    .session_username_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let timeout = self
                    .session_timeout_input
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u32>()
                    .unwrap_or(30);

                let auth_type = match &self.session_auth_type {
                    SshAuthType::Password { .. } => {
                        let password = self
                            .session_password_input
                            .read(cx)
                            .value()
                            .trim()
                            .to_string();
                        SshAuthType::Password {
                            password: if password.is_empty() {
                                None
                            } else {
                                Some(password)
                            },
                        }
                    }
                    SshAuthType::PrivateKey { .. } => {
                        let key_path = self
                            .session_key_path_input
                            .read(cx)
                            .value()
                            .trim()
                            .to_string();
                        let passphrase = self
                            .session_passphrase_input
                            .read(cx)
                            .value()
                            .trim()
                            .to_string();
                        SshAuthType::PrivateKey {
                            key_path,
                            passphrase: if passphrase.is_empty() {
                                None
                            } else {
                                Some(passphrase)
                            },
                        }
                    }
                    SshAuthType::KeyboardInteractive => SshAuthType::KeyboardInteractive,
                    SshAuthType::SshAgent { socket_path } => SshAuthType::SshAgent {
                        socket_path: socket_path.clone(),
                    },
                };
                (host, port, username, timeout, auth_type, None)
            };

        if host.is_empty() || username.is_empty() {
            return;
        }

        self.test_connection_status = TestConnectionStatus::Testing;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = smol::unblock(move || {
                velowork_terminal::ssh_test::test_ssh_connection_blocking(
                    &host,
                    port,
                    &username,
                    &auth_type,
                    timeout,
                    session_config.as_ref(),
                )
            })
            .await;
            this.update(cx, |this, cx| {
                if result.success {
                    this.test_connection_status = TestConnectionStatus::Success {
                        latency_ms: result.latency_ms,
                    };
                } else {
                    this.test_connection_status = TestConnectionStatus::Failed {
                        error: result.error.unwrap_or_else(|| "Unknown error".to_string()),
                    };
                }
                cx.notify();
            })
        })
        .detach();
    }

    fn find_node_by_id<'a>(
        &self,
        nodes: &'a [SessionTreeNode],
        id: &str,
    ) -> Option<&'a SessionTreeNode> {
        for node in nodes {
            if node.id() == id {
                return Some(node);
            }
            if let SessionTreeNode::Folder { children, .. } = node {
                if let Some(found) = self.find_node_by_id(children, id) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn find_session_by_id<'a>(nodes: &'a [SessionTreeNode], id: &str) -> Option<&'a SshSession> {
        for node in nodes {
            match node {
                SessionTreeNode::Session { session } if session.id == id => return Some(session),
                SessionTreeNode::Folder { children, .. } => {
                    if let Some(found) = Self::find_session_by_id(children, id) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// F2 快捷键：重命名当前选中的节点。
    /// 多选时仅对首个选中节点生效。仅文件夹走行内重命名；
    /// 会话（叶子节点）不允许重命名，其名称只能在「编辑」对话框中修改，
    /// 故 F2 对会话节点不触发重命名。
    fn rename_selected_node(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = match self.selected_node_ids.iter().next() {
            Some(id) => id.clone(),
            None => return,
        };
        // 在不可变借用内取出节点类型，随后释放借用再调用可变方法。
        let is_folder = {
            let store = cx
                .global::<velowork_workspace::stores::GlobalSessionStore>()
                .0
                .read(cx);
            let tree = self.active_tree(store, cx);
            match self.find_node_by_id(tree, &id) {
                Some(node) => matches!(node, SessionTreeNode::Folder { .. }),
                None => return,
            }
        };
        let label = {
            let store = cx
                .global::<velowork_workspace::stores::GlobalSessionStore>()
                .0
                .read(cx);
            let tree = self.active_tree(store, cx);
            self.find_node_by_id(tree, &id)
                .map(|n| n.name().to_string())
        };
        if is_folder {
            if let Some(label) = label {
                self.inline_rename_folder(id, label, window, cx);
            }
        } else {
            if let Some(label) = label {
                self.inline_rename_session(id, label, window, cx);
            }
        }
    }

    /// Delete 快捷键：删除当前选中的所有节点（逐个弹窗确认，确认后执行）。
    fn delete_selected_nodes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let targets: Vec<(String, String)> = {
            let store = cx
                .global::<velowork_workspace::stores::GlobalSessionStore>()
                .0
                .read(cx);
            let tree = self.active_tree(store, cx);
            self.selected_node_ids
                .iter()
                .filter_map(|id| {
                    self.find_node_by_id(tree, id)
                        .map(|n| (id.clone(), n.name().to_string()))
                })
                .collect()
        };
        for (id, label) in targets {
            self.request_delete_confirm(id, label, window, cx);
        }
    }

    /// Find the index of a node within its parent's children list.
    /// Returns None if the node is at root level or not found.
    fn find_node_index_in_parent(nodes: &[SessionTreeNode], target_id: &str) -> Option<usize> {
        for node in nodes {
            if let SessionTreeNode::Folder { children, .. } = node {
                for (idx, child) in children.iter().enumerate() {
                    if child.id() == target_id {
                        return Some(idx);
                    }
                }
                if let Some(idx) = Self::find_node_index_in_parent(children, target_id) {
                    return Some(idx);
                }
            }
        }
        None
    }

    // ─── Tab content renderers ───────────────────────────────────────────────

    fn render_tab_general(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        div()
            .flex()
            .flex_col()
            .gap(SPACE_LG)
            // Row 1: Connection Name
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.general.name")),
                    )
                    .child(Self::input_field(&self.session_name_input, cx)),
            )
            // Row 2: Directory and Icon Color side-by-side
            .child(
                h_flex()
                    .w_full()
                    .gap(SPACE_LG)
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(SPACE_XS)
                            .child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.text_secondary))
                                    .child(i18n!(cx, "ssh.general.directory")),
                            )
                            .child(self.render_folder_dropdown(cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(SPACE_XS)
                            .child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.text_secondary))
                                    .child(i18n!(cx, "ssh.general.icon_color")),
                            )
                            .child(self.render_icon_color_selector(cx)),
                    ),
            )
            // Row 3: Host and Port side-by-side
            .child(
                h_flex()
                    .w_full()
                    .gap(SPACE_LG)
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(SPACE_XS)
                            .child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.text_secondary))
                                    .child(i18n!(cx, "ssh.connection.host")),
                            )
                            .child(Self::input_field(&self.session_host_input, cx)),
                    )
                    .child(
                        div()
                            .w(px(70.0))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap(SPACE_XS)
                            .child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.text_secondary))
                                    .child(i18n!(cx, "ssh.connection.port")),
                            )
                            .child(Self::input_field(&self.session_port_input, cx)),
                    ),
            )
            // Row 4: Tags
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.notes.tags")),
                    )
                    .child(Self::input_field(&self.session_tags_input, cx)),
            )
            // Row 5: Startup Command
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.general.startup_command")),
                    )
                    .child(self.render_resizable_textarea(
                        &self.session_startup_command_input,
                        self.startup_command_height,
                        px(80.0),
                        ResizeTarget::StartupCommand,
                        cx,
                    ))
                    .child(
                        div()
                            .text_size(ui_text_ms(cx))
                            .text_color(rgb(t.text_muted))
                            .child(i18n!(cx, "ssh.general.startup_command_hint")),
                    ),
            )
            // Row 6: Notes
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.notes.notes")),
                    )
                    .child(self.render_resizable_textarea(
                        &self.session_notes_input,
                        self.notes_height,
                        px(80.0),
                        ResizeTarget::Notes,
                        cx,
                    )),
            )
    }

    fn render_resizable_textarea(
        &self,
        input: &Entity<SimpleInputState>,
        custom_height: Option<Pixels>,
        default_height: Pixels,
        target: ResizeTarget,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = theme(cx);
        let height = custom_height.unwrap_or(default_height);
        let min_h = px(60.0);
        let max_h = px(300.0);
        let clamped_h = height.max(min_h).min(max_h);

        // Let the inner SimpleInput fill the available space instead of
        // using a fixed visible-row count. This way the input naturally
        // follows the resizable container height.
        input.update(cx, |s, _cx| s.set_fill_height(true));

        div()
            .relative()
            .w_full()
            .min_h(min_h)
            .h(clamped_h)
            .flex()
            .flex_col()
            .bg(surface_bg_t(t.bg_secondary, &t))
            .border_1()
            .border_color(rgb(t.border))
            .input_focus_ring(input, &t, cx)
            .rounded(RADIUS_STD)
            .overflow_hidden()
            .p(SPACE_SM)
            .child(
                SimpleInput::new(input)
                    .text_size(ui_text_md(cx))
                    .fill_height(),
            )
            .child(
                // Resize handle in bottom-right corner
                div()
                    .absolute()
                    .right(px(0.0))
                    .bottom(px(0.0))
                    .w(ICON_STD)
                    .h(ICON_STD)
                    .cursor_row_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event: &MouseDownEvent, _window, cx| {
                            // anchor_y is set on the first mousemove (on the
                            // outer dialog) so the delta is always computed
                            // in the same SessionPanel-relative coordinate space.
                            this.resize_dragging = Some(ResizeDragState {
                                target: target.clone(),
                                start_height: clamped_h,
                                anchor_y: None,
                            });
                            cx.notify();
                        }),
                    )
                    .child(
                        // Visual corner grip lines (two small diagonal lines)
                        div()
                            .absolute()
                            .right(px(3.0))
                            .bottom(px(3.0))
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .items_end()
                            .child(div().w(SPACE_MD).h(px(1.0)).bg(rgb(t.text_muted)))
                            .child(div().w(px(5.0)).h(px(1.0)).bg(rgb(t.text_muted))),
                    ),
            )
    }

    fn render_icon_color_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let current = self.session_icon_color;
        let current_label = current.display_name().to_string();
        let is_open = self.ssh_icon_color_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("icon-color-btn", &current_label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.icon_color_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_icon_color_dropdown_open = !this.ssh_icon_color_dropdown_open;
                this.close_all_dropdowns_except("icon_color");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_icon_color_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let current = self.session_icon_color;
        use crate::views::components::dropdown_overlay;

        dropdown_overlay("icon-color-dropdown-list", &t, cx).children(
            IconColor::all_variants().iter().map(|&color| {
                let is_selected = current == color;
                let label = color.display_name().to_string();
                let dot_color = color.color_hex().unwrap_or(t.text_primary);

                let row = div()
                    .id(ElementId::Name(format!("icon-color-{:?}", color).into()))
                    .px(ICON_SM)
                    .py(SPACE_SM)
                    .cursor_pointer()
                    .text_size(ui_text_md(cx))
                    .text_color(rgb(t.text_primary))
                    .when(is_selected, |d| d.bg(with_alpha(t.border_active, 0.2)))
                    .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                    .flex()
                    .items_center()
                    .gap(SPACE_MD)
                    .child(
                        div()
                            .w(SPACE_LG)
                            .h(SPACE_LG)
                            .rounded_full()
                            .bg(rgb(dot_color)),
                    )
                    .child(label.to_string())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session_icon_color = color;
                        this.ssh_icon_color_dropdown_open = false;
                        cx.notify();
                    }));

                if is_selected {
                    row.child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.border_active))
                            .child("✓"),
                    )
                } else {
                    row
                }
            }),
        )
    }

    fn render_tab_authentication(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let auth_type = self.session_auth_type.clone();

        div()
            .flex()
            .flex_col()
            .gap(SPACE_LG)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.auth.username")),
                    )
                    .child(Self::input_field(&self.session_username_input, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.auth.method.label")),
                    )
                    .child(self.render_auth_method_dropdown(cx)),
            )
            .when(matches!(auth_type, SshAuthType::Password { .. }), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(SPACE_XS)
                        .child(
                            div()
                                .text_size(ui_text_sm(cx))
                                .text_color(rgb(t.text_secondary))
                                .child(i18n!(cx, "ssh.auth.password")),
                        )
                        .child(Self::input_field(&self.session_password_input, cx)),
                )
            })
            .when(matches!(auth_type, SshAuthType::PrivateKey { .. }), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(SPACE_XS)
                        .child(
                            div()
                                .text_size(ui_text_sm(cx))
                                .text_color(rgb(t.text_secondary))
                                .child(i18n!(cx, "ssh.auth.private_key")),
                        )
                        .child(Self::input_field(&self.session_key_path_input, cx)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(SPACE_XS)
                        .child(
                            div()
                                .text_size(ui_text_sm(cx))
                                .text_color(rgb(t.text_secondary))
                                .child(i18n!(cx, "ssh.auth.passphrase")),
                        )
                        .child(Self::input_field(&self.session_passphrase_input, cx)),
                )
            })
    }

    fn render_auth_method_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let current_label = match &self.session_auth_type {
            SshAuthType::Password { .. } => i18n!(cx, "ssh.auth.method.password"),
            SshAuthType::PrivateKey { .. } => i18n!(cx, "ssh.auth.method.private_key"),
            SshAuthType::KeyboardInteractive => i18n!(cx, "ssh.auth.method.keyboard"),
            SshAuthType::SshAgent { .. } => i18n!(cx, "ssh.auth.method.agent"),
        };
        let is_open = self.ssh_auth_method_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("auth-method-btn", &current_label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.auth_method_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_auth_method_dropdown_open = !this.ssh_auth_method_dropdown_open;
                this.close_all_dropdowns_except("auth_method");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_auth_method_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        let methods: Vec<(SshAuthType, String)> = vec![
            (
                SshAuthType::Password { password: None },
                i18n!(cx, "ssh.auth.method.password"),
            ),
            (
                SshAuthType::PrivateKey {
                    key_path: String::new(),
                    passphrase: None,
                },
                i18n!(cx, "ssh.auth.method.private_key"),
            ),
            (
                SshAuthType::KeyboardInteractive,
                i18n!(cx, "ssh.auth.method.keyboard"),
            ),
            (
                SshAuthType::SshAgent { socket_path: None },
                i18n!(cx, "ssh.auth.method.agent"),
            ),
        ];

        dropdown_overlay("auth-method-dropdown-list", &t, cx).children(
            methods.into_iter().enumerate().map(|(idx, (auth, label))| {
                let is_selected = std::mem::discriminant(&self.session_auth_type)
                    == std::mem::discriminant(&auth);
                dropdown_option(format!("auth-method-{}", idx), &label, is_selected, &t, cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session_auth_type = auth.clone();
                        this.ssh_auth_method_dropdown_open = false;
                        cx.notify();
                    }))
            }),
        )
    }

    fn render_tab_connection(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let show_proxy = self.session_proxy_type != ProxyType::None;
        let show_proxy_auth =
            matches!(self.session_proxy_type, ProxyType::Http | ProxyType::Socks5);

        div()
            .flex()
            .flex_col()
            .gap(SPACE_LG)
            // Connection Timeout & TCP NoDelay side-by-side
            .child(
                h_flex()
                    .gap(SPACE_LG)
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(SPACE_XS)
                            .child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.text_secondary))
                                    .child(i18n!(cx, "ssh.connection.timeout")),
                            )
                            .child(Self::input_field(&self.session_timeout_input, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(SPACE_XS)
                            .child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.text_secondary))
                                    .child(i18n!(cx, "ssh.connection.tcp_nodelay")),
                            )
                            .child(
                                h_flex()
                                    .gap(SPACE_MD)
                                    .items_center()
                                    .h(px(32.0))
                                    .child(
                                        div()
                                            .id("checkbox-tcp-nodelay")
                                            .cursor_pointer()
                                            .w(ICON_STD)
                                            .h(ICON_STD)
                                            .rounded(RADIUS_SM)
                                            .border_1()
                                            .border_color(rgb(t.border))
                                            .when(self.session_tcp_nodelay, |d| {
                                                d.bg(rgb(t.border_active))
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.session_tcp_nodelay =
                                                    !this.session_tcp_nodelay;
                                                cx.notify();
                                            }))
                                            .when(self.session_tcp_nodelay, |d| {
                                                d.child(
                                                    AppIcon::Check
                                                        .size(SPACE_LG)
                                                        .text_color(rgb(0xffffff)),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(ui_text_sm(cx))
                                            .text_color(rgb(t.text_secondary))
                                            .child(i18n!(cx, "ssh.connection.tcp_nodelay_desc")),
                                    ),
                            ),
                    ),
            )
            .child(self.render_keepalive_section(cx))
            // Channel Buffer Size
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.connection.channel_buffer")),
                    )
                    .child(self.render_channel_buffer_dropdown(cx)),
            )
            // Proxy Type
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.connection.proxy.label")),
                    )
                    .child(self.render_proxy_type_dropdown(cx)),
            )
            // Proxy Host/Port (for HTTP/SOCKS5)
            .when(show_proxy_auth, |d| {
                d.child(
                    h_flex()
                        .gap(SPACE_MD)
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(SPACE_XS)
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.text_secondary))
                                        .child(i18n!(cx, "ssh.connection.proxy_host")),
                                )
                                .child(Self::input_field(&self.session_proxy_host_input, cx)),
                        )
                        .child(
                            div()
                                .w(px(80.0))
                                .flex()
                                .flex_col()
                                .gap(SPACE_XS)
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.text_secondary))
                                        .child(i18n!(cx, "ssh.connection.proxy_port")),
                                )
                                .child(Self::input_field(&self.session_proxy_port_input, cx)),
                        ),
                )
                .child(
                    h_flex()
                        .gap(SPACE_MD)
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(SPACE_XS)
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.text_secondary))
                                        .child(i18n!(cx, "ssh.connection.proxy_username")),
                                )
                                .child(Self::input_field(&self.session_proxy_username_input, cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(SPACE_XS)
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.text_secondary))
                                        .child(i18n!(cx, "ssh.connection.proxy_password")),
                                )
                                .child(Self::input_field(&self.session_proxy_password_input, cx)),
                        ),
                )
            })
            // Proxy Host (for Jump Host)
            .when(show_proxy && !show_proxy_auth, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(SPACE_XS)
                        .child(
                            div()
                                .text_size(ui_text_sm(cx))
                                .text_color(rgb(t.text_secondary))
                                .child(i18n!(cx, "ssh.connection.proxy_host")),
                        )
                        .child(Self::input_field(&self.session_proxy_host_input, cx)),
                )
            })
            // SFTP & Monitoring toggles
            .child(
                h_flex()
                    .gap(SPACE_XL)
                    .child(
                        h_flex()
                            .gap(SPACE_MD)
                            .items_center()
                            .child(
                                div()
                                    .id("checkbox-enable-sftp")
                                    .cursor_pointer()
                                    .w(ICON_STD)
                                    .h(ICON_STD)
                                    .rounded(RADIUS_SM)
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .when(self.session_enable_sftp, |d| d.bg(rgb(t.border_active)))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.session_enable_sftp = !this.session_enable_sftp;
                                        cx.notify();
                                    }))
                                    .when(self.session_enable_sftp, |d| {
                                        d.child(
                                            AppIcon::Check.size(SPACE_LG).text_color(rgb(0xffffff)),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.text_secondary))
                                    .child(i18n!(cx, "ssh.connection.sftp")),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(SPACE_MD)
                            .items_center()
                            .child(
                                div()
                                    .id("checkbox-enable-monitor")
                                    .cursor_pointer()
                                    .w(ICON_STD)
                                    .h(ICON_STD)
                                    .rounded(RADIUS_SM)
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .when(self.session_enable_monitor, |d| {
                                        d.bg(rgb(t.border_active))
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.session_enable_monitor = !this.session_enable_monitor;
                                        cx.notify();
                                    }))
                                    .when(self.session_enable_monitor, |d| {
                                        d.child(
                                            AppIcon::Check.size(SPACE_LG).text_color(rgb(0xffffff)),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.text_secondary))
                                    .child(i18n!(cx, "ssh.connection.monitor")),
                            ),
                    ),
            )
            // Monitor Options (only visible when enable_monitor is checked)
            .when(self.session_enable_monitor, |d| {
                d.child(
                    div()
                        .pl(SPACE_XL)
                        .flex()
                        .gap(SPACE_XL)
                        .child(
                            h_flex()
                                .gap(SPACE_SM)
                                .items_center()
                                .child(
                                    div()
                                        .id("checkbox-monitor-cpu")
                                        .cursor_pointer()
                                        .w(SPACE_LG)
                                        .h(SPACE_LG)
                                        .rounded(RADIUS_SM)
                                        .border_1()
                                        .border_color(rgb(t.border))
                                        .when(self.session_monitor_cpu, |d| {
                                            d.bg(rgb(t.border_active))
                                        })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.session_monitor_cpu = !this.session_monitor_cpu;
                                            cx.notify();
                                        }))
                                        .when(self.session_monitor_cpu, |d| {
                                            d.child(
                                                AppIcon::Check
                                                    .size(SPACE_LG)
                                                    .text_color(rgb(0xffffff)),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.text_secondary))
                                        .child(i18n!(cx, "ssh.connection.monitor_cpu")),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap(SPACE_SM)
                                .items_center()
                                .child(
                                    div()
                                        .id("checkbox-monitor-mem")
                                        .cursor_pointer()
                                        .w(SPACE_LG)
                                        .h(SPACE_LG)
                                        .rounded(RADIUS_SM)
                                        .border_1()
                                        .border_color(rgb(t.border))
                                        .when(self.session_monitor_mem, |d| {
                                            d.bg(rgb(t.border_active))
                                        })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.session_monitor_mem = !this.session_monitor_mem;
                                            cx.notify();
                                        }))
                                        .when(self.session_monitor_mem, |d| {
                                            d.child(
                                                AppIcon::Check
                                                    .size(SPACE_LG)
                                                    .text_color(rgb(0xffffff)),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.text_secondary))
                                        .child(i18n!(cx, "ssh.connection.monitor_mem")),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap(SPACE_SM)
                                .items_center()
                                .child(
                                    div()
                                        .id("checkbox-monitor-disk")
                                        .cursor_pointer()
                                        .w(SPACE_LG)
                                        .h(SPACE_LG)
                                        .rounded(RADIUS_SM)
                                        .border_1()
                                        .border_color(rgb(t.border))
                                        .when(self.session_monitor_disk, |d| {
                                            d.bg(rgb(t.border_active))
                                        })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.session_monitor_disk = !this.session_monitor_disk;
                                            cx.notify();
                                        }))
                                        .when(self.session_monitor_disk, |d| {
                                            d.child(
                                                AppIcon::Check
                                                    .size(SPACE_LG)
                                                    .text_color(rgb(0xffffff)),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.text_secondary))
                                        .child(i18n!(cx, "ssh.connection.monitor_disk")),
                                ),
                        ),
                )
            })
    }

    fn toggle_algo(&mut self, list_key: &str, algo: &str, cx: &mut Context<Self>) {
        let list = match list_key {
            "ssh.advanced.kex_desc" => &mut self.session_kex_algorithms,
            "ssh.advanced.cipher_desc" => &mut self.session_cipher_algorithms,
            "ssh.advanced.mac_desc" => &mut self.session_mac_algorithms,
            "ssh.advanced.hostkey_desc" => &mut self.session_hostkey_algorithms,
            _ => return,
        };
        if let Some(pos) = list.iter().position(|x| x == algo) {
            list.remove(pos);
        } else {
            list.push(algo.to_string());
        }
        cx.notify();
    }

    fn move_algo(&mut self, list_key: &str, algo: &str, up: bool, cx: &mut Context<Self>) {
        let list = match list_key {
            "ssh.advanced.kex_desc" => &mut self.session_kex_algorithms,
            "ssh.advanced.cipher_desc" => &mut self.session_cipher_algorithms,
            "ssh.advanced.mac_desc" => &mut self.session_mac_algorithms,
            "ssh.advanced.hostkey_desc" => &mut self.session_hostkey_algorithms,
            _ => return,
        };
        let pos = if let Some(pos) = list.iter().position(|x| x == algo) {
            pos
        } else {
            list.push(algo.to_string());
            list.len() - 1
        };

        if up && pos > 0 {
            list.swap(pos, pos - 1);
        } else if !up && pos < list.len() - 1 {
            list.swap(pos, pos + 1);
        }
        cx.notify();
    }

    fn render_algorithm_list(
        &self,
        all_algos: &[&str],
        current_algos: &Vec<String>,
        desc_key: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let mut full_list = current_algos.clone();
        for algo in all_algos {
            let algo_str = algo.to_string();
            if !full_list.contains(&algo_str) {
                full_list.push(algo_str);
            }
        }

        div()
            .flex()
            .flex_col()
            .gap(SPACE_LG)
            .child(
                div()
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_secondary))
                    .child(i18n!(cx, desc_key)),
            )
            .child(div().flex().flex_col().gap(SPACE_SM).children(
                full_list.into_iter().enumerate().map(|(idx, algo)| {
                    let is_checked = current_algos.contains(&algo);
                    let is_first = idx == 0;
                    let is_last = idx == all_algos.len() - 1;
                    let algo_clone1 = algo.clone();
                    let algo_clone2 = algo.clone();
                    let algo_clone3 = algo.clone();

                    h_flex()
                        .px(SPACE_MD)
                        .py(SPACE_SM)
                        .border_1()
                        .border_color(rgb(t.border))
                        .rounded(RADIUS_STD)
                        .justify_between()
                        .items_center()
                        .child(
                            h_flex()
                                .gap(SPACE_MD)
                                .items_center()
                                .child(
                                    div()
                                        .id(format!("check-algo-{}", algo))
                                        .cursor_pointer()
                                        .w(ICON_STD)
                                        .h(ICON_STD)
                                        .rounded(RADIUS_SM)
                                        .border_1()
                                        .border_color(rgb(t.border))
                                        .when(is_checked, |d| d.bg(rgb(t.border_active)))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.toggle_algo(desc_key, &algo_clone1, cx);
                                        }))
                                        .when(is_checked, |d| {
                                            d.child(
                                                AppIcon::Check
                                                    .size(SPACE_LG)
                                                    .text_color(rgb(0xffffff)),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(if is_checked {
                                            t.text_primary
                                        } else {
                                            t.text_muted
                                        }))
                                        .child(algo.clone()),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap(SPACE_XS)
                                .child({
                                    let id = format!("up-algo-{}", algo);
                                    let disabled = is_first;
                                    div()
                                        .id(ElementId::Name(id.into()))
                                        .flex_shrink_0()
                                        .w(px(24.0))
                                        .h(px(24.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(RADIUS_STD)
                                        .when(!disabled, |d| {
                                            d.cursor_pointer()
                                                .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                                        })
                                        .child(AppIcon::ChevronUp.size(SPACE_LG).text_color(rgb(
                                            if disabled {
                                                t.text_muted
                                            } else {
                                                t.text_secondary
                                            },
                                        )))
                                        .when(!disabled, |d| {
                                            d.on_click(cx.listener(move |this, _, _, cx| {
                                                this.move_algo(desc_key, &algo_clone2, true, cx);
                                            }))
                                        })
                                })
                                .child({
                                    let id = format!("down-algo-{}", algo);
                                    let disabled = is_last;
                                    div()
                                        .id(ElementId::Name(id.into()))
                                        .flex_shrink_0()
                                        .w(px(24.0))
                                        .h(px(24.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(RADIUS_STD)
                                        .when(!disabled, |d| {
                                            d.cursor_pointer()
                                                .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                                        })
                                        .child(AppIcon::ChevronDown.size(SPACE_LG).text_color(rgb(
                                            if disabled {
                                                t.text_muted
                                            } else {
                                                t.text_secondary
                                            },
                                        )))
                                        .when(!disabled, |d| {
                                            d.on_click(cx.listener(move |this, _, _, cx| {
                                                this.move_algo(desc_key, &algo_clone3, false, cx);
                                            }))
                                        })
                                }),
                        )
                }),
            ))
    }

    fn render_tab_russh_kex(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let kex_algos = &[
            "curve25519-sha256",
            "ecdh-sha2-nistp256",
            "ecdh-sha2-nistp384",
            "ecdh-sha2-nistp521",
            "diffie-hellman-group14-sha256",
            "diffie-hellman-group16-sha512",
            "diffie-hellman-group18-sha512",
        ];

        div()
            .flex()
            .flex_col()
            .gap(SPACE_XL)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child("Diffie-Hellman Group Exchange Parameters (GEX)"),
                    )
                    .child(
                        h_flex()
                            .gap(SPACE_LG)
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(SPACE_XS)
                                    .child(
                                        div()
                                            .text_size(ui_text_ms(cx))
                                            .text_color(rgb(t.text_secondary))
                                            .child(i18n!(cx, "session.min_bit_length")),
                                    )
                                    .child(self.render_gex_min_dropdown(cx)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(SPACE_XS)
                                    .child(
                                        div()
                                            .text_size(ui_text_ms(cx))
                                            .text_color(rgb(t.text_secondary))
                                            .child(i18n!(cx, "session.pref_bit_length")),
                                    )
                                    .child(self.render_gex_preferred_dropdown(cx)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(SPACE_XS)
                                    .child(
                                        div()
                                            .text_size(ui_text_ms(cx))
                                            .text_color(rgb(t.text_secondary))
                                            .child("Max Bit Length"),
                                    )
                                    .child(self.render_gex_max_dropdown(cx)),
                            ),
                    ),
            )
            .child(self.render_algorithm_list(
                kex_algos,
                &self.session_kex_algorithms,
                "ssh.advanced.kex_desc",
                cx,
            ))
    }

    fn render_tab_russh_cipher(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ciphers = &[
            "chacha20-poly1305@openssh.com",
            "aes256-gcm@openssh.com",
            "aes128-gcm@openssh.com",
            "aes256-ctr",
            "aes192-ctr",
            "aes128-ctr",
        ];
        self.render_algorithm_list(
            ciphers,
            &self.session_cipher_algorithms,
            "ssh.advanced.cipher_desc",
            cx,
        )
    }

    fn render_tab_russh_mac(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let macs = &[
            "hmac-sha2-256-etm@openssh.com",
            "hmac-sha2-512-etm@openssh.com",
            "hmac-sha2-256",
            "hmac-sha2-512",
            "umac-128-etm@openssh.com",
        ];
        self.render_algorithm_list(
            macs,
            &self.session_mac_algorithms,
            "ssh.advanced.mac_desc",
            cx,
        )
    }

    fn render_tab_russh_hostkey(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hostkeys = &["ssh-ed25519", "rsa-sha2-512", "rsa-sha2-256"];
        self.render_algorithm_list(
            hostkeys,
            &self.session_hostkey_algorithms,
            "ssh.advanced.hostkey_desc",
            cx,
        )
    }

    fn render_proxy_type_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = match self.session_proxy_type {
            ProxyType::None => i18n!(cx, "ssh.connection.proxy.none"),
            ProxyType::Socks5 => i18n!(cx, "ssh.connection.proxy.socks5"),
            ProxyType::Http => i18n!(cx, "ssh.connection.proxy.http"),
            ProxyType::Jump => i18n!(cx, "ssh.connection.proxy.jump"),
        };
        let is_open = self.ssh_proxy_type_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("proxy-type-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.proxy_type_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_proxy_type_dropdown_open = !this.ssh_proxy_type_dropdown_open;
                this.close_all_dropdowns_except("proxy_type");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_proxy_type_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        let types: Vec<(ProxyType, String)> = vec![
            (ProxyType::None, i18n!(cx, "ssh.connection.proxy.none")),
            (ProxyType::Socks5, i18n!(cx, "ssh.connection.proxy.socks5")),
            (ProxyType::Http, i18n!(cx, "ssh.connection.proxy.http")),
            (ProxyType::Jump, i18n!(cx, "ssh.connection.proxy.jump")),
        ];

        dropdown_overlay("proxy-type-dropdown-list", &t, cx).children(
            types.into_iter().enumerate().map(|(idx, (pt, label))| {
                let is_selected = self.session_proxy_type == pt;
                dropdown_option(format!("proxy-type-{}", idx), &label, is_selected, &t, cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session_proxy_type = pt;
                        this.ssh_proxy_type_dropdown_open = false;
                        cx.notify();
                    }))
            }),
        )
    }

    fn render_tab_terminal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        div()
            .flex()
            .flex_col()
            .gap(SPACE_LG)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.terminal.type")),
                    )
                    .child(self.render_terminal_type_dropdown(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.terminal.charset")),
                    )
                    .child(self.render_charset_dropdown(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.terminal.scrollback")),
                    )
                    .child(Self::input_field(&self.session_scrollback_input, cx)),
            )
    }

    fn render_terminal_type_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = self
            .session_terminal_type
            .clone()
            .unwrap_or_else(|| i18n!(cx, "ssh.terminal.system_default"));
        let is_open = self.ssh_terminal_type_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("terminal-type-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.terminal_type_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_terminal_type_dropdown_open = !this.ssh_terminal_type_dropdown_open;
                this.close_all_dropdowns_except("terminal_type");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_terminal_type_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        let system_default = i18n!(cx, "ssh.terminal.system_default");
        let mut types = vec![(None, system_default.clone())];
        for &tt in velowork_core::SUPPORTED_TERM_TYPES {
            types.push((Some(tt.to_string()), tt.to_string()));
        }
        if let Some(ref cur) = self.session_terminal_type {
            if !cur.is_empty() && !velowork_core::SUPPORTED_TERM_TYPES.contains(&cur.as_str()) {
                types.push((Some(cur.clone()), cur.clone()));
            }
        }

        dropdown_overlay("terminal-type-dropdown-list", &t, cx).children(
            types.into_iter().enumerate().map(|(idx, (tt, label))| {
                let is_selected = self.session_terminal_type == tt;
                dropdown_option(format!("term-type-{}", idx), &label, is_selected, &t, cx).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.session_terminal_type = tt.clone();
                        this.ssh_terminal_type_dropdown_open = false;
                        cx.notify();
                    }),
                )
            }),
        )
    }

    fn render_charset_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = self
            .session_charset
            .clone()
            .unwrap_or_else(|| i18n!(cx, "ssh.terminal.system_default"));
        let is_open = self.ssh_charset_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("charset-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.charset_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_charset_dropdown_open = !this.ssh_charset_dropdown_open;
                this.close_all_dropdowns_except("charset");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_charset_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        let system_default = i18n!(cx, "ssh.terminal.system_default");
        let mut charsets = vec![(None, system_default.clone())];
        for &cs in velowork_core::charset::SUPPORTED_CHARSETS {
            charsets.push((Some(cs.to_string()), cs.to_string()));
        }

        dropdown_overlay("charset-dropdown-list", &t, cx).children(
            charsets.into_iter().enumerate().map(|(idx, (cs, label))| {
                let is_selected = self.session_charset == cs;
                dropdown_option(format!("charset-{}", idx), &label, is_selected, &t, cx).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.session_charset = cs.clone();
                        this.ssh_charset_dropdown_open = false;
                        cx.notify();
                    }),
                )
            }),
        )
    }

    fn render_tab_advanced(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        div()
            .flex()
            .flex_col()
            .gap(SPACE_LG)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.advanced.compression")),
                    )
                    .child(self.render_compression_dropdown(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.advanced.strict_host_key")),
                    )
                    .child(self.render_strict_host_key_dropdown(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.advanced.max_packets")),
                    )
                    .child(self.render_max_packets_dropdown(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.advanced.recv_window")),
                    )
                    .child(self.render_recv_window_dropdown(cx)),
            )
    }

    fn render_compression_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = self.session_compression.display_name().to_string();
        let is_open = self.ssh_compression_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("compression-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.compression_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_compression_dropdown_open = !this.ssh_compression_dropdown_open;
                this.close_all_dropdowns_except("compression");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_compression_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        dropdown_overlay("compression-dropdown-list", &t, cx).children(
            CompressionType::all_variants()
                .iter()
                .enumerate()
                .map(|(idx, &ct)| {
                    let label = ct.display_name().to_string();
                    let is_selected = self.session_compression == ct;
                    dropdown_option(format!("compression-{}", idx), &label, is_selected, &t, cx)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.session_compression = ct;
                            this.ssh_compression_dropdown_open = false;
                            cx.notify();
                        }))
                }),
        )
    }

    fn render_strict_host_key_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = self.session_strict_host_key.display_name().to_string();
        let is_open = self.ssh_strict_host_key_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("strict-host-key-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.strict_host_key_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_strict_host_key_dropdown_open = !this.ssh_strict_host_key_dropdown_open;
                this.close_all_dropdowns_except("strict_host_key");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_strict_host_key_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        dropdown_overlay("strict-host-key-dropdown-list", &t, cx).children(
            StrictHostKey::all_variants()
                .iter()
                .enumerate()
                .map(|(idx, &shk)| {
                    let label = shk.display_name().to_string();
                    let is_selected = self.session_strict_host_key == shk;
                    dropdown_option(format!("shk-{}", idx), &label, is_selected, &t, cx).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.session_strict_host_key = shk;
                            this.ssh_strict_host_key_dropdown_open = false;
                            cx.notify();
                        }),
                    )
                }),
        )
    }

    fn render_max_packets_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = format!("{}", self.session_max_packets);
        let is_open = self.ssh_max_packets_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("max-packets-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.max_packets_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_max_packets_dropdown_open = !this.ssh_max_packets_dropdown_open;
                this.close_all_dropdowns_except("max_packets");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_max_packets_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        let options = vec![4096u32, 8192, 16384, 32768, 65536, 131072];

        dropdown_overlay("max-packets-dropdown-list", &t, cx).children(options.into_iter().map(
            |val| {
                let label = format!("{}", val);
                let is_selected = self.session_max_packets == val;
                dropdown_option(format!("max-pkt-{}", val), &label, is_selected, &t, cx).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.session_max_packets = val;
                        this.ssh_max_packets_dropdown_open = false;
                        cx.notify();
                    }),
                )
            },
        ))
    }

    fn render_recv_window_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = if self.session_recv_window >= 1048576 {
            format!("{} MB", self.session_recv_window / 1048576)
        } else {
            format!("{} KB", self.session_recv_window / 1024)
        };
        let is_open = self.ssh_recv_window_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("recv-window-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.recv_window_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_recv_window_dropdown_open = !this.ssh_recv_window_dropdown_open;
                this.close_all_dropdowns_except("recv_window");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_recv_window_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        let options: Vec<(u32, String)> = vec![
            (262144, "256 KB".to_string()),
            (524288, "512 KB".to_string()),
            (1048576, "1 MB".to_string()),
            (2097152, "2 MB".to_string()),
            (4194304, "4 MB".to_string()),
            (8388608, "8 MB".to_string()),
        ];

        dropdown_overlay("recv-window-dropdown-list", &t, cx).children(options.into_iter().map(
            |(val, label)| {
                let is_selected = self.session_recv_window == val;
                dropdown_option(format!("recv-win-{}", val), &label, is_selected, &t, cx).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.session_recv_window = val;
                        this.ssh_recv_window_dropdown_open = false;
                        cx.notify();
                    }),
                )
            },
        ))
    }

    fn render_gex_min_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = format!("{}", self.session_gex_min);
        let is_open = self.ssh_gex_min_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("gex-min-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.gex_min_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_gex_min_dropdown_open = !this.ssh_gex_min_dropdown_open;
                this.close_all_dropdowns_except("gex_min");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_gex_preferred_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = format!("{}", self.session_gex_preferred);
        let is_open = self.ssh_gex_preferred_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("gex-pref-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.gex_preferred_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_gex_preferred_dropdown_open = !this.ssh_gex_preferred_dropdown_open;
                this.close_all_dropdowns_except("gex_preferred");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_gex_max_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = format!("{}", self.session_gex_max);
        let is_open = self.ssh_gex_max_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("gex-max-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.gex_max_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_gex_max_dropdown_open = !this.ssh_gex_max_dropdown_open;
                this.close_all_dropdowns_except("gex_max");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_gex_min_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};
        let options = vec![1024, 2048, 3072, 4096];
        dropdown_overlay("gex-min-dropdown-list", &t, cx).children(options.into_iter().map(|val| {
            let is_selected = self.session_gex_min == val;
            dropdown_option(
                format!("gex-min-{}", val),
                &format!("{}", val),
                is_selected,
                &t,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.session_gex_min = val;
                this.ssh_gex_min_dropdown_open = false;
                cx.notify();
            }))
        }))
    }

    fn render_gex_preferred_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};
        let options = vec![2048, 3072, 4096, 8192];
        dropdown_overlay("gex-pref-dropdown-list", &t, cx).children(options.into_iter().map(
            |val| {
                let is_selected = self.session_gex_preferred == val;
                dropdown_option(
                    format!("gex-pref-{}", val),
                    &format!("{}", val),
                    is_selected,
                    &t,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.session_gex_preferred = val;
                    this.ssh_gex_preferred_dropdown_open = false;
                    cx.notify();
                }))
            },
        ))
    }

    fn render_gex_max_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};
        let options = vec![4096, 8192, 16384];
        dropdown_overlay("gex-max-dropdown-list", &t, cx).children(options.into_iter().map(|val| {
            let is_selected = self.session_gex_max == val;
            dropdown_option(
                format!("gex-max-{}", val),
                &format!("{}", val),
                is_selected,
                &t,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.session_gex_max = val;
                this.ssh_gex_max_dropdown_open = false;
                cx.notify();
            }))
        }))
    }

    fn render_folder_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let current_parent_id = self.session_parent_folder_id.clone();
        let mut folders = Vec::new();
        folders.push((None, i18n!(cx, "ssh.general.directory_root")));
        let store = cx
            .global::<velowork_workspace::stores::GlobalSessionStore>()
            .0
            .read(cx);
        let ssh_tree = self.active_tree(store, cx).to_vec();

        fn gather_folders(
            nodes: &[velowork_state::SessionTreeNode],
            list: &mut Vec<(Option<String>, String)>,
            depth: usize,
        ) {
            for node in nodes {
                if let velowork_state::SessionTreeNode::Folder {
                    id, name, children, ..
                } = node
                {
                    let indent = "  ".repeat(depth);
                    list.push((Some(id.clone()), format!("{}{}", indent, name)));
                    gather_folders(children, list, depth + 1);
                }
            }
        }
        gather_folders(&ssh_tree, &mut folders, 1);

        let label = folders
            .iter()
            .find(|(id, _)| *id == current_parent_id)
            .map(|(_, name)| name.trim().to_string())
            .unwrap_or_else(|| i18n!(cx, "ssh.general.directory_root"));

        let is_open = self.ssh_folder_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("folder-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.folder_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_folder_dropdown_open = !this.ssh_folder_dropdown_open;
                this.close_all_dropdowns_except("folder");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_folder_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        let mut folders = Vec::new();
        folders.push((None, i18n!(cx, "ssh.general.directory_root")));
        let store = cx
            .global::<velowork_workspace::stores::GlobalSessionStore>()
            .0
            .read(cx);
        let ssh_tree = self.active_tree(store, cx).to_vec();

        fn gather_folders(
            nodes: &[velowork_state::SessionTreeNode],
            list: &mut Vec<(Option<String>, String)>,
            depth: usize,
        ) {
            for node in nodes {
                if let velowork_state::SessionTreeNode::Folder {
                    id, name, children, ..
                } = node
                {
                    let indent = "  ".repeat(depth);
                    list.push((Some(id.clone()), format!("{}{}", indent, name)));
                    gather_folders(children, list, depth + 1);
                }
            }
        }
        gather_folders(&ssh_tree, &mut folders, 1);

        dropdown_overlay("directory-dropdown-list", &t, cx).children(folders.into_iter().map(
            |(val, label)| {
                let is_selected = self.session_parent_folder_id == val;
                dropdown_option(
                    format!("dir-{}", val.clone().unwrap_or_else(|| "root".to_string())),
                    &label,
                    is_selected,
                    &t,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.session_parent_folder_id = val.clone();
                    this.ssh_folder_dropdown_open = false;
                    cx.notify();
                }))
            },
        ))
    }

    fn keepalive_strategy_option(
        &self,
        cx: &mut Context<Self>,
        strategy: KeepAliveStrategy,
        label: String,
    ) -> impl IntoElement {
        let t = theme(cx);
        let selected = self.session_keepalive_strategy == strategy;
        div()
            .id(SharedString::from(format!("kalive-opt-{:?}", strategy)))
            .cursor_pointer()
            .flex_1()
            .px(SPACE_MD)
            .py(SPACE_SM)
            .rounded(RADIUS_STD)
            .border_1()
            .border_color(if selected {
                rgb(t.border_active)
            } else {
                rgb(t.border)
            })
            .bg(if selected {
                rgb(t.bg_hover)
            } else {
                rgb(t.bg_panel)
            })
            .text_size(ui_text_sm(cx))
            .text_color(if selected {
                rgb(t.text_primary)
            } else {
                rgb(t.text_secondary)
            })
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |this, _, _, cx| {
                let s = strategy;
                match s {
                    KeepAliveStrategy::Always => {
                        this.session_keepalive_input
                            .update(cx, |i, cx| i.set_value("30", cx));
                        this.session_keepalive_max = 3;
                        this.session_idle_disconnect_input
                            .update(cx, |i, cx| i.set_value("0", cx));
                    }
                    KeepAliveStrategy::Save => {
                        this.session_keepalive_input
                            .update(cx, |i, cx| i.set_value("300", cx));
                        this.session_keepalive_max = 3;
                        this.session_idle_disconnect_input
                            .update(cx, |i, cx| i.set_value("300", cx));
                    }
                    KeepAliveStrategy::Minimal => {
                        this.session_keepalive_input
                            .update(cx, |i, cx| i.set_value("0", cx));
                        this.session_keepalive_max = 3;
                        this.session_idle_disconnect_input
                            .update(cx, |i, cx| i.set_value("0", cx));
                    }
                    KeepAliveStrategy::Custom => {}
                }
                this.session_keepalive_strategy = s;
                cx.notify();
            }))
            .child(label)
    }

    fn render_keepalive_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let strategy = self.session_keepalive_strategy;
        let desc = match strategy {
            KeepAliveStrategy::Always => i18n!(cx, "ssh.connection.keepalive.strategy.always.desc"),
            KeepAliveStrategy::Save => i18n!(cx, "ssh.connection.keepalive.strategy.save.desc"),
            KeepAliveStrategy::Minimal => {
                i18n!(cx, "ssh.connection.keepalive.strategy.minimal.desc")
            }
            KeepAliveStrategy::Custom => String::new(),
        };
        div()
            .flex()
            .flex_col()
            .gap(ICON_SM)
            .child(
                div()
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_secondary))
                    .child(i18n!(cx, "ssh.connection.keepalive.strategy.label")),
            )
            .child(
                h_flex()
                    .gap(SPACE_MD)
                    .child(self.keepalive_strategy_option(
                        cx,
                        KeepAliveStrategy::Always,
                        i18n!(cx, "ssh.connection.keepalive.strategy.always.label"),
                    ))
                    .child(self.keepalive_strategy_option(
                        cx,
                        KeepAliveStrategy::Save,
                        i18n!(cx, "ssh.connection.keepalive.strategy.save.label"),
                    ))
                    .child(self.keepalive_strategy_option(
                        cx,
                        KeepAliveStrategy::Minimal,
                        i18n!(cx, "ssh.connection.keepalive.strategy.minimal.label"),
                    ))
                    .child(self.keepalive_strategy_option(
                        cx,
                        KeepAliveStrategy::Custom,
                        i18n!(cx, "ssh.connection.keepalive.strategy.custom"),
                    )),
            )
            .when(strategy != KeepAliveStrategy::Custom, |d| {
                d.child(
                    div()
                        .text_size(ui_text_xs(cx))
                        .text_color(rgb(t.text_muted))
                        .child(desc),
                )
            })
            .when(strategy == KeepAliveStrategy::Custom, |d| {
                d.child(self.render_keepalive_custom_panel(cx))
            })
    }

    fn render_keepalive_custom_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let interval = self
            .session_keepalive_input
            .read(cx)
            .value()
            .trim()
            .parse::<u32>()
            .unwrap_or(0);
        let idle = self
            .session_idle_disconnect_input
            .read(cx)
            .value()
            .trim()
            .parse::<u32>()
            .unwrap_or(0);
        let probe_on = interval > 0;
        let idle_on = idle > 0;
        div()
            .flex()
            .flex_col()
            .gap(ICON_SM)
            .child(self.render_keepalive_toggle(
                cx,
                "kalive-probe-toggle",
                i18n!(cx, "ssh.connection.keepalive.enable_probe.label"),
                Some(i18n!(cx, "ssh.connection.keepalive.enable_probe.desc")),
                probe_on,
                |this: &mut Self, cx: &mut App| {
                    let on = this
                        .session_keepalive_input
                        .read(cx)
                        .value()
                        .trim()
                        .parse::<u32>()
                        .unwrap_or(0)
                        > 0;
                    this.session_keepalive_input
                        .update(cx, |i, cx| i.set_value(if on { "0" } else { "30" }, cx));
                    this.session_keepalive_strategy = KeepAliveStrategy::Custom;
                },
            ))
            .child(self.keepalive_labeled_input(
                cx,
                i18n!(cx, "ssh.connection.keepalive.interval.label"),
                &self.session_keepalive_input,
                Some(i18n!(cx, "ssh.connection.keepalive.interval.help")),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .text_color(rgb(t.text_secondary))
                            .child(i18n!(cx, "ssh.connection.keepalive.max_retries.label")),
                    )
                    .child(self.render_keep_alive_max_dropdown(cx))
                    .child(
                        div()
                            .text_size(ui_text_xs(cx))
                            .text_color(rgb(t.text_muted))
                            .child(i18n!(cx, "ssh.connection.keepalive.max_retries.help")),
                    ),
            )
            .child(
                div()
                    .text_size(ui_text_xs(cx))
                    .text_color(rgb(t.text_muted))
                    .child(format!(
                        "──── {} ────",
                        i18n!(cx, "ssh.connection.keepalive.advanced")
                    )),
            )
            .child(self.render_keepalive_toggle(
                cx,
                "kalive-idle-toggle",
                i18n!(cx, "ssh.connection.keepalive.idle_timeout.enable.label"),
                Some(i18n!(
                    cx,
                    "ssh.connection.keepalive.idle_timeout.enable.desc"
                )),
                idle_on,
                |this: &mut Self, cx: &mut App| {
                    let on = this
                        .session_idle_disconnect_input
                        .read(cx)
                        .value()
                        .trim()
                        .parse::<u32>()
                        .unwrap_or(0)
                        > 0;
                    this.session_idle_disconnect_input
                        .update(cx, |i, cx| i.set_value(if on { "0" } else { "300" }, cx));
                    this.session_keepalive_strategy = KeepAliveStrategy::Custom;
                },
            ))
            .child(self.keepalive_labeled_input(
                cx,
                i18n!(cx, "ssh.connection.keepalive.idle_timeout.label"),
                &self.session_idle_disconnect_input,
                Some(i18n!(cx, "ssh.connection.keepalive.idle_timeout.help")),
            ))
    }

    fn render_keepalive_toggle<F>(
        &self,
        cx: &mut Context<Self>,
        id: &'static str,
        label: String,
        desc: Option<String>,
        checked: bool,
        on_toggle: F,
    ) -> impl IntoElement
    where
        F: Fn(&mut Self, &mut App) + 'static,
    {
        let t = theme(cx);
        let mut row = div().flex().flex_col().gap(px(2.0)).child(
            velowork_ui::Checkbox::new(id)
                .label(label)
                .checked(checked)
                .on_click(cx.listener(move |this, _, _, cx| {
                    on_toggle(this, cx);
                })),
        );
        if let Some(text) = desc {
            row = row.child(
                div()
                    .pl(px(22.0))
                    .text_size(ui_text_xs(cx))
                    .text_color(rgb(t.text_muted))
                    .child(text),
            );
        }
        row
    }

    fn keepalive_labeled_input(
        &self,
        cx: &mut Context<Self>,
        label: String,
        input: &Entity<SimpleInputState>,
        help: Option<String>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let mut col = div()
            .flex()
            .flex_col()
            .gap(SPACE_XS)
            .child(
                div()
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.text_secondary))
                    .child(label),
            )
            .child(Self::input_field(input, cx));
        if let Some(text) = help {
            col = col.child(
                div()
                    .text_size(ui_text_xs(cx))
                    .text_color(rgb(t.text_muted))
                    .child(text),
            );
        }
        col
    }

    fn render_keep_alive_max_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = format!("{}", self.session_keepalive_max);
        let is_open = self.ssh_keep_alive_max_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("keepalive-max-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.keep_alive_max_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_keep_alive_max_dropdown_open = !this.ssh_keep_alive_max_dropdown_open;
                this.close_all_dropdowns_except("keep_alive_max");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_keep_alive_max_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        let options = vec![3, 5, 10, 20, 50];

        dropdown_overlay("keepalive-max-dropdown-list", &t, cx).children(options.into_iter().map(
            |val| {
                let is_selected = self.session_keepalive_max == val;
                dropdown_option(
                    format!("ka-max-{}", val),
                    &format!("{}", val),
                    is_selected,
                    &t,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.session_keepalive_max = val;
                    this.ssh_keep_alive_max_dropdown_open = false;
                    cx.notify();
                }))
            },
        ))
    }

    fn render_channel_buffer_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = if self.session_channel_buffer_size >= 1048576 {
            format!("{} MB", self.session_channel_buffer_size / 1048576)
        } else {
            format!("{} KB", self.session_channel_buffer_size / 1024)
        };
        let is_open = self.ssh_channel_buffer_dropdown_open;

        use crate::views::components::dropdown_button;
        dropdown_button("channel-buffer-btn", &label, is_open, &t, cx, {
            let this = cx.entity();
            move |bounds, _window, _cx| {
                this.update(_cx, |this, _cx| {
                    this.channel_buffer_dropdown_bounds = Some(bounds);
                });
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.ssh_channel_buffer_dropdown_open = !this.ssh_channel_buffer_dropdown_open;
                this.close_all_dropdowns_except("channel_buffer");
                cx.notify();
                cx.stop_propagation();
            }),
        )
    }

    fn render_channel_buffer_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        use crate::views::components::{dropdown_option, dropdown_overlay};

        let options = vec![
            (32768, "32 KB".to_string()),
            (65536, "64 KB".to_string()),
            (131072, "128 KB".to_string()),
            (262144, "256 KB".to_string()),
            (524288, "512 KB".to_string()),
            (1048576, "1 MB".to_string()),
        ];

        dropdown_overlay("channel-buffer-dropdown-list", &t, cx).children(options.into_iter().map(
            |(val, label)| {
                let is_selected = self.session_channel_buffer_size == val;
                dropdown_option(format!("chan-buf-{}", val), &label, is_selected, &t, cx).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.session_channel_buffer_size = val;
                        this.ssh_channel_buffer_dropdown_open = false;
                        cx.notify();
                    }),
                )
            },
        ))
    }

    // ─── Tab sidebar ─────────────────────────────────────────────────────────

    fn render_tab_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let active_tab = self.ssh_dialog_tab.clone();

        div()
            .w(px(170.0))
            .flex_shrink_0()
            .bg(surface_bg_t(t.bg_secondary, &t))
            .border_r_1()
            .border_color(rgb(t.border))
            .flex()
            .flex_col()
            .py(SPACE_MD)
            .children(SshDialogTab::all().iter().map(|tab| {
                let is_active = *tab == active_tab;
                let is_sub = tab.is_sub_tab();
                let label = i18n!(cx, tab.label_key());
                let icon = tab.icon();
                let tab_clone = tab.clone();

                h_flex()
                    .id(ElementId::Name(format!("tab-{:?}", tab).into()))
                    .px(ICON_SM)
                    .py(px(7.0))
                    .gap(SPACE_SM)
                    .items_center()
                    .cursor_pointer()
                    .rounded(RADIUS_STD)
                    .mx(SPACE_SM)
                    .when(is_sub, |d| d.ml(px(20.0)))
                    .when(is_active, |d| d.bg(surface_bg_t(t.bg_selection, &t)))
                    .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                    .child(icon.size(ICON_STD).text_color(rgb(if is_active {
                        t.text_primary
                    } else {
                        t.text_secondary
                    })))
                    .child(
                        div()
                            .text_size(ui_text_sm(cx))
                            .font_weight(if is_active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(rgb(if is_active {
                                t.text_primary
                            } else {
                                t.text_secondary
                            }))
                            .child(label),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.ssh_dialog_tab = tab_clone.clone();
                        this.close_all_dropdowns();
                        cx.notify();
                    }))
            }))
    }

    // ─── Test connection status ──────────────────────────────────────────────

    fn render_test_connection_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        match &self.test_connection_status {
            TestConnectionStatus::Idle => div(),
            TestConnectionStatus::Testing => div()
                .text_size(ui_text_sm(cx))
                .text_color(rgb(t.text_secondary))
                .child(i18n!(cx, "common.state.testing")),
            TestConnectionStatus::Success { latency_ms } => h_flex()
                .gap(SPACE_XS)
                .items_center()
                .child(AppIcon::Check.size(ICON_STD).text_color(rgb(t.success)))
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(rgb(t.success))
                        .child(format!(
                            "{} ({}ms)",
                            i18n!(cx, "ssh.action.test_success"),
                            latency_ms
                        )),
                ),
            TestConnectionStatus::Failed { error } => h_flex()
                .gap(SPACE_XS)
                .items_center()
                .child(AppIcon::Close.size(ICON_STD).text_color(rgb(t.error)))
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(rgb(t.error))
                        .child(format!("{}: {}", i18n!(cx, "status.test_failed"), error)),
                ),
        }
    }

    // ─── Main dialog overlay ─────────────────────────────────────────────────

    pub(crate) fn ssh_dialog(&self) -> Option<&SessionDialogModel> {
        match &self.session_dialog {
            Some(SessionDialogState::Session { model }) => Some(model.as_ref()),
            None => None,
        }
    }

    pub(crate) fn ssh_dialog_mut(&mut self) -> Option<&mut SessionDialogModel> {
        match &mut self.session_dialog {
            Some(SessionDialogState::Session { model }) => Some(model.as_mut()),
            None => None,
        }
    }

    pub(crate) fn open_add_session_dialog_model(
        &mut self,
        parent_id: Option<String>,
        protocol: Option<velowork_state::SessionProtocol>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.test_connection_status = TestConnectionStatus::Idle;
        let proto = protocol.unwrap_or(velowork_state::SessionProtocol::Ssh);
        self.init_protocol_dialog(proto, parent_id, None, window, cx);
    }

    pub(crate) fn open_edit_session_dialog_model(
        &mut self,
        session: SshSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.test_connection_status = TestConnectionStatus::Idle;
        let parent_id = session.parent_folder_id.clone();
        let proto = session.protocol;
        self.init_protocol_dialog(proto, parent_id, Some(session), window, cx);
    }

    pub(crate) fn init_protocol_dialog(
        &mut self,
        protocol: velowork_state::SessionProtocol,
        parent_id: Option<String>,
        edit: Option<SshSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut model = match edit {
            Some(s) => {
                let mut m = SessionDialogModel::new(window, cx);
                m.load(s, window, cx);
                m
            }
            None => {
                let mut m = SessionDialogModel::new(window, cx);
                m.config.protocol = protocol;
                m.config.parent_folder_id = parent_id;
                m.baseline = m.config.clone();
                m
            }
        };

        for &fid in FieldId::ALL {
            if let Some(ent) = model.input_for(fid).cloned() {
                cx.subscribe(
                    &ent,
                    move |this, _, event: &velowork_ui::input::InputEvent, cx| {
                        if matches!(event, velowork_ui::input::InputEvent::Blur | velowork_ui::input::InputEvent::PressEnter) {
                            this.dialog_on_field_changed(fid, cx);
                        }
                    },
                )
                .detach();
            }
        }
        let pid = cx.entity().entity_id();
        model.ui.panel_id = Some(pid);
        model.ui.panel_weak = Some(cx.entity().downgrade());
        self.dialog_panel_id = Some(pid);
        let ns = model.inputs.nav_search.clone();
        cx.subscribe(
            &ns,
            move |this, _, _: &velowork_ui::input::InputEvent, cx| {
                let v = this
                    .ssh_dialog()
                    .map(|m| m.inputs.nav_search.read(cx).text().to_string())
                    .unwrap_or_default();
                this.dialog_set_nav_search(v, cx);
            },
        )
        .detach();

        let picker = model.selects.serial_port_picker.clone();
        cx.subscribe(
            &picker,
            move |this, _, event: &velowork_ui::select::SelectEvent<SharedString>, cx| {
                if let velowork_ui::select::SelectEvent::Change(Some(val)) = event {
                    let port_str = val.to_string();
                    if let Some(m) = this.ssh_dialog_mut() {
                        m.inputs.serial_port.update(cx, |s, cx| s.set_value(&port_str, cx));
                        m.config.serial_port = Some(port_str);
                        m.validate_field(FieldId::SerialPort, cx);
                    }
                    this.dialog_notify(cx);
                }
            },
        )
        .detach();

        if protocol == velowork_state::SessionProtocol::Serial {
            model.refresh_detected_serial_ports(cx);
        }

        if let Some(reg) = self
            .overlay_registry
            .clone()
            .or_else(|| velowork_ui::overlay_registry::OverlayRegistry::global(cx))
        {
            model.set_overlay_registry(reg, cx);
        }
        let panel_entity = cx.entity().clone();
        model.subscribe_selects(panel_entity, cx);
        let active_pid = self.active_project_id(cx);
        model.sync_selects(&active_pid, cx);

        if self.dialog_previous_focus.is_none() {
            self.dialog_previous_focus = window
                .focused(cx)
                .or_else(|| Some(self.focus_handle.clone()));
        }
        self.session_dialog = Some(SessionDialogState::Session {
            model: Box::new(model),
        });
        let origin = self.dialog_origin.take();
        self.start_dialog_enter_animation(origin, cx);
    }

    pub fn dialog_refresh_serial_ports(&mut self, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            m.refresh_detected_serial_ports(cx);
            self.dialog_notify(cx);
        }
    }

    pub fn refresh_session_dialog_selects(&mut self, cx: &mut App) {
        if let Some(m) = self.ssh_dialog() {
            let active_pid = self.active_project_id(cx);
            m.sync_selects(&active_pid, cx);
        }
    }

    pub fn dialog_on_field_changed(&mut self, fid: FieldId, cx: &mut App) {
        if !matches!(self.test_connection_status, TestConnectionStatus::Idle) {
            self.test_connection_status = TestConnectionStatus::Idle;
        }
        let model = match self.ssh_dialog_mut() {
            Some(m) => m,
            None => return,
        };
        let old_validation = model.ui.validation.get(fid).cloned();
        model.validate_field(fid, cx);
        if fid == FieldId::SerialPort {
            model.sync_serial_port_picker_from_input(cx);
        }
        let new_validation = model.ui.validation.get(fid).cloned();
        let changed = old_validation != new_validation;
        if changed {
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_set_nav_search(&mut self, text: String, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            m.ui.nav_search = text.clone();
            if !text.is_empty() {
                let matched =
                    crate::views::overlays::dialogs::session_dialog::SectionRegistry::with_builtins()
                        .match_search(&text);
                if let Some(&first) = matched.first() {
                    m.ui.expanded_sections.insert(first);
                    m.ui.active_section = first;
                    m.ui.pending_scroll.set(Some(first));
                }
            }
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_toggle_section(&mut self, s: SshSection, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            m.toggle_section(s);
            m.ui.active_section = s;
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_nav_to(&mut self, s: SshSection, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            m.sync_config_from_inputs(cx);
            m.ui.expanded_sections.insert(s);
            m.ui.active_section = s;
            m.ui.scroll_handle.set_offset(gpui::point(gpui::px(0.0), gpui::px(0.0)));
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_set_auth_type(&mut self, at: SshAuthType, cx: &mut App) {
        self.test_connection_status = TestConnectionStatus::Idle;
        if let Some(m) = self.ssh_dialog_mut() {
            m.config.auth_type = at;
            m.recompute();
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_set_proxy_type(&mut self, pt: ProxyType, cx: &mut App) {
        self.test_connection_status = TestConnectionStatus::Idle;
        if let Some(m) = self.ssh_dialog_mut() {
            m.config.proxy_type = pt;
            if pt == ProxyType::None {
                m.config.proxy_host = None;
                m.config.proxy_port = None;
                m.config.proxy_username = None;
                m.config.proxy_password = None;
            }
            m.recompute();
            self.refresh_session_dialog_selects(cx);
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_toggle_terminal(&mut self, which: TerminalToggle, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            let toggle = |opt: &mut Option<bool>| {
                *opt = match *opt {
                    Some(true) => Some(false),
                    Some(false) => None,
                    None => Some(true),
                };
            };
            match which {
                TerminalToggle::ShellIntegration => {
                    toggle(&mut m.config.terminal.shell_integration)
                }
                TerminalToggle::BracketedPaste => toggle(&mut m.config.terminal.bracketed_paste),
                TerminalToggle::Osc52 => toggle(&mut m.config.terminal.osc52_clipboard),
                TerminalToggle::TrueColor => toggle(&mut m.config.terminal.true_color),
            }
            m.recompute();
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_set_cursor_shape(
        &mut self,
        shape: Option<velowork_core::types::CursorShape>,
        cx: &mut App,
    ) {
        if let Some(m) = self.ssh_dialog_mut() {
            m.config.terminal.cursor_shape = shape;
            m.recompute();
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_set_cursor_blink(&mut self, blink: Option<bool>, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            m.config.terminal.cursor_blink = blink;
            m.recompute();
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_set_bell_style(
        &mut self,
        style: Option<velowork_core::types::BellStyle>,
        cx: &mut App,
    ) {
        if let Some(m) = self.ssh_dialog_mut() {
            m.config.terminal.bell_style = style;
            m.recompute();
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_set_terminal_enhancement(
        &mut self,
        which: TerminalToggle,
        val: Option<bool>,
        cx: &mut App,
    ) {
        if let Some(m) = self.ssh_dialog_mut() {
            match which {
                TerminalToggle::ShellIntegration => m.config.terminal.shell_integration = val,
                TerminalToggle::BracketedPaste => m.config.terminal.bracketed_paste = val,
                TerminalToggle::Osc52 => m.config.terminal.osc52_clipboard = val,
                TerminalToggle::TrueColor => m.config.terminal.true_color = val,
            }
            m.recompute();
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_toggle_bool(&mut self, key: &str, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            match key {
                "tcp_nodelay" => m.config.tcp_nodelay = !m.config.tcp_nodelay,
                "algorithms_automatic" => {
                    m.config.algorithms_automatic = !m.config.algorithms_automatic
                }
                "enable_sftp" => m.config.enable_sftp = !m.config.enable_sftp,
                "enable_x11_forwarding" => {
                    m.config.enable_x11_forwarding = !m.config.enable_x11_forwarding
                }
                "enable_agent_forwarding" => {
                    m.config.enable_agent_forwarding = !m.config.enable_agent_forwarding
                }
                "enable_monitor" => m.config.enable_monitor = !m.config.enable_monitor,
                "monitor_cpu" => m.config.monitor_cpu = !m.config.monitor_cpu,
                "monitor_mem" => m.config.monitor_mem = !m.config.monitor_mem,
                "monitor_disk" => m.config.monitor_disk = !m.config.monitor_disk,
                "serial_dtr" => m.config.serial_dtr = !m.config.serial_dtr,
                "serial_rts" => m.config.serial_rts = !m.config.serial_rts,
                "serial_local_echo" => m.config.serial_local_echo = !m.config.serial_local_echo,
                "serial_timestamps" => m.config.serial_timestamps = !m.config.serial_timestamps,
                "serial_auto_reconnect" => {
                    m.config.serial_auto_reconnect = !m.config.serial_auto_reconnect
                }
                _ => {}
            }
            m.recompute();
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_toggle_algorithms_advanced(&mut self, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            m.ui.algorithms_advanced_open = !m.ui.algorithms_advanced_open;
            self.dialog_notify(cx);
        }
    }

    fn dialog_dropdown_overlay_id(key: &str) -> SharedString {
        SharedString::from(format!("session-dialog-dropdown-{key}"))
    }

    fn dialog_unregister_dropdown_overlay(&mut self, key: &str, cx: &mut App) {
        if let Some(reg) = self.overlay_registry.clone() {
            let id = Self::dialog_dropdown_overlay_id(key);
            reg.update(cx, |r, _| r.unregister(&id));
        }
    }

    fn dialog_cleanup_dropdown_overlay(&mut self, cx: &mut App) {
        let open_key = self.ssh_dialog().and_then(|m| m.ui.dropdown_open);
        if let Some(k) = open_key {
            self.dialog_unregister_dropdown_overlay(k, cx);
            if let Some(m) = self.ssh_dialog_mut() {
                m.ui.dropdown_open = None;
            }
        }
    }

    pub fn dialog_close_dropdown(&mut self, cx: &mut App) {
        let open_key = self.ssh_dialog().and_then(|m| m.ui.dropdown_open);
        if open_key.is_some() {
            if let Some(k) = open_key {
                self.dialog_unregister_dropdown_overlay(k, cx);
            }
            if let Some(m) = self.ssh_dialog_mut() {
                m.ui.dropdown_open = None;
            }
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_close_dropdown_state_only(&mut self, cx: &mut App) {
        let is_open = self.ssh_dialog().and_then(|m| m.ui.dropdown_open).is_some();
        if is_open {
            if let Some(m) = self.ssh_dialog_mut() {
                m.ui.dropdown_open = None;
            }
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_toggle_algorithm(&mut self, cat: &str, value: &str, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            let list: Option<&mut Vec<String>> = match cat {
                "kex" => Some(&mut m.config.kex_algorithms),
                "cipher" => Some(&mut m.config.cipher_algorithms),
                "mac" => Some(&mut m.config.mac_algorithms),
                "hostkey" | "host_key" => Some(&mut m.config.hostkey_algorithms),
                _ => None,
            };
            if let Some(list) = list {
                if let Some(pos) = list.iter().position(|s| s == value) {
                    list.remove(pos);
                } else {
                    list.push(value.to_string());
                }
                m.recompute();
                self.dialog_notify(cx);
            }
        }
    }

    pub fn dialog_move_algorithm(&mut self, cat: &str, value: &str, delta: isize, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            let list: Option<&mut Vec<String>> = match cat {
                "kex" => Some(&mut m.config.kex_algorithms),
                "cipher" => Some(&mut m.config.cipher_algorithms),
                "mac" => Some(&mut m.config.mac_algorithms),
                "hostkey" | "host_key" => Some(&mut m.config.hostkey_algorithms),
                _ => None,
            };
            if let Some(list) = list {
                let candidates =
                    crate::views::overlays::dialogs::session_dialog::algo_candidates(cat);
                let mut full_list: Vec<String> = list.clone();
                for cand in candidates {
                    if !full_list.iter().any(|s| s == cand) {
                        full_list.push(cand.to_string());
                    }
                }
                if let Some(idx) = full_list.iter().position(|s| s == value) {
                    let new_idx = idx as isize + delta;
                    if new_idx >= 0 && (new_idx as usize) < full_list.len() {
                        let target = new_idx as usize;
                        full_list.swap(idx, target);
                        let selected_set: std::collections::HashSet<String> =
                            list.iter().cloned().collect();
                        *list = full_list
                            .into_iter()
                            .filter(|s| selected_set.contains(s))
                            .collect();
                        m.recompute();
                        self.dialog_notify(cx);
                    }
                }
            }
        }
    }

    pub fn dialog_reset_algorithms(&mut self, cat: &str, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            let def_list =
                crate::views::overlays::dialogs::session_dialog::default_algorithms_for_cat(cat);
            match cat {
                "kex" => m.config.kex_algorithms = def_list,
                "cipher" => m.config.cipher_algorithms = def_list,
                "mac" => m.config.mac_algorithms = def_list,
                "hostkey" | "host_key" => m.config.hostkey_algorithms = def_list,
                _ => {}
            }
            m.recompute();
            self.dialog_notify(cx);
        }
    }

    pub fn open_ssh_key_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = i18n!(cx, "ssh.auth.browse_key");
        let prompt_opts = gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt.into()),
        };
        let paths_future = cx.prompt_for_paths(prompt_opts);
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(selected))) = paths_future.await
                && let Some(path) = selected.first().cloned()
            {
                let path_str = path.to_string_lossy().to_string();
                let _ = this.update(cx, |this, cx| {
                    this.dialog_set_key_path(&path_str, cx);
                });
            }
        })
        .detach();
    }

    pub fn dialog_set_key_path(&mut self, key_path: &str, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            m.inputs
                .key_path
                .update(cx, |s, cx| s.set_value(key_path, cx));
            if let SshAuthType::PrivateKey { key_path: kp, .. } = &mut m.config.auth_type {
                *kp = key_path.to_string();
            }
            m.validate_field(
                crate::views::overlays::dialogs::session_dialog::validation::FieldId::KeyPath,
                cx,
            );
            m.recompute();
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_select_dropdown(&mut self, key: &str, value: String, cx: &mut App) {
        if let Some(m) = self.ssh_dialog_mut() {
            match key {
                "terminal_type" => {
                    m.config.terminal.term_type = if value.is_empty() { None } else { Some(value) }
                }
                "charset" => {
                    m.config.terminal.charset = if value.is_empty() { None } else { Some(value) }
                }
                "font_family" => {
                    m.config.terminal.font_family =
                        if value.is_empty() { None } else { Some(value) }
                }
                "color_scheme" => {
                    m.config.terminal.color_scheme =
                        if value.is_empty() { None } else { Some(value) }
                }
                "cursor_shape" => {
                    m.config.terminal.cursor_shape = match value.as_str() {
                        "block" => Some(velowork_core::types::CursorShape::Block),
                        "bar" => Some(velowork_core::types::CursorShape::Bar),
                        "underline" => Some(velowork_core::types::CursorShape::Underline),
                        _ => None,
                    }
                }
                "cursor_blink" => {
                    m.config.terminal.cursor_blink = match value.as_str() {
                        "enable" => Some(true),
                        "disable" => Some(false),
                        _ => None,
                    }
                }
                "shell_integration" => {
                    m.config.terminal.shell_integration = match value.as_str() {
                        "enable" => Some(true),
                        "disable" => Some(false),
                        _ => None,
                    }
                }
                "bracketed_paste" => {
                    m.config.terminal.bracketed_paste = match value.as_str() {
                        "enable" => Some(true),
                        "disable" => Some(false),
                        _ => None,
                    }
                }
                "osc52_clipboard" => {
                    m.config.terminal.osc52_clipboard = match value.as_str() {
                        "enable" => Some(true),
                        "disable" => Some(false),
                        _ => None,
                    }
                }
                "true_color" => {
                    m.config.terminal.true_color = match value.as_str() {
                        "enable" => Some(true),
                        "disable" => Some(false),
                        _ => None,
                    }
                }
                "compression" => {
                    m.config.compression = match value.as_str() {
                        "Zlib" => CompressionType::Zlib,
                        "None" => CompressionType::None,
                        "ZlibOpenSsh" => CompressionType::ZlibOpenSsh,
                        _ => CompressionType::Zlib,
                    }
                }
                "strict_host" => {
                    m.config.strict_host_key = match value.as_str() {
                        "Yes" => StrictHostKey::Yes,
                        "No" => StrictHostKey::No,
                        "AcceptNew" => StrictHostKey::AcceptNew,
                        _ => StrictHostKey::Yes,
                    }
                }
                "proxy_type" => {
                    m.config.proxy_type = match value.as_str() {
                        "None" => ProxyType::None,
                        "Socks5" => ProxyType::Socks5,
                        "Http" => ProxyType::Http,
                        "Jump" => ProxyType::Jump,
                        _ => ProxyType::None,
                    }
                }
                "icon_color" => {
                    m.config.icon_color = match value.as_str() {
                        "Default" => IconColor::Default,
                        "Blue" => IconColor::Blue,
                        "Green" => IconColor::Green,
                        "Orange" => IconColor::Orange,
                        "Red" => IconColor::Red,
                        _ => IconColor::Default,
                    }
                }
                "parent_folder" => {
                    m.config.parent_folder_id = if value.is_empty() { None } else { Some(value) }
                }
                "jump_session" => {
                    m.config.jump_session_id = if value.is_empty() { None } else { Some(value) }
                }
                "serial_baud" => {
                    m.config.serial_baud_rate = value.parse().unwrap_or(m.config.serial_baud_rate)
                }
                "serial_data_bits" => {
                    m.config.serial_data_bits = value.parse().unwrap_or(m.config.serial_data_bits)
                }
                "serial_stop_bits" => {
                    m.config.serial_stop_bits = value.parse().unwrap_or(m.config.serial_stop_bits)
                }
                "serial_parity" => m.config.serial_parity = value,
                "serial_flow_control" => m.config.serial_flow_control = value,
                "serial_display_mode" => m.config.serial_display_mode = value,
                "serial_line_ending" => m.config.serial_line_ending = value,
                "telnet_encoding" => {
                    m.config.terminal.charset = if value.is_empty() { None } else { Some(value) }
                }
                "local_shell" => {
                    m.config.local_shell = if value.is_empty() { None } else { Some(value) }
                }
                _ => {}
            }
            let open_key = m.ui.dropdown_open;
            m.ui.dropdown_open = None;
            m.recompute();
            if let Some(k) = open_key {
                self.dialog_unregister_dropdown_overlay(k, cx);
            }
            self.dialog_notify(cx);
        }
    }

    pub fn dialog_save(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let state = match self.session_dialog.as_mut() {
            Some(s) => s,
            None => return,
        };

        let (session, target_parent_id, is_edit, session_id, name, parent_folder_id, editing_id) =
            match state {
                SessionDialogState::Session { model } => {
                    model.sync_config_from_inputs(cx);
                    if let Some(err) = model.validate_all(cx) {
                        model.ui.active_section = err.section();
                        model.ui.expanded_sections.insert(err.section());
                        model.ui.pending_scroll.set(Some(err.section()));
                        self.dialog_notify(cx);
                        return;
                    }

                    let name = model.config.name.trim().to_string();
                    let parent_folder_id = model.config.parent_folder_id.clone();
                    let editing_id = model.editing_id.clone();
                    let id = editing_id
                        .clone()
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                    let target_parent_id = parent_folder_id.clone();
                    let is_edit = model.editing_id.is_some();
                    let sess = model.to_session(id.clone());
                    (
                        sess,
                        target_parent_id,
                        is_edit,
                        id,
                        name,
                        parent_folder_id,
                        editing_id,
                    )
                }
            };

        if self.session_name_conflicts(
            cx,
            parent_folder_id.as_deref(),
            &name,
            editing_id.as_deref(),
        ) {
            if let Some(SessionDialogState::Session { model }) = self.session_dialog.as_mut() {
                model.ui.validation.set(
                    FieldId::Name,
                    Some(ValidationResult::error(
                        &i18n!(cx, "sftp.dialog.name_exists").replace("{name}", &name),
                    )),
                );
                model.ui.active_section = SshSection::Basic;
                model.ui.pending_scroll.set(Some(SshSection::Basic));
            }
            self.dialog_notify(cx);
            return;
        }

        let active_pid = self.active_project_id(cx);
        let store = cx
            .global::<velowork_workspace::stores::GlobalSessionStore>()
            .0
            .clone();
        store.update(cx, |store, cx| {
            if let Some(ref p_id) = target_parent_id {
                store.set_folder_collapsed_for_project(active_pid.as_deref(), p_id, false, cx);
            }
            if is_edit {
                store.update_session_for_project(active_pid.as_deref(), session, cx);
            } else {
                store.add_session_for_project(
                    active_pid.as_deref(),
                    target_parent_id.as_deref(),
                    session,
                    cx,
                );
            }
        });

        self.selected_node_ids.clear();
        self.selected_node_ids.insert(session_id.clone());
        self.selection_anchor = Some(session_id);
        self.dialog_cleanup_dropdown_overlay(cx);
        self.session_dialog = None;
        self.active_dialog = None;
        self.dialog_previous_focus = None;
        self.test_connection_status = TestConnectionStatus::Idle;
        self.focus_manager.update(cx, |fm, _| {
            let _ = fm.exit_modal();
        });
        if let Some(window) = window {
            self.focus_session_tree(window, cx);
        }
        self.dialog_notify(cx);
    }

    /// 包含窗口物理焦点回溯的弹窗关闭入口：
    /// 优先安全归还历史焦点，若原焦点已失效则兜底回弹至会话树自身。
    pub fn dialog_request_close_with_window(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(window) = window {
            let mut restored = false;
            if let Some(prev) = self.dialog_previous_focus.take() {
                window.focus(&prev, cx);
                restored = window.focused(cx).is_some();
            }
            if !restored {
                self.focus_session_tree(window, cx);
            }
        }
        self.dialog_request_close(cx);
    }

    pub fn dialog_request_close(&mut self, cx: &mut Context<Self>) {
        self.dialog_previous_focus = None;
        let enable_animations = crate::settings::settings_entity(cx)
            .read(cx)
            .settings
            .enable_animations;
        if enable_animations && self.dialog_motion_state.progress > 0.05 {
            let start_p = self.dialog_motion_state.progress;
            self.dialog_motion_state.start_closing();
            let total_dur = velowork_ui::motion::DURATION_MODAL_LEAVE;
            self.dialog_anim_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
                let start = std::time::Instant::now();
                loop {
                    let elapsed = start.elapsed();
                    let t = (elapsed.as_secs_f32() / total_dur.as_secs_f32()).min(1.0);
                    let progress = start_p * (1.0 - t);
                    let res = this.update(cx, |this, cx| {
                        this.dialog_motion_state.progress = progress;
                        cx.notify();
                    });
                    if res.is_err() || t >= 1.0 {
                        break;
                    }
                    smol::Timer::after(std::time::Duration::from_millis(8)).await;
                }
                let _ = this.update(cx, |this, cx| {
                    this.dialog_cleanup_dropdown_overlay(cx);
                    this.session_dialog = None;
                    this.active_dialog = None;
                    this.test_connection_status = TestConnectionStatus::Idle;
                    this.dialog_motion_state = velowork_ui::motion::ModalMotionState::default();
                    this.dialog_anim_task = None;
                    this.focus_manager.update(cx, |fm, _| {
                        let _ = fm.exit_modal();
                    });
                    cx.notify();
                });
            }));
            return;
        }
        self.dialog_cleanup_dropdown_overlay(cx);
        self.session_dialog = None;
        self.active_dialog = None;
        self.test_connection_status = TestConnectionStatus::Idle;
        self.dialog_motion_state = velowork_ui::motion::ModalMotionState::default();
        self.dialog_anim_task = None;
        self.focus_manager.update(cx, |fm, _| {
            let _ = fm.exit_modal();
        });
        cx.notify();
    }

    pub fn dialog_notify(&self, cx: &mut App) {
        if let Some(pid) = self.dialog_panel_id {
            cx.notify(pid);
        }
    }

    pub(crate) fn render_dialog_overlay(
        &mut self,
        dialog: &SessionPanelDialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let has_rounded_corners = crate::settings::has_rounded_window_corners(window, cx);
        let corner_radius = px(crate::settings::settings_entity(cx)
            .read(cx)
            .settings
            .window_corner_radius);

        let is_session = matches!(
            dialog,
            SessionPanelDialog::AddSession { .. } | SessionPanelDialog::EditSession { .. }
        );

        if is_session && self.session_dialog.is_none() {
            match dialog {
                SessionPanelDialog::EditSession { session } => {
                    self.open_edit_session_dialog_model(session.clone(), window, cx);
                }
                SessionPanelDialog::AddSession {
                    parent_id,
                    protocol,
                } => {
                    self.open_add_session_dialog_model(parent_id.clone(), *protocol, window, cx);
                }
                _ => {}
            }
        }

        let active_project_id = self.active_project_id(cx);

        // 新版「新建/编辑会话」多态对话框渲染
        if let Some(ref mut state) = self.session_dialog {
            let panel_entity = cx.entity();
            let win_size = window.viewport_size();
            let preferred_size = state.preferred_size(win_size);

            let card: AnyElement = match state {
                SessionDialogState::Session { model } => {
                    crate::views::overlays::dialogs::session_dialog::render_session_dialog(
                        model.as_mut(),
                        panel_entity.clone(),
                        &self.focus_handle,
                        &self.test_connection_status,
                        active_project_id.clone(),
                        window,
                        cx,
                    )
                }
            };

            let motion_values = self
                .dialog_motion_state
                .compute_card_values(win_size, preferred_size);

            let card_container = div()
                .id("session-dialog-card-container")
                .relative()
                .left(motion_values.offset.x)
                .top(motion_values.offset.y)
                .opacity(motion_values.card_opacity)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        window.focus(&this.focus_handle, cx);
                    }),
                )
                .child(card);

            let card_wrapper = div()
                .id("session-dialog-card-wrapper")
                .absolute()
                .inset_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(velowork_ui::tokens::SPACE_LG)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.dialog_request_close_with_window(Some(window), cx);
                    }),
                )
                .child(card_container);

            let backdrop = velowork_ui::overlay::modal_backdrop(
                "session-dialog-backdrop",
                &crate::theme::theme(cx),
                cx,
            )
            .opacity(motion_values.backdrop_opacity)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.dialog_request_close_with_window(Some(window), cx);
                }),
            );

            return div()
                .id("session-dialog-overlay-root")
                .occlude()
                .absolute()
                .inset_0()
                .size_full()
                .track_focus(&self.focus_handle)
                .key_context("SessionDialog")
                .on_key_down(
                    cx.listener(|this: &mut Self, event: &KeyDownEvent, window, cx| {
                        let key = event.keystroke.key.as_str();
                        if key == "tab" || key == "\t" {
                            let is_shift = event.keystroke.modifiers.shift;
                            if let Some(model) = this.ssh_dialog_mut() {
                                if model.cycle_focus(is_shift, window, cx) {
                                    this.dialog_notify(cx);
                                    cx.stop_propagation();
                                }
                            }
                        }
                    }),
                )
                .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                    cx.stop_propagation();
                    this.dialog_request_close_with_window(Some(window), cx);
                }))
                .child(backdrop)
                .child(card_wrapper)
                .into_any_element();
        }

        if self.focus_dialog_inputs.get() {
            match dialog {
                SessionPanelDialog::AddFolder { .. } | SessionPanelDialog::EditFolder { .. } => {
                    let focus_handle = self.folder_name_input.read(cx).focus_handle(cx);
                    window.focus(&focus_handle, cx);
                }
                SessionPanelDialog::AddSession { .. } | SessionPanelDialog::EditSession { .. } => {
                    let inputs = self.current_tab_inputs(cx);
                    if let Some(first) = inputs.first() {
                        let focus_handle = first.read(cx).focus_handle(cx);
                        window.focus(&focus_handle, cx);
                    }
                }
            }
            self.focus_dialog_inputs.set(false);
        }

        let t = theme(cx);
        let is_folder = matches!(
            dialog,
            SessionPanelDialog::AddFolder { .. } | SessionPanelDialog::EditFolder { .. }
        );
        let is_session = !is_folder;

        let header_title = match dialog {
            SessionPanelDialog::AddFolder { .. } => i18n!(cx, "dock.folder_name_placeholder"),
            SessionPanelDialog::EditFolder { .. } => i18n!(cx, "dock.folder_name_placeholder"),
            SessionPanelDialog::AddSession { .. } => {
                let proto = self
                    .ssh_dialog()
                    .map(|m| m.config.protocol)
                    .unwrap_or_default();
                crate::views::overlays::dialogs::session_dialog::session_dialog_title(
                    false, proto, cx,
                )
            }
            SessionPanelDialog::EditSession { .. } => {
                let proto = self
                    .ssh_dialog()
                    .map(|m| m.config.protocol)
                    .unwrap_or_default();
                crate::views::overlays::dialogs::session_dialog::session_dialog_title(
                    true, proto, cx,
                )
            }
        };

        // Collect dropdown overlays outside the dialog to avoid borrow issues
        let mut dropdown_overlays: Vec<AnyElement> = Vec::new();

        if is_session {
            // Auth method dropdown
            if self.ssh_auth_method_dropdown_open {
                if let Some(bounds) = self.auth_method_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_auth_method_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // Proxy type dropdown
            if self.ssh_proxy_type_dropdown_open {
                if let Some(bounds) = self.proxy_type_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_proxy_type_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // Terminal type dropdown
            if self.ssh_terminal_type_dropdown_open {
                if let Some(bounds) = self.terminal_type_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_terminal_type_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // Charset dropdown
            if self.ssh_charset_dropdown_open {
                if let Some(bounds) = self.charset_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay = dropdown_anchored_below(bounds, self.render_charset_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // Compression dropdown
            if self.ssh_compression_dropdown_open {
                if let Some(bounds) = self.compression_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_compression_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // Strict host key dropdown
            if self.ssh_strict_host_key_dropdown_open {
                if let Some(bounds) = self.strict_host_key_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_strict_host_key_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // Max packets dropdown
            if self.ssh_max_packets_dropdown_open {
                if let Some(bounds) = self.max_packets_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_max_packets_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // Recv window dropdown
            if self.ssh_recv_window_dropdown_open {
                if let Some(bounds) = self.recv_window_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_recv_window_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // Directory dropdown
            if self.ssh_folder_dropdown_open {
                if let Some(bounds) = self.folder_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_folder_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // Icon color dropdown
            if self.ssh_icon_color_dropdown_open {
                if let Some(bounds) = self.icon_color_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_icon_color_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // KeepAlive Max retry dropdown
            if self.ssh_keep_alive_max_dropdown_open {
                if let Some(bounds) = self.keep_alive_max_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_keep_alive_max_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // Channel buffer dropdown
            if self.ssh_channel_buffer_dropdown_open {
                if let Some(bounds) = self.channel_buffer_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_channel_buffer_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // GEX min dropdown
            if self.ssh_gex_min_dropdown_open {
                if let Some(bounds) = self.gex_min_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay = dropdown_anchored_below(bounds, self.render_gex_min_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // GEX preferred dropdown
            if self.ssh_gex_preferred_dropdown_open {
                if let Some(bounds) = self.gex_preferred_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay =
                        dropdown_anchored_below(bounds, self.render_gex_preferred_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
            // GEX max dropdown
            if self.ssh_gex_max_dropdown_open {
                if let Some(bounds) = self.gex_max_dropdown_bounds {
                    use crate::views::components::dropdown_anchored_below;
                    let overlay = dropdown_anchored_below(bounds, self.render_gex_max_overlay(cx));
                    dropdown_overlays.push(overlay.into_any_element());
                }
            }
        }

        let dialog_width = if is_session { px(680.0) } else { px(420.0) };
        let dialog_height = if is_session { Some(px(560.0)) } else { None };

        let content = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .track_focus(&self.focus_handle)
            .key_context("SessionDialog")
            .bg(rgba(0x000000aa))
            .when(has_rounded_corners, |d| {
                d.rounded(corner_radius).overflow_hidden()
            })
            .flex()
            .items_center()
            .justify_center()
            .p(SPACE_MD)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.handle_key(event, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_all_dropdowns();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if let Some(ref mut drag) = this.resize_dragging {
                    let current_y = event.position.y;
                    // First mousemove after drag starts: record the
                    // SessionPanel-relative anchor so subsequent delta
                    // calculations are consistent.
                    if drag.anchor_y.is_none() {
                        drag.anchor_y = Some(current_y);
                        return;
                    }
                    let delta_y = current_y - drag.anchor_y.unwrap();
                    let new_height = (drag.start_height + delta_y).max(px(60.0)).min(px(300.0));
                    match drag.target {
                        ResizeTarget::StartupCommand => {
                            this.startup_command_height = Some(new_height);
                        }
                        ResizeTarget::Notes => {
                            this.notes_height = Some(new_height);
                        }
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    if this.resize_dragging.is_some() {
                        this.resize_dragging = None;
                        cx.notify();
                    }
                }),
            )
            .on_scroll_wheel(|_, _, cx| {
                cx.stop_propagation();
            })
            .child(
                div()
                    .w(dialog_width)
                    .when_some(dialog_height, |d, h| d.h(h))
                    .bg(surface_bg_t(t.bg_primary, &t))
                    .border_1()
                    .border_color(rgb(t.border))
                    .rounded(px(6.0))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.close_all_dropdowns();
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    )
                    // Header
                    .child(
                        div()
                            .h(px(velowork_ui::tab_height(cx)))
                            .px(SPACE_LG)
                            .border_b_1()
                            .border_color(rgb(t.border))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(t.text_primary))
                                    .child(header_title),
                            )
                            .child(
                                velowork_ui::icon_button::icon_button(
                                    "close-dialog-btn",
                                    AppIcon::Close,
                                    &t,
                                    cx,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.cancel_dialog(Some(window), cx);
                                    },
                                )),
                            ),
                    )
                    // Body
                    .child(if is_folder {
                        self.render_folder_body(dialog, cx).into_any_element()
                    } else {
                        self.render_session_body(cx).into_any_element()
                    })
                    // Footer
                    .child(if is_folder {
                        self.render_folder_footer(cx).into_any_element()
                    } else {
                        self.render_session_footer(cx).into_any_element()
                    }),
            )
            .children(dropdown_overlays);

        content.into_any_element()
    }

    fn input_field(input: &Entity<SimpleInputState>, cx: &mut Context<Self>) -> Div {
        let t = theme(cx);
        let multiline = input.read(cx).is_multiline();
        div()
            .w_full()
            .bg(surface_bg_t(t.bg_secondary, &t))
            .border_1()
            .border_color(rgb(t.border))
            .input_focus_ring(input, &t, cx)
            .rounded(RADIUS_STD)
            .overflow_hidden()
            .when(multiline, |d| d.p(SPACE_XS))
            .when(!multiline, |d| d.h(px(28.0)))
            .child(SimpleInput::new(input).text_size(ui_text_md(cx)))
    }

    fn render_folder_body(
        &self,
        _dialog: &SessionPanelDialog,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);

        div().p(ICON_SM).flex().flex_col().gap(SPACE_MD).child(
            div()
                .flex()
                .flex_col()
                .gap(SPACE_XS)
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(rgb(t.text_secondary))
                        .child(i18n!(cx, "dock.folder_name_placeholder")),
                )
                .child(Self::input_field(&self.folder_name_input, cx)),
        )
    }

    fn render_folder_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        div()
            .p(SPACE_MD)
            .border_t_1()
            .border_color(rgb(t.border))
            .flex()
            .justify_end()
            .gap(SPACE_MD)
            .child(
                button("cancel-btn", i18n!(cx, "common.action.cancel"), &t)
                    .text_size(ui_text_md(cx))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_dialog(Some(window), cx);
                    })),
            )
            .child(
                button_primary("save-btn", i18n!(cx, "common.action.save"), &t)
                    .text_size(ui_text_md(cx))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.submit_dialog(Some(window), cx);
                    })),
            )
    }

    fn render_session_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .overflow_hidden()
            .child(self.render_tab_sidebar(cx))
            .child(
                // Outer wrapper owns the horizontal flex-grow so it fills the
                // remaining row width next to the sidebar. The inner scrollable
                // element MUST have an explicit `size_full()` because
                // `Scrollable` only copies `element.style().size` to its inner
                // container (it drops flex-grow/shrink/basis). Using `flex_1()`
                // directly on the scrollable would leave the container at
                // `width: auto`, collapsing the form fields to their intrinsic
                // width and pushing the scrollbar into the middle of the row.
                div().flex_1().overflow_hidden().child(
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .overflow_y_scrollbar()
                        .p(SPACE_XL)
                        .child(match self.ssh_dialog_tab {
                            SshDialogTab::General => self.render_tab_general(cx).into_any_element(),
                            SshDialogTab::Authentication => {
                                self.render_tab_authentication(cx).into_any_element()
                            }
                            SshDialogTab::Connection => {
                                self.render_tab_connection(cx).into_any_element()
                            }
                            SshDialogTab::Terminal => {
                                self.render_tab_terminal(cx).into_any_element()
                            }
                            SshDialogTab::Advanced => {
                                self.render_tab_advanced(cx).into_any_element()
                            }
                            SshDialogTab::Kex => self.render_tab_russh_kex(cx).into_any_element(),
                            SshDialogTab::Cipher => {
                                self.render_tab_russh_cipher(cx).into_any_element()
                            }
                            SshDialogTab::Mac => self.render_tab_russh_mac(cx).into_any_element(),
                            SshDialogTab::Hostkey => {
                                self.render_tab_russh_hostkey(cx).into_any_element()
                            }
                        }),
                ),
            )
    }

    fn render_session_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let is_testing = matches!(self.test_connection_status, TestConnectionStatus::Testing);
        let has_error = self.validation_error.is_some();

        div()
            .px(SPACE_LG)
            .py(ICON_SM)
            .border_t_1()
            .border_color(rgb(t.border))
            .flex()
            .flex_col()
            .gap(SPACE_MD)
            // Validation error message
            .when(has_error, |d| {
                d.child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(rgb(t.error))
                        .child(self.validation_error.clone().unwrap_or_default()),
                )
            })
            // Status and buttons
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.render_test_connection_status(cx))
                    .child(
                        h_flex()
                            .gap(SPACE_MD)
                            .child(
                                button("cancel-btn", i18n!(cx, "common.action.cancel"), &t)
                                    .text_size(ui_text_md(cx))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.cancel_dialog(Some(window), cx);
                                    })),
                            )
                            .child({
                                let label = if is_testing {
                                    i18n!(cx, "common.state.testing")
                                } else {
                                    i18n!(cx, "ssh.action.test_connection")
                                };
                                div()
                                    .id("test-connection-btn")
                                    .cursor_pointer()
                                    .px(SPACE_LG)
                                    .py(SPACE_SM)
                                    .rounded(RADIUS_STD)
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .text_size(ui_text_md(cx))
                                    .text_color(rgb(t.text_primary))
                                    .bg(surface_bg_t(t.bg_primary, &t))
                                    .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                                    .flex()
                                    .items_center()
                                    .child(label)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        log::info!("Test connection button clicked");
                                        this.test_connection(cx);
                                    }))
                            })
                            .child({
                                div()
                                    .id("connect-btn")
                                    .cursor_pointer()
                                    .px(SPACE_LG)
                                    .py(SPACE_SM)
                                    .rounded(RADIUS_STD)
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .text_size(ui_text_md(cx))
                                    .text_color(rgb(t.text_primary))
                                    .bg(surface_bg_t(t.bg_primary, &t))
                                    .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                                    .flex()
                                    .items_center()
                                    .child(i18n!(cx, "common.action.connect"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        log::info!("Connect button clicked");
                                        this.submit_and_connect(Some(window), cx);
                                    }))
                            })
                            .child(
                                button_primary("save-btn", i18n!(cx, "common.action.save"), &t)
                                    .text_size(ui_text_md(cx))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        log::info!("Save button clicked");
                                        this.submit_dialog(Some(window), cx);
                                    })),
                            ),
                    ),
            )
    }
}

fn filter_tree_by_search(node: &SessionTreeNode, query: &str) -> Option<SessionTreeNode> {
    if query.is_empty() {
        return Some(node.clone());
    }
    let q = query.to_lowercase();
    match node {
        SessionTreeNode::Session { session } => {
            if session.name.to_lowercase().contains(&q) || session.host.to_lowercase().contains(&q)
            {
                Some(node.clone())
            } else {
                None
            }
        }
        SessionTreeNode::Folder {
            id,
            name,
            children,
            is_collapsed: _,
        } => {
            if name.to_lowercase().contains(&q) {
                Some(SessionTreeNode::Folder {
                    id: id.clone(),
                    name: name.clone(),
                    children: children.clone(),
                    is_collapsed: false,
                })
            } else {
                let filtered_children: Vec<_> = children
                    .iter()
                    .filter_map(|child| filter_tree_by_search(child, query))
                    .collect();
                if !filtered_children.is_empty() {
                    Some(SessionTreeNode::Folder {
                        id: id.clone(),
                        name: name.clone(),
                        children: filtered_children,
                        is_collapsed: false,
                    })
                } else {
                    None
                }
            }
        }
    }
}

impl Focusable for SessionPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SessionPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        if self.show_search && self.search_input.is_none() {
            let input = cx
                .new(|cx| InputState::new(cx).placeholder(i18n!(cx, "common.search_placeholder")));
            let input_clone = input.clone();
            cx.subscribe(
                &input_clone,
                |_this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                    cx.notify();
                },
            )
            .detach();
            self.search_input = Some(input);
        }

        let store = cx
            .global::<velowork_workspace::stores::GlobalSessionStore>()
            .0
            .read(cx);
        let ssh_tree = self.active_tree(store, cx).to_vec();

        let this_weak = cx.entity().downgrade();
        let project_selector_canvas = canvas(
            move |bounds, _, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, _| {
                        if this.project_selector_bounds != bounds {
                            this.project_selector_bounds = bounds;
                        }
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let this_weak = cx.entity().downgrade();
        let add_session_btn_canvas = canvas(
            move |bounds, _, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, _| {
                        if this.add_session_button_bounds != bounds {
                            this.add_session_button_bounds = bounds;
                        }
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let this_weak_settings = cx.entity().downgrade();
        let settings_btn_canvas = canvas(
            move |bounds, _, cx| {
                if let Some(this) = this_weak_settings.upgrade() {
                    this.update(cx, |this, _| {
                        if this.settings_button_bounds != bounds {
                            this.settings_button_bounds = bounds;
                        }
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let open_project_menu = cx.listener(|this, _, window, cx| {
            this.open_project_menu(window, cx);
        });

        let open_settings_menu = cx.listener(|this, _, window, cx| {
            this.open_settings_menu(window, cx);
        });

        let query = self
            .search_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string().trim().to_string())
            .unwrap_or_default();
        let filtered_tree: Vec<_> = ssh_tree
            .iter()
            .filter_map(|node| filter_tree_by_search(node, &query))
            .collect();

        // Record the flat visible node order for Shift-range selection / batch open.
        let mut visible_order = Vec::new();
        Self::collect_visible_ids(&filtered_tree, &mut visible_order);
        self.visible_order = visible_order;

        let (active_project_name, active_project_icon, active_project_color) = {
            let active_project_id = self.active_project_id(cx);
            active_project_id
                .and_then(|pid| {
                    self.workspace
                        .read(cx)
                        .project(&pid)
                        .map(|p| (p.name.clone(), p.icon.clone(), p.folder_color))
                })
                .map(|(name, icon, folder_color)| {
                    let icon = AppIcon::from_str(&icon).unwrap_or(AppIcon::Folder);
                    let color = t.get_folder_color(folder_color);
                    (name, icon, color)
                })
                .unwrap_or_else(|| {
                    (
                        "Default".to_string(),
                        AppIcon::Folder,
                        t.get_folder_color(Default::default()),
                    )
                })
        };

        let header = h_flex()
            .h(px(velowork_ui::tab_height(cx)))
            .px(ui_space_xs(cx))
            .border_b_1()
            .border_color(p.border_subtle)
            .items_center()
            .gap(ui_space_xs(cx))
            .child(
                div().relative().child(add_session_btn_canvas).child(
                    velowork_ui::icon_button::icon_button(
                        "add-root-session-hdr",
                        AppIcon::Plus,
                        &t,
                        cx,
                    )
                    .on_hover(cx.listener(|this, hovered: &bool, window, cx| {
                        if *hovered {
                            if this.active_dialog.is_none() {
                                if let Some(ref menu) = this.active_menu {
                                    menu.update(cx, |m, cx| m.set_trigger_hovered(true, cx));
                                } else {
                                    this.open_add_session_menu(window, cx);
                                    if let Some(ref menu) = this.active_menu {
                                        menu.update(cx, |m, cx| m.set_trigger_hovered(true, cx));
                                    }
                                }
                            }
                        } else if let Some(ref menu) = this.active_menu {
                            menu.update(cx, |m, cx| m.set_trigger_hovered(false, cx));
                        }
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.active_menu.is_none() {
                            this.open_add_session_menu(window, cx);
                        }
                    }))
                    .tooltip(move |_, cx| {
                        let __tip = i18n!(cx, "session.new_session");
                        cx.new(|_| Tooltip::new(__tip)).into()
                    }),
                ),
            )
            .child(
                velowork_ui::icon_button::icon_button(
                    "add-root-folder-hdr",
                    AppIcon::NewFolder,
                    &t,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.inline_create_folder(None, window, cx);
                }))
                .tooltip(move |_, cx| {
                    let __tip = i18n!(cx, "common.new_folder");
                    cx.new(|_| Tooltip::new(__tip)).into()
                }),
            )
            .child(
                velowork_ui::icon_button::icon_button(
                    "import-sessions-hdr",
                    AppIcon::FolderInput,
                    &t,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_import_sessions_dialog(cx);
                }))
                .tooltip(move |_, cx| {
                    let __tip = i18n!(cx, "session.import.title");
                    cx.new(|_| Tooltip::new(__tip)).into()
                }),
            )
            .child(
                div()
                    .w(px(1.0))
                    .h(ICON_STD)
                    .bg(p.border_subtle)
                    .mx(ui_space_xs(cx)),
            )
            .child(
                velowork_ui::icon_button::icon_button("toggle-search-hdr", AppIcon::Search, &t, cx)
                    .when(self.show_search, |b| b.bg(surface_bg(t.bg_hover, cx)))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_search = !this.show_search;
                        if !this.show_search {
                            if let Some(ref input) = this.search_input {
                                input.update(cx, |input, cx| {
                                    input.set_value("", cx);
                                });
                            }
                        } else {
                            let input = this.search_input.get_or_insert_with(|| {
                                let input = cx.new(|cx| {
                                    InputState::new(cx)
                                        .placeholder(i18n!(cx, "common.search_placeholder"))
                                });
                                let input_clone = input.clone();
                                cx.subscribe(
                                    &input_clone,
                                    |_this: &mut Self,
                                     _,
                                     _: &velowork_ui::input::InputEvent,
                                     cx| {
                                        cx.notify();
                                    },
                                )
                                .detach();
                                input
                            });
                            input.update(cx, |input, cx| {
                                input.focus(window, cx);
                                input.select_all(cx);
                            });
                        }
                        cx.notify();
                    }))
                    .tooltip(move |_, cx| {
                        let __tip = i18n!(cx, "session.search");
                        cx.new(|_| Tooltip::new(__tip)).into()
                    }),
            )
            .child(
                velowork_ui::icon_button::icon_button("expand-all-hdr", AppIcon::ExpandFolder, &t, cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_all_folders_collapsed(false, cx);
                    }))
                    .tooltip(move |_, cx| {
                        let __tip = i18n!(cx, "session.expand_all");
                        cx.new(|_| Tooltip::new(__tip)).into()
                    }),
            )
            .child(
                velowork_ui::icon_button::icon_button(
                    "collapse-all-hdr",
                    AppIcon::CollapseFolder,
                    &t,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_all_folders_collapsed(true, cx);
                }))
                .tooltip(move |_, cx| {
                    let __tip = i18n!(cx, "session.collapse_all");
                    cx.new(|_| Tooltip::new(__tip)).into()
                }),
            );

        let search_bar = if self.show_search {
            self.search_input.as_ref().map(|input| {
                div()
                    .px(ui_space_xs(cx))
                    .py(ui_space_sm(cx))
                    .child(Input::new(input).search(true))
            })
        } else {
            None
        };

        let active_menu = self.active_menu.clone();

        let settings_tooltip = i18n!(cx, "settings.title");

        let settings_btn_size = px(24.0) * ui_text_scale(cx);

        let settings_trigger = div()
            .id("settings-btn")
            .group("settings-btn")
            .relative()
            .flex_shrink_0()
            .w(settings_btn_size)
            .h(settings_btn_size)
            .flex()
            .items_center()
            .justify_center()
            .rounded(RADIUS_STD)
            .cursor_pointer()
            .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
            .child(
                AppIcon::Settings
                    .size(ui_icon_std_ts(cx))
                    .text_color(rgb(t.text_secondary))
                    .group_hover("settings-btn", |s| s.text_color(rgb(t.text_primary))),
            )
            .on_mouse_down(MouseButton::Left, open_settings_menu)
            .child(settings_btn_canvas);

        let settings_btn = settings_trigger.tooltip(move |_, cx| {
            cx.new(|_| Tooltip::new(settings_tooltip.clone()).direction(TooltipDirection::Right))
                .into()
        });

        let active_project_tooltip = active_project_name.clone();

        let footer_toolbar = h_flex()
            .h(px(velowork_ui::tab_height(cx)))
            .px(ui_space_xs(cx))
            .gap(ui_space_xs(cx))
            .items_center()
            .bg(surface_bg(t.bg_secondary, cx))
            .child(
                h_flex()
                    .id("project-selector-trigger")
                    .group("project-selector-trigger")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h(settings_btn_size)
                    .items_center()
                    .gap(ui_space_xs(cx))
                    .px(ui_space_xs(cx))
                    .rounded(RADIUS_STD)
                    .cursor_pointer()
                    .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
                    .on_mouse_down(MouseButton::Left, open_project_menu)
                    .child(
                        active_project_icon
                            .size(ui_icon_std_ts(cx))
                            .text_color(rgb(active_project_color)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(t.text_secondary))
                            .group_hover("project-selector-trigger", |s| s.text_color(rgb(t.text_primary)))
                            .truncate()
                            .child(active_project_name),
                    )
                    .child(
                        AppIcon::ChevronDown
                            .size(ui_icon_std_ts(cx))
                            .text_color(rgb(t.text_secondary))
                            .group_hover("project-selector-trigger", |s| s.text_color(rgb(t.text_primary))),
                    )
                    .child(project_selector_canvas)
                    .tooltip(move |_, cx| {
                        cx.new(|_| Tooltip::new(active_project_tooltip.clone()).direction(TooltipDirection::Top))
                            .into()
                    }),
            )
            .child(settings_btn);

        // Render tree using velowork_ui::Tree component
        let tree_elements = self.render_tree_node(&filtered_tree, &t, window, cx);

        let active_context_menu = self.render_tree_context_menu(cx);

        let is_custom_titlebar = crate::settings::settings_entity(cx)
            .read(cx)
            .settings
            .titlebar_style
            == velowork_workspace::settings::TitlebarStyle::Custom;
        let window_corner_radius = crate::settings::settings_entity(cx)
            .read(cx)
            .settings
            .window_corner_radius;
        let is_maximized = window.is_maximized();
        let is_fullscreen = window.is_fullscreen();
        let has_rounded_corners =
            is_custom_titlebar && !is_maximized && !is_fullscreen && window_corner_radius > 0.0;
        let radius = px(window_corner_radius);

        div()
            .track_focus(&self.focus_handle)
            .key_context("SessionPanel")
            .id("session-manager-panel")
            .size_full()
            .overflow_hidden()
            .when(has_rounded_corners, |d| d.rounded_bl(radius))
            .relative()
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !this.focus_handle.is_focused(window) {
                        window.focus(&this.focus_handle, cx);
                    }
                    this.focus_manager.update(cx, |fm, _| {
                        fm.request_focus(velowork_workspace::focus::FocusLayer::SessionTree);
                    });
                }),
            )
            // 面板内语义 Action：F2 重命名当前选中节点（文件夹或会话）。
            // 仅在 SessionPanel 的 KeyContext 下生效，不污染其他面板。
            .on_action(cx.listener(|this, _: &RenameActiveNode, window, cx| {
                if this.inline_folder.is_none() && this.inline_session.is_none()
                    && !this.selected_node_ids.is_empty()
                {
                    this.rename_selected_node(window, cx);
                }
            }))
            // Keyboard: Enter opens all selected nodes, Escape clears selection.
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // 行内重命名输入框正在编辑时，方向键/空格等由输入框自行处理，
                // 不要让面板级快捷键插手。
                if this.inline_folder.is_some() || this.inline_session.is_some() {
                    return;
                }
                // Ignore while a dialog, context menu, or delete-confirm owns
                // the interaction (the overlay handles its own keys).
                if this.active_dialog.is_some()
                    || this.context_menu.is_some()
                    || this.overlay_manager.as_ref().is_some_and(|om| om.read(cx).has_modal())
                {
                    return;
                }
                let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                if cmd_or_ctrl && event.keystroke.key.as_str() == "f" {
                    this.show_search = true;
                    let input = this.search_input.get_or_insert_with(|| {
                        let input = cx.new(|cx| {
                            InputState::new(cx)
                                .placeholder(i18n!(cx, "common.search_placeholder"))
                        });
                        let input_clone = input.clone();
                        cx.subscribe(
                            &input_clone,
                            |_this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                                cx.notify();
                            },
                        )
                        .detach();
                        input
                    });
                    input.update(cx, |inp, cx| {
                        inp.focus(window, cx);
                        inp.select_all(cx);
                    });
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }

                match event.keystroke.key.as_str() {
                    // 方向键导航：自动选中首项或上下移动焦点
                    "down" | "up" => {
                        if !this.focus_handle.is_focused(window) {
                            return;
                        }
                        if this.selected_node_ids.is_empty() {
                            if let Some(first_id) = this.visible_order.first().cloned() {
                                this.selected_node_ids.insert(first_id);
                                this.focused_index = Some(0);
                                cx.notify();
                            }
                            cx.stop_propagation();
                            return;
                        }
                        this.move_focus(
                            if event.keystroke.key == "down" { 1 } else { -1 },
                            cx,
                        );
                        cx.stop_propagation();
                    }
                    "right" => {
                        if !this.focus_handle.is_focused(window) {
                            return;
                        }
                        if let Some(id) = this.selected_node_ids.iter().next().cloned() {
                            let is_folder = {
                                let store = cx
                                    .global::<velowork_workspace::stores::GlobalSessionStore>()
                                    .0
                                    .read(cx);
                                let tree = this.active_tree(store, cx);
                                matches!(this.find_node_by_id(tree, &id), Some(SessionTreeNode::Folder { .. }))
                            };
                            if is_folder {
                                this.toggle_folder(&id, cx);
                            } else {
                                this.move_focus(1, cx);
                            }
                            cx.stop_propagation();
                        }
                    }
                    "left" => {
                        if !this.focus_handle.is_focused(window) {
                            return;
                        }
                        if let Some(id) = this.selected_node_ids.iter().next().cloned() {
                            let is_folder = {
                                let store = cx
                                    .global::<velowork_workspace::stores::GlobalSessionStore>()
                                    .0
                                    .read(cx);
                                let tree = this.active_tree(store, cx);
                                matches!(this.find_node_by_id(tree, &id), Some(SessionTreeNode::Folder { .. }))
                            };
                            if is_folder {
                                this.toggle_folder(&id, cx);
                            } else {
                                this.move_focus(-1, cx);
                            }
                            cx.stop_propagation();
                        }
                    }
                    // 空格：展开/收起文件夹，或激活会话
                    "space" => {
                        if !this.focus_handle.is_focused(window) {
                            return;
                        }
                        if this.selected_node_ids.is_empty() {
                            if let Some(first_id) = this.visible_order.first().cloned() {
                                this.selected_node_ids.insert(first_id);
                                this.focused_index = Some(0);
                                cx.notify();
                            }
                            cx.stop_propagation();
                            return;
                        }
                        this.activate_focused(cx);
                        cx.stop_propagation();
                    }
                    "enter" => {
                        if !this.selected_node_ids.is_empty() {
                            let single_selected = if this.selected_node_ids.len() == 1 {
                                this.selected_node_ids.iter().next().cloned()
                            } else {
                                None
                            };
                            if let Some(id) = single_selected {
                                let is_folder = {
                                    let store = cx
                                        .global::<velowork_workspace::stores::GlobalSessionStore>()
                                        .0
                                        .read(cx);
                                    let tree = this.active_tree(store, cx);
                                    matches!(this.find_node_by_id(tree, &id), Some(SessionTreeNode::Folder { .. }))
                                };
                                if is_folder {
                                    this.toggle_folder(&id, cx);
                                    cx.stop_propagation();
                                    return;
                                }
                            }
                            this.open_selected_nodes(Some(window), cx);
                            cx.stop_propagation();
                        }
                    }
                    "escape" => {
                        if this.show_search {
                            this.show_search = false;
                            if let Some(ref input) = this.search_input {
                                input.update(cx, |inp, cx| inp.set_value("", cx));
                            }
                            window.focus(&this.focus_handle, cx);
                            cx.stop_propagation();
                            cx.notify();
                            return;
                        }
                        if !this.selected_node_ids.is_empty() {
                            this.clear_selection(cx);
                            cx.stop_propagation();
                        }
                    }
                    // Delete：删除当前选中的所有节点（弹窗确认）。
                    "delete" => {
                        if !this.selected_node_ids.is_empty() {
                            this.delete_selected_nodes(window, cx);
                            cx.stop_propagation();
                        }
                    }
                    _ => {}
                }
            }))
            .child(header)
            .children(search_bar)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("session-tree")
                            .size_full()
                            .track_scroll(&self.tree_scroll_handle)
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .py(ui_space_xs(cx))
                            .px(ui_space_xs(cx))
                            // Clicking the blank area of the tree clears the selection.
                            // Node rows call cx.stop_propagation() so their clicks don't reach here.
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.clear_selection(cx);
                            }))
                            // Right-clicking the blank area opens the blank-area context menu
                            // (create actions only). Node rows stop propagation, so a right-click
                            // on a node never reaches this handler — the two triggers stay separate.
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    this.open_tree_context_menu(
                                        event.position,
                                        String::new(),
                                        String::new(),
                                        false,
                                        true,
                                        window,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                }),
                            )
                            .children(tree_elements)
                            // 树底部常驻的根级放置区：拖拽节点（会话或目录）悬停于此并释放，
                            // 即把该节点移回顶层（根目录）。这样把一个节点拖入某目录后，仍有
                            // 明确的目标可将其拖出，避免"拖进去就拖不出来"的问题。高亮反馈与
                            // 目录悬停一致。
                            .child({
                                let root_drop_bg = t.bg_hover;
                                div()
                                    .id("session-tree-dropzone")
                                    .flex_1()
                                    .min_h(px(24.0))
                                    .drag_over::<SessionTreeSessionDrag>(move |style, _, _, _| {
                                        style.bg(rgb(root_drop_bg))
                                    })
                                    .drag_over::<SessionTreeFolderDrag>(move |style, _, _, _| {
                                        style.bg(rgb(root_drop_bg))
                                    })
                                    .on_drop(cx.listener(
                                        move |this, drag: &SessionTreeSessionDrag, _window, cx| {
                                            let store = cx
                                                .global::<velowork_workspace::stores::GlobalSessionStore>()
                                                .0
                                                .clone();
                                            let active_pid = this.active_project_id(cx);
                                            store.update(cx, |store, cx| {
                                                store.move_node_for_project(
                                                    active_pid.as_deref(),
                                                    &drag.session_id,
                                                    None,
                                                    usize::MAX,
                                                    cx,
                                                );
                                            });
                                        },
                                    ))
                                    .on_drop(cx.listener(
                                        move |this, drag: &SessionTreeFolderDrag, _window, cx| {
                                            let store = cx
                                                .global::<velowork_workspace::stores::GlobalSessionStore>()
                                                .0
                                                .clone();
                                            let active_pid = this.active_project_id(cx);
                                            store.update(cx, |store, cx| {
                                                store.move_node_for_project(
                                                    active_pid.as_deref(),
                                                    &drag.folder_id,
                                                    None,
                                                    usize::MAX,
                                                    cx,
                                                );
                                            });
                                        },
                                    ))
                            }),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .left_0()
                            .child(Scrollbar::vertical(&self.tree_scroll_handle)),
                    ),
            )
            .child(footer_toolbar)
            .when_some(active_menu, |this, menu| this.child(menu))
            .when_some(active_context_menu, |this, menu| this.child(menu))
    }
}

impl velowork_ui::dock::Panel for SessionPanel {
    fn metadata(&self, cx: &App) -> velowork_ui::dock::PanelInfo {
        let title = i18n!(cx, "dock.explorer");
        let title_str = if title.is_empty() {
            "Explorer".to_string()
        } else {
            title
        };
        velowork_ui::dock::PanelInfo::new(
            "explorer",
            title_str,
            velowork_ui::icon::AppIcon::Folder,
            velowork_ui::dock::PanelKind::Files,
        )
        .closable(false)
    }

    fn focus_handle(&self, _cx: &App) -> Option<FocusHandle> {
        Some(self.focus_handle.clone())
    }
}
