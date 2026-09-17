pub mod context_menu;
pub mod dropdown_menu;
pub mod legacy;
pub mod menu_item;
pub mod popup_menu;

pub use context_menu::*;
pub use dropdown_menu::*;
pub use legacy::*;
pub use menu_item::*;
pub use popup_menu::*;

use crate::design::appearance::ControlSize;
use crate::tokens::{get_ui_density, ui_text_scale};
use gpui::{px, App, Pixels};

/// 标准菜单项行高，与会话树节点行高严格对齐（默认 28px，随 Density 紧凑度与 UI 缩放动态缩放）。
pub fn menu_item_height(cx: &App) -> Pixels {
    let scale = ui_text_scale(cx);
    let density = get_ui_density(cx);
    px((ControlSize::Default.base_height() + density.height_offset()) * scale)
}
