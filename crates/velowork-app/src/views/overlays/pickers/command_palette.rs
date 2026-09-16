use crate::keybindings::{Cancel, format_keystroke, get_action_descriptions, get_config};
use crate::theme::{surface_bg_t, theme};
use crate::ui::tokens::{ui_text_md, ui_text_ms};
use crate::views::components::{
    QuickPickerConfig, QuickPickerState, badge, keyboard_hints_footer,
    modal_content, substring_filter,
};
use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::empty_state::empty_state;
use velowork_ui::h_flex;
use velowork_ui::input::{InputChangedEvent, InputState};
use velowork_ui::tokens::{RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM};

const RECENT_COMMANDS_LIMIT: usize = 20;

/// Remembered state from the last command palette session.
#[derive(Default)]
struct CommandPaletteMemory {
    query: String,
    /// Action keys in most-recently-used order (front = most recent).
    recent: Vec<&'static str>,
}

impl Global for CommandPaletteMemory {}

/// Command entry for the palette
#[derive(Clone)]
struct CommandEntry {
    /// Stable action identifier (HashMap key from `get_action_descriptions`)
    action_key: &'static str,
    /// Display name
    name: String,
    /// Description
    description: String,
    /// Category
    category: String,
    /// Primary keybinding (formatted for display)
    keybinding: Option<String>,
    /// Factory to create the action for dispatch
    factory: fn() -> Box<dyn gpui::Action>,
}

/// Command palette for quick access to all commands
pub struct CommandPalette {
    #[allow(dead_code)]
    workspace: Entity<velowork_workspace::state::Workspace>,
    focus_manager: Entity<velowork_workspace::focus::FocusManager>,
    window_id: velowork_workspace::state::WindowId,
    focus_handle: FocusHandle,
    search_input: Option<Entity<InputState>>,
    state: QuickPickerState<CommandEntry>,
    initial_focus_done: bool,
}

impl CommandPalette {
    pub fn new(
        workspace: Entity<velowork_workspace::state::Workspace>,
        focus_manager: Entity<velowork_workspace::focus::FocusManager>,
        window_id: velowork_workspace::state::WindowId,
        cx: &mut Context<Self>,
    ) -> Self {
        // Build command list from action descriptions
        let descriptions = get_action_descriptions();
        let config_data = get_config();

        let mut commands: Vec<CommandEntry> = descriptions
            .iter()
            .filter(|(_, desc)| desc.show_in_palette)
            .map(|(action, desc)| {
                // Get primary keybinding for this action
                let keybinding = config_data
                    .bindings
                    .get(*action)
                    .and_then(|entries| entries.iter().find(|e| e.enabled))
                    .map(|e| format_keystroke(&e.keystroke));

                CommandEntry {
                    action_key: action,
                    name: desc.name.to_string(),
                    description: desc.description.to_string(),
                    category: desc.category.to_string(),
                    keybinding,
                    factory: desc.factory,
                }
            })
            .collect();

        // Restore from previous session
        let (query, recent) = cx
            .try_global::<CommandPaletteMemory>()
            .map(|m| (m.query.clone(), m.recent.clone()))
            .unwrap_or_default();

        // Sort: most-recently-used first (in MRU order), then remaining by category + name.
        let recent_rank: std::collections::HashMap<&'static str, usize> = recent
            .iter()
            .enumerate()
            .map(|(i, key)| (*key, i))
            .collect();
        commands.sort_by(|a, b| {
            match (recent_rank.get(a.action_key), recent_rank.get(b.action_key)) {
                (Some(ra), Some(rb)) => ra.cmp(rb),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.category.cmp(&b.category).then(a.name.cmp(&b.name)),
            }
        });

        let config = QuickPickerConfig::new(&i18n!(cx, "search_dialogs.command_palette.title"))
            .searchable(&i18n!(cx, "search_dialogs.command_palette.search"))
            .size(550.0, 450.0)
            .empty_message(&i18n!(cx, "search_dialogs.command_palette.no_commands"))
            .keyboard_hints(vec![
                (
                    "Enter",
                    i18n!(cx, "search_dialogs.command_palette.to_select"),
                ),
                ("Esc", i18n!(cx, "search_dialogs.command_palette.to_close")),
            ])
            .key_context("CommandPalette");

        let mut state = QuickPickerState::new(commands, config, cx);
        if !query.is_empty() {
            state.search_query = query;
        }

        let mut palette = Self {
            workspace,
            focus_manager,
            window_id,
            focus_handle: cx.focus_handle(),
            search_input: None,
            state,
            initial_focus_done: false,
        };

        if !palette.state.search_query.is_empty() {
            palette.filter_commands(cx);
        }

        palette
    }

    fn save_memory(&self, cx: &mut Context<Self>) {
        let recent = cx
            .try_global::<CommandPaletteMemory>()
            .map(|m| m.recent.clone())
            .unwrap_or_default();
        let query = self
            .search_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_default();
        cx.set_global(CommandPaletteMemory {
            query,
            recent,
        });
    }

    fn record_recent(&self, action_key: &'static str, cx: &mut Context<Self>) {
        let mut recent = cx
            .try_global::<CommandPaletteMemory>()
            .map(|m| m.recent.clone())
            .unwrap_or_default();
        recent.retain(|k| *k != action_key);
        recent.insert(0, action_key);
        recent.truncate(RECENT_COMMANDS_LIMIT);
        let query = self
            .search_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_default();
        cx.set_global(CommandPaletteMemory {
            query,
            recent,
        });
    }

    fn close(&self, cx: &mut Context<Self>) {
        self.save_memory(cx);
        cx.emit(CommandPaletteEvent::Close);
    }

    fn execute_command(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(filter_result) = self.state.filtered.get(index) {
            let command = &self.state.items[filter_result.index];
            let action = (command.factory)();
            let action_key = command.action_key;
            self.record_recent(action_key, cx);

            // Restore focus to the terminal pane before dispatching so that
            // context-scoped actions (e.g. CloseTerminal on "TerminalPane")
            // are routed to the correct element.
            let pane_map =
                velowork_views_terminal::layout::navigation::get_pane_map(self.window_id);
            if let Some(focused) = self.focus_manager.read(cx).focused_terminal_state()
                && let Some(pane) = pane_map.find_pane(&focused.project_id, &focused.layout_path)
                && let Some(ref fh) = pane.focus_handle
            {
                window.focus(fh, cx);
            }

            // Close the palette before dispatching so it isn't in the way of
            // overlays opened by the command.
            self.close(cx);

            // Dispatch to the window so global and active-element actions both work.
            window.dispatch_action(action, cx);
        }
    }

    fn filter_commands(&mut self, cx: &App) {
        let filtered = substring_filter(&self.state.items, &self.state.search_query, |cmd| {
            let mut haystack = vec![
                cmd.name.clone(),
                cmd.category.clone(),
                i18n!(cx, format!("commands.{}", cmd.name).as_str()),
                i18n!(cx, format!("commands.cat.{}", cmd.category).as_str()),
            ];
            if !cmd.description.is_empty() {
                haystack.push(cmd.description.clone());
                haystack.push(i18n!(cx, format!("commands.{}", cmd.description).as_str()));
            }
            haystack
        });
        self.state.set_filtered(filtered);
    }

    fn render_command_row(
        &self,
        filtered_index: usize,
        original_index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = theme(cx);
        let command = &self.state.items[original_index];
        let is_selected = filtered_index == self.state.selected_index;

        let name = i18n!(cx, format!("commands.{}", command.name).as_str());
        let category = i18n!(cx, format!("commands.cat.{}", command.category).as_str());
        let description = if command.description.is_empty() {
            String::new()
        } else {
            i18n!(cx, format!("commands.{}", command.description).as_str())
        };
        let keybinding = command.keybinding.clone();

        div()
            .id(ElementId::Name(
                format!("command-{}", filtered_index).into(),
            ))
            .px(SPACE_LG)
            .py(SPACE_SM)
            .rounded(RADIUS_STD)
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap(SPACE_MD)
            .when(!is_selected, |d| d.hover(|s| s.bg(surface_bg_t(t.bg_hover, &t))))
            .when(is_selected, |d| d.bg(surface_bg_t(t.bg_selection, &t)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.execute_command(filtered_index, window, cx);
                }),
            )
            .child(
                // Left side: name + description
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        h_flex()
                            .gap(SPACE_MD)
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(t.text_primary))
                                    .child(name),
                            )
                            .child(badge(category, &t, cx)),
                    )
                    .child(
                        div()
                            .text_size(ui_text_ms(cx))
                            .text_color(rgb(t.text_muted))
                            .child(description),
                    ),
            )
            .child(
                // Right side: keybinding
                h_flex().children(keybinding.map(|kb| {
                    div()
                        .px(SPACE_MD)
                        .py(px(2.0))
                        .rounded(RADIUS_STD)
                        .bg(rgb(t.bg_secondary))
                        .text_size(ui_text_ms(cx))
                        .font_family("monospace")
                        .text_color(rgb(t.text_secondary))
                        .child(kb)
                })),
            )
    }
}

pub enum CommandPaletteEvent {
    Close,
}

impl EventEmitter<CommandPaletteEvent> for CommandPalette {}

impl Render for CommandPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let focus_handle = self.focus_handle.clone();
        let config_width = self.state.config.width;
        let config_max_height = self.state.config.max_height;
        let empty_message = self.state.config.empty_message.clone();

        if self.search_input.is_none() {
            let initial_query = self.state.search_query.clone();
            let input = cx.new(|cx| {
                let mut st = InputState::new(cx)
                    .placeholder(i18n!(cx, "search_dialogs.command_palette.search"))
                    .pass_enter(true);
                if !initial_query.is_empty() {
                    st = st.default_value(&initial_query);
                }
                st
            });
            let input_clone = input.clone();
            cx.subscribe(&input_clone, |this: &mut Self, _, _: &InputChangedEvent, cx| {
                if let Some(inp) = this.search_input.as_ref() {
                    this.state.search_query = inp.read(cx).text().to_string();
                    this.filter_commands(cx);
                    this.state.scroll_to_selected();
                    this.save_memory(cx);
                    cx.notify();
                }
            })
            .detach();
            self.search_input = Some(input);
        }

        if !self.initial_focus_done {
            self.initial_focus_done = true;
            if let Some(search_input) = self.search_input.as_ref() {
                search_input.update(cx, |inp, cx| inp.focus(window, cx));
            }
        }

        modal_content("command-palette-modal", cx)
            .w(px(config_width))
            .max_h(px(config_max_height))
            .track_focus(&focus_handle)
            .key_context("CommandPalette")
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                this.close(cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Text input is handled by the Input component itself
                // (IME-aware). Here we only handle list navigation keys that the
                // input deliberately leaves unhandled.
                match event.keystroke.key.as_str() {
                    "up" => {
                        if this.state.select_prev() {
                            this.state.scroll_to_selected();
                            cx.notify();
                        }
                    }
                    "down" => {
                        if this.state.select_next() {
                            this.state.scroll_to_selected();
                            cx.notify();
                        }
                    }
                    "enter" => {
                        let index = this.state.selected_index;
                        this.execute_command(index, window, cx);
                    }
                    "escape" => {
                        this.close(cx);
                    }
                    _ => {}
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(SPACE_SM)
                    .py(px(10.0))
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .when_some(self.search_input.as_ref(), |this, inp| {
                        this.child(velowork_ui::Input::new(inp).cleanable(true))
                    }),
            )
            .child(
                // Command list
                div()
                    .id("command-list")
                    .flex_1()
                    .px(SPACE_SM)
                    .py(px(6.0))
                    .rounded(RADIUS_STD)
                    .overflow_y_scroll()
                    .track_scroll(&self.state.scroll_handle)
                    .children(self.state.filtered.iter().enumerate().map(
                        |(i, filter_result)| {
                            self.render_command_row(i, filter_result.index, cx)
                        },
                    ))
                    .when(self.state.is_empty(), |d| {
                        d.child(empty_state(empty_message.clone(), &t, cx))
                    }),
            )
            .child({
                let hints: Vec<(String, String)> = vec![
                    (
                        "Enter".to_string(),
                        i18n!(cx, "search_dialogs.command_palette.to_select"),
                    ),
                    (
                        "Esc".to_string(),
                        i18n!(cx, "search_dialogs.command_palette.to_close"),
                    ),
                ];
                let hint_refs: Vec<(&str, &str)> = hints
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                keyboard_hints_footer(&hint_refs, &t, cx)
            })
    }
}

velowork_ui::impl_focusable!(CommandPalette);
