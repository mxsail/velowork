//! 通用动效模态外壳组件 (`AnimatedModal`)
//!
//! 负责全屏半透明遮罩、物理弹性微过冲进场动画、加速沉降退场动画及点击遮罩退场处理。
//! 任何业务弹窗卡片 View 放入 `AnimatedModal` 中即可自动拥有统一的微过冲弹性进出场动效。

use std::time::Instant;

use gpui::prelude::*;
use gpui::*;

use crate::motion::{DURATION_MODAL_ENTER, DURATION_MODAL_LEAVE, DURATION_MODAL_MORPH, ModalMotionState};
use crate::overlay::modal::modal_backdrop;
use crate::theme::{theme, with_alpha};
use crate::design::semantic::SemanticPalette;

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
    measured_card_bounds: Option<Bounds<Pixels>>,
    morph_destination_content: Option<AnyView>,
}

impl EventEmitter<AnimatedModalEvent> for AnimatedModal {}

impl AnimatedModal {
    /// 创建一个新的动效模态外壳并自动启动入场弹性过冲动画。
    pub fn new(content: AnyView, cx: &mut Context<Self>) -> Self {
        Self::new_with_origin(content, None, cx)
    }

    /// 创建一个指定触发源点（例如鼠标点击位置）的动效模态外壳。
    pub fn new_with_origin(
        content: AnyView,
        origin: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let animations_enabled = cx
            .try_global::<ModalAnimationsEnabled>()
            .map(|g| g.0)
            .unwrap_or(true);

        let mut modal = Self {
            content,
            motion_state: if animations_enabled {
                ModalMotionState::new_opening(origin)
            } else {
                ModalMotionState::default()
            },
            _anim_task: None,
            dismiss_on_click_outside: true,
            focus_handle: cx.focus_handle(),
            animations_enabled,
            alignment: ModalAlignment::default(),
            measured_card_bounds: None,
            morph_destination_content: None,
        };

        if animations_enabled {
            modal.start_enter_animation(cx);
        }

        modal
    }

    /// 设置动画展开的源点坐标。
    pub fn with_origin(mut self, origin: Option<Point<Pixels>>) -> Self {
        self.motion_state.origin = origin;
        self
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

    /// 启动灵动岛形变收缩退场动画至指定目标几何边界（如终端录制胶囊）。
    pub fn start_morph_exit(
        &mut self,
        target_bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.start_morph_exit_with_content(target_bounds, None, cx);
    }

    /// 启动带有终点内容预览交叉渐变的形变收缩退场动画。
    pub fn start_morph_exit_with_content(
        &mut self,
        target_bounds: Bounds<Pixels>,
        destination_content: Option<AnyView>,
        cx: &mut Context<Self>,
    ) {
        if self.motion_state.is_closing {
            return;
        }

        self.morph_destination_content = destination_content;
        cx.emit(AnimatedModalEvent::Closing);

        if self.animations_enabled && self.motion_state.progress > 0.05 {
            let start_bounds = self.measured_card_bounds.unwrap_or_else(|| {
                let card_w = px(400.0);
                let card_h = px(280.0);
                let fallback_x = (target_bounds.origin.x + target_bounds.size.width / 2.0 - card_w / 2.0).max(px(20.0));
                let fallback_y = target_bounds.origin.y + px(150.0);
                Bounds::new(
                    Point::new(fallback_x, fallback_y),
                    Size::new(card_w, card_h),
                )
            });

            self.motion_state.morph_exit = Some(crate::motion::MorphExitState {
                start_bounds,
                target_bounds,
                start_radius: px(16.0),
                target_radius: crate::tokens::RADIUS_LG,
            });
            self.motion_state.start_closing();
            self.motion_state.progress = 1.0;

            let total_dur = DURATION_MODAL_MORPH;

            self._anim_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
                let start = Instant::now();
                loop {
                    let elapsed = start.elapsed();
                    let t = (elapsed.as_secs_f32() / total_dur.as_secs_f32()).min(1.0);
                    let progress = 1.0 - t;
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
        let origin = self.motion_state.origin;
        self.motion_state = ModalMotionState::new_opening(origin);
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

use crate::tokens::SPACE_LG;

impl Render for AnimatedModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        // Check if currently executing Dynamic Island morph exit
        if let Some(mv) = self.motion_state.compute_morph_values() {
            let backdrop = modal_backdrop("animated-modal-backdrop", &t, cx)
                .opacity(mv.backdrop_opacity);

            let border_color = if mv.inner_content_opacity < 0.5 {
                with_alpha(t.error, 0.35)
            } else {
                p.border_subtle
            };

            let morph_card = deferred(
                anchored()
                    .position(mv.current_bounds.origin)
                    .snap_to_window()
                    .child(
                        div()
                            .id("animated-modal-morph-card")
                            .relative()
                            .w(mv.current_bounds.size.width)
                            .h(mv.current_bounds.size.height)
                            .rounded(mv.border_radius)
                            .bg(p.surface_overlay)
                            .border_1()
                            .border_color(border_color)
                            .shadow_xl()
                            .overflow_hidden()
                            .opacity(mv.card_opacity)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .when(mv.inner_content_opacity > 0.001, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .size_full()
                                        .opacity(mv.inner_content_opacity)
                                        .child(self.content.clone()),
                                )
                            })
                            .when_some(
                                if mv.dest_content_opacity > 0.001 {
                                    self.morph_destination_content.as_ref()
                                } else {
                                    None
                                },
                                |el, dest| {
                                    el.child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .size_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .opacity(mv.dest_content_opacity)
                                            .child(dest.clone()),
                                    )
                                },
                            ),
                    ),
            );

            return div()
                .id("animated-modal-root")
                .occlude()
                .absolute()
                .inset_0()
                .size_full()
                .track_focus(&self.focus_handle)
                .key_context("AnimatedModal")
                .child(backdrop)
                .child(morph_card);
        }

        let win_size = window.viewport_size();
        let target_center = match self.alignment {
            ModalAlignment::Center => Point::new(win_size.width / 2.0, win_size.height / 2.0),
            ModalAlignment::Top(offset) => Point::new(win_size.width / 2.0, offset + px(225.0)),
        };
        let motion_values = self.motion_state.compute_values_at(win_size, target_center);

        // Layer 1: Semi-transparent backdrop with smooth ease_out_cubic dimming
        let mut backdrop = modal_backdrop("animated-modal-backdrop", &t, cx)
            .opacity(motion_values.backdrop_opacity);

        if self.dismiss_on_click_outside {
            backdrop = backdrop.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.request_close(cx);
                }),
            );
        }

        // Layer 2: Card wrapper positioned over the backdrop as a sibling layer
        let is_closing = self.motion_state.is_closing;
        let mut card_wrapper = div()
            .id("animated-modal-card-wrapper")
            .absolute()
            .inset_0()
            .size_full()
            .flex();

        match self.alignment {
            ModalAlignment::Center => {
                card_wrapper = card_wrapper.items_center().justify_center().p(SPACE_LG);
            }
            ModalAlignment::Top(offset) => {
                card_wrapper = card_wrapper.items_start().justify_center().pt(offset).px(SPACE_LG);
            }
        }

        if self.dismiss_on_click_outside {
            card_wrapper = card_wrapper.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.request_close(cx);
                }),
            );
        }

        let this_weak = cx.entity().downgrade();
        let bounds_tracker = canvas(
            move |bounds, _, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, _| {
                        this.measured_card_bounds = Some(bounds);
                    });
                }
            },
            |_, _, _, _| {},
        );

        let mut card_container = div()
            .id("animated-modal-card-container")
            .relative()
            .left(motion_values.offset.x)
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

        let card_container = card_container
            .child(bounds_tracker.absolute().inset_0())
            .child(self.content.clone());
        let card_wrapper = card_wrapper.child(card_container);

        // Sibling layer container: Root covers window, intercepts Cancel action and tracks focus
        div()
            .id("animated-modal-root")
            .occlude()
            .absolute()
            .inset_0()
            .size_full()
            .track_focus(&self.focus_handle)
            .key_context("AnimatedModal")
            .on_action(cx.listener(|this, _: &crate::Cancel, _, cx| {
                cx.stop_propagation();
                this.request_close(cx);
            }))
            .child(backdrop)
            .child(card_wrapper)
    }
}
