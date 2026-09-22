use crate::keybindings::Cancel;
use velowork_ui::button::{button, Button};
use velowork_ui::focus_group::{FocusGroup, FocusGroupExt};
use velowork_ui::icon::AppIcon;
use crate::views::panels::quick_commands_panel::send_command_to_focused_terminal;
use velowork_workspace::focus::FocusManager;
use velowork_workspace::quick_commands::QuickCommandVar;
use velowork_workspace::state::Workspace;
use velowork_terminal::TerminalsRegistry;
use velowork_ui::focusable::FocusSurfaceExt;
use velowork_ui::form::form_item;
use velowork_ui::h_flex;
use velowork_ui::input::{InputEvent, InputState};
use velowork_ui::overlay::{modal_content, modal_header};
use velowork_ui::theme::theme;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::tokens::{ui_text_ms, ui_text_sm, SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG, RADIUS_STD};
use velowork_ui::overlay::CloseEvent;
use velowork_i18n::i18n;
use gpui::prelude::*;
use gpui::*;
use velowork_ui::scrollable::{ScrollableElement, Scrollbar};
use velowork_ui::tooltip::Tooltip;

struct VarInput {
    name: String,
    input: Entity<InputState>,
}

pub struct QuickCommandVarDialog {
    focus_manager: Entity<FocusManager>,
    workspace: Entity<Workspace>,
    terminals: TerminalsRegistry,
    focus_handle: FocusHandle,
    cancel_focus: FocusHandle,
    send_focus: FocusHandle,
    #[allow(dead_code)]
    command_name: String,
    template: String,
    variables: Vec<QuickCommandVar>,
    vars: Vec<VarInput>,
    initial_focus_done: bool,
    scroll_handle: ScrollHandle,
}

pub enum QuickCommandVarDialogEvent {
    Close,
    Executed,
}

impl EventEmitter<QuickCommandVarDialogEvent> for QuickCommandVarDialog {}

impl CloseEvent for QuickCommandVarDialogEvent {
    fn is_close(&self) -> bool {
        matches!(self, Self::Close | Self::Executed)
    }
}

/// Replace every `{{name}}` placeholder in `template` with its value.
fn interpolate(template: &str, values: &[(String, String)]) -> String {
    let mut result = template.to_string();
    for (name, value) in values {
        let placeholder = ["{{", name, "}}"].concat();
        result = result.replace(&placeholder, value);
    }
    result
}

impl QuickCommandVarDialog {
    pub fn new(
        focus_manager: Entity<FocusManager>,
        workspace: Entity<Workspace>,
        terminals: TerminalsRegistry,
        command_name: String,
        template: String,
        variables: Vec<QuickCommandVar>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut vars = Vec::new();
        for v in &variables {
            let input = cx.new(|cx| {
                InputState::new(cx).default_value(&v.default_value)
            });
            cx.subscribe(&input, |_, _, _event: &InputEvent, cx| {
                cx.notify();
            })
            .detach();
            vars.push(VarInput {
                name: v.name.clone(),
                input,
            });
        }

        Self {
            focus_manager,
            workspace,
            terminals,
            focus_handle: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            send_focus: cx.focus_handle(),
            command_name,
            template,
            variables,
            vars,
            initial_focus_done: false,
            scroll_handle: ScrollHandle::new(),
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(QuickCommandVarDialogEvent::Close);
    }

    /// Current `(name, value)` pairs from the inputs.
    fn current_values(&self, cx: &App) -> Vec<(String, String)> {
        if self.vars.is_empty() {
            self.variables
                .iter()
                .map(|v| (v.name.clone(), v.default_value.clone()))
                .collect()
        } else {
            self.vars
                .iter()
                .map(|v| (v.name.clone(), v.input.read(cx).text().to_string()))
                .collect()
        }
    }

    fn preview(&self, cx: &App) -> String {
        interpolate(&self.template, &self.current_values(cx))
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let cmd = self.preview(cx);
        send_command_to_focused_terminal(
            &self.focus_manager,
            &self.workspace,
            &self.terminals,
            &cmd,
            cx,
        );
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
        });
        cx.emit(QuickCommandVarDialogEvent::Executed);
    }
}

impl Render for QuickCommandVarDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);

        if !self.initial_focus_done {
            self.initial_focus_done = true;
            if let Some(first_var) = self.vars.first() {
                first_var.input.update(cx, |inp, cx| inp.focus(window, cx));
            }
        }

        let preview = self.preview(cx);
        let _this = cx.entity();

        let focus_group = FocusGroup::new();
        for v in &self.vars {
            focus_group.add(v.input.read(cx).focus_handle(cx));
        }
        focus_group.add(self.cancel_focus.clone());
        focus_group.add(self.send_focus.clone());

        let win_size = window.viewport_size();
        let card_w = px(560.0).min(win_size.width - px(48.0));
        let card_h = px(480.0).min(win_size.height - px(80.0)).max(px(240.0));

        let body_content = div()
            .id(ElementId::Name("qc-var-dialog-scroll".into()))
            .relative()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .p(px(20.0))
            .min_h(px(0.0))
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .w_full()
            .children(self.vars.iter().enumerate().map(|(i, v)| {
                let label = v.name.clone();
                let item_id = format!("qc-var-field-{}", i);
                form_item(item_id)
                    .label(label)
                    .focus(&v.input.read(cx).focus_handle(cx))
                    .child(velowork_ui::Input::new(&v.input).cleanable(true))
                    .render(&t, cx)
                    .into_any_element()
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SPACE_SM)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(ui_text_ms(cx))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(t.text_secondary))
                                    .child(i18n!(cx, "quick_commands.preview")),
                            )
                            .child(
                                div()
                                    .id("qc-var-copy")
                                    .cursor_pointer()
                                    .px(SPACE_MD)
                                    .py(px(3.0))
                                    .rounded(RADIUS_STD)
                                    .hover(|s| s.bg(rgb(t.bg_hover)))
                                    .flex()
                                    .items_center()
                                    .gap(SPACE_XS)
                                    .child(
                                        AppIcon::Copy
                                            .size(px(12.0))
                                            .text_color(rgb(t.text_muted)),
                                    )
                                    .child(
                                        div()
                                            .text_size(ui_text_sm(cx))
                                            .text_color(rgb(t.text_muted))
                                            .child(i18n!(cx, "common.action.copy")),
                                    )
                                    .tooltip(move |_, cx| { let __tip = i18n!(cx, "common.action.copy"); cx.new(|_| Tooltip::new(__tip)).into() })
                                    .on_click({
                                        let preview = preview.clone();
                                        cx.listener(move |_, _, _window, cx| {
                                            cx.write_to_clipboard(
                                                ClipboardItem::new_string(preview.clone()),
                                            );
                                            crate::views::panels::toast::ToastManager::info(
                                                i18n!(cx, "quick_commands.copied"),
                                                cx,
                                            );
                                        })
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_h(px(48.0))
                            .max_h(px(140.0))
                            .overflow_y_scrollbar()
                            .p(px(10.0))
                            .rounded(RADIUS_STD)
                            .border_1()
                            .border_color(p.border_subtle)
                            .bg(p.surface_card)
                            .text_size(ui_text_sm(cx))
                            .text_color(p.text_primary)
                            .font_family(crate::settings::settings(cx).font_family.as_str())
                            .whitespace_normal()
                            .child(
                                if preview.is_empty() {
                                    i18n!(cx, "quick_commands.preview_placeholder")
                                } else {
                                    preview.clone()
                                },
                            ),
                    ),
            );

        let body_container = div()
            .relative()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_hidden()
            .child(body_content.h_full())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .left_0()
                    .child(Scrollbar::vertical(&self.scroll_handle)),
            );

        modal_content("qc-var-dialog", cx)
            .w(card_w)
            .h(card_h)
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .key_context("QuickCommandVarDialog")
            .tab_cycle(&focus_group)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.close(cx);
            }))
            .focus_scope_on_click(&self.focus_handle)
            .child(modal_header(
                &i18n!(cx, "quick_commands.execute_title"),
                None::<&str>,
                &t,
                cx,
                cx.listener(|this, _, _, cx| this.close(cx)),
            ))
            .child(body_container)
            .child(
                h_flex()
                    .h(px(48.0))
                    .flex_shrink_0()
                    .items_center()
                    .justify_end()
                    .gap(SPACE_MD)
                    .px(SPACE_LG)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .child(
                        button(
                            "qc-var-cancel",
                            i18n!(cx, "common.action.cancel"),
                            &t,
                        )
                        .focus_handle(&self.cancel_focus)
                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(
                        Button::new("qc-var-send", &t)
                            .primary()
                            .icon_left(AppIcon::Play)
                            .label(i18n!(cx, "quick_commands.send_execute"))
                            .focus_handle(&self.send_focus)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.send(cx);
                            })),
                    ),
            )
    }
}

impl_focusable!(QuickCommandVarDialog);
