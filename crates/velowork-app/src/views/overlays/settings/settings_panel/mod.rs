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
use crate::terminal::available_shell_select_options;
use crate::theme::theme;
use crate::ui::tokens::{ui_font_family, use_custom_ui_font};
use crate::views::components::{PathAutoCompleteState, dropdown_anchored_below};
use crate::workspace::settings::SyncProvider;
use crate::workspace::settings::TextAntialiasingMode;
use crate::workspace::state::Workspace;
use gpui::prelude::*;
use gpui::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;
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
    RADIUS_CARD, SPACE_CARD_GAP, SPACE_MD, SPACE_LG, SPACE_XL, ui_text_lg,
};
use velowork_ui::{AnimatedModal, AnimatedModalEvent};

// ============================================================================
// Settings Panel
use crate::terminal::session_backend::SessionBackend;
use crate::terminal::shell_config::ShellType;
use velowork_workspace::settings::{ColorSchema, ColorTheme, CustomTitlebarPreset};

// ============================================================================

/// 同步连接测试状态机
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum SyncTestStatus {
    #[default]
    Idle,
    Testing,
    Success(String),
    Failed(String),
    Cancelled,
}

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
    // Sync inputs (WebDAV)
    pub(super) sync_server_url_input: Entity<InputState>,
    pub(super) sync_username_input: Entity<InputState>,
    pub(super) sync_password_input: Entity<InputState>,
    pub(super) sync_remote_path_input: Entity<InputState>,
    // Sync inputs (S3)
    pub(super) sync_s3_endpoint_input: Entity<InputState>,
    pub(super) sync_s3_bucket_input: Entity<InputState>,
    pub(super) sync_s3_region_input: Entity<InputState>,
    pub(super) sync_s3_access_key_input: Entity<InputState>,
    pub(super) sync_s3_secret_key_input: Entity<InputState>,
    pub(super) sync_s3_prefix_input: Entity<InputState>,
    pub(super) sync_provider_select: Entity<SelectState<Option<SyncProvider>>>,
    pub(super) sync_scope_card: Entity<render_sync::SyncScopeCardView>,
    /// 提供商测试连接状态
    pub(super) sync_test_status: SyncTestStatus,
    pub(super) sync_test_detail: Option<String>,
    pub(super) sync_test_abort_handle: Option<tokio::task::AbortHandle>,
    pub(super) sync_test_task: Option<gpui::Task<()>>,
    /// 立即同步结果：None = 空闲，Some(Ok(msg)) = 成功，Some(Err(msg)) = 失败
    pub(super) sync_result: Option<Result<String, String>>,
    pub(super) sync_detail: Option<String>,
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
    /// 左侧导航搜索框输入实体（定位器）。
    pub(super) nav_search_input: Entity<InputState>,
    /// 导航搜索当前文本（冗余缓存，便于无窗口上下文读取）。
    pub(super) nav_search: String,
    /// 动态按 ID 缓存数字步进器输入框实体
    pub(super) stepper_inputs: HashMap<String, Entity<InputState>>,
    /// 已绑定单次提交防抖逻辑的步进器输入框 ID 集合（防止 render 期重复注册泄漏）
    pub(super) bound_stepper_inputs: HashSet<String>,
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

/// 为设置面板输入框绑定时间戳精确单任务防抖 (300ms) + 失焦 (Blur) + 回车 (PressEnter) 提交逻辑。
/// 彻底阻断长按按键连续输入/退格引发的 Task 调度和堆内存分配风暴，实现零延迟原生手感。
#[allow(clippy::type_complexity)]
fn bind_debounced_input<T: 'static, F>(
    input: &Entity<InputState>,
    cx: &mut Context<T>,
    on_change_extra: Option<Rc<dyn Fn(&mut T, &mut Context<T>)>>,
    on_commit: F,
) where
    F: Fn(&str, &mut T, &mut Context<T>) + 'static,
{
    let on_commit = Rc::new(on_commit);
    let last_committed_val = Rc::new(RefCell::new(None::<String>));
    let committed_clone = last_committed_val.clone();

    cx.subscribe(input, move |this, entity, event: &InputEvent, cx| {
        match event {
            InputEvent::Blur | InputEvent::PressEnter => {
                // 严格在失焦 (FocusOut / Blur) 或按回车 (PressEnter) 时持久化提交
                let val = entity.read(cx).text().to_string();
                let has_changed = committed_clone.borrow().as_deref() != Some(&val);
                if has_changed {
                    *committed_clone.borrow_mut() = Some(val.clone());
                    if let Some(extra) = on_change_extra.as_ref() {
                        extra(this, cx);
                    }
                    on_commit(&val, this, cx);
                }
            }
            _ => {}
        }
    })
    .detach();
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
        bind_debounced_input(&file_opener_input, cx, None, |val, _, cx| {
            settings_entity(cx).update(cx, |state, cx| state.set_file_opener(val.to_string(), cx));
        });

        // SFTP default file permission input
        let sftp_file_mode_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("0644");
            if !s.sftp_default_file_mode.is_empty() {
                state.set_value(s.sftp_default_file_mode.clone(), cx);
            }
            state
        });
        bind_debounced_input(&sftp_file_mode_input, cx, None, |val, _, cx| {
            settings_entity(cx)
                .update(cx, |state, cx| state.set_sftp_default_file_mode(val.to_string(), cx));
        });

        // SFTP default directory permission input
        let sftp_dir_mode_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("0755");
            if !s.sftp_default_dir_mode.is_empty() {
                state.set_value(s.sftp_default_dir_mode.clone(), cx);
            }
            state
        });
        bind_debounced_input(&sftp_dir_mode_input, cx, None, |val, _, cx| {
            settings_entity(cx)
                .update(cx, |state, cx| state.set_sftp_default_dir_mode(val.to_string(), cx));
        });

        // Proxy host input
        let proxy_host_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("e.g. 127.0.0.1");
            if !s.proxy_host.is_empty() {
                state.set_value(s.proxy_host.clone(), cx);
            }
            state
        });
        bind_debounced_input(&proxy_host_input, cx, None, |val, _, cx| {
            settings_entity(cx).update(cx, |state, cx| state.set_proxy_host(val.to_string(), cx));
        });

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
        let inner_dr_input = data_root_custom_input.read(cx).input().clone();
        cx.subscribe(
            &inner_dr_input,
            |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter) {
                    this.recompute_data_root_error(cx);
                    cx.notify();
                }
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
        let inner_bg_input = terminal_bg_image_input.read(cx).input().clone();
        let committed_bg_img = Rc::new(RefCell::new(s.terminal_background_image.clone()));
        let committed_bg_clone = committed_bg_img.clone();
        cx.subscribe(
            &inner_bg_input,
            move |this, entity, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter) {
                    let val = entity.read(cx).text().to_string();
                    let opt = if val.trim().is_empty() {
                        None
                    } else {
                        Some(val.clone())
                    };
                    if *committed_bg_clone.borrow() != opt {
                        *committed_bg_clone.borrow_mut() = opt.clone();
                        this.bg_image_show_success = false;
                        settings_entity(cx)
                            .update(cx, |state, cx| state.set_terminal_background_image(opt, cx));
                    }
                }
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
        bind_debounced_input(&word_selection_delimiters_input, cx, None, |val, _, cx| {
            settings_entity(cx)
                .update(cx, |state, cx| state.set_word_selection_delimiters(val.to_string(), cx));
        });

        // Command history ignored bare commands input
        let command_history_ignored_commands_input = cx.new(|cx| {
            let mut state = InputState::new(cx)
                .placeholder(i18n!(cx, "command_history.setting_ignored_commands_placeholder"));
            if !s.command_history_ignored_commands.is_empty() {
                state.set_value(s.command_history_ignored_commands.join(", "), cx);
            }
            state
        });
        bind_debounced_input(&command_history_ignored_commands_input, cx, None, |raw, _, cx| {
            let list: Vec<String> = raw
                .split(|c| c == ',' || c == '，')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            settings_entity(cx)
                .update(cx, |state, cx| state.set_command_history_ignored_commands(list, cx));
        });

        // Sync inputs
        let sync_server_url_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("https://dav.example.com/dav/");
            if !s.sync.webdav.server_url.is_empty() {
                state.set_value(s.sync.webdav.server_url.clone(), cx);
            }
            state
        });
        bind_debounced_input(
            &sync_server_url_input,
            cx,
            Some(Rc::new(|this, cx| this.reset_sync_test(cx))),
            |val, _, cx| {
                settings_entity(cx).update(cx, |state, cx| state.set_webdav_server_url(val.to_string(), cx));
            },
        );

        let sync_username_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("username");
            if !s.sync.webdav.username.is_empty() {
                state.set_value(s.sync.webdav.username.clone(), cx);
            }
            state
        });
        bind_debounced_input(
            &sync_username_input,
            cx,
            Some(Rc::new(|this, cx| this.reset_sync_test(cx))),
            |val, _, cx| {
                settings_entity(cx).update(cx, |state, cx| state.set_webdav_username(val.to_string(), cx));
            },
        );

        let initial_sync_password =
            velowork_workspace::secure_storage::load_webdav_password().unwrap_or_default();
        let sync_password_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("password")
                .masked(true)
                .default_value(initial_sync_password)
        });
        bind_debounced_input(
            &sync_password_input,
            cx,
            Some(Rc::new(|this, cx| this.reset_sync_test(cx))),
            |val, _, cx| {
                let val_str = val.to_string();
                if val_str.is_empty() {
                    settings_entity(cx)
                        .update(cx, |state, cx| state.set_webdav_password_stored(false, cx));
                    smol::spawn(smol::unblock(move || {
                        if let Err(e) = velowork_workspace::secure_storage::delete_webdav_password() {
                            log::warn!("[webdav] 清除已保存的 WebDAV 密码失败: {}", e);
                        }
                    }))
                    .detach();
                } else {
                    settings_entity(cx)
                        .update(cx, |state, cx| state.set_webdav_password_stored(true, cx));
                    smol::spawn(smol::unblock(move || {
                        if let Err(e) = velowork_workspace::secure_storage::store_webdav_password(&val_str) {
                            log::warn!("[webdav] 保存 WebDAV 密码失败: {}", e);
                        }
                    }))
                    .detach();
                }
            },
        );

        let sync_remote_path_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("/velowork/");
            if !s.sync.webdav.remote_path.is_empty() {
                state.set_value(s.sync.webdav.remote_path.clone(), cx);
            }
            state
        });
        bind_debounced_input(
            &sync_remote_path_input,
            cx,
            Some(Rc::new(|this, cx| this.reset_sync_test(cx))),
            |val, _, cx| {
                settings_entity(cx).update(cx, |state, cx| state.set_webdav_remote_path(val.to_string(), cx));
            },
        );

        // S3 inputs
        let sync_s3_endpoint_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("https://s3.amazonaws.com");
            if !s.sync.s3.endpoint.is_empty() {
                state.set_value(s.sync.s3.endpoint.clone(), cx);
            }
            state
        });
        bind_debounced_input(
            &sync_s3_endpoint_input,
            cx,
            Some(Rc::new(|this, cx| this.reset_sync_test(cx))),
            |val, _, cx| {
                settings_entity(cx).update(cx, |state, cx| state.set_s3_endpoint(val.to_string(), cx));
            },
        );

        let sync_s3_bucket_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("my-velowork-bucket");
            if !s.sync.s3.bucket.is_empty() {
                state.set_value(s.sync.s3.bucket.clone(), cx);
            }
            state
        });
        bind_debounced_input(
            &sync_s3_bucket_input,
            cx,
            Some(Rc::new(|this, cx| this.reset_sync_test(cx))),
            |val, _, cx| {
                settings_entity(cx).update(cx, |state, cx| state.set_s3_bucket(val.to_string(), cx));
            },
        );

        let sync_s3_region_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("us-east-1 (or auto)");
            if !s.sync.s3.region.is_empty() {
                state.set_value(s.sync.s3.region.clone(), cx);
            }
            state
        });
        bind_debounced_input(
            &sync_s3_region_input,
            cx,
            Some(Rc::new(|this, cx| this.reset_sync_test(cx))),
            |val, _, cx| {
                settings_entity(cx).update(cx, |state, cx| state.set_s3_region(val.to_string(), cx));
            },
        );

        let sync_s3_access_key_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("AKIAIOSFODNN7EXAMPLE");
            if !s.sync.s3.access_key_id.is_empty() {
                state.set_value(s.sync.s3.access_key_id.clone(), cx);
            }
            state
        });
        bind_debounced_input(
            &sync_s3_access_key_input,
            cx,
            Some(Rc::new(|this, cx| this.reset_sync_test(cx))),
            |val, _, cx| {
                settings_entity(cx).update(cx, |state, cx| state.set_s3_access_key_id(val.to_string(), cx));
            },
        );

        let initial_s3_secret_key =
            velowork_workspace::secure_storage::load_s3_secret_key().unwrap_or_default();
        let sync_s3_secret_key_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Secret Access Key")
                .masked(true)
                .default_value(initial_s3_secret_key)
        });
        bind_debounced_input(
            &sync_s3_secret_key_input,
            cx,
            Some(Rc::new(|this, cx| this.reset_sync_test(cx))),
            |val, _, cx| {
                let val_str = val.to_string();
                if val_str.is_empty() {
                    settings_entity(cx)
                        .update(cx, |state, cx| state.set_s3_secret_key_stored(false, cx));
                    smol::spawn(smol::unblock(move || {
                        if let Err(e) = velowork_workspace::secure_storage::delete_s3_secret_key() {
                            log::warn!("[s3] 清除已保存的 S3 Secret Key 失败: {}", e);
                        }
                    }))
                    .detach();
                } else {
                    settings_entity(cx)
                        .update(cx, |state, cx| state.set_s3_secret_key_stored(true, cx));
                    smol::spawn(smol::unblock(move || {
                        if let Err(e) = velowork_workspace::secure_storage::store_s3_secret_key(&val_str) {
                            log::warn!("[s3] 保存 S3 Secret Key 失败: {}", e);
                        }
                    }))
                    .detach();
                }
            },
        );

        let sync_s3_prefix_input = cx.new(|cx| {
            let mut state = InputState::new(cx).placeholder("velowork (optional prefix)");
            if !s.sync.s3.prefix.is_empty() {
                state.set_value(s.sync.s3.prefix.clone(), cx);
            }
            state
        });
        bind_debounced_input(
            &sync_s3_prefix_input,
            cx,
            Some(Rc::new(|this, cx| this.reset_sync_test(cx))),
            |val, _, cx| {
                settings_entity(cx).update(cx, |state, cx| state.set_s3_prefix(val.to_string(), cx));
            },
        );

        let cur_sync_provider = if s.sync.enabled {
            Some(s.sync.provider)
        } else {
            None
        };
        let sync_provider_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new(None, i18n!(cx, "common.state.none")),
                    SelectOption::new(Some(SyncProvider::WebDav), "WebDAV"),
                    SelectOption::new(Some(SyncProvider::S3), i18n!(cx, "settings.sync.provider.s3")),
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

        let sync_scope_card = cx.new(render_sync::SyncScopeCardView::new);

        // 左侧导航搜索框（定位器：输入时展开并滚动到首个匹配分类）
        let nav_search_input =
            cx.new(|cx| InputState::new(cx).placeholder(i18n!(cx, "settings.search_placeholder")));
        let ns_entity = nav_search_input.clone();
        let search_last_change = Rc::new(Cell::new(Instant::now()));
        let search_timer_running = Rc::new(Cell::new(false));
        let slc_clone = search_last_change.clone();
        let str_clone = search_timer_running.clone();

        cx.subscribe(
            &nav_search_input,
            move |_this, _entity, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                slc_clone.set(Instant::now());
                if !str_clone.get() {
                    str_clone.set(true);
                    let last_time = slc_clone.clone();
                    let is_running = str_clone.clone();
                    let ns_weak = ns_entity.downgrade();

                    cx.spawn(async move |this, cx| {
                        loop {
                            let elapsed = last_time.get().elapsed();
                            if elapsed < std::time::Duration::from_millis(150) {
                                let remain = std::time::Duration::from_millis(150) - elapsed;
                                cx.background_executor().timer(remain).await;
                            }

                            if !is_running.get() {
                                break;
                            }

                            if last_time.get().elapsed() >= std::time::Duration::from_millis(150) {
                                is_running.set(false);
                                let _ = this.update(cx, |this, cx| {
                                    if let Some(ns) = ns_weak.upgrade() {
                                        let q = ns.read(cx).text().to_string();
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
                                                this.nav_to_category(target, cx);
                                            }
                                        }
                                        cx.notify();
                                    }
                                });
                                break;
                            }
                        }
                    })
                    .detach();
                }
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

        let cur_weight = match s.font_weight.as_str() {
            "Light" | "light" => "Light".to_string(),
            "Medium" | "medium" => "Medium".to_string(),
            "Bold" | "bold" => "Bold".to_string(),
            _ => "Normal".to_string(),
        };
        let font_weight_select = cx.new(|cx| {
            SelectState::new(cx)
                .options(vec![
                    SelectOption::new("Light".to_string(), "Light"),
                    SelectOption::new("Normal".to_string(), "Normal"),
                    SelectOption::new("Medium".to_string(), "Medium"),
                    SelectOption::new("Bold".to_string(), "Bold"),
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
                .options(available_shell_select_options(cx))
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
        bind_debounced_input(&ai_max_context_tokens_input, cx, None, |text, _, cx| {
            if let Ok(val) = text.trim().parse::<usize>() {
                if val >= 1024 {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_ai_max_context_tokens(val, cx);
                    });
                }
            }
        });

        let ai_max_history_messages_input =
            cx.new(|cx| InputState::new(cx).default_value(s.ai_max_history_messages.to_string()));
        bind_debounced_input(&ai_max_history_messages_input, cx, None, |text, _, cx| {
            if let Ok(val) = text.trim().parse::<usize>() {
                if val >= 2 {
                    settings_entity(cx).update(cx, |state, cx| {
                        state.set_ai_max_history_messages(val, cx);
                    });
                }
            }
        });

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
            sync_s3_endpoint_input,
            sync_s3_bucket_input,
            sync_s3_region_input,
            sync_s3_access_key_input,
            sync_s3_secret_key_input,
            sync_s3_prefix_input,
            sync_provider_select,
            sync_scope_card,
            sync_test_status: SyncTestStatus::Idle,
            sync_test_detail: None,
            sync_test_abort_handle: None,
            sync_test_task: None,
            sync_result: None,
            sync_detail: None,
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
            bound_stepper_inputs: HashSet::new(),
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
        let was_empty = Rc::new(Cell::new(true));
        cx.subscribe(&input, move |_, entity, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let is_empty = entity.read(cx).text().is_empty();
                if was_empty.get() != is_empty {
                    was_empty.set(is_empty);
                    cx.notify();
                }
            }
        })
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

    /// 将所有输入框当前的最新值立即同步到全局设置中（兜底保障，避免防抖尚未到期时关闭弹窗导致数据丢失）
    pub fn flush_inputs_to_settings(&mut self, cx: &mut Context<Self>) {
        let file_opener = self.file_opener_input.read(cx).text().to_string();
        let sftp_file_mode = self.sftp_file_mode_input.read(cx).text().to_string();
        let sftp_dir_mode = self.sftp_dir_mode_input.read(cx).text().to_string();
        let proxy_host = self.proxy_host_input.read(cx).text().to_string();
        let delimiters = self.word_selection_delimiters_input.read(cx).text().to_string();
        let ignored_cmds: Vec<String> = self
            .command_history_ignored_commands_input
            .read(cx)
            .text()
            .split(|c| c == ',' || c == '，')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let webdav_url = self.sync_server_url_input.read(cx).text().to_string();
        let webdav_user = self.sync_username_input.read(cx).text().to_string();
        let webdav_pw = self.sync_password_input.read(cx).text().to_string();
        let webdav_path = self.sync_remote_path_input.read(cx).text().to_string();
        let s3_endpoint = self.sync_s3_endpoint_input.read(cx).text().to_string();
        let s3_bucket = self.sync_s3_bucket_input.read(cx).text().to_string();
        let s3_region = self.sync_s3_region_input.read(cx).text().to_string();
        let s3_ak = self.sync_s3_access_key_input.read(cx).text().to_string();
        let s3_secret = self.sync_s3_secret_key_input.read(cx).text().to_string();
        let s3_prefix = self.sync_s3_prefix_input.read(cx).text().to_string();

        let ai_ctx_text = self.ai_max_context_tokens_input.read(cx).text().to_string();
        let ai_ctx = ai_ctx_text.trim().parse::<usize>().ok().filter(|v| *v >= 1024);

        let ai_history_text = self.ai_max_history_messages_input.read(cx).text().to_string();
        let ai_history = ai_history_text.trim().parse::<usize>().ok().filter(|v| *v >= 2);

        settings_entity(cx).update(cx, |state, cx| {
            state.set_file_opener(file_opener, cx);
            state.set_sftp_default_file_mode(sftp_file_mode, cx);
            state.set_sftp_default_dir_mode(sftp_dir_mode, cx);
            state.set_proxy_host(proxy_host, cx);
            state.set_word_selection_delimiters(delimiters, cx);
            state.set_command_history_ignored_commands(ignored_cmds, cx);
            state.set_webdav_server_url(webdav_url, cx);
            state.set_webdav_username(webdav_user, cx);
            state.set_webdav_remote_path(webdav_path, cx);
            state.set_s3_endpoint(s3_endpoint, cx);
            state.set_s3_bucket(s3_bucket, cx);
            state.set_s3_region(s3_region, cx);
            state.set_s3_access_key_id(s3_ak, cx);
            state.set_s3_prefix(s3_prefix, cx);
            if let Some(ctx_tokens) = ai_ctx {
                state.set_ai_max_context_tokens(ctx_tokens, cx);
            }
            if let Some(hist) = ai_history {
                state.set_ai_max_history_messages(hist, cx);
            }
        });

        if !s3_secret.is_empty() {
            let s3_sec = s3_secret.clone();
            smol::spawn(smol::unblock(move || {
                let _ = velowork_workspace::secure_storage::store_s3_secret_key(&s3_sec);
            }))
            .detach();
        }
        if !webdav_pw.is_empty() {
            let pw = webdav_pw.clone();
            smol::spawn(smol::unblock(move || {
                let _ = velowork_workspace::secure_storage::store_webdav_password(&pw);
            }))
            .detach();
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.flush_inputs_to_settings(cx);
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

    /// 右侧主内容区（Zed 模式：单分类按需独立渲染）。
    /// 仅挂载当前选中的单个分类卡片与内容体，将 GPUI 遍历 DOM 树节点体量骤降 90%，
    /// 彻底切断全量长列表对输入框打字造成的 Layout/Prepaint 级联帧积压。
    fn render_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 检查是否有 Tab 带来的待滚动焦点句柄（延迟滚动定位）
        if let Some(fh) = self.pending_scroll_focus_handle.take() {
            self.scroll_handle_into_view(&fh, cx);
        }

        let active_cat = self.active_category.clone();
        let title = self.category_title(&active_cat, cx);
        let t = theme(cx);
        let palette = velowork_ui::SemanticPalette::from_context(cx);

        let header = div()
            .id("settings-active-card-header")
            .flex()
            .items_center()
            .gap(SPACE_MD)
            .pb(SPACE_MD)
            .border_b_1()
            .border_color(palette.border_subtle)
            .child(
                active_cat
                    .icon()
                    .size(crate::ui::tokens::ui_icon_std_ts(cx))
                    .text_color(palette.text_secondary),
            )
            .child(
                div()
                    .text_size(ui_text_lg(cx))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(t.text_primary))
                    .child(title),
            );

        let body = self.render_category_body(&active_cat, window, cx);

        let card = div()
            .id("settings-active-card")
            .w_full()
            .bg(palette.surface_card)
            .border_1()
            .border_color(palette.border_subtle)
            .rounded(RADIUS_CARD)
            .px(SPACE_LG)
            .py(SPACE_MD)
            .flex()
            .flex_col()
            .gap(SPACE_MD)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this: &mut Self, _event: &MouseDownEvent, window, cx| {
                    if let Some(focused) = window.focused(cx) {
                        if focused != this.focus_handle {
                            window.focus(&this.focus_handle, cx);
                        }
                    }
                    this.flush_inputs_to_settings(cx);
                }),
            )
            .child(header)
            .child(div().w_full().child(body));

        let right = div()
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
            .flex_1()
            .child(card)
            .child(
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
            let guard = settings_entity(cx).read(cx);
            let enabled = &guard.settings.enabled_extensions;
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

    /// 导航点击：高亮 + 展开 + 切换当前活动分类并重置滚动位置到顶部。
    pub fn nav_to_category(&mut self, cat: SettingsCategory, cx: &mut Context<Self>) {
        self.flush_inputs_to_settings(cx);
        self.active_category = cat.clone();
        self.expanded_categories.insert(cat);
        self.scroll_handle.set_offset(point(px(0.0), px(0.0)));
        self.close_all_dropdowns();
        cx.notify();
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
        let settings_state = settings_entity(cx);
        let s = &settings_state.read(cx).settings;

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
                if s.sync.enabled {
                    match s.sync.provider {
                        SyncProvider::WebDav => {
                            handles.push(self.sync_server_url_input.read(cx).focus_handle(cx));
                            handles.push(self.sync_username_input.read(cx).focus_handle(cx));
                            handles.push(self.sync_password_input.read(cx).focus_handle(cx));
                            handles.push(self.sync_remote_path_input.read(cx).focus_handle(cx));
                        }
                        SyncProvider::S3 => {
                            handles.push(self.sync_s3_endpoint_input.read(cx).focus_handle(cx));
                            handles.push(self.sync_s3_bucket_input.read(cx).focus_handle(cx));
                            handles.push(self.sync_s3_region_input.read(cx).focus_handle(cx));
                            handles.push(self.sync_s3_access_key_input.read(cx).focus_handle(cx));
                            handles.push(self.sync_s3_secret_key_input.read(cx).focus_handle(cx));
                            handles.push(self.sync_s3_prefix_input.read(cx).focus_handle(cx));
                            handles.push(self.get_or_create_toggle_focus_handle("sync-s3-path-style", cx));
                        }
                    }
                    handles.push(self.get_or_create_button_focus_handle("sync-test-conn-btn", cx));
                    handles.extend(self.sync_scope_card.read(cx).focus_handles());
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

    /// 线性有序焦点句柄列表：搜索框 -> 左侧导航 -> 当前激活分类配置项
    pub fn all_focus_handles(&self, cx: &App) -> Vec<FocusHandle> {
        let mut handles = Vec::new();
        handles.push(self.nav_search_input.read(cx).focus_handle(cx));
        handles.push(self.nav_focus_handle.clone());
        handles.extend(self.category_focus_handles(&self.active_category, cx));
        handles
    }

    /// 根据焦点句柄查找所属分类（在当前激活分类中查找）
    pub fn category_for_focus_handle(&self, handle: &FocusHandle, cx: &App) -> Option<SettingsCategory> {
        let handles = self.category_focus_handles(&self.active_category, cx);
        if handles.contains(handle) {
            Some(self.active_category.clone())
        } else {
            None
        }
    }

    /// 计算指定分类中某一焦点控件在该卡片内部的相对垂直范围 [top, bottom]
    pub fn handle_y_range_in_category(
        &self,
        cat: &SettingsCategory,
        handle: &FocusHandle,
        cx: &App,
    ) -> (f32, f32) {
        let handles = self.category_focus_handles(cat, cx);
        let row_idx = handles.iter().position(|h| h == handle).unwrap_or(0);

        let header_offset = 48.0;
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
            _ => header_offset + row_idx as f32 * row_h,
        };

        (in_card_top, in_card_top + 40.0)
    }

    /// 视口自适应平滑滚动（Scroll Into View with 40px Safe Viewport Padding）：
    /// 当 Tab 或交互导致某控件获焦时，确保其完整展示在视口内；若已在视口舒适区域内则绝不触发多余滚动。
    pub fn scroll_handle_into_view(&mut self, handle: &FocusHandle, cx: &App) {
        let Some(cat) = self.category_for_focus_handle(handle, cx) else {
            return;
        };

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
        let item_top = in_card_top;
        let item_bottom = in_card_bottom;

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

    /// 检查当前是否存在任何打开的子模态弹窗
    pub(super) fn has_open_modal(&self) -> bool {
        self.ai_add_model_dialog_open
            || self.search_add_dialog_open
            || self.search_edit_id.is_some()
            || self.show_data_root_confirm_modal
            || self.active_color_scheme_dialog.is_some()
    }

    /// 关闭当前最顶层的活动子模态弹窗，消费该动作并阻止向外冒泡。
    /// 若成功关闭了子弹窗则返回 `true`；若当前无子弹窗则返回 `false`。
    pub(super) fn close_active_modal(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.ai_add_model_dialog_open {
            self.close_add_model_dialog(window, cx);
            return true;
        }
        if self.search_add_dialog_open || self.search_edit_id.is_some() {
            self.close_search_dialog(window, cx);
            return true;
        }
        if self.show_data_root_confirm_modal {
            self.show_data_root_confirm_modal = false;
            cx.notify();
            return true;
        }
        if let Some(modal) = self.active_color_scheme_dialog.as_ref() {
            modal.update(cx, |modal, cx| modal.request_close(cx));
            return true;
        }
        false
    }
}

pub enum SettingsPanelEvent {
    Close,
}

impl EventEmitter<SettingsPanelEvent> for SettingsPanel {}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this: &mut Self, _event: &MouseDownEvent, window, cx| {
                    if let Some(focused) = window.focused(cx) {
                        if focused != this.focus_handle {
                            window.focus(&this.focus_handle, cx);
                        }
                    }
                    this.flush_inputs_to_settings(cx);
                }),
            )
            .on_key_down(cx.listener(|this: &mut Self, event: &KeyDownEvent, window, cx| {
                // Tab 键流转与视口自适应平滑滚动联动（40px 呼吸边距，对齐新建会话弹窗规范）
                if event.keystroke.key == "\t" || event.keystroke.key == "tab" {
                    cx.stop_propagation();
                    this.flush_inputs_to_settings(cx);
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
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                if this.has_open_dropdown() {
                    this.close_all_dropdowns();
                    cx.notify();
                } else if this.has_open_modal() {
                    this.close_active_modal(Some(window), cx);
                    cx.stop_propagation();
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

impl Drop for SettingsPanel {
    fn drop(&mut self) {
        if let Some(handle) = self.sync_test_abort_handle.take() {
            handle.abort();
        }
    }
}

