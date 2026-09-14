use crate::settings::settings_entity;
use gpui::*;
use velowork_ui::dock::DockPosition;
use velowork_workspace::dock_controller::{
    ANIMATION_DURATION_MS, AnimationTarget, DockController, FRAME_TIME_MS,
};

use super::WindowView;

impl WindowView {
    /// Show (expand) Left Dock
    pub(super) fn show_left_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.left_dock_ctrl.set_open(true);
        self.left_dock.update(cx, |dp, cx| {
            dp.collapse_state = velowork_ui::dock::PanelCollapseState::Normal;
            cx.notify();
        });
        let window_id = self.window_id;
        self.workspace.update(cx, |ws, cx| {
            ws.set_dock_open(window_id, DockPosition::Left, true, cx)
        });
        settings_entity(cx).update(cx, |s, cx| s.set_sidebar_open(true, cx));
        self.sync_status_bar_dock_state(cx);
        self.animate_dock_to(DockPosition::Left, AnimationTarget::Open, cx);
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::SessionTree);
        });
        self.sidebar.update(cx, |sb, cx| {
            sb.focus_session_tree(window, cx);
        });
    }

    /// Hide (collapse) Left Dock
    pub(super) fn hide_left_dock(&mut self, cx: &mut Context<Self>) {
        self.left_dock_ctrl.set_open(false);
        let window_id = self.window_id;
        self.workspace.update(cx, |ws, cx| {
            ws.set_dock_open(window_id, DockPosition::Left, false, cx)
        });
        settings_entity(cx).update(cx, |s, cx| s.set_sidebar_open(false, cx));
        self.sync_status_bar_dock_state(cx);
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
        });
        self.animate_dock_to(DockPosition::Left, AnimationTarget::Close, cx);
    }

    /// 弹窗专用发起源精准归还及三级降级兜底链路：
    /// 1. 优先尝试归还原主控件句柄 (Origin FocusHandle)
    /// 2. 降级 1：若原项失效（如被删除/unmount），尝试聚焦宿主面板容器 (Panel Container FocusHandle)
    /// 3. 若仍有父级弹窗在栈中（多层嵌套），聚焦父级弹窗
    /// 4. 降级 2：终极兜底至当前活动终端或欢迎页快速连接输入框
    pub fn restore_modal_closed_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (origin, panel) = self.overlay_manager.update(cx, |om, _| {
            (
                om.take_pending_restore_origin(),
                om.take_pending_restore_panel(),
            )
        });

        // 1. 优先尝试归还原主控件句柄 (Origin FocusHandle)
        if let Some(ref handle) = origin {
            window.focus(handle, cx);
            if window.focused(cx).is_some() {
                return;
            }
        }

        // 2. 降级 1：若原项失效（如被删除/unmount），尝试聚焦宿主面板容器 (Panel Container FocusHandle)
        if let Some(ref panel_handle) = panel {
            window.focus(panel_handle, cx);
            if window.focused(cx).is_some() {
                return;
            }
        }

        // 3. 若仍有父级弹窗在栈中（多层嵌套），聚焦父级弹窗
        if let Some(parent_modal_handle) =
            self.overlay_manager.read(cx).active_modal_focus_handle(cx)
        {
            window.focus(&parent_modal_handle, cx);
            if window.focused(cx).is_some() {
                return;
            }
        }

        // 4. 降级 2：意图层驱动的级联恢复 (根据 FocusLayer 回流至会话树或终端/欢迎页)
        self.restore_focus_cascade(window, cx);
    }

    /// 意图层驱动的级联焦点恢复：
    /// 根据 FocusManager::current_layer 精准感知用户意图：
    /// - 若意图层为 SessionTree 且侧边栏已展开，恢复会话树选中节点焦点；
    /// - 若意图层为 Terminal 或侧边栏未打开/无法聚焦，回退至终端恢复链路；
    /// - 确保在任何弹窗关闭或焦点悬空时永不越权错乱。
    pub fn restore_focus_cascade(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current_layer = self.focus_manager.read(cx).current_layer();
        match current_layer {
            velowork_workspace::focus::FocusLayer::SessionTree => {
                if self.left_dock_ctrl.is_open() {
                    self.sidebar.update(cx, |sb, cx| {
                        sb.focus_session_tree(window, cx);
                    });
                    if window.focused(cx).is_some() {
                        return;
                    }
                }
                self.focus_active_terminal(window, cx);
            }
            _ => {
                self.focus_active_terminal(window, cx);
            }
        }
    }

    /// 焦点委托恢复：四级级联逐级探测并硬兜底回退至主窗口根视图（div id="root"），确保物理焦点永不悬空
    pub fn focus_active_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
        });

        // 1. 优先通过 pane_map 查询并聚焦当前活动终端
        if let Some((project_id, _)) = self.focused_terminal_id(cx) {
            let pane_map = crate::views::layout::navigation::get_pane_map(self.window_id);
            if let Some(handle) = pane_map
                .panes()
                .iter()
                .find(|p| p.project_id == project_id)
                .and_then(|p| p.focus_handle.as_ref())
            {
                window.focus(handle, cx);
                if window.focused(cx).is_some() {
                    return;
                }
            }
        }

        // 2. 尝试 pane_map 中首个可用终端
        let pane_map = crate::views::layout::navigation::get_pane_map(self.window_id);
        if let Some(handle) = pane_map
            .panes()
            .first()
            .and_then(|p| p.focus_handle.as_ref())
        {
            window.focus(handle, cx);
            if window.focused(cx).is_some() {
                return;
            }
        }

        // 3. 直接通过 ProjectColumn 实体树探测活动终端或欢迎页输入框（覆盖冷启动 Canvas 尚未 Paint Bounds 的时序）
        let active_project_id = self.focus_manager.read(cx).active_project_id().cloned();
        let target_col = active_project_id
            .as_ref()
            .and_then(|pid| self.project_columns.get(pid).cloned())
            .or_else(|| self.project_columns.values().next().cloned());

        if let Some(col) = target_col {
            col.update(cx, |p, cx| {
                p.focus_active_element(window, cx);
            });
            if window.focused(cx).is_some() {
                return;
            }
        }

        // 4. 终极硬兜底：聚焦主窗口根容器 div id="root"，确保全局快捷键时刻就绪
        window.focus(&self.focus_handle, cx);
    }

    /// 切换左侧侧边栏：未展开时展开并聚焦，已展开时直接收起并归还焦点给活动终端
    pub(super) fn toggle_left_dock_with_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_open = self.left_dock_ctrl.is_open();

        if !is_open {
            self.show_left_dock(window, cx);
        } else {
            self.hide_left_dock(cx);
            self.focus_active_terminal(window, cx);
        }
    }

    /// Toggle Left Dock visibility with animation
    pub(super) fn toggle_left_dock(&mut self, cx: &mut Context<Self>) {
        let target = self.left_dock_ctrl.toggle();
        let open = self.left_dock_ctrl.is_open();
        if open {
            self.left_dock.update(cx, |dp, cx| {
                dp.collapse_state = velowork_ui::dock::PanelCollapseState::Normal;
                cx.notify();
            });
        } else {
            self.focus_manager.update(cx, |fm, _| {
                fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
            });
        }
        let window_id = self.window_id;

        self.workspace.update(cx, |ws, cx| {
            ws.set_dock_open(window_id, DockPosition::Left, open, cx)
        });
        settings_entity(cx).update(cx, |s, cx| s.set_sidebar_open(open, cx));
        self.sync_status_bar_dock_state(cx);
        self.animate_dock_to(DockPosition::Left, target, cx);
    }

    /// Alias for toggle_left_dock for keybindings backward compatibility
    pub(super) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.toggle_left_dock(cx);
    }

    /// 切换右侧 Dock：未展开时展开并聚焦，已展开时直接收起并归还焦点给活动终端
    pub(super) fn toggle_right_dock_with_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_open = self.right_dock_ctrl.is_open();

        if !is_open {
            let panel_id = self
                .right_toolbar_active
                .clone()
                .or_else(|| self.right_toolbar_last_panel.clone())
                .unwrap_or_else(|| "ai_assistant".to_string());
            self.show_right_dock(&panel_id, window, cx);
        } else {
            self.hide_right_dock(cx);
            self.focus_active_terminal(window, cx);
        }
    }

    /// Show (expand) Right Dock with given panel_id
    pub(super) fn show_right_dock(
        &mut self,
        panel_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.right_dock.is_none() {
            let panel = self.create_right_panel(panel_id, window, cx);
            self.right_dock = Some(panel);
            self.right_toolbar_active = Some(panel_id.to_string());
            self.right_toolbar_last_panel = Some(panel_id.to_string());
        } else if self.right_toolbar_last_panel.as_deref() != Some(panel_id) {
            if let Some(dock) = self.right_dock.as_ref() {
                dock.update(cx, |d, cx| {
                    d.activate_or_add_panel(panel_id, window, cx);
                });
            }
            self.right_toolbar_active = Some(panel_id.to_string());
            self.right_toolbar_last_panel = Some(panel_id.to_string());
        } else {
            self.right_toolbar_active = Some(panel_id.to_string());
        }
        self.right_dock_ctrl.set_open(true);
        if let Some(dock) = self.right_dock.as_ref() {
            dock.update(cx, |d, cx| {
                d.collapse_state = velowork_ui::dock::PanelCollapseState::Normal;
                d.focus_active_tab(window, cx);
            });
        }
        let window_id = self.window_id;
        self.workspace.update(cx, |ws, cx| {
            ws.set_dock_open(window_id, DockPosition::Right, true, cx)
        });
        settings_entity(cx).update(cx, |s, cx| s.set_right_sidebar_open(true, cx));
        self.sync_status_bar_dock_state(cx);
        self.animate_dock_to(DockPosition::Right, AnimationTarget::Open, cx);
    }

    /// Hide (collapse) Right Dock
    pub(super) fn hide_right_dock(&mut self, cx: &mut Context<Self>) {
        self.right_toolbar_active = None;
        self.right_dock_ctrl.set_open(false);
        let window_id = self.window_id;
        self.workspace.update(cx, |ws, cx| {
            ws.set_dock_open(window_id, DockPosition::Right, false, cx)
        });
        settings_entity(cx).update(cx, |s, cx| s.set_right_sidebar_open(false, cx));
        self.sync_status_bar_dock_state(cx);
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
        });
        self.animate_dock_to(DockPosition::Right, AnimationTarget::Close, cx);
    }

    /// Toggle Right Dock visibility with animation
    pub(super) fn toggle_right_dock(&mut self, cx: &mut Context<Self>) {
        let target = self.right_dock_ctrl.toggle();
        let open = self.right_dock_ctrl.is_open();
        if open {
            if let Some(dock) = self.right_dock.as_ref() {
                dock.update(cx, |dp, cx| {
                    dp.collapse_state = velowork_ui::dock::PanelCollapseState::Normal;
                    cx.notify();
                });
            }
        } else {
            self.focus_manager.update(cx, |fm, _| {
                fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
            });
        }
        let window_id = self.window_id;

        self.workspace.update(cx, |ws, cx| {
            ws.set_dock_open(window_id, DockPosition::Right, open, cx)
        });
        settings_entity(cx).update(cx, |s, cx| s.set_right_sidebar_open(open, cx));
        self.sync_status_bar_dock_state(cx);
        self.animate_dock_to(DockPosition::Right, target, cx);
    }

    /// Toggle auto-hide mode on the left dock
    pub(super) fn toggle_sidebar_auto_hide(&mut self, cx: &mut Context<Self>) {
        let target = self.left_dock_ctrl.toggle_auto_hide();
        self.left_dock_ctrl = self.left_dock_ctrl.clone();
        let open = self.left_dock_ctrl.is_open();
        let auto_hide = self.left_dock_ctrl.is_auto_hide();
        let window_id = self.window_id;

        self.workspace.update(cx, |ws, cx| {
            ws.set_dock_open(window_id, DockPosition::Left, open, cx)
        });
        settings_entity(cx).update(cx, |s, cx| {
            s.set_sidebar_auto_hide(auto_hide, cx);
            s.set_sidebar_open(open, cx);
        });
        self.sync_status_bar_dock_state(cx);
        self.animate_dock_to(DockPosition::Left, target, cx);
        cx.notify();
    }

    /// Hide left dock on leave in auto-hide mode
    pub(super) fn hide_sidebar_on_leave(&mut self, cx: &mut Context<Self>) {
        let target = self.left_dock_ctrl.hide_on_leave();
        self.left_dock_ctrl = self.left_dock_ctrl.clone();
        self.animate_dock_to(DockPosition::Left, target, cx);
    }

    /// Check if the right toolbar strip is open
    pub(super) fn is_right_toolbar_open(&self) -> bool {
        self.right_toolbar_open
    }

    /// Toggle visibility of the right vertical icon toolbar strip
    pub(super) fn toggle_right_toolbar(&mut self, cx: &mut Context<Self>) {
        self.right_toolbar_open = !self.right_toolbar_open;
        let is_open = self.right_toolbar_open;
        settings_entity(cx).update(cx, |s, cx| {
            s.set_right_toolbar_open(is_open, cx);
        });
        cx.notify();
    }

    /// Handle clicking a panel icon on the right toolbar strip or activating its shortcut.
    pub(super) fn handle_right_toolbar_click(
        &mut self,
        panel_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.right_toolbar_active.as_deref() == Some(panel_id) && self.right_dock_ctrl.is_open()
        {
            self.hide_right_dock(cx);
            self.focus_active_terminal(window, cx);
        } else {
            self.show_right_dock(panel_id, window, cx);
        }
    }

    /// Sync dock open state to the status bar and title bar
    fn sync_status_bar_dock_state(&self, cx: &mut Context<Self>) {
        let left_open = self.left_dock_ctrl.is_open();
        let right_open = self.right_dock_ctrl.is_open();
        self.status_bar.update(cx, |sb, cx| {
            sb.set_sidebar_open(left_open, cx);
        });
        self.title_bar.update(cx, |tb, cx| {
            tb.set_sidebar_open(left_open, cx);
            tb.set_right_sidebar_open(right_open, cx);
        });
    }

    /// Animate dock container to target
    pub(super) fn animate_dock_to(
        &mut self,
        pos: DockPosition,
        target: AnimationTarget,
        cx: &mut Context<Self>,
    ) {
        if let Some(target_value) = target.value() {
            self.animate_dock(pos, target_value, cx);
        }
    }

    /// Animate dock container to target value (0.0 = collapsed, 1.0 = expanded)
    pub(super) fn animate_dock(&mut self, pos: DockPosition, target: f32, cx: &mut Context<Self>) {
        let current = match pos {
            DockPosition::Left => self.left_dock_ctrl.animation(),
            DockPosition::Right => self.right_dock_ctrl.animation(),
            DockPosition::Bottom => self.bottom_dock_ctrl.animation(),
            DockPosition::Top => 1.0,
        };

        if (current - target).abs() < 0.01 {
            match pos {
                DockPosition::Left => {
                    self.left_dock_ctrl.set_animation(target);
                    self.left_dock_anim_task = None;
                }
                DockPosition::Right => {
                    self.right_dock_ctrl.set_animation(target);
                    self.right_dock_anim_task = None;
                    if target <= 0.01 {
                        self.right_dock = None;
                    }
                }
                DockPosition::Bottom => {
                    self.bottom_dock_ctrl.set_animation(target);
                    self.bottom_dock_anim_task = None;
                }
                DockPosition::Top => {}
            }
            cx.notify();
            return;
        }

        let seq = match pos {
            DockPosition::Left => {
                self.left_dock_anim_seq = self.left_dock_anim_seq.wrapping_add(1);
                self.left_dock_anim_seq
            }
            DockPosition::Right => {
                self.right_dock_anim_seq = self.right_dock_anim_seq.wrapping_add(1);
                self.right_dock_anim_seq
            }
            DockPosition::Bottom => {
                self.bottom_dock_anim_seq = self.bottom_dock_anim_seq.wrapping_add(1);
                self.bottom_dock_anim_seq
            }
            DockPosition::Top => 0,
        };

        let duration = std::time::Duration::from_millis(ANIMATION_DURATION_MS);
        let step_duration = std::time::Duration::from_millis(FRAME_TIME_MS);

        let task = cx.spawn(async move |this: WeakEntity<WindowView>, cx| {
            let start = std::time::Instant::now();
            loop {
                smol::Timer::after(step_duration).await;
                let elapsed = start.elapsed();
                let ratio = (elapsed.as_secs_f32() / duration.as_secs_f32()).min(1.0);
                let progress = DockController::ease_progress_ratio(current, target, ratio);

                let result = this.update(cx, |this, cx| {
                    let current_seq = match pos {
                        DockPosition::Left => this.left_dock_anim_seq,
                        DockPosition::Right => this.right_dock_anim_seq,
                        DockPosition::Bottom => this.bottom_dock_anim_seq,
                        DockPosition::Top => 0,
                    };
                    if current_seq != seq {
                        return false;
                    }
                    match pos {
                        DockPosition::Left => {
                            this.left_dock_ctrl.set_animation(progress);
                        }
                        DockPosition::Right => this.right_dock_ctrl.set_animation(progress),
                        DockPosition::Bottom => this.bottom_dock_ctrl.set_animation(progress),
                        DockPosition::Top => {}
                    }
                    cx.notify();
                    true
                });
                if result.map(|cont| !cont).unwrap_or(true) || ratio >= 1.0 {
                    break;
                }
            }

            let _ = this.update(cx, |this, cx| {
                let current_seq = match pos {
                    DockPosition::Left => this.left_dock_anim_seq,
                    DockPosition::Right => this.right_dock_anim_seq,
                    DockPosition::Bottom => this.bottom_dock_anim_seq,
                    DockPosition::Top => 0,
                };
                if current_seq != seq {
                    return;
                }
                match pos {
                    DockPosition::Left => {
                        this.left_dock_ctrl.set_animation(target);
                        this.left_dock_anim_task = None;
                    }
                    DockPosition::Right => {
                        this.right_dock_ctrl.set_animation(target);
                        this.right_dock_anim_task = None;
                        // 保留右侧 dock 实体（保活缓存），避免下次点击展开时在主线程重新构建面板导致卡顿
                    }
                    DockPosition::Bottom => {
                        this.bottom_dock_ctrl.set_animation(target);
                        this.bottom_dock_anim_task = None;
                    }
                    DockPosition::Top => {}
                }
                cx.notify();
            });
        });

        match pos {
            DockPosition::Left => self.left_dock_anim_task = Some(task),
            DockPosition::Right => self.right_dock_anim_task = Some(task),
            DockPosition::Bottom => self.bottom_dock_anim_task = Some(task),
            DockPosition::Top => {}
        }
    }
}
