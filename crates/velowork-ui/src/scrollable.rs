use std::{
    cell::Cell,
    ops::Deref,
    panic::Location,
    rc::Rc,
    time::{Duration, Instant},
};

// 引入 GPUI 全部 trait 与常用的类型/宏（与 velowork-ui 其他组件一致）。
use gpui::*;
// 同时引入 IntoElement derive 宏（与 trait 同名，分属不同命名空间）。
use gpui::IntoElement;
use gpui::Interactivity;
use gpui::prelude::FluentBuilder;

use crate::design::semantic::SemanticPalette;
use crate::theme::{ThemeColors, theme, with_alpha};

/// 滚动条总宽度（THUMB_ACTIVE_INSET * 2 + THUMB_ACTIVE_WIDTH）
const WIDTH: Pixels = px(4. * 2. + 8.);
const MIN_THUMB_SIZE: f32 = 48.;

const THUMB_WIDTH: Pixels = px(6.);
const THUMB_RADIUS: Pixels = px(6. / 2.);
const THUMB_INSET: Pixels = px(4.);

const THUMB_ACTIVE_WIDTH: Pixels = px(8.);
const THUMB_ACTIVE_RADIUS: Pixels = px(8. / 2.);
const THUMB_ACTIVE_INSET: Pixels = px(4.);

const FADE_OUT_DURATION: f32 = 0.45;
const FADE_OUT_DELAY: f32 = 0.15;

pub use velowork_core::types::ScrollbarShow;

/// 可被滚动条读取/设置偏移量的句柄。
pub trait ScrollbarHandle: 'static {
    /// 当前偏移量。
    fn offset(&self) -> Point<Pixels>;
    /// 设置偏移量。
    fn set_offset(&self, offset: Point<Pixels>);
    /// 内容总尺寸（含 padding）。
    fn content_size(&self) -> Size<Pixels>;
    /// 开始拖拽滚动条滑块时调用。
    fn start_drag(&self) {}
    /// 结束拖拽滚动条滑块时调用。
    fn end_drag(&self) {}
}

impl ScrollbarHandle for ScrollHandle {
    fn offset(&self) -> Point<Pixels> {
        self.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.set_offset(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        (self.max_offset() + self.bounds().size.into()).into()
    }
}

impl ScrollbarHandle for UniformListScrollHandle {
    fn offset(&self) -> Point<Pixels> {
        self.0.borrow().base_handle.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.0.borrow_mut().base_handle.set_offset(offset)
    }

    fn content_size(&self) -> Size<Pixels> {
        let base_handle = &self.0.borrow().base_handle;
        (base_handle.max_offset() + base_handle.bounds().size.into()).into()
    }
}

impl ScrollbarHandle for ListState {
    fn offset(&self) -> Point<Pixels> {
        self.scroll_px_offset_for_scrollbar()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.set_offset_from_scrollbar(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        self.viewport_bounds().size + self.max_offset_for_scrollbar().into()
    }

    fn start_drag(&self) {
        self.scrollbar_drag_started();
    }

    fn end_drag(&self) {
        self.scrollbar_drag_ended();
    }
}

// ── 滚动条内部状态 ──────────────────────────────────────────────────

#[doc(hidden)]
#[derive(Debug, Clone)]
struct ScrollbarState(Rc<Cell<ScrollbarStateInner>>);

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
struct ScrollbarStateInner {
    hovered_axis: Option<Axis>,
    hovered_on_thumb: Option<Axis>,
    content_hovered: bool,
    dragged_axis: Option<Axis>,
    drag_pos: Point<Pixels>,
    last_scroll_offset: Point<Pixels>,
    last_scroll_time: Option<Instant>,
    // 最近一次更新偏移量时间
    last_update: Instant,
    idle_timer_scheduled: bool,
}

impl Default for ScrollbarState {
    fn default() -> Self {
        Self(Rc::new(Cell::new(ScrollbarStateInner {
            hovered_axis: None,
            hovered_on_thumb: None,
            content_hovered: false,
            dragged_axis: None,
            drag_pos: point(px(0.), px(0.)),
            last_scroll_offset: point(px(0.), px(0.)),
            last_scroll_time: None,
            last_update: Instant::now(),
            idle_timer_scheduled: false,
        })))
    }
}

impl Deref for ScrollbarState {
    type Target = Rc<Cell<ScrollbarStateInner>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ScrollbarStateInner {
    fn with_drag_pos(&self, axis: Axis, pos: Point<Pixels>) -> Self {
        let mut state = *self;
        if matches!(axis, Axis::Vertical) {
            state.drag_pos.y = pos.y;
        } else {
            state.drag_pos.x = pos.x;
        }

        state.dragged_axis = Some(axis);
        state
    }

    fn with_unset_drag_pos(&self) -> Self {
        let mut state = *self;
        state.dragged_axis = None;
        state
    }

    fn with_content_hovered(&self, hovered: bool) -> Self {
        let mut state = *self;
        state.content_hovered = hovered;
        if hovered {
            state.last_scroll_time = Some(Instant::now());
        }
        state
    }

    fn with_hovered(&self, axis: Option<Axis>) -> Self {
        let mut state = *self;
        state.hovered_axis = axis;
        if axis.is_some() {
            state.last_scroll_time = Some(Instant::now());
        }
        state
    }

    fn with_hovered_on_thumb(&self, axis: Option<Axis>) -> Self {
        let mut state = *self;
        state.hovered_on_thumb = axis;
        if self.is_scrollbar_visible() {
            if axis.is_some() {
                state.last_scroll_time = Some(Instant::now());
            }
        }
        state
    }

    fn with_last_scroll(
        &self,
        last_scroll_offset: Point<Pixels>,
        last_scroll_time: Option<Instant>,
    ) -> Self {
        let mut state = *self;
        state.last_scroll_offset = last_scroll_offset;
        state.last_scroll_time = last_scroll_time;
        state
    }

    fn with_last_scroll_time(&self, t: Option<Instant>) -> Self {
        let mut state = *self;
        state.last_scroll_time = t;
        state
    }

    fn with_last_update(&self, t: Instant) -> Self {
        let mut state = *self;
        state.last_update = t;
        state
    }

    fn with_idle_timer_scheduled(&self, scheduled: bool) -> Self {
        let mut state = *self;
        state.idle_timer_scheduled = scheduled;
        state
    }

    fn is_scrollbar_visible(&self) -> bool {
        // 拖拽中或光标悬停在内容区域/滚动条上时显现
        if self.dragged_axis.is_some() || self.content_hovered || self.hovered_axis.is_some() {
            return true;
        }

        if let Some(last_time) = self.last_scroll_time {
            let elapsed = Instant::now().duration_since(last_time).as_secs_f32();
            elapsed < FADE_OUT_DURATION
        } else {
            false
        }
    }
}

/// 滚动条轴向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarAxis {
    /// 纵向滚动条。
    Vertical,
    /// 横向滚动条。
    Horizontal,
    /// 同时显示纵向与横向滚动条。
    Both,
}

impl From<Axis> for ScrollbarAxis {
    fn from(axis: Axis) -> Self {
        match axis {
            Axis::Vertical => Self::Vertical,
            Axis::Horizontal => Self::Horizontal,
        }
    }
}

impl ScrollbarAxis {
    /// 是否为纵向。
    #[inline]
    pub fn is_vertical(&self) -> bool {
        matches!(self, Self::Vertical)
    }

    /// 是否为横向。
    #[inline]
    pub fn is_horizontal(&self) -> bool {
        matches!(self, Self::Horizontal)
    }

    /// 是否为双向。
    #[inline]
    pub fn is_both(&self) -> bool {
        matches!(self, Self::Both)
    }

    /// 是否包含纵向。
    #[inline]
    pub fn has_vertical(&self) -> bool {
        matches!(self, Self::Vertical | Self::Both)
    }

    /// 是否包含横向。
    #[inline]
    pub fn has_horizontal(&self) -> bool {
        matches!(self, Self::Horizontal | Self::Both)
    }

    #[inline]
    fn all(&self) -> Vec<Axis> {
        match self {
            Self::Vertical => vec![Axis::Vertical],
            Self::Horizontal => vec![Axis::Horizontal],
            // 横向在前，纵向为主轴（纵向不可见时保留右侧边距）。
            Self::Both => vec![Axis::Horizontal, Axis::Vertical],
        }
    }
}

/// 滚动区域或 uniform-list 的滚动条控件。
pub struct Scrollbar {
    pub(crate) id: ElementId,
    axis: ScrollbarAxis,
    scrollbar_show: Option<ScrollbarShow>,
    scroll_handle: Rc<dyn ScrollbarHandle>,
    scroll_size: Option<Size<Pixels>>,
    /// 拖拽时的最大刷新帧率，默认 120 FPS。
    max_fps: usize,
}

impl Scrollbar {
    /// 创建一个双向滚动条。
    #[track_caller]
    pub fn new<H: ScrollbarHandle + Clone>(scroll_handle: &H) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::CodeLocation(*caller),
            axis: ScrollbarAxis::Both,
            scrollbar_show: None,
            scroll_handle: Rc::new(scroll_handle.clone()),
            max_fps: 120,
            scroll_size: None,
        }
    }

    /// 仅横向滚动条。
    #[track_caller]
    pub fn horizontal<H: ScrollbarHandle + Clone>(scroll_handle: &H) -> Self {
        Self::new(scroll_handle).axis(ScrollbarAxis::Horizontal)
    }

    /// 仅纵向滚动条。
    #[track_caller]
    pub fn vertical<H: ScrollbarHandle + Clone>(scroll_handle: &H) -> Self {
        Self::new(scroll_handle).axis(ScrollbarAxis::Vertical)
    }

    /// 设置特定的元素 id（默认使用 `Location::caller`）。
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// 设置滚动条显隐模式 [`ScrollbarShow`]，未设置时使用默认 `Scrolling`。
    pub fn scrollbar_show(mut self, scrollbar_show: ScrollbarShow) -> Self {
        self.scrollbar_show = Some(scrollbar_show);
        self
    }

    /// 设置内容区域特殊尺寸，默认从 `scroll_handle` 同步。
    pub fn scroll_size(mut self, scroll_size: Size<Pixels>) -> Self {
        self.scroll_size = Some(scroll_size);
        self
    }

    /// 设置滚动条轴向。
    pub fn axis(mut self, axis: impl Into<ScrollbarAxis>) -> Self {
        self.axis = axis.into();
        self
    }

    /// 设置拖拽时的最大帧率（30..120）。
    #[allow(dead_code)]
    pub(crate) fn max_fps(mut self, max_fps: usize) -> Self {
        self.max_fps = max_fps.clamp(30, 120);
        self
    }

    /// 滚动条宽度。
    #[allow(dead_code)]
    pub(crate) const fn width() -> Pixels {
        WIDTH
    }

    fn style_for_active(cx: &App) -> (Hsla, Hsla, Hsla, Pixels, Pixels, Pixels) {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        (
            Hsla { a: 0.7, ..p.text_primary },
            gpui::transparent_black(),
            gpui::transparent_black(),
            THUMB_ACTIVE_WIDTH,
            THUMB_ACTIVE_INSET,
            THUMB_ACTIVE_RADIUS,
        )
    }

    fn style_for_hovered_thumb(cx: &App) -> (Hsla, Hsla, Hsla, Pixels, Pixels, Pixels) {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        (
            Hsla { a: 0.6, ..p.text_primary },
            gpui::transparent_black(),
            gpui::transparent_black(),
            THUMB_ACTIVE_WIDTH,
            THUMB_ACTIVE_INSET,
            THUMB_ACTIVE_RADIUS,
        )
    }

    fn style_for_hovered_bar(cx: &App) -> (Hsla, Hsla, Hsla, Pixels, Pixels, Pixels) {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        (
            Hsla { a: 0.45, ..p.text_muted },
            gpui::transparent_black(),
            gpui::transparent_black(),
            THUMB_ACTIVE_WIDTH,
            THUMB_ACTIVE_INSET,
            THUMB_ACTIVE_RADIUS,
        )
    }

    fn style_for_normal(&self, cx: &App) -> (Hsla, Hsla, Hsla, Pixels, Pixels, Pixels) {
        let scrollbar_show = self.scrollbar_show.unwrap_or(ScrollbarShow::Hover);
        let (width, inset, radius) = match scrollbar_show {
            ScrollbarShow::Scrolling => (THUMB_WIDTH, THUMB_INSET, THUMB_RADIUS),
            _ => (THUMB_ACTIVE_WIDTH, THUMB_ACTIVE_INSET, THUMB_ACTIVE_RADIUS),
        };

        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        (
            Hsla { a: 0.35, ..p.text_muted },
            gpui::transparent_black(),
            gpui::transparent_black(),
            width,
            inset,
            radius,
        )
    }

    fn style_for_idle(&self, cx: &App) -> (Hsla, Hsla, Hsla, Pixels, Pixels, Pixels) {
        let _ = theme(cx);
        (
            gpui::transparent_black(),
            gpui::transparent_black(),
            gpui::transparent_black(),
            THUMB_WIDTH,
            THUMB_INSET,
            THUMB_RADIUS,
        )
    }
}

impl IntoElement for Scrollbar {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[doc(hidden)]
pub struct PrepaintState {
    hitbox: Hitbox,
    scrollbar_state: ScrollbarState,
    states: Vec<AxisPrepaintState>,
}

#[doc(hidden)]
#[allow(dead_code)]
pub struct AxisPrepaintState {
    axis: Axis,
    bar_hitbox: Hitbox,
    bounds: Bounds<Pixels>,
    radius: Pixels,
    bg: Hsla,
    border: Hsla,
    thumb_bounds: Bounds<Pixels>,
    // 实际渲染的滑块区域
    thumb_fill_bounds: Bounds<Pixels>,
    thumb_bg: Hsla,
    scroll_size: Pixels,
    container_size: Pixels,
    thumb_size: Pixels,
    raw_thumb_length: Pixels,
    track_travel_range: Pixels,
    inset: Pixels,
    margin_end: Pixels,
}

impl Element for Scrollbar {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<gpui::ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.inset = Edges {
            top: px(0.0).into(),
            left: px(0.0).into(),
            bottom: px(0.0).into(),
            right: px(0.0).into(),
        };
        style.flex_grow = 1.0;
        style.flex_shrink = 1.0;
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();

        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let hitbox = window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.insert_hitbox(bounds, HitboxBehavior::Normal)
        });

        let state = window
            .use_state(cx, |_, _| ScrollbarState::default())
            .read(cx)
            .clone();

        let mut states = vec![];
        let mut has_both = self.axis.is_both();
        let scroll_size = self
            .scroll_size
            .unwrap_or(self.scroll_handle.content_size());

        for axis in self.axis.all().into_iter() {
            let is_vertical = matches!(axis, Axis::Vertical);
            let (scroll_area_size, container_size, scroll_position) = if is_vertical {
                (
                    scroll_size.height,
                    hitbox.size.height,
                    self.scroll_handle.offset().y,
                )
            } else {
                (
                    scroll_size.width,
                    hitbox.size.width,
                    self.scroll_handle.offset().x,
                )
            };

            // 横向滚动条避免与纵向滚动条重叠。
            let margin_end = if has_both && !is_vertical {
                WIDTH
            } else {
                px(0.)
            };

            // 内容小于容器时隐藏滚动条。
            if scroll_area_size <= container_size {
                has_both = false;
                continue;
            }

            let raw_thumb_length =
                (container_size / scroll_area_size * container_size).max(px(MIN_THUMB_SIZE));
            let track_travel_range = (container_size - margin_end - raw_thumb_length).max(px(1.0));
            let thumb_start = -(scroll_position / (scroll_area_size - container_size)
                * track_travel_range);
            let thumb_end = (thumb_start + raw_thumb_length).min(container_size - margin_end);

            let horiz_bar_h = px(6.0);
            let horiz_thumb_h = px(4.0);

            let bounds = Bounds {
                origin: if is_vertical {
                    point(hitbox.origin.x + hitbox.size.width - WIDTH, hitbox.origin.y)
                } else {
                    point(
                        hitbox.origin.x,
                        hitbox.origin.y + hitbox.size.height - horiz_bar_h,
                    )
                },
                size: gpui::Size {
                    width: if is_vertical {
                        WIDTH
                    } else {
                        hitbox.size.width
                    },
                    height: if is_vertical {
                        hitbox.size.height
                    } else {
                        horiz_bar_h
                    },
                },
            };

            let scrollbar_show = self.scrollbar_show.unwrap_or(ScrollbarShow::Scrolling);
            let is_always_to_show = scrollbar_show.is_always();
            let is_hover_to_show = scrollbar_show.is_hover();
            let is_content_hovered = state.get().content_hovered;
            let is_hovered_on_bar = state.get().hovered_axis == Some(axis);
            let is_hovered_on_thumb = state.get().hovered_on_thumb == Some(axis);
            let is_offset_changed = state.get().last_scroll_offset != self.scroll_handle.offset();

            let (thumb_bg, bar_bg, bar_border, thumb_width, inset, radius) =
                if state.get().dragged_axis == Some(axis) {
                    Self::style_for_active(cx)
                } else if (is_hover_to_show || is_content_hovered) && (is_hovered_on_bar || is_hovered_on_thumb || is_content_hovered) {
                    if is_hovered_on_thumb {
                        Self::style_for_hovered_thumb(cx)
                    } else if is_hovered_on_bar {
                        Self::style_for_hovered_bar(cx)
                    } else {
                        self.style_for_normal(cx)
                    }
                } else if is_offset_changed {
                    self.style_for_normal(cx)
                } else if is_always_to_show {
                    if is_hovered_on_thumb {
                        Self::style_for_hovered_thumb(cx)
                    } else {
                        Self::style_for_hovered_bar(cx)
                    }
                } else {
                    let mut idle_state = self.style_for_idle(cx);
                    if let Some(last_time) = state.get().last_scroll_time {
                        let elapsed = Instant::now().duration_since(last_time).as_secs_f32();
                        let fade_delay = if is_hover_to_show { 0.1 } else { FADE_OUT_DELAY };
                        let fade_duration = if is_hover_to_show { 0.4 } else { FADE_OUT_DURATION };

                        if is_hovered_on_bar || is_content_hovered {
                            state.set(state.get().with_last_scroll_time(Some(Instant::now())));
                            idle_state = if is_hovered_on_thumb {
                                Self::style_for_hovered_thumb(cx)
                            } else if is_hovered_on_bar {
                                Self::style_for_hovered_bar(cx)
                            } else {
                                self.style_for_normal(cx)
                            };
                        } else if elapsed < fade_delay {
                            idle_state.0 = with_alpha(theme(cx).text_muted, 0.4);

                            if !state.get().idle_timer_scheduled {
                                let state = state.clone();
                                state.set(state.get().with_idle_timer_scheduled(true));
                                let current_view = window.current_view();
                                let next_delay = Duration::from_secs_f32(fade_delay - elapsed);
                                window
                                    .spawn(cx, async move |cx| {
                                        cx.background_executor().timer(next_delay).await;
                                        state.set(state.get().with_idle_timer_scheduled(false));
                                        cx.update(|_, cx| cx.notify(current_view)).ok();
                                    })
                                    .detach();
                            }
                        } else if elapsed < fade_duration {
                            let progress = ((elapsed - fade_delay) / (fade_duration - fade_delay)).clamp(0.0, 1.0);
                            let opacity = (1.0 - progress) * (1.0 - progress);
                            idle_state.0 = theme(cx).scrollbar_hover_color(opacity);

                            if !state.get().idle_timer_scheduled {
                                let state = state.clone();
                                state.set(state.get().with_idle_timer_scheduled(true));
                                let current_view = window.current_view();
                                window
                                    .spawn(cx, async move |cx| {
                                        cx.background_executor().timer(Duration::from_millis(16)).await;
                                        state.set(state.get().with_idle_timer_scheduled(false));
                                        cx.update(|_, cx| cx.notify(current_view)).ok();
                                    })
                                    .detach();
                            }
                        }
                    }

                    idle_state
                };

            // 滑块可点击区域
            let thumb_length = (thumb_end - thumb_start - inset * 2.).max(px(8.0));
            let thumb_bounds = if is_vertical {
                Bounds::from_anchor_and_size(
                    Anchor::TopRight,
                    bounds.top_right() + point(-inset, inset + thumb_start),
                    size(WIDTH, thumb_length),
                )
            } else {
                Bounds::from_anchor_and_size(
                    Anchor::BottomLeft,
                    bounds.bottom_left() + point(inset + thumb_start, -px(1.0)),
                    size(thumb_length, horiz_bar_h),
                )
            };

            // 滑块实际渲染区域
            let thumb_fill_bounds = if is_vertical {
                Bounds::from_anchor_and_size(
                    Anchor::TopRight,
                    bounds.top_right() + point(-inset, inset + thumb_start),
                    size(thumb_width, thumb_length),
                )
            } else {
                Bounds::from_anchor_and_size(
                    Anchor::BottomLeft,
                    bounds.bottom_left() + point(inset + thumb_start, -px(1.0)),
                    size(thumb_length, horiz_thumb_h),
                )
            };

            let bar_hitbox = window.with_content_mask(Some(ContentMask { bounds }), |window| {
                window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal)
            });

            states.push(AxisPrepaintState {
                axis,
                bar_hitbox,
                bounds,
                radius,
                bg: bar_bg,
                border: bar_border,
                thumb_bounds,
                thumb_fill_bounds,
                thumb_bg,
                scroll_size: scroll_area_size,
                container_size,
                thumb_size: thumb_length,
                raw_thumb_length,
                track_travel_range,
                inset,
                margin_end,
            })
        }

        PrepaintState {
            hitbox,
            states,
            scrollbar_state: state,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let scrollbar_state = &prepaint.scrollbar_state;
        let scrollbar_show = self.scrollbar_show.unwrap_or(ScrollbarShow::Scrolling);
        let view_id = window.current_view();
        let hitbox_bounds = prepaint.hitbox.bounds;
        let is_visible =
            scrollbar_state.get().is_scrollbar_visible() || scrollbar_show.is_always();
        let is_hover_to_show = scrollbar_show.is_hover();

        // 偏移变化更新 last_scroll_time
        if self.scroll_handle.offset() != scrollbar_state.get().last_scroll_offset {
            scrollbar_state.set(
                scrollbar_state
                    .get()
                    .with_last_scroll(self.scroll_handle.offset(), Some(Instant::now())),
            );
            cx.notify(view_id);
        }

        window.with_content_mask(
            Some(ContentMask {
                bounds: hitbox_bounds,
            }),
            |window| {
                for state in prepaint.states.iter() {
                    let axis = state.axis;
                    let radius = state.radius;
                    let bounds = state.bounds;
                    let thumb_bounds = state.thumb_bounds;
                    let scroll_area_size = state.scroll_size;
                    let container_size = state.container_size;
                    let is_vertical = matches!(axis, Axis::Vertical);

                    let raw_thumb_length = state.raw_thumb_length;
                    let track_travel_range = state.track_travel_range;
                    let inset = state.inset;

                    window.set_cursor_style(CursorStyle::default(), &state.bar_hitbox);

                    window.paint_layer(hitbox_bounds, |cx| {
                        cx.paint_quad(fill(state.bounds, state.bg));

                        cx.paint_quad(PaintQuad {
                            bounds,
                            corner_radii: (0.).into(),
                            background: gpui::transparent_black().into(),
                            border_widths: if is_vertical {
                                Edges {
                                    top: px(0.),
                                    right: px(0.),
                                    bottom: px(0.),
                                    left: px(0.),
                                }
                            } else {
                                Edges {
                                    top: px(0.),
                                    right: px(0.),
                                    bottom: px(0.),
                                    left: px(0.),
                                }
                            },
                            border_color: state.border,
                            border_style: gpui::BorderStyle::default(),
                        });

                        cx.paint_quad(
                            fill(state.thumb_fill_bounds, state.thumb_bg).corner_radii(radius),
                        );
                    });

                    window.on_mouse_event({
                        let state = scrollbar_state.clone();
                        let scroll_handle = self.scroll_handle.clone();

                        move |event: &ScrollWheelEvent, phase, _, cx| {
                            if phase.bubble() && hitbox_bounds.contains(&event.position) {
                                if scroll_handle.offset() != state.get().last_scroll_offset {
                                    state.set(state.get().with_last_scroll(
                                        scroll_handle.offset(),
                                        Some(Instant::now()),
                                    ));
                                    cx.notify(view_id);
                                }
                            }
                        }
                    });

                    let safe_range = (-scroll_area_size + container_size)..px(0.);

                    if is_hover_to_show || is_visible {
                        window.on_mouse_event({
                            let state = scrollbar_state.clone();
                            let scroll_handle = self.scroll_handle.clone();

                            move |event: &MouseDownEvent, phase, _, cx| {
                                if phase.bubble() && bounds.contains(&event.position) {
                                    cx.stop_propagation();

                                    if thumb_bounds.contains(&event.position) {
                                        // 点击滑块，记录拖拽起点
                                        let pos = event.position - thumb_bounds.origin;

                                        scroll_handle.start_drag();
                                        state.set(state.get().with_drag_pos(axis, pos));

                                        cx.notify(view_id);
                                    } else {
                                        // 点击滚动条空白区域，跳转到该位置（滑块居中于点击点）
                                        let offset = scroll_handle.offset();
                                        let target_center = if is_vertical {
                                            event.position.y - raw_thumb_length / 2. - bounds.origin.y - inset
                                        } else {
                                            event.position.x - raw_thumb_length / 2. - bounds.origin.x - inset
                                        };
                                        let percentage = (f32::from(target_center) / f32::from(track_travel_range)).clamp(0.0, 1.0);

                                        if is_vertical {
                                            scroll_handle.set_offset(point(
                                                offset.x,
                                                (-(scroll_area_size - container_size) * percentage)
                                                    .clamp(safe_range.start, safe_range.end),
                                            ));
                                        } else {
                                            scroll_handle.set_offset(point(
                                                (-(scroll_area_size - container_size) * percentage)
                                                    .clamp(safe_range.start, safe_range.end),
                                                offset.y,
                                            ));
                                        }
                                    }
                                }
                            }
                        });
                    }

                    window.on_mouse_event({
                        let scroll_handle = self.scroll_handle.clone();
                        let state = scrollbar_state.clone();

                        move |event: &MouseMoveEvent, _, _, cx| {
                            let mut notify = false;
                            let in_content = hitbox_bounds.contains(&event.position);
                            let in_bar = bounds.contains(&event.position);
                            let in_thumb = thumb_bounds.contains(&event.position);

                            if in_content != state.get().content_hovered {
                                state.set(state.get().with_content_hovered(in_content));
                                notify = true;
                            }

                            if in_bar {
                                if state.get().hovered_axis != Some(axis) {
                                    state.set(state.get().with_hovered(Some(axis)));
                                    notify = true;
                                } else {
                                    state.set(state.get().with_last_scroll_time(Some(Instant::now())));
                                }
                            } else {
                                if state.get().hovered_axis == Some(axis) {
                                    state.set(state.get().with_hovered(None));
                                    notify = true;
                                }
                            }

                            if in_thumb {
                                if state.get().hovered_on_thumb != Some(axis) {
                                    state.set(state.get().with_hovered_on_thumb(Some(axis)));
                                    notify = true;
                                }
                            } else {
                                if state.get().hovered_on_thumb == Some(axis) {
                                    state.set(state.get().with_hovered_on_thumb(None));
                                    notify = true;
                                }
                            }

                            // 拖拽移动滑块位置（精准 1:1 跟手）
                            if state.get().dragged_axis == Some(axis) && event.dragging() {
                                cx.stop_propagation();

                                let drag_pos = state.get().drag_pos;

                                let target_start = if is_vertical {
                                    event.position.y - drag_pos.y - bounds.origin.y - inset
                                } else {
                                    event.position.x - drag_pos.x - bounds.origin.x - inset
                                };

                                let percentage = (f32::from(target_start) / f32::from(track_travel_range)).clamp(0.0, 1.0);

                                let offset = if is_vertical {
                                    point(
                                        scroll_handle.offset().x,
                                        (-(scroll_area_size - container_size) * percentage)
                                            .clamp(safe_range.start, safe_range.end),
                                    )
                                } else {
                                    point(
                                        (-(scroll_area_size - container_size) * percentage)
                                            .clamp(safe_range.start, safe_range.end),
                                        scroll_handle.offset().y,
                                    )
                                };

                                if scroll_handle.offset() != offset {
                                    scroll_handle.set_offset(offset);
                                    state.set(state.get().with_last_update(Instant::now()));
                                    notify = true;
                                }
                            }

                            if notify {
                                cx.notify(view_id);
                            }
                        }
                    });

                    window.on_mouse_event({
                        let state = scrollbar_state.clone();
                        let scroll_handle = self.scroll_handle.clone();

                        move |_event: &MouseUpEvent, phase, _, cx| {
                            if phase.bubble() {
                                scroll_handle.end_drag();
                                state.set(state.get().with_unset_drag_pos());
                                cx.notify(view_id);
                            }
                        }
                    });
                }
            },
        );
    }
}

/// 为可交互元素增加滚动条能力的 trait。
pub trait ScrollableElement:
    gpui::InteractiveElement + gpui::Styled + gpui::ParentElement + Element
{
    /// 添加滚动条。
    #[track_caller]
    fn scrollbar<H: ScrollbarHandle + Clone>(
        self,
        scroll_handle: &H,
        axis: impl Into<ScrollbarAxis>,
    ) -> Self {
        self.child(ScrollbarLayer {
            id: "scrollbar_layer".into(),
            axis: axis.into(),
            scroll_handle: Rc::new(scroll_handle.clone()),
        })
    }

    /// 添加纵向滚动条。
    #[track_caller]
    fn vertical_scrollbar<H: ScrollbarHandle + Clone>(self, scroll_handle: &H) -> Self {
        self.scrollbar(scroll_handle, ScrollbarAxis::Vertical)
    }
    /// 添加横向滚动条。
    #[track_caller]
    fn horizontal_scrollbar<H: ScrollbarHandle + Clone>(self, scroll_handle: &H) -> Self {
        self.scrollbar(scroll_handle, ScrollbarAxis::Horizontal)
    }

    /// 等价于 `overflow_scroll` + 双向滚动条。
    #[track_caller]
    fn overflow_scrollbar(self) -> Scrollable<Self> {
        Scrollable::new(self, ScrollbarAxis::Both)
    }

    /// 等价于 `overflow_x_scroll` + 横向滚动条。
    #[track_caller]
    fn overflow_x_scrollbar(self) -> Scrollable<Self> {
        Scrollable::new(self, ScrollbarAxis::Horizontal)
    }

    /// 等价于 `overflow_y_scroll` + 纵向滚动条。
    #[track_caller]
    fn overflow_y_scrollbar(self) -> Scrollable<Self> {
        Scrollable::new(self, ScrollbarAxis::Vertical)
    }
}

/// 为元素包裹滚动条的包装器。
#[derive(IntoElement)]
pub struct Scrollable<E: gpui::InteractiveElement + gpui::Styled + gpui::ParentElement + Element> {
    id: ElementId,
    element: E,
    axis: ScrollbarAxis,
}

impl<E> Scrollable<E>
where
    E: gpui::InteractiveElement + gpui::Styled + gpui::ParentElement + Element,
{
    #[track_caller]
    fn new(element: E, axis: impl Into<ScrollbarAxis>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::CodeLocation(*caller),
            element,
            axis: axis.into(),
        }
    }
}

impl<E> gpui::Styled for Scrollable<E>
where
    E: gpui::InteractiveElement + gpui::Styled + gpui::ParentElement + Element,
{
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.element.style()
    }
}

impl<E> gpui::ParentElement for Scrollable<E>
where
    E: gpui::InteractiveElement + gpui::Styled + gpui::ParentElement + Element,
{
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.element.extend(elements)
    }
}

impl gpui::InteractiveElement for Scrollable<gpui::Div> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.element.interactivity()
    }
}

impl gpui::InteractiveElement for Scrollable<gpui::Stateful<gpui::Div>> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.element.interactivity()
    }
}

impl<E> gpui::RenderOnce for Scrollable<E>
where
    E: gpui::InteractiveElement + gpui::Styled + gpui::ParentElement + Element + 'static,
{
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scroll_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, _| ScrollHandle::default())
            .read(cx)
            .clone();

        // 继承元素样式中的尺寸。
        let elem_size = self.element.style().size.clone();
        let mut container = gpui::div()
            .id(self.id)
            .size_full()
            .relative();
        container.style().size = elem_size;

        container
            .child(
                gpui::div()
                    .id("scroll-area")
                    .flex()
                    .size_full()
                    .track_scroll(&scroll_handle)
                    .map(|this| match self.axis {
                        ScrollbarAxis::Vertical => this.flex_col().overflow_y_scroll(),
                        ScrollbarAxis::Horizontal => this.flex_row().overflow_x_scroll(),
                        ScrollbarAxis::Both => this.overflow_scroll(),
                    })
                    .child(
                        self.element
                            .size_auto()
                            .map(|this| match self.axis {
                                ScrollbarAxis::Vertical => this.w_full(),
                                ScrollbarAxis::Horizontal => this.h_full(),
                                ScrollbarAxis::Both => this,
                            }),
                    ),
            )
            .child(render_scrollbar(
                "scrollbar",
                &scroll_handle,
                self.axis,
                window,
                cx,
            ))
    }
}

impl ScrollableElement for gpui::Div {}
impl<E> ScrollableElement for gpui::Stateful<E>
where
    E: gpui::ParentElement + gpui::Styled + Element,
    Self: gpui::InteractiveElement,
{
}

#[derive(IntoElement)]
struct ScrollbarLayer<H: ScrollbarHandle + Clone> {
    id: ElementId,
    axis: ScrollbarAxis,
    scroll_handle: Rc<H>,
}

impl<H> gpui::RenderOnce for ScrollbarLayer<H>
where
    H: ScrollbarHandle + Clone + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        render_scrollbar(self.id, self.scroll_handle.as_ref(), self.axis, window, cx)
    }
}

#[inline]
#[track_caller]
fn render_scrollbar<H: ScrollbarHandle + Clone>(
    id: impl Into<ElementId>,
    scroll_handle: &H,
    axis: ScrollbarAxis,
    window: &mut Window,
    cx: &mut App,
) -> gpui::Div {
    // 取色器拾取元素时不渲染滚动条，便于选中底层元素。
    let is_inspector_picking = window.is_inspector_picking(cx);
    if is_inspector_picking {
        return gpui::div();
    }

    match axis {
        ScrollbarAxis::Vertical => gpui::div()
            .absolute()
            .top_0()
            .right_0()
            .bottom_0()
            .w(px(12.0))
            .child(
                Scrollbar::new(scroll_handle)
                    .id(id)
                    .axis(axis)
                    .scrollbar_show(ScrollbarShow::Hover),
            ),
        ScrollbarAxis::Horizontal => gpui::div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(12.0))
            .child(
                Scrollbar::new(scroll_handle)
                    .id(id)
                    .axis(axis)
                    .scrollbar_show(ScrollbarShow::Hover),
            ),
        ScrollbarAxis::Both => gpui::div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .child(
                Scrollbar::new(scroll_handle)
                    .id(id)
                    .axis(axis)
                    .scrollbar_show(ScrollbarShow::Hover),
            ),
    }
}

// 主题色辅助：带透明度的滚动条悬停色。
trait ScrollbarColorExt {
    fn scrollbar_hover_color(&self, alpha: f32) -> Hsla;
}

impl ScrollbarColorExt for ThemeColors {
    fn scrollbar_hover_color(&self, alpha: f32) -> Hsla {
        let rgba = rgb(self.text_secondary);
        Hsla::from(Rgba {
            a: alpha,
            ..rgba
        })
    }
}
