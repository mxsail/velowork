//! Folder icon for tree / outline nodes.
//!
//! The open/closed state is reflected directly by the glyph (`folder.svg` ↔
//! `folder_open.svg`) — no cross-fade animation. Switching instantly avoids the
//! icon flashing (open↔closed) when an inline rename row mounts/unmounts and the
//! normal row remounts, which would otherwise replay a fade-from-zero.
//! The icon's hue is derived from the nesting `depth` (via the active theme's
//! folder-color palette) so different levels are visually distinguishable —
//! every color comes from the theme, never hard-coded.

use gpui::*;
use crate::icon::AppIcon;
use velowork_core::theme::ThemeColors;

/// Render a folder icon whose open/closed state is reflected directly by the
/// glyph (`folder.svg` ↔ `folder_open.svg`). The glyph switches immediately
/// when `is_expanded` changes (no animation), so it never flashes on rename
/// commit / row remount.
///
/// * `id` — a unique, stable identifier for this folder node (kept for API
///   compatibility and potential future use).
/// * `is_expanded` — whether the folder is currently open.
/// * `depth` — nesting level, used to pick a distinct theme hue per level.
/// * `size` — icon size in pixels.
/// * `t` — active theme colors.
pub fn folder_tree_icon(
    _id: &str,
    is_expanded: bool,
    depth: usize,
    size: Pixels,
    t: &ThemeColors,
) -> impl IntoElement {
    let color = folder_color_for_depth(t, depth);
    let open_opacity = if is_expanded { 1.0 } else { 0.0 };
    folder_icon_glyphs(color, size, open_opacity).into_any_element()
}

/// Build the folder icon's two stacked glyphs (`folder.svg` + `folder_open.svg`)
/// at the given `open_opacity` (the closed glyph uses `1.0 - open_opacity`).
fn folder_icon_glyphs(color: u32, size: Pixels, open_opacity: f32) -> Div {
    let closed_opacity = 1.0 - open_opacity;
    div()
        .relative()
        .flex_shrink_0()
        .size(size)
        .child(
                AppIcon::Folder
                    .size(size)
                    .text_color(rgb(color))
                    .opacity(closed_opacity)
                    .absolute(),
            )
            .child(
                AppIcon::FolderOpen
                    .size(size)
                    .text_color(rgb(color))
                    .opacity(open_opacity)
                    .absolute(),
        )
}

/// Pick a distinct, theme-driven hue for each nesting level so that deeply
/// nested folders read as a different color from their parents. All values
/// come from the active theme's folder palette — nothing is hard-coded.
fn folder_color_for_depth(t: &ThemeColors, depth: usize) -> u32 {
    const PALETTE: &[fn(&ThemeColors) -> u32] = &[
        |t| t.folder_default,
        |t| t.folder_blue,
        |t| t.folder_cyan,
        |t| t.folder_teal,
        |t| t.folder_green,
        |t| t.folder_lime,
        |t| t.folder_yellow,
        |t| t.folder_orange,
        |t| t.folder_red,
        |t| t.folder_pink,
        |t| t.folder_purple,
        |t| t.folder_indigo,
    ];
    PALETTE[depth % PALETTE.len()](t)
}
