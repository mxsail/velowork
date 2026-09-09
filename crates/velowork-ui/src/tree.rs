//! 通用高复用性树形组件（参考 Ant Design Tree 设计）。
//!
//! 提供 `TreeNodeData`、`TreeNodeContext` 与 `Tree` 构建器，
//! 支持受控展开/收起 (`expanded_keys` / `on_expand`)、
//! 受控选中状态 (`selected_keys` / `on_select`)、自定义节点渲染 (`render_node`)，
//! 以及同级排序 (`folders_first`，默认目录排在前)。

use crate::icon::AppIcon;
use std::collections::{BTreeMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;
use velowork_core::theme::ThemeColors;

use crate::behavior::{ElementBehaviorExt, HoverBehavior, SelectedBehavior};
use crate::design::appearance::{ControlAppearance, ControlSize, ControlVariant};
use crate::file_icon::file_icon;
use crate::icon::folder_tree_icon;
use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::design::semantic::SemanticPalette;
use crate::theme::surface_bg;
use crate::tokens::{
    get_ui_density, ui_icon_std_ts, ui_space_lg, ui_space_md, ui_space_sm, ui_space_tree_indent,
    ui_space_xs, ui_text_md, ui_text_scale, ui_text_xs, RADIUS_STD,
};

/// 树节点数据结构。
#[derive(Clone, Debug)]
pub struct TreeNodeData<K = String, T = ()> {
    pub key: K,
    pub title: String,
    pub is_folder: bool,
    pub is_disabled: bool,
    pub icon: Option<AppIcon>,
    pub children: Vec<TreeNodeData<K, T>>,
    pub payload: Option<T>,
}

impl<K: Clone, T: Clone> TreeNodeData<K, T> {
    pub fn new(key: K, title: impl Into<String>, is_folder: bool) -> Self {
        Self {
            key,
            title: title.into(),
            is_folder,
            is_disabled: false,
            icon: None,
            children: Vec::new(),
            payload: None,
        }
    }

    pub fn with_icon(mut self, icon: impl Into<AppIcon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn with_children(mut self, children: Vec<TreeNodeData<K, T>>) -> Self {
        self.children = children;
        self
    }

    pub fn with_payload(mut self, payload: T) -> Self {
        self.payload = Some(payload);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.is_disabled = disabled;
        self
    }
}

/// 传入 `render_node` 自定义渲染闭包的节点上下文。
pub struct TreeNodeContext<'a, K = String, T = ()> {
    pub node: &'a TreeNodeData<K, T>,
    pub depth: usize,
    pub is_expanded: bool,
    pub is_selected: bool,
    pub is_first: bool,
    pub is_last: bool,
    pub parent_key: Option<&'a K>,
}

/// 通用 Ant Design 风格树组件构建器。
pub struct Tree<
    V: 'static,
    K: Clone + Eq + Hash + Send + Sync + 'static = String,
    T: Clone + Send + Sync + 'static = (),
> {
    id: ElementId,
    nodes: Vec<TreeNodeData<K, T>>,
    expanded_keys: HashSet<K>,
    selected_keys: HashSet<K>,
    indent_width: f32,
    /// 同级排序：目录(folder)是否排在叶子节点(会话/指令)之前。默认 `true`。
    folders_first: bool,

    on_expand: Option<Arc<dyn Fn(&mut V, &K, bool, &mut Window, &mut Context<V>) + 'static>>,
    on_select: Option<Arc<dyn Fn(&mut V, &K, bool, &mut Window, &mut Context<V>) + 'static>>,
    render_node: Option<
        Arc<
            dyn Fn(&mut V, &TreeNodeContext<K, T>, &mut Window, &mut Context<V>) -> AnyElement
                + 'static,
        >,
    >,
    render_before_children: Option<
        Arc<
            dyn Fn(&mut V, Option<&K>, usize, &mut Window, &mut Context<V>) -> Option<AnyElement>
                + 'static,
        >,
    >,
}

pub fn tree<
    V: 'static,
    K: Clone + Eq + Hash + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
>(
    id: impl Into<ElementId>,
) -> Tree<V, K, T> {
    Tree {
        id: id.into(),
        nodes: Vec::new(),
        expanded_keys: HashSet::new(),
        selected_keys: HashSet::new(),
        indent_width: 14.0,
        folders_first: true,
        on_expand: None,
        on_select: None,
        render_node: None,
        render_before_children: None,
    }
}

impl<V: 'static, K: Clone + Eq + Hash + Send + Sync + 'static, T: Clone + Send + Sync + 'static>
    Tree<V, K, T>
{
    pub fn nodes(mut self, nodes: Vec<TreeNodeData<K, T>>) -> Self {
        self.nodes = nodes;
        self
    }

    pub fn expanded_keys(mut self, keys: HashSet<K>) -> Self {
        self.expanded_keys = keys;
        self
    }

    pub fn selected_keys(mut self, keys: HashSet<K>) -> Self {
        self.selected_keys = keys;
        self
    }

    pub fn indent_width(mut self, width: f32) -> Self {
        self.indent_width = width;
        self
    }

    /// 配置同级节点排序：目录(folder)是否排在叶子节点(会话/指令)之前。
    /// 默认 `true`。关闭后按 `nodes` 传入的原始顺序排列。
    pub fn folders_first(mut self, folders_first: bool) -> Self {
        self.folders_first = folders_first;
        self
    }

    pub fn on_expand(
        mut self,
        listener: impl Fn(&mut V, &K, bool, &mut Window, &mut Context<V>) + 'static,
    ) -> Self {
        self.on_expand = Some(Arc::new(listener));
        self
    }

    pub fn on_select(
        mut self,
        listener: impl Fn(&mut V, &K, bool, &mut Window, &mut Context<V>) + 'static,
    ) -> Self {
        self.on_select = Some(Arc::new(listener));
        self
    }

    pub fn render_node(
        mut self,
        renderer: impl Fn(&mut V, &TreeNodeContext<K, T>, &mut Window, &mut Context<V>) -> AnyElement
        + 'static,
    ) -> Self {
        self.render_node = Some(Arc::new(renderer));
        self
    }

    pub fn render_before_children(
        mut self,
        renderer: impl Fn(&mut V, Option<&K>, usize, &mut Window, &mut Context<V>) -> Option<AnyElement>
        + 'static,
    ) -> Self {
        self.render_before_children = Some(Arc::new(renderer));
        self
    }

    /// 递归渲染所有树节点到 `out` 数组中。
    pub fn render_tree_nodes(
        &self,
        nodes: &[TreeNodeData<K, T>],
        depth: usize,
        parent_key: Option<&K>,
        view: &mut V,
        window: &mut Window,
        cx: &mut Context<V>,
        out: &mut Vec<AnyElement>,
    ) {
        if let Some(ref render_before) = self.render_before_children {
            if let Some(elem) = render_before(view, parent_key, depth, window, cx) {
                out.push(elem);
            }
        }

        // 同级排序：默认目录(folder)排在叶子节点(会话/指令)之前，
        // 使用稳定排序以保持各分组内部的相对顺序。
        let mut ordered: Vec<&TreeNodeData<K, T>> = nodes.iter().collect();
        if self.folders_first {
            ordered.sort_by(|a, b| b.is_folder.cmp(&a.is_folder));
        }
        let total = ordered.len();
        for (idx, &node) in ordered.iter().enumerate() {
            let is_expanded = self.expanded_keys.contains(&node.key);
            let is_selected = self.selected_keys.contains(&node.key);
            let tree_ctx = TreeNodeContext {
                node,
                depth,
                is_expanded,
                is_selected,
                is_first: idx == 0,
                is_last: idx + 1 == total,
                parent_key,
            };

            let row_element = if let Some(ref custom_render) = self.render_node {
                custom_render(view, &tree_ctx, window, cx)
            } else {
                Self::render_default_node(self, view, &tree_ctx, window, cx)
            };

            out.push(row_element);

            if node.is_folder && is_expanded {
                self.render_tree_nodes(
                    &node.children,
                    depth + 1,
                    Some(&node.key),
                    view,
                    window,
                    cx,
                    out,
                );
            }
        }
    }

    /// 构建渲染整树的 GPUI Div Element。
    pub fn render(self, view: &mut V, window: &mut Window, cx: &mut Context<V>) -> AnyElement {
        let mut out = Vec::new();
        self.render_tree_nodes(&self.nodes, 0, None, view, window, cx, &mut out);
        div()
            .id(self.id)
            .flex()
            .flex_col()
            .w_full()
            .children(out)
            .into_any_element()
    }

    /// 默认节点渲染样式。
    fn render_default_node(
        &self,
        _view: &mut V,
        ctx: &TreeNodeContext<K, T>,
        _window: &mut Window,
        cx: &mut Context<V>,
    ) -> AnyElement {
        let p = SemanticPalette::from_context(cx);
        let is_expanded = ctx.is_expanded;
        let is_selected = ctx.is_selected;
        let is_folder = ctx.node.is_folder;
        let title = ctx.node.title.clone();
        let row_h = crate::design::appearance::tree_row_height(cx);
        let indent = f32::from(ui_space_md(cx)) + ctx.depth as f32 * self.indent_width;

        let expand_arrow = if is_folder {
            div()
                .w(px(16.0))
                .h(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    (if is_expanded {
                        AppIcon::ChevronDown
                    } else {
                        AppIcon::ChevronRight
                    })
                    .size(ui_icon_std_ts(cx))
                    .text_color(p.text_secondary),
                )
        } else {
            div().w(px(16.0))
        };

        div()
            .h(row_h)
            .pl(px(indent))
            .pr(ui_space_md(cx))
            .flex()
            .items_center()
            .gap(ui_space_xs(cx))
            .cursor_pointer()
            .rounded(RADIUS_STD)
            .behavior(HoverBehavior {
                hover_bg: if is_selected {
                    p.surface_selection
                } else {
                    p.surface_hover
                },
                ..Default::default()
            })
            .behavior(SelectedBehavior {
                selected: is_selected,
                bg: p.surface_selection,
                fg: None,
            })
            .child(expand_arrow)
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(if is_selected {
                        p.text_primary
                    } else {
                        p.text_secondary
                    })
                    .child(title),
            )
            .into_any_element()
    }
}

/// Resolved appearance for tree rows (height / font / icon size).
pub fn tree_row_appearance(t: &ThemeColors, cx: &App) -> ControlAppearance {
    let scale = ui_text_scale(cx);
    let mut ap = ControlAppearance::resolve(
        ControlSize::Default,
        ControlVariant::Ghost,
        &SemanticPalette::from_theme(t),
        get_ui_density(cx),
        scale,
    );
    ap.font_size = ui_text_md(cx);
    ap.icon_size = ui_icon_std_ts(cx);
    ap
}

/// A node in the file tree.
#[derive(Default, Clone)]
pub struct FileTreeNode {
    /// Files at this level (index into files vec).
    pub files: Vec<usize>,
    /// Subdirectories.
    pub children: BTreeMap<String, FileTreeNode>,
}

/// Build a file tree from an iterator of (index, relative_path) pairs.
pub fn build_file_tree(paths: impl Iterator<Item = (usize, impl AsRef<str>)>) -> FileTreeNode {
    let mut root = FileTreeNode::default();
    for (index, path) in paths {
        let parts: Vec<&str> = path.as_ref().split(['/', '\\']).collect();
        let mut node = &mut root;
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                node.files.push(index);
            } else {
                node = node.children.entry(part.to_string()).or_default();
            }
        }
    }
    root
}

/// Base div for an expandable folder row: chevron + folder icon + name.
///
/// Caller chains `.id(...)`, `.on_click(...)`, `.when(...)` for selection,
/// and `.child(...)` for extras (e.g. scope button).
pub fn expandable_folder_row(
    node_id: &str,
    name: &str,
    depth: usize,
    is_expanded: bool,
    is_selected: bool,
    t: &ThemeColors,
    cx: &App,
) -> Div {
    let indent = px(depth as f32 * f32::from(ui_space_tree_indent(cx)));
    let ap = tree_row_appearance(t, cx);
    let icon_size = ap.icon_size;
    let font_color = if is_selected { t.text_primary } else { t.text_secondary };
    div()
        .flex()
        .items_center()
        .h(ap.height)
        .pl(indent + ui_space_sm(cx))
        .pr(ui_space_lg(cx))
        .gap(ui_space_xs(cx))
        .cursor_pointer()
        .rounded(RADIUS_STD)
        .text_color(rgb(font_color))
        .behavior(HoverBehavior {
            hover_bg: surface_bg(t.bg_hover, cx),
            hover_fg: Some(rgb(t.text_primary).into()),
            ..Default::default()
        })
        .child(
            div()
                .w(icon_size)
                .h(icon_size)
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .child(
                    (if is_expanded { AppIcon::ChevronDown } else { AppIcon::ChevronRight })
                        .size(icon_size)
                        .text_color(rgb(t.text_muted)),
                ),
        )
        .child(
            div()
                .w(icon_size)
                .h(icon_size)
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .child(folder_tree_icon(
                    node_id,
                    is_expanded,
                    depth,
                    icon_size,
                    t,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(ap.font_size)
                .overflow_hidden()
                .whitespace_nowrap()
                .child(name.to_string()),
        )
}

/// Base div for an expandable file row: file icon + filename + optional subtext.
///
/// Caller chains `.id(...)`, `.on_click(...)`, `.when(...)` for selection,
/// and `.child(...)` for extras (e.g. match count badge, diff stats).
///
/// Use `name_color` to override the filename color (e.g. for diff status).
/// Pass `None` to use the default `text_primary`.
#[allow(clippy::too_many_arguments)]
pub fn expandable_file_row(
    filename: &str,
    subtext: Option<&str>,
    depth: usize,
    name_color: Option<u32>,
    icon_path: Option<AppIcon>,
    icon_color: Option<u32>,
    is_open: bool,
    is_selected: bool,
    t: &ThemeColors,
    cx: &App,
) -> Div {
    let indent = px(depth as f32 * f32::from(ui_space_tree_indent(cx)));
    let ap = tree_row_appearance(t, cx);
    let icon_size = ap.icon_size;
    let font_color = if is_selected { t.text_primary } else { name_color.unwrap_or(t.text_secondary) };
    div()
        .relative()
        .flex()
        .items_center()
        .h(ap.height)
        .pl(indent + ui_space_sm(cx))
        .pr(ui_space_lg(cx))
        .gap(ui_space_xs(cx))
        .cursor_pointer()
        .rounded(RADIUS_STD)
        .text_color(rgb(font_color))
        .behavior(HoverBehavior {
            hover_bg: surface_bg(t.bg_hover, cx),
            hover_fg: Some(rgb(t.text_primary).into()),
            ..Default::default()
        })
        .when(is_open, |d| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(5.0))
                    .bottom(px(5.0))
                    .w(px(2.0))
                    .bg(rgb(t.border_active))
                    .rounded_r(px(1.0)),
            )
        })
        // Chevron placeholder: keeps file icons horizontally aligned with folder icons
        .child(
            div()
                .w(icon_size)
                .h(icon_size)
                .flex_shrink_0(),
        )
        // File icon container: centered, fixed size, flex_shrink_0
        .child(
            div()
                .w(icon_size)
                .h(icon_size)
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .child(match icon_path {
                    Some(path) => path
                        .size(icon_size)
                        .text_color(rgb(icon_color.unwrap_or(t.text_secondary)))
                        .into_any_element(),
                    None => file_icon(filename, t, cx)
                        .into_any_element(),
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .justify_between()
                .gap(crate::tokens::ui_space_xs(cx))
                .overflow_hidden()
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w_full()
                        .text_size(ap.font_size)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(filename.to_string()),
                )
                .when_some(subtext, |d, sub| {
                    d.child(
                        div()
                            .flex_shrink_1()
                            .min_w(px(0.0))
                            .text_size(ui_text_xs(cx))
                            .text_color(rgb(t.text_muted))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(sub.to_string()),
                    )
                }),
        )
}
