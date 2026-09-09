//! 通用可复用虚拟列表组件。
//!
//! 该模块提供两部分能力：
//!
//! 1. [`ListSelection`] —— 纯逻辑的列表选择状态管理器，支持单选、Ctrl 追加/
//!    取消、Shift 范围选择以及方向键导航。它不依赖 GPUI，可独立单元测试。
//! 2. [`virtual_list`] —— 基于 GPUI `uniform_list` 的高性能虚拟列表渲染函数，
//!    只渲染可见区域的行，并自动叠加 velowork 的 [`Scrollbar`]（悬停显示）。
//!
//! 这样任何视图都能以极小代价获得「虚拟化 + 滚动条 + 多选 + 键盘导航 + 自动
//! 滚动定位」的完整列表体验，而无需重复实现渲染细节。
//!
//! 设计参考了 gpui-component 的 `List`/`VirtualList`，但完全使用底层 GPUI 构建
//! 块与 velowork 自有的滚动条实现，不引入任何第三方组件库。

use std::collections::HashSet;
use std::ops::Range;

use gpui::{
    div, px, AnyElement, Context, Div, ElementId, Entity, ParentElement, Render,
    ScrollStrategy, SharedString, Styled, UniformListScrollHandle, Window, uniform_list,
};

use crate::scrollable::{Scrollbar, ScrollbarShow};

/// 通用列表选择状态。
///
/// 行以 0 起始的 `usize` 索引标识。内部维护：
/// - `selected`：当前被选中的所有行；
/// - `anchor`：Shift 范围选择的锚点（最近一次单选/Ctrl 点击的位置）；
/// - `active`：当前「活动行」（键盘光标 / 主选择），用于自动滚动定位。
#[derive(Clone, Debug, Default)]
pub struct ListSelection {
    selected: HashSet<usize>,
    anchor: Option<usize>,
    active: Option<usize>,
}

impl ListSelection {
    /// 创建空的选择状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 指定行是否被选中。
    pub fn is_selected(&self, ix: usize) -> bool {
        self.selected.contains(&ix)
    }

    /// 当前选中的行集合（无序）。
    pub fn selected(&self) -> &HashSet<usize> {
        &self.selected
    }

    /// 当前选中的行，按升序排序后返回。
    pub fn selected_sorted(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.selected.iter().copied().collect();
        v.sort_unstable();
        v
    }

    /// 当前活动行（键盘光标 / 主选择）。
    pub fn active(&self) -> Option<usize> {
        self.active
    }

    /// 已选中的行数。
    pub fn count(&self) -> usize {
        self.selected.len()
    }

    /// 是否没有任何选择。
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// 清空所有选择。
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.active = None;
    }

    /// 单选：仅选中 `ix`，并将其设为锚点与活动行。
    pub fn select_one(&mut self, ix: usize) {
        self.selected.clear();
        self.selected.insert(ix);
        self.anchor = Some(ix);
        self.active = Some(ix);
    }

    /// Ctrl 点击：切换 `ix` 的选中状态，并把锚点移动到 `ix`。
    pub fn toggle(&mut self, ix: usize) {
        if self.selected.contains(&ix) {
            self.selected.remove(&ix);
            if self.active == Some(ix) {
                self.active = self.selected.iter().copied().min();
            }
        } else {
            self.selected.insert(ix);
            self.active = Some(ix);
        }
        self.anchor = Some(ix);
    }

    /// Shift 点击：以锚点到 `ix` 的闭区间作为新的选择集。
    ///
    /// 若尚无锚点，则退化为单选。
    pub fn select_range_to(&mut self, ix: usize) {
        let anchor = match self.anchor {
            Some(a) => a,
            None => {
                self.select_one(ix);
                return;
            }
        };
        let (lo, hi) = if anchor <= ix { (anchor, ix) } else { (ix, anchor) };
        self.selected.clear();
        for i in lo..=hi {
            self.selected.insert(i);
        }
        self.active = Some(ix);
    }

    /// 根据修饰键分派点击行为：Shift > Ctrl > 单选。
    pub fn click(&mut self, ix: usize, ctrl: bool, shift: bool) {
        if shift {
            self.select_range_to(ix);
        } else if ctrl {
            self.toggle(ix);
        } else {
            self.select_one(ix);
        }
    }

    /// 键盘上移：把活动行向上移动一格并单选。
    ///
    /// `min_ix` 为可选的下界（通常为 0）。返回新的活动行，若无法移动则 `None`。
    pub fn move_up(&mut self, min_ix: usize) -> Option<usize> {
        let cur = self.active.unwrap_or(min_ix);
        if cur > min_ix {
            let n = cur - 1;
            self.select_one(n);
            Some(n)
        } else {
            None
        }
    }

    /// 键盘下移：把活动行向下移动一格并单选。
    ///
    /// `count` 为列表总行数。返回新的活动行，若无法移动则 `None`。
    pub fn move_down(&mut self, count: usize) -> Option<usize> {
        if count == 0 {
            return None;
        }
        let n = match self.active {
            Some(c) => c + 1,
            None => 0,
        };
        if n < count {
            self.select_one(n);
            Some(n)
        } else {
            None
        }
    }

    /// 列表内容变化后，裁剪掉越界的选择/锚点/活动行。
    pub fn clamp(&mut self, count: usize) {
        self.selected.retain(|i| *i < count);
        if self.anchor.map_or(false, |a| a >= count) {
            self.anchor = None;
        }
        if self.active.map_or(false, |a| a >= count) {
            self.active = self.selected.iter().copied().min();
        }
    }
}

/// 构建一个虚拟列表元素（仅渲染可见行）并叠加纵向滚动条。
///
/// - `base_id`：用于派生内部元素 id 的基名（需在同一父级内唯一）。
/// - `view`：宿主视图实体；渲染闭包会在其上下文中被调用。
/// - `scroll_handle`：驱动虚拟化与滚动条的 [`UniformListScrollHandle`]，
///    宿主应把它保存在自身状态里，以便调用 [`UniformListScrollHandle::scroll_to_item`]
///    实现「自动滚动到当前项」。
/// - `item_count`：列表总行数。
/// - `render_range`：给定可见区间，返回对应的行元素列表。
///
/// **注意（避免视口塌陷）：**
/// 放置 `virtual_list` 的外层父容器必须具备明确的高/宽度约束（例如外层配置了 `.flex_col().h_full()`，
/// 或具备非零的绝对高度/伸缩权重）。
pub fn virtual_list<V, F>(
    base_id: impl Into<SharedString>,
    view: Entity<V>,
    scroll_handle: &UniformListScrollHandle,
    item_count: usize,
    render_range: F,
) -> Div
where
    V: Render,
    F: Fn(&mut V, Range<usize>, &mut Window, &mut Context<V>) -> Vec<AnyElement> + 'static,
{
    let base = base_id.into();
    let list_id = ElementId::Name(format!("{base}-ulist").into());
    let scrollbar_id = ElementId::Name(format!("{base}-scrollbar").into());

    let list = uniform_list(list_id, item_count, move |range, window, cx| {
        view.update(cx, |this, cx| render_range(this, range, window, cx))
    })
    .size_full()
    .track_scroll(scroll_handle);

    div()
        .relative()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .child(list)
        .child(
            div()
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .w(px(12.0))
                .child(
                    Scrollbar::vertical(scroll_handle)
                        .id(scrollbar_id)
                        .scrollbar_show(ScrollbarShow::Hover),
                ),
        )
}

/// 便捷方法：让 `UniformListScrollHandle` 滚动到指定行顶部对齐。
pub fn scroll_to_row(handle: &UniformListScrollHandle, ix: usize) {
    handle.scroll_to_item(ix, ScrollStrategy::Top);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_select_sets_anchor_and_active() {
        let mut s = ListSelection::new();
        s.select_one(3);
        assert!(s.is_selected(3));
        assert_eq!(s.active(), Some(3));
        assert_eq!(s.count(), 1);
    }

    #[test]
    fn ctrl_toggle_adds_and_removes() {
        let mut s = ListSelection::new();
        s.click(1, false, false); // 单选 1
        s.click(3, true, false); // Ctrl 追加 3
        assert!(s.is_selected(1));
        assert!(s.is_selected(3));
        assert_eq!(s.count(), 2);

        s.click(1, true, false); // Ctrl 取消 1
        assert!(!s.is_selected(1));
        assert!(s.is_selected(3));
        assert_eq!(s.count(), 1);
    }

    #[test]
    fn shift_selects_inclusive_range() {
        let mut s = ListSelection::new();
        s.click(2, false, false); // 锚点 = 2
        s.click(5, false, true); // Shift 到 5
        assert_eq!(s.selected_sorted(), vec![2, 3, 4, 5]);
        assert_eq!(s.active(), Some(5));
    }

    #[test]
    fn shift_range_works_backwards() {
        let mut s = ListSelection::new();
        s.click(5, false, false);
        s.click(2, false, true);
        assert_eq!(s.selected_sorted(), vec![2, 3, 4, 5]);
        assert_eq!(s.active(), Some(2));
    }

    #[test]
    fn shift_without_anchor_is_single() {
        let mut s = ListSelection::new();
        s.click(4, false, true);
        assert_eq!(s.selected_sorted(), vec![4]);
    }

    #[test]
    fn keyboard_navigation_moves_active() {
        let mut s = ListSelection::new();
        s.select_one(0);
        assert_eq!(s.move_down(5), Some(1));
        assert_eq!(s.move_down(5), Some(2));
        assert_eq!(s.move_up(0), Some(1));
        // 到达下界
        s.select_one(0);
        assert_eq!(s.move_up(0), None);
        // 到达上界
        s.select_one(4);
        assert_eq!(s.move_down(5), None);
    }

    #[test]
    fn clamp_drops_out_of_range() {
        let mut s = ListSelection::new();
        s.click(1, false, false);
        s.click(6, true, false);
        s.clamp(4); // 只保留 < 4
        assert!(s.is_selected(1));
        assert!(!s.is_selected(6));
    }

    #[test]
    fn clear_resets_everything() {
        let mut s = ListSelection::new();
        s.click(2, false, false);
        s.click(4, false, true);
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.active(), None);
    }
}