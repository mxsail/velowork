//! 通用动效模态外壳组件 (`AnimatedModal`)
//!
//! 负责全屏半透明遮罩、物理弹性微过冲进场动画、加速沉降退场动画及点击遮罩退场处理。
//! 任何业务弹窗卡片 View 放入 `AnimatedModal` 中即可自动拥有统一的微过冲弹性进出场动效。

use std::time::Instant;

use gpui::prelude::*;
use gpui::*;

use crate::motion::{DURATION_MODAL_ENTER, DURATION_MODAL_LEAVE, ModalMotionState};
use crate::overlay::modal::modal_backdrop;
use crate::theme::theme;

/// 全局动画开关配置（可选注入，默认为 true）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModalAnimationsEnabled(pub bool);

impl Global for ModalAnimationsEnabled {}

/// 动效模态外壳生命周期事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnimatedModalEvent {
    /// 模态框开始执行退场动画（用于业务层提前做局部清理）。
    Closing,
    /// 退场动画彻底播放完毕，外层宿主应真正卸载释放该模态实体并恢复焦点。
    Dismissed,
}

/// 弹窗在屏幕中的垂直/水平对齐方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ModalAlignment {
    /// 默认：屏幕水平与垂直正中对齐。
    #[default]
    Center,
    /// 顶部悬浮：水平居中，距离屏幕顶部固定偏移（适用于命令面板、快速选择器）。
    Top(Pixels),
}

/// 通用动效模态外壳组件。
pub struct AnimatedModal {
    content: AnyView,
    motion_state: ModalMotionState,
    _anim_task: Option<Task<()>>,
    dismiss_on_click_outside: bool,
    focus_handle: FocusHandle,
    animations_enabled: bool,
    alignment: ModalAlignment,
}

impl EventEmitter<AnimatedModalEvent> for AnimatedModal {}

impl AnimatedModal {
    /// 创建一个新的动效模态外壳并自动启动入场弹性过冲动画。
    pub fn new(content: AnyView, cx: &mut Context<Self>) -> Self {
        let animations_enabled = cx
            .try_global::<ModalAnimationsEnabled>()
            .map(|g| g.0)
            .unwrap_or(true);

        let mut modal = Self {
            content,
            motion_state: if animations_enabled {
                ModalMotionState::new_opening(None)
            } else {
                ModalMotionState::default()
            },
            _anim_task: None,
            dismiss_on_click_outside: true,
            focus_handle: cx.focus_handle(),
            animations_enabled,
            alignment: ModalAlignment::default(),
        };

        if animations_enabled {
            modal.start_enter_animation(cx);
        }

        modal
    }

    /// 设置弹窗在屏幕中的对齐方式。
    pub fn with_alignment(mut self, alignment: ModalAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// 便捷方法：将弹窗设置为顶部悬浮对齐，指定距顶偏移量。
    pub fn align_top(self, offset: Pixels) -> Self {
        self.with_alignment(ModalAlignment::Top(offset))
    }

    /// 设置是否允许点击外部遮罩关闭弹窗（默认为 true）。
    pub fn dismiss_on_click_outside(mut self, enabled: bool) -> Self {
        self.dismiss_on_click_outside = enabled;
        self
    }

    /// 显式覆盖动画开关。
    pub fn with_animations(mut self, enabled: bool, cx: &mut Context<Self>) -> Self {
        self.animations_enabled = enabled;
        if !enabled {
            self.motion_state = ModalMotionState::default();
            self._anim_task = None;
        } else if self.motion_state.progress < 1.0 && self._anim_task.is_none() {
            self.start_enter_animation(cx);
        }
        self
    }

    /// 获取外壳的焦点句柄。
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// 获取内部实际卡片视图。
    pub fn content(&self) -> &AnyView {
        &self.content
    }

    /// 检查模态弹窗当前是否正处于退场动画过程中。
    pub fn is_closing(&self) -> bool {
        self.motion_state.is_closing
    }

    /// 请求平滑关闭弹窗。
    ///
    /// 若启用了动画且当前进度大于 0.05，将启动 220ms 加速沉降退场动画；
    /// 退场完毕后发出 [`AnimatedModalEvent::Dismissed`]。
    pub fn request_close(&mut self, cx: &mut Context<Self>) {
        if self.motion_state.is_closing {
            return;
        }

        cx.emit(AnimatedModalEvent::Closing);

        if self.animations_enabled && self.motion_state.progress > 0.05 {
            let start_p = self.motion_state.progress;
            self.motion_state.start_closing();
            let total_dur = DURATION_MODAL_LEAVE;

            self._anim_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
                let start = Instant::now();
                loop {
                    let elapsed = start.elapsed();
                    let t = (elapsed.as_secs_f32() / total_dur.as_secs_f32()).min(1.0);
                    let progress = start_p * (1.0 - t);
                    let res = this.update(cx, |this, cx| {
                        this.motion_state.progress = progress;
                        cx.notify();
                    });
                    if res.is_err() || t >= 1.0 {
                        break;
                    }
                    smol::Timer::after(std::time::Duration::from_millis(8)).await;
                }
                let _ = this.update(cx, |_, cx| {
                    cx.emit(AnimatedModalEvent::Dismissed);
                });
            }));
            return;
        }

        cx.emit(AnimatedModalEvent::Dismissed);
    }

    fn start_enter_animation(&mut self, cx: &mut Context<Self>) {
        self.motion_state = ModalMotionState::new_opening(None);
        let total_dur = DURATION_MODAL_ENTER;

        self._anim_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let start = Instant::now();
            loop {
                let elapsed = start.elapsed();
                let progress = (elapsed.as_secs_f32() / total_dur.as_secs_f32()).min(1.0);
                let res = this.update(cx, |this, cx| {
                    this.motion_state.progress = progress;
                    cx.notify();
                });
                if res.is_err() || progress >= 1.0 {
                    break;
                }
                smol::Timer::after(std::time::Duration::from_millis(8)).await;
            }
            let _ = this.update(cx, |this, cx| {
                this.motion_state.progress = 1.0;
                this._anim_task = None;
                cx.notify();
            });
        }));
    }
}

impl Render for AnimatedModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let win_size = window.viewport_size();
        let motion_values = self.motion_state.compute_values(win_size);

        let mut backdrop = modal_backdrop("animated-modal-backdrop", &t, cx)
            .opacity(motion_values.backdrop_opacity)
            .track_focus(&self.focus_handle);

        match self.alignment {
            ModalAlignment::Center => {}
            ModalAlignment::Top(offset) => {
                backdrop = backdrop.items_start().justify_center().pt(offset);
            }
        }

        backdrop = backdrop.on_action(cx.listener(|this, _: &crate::Cancel, _, cx| {
            cx.stop_propagation();
            this.request_close(cx);
        }));

        if self.dismiss_on_click_outside {
            backdrop = backdrop.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.request_close(cx);
                }),
            );
        }

        let is_closing = self.motion_state.is_closing;
        let mut card_container = div()
            .relative()
            .top(motion_values.offset.y)
            .opacity(motion_values.card_opacity)
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            });

        if is_closing {
            card_container = card_container.on_any_mouse_down(|_, _, cx| {
                cx.stop_propagation();
            });
        }

        let card_container = card_container.child(self.content.clone());

        backdrop.child(card_container)
    }
}
