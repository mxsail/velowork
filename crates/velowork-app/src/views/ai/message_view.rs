//! Unified chat message rendering components across floating popovers and side panels.
//! Follows the clean, modern, immersive Antigravity aesthetic:
//! - No redundant AI avatar/header above assistant replies;
//! - High-contrast error alert cards;
//! - Interactive code block actions (Run / Copy);
//! - Sleek terminal quote capsules without redundant labels.

use std::sync::Arc;
use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, AnyElement, App, AppContext, ClipboardItem, ElementId,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window,
};
use velowork_i18n::i18n;
use velowork_markdown::{MarkdownElement, MarkdownSelectionEvent};
use velowork_ui::icon::AppIcon;
use velowork_ui::tokens::*;
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex, SemanticPalette};

use super::commands::extract_commands;
use super::types::{ChatAttachment, ChatMessage};

/// 回调集合，用于解耦宿主视图（浮窗 vs 侧边栏面板）的差异化交互。
#[derive(Clone, Default)]
pub struct ChatMessageCallbacks {
    /// 复制完整消息正文
    pub on_copy_message: Option<Arc<dyn Fn(usize, &str, &mut Window, &mut App) + Send + Sync>>,
    /// 执行终端命令
    pub on_run_command: Option<Arc<dyn Fn(&str, &mut Window, &mut App) + Send + Sync>>,
    /// 写入命令到终端（不回车）
    pub on_insert_command: Option<Arc<dyn Fn(&str, &mut Window, &mut App) + Send + Sync>>,
    /// Markdown 划词选区变更事件
    pub on_selection_event:
        Option<Arc<dyn Fn(usize, MarkdownSelectionEvent, &mut Window, &mut App) + Send + Sync>>,
    /// 消息右键上下文菜单
    pub on_context_menu:
        Option<Arc<dyn Fn(usize, &MouseDownEvent, &mut Window, &mut App) + Send + Sync>>,
    /// 展开 / 收起引用内容
    pub on_toggle_quote: Option<Arc<dyn Fn(usize, &mut Window, &mut App) + Send + Sync>>,
    /// 编辑用户消息
    pub on_edit_message: Option<Arc<dyn Fn(usize, &mut Window, &mut App) + Send + Sync>>,
}

/// 渲染单条通用聊天消息卡片（支持用户卡片与 AI 卡片）
pub fn render_chat_message(
    msg: &ChatMessage,
    msg_index: usize,
    frame: u64,
    copied: bool,
    quote_expanded: bool,
    active_selection: Option<(usize, usize)>,
    callbacks: &ChatMessageCallbacks,
    cx: &mut App,
) -> AnyElement {
    let p = SemanticPalette::from_context(cx);

    if msg.is_user {
        render_user_message(msg, msg_index, copied, quote_expanded, callbacks, &p, cx)
    } else {
        render_assistant_message(msg, msg_index, frame, copied, active_selection, callbacks, &p, cx)
    }
}

/// 渲染用户消息卡片
fn render_user_message(
    msg: &ChatMessage,
    msg_index: usize,
    copied: bool,
    quote_expanded: bool,
    callbacks: &ChatMessageCallbacks,
    p: &SemanticPalette,
    cx: &mut App,
) -> AnyElement {
    let mut card_children: Vec<AnyElement> = Vec::new();

    // 1. 附件 Chips（如果有）
    if !msg.attachments.is_empty() {
        let chips = msg.attachments.iter().map(|att| {
            render_attachment_chip(att, p, cx)
        });
        card_children.push(
            h_flex()
                .flex_wrap()
                .gap(SPACE_XS)
                .children(chips)
                .into_any_element(),
        );
    }

    // 2. 终端选区引用胶囊（如果有）
    if let Some(ref q) = msg.quote {
        if !q.trim().is_empty() {
            card_children.push(render_quote_capsule(
                q,
                msg_index,
                quote_expanded,
                callbacks.on_toggle_quote.clone(),
                p,
                cx,
            ));
        }
    }

    // 3. 用户消息正文
    if !msg.text.trim().is_empty() {
        card_children.push(
            div()
                .w_full()
                .min_w(px(0.0))
                .text_size(ui_text_md(cx))
                .text_color(p.text_primary)
                .child(msg.text.clone())
                .into_any_element(),
        );
    }

    let group_id = SharedString::from(format!("user-msg-group-{}", msg_index));
    let msg_text = msg.text.clone();
    let on_copy = callbacks.on_copy_message.clone();
    let on_edit = callbacks.on_edit_message.clone();

    v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap(SPACE_XS)
        .group(group_id.clone())
        .child(
            div()
                .w_full()
                .min_w(px(0.0))
                .p(SPACE_SM)
                .rounded(RADIUS_MD)
                .bg(p.surface_card)
                .border_1()
                .border_color(p.border_subtle)
                .flex()
                .flex_col()
                .gap(SPACE_XS)
                .children(card_children),
        )
        // 底部悬浮操作按钮行（复制、编辑）
        .child(
            h_flex()
                .justify_end()
                .gap(SPACE_XS)
                .opacity(0.0)
                .group_hover(group_id, |s| s.opacity(1.0))
                .when(on_edit.is_some(), |d| {
                    let on_edit = on_edit.unwrap();
                    let edit_tip: &'static str = Box::leak(i18n!(cx, "common.edit").into_boxed_str());
                    d.child(
                        div()
                            .id(SharedString::from(format!("btn-edit-{}", msg_index)))
                            .cursor_pointer()
                            .p(px(2.0))
                            .rounded(RADIUS_SM)
                            .hover(|s| s.bg(p.surface_hover))
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(edit_tip)).into())
                            .on_click(move |_ev, window, cx| {
                                on_edit(msg_index, window, cx);
                            })
                            .child(AppIcon::Edit.size(px(12.0)).text_color(p.text_muted)),
                    )
                })
                .when(on_copy.is_some(), |d| {
                    let on_copy = on_copy.unwrap();
                    let copy_tip: &'static str = if copied {
                        Box::leak(i18n!(cx, "ai_assistant.copied").into_boxed_str())
                    } else {
                        Box::leak(i18n!(cx, "common.copy").into_boxed_str())
                    };
                    let icon = if copied {
                        AppIcon::Check.size(px(12.0)).text_color(p.status_success)
                    } else {
                        AppIcon::Copy.size(px(12.0)).text_color(p.text_muted)
                    };
                    d.child(
                        div()
                            .id(SharedString::from(format!("btn-copy-user-{}", msg_index)))
                            .cursor_pointer()
                            .p(px(2.0))
                            .rounded(RADIUS_SM)
                            .hover(|s| s.bg(p.surface_hover))
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(copy_tip)).into())
                            .on_click(move |_ev, window, cx| {
                                on_copy(msg_index, &msg_text, window, cx);
                            })
                            .child(icon),
                    )
                }),
        )
        .into_any_element()
}

/// 渲染 AI 回复消息卡片（极简 Antigravity 风格：无顶部头像/标题）
fn render_assistant_message(
    msg: &ChatMessage,
    msg_index: usize,
    frame: u64,
    copied: bool,
    active_selection: Option<(usize, usize)>,
    callbacks: &ChatMessageCallbacks,
    p: &SemanticPalette,
    cx: &mut App,
) -> AnyElement {
    let error_prefix = format!("{}:", i18n!(cx, "ai_assistant.error"));
    let is_error = msg.text.starts_with(&error_prefix);
    let is_loading = msg.streaming && msg.text.is_empty();

    let mut body_children: Vec<AnyElement> = Vec::new();

    // 1. 思考过程 Thinking Block（如有）
    if let Some(ref thinking) = msg.thinking {
        if !thinking.trim().is_empty() {
            body_children.push(render_thinking_block(thinking, p, cx));
        }
    }

    // 2. 状态呈现：错误告警卡片 vs 思考中动画 vs 正文内容
    if is_error {
        body_children.push(render_error_card(&msg.text, p));
    } else if is_loading {
        body_children.push(loading_indicator(cx, frame).into_any_element());
    } else if !msg.text.is_empty() {
        // Markdown 渲染
        let on_sel = callbacks.on_selection_event.clone();
        let md_id = format!("chat-md-{}", msg_index);
        let mut md_el = MarkdownElement::new(ElementId::from(md_id), &msg.text)
            .selection(active_selection)
            .on_url_click(move |url, _window, cx| {
                cx.open_url(url);
            });

        if let Some(on_sel) = on_sel {
            md_el = md_el.on_selection_event(move |ev, window, cx| {
                on_sel(msg_index, ev, window, cx);
            });
        }
        body_children.push(md_el.into_any_element());

        // 提取的可执行代码块卡片
        let commands = extract_commands(&msg.text);
        if !commands.is_empty() {
            for (c_idx, cmd) in commands.iter().enumerate() {
                body_children.push(render_command_card(
                    cmd,
                    msg_index,
                    c_idx,
                    callbacks.on_run_command.clone(),
                    callbacks.on_insert_command.clone(),
                    p,
                    cx,
                ));
            }
        }
    } else if !msg.streaming {
        // 异常收尾但文本为空：兜底展示未收到模型有效回复卡片
        let fallback_err = format!(
            "{}: {}",
            i18n!(cx, "ai_assistant.error"),
            i18n!(cx, "ai_assistant.empty_response")
        );
        body_children.push(render_error_card(&fallback_err, p));
    }

    let group_id = SharedString::from(format!("ai-msg-group-{}", msg_index));
    let msg_text = msg.text.clone();
    let on_copy = callbacks.on_copy_message.clone();
    let on_context_menu = callbacks.on_context_menu.clone();

    let mut outer = v_flex()
        .w_full()
        .min_w(px(0.0))
        .gap(SPACE_XS)
        .py(SPACE_XS)
        .group(group_id.clone())
        .children(body_children);

    if let Some(on_cm) = on_context_menu {
        outer = outer.on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, window, cx| {
            on_cm(msg_index, event, window, cx);
        });
    }

    // 悬浮复制整条回复按钮行
    if let Some(on_copy) = on_copy {
        if !is_loading && !msg.text.is_empty() {
            let copy_tip: &'static str = if copied {
                Box::leak(i18n!(cx, "ai_assistant.copied").into_boxed_str())
            } else {
                Box::leak(i18n!(cx, "common.copy").into_boxed_str())
            };
            let icon = if copied {
                AppIcon::Check.size(px(12.0)).text_color(p.status_success)
            } else {
                AppIcon::Copy.size(px(12.0)).text_color(p.text_muted)
            };
            outer = outer.child(
                h_flex()
                    .justify_start()
                    .gap(SPACE_XS)
                    .opacity(0.0)
                    .group_hover(group_id, |s| s.opacity(1.0))
                    .child(
                        div()
                            .id(SharedString::from(format!("btn-copy-ai-{}", msg_index)))
                            .cursor_pointer()
                            .p(px(2.0))
                            .rounded(RADIUS_SM)
                            .hover(|s| s.bg(p.surface_hover))
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(copy_tip)).into())
                            .on_click(move |_ev, window, cx| {
                                on_copy(msg_index, &msg_text, window, cx);
                            })
                            .child(icon),
                    ),
            );
        }
    }

    outer.into_any_element()
}

/// 渲染终端选区引用胶囊
pub fn render_quote_capsule(
    quote_text: &str,
    msg_index: usize,
    expanded: bool,
    on_toggle: Option<Arc<dyn Fn(usize, &mut Window, &mut App) + Send + Sync>>,
    p: &SemanticPalette,
    cx: &mut App,
) -> AnyElement {
    let card = div()
        .id(SharedString::from(format!("ai-quote-card-{}", msg_index)))
        .w_full()
        .min_w(px(0.0))
        .p(SPACE_XS)
        .rounded(RADIUS_SM)
        .bg(p.surface_hover)
        .border_l_2()
        .border_color(p.surface_accent)
        .when(on_toggle.is_some(), |d| {
            let on_toggle = on_toggle.unwrap();
            d.cursor_pointer()
                .hover(|s| s.bg(p.surface_selection))
                .on_click(move |_ev, window, cx| {
                    on_toggle(msg_index, window, cx);
                })
        });

    let toggle_hint: &'static str = if expanded {
        Box::leak(i18n!(cx, "ai_assistant.quote_collapse").into_boxed_str())
    } else {
        Box::leak(i18n!(cx, "ai_assistant.quote_expand").into_boxed_str())
    };

    card.child(
        h_flex()
            .when(expanded, |d| d.items_start())
            .when(!expanded, |d| d.items_center())
            .gap(SPACE_XS)
            .child(
                div()
                    .flex_shrink_0()
                    .when(expanded, |d| d.pt(px(2.0)))
                    .child(
                        AppIcon::Terminal
                            .size(px(12.0))
                            .text_color(p.text_muted),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .font_family(mono_font_family(cx))
                    .text_size(ui_text_md(cx))
                    .text_color(p.text_muted)
                    .when(!expanded, |d| {
                        d.truncate()
                            .whitespace_nowrap()
                            .child(quote_text.lines().collect::<Vec<_>>().join(" "))
                    })
                    .when(expanded, |d| d.child(quote_text.to_string())),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .when(expanded, |d| d.pt(px(2.0)))
                    .text_size(ui_text_md(cx))
                    .text_color(p.text_muted)
                    .child(toggle_hint),
            ),
    )
    .into_any_element()
}

/// 渲染附件 Chip
pub fn render_attachment_chip(att: &ChatAttachment, p: &SemanticPalette, cx: &App) -> AnyElement {
    h_flex()
        .items_center()
        .gap(SPACE_XS)
        .px(px(6.0))
        .py(px(2.0))
        .rounded(RADIUS_SM)
        .bg(p.surface_hover)
        .border_1()
        .border_color(p.border_subtle)
        .child(AppIcon::File.size(px(11.0)).text_color(p.text_muted))
        .child(
            div()
                .text_size(ui_text_md(cx))
                .text_color(p.text_secondary)
                .child(att.name.clone()),
        )
        .into_any_element()
}

/// 渲染错误告警卡片（浅红底色 + 红色边框 + 禁用/警告图标）
pub fn render_error_card(error_text: &str, p: &SemanticPalette) -> AnyElement {
    let is_multiline = error_text.contains('\n') || error_text.chars().count() > 36;
    h_flex()
        .w_full()
        .gap(SPACE_SM)
        .p(SPACE_SM)
        .rounded(RADIUS_MD)
        .bg(p.surface_danger.opacity(0.12))
        .border_1()
        .border_color(p.status_error)
        .when(is_multiline, |d| d.items_start())
        .when(!is_multiline, |d| d.items_center())
        .child(
            div()
                .flex_shrink_0()
                .when(is_multiline, |d| d.pt(px(2.0)))
                .child(
                    AppIcon::Ban
                        .size(px(14.0))
                        .text_color(p.status_error),
                ),
        )
        .child(
            div()
                .flex_1()
                .text_size(ui_text_md_val())
                .text_color(p.status_error)
                .child(error_text.to_string()),
        )
        .into_any_element()
}

/// 渲染思考过程折叠块
pub fn render_thinking_block(content: &str, p: &SemanticPalette, cx: &mut App) -> AnyElement {
    let header_label = i18n!(cx, "ai_assistant.thinking_process");
    div()
        .rounded(RADIUS_SM)
        .border_1()
        .border_color(p.border_subtle)
        .bg(p.surface_hover.opacity(0.5))
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .gap(SPACE_SM)
                .px(px(8.0))
                .py(SPACE_XS)
                .cursor_pointer()
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(p.text_muted)
                        .child(header_label),
                ),
        )
        .child(
            div().px(px(8.0)).pb(SPACE_SM).child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(p.text_secondary)
                    .child(content.to_string()),
            ),
        )
        .into_any_element()
}

/// 渲染命令操作卡片（可执行终端命令）
pub fn render_command_card(
    cmd: &str,
    msg_index: usize,
    c_index: usize,
    on_run: Option<Arc<dyn Fn(&str, &mut Window, &mut App) + Send + Sync>>,
    on_insert: Option<Arc<dyn Fn(&str, &mut Window, &mut App) + Send + Sync>>,
    p: &SemanticPalette,
    cx: &mut App,
) -> AnyElement {
    let cmd_run = cmd.to_string();
    let cmd_ins = cmd.to_string();
    let cmd_copy = cmd.to_string();

    let ins_tip: &'static str = Box::leak(
        i18n!(cx, "terminal.inline_ai_insert_terminal_tip").into_boxed_str(),
    );
    let run_tip: &'static str = Box::leak(
        i18n!(cx, "terminal.inline_ai_run_terminal_tip").into_boxed_str(),
    );
    let copy_tip: &'static str = Box::leak(
        i18n!(cx, "terminal.inline_ai_copy_command").into_boxed_str(),
    );

    div()
        .w_full()
        .min_w(px(0.0))
        .mt(SPACE_XS)
        .p(SPACE_XS)
        .bg(p.surface_card)
        .border_1()
        .border_color(p.border_subtle)
        .rounded(RADIUS_MD)
        .flex()
        .items_center()
        .justify_between()
        .gap(SPACE_XS)
        .child(
            h_flex()
                .items_center()
                .gap(SPACE_XS)
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .child(AppIcon::Terminal.size(px(12.0)).text_color(p.text_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .font_family(mono_font_family(cx))
                        .text_size(ui_text_md(cx))
                        .text_color(p.text_primary)
                        .child(cmd.to_string()),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(4.0))
                .when(on_insert.is_some(), |d| {
                    let on_insert = on_insert.unwrap();
                    let cmd_ins = cmd_ins.clone();
                    d.child(
                        div()
                            .id(SharedString::from(format!("cmd-ins-{}-{}", msg_index, c_index)))
                            .cursor_pointer()
                            .p(px(2.0))
                            .rounded(RADIUS_SM)
                            .hover(|s| s.bg(p.surface_hover))
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(ins_tip)).into())
                            .on_click(move |_ev, window, cx| {
                                on_insert(&cmd_ins, window, cx);
                            })
                            .child(AppIcon::ChevronRight.size(px(12.0)).text_color(p.text_muted)),
                    )
                })
                .when(on_run.is_some(), |d| {
                    let on_run = on_run.unwrap();
                    let cmd_run = cmd_run.clone();
                    d.child(
                        div()
                            .id(SharedString::from(format!("cmd-run-{}-{}", msg_index, c_index)))
                            .cursor_pointer()
                            .p(px(2.0))
                            .rounded(RADIUS_SM)
                            .hover(|s| s.bg(p.surface_hover))
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(run_tip)).into())
                            .on_click(move |_ev, window, cx| {
                                on_run(&cmd_run, window, cx);
                            })
                            .child(AppIcon::Terminal.size(px(12.0)).text_color(p.surface_accent)),
                    )
                })
                .child(
                    div()
                        .id(SharedString::from(format!("cmd-cp-{}-{}", msg_index, c_index)))
                        .cursor_pointer()
                        .p(px(2.0))
                        .rounded(RADIUS_SM)
                        .hover(|s| s.bg(p.surface_hover))
                        .tooltip(move |_, cx| cx.new(|_| Tooltip::new(copy_tip)).into())
                        .on_click(move |_ev, _window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(cmd_copy.clone()));
                        })
                        .child(AppIcon::Copy.size(px(12.0)).text_color(p.text_muted)),
                ),
        )
        .into_any_element()
}

/// 加载状态指示器：三个错相位呼吸跳动的圆点 + 文案，直观表达「等待回复中」。
pub fn loading_indicator(cx: &App, frame: u64) -> impl IntoElement {
    let p = SemanticPalette::from_context(cx);
    let label = i18n!(cx, "ai_assistant.thinking");
    let dots = (0..3).map(|i| {
        let phase = (frame + i * 10) % 30;
        let wave = (phase as f32 / 30.0 * std::f32::consts::PI * 2.0).sin();
        let opacity = 0.35 + 0.65 * ((wave + 1.0) / 2.0);
        div()
            .w(px(7.0))
            .h(px(7.0))
            .rounded(px(3.5))
            .bg(p.surface_accent)
            .opacity(opacity)
    });
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .py(SPACE_SM)
        .child(h_flex().gap(px(5.0)).items_center().children(dots))
        .child(
            div()
                .text_size(ui_text_md(cx))
                .text_color(p.text_muted)
                .child(label),
        )
}

fn ui_text_md_val() -> gpui::Pixels {
    px(12.0)
}
