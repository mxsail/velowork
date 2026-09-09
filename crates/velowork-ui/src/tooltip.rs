use gpui::prelude::*;
use gpui::*;
use std::sync::Arc;

use crate::design::semantic::SemanticPalette;
use crate::theme::theme;
use crate::tokens::{ui_space_md, ui_space_sm, ui_text};

#[allow(dead_code)]
const ARROW_SIZE: f32 = 8.0;
const ARROW_INSET: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum TooltipDirection {
    #[default]
    Top,
    TopLeft,
    TopRight,
    Bottom,
    BottomLeft,
    BottomRight,
    Left,
    LeftTop,
    LeftBottom,
    Right,
    RightTop,
    RightBottom,
}

use TooltipDirection::*;

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum ArrowAnchor {
    TopLeft(Pixels),
    TopRight(Pixels),
    TopCenter,
    BottomLeft(Pixels),
    BottomRight(Pixels),
    BottomCenter,
    LeftTop(Pixels),
    LeftBottom(Pixels),
    LeftCenter,
    RightTop(Pixels),
    RightBottom(Pixels),
    RightCenter,
}

#[derive(Clone)]
enum TooltipContent {
    Text(SharedString),
    Element(Arc<dyn Fn(&mut Window, &mut App) -> AnyElement>),
}

#[derive(Clone)]
pub struct Tooltip {
    content: TooltipContent,
    action: Option<Arc<dyn Action>>,
    direction: TooltipDirection,
    max_width: Pixels,
    custom_padding: bool,
    bare: bool,
}

impl Tooltip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            content: TooltipContent::Text(text.into()),
            action: None,
            direction: TooltipDirection::default(),
            max_width: px(360.0),
            custom_padding: false,
            bare: false,
        }
    }

    pub fn element(builder: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
        Self {
            content: TooltipContent::Element(Arc::new(builder)),
            action: None,
            direction: TooltipDirection::default(),
            max_width: px(560.0),
            custom_padding: true,
            bare: false,
        }
    }

    /// Mark this tooltip as a bare container without wrapper border, shadow, or background.
    /// Used when the contained element is a fully self-styled card (e.g. `terminal_preview_card`).
    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    pub fn max_width(mut self, max_width: impl Into<Pixels>) -> Self {
        self.max_width = max_width.into();
        self
    }

    pub fn direction(mut self, direction: TooltipDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn action(mut self, action: &dyn Action, _context: Option<&str>) -> Self {
        self.action = Some(Arc::from(action.boxed_clone()));
        self
    }

    /// 根据方向计算默认箭头锚点
    fn arrow_anchor(&self) -> ArrowAnchor {
        match self.direction {
            Bottom => ArrowAnchor::TopCenter,
            BottomLeft => ArrowAnchor::TopLeft(px(ARROW_INSET)),
            BottomRight => ArrowAnchor::TopRight(px(ARROW_INSET)),
            Top => ArrowAnchor::BottomCenter,
            TopLeft => ArrowAnchor::BottomLeft(px(ARROW_INSET)),
            TopRight => ArrowAnchor::BottomRight(px(ARROW_INSET)),
            Right => ArrowAnchor::LeftCenter,
            RightTop => ArrowAnchor::LeftTop(px(ARROW_INSET)),
            RightBottom => ArrowAnchor::LeftBottom(px(ARROW_INSET)),
            Left => ArrowAnchor::RightCenter,
            LeftTop => ArrowAnchor::RightTop(px(ARROW_INSET)),
            LeftBottom => ArrowAnchor::RightBottom(px(ARROW_INSET)),
        }
    }

    /// 构建渲染气泡 Div
    pub fn render_bubble(&self, window: &mut Window, cx: &mut App) -> Div {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        // 背景跟随全局 bg_opacity 透明度（与浮动面板统一使用 bg_panel）。
        let bg = p.surface_overlay;
        let text_color = p.text_primary;

        let is_custom = matches!(&self.content, TooltipContent::Element(_)) && self.custom_padding;

        let inner = match &self.content {
            TooltipContent::Text(text) => {
                let formatted = format_soft_break_text(text.as_ref());
                div()
                    .flex_1()
                    .min_w_0()
                    .max_h(px(240.0))
                    .overflow_y_hidden()
                    .whitespace_normal()
                    .child(formatted)
            }
            TooltipContent::Element(builder) => div()
                .flex_1()
                .min_w_0()
                .child(builder(window, cx)),
        };

        if self.bare {
            return div()
                .relative()
                .flex()
                .flex_row()
                .items_center()
                .max_w(self.max_width)
                .child(inner);
        }

        let keybinding: Option<SharedString> = self.action.as_ref().and_then(|action| {
            let bindings = window.bindings_for_action(action.as_ref());
            bindings.first().map(|binding| {
                binding
                    .keystrokes()
                    .iter()
                    .map(|ks| ks.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .into()
            })
        });

        let mut bubble = div()
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .bg(bg)
            .border_1()
            .border_color(p.border_subtle)
            .text_color(text_color)
            .rounded(px(6.0))
            .shadow_lg()
            .max_w(self.max_width)
            .when(!is_custom, |d| {
                d.gap(px(8.0))
                    .px(ui_space_md(cx))
                    .py(ui_space_sm(cx))
                    .text_size(ui_text(13.0, cx))
            })
            .child(inner);

        if let Some(kbd) = keybinding {
            bubble = bubble.child(
                div()
                    .flex_shrink_0()
                    .text_size(ui_text(11.0, cx))
                    .text_color(rgb(t.text_muted))
                    .child(kbd),
            );
        }

        if let Some(arrow) = build_arrow(bg, self.arrow_anchor()) {
            bubble = bubble.child(arrow);
        }

        bubble
    }
}

/// 在路径/长标识符常见分隔符后插入零宽空格（\u{200B}），使得 GPUI 排版引擎在无常规空格的长路径、URL 或代码标识符中可以自适应折行，避免溢出边界。
pub fn format_soft_break_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for ch in s.chars() {
        out.push(ch);
        if matches!(ch, '/' | '\\' | ':' | '.' | '_' | '-' | '?' | '&' | '=') {
            out.push('\u{200B}');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format_soft_break_text;

    #[std::prelude::v1::test]
    fn test_format_soft_break_text() {
        let path = "/var/log/velowork_test.log";
        let formatted = format_soft_break_text(path);
        assert!(formatted.contains('\u{200B}'));
        let without_zwsp: String = formatted.chars().filter(|&c| c != '\u{200B}').collect();
        assert_eq!(without_zwsp, path);
    }
}

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_bubble(window, cx)
    }
}

/// 绘制箭头（Ant Design 风格）
///
/// 当前隐藏小尖角，后期再优化处理。保留实现以便恢复。
fn build_arrow(bg: Hsla, arrow_anchor: ArrowAnchor) -> Option<Div> {
    let _ = (bg, arrow_anchor);
    None
}
