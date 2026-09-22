//! Shell selector overlay for switching terminal shells.

use crate::actions::Cancel;
use velowork_terminal::shell_config::{available_shells, AvailableShell, ShellType};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::theme::theme;
use velowork_ui::tokens::{ui_text, ui_text_md, SPACE_XS, SPACE_MD, SPACE_LG};
use velowork_ui::quick_picker::{
    handle_quick_picker_key, QuickPickerAction, QuickPickerConfig, QuickPickerState,
};
use velowork_ui::overlay::{modal_content, modal_header};
use velowork_i18n::i18n;
use gpui::*;
use velowork_ui::h_flex;
use gpui::prelude::*;

/// Shell selector overlay for choosing a shell.
pub struct ShellSelectorOverlay {
    focus_handle: FocusHandle,
    state: QuickPickerState<AvailableShell>,
    current_shell: ShellType,
    /// Context: which terminal this is for (project_id, terminal_id)
    context: Option<(String, String)>,
}

impl ShellSelectorOverlay {
    pub fn new(current_shell: ShellType, context: Option<(String, String)>, cx: &mut Context<Self>) -> Self {
        let shells: Vec<_> = available_shells().into_iter().filter(|s| s.available).collect();
        let selected_index = shells
            .iter()
            .position(|s| s.shell_type == current_shell)
            .unwrap_or(0);

        let config = QuickPickerConfig::new(&i18n!(cx, "terminal.shell_selector_title"))
            .subtitle(&i18n!(cx, "terminal.shell_selector_subtitle"))
            .size(280.0, 300.0)
            .centered()
            .keyboard_hints(vec![
                ("Enter".to_string(), i18n!(cx, "common.select")),
                ("Esc".to_string(), i18n!(cx, "common.action.close")),
            ])
            .key_context("ShellSelectorOverlay");

        let state = QuickPickerState::with_selected(shells, config, selected_index, cx);
        let focus_handle = state.focus_handle.clone();

        Self {
            focus_handle,
            state,
            current_shell,
            context,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(ShellSelectorOverlayEvent::Close);
    }

    fn select_shell(&mut self, shell_type: ShellType, cx: &mut Context<Self>) {
        cx.emit(ShellSelectorOverlayEvent::ShellSelected {
            shell_type,
            context: self.context.clone(),
        });
    }

    fn select_current(&mut self, cx: &mut Context<Self>) {
        if let Some(shell) = self.state.selected_item() {
            self.select_shell(shell.shell_type.clone(), cx);
        }
    }
}

#[derive(Clone)]
pub enum ShellSelectorOverlayEvent {
    Close,
    ShellSelected {
        shell_type: ShellType,
        context: Option<(String, String)>,
    },
}

impl velowork_ui::overlay::CloseEvent for ShellSelectorOverlayEvent {
    fn is_close(&self) -> bool { matches!(self, Self::Close) }
}

impl EventEmitter<ShellSelectorOverlayEvent> for ShellSelectorOverlay {}

impl Render for ShellSelectorOverlay {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let focus_handle = self.focus_handle.clone();
        let current_shell = self.current_shell.clone();
        let selected_index = self.state.selected_index;
        let config_width = self.state.config.width;
        let config_title = self.state.config.title.clone();
        let config_subtitle = self.state.config.subtitle.clone();



        modal_content("shell-selector-overlay-modal", cx)
            .w(px(config_width))
            .track_focus(&focus_handle)
            .key_context("ShellSelectorOverlay")
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                this.close(cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                match handle_quick_picker_key(&mut this.state, event, &[]) {
                    QuickPickerAction::Close => this.close(cx),
                    QuickPickerAction::Confirm => this.select_current(cx),
                    QuickPickerAction::SelectPrev | QuickPickerAction::SelectNext => cx.notify(),
                    _ => {}
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(modal_header(
                config_title,
                config_subtitle,
                &t,
                cx,
                cx.listener(|this, _, _window, cx| this.close(cx)),
            ))
            .child(
                div()
                    .id("shell-selector-list")
                    .py(SPACE_XS)
                    .max_h(px(self.state.config.max_height))
                    .overflow_y_scroll()
                    .children(self.state.filtered.iter().enumerate().map(|(i, filter_result)| {
                        let shell = &self.state.items[filter_result.index];
                        let is_current = shell.shell_type == current_shell;
                        let is_selected = i == selected_index;
                        let shell_type = shell.shell_type.clone();
                        let name = shell.name.clone();

                        div()
                            .id(ElementId::Name(format!("shell-opt-{}", i).into()))
                            .w_full()
                            .px(SPACE_LG)
                            .py(SPACE_MD)
                            .cursor_pointer()
                            .when(is_selected, |d| d.bg(rgb(t.bg_hover)))
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _window, cx| {
                                    cx.stop_propagation();
                                    this.select_shell(shell_type.clone(), cx);
                                }),
                            )
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(ui_text(13.0, cx))
                                            .text_color(p.text_primary)
                                            .child(name),
                                    )
                                    .when(is_current, |d| {
                                        d.child(
                                            div()
                                                .text_size(ui_text_md(cx))
                                                .text_color(p.status_success)
                                                .child("✓"),
                                        )
                                    }),
                            )
                    })),
            )
    }
}

velowork_ui::impl_focusable!(ShellSelectorOverlay);
