use super::file_row::{
    ColumnWidths, DisplayFile, file_row, format_owner, format_size, format_time, parent_row,
};
use super::url_detector::UrlDetector;
use crate::transfer_store::{
    GlobalTransferStore, TransferDirection, TransferProgress, TransferStatus, TransferTask,
    next_transfer_id,
};
use gpui::prelude::*;
use gpui::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use velowork_core::theme::ThemeColors;
use velowork_i18n::i18n;
use velowork_terminal::TerminalsRegistry;
use velowork_terminal::backend::TerminalBackend;
use velowork_ui::button::{button, button_primary};
use velowork_ui::confirm_dialog::{ConfirmDialog, ConfirmDialogEvent};
use velowork_ui::context_menu_backdrop::context_menu_backdrop;
use velowork_ui::h_flex;
use velowork_ui::icon_button::icon_button;
use velowork_ui::input::{labeled_input, InputEvent, InputState};
use velowork_ui::menu::{
    context_menu_panel, menu_item, menu_item_conditional, menu_item_conditional_with_shortcut,
    menu_item_with_shortcut, menu_separator,
};
use velowork_ui::overlay::{modal_backdrop, modal_content, modal_header};
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::scrollable::ScrollableElement;
use velowork_ui::simple_input::{
    InputChangedEvent, SimpleInput, SimpleInputState,
};
use velowork_ui::theme::{theme, surface_bg_t};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::tokens::{ui_font_family, ui_text_md, ui_text_ms, SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG, RADIUS_MD, ICON_SM, ICON_STD};
use velowork_ui::syntax::map_extension_to_syntax;
use velowork_ui::virtual_list::{ListSelection, scroll_to_row, virtual_list};
use velowork_workspace::focus::FocusManager;
use velowork_workspace::settings::FileSortBy;
use velowork_workspace::state::{LayoutNode, Workspace};

pub use velowork_terminal::pty_manager::SshClient as Client;

pub struct SftpConnection {
    pub sftp: Arc<russh_sftp::client::SftpSession>,
    pub ssh_handle: Arc<russh::client::Handle<Client>>,
}

impl SftpConnection {}

async fn run_in_tokio<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    velowork_terminal::pty_manager::get_tokio_runtime()
        .spawn(f)
        .await
        .expect("Tokio runtime panicked")
}

/// Whether `name` looks like a text/code file we can safely hand to an editor.
/// Used to gate the "Open with Default Editor" context-menu item. We treat a
/// file as text when its extension maps to a known syntax highlighter, when it
/// is a common plain-text extension, or when it has no extension at all
/// (e.g. `Makefile`, `LICENSE`, `README`).
fn is_likely_text_file(name: &str) -> bool {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some(e) if map_extension_to_syntax(e).is_some() => true,
        Some(
            "txt" | "text" | "log" | "csv" | "tsv" | "rst" | "adoc" | "org" | "gitignore"
            | "env" | "editorconfig" | "properties" | "out" | "trace" | "stacktrace"
            | "dump" | "npmrc" | "gitattributes" | "dockerignore",
        ) => true,
        // No extension → assume text (Makefile, LICENSE, README, …).
        None => true,
        _ => false,
    }
}

#[derive(Clone, Debug)]
pub struct SftpFile {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: u32,
    pub mode: u32,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub user: Option<String>,
    pub group: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SftpDialogState {
    pub is_dir: bool,
    pub name_input: Entity<SimpleInputState>,
    pub perm_user: (bool, bool, bool),  // r, w, x
    pub perm_group: (bool, bool, bool), // r, w, x
    pub perm_other: (bool, bool, bool), // r, w, x
    /// 重名校验失败时的错误提示（已本地化）。`Some` 时弹窗内显示提示并
    /// 阻止创建；用户修改名称后由订阅回调清空。
    pub error: Option<SharedString>,
}

/// Right-click target: a concrete list row (`Item(idx)` — `idx == 0` is the
/// synthetic parent ".." entry, `idx > 0` is a real file/dir) or the blank
/// area (`Blank`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuTarget {
    Item(usize),
    Blank,
}

/// 文件列表排序键。目录始终排在文件之前，此键决定同组内的次序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Perm,
    Owner,
    Size,
    Modified,
}

#[derive(Clone, Debug)]
pub struct ContextMenuState {
    pub position: gpui::Point<gpui::Pixels>,
    pub target: MenuTarget,
}

/// Inline rename dialog state (centered modal).
#[derive(Clone, Debug)]
pub struct SftpRenameState {
    pub idx: usize,
    pub old_path: String,
    pub old_name: String,
    pub name_input: Entity<SimpleInputState>,
}

/// Inline rename state - replaces the modal rename with an in-place input field.
#[derive(Clone, Debug)]
pub struct InlineRenameState {
    pub idx: usize,
    pub old_path: String,
    pub old_name: String,
    pub name_input: Option<Entity<InputState>>,
    pub error: Option<SharedString>,
}

impl InlineRenameState {
    pub fn ensure_input(&mut self, _window: &mut Window, cx: &mut Context<BottomPanel>) -> Entity<InputState> {
        if let Some(ref mut input) = self.name_input {
            return input.clone();
        }
        let old_name = self.old_name.clone();
        let input = cx.new(|cx| {
            InputState::new(cx)
                .default_value(old_name)
                .placeholder("New name...")
        });
        cx.subscribe(&input, |this: &mut BottomPanel, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                if let Some(ref mut r) = this.inline_rename {
                    if r.error.is_some() {
                        r.error = None;
                        cx.notify();
                    }
                }
            }
        })
        .detach();
        self.name_input = Some(input.clone());
        input
    }
}

/// "Move to" directory picker state (centered modal). `current` is the directory
/// currently being browsed; `entries` are its subdirectories.
#[derive(Clone, Debug)]
pub struct SftpMoveState {
    pub idx: usize,
    pub old_path: String,
    pub name: String,
    pub current: String,
    pub entries: Vec<SftpFile>,
    pub loading: bool,
    pub address_input: Entity<SimpleInputState>,
}

/// "New Link" dialog state (centered modal).
#[derive(Clone, Debug)]
pub struct SftpLinkState {
    pub name_input: Entity<SimpleInputState>,
    pub target_input: Entity<SimpleInputState>,
}

/// "Properties" snapshot (centered modal). `ctime` is `None` because the SFTP
/// protocol does not expose file creation time.
#[derive(Clone, Debug)]
pub struct SftpPropertiesState {
    pub name: String,
    pub is_dir: bool,
    pub path: String,
    pub size: u64,
    pub mtime: u32,
    pub ctime: Option<u32>,
}

/// Centered modal kinds for the SFTP file manager.
#[derive(Clone, Debug)]
pub enum SftpModal {
    Rename(SftpRenameState),
    Move(SftpMoveState),
    Link(SftpLinkState),
    Properties(SftpPropertiesState),
}

/// Tracks an in-progress bottom-panel resize drag.
pub enum SftpConnectionState {
    Disconnected,
    Connecting,
    Connected {
        conn: SftpConnection,
        current_dir: String,
        files: Vec<SftpFile>,
    },
    Failed(String),
}

use super::commands_panel::CommandsPanel;
use velowork_ui::dock::{Panel, PanelInfo, PanelKind, ToolbarItem};
use velowork_ui::icon::AppIcon;

#[derive(Clone, Copy, Debug)]
pub enum BottomPanelEvent {
    Close,
    DetachToggle,
}

impl velowork_ui::overlay::CloseEvent for BottomPanelEvent {
    fn is_close(&self) -> bool {
        matches!(self, BottomPanelEvent::Close)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomPanelTab {
    Sftp,
    Commands,
}

pub struct BottomPanel {
    pub(super) is_visible: bool,
    pub(super) is_collapsed: bool,
    pub(super) is_fullscreen: bool,
    pub(super) is_detached: bool,

    // Connection state (resolved dynamically from the focused terminal's
    // live SSH session — see `bind_active_terminal`).
    connection_state: SftpConnectionState,
    /// 通用列表选择状态（单选 / Ctrl 追加 / Shift 范围 / 键盘导航）。
    /// 行索引 0 表示父目录 ".." 项，1..=N 对应 `filtered_files()` 中的条目。
    selection: ListSelection,
    show_hidden: bool,

    /// 当前排序键与升降序。目录始终优先。
    sort_key: SortKey,
    sort_asc: bool,

    /// 目录列表本地缓存（key = `terminal_id\u{0}dir`）。切换目录时先展示缓存
    /// 内容以获得即时响应，随后后台刷新覆盖。
    dir_cache: std::collections::HashMap<String, Vec<SftpFile>>,

    address_input: Option<Entity<InputState>>,
    address_val: String,
    /// 驱动虚拟列表与滚动条的滚动句柄，同时支持 `scroll_to_item` 自动定位。
    scroll_handle: UniformListScrollHandle,

    active_dialog: Option<SftpDialogState>,
    context_menu: Option<ContextMenuState>,
    modal: Option<SftpModal>,
    /// Inline rename state - when set, shows an input field at the row position
    /// instead of opening a modal dialog.
    inline_rename: Option<InlineRenameState>,
    focus_handle: FocusHandle,
    /// When set, the corresponding input is focused on the next render.
    /// Used to auto-focus the first input of a freshly opened dialog so it is
    /// immediately editable (mirrors `AddProjectDialog`'s focus-on-open pattern).
    dialog_focus_target: Option<Entity<SimpleInputState>>,
    dialog_focus_pending: bool,
    /// When set, the panel's own `focus_handle` is refocused on the next render.
    /// Used to restore keyboard navigation after an inline rename is committed or
    /// cancelled, so arrow keys keep working.
    focus_self_pending: bool,

    // Bottom Panel Tab and Context
    active_tab: BottomPanelTab,
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,

    /// Backend used to look up a terminal's live SSH session handle.
    backend: Arc<dyn TerminalBackend>,

    /// Shared terminal registry: access active terminal's reported_cwd and send input.
    terminals: TerminalsRegistry,
    /// Auto sync directory with active terminal
    auto_sync: bool,
    /// Last observed terminal cwd to detect terminal cd events
    last_terminal_cwd: Option<String>,
    /// Last successfully loaded directory (for rollback on navigation failure)
    last_valid_dir: Option<String>,
    /// Cached remote shell PID on Linux hosts for zero-overhead /proc/<pid>/cwd queries
    remote_shell_pid: Option<u32>,
    /// Prevent concurrent async sync requests
    auto_sync_in_flight: bool,
    /// Background auto sync task handle
    _auto_sync_task: Option<Task<()>>,

    /// Terminal id the panel is currently bound to (`None` = empty /
    /// "Not Connected" state).
    active_terminal_id: Option<String>,
    /// Last terminal we successfully attached SFTP to (history fallback when
    /// the currently focused terminal has no SSH session).
    last_sftp_terminal_id: Option<String>,
    /// Remembered browse directory per terminal id, so switching terminals
    /// and back restores the previous location.
    remembered_dirs: std::collections::HashMap<String, String>,

    // Unified CommandsPanel and more menu state
    commands_panel: Entity<CommandsPanel>,

    /// Weak self-reference for toolbar callbacks that need entity access.
    self_weak: WeakEntity<Self>,

    /// Set while a directory listing is in flight (e.g. after pressing the
    /// refresh button). Drives the skeleton-shimmer loading state so the user
    /// gets clear visual feedback that data is being fetched.
    refreshing: bool,

    /// Bumped every time a fresh listing lands, used to replay the list
    /// fade-in animation on each refresh.
    refresh_token: u64,

    /// 新建目录/文件成功后，记录待定位项名称。在 `refresh` 拉取最新列表后，
    /// 据此自动选中并滚动高亮该项，随后清空。
    pending_locate: Option<String>,

    /// Pending delete-confirmation dialog (mirrors the quick-command delete
    /// confirm). Set when the user triggers a delete via key or context menu;
    /// the actual removal only happens after the user confirms.
    confirm_dialog: Option<Entity<ConfirmDialog>>,

    /// Shared overlay registry, injected by the dock so the delete-confirm
    /// dialog can register itself for centralized click-outside dismissal
    /// (same mechanism the quick-command confirm uses).
    overlay_registry: Option<Entity<OverlayRegistry>>,

    // Column widths and resizer state
    col_name_w: f32,
    col_perm_w: f32,
    col_owner_w: f32,
    col_size_w: f32,
    col_mtime_w: f32,
    col_resize_dragging: Option<(usize, f32, f32)>,
}

impl EventEmitter<BottomPanelEvent> for BottomPanel {}

impl BottomPanel {
    pub fn new(
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        backend: Arc<dyn TerminalBackend>,
        commands_panel: Entity<CommandsPanel>,
        terminals: TerminalsRegistry,
        cx: &mut Context<Self>,
    ) -> Self {
        // Re-render on focus changes. `Workspace::set_focused_terminal`
        // notifies the workspace entity, so observing it makes this single
        // shared panel follow the active terminal in real time (it re-binds
        // in `render` via `bind_active_terminal`).
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        cx.observe(&focus_manager, |_, _, cx| cx.notify()).detach();

        // Re-render on CommandsPanel changes
        cx.observe(&commands_panel, |_, _, cx| cx.notify()).detach();

        // Keep the SFTP list's "show hidden files" state in sync with the
        // global file-manager setting (so the setting applies live).
        let settings_entity = velowork_app_core::settings::settings_entity(cx);
        cx.observe(&settings_entity, |this, _state, cx| {
            this.show_hidden = velowork_app_core::settings::settings(cx).show_hidden_files;
            cx.notify();
        })
        .detach();

        let auto_sync_task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(1000))
                    .await;
                let res = this.update(cx, |this, cx| {
                    this.check_auto_sync(cx);
                });
                if res.is_err() {
                    break;
                }
            }
        });

        Self {
            is_visible: false,
            is_collapsed: false,
            is_fullscreen: false,
            is_detached: false,
            connection_state: SftpConnectionState::Disconnected,
            selection: ListSelection::new(),
            show_hidden: velowork_app_core::settings::settings(cx).show_hidden_files,
            sort_key: Self::sort_key_from_file_sort_by(velowork_app_core::settings::settings(cx).file_sort_by),
            sort_asc: true,
            dir_cache: std::collections::HashMap::new(),
            address_input: None,
            address_val: String::new(),
            scroll_handle: UniformListScrollHandle::new(),
            active_dialog: None,
            context_menu: None,
            modal: None,
            inline_rename: None,
            focus_handle: cx.focus_handle(),
            dialog_focus_target: None,
            dialog_focus_pending: false,
            focus_self_pending: false,
            active_tab: BottomPanelTab::Sftp,
            workspace,
            focus_manager,
            backend,
            terminals,
            auto_sync: true,
            last_terminal_cwd: None,
            last_valid_dir: None,
            remote_shell_pid: None,
            auto_sync_in_flight: false,
            _auto_sync_task: Some(auto_sync_task),
            active_terminal_id: None,
            last_sftp_terminal_id: None,
            remembered_dirs: std::collections::HashMap::new(),
            commands_panel,
            self_weak: cx.entity().downgrade(),
            refreshing: false,
            refresh_token: 0,
            pending_locate: None,
            confirm_dialog: None,
            overlay_registry: None,
            col_name_w: 260.0,
            col_perm_w: 100.0,
            col_owner_w: 100.0,
            col_size_w: 90.0,
            col_mtime_w: 140.0,
            col_resize_dragging: None,
        }
    }

    /// Inject the shared overlay registry (called by the dock alongside its own
    /// `set_overlay_registry`). Enables click-outside dismissal for the
    /// delete-confirmation dialog.
    pub fn set_overlay_registry(&mut self, reg: Entity<OverlayRegistry>) {
        self.overlay_registry = Some(reg);
    }

    pub fn ensure_address_input(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(ref input) = self.address_input {
            return input.clone();
        }
        let current_path = self.address_val.clone();
        let input = cx.new(|cx| {
            InputState::new(cx)
                .default_value(current_path)
                .placeholder("Remote directory...")
        });
        cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                if let Some(ref input) = this.address_input {
                    this.address_val = input.read(cx).text().to_string();
                }
            }
        })
        .detach();
        self.address_input = Some(input.clone());
        input
    }

    fn render_col_resizer(
        &self,
        col_idx: usize,
        current_w: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let is_dragging = self
            .col_resize_dragging
            .as_ref()
            .map_or(false, |d| d.0 == col_idx);
        div()
            .id(ElementId::Name(
                format!("sftp-col-resizer-{}", col_idx).into(),
            ))
            .w(SPACE_SM)
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_col_resize()
            .child(
                div()
                    .w(px(1.0))
                    .h_full()
                    .hover(|s| s.bg(rgb(t.border_active)))
                    .bg(if is_dragging {
                        rgb(t.border_active).into()
                    } else {
                        p.border_subtle
                    }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    this.col_resize_dragging =
                        Some((col_idx, f32::from(event.position.x), current_w));
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
    }

    pub fn toggle_visible(&mut self, cx: &mut Context<Self>) {
        self.is_visible = !self.is_visible;
        if self.is_visible {
            self.bind_active_terminal(cx);
        }
        cx.notify();
    }

    pub fn toggle_fullscreen(&mut self, cx: &mut Context<Self>) {
        self.is_fullscreen = !self.is_fullscreen;
        cx.notify();
    }

    pub fn toggle_collapsed(&mut self, cx: &mut Context<Self>) {
        self.is_collapsed = !self.is_collapsed;
        cx.notify();
    }

    pub fn show_tab(&mut self, tab: BottomPanelTab, cx: &mut Context<Self>) {
        self.active_tab = tab;
        self.is_visible = true;
        self.is_collapsed = false;
        if tab == BottomPanelTab::Sftp {
            self.bind_active_terminal(cx);
        } else if tab == BottomPanelTab::Commands {
            self.commands_panel.update(cx, |cp, cx| {
                cp.focus_input(cx);
            });
        }
        cx.notify();
    }

    pub fn toggle_tab(&mut self, tab: BottomPanelTab, cx: &mut Context<Self>) {
        if self.is_visible && !self.is_collapsed && self.active_tab == tab {
            self.is_visible = false;
        } else {
            self.active_tab = tab;
            self.is_visible = true;
            self.is_collapsed = false;
            if tab == BottomPanelTab::Sftp {
                self.bind_active_terminal(cx);
            } else if tab == BottomPanelTab::Commands {
                self.commands_panel.update(cx, |cp, cx| {
                    cp.focus_input(cx);
                });
            }
        }
        cx.notify();
    }

    pub fn is_detached(&self) -> bool {
        self.is_detached
    }

    pub fn set_detached(&mut self, detached: bool, cx: &mut Context<Self>) {
        self.is_detached = detached;
        cx.notify();
    }

    pub fn is_disconnected(&self) -> bool {
        matches!(self.connection_state, SftpConnectionState::Disconnected)
    }

    pub fn check_auto_sync(&mut self, cx: &mut Context<Self>) {
        if !self.auto_sync {
            return;
        }
        if self.active_terminal_id.is_none() {
            self.bind_active_terminal(cx);
        }
        let SftpConnectionState::Connected { ref conn, .. } = self.connection_state else {
            return;
        };
        let Some(ref tid) = self.active_terminal_id else {
            return;
        };
        let reported = self.terminals.lock().get(tid).and_then(|t| t.reported_cwd());
        if let Some(target_dir) = reported {
            let target_dir = target_dir.trim().to_string();
            if !target_dir.is_empty() {
                let norm_target = if target_dir.len() > 1 && target_dir.ends_with('/') {
                    target_dir.trim_end_matches('/').to_string()
                } else {
                    target_dir.clone()
                };
                let terminal_changed = match self.last_terminal_cwd.as_deref() {
                    Some(old) => {
                        let norm_old = if old.len() > 1 && old.ends_with('/') {
                            old.trim_end_matches('/')
                        } else {
                            old
                        };
                        norm_old != norm_target
                    }
                    None => true,
                };
                if terminal_changed {
                    self.last_terminal_cwd = Some(target_dir.clone());
                    let current = self.current_dir();
                    let norm_current = if current.len() > 1 && current.ends_with('/') {
                        current.trim_end_matches('/').to_string()
                    } else {
                        current.clone()
                    };
                    if norm_current != norm_target {
                        self.change_directory(target_dir, cx);
                    }
                }
            }
            return;
        }

        // If terminal has not reported cwd via OSC 7 / local proc, query remote shell cwd
        // via a lightweight background exec channel (0 terminal noise, 100% reliable /proc scan)
        if self.auto_sync_in_flight {
            return;
        }
        self.auto_sync_in_flight = true;
        let ssh_handle = conn.ssh_handle.clone();

        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            let res = run_in_tokio(async move {
                let mut channel = ssh_handle.channel_open_session().await?;
                let script = "sh -c 'if [ -d \"/proc\" ]; then for p in $(ls -dt /proc/[0-9]* 2>/dev/null); do [ \"$p\" = \"/proc/$$\" ] && continue; [ \"$p\" = \"/proc/$PPID\" ] && continue; fd0=$(readlink \"$p/fd/0\" 2>/dev/null); case \"$fd0\" in /dev/pts/*|/dev/tty*) comm=$(cat \"$p/comm\" 2>/dev/null); case \"$comm\" in bash|zsh|fish|sh|ash|dash|csh|tcsh|nu) target=$(readlink \"$p/cwd\" 2>/dev/null); if [ -n \"$target\" ] && [ -d \"$target\" ]; then echo \"$target\"; exit 0; fi;; esac;; esac; done; fi; if [ \"$(uname 2>/dev/null)\" = \"Darwin\" ] || [ -x \"/usr/sbin/lsof\" ] || which lsof >/dev/null 2>&1; then for p in $(lsof -a -u \"$(id -u 2>/dev/null || id -u)\" -c zsh -c bash -c fish -d 0 -Fn 2>/dev/null | grep -B 1 \"^n/dev/ttys\" | sed -n \"s/^p//p\"); do target=$(lsof -a -p \"$p\" -d cwd -Fn 2>/dev/null | sed -n \"s/^n//p\" | tail -n 1); if [ -n \"$target\" ] && [ -d \"$target\" ]; then echo \"$target\"; exit 0; fi; done; fi'";
                channel.exec(false, script).await?;
                let mut out = String::new();
                loop {
                    match channel.wait().await {
                        Some(russh::ChannelMsg::Data { data }) => {
                            out.push_str(&String::from_utf8_lossy(&data));
                        }
                        Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                        _ => {}
                    }
                }
                let target = out.trim().replace('\\', "/");
                if !target.is_empty() && (target.starts_with('/') || (target.len() >= 2 && target.chars().nth(1) == Some(':'))) {
                    Ok::<_, anyhow::Error>(target)
                } else {
                    anyhow::bail!("no valid cwd found")
                }
            })
            .await;

            let _ = this.update(cx, |this, cx| {
                this.auto_sync_in_flight = false;
                if let Ok(target_dir) = res {
                    let target_dir = target_dir.trim().to_string();
                    if !target_dir.is_empty() && (target_dir.starts_with('/') || (target_dir.len() >= 2 && target_dir.chars().nth(1) == Some(':'))) {
                        let norm_target = if target_dir.len() > 1 && target_dir.ends_with('/') {
                            target_dir.trim_end_matches('/').to_string()
                        } else {
                            target_dir.clone()
                        };
                        let terminal_changed = match this.last_terminal_cwd.as_deref() {
                            Some(old) => {
                                let norm_old = if old.len() > 1 && old.ends_with('/') {
                                    old.trim_end_matches('/')
                                } else {
                                    old
                                };
                                norm_old != norm_target
                            }
                            None => true,
                        };
                        if terminal_changed {
                            this.last_terminal_cwd = Some(target_dir.clone());
                            let current = this.current_dir();
                            let norm_current = if current.len() > 1 && current.ends_with('/') {
                                current.trim_end_matches('/').to_string()
                            } else {
                                current.clone()
                            };
                            if norm_current != norm_target {
                                this.change_directory(target_dir, cx);
                            }
                        }
                    }
                }
            });
        })
        .detach();
    }

    pub fn set_ssh_session(
        &mut self,
        ssh_session: Arc<russh::client::Handle<Client>>,
        restore_dir: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.connection_state = SftpConnectionState::Connecting;
        cx.notify();

        let ssh_handle = ssh_session;
        let initial_reported_cwd = if restore_dir.is_none() {
            if let Some(ref tid) = self.active_terminal_id {
                self.terminals.lock().get(tid).and_then(|t| t.reported_cwd())
            } else {
                None
            }
        } else {
            None
        };

        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            let res = run_in_tokio(async move {
                let channel = ssh_handle.channel_open_session().await?;
                channel.request_subsystem(true, "sftp").await?;
                let sftp =
                    Arc::new(russh_sftp::client::SftpSession::new(channel.into_stream()).await?);
                let start_dir = if let Some(dir) = restore_dir {
                    dir
                } else if let Some(dir) = initial_reported_cwd {
                    dir
                } else {
                    let canon = match sftp.canonicalize(".").await {
                        Ok(d) if !d.is_empty() => Some(d),
                        _ => sftp.canonicalize("").await.ok().filter(|d| !d.is_empty()),
                    };
                    canon.unwrap_or_else(|| "/".to_string())
                };
                Ok::<_, anyhow::Error>((SftpConnection { sftp, ssh_handle }, start_dir))
            })
            .await;

            let _ = this.update(cx, |this, cx| {
                match res {
                    Ok((conn, start_dir)) => {
                        // Remember which terminal we're attached to (history fallback).
                        this.last_sftp_terminal_id = this.active_terminal_id.clone();
                        this.last_terminal_cwd = Some(start_dir.clone());
                        this.last_valid_dir = Some(start_dir.clone());
                        this.connection_state = SftpConnectionState::Connected {
                            conn,
                            current_dir: start_dir.clone(),
                            files: Vec::new(),
                        };
                        this.address_val = start_dir.clone();
                        this.address_input = None;
                        this.refresh(cx);
                    }
                    Err(err) => {
                        this.connection_state = SftpConnectionState::Failed(err.to_string());
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Resolve which terminal's SFTP the panel should show, following the
    /// focus rules:
    ///   1. the terminal that currently holds the cursor (focused terminal
    ///      that has a live SSH session), else
    ///   2. the last terminal we successfully attached SFTP to (history), else
    ///   3. `None` → empty "Not Connected" state.
    fn resolve_target(&self, cx: &App) -> Option<String> {
        if let Some(tid) = self.focused_terminal_id(cx) {
            if self.backend.get_ssh_session(&tid).is_some() {
                return Some(tid);
            }
            // Explicitly focused terminal has no SSH session (e.g. local terminal).
            // Do not fall back to previous SFTP session.
            return None;
        }
        if let Some(ref tid) = self.last_sftp_terminal_id {
            if self.backend.get_ssh_session(tid).is_some() {
                return Some(tid.clone());
            }
        }
        None
    }

    /// The terminal id that currently holds focus (cursor), resolved from the
    /// focus manager → project → layout node. `None` if no terminal is focused.
    fn focused_terminal_id(&self, cx: &App) -> Option<String> {
        let focused = self.focus_manager.read(cx).focused_terminal_state()?;
        let project = self.workspace.read(cx).project(&focused.project_id)?;
        let layout = project.layout.as_ref()?;
        match layout.get_at_path(&focused.layout_path) {
            Some(LayoutNode::Terminal {
                terminal_id: Some(id),
                ..
            }) => Some(id.clone()),
            _ => None,
        }
    }

    /// Bind the panel to the focused terminal, re-resolving on every call so the
    /// single shared panel always reflects the active terminal in real time.
    /// No-op when the target hasn't changed since the last bind.
    fn bind_active_terminal(&mut self, cx: &mut Context<Self>) {
        let target = self.resolve_target(cx);
        if target == self.active_terminal_id {
            return;
        }
        self.active_terminal_id = target.clone();
        self.last_terminal_cwd = None;
        self.last_valid_dir = None;
        self.remote_shell_pid = None;
        self.selection.clear();
        match target {
            None => {
                self.connection_state = SftpConnectionState::Disconnected;
                cx.notify();
            }
            Some(tid) => {
                if let Some(session) = self.backend.get_ssh_session(&tid) {
                    let restore = self.remembered_dirs.get(&tid).cloned();
                    self.set_ssh_session(session, restore, cx);
                } else {
                    self.connection_state = SftpConnectionState::Disconnected;
                    cx.notify();
                }
            }
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let (sftp, current_dir, ssh) = match &self.connection_state {
            SftpConnectionState::Connected {
                conn, current_dir, ..
            } => (
                conn.sftp.clone(),
                current_dir.clone(),
                conn.ssh_handle.clone(),
            ),
            _ => return,
        };

        // Show the skeleton-shimmer loading state immediately.
        self.refreshing = true;
        cx.notify();

        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            let target_req_dir = current_dir.clone();
            let files_res = run_in_tokio(async move { sftp.read_dir(&current_dir).await }).await;
            // Materialise the listing once so we can (1) scan it for ids and
            // (2) hand it to the UI update without re-borrowing `files_res`.
            let (listed, read_err) = match files_res {
                Ok(e) => (Some(e.into_iter().collect::<Vec<_>>()), None),
                Err(err) => (None, Some(err.to_string())),
            };

            // ── Resolve uid/gid → user/group names on the *remote* host ──
            // A local nsswitch lookup would map the remote ids against the local
            // passwd DB and produce wrong names, so we ask the remote shell via
            // `getent`. Unresolved ids (no `getent`, or unknown id) simply fall
            // back to the numeric id already handled by `format_owner`.
            let name_map: Option<(HashMap<u32, String>, HashMap<u32, String>)> =
                if let Some(entries) = &listed {
                    let mut uids = std::collections::HashSet::new();
                    let mut gids = std::collections::HashSet::new();
                    for e in entries {
                        let m = e.metadata();
                        if let Some(u) = m.uid {
                            uids.insert(u);
                        }
                        if let Some(g) = m.gid {
                            gids.insert(g);
                        }
                    }
                    let uids: Vec<u32> = uids.into_iter().collect();
                    let gids: Vec<u32> = gids.into_iter().collect();
                    if uids.is_empty() && gids.is_empty() {
                        None
                    } else {
                        let ssh2 = ssh.clone();
                        run_in_tokio(async move {
                            Self::resolve_owner_names(&ssh2, &uids, &gids).await
                        })
                        .await
                    }
                } else {
                    None
                };

            let _ = this.update(cx, |this, cx| {
                // Loading finished — clear the shimmer and replay the fade-in.
                this.refreshing = false;
                this.refresh_token = this.refresh_token.wrapping_add(1);
                if let Some(entries) = listed {
                    this.last_valid_dir = Some(target_req_dir.clone());
                    let (owner_names, group_names) = name_map.unwrap_or_default();
                    let mut files = Vec::new();
                    for entry in entries {
                        let name = entry.file_name();
                        if name == "." || name == ".." {
                            continue;
                        }
                        let metadata = entry.metadata();
                        let permissions = metadata.permissions.unwrap_or(0);
                        let is_dir = metadata.is_dir();
                        let size = metadata.size.unwrap_or(0);
                        let mtime = metadata.mtime.unwrap_or(0);
                        let uid = metadata.uid;
                        let gid = metadata.gid;
                        // Prefer the name the SFTP server already shipped;
                        // otherwise map the numeric id to its remote owner/group.
                        let user = metadata
                            .user
                            .clone()
                            .or_else(|| uid.and_then(|u| owner_names.get(&u).cloned()));
                        let group = metadata
                            .group
                            .clone()
                            .or_else(|| gid.and_then(|g| group_names.get(&g).cloned()));

                        files.push(SftpFile {
                            name,
                            is_dir,
                            size,
                            mtime,
                            mode: permissions,
                            uid,
                            gid,
                            user,
                            group,
                        });
                    }

                    files.sort_by(|a, b| {
                        if a.is_dir != b.is_dir {
                            b.is_dir.cmp(&a.is_dir)
                        } else {
                            a.name.cmp(&b.name)
                        }
                    });

                    // 写入本地缓存，供下次切回该目录时即时展示。
                    let key = this.cache_key(&this.current_dir());
                    this.dir_cache.insert(key, files.clone());

                    if let SftpConnectionState::Connected {
                        files: ref mut current_files,
                        ..
                    } = this.connection_state
                    {
                        *current_files = files;
                    }
                    // 内容变化后裁剪越界选择。
                    this.selection.clamp(this.filtered_files().len() + 1);

                    // 新建完成后自动定位到刚创建的项：选中（高亮）并滚动至其所在行。
                    if let Some(target) = this.pending_locate.take() {
                        let filtered = this.filtered_files();
                        if let Some(pos) = filtered.iter().position(|f| f.name == target) {
                            // 行索引 0 为父目录 ".." 项，真实文件从 1 开始。
                            let row_idx = pos + 1;
                            this.selection.select_one(row_idx);
                            scroll_to_row(&this.scroll_handle, row_idx);
                        }
                    }
                } else if let Some(_err) = read_err {
                    // Remove failed dir from cache
                    let key = this.cache_key(&target_req_dir);
                    this.dir_cache.remove(&key);

                    // Show error toast
                    let template = i18n!(cx, "sftp.dir_inaccessible");
                    let msg = template.replace("{path}", &target_req_dir);
                    velowork_workspace::toast::ToastManager::error(msg, cx);

                    // Rollback to last valid directory, or "/"
                    let fallback = this.last_valid_dir.clone().unwrap_or_else(|| "/".to_string());
                    if fallback != target_req_dir {
                        this.change_directory(fallback, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Resolve a set of remote uid/gid values to their user/group names by
    /// asking the remote shell (`getent passwd` / `getent group`). Returns
    /// `None` when nothing could be resolved, so callers keep showing numeric
    /// ids via `format_owner`.
    async fn resolve_owner_names(
        ssh: &Arc<russh::client::Handle<Client>>,
        uids: &[u32],
        gids: &[u32],
    ) -> Option<(HashMap<u32, String>, HashMap<u32, String>)> {
        let mut users: HashMap<u32, String> = HashMap::new();
        let mut groups: HashMap<u32, String> = HashMap::new();

        if !uids.is_empty() {
            let cmd = format!(
                "getent passwd {}",
                uids.iter()
                    .map(|u| u.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            if let Ok(out) = Self::run_remote_command(ssh, &cmd).await {
                for line in out.lines() {
                    let parts: Vec<&str> = line.split(':').collect();
                    // `name:x:uid:gid:...`
                    if parts.len() >= 3 {
                        if let Ok(uid) = parts[2].parse::<u32>() {
                            users.insert(uid, parts[0].to_string());
                        }
                    }
                }
            }
        }

        if !gids.is_empty() {
            let cmd = format!(
                "getent group {}",
                gids.iter()
                    .map(|g| g.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            if let Ok(out) = Self::run_remote_command(ssh, &cmd).await {
                for line in out.lines() {
                    let parts: Vec<&str> = line.split(':').collect();
                    // `name:x:gid:...`
                    if parts.len() >= 3 {
                        if let Ok(gid) = parts[2].parse::<u32>() {
                            groups.insert(gid, parts[0].to_string());
                        }
                    }
                }
            }
        }

        if users.is_empty() && groups.is_empty() {
            None
        } else {
            Some((users, groups))
        }
    }

    /// Open a throwaway exec channel on the shared SSH session and drain its
    /// stdout/stderr. Mirrors `SshMonitorSource::exec` in `ssh_monitor.rs`.
    async fn run_remote_command(
        ssh: &Arc<russh::client::Handle<Client>>,
        script: &str,
    ) -> std::result::Result<String, anyhow::Error> {
        let mut channel = ssh
            .channel_open_session()
            .await
            .map_err(|e| anyhow::anyhow!("ssh channel open failed: {:?}", e))?;
        channel
            .exec(false, script)
            .await
            .map_err(|e| anyhow::anyhow!("ssh exec failed: {:?}", e))?;
        let mut out = String::new();
        loop {
            match channel.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    out.push_str(&String::from_utf8_lossy(&data.to_vec()));
                }
                Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                    out.push_str(&String::from_utf8_lossy(&data.to_vec()));
                }
                Some(russh::ChannelMsg::Eof)
                | Some(russh::ChannelMsg::Close)
                | None => break,
                Some(_) => {}
            }
        }
        Ok(out)
    }

    /// 构建目录缓存键：`terminal_id\u{0}dir`，隔离不同会话的同名目录。
    fn cache_key(&self, dir: &str) -> String {
        format!(
            "{}\u{0}{}",
            self.active_terminal_id.as_deref().unwrap_or(""),
            dir
        )
    }

    fn change_directory(&mut self, path: String, cx: &mut Context<Self>) {
        let cache_key = self.cache_key(&path);
        let cached = self.dir_cache.get(&cache_key).cloned();
        if let SftpConnectionState::Connected {
            ref mut current_dir,
            ref mut files,
            ..
        } = self.connection_state
        {
            *current_dir = path.clone();
            // 命中缓存：立即展示旧内容以获得即时响应，随后由 `refresh` 后台覆盖；
            // 未命中则清空，配合 `refreshing` 展示骨架屏。
            match cached {
                Some(list) => *files = list,
                None => files.clear(),
            }
        }
        self.address_val = path.clone();
        self.address_input = None;
        // Remember the browse location per terminal so switching
        // terminals and back restores the previous directory.
        if let Some(ref tid) = self.active_terminal_id {
            self.remembered_dirs.insert(tid.clone(), path.clone());
        }
        self.selection.clear();
        // 切换目录后回到列表顶部。
        scroll_to_row(&self.scroll_handle, 0);
        self.refresh(cx);
    }

    /// Resolve which file row (1-based, skipping the parent row) is the
    /// effective selection for actions like download.
    fn selected_file_row(&self, file_count: usize) -> Option<usize> {
        if let Some(i) = self.selection.active() {
            if i != 0 && i <= file_count {
                return Some(i);
            }
        }
        self.selection
            .selected()
            .iter()
            .copied()
            .filter(|i| *i != 0 && *i <= file_count)
            .min()
    }

    fn go_to_parent_directory(&mut self, cx: &mut Context<Self>) {
        let current_dir = match &self.connection_state {
            SftpConnectionState::Connected { current_dir, .. } => current_dir.clone(),
            _ => return,
        };
        if current_dir == "/" || current_dir.is_empty() {
            return;
        }
        let parts: Vec<&str> = current_dir.split('/').collect();
        if parts.len() > 1 {
            let mut parent = parts[..parts.len() - 1].join("/");
            if parent.is_empty() {
                parent = "/".to_string();
            }
            self.change_directory(parent, cx);
        }
    }

    fn handle_address_enter(&mut self, cx: &mut Context<Self>) {
        let path = self.address_input.as_ref().map(|i| i.read(cx).text().to_string().trim().to_string()).unwrap_or_else(|| self.address_val.trim().to_string());
        if !path.is_empty() {
            self.change_directory(path, cx);
        }
    }

    fn open_item(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx == 0 {
            self.go_to_parent_directory(cx);
        } else {
            // Must index into the *filtered* list (the same list the UI renders),
            // otherwise toggling "show hidden files" off leaves the row index
            // pointing at the raw entry at that position — which is the hidden
            // file that was filtered out of the visible list.
            let files = self.filtered_files();

            let file_idx = idx - 1;
            if file_idx < files.len() {
                let file = &files[file_idx];
                if file.is_dir {
                    let mut target_dir = match &self.connection_state {
                        SftpConnectionState::Connected { current_dir, .. } => current_dir.clone(),
                        _ => return,
                    };
                    if !target_dir.ends_with('/') {
                        target_dir.push('/');
                    }
                    target_dir.push_str(&file.name);
                    self.change_directory(target_dir, cx);
                } else {
                    // 文件：双击 / Enter 触发下载（与右键「下载」菜单行为一致）。
                    self.trigger_download(cx);
                }
            }
        }
    }

    fn open_create_dialog(&mut self, is_dir: bool, cx: &mut Context<Self>) {
        let name_input = cx.new(|cx| {
            SimpleInputState::new(cx).placeholder(if is_dir {
                i18n!(cx, "sftp.dialog.dir_name_placeholder")
            } else {
                i18n!(cx, "sftp.dialog.file_name_placeholder")
            })
        });
        // 用户修改名称时清除之前的重名校验错误提示。
        cx.subscribe(&name_input, |this, _, _: &InputChangedEvent, cx| {
            if let Some(ref mut d) = this.active_dialog {
                if d.error.is_some() {
                    d.error = None;
                    cx.notify();
                }
            }
        })
        .detach();
        let default_mode = if is_dir {
            Self::parse_mode_string(&velowork_app_core::settings::settings(cx).sftp_default_dir_mode)
        } else {
            Self::parse_mode_string(&velowork_app_core::settings::settings(cx).sftp_default_file_mode)
        };
        let (perm_user, perm_group, perm_other) = Self::triples_from_mode(default_mode);

        self.active_dialog = Some(SftpDialogState {
            is_dir,
            name_input: name_input.clone(),
            perm_user,
            perm_group,
            perm_other,
            error: None,
        });
        self.dialog_focus_target = Some(name_input);
        self.dialog_focus_pending = true;
        self.focus_manager.update(cx, |fm, cx| {
            self.workspace.update(cx, |ws, cx| {
                ws.clear_focused_terminal(fm, cx);
            });
        });
        cx.notify();
    }

    fn submit_creation_dialog(&mut self, cx: &mut Context<Self>) {
        let dialog = match &self.active_dialog {
            Some(d) => d.clone(),
            None => return,
        };

        let name = dialog.name_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }

        // 重名校验：若与当前目录已有项（含隐藏项）重名，提示用户并阻止创建。
        let name_conflict = match &self.connection_state {
            SftpConnectionState::Connected { files, .. } => files.iter().any(|f| f.name == name),
            _ => false,
        };
        if name_conflict {
            let msg: SharedString = i18n!(cx, "sftp.dialog.name_exists")
                .replace("{name}", &name)
                .into();
            if let Some(ref mut d) = self.active_dialog {
                d.error = Some(msg);
            }
            cx.notify();
            return;
        }

        let (sftp, current_dir) = match &self.connection_state {
            SftpConnectionState::Connected {
                conn, current_dir, ..
            } => (conn.sftp.clone(), current_dir.clone()),
            _ => return,
        };

        let mut target_path = current_dir.clone();
        if !target_path.ends_with('/') {
            target_path.push('/');
        }
        target_path.push_str(&name);

        let is_dir = dialog.is_dir;
        let mode = mode_from_permissions(
            is_dir,
            dialog.perm_user,
            dialog.perm_group,
            dialog.perm_other,
        );

        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            use russh_sftp::protocol::FileAttributes;
            let mut attrs = FileAttributes::default();
            attrs.permissions = Some(mode);

            let res = run_in_tokio(async move {
                if is_dir {
                    match sftp.create_dir(&target_path).await {
                        Ok(_) => {
                            // 设置权限(chmod)尽力而为：即使失败也视为目录创建成功，
                            // 避免弹窗因 set_metadata 报错而无法关闭。
                            let _ = sftp.set_metadata(&target_path, attrs).await;
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    use russh_sftp::protocol::OpenFlags;
                    sftp.open_with_flags_and_attributes(
                        &target_path,
                        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                        attrs,
                    )
                    .await
                    .map(|_| ())
                    .map_err(|e| e.into())
                }
            })
            .await;

            let _ = this.update(cx, |this, cx| match res {
                Ok(()) => {
                    // 记住刚创建项的名称，待 refresh 拉取最新列表后自动定位并高亮。
                    this.pending_locate = Some(name.clone());
                    this.close_active_dialog(cx);
                    this.refresh(cx);
                }
                Err(e) => {
                    // 创建失败（如目标目录无写入权限），提示用户并保持弹窗打开。
                    let msg: SharedString = i18n!(cx, "sftp.dialog.create_failed")
                        .replace("{name}", &name)
                        .replace("{error}", &e.to_string())
                        .into();
                    if let Some(ref mut d) = this.active_dialog {
                        d.error = Some(msg);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    // ─── Path helpers ────────────────────────────────────────────────────────
    fn current_dir(&self) -> String {
        match &self.connection_state {
            SftpConnectionState::Connected { current_dir, .. } => current_dir.clone(),
            _ => String::new(),
        }
    }

    fn filtered_files(&self) -> Vec<SftpFile> {
        let mut v: Vec<SftpFile> = match &self.connection_state {
            SftpConnectionState::Connected { files, .. } => files
                .iter()
                .filter(|f| self.show_hidden || !f.name.starts_with('.'))
                .cloned()
                .collect(),
            _ => return Vec::new(),
        };

        let asc = self.sort_asc;
        let key = self.sort_key;
        v.sort_by(|a, b| {
            // 目录始终排在文件之前，与主流文件管理器一致。
            if a.is_dir != b.is_dir {
                return b.is_dir.cmp(&a.is_dir);
            }
            let ord = match key {
                SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortKey::Perm => a.mode.cmp(&b.mode),
                SortKey::Owner => format_owner(a).cmp(&format_owner(b)),
                SortKey::Size => a.size.cmp(&b.size),
                SortKey::Modified => a.mtime.cmp(&b.mtime),
            };
            if asc { ord } else { ord.reverse() }
        });
        v
    }

    /// Map the global `FileSortBy` setting to the SFTP panel's internal `SortKey`.
    fn sort_key_from_file_sort_by(f: velowork_workspace::settings::FileSortBy) -> SortKey {
        match f {
            FileSortBy::Name => SortKey::Name,
            FileSortBy::Size => SortKey::Size,
            FileSortBy::Date => SortKey::Modified,
            // No dedicated "type" sort in the SFTP list; fall back to name,
            // which groups by extension/name and is the closest approximation.
            FileSortBy::Type => SortKey::Name,
        }
    }

    /// Parse an octal permission string (e.g. "0644" or "644") into a `u32`.
    /// Falls back to `0o644` on empty/invalid input.
    fn parse_mode_string(s: &str) -> u32 {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return 0o644;
        }
        u32::from_str_radix(trimmed, 8)
            .ok()
            .filter(|m| *m <= 0o777)
            .unwrap_or(0o644)
    }

    /// Decompose a `u32` mode (rwxrwxrwx) into per-class (user/group/other)
    /// (read, write, execute) triples used by the create dialog.
    fn triples_from_mode(
        mode: u32,
    ) -> ((bool, bool, bool), (bool, bool, bool), (bool, bool, bool)) {
        let bit = |m: u32, p: u32| (m & (1 << p)) != 0;
        let user = (bit(mode, 8), bit(mode, 7), bit(mode, 6));
        let group = (bit(mode, 5), bit(mode, 4), bit(mode, 3));
        let other = (bit(mode, 2), bit(mode, 1), bit(mode, 0));
        (user, group, other)
    }

    /// 当前列宽快照。
    fn col_widths(&self) -> ColumnWidths {
        ColumnWidths {
            name: self.col_name_w,
            perm: self.col_perm_w,
            owner: self.col_owner_w,
            size: self.col_size_w,
            mtime: self.col_mtime_w,
        }
    }

    /// 点击表头切换排序：同键切换升降序，异键切到该键并默认升序。
    fn toggle_sort(&mut self, key: SortKey, cx: &mut Context<Self>) {
        if self.sort_key == key {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort_key = key;
            self.sort_asc = true;
        }
        self.selection.clear();
        cx.notify();
    }

    /// 可排序的列表表头。点击列名按该列排序（目录始终在前），当前排序列显示升降箭头。
    fn sftp_header_row(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let active = self.sort_key;
        let asc = self.sort_asc;

        let cell = |key: SortKey,
                    label: String,
                    width: Option<f32>,
                    cx: &mut Context<Self>|
         -> Stateful<Div> {
            let t = theme(cx);
            let p = SemanticPalette::from_theme(&t);
            let is_active = key == active;
            let arrow = if is_active {
                Some(
                    (if asc {
                        AppIcon::SortAsc
                    } else {
                        AppIcon::SortDesc
                    })
                    .size(px(12.0))
                    .text_color(p.text_primary),
                )
            } else {
                None
            };
            let base = div()
                .id(ElementId::Name(format!("sftp-hdr-{:?}", key).into()))
                .overflow_hidden()
                .text_size(ui_text_md(cx))
                .text_color(p.text_secondary)
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(2.0));
            let base = if let Some(w) = width {
                base.w(px(w))
            } else {
                base.flex_1().min_w(px(self.col_mtime_w))
            };
            let base = base.child(div().flex_1().child(label));
            let base = if let Some(arrow) = arrow {
                base.child(arrow)
            } else {
                base
            };
            base.on_click(cx.listener(move |this, _event, _window, cx| {
                this.toggle_sort(key, cx);
            }))
        };

        h_flex()
            .id("sftp-header-row")
            .px(SPACE_LG)
            .py(SPACE_XS)
            .bg(surface_bg_t(t.bg_header, &t))
            .border_b_1()
            .border_color(p.border_subtle)
            .items_center()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_this, _event, _window, cx| {
                    cx.stop_propagation();
                }),
            )
            .child(cell(
                SortKey::Name,
                i18n!(cx, "sftp.props.name"),
                Some(self.col_name_w),
                cx,
            ))
            .child(self.render_col_resizer(0, self.col_name_w, cx))
            .child(cell(
                SortKey::Perm,
                i18n!(cx, "sftp.header.permissions"),
                Some(self.col_perm_w),
                cx,
            ))
            .child(self.render_col_resizer(1, self.col_perm_w, cx))
            .child(cell(
                SortKey::Owner,
                i18n!(cx, "sftp.header.owner"),
                Some(self.col_owner_w),
                cx,
            ))
            .child(self.render_col_resizer(2, self.col_owner_w, cx))
            .child(cell(
                SortKey::Size,
                i18n!(cx, "sftp.props.size"),
                Some(self.col_size_w),
                cx,
            ))
            .child(self.render_col_resizer(3, self.col_size_w, cx))
            .child(cell(
                SortKey::Modified,
                i18n!(cx, "sftp.props.modified"),
                None,
                cx,
            ))
    }

    /// 刷新进行中时，列表顶部显示的加载进度条（脉动），
    /// 提示后台正在拉取目录内容；缓存内容仍可见，不阻塞浏览。
    fn loading_bar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let anim_id = "sftp-loading-bar".to_string();
        div()
            .id("sftp-loading-bar")
            .relative()
            .h(px(2.0))
            .w_full()
            .overflow_hidden()
            .bg(surface_bg_t(t.bg_hover, &t))
            .child(
                div()
                    .id(ElementId::Name(anim_id.clone().into()))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .h_full()
                    .w_full()
                    .bg(p.status_info)
                    .with_animation(
                        anim_id,
                        Animation::new(Duration::from_millis(900)).repeat(),
                        |this, delta| {
                            // 0 → 1 → 0 的透明度脉动，给出明确的“正在加载”反馈。
                            let pulse = 1.0 - (2.0 * delta - 1.0).abs();
                            this.opacity(0.35 + 0.45 * pulse)
                        },
                    ),
            )
    }

    /// 首次加载（无缓存内容可展示）时，列表区域居中显示的旋转加载指示器。
    fn loading_overlay(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let anim_id = "sftp-loading-spinner".to_string();
        div()
            .id("sftp-loading-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .bg(surface_bg_t(t.bg_panel, &t))
            .child(
                div()
                    .id(ElementId::Name(anim_id.clone().into()))
                    .flex()
                    .items_center()
                    .justify_center()
                    .with_animation(
                        anim_id,
                        Animation::new(Duration::from_millis(1000)).repeat(),
                        move |this, delta| {
                            let angle = delta * std::f32::consts::TAU;
                            this.child(
                                AppIcon::Refresh
                                    .size(px(18.0))
                                    .text_color(p.text_secondary)
                                    .with_transformation(Transformation::rotate(radians(angle))),
                            )
                        },
                    ),
            )
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(p.text_muted)
                    .child(i18n!(cx, "common.loading")),
            )
    }

    /// Resolve the absolute path, `is_dir` and `is_parent` flag for a list row index.
    fn abs_path(&self, idx: usize) -> Option<(String, bool, bool)> {
        let dir = self.current_dir();
        if idx == 0 {
            return Some((dir, true, true));
        }
        let files = self.filtered_files();
        if idx <= files.len() {
            let f = &files[idx - 1];
            let mut p = dir;
            if !p.ends_with('/') {
                p.push('/');
            }
            p.push_str(&f.name);
            return Some((p, f.is_dir, false));
        }
        None
    }

    fn close_active_dialog(&mut self, cx: &mut Context<Self>) {
        if self.active_dialog.is_some() {
            self.active_dialog = None;
            self.focus_manager.update(cx, |fm, cx| {
                self.workspace.update(cx, |ws, cx| {
                    ws.restore_focused_terminal(fm, cx);
                });
            });
            cx.notify();
        }
    }

    fn close_modal(&mut self, cx: &mut Context<Self>) {
        if self.modal.is_some() {
            self.modal = None;
            self.focus_manager.update(cx, |fm, cx| {
                self.workspace.update(cx, |ws, cx| {
                    ws.restore_focused_terminal(fm, cx);
                });
            });
            cx.notify();
        }
    }

    // ─── Delete ──────────────────────────────────────────────────────────────
    /// Perform the actual removal of `path` (already resolved, not an index, so
    /// it stays valid even if the listing changes while the confirm dialog is
    /// open). `is_dir` selects recursive-dir vs single-file removal.
    fn delete_path(&mut self, path: String, is_dir: bool, cx: &mut Context<Self>) {
        let sftp = match &self.connection_state {
            SftpConnectionState::Connected { conn, .. } => conn.sftp.clone(),
            _ => return,
        };
        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            let res = run_in_tokio(async move {
                if is_dir {
                    sftp.remove_dir(path).await
                } else {
                    sftp.remove_file(path).await
                }
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                if res.is_ok() {
                    this.modal = None;
                    this.context_menu = None;
                    this.refresh(cx);
                }
            });
        })
        .detach();
    }

    /// Open a confirmation dialog before deleting the item at `idx`
    /// (mirrors the quick-command delete confirmation). The real removal only
    /// happens after the user presses Confirm. `idx == 0` (the ".." parent row)
    /// is never deletable.
    fn request_delete_confirm(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx == 0 {
            return;
        }
        let (path, is_dir, name) = match self.abs_path(idx) {
            Some((p, d, false)) => (p, d, {
                let files = self.filtered_files();
                files
                    .get(idx - 1)
                    .map(|f| f.name.clone())
                    .unwrap_or_default()
            }),
            _ => return,
        };

        let title = i18n!(cx, "sftp.delete_confirm_title");
        let message = if is_dir {
            i18n!(cx, "sftp.delete_confirm_dir").replace("{name}", &name)
        } else {
            i18n!(cx, "sftp.delete_confirm").replace("{name}", &name)
        };

        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                title,
                message,
                i18n!(cx, "common.delete"),
                i18n!(cx, "common.cancel"),
                true,
                self.overlay_registry.clone(),
                "sftp-delete-confirm",
            )
        });

        cx.subscribe(&dialog, {
            let path = path.clone();
            move |this, _dialog, event, cx| {
                if matches!(event, ConfirmDialogEvent::Confirmed { .. }) {
                    let (path, is_dir) = (path.clone(), is_dir);
                    this.delete_path(path, is_dir, cx);
                }
                this.confirm_dialog = None;
                cx.notify();
            }
        })
        .detach();

        self.context_menu = None;
        self.confirm_dialog = Some(dialog);
        cx.notify();
    }

    // ─── Rename ─────────────────────────────────────────────────────────────
    fn open_rename_dialog(&mut self, idx: usize, cx: &mut Context<Self>) {
        let (old_path, old_name) = match self.abs_path(idx) {
            Some((p, _, false)) => {
                let files = self.filtered_files();
                (p, files[idx - 1].name.clone())
            }
            _ => return,
        };
        // Use inline rename instead of modal
        self.inline_rename = Some(InlineRenameState {
            idx,
            old_path,
            old_name,
            name_input: None,
            error: None,
        });
        self.context_menu = None;
        cx.notify();
    }

    fn submit_rename(&mut self, cx: &mut Context<Self>) {
        // Support both modal and inline rename
        let (sftp, old_path, new_path, old_name, is_inline) = if let Some(s) = &self.inline_rename {
            let new_name = if let Some(ref input) = s.name_input {
                input.read(cx).text().to_string().trim().to_string()
            } else {
                s.old_name.clone()
            };
            if new_name.is_empty() || new_name == s.old_name {
                self.inline_rename = None;
                self.focus_self_pending = true;
                cx.notify();
                return;
            }
            let parent = parent_dir_of(&s.old_path);
            (
                match &self.connection_state {
                    SftpConnectionState::Connected { conn, .. } => conn.sftp.clone(),
                    _ => return,
                },
                s.old_path.clone(),
                join_path(&parent, &new_name),
                s.old_name.clone(),
                true,
            )
        } else if let Some(SftpModal::Rename(s)) = &self.modal {
            let new_name = s.name_input.read(cx).value().trim().to_string();
            if new_name.is_empty() || new_name == s.old_name {
                return;
            }
            let parent = parent_dir_of(&s.old_path);
            (
                match &self.connection_state {
                    SftpConnectionState::Connected { conn, .. } => conn.sftp.clone(),
                    _ => return,
                },
                s.old_path.clone(),
                join_path(&parent, &new_name),
                s.old_name.clone(),
                false,
            )
        } else {
            return;
        };
        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            let res = run_in_tokio(async move { sftp.rename(&old_path, &new_path).await }).await;
            let _ = this.update(cx, |this, cx| match res {
                Ok(()) => {
                    this.inline_rename = None;
                    this.modal = None;
                    this.context_menu = None;
                    this.focus_self_pending = true;
                    this.refresh(cx);
                }
                Err(e) => {
                    // 重命名失败（如目标无写入/重命名权限），提示用户并保持编辑态。
                    // 仅行内重命名需要提示；弹窗重命名已不再使用。
                    if is_inline {
                        let msg: SharedString = i18n!(cx, "sftp.dialog.rename_failed")
                            .replace("{name}", &old_name)
                            .replace("{error}", &e.to_string())
                            .into();
                        if let Some(ref mut r) = this.inline_rename {
                            r.error = Some(msg);
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    // ─── Move to (directory picker) ───────────────────────────────────────────
    fn open_move_dialog(&mut self, idx: usize, cx: &mut Context<Self>) {
        let (old_path, name) = match self.abs_path(idx) {
            Some((p, _, false)) => {
                let files = self.filtered_files();
                (p, files[idx - 1].name.clone())
            }
            _ => return,
        };
        let current = self.current_dir();
        let address_input = cx.new(|cx| {
            SimpleInputState::new(cx)
                .placeholder("Target directory...")
                .default_value(current.clone())
        });
        self.modal = Some(SftpModal::Move(SftpMoveState {
            idx,
            old_path,
            name,
            current: current.clone(),
            entries: Vec::new(),
            loading: true,
            address_input: address_input.clone(),
        }));
        self.dialog_focus_target = Some(address_input);
        self.dialog_focus_pending = true;
        self.focus_manager.update(cx, |fm, cx| {
            self.workspace.update(cx, |ws, cx| {
                ws.clear_focused_terminal(fm, cx);
            });
        });
        self.load_move_entries(cx);
        cx.notify();
    }

    fn load_move_entries(&mut self, cx: &mut Context<Self>) {
        let (sftp, current) = match (&self.connection_state, &self.modal) {
            (SftpConnectionState::Connected { conn, .. }, Some(SftpModal::Move(m))) => {
                (conn.sftp.clone(), m.current.clone())
            }
            _ => return,
        };
        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            let res = run_in_tokio(async move {
                let rd = sftp.read_dir(&current).await?;
                let mut entries: Vec<SftpFile> = Vec::new();
                for entry in rd {
                    let name = entry.file_name();
                    if name == "." || name == ".." {
                        continue;
                    }
                    let metadata = entry.metadata();
                    entries.push(SftpFile {
                        name,
                        is_dir: metadata.is_dir(),
                        size: metadata.size.unwrap_or(0),
                        mtime: metadata.mtime.unwrap_or(0),
                        mode: metadata.permissions.unwrap_or(0),
                        uid: metadata.uid,
                        gid: metadata.gid,
                        user: metadata.user.clone(),
                        group: metadata.group.clone(),
                    });
                }
                entries.retain(|f| f.is_dir);
                entries.sort_by(|a, b| a.name.cmp(&b.name));
                Ok::<_, anyhow::Error>(entries)
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(SftpModal::Move(m)) = &mut this.modal {
                    m.loading = false;
                    if let Ok(entries) = res {
                        m.entries = entries;
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn move_picker_navigate(&mut self, new_dir: String, cx: &mut Context<Self>) {
        if let Some(SftpModal::Move(m)) = &mut self.modal {
            m.current = new_dir;
            m.loading = true;
            m.entries.clear();
            cx.notify();
            self.load_move_entries(cx);
        }
    }

    fn move_picker_enter(&mut self, row_idx: usize, cx: &mut Context<Self>) {
        let (current, name) = match &self.modal {
            Some(SftpModal::Move(m)) if row_idx > 0 && row_idx <= m.entries.len() => {
                (m.current.clone(), m.entries[row_idx - 1].name.clone())
            }
            _ => return,
        };
        self.move_picker_navigate(join_path(&current, &name), cx);
    }

    fn move_picker_parent(&mut self, cx: &mut Context<Self>) {
        let current = match &self.modal {
            Some(SftpModal::Move(m)) => m.current.clone(),
            _ => return,
        };
        self.move_picker_navigate(parent_dir_of(&current), cx);
    }

    fn move_picker_set_dir(&mut self, cx: &mut Context<Self>) {
        let dir = match &self.modal {
            Some(SftpModal::Move(m)) => m.address_input.read(cx).value().trim().to_string(),
            _ => return,
        };
        if !dir.is_empty() {
            self.move_picker_navigate(dir, cx);
        }
    }

    fn submit_move(&mut self, cx: &mut Context<Self>) {
        let (sftp, old_path, new_path) = match &self.modal {
            Some(SftpModal::Move(m)) => {
                let new_path = join_path(&m.current, &m.name);
                (
                    match &self.connection_state {
                        SftpConnectionState::Connected { conn, .. } => conn.sftp.clone(),
                        _ => return,
                    },
                    m.old_path.clone(),
                    new_path,
                )
            }
            _ => return,
        };
        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            let res = run_in_tokio(async move { sftp.rename(&old_path, &new_path).await }).await;
            let _ = this.update(cx, |this, cx| {
                if res.is_ok() {
                    this.close_modal(cx);
                    this.context_menu = None;
                    this.refresh(cx);
                }
            });
        })
        .detach();
    }

    // ─── New Link (symlink) ────────────────────────────────────────────────────
    fn open_link_dialog(&mut self, cx: &mut Context<Self>) {
        let name_input = cx.new(|cx| SimpleInputState::new(cx).placeholder("Link name..."));
        let target_input = cx.new(|cx| SimpleInputState::new(cx).placeholder("Target path..."));
        self.modal = Some(SftpModal::Link(SftpLinkState {
            name_input: name_input.clone(),
            target_input,
        }));
        self.dialog_focus_target = Some(name_input);
        self.dialog_focus_pending = true;
        self.focus_manager.update(cx, |fm, cx| {
            self.workspace.update(cx, |ws, cx| {
                ws.clear_focused_terminal(fm, cx);
            });
        });
        cx.notify();
    }

    fn submit_link(&mut self, cx: &mut Context<Self>) {
        let (sftp, link_path, target_path) = match &self.modal {
            Some(SftpModal::Link(s)) => {
                let name = s.name_input.read(cx).value().trim().to_string();
                let target = s.target_input.read(cx).value().trim().to_string();
                if name.is_empty() || target.is_empty() {
                    return;
                }
                let dir = self.current_dir();
                (
                    match &self.connection_state {
                        SftpConnectionState::Connected { conn, .. } => conn.sftp.clone(),
                        _ => return,
                    },
                    join_path(&dir, &name),
                    target,
                )
            }
            _ => return,
        };
        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            // symlink(linkpath, targetpath): create `link_path` pointing at `target_path`.
            let res = run_in_tokio(async move { sftp.symlink(link_path, target_path).await }).await;
            let _ = this.update(cx, |this, cx| {
                if res.is_ok() {
                    this.close_modal(cx);
                    this.context_menu = None;
                    this.refresh(cx);
                }
            });
        })
        .detach();
    }

    // ─── Clipboard copy ────────────────────────────────────────────────────────
    fn copy_path_of(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some((path, _, _)) = self.abs_path(idx) {
            cx.write_to_clipboard(ClipboardItem::new_string(path));
        }
        self.context_menu = None;
        cx.notify();
    }

    fn copy_name_of(&mut self, idx: usize, cx: &mut Context<Self>) {
        let name = match self.abs_path(idx) {
            Some((_, _, true)) => "..".to_string(),
            Some((_, _, false)) => self
                .filtered_files()
                .get(idx - 1)
                .map(|f| f.name.clone())
                .unwrap_or_default(),
            None => return,
        };
        cx.write_to_clipboard(ClipboardItem::new_string(name));
        self.context_menu = None;
        cx.notify();
    }

    fn copy_current_path(&mut self, cx: &mut Context<Self>) {
        let dir = self.current_dir();
        cx.write_to_clipboard(ClipboardItem::new_string(dir));
        self.context_menu = None;
        cx.notify();
    }

    // ─── Terminal path paste ───────────────────────────────────────────────────
    fn format_path_for_terminal(path: &str) -> String {
        if path.chars().any(|c| {
            c.is_whitespace()
                || c == '$'
                || c == '`'
                || c == '"'
                || c == '\\'
                || c == '('
                || c == ')'
                || c == '&'
                || c == ';'
                || c == '<'
                || c == '>'
                || c == '*'
                || c == '?'
                || c == '['
                || c == ']'
                || c == '{'
                || c == '}'
                || c == '~'
                || c == '#'
                || c == '!'
                || c == '|'
                || c == '\''
        }) {
            format!("'{}'", path.replace('\'', "'\\''"))
        } else {
            path.to_string()
        }
    }

    fn send_path_to_terminal(&mut self, path: &str) {
        if let Some(ref tid) = self.active_terminal_id {
            if let Some(term) = self.terminals.lock().get(tid) {
                let formatted = Self::format_path_for_terminal(path);
                term.send_paste(&formatted);
            }
        }
    }

    fn send_path_of(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some((path, _, _)) = self.abs_path(idx) {
            self.send_path_to_terminal(&path);
        }
        self.context_menu = None;
        cx.notify();
    }

    fn send_current_path_to_terminal(&mut self, cx: &mut Context<Self>) {
        let dir = self.current_dir();
        self.send_path_to_terminal(&dir);
        self.context_menu = None;
        cx.notify();
    }

    // ─── Properties ────────────────────────────────────────────────────────────
    fn show_properties_of(&mut self, idx: usize, cx: &mut Context<Self>) {
        let info = match self.abs_path(idx) {
            Some((path, is_dir, is_parent)) => {
                let (name, size, mtime) = if is_parent {
                    ("..".to_string(), 0, 0)
                } else {
                    let f = &self.filtered_files()[idx - 1];
                    (f.name.clone(), f.size, f.mtime)
                };
                SftpPropertiesState {
                    name,
                    is_dir,
                    path,
                    size,
                    mtime,
                    ctime: None,
                }
            }
            None => return,
        };
        self.modal = Some(SftpModal::Properties(info));
        self.context_menu = None;
        self.focus_manager.update(cx, |fm, cx| {
            self.workspace.update(cx, |ws, cx| {
                ws.clear_focused_terminal(fm, cx);
            });
        });
        cx.notify();
    }

    fn trigger_upload(&mut self, cx: &mut Context<Self>) {
        let paths_future = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select File to Upload".into()),
        });

        // Hoist the transfer-store entity from the outer `Context` so it can be
        // captured by the `cx.spawn` closure below (`cx.global()` isn't
        // callable on `&mut AsyncApp` inside async spawns).
        let store = cx.global::<GlobalTransferStore>().0.clone();

        let (sftp, current_dir) = match &self.connection_state {
            SftpConnectionState::Connected {
                conn, current_dir, ..
            } => (conn.sftp.clone(), current_dir.clone()),
            _ => return,
        };

        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            if let Ok(Ok(Some(selected_paths))) = paths_future.await
                && let Some(local_path) = selected_paths.first()
            {
                let filename = local_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let mut remote_path = current_dir.clone();
                if !remote_path.ends_with('/') {
                    remote_path.push('/');
                }
                remote_path.push_str(&filename);

                match std::fs::read(local_path) {
                    Ok(bytes) => {
                        let total = bytes.len() as u64;
                        let local_display = local_path.to_string_lossy().to_string();
                        let id = next_transfer_id();
                        store.update(cx, |s, cx| {
                            s.add(TransferTask {
                                id: id.clone(),
                                name: filename.clone(),
                                direction: TransferDirection::Upload,
                                local_path: local_display,
                                remote_path: remote_path.clone(),
                                total_bytes: total,
                                transferred_bytes: 0,
                                status: TransferStatus::Active,
                                speed_bps: 0.0,
                                error: None,
                            });
                            cx.notify();
                        });

                        // Shared progress published by the SFTP I/O task and
                        // polled by a GPUI ticker so the list updates live.
                        let progress = Arc::new(Mutex::new(TransferProgress::default()));
                        let store_tick = store.clone();
                        let id_tick = id.clone();
                        let prog_tick = progress.clone();
                        cx.spawn(async move |cx| {
                            loop {
                                smol::Timer::after(Duration::from_millis(200)).await;
                                let p = prog_tick.lock().clone();
                                store_tick.update(cx, |s, cx| {
                                    if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id_tick) {
                                        t.transferred_bytes = p.transferred;
                                        t.total_bytes = p.total;
                                        t.speed_bps = p.speed_bps;
                                        if p.done {
                                            t.status = p.status;
                                            t.error = p.error.clone();
                                        }
                                    }
                                    cx.notify();
                                });
                                if p.done {
                                    break;
                                }
                            }
                        })
                        .detach();

                        let remote_path_io = remote_path.clone();
                        let prog_io = progress.clone();
                        let res = run_in_tokio(async move {
                            use russh_sftp::protocol::{FileAttributes, OpenFlags};
                            use tokio::io::AsyncWriteExt;
                            let create_res = sftp
                                .open_with_flags_and_attributes(
                                    &remote_path_io,
                                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                                    FileAttributes::default(),
                                )
                                .await;
                            let mut done_status = TransferStatus::Complete;
                            let mut done_error: Option<String> = None;
                            let ok = match create_res {
                                Ok(mut file) => {
                                    let start = Instant::now();
                                    let chunk = 64 * 1024;
                                    let len = bytes.len();
                                    let mut pos = 0usize;
                                    let mut success = true;
                                    while pos < len {
                                        let end = (pos + chunk).min(len);
                                        match file.write_all(&bytes[pos..end]).await {
                                            Ok(()) => {
                                                pos = end;
                                                let transferred = pos as u64;
                                                let elapsed =
                                                    start.elapsed().as_secs_f64().max(0.001);
                                                let mut p = prog_io.lock();
                                                p.transferred = transferred;
                                                p.speed_bps = transferred as f64 / elapsed;
                                            }
                                            Err(e) => {
                                                done_status = TransferStatus::Error;
                                                done_error = Some(format!("{:?}", e));
                                                success = false;
                                                break;
                                            }
                                        }
                                    }
                                    success
                                }
                                Err(e) => {
                                    done_status = TransferStatus::Error;
                                    done_error = Some(format!("{:?}", e));
                                    false
                                }
                            };
                            let mut p = prog_io.lock();
                            p.done = true;
                            p.status = if ok {
                                TransferStatus::Complete
                            } else {
                                done_status
                            };
                            p.error = if ok { None } else { done_error };
                            ok
                        })
                        .await;

                        let _ = this.update(cx, |this, cx| {
                            if res {
                                this.refresh(cx);
                            }
                        });
                    }
                    Err(e) => {
                        let id = next_transfer_id();
                        let name = local_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let local_display = local_path.to_string_lossy().to_string();
                        store.update(cx, |s, cx| {
                            s.add(TransferTask {
                                id: id.clone(),
                                name,
                                direction: TransferDirection::Upload,
                                local_path: local_display,
                                remote_path: remote_path.clone(),
                                total_bytes: 0,
                                transferred_bytes: 0,
                                status: TransferStatus::Error,
                                speed_bps: 0.0,
                                error: Some(format!("{:?}", e)),
                            });
                            cx.notify();
                        });
                    }
                }
            }
        })
        .detach();
    }

    pub fn upload_local_paths(&mut self, local_paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        if local_paths.is_empty() {
            return;
        }

        let store = cx.global::<GlobalTransferStore>().0.clone();

        let (sftp, current_dir) = match &self.connection_state {
            SftpConnectionState::Connected {
                conn, current_dir, ..
            } => (conn.sftp.clone(), current_dir.clone()),
            _ => return,
        };

        let paths = local_paths.to_vec();

        cx.spawn(async move |this: WeakEntity<BottomPanel>, cx| {
            for local_path in paths {
                let filename = local_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if filename.is_empty() {
                    continue;
                }

                let mut remote_base = current_dir.clone();
                if !remote_base.ends_with('/') {
                    remote_base.push('/');
                }

                let is_dir = local_path.is_dir();
                if is_dir {
                    let root_remote = format!("{}{}", remote_base, filename);
                    let mut file_list = Vec::new();
                    let mut stack = vec![(local_path.clone(), root_remote)];

                    while let Some((local_dir, remote_dir)) = stack.pop() {
                        let _ = run_in_tokio({
                            let sftp = sftp.clone();
                            let remote_dir = remote_dir.clone();
                            async move {
                                let _ = sftp.create_dir(&remote_dir).await;
                            }
                        })
                        .await;

                        if let Ok(entries) = std::fs::read_dir(&local_dir) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                let name = entry.file_name().to_string_lossy().to_string();
                                let child_remote = format!("{}/{}", remote_dir, name);
                                if path.is_dir() {
                                    stack.push((path, child_remote));
                                } else if path.is_file() {
                                    file_list.push((path, child_remote, name));
                                }
                            }
                        }
                    }

                    for (local_file, remote_file_path, file_name) in file_list {
                        Self::upload_single_file_task(
                            &sftp,
                            &local_file,
                            &remote_file_path,
                            &file_name,
                            &store,
                            &this,
                            cx,
                        )
                        .await;
                    }
                } else {
                    let remote_file_path = format!("{}{}", remote_base, filename);
                    Self::upload_single_file_task(
                        &sftp,
                        &local_path,
                        &remote_file_path,
                        &filename,
                        &store,
                        &this,
                        cx,
                    )
                    .await;
                }
            }

            let _ = this.update(cx, |this, cx| {
                this.refresh(cx);
            });
        })
        .detach();
    }

    async fn upload_single_file_task(
        sftp: &Arc<russh_sftp::client::SftpSession>,
        local_path: &std::path::PathBuf,
        remote_path: &str,
        filename: &str,
        store: &Entity<crate::transfer_store::TransferStore>,
        _this: &WeakEntity<BottomPanel>,
        cx: &mut AsyncApp,
    ) {
        let total = std::fs::metadata(local_path).map(|m| m.len()).unwrap_or(0);
        let local_display = local_path.to_string_lossy().to_string();
        let id = next_transfer_id();
        store.update(cx, |s, cx| {
            s.add(TransferTask {
                id: id.clone(),
                name: filename.to_string(),
                direction: TransferDirection::Upload,
                local_path: local_display,
                remote_path: remote_path.to_string(),
                total_bytes: total,
                transferred_bytes: 0,
                status: TransferStatus::Active,
                speed_bps: 0.0,
                error: None,
            });
            cx.notify();
        });

        let progress = Arc::new(Mutex::new(TransferProgress::default()));
        let store_tick = store.clone();
        let id_tick = id.clone();
        let prog_tick = progress.clone();
        cx.spawn(async move |cx| {
            loop {
                smol::Timer::after(Duration::from_millis(200)).await;
                let p = prog_tick.lock().clone();
                store_tick.update(cx, |s, cx| {
                    if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id_tick) {
                        t.transferred_bytes = p.transferred;
                        t.total_bytes = if p.total > 0 { p.total } else { total };
                        t.speed_bps = p.speed_bps;
                        if p.done {
                            t.status = p.status;
                            t.error = p.error.clone();
                        }
                    }
                    cx.notify();
                });
                if p.done {
                    break;
                }
            }
        })
        .detach();

        let local_path_io = local_path.clone();
        let remote_path_io = remote_path.to_string();
        let prog_io = progress.clone();
        let sftp = sftp.clone();
        let _res = run_in_tokio(async move {
            use russh_sftp::protocol::{FileAttributes, OpenFlags};
            use std::io::Read;
            use tokio::io::AsyncWriteExt;
            let mut local_file = match std::fs::File::open(&local_path_io) {
                Ok(f) => f,
                Err(e) => {
                    let mut p = prog_io.lock();
                    p.done = true;
                    p.status = TransferStatus::Error;
                    p.error = Some(format!("{:?}", e));
                    return false;
                }
            };
            let create_res = sftp
                .open_with_flags_and_attributes(
                    &remote_path_io,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                    FileAttributes::default(),
                )
                .await;
            let mut done_status = TransferStatus::Complete;
            let mut done_error: Option<String> = None;
            let ok = match create_res {
                Ok(mut file) => {
                    let start = Instant::now();
                    let mut buf = vec![0u8; 64 * 1024];
                    let mut transferred = 0u64;
                    let mut success = true;
                    loop {
                        match local_file.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                match file.write_all(&buf[..n]).await {
                                    Ok(()) => {
                                        transferred += n as u64;
                                        let elapsed = start.elapsed().as_secs_f64().max(0.001);
                                        let mut p = prog_io.lock();
                                        p.transferred = transferred;
                                        p.total = total;
                                        p.speed_bps = transferred as f64 / elapsed;
                                    }
                                    Err(e) => {
                                        done_status = TransferStatus::Error;
                                        done_error = Some(format!("{:?}", e));
                                        success = false;
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                done_status = TransferStatus::Error;
                                done_error = Some(format!("{:?}", e));
                                success = false;
                                break;
                            }
                        }
                    }
                    success
                }
                Err(e) => {
                    done_status = TransferStatus::Error;
                    done_error = Some(format!("{:?}", e));
                    false
                }
            };
            let mut p = prog_io.lock();
            p.done = true;
            p.transferred = if ok { total } else { p.transferred };
            p.total = total;
            p.status = if ok {
                TransferStatus::Complete
            } else {
                done_status
            };
            p.error = if ok { None } else { done_error };
            velowork_core::memory::trim_process_memory();
            ok
        })
        .await;
        let p = progress.lock().clone();
        store.update(cx, |s, cx| {
            if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id) {
                t.transferred_bytes = p.transferred;
                t.total_bytes = total;
                t.speed_bps = p.speed_bps;
                t.status = p.status;
                t.error = p.error;
            }
            cx.notify();
        });
    }

    fn handle_external_drop(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let local_paths: Vec<std::path::PathBuf> = paths.paths().to_vec();
        if !local_paths.is_empty() {
            self.upload_local_paths(&local_paths, cx);
        }
    }

    fn trigger_download(&mut self, cx: &mut Context<Self>) {
        let (sftp, current_dir, filtered) = match &self.connection_state {
            SftpConnectionState::Connected {
                conn,
                current_dir,
                files,
            } => {
                let filtered: Vec<SftpFile> = files
                    .iter()
                    .filter(|f| self.show_hidden || !f.name.starts_with('.'))
                    .cloned()
                    .collect();
                (conn.sftp.clone(), current_dir.clone(), filtered)
            }
            _ => return,
        };

        let store = cx.global::<GlobalTransferStore>().0.clone();

        let row = match self.selected_file_row(filtered.len()) {
            Some(r) => r,
            None => return,
        };

        let file = &filtered[row - 1];
        if file.is_dir {
            return;
        }
        let total_size = file.size;

        let filename = file.name.clone();
        let mut remote_path = current_dir.clone();
        if !remote_path.ends_with('/') {
            remote_path.push('/');
        }
        remote_path.push_str(&filename);

        let paths_future = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select Destination Folder".into()),
        });

        cx.spawn(async move |_this: WeakEntity<BottomPanel>, cx| {
            if let Ok(Ok(Some(selected_paths))) = paths_future.await
                && let Some(local_dir) = selected_paths.first()
            {
                let local_path = local_dir.join(&filename);
                let id = next_transfer_id();
                let name = filename.clone();
                let remote = remote_path.clone();
                let local_display = local_path.to_string_lossy().to_string();
                store.update(cx, |s, cx| {
                    s.add(TransferTask {
                        id: id.clone(),
                        name,
                        direction: TransferDirection::Download,
                        local_path: local_display,
                        remote_path: remote,
                        total_bytes: total_size,
                        transferred_bytes: 0,
                        status: TransferStatus::Active,
                        speed_bps: 0.0,
                        error: None,
                    });
                    cx.notify();
                });

                let progress = Arc::new(Mutex::new(TransferProgress::default()));
                let store_tick = store.clone();
                let id_tick = id.clone();
                let prog_tick = progress.clone();
                cx.spawn(async move |cx| {
                    loop {
                        smol::Timer::after(Duration::from_millis(200)).await;
                        let p = prog_tick.lock().clone();
                        store_tick.update(cx, |s, cx| {
                            if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id_tick) {
                                t.transferred_bytes = p.transferred;
                                t.total_bytes = p.total;
                                t.speed_bps = p.speed_bps;
                                if p.done {
                                    t.status = p.status;
                                    t.error = p.error.clone();
                                }
                            }
                            cx.notify();
                        });
                        if p.done {
                            break;
                        }
                    }
                })
                .detach();

                let remote_path_io = remote_path.clone();
                let local_path_io = local_path.clone();
                let prog_io = progress.clone();
                let res = run_in_tokio(async move {
                    use std::io::Write;
                    use tokio::io::AsyncReadExt;
                    let open_res = sftp.open(&remote_path_io).await;
                    let mut done_status = TransferStatus::Complete;
                    let mut done_error: Option<String> = None;
                    let ok = match open_res {
                        Ok(mut file) => {
                            let mut local_file = match std::fs::File::create(&local_path_io) {
                                Ok(f) => f,
                                Err(e) => {
                                    let mut p = prog_io.lock();
                                    p.done = true;
                                    p.status = TransferStatus::Error;
                                    p.error = Some(format!("{:?}", e));
                                    return false;
                                }
                            };
                            // Use the file size we already resolved from the
                            // SFTP listing (avoids a separate `stat` call).
                            if total_size > 0 {
                                let mut p = prog_io.lock();
                                p.total = total_size;
                            }
                            let start = Instant::now();
                            let mut buf = vec![0u8; 64 * 1024];
                            let mut transferred = 0u64;
                            let mut success = true;
                            loop {
                                match file.read(&mut buf).await {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if let Err(e) = local_file.write_all(&buf[..n]) {
                                            done_status = TransferStatus::Error;
                                            done_error = Some(format!("{:?}", e));
                                            success = false;
                                            break;
                                        }
                                        transferred += n as u64;
                                        let elapsed = start.elapsed().as_secs_f64().max(0.001);
                                        let mut p = prog_io.lock();
                                        p.transferred = transferred;
                                        p.speed_bps = transferred as f64 / elapsed;
                                    }
                                    Err(e) => {
                                        done_status = TransferStatus::Error;
                                        done_error = Some(format!("{:?}", e));
                                        success = false;
                                        break;
                                    }
                                }
                            }
                            let _ = local_file.flush();
                            success
                        }
                        Err(e) => {
                            done_status = TransferStatus::Error;
                            done_error = Some(format!("{:?}", e));
                            false
                        }
                    };
                    let mut p = prog_io.lock();
                    p.done = true;
                    p.status = if ok {
                        TransferStatus::Complete
                    } else {
                        done_status
                    };
                    p.error = done_error;
                    velowork_core::memory::trim_process_memory();
                    ok
                })
                .await;

                let _ = res;
            }
        })
        .detach();
    }

    /// Download a remote file into a per-connection temp directory and then
    /// open it:
    /// - `use_editor == false` → open with the OS default program
    ///   (`file_opener` setting ignored).
    /// - `use_editor == true`  → open with the user-configured `file_opener`
    ///   (falls back to the OS default when unset).
    ///
    /// The temp directory is keyed by the active terminal id, which identifies
    /// the SSH session this SFTP panel is bound to, so each connection gets its
    /// own scratch folder.
    fn open_remote_file(&mut self, use_editor: bool, cx: &mut Context<Self>) {
        // ── Resolve the target file (same selection logic as `trigger_download`) ──
        let (sftp, current_dir, filtered) = match &self.connection_state {
            SftpConnectionState::Connected {
                conn,
                current_dir,
                files,
            } => {
                let filtered: Vec<SftpFile> = files
                    .iter()
                    .filter(|f| self.show_hidden || !f.name.starts_with('.'))
                    .cloned()
                    .collect();
                (conn.sftp.clone(), current_dir.clone(), filtered)
            }
            _ => return,
        };

        let row = match self.selected_file_row(filtered.len()) {
            Some(r) => r,
            None => return,
        };
        let file = filtered[row - 1].clone();
        if file.is_dir {
            return;
        }
        let total_size = file.size;
        let filename = file.name.clone();
        let mut remote_path = current_dir.clone();
        if !remote_path.ends_with('/') {
            remote_path.push('/');
        }
        remote_path.push_str(&filename);

        // ── Per-connection temp directory ──
        let conn_id = self
            .active_terminal_id
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let sanitized: String = conn_id
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let temp_dir = std::env::temp_dir()
            .join("velowork-sftp")
            .join(sanitized);
        let _ = std::fs::create_dir_all(&temp_dir);
        let local_path = temp_dir.join(&filename);
        let local_display = local_path.to_string_lossy().to_string();

        // ── Choose the opener ──
        let opener = if use_editor {
            crate::terminal_view_settings(cx).file_opener.clone()
        } else {
            String::new() // empty ⇒ OS default program
        };

        // ── Register a transfer task so it shows in the monitor ──
        let store = cx.global::<GlobalTransferStore>().0.clone();
        let id = next_transfer_id();
        let name = filename.clone();
        let remote = remote_path.clone();
        let local_for_task = local_display.clone();
        store.update(cx, |s, cx| {
            s.add(TransferTask {
                id: id.clone(),
                name,
                direction: TransferDirection::Download,
                local_path: local_for_task,
                remote_path: remote,
                total_bytes: total_size,
                transferred_bytes: 0,
                status: TransferStatus::Active,
                speed_bps: 0.0,
                error: None,
            });
            cx.notify();
        });

        cx.spawn(async move |panel: WeakEntity<BottomPanel>, cx| {
            // Progress ticker (mirrors `trigger_download`).
            let progress = Arc::new(Mutex::new(TransferProgress::default()));
            let store_tick = store.clone();
            let id_tick = id.clone();
            let prog_tick = progress.clone();
            cx.spawn(async move |cx| {
                loop {
                    smol::Timer::after(Duration::from_millis(200)).await;
                    let p = prog_tick.lock().clone();
                    store_tick.update(cx, |s, cx| {
                        if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id_tick) {
                            t.transferred_bytes = p.transferred;
                            t.total_bytes = p.total;
                            t.speed_bps = p.speed_bps;
                            if p.done {
                                t.status = p.status;
                                t.error = p.error.clone();
                            }
                        }
                        cx.notify();
                    });
                    if p.done {
                        break;
                    }
                }
            })
            .detach();

            let remote_path_io = remote_path.clone();
            let local_path_io = local_path.clone();
            let prog_io = progress.clone();
        let opener_io = opener.clone();
        let local_display_io = local_display.clone();
        let sftp_for_watch = sftp.clone();
        let res = run_in_tokio(async move {
                use std::io::Write;
                use tokio::io::AsyncReadExt;
                let open_res = sftp.open(&remote_path_io).await;
                let mut done_status = TransferStatus::Complete;
                let mut done_error: Option<String> = None;
                let ok = match open_res {
                    Ok(mut file) => {
                        let mut local_file = match std::fs::File::create(&local_path_io) {
                            Ok(f) => f,
                            Err(e) => {
                                let mut p = prog_io.lock();
                                p.done = true;
                                p.status = TransferStatus::Error;
                                p.error = Some(format!("{:?}", e));
                                return false;
                            }
                        };
                        if total_size > 0 {
                            let mut p = prog_io.lock();
                            p.total = total_size;
                        }
                        let start = Instant::now();
                        let mut buf = vec![0u8; 64 * 1024];
                        let mut transferred = 0u64;
                        let mut success = true;
                        loop {
                            match file.read(&mut buf).await {
                                Ok(0) => break,
                                Ok(n) => {
                                    if let Err(e) = local_file.write_all(&buf[..n]) {
                                        done_status = TransferStatus::Error;
                                        done_error = Some(format!("{:?}", e));
                                        success = false;
                                        break;
                                    }
                                    transferred += n as u64;
                                    let elapsed = start.elapsed().as_secs_f64().max(0.001);
                                    let mut p = prog_io.lock();
                                    p.transferred = transferred;
                                    p.speed_bps = transferred as f64 / elapsed;
                                }
                                Err(e) => {
                                    done_status = TransferStatus::Error;
                                    done_error = Some(format!("{:?}", e));
                                    success = false;
                                    break;
                                }
                            }
                        }
                        let _ = local_file.flush();
                        success
                    }
                    Err(e) => {
                        done_status = TransferStatus::Error;
                        done_error = Some(format!("{:?}", e));
                        false
                    }
                };
                let mut p = prog_io.lock();
                p.done = true;
                p.status = if ok {
                    TransferStatus::Complete
                } else {
                    done_status
                };
                p.error = done_error;
                velowork_core::memory::trim_process_memory();
                ok
            })
            .await;

            if res {
                UrlDetector::open_file(&local_display_io, None, None, &opener_io);

                // ── Watch the local temp copy; offer to upload back on save ──
                let watch_local = local_path.clone();
                let watch_remote = remote_path.clone();
                let watch_sftp = sftp_for_watch.clone();
                cx.spawn(async move |cx| {
                    // Baseline = state right after the download write, so the
                    // initial open doesn't count as a "change".
                    let mut baseline = std::fs::metadata(&watch_local)
                        .ok()
                        .map(|m| (m.modified().ok(), m.len()));
                    loop {
                        smol::Timer::after(Duration::from_secs(1)).await;
                        let meta = match std::fs::metadata(&watch_local) {
                            Ok(m) => m,
                            Err(_) => break, // temp file removed → stop watching
                        };
                        let changed = match baseline {
                            Some((Some(bt), bl)) => {
                                meta.modified().ok() != Some(bt) || meta.len() != bl
                            }
                            _ => false,
                        };
                        if changed {
                            let dialog_open = panel
                                .update(cx, |this, _cx| this.confirm_dialog.is_some())
                                .unwrap_or(false);
                            if !dialog_open {
                                panel
                                    .update(cx, |this, cx| {
                                        this.request_upload_confirm(
                                            watch_local.clone(),
                                            watch_remote.clone(),
                                            watch_sftp.clone(),
                                            cx,
                                        );
                                    })
                                    .ok();
                            }
                            // Reset baseline so the same save doesn't re-prompt.
                            baseline = Some((meta.modified().ok(), meta.len()));
                        }
                    }
                })
                .detach();
            }
        })
        .detach();
    }

    /// Show a confirmation dialog offering to upload a locally-edited temp copy
    /// back to the remote server. Triggered by the file watcher installed in
    /// `open_remote_file` after the user saves the local file.
    fn request_upload_confirm(
        &mut self,
        local_path: std::path::PathBuf,
        remote_path: String,
        sftp: Arc<russh_sftp::client::SftpSession>,
        cx: &mut Context<Self>,
    ) {
        let title = i18n!(cx, "sftp.upload_confirm_title");
        let name = local_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let message = i18n!(cx, "sftp.upload_confirm")
            .replace("{name}", &name)
            .replace("{path}", &remote_path);

        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                title,
                message,
                i18n!(cx, "sftp.upload"),
                i18n!(cx, "common.cancel"),
                false,
                self.overlay_registry.clone(),
                "sftp-upload-confirm",
            )
        });

        cx.subscribe(&dialog, {
            let local_path = local_path.clone();
            let remote_path = remote_path.clone();
            let sftp = sftp.clone();
            move |this, _dialog, event, cx| {
                if matches!(event, ConfirmDialogEvent::Confirmed { .. }) {
                    let local_path = local_path.clone();
                    let remote_path = remote_path.clone();
                    let sftp = sftp.clone();
                    this.upload_file_to_remote(local_path, remote_path, sftp, cx);
                }
                this.confirm_dialog = None;
                cx.notify();
            }
        })
        .detach();

        self.confirm_dialog = Some(dialog);
        cx.notify();
    }

    /// Upload a locally-edited temp file back to the remote path, reusing the
    /// same transfer pipeline as a normal upload (progress monitor, refresh).
    fn upload_file_to_remote(
        &mut self,
        local_path: std::path::PathBuf,
        remote_path: String,
        sftp: Arc<russh_sftp::client::SftpSession>,
        cx: &mut Context<Self>,
    ) {
        let store = cx.global::<GlobalTransferStore>().0.clone();
        let filename = local_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let this_weak = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<BottomPanel>, cx| {
            Self::upload_single_file_task(
                &sftp,
                &local_path,
                &remote_path,
                &filename,
                &store,
                &this_weak,
                cx,
            )
            .await;
            let _ = this_weak.update(cx, |this, cx| this.refresh(cx));
        })
        .detach();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();

        // 行数 = 父目录 ".." 项(1) + 可见文件数；键盘导航范围与渲染/操作一致。
        let row_count = self.filtered_files().len() + 1;

        match key {
            "up" => {
                cx.stop_propagation();
                if let Some(n) = self.selection.move_up(0) {
                    scroll_to_row(&self.scroll_handle, n);
                    cx.notify();
                }
            }
            "down" => {
                cx.stop_propagation();
                if let Some(n) = self.selection.move_down(row_count) {
                    scroll_to_row(&self.scroll_handle, n);
                    cx.notify();
                }
            }
            "enter" => {
                cx.stop_propagation();
                let alt = event.keystroke.modifiers.alt;
                // 行内重命名进行中则提交重命名。
                if self.inline_rename.is_some() {
                    self.submit_rename(cx);
                } else if let Some(idx) = self.selection.active() {
                    if alt {
                        // Alt+Enter：打开文件/目录属性。
                        self.show_properties_of(idx, cx);
                    } else {
                        self.open_item(idx, cx);
                    }
                }
            }
            "space" => {
                // 空格：打开当前选中项。文件 → 下载并打开（对应右键「打开」）；
                // 目录 → 进入该目录。行内重命名进行中不触发，父目录项不响应。
                if self.inline_rename.is_none() {
                    if let Some(idx) = self.selection.active() {
                        if idx != 0 {
                            let files = self.filtered_files();
                            let file_idx = idx - 1;
                            if file_idx < files.len() {
                                let file = &files[file_idx];
                                cx.stop_propagation();
                                if file.is_dir {
                                    let mut target_dir =
                                        match &self.connection_state {
                                            SftpConnectionState::Connected {
                                                current_dir,
                                                ..
                                            } => current_dir.clone(),
                                            _ => return,
                                        };
                                    if !target_dir.ends_with('/') {
                                        target_dir.push('/');
                                    }
                                    target_dir.push_str(&file.name);
                                    self.change_directory(target_dir, cx);
                                } else {
                                    self.open_remote_file(false, cx);
                                }
                            }
                        }
                    }
                }
            }
            "escape" => {
                // Cancel inline rename first, then modals
                if self.inline_rename.is_some() {
                    self.inline_rename = None;
                    self.focus_self_pending = true;
                    cx.notify();
                } else if self.modal.is_some() || self.active_dialog.is_some() {
                    self.close_modal(cx);
                    self.close_active_dialog(cx);
                } else if !self.selection.is_empty() {
                    self.selection.clear();
                    cx.notify();
                }
            }
            // F2：重命名当前选中项（行内编辑）。重命名进行中不再重复触发。
            "f2" => {
                cx.stop_propagation();
                if self.inline_rename.is_none() {
                    if let Some(idx) = self.selection.active() {
                        // idx 0 为 ".." 父目录项，不可重命名。
                        if idx != 0 {
                            self.open_rename_dialog(idx, cx);
                        }
                    }
                }
            }
            // Delete：删除当前选中项（弹窗确认在删除流程内处理）。
            "delete" => {
                cx.stop_propagation();
                if self.inline_rename.is_none() {
                    if let Some(idx) = self.selection.active() {
                        if idx != 0 {
                            self.request_delete_confirm(idx, cx);
                        }
                    }
                }
            }
            // Backspace：返回上一级目录（文件管理器惯例；行内重命名时
            // 不拦截，让输入框正常删除字符）。
            "backspace" => {
                if self.inline_rename.is_none() {
                    cx.stop_propagation();
                    self.go_to_parent_directory(cx);
                }
            }
            // F5：刷新当前目录列表。
            "f5" => {
                cx.stop_propagation();
                self.refresh(cx);
            }
            _ => {}
        }
    }

    fn render_creation_dialog(
        &self,
        dialog: &SftpDialogState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let title: SharedString = if dialog.is_dir {
            i18n!(cx, "common.new_folder")
        } else {
            i18n!(cx, "sftp.dialog.create_file")
        }
        .into();

        let card = modal_content("sftp-create-modal", cx)
            .relative()
            .w(px(320.0))
            .child(modal_header(
                title,
                None::<&str>,
                &t,
                cx,
                cx.listener(|this, _, _, cx| {
                    this.close_active_dialog(cx);
                }),
            ))
            .child(
                div()
                    .p(SPACE_MD)
                    .flex()
                    .flex_col()
                    .gap(SPACE_SM)
                    .child(
                        labeled_input(&i18n!(cx, "sftp.dialog.name"), &t, cx).child(
                            SimpleInput::new(&dialog.name_input).text_size(ui_text_md(cx)),
                        ),
                    )
                    // 重名冲突等错误提示（红色），用户修改名称后由订阅回调清除。
                    .when_some(dialog.error.clone(), |el, err| {
                        el.child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(p.status_error)
                                .child(err.to_string()),
                        )
                    })
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .text_color(p.text_secondary)
                            .child(i18n!(cx, "sftp.dialog.permissions")),
                    )
                    .child(
                        h_flex()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .w(px(50.0))
                                    .text_size(ui_text_md(cx))
                                    .text_color(p.text_muted)
                                    .child(i18n!(cx, "sftp.dialog.owner")),
                            )
                            .child(checkbox(
                                "perm-u-r",
                                dialog.perm_user.0,
                                i18n!(cx, "sftp.dialog.read"),
                                &t,
                                cx.listener(|this, _, _, cx| {
                                    if let Some(ref mut d) = this.active_dialog {
                                        d.perm_user.0 = !d.perm_user.0;
                                        cx.notify();
                                    }
                                }),
                                cx,
                            ))
                            .child(checkbox(
                                "perm-u-w",
                                dialog.perm_user.1,
                                i18n!(cx, "sftp.dialog.write"),
                                &t,
                                cx.listener(|this, _, _, cx| {
                                    if let Some(ref mut d) = this.active_dialog {
                                        d.perm_user.1 = !d.perm_user.1;
                                        cx.notify();
                                    }
                                }),
                                cx,
                            ))
                            .child(checkbox(
                                "perm-u-x",
                                dialog.perm_user.2,
                                i18n!(cx, "sftp.dialog.execute"),
                                &t,
                                cx.listener(|this, _, _, cx| {
                                    if let Some(ref mut d) = this.active_dialog {
                                        d.perm_user.2 = !d.perm_user.2;
                                        cx.notify();
                                    }
                                }),
                                cx,
                            )),
                    )
                    .child(
                        h_flex()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .w(px(50.0))
                                    .text_size(ui_text_md(cx))
                                    .text_color(p.text_muted)
                                    .child(i18n!(cx, "sftp.dialog.group")),
                            )
                            .child(checkbox(
                                "perm-g-r",
                                dialog.perm_group.0,
                                i18n!(cx, "sftp.dialog.read"),
                                &t,
                                cx.listener(|this, _, _, cx| {
                                    if let Some(ref mut d) = this.active_dialog {
                                        d.perm_group.0 = !d.perm_group.0;
                                        cx.notify();
                                    }
                                }),
                                cx,
                            ))
                            .child(checkbox(
                                "perm-g-w",
                                dialog.perm_group.1,
                                i18n!(cx, "sftp.dialog.write"),
                                &t,
                                cx.listener(|this, _, _, cx| {
                                    if let Some(ref mut d) = this.active_dialog {
                                        d.perm_group.1 = !d.perm_group.1;
                                        cx.notify();
                                    }
                                }),
                                cx,
                            ))
                            .child(checkbox(
                                "perm-g-x",
                                dialog.perm_group.2,
                                i18n!(cx, "sftp.dialog.execute"),
                                &t,
                                cx.listener(|this, _, _, cx| {
                                    if let Some(ref mut d) = this.active_dialog {
                                        d.perm_group.2 = !d.perm_group.2;
                                        cx.notify();
                                    }
                                }),
                                cx,
                            )),
                    )
                    .child(
                        h_flex()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .w(px(50.0))
                                    .text_size(ui_text_md(cx))
                                    .text_color(p.text_muted)
                                    .child(i18n!(cx, "sftp.dialog.others")),
                            )
                            .child(checkbox(
                                "perm-o-r",
                                dialog.perm_other.0,
                                i18n!(cx, "sftp.dialog.read"),
                                &t,
                                cx.listener(|this, _, _, cx| {
                                    if let Some(ref mut d) = this.active_dialog {
                                        d.perm_other.0 = !d.perm_other.0;
                                        cx.notify();
                                    }
                                }),
                                cx,
                            ))
                            .child(checkbox(
                                "perm-o-w",
                                dialog.perm_other.1,
                                i18n!(cx, "sftp.dialog.write"),
                                &t,
                                cx.listener(|this, _, _, cx| {
                                    if let Some(ref mut d) = this.active_dialog {
                                        d.perm_other.1 = !d.perm_other.1;
                                        cx.notify();
                                    }
                                }),
                                cx,
                            ))
                            .child(checkbox(
                                "perm-o-x",
                                dialog.perm_other.2,
                                i18n!(cx, "sftp.dialog.execute"),
                                &t,
                                cx.listener(|this, _, _, cx| {
                                    if let Some(ref mut d) = this.active_dialog {
                                        d.perm_other.2 = !d.perm_other.2;
                                        cx.notify();
                                    }
                                }),
                                cx,
                            )),
                    ),
            )
            .child(
                h_flex()
                    .p(SPACE_SM)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .justify_end()
                    .gap(SPACE_SM)
                    .child(
                        button("cancel-creation-btn", &i18n!(cx, "common.cancel"), &t).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.close_active_dialog(cx);
                            }),
                        ),
                    )
                    .child(
                        button_primary("save-creation-btn", &i18n!(cx, "sftp.dialog.create"), &t)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.submit_creation_dialog(cx);
                            })),
                    ),
            );

        modal_backdrop("sftp-create-backdrop", &t, cx)
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_active_dialog(cx);
                }),
            )
            .child(card)
    }

    fn render_modal(&self, m: &SftpModal, cx: &mut Context<Self>) -> gpui::AnyElement {
        match m {
            SftpModal::Rename(s) => self.render_rename_dialog(s, cx).into_any_element(),
            SftpModal::Move(s) => self.render_move_dialog(s, cx).into_any_element(),
            SftpModal::Link(s) => self.render_link_dialog(s, cx).into_any_element(),
            SftpModal::Properties(s) => self.render_properties_dialog(s, cx).into_any_element(),
        }
    }

    fn render_rename_dialog(
        &self,
        state: &SftpRenameState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let title: SharedString = i18n!(cx, "common.rename").into();

        let card =
            modal_content("sftp-rename-modal", cx)
                .relative()
                .w(px(320.0))
                .child(modal_header(
                    title,
                    None::<&str>,
                    &t,
                    cx,
                    cx.listener(|this, _, _, cx| {
                        this.close_modal(cx);
                    }),
                ))
                .child(
                    div()
                        .p(SPACE_MD)
                        .flex()
                        .flex_col()
                        .gap(SPACE_SM)
                        .child(labeled_input(&i18n!(cx, "sftp.dialog.new_name"), &t, cx).child(
                            SimpleInput::new(&state.name_input).text_size(ui_text_md(cx)),
                        ))
                        .child(
                            div()
                                .text_size(ui_text_ms(cx))
                                .text_color(p.text_muted)
                                .child(format!(
                                    "{} {}",
                                    i18n!(cx, "sftp.dialog.current"),
                                    state.old_name
                                )),
                        ),
                )
                .child(
                    h_flex()
                        .p(SPACE_SM)
                        .border_t_1()
                        .border_color(p.border_subtle)
                        .justify_end()
                        .gap(SPACE_SM)
                        .child(
                            button("rename-cancel", &i18n!(cx, "common.cancel"), &t).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.close_modal(cx);
                                }),
                            ),
                        )
                        .child(
                            button_primary("rename-save", &i18n!(cx, "common.rename"), &t)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.submit_rename(cx);
                                })),
                        ),
                );

        modal_backdrop("sftp-rename-backdrop", &t, cx)
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_modal(cx);
                }),
            )
            .child(card)
    }

    fn render_move_dialog(
        &self,
        state: &SftpMoveState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let title: SharedString = i18n!(cx, "sftp.dialog.move").into();

        let card =
            modal_content("sftp-move-modal", cx)
                .relative()
                .w(px(420.0))
                .max_h(px(480.0))
                .flex()
                .flex_col()
                .child(modal_header(
                    title,
                    None::<&str>,
                    &t,
                    cx,
                    cx.listener(|this, _, _, cx| {
                        this.close_modal(cx);
                    }),
                ))
                .child(
                    h_flex()
                        .px(SPACE_MD)
                        .py(SPACE_XS)
                        .gap(SPACE_XS)
                        .child(div().flex_1().child(
                            SimpleInput::new(&state.address_input).text_size(ui_text_md(cx)),
                        ))
                        .child(
                            icon_button("move-go", AppIcon::ChevronRight, &t, cx).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.move_picker_set_dir(cx);
                                }),
                            ),
                        )
                        .child(icon_button("move-up", AppIcon::ChevronUp, &t, cx).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.move_picker_parent(cx);
                            }),
                        )),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.0))
                        .max_h(px(300.0))
                        .flex()
                        .flex_col()
                        .bg(surface_bg_t(t.bg_secondary, &t))
                        .overflow_y_scrollbar()
                        .child(if state.loading {
                            div()
                                .id("move-loading")
                                .p(SPACE_LG)
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_muted)
                                .child(i18n!(cx, "sftp.dialog.loading"))
                                .into_any_element()
                        } else if state.entries.is_empty() {
                            div()
                                .id("move-empty")
                                .p(SPACE_LG)
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_muted)
                                .child(i18n!(cx, "sftp.dialog.no_subdirs"))
                                .into_any_element()
                        } else {
                            div()
                                .id("move-entries")
                                .children((0..state.entries.len()).map(|i| {
                                    let name = state.entries[i].name.clone();
                                    h_flex()
                                        .id(ElementId::Name(format!("move-row-{}", i).into()))
                                        .px(SPACE_LG)
                                        .py(SPACE_SM)
                                        .items_center()
                                        .gap(SPACE_SM)
                                        .cursor_pointer()
                                        .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.move_picker_enter(i + 1, cx);
                                        }))
                                        .on_click(cx.listener(
                                            move |this, event: &ClickEvent, _, cx| {
                                                if event.click_count() == 2 {
                                                    this.move_picker_enter(i + 1, cx);
                                                }
                                            },
                                        ))
                                        .child(
                                            AppIcon::Folder
                                                .size(ICON_STD)
                                                .flex_shrink_0()
                                                .text_color(p.text_secondary),
                                        )
                                        .child(
                                            div()
                                                .text_size(ui_text_md(cx))
                                                .text_color(p.text_primary)
                                                .child(name),
                                        )
                                        .into_any_element()
                                }))
                                .into_any_element()
                        }),
                )
                .child(
                    h_flex()
                        .p(SPACE_SM)
                        .border_t_1()
                        .border_color(p.border_subtle)
                        .justify_between()
                        .gap(SPACE_SM)
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_muted)
                                .child(format!(
                                    "\"{}\" {}",
                                    state.name,
                                    i18n!(cx, "sftp.dialog.move_hint")
                                )),
                        )
                        .child(
                            h_flex()
                                .gap(SPACE_SM)
                                .child(
                                    button("move-cancel", &i18n!(cx, "common.cancel"), &t)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_modal(cx);
                                        }))
                                )
                                .child(
                                    button_primary(
                                        "move-confirm",
                                        &i18n!(cx, "sftp.dialog.move_here"),
                                        &t,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.submit_move(cx);
                                        },
                                    )),
                                ),
                        ),
                );

        modal_backdrop("sftp-move-backdrop", &t, cx)
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_modal(cx);
                }),
            )
            .child(card)
    }

    fn render_link_dialog(
        &self,
        state: &SftpLinkState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let title: SharedString = i18n!(cx, "sftp.dialog.new_link").into();

        let card = modal_content("sftp-link-modal", cx)
            .relative()
            .w(px(360.0))
            .child(modal_header(
                title,
                None::<&str>,
                &t,
                cx,
                cx.listener(|this, _, _, cx| {
                    this.close_modal(cx);
                }),
            ))
            .child(
                div()
                    .p(SPACE_MD)
                    .flex()
                    .flex_col()
                    .gap(SPACE_SM)
                    .child(
                        labeled_input(&i18n!(cx, "sftp.dialog.link_name"), &t, cx).child(
                            SimpleInput::new(&state.name_input).text_size(ui_text_md(cx)),
                        ),
                    )
                    .child(
                        labeled_input(&i18n!(cx, "sftp.dialog.link_target"), &t, cx).child(
                            SimpleInput::new(&state.target_input).text_size(ui_text_md(cx)),
                        ),
                    )
                    .child(
                        div()
                            .text_size(ui_text_ms(cx))
                            .text_color(p.text_muted)
                            .child(i18n!(cx, "sftp.dialog.link_hint")),
                    ),
            )
            .child(
                h_flex()
                    .p(SPACE_SM)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .justify_end()
                    .gap(SPACE_SM)
                    .child(
                        button("link-cancel", &i18n!(cx, "common.cancel"), &t).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.close_modal(cx);
                            }),
                        ),
                    )
                    .child(
                        button_primary("link-create", &i18n!(cx, "sftp.dialog.create"), &t)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.submit_link(cx);
                            })),
                    ),
            );

        modal_backdrop("sftp-link-backdrop", &t, cx)
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_modal(cx);
                }),
            )
            .child(card)
    }

    fn render_properties_dialog(
        &self,
        state: &SftpPropertiesState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let ptype = if state.is_dir {
            i18n!(cx, "sftp.props.directory")
        } else {
            i18n!(cx, "sftp.props.name")
        };
        let ctime_str = match state.ctime {
            Some(c) => format_time(c),
            None => i18n!(cx, "sftp.props.ctime_na"),
        };
        let row = |label: &str, value: String| {
            h_flex()
                .justify_between()
                .py(SPACE_XS)
                .gap(SPACE_LG)
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(p.text_secondary)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(p.text_primary)
                        .child(value),
                )
        };

        let card = modal_content("sftp-properties-modal", cx)
            .relative()
            .w(px(380.0))
            .child(modal_header(
                i18n!(cx, "sftp.props.title"),
                None::<&str>,
                &t,
                cx,
                cx.listener(|this, _, _, cx| {
                    this.close_modal(cx);
                }),
            ))
            .child(
                div()
                    .p(SPACE_LG)
                    .flex()
                    .flex_col()
                    .gap(SPACE_XS)
                    .child(row(&i18n!(cx, "sftp.props.name"), state.name.clone()))
                    .child(row(&i18n!(cx, "sftp.props.type"), ptype.to_string()))
                    .child(row(&i18n!(cx, "sftp.props.path"), state.path.clone()))
                    .child(row(&i18n!(cx, "sftp.props.size"), format_size(state.size)))
                    .child(row(
                        &i18n!(cx, "sftp.props.modified"),
                        format_time(state.mtime),
                    ))
                    .child(row(&i18n!(cx, "sftp.props.created"), ctime_str)),
            )
            .child(
                h_flex()
                    .p(SPACE_MD)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .justify_end()
                    .child(
                        button("props-close", &i18n!(cx, "common.close"), &t).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.close_modal(cx);
                            }),
                        ),
                    ),
            );

        modal_backdrop("sftp-properties-backdrop", &t, cx)
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_modal(cx);
                }),
            )
            .child(card)
    }
}

/// Mount a dialog element as a WINDOW-level overlay so its `modal_backdrop`
/// (`absolute().inset_0()`) covers the entire window instead of just
/// the bottom panel.
///
/// `deferred(anchored().snap_to_window())` renders the element in the
/// window's top deferred layer: `anchored` is `Position::Absolute` and
/// `snap_to_window` pins its origin to the window's top-left in window
/// coordinates (painted above all normal content).
///
/// IMPORTANT: an `anchored` layer's size is determined by its *content*
/// (it is not auto-expanded to the viewport), and `modal_backdrop` uses
/// `absolute().inset_0()` — so if the inner element had no definite
/// size the layer would collapse to zero and the card would drop into the
/// top-left corner (the original bug). To give the layer a real, full
/// window size we wrap `inner` in an explicitly window-sized
/// `relative()` div built from `window.viewport_size()`. The `relative()`
/// div then becomes the positioned containing block for `modal_backdrop`,
/// so `inset_0()` fills the whole window. `render` is re-run on window
/// resize, so the explicit size always stays in sync with the viewport.
fn sftp_window_overlay(inner: impl IntoElement, window: &Window) -> impl IntoElement {
    let size = window.viewport_size();
    deferred(
        anchored()
            .position(Point::new(px(0.0), px(0.0)))
            .snap_to_window()
            .child(
                div()
                    .id("sftp-dialog-window-overlay")
                    .relative()
                    .w(size.width)
                    .h(size.height)
                    .child(inner),
            ),
    )
}

impl Focusable for BottomPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for BottomPanel {
    fn metadata(&self, cx: &App) -> PanelInfo {
        PanelInfo::new(
            "sftp",
            i18n!(cx, "sftp.panel.title"),
            AppIcon::Folder,
            PanelKind::Custom,
        )
        .closable(false)
    }

    fn on_open(&mut self, cx: &mut Context<Self>) {
        self.bind_active_terminal(cx);
    }

    fn focus_handle(&self, _cx: &App) -> Option<FocusHandle> {
        Some(self.focus_handle.clone())
    }

    fn toolbar_elements(&self, cx: &App) -> Vec<ToolbarItem> {
        // Helper: same pattern as in commands_panel.rs — avoids closure
        // lifetime issues when embedded in ToolbarItem::IconButton callbacks.
        fn make_click_cb<T: 'static>(
            weak: WeakEntity<T>,
            action: impl Fn(&mut T, &mut Context<T>) + Send + Sync + 'static,
        ) -> Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> {
            Arc::new(move |_: &mut Window, cx: &mut App| {
                if let Some(slf) = weak.upgrade() {
                    let _ = slf.update(cx, |t, cx| action(t, cx));
                }
            })
        }

        let this = self.self_weak.clone();

        let back_cb = make_click_cb(this.clone(), |t, cx| {
            t.go_to_parent_directory(cx);
        });
        let refresh_cb = make_click_cb(this.clone(), |t, cx| {
            t.refresh(cx);
        });
        let hidden_cb = make_click_cb(this.clone(), |_t, cx| {
            let current = velowork_app_core::settings::settings(cx).show_hidden_files;
            velowork_app_core::settings::settings_entity(cx)
                .update(cx, |state, cx| state.set_show_hidden_files(!current, cx));
        });
        let enter_cb = make_click_cb(this.clone(), |t, cx| {
            t.handle_address_enter(cx);
        });
        let auto_sync_cb = make_click_cb(this.clone(), |t, cx| {
            t.auto_sync = !t.auto_sync;
            if t.auto_sync {
                t.check_auto_sync(cx);
            }
            cx.notify();
        });

        let mut items = Vec::new();
        if let Some(ref address_input) = self.address_input {
            items.push(ToolbarItem::TextInput {
                entity: address_input.clone(),
                width_px: 260.0,
                suffix: None,
                on_enter: Some(enter_cb),
            });
        }
        items.push(ToolbarItem::IconButton {
            icon: AppIcon::ArrowUp,
            tooltip: i18n!(cx, "sftp.toolbar.back_parent"),
            enabled: true,
            on_click: back_cb,
            accent: None,
            active: false,
        });
        items.push(ToolbarItem::IconButton {
            icon: AppIcon::Refresh,
            tooltip: i18n!(cx, "common.refresh"),
            enabled: true,
            on_click: refresh_cb,
            accent: None,
            active: false,
        });
        items.push(ToolbarItem::IconButton {
            icon: if self.show_hidden {
                AppIcon::Eye
            } else {
                AppIcon::EyeOff
            },
            tooltip: if self.show_hidden {
                i18n!(cx, "sftp.toolbar.hide_dotfiles")
            } else {
                i18n!(cx, "sftp.toolbar.show_dotfiles")
            },
            enabled: true,
            on_click: hidden_cb,
            accent: None,
            active: false,
        });
        items.push(ToolbarItem::IconButton {
            icon: if self.auto_sync {
                AppIcon::Link
            } else {
                AppIcon::Unlink
            },
            tooltip: if self.auto_sync {
                i18n!(cx, "sftp.toolbar.auto_sync_disable")
            } else {
                i18n!(cx, "sftp.toolbar.auto_sync_enable")
            },
            enabled: true,
            on_click: auto_sync_cb,
            accent: None,
            active: self.auto_sync,
        });
        items
    }
}

impl Render for BottomPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_address_input(window, cx);
        if let Some(ref mut rename) = self.inline_rename {
            rename.ensure_input(window, cx);
        }
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        // Keep the single shared panel bound to the active terminal in
        // real time. Focus changes notify the `Workspace` entity, which
        // this panel observes, so this runs on every focus switch.
        self.bind_active_terminal(cx);

        let any_focused = self
            .address_input
            .as_ref()
            .map(|i| i.read(cx).focus_handle(cx).is_focused(window))
            .unwrap_or(false);

        if any_focused {
            let is_modal = self.focus_manager.read(cx).is_modal();
            if !is_modal {
                self.focus_manager.update(cx, |fm, cx| {
                    self.workspace.update(cx, |ws, cx| {
                        ws.clear_focused_terminal(fm, cx);
                    });
                });
            }
        }

        // Auto-focus the first input of a freshly opened dialog/modal so it is
        // immediately editable (new file/dir, rename, move, new link).
        if self.dialog_focus_pending {
            if let Some(target) = &self.dialog_focus_target {
                let fh = target.read(cx).focus_handle(cx);
                if !fh.is_focused(window) {
                    window.focus(&fh, cx);
                }
            }
            self.dialog_focus_pending = false;
        }

        // After an inline rename is committed/cancelled, refocus the panel itself
        // so keyboard navigation (arrow keys etc.) keeps working.
        if self.focus_self_pending {
            if !self.focus_handle.is_focused(window) {
                window.focus(&self.focus_handle, cx);
            }
            self.focus_self_pending = false;
        }

        let body = match &self.connection_state {
            SftpConnectionState::Disconnected => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(p.text_muted)
                        .child(i18n!(cx, "sftp.status.not_connected")),
                )
                .into_any_element(),
            SftpConnectionState::Connecting => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(p.text_muted)
                        .child(i18n!(cx, "status.connecting")),
                )
                .into_any_element(),
            SftpConnectionState::Failed(err) => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .flex_col()
                .gap(SPACE_MD)
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(p.surface_danger)
                        .child(format!("{}{}", i18n!(cx, "sftp.status.failed"), err)),
                )
                .child(
                    button_primary("sftp-retry-btn", i18n!(cx, "common.retry"), &t).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.bind_active_terminal(cx);
                        }),
                    ),
                )
                .into_any_element(),
            SftpConnectionState::Connected { .. } => {
                let header_row = self.sftp_header_row(cx);
                let font = ui_font_family(cx);

                div()
                        .id("sftp-list-wrapper")
                        .size_full()
                        .flex()
                        .flex_col()
                        .font_family(font)
                        .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                            this.handle_external_drop(paths, cx);
                        }))
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                            if let Some((col_idx, start_x, start_w)) = this.col_resize_dragging {
                                let delta = f32::from(event.position.x) - start_x;
                                match col_idx {
                                    0 => this.col_name_w = (start_w + delta).max(100.0),
                                    1 => this.col_perm_w = (start_w + delta).max(60.0),
                                    2 => this.col_owner_w = (start_w + delta).max(60.0),
                                    3 => this.col_size_w = (start_w + delta).max(60.0),
                                    _ => {}
                                }
                                cx.notify();
                            }
                        }))
                        .on_mouse_up(MouseButton::Left, cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                            if this.col_resize_dragging.is_some() {
                                this.col_resize_dragging = None;
                                cx.notify();
                            }
                        }))
                        .child(
                            h_flex()
                                .id("sftp-list-body")
                                .size_full()
                                // 右侧主内容区
                                .child(
                                    div()
                                        .flex_1()
                                        .h_full()
                                        .flex()
                                        .flex_col()
                                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _event, _window, cx| {
                                            this.selection.clear();
                                            cx.notify();
                                        }))
                                        .on_mouse_down(MouseButton::Right, cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                            this.selection.clear();
                                            this.context_menu = Some(ContextMenuState {
                                                position: event.position,
                                                target: MenuTarget::Blank,
                                            });
                                            cx.stop_propagation();
                                            cx.notify();
                                        }))
                                        // 1. 顶部表头
                                        .child(header_row)
                                        // 2. 顶栏刷新细条
                                        .when(self.refreshing, |this| this.child(self.loading_bar(cx)))
                                        // 3. 列表绝对铺满容器：用 flex_1 + size_full 锁死视口尺寸！
                                        .child(
                                            div()
                                                .flex_1()
                                                .size_full()
                                                .overflow_hidden()
                                                .child({
                                                    // 行高与会话树保持一致：统一走 tree_row_height，随「界面密度」
                                                    // 与全局 UI 缩放实时变化（不再跟随终端字号 line_height + 6px）。
                                                    // 预先算成 Pixels（Copy）传入，避免 window 引用逃逸进 virtual_list。
                                                    let row_h = velowork_ui::tree_row_height(cx);
                                                    virtual_list(
                                                        "sftp-files",
                                                        cx.entity(),
                                                        &self.scroll_handle,
                                                        self.filtered_files().len() + 1,
                                                        move |this, range, _window, cx| {
                                                            let filtered = this.filtered_files();
                                                            let cols = this.col_widths();
                                                            range
                                                                .map(|row_idx| {
                                                                    let is_selected = this.selection.is_selected(row_idx);
                                                                    let is_inline_rename_parent =
                                                                        row_idx == 0 && this.inline_rename.is_some();
                                                                    // 行内重命名进行中：不显示该行的选中高亮（背景 + 边框），
                                                                    // 仅保留输入框自身的边框高亮，与全局其它模块行为一致。
                                                                    let is_selected = is_selected && !is_inline_rename_parent;
                                                                    if row_idx == 0 {
                                                                        parent_row(cols, is_selected, row_h, cx)
                                                                            .on_mouse_down(
                                                                                MouseButton::Left,
                                                                                cx.listener(move |this, _event, window, cx| {
                                                                                    // 点击列表区域时把焦点移到根节点，使 F2/Delete/F5 等
                                                                                    // 快捷键的 on_key_down 能正常接收事件。行内重命名进行中
                                                                                    // 不抢焦点，避免打断输入。
                                                                                    if this.inline_rename.is_none() {
                                                                                        window.focus(&this.focus_handle, cx);
                                                                                    }
                                                                                    cx.stop_propagation();
                                                                                }),
                                                                            )
                                                                            .on_mouse_down(
                                                                                MouseButton::Right,
                                                                                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                                                                    this.context_menu = Some(ContextMenuState {
                                                                                        position: event.position,
                                                                                        target: MenuTarget::Item(0),
                                                                                    });
                                                                                    cx.stop_propagation();
                                                                                    cx.notify();
                                                                                }),
                                                                            )
                                                                            .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
                                                                                let ctrl = event.modifiers().control || event.modifiers().platform;
                                                                                this.selection.click(0, ctrl, false);
                                                                                cx.notify();
                                                                            }))
                                                                            .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
                                                                                if event.click_count() == 2 {
                                                                                    this.go_to_parent_directory(cx);
                                                                                }
                                                                            }))
                                                                            .into_any_element()
                                                                    } else {
                                                                        let file = filtered[row_idx - 1].clone();
                                                                        let display = DisplayFile::from_sftp_file(file);
                                                                        let is_inline_rename =
                                                                            this.inline_rename.as_ref().map_or(false, |r| r.idx == row_idx);
                                                                        let rename_state = if is_inline_rename {
                                                                            this.inline_rename.clone()
                                                                        } else {
                                                                            None
                                                                        };
                                                                        // 行内重命名进行中：不显示该行的选中高亮（背景 + 边框），
                                                                        // 仅保留输入框自身的边框高亮，与全局其它模块行为一致。
                                                                        let is_selected = is_selected && !is_inline_rename;
                                                                        file_row(row_idx, &display, cols, is_selected, rename_state.as_ref(), row_h, cx)
                                                                            .on_mouse_down(
                                                                                MouseButton::Left,
                                                                                cx.listener(move |this, _event, window, cx| {
                                                                                    // 点击列表区域时把焦点移到根节点，使 F2/Delete/F5 等
                                                                                    // 快捷键的 on_key_down 能正常接收事件。行内重命名进行中
                                                                                    // 不抢焦点，避免打断输入。
                                                                                    if this.inline_rename.is_none() {
                                                                                        window.focus(&this.focus_handle, cx);
                                                                                    }
                                                                                    cx.stop_propagation();
                                                                                }),
                                                                            )
                                                                            .on_mouse_down(
                                                                                MouseButton::Right,
                                                                                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                                                                    this.context_menu = Some(ContextMenuState {
                                                                                        position: event.position,
                                                                                        target: MenuTarget::Item(row_idx),
                                                                                    });
                                                                                    cx.stop_propagation();
                                                                                    cx.notify();
                                                                                }),
                                                                            )
                                                                            .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
                                                                                let ctrl = event.modifiers().control || event.modifiers().platform;
                                                                                let shift = event.modifiers().shift;
                                                                                this.selection.click(row_idx, ctrl, shift);
                                                                                cx.notify();
                                                                            }))
                                                                            .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
                                                                                if event.click_count() == 2 && !is_inline_rename {
                                                                                    this.open_item(row_idx, cx);
                                                                                }
                                                                            }))
                                                                            .into_any_element()
                                                                    }
                                                                })
                                                                .collect()
                                                        },
                                                    )
                                                    }
                                                )
                                                .when(self.refreshing && self.filtered_files().is_empty(), |this| {
                                                    this.child(self.loading_overlay(cx))
                                                })
                                        )
                                )
                        )
                        .into_any_element()
            }
        };

        div()
            .id("bottom-panel-root")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event, _, cx| {
                this.handle_key(event, cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .size_full()
            // Do NOT paint our own background: the enclosing `DockPanel` root
            // already paints a single translucent `surface_bg(t.bg_panel)`
            // layer. Painting `bg_panel` again here would stack a second
            // translucent layer, compounding the alpha and making the list area
            // look nearly opaque (transparency setting appears to have no
            // effect). Staying transparent lets the dock's single surface show
            // through — matching every other dock panel (see
            // `velowork-views-services::panel`).
            .flex()
            .flex_col()
            .relative()
            .child(div().flex_1().w_full().min_h(px(0.0)).child(body))
            .when_some(self.context_menu.clone(), |el, menu| {
                let on_close = Arc::new(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.context_menu = None;
                    cx.notify();
                }));
                let on_close_clone = on_close.clone();
                // Backdrop: window-level overlay so clicking anywhere outside the
                // menu (terminal, sidebar, status bar, etc.) closes it. The menu
                // itself is a *separate top-level* anchored popup (below), NOT
                // nested inside this overlay — nesting anchored layers triggers
                // GPUI's "node not part of reused subtree" panic.
                let backdrop = sftp_window_overlay(
                    context_menu_backdrop("sftp-menu-backdrop", move |ev, window, cx| {
                        on_close_clone(ev, window, cx)
                    }),
                    window,
                );
                // Menu: top-level `deferred(anchored().snap_to_window())`.
                // `snap_to_window()` makes GPUI detect the viewport bounds and
                // auto-shift the menu so it never overflows any edge (flipping
                // the offset near the bottom/right borders, re-clamping on
                // window resize / scroll).
                let menu_el =
                    deferred(anchored().position(menu.position).snap_to_window().child({
                        let is_item = matches!(menu.target, MenuTarget::Item(_));
                        let is_parent = matches!(menu.target, MenuTarget::Item(0));
                        let item_idx = match menu.target {
                            MenuTarget::Item(i) => i,
                            MenuTarget::Blank => 0,
                        };
                        let has_selection = is_item && !is_parent;
                        let props_idx = if is_item { item_idx } else { 0 };
                        // Whether the right-clicked entry is a directory or a
                        // text file — gates the "open" menu items below.
                        let menu_files = self.filtered_files();
                        let menu_selected = if is_item && !is_parent
                            && item_idx >= 1
                            && item_idx <= menu_files.len()
                        {
                            Some(menu_files[item_idx - 1].clone())
                        } else {
                            None
                        };
                        let sel_is_dir = menu_selected
                            .as_ref()
                            .map(|f| f.is_dir)
                            .unwrap_or(false);
                        let sel_is_text = menu_selected
                            .as_ref()
                            .map(|f| is_likely_text_file(&f.name))
                            .unwrap_or(false);
                        context_menu_panel("sftp-menu-panel", &t, cx)
                            // ─── Top: Refresh (always) ───
                            .child(
                                menu_item(
                                    "sftp-menu-refresh",
                                    AppIcon::Refresh,
                                    i18n!(cx, "common.refresh"),
                                    &t, cx,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.context_menu = None;
                                        this.refresh(cx);
                                    },
                                )),
                            )
                            .child(menu_separator(&t))
                            // ─── Open actions (file only, never a directory) ───
                            .when(is_item && !is_parent && !sel_is_dir, |p| {
                                p.child(
                                    menu_item_with_shortcut(
                                        "sftp-menu-open",
                                        AppIcon::ExternalLink,
                                        i18n!(cx, "common.open"),
                                        Some("Space".into()),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.context_menu = None;
                                        this.open_remote_file(false, cx);
                                    })),
                                )
                            })
                            .when(is_item && !is_parent && !sel_is_dir && sel_is_text, |p| {
                                p.child(
                                    menu_item(
                                        "sftp-menu-open-editor",
                                        AppIcon::Edit,
                                        i18n!(cx, "sftp.menu.open_with_editor"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.context_menu = None;
                                        this.open_remote_file(true, cx);
                                    })),
                                )
                            })
                            // ─── Category 1: on a real file/dir ───
                            .when(is_item && !is_parent, |p| {
                                p.child(
                                    menu_item_with_shortcut(
                                        "sftp-menu-delete",
                                        AppIcon::Trash,
                                        i18n!(cx, "common.delete"),
                                        Some("Delete".into()),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.request_delete_confirm(item_idx, cx);
                                        },
                                    )),
                                )
                                .child(
                                    menu_item_with_shortcut(
                                        "sftp-menu-rename",
                                        AppIcon::Edit,
                                        i18n!(cx, "common.rename"),
                                        Some("F2".into()),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.context_menu = None;
                                            this.open_rename_dialog(item_idx, cx);
                                        },
                                    )),
                                )
                                .child(
                                    menu_item(
                                        "sftp-menu-move",
                                        AppIcon::Folder,
                                        i18n!(cx, "sftp.menu.move"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.context_menu = None;
                                            this.open_move_dialog(item_idx, cx);
                                        },
                                    )),
                                )
                                .child(menu_separator(&t))
                                .child(
                                    menu_item(
                                        "sftp-menu-copy-path",
                                        AppIcon::Copy,
                                        i18n!(cx, "sftp.menu.copy_path"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.context_menu = None;
                                            this.copy_path_of(item_idx, cx);
                                        },
                                    )),
                                )
                                .child(
                                    menu_item(
                                        "sftp-menu-copy-name",
                                        AppIcon::Copy,
                                        i18n!(cx, "sftp.menu.copy_name"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.context_menu = None;
                                            this.copy_name_of(item_idx, cx);
                                        },
                                    )),
                                )
                                .child(
                                    menu_item(
                                        "sftp-menu-send-path",
                                        AppIcon::Send,
                                        i18n!(cx, "sftp.menu.send_path_to_terminal"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.send_path_of(item_idx, cx);
                                        },
                                    )),
                                )
                            })
                            // ─── Parent ".." entry ───
                            .when(is_parent, |p| {
                                p.child(menu_separator(&t))
                                .child(
                                    menu_item(
                                        "sftp-menu-copy-cur",
                                        AppIcon::Copy,
                                        i18n!(cx, "sftp.menu.copy_current_path"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.context_menu = None;
                                            this.copy_current_path(cx);
                                        },
                                    )),
                                )
                                .child(
                                    menu_item(
                                        "sftp-menu-send-cur",
                                        AppIcon::Send,
                                        i18n!(cx, "sftp.menu.send_current_path_to_terminal"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.send_current_path_to_terminal(cx);
                                        },
                                    )),
                                )
                            })
                            // ─── Category 2: blank area ───
                            .when(!is_item, |p| {
                                p.child(
                                    menu_item(
                                        "sftp-menu-new-file",
                                        AppIcon::Plus,
                                        i18n!(cx, "sftp.menu.new_file"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.context_menu = None;
                                            this.open_create_dialog(false, cx);
                                        },
                                    )),
                                )
                                .child(
                                    menu_item(
                                        "sftp-menu-new-dir",
                                        AppIcon::Folder,
                                        i18n!(cx, "common.new_folder"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.context_menu = None;
                                            this.open_create_dialog(true, cx);
                                        },
                                    )),
                                )
                                .child(
                                    menu_item(
                                        "sftp-menu-new-link",
                                        AppIcon::Link,
                                        i18n!(cx, "sftp.menu.new_link"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.context_menu = None;
                                            this.open_link_dialog(cx);
                                        },
                                    )),
                                )
                                .child(menu_separator(&t))
                                .child(
                                    menu_item(
                                        "sftp-menu-copy-cur2",
                                        AppIcon::Copy,
                                        i18n!(cx, "sftp.menu.copy_current_path"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.context_menu = None;
                                            this.copy_current_path(cx);
                                        },
                                    )),
                                )
                                .child(
                                    menu_item(
                                        "sftp-menu-send-cur2",
                                        AppIcon::Send,
                                        i18n!(cx, "sftp.menu.send_current_path_to_terminal"),
                                        &t, cx,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.send_current_path_to_terminal(cx);
                                        },
                                    )),
                                )
                            })
                            // ─── Shared: Upload / Download ───
                            .child(menu_separator(&t))
                            .child(
                                menu_item_conditional(
                                    "sftp-menu-upload",
                                    AppIcon::ClipboardPaste,
                                    i18n!(cx, "sftp.menu.upload"),
                                    true,
                                    &t, cx,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.context_menu = None;
                                        this.trigger_upload(cx);
                                    },
                                )),
                            )
                            .child(
                                menu_item_conditional_with_shortcut(
                                    "sftp-menu-download",
                                    AppIcon::Copy,
                                    i18n!(cx, "sftp.menu.download"),
                                    has_selection,
                                    Some("Enter".into()),
                                    &t, cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.context_menu = None;
                                        if has_selection {
                                            this.trigger_download(cx);
                                        }
                                    },
                                )),
                            )
                            .child(menu_separator(&t))
                            // ─── Category 3: Properties (all scenarios) ───
                            .child(
                                menu_item_with_shortcut(
                                    "sftp-menu-properties",
                                    AppIcon::Settings,
                                    i18n!(cx, "sftp.menu.properties"),
                                    Some("Alt+Enter".into()),
                                    &t, cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.context_menu = None;
                                        this.show_properties_of(props_idx, cx);
                                    },
                                )),
                            )
                    }))
                    .with_priority(2);
                el.child(backdrop).child(menu_el)
            })
            // Render dialogs as a window-level overlay so the backdrop covers the
            // ENTIRE window (not just the bottom panel) and the card is centered
            // over the whole window — matching the "Add Project" / shell picker
            // global full-screen backdrop.
            //
            // Mechanism: `deferred(anchored().snap_to_window())` mounts the
            // element in the window's top deferred layer (window-coordinates,
            // painted above all normal content). Inside it we add an *in-flow*
            // `div().w_full().h_full()` child: because the deferred+anchored
            // layer is window-scoped (available space = full window), that div
            // resolves to the full window size and — crucially — gives the
            // anchored layer itself a non-zero (window) size. The dialog's
            // `modal_backdrop` is `absolute().inset_0()`, which anchors to
            // this now-window-sized layer and therefore fills the whole window.
            // (Putting `modal_backdrop` directly under `anchored` without the
            // in-flow `w_full` wrapper collapses the layer to zero size, which
            // is what previously dropped the card into the top-left corner.)
            .when_some(self.active_dialog.clone(), |el, dialog| {
                el.child(sftp_window_overlay(
                    self.render_creation_dialog(&dialog, cx),
                    window,
                ))
            })
            .when_some(self.modal.clone(), |el, m| {
                el.child(sftp_window_overlay(self.render_modal(&m, cx), window))
            })
            .when_some(self.confirm_dialog.clone(), |el, dialog| {
                el.child(sftp_window_overlay(dialog, window))
            })
            .into_any_element()
    }
}

fn checkbox(
    id: impl Into<ElementId>,
    checked: bool,
    label: impl Into<SharedString>,
    t: &ThemeColors,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let id = id.into();
    let p = SemanticPalette::from_theme(t);
    h_flex()
        .id(id)
        .gap(SPACE_XS)
        .items_center()
        .cursor_pointer()
        .on_click(on_click)
        .child(
            div()
                .w(px(14.0))
                .h(px(14.0))
                .border_1()
                .border_color(p.border_subtle)
                .rounded(RADIUS_MD)
                .flex()
                .items_center()
                .justify_center()
                .bg(surface_bg_t(if checked {
                    t.bg_selection
                } else {
                    t.bg_primary
                }, &t))
                .when(checked, |el| {
                    el.child(
                        AppIcon::Check
                            .size(ICON_SM)
                            .text_color(p.text_primary),
                    )
                }),
        )
        .child(
            div()
                .text_size(ui_text_md(cx))
                .text_color(p.text_secondary)
                .child(label.into()),
        )
}

fn mode_from_permissions(
    is_dir: bool,
    user: (bool, bool, bool),
    group: (bool, bool, bool),
    other: (bool, bool, bool),
) -> u32 {
    let mut mode = if is_dir { 0o040000 } else { 0o100000 };

    if user.0 {
        mode |= 0o400;
    }
    if user.1 {
        mode |= 0o200;
    }
    if user.2 {
        mode |= 0o100;
    }

    if group.0 {
        mode |= 0o040;
    }
    if group.1 {
        mode |= 0o020;
    }
    if group.2 {
        mode |= 0o010;
    }

    if other.0 {
        mode |= 0o004;
    }
    if other.1 {
        mode |= 0o002;
    }
    if other.2 {
        mode |= 0o001;
    }

    mode
}

/// Join a directory and a file/dir name into an absolute-looking path.
fn join_path(dir: &str, name: &str) -> String {
    let mut p = dir.to_string();
    if !p.ends_with('/') {
        p.push('/');
    }
    p.push_str(name);
    p
}

/// Return the parent directory of a path (keeps the trailing separator semantics of
/// `join_path` — root stays `"/"`).
fn parent_dir_of(path: &str) -> String {
    let mut s = path.to_string();
    if let Some(pos) = s.rfind('/') {
        s.truncate(pos);
        if s.is_empty() {
            s = "/".to_string();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::BottomPanel;

    #[test]
    fn test_format_path_for_terminal() {
        assert_eq!(
            BottomPanel::format_path_for_terminal("/var/log/syslog"),
            "/var/log/syslog"
        );
        assert_eq!(
            BottomPanel::format_path_for_terminal("/var/log/my test.log"),
            "'/var/log/my test.log'"
        );
        assert_eq!(
            BottomPanel::format_path_for_terminal("/home/user/let's go.txt"),
            "'/home/user/let'\\''s go.txt'"
        );
        assert_eq!(
            BottomPanel::format_path_for_terminal("/home/user/$PATH.txt"),
            "'/home/user/$PATH.txt'"
        );
    }
}

