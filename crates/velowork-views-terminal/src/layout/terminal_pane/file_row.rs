use gpui::prelude::FluentBuilder;
use velowork_ui::icon::AppIcon;
use gpui::*;
use velowork_core::theme::ThemeColors;
use velowork_ui::h_flex;
use velowork_ui::input::Input;
use velowork_ui::theme::{theme, surface_bg_t, with_alpha};
use velowork_ui::tokens::{ui_text, ui_text_md, SPACE_SM, SPACE_MD, SPACE_LG, SPACE_XS, RADIUS_STD};
use velowork_ui::{
    ControlAppearance, ControlSize, ControlVariant, HoverBehavior, SemanticPalette,
    StatefulElementBehaviorExt,
};

use super::sftp_panel::{InlineRenameState, SftpFile};

/// SFTP 行的设计系统外观（Default 档）：统一图标尺寸与行几何，对齐会话树。
fn sftp_row_appearance(t: &ThemeColors, cx: &App) -> ControlAppearance {
    let scale = velowork_ui::tokens::ui_text_scale(cx);
    let mut ap = ControlAppearance::resolve(
        ControlSize::Default,
        ControlVariant::Ghost,
        &SemanticPalette::from_theme(t),
        velowork_ui::tokens::get_ui_density(cx),
        scale,
    );
    ap.font_size = velowork_ui::tokens::ui_text_md(cx);
    ap.icon_size = px(14.5 * scale);
    ap
}

// ---------------------------------------------------------------------------
// Column widths — shared between header, file rows and parent row.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct ColumnWidths {
    pub name: f32,
    pub perm: f32,
    pub owner: f32,
    pub size: f32,
    pub mtime: f32,
}

const COL_SPACER_PX: f32 = 6.0;

fn col_spacer() -> Div {
    div().w(px(COL_SPACER_PX))
}

fn file_text_size(cx: &App) -> Pixels {
    ui_text_md(cx)
}

// ---------------------------------------------------------------------------
// DisplayFile — pre-computed display strings (avoids re-formatting every frame).
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct DisplayFile {
    pub file: SftpFile,
    pub icon_path: AppIcon,
    pub size_text: SharedString,
    pub date_text: SharedString,
    pub perm_text: SharedString,
    pub owner_text: SharedString,
}

impl DisplayFile {
    pub fn from_sftp_file(file: SftpFile) -> Self {
        let icon_path = if file.is_dir {
            AppIcon::Folder
        } else {
            AppIcon::File
        };
        Self {
            size_text: format_size(file.size).into(),
            date_text: format_time(file.mtime).into(),
            perm_text: format_mode(file.mode).into(),
            owner_text: format_owner(&file).into(),
            file,
            icon_path,
        }
    }
}

// ---------------------------------------------------------------------------
// parent_row — the synthetic ".." entry (row index 0).
//
// Returns a `Stateful<Div>` with layout and styling applied.  Callers chain
// `.on_click()` / `.on_mouse_down()` for event handling (same pattern as
// `list_row()` / `selectable_list_item()` in velowork-ui).
// ---------------------------------------------------------------------------

pub fn parent_row(
    cols: ColumnWidths,
    selected: bool,
    row_h: Pixels,
    cx: &mut App,
) -> Stateful<Div> {
    let t = theme(cx);
    let p = SemanticPalette::from_theme(&t);

    h_flex()
        .id(ElementId::Name("sftp-parent-row".into()))
        .px(SPACE_LG)
        .h(row_h)
        .items_center()
        .cursor_pointer()
        // Unselected rows are transparent so the panel's single translucent
        // `bg_primary` layer shows through (avoids double-stacking the alpha
        // when `bg_opacity` < 1). Selected rows use the hover background plus a
        // 1px accent border — same treatment as the session/tunnel/qc tree rows —
        // so the selected state is visually distinct from plain hover only by the
        // border (no separate selection color). The synthetic ".." parent row has
        // no index, so the alternating-row stripe (only applied to odd file rows)
        // never applies here.
        .rounded(RADIUS_STD)
        .border_1()
        .border_color(with_alpha(0x00000000, 0.0))
        .bg(if selected {
            surface_bg_t(t.bg_hover, &t)
        } else {
            hsla(0.0, 0.0, 0.0, 0.0)
        })
        .when(selected, |d| d.border_color(rgb(t.border_active)))
        .when(!selected, |d| {
            d.stateful_behavior(HoverBehavior {
                hover_bg: surface_bg_t(t.bg_hover, &t),
                ..Default::default()
            })
        })
        .child(
            h_flex()
                .w(px(cols.name))
                .overflow_hidden()
                .gap(SPACE_SM)
                .child(
                    AppIcon::ArrowUp
                        .size(sftp_row_appearance(&t, cx).icon_size)
                        .flex_shrink_0()
                        .text_color(p.text_secondary),
                )
                .child(
                    div()
                        .text_size(file_text_size(cx))
                        .text_color(p.text_secondary)
                        .child("../"),
                ),
        )
        .child(col_spacer())
        .child(div().w(px(cols.perm)).overflow_hidden())
        .child(col_spacer())
        .child(div().w(px(cols.owner)).overflow_hidden())
        .child(col_spacer())
        .child(div().w(px(cols.size)).overflow_hidden())
        .child(col_spacer())
        .child(div().flex_1().min_w(px(cols.mtime)).overflow_hidden())
        .child(col_spacer())
}

// ---------------------------------------------------------------------------
// file_row — a regular file or directory entry.
//
// Returns a `Stateful<Div>` with layout and styling applied.  Callers chain
// `.on_click()` / `.on_mouse_down()` for event handling.
// ---------------------------------------------------------------------------

pub fn file_row(
    row_idx: usize,
    display: &DisplayFile,
    cols: ColumnWidths,
    selected: bool,
    inline_rename: Option<&InlineRenameState>,
    row_h: Pixels,
    cx: &mut App,
) -> Stateful<Div> {
    let t = theme(cx);
    let p = SemanticPalette::from_theme(&t);
    // 行高跟随字体行高（与 SimpleInput 单行高度 line_height + px(4.0) 一致），
    // 随用户字体设置缩放，避免调字号后错位或重命名输入框顶部留白。
    // `row_h` 由调用方用 window.line_height() 预先算好（引用不能逃逸进 virtual_list）。

    let alternating_bg = velowork_app_core::settings::settings(cx).alternating_row_bg;
    let row_bg = if selected {
        surface_bg_t(t.bg_hover, &t)
    } else if alternating_bg && row_idx % 2 == 1 {
        with_alpha(t.text_primary, 0.03)
    } else {
        hsla(0.0, 0.0, 0.0, 0.0)
    };

    h_flex()
        .id(ElementId::Name(format!("sftp-file-row-{}", row_idx).into()))
        .px(SPACE_LG)
        .h(row_h)
        .items_center()
        .cursor_pointer()
        // Unselected rows are transparent so the panel's single translucent
        // `bg_primary` layer shows through (avoids double-stacking the alpha
        // when `bg_opacity` < 1). Selected rows use the hover background plus a
        // 1px accent border — same treatment as the session/tunnel/qc tree rows —
        // so the selected state is visually distinct from plain hover only by the
        // border (no separate selection color). The synthetic ".." parent row has
        // no index, so the alternating-row stripe (only applied to odd file rows)
        // never applies here.
        .rounded(RADIUS_STD)
        .border_1()
        .border_color(with_alpha(0x00000000, 0.0))
        .bg(row_bg)
        .when(selected, |d| d.border_color(rgb(t.border_active)))
        .when(!selected, |d| {
            d.stateful_behavior(HoverBehavior {
                hover_bg: surface_bg_t(t.bg_hover, &t),
                ..Default::default()
            })
        })
        .child(render_name_cell(
            display,
            cols.name,
            inline_rename,
            row_h,
            cx,
            &t,
        ))
        .child(col_spacer())
        .child(
            div()
                .w(px(cols.perm))
                .overflow_hidden()
                .text_size(file_text_size(cx))
                .text_color(p.text_muted)
                .child(display.perm_text.clone()),
        )
        .child(col_spacer())
        .child(
            div()
                .w(px(cols.owner))
                .overflow_hidden()
                .text_size(file_text_size(cx))
                .text_color(p.text_muted)
                .child(display.owner_text.clone()),
        )
        .child(col_spacer())
        .child(
            div()
                .w(px(cols.size))
                .overflow_hidden()
                .text_right()
                .text_size(file_text_size(cx))
                .text_color(p.text_muted)
                .child(display.size_text.clone()),
        )
        .child(col_spacer())
        .child(
            div()
                .flex_1()
                .min_w(px(cols.mtime))
                .overflow_hidden()
                .text_size(file_text_size(cx))
                .text_color(p.text_muted)
                .child(display.date_text.clone()),
        )
        .child(col_spacer())
}

fn render_name_cell(
    display: &DisplayFile,
    name_w: f32,
    inline_rename: Option<&InlineRenameState>,
    row_h: Pixels,
    cx: &mut App,
    t: &ThemeColors,
) -> AnyElement {
    let ap = sftp_row_appearance(t, cx);
    let p = SemanticPalette::from_theme(t);
    if let Some(rename) = inline_rename {
        let mut cell = h_flex()
            .w(px(name_w))
            .gap(SPACE_SM)
            .items_center()
            .relative() // 建立相对定位基准，使绝对定位的错误提示框相对于本单元格定位
                .child(
                    display
                        .icon_path
                        .size(ap.icon_size)
                        .flex_shrink_0()
                        .text_color(p.text_secondary),
                );
        if let Some(ref name_input) = rename.name_input {
            cell = cell.child(
                div()
                    .flex_1()
                    .h(row_h)
                    .flex()
                    .items_center()
                    .child(Input::new(name_input)),
            );
        }

        // 如果存在错误信息，挂载绝对定位浮层
        if let Some(err_msg) = &rename.error {
        cell = cell.child(
            deferred(
                div()
                    .absolute()
                    .top(row_h + px(2.0))
                    .left(px(20.0))
                    .px(SPACE_MD)
                    .py(SPACE_XS)
                    .bg(surface_bg_t(t.bg_secondary, &t))
                    .border_1()
                    .border_color(p.status_error)
                    .rounded(RADIUS_STD)
                    .shadow_md()
                    .text_size(ui_text(11.0, cx))
                    .text_color(p.status_error)
                    .whitespace_nowrap()
                    .child(err_msg.to_string()),
            )
            .with_priority(1), // 👈 GPUI 中替代 z_index 的高优先级绘制提升
        );
        }

        cell.into_any_element()
    } else {
        h_flex()
            .w(px(name_w))
            .overflow_hidden()
            .gap(SPACE_SM)
                .child(
                    display
                        .icon_path
                        .size(ap.icon_size)
                        .flex_shrink_0()
                        .text_color(p.text_secondary),
                )
            .child(
                div()
                    .text_size(file_text_size(cx))
                    .text_color(p.text_primary)
                    .child(display.file.name.clone()),
            )
            .into_any_element()
    }
}

// ---------------------------------------------------------------------------
// blank_row — the empty row at the bottom of the list.
// ---------------------------------------------------------------------------

pub fn blank_row(cx: &mut App) -> Stateful<Div> {
    let t = theme(cx);

    div()
        .id(ElementId::Name("sftp-blank-row".into()))
        .h(sftp_row_appearance(&t, cx).height)
        .w_full()
        // Transparent so the panel's single translucent `bg_primary` layer
        // shows through (consistent with the file/parent rows above).
        .bg(hsla(0.0, 0.0, 0.0, 0.0))
}

// ---------------------------------------------------------------------------
// Formatting helpers (shared across the SFTP module).
// ---------------------------------------------------------------------------

pub fn format_time(timestamp: u32) -> String {
    let epoch = timestamp as i64;
    let days_since_epoch = epoch / 86400;
    let seconds_in_day = epoch % 86400;

    let mut year = 1970;
    let mut days = days_since_epoch;

    loop {
        let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if is_leap { 366 } else { 365 };
        if days >= days_in_year {
            days -= days_in_year;
            year += 1;
        } else {
            break;
        }
    }

    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let month_days = if is_leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    let mut d = days;
    for &m_days in month_days.iter() {
        if d >= m_days {
            d -= m_days;
            month += 1;
        } else {
            break;
        }
    }
    let day = d + 1;

    let hour = seconds_in_day / 3600;
    let minute = (seconds_in_day % 3600) / 60;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        year, month, day, hour, minute
    )
}

pub fn format_mode(mode: u32) -> String {
    russh_sftp::protocol::FilePermissions::from(mode).to_string()
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

pub fn format_owner(file: &SftpFile) -> String {
    match (file.user.clone(), file.group.clone()) {
        (Some(u), Some(g)) => format!("{}:{}", u, g),
        _ => match (file.uid, file.gid) {
            (Some(u), Some(g)) => format!("{}:{}", u, g),
            (Some(u), None) => format!("{}", u),
            _ => "-".to_string(),
        },
    }
}
