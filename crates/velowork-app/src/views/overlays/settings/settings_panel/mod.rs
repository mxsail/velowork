//! Settings panel for visual settings configuration
//!
//! Provides a Zed-style settings dialog with sidebar categories (global / user settings).

mod categories;
mod category_nav;
mod components;
mod controls;
mod render_ai;
mod render_appearance;
mod render_data_storage;
mod render_extensions;
mod render_file_manager;
mod render_font;
mod render_general;
mod render_search;
mod render_security;
pub mod render_sync;
mod render_terminal;

pub use categories::SettingsCategory;

use crate::keybindings::Cancel;
use crate::settings::settings_entity;
use crate::views::overlays::dialogs::terminal_color_scheme_dialog::{
    TerminalColorSchemeDialog, TerminalColorSchemeDialogEvent,
};
use crate::terminal::shell_config::{AvailableShell, available_shells};
use crate::theme::theme;
use crate::ui::tokens::{ui_font_family, use_custom_ui_font};
use crate::views::components::{PathAutoCompleteState, dropdown_anchored_below};
use crate::workspace::settings::SyncProvider;
use crate::workspace::settings::TextAntialiasingMode;
use crate::workspace::state::Workspace;
use gpui::prelude::*;
use gpui::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use velowork_extensions::ExtensionRegistry;
use velowork_i18n::i18n;
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::input::{InputEvent, InputState};
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::select::{
    SelectEvent, SelectOption, SelectPlacement, SelectState, SelectWidthMode,
};
use velowork_ui::slider::SliderState;
use velowork_ui::tokens::{
    RADIUS_CARD, SPACE_CARD_GAP, SPACE_SM, SPACE_MD, SPACE_LG, SPACE_XL, ui_text_lg,
};
use velowork_ui::{AnimatedModal, AnimatedModalEvent};

// ============================================================================
// Settings Panel
use crate::terminal::session_backend::SessionBackend;
use crate::terminal::shell_config::ShellType;
use velowork_workspace::settings::{ColorSchema, ColorTheme, CustomTitlebarPreset};

// ============================================================================

/// Settings panel overlay for configuring app settings
pub struct SettingsPanel {
    pub(super) _workspace: Entity<Workspace>,
    focus_handle: FocusHandle,
    pub(super) active_category: SettingsCategory,
    pub(super) language_select: Entity<SelectState<String>>,
    pub(super) theme_mode_select: Entity<SelectState<ColorSchema>>,
    pub(super) dark_palette_select: Entity<SelectState<ColorTheme>>,
    pub(super) light_palette_select: Entity<SelectState<ColorTheme>>,
    pub(super) titlebar_preset_select: Entity<SelectState<CustomTitlebarPreset>>,
    pub(super) ui_font_select: Entity<SelectState<String>>,
    pub(super) font_select: Entity<SelectState<String>>,
    pub(super) font_weight_select: Entity<SelectState<String>>,
    pub(super) text_antialiasing_select: Entity<SelectState<TextAntialiasingMode>>,
    pub(super) shell_select: Entity<SelectState<ShellType>>,
    pub(super) session_backend_select: Entity<SelectState<SessionBackend>>,
    pub(super) charset_select: Entity<SelectState<String>>,
    pub(super) color_scheme_select: Entity<SelectState<String>>,
    pub(super) term_type_select: Entity<SelectState<String>>,
    pub(super) ai_default_model_select: Entity<SelectState<String>>,
    pub(super) ai_compression_strategy_select:
        Entity<SelectState<velowork_workspace::settings::AiCompressionStrategy>>,
    pub(super) ai_max_context_tokens_input: Entity<InputState>,
    pub(super) ai_max_history_messages_input: Entity<InputState>,
    pub(super) overlay_registry: Option<Entity<OverlayRegistry>>,
    pub(super) _available_shells: Vec<AvailableShell>,
    // File opener input
    pub(super) file_opener_input: Entity<InputState>,
    // SFTP default permission inputs
    pub(super) sftp_file_mode_input: Entity<InputState>,
    pub(super) sftp_dir_mode_input: Entity<InputState>,
    // Proxy host input
    pub(super) proxy_host_input: Entity<InputState>,
    // Data & Storage inputs
    pub(super) data_root_mode_select: Entity<SelectState<velowork_core::data_root::DataRootMode>>,
    pub(super) data_root_custom_input: Entity<PathAutoCompleteState>,
    pub(super) data_root_input_bounds: Option<Bounds<Pixels>>,
    pub(super) data_root_error: Option<String>,
    pub(super) show_data_root_confirm_modal: bool,
    // Terminal background image input (path auto-complete + file picker)
    pub(super) terminal_bg_image_input: Entity<PathAutoCompleteState>,
    /// Word selection delimiters input
    pub(super) word_selection_delimiters_input: Entity<InputState>,
    /// Command history ignored bare commands input
    pub(super) command_history_ignored_commands_input: Entity<InputState>,
    /// Window-space bounds of the terminal background image input, used to
    /// anchor the path-completion suggestions overlay below it.
    pub(super) bg_image_input_bounds: Option<Bounds<Pixels>>,
    pub(super) bg_image_show_success: bool,
    pub(super) bg_image_last_loaded_key: Option<(String, bool)>,
    pub(super) bg_image_success_seq: usize,
    // Sync inputs
    pub(super) sync_server_url_input: Entity<InputState>,
    pub(super) sync_username_input: Entity<InputState>,
    pub(super) sync_password_input: Entity<InputState>,
    pub(super) sync_remote_path_input: Entity<InputState>,
    pub(super) sync_provider_select: Entity<SelectState<Option<SyncProvider>>>,
    /// WebDAV 测试连接结果：None = 空闲，Some(Ok(msg)) = 成功，Some(Err(msg)) = 失败
    pub(super) sync_test_result: Option<Result<String, String>>,
    pub(super) sync_test_in_progress: bool,
    /// 立即同步结果：None = 空闲，Some(Ok(msg)) = 成功，Some(Err(msg)) = 失败
    pub(super) sync_result: Option<Result<String, String>>,
    pub(super) sync_in_progress: bool,
    /// 从云端恢复结果：None = 空闲，Some(Ok(msg)) = 成功，Some(Err(msg)) = 失败
    pub(super) restore_result: Option<Result<String, String>>,
    pub(super) restore_in_progress: bool,
    /// Cached extension settings views (lazily created on first access).
    extension_views: HashMap<String, AnyView>,
    // AI model dialog state
    pub(super) ai_add_model_dialog_open: bool,
    pub(super) ai_edit_model_id: Option<String>,
    pub(super) ai_model_name_input: Entity<InputState>,
    pub(super) ai_model_base_url_input: Entity<InputState>,
    pub(super) ai_model_api_key_input: Entity<InputState>,
    pub(super) ai_model_id_input: Entity<InputState>,
    pub(super) ai_model_desc_input: Entity<InputState>,
    pub(super) ai_temperature_input: Entity<InputState>,
    pub(super) ai_max_tokens_input: Entity<InputState>,
    // Search engine dialog state
    pub(super) search_add_dialog_open: bool,
    pub(super) search_edit_id: Option<String>,
    pub(super) search_name_input: Entity<InputState>,
    pub(super) search_url_input: Entity<InputState>,
    pub(super) search_keyword_input: Entity<InputState>,
    // Terminal color scheme manager dialog
    pub(super) active_color_scheme_dialog: Option<Entity<AnimatedModal>>,
    // Dialog button focus handles (persistent so they stay Tab-reachable).
    pub(super) ai_dialog_test_focus: FocusHandle,
    pub(super) ai_dialog_cancel_focus: FocusHandle,
    pub(super) ai_dialog_save_focus: FocusHandle,
    pub(super) search_dialog_cancel_focus: FocusHandle,
    pub(super) search_dialog_save_focus: FocusHandle,

    /// None = idle, Some(Ok(msg)) = success, Some(Err(msg)) = failure
    pub(super) ai_test_result: Option<Result<String, String>>,
    pub(super) ai_test_in_progress: bool,
    // Security: master-password inputs
    pub(super) security_new_password_input: Entity<InputState>,
    pub(super) security_current_password_input: Entity<InputState>,
    /// 标准模式：确认密码输入框（启用增强模式时与密码框配对）
    pub(super) security_confirm_password_input: Entity<InputState>,
    /// Security operation result: None = idle, Some(Ok) = success, Some(Err) = failure
    pub(super) security_result: Option<Result<String, String>>,
    pub(super) security_busy: bool,
    // Security: master-password UI 交互状态
    /// 增强模式下是否展开「修改主密码」输入框
    pub(super) security_change_mode: bool,
    /// 增强模式下是否展开「移除主密码」确认输入框
    pub(super) security_remove_mode: bool,
    /// 标准模式：是否展开「启用增强模式」设置区（密码 + 确认密码输入）
    pub(super) security_setup_mode: bool,
    // ---- 折叠卡片 / 滚动定位（对齐「新建会话」弹窗）----
    /// 已展开的分类集合（折叠卡片用）。
    pub(super) expanded_categories: HashSet<SettingsCategory>,
    /// 右侧卡片区滚动句柄，用于导航点击后滚动到目标卡片。
    pub(super) scroll_handle: ScrollHandle,
    /// 导航点击/搜索触发后待滚动到的目标分类（下帧 prepaint 测量后定位并清除）。
    pub(super) pending_scroll: Rc<RefCell<Option<SettingsCategory>>>,
    /// 各分类卡片动态布局高度缓存（基于 canvas 测量，用于精准算距贴顶）
    pub(super) card_heights: Rc<RefCell<HashMap<SettingsCategory, f32>>>,
    /// 左侧导航搜索框输入实体（定位器）。
    pub(super) nav_search_input: Entity<InputState>,
    /// 导航搜索当前文本（冗余缓存，便于无窗口上下文读取）。
    pub(super) nav_search: String,
    /// 动态按 ID 缓存数字步进器输入框实体
    pub(super) stepper_inputs: HashMap<String, Entity<InputState>>,
    /// 背景透明度滑块实体（懒初始化，避免每次渲染重建导致拖动状态丢失）。
    pub(super) bg_opacity_slider: Option<Entity<SliderState>>,
    /// 左侧导航列表整体聚焦句柄（单一 Tab stop）。
    pub(super) nav_focus_handle: FocusHandle,
    /// 开关 (Switch) 持久化聚焦句柄缓存池，保证重绘时不失焦。
    pub(super) toggle_focus_handles: RefCell<HashMap<String, FocusHandle>>,
    /// 单选组 (RadioGroup) 持久化聚焦句柄缓存池，保证重绘时不失焦。
    pub(super) radio_focus_handles: RefCell<HashMap<String, FocusHandle>>,
    /// 按钮 (Button) 持久化聚焦句柄缓存池，保证重绘时不失焦。
    pub(super) button_focus_handles: RefCell<HashMap<String, FocusHandle>>,
    /// 弹窗/展开前触发源焦点暂存（用于关闭/取消时精准平滑回退焦点）
    pub(super) dialog_trigger_focus_handle: Option<FocusHandle>,
    /// Tab 导航切焦待滚入视口的句柄暂存（首次布局尚未产生视口高度时使用）
    pub(super) pending_scroll_focus_handle: Option<FocusHandle>,
}

impl SettingsPanel {
    pub fn new(workspace: Entity<Workspace>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = settings_entity(cx).read(cx).settings.clone();

        // File opener input
        let file_opener_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("e.g. code, cursor, zed, vim");
            if !s.file_opener.is_empty() {
                state.set_value(s.file_opener.clone(), cx);
            }
            state
        });
        cx.subscribe(
            &file_opener_input,
            |_this, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).text().to_string();
                settings_entity(cx).update(cx, |state, cx| state.set_file_opener(val, cx));
            },
        )
        .detach();

        // SFTP default file permission input
        let sftp_file_mode_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("0644");
            if !s.sftp_default_file_mode.is_empty() {
                state.set_value(s.sftp_default_file_mode.clone(), cx);
            }
            state
        });
        cx.subscribe(
            &sftp_file_mode_input,
            |_this, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).text().to_string();
                settings_entity(cx)
                    .update(cx, |state, cx| state.set_sftp_default_file_mode(val, cx));
            },
        )
        .detach();

        // SFTP default directory permission input
        let sftp_dir_mode_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("0755");
            if !s.sftp_default_dir_mode.is_empty() {
                state.set_value(s.sftp_default_dir_mode.clone(), cx);
            }
            state
        });
        cx.subscribe(
            &sftp_dir_mode_input,
            |_this, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).text().to_string();
                settings_entity(cx)
                    .update(cx, |state, cx| state.set_sftp_default_dir_mode(val, cx));
            },
        )
        .detach();

        // Proxy host input
        let proxy_host_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("e.g. 127.0.0.1");
            if !s.proxy_host.is_empty() {
                state.set_value(s.proxy_host.clone(), cx);
            }
            state
        });
        cx.subscribe(
            &proxy_host_input,
            |_this, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).text().to_string();
                settings_entity(cx).update(cx, |state, cx| state.set_proxy_host(val, cx));
            },
        )
        .detach();

        // Data Root / Storage configuration inputs
        let current_dr = velowork_core::data_root::current();
        let cur_mode = current_dr.mode();
        let data_root_mode_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new(
                        velowork_core::data_root::DataRootMode::Default,
                        i18n!(cx, "settings.data_storage.mode_default"),
                    ),
                    SelectOption::new(
                        velowork_core::data_root::DataRootMode::AppDir,
                        i18n!(cx, "settings.data_storage.mode_app_dir"),
                    ),
                    SelectOption::new(
                        velowork_core::data_root::DataRootMode::Custom,
                        i18n!(cx, "settings.data_storage.mode_custom"),
                    ),
                ])
                .selected(Some(cur_mode))
                .placement(SelectPlacement::Below)
        });

        let data_root_custom_input = cx.new(PathAutoCompleteState::new);
        let custom_placeholder = i18n!(cx, "settings.data_storage.custom_path_placeholder");
        data_root_custom_input.update(cx, |p, _cx| {
            p.set_placeholder(custom_placeholder);
            if current_dr.mode() == velowork_core::data_root::DataRootMode::Custom {
                p.set_value_quiet(current_dr.root().to_string_lossy().to_string(), _cx);
            }
        });
        cx.subscribe(
            &data_root_custom_input,
            |this, _, event: &crate::views::components::PathAutoCompleteEvent, cx| {
                let crate::views::components::PathAutoCompleteEvent::Change(_) = event;
                this.recompute_data_root_error(cx);
                cx.notify();
            },
        )
        .detach();
        cx.subscribe(
            &data_root_mode_select,
            |this, _, event: &SelectEvent<velowork_core::data_root::DataRootMode>, cx| {
                let SelectEvent::Change(mode_opt) = event;
                if *mode_opt == Some(velowork_core::data_root::DataRootMode::Custom) {
                    this.recompute_data_root_error(cx);
                } else {
                    this.data_root_error = None;
                }
                cx.notify();
            },
        )
        .detach();

        // Terminal background image input (path auto-complete + file picker).
        // Uses `PathAutoCompleteState` so the user gets filesystem path
        // completion while typing, and a "browse" button to pick an image.
        let terminal_bg_image_input = cx.new(PathAutoCompleteState::new);
        let bg_placeholder = i18n!(cx, "settings.terminal_background_image_placeholder");
        terminal_bg_image_input.update(cx, |p, _cx| {
            p.set_placeholder(bg_placeholder);
        });
        if let Some(ref img) = s.terminal_background_image
            && !img.is_empty()
        {
            terminal_bg_image_input.update(cx, |p, cx| p.set_value_quiet(img.clone(), cx));
        }
        cx.subscribe(
            &terminal_bg_image_input,
            |this, _, event: &crate::views::components::PathAutoCompleteEvent, cx| {
                let crate::views::components::PathAutoCompleteEvent::Change(val) = event;
                this.bg_image_show_success = false;
                let opt = if val.trim().is_empty() {
                    None
                } else {
                    Some(val.clone())
                };
                settings_entity(cx)
                    .update(cx, |state, cx| state.set_terminal_background_image(opt, cx));
            },
        )
        .detach();

        // Initial background cache key (avoid showing "ready" on initial panel open)
        let initial_bg_key = velowork_views_terminal::terminal_background_cache(cx).and_then(|c| {
            let cache = c.read(cx);
            cache.path().map(|p| (p.to_string(), cache.blur()))
        });

        // Observe TerminalBackgroundCache so settings panel re-renders when
        // background processing begins, updates, succeeds, or encounters error.
        if let Some(cache_entity) = velowork_views_terminal::terminal_background_cache(cx) {
            cx.observe(&cache_entity, |this, cache, cx| {
                let c = cache.read(cx);
                let is_loading = c.loading();
                let has_image = c.image().is_some();
                let has_error = c.error().is_some();
                let current_key = c.path().map(|p| (p.to_string(), c.blur()));

                if !is_loading && !has_error && has_image {
                    if this.bg_image_last_loaded_key != current_key {
                        this.bg_image_last_loaded_key = current_key;
                        this.bg_image_show_success = true;
                        this.bg_image_success_seq += 1;
                        let seq = this.bg_image_success_seq;

                        // Automatically hide the success status after 2.5 seconds
                        cx.spawn(async move |this, cx| {
                            smol::Timer::after(std::time::Duration::from_millis(2500)).await;
                            let _ = this.update(cx, |this, cx| {
                                if this.bg_image_success_seq == seq {
                                    this.bg_image_show_success = false;
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
                    }
                } else if is_loading {
                    this.bg_image_show_success = false;
                } else if has_error {
                    this.bg_image_show_success = false;
                    this.bg_image_last_loaded_key = None;
                }

                cx.notify();
            })
            .detach();
        }

        // Word selection delimiters input
        let word_selection_delimiters_input = cx.new(|cx| {
            let mut state = InputState::new(cx)
                .placeholder(r#"/ \ ( ) " ' - : . , ; < > ~ ! @ # $ % ^ & * | + = [ ] { } ~ ?"#);
            if !s.word_selection_delimiters.is_empty() {
                state.set_value(s.word_selection_delimiters.clone(), cx);
            }
            state
        });
        cx.subscribe(
            &word_selection_delimiters_input,
            |_this, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).text().to_string();
                settings_entity(cx)
                    .update(cx, |state, cx| state.set_word_selection_delimiters(val, cx));
            },
        )
        .detach();

        // Command history ignored bare commands input
        let command_history_ignored_commands_input = cx.new(|cx| {
            let mut state = InputState::new(cx)
                .placeholder(i18n!(cx, "command_history.setting_ignored_commands_placeholder"));
            if !s.command_history_ignored_commands.is_empty() {
                state.set_value(s.command_history_ignored_commands.join(", "), cx);
            }
            state
        });
        cx.subscribe(
            &command_history_ignored_commands_input,
            |_this, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let raw = entity.read(cx).text().to_string();
                let list: Vec<String> = raw
                    .split(|c| c == ',' || c == '，')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                settings_entity(cx)
                    .update(cx, |state, cx| state.set_command_history_ignored_commands(list, cx));
            },
        )
        .detach();

        // Sync inputs
        let sync_server_url_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("https://dav.example.com/dav/");
            if !s.sync.webdav.server_url.is_empty() {
                state.set_value(s.sync.webdav.server_url.clone(), cx);
            }
            state
        });
        cx.subscribe(
            &sync_server_url_input,
            |this, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).text().to_string();
                settings_entity(cx).update(cx, |state, cx| state.set_webdav_server_url(val, cx));
                this.sync_test_result = None;
                cx.notify();
            },
        )
        .detach();

        let sync_username_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("username");
            if !s.sync.webdav.username.is_empty() {
                state.set_value(s.sync.webdav.username.clone(), cx);
            }
            state
        });
        cx.subscribe(
            &sync_username_input,
            |this, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).text().to_string();
                settings_entity(cx).update(cx, |state, cx| state.set_webdav_username(val, cx));
                this.sync_test_result = None;
                cx.notify();
            },
        )
        .detach();

        let initial_sync_password =
            velowork_workspace::secure_storage::load_webdav_password().unwrap_or_default();
        let sync_password_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("password")
                .masked(true)
                .default_value(initial_sync_password)
        });
        cx.subscribe(
            &sync_password_input,
            |this, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).text().to_string();
                if val.is_empty() {
                    // 用户清空密码：从密钥库删除并清除“已保存”标志
                    if let Err(e) = velowork_workspace::secure_storage::delete_webdav_password() {
                        log::warn!("[webdav] 清除已保存的 WebDAV 密码失败: {}", e);
                    }
                    settings_entity(cx)
                        .update(cx, |state, cx| state.set_webdav_password_stored(false, cx));
                } else {
                    // 用户正在输入密码：实时持久化到密钥库，供后续自动同步免交互使用
                    match velowork_workspace::secure_storage::store_webdav_password(&val) {
                        Ok(_) => {
                            settings_entity(cx)
                                .update(cx, |state, cx| state.set_webdav_password_stored(true, cx));
                        }
                        Err(e) => log::warn!("[webdav] 保存 WebDAV 密码失败: {}", e),
                    }
                }
                this.sync_test_result = None;
                cx.notify();
            },
        )
        .detach();

        let sync_remote_path_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("/velowork/");
            if !s.sync.webdav.remote_path.is_empty() {
                state.set_value(s.sync.webdav.remote_path.clone(), cx);
            }
            state
        });
        cx.subscribe(
            &sync_remote_path_input,
            |this, entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let val = entity.read(cx).text().to_string();
                settings_entity(cx).update(cx, |state, cx| state.set_webdav_remote_path(val, cx));
                this.sync_test_result = None;
                cx.notify();
            },
        )
        .detach();

        let cur_sync_provider = if s.sync.enabled {
            Some(s.sync.provider)
        } else {
            None
        };
        let sync_provider_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new(None, i18n!(cx, "common.none")),
                    SelectOption::new(Some(SyncProvider::WebDav), "WebDAV"),
                ])
                .selected(Some(cur_sync_provider))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &sync_provider_select,
            |_, _, event: &SelectEvent<Option<SyncProvider>>, cx| {
                let SelectEvent::Change(opt) = event;
                match opt {
                    Some(Some(provider)) => {
                        let provider = *provider;
                        settings_entity(cx).update(cx, |state, cx| {
                            state.set_sync_enabled(true, cx);
                            state.set_sync_provider(provider, cx);
                        });
                    }
                    _ => {
                        settings_entity(cx).update(cx, |state, cx| {
                            state.set_sync_enabled(false, cx);
                        });
                    }
                }
            },
        )
        .detach();

        // 左侧导航搜索框（定位器：输入时展开并滚动到首个匹配分类）
        let nav_search_input =
            cx.new(|cx| InputState::new(cx).placeholder(i18n!(cx, "settings.search_placeholder")));
        let ns_entity = nav_search_input.clone();
        cx.subscribe(
            &nav_search_input,
            move |this, _entity, _: &InputEvent, cx| {
                let q = ns_entity.read(cx).text().to_string();
                this.nav_search = q.clone();
                let ql = q.to_lowercase();
                if !ql.is_empty() {
                    let cats = this.ordered_categories(cx);
                    let mut first_matched = None;
                    for cat in cats.iter() {
                        if cat.matches_search(&ql, cx) {
                            this.expanded_categories.insert(cat.clone());
                            if first_matched.is_none() {
                                first_matched = Some(cat.clone());
                            }
                        }
                    }
                    if let Some(target) = first_matched {
                        this.active_category = target.clone();
                        *this.pending_scroll.borrow_mut() = Some(target);
                    }
                }
                cx.notify();
            },
        )
        .detach();

        let cur_lang = s.locale;
        let language_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new("en".to_string(), "English"),
                    SelectOption::new("zh".to_string(), "简体中文"),
                ])
                .selected(Some(cur_lang.code().to_string()))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(&language_select, |_, _, event: &SelectEvent<String>, cx| {
            if let SelectEvent::Change(Some(lang)) = event {
                let locale = if lang == "en" {
                    velowork_i18n::Locale::En
                } else {
                    velowork_i18n::Locale::Zh
                };
                settings_entity(cx).update(cx, |state, cx| {
                    state.set_locale(locale, cx);
                });
            }
        })
        .detach();

        let cur_scheme = s.color_schema;
        let theme_mode_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new(ColorSchema::Dark, i18n!(cx, "settings.color_schema.dark")),
                    SelectOption::new(ColorSchema::Light, i18n!(cx, "settings.color_schema.light")),
                    SelectOption::new(
                        ColorSchema::System,
                        i18n!(cx, "settings.color_schema.system"),
                    ),
                ])
                .selected(Some(cur_scheme))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &theme_mode_select,
            |_, _, event: &SelectEvent<ColorSchema>, cx| {
                if let SelectEvent::Change(Some(scheme)) = event {
                    let scheme = *scheme;
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_color_theme(scheme, cx);
                    });
                }
            },
        )
        .detach();

        let cur_dark = s.dark_color_theme;
        let dark_palette_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(
                    ColorTheme::all_variants()
                        .iter()
                        .map(|&ct| SelectOption::new(ct, i18n!(cx, ct.translation_key())))
                        .collect(),
                )
                .selected(Some(cur_dark))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &dark_palette_select,
            |_, _, event: &SelectEvent<ColorTheme>, cx| {
                if let SelectEvent::Change(Some(ct)) = event {
                    let ct = *ct;
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_dark_color_theme(ct, cx);
                    });
                }
            },
        )
        .detach();

        let cur_light = s.light_color_theme;
        let light_palette_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(
                    ColorTheme::all_variants()
                        .iter()
                        .map(|&ct| SelectOption::new(ct, i18n!(cx, ct.translation_key())))
                        .collect(),
                )
                .selected(Some(cur_light))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &light_palette_select,
            |_, _, event: &SelectEvent<ColorTheme>, cx| {
                if let SelectEvent::Change(Some(ct)) = event {
                    let ct = *ct;
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_light_color_theme(ct, cx);
                    });
                }
            },
        )
        .detach();

        let cur_preset = s.titlebar_preset;
        let titlebar_preset_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(
                    CustomTitlebarPreset::all_variants()
                        .iter()
                        .map(|&p| SelectOption::new(p, i18n!(cx, p.translation_key())))
                        .collect(),
                )
                .selected(Some(cur_preset))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &titlebar_preset_select,
            |_, _, event: &SelectEvent<CustomTitlebarPreset>, cx| {
                if let SelectEvent::Change(Some(preset)) = event {
                    let preset = *preset;
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_titlebar_preset(preset, cx);
                    });
                }
            },
        )
        .detach();

        let mut system_fonts = crate::font_cache::get_system_font_names();
        if system_fonts.is_empty() {
            system_fonts = components::FONT_FAMILIES
                .iter()
                .map(|s| s.to_string())
                .collect();
        }
        // Filter out empty names and hidden/internal system fonts starting with '.'
        system_fonts.retain(|f| {
            let trimmed = f.trim();
            !trimmed.is_empty() && !trimmed.starts_with('.')
        });
        system_fonts.sort_by_key(|a| a.to_lowercase());
        system_fonts.dedup();

        if !s.font_family.is_empty() && !system_fonts.contains(&s.font_family) {
            system_fonts.push(s.font_family.clone());
        }
        if !s.ui_font_family.is_empty() && !system_fonts.contains(&s.ui_font_family) {
            system_fonts.push(s.ui_font_family.clone());
        }
        if !s.mono_font_family.is_empty() && !system_fonts.contains(&s.mono_font_family) {
            system_fonts.push(s.mono_font_family.clone());
        }
        if !s.markdown_font_family.is_empty() && !system_fonts.contains(&s.markdown_font_family) {
            system_fonts.push(s.markdown_font_family.clone());
        }

        let font_options: Vec<SelectOption<String>> = std::iter::once("System Default".to_string())
            .chain(system_fonts.into_iter())
            .map(|f| SelectOption::new(f.clone(), f))
            .collect();

        let cur_ui_font = if s.ui_font_family.is_empty() {
            "System Default".to_string()
        } else {
            s.ui_font_family.clone()
        };
        let ui_font_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(font_options.clone())
                .selected(Some(cur_ui_font))
                .searchable(true)
                .placement(SelectPlacement::Below)
                .width_mode(SelectWidthMode::ContentAdaptive)
                .virtual_scroll(true)
        });
        cx.subscribe(&ui_font_select, |_, _, event: &SelectEvent<String>, cx| {
            if let SelectEvent::Change(Some(f)) = event {
                let f_str = f.clone();
                settings_entity(cx).update(cx, |state, cx| {
                    state.set_ui_font_family(f_str, cx);
                });
            }
        })
        .detach();

        let cur_font = s.font_family.clone();
        let font_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(font_options.clone())
                .selected(Some(cur_font))
                .searchable(true)
                .placement(SelectPlacement::Below)
                .width_mode(SelectWidthMode::ContentAdaptive)
                .virtual_scroll(true)
        });
        cx.subscribe(&font_select, |_, _, event: &SelectEvent<String>, cx| {
            if let SelectEvent::Change(Some(f)) = event {
                let f_str = f.clone();
                settings_entity(cx).update(cx, |state, cx| {
                    state.set_font_family(f_str, cx);
                });
            }
        })
        .detach();

        let cur_weight = s.font_weight.clone();
        let font_weight_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new("normal".to_string(), "Normal"),
                    SelectOption::new("medium".to_string(), "Medium"),
                    SelectOption::new("bold".to_string(), "Bold"),
                ])
                .selected(Some(cur_weight))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &font_weight_select,
            |_, _, event: &SelectEvent<String>, cx| {
                if let SelectEvent::Change(Some(w)) = event {
                    let w = w.clone();
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_font_weight(w, cx);
                    });
                }
            },
        )
        .detach();

        let cur_text_aa = s.text_antialiasing;
        let text_antialiasing_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(
                    TextAntialiasingMode::all_variants()
                        .iter()
                        .map(|&mode| {
                            SelectOption::new(
                                mode,
                                i18n!(cx, mode.translation_key()).to_string(),
                            )
                        })
                        .collect(),
                )
                .selected(Some(cur_text_aa))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &text_antialiasing_select,
            |_, _, event: &SelectEvent<TextAntialiasingMode>, cx| {
                if let SelectEvent::Change(Some(mode)) = event {
                    let mode = *mode;
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_text_antialiasing(mode, cx);
                    });
                }
            },
        )
        .detach();

        let cur_shell = s.default_shell.clone();
        let shell_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(
                    available_shells()
                        .into_iter()
                        .filter(|sh| sh.available)
                        .map(|sh| SelectOption::new(sh.shell_type.clone(), sh.name.clone()))
                        .collect(),
                )
                .selected(Some(cur_shell))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(&shell_select, |_, _, event: &SelectEvent<ShellType>, cx| {
            if let SelectEvent::Change(Some(sh)) = event {
                let sh = sh.clone();
                settings_entity(cx).update(cx, |state, cx| {
                    state.set_default_shell(sh, cx);
                });
            }
        })
        .detach();

        let cur_backend = s.session_backend;
        let session_backend_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new(SessionBackend::Auto, "Auto (Default)"),
                    SelectOption::new(SessionBackend::Tmux, "tmux"),
                    SelectOption::new(SessionBackend::Screen, "screen"),
                    SelectOption::new(SessionBackend::Dtach, "dtach"),
                    SelectOption::new(SessionBackend::Psmux, "psmux"),
                    SelectOption::new(SessionBackend::None, "None"),
                ])
                .selected(Some(cur_backend))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &session_backend_select,
            |_, _, event: &SelectEvent<SessionBackend>, cx| {
                if let SelectEvent::Change(Some(bk)) = event {
                    let bk = *bk;
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_session_backend(bk, cx);
                    });
                }
            },
        )
        .detach();

        let cur_charset = s.charset.clone();
        let charset_opts = velowork_core::charset::SUPPORTED_CHARSETS
            .iter()
            .map(|&c| SelectOption::new(c.to_string(), c))
            .collect();
        let charset_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(charset_opts)
                .selected(Some(cur_charset))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(&charset_select, |_, _, event: &SelectEvent<String>, cx| {
            if let SelectEvent::Change(Some(cs)) = event {
                let cs = cs.clone();
                settings_entity(cx).update(cx, |state, cx| {
                    state.set_charset(cs, cx);
                });
            }
        })
        .detach();

        let cur_color_scheme = if s.color_scheme.is_empty() {
            "Dark".to_string()
        } else {
            s.color_scheme.clone()
        };
        let mut color_scheme_options = vec![
            SelectOption::new("Dark".to_string(), "Dark"),
            SelectOption::new("Light".to_string(), "Light"),
            SelectOption::new("Solarized Dark".to_string(), "Solarized Dark"),
            SelectOption::new("Solarized Light".to_string(), "Solarized Light"),
            SelectOption::new("Monokai".to_string(), "Monokai"),
            SelectOption::new("Dracula".to_string(), "Dracula"),
            SelectOption::new("Nord".to_string(), "Nord"),
            SelectOption::new("One Dark".to_string(), "One Dark"),
            SelectOption::new("Gruvbox Dark".to_string(), "Gruvbox Dark"),
        ];
        for custom in &s.custom_terminal_color_schemes {
            color_scheme_options.push(SelectOption::new(custom.name.clone(), custom.name.clone()));
        }
        let color_scheme_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(color_scheme_options)
                .selected(Some(cur_color_scheme))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &color_scheme_select,
            |_, _, event: &SelectEvent<String>, cx| {
                if let SelectEvent::Change(Some(cs)) = event {
                    let cs = cs.clone();
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_color_scheme(cs, cx);
                    });
                }
            },
        )
        .detach();

        let cur_ttype = s.term_type.clone();
        let mut term_type_options: Vec<SelectOption<String>> = velowork_core::SUPPORTED_TERM_TYPES
            .iter()
            .map(|&t| SelectOption::new(t.to_string(), t))
            .collect();
        if !cur_ttype.is_empty() && !velowork_core::SUPPORTED_TERM_TYPES.contains(&cur_ttype.as_str()) {
            term_type_options.push(SelectOption::new(cur_ttype.clone(), cur_ttype.clone()));
        }
        let term_type_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(term_type_options)
                .selected(Some(cur_ttype))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &term_type_select,
            |_, _, event: &SelectEvent<String>, cx| {
                if let SelectEvent::Change(Some(tt)) = event {
                    let tt = tt.clone();
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_term_type(tt.clone(), cx);
                    });
                    if cx.has_global::<crate::GlobalPtyManager>() {
                        cx.global::<crate::GlobalPtyManager>().0.set_default_term_type(tt);
                    }
                }
            },
        )
        .detach();

        let cur_ai_model = s.ai_default_model_id.clone().unwrap_or_default();
        let ai_default_model_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(
                    s.ai_models
                        .iter()
                        .map(|m| {
                            SelectOption::new(m.id.clone(), format!("{} ({})", m.name, m.model_id))
                        })
                        .collect(),
                )
                .selected(Some(cur_ai_model))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &ai_default_model_select,
            |_, _, event: &SelectEvent<String>, cx| {
                if let SelectEvent::Change(Some(m)) = event {
                    let m = if m.is_empty() { None } else { Some(m.clone()) };
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_ai_default_model(m, cx);
                    });
                }
            },
        )
        .detach();

        let ai_compression_strategy_select = cx.new(|cx| {
            let options = vec![
                SelectOption::new(
                    velowork_workspace::settings::AiCompressionStrategy::Summarize,
                    i18n!(cx, "settings.ai_assistant.strategy_summarize"),
                ),
                SelectOption::new(
                    velowork_workspace::settings::AiCompressionStrategy::SlidingWindow,
                    i18n!(cx, "settings.ai_assistant.strategy_sliding_window"),
                ),
                SelectOption::new(
                    velowork_workspace::settings::AiCompressionStrategy::TruncateOldest,
                    i18n!(cx, "settings.ai_assistant.strategy_truncate_oldest"),
                ),
            ];
            SelectState::new(cx)
                .options(options)
                .selected(Some(s.ai_compression_strategy))
                .placement(SelectPlacement::Below)
        });
        cx.subscribe(
            &ai_compression_strategy_select,
            |_, _, event: &SelectEvent<velowork_workspace::settings::AiCompressionStrategy>, cx| {
                if let SelectEvent::Change(Some(strat)) = event {
                    let strat = *strat;
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_ai_compression_strategy(strat, cx);
                    });
                }
            },
        )
        .detach();

        let ai_max_context_tokens_input =
            cx.new(|cx| InputState::new(cx).default_value(s.ai_max_context_tokens.to_string()));
        cx.subscribe(
            &ai_max_context_tokens_input,
            |this, _, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = this.ai_max_context_tokens_input.read(cx).text().to_string();
                if let Ok(val) = text.trim().parse::<usize>() {
                    if val >= 1024 {
                        settings_entity(cx).update(cx, |state, cx| {
                            state.set_ai_max_context_tokens(val, cx);
                        });
                    }
                }
            },
        )
        .detach();

        let ai_max_history_messages_input =
            cx.new(|cx| InputState::new(cx).default_value(s.ai_max_history_messages.to_string()));
        cx.subscribe(
            &ai_max_history_messages_input,
            |this, _, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = this
                    .ai_max_history_messages_input
                    .read(cx)
                    .text()
                    .to_string();
                if let Ok(val) = text.trim().parse::<usize>() {
                    if val >= 2 {
                        settings_entity(cx).update(cx, |state, cx| {
                            state.set_ai_max_history_messages(val, cx);
                        });
                    }
                }
            },
        )
        .detach();

        let panel = Self {
            _workspace: workspace,
            focus_handle: cx.focus_handle(),
            active_category: SettingsCategory::General,
            expanded_categories: {
                let mut s = HashSet::new();
                for cat in SettingsCategory::all() {
                    if cat.default_expand() {
                        s.insert(cat.clone());
                    }
                }
                s
            },
            scroll_handle: ScrollHandle::new(),
            pending_scroll: Rc::new(RefCell::new(None)),
            card_heights: Rc::new(RefCell::new(HashMap::new())),
            nav_search: String::new(),
            nav_search_input,
            language_select,
            theme_mode_select,
            dark_palette_select,
            light_palette_select,
            titlebar_preset_select,
            ui_font_select,
            font_select,
            font_weight_select,
            text_antialiasing_select,
            shell_select,
            session_backend_select,
            charset_select,
            color_scheme_select,
            term_type_select,
            ai_default_model_select,
            ai_compression_strategy_select,
            ai_max_context_tokens_input,
            ai_max_history_messages_input,
            overlay_registry: None,
            _available_shells: available_shells(),
            file_opener_input,
            sftp_file_mode_input,
            sftp_dir_mode_input,
            proxy_host_input,
            data_root_mode_select,
            data_root_custom_input,
            data_root_input_bounds: None,
            data_root_error: None,
            show_data_root_confirm_modal: false,
            terminal_bg_image_input,
            word_selection_delimiters_input,
            command_history_ignored_commands_input,
            bg_image_input_bounds: None,
            bg_image_show_success: false,
            bg_image_last_loaded_key: initial_bg_key,
            bg_image_success_seq: 0,
            sync_server_url_input,
            sync_username_input,
            sync_password_input,
            sync_remote_path_input,
            sync_provider_select,
            sync_test_result: None,
            sync_test_in_progress: false,
            sync_result: None,
            sync_in_progress: false,
            restore_result: None,
            restore_in_progress: false,
            extension_views: HashMap::new(),
            ai_add_model_dialog_open: false,
            ai_edit_model_id: None,
            ai_model_name_input: cx.new(|cx| InputState::new(cx).placeholder("OpenAI")),
            ai_model_base_url_input: cx
                .new(|cx| InputState::new(cx).placeholder("https://api.openai.com/v1")),
            ai_model_api_key_input: cx
                .new(|cx| InputState::new(cx).placeholder("sk-...").masked(true)),
            ai_model_id_input: cx.new(|cx| InputState::new(cx).placeholder("gpt-4o")),
            ai_model_desc_input: cx
                .new(|cx| InputState::new(cx).placeholder("Optional description")),
            ai_temperature_input: cx.new(|cx| {
                let val = format!("{:.1}", s.ai_temperature);
                let mut state = InputState::new(cx);
                state.set_value(val, cx);
                state
            }),
            ai_max_tokens_input: cx.new(|cx| {
                let val = s.ai_max_tokens.to_string();
                let mut state = InputState::new(cx);
                state.set_value(val, cx);
                state
            }),
            search_add_dialog_open: false,
            search_edit_id: None,
            search_name_input: cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "settings.search_engines.name_placeholder"))
            }),
            search_url_input: cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "settings.search_engines.url_placeholder"))
            }),
            search_keyword_input: cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "settings.search_engines.keyword_placeholder"))
            }),
            active_color_scheme_dialog: None,
            ai_dialog_test_focus: cx.focus_handle(),
            ai_dialog_cancel_focus: cx.focus_handle(),
            ai_dialog_save_focus: cx.focus_handle(),
            search_dialog_cancel_focus: cx.focus_handle(),
            search_dialog_save_focus: cx.focus_handle(),
            ai_test_result: None,
            ai_test_in_progress: false,
            security_new_password_input: cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "settings.security.master_password_placeholder"))
                    .masked(true)
            }),
            security_current_password_input: cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "settings.security.current_password_placeholder"))
                    .masked(true)
            }),
            security_confirm_password_input: cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "settings.security.confirm_password_placeholder"))
                    .masked(true)
            }),
            security_result: None,
            security_busy: false,
            security_change_mode: false,
            security_remove_mode: false,
            security_setup_mode: false,
            stepper_inputs: {
                let mut map = HashMap::new();
                let keys = [
                    // General
                    "proxy-port",
                    // Appearance
                    "titlebar-height",
                    "button-gap",
                    "control-margin",
                    "window-corner-radius",
                    "window-control-icon-size",
                    // Font
                    "ui-font-size",
                    "ui-scale",
                    "font-size",
                    "line-height",
                    // Terminal
                    "scrollback",
                    "bell-cooldown",
                    "idle-timeout",
                    "close-grace-secs",
                    "history-max-count",
                    "history-retention-days",
                    // Sync
                    "sync-interval",
                    // Security
                    "password-timeout",
                ];
                for key in keys {
                    let input = cx.new(|cx| InputState::new(cx));
                    map.insert(key.to_string(), input);
                }
                map
            },
            bg_opacity_slider: Some(cx.new(|cx| {
                velowork_ui::slider::SliderState::new(cx)
                    .min(10.0)
                    .max(100.0)
                    .step(1.0)
                    .value(settings_entity(cx).read(cx).settings.bg_opacity * 100.0)
            })),
            nav_focus_handle: cx.focus_handle(),
            toggle_focus_handles: {
                let mut map = HashMap::new();
                let ids = [
                    "start-on-boot",
                    "auto-check-updates",
                    "desktop-notifications",
                    "notify-osc",
                    "notify-bell",
                    "focus-border",
                    "enable-animations",
                    "color-tinted-bg",
                    "enable-tab-preview",
                    "show-shell-selector",
                    "restore-terminals",
                    "cursor-blink",
                    "show-line-numbers",
                    "terminal-bg-blur",
                    "ctrl-c-copies",
                    "copy-on-select",
                    "right-click-paste",
                    "shell-integration",
                    "bracketed-paste",
                    "osc52-clipboard",
                    "true-color",
                    "idle-detection",
                    "close-grace",
                    "history-auto-completion",
                    "history-ignore-space",
                    "show-hidden-files",
                    "alternating-row-bg",
                    "auto-sync",
                    "ai-enable",
                    "ai-auto-compress",
                ];
                for id in ids {
                    map.insert(id.to_string(), cx.focus_handle());
                }
                RefCell::new(map)
            },
            radio_focus_handles: {
                let mut map = HashMap::new();
                let ids = [
                    "close-behavior",
                    "proxy-mode",
                    "color-theme",
                    "titlebar-style",
                    "titlebar-position",
                    "ui-density",
                    "tab-width-mode",
                    "cursor-style",
                    "terminal-scrollbar-show",
                    "bell-style",
                    "file-sort-by",
                ];
                for id in ids {
                    map.insert(id.to_string(), cx.focus_handle());
                }
                RefCell::new(map)
            },
            button_focus_handles: {
                let mut map = HashMap::new();
                let ids = [
                    "terminal-bg-image-picker",
                    "manage-color-schemes-btn",
                    "security-enable-enhanced",
                    "security-setup-confirm",
                    "security-setup-cancel",
                    "security-change-pw",
                    "security-remove-pw",
                    "security-change-confirm",
                    "security-change-cancel",
                    "security-remove-confirm",
                    "security-remove-cancel",
                    "ai-add-model-btn",
                    "data-root-browse-btn",
                    "apply-data-root-btn",
                    "search-add-engine-btn",
                    "sync-test-conn-btn",
                    "sync-now-btn",
                    "sync-restore-backup-btn",
                    "sync-force-push-btn",
                ];
                for id in ids {
                    map.insert(id.to_string(), cx.focus_handle());
                }
                RefCell::new(map)
            },
            dialog_trigger_focus_handle: None,
            pending_scroll_focus_handle: None,
        };

        // 离帧调度初次聚焦首个配置项（语言选择器），严禁在 render 同步抢焦
        let initial_fh = panel.language_select.read(cx).focus_handle().clone();
        _window.defer(cx, move |window, cx| {
            window.focus(&initial_fh, cx);
        });

        panel.with_security_input_subscription(cx)
    }

    /// 订阅主密码输入框的变更事件，使面板在输入内容变化时重渲染
    /// （用于「未输入密码时隐藏启用按钮」等按内容显隐的交互）。
    fn with_security_input_subscription(self, cx: &mut Context<Self>) -> Self {
        let input = self.security_new_password_input.clone();
        cx.subscribe(&input, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        self
    }

    pub fn set_overlay_registry(&mut self, reg: Entity<OverlayRegistry>, cx: &mut Context<Self>) {
        self.overlay_registry = Some(reg.clone());
        self.language_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.theme_mode_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.dark_palette_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.light_palette_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.titlebar_preset_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.ui_font_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.font_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.font_weight_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.text_antialiasing_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.shell_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.session_backend_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.charset_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.color_scheme_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.term_type_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.ai_default_model_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
        self.ai_compression_strategy_select
            .update(cx, |s, _| s.set_overlay_registry(reg.clone()));
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(SettingsPanelEvent::Close);
    }

    /// Create a bounds tracking callback for dropdown buttons.
    pub(super) fn bounds_setter(
        cx: &mut Context<Self>,
        setter: fn(&mut Self, Option<Bounds<Pixels>>),
    ) -> impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static {
        let entity = cx.entity().downgrade();
        move |bounds, _, cx: &mut App| {
            if let Some(entity) = entity.upgrade() {
                entity.update(cx, |this, _| setter(this, Some(bounds)));
            }
        }
    }

    pub(super) fn close_all_dropdowns(&mut self) {}

    fn has_open_dropdown(&self) -> bool {
        false
    }

    /// 根据当前右侧滚动偏移量计算当前视口顶部的分类卡片，并同步到左侧导航高亮选中项。
    fn sync_active_category_from_scroll(&mut self, cx: &App) {
        if self.pending_scroll.borrow().is_none() {
            let scroll_y = -f32::from(self.scroll_handle.offset().y);
            if scroll_y >= 0.0 {
                let categories = self.ordered_categories(cx);
                let gap = f32::from(SPACE_CARD_GAP);
                let mut y_acc: f32 = 0.0;
                let heights = self.card_heights.borrow();
                let mut active_cat = None;

                for cat in &categories {
                    let is_expanded = self.expanded_categories.contains(cat);
                    let card_h = heights
                        .get(cat)
                        .copied()
                        .unwrap_or_else(|| {
                            if is_expanded {
                                self.default_expanded_height(cat, cx)
                            } else {
                                44.0
                            }
                        });
                    if active_cat.is_none() {
                        active_cat = Some(cat.clone());
                    }
                    if scroll_y + 30.0 >= y_acc {
                        active_cat = Some(cat.clone());
                    }
                    y_acc += card_h + gap;
                }

                if let Some(cat) = active_cat {
                    if self.active_category != cat {
                        self.active_category = cat;
                    }
                }
            }
        }
    }

    /// 右侧单一滚动区：按 `ordered_categories` 顺序渲染全部分类折叠卡片。
    /// 基于 canvas 动态真实 Layout 测量记录各卡片高度，在 track_scroll 之前精准设定目标 y 偏移量，
    /// 解决 GPUI 渲染帧序机制下 prepaint 延迟滚动的问题，确保点击导航、向上/向下滚动以及展开卡片时全场景精准贴顶。
    fn render_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 检查是否有 Tab 带来的待滚动焦点句柄（延迟滚动定位）
        if let Some(fh) = self.pending_scroll_focus_handle.take() {
            self.scroll_handle_into_view(&fh, cx);
        }

        let categories = self.ordered_categories(cx);

        // 检查是否有待滚动的导航目标
        let pending_target = self.pending_scroll.borrow().clone();
        if let Some(ref target) = pending_target {
            let gap = f32::from(SPACE_CARD_GAP);
            let mut y_acc: f32 = 0.0;
            let mut target_y: Option<f32> = None;
            let mut all_measured = true;
            let heights = self.card_heights.borrow();

            for cat in &categories {
                if cat == target {
                    target_y = Some(y_acc);
                    break;
                }
                if !heights.contains_key(cat) {
                    all_measured = false;
                }
                let is_expanded = self.expanded_categories.contains(cat);
                let card_h = heights
                    .get(cat)
                    .copied()
                    .unwrap_or_else(|| {
                        if is_expanded {
                            self.default_expanded_height(cat, cx)
                        } else {
                            44.0
                        }
                    });
                y_acc += card_h + gap;
            }

            if let Some(y) = target_y {
                self.scroll_handle.set_offset(point(px(0.0), px(-y)));
            }

            if all_measured && !heights.is_empty() {
                *self.pending_scroll.borrow_mut() = None;
            }
        }

        let card_heights_rc = self.card_heights.clone();

        let mut right = div()
            .id("settings-scroll")
            .relative()
            .flex()
            .flex_col()
            .gap(SPACE_CARD_GAP)
            .overflow_y_scroll()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .px(SPACE_XL)
            .py(SPACE_MD)
            .track_scroll(&self.scroll_handle)
            .focus_scope_on_click(&self.focus_handle)
            .flex_1();

        for (i, cat) in categories.iter().enumerate() {
            let cat_card = self.render_category_card(cat.clone(), i, window, cx);
            let cat_key = cat.clone();
            let height_setter = card_heights_rc.clone();
            let pending_scroll_rc = self.pending_scroll.clone();
            let notify_entity = cx.entity().downgrade();

            let card_wrapper = div().relative().child(cat_card).child(
                canvas(
                    move |bounds, _, cx| {
                        let h = f32::from(bounds.size.height);
                        let prev_h = height_setter.borrow().get(&cat_key).copied();
                        height_setter
                            .borrow_mut()
                            .insert(cat_key.clone(), h);

                        // 当存在待滚动导航且卡片真实高度刚完成初次测量或发生显著变化时，触发重绘以在下一帧以真实像素精准对齐
                        if pending_scroll_rc.borrow().is_some()
                            && (prev_h.is_none() || (prev_h.unwrap() - h).abs() > 1.0)
                        {
                            if let Some(entity) = notify_entity.upgrade() {
                                entity.update(cx, |_, cx| cx.notify());
                            }
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            );

            right = right.child(card_wrapper);
        }

        // 底部弹性安全间距：确保最后几个分类卡片点击导航时也能完整滚动置顶，避免触底提前截断导致高亮回跳
        right = right.child(
            div()
                .h(velowork_ui::tokens::SCROLL_BOTTOM_SPACER_H)
                .flex_shrink_0(),
        );

        div()
            .relative()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_hidden()
            .child(right.w_full().h_full())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .left_0()
                    .child(Scrollbar::vertical(&self.scroll_handle)),
            )
    }

    /// 左侧导航与右侧滚动区共用的有序分类列表（静态分类 + 已启用的扩展分类）。
    fn ordered_categories(&self, cx: &App) -> Vec<SettingsCategory> {
        let mut cats: Vec<SettingsCategory> = SettingsCategory::all().to_vec();
        if let Some(registry) = cx.try_global::<ExtensionRegistry>() {
            let enabled = settings_entity(cx)
                .read(cx)
                .settings
                .enabled_extensions
                .clone();
            for ext in registry.extensions().iter() {
                if ext.settings_view.is_some() && enabled.contains(ext.manifest.id) {
                    cats.push(SettingsCategory::Extension(ext.manifest.id.to_string()));
                }
            }
        }
        cats
    }

    /// 分类显示标题（扩展分类回退到扩展注册表中的名称）。
    fn category_title(&self, cat: &SettingsCategory, cx: &App) -> String {
        match cat {
            SettingsCategory::Extension(id) => cx
                .try_global::<ExtensionRegistry>()
                .and_then(|r| r.extensions().iter().find(|e| e.manifest.id == *id))
                .map(|e| e.manifest.name.to_string())
                .unwrap_or_else(|| id.clone()),
            other => other.label(cx),
        }
    }

    /// 导航点击：高亮 + 展开 + 滚动定位到目标分类。
    pub fn nav_to_category(&mut self, cat: SettingsCategory, cx: &mut Context<Self>) {
        self.active_category = cat.clone();
        self.expanded_categories.insert(cat.clone());
        *self.pending_scroll.borrow_mut() = Some(cat);
        self.close_all_dropdowns();
        cx.notify();
    }

    /// 渲染单个分类折叠卡片：表头可点击展开/收起；展开时渲染该分类内容体。
    /// 视觉语言对齐「新建会话」弹窗的 section-card（圆角卡片 + 表头 + chevron）。
    fn render_category_card(
        &mut self,
        cat: SettingsCategory,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx);
        let palette = velowork_ui::SemanticPalette::from_context(cx);
        let _active = self.active_category == cat;
        let title = self.category_title(&cat, cx);

        let content = self.render_category_body(&cat, window, cx);

        div()
            .id(ElementId::Name(format!("settings-card-{}", index).into()))
            .w_full()
            .flex_shrink_0()
            .bg(palette.surface_card)
            .border_1()
            .border_color(palette.border_subtle)
            .rounded(RADIUS_CARD)
            .px(SPACE_LG)
            .py(SPACE_MD)
            .child(
                div()
                    .id(ElementId::Name(format!("settings-card-h-{}", index).into()))
                    .pb(SPACE_SM)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(SPACE_MD)
                            .child(
                                cat.icon()
                                    .size(crate::ui::tokens::ui_icon_std_ts(cx))
                                    .text_color(palette.text_secondary),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .text_size(ui_text_lg(cx))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(t.text_primary))
                                    .child(title),
                            ),
                    ),
            )
            .child(div().w_full().child(content))
            .into_any_element()
    }

    /// 根据分类分发到对应的内容渲染函数（返回裸内容体，不含卡片头）。
    fn render_category_body(
        &mut self,
        cat: &SettingsCategory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match cat {
            SettingsCategory::General => self.render_general(window, cx).into_any_element(),
            SettingsCategory::Font => self.render_font(window, cx).into_any_element(),
            SettingsCategory::Terminal => self.render_terminal(window, cx).into_any_element(),
            SettingsCategory::Extensions => self.render_extensions(cx).into_any_element(),
            SettingsCategory::Appearance => self.render_appearance(window, cx).into_any_element(),
            SettingsCategory::FileManager => self.render_file_manager(cx).into_any_element(),
            SettingsCategory::Security => self.render_security(window, cx).into_any_element(),
            SettingsCategory::Sync => self.render_sync(window, cx).into_any_element(),
            SettingsCategory::AiAssistant => self.render_ai(cx).into_any_element(),
            SettingsCategory::SearchEngines => self.render_search(cx).into_any_element(),
            SettingsCategory::DataStorage => self.render_data_storage(cx).into_any_element(),
            SettingsCategory::Extension(id) => self.render_extension_settings(id.clone(), cx),
        }
    }

    fn render_extension_settings(&mut self, ext_id: String, cx: &mut Context<Self>) -> AnyElement {
        // Lazily create and cache the extension's settings view
        if !self.extension_views.contains_key(&ext_id) {
            // Clone the factory out to avoid holding a borrow on cx
            let factory = cx.try_global::<ExtensionRegistry>().and_then(|registry| {
                registry
                    .extensions()
                    .iter()
                    .find(|ext| ext.manifest.id == ext_id)
                    .and_then(|ext| ext.settings_view.clone())
            });
            if let Some(factory) = factory {
                let view = factory(cx);
                self.extension_views.insert(ext_id.clone(), view);
            }
        }

        if let Some(view) = self.extension_views.get(&ext_id) {
            view.clone().into_any_element()
        } else {
            div().into_any_element()
        }
    }

    /// Open terminal color scheme manager dialog.
    pub(super) fn open_color_scheme_dialog(&mut self, cx: &mut Context<Self>) {
        let dialog = cx.new(TerminalColorSchemeDialog::new);
        let enable_animations = settings_entity(cx).read(cx).settings.enable_animations;

        let animated = cx.new(|cx| {
            AnimatedModal::new(dialog.clone().into(), cx)
                .with_animations(enable_animations, cx)
        });

        let anim_handle = animated.clone();
        cx.subscribe(
            &dialog,
            move |this, _, event: &TerminalColorSchemeDialogEvent, cx| match event {
                TerminalColorSchemeDialogEvent::Close => {
                    anim_handle.update(cx, |modal, cx| modal.request_close(cx));
                }
                TerminalColorSchemeDialogEvent::SchemeSaved => {
                    this.refresh_color_scheme_select(cx);
                    anim_handle.update(cx, |modal, cx| modal.request_close(cx));
                }
            },
        )
        .detach();

        cx.subscribe(&animated, |this, _, event: &AnimatedModalEvent, cx| match event {
            AnimatedModalEvent::Dismissed => {
                this.active_color_scheme_dialog = None;
                cx.notify();
            }
            AnimatedModalEvent::Closing => {}
        })
        .detach();

        self.active_color_scheme_dialog = Some(animated);
        cx.notify();
    }

    /// Refresh color scheme select dropdown options from current settings.
    pub(super) fn refresh_color_scheme_select(&mut self, cx: &mut Context<Self>) {
        let s = settings_entity(cx).read(cx).settings.clone();
        let cur_color_scheme = if s.color_scheme.is_empty() {
            "Dark".to_string()
        } else {
            s.color_scheme.clone()
        };
        let mut color_scheme_options = vec![
            SelectOption::new("Dark".to_string(), "Dark"),
            SelectOption::new("Light".to_string(), "Light"),
            SelectOption::new("Solarized Dark".to_string(), "Solarized Dark"),
            SelectOption::new("Solarized Light".to_string(), "Solarized Light"),
            SelectOption::new("Monokai".to_string(), "Monokai"),
            SelectOption::new("Dracula".to_string(), "Dracula"),
            SelectOption::new("Nord".to_string(), "Nord"),
            SelectOption::new("One Dark".to_string(), "One Dark"),
            SelectOption::new("Gruvbox Dark".to_string(), "Gruvbox Dark"),
        ];
        for custom in &s.custom_terminal_color_schemes {
            color_scheme_options.push(SelectOption::new(custom.name.clone(), custom.name.clone()));
        }
        self.color_scheme_select.update(cx, |select, cx| {
            select.set_options(color_scheme_options, cx);
            select.set_selected_value(Some(cur_color_scheme), cx);
        });
    }

    /// 收集指定分类下所有可见/可交互控件的焦点句柄（严格与界面视觉顺序保持 100% 一致）
    pub fn category_focus_handles(&self, cat: &SettingsCategory, cx: &App) -> Vec<FocusHandle> {
        let mut handles = Vec::new();
        let s = settings_entity(cx).read(cx).settings.clone();

        match cat {
            SettingsCategory::General => {
                // 1. Language
                handles.push(self.language_select.read(cx).focus_handle().clone());
                // 2. Application toggles (严格与 render_general 顺序一致：开机自启 -> 自动检查更新 -> 关闭窗口行为)
                handles.push(self.get_or_create_toggle_focus_handle("start-on-boot", cx));
                handles.push(self.get_or_create_toggle_focus_handle("auto-check-updates", cx));
                handles.push(self.get_or_create_radio_focus_handle("close-behavior", cx));
                // 3. Proxy mode
                handles.push(self.get_or_create_radio_focus_handle("proxy-mode", cx));
                if s.proxy_mode == crate::workspace::settings::ProxyMode::Http {
                    handles.push(self.proxy_host_input.read(cx).focus_handle(cx));
                    if let Some(input) = self.stepper_inputs.get("proxy-port") {
                        handles.push(input.read(cx).focus_handle(cx));
                    }
                }
                // 4. Notifications
                handles.push(self.get_or_create_toggle_focus_handle("desktop-notifications", cx));
                if s.notifications.enabled {
                    handles.push(self.get_or_create_toggle_focus_handle("notify-osc", cx));
                    handles.push(self.get_or_create_toggle_focus_handle("notify-bell", cx));
                }
            }
            SettingsCategory::Appearance => {
                handles.push(self.get_or_create_radio_focus_handle("color-theme", cx));
                handles.push(self.dark_palette_select.read(cx).focus_handle().clone());
                handles.push(self.light_palette_select.read(cx).focus_handle().clone());
                if let Some(ref slider) = self.bg_opacity_slider {
                    handles.push(slider.read(cx).focus_handle().clone());
                }
                handles.push(self.get_or_create_toggle_focus_handle("focus-border", cx));
                handles.push(self.get_or_create_toggle_focus_handle("enable-animations", cx));
                handles.push(self.get_or_create_toggle_focus_handle("color-tinted-bg", cx));
                handles.push(self.get_or_create_radio_focus_handle("titlebar-style", cx));
                if s.titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom {
                    handles.push(self.titlebar_preset_select.read(cx).focus_handle().clone());
                    handles.push(self.get_or_create_radio_focus_handle("titlebar-position", cx));
                    if let Some(input) = self.stepper_inputs.get("titlebar-height") {
                        handles.push(input.read(cx).focus_handle(cx));
                    }
                    if let Some(input) = self.stepper_inputs.get("button-gap") {
                        handles.push(input.read(cx).focus_handle(cx));
                    }
                    if let Some(input) = self.stepper_inputs.get("control-margin") {
                        handles.push(input.read(cx).focus_handle(cx));
                    }
                    if let Some(input) = self.stepper_inputs.get("window-corner-radius") {
                        handles.push(input.read(cx).focus_handle(cx));
                    }
                    if let Some(input) = self.stepper_inputs.get("window-control-icon-size") {
                        handles.push(input.read(cx).focus_handle(cx));
                    }
                }
                handles.push(self.get_or_create_radio_focus_handle("ui-density", cx));
                handles.push(self.get_or_create_radio_focus_handle("tab-width-mode", cx));
                handles.push(self.get_or_create_toggle_focus_handle("enable-tab-preview", cx));
            }
            SettingsCategory::Font => {
                if let Some(input) = self.stepper_inputs.get("ui-font-size") {
                    handles.push(input.read(cx).focus_handle(cx));
                }
                handles.push(self.ui_font_select.read(cx).focus_handle().clone());
                if let Some(input) = self.stepper_inputs.get("ui-scale") {
                    handles.push(input.read(cx).focus_handle(cx));
                }
                if let Some(input) = self.stepper_inputs.get("font-size") {
                    handles.push(input.read(cx).focus_handle(cx));
                }
                handles.push(self.font_select.read(cx).focus_handle().clone());
                handles.push(self.font_weight_select.read(cx).focus_handle().clone());
                handles.push(self.text_antialiasing_select.read(cx).focus_handle().clone());
                if let Some(input) = self.stepper_inputs.get("line-height") {
                    handles.push(input.read(cx).focus_handle(cx));
                }
            }
            SettingsCategory::Terminal => {
                handles.push(self.shell_select.read(cx).focus_handle().clone());
                handles.push(self.get_or_create_toggle_focus_handle("show-shell-selector", cx));
                handles.push(self.session_backend_select.read(cx).focus_handle().clone());
                handles.push(self.term_type_select.read(cx).focus_handle().clone());
                handles.push(self.charset_select.read(cx).focus_handle().clone());
                handles.push(self.get_or_create_toggle_focus_handle("restore-terminals", cx));
                handles.push(self.color_scheme_select.read(cx).focus_handle().clone());
                handles.push(self.get_or_create_button_focus_handle("manage-color-schemes-btn", cx));
                handles.push(self.get_or_create_radio_focus_handle("cursor-style", cx));
                handles.push(self.get_or_create_toggle_focus_handle("cursor-blink", cx));
                handles.push(self.get_or_create_radio_focus_handle("terminal-scrollbar-show", cx));
                if let Some(input) = self.stepper_inputs.get("scrollback") {
                    handles.push(input.read(cx).focus_handle(cx));
                }
                handles.push(self.get_or_create_toggle_focus_handle("show-line-numbers", cx));
                handles.push(self.terminal_bg_image_input.read(cx).focus_handle(cx));
                handles.push(self.get_or_create_button_focus_handle("terminal-bg-image-picker", cx));
                handles.push(self.get_or_create_toggle_focus_handle("terminal-bg-blur", cx));
                handles.push(self.get_or_create_toggle_focus_handle("ctrl-c-copies", cx));
                handles.push(self.get_or_create_toggle_focus_handle("copy-on-select", cx));
                handles.push(self.get_or_create_toggle_focus_handle("right-click-paste", cx));
                handles.push(self.word_selection_delimiters_input.read(cx).focus_handle(cx));
                handles.push(self.get_or_create_radio_focus_handle("bell-style", cx));
                if let Some(input) = self.stepper_inputs.get("bell-cooldown") {
                    handles.push(input.read(cx).focus_handle(cx));
                }
                handles.push(self.get_or_create_toggle_focus_handle("shell-integration", cx));
                handles.push(self.get_or_create_toggle_focus_handle("bracketed-paste", cx));
                handles.push(self.get_or_create_toggle_focus_handle("osc52-clipboard", cx));
                handles.push(self.get_or_create_toggle_focus_handle("true-color", cx));
                handles.push(self.get_or_create_toggle_focus_handle("idle-detection", cx));
                if s.idle_timeout_secs > 0
                    && let Some(input) = self.stepper_inputs.get("idle-timeout")
                {
                    handles.push(input.read(cx).focus_handle(cx));
                }
                handles.push(self.get_or_create_toggle_focus_handle("close-grace", cx));
                if s.terminal_close_grace_secs > 0
                    && let Some(input) = self.stepper_inputs.get("close-grace-secs")
                {
                    handles.push(input.read(cx).focus_handle(cx));
                }
                handles.push(self.get_or_create_toggle_focus_handle("history-auto-completion", cx));
                handles.push(self.get_or_create_toggle_focus_handle("history-ignore-space", cx));
                handles.push(self.command_history_ignored_commands_input.read(cx).focus_handle(cx));
                if let Some(input) = self.stepper_inputs.get("history-max-count") {
                    handles.push(input.read(cx).focus_handle(cx));
                }
                if let Some(input) = self.stepper_inputs.get("history-retention-days") {
                    handles.push(input.read(cx).focus_handle(cx));
                }
            }
            SettingsCategory::FileManager => {
                handles.push(self.file_opener_input.read(cx).focus_handle(cx));
                handles.push(self.get_or_create_toggle_focus_handle("show-hidden-files", cx));
                handles.push(self.get_or_create_radio_focus_handle("file-sort-by", cx));
                handles.push(self.get_or_create_toggle_focus_handle("alternating-row-bg", cx));
                handles.push(self.sftp_file_mode_input.read(cx).focus_handle(cx));
                handles.push(self.sftp_dir_mode_input.read(cx).focus_handle(cx));
            }
            SettingsCategory::Security => {
                if s.security.security_mode == "enhanced" {
                    handles.push(self.get_or_create_button_focus_handle("security-change-pw", cx));
                    handles.push(self.get_or_create_button_focus_handle("security-remove-pw", cx));
                    if self.security_change_mode {
                        handles.push(self.security_current_password_input.read(cx).focus_handle(cx));
                        handles.push(self.security_new_password_input.read(cx).focus_handle(cx));
                        handles.push(self.get_or_create_button_focus_handle("security-change-confirm", cx));
                        handles.push(self.get_or_create_button_focus_handle("security-change-cancel", cx));
                    }
                    if self.security_remove_mode {
                        handles.push(self.security_current_password_input.read(cx).focus_handle(cx));
                        handles.push(self.get_or_create_button_focus_handle("security-remove-confirm", cx));
                        handles.push(self.get_or_create_button_focus_handle("security-remove-cancel", cx));
                    }
                    if let Some(input) = self.stepper_inputs.get("password-timeout") {
                        handles.push(input.read(cx).focus_handle(cx));
                    }
                } else if self.security_setup_mode {
                    handles.push(self.security_new_password_input.read(cx).focus_handle(cx));
                    handles.push(self.security_confirm_password_input.read(cx).focus_handle(cx));
                    handles.push(self.get_or_create_button_focus_handle("security-setup-confirm", cx));
                    handles.push(self.get_or_create_button_focus_handle("security-setup-cancel", cx));
                } else {
                    handles.push(self.get_or_create_button_focus_handle("security-enable-enhanced", cx));
                }
            }
            SettingsCategory::Sync => {
                handles.push(self.sync_provider_select.read(cx).focus_handle().clone());
                if s.sync.enabled && s.sync.provider == SyncProvider::WebDav {
                    handles.push(self.sync_server_url_input.read(cx).focus_handle(cx));
                    handles.push(self.sync_username_input.read(cx).focus_handle(cx));
                    handles.push(self.sync_password_input.read(cx).focus_handle(cx));
                    handles.push(self.sync_remote_path_input.read(cx).focus_handle(cx));
                    handles.push(self.get_or_create_button_focus_handle("sync-test-conn-btn", cx));
                    handles.push(self.get_or_create_toggle_focus_handle("sync-scope-sessions", cx));
                    handles.push(self.get_or_create_toggle_focus_handle("sync-scope-tunnels", cx));
                    handles.push(self.get_or_create_toggle_focus_handle("sync-scope-services", cx));
                    handles.push(self.get_or_create_toggle_focus_handle("sync-scope-qc", cx));
                    handles.push(self.get_or_create_toggle_focus_handle("sync-scope-ai", cx));
                    handles.push(self.get_or_create_toggle_focus_handle("sync-scope-history", cx));
                    handles.push(self.get_or_create_toggle_focus_handle("sync-scope-settings", cx));
                    handles.push(self.get_or_create_toggle_focus_handle("sync-scope-themes", cx));
                    handles.push(self.get_or_create_toggle_focus_handle("sync-scope-credentials", cx));
                    if let Some(input) = self.stepper_inputs.get("sync-interval") {
                        handles.push(input.read(cx).focus_handle(cx));
                    }
                    handles.push(self.get_or_create_toggle_focus_handle("sync-auto-sync-toggle", cx));
                    handles.push(self.get_or_create_button_focus_handle("sync-now-btn", cx));
                    handles.push(self.get_or_create_button_focus_handle("sync-restore-backup-btn", cx));
                    handles.push(self.get_or_create_button_focus_handle("sync-force-push-btn", cx));
                }
            }
            SettingsCategory::AiAssistant => {
                handles.push(self.get_or_create_toggle_focus_handle("ai-enable", cx));
                if s.ai_enabled {
                    for model in &s.ai_models {
                        handles.push(self.get_or_create_toggle_focus_handle(&format!("ai-model-toggle-{}", model.id), cx));
                        handles.push(self.get_or_create_button_focus_handle(&format!("ai-model-edit-{}", model.id), cx));
                        handles.push(self.get_or_create_button_focus_handle(&format!("ai-model-delete-{}", model.id), cx));
                    }
                    handles.push(self.get_or_create_button_focus_handle("ai-add-model-btn", cx));
                    let builtin_skills = velowork_ai::SkillRegistry::new(velowork_ai::builtin_skills()).metas();
                    for meta in builtin_skills {
                        handles.push(self.get_or_create_toggle_focus_handle(&format!("skill-toggle-{}", meta.name), cx));
                    }
                    handles.push(self.ai_default_model_select.read(cx).focus_handle().clone());
                    handles.push(self.ai_temperature_input.read(cx).focus_handle(cx));
                    handles.push(self.ai_max_tokens_input.read(cx).focus_handle(cx));
                    handles.push(self.get_or_create_toggle_focus_handle("ai-auto-compress", cx));
                    handles.push(self.ai_compression_strategy_select.read(cx).focus_handle().clone());
                    handles.push(self.ai_max_context_tokens_input.read(cx).focus_handle(cx));
                    handles.push(self.ai_max_history_messages_input.read(cx).focus_handle(cx));
                }
            }
            SettingsCategory::SearchEngines => {
                for engine in &s.search_engines {
                    handles.push(self.get_or_create_toggle_focus_handle(&format!("search-engine-toggle-{}", engine.id), cx));
                    handles.push(self.get_or_create_button_focus_handle(&format!("search-engine-edit-{}", engine.id), cx));
                    handles.push(self.get_or_create_button_focus_handle(&format!("search-engine-delete-{}", engine.id), cx));
                }
                handles.push(self.get_or_create_button_focus_handle("search-add-engine-btn", cx));
            }
            SettingsCategory::DataStorage => {
                handles.push(self.data_root_mode_select.read(cx).focus_handle().clone());
                if self.data_root_mode_select.read(cx).selected_value().copied().unwrap_or_default() == velowork_core::data_root::DataRootMode::Custom {
                    handles.push(self.data_root_custom_input.read(cx).focus_handle(cx));
                    handles.push(self.get_or_create_button_focus_handle("data-root-browse-btn", cx));
                    handles.push(self.get_or_create_button_focus_handle("apply-data-root-btn", cx));
                }
            }
            SettingsCategory::Extensions => {}
            SettingsCategory::Extension(_) => {}
        }

        handles
    }

    /// 全量线性有序焦点句柄列表：搜索框 -> 左侧导航 -> 各分类配置项
    pub fn all_focus_handles(&self, cx: &App) -> Vec<FocusHandle> {
        let mut handles = Vec::new();
        handles.push(self.nav_search_input.read(cx).focus_handle(cx));
        handles.push(self.nav_focus_handle.clone());
        let categories = self.ordered_categories(cx);
        for cat in &categories {
            handles.extend(self.category_focus_handles(cat, cx));
        }
        handles
    }

    /// 根据焦点句柄查找所属分类
    pub fn category_for_focus_handle(&self, handle: &FocusHandle, cx: &App) -> Option<SettingsCategory> {
        let categories = self.ordered_categories(cx);
        for cat in &categories {
            let handles = self.category_focus_handles(cat, cx);
            if handles.contains(handle) {
                return Some(cat.clone());
            }
        }
        None
    }

    /// 获取当前获得键盘焦点且在可视视口内的 SettingsCategory 分组（如有）：
    /// 用于「焦点驱动高亮」，当 Tab 或交互使某卡片内控件获焦时，
    /// 左侧导航立即高亮该卡片，避免因无需滚动而被 Scroll Spy 反向覆写。
    pub fn focused_category(&self, window: &Window, cx: &App) -> Option<SettingsCategory> {
        let categories = self.ordered_categories(cx);
        for cat in &categories {
            let handles = self.category_focus_handles(cat, cx);
            for h in &handles {
                if h.is_focused(window) {
                    let (top, bottom) = self.handle_y_range_in_category(cat, h, cx);
                    let cat_top = self.category_top_offset(cat, cx);
                    let item_top = cat_top + top;
                    let item_bottom = cat_top + bottom;
                    let cur_scroll_y = -f32::from(self.scroll_handle.offset().y);
                    let vp_h = f32::from(self.scroll_handle.bounds().size.height);
                    if vp_h > 0.0 {
                        // 若控件已被完全滚出视口顶部或底部，让位给 Scroll Spy
                        if item_bottom < cur_scroll_y || item_top > cur_scroll_y + vp_h {
                            return None;
                        }
                    }
                    return Some(cat.clone());
                }
            }
        }
        None
    }

    /// 针对不同设置分类在展开状态下的基准预估高度（对齐 SessionDialog 预估高度机制，
    /// 动态感知自定义标题栏、代理模式、数据同步等动态展开项，用于首次布局或无测量时的精准坐标对齐）
    pub fn default_expanded_height(&self, cat: &SettingsCategory, cx: &App) -> f32 {
        let s = settings_entity(cx).read(cx).settings.clone();
        match cat {
            SettingsCategory::General => {
                let mut h = 380.0;
                if s.proxy_mode == crate::workspace::settings::ProxyMode::Http {
                    h += 96.0;
                }
                if s.notifications.enabled {
                    h += 96.0;
                }
                h
            }
            SettingsCategory::Appearance => {
                if s.titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom {
                    // 自定义标题栏开启时展开 7 个细分配置项（预设、位置、高度、间距、边距、圆角、图标大小）
                    1060.0
                } else {
                    680.0
                }
            }
            SettingsCategory::Font => 520.0,
            SettingsCategory::Terminal => 1100.0,
            SettingsCategory::FileManager => 420.0,
            SettingsCategory::Security => {
                if s.security.security_mode == "enhanced" {
                    520.0
                } else {
                    320.0
                }
            }
            SettingsCategory::Sync => {
                if s.sync.enabled {
                    540.0
                } else {
                    320.0
                }
            }
            SettingsCategory::AiAssistant => 560.0,
            SettingsCategory::SearchEngines => 480.0,
            SettingsCategory::DataStorage => 460.0,
            SettingsCategory::Extensions => 400.0,
            SettingsCategory::Extension(_) => 350.0,
        }
    }

    /// 计算指定分类卡片在右侧滚动容器内容空间的顶部 Y 偏移量
    pub fn category_top_offset(&self, target: &SettingsCategory, cx: &App) -> f32 {
        let categories = self.ordered_categories(cx);
        let gap = f32::from(SPACE_CARD_GAP);
        let heights = self.card_heights.borrow();
        let mut y_acc = 0.0;
        for cat in &categories {
            if cat == target {
                break;
            }
            let is_expanded = self.expanded_categories.contains(cat);
            let card_h = heights
                .get(cat)
                .copied()
                .unwrap_or_else(|| {
                    if is_expanded {
                        self.default_expanded_height(cat, cx)
                    } else {
                        44.0
                    }
                });
            y_acc += card_h + gap;
        }
        y_acc
    }

    /// 计算指定分类中某一焦点控件在该卡片内部的相对垂直范围 [top, bottom]
    /// 采用与新建会话弹窗一致的离散行高与分组偏移计算，彻底避免线性插值造成的坐标放大与滚动过多问题。
    pub fn handle_y_range_in_category(
        &self,
        cat: &SettingsCategory,
        handle: &FocusHandle,
        cx: &App,
    ) -> (f32, f32) {
        let handles = self.category_focus_handles(cat, cx);
        let row_idx = handles.iter().position(|h| h == handle).unwrap_or(0);

        let header_offset = 44.0 + 36.0;
        let row_h = 48.0;

        let in_card_top = match cat {
            SettingsCategory::Appearance => {
                // 前4项在「主题模式」分组，第5项及之后在「界面元素」分组（包含额外小节表头与间距 ~40px）
                if row_idx < 4 {
                    header_offset + row_idx as f32 * row_h
                } else {
                    header_offset + 40.0 + row_idx as f32 * row_h
                }
            }
            SettingsCategory::General => {
                header_offset + row_idx as f32 * row_h
            }
            _ => header_offset + row_idx as f32 * row_h,
        };

        (in_card_top, in_card_top + 40.0)
    }

    /// 视口自适应平滑滚动（Scroll Into View with 40px Safe Viewport Padding）：
    /// 当 Tab 或交互导致某控件获焦时，确保其完整展示在视口内；若已在视口舒适区域内则绝不触发多余滚动，
    /// 严格对齐 SessionDialog 视口微调算法，彻底修复开启自定义标题栏时选项展开导致的滚动过多问题。
    pub fn scroll_handle_into_view(&mut self, handle: &FocusHandle, cx: &App) {
        let Some(cat) = self.category_for_focus_handle(handle, cx) else {
            return;
        };

        self.expanded_categories.insert(cat.clone());
        self.active_category = cat.clone();

        let vp_bounds = self.scroll_handle.bounds();
        let viewport_h = f32::from(vp_bounds.size.height);
        if viewport_h <= 0.0 {
            self.pending_scroll_focus_handle = Some(handle.clone());
            return;
        }

        let pad = 40.0;
        let cur_scroll_y = -f32::from(self.scroll_handle.offset().y);
        let max_offset_y = f32::from(self.scroll_handle.max_offset().y);

        let (in_card_top, in_card_bottom) = self.handle_y_range_in_category(&cat, handle, cx);
        let cat_top = self.category_top_offset(&cat, cx);

        let item_top = cat_top + in_card_top;
        let item_bottom = cat_top + in_card_bottom;

        let vis_top = item_top - cur_scroll_y;
        let vis_bottom = item_bottom - cur_scroll_y;

        // 1. 如果当前控件已在视口安全舒适区域内，绝不滚动，保持界面稳定
        if vis_top >= pad && vis_bottom <= viewport_h - pad {
            return;
        }

        // 2. 如果当前项在视口底部下方，平滑向下微调以使其露出，保留 40px 呼吸内边距
        if vis_bottom > viewport_h - pad {
            let needed = item_bottom - (viewport_h - pad);
            let target_scroll_y = needed.min(item_top - pad);
            if target_scroll_y > cur_scroll_y {
                let clamped = target_scroll_y.clamp(0.0, max_offset_y.max(0.0));
                self.scroll_handle.set_offset(point(px(0.0), px(-clamped)));
            }
            return;
        }

        // 3. 如果当前项在视口顶部上方，平滑向上滚动以使其露出，保留 40px 呼吸内边距
        if vis_top < pad {
            let target_scroll_y = item_top - pad;
            if target_scroll_y < cur_scroll_y {
                let clamped = target_scroll_y.clamp(0.0, max_offset_y.max(0.0));
                self.scroll_handle.set_offset(point(px(0.0), px(-clamped)));
            }
        }
    }

    /// 执行 Tab / Shift+Tab 焦点流转，并联动视口自适应平滑滚动
    pub fn cycle_focus(
        &mut self,
        is_shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let handles = self.all_focus_handles(cx);
        if handles.is_empty() {
            return false;
        }

        // 若当前焦点在左侧导航列表且为正向 Tab，直接切入当前活动分类的首项
        if self.nav_focus_handle.is_focused(window) && !is_shift {
            let cat_handles = self.category_focus_handles(&self.active_category, cx);
            if let Some(first) = cat_handles.first() {
                window.focus(first, cx);
                self.scroll_handle_into_view(first, cx);
                return true;
            }
        }

        let cur_idx = handles.iter().position(|h| h.is_focused(window));
        let next_idx = match cur_idx {
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
                    let cat_handles = self.category_focus_handles(&self.active_category, cx);
                    if let Some(first) = cat_handles.first() {
                        handles.iter().position(|h| h == first).unwrap_or(0)
                    } else if handles.len() > 2 {
                        2
                    } else {
                        0
                    }
                }
            }
        };

        let next_handle = &handles[next_idx];
        window.focus(next_handle, cx);
        self.scroll_handle_into_view(next_handle, cx);
        true
    }

    /// 当模态弹窗（AI模型弹窗、搜索引擎弹窗）处于激活状态时，收集弹窗内部专有焦点序列（实现严格的 Focus Trap）
    pub(super) fn active_modal_focus_handles(&self, cx: &App) -> Option<Vec<FocusHandle>> {
        if self.ai_add_model_dialog_open {
            return Some(vec![
                self.ai_model_name_input.read(cx).focus_handle(cx),
                self.ai_model_base_url_input.read(cx).focus_handle(cx),
                self.ai_model_api_key_input.read(cx).focus_handle(cx),
                self.ai_model_id_input.read(cx).focus_handle(cx),
                self.ai_model_desc_input.read(cx).focus_handle(cx),
                self.ai_dialog_test_focus.clone(),
                self.ai_dialog_cancel_focus.clone(),
                self.ai_dialog_save_focus.clone(),
            ]);
        }
        if self.search_add_dialog_open || self.search_edit_id.is_some() {
            return Some(vec![
                self.search_name_input.read(cx).focus_handle(cx),
                self.search_url_input.read(cx).focus_handle(cx),
                self.search_keyword_input.read(cx).focus_handle(cx),
                self.search_dialog_cancel_focus.clone(),
                self.search_dialog_save_focus.clone(),
            ]);
        }
        None
    }

    /// 在指定的焦点序列中循环（用于模态弹窗 Focus Trap，严格在弹窗内闭环且不发生外部视口滚动）
    pub(super) fn cycle_focus_in_handles(
        &mut self,
        handles: &[FocusHandle],
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if handles.is_empty() {
            return;
        }

        let current_index = handles.iter().position(|h| h.is_focused(window));

        let next_index = match current_index {
            Some(idx) => {
                if reverse {
                    if idx == 0 {
                        handles.len() - 1
                    } else {
                        idx - 1
                    }
                } else {
                    (idx + 1) % handles.len()
                }
            }
            None => {
                if reverse {
                    handles.len() - 1
                } else {
                    0
                }
            }
        };

        if let Some(target_handle) = handles.get(next_index) {
            window.focus(target_handle, cx);
        }
    }
}

pub enum SettingsPanelEvent {
    Close,
}

impl EventEmitter<SettingsPanelEvent> for SettingsPanel {}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(focused_cat) = self.focused_category(window, cx) {
            self.active_category = focused_cat;
        } else {
            self.sync_active_category_from_scroll(cx);
        }
        let t = theme(cx);
        let focus_handle = self.focus_handle.clone();

        // 弹窗整体宽度取应用总宽度的 ~80%，以在较宽布局下让右侧可折叠分组滚动区
        // 获得更充裕的空间（左侧导航宽度固定、功能不变，仅扩展右侧可用空间）。
        // 同时约束在合理区间内：最窄 360px，最宽 1200px；低于 720px 时分类导航折叠为顶部标签栏。
        let window_w = window.window_bounds().get_bounds().size.width;
        let narrow = window_w < px(720.0);

        let window_corner_radius =
            if let Some(global) = cx.try_global::<velowork_app_core::settings::GlobalSettings>() {
                global.0.read(cx).settings.window_corner_radius
            } else {
                8.0
            };
        let is_custom_titlebar = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            if let Some(global) = cx.try_global::<velowork_app_core::settings::GlobalSettings>() {
                global.0.read(cx).settings.titlebar_style
                    == velowork_workspace::settings::TitlebarStyle::Custom
            } else {
                true
            }
        } else {
            matches!(window.window_decorations(), Decorations::Client { .. })
        };
        let has_rounded_corners = is_custom_titlebar
            && !window.is_maximized()
            && !window.is_fullscreen()
            && window_corner_radius > 0.0;
        let radius = px(window_corner_radius);

        div()
            .id("settings-panel-root")
            .size_full()
            .flex()
            .flex_col()
            .when(has_rounded_corners, |d| {
                d.rounded_b(radius).overflow_hidden()
            })
            .relative()
            .track_focus(&focus_handle)
            .on_key_down(cx.listener(|this: &mut Self, event: &KeyDownEvent, window, cx| {
                // Tab 键流转与视口自适应平滑滚动联动（40px 呼吸边距，对齐新建会话弹窗规范）
                if event.keystroke.key == "\t" || event.keystroke.key == "tab" {
                    cx.stop_propagation();
                    if this.has_open_dropdown() {
                        this.close_all_dropdowns();
                    }
                    if this.active_color_scheme_dialog.is_some() {
                        return;
                    }
                    let is_shift = event.keystroke.modifiers.shift;
                    if let Some(modal_handles) = this.active_modal_focus_handles(cx) {
                        this.cycle_focus_in_handles(&modal_handles, is_shift, window, cx);
                    } else {
                        this.cycle_focus(is_shift, window, cx);
                    }
                    cx.notify();
                    return;
                }

                // Ctrl+F / Cmd+F 全局快速定位到左侧分类搜索框并全选文本
                let is_ctrl = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
                if is_ctrl && event.keystroke.key == "f" {
                    cx.stop_propagation();
                    this.nav_search_input.update(cx, |input, cx| {
                        input.focus(window, cx);
                        input.select_all(cx);
                    });
                }
            }))
            .key_context("SettingsPanel")
            .when(use_custom_ui_font(cx), |m| {
                m.font_family(ui_font_family(cx))
            })
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                if this.has_open_dropdown() {
                    this.close_all_dropdowns();
                    cx.notify();
                } else if let Some(modal) = this.active_color_scheme_dialog.as_ref() {
                    modal.update(cx, |modal, cx| modal.request_close(cx));
                } else {
                    this.close(cx);
                }
            }))
            .child(
                div()
                    .flex()
                    .when(narrow, |d| d.flex_col())
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_sidebar(narrow, has_rounded_corners, radius, window, cx))
                    .child(self.render_content(window, cx)),
            )
            .when_some(
                if self.terminal_bg_image_input.read(cx).has_suggestions() {
                    self.bg_image_input_bounds
                } else {
                    None
                },
                |modal, bounds| {
                    modal.child(dropdown_anchored_below(
                        bounds,
                        self.render_bg_image_suggestions(cx),
                    ))
                },
            )
            .when_some(
                if self.data_root_custom_input.read(cx).has_suggestions() {
                    self.data_root_input_bounds
                } else {
                    None
                },
                |modal, bounds| {
                    modal.child(dropdown_anchored_below(
                        bounds,
                        self.render_data_root_suggestions(cx),
                    ))
                },
            )
            .when(self.ai_add_model_dialog_open, |modal| {
                modal.child(self.render_add_model_dialog(&t, cx))
            })
            .when(
                self.search_add_dialog_open || self.search_edit_id.is_some(),
                |modal| modal.child(self.render_search_dialog(&t, cx)),
            )
            .when_some(self.active_color_scheme_dialog.clone(), |modal, dialog| {
                modal.child(dialog)
            })
    }
}

impl Focusable for SettingsPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        let handles = self.category_focus_handles(&self.active_category, cx);
        if let Some(first) = handles.first() {
            first.clone()
        } else {
            self.language_select.read(cx).focus_handle().clone()
        }
    }
}
