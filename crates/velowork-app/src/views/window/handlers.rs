use crate::action_dispatch::ActionDispatcher;
use crate::settings::GlobalSettings;
use crate::views::overlays::overlay_manager::{OverlayManager, OverlayManagerEvent};
use crate::workspace::persistence;
use crate::workspace::requests::{
    FolderOverlay, FolderOverlayKind, OverlayRequest, ProjectOverlay, ProjectOverlayKind,
    SidebarRequest,
};
use crate::workspace::state::{GlobalWorkspace, LayoutNode, Workspace};
use gpui::*;

use velowork_core::api::ActionRequest;

use super::WindowView;

impl WindowView {
    /// Build an ActionDispatcher for the given project.
    /// Returns Remote variant if the project is a remote project,
    /// otherwise returns Local variant.
    fn dispatcher_for_project(&self, project_id: &str, cx: &Context<Self>) -> ActionDispatcher {
        let backend = Some(self.backend.clone());
        crate::action_dispatch::dispatcher_for_project(
            project_id,
            self.window_id,
            &self.workspace,
            &self.focus_manager,
            &backend,
            &self.terminals,
            Some(self.overlay_manager.clone()),
            cx,
        ).unwrap_or_else(|| ActionDispatcher::Local {
            workspace: self.workspace.clone(),
            focus_manager: self.focus_manager.clone(),
            backend: self.backend.clone(),
            terminals: self.terminals.clone(),
            window_id: self.window_id,
            overlay_manager: Some(self.overlay_manager.clone()),
        })
    }

    /// Resolve the focused terminal_id from this window's focus_manager and the
    /// project layout. Returns (project_id, terminal_id) or None if no terminal
    /// is focused or the path doesn't lead to a Terminal node with an assigned id.
    pub(super) fn focused_terminal_id(&self, cx: &Context<Self>) -> Option<(String, String)> {
        let state = self.focus_manager.read(cx).focused_terminal_state()?;
        let ws = self.workspace.read(cx);
        let project = ws.project(&state.project_id)?;
        let layout = project.layout.as_ref()?;
        let node = layout.get_at_path(&state.layout_path)?;
        if let LayoutNode::Terminal { terminal_id: Some(id), .. } = node {
            Some((state.project_id, id.clone()))
        } else {
            None
        }
    }

    /// Open or toggle inline AI popover for the focused terminal.
    pub(super) fn handle_terminal_inline_ai(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = crate::settings::settings_entity(cx).read(cx).settings.clone();
        if !settings.ai_enabled {
            return;
        }

        let Some((project_id, terminal_id)) = self.focused_terminal_id(cx) else {
            return;
        };

        let mut selection = String::new();
        {
            let terminals = self.terminals.lock();
            if let Some(terminal) = terminals.get(terminal_id.as_str()) {
                selection = terminal.get_selected_text().unwrap_or_default();
            }
        }

        let vp = window.viewport_size();
        let pos = point(
            (vp.width - px(460.0)).max(px(20.0)) / 2.0,
            (vp.height - px(360.0)).max(px(40.0)) / 3.0,
        );

        self.overlay_manager.update(cx, |om, cx| {
            om.show_terminal_ai_popover(
                terminal_id,
                project_id,
                pos,
                selection,
                cx,
            );
        });
        cx.notify();
    }

    /// Paste a "Send to Terminal" payload into the currently focused terminal.
    ///
    /// Resolves the focused terminal's working directory (OSC 7-reported, else
    /// the PTY's initial cwd) and formats the payload relative to it before
    /// sending. Always wrapped in bracketed-paste sequences — see
    /// `Terminal::send_paste_force_bracketed` for rationale on why we don't
    /// trust the tracked DECSET 2004 mode flag. Toasts a warning if no
    /// terminal is focused.
    fn send_payload_to_active_terminal(
        &self,
        payload: velowork_core::send_payload::SendPayload,
        cx: &mut Context<Self>,
    ) {
        let Some((_project_id, terminal_id)) = self.focused_terminal_id(cx) else {
            velowork_workspace::toast::ToastManager::warning(
                "No active terminal to send selection to",
                cx,
            );
            return;
        };
        let terminals = self.terminals.lock();
        if let Some(terminal) = terminals.get(&terminal_id) {
            let cwd = terminal.current_cwd();
            let cwd_path = std::path::Path::new(&cwd);
            let text = payload.format(Some(cwd_path));
            if !text.is_empty() {
                terminal.send_paste_force_bracketed(&text);
            }
        }
    }
}

impl WindowView {
    /// Handle events from the OverlayManager that require WindowView access.
    /// Handle a click on a toast action button (soft-close undo / close-now).
    pub(super) fn handle_toast_action(
        &mut self,
        _: Entity<crate::views::panels::toast::ToastOverlay>,
        event: &crate::views::panels::toast::ToastActionEvent,
        cx: &mut Context<Self>,
    ) {
        use crate::soft_close::{decode_action, KILL_PREFIX, UNDO_PREFIX};
        use crate::workspace::toast::ToastManager;

        if let Some((_project_id, terminal_id)) = decode_action(&event.action_id, UNDO_PREFIX) {
            // The PTY is only restorable if it's still in the registry — if the
            // shell exited on its own during the grace window there's nothing to
            // bring back, and `undo_soft_close` just drops the pending record.
            let alive = self.terminals.lock().contains_key(terminal_id.as_str());
            let ws = self.workspace.clone();
            let fm = self.focus_manager.clone();
            fm.update(cx, |fm, cx| {
                ws.update(cx, |ws, cx| {
                    ws.undo_soft_close(fm, &terminal_id, alive, cx);
                });
                cx.notify();
            });
            ToastManager::dismiss(&event.toast_id, cx);
        } else if let Some((_project_id, terminal_id)) =
            decode_action(&event.action_id, KILL_PREFIX)
        {
            let ws = self.workspace.clone();
            ws.update(cx, |ws, cx| {
                ws.finalize_soft_close(&terminal_id, cx);
            });
            ToastManager::dismiss(&event.toast_id, cx);
        } else if event.action_id == "webdav_restore_confirm" {
            // 从云端恢复：拉取远端配置覆盖本地并重新加载运行中的设置。
            crate::views::overlays::settings::settings_panel::render_sync::restore_from_cloud(cx);
            ToastManager::dismiss(&event.toast_id, cx);
        } else if event.action_id == "webdav_force_push_confirm" {
            // 强制覆盖云端：以本地配置为准强制推送到云端。
            crate::views::overlays::settings::settings_panel::render_sync::force_push_to_cloud(cx);
            ToastManager::dismiss(&event.toast_id, cx);
        } else if event.action_id == "open_sync_settings" {
            self.overlay_manager.update(cx, |om, cx| {
                om.open_settings_panel_to(
                    Some(crate::views::overlays::settings::settings_panel::SettingsCategory::Sync),
                    cx,
                );
            });
            ToastManager::dismiss(&event.toast_id, cx);
        } else if event.action_id == "restart_app" {
            ToastManager::dismiss(&event.toast_id, cx);
            velowork_updater::restart_app(cx);
        }
    }

    pub(super) fn handle_overlay_manager_event(
        &mut self,
        _: Entity<OverlayManager>,
        event: &OverlayManagerEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            OverlayManagerEvent::ShellSelected { shell_type, project_id, terminal_id } => {
                self.switch_terminal_shell(project_id, terminal_id, shell_type.clone(), cx);
            }
            OverlayManagerEvent::AddTerminal { project_id } => {
                let dispatcher = self.dispatcher_for_project(project_id, cx);
                dispatcher.dispatch(ActionRequest::CreateTerminal {
                    project_id: project_id.clone(),
                }, cx);
            }
            OverlayManagerEvent::RenameProject { project_id, project_name } => {
                self.request_broker.update(cx, |broker, cx| {
                    broker.push_sidebar_request(SidebarRequest::RenameProject {
                        project_id: project_id.clone(),
                        project_name: project_name.clone(),
                    }, cx);
                });
            }
            OverlayManagerEvent::RenameDirectory { project_id, project_path } => {
                self.overlay_manager.update(cx, |om, cx| {
                    om.show_rename_directory_dialog(project_id.clone(), project_path.clone(), cx);
                });
            }
            OverlayManagerEvent::DeleteProject { project_id } => {
                let workspace = self.workspace.clone();
                let pid = project_id.clone();
                self.focus_manager.update(cx, |fm, cx| {
                    workspace.update(cx, |ws, cx| {
                        ws.delete_project(fm, &pid, cx);
                    });
                    cx.notify();
                });
            }
            OverlayManagerEvent::ProjectColorChanged { .. } => {
                // Handled internally by active project panels / new design
            }
            OverlayManagerEvent::FocusParent { .. } => {
                // Parent project focus is handled by the focus manager; no
                // extra work needed here.
            }
            OverlayManagerEvent::FocusProject(project_id) => {
                let workspace = self.workspace.clone();
                let pid = project_id.clone();
                self.focus_manager.update(cx, |fm, cx| {
                    workspace.update(cx, |ws, cx| {
                        ws.set_focused_project(fm, Some(pid), self.window_id, cx);
                    });
                    cx.notify();
                });
            }
            OverlayManagerEvent::JumpToProject(project_id) => {
                // Defer the cross-window work to Velowork, which owns every
                // window's view + OS handle. `origin` is this window so it is
                // preferred when the project is open in more than one place.
                cx.emit(super::WindowViewEvent::JumpToProject {
                    origin: self.window_id,
                    project_id: project_id.clone(),
                });
            }
            OverlayManagerEvent::ToggleProjectVisibility(project_id) => {
                let window_id = self.window_id;
                let workspace = self.workspace.clone();
                let project_id = project_id.clone();
                self.focus_manager.update(cx, |fm, cx| {
                    workspace.update(cx, |ws, cx| {
                        ws.toggle_project_overview_visibility(fm, window_id, &project_id, cx);
                    });
                });
            }
            OverlayManagerEvent::TerminalCopy { terminal_id } => {
                let terminals = self.terminals.lock();
                if let Some(terminal) = terminals.get(terminal_id)
                    && let Some(text) = terminal.get_selected_text() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
            }
            OverlayManagerEvent::TerminalPaste { terminal_id } => {
                let text = cx.read_from_clipboard()
                    .and_then(|item| item.text().map(|t| t.to_string()));
                if let Some(text) = text {
                    let terminals = self.terminals.lock();
                    if let Some(terminal) = terminals.get(terminal_id) {
                        terminal.send_paste(&text);
                    }
                }
            }
            OverlayManagerEvent::TerminalClear { terminal_id } => {
                let terminals = self.terminals.lock();
                if let Some(terminal) = terminals.get(terminal_id) {
                    terminal.send_bytes(b"\x0c");
                    let mut registry = crate::views::window::content_pane_registry().lock();
                    if let Some(weaks) = registry.get_mut(terminal_id) {
                        crate::views::window::notify_pane_weaks(weaks, cx);
                    }
                }
            }
            OverlayManagerEvent::TerminalExportSelected { terminal_id } => {
                let text_opt = {
                    let terminals = self.terminals.lock();
                    terminals.get(terminal_id).and_then(|t| t.get_selected_text())
                };
                if let Some(text) = text_opt {
                    let temp_path = std::env::temp_dir().join(format!("velowork-selection-{}.txt", uuid::Uuid::new_v4().to_string().split('-').next().unwrap()));
                    if std::fs::write(&temp_path, text).is_ok() {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(temp_path.to_string_lossy().to_string()));
                        crate::views::panels::toast::ToastManager::info(
                            format!("已导出选中内容至: {}", temp_path.to_string_lossy()),
                            cx
                        );
                    } else {
                        crate::views::panels::toast::ToastManager::warning("导出选中内容失败", cx);
                    }
                }
            }
            OverlayManagerEvent::TerminalExportAll { terminal_id } => {
                if let Some(path) = self.backend.capture_buffer(terminal_id) {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.to_string_lossy().to_string()));
                    crate::views::panels::toast::ToastManager::info(
                        format!("已导出全部缓冲至: {}", path.to_string_lossy()),
                        cx
                    );
                } else {
                    crate::views::panels::toast::ToastManager::warning("导出缓冲失败", cx);
                }
            }
            OverlayManagerEvent::ShowLogRecordDialog { terminal_id } => {
                self.overlay_manager.update(cx, |om, cx| {
                    om.show_log_record_dialog(terminal_id.clone(), cx);
                });
            }
            OverlayManagerEvent::TerminalLogStart { terminal_id, filename, append_mode, auto_save_interval } => {
                let rec_dir = velowork_core::profiles::current().recordings_dir();
                let _ = std::fs::create_dir_all(&rec_dir);
                let path = rec_dir.join(filename);
                let terminals = self.terminals.lock();
                if let Some(terminal) = terminals.get(terminal_id) {
                    if terminal.start_log_recording(path.clone(), *append_mode).is_ok() {
                        crate::views::panels::toast::ToastManager::info(
                            format!("开始录制日志，保存至: {}", path.to_string_lossy()),
                            cx
                        );
                        let mut registry = crate::views::window::content_pane_registry().lock();
                        if let Some(weaks) = registry.get_mut(terminal_id) {
                            crate::views::window::notify_pane_weaks(weaks, cx);
                        }

                        // Spawn periodic flush task if interval > 0
                        if *auto_save_interval > 0 {
                            let terminal_id = terminal_id.clone();
                            let interval = *auto_save_interval;
                            cx.spawn(async move |this: WeakEntity<WindowView>, cx| {
                                loop {
                                    smol::Timer::after(std::time::Duration::from_secs(interval as u64)).await;
                                    let active = this.update(cx, |this, _cx| {
                                        let terminals = this.terminals.lock();
                                        if let Some(t) = terminals.get(&terminal_id) {
                                            if t.is_log_recording() && !t.is_log_recording_paused() {
                                                t.flush_log();
                                                true
                                            } else {
                                                false
                                            }
                                        } else {
                                            false
                                        }
                                    });
                                    if active.is_err() || !active.unwrap() {
                                        break;
                                    }
                                }
                            })
                            .detach();
                        }
                    } else {
                        crate::views::panels::toast::ToastManager::warning("开始录制日志失败", cx);
                    }
                }
            }
            OverlayManagerEvent::TerminalLogPause { terminal_id } => {
                let terminals = self.terminals.lock();
                if let Some(terminal) = terminals.get(terminal_id) {
                    terminal.pause_log_recording();
                    crate::views::panels::toast::ToastManager::info("已暂停日志录制", cx);
                    let mut registry = crate::views::window::content_pane_registry().lock();
                    if let Some(weaks) = registry.get_mut(terminal_id) {
                        crate::views::window::notify_pane_weaks(weaks, cx);
                    }
                }
            }
            OverlayManagerEvent::TerminalLogResume { terminal_id } => {
                let terminals = self.terminals.lock();
                if let Some(terminal) = terminals.get(terminal_id) {
                    terminal.resume_log_recording();
                    crate::views::panels::toast::ToastManager::info("已继续日志录制", cx);
                    let mut registry = crate::views::window::content_pane_registry().lock();
                    if let Some(weaks) = registry.get_mut(terminal_id) {
                        crate::views::window::notify_pane_weaks(weaks, cx);
                    }
                }
            }
            OverlayManagerEvent::TerminalLogStop { terminal_id } => {
                log::info!("[TerminalLogStop] Stop request received for terminal_id: {}", terminal_id);
                let (has_terminal, is_recording, path_opt) = {
                    let terminals = self.terminals.lock();
                    if let Some(t) = terminals.get(terminal_id) {
                        let is_rec = t.is_log_recording();
                        let is_p = t.is_log_recording_paused();
                        log::info!("[TerminalLogStop] Terminal found in registry. is_recording: {}, is_paused: {}", is_rec, is_p);
                        (true, is_rec, t.stop_log_recording())
                    } else {
                        log::warn!("[TerminalLogStop] Terminal {} not found in registry!", terminal_id);
                        (false, false, None)
                    }
                };
                if let Some(path) = path_opt {
                    log::info!("[TerminalLogStop] Stop log recording succeeded, path: {}", path.to_string_lossy());
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.to_string_lossy().to_string()));
                    
                    let mut registry = crate::views::window::content_pane_registry().lock();
                    if let Some(weaks) = registry.get_mut(terminal_id) {
                        crate::views::window::notify_pane_weaks(weaks, cx);
                    }

                    self.overlay_manager.update(cx, |om, cx| {
                        om.show_log_saved_dialog(terminal_id.clone(), path.clone(), cx);
                    });
                } else {
                    log::warn!("[TerminalLogStop] Stop log recording failed. has_terminal: {}, is_recording: {}", has_terminal, is_recording);
                    crate::views::panels::toast::ToastManager::warning("停止日志录制失败或未在录制中", cx);
                }
            }
            OverlayManagerEvent::ShowLogSavedDialog { terminal_id, path } => {
                self.overlay_manager.update(cx, |om, cx| {
                    om.show_log_saved_dialog(terminal_id.clone(), path.clone(), cx);
                });
            }
            OverlayManagerEvent::TerminalLogOpenFileWithPath { path } => {
                let file_url = format!("file://{}", path.to_string_lossy());
                crate::views::layout::terminal_pane::url_detector::UrlDetector::open_url(&file_url);
            }
            OverlayManagerEvent::TerminalLogOpenFolderWithPath { path } => {
                if let Some(parent) = path.parent() {
                    let folder_url = format!("file://{}", parent.to_string_lossy());
                    crate::views::layout::terminal_pane::url_detector::UrlDetector::open_url(&folder_url);
                }
            }
            OverlayManagerEvent::TerminalLogOpenFile { terminal_id } => {
                let path_opt = {
                    let terminals = self.terminals.lock();
                    terminals.get(terminal_id).and_then(|t| t.get_log_recording_path())
                };
                if let Some(path) = path_opt {
                    let file_url = format!("file://{}", path.to_string_lossy());
                    crate::views::layout::terminal_pane::url_detector::UrlDetector::open_url(&file_url);
                }
            }
            OverlayManagerEvent::TerminalLogOpenFolder { terminal_id } => {
                let path_opt = {
                    let terminals = self.terminals.lock();
                    terminals.get(terminal_id).and_then(|t| t.get_log_recording_path())
                };
                if let Some(path) = path_opt {
                    if let Some(parent) = path.parent() {
                        let folder_url = format!("file://{}", parent.to_string_lossy());
                        crate::views::layout::terminal_pane::url_detector::UrlDetector::open_url(&folder_url);
                    }
                }
            }
            OverlayManagerEvent::TerminalAIInterpret { terminal_id: _, text } => {
                if !text.is_empty() {
                    // Defer the panel activation (needs a Window) to the next render pass.
                    self.pending_ai_interpret = Some(text.clone());
                    cx.notify();
                }
            }
            OverlayManagerEvent::TerminalAiInline(inline_ev) => match inline_ev {
                crate::views::overlays::terminal_ai_inline::TerminalAiInlineEvent::InsertToTerminal { terminal_id, command } => {
                    let terminals = self.terminals.lock();
                    if let Some(terminal) = terminals.get(terminal_id.as_str()) {
                        terminal.send_paste(&command);
                    }
                }
                crate::views::overlays::terminal_ai_inline::TerminalAiInlineEvent::RunInTerminal { terminal_id, command } => {
                    let terminals = self.terminals.lock();
                    if let Some(terminal) = terminals.get(terminal_id.as_str()) {
                        let cmd_with_nl = if command.ends_with('\n') {
                            command.clone()
                        } else {
                            format!("{}\n", command)
                        };
                        terminal.send_bytes(cmd_with_nl.as_bytes());
                    }
                }
                crate::views::overlays::terminal_ai_inline::TerminalAiInlineEvent::ContinueInSidePanel { quote: _, reply: _ } => {
                    self.pending_ai_open = true;
                    cx.notify();
                }
                crate::views::overlays::terminal_ai_inline::TerminalAiInlineEvent::AppendConversation { project_id, user_message, quote, assistant_reply } => {
                    if let Some(ai) = self.find_ai_assistant_panel(cx) {
                        ai.update(cx, |ai, cx| {
                            ai.append_external_turn(&project_id, &user_message, quote.as_deref(), &assistant_reply, cx);
                        });
                    } else if let Some(db) = velowork_core::storage::database() {
                        let repo = velowork_workspace::repositories::AiRepository::new(db);
                        let conv_id = format!("conv_{}", project_id);
                        let now_iso = chrono::Utc::now().to_rfc3339();
                        if repo.get_conversation(&conv_id).ok().flatten().is_none() {
                            let conv_row = velowork_workspace::repositories::AiConversationRow {
                                id: conv_id.clone(),
                                profile_id: Some("default".into()),
                                project_id: Some(project_id.clone()),
                                title: Some(user_message.clone()),
                                provider_id: None,
                                model: None,
                                status: "active".into(),
                                context_mode: "session".into(),
                                created_at: now_iso.clone(),
                                updated_at: now_iso.clone(),
                                revision: 1,
                                device_id: String::new(),
                            };
                            let _ = repo.save_conversation(&conv_row);
                        }
                        let count = repo.list_messages(&conv_id).map(|m| m.len()).unwrap_or(0);
                        let u_meta = serde_json::json!({
                            "quote": quote,
                        });
                        let user_row = velowork_workspace::repositories::AiMessageRow {
                            id: format!("{}_{}", conv_id, count),
                            conversation_id: conv_id.clone(),
                            role: "user".into(),
                            content: user_message.clone(),
                            token_count: None,
                            metadata: u_meta.to_string(),
                            created_at: now_iso.clone(),
                            revision: 1,
                            device_id: String::new(),
                        };
                        let _ = repo.save_message(&user_row);
                        let asst_row = velowork_workspace::repositories::AiMessageRow {
                            id: format!("{}_{}", conv_id, count + 1),
                            conversation_id: conv_id,
                            role: "assistant".into(),
                            content: assistant_reply.clone(),
                            token_count: None,
                            metadata: "{}".to_string(),
                            created_at: now_iso,
                            revision: 1,
                            device_id: String::new(),
                        };
                        let _ = repo.save_message(&asst_row);
                    }
                }
                crate::views::overlays::terminal_ai_inline::TerminalAiInlineEvent::OpenSettings => {
                    self.overlay_manager.update(cx, |om, cx| {
                        om.open_settings_panel_to(Some(crate::views::overlays::settings::settings_panel::SettingsCategory::AiAssistant), cx);
                    });
                }
                crate::views::overlays::terminal_ai_inline::TerminalAiInlineEvent::Close => {}
            },
            OverlayManagerEvent::TerminalFind { terminal_id: _ } => {
                cx.dispatch_action(&velowork_views_terminal::actions::Search);
            }
            OverlayManagerEvent::TerminalWebSearch { terminal_id } => {
                let text_opt = {
                    let terminals = self.terminals.lock();
                    terminals.get(terminal_id).and_then(|t| t.get_selected_text())
                };
                if let Some(text) = text_opt {
                    let query = percent_encode(&text);
                    let search_url = format!("https://www.google.com/search?q={}", query);
                    crate::views::layout::terminal_pane::url_detector::UrlDetector::open_url(&search_url);
                }
            }
            OverlayManagerEvent::TerminalSearchWithEngine { terminal_id: _, url } => {
                // The search URL (with %s replaced by the encoded query) is built
                // when the context menu is opened, so no terminal selection is
                // needed here. Just open it.
                crate::views::layout::terminal_pane::url_detector::UrlDetector::open_url(&url);
            }
            OverlayManagerEvent::TerminalToggleWordWrap => {
                let current_wrap = crate::settings::settings_entity(cx).read(cx).settings.wrap_mode;
                let new_wrap = if current_wrap == crate::workspace::settings::WrapMode::WindowEdge {
                    crate::workspace::settings::WrapMode::NoWrap
                } else {
                    crate::workspace::settings::WrapMode::WindowEdge
                };
                crate::settings::settings_entity(cx).update(cx, |state, cx| {
                    state.set_wrap_mode(new_wrap, cx);
                });
            }
            OverlayManagerEvent::TerminalToggleLineNumbers => {
                let current_ln = crate::settings::settings_entity(cx).read(cx).settings.show_line_numbers;
                crate::settings::settings_entity(cx).update(cx, |state, cx| {
                    state.set_show_line_numbers(!current_ln, cx);
                });
            }
            OverlayManagerEvent::TerminalClearScrollback { terminal_id } => {
                let terminals = self.terminals.lock();
                if let Some(terminal) = terminals.get(terminal_id) {
                    terminal.process_output(b"\x1b[3J");
                    terminal.scroll_to_bottom();
                    let mut registry = crate::views::window::content_pane_registry().lock();
                    if let Some(weaks) = registry.get_mut(terminal_id) {
                        crate::views::window::notify_pane_weaks(weaks, cx);
                    }
                }
            }
            OverlayManagerEvent::TerminalClearAll { terminal_id } => {
                let terminals = self.terminals.lock();
                if let Some(terminal) = terminals.get(terminal_id) {
                    terminal.clear();
                    let mut registry = crate::views::window::content_pane_registry().lock();
                    if let Some(weaks) = registry.get_mut(terminal_id) {
                        crate::views::window::notify_pane_weaks(weaks, cx);
                    }
                }
            }
            OverlayManagerEvent::TerminalSelectAll { terminal_id } => {
                let terminals = self.terminals.lock();
                if let Some(terminal) = terminals.get(terminal_id) {
                    terminal.select_all();
                }
                cx.notify();
            }
            OverlayManagerEvent::TerminalZmodemUpload { terminal_id } => {
                let terminals = self.terminals.lock();
                if let Some(terminal) = terminals.get(terminal_id) {
                    terminal.send_input("rz\r");
                }
                cx.notify();
            }
            OverlayManagerEvent::TerminalSplit { project_id, layout_path, direction } => {
                let dispatcher = self.dispatcher_for_project(project_id, cx);
                dispatcher.dispatch(ActionRequest::SplitTerminal {
                    project_id: project_id.clone(),
                    path: layout_path.clone(),
                    direction: *direction,
                }, cx);
            }
            OverlayManagerEvent::TerminalClose { project_id, terminal_id } => {
                let dispatcher = self.dispatcher_for_project(project_id, cx);
                dispatcher.dispatch(ActionRequest::CloseTerminal {
                    project_id: project_id.clone(),
                    terminal_id: terminal_id.clone(),
                }, cx);
            }
            OverlayManagerEvent::TabDuplicateSession { project_id, layout_path, tab_index } => {
                let terminal_ids = collect_tab_terminal_ids(&self.workspace, project_id, layout_path, cx);
                if let Some(target_tid) = terminal_ids.get(*tab_index).cloned() {
                    let shell_type = {
                        let ws = self.workspace.read(cx);
                        if let Some(project) = ws.project(project_id)
                            && let Some(ref layout) = project.layout
                            && let Some(path) = layout.find_terminal_path(&target_tid) {
                                ws.get_terminal_shell(project_id, &path)
                            } else {
                                None
                            }
                    };
                    let focus_manager = self.focus_manager.clone();
                    let workspace = self.workspace.clone();
                    let pid = project_id.clone();
                    let path = layout_path.clone();
                    focus_manager.update(cx, |fm, cx| {
                        workspace.update(cx, |ws, cx| {
                            ws.add_tab(fm, &pid, &path, cx);
                            if let Some(shell) = shell_type {
                                let new_path = fm.focused_terminal_state()
                                    .filter(|s| s.project_id == pid)
                                    .map(|s| s.layout_path.clone());
                                if let Some(np) = new_path {
                                    ws.set_terminal_shell(&pid, &np, shell, cx);
                                }
                            }
                        });
                    });
                    self.spawn_terminals_for_project(project_id.clone(), cx);
                }
            }
            OverlayManagerEvent::TabDuplicateChannel { project_id, layout_path, tab_index } => {
                let terminal_ids = collect_tab_terminal_ids(&self.workspace, project_id, layout_path, cx);
                if let Some(target_tid) = terminal_ids.get(*tab_index).cloned() {
                    let orig_shell = {
                        let ws = self.workspace.read(cx);
                        if let Some(project) = ws.project(project_id)
                            && let Some(ref layout) = project.layout
                            && let Some(path) = layout.find_terminal_path(&target_tid) {
                                ws.get_terminal_shell(project_id, &path)
                            } else {
                                None
                            }
                    };

                    let shell_type = if let Some(velowork_terminal::shell_config::ShellType::Custom { path, mut args }) = orig_shell {
                        if path == "ssh" {
                            args.push("--reuse-from".to_string());
                            args.push(target_tid.clone());
                        }
                        Some(velowork_terminal::shell_config::ShellType::Custom { path, args })
                    } else {
                        orig_shell
                    };

                    let focus_manager = self.focus_manager.clone();
                    let workspace = self.workspace.clone();
                    let pid = project_id.clone();
                    let path = layout_path.clone();
                    focus_manager.update(cx, |fm, cx| {
                        workspace.update(cx, |ws, cx| {
                            ws.add_tab(fm, &pid, &path, cx);
                            if let Some(shell) = shell_type {
                                let new_path = fm.focused_terminal_state()
                                    .filter(|s| s.project_id == pid)
                                    .map(|s| s.layout_path.clone());
                                if let Some(np) = new_path {
                                    ws.set_terminal_shell(&pid, &np, shell, cx);
                                }
                            }
                        });
                    });
                    self.spawn_terminals_for_project(project_id.clone(), cx);
                }
            }
            OverlayManagerEvent::TabReconnect { project_id, layout_path, tab_index } => {
                let terminal_ids = collect_tab_terminal_ids(&self.workspace, project_id, layout_path, cx);
                if let Some(target_tid) = terminal_ids.get(*tab_index).cloned() {
                    let old_size = self.terminals.lock().get(&target_tid).map(|t| t.size());
                    self.backend.kill(&target_tid);
                    let cwd = self.workspace.read(cx).project(project_id).map(|p| p.path.clone()).unwrap_or_default();
                    let shell = {
                        let ws = self.workspace.read(cx);
                        if let Some(project) = ws.project(project_id)
                            && let Some(ref layout) = project.layout
                            && let Some(path) = layout.find_terminal_path(&target_tid) {
                                ws.get_terminal_shell(project_id, &path)
                            } else {
                                None
                            }
                    };

                    let session_id = shell.as_ref().and_then(|s| match s {
                        velowork_core::shell::ShellType::Custom { path, args } if path == "ssh" => {
                            velowork_terminal::pty_manager::parse_ssh_args(args).and_then(|(_, _, _, _, sid, _)| sid)
                        }
                        velowork_core::shell::ShellType::Custom { path, args } if path == "serial" => {
                            velowork_terminal::pty_manager::parse_serial_args(args).and_then(|(_, _, sid)| sid)
                        }
                        velowork_core::shell::ShellType::Custom { path, args } if path == "telnet" => {
                            velowork_terminal::pty_manager::parse_telnet_args(args).and_then(|(_, _, sid)| sid)
                        }
                        _ => None,
                    }).or_else(|| self.backend.get_ssh_session_id(&target_tid));

                    if let Some(ref sid) = session_id {
                        if let Some(conn_store) = cx.try_global::<velowork_workspace::stores::GlobalConnectionStore>().map(|c| c.0.clone()) {
                            conn_store.update(cx, |store, cx| {
                                store.mark_connected(sid, cx);
                            });
                        }
                    }

                    let size = old_size.unwrap_or_default();
                    match self.backend.create_terminal(&cwd, shell.as_ref()) {
                        Ok(new_tid) => {
                            self.workspace.update(cx, |ws, cx| {
                                if let Some(project) = ws.project(project_id)
                                    && let Some(ref layout) = project.layout
                                    && let Some(path) = layout.find_terminal_path(&target_tid) {
                                        ws.set_terminal_id(project_id, &path, new_tid.clone(), cx);
                                    }
                            });
                            let app_settings = crate::settings::settings(cx);
                            let terminal = std::sync::Arc::new(crate::terminal::terminal::Terminal::new_with_scrollback(
                                new_tid.clone(),
                                size,
                                self.backend.transport(),
                                cwd,
                                app_settings.scrollback_lines as usize,
                            ));
                            if let Some(pid) = self.backend.get_foreground_shell_pid(&new_tid) {
                                terminal.set_shell_pid(pid);
                            }
                            self.terminals.lock().insert(new_tid.clone(), terminal);
                        }
                        Err(e) => {
                            if let Some(ref sid) = session_id {
                                if let Some(conn_store) = cx.try_global::<velowork_workspace::stores::GlobalConnectionStore>().map(|c| c.0.clone()) {
                                    conn_store.update(cx, |store, cx| {
                                        store.mark_disconnected(sid, cx);
                                    });
                                }
                            }
                            velowork_workspace::toast::ToastManager::error(format!("Failed to reconnect: {}", e), cx);
                        }
                    }
                    cx.notify();
                }
            }
            OverlayManagerEvent::TabSplitHorizontal { project_id, layout_path, tab_index } => {
                let focus_manager = self.focus_manager.clone();
                let workspace = self.workspace.clone();
                let pid = project_id.clone();
                let path = layout_path.clone();
                let idx = *tab_index;
                focus_manager.update(cx, |fm, cx| {
                    workspace.update(cx, |ws, cx| {
                        ws.set_active_tab(&pid, &path, idx, cx);
                        ws.split_terminal(fm, &pid, &path, velowork_core::types::SplitDirection::Horizontal, cx);
                    });
                });
                self.spawn_terminals_for_project(project_id.clone(), cx);
            }
            OverlayManagerEvent::TabSplitVertical { project_id, layout_path, tab_index } => {
                let focus_manager = self.focus_manager.clone();
                let workspace = self.workspace.clone();
                let pid = project_id.clone();
                let path = layout_path.clone();
                let idx = *tab_index;
                focus_manager.update(cx, |fm, cx| {
                    workspace.update(cx, |ws, cx| {
                        ws.set_active_tab(&pid, &path, idx, cx);
                        ws.split_terminal(fm, &pid, &path, velowork_core::types::SplitDirection::Vertical, cx);
                    });
                });
                self.spawn_terminals_for_project(project_id.clone(), cx);
            }
            OverlayManagerEvent::TabClose { project_id, layout_path, tab_index } => {
                let terminal_ids = collect_tab_terminal_ids(&self.workspace, project_id, layout_path, cx);
                if let Some(tid) = terminal_ids.get(*tab_index).cloned() {
                    let dispatcher = self.dispatcher_for_project(project_id, cx);
                    dispatcher.dispatch(ActionRequest::CloseTerminal {
                        project_id: project_id.clone(),
                        terminal_id: tid,
                    }, cx);
                }
            }
            OverlayManagerEvent::TabCloseOthers { project_id, layout_path, tab_index } => {
                let terminal_ids = collect_tab_terminal_ids(&self.workspace, project_id, layout_path, cx);
                let to_close: Vec<String> = terminal_ids.into_iter().enumerate()
                    .filter(|(i, _)| *i != *tab_index)
                    .map(|(_, id)| id)
                    .collect();
                if !to_close.is_empty() {
                    let dispatcher = self.dispatcher_for_project(project_id, cx);
                    dispatcher.dispatch(ActionRequest::CloseTerminals {
                        project_id: project_id.clone(),
                        terminal_ids: to_close,
                    }, cx);
                }
            }
            OverlayManagerEvent::TabCloseToRight { project_id, layout_path, tab_index } => {
                let terminal_ids = collect_tab_terminal_ids(&self.workspace, project_id, layout_path, cx);
                let to_close: Vec<String> = terminal_ids.into_iter().skip(tab_index + 1).collect();
                if !to_close.is_empty() {
                    let dispatcher = self.dispatcher_for_project(project_id, cx);
                    dispatcher.dispatch(ActionRequest::CloseTerminals {
                        project_id: project_id.clone(),
                        terminal_ids: to_close,
                    }, cx);
                }
            }
            OverlayManagerEvent::TabCloseInactive { project_id, layout_path } => {
                let terminal_ids = collect_tab_terminal_ids(&self.workspace, project_id, layout_path, cx);
                let inactive_ids: Vec<String> = terminal_ids.into_iter().filter(|tid| {
                    let ssh_disconnected = self.backend.get_ssh_session(tid).map(|s| s.is_closed()).unwrap_or(false);
                    let local_dead = self.backend.get_ssh_session(tid).is_none() && self.backend.get_shell_pid(tid).is_none();
                    ssh_disconnected || local_dead
                }).collect();
                if !inactive_ids.is_empty() {
                    let dispatcher = self.dispatcher_for_project(project_id, cx);
                    dispatcher.dispatch(ActionRequest::CloseTerminals {
                        project_id: project_id.clone(),
                        terminal_ids: inactive_ids,
                    }, cx);
                }
            }
            OverlayManagerEvent::TabToggleMinimize { project_id, layout_path, tab_index } => {
                let terminal_ids = collect_tab_terminal_ids(&self.workspace, project_id, layout_path, cx);
                if let Some(target_tid) = terminal_ids.get(*tab_index).cloned() {
                    let dispatcher = self.dispatcher_for_project(project_id, cx);
                    dispatcher.dispatch(ActionRequest::ToggleMinimized {
                        project_id: project_id.clone(),
                        terminal_id: target_tid,
                    }, cx);
                }
            }
            OverlayManagerEvent::TerminalEditConfig { terminal_id } => {
                let ssh_session_id = self.backend.get_ssh_session_id(terminal_id);
                if let Some(sid) = ssh_session_id {
                    let session_opt = {
                        let session_store = cx.global::<velowork_workspace::stores::GlobalSessionStore>().0.read(cx);
                        session_store.find_session(&sid).cloned()
                    };
                    if let Some(session) = session_opt {
                        self.sidebar.update(cx, |sp, cx| {
                            sp.open_edit_session_dialog(session, cx);
                        });
                        return;
                    }
                }
                self.overlay_manager.update(cx, |om, cx| {
                    om.open_settings_panel_to(Some(crate::views::overlays::settings::settings_panel::SettingsCategory::Terminal), cx);
                });
            }
            OverlayManagerEvent::TabSessionSettings { project_id, layout_path, tab_index } => {
                let terminal_ids = collect_tab_terminal_ids(&self.workspace, project_id, layout_path, cx);
                if let Some(target_tid) = terminal_ids.get(*tab_index).cloned() {
                    let ssh_session_id = self.backend.get_ssh_session_id(&target_tid);
                    if let Some(sid) = ssh_session_id {
                        let session_opt = {
                            let session_store = cx.global::<velowork_workspace::stores::GlobalSessionStore>().0.read(cx);
                            session_store.find_session(&sid).cloned()
                        };
                        if let Some(session) = session_opt {
                            self.sidebar.update(cx, |sp, cx| {
                                sp.open_edit_session_dialog(session, cx);
                            });
                            return;
                        }
                    }
                    self.overlay_manager.update(cx, |om, cx| {
                        om.toggle_settings_panel(cx);
                    });
                }
            }
            OverlayManagerEvent::SwitchProfile(id) => {
                self.handle_switch_profile(id.clone(), cx);
            }
            OverlayManagerEvent::ModalClosed => {
                self.needs_focus_restore = true;
                cx.notify();
            }
        }
    }

    /// Flush pending saves, spawn a new Velowork process for `id`, then quit.
    /// The spawned child is dropped immediately and survives as an orphan (Unix)
    /// or independent process (Windows) — same pattern as the updater's restart_app.
    pub(super) fn handle_switch_profile(&self, id: String, cx: &mut Context<Self>) {
        // 1. Flush settings
        if let Some(gs) = cx.try_global::<GlobalSettings>() {
            gs.0.read(cx).flush_pending_save();
        }

        // 2. Flush workspace
        if let Some(gw) = cx.try_global::<GlobalWorkspace>()
            && let Err(e) = persistence::save_workspace(gw.0.read(cx).data()) {
                log::error!("[app:profile] Failed to flush workspace before profile switch | error: {:#}", e);
            }

        // 3. Spawn current_exe with --profile <id>. Strip any existing --profile arg
        //    so we don't double-pass it.
        match std::env::current_exe() {
            Ok(exe) => {
                let mut args: Vec<String> = std::env::args().skip(1).collect();
                strip_profile_args(&mut args);
                let _ = std::process::Command::new(&exe)
                    .args(&args)
                    .arg("--profile")
                    .arg(&id)
                    .env("VELOWORK_ACTIVATE", "1")
                    .spawn();
            }
            Err(e) => {
                log::error!("[app:profile] Relaunch aborted: could not resolve current_exe | error: {:#}", e);
            }
        }

        cx.quit();
    }

    /// Process pending overlay requests from workspace state.
    ///
    /// Drains the overlay request queue and dispatches each request to the
    /// OverlayManager. Requests for already-open overlays are silently dropped.
    pub(super) fn process_pending_requests(&mut self, cx: &mut Context<Self>) {
        let requests: Vec<_> = self.request_broker.update(cx, |broker, _cx| {
            broker.drain_overlay_requests()
        });

        for request in requests {
            match request {
                OverlayRequest::Project(ProjectOverlay { project_id, kind }) => match kind {
                    ProjectOverlayKind::ContextMenu { position } => {
                        if !self.overlay_manager.read(cx).has_context_menu() {
                            self.overlay_manager.update(cx, |om, cx| {
                                om.show_context_menu(project_id, position, cx);
                            });
                        }
                    }
                    ProjectOverlayKind::ShellSelector { terminal_id, current_shell } => {
                        self.overlay_manager.update(cx, |om, cx| {
                            om.show_shell_selector(current_shell, project_id, terminal_id, cx);
                        });
                    }
                    ProjectOverlayKind::TerminalContextMenu { terminal_id, layout_path, position, has_selection, link_url } => {
                        let (is_recording, is_recording_paused) = {
                            let terminals = self.terminals.lock();
                            if let Some(terminal) = terminals.get(&terminal_id) {
                                (terminal.is_log_recording(), terminal.is_log_recording_paused())
                            } else {
                                (false, false)
                            }
                        };
                        let settings = crate::settings::settings_entity(cx).read(cx).settings.clone();
                        let word_wrap_enabled = settings.wrap_mode == crate::workspace::settings::WrapMode::WindowEdge;
                        let line_numbers_enabled = settings.show_line_numbers;
                        let selection = self
                            .terminals
                            .lock()
                            .get(&terminal_id)
                            .and_then(|t| t.get_selected_text())
                            .unwrap_or_default();

                        self.overlay_manager.update(cx, |om, cx| {
                            om.show_terminal_context_menu(
                                terminal_id,
                                project_id,
                                layout_path,
                                position,
                                has_selection,
                                selection,
                                link_url,
                                is_recording,
                                is_recording_paused,
                                word_wrap_enabled,
                                line_numbers_enabled,
                                cx,
                            );
                        });
                    }
                    ProjectOverlayKind::TerminalLogStop { terminal_id } => {
                        self.handle_overlay_manager_event(
                            self.overlay_manager.clone(),
                            &OverlayManagerEvent::TerminalLogStop { terminal_id },
                            cx,
                        );
                    }
                    ProjectOverlayKind::TabContextMenu { tab_index, num_tabs, layout_path, position } => {
                        let terminal_ids = collect_tab_terminal_ids(&self.workspace, &project_id, &layout_path, cx);
                        let target_tid = terminal_ids.get(tab_index);
                        let is_ssh = if let Some(target_tid) = target_tid {
                            let shell = {
                                let ws = self.workspace.read(cx);
                                if let Some(project) = ws.project(&project_id)
                                    && let Some(ref layout) = project.layout
                                    && let Some(path) = layout.find_terminal_path(target_tid) {
                                        ws.get_terminal_shell(&project_id, &path)
                                    } else {
                                        None
                                    }
                            };
                            if let Some(velowork_terminal::shell_config::ShellType::Custom { path, .. }) = shell {
                                path == "ssh"
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        let is_minimized = if let Some(target_tid) = target_tid {
                            let ws = self.workspace.read(cx);
                            if let Some(project) = ws.project(&project_id)
                                && let Some(ref layout) = project.layout
                                && let Some(path) = layout.find_terminal_path(target_tid)
                                && let Some(node) = layout.get_at_path(&path) {
                                    match node {
                                        LayoutNode::Terminal { minimized, .. } => *minimized,
                                        _ => false,
                                    }
                                } else {
                                    false
                                }
                        } else {
                            false
                        };
                        let minimize_shortcut = crate::keybindings::shortcut_for_action("MinimizeTerminal").map(|s| s.into());
                        self.overlay_manager.update(cx, |om, cx| {
                            om.show_tab_context_menu(
                                tab_index,
                                num_tabs,
                                project_id,
                                layout_path,
                                position,
                                is_ssh,
                                is_minimized,
                                minimize_shortcut,
                                cx,
                            );
                        });
                    }
                    ProjectOverlayKind::ToggleSftpPanel => {
                        let _ = project_id;
                        self.toggle_sftp(cx);
                    }
                    ProjectOverlayKind::ShowAiFloatingToolbar { terminal_id, position, selection_text } => {
                        self.overlay_manager.update(cx, |om, cx| {
                            om.show_terminal_ai_floating_toolbar(
                                terminal_id,
                                project_id,
                                position,
                                selection_text,
                                cx,
                            );
                        });
                    }
                    ProjectOverlayKind::DismissAiFloatingToolbar => {
                        self.overlay_manager.update(cx, |om, cx| {
                            om.dismiss_terminal_ai_inline(cx);
                        });
                    }
                },
                OverlayRequest::Folder(FolderOverlay { folder_id, kind }) => match kind {
                    FolderOverlayKind::ContextMenu { folder_name, position } => {
                        if !self.overlay_manager.read(cx).has_folder_context_menu() {
                            self.overlay_manager.update(cx, |om, cx| {
                                om.show_folder_context_menu(folder_id, folder_name, position, cx);
                            });
                        }
                    }
                },
                OverlayRequest::ManageProjectsDialog => {
                    self.overlay_manager.update(cx, |om, cx| {
                        om.toggle_manage_projects_dialog(cx);
                    });
                }
                OverlayRequest::AddProjectDialog => {
                    self.overlay_manager.update(cx, |om, cx| {
                        om.toggle_add_project_dialog(cx);
                    });
                }
                OverlayRequest::ShowCommandPalette => {
                    self.overlay_manager.update(cx, |om, cx| {
                        om.toggle_command_palette(cx);
                    });
                }
                OverlayRequest::ImportSessionsDialog => {
                    self.overlay_manager.update(cx, |om, cx| {
                        om.toggle_import_sessions_dialog(cx);
                    });
                }
            }
        }
    }

    /// Drain the broker's "send to terminal" queue and paste each payload into
    /// the currently focused terminal. Resolves the terminal's CWD per call so
    /// queued payloads sent while the user navigates use the latest known cwd.
    pub(super) fn process_pending_send_to_terminal(&mut self, cx: &mut Context<Self>) {
        let payloads = self.request_broker.update(cx, |broker, _cx| {
            broker.drain_send_to_terminal()
        });
        for payload in payloads {
            self.send_payload_to_active_terminal(payload, cx);
        }
    }

    pub fn toggle_active_sftp(&mut self, cx: &mut Context<Self>) {
        self.toggle_sftp(cx);
    }

    pub fn toggle_active_sftp_with_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let is_open = self.bottom_dock_ctrl.is_open() && self.bottom_dock_ctrl.animation() > 0.01;
        let sftp_active = self.bottom_dock.read(cx)
            .active_tab()
            .map_or(false, |t| t.metadata(cx).id.0 == "sftp");

        if !is_open || !sftp_active {
            self.toggle_sftp(cx);
            let handle = self.sftp_panel.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        } else {
            self.toggle_sftp(cx);
            self.focus_active_terminal(window, cx);
        }
    }

    pub fn toggle_active_commands(&mut self, cx: &mut Context<Self>) {
        self.toggle_commands(cx);
    }

    pub fn toggle_active_commands_with_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let is_open = self.bottom_dock_ctrl.is_open() && self.bottom_dock_ctrl.animation() > 0.01;
        let commands_active = self.bottom_dock.read(cx)
            .active_tab()
            .map_or(false, |t| t.metadata(cx).id.0 == "commands");

        if !is_open || !commands_active {
            self.toggle_commands(cx);
            self.commands_panel.update(cx, |cp, cx| {
                cp.focus_input(cx);
            });
        } else {
            self.toggle_commands(cx);
            self.focus_active_terminal(window, cx);
        }
    }

    /// 切换底部面板：未展开时展开并聚焦当前活动 Tab，已展开时折叠并归还焦点给活动终端
    pub(super) fn toggle_bottom_dock_with_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let is_open = self.bottom_dock_ctrl.is_open() && self.bottom_dock_ctrl.animation() > 0.01;

        if !is_open {
            self.show_bottom_dock(cx);
            self.bottom_dock.update(cx, |d, cx| {
                d.focus_active_tab(window, cx);
            });
        } else {
            self.hide_bottom_dock(cx);
            self.focus_active_terminal(window, cx);
        }
    }

    /// Focus a dock by cycle index (0=Left sidebar, 1=Center terminal, 2=Bottom dock, 3=Right dock).
    /// Opens the dock first when it is collapsed, or focuses it if open but unfocused, or closes it if focused.
    pub(super) fn focus_dock_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        match index {
            0 => {
                self.toggle_left_dock_with_window(window, cx);
            }
            1 => {
                self.focus_active_terminal(window, cx);
            }
            2 => {
                self.toggle_bottom_dock_with_window(window, cx);
            }
            _ => {
                self.toggle_right_dock_with_window(window, cx);
            }
        }
    }

    /// Toggle the bottom "Commands" panel for the active project.
    ///
    /// The status-bar command icon is a *single* interaction: it only ever
    /// shows or hides the commands panel. It must never open the unrelated
    /// global command palette overlay — `CommandPalette` (a separate entity).
    ///
    /// Whether the project identified by `project_id` currently has at least
    /// one open terminal (i.e. a terminal layout node with an initialized
    /// terminal id exists in its layout tree).
    ///
    /// The bottom Commands / SFTP panels live inside a `TerminalPane`, which is
    /// only rendered when such a node exists. This is the correct predicate for
    /// deciding whether the panels can be hosted — distinct from the mere
    /// presence of a `layout_container`, which may exist with an empty layout.
    #[allow(dead_code)]
    fn project_has_terminal(&self, project_id: &str, cx: &App) -> bool {
        self.workspace
            .read(cx)
            .project(project_id)
            .and_then(|p| p.layout.as_ref())
            .is_some_and(|l| !l.collect_terminal_ids().is_empty())
    }

    /// Check whether there are active session connections across the application.
    pub fn active_session_count(&self, cx: &App) -> usize {
        let store_count = cx
            .try_global::<velowork_workspace::stores::GlobalConnectionStore>()
            .map(|g| g.0.read(cx).active_count())
            .unwrap_or(0);
        let pty_count = self.pty_manager.active_ssh_session_count();
        store_count.max(pty_count)
    }

    /// Count the number of open terminal tabs across the application.
    pub fn open_terminal_count(&self) -> usize {
        self.terminals.lock().len()
    }

    pub(crate) fn flush_all_state(cx: &mut App) {
        if let Some(gs) = cx.try_global::<GlobalSettings>() {
            gs.0.read(cx).flush_pending_save();
        }
        if let Some(gw) = cx.try_global::<GlobalWorkspace>()
            && let Err(e) = persistence::save_workspace(gw.0.read(cx).data()) {
                log::error!("Failed to flush workspace on quit: {}", e);
            }
        if let Some(ss) = cx.try_global::<velowork_workspace::stores::GlobalSessionStore>() {
            ss.0.read(cx).flush_pending_save();
        }
        if let Some(ts) = cx.try_global::<velowork_workspace::stores::GlobalTunnelStore>() {
            ts.0.read(cx).flush_pending_save();
        }
    }

    /// Request application quit. If there are open terminal tabs or active
    /// session connections, prompts for confirmation; otherwise quits immediately.
    pub fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.allow_force_quit {
            Self::flush_all_state(cx);
            cx.quit();
            return;
        }

        let session_count = self.active_session_count(cx);
        let terminal_count = self.open_terminal_count();
        if session_count == 0 && terminal_count == 0 {
            Self::flush_all_state(cx);
            self.allow_force_quit = true;
            cx.quit();
            return;
        }

        // Before prompting confirmation, ensure window is unminimized and brought to foreground
        window.activate_window();
        window.refresh();

        let weak = cx.entity().downgrade();
        self.overlay_manager.update(cx, |om, cx| {
            om.open_confirm_quit_dialog(session_count, terminal_count, cx, move |cx| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |v, cx| {
                        Self::flush_all_state(cx);
                        v.allow_force_quit = true;
                        cx.quit();
                    });
                } else {
                    Self::flush_all_state(cx);
                    cx.quit();
                }
            });
        });
    }

    /// Request closing this window. For the main window, honors `close_behavior`
    /// (minimize or exit). For extra windows, removes the window.
    pub fn request_close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.window_id {
            crate::workspace::state::WindowId::Main => {
                let behavior = crate::settings::settings_entity(cx)
                    .read(cx)
                    .settings
                    .close_behavior;
                match behavior {
                    crate::workspace::settings::CloseBehavior::Minimize => {
                        window.minimize_window();
                    }
                    crate::workspace::settings::CloseBehavior::Exit => {
                        self.request_quit(window, cx);
                    }
                }
            }
            extra_id @ crate::workspace::state::WindowId::Extra(_) => {
                self.workspace.update(cx, |ws, cx| {
                    ws.close_extra_window(extra_id, cx);
                });
                window.remove_window();
            }
        }
    }

    /// Global handler for AddTab action (always creates a new terminal tab).
    pub fn handle_global_add_tab(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let (project_id, target_path) = {
            let fm = self.focus_manager.read(cx);
            if let Some(focused) = fm.focused_terminal_state() {
                (focused.project_id.clone(), Some(focused.layout_path.clone()))
            } else if let Some(active_pid) = fm.active_project_id() {
                (active_pid.clone(), None)
            } else {
                let ws = self.workspace.read(cx);
                let pid = ws.projects().first().map(|p| p.id.clone());
                (pid.unwrap_or_default(), None)
            }
        };

        if project_id.is_empty() {
            return;
        }

        let dispatcher = self.dispatcher_for_project(&project_id, cx);
        let path = target_path.unwrap_or_default();
        dispatcher.dispatch(
            ActionRequest::AddTab {
                project_id: project_id.clone(),
                path,
                in_group: false,
            },
            cx,
        );
    }

    /// Global handler to open add session dialog
    pub fn open_add_session_dialog(&mut self, cx: &mut Context<Self>) {
        if !self.left_dock_ctrl.is_open() && !self.left_dock_ctrl.is_hover_shown() {
            self.toggle_sidebar(cx);
        }
        self.sidebar.update(cx, |sp, cx| {
            sp.open_add_session_dialog(None, cx);
        });
    }

    /// Global handler to open add session dialog with window to focus the initial input
    pub fn open_add_session_dialog_with_window(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.left_dock_ctrl.is_open() && !self.left_dock_ctrl.is_hover_shown() {
            self.toggle_sidebar(cx);
        }
        self.sidebar.update(cx, |sp, cx| {
            sp.open_add_session_dialog(None, cx);
        });
    }
}

/// Collect terminal IDs from children of a Tabs node at the given layout path.
///
/// Each child subtree is traversed with `collect_terminal_ids()`, so nested
/// splits/tabs within a tab are handled correctly. Returns one entry per child.
fn collect_tab_terminal_ids(
    workspace: &Entity<Workspace>,
    project_id: &str,
    layout_path: &[usize],
    cx: &Context<WindowView>,
) -> Vec<String> {
    let ws = workspace.read(cx);
    let Some(project) = ws.project(project_id) else {
        return Vec::new();
    };
    let Some(ref layout) = project.layout else {
        return Vec::new();
    };
    let Some(node) = layout.get_at_path(layout_path) else {
        return Vec::new();
    };
    match node {
        LayoutNode::Tabs { children, .. } => {
            children.iter().filter_map(|child| {
                // For simple Terminal children, get the ID directly.
                // For nested structures, get the first terminal ID.
                child.collect_terminal_ids().into_iter().next()
            }).collect()
        }
        LayoutNode::Terminal { terminal_id, .. } => {
            terminal_id.iter().cloned().collect()
        }
        _ => Vec::new(),
    }
}

/// Remove profile-selecting flags so the relaunched process picks them up fresh.
///
/// Strips both `--profile` and `--new-profile` (with their values, in either
/// `--flag value` or `--flag=value` form). If `--new-profile` survived the
/// relaunch it would re-trigger profile creation each time the user switches
/// profiles via the GUI, and would also override the `--profile <id>` we
/// append.
fn strip_profile_args(args: &mut Vec<String>) {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--profile" || args[i] == "--new-profile" {
            args.remove(i);
            if i < args.len() {
                args.remove(i);
            }
        } else if args[i].starts_with("--profile=") || args[i].starts_with("--new-profile=") {
            args.remove(i);
        } else {
            i += 1;
        }
    }
}

fn percent_encode(text: &str) -> String {
    let mut encoded = String::new();
    for b in text.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => {
                encoded.push('+');
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}
