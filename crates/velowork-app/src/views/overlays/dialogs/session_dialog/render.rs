//! SSH「新建/编辑会话」对话框渲染：左侧状态导航 + 右侧可折叠卡片 + 底部操作栏。
//!
//! 所有交互通过 `SessionPanel` 的方法回调实现（闭包捕获 `&mut SessionPanel`），
//! 渲染本身只读取 [`SessionDialogModel`](crate::views::overlays::dialogs::session_dialog::model::SessionDialogModel)
//! 的缓存（摘要 / 校验 / 变更集），符合 GPUI 状态驱动模型。

use std::collections::HashMap;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui::{InteractiveElement, StatefulInteractiveElement};
use velowork_i18n::i18n;
use velowork_state::{CompressionType, ProxyType, SessionTreeNode, SshAuthType, StrictHostKey};
use velowork_ui::button::{Button, button, button_primary};
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::form::{FormLayout, form_item};
use velowork_ui::icon::AppIcon;
use velowork_ui::icon_button::{icon_button, FocusableIconButtonExt};
use velowork_ui::input::{Input, InputState};
use velowork_ui::radio::{RadioGroup, RadioMode, RadioOption};
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::select::{Select, SelectState};
use velowork_ui::theme::{surface_bg_t, theme, with_alpha, ThemeColors};
use velowork_ui::tokens::{
    ICON_SM, ICON_STD, RADIUS_CARD, RADIUS_LG, RADIUS_STD, RADIUS_XS,
    SCROLL_BOTTOM_SPACER_H, SPACE_CARD_GAP, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS, ui_space_md, ui_text_lg, ui_text_md, ui_text_ms, ui_text_sm,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::h_flex;
use velowork_workspace::folder_path::parse_and_validate_folder_path;
use velowork_workspace::stores::GlobalSessionStore;

use crate::views::overlays::dialogs::session_dialog::model::{
    AlgorithmCategory, SessionDialogModel, algo_candidates,
};
use crate::views::overlays::dialogs::session_dialog::section::{SshSection, visible_sections};
use crate::views::overlays::dialogs::session_dialog::validation::{FieldId, ValidationSeverity};
use crate::views::panels::session_panel::SessionPanel;

/// 终端增强开关（对应 `SshSession` 的布尔字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalToggle {
    ShellIntegration,
    BracketedPaste,
    Osc52,
    TrueColor,
}

/// 渲染整个对话框卡片（不含遮罩层；遮罩由 `render_dialog_overlay` 提供）。
pub fn render_session_dialog(
    model: &mut SessionDialogModel,
    panel: Entity<SessionPanel>,
    panel_focus: &FocusHandle,
    test_status: &crate::views::panels::session_panel::TestConnectionStatus,
    active_project_id: Option<String>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme_colors = theme(cx);
    let t = &theme_colors;
    let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
    let search = model.ui.nav_search.clone();
    let matched = crate::views::overlays::dialogs::session_dialog::SectionRegistry::with_builtins()
        .match_search(&search);
    // 定位式搜索：始终渲染全部配置项，仅高亮匹配项并自动滚动到第一个匹配项。
    let show_all = true;
    let visible_sec_list = visible_sections(model.config.protocol);

    // 1. 处理左侧导航点击带来的整卡定位
    let scroll_to_target = model.ui.pending_scroll.get();
    if let Some(target) = scroll_to_target {
        model.ui.pending_scroll.set(None);
        let y = model.section_top_offset(target);
        model.ui.scroll_handle.set_offset(point(px(0.0), px(-y)));
        model.ui.active_section = target;
    } else if let Some(fh) = model.ui.pending_scroll_focus_handle.take() {
        // 处理 Tab 聚焦带来的控件级视口自适应平滑滚动
        model.scroll_handle_into_view(&fh, cx);
    } else if let Some(focused_sec) = model.focused_section(window, cx) {
        // 2. 焦点驱动高亮：若当前有表单项或导航项获焦且位于视口内，左侧导航紧随焦点所在分组，绝不被滚动位覆写
        model.ui.active_section = focused_sec;
    } else {
        // 3. 根据用户手动滚动位置，反向推导并更新左侧导航激活态 (Scroll Spy)
        let heights = model.ui.card_heights.borrow();
        let current_scroll_y = -f32::from(model.ui.scroll_handle.offset().y);
        let gap = f32::from(SPACE_CARD_GAP);

        if !heights.is_empty() {
            let mut y_acc = 0.0;
            let mut current_active = None;

            for &section in visible_sec_list {
                if !show_all && !matched.contains(&section) {
                    continue;
                }
                let is_expanded = model.is_expanded(section);
                let card_h = heights.get(&section).copied().unwrap_or_else(|| {
                    if is_expanded {
                        default_expanded_height(section, model.config.protocol)
                    } else {
                        52.0
                    }
                });

                // 当滚动距离超过该卡片顶部 - 40px 偏移阈值时激活该分类
                if current_scroll_y + 40.0 >= y_acc {
                    current_active = Some(section);
                }
                y_acc += card_h + gap;
            }

            if let Some(active) = current_active {
                model.ui.active_section = active;
            }
        }
    }

    // ---- 左侧导航 ----
    let nav = div()
        .id("session-dialog-nav")
        .w(px(220.0))
        .flex_shrink_0()
        .h_full()
        .border_r_1()
        .border_color(p.border_subtle)
        .overflow_hidden()
        .flex()
        .flex_col()
        .min_h_0()
        .child(
            div()
                .p(SPACE_LG)
                .child(Input::new(&model.inputs.nav_search).search(true)),
        )
        .child(
            div()
                .id(ElementId::Name("dialog-nav-list".into()))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .overflow_y_scroll()
                .flex_1()
                .min_h_0()
                .px(SPACE_SM)
                .py(ui_space_md(cx))
                .children(visible_sec_list.iter().filter_map(|&section| {
                    if !show_all && !matched.contains(&section) {
                        return None;
                    }
                    let is_active = model.ui.active_section == section;
                    let is_match = matched.contains(&section);
                    let has_error = model.ui.validation.section_severity(section)
                        == Some(ValidationSeverity::Error);
                    let nav_focus = model.focus.nav.get(&section).cloned()?;
                    let nav_map: HashMap<SshSection, FocusHandle> = model
                        .focus
                        .nav
                        .iter()
                        .map(|(k, v)| (*k, v.clone()))
                        .collect();
                    let order: Vec<SshSection> = visible_sec_list
                        .iter()
                        .copied()
                        .filter(|s| show_all || matched.contains(s))
                        .collect();
                    let idx = order.iter().position(|s| *s == section).unwrap_or(0);
                    let label = i18n!(cx, section_title_key(section));
                    let is_focused = nav_focus.is_focused(window);
                    Some(
                        div()
                            .id(ElementId::Name(format!("nav-{}", section.index()).into()))
                            .track_focus(&nav_focus)
                            .cursor_pointer()
                            .px(SPACE_MD)
                            .py(SPACE_MD)
                            .rounded(RADIUS_STD)
                            .border_1()
                            .border_color(if is_focused {
                                p.border_active
                            } else if is_active {
                                p.surface_accent.opacity(0.3)
                            } else {
                                with_alpha(0x00000000, 0.0)
                            })
                            .flex()
                            .items_center()
                            .gap(SPACE_MD)
                            .text_size(ui_text_md(cx))
                            .when(is_active, |d| {
                                d.bg(if is_focused {
                                    p.surface_accent.opacity(0.20)
                                } else {
                                    p.surface_accent.opacity(0.14)
                                })
                                .text_color(p.text_primary)
                            })
                            .when(!is_active, |d| {
                                d.text_color(if is_match {
                                    p.text_secondary
                                } else {
                                    p.text_muted
                                })
                                .hover(|s| s.bg(p.surface_hover))
                            })
                            .on_click({
                                let panel = panel.clone();
                                let nav_focus = nav_focus.clone();
                                move |_: &ClickEvent, window, cx| {
                                    nav_focus.focus(window, cx);
                                    panel.update(cx, |this, cx| {
                                        this.dialog_nav_to(section, cx);
                                        cx.notify();
                                    });
                                }
                            })
                            .on_key_down({
                                let panel = panel.clone();
                                let order = order.clone();
                                let nav_map = nav_map.clone();
                                move |event: &KeyDownEvent, window, cx| {
                                    match event.keystroke.key.as_str() {
                                        // 左侧导航内：方向键在可见分组间移动焦点并联动卡片贴顶滚动
                                        "up" => {
                                            let prev = order[(idx + order.len() - 1) % order.len()];
                                            panel.update(cx, |this, cx| {
                                                this.dialog_nav_to(prev, cx);
                                                cx.notify();
                                            });
                                            if let Some(fh) = nav_map.get(&prev) {
                                                fh.focus(window, cx);
                                            }
                                            cx.stop_propagation();
                                        }
                                        "down" => {
                                            let next = order[(idx + 1) % order.len()];
                                            panel.update(cx, |this, cx| {
                                                this.dialog_nav_to(next, cx);
                                                cx.notify();
                                            });
                                            if let Some(fh) = nav_map.get(&next) {
                                                fh.focus(window, cx);
                                            }
                                            cx.stop_propagation();
                                        }
                                        _ => {}
                                    }
                                }
                            })
                            .child(div().flex_1().min_w(px(0.0)).truncate().child(label))
                            .children(if has_error {
                                Some(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(t.error)))
                            } else {
                                None
                            }),
                    )
                })),
        );

    let card_heights_rc = model.ui.card_heights.clone();

    let mut right = div()
        .id("session-dialog-right")
        .flex()
        .flex_col()
        .gap(SPACE_CARD_GAP)
        .overflow_y_scroll()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .px(SPACE_XL)
        .py(SPACE_MD)
        .track_scroll(&model.ui.scroll_handle)
        .flex_1();

    for &section in visible_sec_list {
        if !show_all && !matched.contains(&section) {
            continue;
        }

        let section_card = render_section_card(
            section,
            model,
            panel.clone(),
            &active_project_id,
            t,
            cx,
            window,
        );
        let height_setter = card_heights_rc.clone();

        let card_wrapper = div().relative().child(section_card).child(
            canvas(
                move |bounds, _, _| {
                    // 实时测量并更新卡片经 layout 计算后的真实高度
                    height_setter
                        .borrow_mut()
                        .insert(section, f32::from(bounds.size.height));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        );

        right = right.child(card_wrapper);
    }

    // 底部弹性安全间距：确保最后几个分类卡片点击导航时也能完整滚动置顶，避免触底提前截断导致高亮回跳
    right = right.child(div().h(SCROLL_BOTTOM_SPACER_H).flex_shrink_0());

    // 头部标题（带 Dirty 标记）
    let title = super::session_dialog_title(model.editing_id.is_some(), model.config.protocol, cx);
    let dirty_mark = if model.is_dirty() { " *" } else { "" };

    // ---- 底部操作栏 ----
    let can_save = model.is_valid();
    let save_label = if model.editing_id.is_some() {
        i18n!(cx, "ssh.dialog.save_changes")
    } else {
        i18n!(cx, "common.save")
    };

    let is_ssh_session = model.config.protocol == velowork_state::SessionProtocol::Ssh;

    let footer = div()
        .h(px(48.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .px(SPACE_LG)
        .rounded_b(RADIUS_LG)
        .border_t_1()
        .border_color(p.border_subtle)
        .child(
            div()
                .flex()
                .items_center()
                .gap(SPACE_MD)
                .when(is_ssh_session, |d| {
                    d.child(render_dialog_test_status(test_status, t, cx))
                        .child(action_button(
                            "test-connection",
                            &i18n!(cx, "ssh.action.test_connection"),
                            t,
                            cx,
                            false,
                            &model.focus.test,
                            {
                                let panel = panel.clone();
                                move |_: &ClickEvent, _w, cx| {
                                    panel.update(cx, |this, cx| {
                                        this.test_connection(cx);
                                    })
                                }
                            },
                        ))
                })
                .child(action_button(
                    "cancel",
                    &i18n!(cx, "common.cancel"),
                    t,
                    cx,
                    false,
                    &model.focus.cancel,
                    {
                        let panel = panel.clone();
                        move |_: &ClickEvent, window, cx| {
                            panel.update(cx, |this, cx| {
                                this.dialog_request_close_with_window(Some(window), cx);
                                cx.notify();
                            })
                        }
                    },
                ))
                .child(focusable_action_button(
                    "save",
                    &save_label,
                    t,
                    cx,
                    !can_save,
                    true,
                    &model.focus.save,
                    {
                        let panel = panel.clone();
                        move |_: &ClickEvent, window, cx| {
                            panel.update(cx, |this, cx| {
                                this.dialog_save(Some(window), cx);
                                cx.notify();
                            })
                        }
                    },
                )),
        );

    // 首次渲染弹窗时自动聚焦「基础信息」中的名称 (name) 输入框（仅触发一次，防止后续渲染帧抢夺焦点）
    if !model.ui.auto_focused {
        model.ui.auto_focused = true;
        model
            .inputs
            .name
            .update(cx, |input, cx| input.focus(window, cx));
    }

    let right_container = div()
        .relative()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .overflow_hidden()
        .child(right.w_full().h_full())
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right_0()
                .left_0()
                .child(Scrollbar::vertical(&model.ui.scroll_handle)),
        );

    let win_size = window.viewport_size();
    let (card_w, card_h) = if is_ssh_session {
        (
            px(900.0).min(win_size.width - px(48.0)),
            px(680.0).min(win_size.height - px(80.0)).max(px(240.0)),
        )
    } else {
        (
            px(820.0).min(win_size.width - px(48.0)),
            px(560.0).min(win_size.height - px(80.0)).max(px(240.0)),
        )
    };

    velowork_ui::overlay::modal_content("session-dialog-modal", cx)
        .w(card_w)
        .h(card_h)
        .text_size(ui_text_md(cx))
        .overflow_hidden()
        .focus_scope_on_click(panel_focus)
        .on_key_down({
            let panel = panel.clone();
            move |event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if key == "tab" || key == "\t" {
                    let is_shift = event.keystroke.modifiers.shift;
                    panel.update(cx, |this, cx| {
                        if let Some(m) = this.ssh_dialog_mut() {
                            if m.cycle_focus(is_shift, window, cx) {
                                this.dialog_notify(cx);
                            }
                        }
                    });
                    cx.stop_propagation();
                }
            }
        })
        .child(velowork_ui::overlay::modal_header(
            format!("{}{}", title, dirty_mark),
            None::<&'static str>,
            &t,
            cx,
            {
                let panel_close = panel.clone();
                move |_, window, cx| {
                    panel_close.update(cx, |this, cx| {
                        this.dialog_request_close_with_window(Some(window), cx);
                    });
                }
            },
        ))
        .child(
            div()
                .flex()
                .flex_1()
                .min_h(px(0.0))
                .child(nav)
                .child(right_container),
        )
        .child(footer)
        .into_any_element()
}

fn section_title_key(section: SshSection) -> &'static str {
    use SshSection::*;
    match section {
        Basic => "ssh.section.basic",
        Connection => "ssh.section.connection",
        Authentication => "ssh.section.authentication",
        Terminal => "ssh.section.terminal",
        Network => "ssh.section.network",
        Security => "ssh.section.security",
        Advanced => "ssh.section.advanced",
        Notes => "ssh.section.notes",
    }
}

/// 可被键盘聚焦并支持回车/空格激活的操作按钮。
fn focusable_action_button(
    id: &str,
    label: &str,
    t: &ThemeColors,
    _cx: &App,
    disabled: bool,
    primary: bool,
    focus: &FocusHandle,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    let btn = if primary {
        button_primary(format!("act-{id}"), label, t)
    } else {
        button(format!("act-{id}"), label, t)
    };
    btn.disabled(disabled)
        .focus_handle(focus)
        .on_click(on_click)
}

fn action_button(
    id: &str,
    label: &str,
    t: &ThemeColors,
    cx: &App,
    disabled: bool,
    focus: &FocusHandle,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    focusable_action_button(id, label, t, cx, disabled, false, focus, on_click)
}

fn render_section_card(
    section: SshSection,
    model: &mut SessionDialogModel,
    panel: Entity<SessionPanel>,
    active_project_id: &Option<String>,
    t: &ThemeColors,
    cx: &mut App,
    window: &mut Window,
) -> AnyElement {
    let p = velowork_ui::SemanticPalette::from_context(cx);
    let content = render_section_content(section, model, panel, active_project_id, t, cx, window);
    div()
        .id(ElementId::Name(format!("card-{}", section.index()).into()))
        .w_full()
        .flex_shrink_0()
        .bg(p.surface_card)
        .border_1()
        .border_color(p.border_subtle)
        .rounded(RADIUS_CARD)
        .px(SPACE_LG)
        .py(SPACE_MD)
        .child(
            div()
                .id(ElementId::Name(
                    format!("card-h-{}", section.index()).into(),
                ))
                .pb(SPACE_SM)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .text_size(ui_text_lg(cx))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(t.text_primary))
                                .child(i18n!(cx, section_title_key(section))),
                        ),
                ),
        )
        .child(div().w_full().child(content))
        .into_any_element()
}

fn render_section_content(
    section: SshSection,
    model: &mut SessionDialogModel,
    panel: Entity<SessionPanel>,
    active_project_id: &Option<String>,
    t: &ThemeColors,
    cx: &mut App,
    window: &mut Window,
) -> AnyElement {
    let content: AnyElement = match section {
        SshSection::Basic => render_basic(model, panel, active_project_id, t, cx, window),
        SshSection::Connection => render_connection(model, panel, t, cx, window),
        SshSection::Authentication => render_authentication(model, panel, t, cx, window),
        SshSection::Terminal => render_terminal(model, panel, t, cx, window),
        SshSection::Network => render_network(model, panel, t, cx, window),
        SshSection::Security => render_security(model, panel, t, cx, window),
        SshSection::Advanced => render_advanced(model, panel, t, cx, window),
        SshSection::Notes => render_notes(model, panel, t, cx, window),
    };
    content
}

fn field_block(
    id: &str,
    label: &str,
    input: &Entity<InputState>,
    model: &SessionDialogModel,
    t: &ThemeColors,
    cx: &App,
) -> impl IntoElement {
    let is_password = input.read(cx).is_password();
    let fh = input.read(cx).focus_handle(cx);
    let mut item = velowork_ui::form::form_item(id.to_string())
        .label(label.to_string())
        .focus(&fh);
    if let Some(fid) = FieldId::from_key(id) {
        if let Some(res) = model.ui.validation.get(fid) {
            if res.severity == ValidationSeverity::Error {
                item = item.error(res.message.clone());
            }
        }
    }
    item.child(Input::new(input).mask_toggle(is_password))
        .render(t, cx)
}

fn key_path_field_block(
    id: &str,
    label: &str,
    input: &Entity<InputState>,
    panel: Entity<SessionPanel>,
    model: &SessionDialogModel,
    t: &ThemeColors,
    cx: &App,
) -> impl IntoElement {
    let is_password = input.read(cx).is_password();
    let fh = input.read(cx).focus_handle(cx);
    let mut item = velowork_ui::form::form_item(id.to_string())
        .label(label.to_string())
        .focus(&fh);
    if let Some(fid) = FieldId::from_key(id) {
        if let Some(res) = model.ui.validation.get(fid) {
            if res.severity == ValidationSeverity::Error {
                item = item.error(res.message.clone());
            }
        }
    }

    let picker_btn = Button::new("ssh-key-path-picker", t)
        .icon_left(AppIcon::FolderOpen)
        .tooltip(i18n!(cx, "ssh.auth.browse_key"))
        .on_click({
            let panel = panel.clone();
            move |_, window, cx| {
                panel.update(cx, |this, cx| {
                    this.open_ssh_key_picker(window, cx);
                });
            }
        });

    let row = div()
        .flex()
        .items_center()
        .gap(SPACE_SM)
        .child(div().flex_1().child(Input::new(input).mask_toggle(is_password)))
        .child(picker_btn);

    item.child(row).render(t, cx)
}

/// 下拉框外框：与 `field_block` 保持一致的两行布局（标题在上、下拉框在下）。
fn select_block(
    label: impl Into<SharedString>,
    select: &Entity<SelectState<SharedString>>,
    t: &ThemeColors,
    cx: &App,
) -> impl IntoElement {
    let lbl: SharedString = label.into();
    let id_str = format!("select-block-{}", lbl);
    velowork_ui::form::form_item(id_str)
        .label(lbl)
        .child(Select::new(select).into_any_element())
        .render(t, cx)
}

/// 步进输入框外框：与 `field_block` 保持一致的两行布局（标题在上、步进器在下）。
fn stepper_block(
    id: &str,
    label: impl Into<SharedString>,
    input: &Entity<InputState>,
    min: f32,
    max: f32,
    step: f32,
    default_val: f32,
    is_integer: bool,
    model: &SessionDialogModel,
    t: &ThemeColors,
    cx: &App,
) -> impl IntoElement {
    let fh = input.read(cx).focus_handle(cx);
    let mut item = velowork_ui::form::form_item(id.to_string())
        .label(label)
        .focus(&fh);
    if let Some(fid) = FieldId::from_key(id) {
        if let Some(res) = model.ui.validation.get(fid) {
            if res.severity == ValidationSeverity::Error {
                item = item.error(res.message.clone());
            }
        }
    }

    let input_dec = input.clone();
    let input_inc = input.clone();
    let id_str = format!("dialog-stepper-{}", id);

    let stepper = velowork_ui::number_stepper(id_str, input.clone(), t)
        .min(min)
        .max(max)
        .step(step)
        .on_dec(move |_, _, cx| {
            let current_text = input_dec.read(cx).text().trim().to_string();
            let cur_val = if current_text.is_empty() {
                default_val
            } else {
                current_text.parse::<f32>().unwrap_or(default_val)
            };
            let new_val = (cur_val - step).clamp(min, max);
            let formatted = if is_integer {
                format!("{}", new_val as u32)
            } else if (new_val.fract()).abs() < 1e-4 {
                format!("{:.0}", new_val)
            } else {
                format!("{:.1}", new_val)
            };
            input_dec.update(cx, |s, cx| s.set_value(&formatted, cx));
        })
        .on_inc(move |_, _, cx| {
            let current_text = input_inc.read(cx).text().trim().to_string();
            let cur_val = if current_text.is_empty() {
                default_val
            } else {
                current_text.parse::<f32>().unwrap_or(default_val)
            };
            let new_val = (cur_val + step).clamp(min, max);
            let formatted = if is_integer {
                format!("{}", new_val as u32)
            } else if (new_val.fract()).abs() < 1e-4 {
                format!("{:.0}", new_val)
            } else {
                format!("{:.1}", new_val)
            };
            input_inc.update(cx, |s, cx| s.set_value(&formatted, cx));
        });

    item.child(stepper).render(t, cx)
}

fn render_folder_select_field(
    model: &SessionDialogModel,
    panel: Entity<SessionPanel>,
    active_project_id: &Option<String>,
    t: &ThemeColors,
    cx: &mut App,
) -> impl IntoElement {
    let field_body = if model.ui.creating_parent_folder {
        let confirm_tip = i18n!(cx, "common.confirm");
        let cancel_tip = i18n!(cx, "common.cancel");
        let active_pid = active_project_id.clone();
        let panel_clone = panel.clone();
        let confirm = icon_button("parent-folder-confirm", AppIcon::Check, t, cx)
            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(confirm_tip.clone())).into())
            .focus_action(&model.focus.dir_confirm, t, {
                let panel = panel_clone.clone();
                let active_pid = active_pid.clone();
                move |_, window, cx| {
                    commit_new_parent_folder(&panel, active_pid.as_deref(), window, cx);
                }
            });

        let panel_clone2 = panel.clone();
        let cancel = icon_button("parent-folder-cancel", AppIcon::Close, t, cx)
            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(cancel_tip.clone())).into())
            .focus_action(&model.focus.dir_cancel, t, {
                let panel = panel_clone2.clone();
                move |_, window, cx| {
                    cancel_new_parent_folder(&panel, window, cx);
                }
            });

        let input_handle = model.ui.parent_folder_input.as_ref().map(|i| i.read(cx).focus_handle(cx));
        let confirm_focus = model.focus.dir_confirm.clone();
        let cancel_focus = model.focus.dir_cancel.clone();
        let active_pid2 = active_project_id.clone();
        let panel_clone3 = panel.clone();
        let panel_clone4 = panel.clone();

        h_flex()
            .gap(SPACE_SM)
            .on_key_down(move |e: &KeyDownEvent, window, cx| {
                let key = e.keystroke.key.as_str();
                if key == "tab" || key == "\t" {
                    let is_shift = e.keystroke.modifiers.shift;
                    let mut handles = Vec::with_capacity(3);
                    if let Some(ref ih) = input_handle {
                        handles.push(ih.clone());
                    }
                    handles.push(confirm_focus.clone());
                    handles.push(cancel_focus.clone());
                    velowork_ui::FocusGroup::cycle_handles(&handles, is_shift, window, cx);
                    cx.stop_propagation();
                } else if key == "escape" {
                    cancel_new_parent_folder(&panel_clone4, window, cx);
                    cx.stop_propagation();
                } else if (key == "enter" || key == "\n")
                    && let Some(ref ih) = input_handle
                    && ih.is_focused(window)
                {
                    commit_new_parent_folder(&panel_clone3, active_pid2.as_deref(), window, cx);
                    cx.stop_propagation();
                }
            })
            .child(
                div()
                    .flex_1()
                    .when_some(model.ui.parent_folder_input.as_ref(), |this, dir_inp| {
                        this.child(Input::new(dir_inp).cleanable(true))
                    }),
            )
            .child(confirm)
            .child(cancel)
            .into_any_element()
    } else {
        let new_tip = i18n!(cx, "common.new_folder");
        let panel_clone = panel.clone();
        let active_pid = active_project_id.clone();
        let new_btn = icon_button("parent-folder-new", AppIcon::NewFolder, t, cx)
            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(new_tip.clone())).into())
            .focus_action(&model.focus.new_parent_folder, t, move |_, window, cx| {
                panel_clone.update(cx, |this, cx| {
                    if let Some(m) = this.ssh_dialog_mut() {
                        m.ui.creating_parent_folder = true;
                        let initial_text = if let Some(ref sel_id) = m.config.parent_folder_id {
                            let folders = session_folder_options(&active_pid, cx);
                            folders
                                .iter()
                                .find(|(id, _)| id == sel_id)
                                .map(|(_, path)| format!("{}/", path))
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };
                        let placeholder = i18n!(cx, "common.new_folder_placeholder");
                        let inp = cx.new(|cx| {
                            InputState::new(cx)
                                .placeholder(placeholder)
                                .default_value(&initial_text)
                        });
                        m.ui.parent_folder_input = Some(inp);
                        if let Some(dir_inp) = m.ui.parent_folder_input.as_ref() {
                            dir_inp.update(cx, |st, cx| st.focus(window, cx));
                        }
                        cx.notify();
                    }
                });
            });

        h_flex()
            .gap(SPACE_SM)
            .child(div().flex_1().child(Select::new(&model.selects.parent_folder)))
            .child(new_btn)
            .into_any_element()
    };

    velowork_ui::form::form_item("session-parent-folder")
        .label(i18n!(cx, "ssh.field.parent_folder"))
        .focus(model.selects.parent_folder.read(cx).focus_handle())
        .child(field_body)
        .render(t, cx)
}

fn commit_new_parent_folder(
    panel: &Entity<SessionPanel>,
    active_project_id: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) {
    let pid_string = active_project_id.map(|s| s.to_string());
    panel.update(cx, |this, cx| {
        let raw = if let Some(m) = this.ssh_dialog() {
            m.ui
                .parent_folder_input
                .as_ref()
                .map(|i| i.read(cx).text().to_string())
                .unwrap_or_default()
        } else {
            return;
        };

        match parse_and_validate_folder_path(&raw) {
            Ok(segments) => {
                let store = cx.global::<GlobalSessionStore>().0.clone();
                let new_id = store.update(cx, |st, cx| {
                    st.ensure_folder_path_for_project(pid_string.as_deref(), &segments, cx)
                });
                if let Some(m) = this.ssh_dialog_mut() {
                    if let Some(id) = new_id {
                        m.config.parent_folder_id = Some(id);
                    }
                    m.ui.creating_parent_folder = false;
                    m.ui.parent_folder_input = None;
                    m.sync_selects(&pid_string, cx);
                    let sel_handle = m.selects.parent_folder.read(cx).focus_handle().clone();
                    window.focus(&sel_handle, cx);
                    cx.notify();
                }
            }
            Err(err) => {
                velowork_workspace::toast::ToastManager::warning(err, cx);
                if let Some(m) = this.ssh_dialog_mut() {
                    if let Some(ref inp) = m.ui.parent_folder_input {
                        inp.update(cx, |st, cx| st.focus(window, cx));
                    }
                }
            }
        }
    });
}

fn cancel_new_parent_folder(
    panel: &Entity<SessionPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    panel.update(cx, |this, cx| {
        if let Some(m) = this.ssh_dialog_mut() {
            m.ui.creating_parent_folder = false;
            m.ui.parent_folder_input = None;
            window.focus(&m.focus.new_parent_folder, cx);
            cx.notify();
        }
    });
}

fn switch_row(
    id: &str,
    label: impl Into<SharedString>,
    checked: bool,
    focus: &FocusHandle,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    t: &ThemeColors,
    cx: &App,
) -> impl IntoElement {
    let on_click = Arc::new(on_click);
    let lbl: SharedString = label.into();
    form_item(id.to_string())
        .label(lbl)
        .layout(FormLayout::Horizontal)
        .justify_between(true)
        .child(
            velowork_ui::Switch::new(format!("sw-{id}"))
                .checked(checked)
                .focus(focus)
                .on_click(move |_checked: &bool, window, cx| {
                    on_click(&ClickEvent::default(), window, cx);
                }),
        )
        .render(t, cx)
}

fn checkbox_item(
    id: &str,
    label: impl Into<SharedString>,
    checked: bool,
    focus: &FocusHandle,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    _t: &ThemeColors,
    _cx: &App,
) -> impl IntoElement {
    let on_click = Arc::new(on_click);
    velowork_ui::Checkbox::new(format!("cb-{id}"))
        .label(label.into())
        .checked(checked)
        .focus(focus)
        .on_click(move |_checked: &bool, window, cx| {
            on_click(&ClickEvent::default(), window, cx);
        })
}

fn render_dialog_test_status(
    status: &crate::views::panels::session_panel::TestConnectionStatus,
    t: &ThemeColors,
    cx: &App,
) -> impl IntoElement {
    use crate::views::panels::session_panel::TestConnectionStatus;
    match status {
        TestConnectionStatus::Idle => div(),
        TestConnectionStatus::Testing => div()
            .text_size(ui_text_ms(cx))
            .text_color(rgb(t.text_secondary))
            .child(i18n!(cx, "common.testing")),
        TestConnectionStatus::Success { latency_ms } => div()
            .flex()
            .items_center()
            .gap(SPACE_XS)
            .child(AppIcon::Check.size(ICON_STD).text_color(rgb(t.success)))
            .child(
                div()
                    .text_size(ui_text_ms(cx))
                    .text_color(rgb(t.success))
                    .child(format!(
                        "{} ({}ms)",
                        i18n!(cx, "ssh.action.test_success"),
                        latency_ms
                    )),
            ),
        TestConnectionStatus::Failed { error } => div()
            .flex()
            .items_center()
            .gap(SPACE_XS)
            .child(AppIcon::Close.size(ICON_STD).text_color(rgb(t.error)))
            .child(
                div()
                    .text_size(ui_text_ms(cx))
                    .text_color(rgb(t.error))
                    .child(format!("{}: {}", i18n!(cx, "status.test_failed"), error)),
            ),
    }
}

fn two_col(a: impl IntoElement, b: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .gap(SPACE_LG)
        .child(div().flex().flex_1().min_w(px(0.0)).child(a))
        .child(div().flex().flex_1().min_w(px(0.0)).child(b))
}

// ---------------------------------------------------------------------------
// 分组内容
// ---------------------------------------------------------------------------

fn render_serial_basic_fields(
    model: &SessionDialogModel,
    panel: Entity<SessionPanel>,
    t: &ThemeColors,
    cx: &App,
) -> AnyElement {
    let inputs = &model.inputs;
    let detected_ports = velowork_terminal::list_available_serial_ports();
    let serial_port_input = &inputs.serial_port;

    let port_row = div()
        .flex()
        .flex_col()
        .gap(SPACE_XS)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(ui_text_ms(cx))
                        .text_color(rgb(t.text_secondary))
                        .child(i18n!(cx, "ssh.serial.port")),
                )
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(rgb(t.text_muted))
                        .child(if detected_ports.is_empty() {
                            i18n!(cx, "ssh.serial.no_ports")
                        } else {
                            format!(
                                "{} {}",
                                detected_ports.len(),
                                i18n!(cx, "ssh.serial.refresh")
                            )
                        }),
                ),
        )
        .child(Input::new(serial_port_input))
        .when(!detected_ports.is_empty(), |d| {
            let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
            d.child(div().flex().flex_wrap().gap(SPACE_XS).pt(px(2.0)).children(
                detected_ports.into_iter().map(|port_desc| {
                    let port_name = port_desc.port_name.clone();
                    let label = port_desc.display_label();
                    let input_ref = serial_port_input.clone();
                    div()
                        .id(ElementId::Name(
                            format!("detected-port-{}", port_name).into(),
                        ))
                        .px(SPACE_SM)
                        .py(px(2.0))
                        .rounded(RADIUS_STD)
                        .border_1()
                        .border_color(p.border_subtle)
                        .bg(p.surface_card)
                        .hover(|s| s.bg(p.surface_hover))
                        .cursor_pointer()
                        .on_click(move |_, _w, cx| {
                            input_ref.update(cx, |s, cx| s.set_value(&port_name, cx));
                        })
                        .child(
                            div()
                                .text_size(ui_text_sm(cx))
                                .text_color(p.text_secondary)
                                .child(label),
                        )
                }),
            ))
        });

    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(port_row)
        .child(two_col(
            select_block(
                i18n!(cx, "ssh.serial.baud_rate"),
                &model.selects.serial_baud_rate,
                t,
                cx,
            ),
            select_block(
                i18n!(cx, "ssh.serial.data_bits"),
                &model.selects.serial_data_bits,
                t,
                cx,
            ),
        ))
        .child(two_col(
            select_block(
                i18n!(cx, "ssh.serial.stop_bits"),
                &model.selects.serial_stop_bits,
                t,
                cx,
            ),
            select_block(
                i18n!(cx, "ssh.serial.parity"),
                &model.selects.serial_parity,
                t,
                cx,
            ),
        ))
        .child(select_block(
            i18n!(cx, "ssh.serial.flow_control"),
            &model.selects.serial_flow_control,
            t,
            cx,
        ))
        .child(two_col(
            switch_row(
                "serial_dtr",
                i18n!(cx, "ssh.serial.dtr"),
                model.config.serial_dtr,
                &model.focus.serial_dtr,
                {
                    let panel = panel.clone();
                    move |_, _, cx| {
                        panel.update(cx, |this, cx| {
                            this.dialog_toggle_bool("serial_dtr", cx)
                        });
                    }
                },
                t,
                cx,
            ),
            switch_row(
                "serial_rts",
                i18n!(cx, "ssh.serial.rts"),
                model.config.serial_rts,
                &model.focus.serial_rts,
                {
                    let panel = panel.clone();
                    move |_, _, cx| {
                        panel.update(cx, |this, cx| {
                            this.dialog_toggle_bool("serial_rts", cx)
                        });
                    }
                },
                t,
                cx,
            ),
        ))
        .child(two_col(
            select_block(
                i18n!(cx, "ssh.serial.display_mode"),
                &model.selects.serial_display_mode,
                t,
                cx,
            ),
            select_block(
                i18n!(cx, "ssh.serial.line_ending"),
                &model.selects.serial_line_ending,
                t,
                cx,
            ),
        ))
        .child(two_col(
            switch_row(
                "serial_local_echo",
                i18n!(cx, "ssh.serial.local_echo"),
                model.config.serial_local_echo,
                &model.focus.serial_local_echo,
                {
                    let panel = panel.clone();
                    move |_, _, cx| {
                        panel.update(cx, |this, cx| {
                            this.dialog_toggle_bool("serial_local_echo", cx)
                        });
                    }
                },
                t,
                cx,
            ),
            switch_row(
                "serial_timestamps",
                i18n!(cx, "ssh.serial.timestamps"),
                model.config.serial_timestamps,
                &model.focus.serial_timestamps,
                {
                    let panel = panel.clone();
                    move |_, _, cx| {
                        panel.update(cx, |this, cx| {
                            this.dialog_toggle_bool("serial_timestamps", cx)
                        });
                    }
                },
                t,
                cx,
            ),
        ))
        .child(switch_row(
            "serial_auto_reconnect",
            i18n!(cx, "ssh.serial.auto_reconnect"),
            model.config.serial_auto_reconnect,
            &model.focus.serial_auto_reconnect,
            {
                let panel = panel.clone();
                move |_, _, cx| {
                    panel.update(cx, |this, cx| {
                        this.dialog_toggle_bool("serial_auto_reconnect", cx)
                    });
                }
            },
            t,
            cx,
        ))
        .into_any_element()
}

fn render_telnet_basic_fields(
    model: &SessionDialogModel,
    t: &ThemeColors,
    cx: &App,
) -> AnyElement {
    let inputs = &model.inputs;
    let host_lbl = i18n!(cx, "ssh.telnet.host");
    let port_lbl = i18n!(cx, "ssh.telnet.port");
    two_col(
        field_block(
            "telnet_host",
            &host_lbl,
            &inputs.telnet_host,
            model,
            t,
            cx,
        ),
        field_block(
            "telnet_port",
            &port_lbl,
            &inputs.telnet_port,
            model,
            t,
            cx,
        ),
    )
    .into_any_element()
}

fn render_local_basic_fields(
    model: &SessionDialogModel,
    t: &ThemeColors,
    cx: &App,
) -> AnyElement {
    let inputs = &model.inputs;
    let cwd_lbl = i18n!(cx, "session_dialog.local_cwd");
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(select_block(
            i18n!(cx, "ssh.local.shell"),
            &model.selects.local_shell,
            t,
            cx,
        ))
        .child(field_block(
            "local_cwd",
            &cwd_lbl,
            &inputs.local_cwd,
            model,
            t,
            cx,
        ))
        .into_any_element()
}

fn render_basic(
    model: &mut SessionDialogModel,
    panel: Entity<SessionPanel>,
    active_project_id: &Option<String>,
    t: &ThemeColors,
    cx: &mut App,
    _window: &mut Window,
) -> AnyElement {
    let inputs = &model.inputs;
    let current_protocol = model.config.protocol;

    let mut content = div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(field_block(
            "name",
            &i18n!(cx, "ssh.field.name"),
            &inputs.name,
            model,
            t,
            cx,
        ))
        .child(render_folder_select_field(
            model,
            panel.clone(),
            active_project_id,
            t,
            cx,
        ));

    // 根据所选协议自适应渲染专属子表单字段
    match current_protocol {
        velowork_state::SessionProtocol::Ssh => {}
        velowork_state::SessionProtocol::Serial => {
            content = content.child(render_serial_basic_fields(model, panel.clone(), t, cx));
        }
        velowork_state::SessionProtocol::Telnet => {
            content = content.child(render_telnet_basic_fields(model, t, cx));
        }
        velowork_state::SessionProtocol::Local => {
            content = content.child(render_local_basic_fields(model, t, cx));
        }
    }

    content = content.child(field_block(
        "startup",
        &i18n!(cx, "ssh.field.startup_command"),
        &inputs.startup_command,
        model,
        t,
        cx,
    ));

    content.into_any_element()
}

fn render_connection(
    model: &mut SessionDialogModel,
    panel: Entity<SessionPanel>,
    t: &ThemeColors,
    cx: &mut App,
    _window: &mut Window,
) -> AnyElement {
    let inputs = &model.inputs;
    let proxy_disabled = model.config.proxy_type == ProxyType::None;

    let proxy_block = if proxy_disabled {
        let panel = panel.clone();
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(rgb(t.text_muted))
                    .child(i18n!(cx, "ssh.proxy.disabled")),
            )
            .child(action_button(
                "enable-proxy",
                &i18n!(cx, "ssh.proxy.enable"),
                t,
                cx,
                false,
                &model.focus.proxy,
                move |_: &ClickEvent, _w, cx| {
                    panel.update(cx, |this, cx| {
                        this.dialog_set_proxy_type(ProxyType::Http, cx);
                        cx.notify();
                    })
                },
            ))
    } else {
        let mut proxy_col = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(select_block(
                i18n!(cx, "ssh.proxy.type"),
                &model.selects.proxy_type,
                t,
                cx,
            ));

        if model.config.proxy_type == ProxyType::Jump {
            proxy_col = proxy_col.child(select_block(
                i18n!(cx, "ssh.proxy.jump_session"),
                &model.selects.jump_session,
                t,
                cx,
            ));
        } else {
            proxy_col = proxy_col
                .child(two_col(
                    field_block(
                        "proxy_host",
                        &i18n!(cx, "ssh.proxy.host"),
                        &inputs.proxy_host,
                        model,
                        t,
                        cx,
                    ),
                    field_block(
                        "proxy_port",
                        &i18n!(cx, "ssh.proxy.port"),
                        &inputs.proxy_port,
                        model,
                        t,
                        cx,
                    ),
                ))
                .child(two_col(
                    field_block(
                        "proxy_user",
                        &i18n!(cx, "ssh.proxy.username"),
                        &inputs.proxy_username,
                        model,
                        t,
                        cx,
                    ),
                    field_block(
                        "proxy_pass",
                        &i18n!(cx, "ssh.proxy.password"),
                        &inputs.proxy_password,
                        model,
                        t,
                        cx,
                    ),
                ));
        }
        proxy_col
    };

    let sftp_row = switch_row(
        "enable_sftp",
        i18n!(cx, "ssh.connection.sftp"),
        model.config.enable_sftp,
        &model.focus.sftp,
        {
            let panel = panel.clone();
            move |_: &ClickEvent, _w, cx| {
                panel.update(cx, |this, cx| {
                    this.dialog_toggle_bool("enable_sftp", cx);
                    cx.notify();
                })
            }
        },
        t,
        cx,
    );

    let cfg = &model.config;
    let p_mon = panel.clone();
    let monitor_row = switch_row(
        "enable_monitor",
        i18n!(cx, "ssh.connection.monitor"),
        cfg.enable_monitor,
        &model.focus.monitor,
        move |_: &ClickEvent, _w, cx| {
            p_mon.update(cx, |this, cx| {
                this.dialog_toggle_bool("enable_monitor", cx);
                cx.notify();
            })
        },
        t,
        cx,
    );

    let p_cpu = panel.clone();
    let p_mem = panel.clone();
    let p_disk = panel.clone();

    let monitor_sub = if cfg.enable_monitor {
        Some(
            div()
                .flex()
                .gap(SPACE_XL)
                .pl(SPACE_LG)
                .pb(SPACE_XS)
                .child(checkbox_item(
                    "monitor_cpu",
                    i18n!(cx, "ssh.connection.monitor_cpu"),
                    cfg.monitor_cpu,
                    &model.focus.monitor_cpu,
                    move |_: &ClickEvent, _w, cx| {
                        p_cpu.update(cx, |this, cx| {
                            this.dialog_toggle_bool("monitor_cpu", cx);
                            cx.notify();
                        })
                    },
                    t,
                    cx,
                ))
                .child(checkbox_item(
                    "monitor_mem",
                    i18n!(cx, "ssh.connection.monitor_mem"),
                    cfg.monitor_mem,
                    &model.focus.monitor_mem,
                    move |_: &ClickEvent, _w, cx| {
                        p_mem.update(cx, |this, cx| {
                            this.dialog_toggle_bool("monitor_mem", cx);
                            cx.notify();
                        })
                    },
                    t,
                    cx,
                ))
                .child(checkbox_item(
                    "monitor_disk",
                    i18n!(cx, "ssh.connection.monitor_disk"),
                    cfg.monitor_disk,
                    &model.focus.monitor_disk,
                    move |_: &ClickEvent, _w, cx| {
                        p_disk.update(cx, |this, cx| {
                            this.dialog_toggle_bool("monitor_disk", cx);
                            cx.notify();
                        })
                    },
                    t,
                    cx,
                )),
        )
    } else {
        None
    };

    let content = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(field_block(
            "host",
            &i18n!(cx, "ssh.field.host"),
            &inputs.host,
            model,
            t,
            cx,
        ))
        .child(two_col(
            field_block("port", &i18n!(cx, "ssh.field.port"), &inputs.port, model, t, cx),
            field_block(
                "timeout",
                &i18n!(cx, "ssh.field.connection_timeout"),
                &inputs.connection_timeout,
                model,
                t,
                cx,
            ),
        ))
        .child(sftp_row)
        .child(monitor_row)
        .children(monitor_sub)
        .child(div().pt(px(10.0)).child(proxy_block));
    content.into_any_element()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthMethodKind {
    Password,
    PrivateKey,
    Agent,
    Keyboard,
}

impl AuthMethodKind {
    fn from_auth_type(auth: &SshAuthType) -> Self {
        match auth {
            SshAuthType::Password { .. } => AuthMethodKind::Password,
            SshAuthType::PrivateKey { .. } => AuthMethodKind::PrivateKey,
            SshAuthType::SshAgent { .. } => AuthMethodKind::Agent,
            SshAuthType::KeyboardInteractive => AuthMethodKind::Keyboard,
        }
    }

    fn to_default_auth_type(self) -> SshAuthType {
        match self {
            AuthMethodKind::Password => SshAuthType::Password { password: None },
            AuthMethodKind::PrivateKey => SshAuthType::PrivateKey {
                key_path: String::new(),
                passphrase: None,
            },
            AuthMethodKind::Agent => SshAuthType::SshAgent { socket_path: None },
            AuthMethodKind::Keyboard => SshAuthType::KeyboardInteractive,
        }
    }
}

fn render_authentication(
    model: &mut SessionDialogModel,
    panel: Entity<SessionPanel>,
    t: &ThemeColors,
    cx: &mut App,
    _window: &mut Window,
) -> AnyElement {
    let inputs = &model.inputs;
    let current = &model.config.auth_type;

    let selected_kind = AuthMethodKind::from_auth_type(current);

    let auth_options = vec![
        RadioOption::new(
            AuthMethodKind::Password,
            i18n!(cx, "ssh.auth.method.password"),
        ),
        RadioOption::new(
            AuthMethodKind::PrivateKey,
            i18n!(cx, "ssh.auth.method.private_key"),
        ),
        RadioOption::new(AuthMethodKind::Agent, i18n!(cx, "ssh.auth.method.agent")),
        RadioOption::new(
            AuthMethodKind::Keyboard,
            i18n!(cx, "ssh.auth.method.keyboard"),
        ),
    ];

    let seg = RadioGroup::new("auth-method-radio")
        .options(auth_options)
        .selected(Some(selected_kind))
        .mode(RadioMode::Button)
        .focus(&model.focus.auth_method)
        .on_change({
            let panel = panel.clone();
            move |kind: &AuthMethodKind, _window: &mut Window, cx: &mut App| {
                let default_auth = kind.to_default_auth_type();
                panel.update(cx, |this, cx| {
                    this.dialog_set_auth_type(default_auth, cx);
                    cx.notify();
                });
            }
        });

    let dynamic = match &model.config.auth_type {
        SshAuthType::Password { .. } => field_block(
            "password",
            &i18n!(cx, "ssh.field.password"),
            &inputs.password,
            model,
            t,
            cx,
        )
        .into_any_element(),
        SshAuthType::PrivateKey { .. } => two_col(
            key_path_field_block(
                "key_path",
                &i18n!(cx, "ssh.field.key_path"),
                &inputs.key_path,
                panel.clone(),
                model,
                t,
                cx,
            ),
            field_block(
                "passphrase",
                &i18n!(cx, "ssh.field.passphrase"),
                &inputs.passphrase,
                model,
                t,
                cx,
            ),
        )
        .into_any_element(),
        SshAuthType::SshAgent { .. } => field_block(
            "agent_socket",
            &i18n!(cx, "ssh.field.agent_socket"),
            &inputs.agent_socket_path,
            model,
            t,
            cx,
        )
        .into_any_element(),
        SshAuthType::KeyboardInteractive => div()
            .text_size(ui_text_md(cx))
            .text_color(rgb(t.text_muted))
            .child(i18n!(cx, "ssh.auth.keyboard_tip"))
            .into_any_element(),
    };

    let content = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(field_block(
            "username",
            &i18n!(cx, "ssh.field.username"),
            &inputs.username,
            model,
            t,
            cx,
        ))
        .child(div().pt(px(2.0)).child(seg))
        .child(dynamic)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(field_block(
                    "totp_secret",
                    &i18n!(cx, "ssh.auth.totp_secret"),
                    &inputs.totp_secret,
                    model,
                    t,
                    cx,
                ))
                .child(
                    div()
                        .text_size(ui_text_sm(cx))
                        .text_color(rgb(t.text_muted))
                        .child(i18n!(cx, "ssh.auth.totp_tip")),
                ),
        );
    content.into_any_element()
}

fn radio_block<T: Clone + PartialEq + 'static>(
    label: String,
    group: RadioGroup<T>,
    t: &ThemeColors,
) -> Div {
    let text_muted_color = rgb(t.text_muted);
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .w_full()
        .child(
            div()
                .text_size(px(11.0))
                .text_color(text_muted_color)
                .child(label),
        )
        .child(group)
}

fn enhancement_radio_block(
    id: &'static str,
    label: String,
    which: TerminalToggle,
    cur_val: Option<bool>,
    focus: &FocusHandle,
    panel: Entity<SessionPanel>,
    t: &ThemeColors,
    cx: &App,
) -> Div {
    let options = vec![
        RadioOption::new(None, i18n!(cx, "ssh.terminal.tri_state_default")),
        RadioOption::new(Some(true), i18n!(cx, "ssh.terminal.tri_state_enable")),
        RadioOption::new(Some(false), i18n!(cx, "ssh.terminal.tri_state_disable")),
    ];
    let group = RadioGroup::new(id)
        .mode(RadioMode::Button)
        .focus(focus)
        .selected(Some(cur_val))
        .options(options)
        .on_change({
            let panel = panel.clone();
            move |val: &Option<bool>, _w: &mut Window, cx: &mut App| {
                let val = *val;
                panel.update(cx, |this, cx| {
                    this.dialog_set_terminal_enhancement(which, val, cx);
                });
            }
        });
    radio_block(label, group, t)
}

fn render_terminal(
    model: &mut SessionDialogModel,
    panel: Entity<SessionPanel>,
    t: &ThemeColors,
    cx: &mut App,
    _window: &mut Window,
) -> AnyElement {
    let inputs = &model.inputs;
    let proto = model.config.protocol;

    let cur_shape = model.config.terminal.cursor_shape;
    let shape_options = vec![
        RadioOption::new(None, i18n!(cx, "ssh.terminal.cursor_shape_default")),
        RadioOption::new(
            Some(velowork_core::types::CursorShape::Block),
            i18n!(cx, "ssh.terminal.cursor_shape_block"),
        ),
        RadioOption::new(
            Some(velowork_core::types::CursorShape::Bar),
            i18n!(cx, "ssh.terminal.cursor_shape_bar"),
        ),
        RadioOption::new(
            Some(velowork_core::types::CursorShape::Underline),
            i18n!(cx, "ssh.terminal.cursor_shape_underline"),
        ),
    ];
    let panel_shape = panel.clone();
    let shape_group = RadioGroup::new("dialog-cursor-shape-radio")
        .mode(RadioMode::Button)
        .focus(&model.focus.cursor_shape)
        .selected(Some(cur_shape))
        .options(shape_options)
        .on_change(
            move |shape: &Option<velowork_core::types::CursorShape>,
                  _w: &mut Window,
                  cx: &mut App| {
                let shape = *shape;
                panel_shape.update(cx, |this, cx| {
                    this.dialog_set_cursor_shape(shape, cx);
                });
            },
        );

    let cur_blink = model.config.terminal.cursor_blink;
    let blink_options = vec![
        RadioOption::new(None, i18n!(cx, "ssh.terminal.tri_state_default")),
        RadioOption::new(Some(true), i18n!(cx, "ssh.terminal.tri_state_enable")),
        RadioOption::new(Some(false), i18n!(cx, "ssh.terminal.tri_state_disable")),
    ];
    let panel_blink = panel.clone();
    let blink_group = RadioGroup::new("dialog-cursor-blink-radio")
        .mode(RadioMode::Button)
        .focus(&model.focus.cursor_blink)
        .selected(Some(cur_blink))
        .options(blink_options)
        .on_change(move |blink: &Option<bool>, _w: &mut Window, cx: &mut App| {
            let blink = *blink;
            panel_blink.update(cx, |this, cx| {
                this.dialog_set_cursor_blink(blink, cx);
            });
        });

    let cur_bell = model.config.terminal.bell_style;
    let bell_options = vec![
        RadioOption::new(None, i18n!(cx, "ssh.terminal.bell_style_default")),
        RadioOption::new(
            Some(velowork_core::types::BellStyle::Visual),
            i18n!(cx, "ssh.terminal.bell_style_visual"),
        ),
        RadioOption::new(
            Some(velowork_core::types::BellStyle::Audible),
            i18n!(cx, "ssh.terminal.bell_style_audible"),
        ),
        RadioOption::new(
            Some(velowork_core::types::BellStyle::Both),
            i18n!(cx, "ssh.terminal.bell_style_both"),
        ),
        RadioOption::new(
            Some(velowork_core::types::BellStyle::Disabled),
            i18n!(cx, "ssh.terminal.bell_style_disabled"),
        ),
    ];
    let panel_bell = panel.clone();
    let bell_group = RadioGroup::new("dialog-bell-style-radio")
        .mode(RadioMode::Button)
        .focus(&model.focus.bell_style)
        .selected(Some(cur_bell))
        .options(bell_options)
        .on_change(
            move |style: &Option<velowork_core::types::BellStyle>,
                  _w: &mut Window,
                  cx: &mut App| {
                let style = *style;
                panel_bell.update(cx, |this, cx| {
                    this.dialog_set_bell_style(style, cx);
                });
            },
        );

    let mut content = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(two_col(
            select_block(
                i18n!(cx, "session_dialog.font_family"),
                &model.selects.font_family,
                t,
                cx,
            ),
            stepper_block(
                "font_size",
                i18n!(cx, "session_dialog.font_size"),
                &inputs.font_size,
                6.0,
                72.0,
                0.5,
                14.0,
                false,
                model,
                t,
                cx,
            ),
        ))
        .child(two_col(
            select_block(
                i18n!(cx, "settings.color_scheme"),
                &model.selects.color_scheme,
                t,
                cx,
            ),
            select_block(
                i18n!(cx, "ssh.terminal.charset"),
                &model.selects.charset,
                t,
                cx,
            ),
        ));

    if proto != velowork_state::SessionProtocol::Serial {
        content = content.child(two_col(
            select_block(
                i18n!(cx, "ssh.terminal.term"),
                &model.selects.terminal_type,
                t,
                cx,
            ),
            stepper_block(
                "scrollback",
                i18n!(cx, "ssh.field.scrollback"),
                &inputs.scrollback,
                100.0,
                100000.0,
                500.0,
                10000.0,
                true,
                model,
                t,
                cx,
            ),
        ));
    } else {
        content = content.child(stepper_block(
            "scrollback",
            i18n!(cx, "ssh.field.scrollback"),
            &inputs.scrollback,
            100.0,
            100000.0,
            500.0,
            10000.0,
            true,
            model,
            t,
            cx,
        ));
    }

    content = content
        .child(radio_block(
            i18n!(cx, "ssh.terminal.cursor_shape"),
            shape_group,
            t,
        ))
        .child(radio_block(
            i18n!(cx, "ssh.terminal.cursor_blink"),
            blink_group,
            t,
        ))
        .child(radio_block(
            i18n!(cx, "ssh.terminal.bell_style"),
            bell_group,
            t,
        ))
        .child(stepper_block(
            "bell_cooldown",
            i18n!(cx, "ssh.terminal.bell_cooldown"),
            &inputs.bell_cooldown_ms,
            0.0,
            5000.0,
            100.0,
            500.0,
            true,
            model,
            t,
            cx,
        ))
        .child(field_block(
            "word_separators",
            &i18n!(cx, "ssh.terminal.word_separators"),
            &inputs.word_separators,
            model,
            t,
            cx,
        ));

    if proto != velowork_state::SessionProtocol::Serial {
        content = content
            .child(two_col(
                enhancement_radio_block(
                    "dialog-shell-integration-radio",
                    i18n!(cx, "ssh.terminal.shell_integration"),
                    TerminalToggle::ShellIntegration,
                    model.config.terminal.shell_integration,
                    &model.focus.term_shell_integration,
                    panel.clone(),
                    t,
                    cx,
                ),
                enhancement_radio_block(
                    "dialog-bracketed-paste-radio",
                    i18n!(cx, "ssh.terminal.bracketed_paste"),
                    TerminalToggle::BracketedPaste,
                    model.config.terminal.bracketed_paste,
                    &model.focus.term_bracketed_paste,
                    panel.clone(),
                    t,
                    cx,
                ),
            ))
            .child(two_col(
                enhancement_radio_block(
                    "dialog-osc52-radio",
                    i18n!(cx, "ssh.terminal.osc52"),
                    TerminalToggle::Osc52,
                    model.config.terminal.osc52_clipboard,
                    &model.focus.term_osc52,
                    panel.clone(),
                    t,
                    cx,
                ),
                enhancement_radio_block(
                    "dialog-true-color-radio",
                    i18n!(cx, "ssh.terminal.true_color"),
                    TerminalToggle::TrueColor,
                    model.config.terminal.true_color,
                    &model.focus.term_true_color,
                    panel.clone(),
                    t,
                    cx,
                ),
            ));
    }

    content.into_any_element()
}

fn render_network(
    model: &mut SessionDialogModel,
    panel: Entity<SessionPanel>,
    t: &ThemeColors,
    cx: &mut App,
    _window: &mut Window,
) -> AnyElement {
    let inputs = &model.inputs;
    let cfg = &model.config;
    let content = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(two_col(
            field_block(
                "keepalive",
                &i18n!(cx, "ssh.field.keepalive_interval"),
                &inputs.keepalive_interval,
                model,
                t,
                cx,
            ),
            field_block(
                "keepalive_max",
                &i18n!(cx, "ssh.field.keepalive_max"),
                &inputs.keepalive_max,
                model,
                t,
                cx,
            ),
        ))
        .child(field_block(
            "idle",
            &i18n!(cx, "ssh.field.idle_timeout"),
            &inputs.idle_disconnect,
            model,
            t,
            cx,
        ))
        .child(switch_row(
            "tcp_nodelay",
            &i18n!(cx, "ssh.connection.tcp_nodelay"),
            cfg.tcp_nodelay,
            &model.focus.net_tcp_nodelay,
            {
                let panel = panel.clone();
                move |_: &ClickEvent, _w, cx| {
                    panel.update(cx, |this, cx| {
                        this.dialog_toggle_bool("tcp_nodelay", cx);
                        cx.notify();
                    })
                }
            },
            t,
            cx,
        ))
        .child(switch_row(
            "enable_x11_forwarding",
            &i18n!(cx, "ssh.network.enable_x11"),
            cfg.enable_x11_forwarding,
            &model.focus.net_x11_enable,
            {
                let panel = panel.clone();
                move |_: &ClickEvent, _w, cx| {
                    panel.update(cx, |this, cx| {
                        this.dialog_toggle_bool("enable_x11_forwarding", cx);
                        cx.notify();
                    })
                }
            },
            t,
            cx,
        ))
        .when(cfg.enable_x11_forwarding, |d| {
            d.child(field_block(
                "x11_display",
                &i18n!(cx, "ssh.network.x11_display"),
                &inputs.x11_display,
                model,
                t,
                cx,
            ))
        })
        .child(switch_row(
            "enable_agent_forwarding",
            &i18n!(cx, "session_dialog.enable_agent_forwarding"),
            cfg.enable_agent_forwarding,
            &model.focus.net_agent_fwd_enable,
            {
                let panel = panel.clone();
                move |_: &ClickEvent, _w, cx| {
                    panel.update(cx, |this, cx| {
                        this.dialog_toggle_bool("enable_agent_forwarding", cx);
                        cx.notify();
                    })
                }
            },
            t,
            cx,
        ))
        .child(select_block(
            i18n!(cx, "ssh.advanced.compression"),
            &model.selects.compression,
            t,
            cx,
        ))
        .child(two_col(
            field_block(
                "max_packets",
                &i18n!(cx, "ssh.advanced.max_packets"),
                &inputs.max_packets,
                model,
                t,
                cx,
            ),
            field_block(
                "recv_window",
                &i18n!(cx, "ssh.advanced.recv_window"),
                &inputs.recv_window,
                model,
                t,
                cx,
            ),
        ));
    content.into_any_element()
}

fn render_security(
    model: &mut SessionDialogModel,
    panel: Entity<SessionPanel>,
    t: &ThemeColors,
    cx: &mut App,
    _window: &mut Window,
) -> AnyElement {
    let cfg = &model.config;
    let auto = cfg.algorithms_automatic;

    let algo_block = if auto {
        div()
            .flex()
            .flex_col()
            .gap(SPACE_MD)
            .child(action_button(
                "algo-advanced",
                &i18n!(cx, "ssh.algorithms.advanced"),
                t,
                cx,
                false,
                &model.focus.algo_advanced,
                {
                    let panel = panel.clone();
                    move |_: &ClickEvent, _w, cx| {
                        panel.update(cx, |this, cx| {
                            this.dialog_toggle_algorithms_advanced(cx);
                            cx.notify();
                        })
                    }
                },
            ))
            .when(model.ui.algorithms_advanced_open, |d| {
                d.child(render_algorithm_lists(model, panel.clone(), t, cx))
            })
    } else {
        render_algorithm_lists(model, panel.clone(), t, cx)
    };

    let content = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(switch_row(
            "algo_auto",
            &i18n!(cx, "ssh.algorithms.automatic"),
            auto,
            &model.focus.adv_algo_auto,
            {
                let panel = panel.clone();
                move |_: &ClickEvent, _w, cx| {
                    panel.update(cx, |this, cx| {
                        this.dialog_toggle_bool("algorithms_automatic", cx);
                        cx.notify();
                    })
                }
            },
            t,
            cx,
        ))
        .child(algo_block)
        .child(select_block(
            i18n!(cx, "ssh.advanced.strict_host_key"),
            &model.selects.strict_host,
            t,
            cx,
        ));
    content.into_any_element()
}

fn algo_list_row(
    cat: &'static str,
    algo: String,
    is_checked: bool,
    is_first: bool,
    is_last: bool,
    panel: Entity<SessionPanel>,
    move_up_tip: &SharedString,
    move_down_tip: &SharedString,
    t: &ThemeColors,
    cx: &App,
) -> impl IntoElement {
    let panel_toggle = panel.clone();
    let algo_toggle = algo.clone();

    let panel_up = panel.clone();
    let algo_up = algo.clone();

    let panel_down = panel.clone();
    let algo_down = algo.clone();

    let up_tip = move_up_tip.clone();
    let down_tip = move_down_tip.clone();
    let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);

    div()
        .id(ElementId::Name(format!("algo-row-{}-{}", cat, algo).into()))
        .flex()
        .items_center()
        .justify_between()
        .px(SPACE_MD)
        .py(px(5.0))
        .when(!is_last, |d| d.border_b_1().border_color(p.border_subtle))
        .hover(|d| d.bg(p.surface_hover))
        // Left side: Checkbox + Name (clickable to toggle)
        .child(
            div()
                .id(ElementId::Name(
                    format!("algo-toggle-{}-{}", cat, algo).into(),
                ))
                .flex()
                .flex_1()
                .items_center()
                .gap(SPACE_MD)
                .cursor_pointer()
                .on_click(move |_: &ClickEvent, _w, cx| {
                    panel_toggle.update(cx, |this, cx| {
                        this.dialog_toggle_algorithm(cat, &algo_toggle, cx);
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .size(px(16.0))
                        .rounded(RADIUS_XS)
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(is_checked, |d| {
                            d.bg(p.surface_accent)
                                .child(AppIcon::Check.size(ICON_SM).text_color(p.text_on_accent))
                        })
                        .when(!is_checked, |d| {
                            d.border_1()
                                .border_color(p.border_subtle)
                                .bg(p.surface_card)
                        }),
                )
                .child(
                    div()
                        .text_size(ui_text_ms(cx))
                        .when(is_checked, |d| d.text_color(p.text_primary))
                        .when(!is_checked, |d| d.text_color(p.text_muted))
                        .child(algo.clone()),
                ),
        )
        // Right side: Up & Down priority move buttons
        .child(
            div()
                .flex()
                .items_center()
                .gap(SPACE_XS)
                // Up button
                .child({
                    let up_id = format!("algo-up-{}-{}", cat, algo);
                    if is_first {
                        div()
                            .id(ElementId::Name(up_id.into()))
                            .size(px(24.0))
                            .rounded(RADIUS_STD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(0.3)
                            .cursor_not_allowed()
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(up_tip.clone())).into())
                            .child(
                                AppIcon::ChevronUp
                                    .size(ICON_STD)
                                    .text_color(p.text_muted),
                            )
                    } else {
                        div()
                            .id(ElementId::Name(up_id.into()))
                            .size(px(24.0))
                            .rounded(RADIUS_STD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|h| {
                                h.bg(p.surface_hover)
                                    .border_1()
                                    .border_color(p.border_subtle)
                            })
                            .active(|a| a.bg(surface_bg_t(t.bg_selection, t)))
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(up_tip.clone())).into())
                            .child(
                                AppIcon::ChevronUp
                                    .size(ICON_STD)
                                    .text_color(p.text_primary),
                            )
                            .on_click(move |_: &ClickEvent, _w, cx| {
                                panel_up.update(cx, |this, cx| {
                                    this.dialog_move_algorithm(cat, &algo_up, -1, cx);
                                    cx.notify();
                                });
                            })
                    }
                })
                // Down button
                .child({
                    let down_id = format!("algo-down-{}-{}", cat, algo);
                    if is_last {
                        div()
                            .id(ElementId::Name(down_id.into()))
                            .size(px(24.0))
                            .rounded(RADIUS_STD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(0.3)
                            .cursor_not_allowed()
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(down_tip.clone())).into())
                            .child(
                                AppIcon::ChevronDown
                                    .size(ICON_STD)
                                    .text_color(p.text_muted),
                            )
                    } else {
                        div()
                            .id(ElementId::Name(down_id.into()))
                            .size(px(24.0))
                            .rounded(RADIUS_STD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|h| {
                                h.bg(p.surface_hover)
                                    .border_1()
                                    .border_color(p.border_subtle)
                            })
                            .active(|a| a.bg(surface_bg_t(t.bg_selection, t)))
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(down_tip.clone())).into())
                            .child(
                                AppIcon::ChevronDown
                                    .size(ICON_STD)
                                    .text_color(p.text_primary),
                            )
                            .on_click(move |_: &ClickEvent, _w, cx| {
                                panel_down.update(cx, |this, cx| {
                                    this.dialog_move_algorithm(cat, &algo_down, 1, cx);
                                    cx.notify();
                                });
                            })
                    }
                }),
        )
}

/// 渲染算法列表为带分类 Tab、复选框、支持上下调序优先级和恢复默认的结构化列表。
fn render_algorithm_lists(
    model: &SessionDialogModel,
    panel: Entity<SessionPanel>,
    t: &ThemeColors,
    cx: &App,
) -> Div {
    let current_cat = model.ui.algorithm_category;
    let cat_key = current_cat.key();

    let cat_options: Vec<RadioOption<AlgorithmCategory>> = AlgorithmCategory::ALL
        .iter()
        .map(|&cat| RadioOption::new(cat, i18n!(cx, cat.title_key())))
        .collect();

    let seg = RadioGroup::new("algo-category-tabs")
        .options(cat_options)
        .selected(Some(current_cat))
        .mode(RadioMode::Button)
        .on_change({
            let panel = panel.clone();
            move |cat: &AlgorithmCategory, _window: &mut Window, cx: &mut App| {
                let cat = *cat;
                panel.update(cx, |this, cx| {
                    if let Some(m) = this.ssh_dialog_mut() {
                        m.ui.algorithm_category = cat;
                        cx.notify();
                    }
                });
            }
        });

    let selected = match current_cat {
        AlgorithmCategory::Kex => &model.config.kex_algorithms,
        AlgorithmCategory::Cipher => &model.config.cipher_algorithms,
        AlgorithmCategory::Mac => &model.config.mac_algorithms,
        AlgorithmCategory::HostKey => &model.config.hostkey_algorithms,
    };

    let candidates = algo_candidates(cat_key);
    let mut full_list: Vec<String> = selected.clone();
    for cand in candidates {
        if !full_list.iter().any(|s| s == cand) {
            full_list.push(cand.to_string());
        }
    }

    let total_len = full_list.len();
    let move_up_tip = SharedString::from(i18n!(cx, "ssh.advanced.move_up"));
    let move_down_tip = SharedString::from(i18n!(cx, "ssh.advanced.move_down"));
    let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);

    let mut list_container = div()
        .flex()
        .flex_col()
        .rounded(RADIUS_STD)
        .border_1()
        .border_color(p.border_subtle)
        .bg(p.surface_card)
        .overflow_hidden();

    for (idx, algo) in full_list.into_iter().enumerate() {
        let is_first = idx == 0;
        let is_last = idx + 1 == total_len;
        let is_checked = selected.iter().any(|s| s == &algo);

        list_container = list_container.child(algo_list_row(
            cat_key,
            algo,
            is_checked,
            is_first,
            is_last,
            panel.clone(),
            &move_up_tip,
            &move_down_tip,
            t,
            cx,
        ));
    }

    // Header with title and "恢复默认"
    let reset_label = i18n!(cx, "ssh.advanced.reset_default");
    let reset_tip = reset_label.clone();
    let panel_reset = panel.clone();

    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(ui_text_ms(cx))
                .text_color(rgb(t.text_muted))
                .child(i18n!(cx, current_cat.desc_key())),
        )
        .child(
            div()
                .id(ElementId::Name(format!("algo-reset-{}", cat_key).into()))
                .flex()
                .items_center()
                .gap(SPACE_XS)
                .px(SPACE_SM)
                .py(px(2.0))
                .rounded(RADIUS_STD)
                .cursor_pointer()
                .hover(|d| d.bg(surface_bg_t(t.bg_hover, t)))
                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(reset_tip.clone())).into())
                .on_click(move |_: &ClickEvent, _w, cx| {
                    panel_reset.update(cx, |this, cx| {
                        this.dialog_reset_algorithms(cat_key, cx);
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .text_size(ui_text_ms(cx))
                        .text_color(rgb(t.accent))
                        .child(reset_label),
                ),
        );

    div()
        .flex()
        .flex_col()
        .gap(SPACE_MD)
        .child(div().flex().items_center().child(seg))
        .child(header)
        .child(list_container)
}

fn render_advanced(
    model: &mut SessionDialogModel,
    _panel: Entity<SessionPanel>,
    t: &ThemeColors,
    cx: &mut App,
    _window: &mut Window,
) -> AnyElement {
    let inputs = &model.inputs;
    let content = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(field_block(
            "rekey",
            &i18n!(cx, "ssh.field.rekey_time"),
            &inputs.rekey_time,
            model,
            t,
            cx,
        ))
        .child(two_col(
            field_block(
                "gex_min",
                &i18n!(cx, "ssh.advanced.gex_min"),
                &inputs.gex_min,
                model,
                t,
                cx,
            ),
            field_block(
                "gex_preferred",
                &i18n!(cx, "ssh.advanced.gex_preferred"),
                &inputs.gex_preferred,
                model,
                t,
                cx,
            ),
        ))
        .child(field_block(
            "gex_max",
            &i18n!(cx, "ssh.advanced.gex_max"),
            &inputs.gex_max,
            model,
            t,
            cx,
        ));
    content.into_any_element()
}

fn render_notes(
    model: &mut SessionDialogModel,
    _panel: Entity<SessionPanel>,
    t: &ThemeColors,
    cx: &mut App,
    _window: &mut Window,
) -> AnyElement {
    let inputs = &model.inputs;
    let fh_notes = inputs.notes.read(cx).focus_handle(cx);
    let content = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(field_block(
            "tags",
            &i18n!(cx, "ssh.field.tags"),
            &inputs.tags,
            model,
            t,
            cx,
        ))
        .child(
            form_item("notes".to_string())
                .label(i18n!(cx, "ssh.field.notes"))
                .focus(&fh_notes)
                .child(Input::new(&inputs.notes).fill_height().h(px(80.0)))
                .render(t, cx),
        );
    content.into_any_element()
}

// ---------------------------------------------------------------------------
// 下拉选项列表（供 Select 组件复用；旧 overlay 渲染已移除）
// ---------------------------------------------------------------------------

pub(crate) fn dropdown_option_list(
    key: &'static str,
    model: &SessionDialogModel,
    active_project_id: &Option<String>,
    cx: &App,
) -> Vec<(String, String, bool)> {
    match key {
        "terminal_type" => {
            let mut opts = vec![""];
            opts.extend(velowork_core::SUPPORTED_TERM_TYPES.iter().copied());
            let cur = model.config.terminal.term_type.clone().unwrap_or_default();
            let mut res: Vec<(String, String, bool)> = opts
                .iter()
                .map(|&o| {
                    let label = if o.is_empty() {
                        i18n!(cx, "ssh.terminal.default").to_string()
                    } else {
                        o.to_string()
                    };
                    (label, o.to_string(), cur == o)
                })
                .collect();
            if !cur.is_empty() && !opts.contains(&cur.as_str()) {
                res.push((cur.clone(), cur, true));
            }
            res
        }
        "charset" => {
            let mut opts = vec![(
                i18n!(cx, "ssh.terminal.default").to_string(),
                String::new(),
                model.config.terminal.charset.is_none(),
            )];
            for &o in velowork_core::charset::SUPPORTED_CHARSETS {
                let is_selected = model
                    .config
                    .terminal
                    .charset
                    .as_deref()
                    .map(|c| c.eq_ignore_ascii_case(o))
                    .unwrap_or(false);
                opts.push((o.to_string(), o.to_string(), is_selected));
            }
            opts
        }
        "font_family" => {
            let cur = model.config.terminal.font_family.as_deref().unwrap_or("");
            let mut opts = vec![(
                i18n!(cx, "ssh.terminal.default").to_string(),
                String::new(),
                cur.is_empty(),
            )];
            let mut system_fonts: Vec<String> = crate::font_cache::get_system_font_names();
            system_fonts.retain(|f| {
                let trimmed = f.trim();
                !trimmed.is_empty() && !trimmed.starts_with('.')
            });
            system_fonts.sort_by_key(|a| a.to_lowercase());
            system_fonts.dedup();
            for f in system_fonts {
                opts.push((f.clone(), f.clone(), cur == f));
            }
            opts
        }
        "color_scheme" => {
            let cur = model.config.terminal.color_scheme.as_deref().unwrap_or("");
            let mut opts = vec![(
                i18n!(cx, "ssh.terminal.default").to_string(),
                String::new(),
                cur.is_empty(),
            )];
            let builtin = [
                "Dark",
                "Light",
                "Solarized Dark",
                "Solarized Light",
                "Monokai",
                "Dracula",
                "Nord",
                "One Dark",
                "Gruvbox Dark",
            ];
            for name in builtin {
                opts.push((name.to_string(), name.to_string(), cur == name));
            }
            opts
        }
        "compression" => CompressionType::all_variants()
            .iter()
            .map(|c| {
                let v = format!("{:?}", c);
                (
                    c.display_name().to_string(),
                    v,
                    model.config.compression == *c,
                )
            })
            .collect(),
        "strict_host" => StrictHostKey::all_variants()
            .iter()
            .map(|s| {
                let v = format!("{:?}", s);
                (
                    s.display_name().to_string(),
                    v,
                    model.config.strict_host_key == *s,
                )
            })
            .collect(),
        "proxy_type" => ProxyType::all_variants()
            .iter()
            .map(|p| {
                let v = format!("{:?}", p);
                (
                    p.display_name().to_string(),
                    v,
                    model.config.proxy_type == *p,
                )
            })
            .collect(),
        "parent_folder" => {
            // 会话所属目录：列出当前项目的全部文件夹（含嵌套路径），
            // 并提供一个「无（顶级）」选项（value 为空字符串）。
            let mut opts: Vec<(String, String, bool)> = vec![(
                i18n!(cx, "ssh.field.no_parent_folder"),
                String::new(),
                model.config.parent_folder_id.is_none(),
            )];
            for (id, label) in session_folder_options(active_project_id, cx) {
                let selected = model.config.parent_folder_id.as_deref() == Some(id.as_str());
                opts.push((label, id, selected));
            }
            opts
        }
        "jump_session" => {
            // 跳板机会话：列出所有已保存的 SSH 会话（排除当前编辑的会话自身）。
            let mut opts: Vec<(String, String, bool)> = vec![(
                i18n!(cx, "tunnels.no_session"),
                String::new(),
                model.config.jump_session_id.is_none(),
            )];
            if let Some(store) = cx.try_global::<GlobalSessionStore>() {
                let all = store.0.read(cx).all_sessions();
                for (id, name) in all {
                    if id == model.config.id {
                        continue; // 排除自身
                    }
                    let selected = model.config.jump_session_id.as_deref() == Some(&id);
                    opts.push((name, id, selected));
                }
            }
            opts
        }
        "serial_baud" => {
            let bauds = [
                "115200", "57600", "38400", "19200", "9600", "4800", "2400", "1200", "300",
                "230400", "460800", "921600",
            ];
            let cur = model.config.serial_baud_rate.to_string();
            bauds
                .iter()
                .map(|b| (b.to_string(), b.to_string(), cur == *b))
                .collect()
        }
        "serial_data_bits" => {
            let bits = ["8", "7", "6", "5"];
            let cur = model.config.serial_data_bits.to_string();
            bits.iter()
                .map(|b| (b.to_string(), b.to_string(), cur == *b))
                .collect()
        }
        "serial_stop_bits" => {
            let bits = ["1", "2"];
            let cur = model.config.serial_stop_bits.to_string();
            bits.iter()
                .map(|b| (b.to_string(), b.to_string(), cur == *b))
                .collect()
        }
        "serial_parity" => {
            let parities = [
                (i18n!(cx, "ssh.serial.parity_none"), "none"),
                (i18n!(cx, "ssh.serial.parity_even"), "even"),
                (i18n!(cx, "ssh.serial.parity_odd"), "odd"),
                (i18n!(cx, "ssh.serial.parity_mark"), "mark"),
                (i18n!(cx, "ssh.serial.parity_space"), "space"),
            ];
            let cur = model.config.serial_parity.to_lowercase();
            parities
                .iter()
                .map(|(label, val)| (label.clone(), val.to_string(), cur == *val))
                .collect()
        }
        "serial_flow_control" => {
            let flows = [
                (i18n!(cx, "ssh.serial.flow_none"), "none"),
                (i18n!(cx, "ssh.serial.flow_software"), "software"),
                (i18n!(cx, "ssh.serial.flow_hardware"), "hardware"),
            ];
            let cur = model.config.serial_flow_control.to_lowercase();
            flows
                .iter()
                .map(|(label, val)| (label.clone(), val.to_string(), cur == *val))
                .collect()
        }
        "serial_display_mode" => {
            let modes = [
                (i18n!(cx, "ssh.serial.display_mode_text"), "text"),
                (i18n!(cx, "ssh.serial.display_mode_hex"), "hex"),
            ];
            let cur = model.config.serial_display_mode.to_lowercase();
            modes
                .iter()
                .map(|(label, val)| (label.clone(), val.to_string(), cur == *val))
                .collect()
        }
        "serial_line_ending" => {
            let endings = [
                (i18n!(cx, "ssh.serial.line_ending_crlf"), "crlf"),
                (i18n!(cx, "ssh.serial.line_ending_lf"), "lf"),
                (i18n!(cx, "ssh.serial.line_ending_cr"), "cr"),
                (i18n!(cx, "ssh.serial.line_ending_none"), "none"),
            ];
            let cur = model.config.serial_line_ending.to_lowercase();
            endings
                .iter()
                .map(|(label, val)| (label.clone(), val.to_string(), cur == *val))
                .collect()
        }
        "cursor_shape" => {
            let shapes = [
                (i18n!(cx, "ssh.terminal.cursor_shape_default"), ""),
                (i18n!(cx, "ssh.terminal.cursor_shape_block"), "block"),
                (i18n!(cx, "ssh.terminal.cursor_shape_bar"), "bar"),
                (i18n!(cx, "ssh.terminal.cursor_shape_underline"), "underline"),
            ];
            let cur = match model.config.terminal.cursor_shape {
                Some(velowork_core::types::CursorShape::Block) => "block",
                Some(velowork_core::types::CursorShape::Bar) => "bar",
                Some(velowork_core::types::CursorShape::Underline) => "underline",
                None => "",
            };
            shapes
                .iter()
                .map(|(label, val)| (label.clone(), val.to_string(), cur == *val))
                .collect()
        }
        "cursor_blink" => {
            let blinks = [
                (i18n!(cx, "ssh.terminal.cursor_blink_default"), ""),
                (i18n!(cx, "ssh.terminal.cursor_blink_enable"), "enable"),
                (i18n!(cx, "ssh.terminal.cursor_blink_disable"), "disable"),
            ];
            let cur = match model.config.terminal.cursor_blink {
                Some(true) => "enable",
                Some(false) => "disable",
                None => "",
            };
            blinks
                .iter()
                .map(|(label, val)| (label.clone(), val.to_string(), cur == *val))
                .collect()
        }
        "shell_integration" | "bracketed_paste" | "osc52_clipboard" | "true_color" => {
            let tri_val = match key {
                "shell_integration" => model.config.terminal.shell_integration,
                "bracketed_paste" => model.config.terminal.bracketed_paste,
                "osc52_clipboard" => model.config.terminal.osc52_clipboard,
                "true_color" => model.config.terminal.true_color,
                _ => None,
            };
            let cur = match tri_val {
                Some(true) => "enable",
                Some(false) => "disable",
                None => "",
            };
            let items = [
                (i18n!(cx, "ssh.terminal.tri_state_default"), ""),
                (i18n!(cx, "ssh.terminal.tri_state_enable"), "enable"),
                (i18n!(cx, "ssh.terminal.tri_state_disable"), "disable"),
            ];
            items
                .iter()
                .map(|(label, val)| (label.clone(), val.to_string(), cur == *val))
                .collect()
        }
        "local_shell" => {
            let mut opts: Vec<(String, String, bool)> = vec![(
                i18n!(cx, "ssh.local.default_shell"),
                String::new(),
                model.config.local_shell.is_none(),
            )];
            let detected_shells = velowork_terminal::shell_config::available_shells();
            for shell in detected_shells {
                if !shell.available {
                    continue;
                }
                if matches!(
                    shell.shell_type,
                    velowork_terminal::shell_config::ShellType::Default
                ) {
                    continue;
                }
                let json_val = serde_json::to_string(&shell.shell_type).unwrap_or_default();
                let is_selected = model.config.local_shell.as_deref() == Some(&json_val);
                opts.push((shell.name, json_val, is_selected));
            }
            opts
        }
        _ => Vec::new(),
    }
}

/// 递归收集会话树中的所有文件夹，返回 `(id, 带层级路径的显示名)` 列表。
fn collect_session_folders(
    nodes: &[SessionTreeNode],
    prefix: &str,
    out: &mut Vec<(String, String)>,
) {
    for node in nodes {
        if let SessionTreeNode::Folder {
            id, name, children, ..
        } = node
        {
            let label = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            out.push((id.clone(), label.clone()));
            collect_session_folders(children, &label, out);
        }
    }
}

/// 当前项目下所有可选文件夹（`id` → 层级路径显示名）。
fn session_folder_options(active_project_id: &Option<String>, cx: &App) -> Vec<(String, String)> {
    let store = cx.global::<GlobalSessionStore>().0.read(cx);
    let nodes = store.tree_for_project(active_project_id.as_deref());
    let mut out = Vec::new();
    collect_session_folders(nodes, "", &mut out);
    out
}

fn default_expanded_height(section: SshSection, protocol: velowork_state::SessionProtocol) -> f32 {
    super::model::default_expanded_height(section, protocol)
}
