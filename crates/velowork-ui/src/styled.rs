use gpui::*;

/// 返回一个横向 flex 布局的 [`Div`](gpui::Div)（主轴水平、交叉轴居中）。
#[inline(always)]
pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

/// 返回一个纵向 flex 布局的 [`Div`](gpui::Div)（主轴垂直）。
#[inline(always)]
pub fn v_flex() -> Div {
    div().flex().flex_col()
}
